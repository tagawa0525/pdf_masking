// Phase 7: PDF構築（MRC → PDF）テスト

use lopdf::{Document, Object, dictionary};
use pdf_masking::config::job::ColorMode;
use pdf_masking::mrc::MrcLayers;
use pdf_masking::pdf::content_stream::BBox;
use pdf_masking::pdf::image_xobject::bbox_overlaps;
use pdf_masking::pdf::writer::MrcPageWriter;

// ============================================================
// 1. writer.rs テスト（pub APIのみ）
// ============================================================

#[test]
fn test_build_mrc_content_stream() {
    // MRC用のコンテンツストリームを生成し、正しいオペレータ列であることを検証する。
    let stream_bytes = MrcPageWriter::build_mrc_content_stream("BgImg", "FgImg", 640, 480);

    let content_str = String::from_utf8(stream_bytes).expect("valid UTF-8");

    // 背景画像の描画コマンド: q <width> 0 0 <height> 0 0 cm /BgImg Do Q
    assert!(
        content_str.contains("BgImg"),
        "should contain BgImg reference"
    );

    // 前景画像の描画コマンド: q <width> 0 0 <height> 0 0 cm /FgImg Do Q
    assert!(
        content_str.contains("FgImg"),
        "should contain FgImg reference"
    );

    // q/Q ペアがあること
    assert!(content_str.contains('q'), "should contain save state (q)");
    assert!(
        content_str.contains('Q'),
        "should contain restore state (Q)"
    );

    // cm オペレータがあること
    assert!(content_str.contains("cm"), "should contain cm operator");

    // Do オペレータがあること
    assert!(content_str.contains("Do"), "should contain Do operator");

    // 幅と高さの値が含まれること
    assert!(content_str.contains("640"), "should contain width value");
    assert!(content_str.contains("480"), "should contain height value");
}

#[test]
fn test_build_mrc_content_stream_escapes_names() {
    // PDF Name仕様に従って名前がエスケープされることを検証する。
    let stream_bytes = MrcPageWriter::build_mrc_content_stream("Bg Img", "Fg/Img", 100, 100);
    let content_str = String::from_utf8(stream_bytes).expect("valid UTF-8");

    // 空白は#20にエスケープされること
    assert!(
        content_str.contains("/Bg#20Img"),
        "space should be escaped: {}",
        content_str
    );

    // /は#2Fにエスケープされること
    assert!(
        content_str.contains("/Fg#2FImg"),
        "slash should be escaped: {}",
        content_str
    );
}

#[test]
fn test_write_mrc_page() {
    // MrcLayersからPDFページを構築し、有効なPDFが生成されることを検証する。
    let layers = MrcLayers {
        background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0], // ダミー背景JPEG
        foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1], // ダミー前景JPEG
        mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],      // ダミーJBIG2マスク
        width: 640,
        height: 480,
        color_mode: ColorMode::Rgb,
    };

    let mut writer = MrcPageWriter::new();
    writer.write_mrc_page(&layers).expect("write MRC page");

    let pdf_bytes = writer.save_to_bytes().expect("save to bytes");

    // 有効なPDFとして読み込めること
    let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");

    // 1ページ存在すること
    assert_eq!(doc.get_pages().len(), 1);

    // ページにリソースとコンテンツがあること
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let page_dict = doc.get_dictionary(page_id).expect("get page dict");

    // MediaBoxが設定されていること
    assert!(
        page_dict.get(b"MediaBox").is_ok(),
        "page should have MediaBox"
    );

    // Resourcesが設定されていること
    assert!(
        page_dict.get(b"Resources").is_ok(),
        "page should have Resources"
    );

    // Contentsが設定されていること
    assert!(
        page_dict.get(b"Contents").is_ok(),
        "page should have Contents"
    );
}

// ============================================================
// 1b. write_text_masked_page テスト
// ============================================================

/// ソースPDFから元ページをdeep copyし、テキスト領域XObjectを追加して正しい構造の
/// PDFが生成されることを検証する。
#[test]
fn test_write_text_masked_page_basic() {
    use pdf_masking::mrc::TextMaskedData;
    use pdf_masking::mrc::TextRegionCrop;
    use std::collections::HashMap;

    // ソースPDFを作成（1ページ、空のコンテンツ）
    let mut source_doc = Document::with_version("1.5");
    let pages_id = source_doc.new_object_id();
    let content_id = source_doc.add_object(lopdf::Stream::new(dictionary! {}, b"q Q".to_vec()));
    let page_id = source_doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => dictionary! {},
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    source_doc
        .objects
        .insert(pages_id, Object::Dictionary(pages));
    let catalog_id = source_doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    source_doc.trailer.set("Root", catalog_id);

    // TextMaskedDataを作成（1つのテキスト領域、リダクション画像なし）
    let data = TextMaskedData {
        stripped_content_stream: b"q Q".to_vec(),
        text_regions: vec![TextRegionCrop {
            jpeg_data: vec![0xFF, 0xD8, 0xFF, 0xE0], // ダミーJPEG
            bbox_points: BBox {
                x_min: 72.0,
                y_min: 600.0,
                x_max: 200.0,
                y_max: 700.0,
            },
            pixel_width: 128,
            pixel_height: 100,
        }],
        modified_images: HashMap::new(),
        page_index: 0,
        color_mode: ColorMode::Rgb,
    };

    // write_text_masked_pageを実行
    let mut writer = MrcPageWriter::new();
    let page_obj_id = writer
        .write_text_masked_page(&source_doc, 1, &data)
        .expect("write text masked page");

    // save to bytes and reload
    let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
    let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");

    // 1ページ存在すること
    assert_eq!(doc.get_pages().len(), 1);

    // ページ辞書を取得
    let page_dict = doc.get_dictionary(page_obj_id).expect("get page dict");

    // MediaBoxがあること
    assert!(page_dict.get(b"MediaBox").is_ok(), "should have MediaBox");

    // Contentsがあること
    assert!(page_dict.get(b"Contents").is_ok(), "should have Contents");

    // コンテンツストリームにテキスト領域のDoオペレータが含まれること
    let content_ref = page_dict
        .get(b"Contents")
        .and_then(lopdf::Object::as_reference)
        .expect("Contents should be a reference");
    let content_stream = doc
        .get_object(content_ref)
        .and_then(lopdf::Object::as_stream)
        .expect("Contents should be a stream");
    let content_str = String::from_utf8_lossy(&content_stream.content);
    assert!(
        content_str.contains("TxtRgn0"),
        "content should reference TxtRgn0, got: {}",
        content_str
    );
    assert!(
        content_str.contains("Do"),
        "content should have Do operator"
    );
}

/// テキスト領域が空の場合（テキストなしページ）でも正常に動作することを検証。
#[test]
fn test_write_text_masked_page_no_text_regions() {
    use pdf_masking::mrc::TextMaskedData;
    use std::collections::HashMap;

    let mut source_doc = Document::with_version("1.5");
    let pages_id = source_doc.new_object_id();
    let content_id = source_doc.add_object(lopdf::Stream::new(dictionary! {}, b"q Q".to_vec()));
    let page_id = source_doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => dictionary! {},
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    source_doc
        .objects
        .insert(pages_id, Object::Dictionary(pages));
    let catalog_id = source_doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    source_doc.trailer.set("Root", catalog_id);

    // テキスト領域なし
    let data = TextMaskedData {
        stripped_content_stream: b"q Q".to_vec(),
        text_regions: vec![],
        modified_images: HashMap::new(),
        page_index: 0,
        color_mode: ColorMode::Rgb,
    };

    let mut writer = MrcPageWriter::new();
    let result = writer.write_text_masked_page(&source_doc, 1, &data);
    assert!(
        result.is_ok(),
        "should succeed with no text regions: {:?}",
        result.err()
    );

    let pdf_bytes = writer.save_to_bytes().expect("save");
    let doc = Document::load_mem(&pdf_bytes).expect("load");
    assert_eq!(doc.get_pages().len(), 1);
}

/// リダクション済み画像の差し替えが正しく行われることを検証。
#[test]
fn test_write_text_masked_page_with_modified_images() {
    use pdf_masking::mrc::{ImageModification, TextMaskedData};
    use std::collections::HashMap;

    // ソースPDFに画像XObjectを持つページを作成
    let mut source_doc = Document::with_version("1.5");
    let pages_id = source_doc.new_object_id();

    // 画像XObjectを作成
    let img_stream = lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 50,
            "Height" => 50,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        },
        vec![0xAA; 100], // ダミー元画像データ
    );
    let img_id = source_doc.add_object(Object::Stream(img_stream));

    let xobj_dict = dictionary! {
        "Im1" => Object::Reference(img_id),
    };
    let resources_id = source_doc.add_object(dictionary! {
        "XObject" => Object::Dictionary(xobj_dict),
    });

    let content_id = source_doc.add_object(lopdf::Stream::new(
        dictionary! {},
        b"q 50 0 0 50 100 200 cm /Im1 Do Q".to_vec(),
    ));

    let page_id = source_doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    source_doc
        .objects
        .insert(pages_id, Object::Dictionary(pages));
    let catalog_id = source_doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    source_doc.trailer.set("Root", catalog_id);

    // リダクション済み画像データ
    let mut modified_images = HashMap::new();
    modified_images.insert(
        "Im1".to_string(),
        ImageModification {
            data: vec![0xBB; 200], // リダクション後のデータ
            filter: "DCTDecode".to_string(),
            color_space: "DeviceGray".to_string(),
            bits_per_component: 8,
        },
    );

    let data = TextMaskedData {
        stripped_content_stream: b"q 50 0 0 50 100 200 cm /Im1 Do Q".to_vec(),
        text_regions: vec![],
        modified_images,
        page_index: 0,
        color_mode: ColorMode::Rgb,
    };

    let mut writer = MrcPageWriter::new();
    let page_obj_id = writer
        .write_text_masked_page(&source_doc, 1, &data)
        .expect("write text masked page");

    // writerの内部documentを直接検証（lopdfのsave/loadラウンドトリップを避ける）
    let doc = writer.document_mut();

    // ページのXObjectからIm1を探し、差し替えられたことを確認
    let page_dict = doc.get_dictionary(page_obj_id).expect("get page dict");
    let resources = page_dict.get(b"Resources").expect("Resources");
    let resources_dict = match resources {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve Resources"),
        Object::Dictionary(d) => d,
        _ => panic!("unexpected Resources type"),
    };
    let xobject = resources_dict.get(b"XObject").expect("XObject");
    let xobject_dict = match xobject {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve XObject"),
        Object::Dictionary(d) => d,
        _ => panic!("unexpected XObject type"),
    };
    assert!(xobject_dict.has(b"Im1"), "should still have Im1");

    // Im1のストリームデータが差し替えられていること
    let im1_ref = xobject_dict
        .get(b"Im1")
        .and_then(lopdf::Object::as_reference)
        .expect("Im1 should be reference");
    let im1_stream = doc
        .get_object(im1_ref)
        .and_then(lopdf::Object::as_stream)
        .expect("Im1 should be stream");
    assert_eq!(
        im1_stream.content,
        vec![0xBB; 200],
        "Im1 data should be replaced with redacted data"
    );

    // ColorSpaceが更新されていること
    let cs = im1_stream
        .dict
        .get(b"ColorSpace")
        .and_then(lopdf::Object::as_name)
        .expect("ColorSpace");
    assert_eq!(cs, b"DeviceGray", "ColorSpace should be updated");
}

// ============================================================
// 2. image_xobject.rs テスト
// ============================================================

#[test]
fn test_bbox_overlap_detection() {
    // 重なるBBoxペア
    let a = BBox {
        x_min: 0.0,
        y_min: 0.0,
        x_max: 100.0,
        y_max: 100.0,
    };
    let b = BBox {
        x_min: 50.0,
        y_min: 50.0,
        x_max: 150.0,
        y_max: 150.0,
    };
    assert!(
        bbox_overlaps(&a, &b),
        "overlapping bboxes should be detected"
    );
    assert!(
        bbox_overlaps(&b, &a),
        "overlap detection should be symmetric"
    );
}

#[test]
fn test_bbox_no_overlap() {
    // 重ならないBBoxペア
    let a = BBox {
        x_min: 0.0,
        y_min: 0.0,
        x_max: 100.0,
        y_max: 100.0,
    };
    let b = BBox {
        x_min: 200.0,
        y_min: 200.0,
        x_max: 300.0,
        y_max: 300.0,
    };
    assert!(
        !bbox_overlaps(&a, &b),
        "non-overlapping bboxes should not be detected"
    );

    // 辺が接しているケース（重ならない）
    let c = BBox {
        x_min: 100.0,
        y_min: 0.0,
        x_max: 200.0,
        y_max: 100.0,
    };
    assert!(
        !bbox_overlaps(&a, &c),
        "touching edges should not be considered overlapping"
    );
}
