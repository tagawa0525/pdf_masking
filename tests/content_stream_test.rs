// Phase 2: コンテンツストリーム解析テスト

use pdf_masking::pdf::content_stream::{Matrix, extract_xobject_placements};
use pdf_masking::pdf::reader::PdfReader;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};

// ============================================================
// Helper: lopdfで最小限のテスト用PDFをファイルに書き出す
// ============================================================

/// 1ページのPDFを生成しファイルに保存する。
/// content_ops: ページのコンテンツストリームに書き込むオペレータ列
/// xobjects: (名前, XObjectストリーム) のペア列（Resourcesに登録する）
/// 戻り値: 保存先パス（tempfileのTempDirも返して寿命を管理）
fn create_test_pdf(
    content_ops: Vec<Operation>,
    xobjects: Vec<(&str, Stream)>,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("test.pdf");

    let mut doc = Document::with_version("1.5");

    let pages_id = doc.new_object_id();

    // XObjectを登録
    let mut xobject_dict = lopdf::Dictionary::new();
    for (name, stream) in xobjects {
        let xobj_id = doc.add_object(Object::Stream(stream));
        xobject_dict.set(name.as_bytes(), Object::Reference(xobj_id));
    }

    let resources_id = doc.add_object(dictionary! {
        "XObject" => Object::Dictionary(xobject_dict),
    });

    // コンテンツストリーム
    let content = Content {
        operations: content_ops,
    };
    let content_bytes = content.encode().expect("encode content");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content_bytes));

    // ページ
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });

    // Pages
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    // Catalog
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    doc.save(&path).expect("save PDF");

    (dir, path)
}

/// Image XObject用の最小ストリームを作成する
fn make_image_xobject() -> Stream {
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => 100,
        "Height" => 100,
        "ColorSpace" => "DeviceRGB",
        "BitsPerComponent" => 8,
    };
    Stream::new(dict, vec![0u8; 4]) // 最小限のダミーデータ
}

/// Form XObject用の最小ストリームを作成する（Image以外のXObject）
fn make_form_xobject() -> Stream {
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
    };
    Stream::new(dict, vec![])
}

// ============================================================
// 1. PdfReader テスト
// ============================================================

#[test]
fn test_open_pdf_file() {
    let (_dir, path) = create_test_pdf(vec![], vec![]);
    let reader = PdfReader::open(&path);
    assert!(reader.is_ok(), "should open a valid PDF file");
}

#[test]
fn test_page_count() {
    let (_dir, path) = create_test_pdf(vec![], vec![]);
    let reader = PdfReader::open(&path).expect("open PDF");
    assert_eq!(reader.page_count(), 1);
}

#[test]
fn test_page_content_stream_bytes() {
    let ops = vec![Operation::new("q", vec![]), Operation::new("Q", vec![])];
    let (_dir, path) = create_test_pdf(ops, vec![]);
    let reader = PdfReader::open(&path).expect("open PDF");
    let content = reader.page_content_stream(1).expect("get content stream");
    // コンテンツストリームにはq/Qオペレータが含まれるはず
    let text = String::from_utf8_lossy(&content);
    assert!(text.contains('q'), "content should contain 'q' operator");
    assert!(text.contains('Q'), "content should contain 'Q' operator");
}

#[test]
fn test_page_resources_xobjects() {
    let xobjects = vec![
        ("Im1", make_image_xobject()),
        ("Im2", make_image_xobject()),
        ("Fm1", make_form_xobject()), // Form XObjectはImage一覧に含まれない
    ];
    let (_dir, path) = create_test_pdf(vec![], xobjects);
    let reader = PdfReader::open(&path).expect("open PDF");
    let names = reader.page_xobject_names(1).expect("get xobject names");
    // Image XObjectのみが返される
    assert!(names.contains(&"Im1".to_string()), "should contain Im1");
    assert!(names.contains(&"Im2".to_string()), "should contain Im2");
    assert!(
        !names.contains(&"Fm1".to_string()),
        "should not contain Form XObject Fm1"
    );
    assert_eq!(names.len(), 2);
}

#[test]
fn test_open_nonexistent_file() {
    let result = PdfReader::open("/nonexistent/path/to/file.pdf");
    assert!(result.is_err(), "should fail for nonexistent file");
}

// ============================================================
// 2. content_stream テスト
// ============================================================

#[test]
fn test_ctm_identity() {
    // 空のコンテンツストリーム → ImagePlacement無し
    let placements = extract_xobject_placements(b"").expect("parse empty stream");
    assert!(
        placements.is_empty(),
        "empty stream should yield no placements"
    );
}

#[test]
fn test_ctm_cm_operator() {
    // cm オペレータでCTMを設定し、Do で画像を配置
    let ops = vec![
        // cm 100 0 0 200 50 60
        Operation::new(
            "cm",
            vec![
                100.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                200.into(),
                50.into(),
                60.into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");
    let placements = extract_xobject_placements(&bytes).expect("parse");
    assert_eq!(placements.len(), 1);
    let p = &placements[0];
    assert_eq!(p.name, "Im1");

    // CTM = identity * cm(100,0,0,200,50,60) = (100,0,0,200,50,60)
    let expected_ctm = Matrix {
        a: 100.0,
        b: 0.0,
        c: 0.0,
        d: 200.0,
        e: 50.0,
        f: 60.0,
    };
    assert_eq!(p.ctm, expected_ctm);

    // BBox: 単位正方形 [0,0]-[1,1] をCTMで変換
    // (0,0) -> (50, 60)
    // (1,0) -> (150, 60)
    // (0,1) -> (50, 260)
    // (1,1) -> (150, 260)
    assert_approx(p.bbox.x_min, 50.0);
    assert_approx(p.bbox.y_min, 60.0);
    assert_approx(p.bbox.x_max, 150.0);
    assert_approx(p.bbox.y_max, 260.0);
}

#[test]
fn test_graphics_state_save_restore() {
    // q → cm → Q → Do  (Doの時点ではCTMはidentityに戻っている)
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                200.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                300.into(),
                10.into(),
                20.into(),
            ],
        ),
        Operation::new("Q", vec![]),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");
    let placements = extract_xobject_placements(&bytes).expect("parse");
    assert_eq!(placements.len(), 1);
    // Q後なのでCTMはidentityに戻る
    assert_eq!(placements[0].ctm, Matrix::identity());
}

#[test]
fn test_do_operator_extracts_image() {
    let ops = vec![Operation::new("Do", vec![Object::Name(b"Im1".to_vec())])];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");
    let placements = extract_xobject_placements(&bytes).expect("parse");
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].name, "Im1");
    assert_eq!(placements[0].ctm, Matrix::identity());
}

#[test]
fn test_multiple_images() {
    let ops = vec![
        Operation::new(
            "cm",
            vec![
                100.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                100.into(),
                0.into(),
                0.into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
        Operation::new(
            "cm",
            vec![
                Object::Real(1.0),
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(1.0),
                200.into(),
                300.into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(b"Im2".to_vec())]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");
    let placements = extract_xobject_placements(&bytes).expect("parse");
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].name, "Im1");
    assert_eq!(placements[1].name, "Im2");

    // Im1のCTM: identity * cm(100,0,0,100,0,0) = (100,0,0,100,0,0)
    assert_eq!(
        placements[0].ctm,
        Matrix {
            a: 100.0,
            b: 0.0,
            c: 0.0,
            d: 100.0,
            e: 0.0,
            f: 0.0,
        }
    );

    // 行列積: CTM' = current_ctm.multiply(&cm_matrix)
    // current_ctm = (100, 0, 0, 100, 0, 0)
    // cm_matrix   = (  1, 0, 0,   1, 200, 300)
    // → (100, 0, 0, 100, 200, 300)
    assert_eq!(
        placements[1].ctm,
        Matrix {
            a: 100.0,
            b: 0.0,
            c: 0.0,
            d: 100.0,
            e: 200.0,
            f: 300.0,
        }
    );
}

#[test]
fn test_ctm_bbox_calculation() {
    // CTM that scales and translates: (200, 0, 0, 150, 100, 50)
    // Unit square [0,0]-[1,1] transformed:
    // (0,0) -> x'=200*0+0*0+100=100, y'=0*0+150*0+50=50
    // (1,0) -> x'=200*1+0*0+100=300, y'=0*1+150*0+50=50
    // (0,1) -> x'=200*0+0*1+100=100, y'=0*0+150*1+50=200
    // (1,1) -> x'=200*1+0*1+100=300, y'=0*1+150*1+50=200
    let ops = vec![
        Operation::new(
            "cm",
            vec![
                200.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                150.into(),
                100.into(),
                50.into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");
    let placements = extract_xobject_placements(&bytes).expect("parse");
    assert_eq!(placements.len(), 1);
    assert_approx(placements[0].bbox.x_min, 100.0);
    assert_approx(placements[0].bbox.y_min, 50.0);
    assert_approx(placements[0].bbox.x_max, 300.0);
    assert_approx(placements[0].bbox.y_max, 200.0);
}

#[test]
fn test_nested_graphics_states() {
    // q → cm(A) → q → cm(B) → Q → Do(Im1) → Q → Do(Im2)
    // Im1は cm(A) のCTMで配置される（内側のq/Q後、cm(A)は残る）
    // Im2はidentityで配置される（外側のQ後）
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                2.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                2.into(),
                10.into(),
                20.into(),
            ],
        ),
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                3.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                3.into(),
                30.into(),
                40.into(),
            ],
        ),
        Operation::new("Q", vec![]),
        // 内側Qでcm(B)が消え、cm(A)の状態に戻る
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
        Operation::new("Q", vec![]),
        // 外側Qでcm(A)も消え、identityに戻る
        Operation::new("Do", vec![Object::Name(b"Im2".to_vec())]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");
    let placements = extract_xobject_placements(&bytes).expect("parse");
    assert_eq!(placements.len(), 2);

    // Im1: cm(A) = (2, 0, 0, 2, 10, 20)
    assert_eq!(
        placements[0].ctm,
        Matrix {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 2.0,
            e: 10.0,
            f: 20.0,
        }
    );
    assert_eq!(placements[0].name, "Im1");

    // Im2: identity
    assert_eq!(placements[1].ctm, Matrix::identity());
    assert_eq!(placements[1].name, "Im2");
}

// ============================================================
// ヘルパー
// ============================================================

fn assert_approx(actual: f64, expected: f64) {
    let epsilon = 1e-6;
    assert!(
        (actual - expected).abs() < epsilon,
        "expected {expected}, got {actual}"
    );
}
