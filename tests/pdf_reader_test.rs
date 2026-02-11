use lopdf::{Document, Object, Stream, dictionary};
use pdf_masking::pdf::reader::PdfReader;

/// ヘルパー: 指定されたMediaBoxを持つ最小限のPDFドキュメントを作成する
fn create_test_pdf_with_media_box(media_box: Vec<Object>) -> Document {
    let mut doc = Document::with_version("1.7");

    // ページを作成
    let pages_id = doc.new_object_id();
    let contents_id = doc.add_object(Stream::new(dictionary! {}, vec![]));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => media_box,
        "Contents" => contents_id,
    });

    // Pagesノードを作成
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );

    // Catalogを作成
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    doc
}

/// ヘルパー: MediaBoxを持たないページと、MediaBoxを持つ親Pagesノードを持つPDFを作成
fn create_test_pdf_with_inherited_media_box(media_box: Vec<Object>) -> Document {
    let mut doc = Document::with_version("1.7");

    // Pagesノード（MediaBoxを継承元として持つ）
    let pages_id = doc.new_object_id();

    // ページ（MediaBoxなし）
    let contents_id = doc.add_object(Stream::new(dictionary! {}, vec![]));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => contents_id,
    });

    // Pagesノードを作成（MediaBoxを持つ）
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
            "MediaBox" => media_box,
        }),
    );

    // Catalogを作成
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    doc
}

#[test]
fn test_page_dimensions_basic_functionality() {
    // A4サイズ（595.276 × 841.89 pt）のMediaBoxを持つPDFを作成
    let media_box = vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Real(595.276),
        Object::Real(841.89),
    ];
    let mut doc = create_test_pdf_with_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let (width, height) = reader.page_dimensions(1).unwrap();

    assert!((width - 595.276).abs() < 0.01, "width should be ~595.276");
    assert!((height - 841.89).abs() < 0.01, "height should be ~841.89");
}

#[test]
fn test_page_dimensions_with_integer_values() {
    // 整数値のMediaBox（Letter サイズ: 612 × 792 pt）
    let media_box = vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ];
    let mut doc = create_test_pdf_with_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let (width, height) = reader.page_dimensions(1).unwrap();

    assert_eq!(width, 612.0);
    assert_eq!(height, 792.0);
}

#[test]
fn test_page_dimensions_with_non_zero_origin() {
    // 原点が (0, 0) でないMediaBox
    let media_box = vec![
        Object::Integer(10),
        Object::Integer(20),
        Object::Integer(605),
        Object::Integer(812),
    ];
    let mut doc = create_test_pdf_with_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let (width, height) = reader.page_dimensions(1).unwrap();

    assert_eq!(width, 595.0);
    assert_eq!(height, 792.0);
}

#[test]
fn test_page_dimensions_inherited_from_parent() {
    // MediaBoxが親Pagesノードから継承される場合
    let media_box = vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(612),
        Object::Integer(792),
    ];
    let mut doc = create_test_pdf_with_inherited_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let (width, height) = reader.page_dimensions(1).unwrap();

    assert_eq!(width, 612.0);
    assert_eq!(height, 792.0);
}

#[test]
fn test_page_dimensions_error_on_zero_dimensions() {
    // 幅がゼロのMediaBox
    let media_box = vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(0), // width = 0
        Object::Integer(792),
    ];
    let mut doc = create_test_pdf_with_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let result = reader.page_dimensions(1);

    assert!(result.is_err(), "should fail with zero width");
    assert!(
        result.unwrap_err().to_string().contains("non-positive"),
        "error should mention non-positive dimensions"
    );
}

#[test]
fn test_page_dimensions_error_on_negative_dimensions() {
    // 負の寸法を持つMediaBox（座標が逆転）
    let media_box = vec![
        Object::Integer(612),
        Object::Integer(792),
        Object::Integer(0),
        Object::Integer(0),
    ];
    let mut doc = create_test_pdf_with_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let (width, height) = reader.page_dimensions(1).unwrap();

    // abs()により正の値になるので、成功するはず
    assert_eq!(width, 612.0);
    assert_eq!(height, 792.0);
}

#[test]
fn test_page_dimensions_error_on_excessive_dimensions() {
    // PDF仕様の上限を超える寸法（14,400 pt = 200 inches）
    let media_box = vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(20000), // 上限超過
        Object::Integer(792),
    ];
    let mut doc = create_test_pdf_with_media_box(media_box);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let result = reader.page_dimensions(1);

    assert!(
        result.is_err(),
        "should fail with dimensions exceeding PDF limits"
    );
    assert!(
        result.unwrap_err().to_string().contains("exceed"),
        "error should mention exceeding limits"
    );
}

#[test]
fn test_page_dimensions_error_on_invalid_media_box_format() {
    // MediaBoxが配列でない、または要素数が不足している場合
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();

    // MediaBoxが3要素しかない（本来は4要素必要）
    let contents_id = doc.add_object(Stream::new(dictionary! {}, vec![]));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(612)],
        "Contents" => contents_id,
    });

    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let result = reader.page_dimensions(1);

    assert!(result.is_err(), "should fail with invalid MediaBox format");
}

#[test]
fn test_page_dimensions_error_on_missing_media_box() {
    // MediaBoxが全く存在しない場合
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();

    // MediaBoxなしのページ
    let contents_id = doc.add_object(Stream::new(dictionary! {}, vec![]));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => contents_id,
    });

    // Pagesノード（MediaBoxなし）
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let temp_file = tempfile::NamedTempFile::new().unwrap();
    doc.save(temp_file.path()).unwrap();

    let reader = PdfReader::open(temp_file.path()).unwrap();
    let result = reader.page_dimensions(1);

    assert!(result.is_err(), "should fail when MediaBox is not found");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("MediaBox not found"),
        "error should mention MediaBox not found"
    );
}
