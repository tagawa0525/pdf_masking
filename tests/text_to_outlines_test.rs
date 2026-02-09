// テキスト→アウトライン変換テスト (RED phase)

use std::collections::HashMap;

use pdf_masking::pdf::font::ParsedFont;
use pdf_masking::pdf::text_to_outlines::convert_text_to_outlines;

// ============================================================
// ヘルパー: サンプルPDFからフォントを取得
// ============================================================

fn load_sample_fonts() -> HashMap<String, ParsedFont> {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts")
}

fn sample_content_bytes() -> Vec<u8> {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let page_id = doc.page_iter().next().expect("at least one page");
    let content = doc.get_page_content(page_id).expect("get content");
    content
}

// ============================================================
// 1. 基本変換
// ============================================================

#[test]
fn test_text_removed_from_output() {
    // BT...ETブロックがPDFテキストオペレータとして出力に残らないこと
    let content = sample_content_bytes();
    let fonts = load_sample_fonts();

    let result = convert_text_to_outlines(&content, &fonts);
    assert!(
        result.is_ok(),
        "conversion should succeed: {:?}",
        result.err()
    );

    let output = result.unwrap();
    let text = String::from_utf8_lossy(&output);

    // BT/ETペアが存在しないこと（テキストオペレータが除去された）
    assert!(!text.contains("\nBT\n"), "BT should not appear in output");
    assert!(!text.contains("\nET\n"), "ET should not appear in output");
}

#[test]
fn test_output_contains_path_operators() {
    // テキストがパス演算子(m, l, c, h, f)に変換されること
    let content = sample_content_bytes();
    let fonts = load_sample_fonts();

    let result = convert_text_to_outlines(&content, &fonts).unwrap();
    let text = String::from_utf8_lossy(&result);

    // パス演算子が含まれること
    assert!(text.contains(" m"), "should contain moveto operators");
    assert!(text.contains("\nf"), "should contain fill operators");
}

#[test]
fn test_non_text_operators_preserved() {
    // 画像Do, ベクター描画などの非テキストオペレータが保持されること
    let content = sample_content_bytes();
    let fonts = load_sample_fonts();

    let result = convert_text_to_outlines(&content, &fonts).unwrap();
    let text = String::from_utf8_lossy(&result);

    // q/Q（グラフィックス状態保存/復元）が保持される
    assert!(
        text.contains("\nq\n") || text.contains("\nq "),
        "should preserve q operator"
    );
    assert!(
        text.contains("\nQ\n") || text.contains("\nQ "),
        "should preserve Q operator"
    );
}

// ============================================================
// 2. 空・テキストなし
// ============================================================

#[test]
fn test_empty_content() {
    let fonts = HashMap::new();
    let result = convert_text_to_outlines(&[], &fonts);
    assert!(result.is_ok());
    assert!(
        result.unwrap().is_empty(),
        "empty input should produce empty output"
    );
}

#[test]
fn test_no_text_blocks() {
    // テキストブロックがないコンテンツ → そのまま返す
    let content = b"q 1 0 0 1 0 0 cm /Im0 Do Q";
    let fonts = HashMap::new();

    let result = convert_text_to_outlines(content, &fonts).unwrap();
    let text = String::from_utf8_lossy(&result);

    // 元のオペレータが保持される
    assert!(text.contains("Do"), "should preserve Do operator");
}

// ============================================================
// 3. フォント不在時のフォールバック
// ============================================================

#[test]
fn test_missing_font_returns_error() {
    // フォントマップに含まれないフォントを参照するテキスト → Err
    let content = b"BT /F99 12 Tf (Hello) Tj ET";
    let fonts = HashMap::new(); // 空のフォントマップ

    let result = convert_text_to_outlines(content, &fonts);
    assert!(
        result.is_err(),
        "missing font should return error for fallback"
    );
}

// ============================================================
// 4. サンプルPDFでの結合テスト
// ============================================================

#[test]
fn test_sample_pdf_roundtrip() {
    // サンプルPDFの全ページでconvert_text_to_outlinesが成功すること
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = load_sample_fonts();

    // ページ1のみテスト（フォントは先頭ページ用に読み込み済み）
    let page_id = doc.page_iter().next().expect("at least one page");
    let content = doc.get_page_content(page_id).expect("get content");

    let result = convert_text_to_outlines(&content, &fonts);
    assert!(
        result.is_ok(),
        "page 1 conversion failed: {:?}",
        result.err()
    );
}

#[test]
fn test_output_is_valid_content_stream() {
    // 出力がlopdfでデコード可能な有効なコンテンツストリームであること
    let content = sample_content_bytes();
    let fonts = load_sample_fonts();

    let result = convert_text_to_outlines(&content, &fonts).unwrap();

    // lopdfでデコードできることを確認
    let decoded = lopdf::content::Content::decode(&result);
    assert!(
        decoded.is_ok(),
        "output should be a valid content stream: {:?}",
        decoded.err()
    );
}
