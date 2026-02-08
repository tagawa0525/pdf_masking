// Phase 7: PDF構築（MRC → PDF）テスト

use lopdf::{Document, Object};
use pdf_masking::mrc::MrcLayers;
use pdf_masking::pdf::content_stream::BBox;
use pdf_masking::pdf::image_xobject::{bbox_overlaps, redact_region_placeholder};
use pdf_masking::pdf::writer::MrcPageWriter;

// ============================================================
// 1. writer.rs テスト
// ============================================================

#[test]
fn test_create_background_xobject() {
    // JPEG bytesからBgImg XObjectを作成し、正しいPDF構造を持つことを検証する。
    let jpeg_data: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0]; // ダミーJPEGヘッダ
    let mut writer = MrcPageWriter::new();
    let obj_id = writer.add_background_xobject(&jpeg_data, 640, 480);

    // ObjectIdが有効であることを確認
    assert!(obj_id.0 > 0, "object id should be positive");

    // PDFをバイトに書き出して再読み込みし、XObject構造を検証
    let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
    let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");

    let stream = doc
        .get_object(obj_id)
        .expect("get object")
        .as_stream()
        .expect("as stream");

    // Type = XObject
    let type_name = stream
        .dict
        .get(b"Type")
        .expect("get Type")
        .as_name()
        .expect("as name");
    assert_eq!(type_name, b"XObject");

    // Subtype = Image
    let subtype = stream
        .dict
        .get(b"Subtype")
        .expect("get Subtype")
        .as_name()
        .expect("as name");
    assert_eq!(subtype, b"Image");

    // Width = 640
    let width = stream
        .dict
        .get(b"Width")
        .expect("get Width")
        .as_i64()
        .expect("as i64");
    assert_eq!(width, 640);

    // Height = 480
    let height = stream
        .dict
        .get(b"Height")
        .expect("get Height")
        .as_i64()
        .expect("as i64");
    assert_eq!(height, 480);

    // ColorSpace = DeviceRGB
    let cs = stream
        .dict
        .get(b"ColorSpace")
        .expect("get ColorSpace")
        .as_name()
        .expect("as name");
    assert_eq!(cs, b"DeviceRGB");

    // BitsPerComponent = 8
    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .expect("get BitsPerComponent")
        .as_i64()
        .expect("as i64");
    assert_eq!(bpc, 8);

    // Filter = DCTDecode
    let filter = stream
        .dict
        .get(b"Filter")
        .expect("get Filter")
        .as_name()
        .expect("as name");
    assert_eq!(filter, b"DCTDecode");

    // ストリームデータがJPEGデータと一致すること
    assert_eq!(stream.content, jpeg_data);
}

#[test]
fn test_create_foreground_xobject_with_smask() {
    // JPEG bytesからFgImg XObjectを作成し、SMask参照が付いていることを検証する。
    let fg_jpeg: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE1]; // ダミー前景JPEG
    let mask_jbig2: Vec<u8> = vec![0x97, 0x4A, 0x42, 0x32]; // ダミーJBIG2

    let mut writer = MrcPageWriter::new();

    // まずマスクXObjectを作成
    let mask_id = writer.add_mask_xobject(&mask_jbig2, 640, 480);

    // 前景XObjectを作成（SMask参照付き）
    let fg_id = writer.add_foreground_xobject(&fg_jpeg, 640, 480, mask_id);

    let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
    let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");

    let stream = doc
        .get_object(fg_id)
        .expect("get object")
        .as_stream()
        .expect("as stream");

    // 基本構造の検証
    let subtype = stream
        .dict
        .get(b"Subtype")
        .expect("get Subtype")
        .as_name()
        .expect("as name");
    assert_eq!(subtype, b"Image");

    let filter = stream
        .dict
        .get(b"Filter")
        .expect("get Filter")
        .as_name()
        .expect("as name");
    assert_eq!(filter, b"DCTDecode");

    // SMaskがマスクオブジェクトへの参照であること
    let smask = stream.dict.get(b"SMask").expect("get SMask");
    match smask {
        Object::Reference(ref_id) => {
            assert_eq!(*ref_id, mask_id, "SMask should reference the mask object");
        }
        _ => panic!("SMask should be a Reference, got {:?}", smask),
    }
}

#[test]
fn test_create_mask_xobject() {
    // JBIG2 bytesからMaskImg XObjectを作成し、正しいPDF構造を持つことを検証する。
    let jbig2_data: Vec<u8> = vec![0x97, 0x4A, 0x42, 0x32]; // ダミーJBIG2
    let mut writer = MrcPageWriter::new();
    let obj_id = writer.add_mask_xobject(&jbig2_data, 800, 600);

    let pdf_bytes = writer.save_to_bytes().expect("save to bytes");
    let doc = Document::load_mem(&pdf_bytes).expect("load PDF from memory");

    let stream = doc
        .get_object(obj_id)
        .expect("get object")
        .as_stream()
        .expect("as stream");

    // ColorSpace = DeviceGray
    let cs = stream
        .dict
        .get(b"ColorSpace")
        .expect("get ColorSpace")
        .as_name()
        .expect("as name");
    assert_eq!(cs, b"DeviceGray");

    // BitsPerComponent = 1
    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .expect("get BitsPerComponent")
        .as_i64()
        .expect("as i64");
    assert_eq!(bpc, 1);

    // Filter = JBIG2Decode
    let filter = stream
        .dict
        .get(b"Filter")
        .expect("get Filter")
        .as_name()
        .expect("as name");
    assert_eq!(filter, b"JBIG2Decode");

    // ストリームデータがJBIG2データと一致すること
    assert_eq!(stream.content, jbig2_data);
}

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
fn test_write_mrc_page() {
    // MrcLayersからPDFページを構築し、有効なPDFが生成されることを検証する。
    let layers = MrcLayers {
        background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0], // ダミー背景JPEG
        foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1], // ダミー前景JPEG
        mask_jbig2: vec![0x97, 0x4A, 0x42, 0x32],      // ダミーJBIG2マスク
        width: 640,
        height: 480,
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

#[test]
fn test_redact_overlapping_region() {
    // v1ではplaceholder実装。trueを返すことを確認する。
    assert!(
        redact_region_placeholder(),
        "placeholder should return true"
    );
}
