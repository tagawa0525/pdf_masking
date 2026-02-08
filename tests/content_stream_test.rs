// Phase 2: コンテンツストリーム解析テスト

use pdf_masking::mrc::segmenter::PixelBBox;
use pdf_masking::pdf::content_stream::{
    Matrix, extract_white_fill_rects, extract_xobject_placements, pixel_to_page_coords,
    strip_text_operators,
};
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
#[ignore = "PR 7 RED: stub not yet implemented"]
fn test_page_image_streams_returns_image_xobjects() {
    // Image XObjectを持つPDFからストリームを取得できることを検証
    let (_dir, path) = create_test_pdf(
        vec![],
        vec![
            ("Im1", make_image_xobject()),
            ("Im2", make_image_xobject()),
            ("Fm1", make_form_xobject()),
        ],
    );
    let reader = PdfReader::open(&path).expect("open PDF");
    let streams = reader.page_image_streams(1).expect("get image streams");

    // Image XObjectのみが返される（Form XObjectは除外）
    assert_eq!(streams.len(), 2, "should contain exactly 2 image streams");
    assert!(streams.contains_key("Im1"), "should contain Im1");
    assert!(streams.contains_key("Im2"), "should contain Im2");
    assert!(
        !streams.contains_key("Fm1"),
        "should not contain Form XObject Fm1"
    );

    // 各ストリームにSubtype=Imageが含まれている
    for (name, stream) in &streams {
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(lopdf::Object::as_name)
            .expect("stream should have Subtype");
        assert_eq!(subtype, b"Image", "{name} should be Image subtype");
    }
}

#[test]
#[ignore = "PR 7 RED: stub not yet implemented"]
fn test_page_image_streams_empty_when_no_images() {
    // XObjectがないPDFでは空のHashMapが返る
    let (_dir, path) = create_test_pdf(vec![], vec![]);
    let reader = PdfReader::open(&path).expect("open PDF");
    let streams = reader.page_image_streams(1).expect("get image streams");
    assert!(streams.is_empty(), "should return empty map when no images");
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
// 3. strip_text_operators テスト
// ============================================================

#[test]
fn test_strip_text_operators_removes_bt_et_blocks() {
    // BT...ETブロックが除去されることを確認
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
        Operation::new(
            "Tj",
            vec![Object::String(
                b"Hello".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]),
        Operation::new("Q", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let result = strip_text_operators(&bytes).expect("strip text");
    let decoded = Content::decode(&result).expect("decode result");

    // q と Q のみが残る
    assert_eq!(decoded.operations.len(), 2);
    assert_eq!(decoded.operations[0].operator, "q");
    assert_eq!(decoded.operations[1].operator, "Q");
}

#[test]
fn test_strip_text_operators_preserves_non_text() {
    // 非テキストオペレーション（グラフィックス、XObject等）は保持される
    let ops = vec![
        Operation::new("q", vec![]),
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
        Operation::new("BT", vec![]),
        Operation::new(
            "Tj",
            vec![Object::String(
                b"Text".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]),
        Operation::new("Q", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let result = strip_text_operators(&bytes).expect("strip text");
    let decoded = Content::decode(&result).expect("decode result");

    // q, cm, Do, Q が残る（BT, Tj, ETは除去）
    assert_eq!(decoded.operations.len(), 4);
    assert_eq!(decoded.operations[0].operator, "q");
    assert_eq!(decoded.operations[1].operator, "cm");
    assert_eq!(decoded.operations[2].operator, "Do");
    assert_eq!(decoded.operations[3].operator, "Q");
}

#[test]
fn test_strip_text_operators_empty_stream() {
    // 空のコンテンツストリーム
    let result = strip_text_operators(b"").expect("strip empty");
    assert!(result.is_empty(), "empty input should return empty output");
}

#[test]
fn test_strip_text_operators_no_text() {
    // テキストオペレーションがない場合、全て保持される
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                100.into(),
                Object::Real(0.0),
                Object::Real(0.0),
                100.into(),
                50.into(),
                60.into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
        Operation::new("Q", vec![]),
    ];
    let content = Content {
        operations: ops.clone(),
    };
    let bytes = content.encode().expect("encode");

    let result = strip_text_operators(&bytes).expect("strip text");
    let decoded = Content::decode(&result).expect("decode result");

    // 全てのオペレーションが保持される
    assert_eq!(decoded.operations.len(), ops.len());
    for (i, op) in decoded.operations.iter().enumerate() {
        assert_eq!(op.operator, ops[i].operator);
    }
}

#[test]
fn test_strip_text_operators_nested_bt_et() {
    // ネストしたBT...ETブロック（通常PDFでは発生しないが、ネスト深度追跡の動作確認）
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new(
            "Tj",
            vec![Object::String(
                b"Outer".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("BT", vec![]), // ネストBT
        Operation::new(
            "Tj",
            vec![Object::String(
                b"Inner".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]), // 内側ET
        Operation::new(
            "Tj",
            vec![Object::String(
                b"Back to outer".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]), // 外側ET
        Operation::new("Q", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let result = strip_text_operators(&bytes).expect("strip text");
    let decoded = Content::decode(&result).expect("decode result");

    // q と Q のみが残る（全てのBT...ET内容が除去される）
    assert_eq!(decoded.operations.len(), 2);
    assert_eq!(decoded.operations[0].operator, "q");
    assert_eq!(decoded.operations[1].operator, "Q");
}

#[test]
fn test_strip_text_operators_multiple_text_blocks() {
    // 複数のBT...ETブロック
    let ops = vec![
        Operation::new("BT", vec![]),
        Operation::new(
            "Tj",
            vec![Object::String(
                b"First".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]),
        Operation::new("q", vec![]),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
        Operation::new("Q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new(
            "Tj",
            vec![Object::String(
                b"Second".to_vec(),
                lopdf::StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let result = strip_text_operators(&bytes).expect("strip text");
    let decoded = Content::decode(&result).expect("decode result");

    // q, Do, Q のみが残る
    assert_eq!(decoded.operations.len(), 3);
    assert_eq!(decoded.operations[0].operator, "q");
    assert_eq!(decoded.operations[1].operator, "Do");
    assert_eq!(decoded.operations[2].operator, "Q");
}

// ============================================================
// 4. pixel_to_page_coords テスト
// ============================================================

#[test]
fn test_pixel_to_page_coords_basic() {
    // 612x792pt ページ、300DPI → 2550x3300px ビットマップ
    let pixel_bbox = PixelBBox {
        x: 0,
        y: 0,
        width: 2550,
        height: 3300,
    };
    let bbox = pixel_to_page_coords(&pixel_bbox, 612.0, 792.0, 2550, 3300).expect("coords");
    assert_approx(bbox.x_min, 0.0);
    assert_approx(bbox.y_min, 0.0);
    assert_approx(bbox.x_max, 612.0);
    assert_approx(bbox.y_max, 792.0);
}

#[test]
fn test_pixel_to_page_coords_y_inversion() {
    // ビットマップ上端（y=0）はPDFの上端（y=792）
    let pixel_bbox = PixelBBox {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    // 1000x1000px bitmap, 100x100pt page
    let bbox = pixel_to_page_coords(&pixel_bbox, 100.0, 100.0, 1000, 1000).expect("coords");
    // ビットマップ (0,0)-(100,100) → PDF (0,90)-(10,100)
    assert_approx(bbox.x_min, 0.0);
    assert_approx(bbox.y_min, 90.0); // 100 - 100*0.1 = 90
    assert_approx(bbox.x_max, 10.0);
    assert_approx(bbox.y_max, 100.0); // 100 - 0*0.1 = 100
}

#[test]
fn test_pixel_to_page_coords_center_region() {
    // ビットマップ中央の領域
    let pixel_bbox = PixelBBox {
        x: 250,
        y: 250,
        width: 500,
        height: 500,
    };
    // 1000x1000px bitmap, 100x100pt page → scale=0.1
    let bbox = pixel_to_page_coords(&pixel_bbox, 100.0, 100.0, 1000, 1000).expect("coords");
    assert_approx(bbox.x_min, 25.0);
    assert_approx(bbox.y_min, 25.0); // 100 - (250+500)*0.1 = 25
    assert_approx(bbox.x_max, 75.0);
    assert_approx(bbox.y_max, 75.0); // 100 - 250*0.1 = 75
}

#[test]
fn test_pixel_to_page_coords_bottom_right() {
    // ビットマップ右下（PDFの右下は y_min=0）
    let pixel_bbox = PixelBBox {
        x: 900,
        y: 900,
        width: 100,
        height: 100,
    };
    // 1000x1000px bitmap, 100x100pt page → scale=0.1
    let bbox = pixel_to_page_coords(&pixel_bbox, 100.0, 100.0, 1000, 1000).expect("coords");
    assert_approx(bbox.x_min, 90.0);
    assert_approx(bbox.y_min, 0.0); // 100 - (900+100)*0.1 = 0
    assert_approx(bbox.x_max, 100.0);
    assert_approx(bbox.y_max, 10.0); // 100 - 900*0.1 = 10
}

#[test]
fn test_pixel_to_page_coords_zero_bitmap_rejected() {
    let pixel_bbox = PixelBBox {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    let result = pixel_to_page_coords(&pixel_bbox, 100.0, 100.0, 0, 1000);
    assert!(result.is_err(), "Should reject zero bitmap width");

    let result = pixel_to_page_coords(&pixel_bbox, 100.0, 100.0, 1000, 0);
    assert!(result.is_err(), "Should reject zero bitmap height");
}

// ============================================================
// 5. extract_white_fill_rects テスト
// ============================================================

#[test]
fn test_white_fill_rects_empty_stream() {
    let result = extract_white_fill_rects(b"").expect("parse empty");
    assert!(result.is_empty(), "empty stream should yield no rects");
}

#[test]
fn test_white_fill_rects_rgb_white() {
    // 白色RGB(1,1,1)でfill矩形
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 1);
    assert_approx(rects[0].x_min, 10.0);
    assert_approx(rects[0].y_min, 20.0);
    assert_approx(rects[0].x_max, 110.0);
    assert_approx(rects[0].y_max, 70.0);
}

#[test]
fn test_white_fill_rects_gray_white() {
    // 白色Gray(1)でfill矩形
    let ops = vec![
        Operation::new("g", vec![Object::Real(1.0)]),
        Operation::new(
            "re",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(200.0),
                Object::Real(300.0),
            ],
        ),
        Operation::new("f*", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 1);
    assert_approx(rects[0].x_min, 0.0);
    assert_approx(rects[0].y_min, 0.0);
    assert_approx(rects[0].x_max, 200.0);
    assert_approx(rects[0].y_max, 300.0);
}

#[test]
fn test_white_fill_rects_cmyk_white() {
    // 白色CMYK(0,0,0,0)でfill矩形
    let ops = vec![
        Operation::new(
            "k",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(0.0),
            ],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(50.0),
                Object::Real(50.0),
                Object::Real(100.0),
                Object::Real(100.0),
            ],
        ),
        Operation::new("F", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 1);
    assert_approx(rects[0].x_min, 50.0);
    assert_approx(rects[0].y_min, 50.0);
    assert_approx(rects[0].x_max, 150.0);
    assert_approx(rects[0].y_max, 150.0);
}

#[test]
fn test_white_fill_rects_non_white_ignored() {
    // 非白色（赤）はfillしても検出されない
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(0.0), Object::Real(0.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert!(rects.is_empty(), "non-white fill should not be detected");
}

#[test]
fn test_white_fill_rects_stroke_not_detected() {
    // 白色設定 + re + S(stroke) → fill矩形としては検出されない
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("S", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert!(
        rects.is_empty(),
        "stroke should not be detected as fill rect"
    );
}

#[test]
fn test_white_fill_rects_ctm_applied() {
    // CTMでスケーリングされた白色fill矩形
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                Object::Real(2.0),
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(3.0),
                Object::Real(100.0),
                Object::Real(200.0),
            ],
        ),
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(50.0),
                Object::Real(30.0),
            ],
        ),
        Operation::new("f", vec![]),
        Operation::new("Q", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 1);
    // CTM: (2,0,0,3,100,200)
    // rect: (10,20,50,30) → corners: (10,20),(60,20),(10,50),(60,50)
    // transformed: (2*10+100, 3*20+200) = (120, 260)
    //              (2*60+100, 3*20+200) = (220, 260)
    //              (2*10+100, 3*50+200) = (120, 350)
    //              (2*60+100, 3*50+200) = (220, 350)
    assert_approx(rects[0].x_min, 120.0);
    assert_approx(rects[0].y_min, 260.0);
    assert_approx(rects[0].x_max, 220.0);
    assert_approx(rects[0].y_max, 350.0);
}

#[test]
fn test_white_fill_rects_color_state_save_restore() {
    // q → 白設定 → Q → re+f → デフォルト（黒）なので検出されない
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new("Q", vec![]),
        // Q後、fill colorはデフォルト（黒）に戻る
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert!(
        rects.is_empty(),
        "fill color should be restored after Q, default is black"
    );
}

#[test]
fn test_white_fill_rects_multiple() {
    // 複数の白色fill矩形
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("f", vec![]),
        Operation::new(
            "re",
            vec![
                Object::Real(200.0),
                Object::Real(300.0),
                Object::Real(150.0),
                Object::Real(80.0),
            ],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 2);
    assert_approx(rects[0].x_min, 10.0);
    assert_approx(rects[1].x_min, 200.0);
}

#[test]
fn test_white_fill_rects_scn_gray_operator() {
    // scn オペレータでGray白色設定（1値 = DeviceGrayデフォルト）
    let ops = vec![
        Operation::new("scn", vec![Object::Real(1.0)]),
        Operation::new(
            "re",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(50.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 1);
}

#[test]
fn test_white_fill_rects_no_rect_before_fill() {
    // re なしで f → 何も検出されない
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert!(rects.is_empty(), "no rect before fill should yield nothing");
}

#[test]
fn test_white_fill_rects_multiple_re_single_fill() {
    // 複数のreオペレータ → 1つのf → 全矩形が検出される
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(200.0),
                Object::Real(300.0),
                Object::Real(80.0),
                Object::Real(40.0),
            ],
        ),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert_eq!(rects.len(), 2);
    assert_approx(rects[0].x_min, 10.0);
    assert_approx(rects[0].x_max, 110.0);
    assert_approx(rects[1].x_min, 200.0);
    assert_approx(rects[1].x_max, 280.0);
}

#[test]
fn test_white_fill_rects_path_op_after_re_clears() {
    // re → m（lineTo）→ f → 矩形は検出されない（パスが純粋な矩形でなくなる）
    let ops = vec![
        Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        ),
        Operation::new(
            "re",
            vec![
                Object::Real(10.0),
                Object::Real(20.0),
                Object::Real(100.0),
                Object::Real(50.0),
            ],
        ),
        Operation::new("m", vec![Object::Real(0.0), Object::Real(0.0)]),
        Operation::new("f", vec![]),
    ];
    let content = Content { operations: ops };
    let bytes = content.encode().expect("encode");

    let rects = extract_white_fill_rects(&bytes).expect("extract");
    assert!(
        rects.is_empty(),
        "path with non-rect ops after re should not be detected"
    );
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
