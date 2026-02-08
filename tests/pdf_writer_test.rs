// Phase 7: PDF構築（MRC → PDF）テスト

use lopdf::Document;
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
