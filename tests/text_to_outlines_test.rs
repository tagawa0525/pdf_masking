// テキスト→アウトライン変換テスト

use std::collections::HashMap;

use pdf_masking::pdf::font::{FontEncoding, ParsedFont};
use pdf_masking::pdf::text_to_outlines::{
    convert_text_to_outlines, extract_char_codes_for_encoding, parse_tj_entries_for_encoding,
};

// ============================================================
// ヘルパー: サンプルPDFからフォントを取得
// ============================================================

fn load_sample_fonts() -> HashMap<String, ParsedFont> {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts")
}

// ============================================================
// 1. 空・テキストなし
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
// 2. フォント不在時のフォールバック
// ============================================================

#[test]
fn test_missing_font_returns_error() {
    // フォントマップに含まれないフォントを参照するテキスト → Err
    let content = b"BT /F99 12 Tf (Hello) Tj ET";
    let fonts = HashMap::new();

    let result = convert_text_to_outlines(content, &fonts);
    assert!(
        result.is_err(),
        "missing font should return error for fallback"
    );
}

#[test]
fn test_sample_pdf_missing_embedded_font_returns_error() {
    // サンプルPDFのF1は埋め込みフォントがないため、変換時にエラーになる
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = load_sample_fonts();

    let page_id = doc.page_iter().next().expect("at least one page");
    let content = doc.get_page_content(page_id).expect("get content");

    let result = convert_text_to_outlines(&content, &fonts);
    // F1に埋め込みフォントがないためエラーになる（pdfiumフォールバック用）
    assert!(
        result.is_err(),
        "should error when non-embedded font (F1) is referenced"
    );
}

// ============================================================
// 3. CIDフォント（IdentityH）の2バイト文字コード
// ============================================================

#[test]
fn test_extract_char_codes_identity_h_two_byte_pairs() {
    // IdentityH CIDフォント: バイト列 [0x30, 0xC6] → CID 0x30C6 の1文字
    let obj = lopdf::Object::String(vec![0x30, 0xC6], lopdf::StringFormat::Hexadecimal);
    let codes = extract_char_codes_for_encoding(&obj, &FontEncoding::IdentityH);

    assert_eq!(codes.len(), 1, "2 bytes should produce 1 CID code");
    assert_eq!(codes[0], 0x30C6, "CID should be 0x30C6 (テ)");
}

#[test]
fn test_extract_char_codes_identity_h_multi_chars() {
    // テスト = CID 0x30C6, 0x30B9, 0x30C8
    let obj = lopdf::Object::String(
        vec![0x30, 0xC6, 0x30, 0xB9, 0x30, 0xC8],
        lopdf::StringFormat::Hexadecimal,
    );
    let codes = extract_char_codes_for_encoding(&obj, &FontEncoding::IdentityH);

    assert_eq!(codes.len(), 3, "6 bytes should produce 3 CID codes");
    assert_eq!(codes[0], 0x30C6);
    assert_eq!(codes[1], 0x30B9);
    assert_eq!(codes[2], 0x30C8);
}

#[test]
fn test_extract_char_codes_winansi_unchanged() {
    // WinAnsi: 各バイトが1つの文字コード（従来動作）
    let obj = lopdf::Object::String(vec![0x41, 0x42], lopdf::StringFormat::Literal);
    let encoding = FontEncoding::WinAnsi {
        differences: HashMap::new(),
    };
    let codes = extract_char_codes_for_encoding(&obj, &encoding);

    assert_eq!(codes.len(), 2, "2 bytes should produce 2 codes for WinAnsi");
    assert_eq!(codes[0], 0x41);
    assert_eq!(codes[1], 0x42);
}

#[test]
fn test_parse_tj_entries_identity_h_two_byte_pairs() {
    // TJ配列内の文字列もIdentityHでは2バイトペアとして解釈される
    use pdf_masking::pdf::text_state::TjArrayEntry;

    let arr = lopdf::Object::Array(vec![
        lopdf::Object::String(vec![0x30, 0xC6], lopdf::StringFormat::Hexadecimal),
        lopdf::Object::Integer(-100),
        lopdf::Object::String(vec![0x30, 0xB9], lopdf::StringFormat::Hexadecimal),
    ]);

    let entries = parse_tj_entries_for_encoding(&arr, &FontEncoding::IdentityH);
    assert_eq!(entries.len(), 3, "should have 3 entries (text, adj, text)");

    match &entries[0] {
        TjArrayEntry::Text(codes) => {
            assert_eq!(codes.len(), 1, "first text entry: 2 bytes → 1 CID");
            assert_eq!(codes[0], 0x30C6);
        }
        _ => panic!("expected Text entry"),
    }
    match &entries[1] {
        TjArrayEntry::Adjustment(val) => assert_eq!(*val, -100.0),
        _ => panic!("expected Adjustment entry"),
    }
    match &entries[2] {
        TjArrayEntry::Text(codes) => {
            assert_eq!(codes.len(), 1, "second text entry: 2 bytes → 1 CID");
            assert_eq!(codes[0], 0x30B9);
        }
        _ => panic!("expected Text entry"),
    }
}

// ============================================================
// 4. 合成テストケースで基本変換をテスト
// ============================================================
// 注: F4はWinAnsiEncoding（'A'=0x41のアウトラインあり）
//     F2/F3/F5はIdentityH（CIDフォント、単バイトではアウトライン取得不可）

#[test]
fn test_text_replaced_with_paths() {
    let fonts = load_sample_fonts();
    assert!(fonts.contains_key("F4"), "F4 should be in parsed fonts");

    // F4のみを使うコンテンツストリームを構築
    let content = b"q BT /F4 12 Tf (A) Tj ET Q";

    let result = convert_text_to_outlines(content, &fonts);
    assert!(result.is_ok(), "should succeed: {:?}", result.err());

    let output = result.unwrap();
    let text = String::from_utf8_lossy(&output);

    // BT/ETが出力に残らないこと
    assert!(!text.contains(" BT"), "BT should not appear in output");
    assert!(!text.contains(" ET"), "ET should not appear in output");
    // Tfが出力に残らないこと
    assert!(!text.contains(" Tf"), "Tf should not appear in output");
    // Tjが出力に残らないこと
    assert!(!text.contains(" Tj"), "Tj should not appear in output");
}

#[test]
fn test_output_contains_path_operators() {
    let fonts = load_sample_fonts();
    let content = b"BT /F4 12 Tf (A) Tj ET";

    let result = convert_text_to_outlines(content, &fonts).unwrap();
    let text = String::from_utf8_lossy(&result);

    // パス演算子が含まれること
    let has_moveto = text.split_whitespace().any(|token| token == "m");
    assert!(
        has_moveto,
        "should contain moveto (m) operators as PDF tokens"
    );
    let has_fill = text.split_whitespace().any(|token| token == "f");
    assert!(has_fill, "should contain fill (f) operators as PDF tokens");
}

#[test]
fn test_non_text_operators_preserved() {
    let fonts = load_sample_fonts();
    let content = b"q 1 0 0 1 0 0 cm BT /F4 12 Tf (A) Tj ET Q";

    let result = convert_text_to_outlines(content, &fonts).unwrap();
    let text = String::from_utf8_lossy(&result);

    // q/Q（グラフィックス状態保存/復元）が保持される
    assert!(text.contains("q"), "should preserve q operator");
    assert!(text.contains("Q"), "should preserve Q operator");
    assert!(text.contains("cm"), "should preserve cm operator");
}

#[test]
fn test_output_is_valid_content_stream() {
    let fonts = load_sample_fonts();
    let content = b"q BT /F4 10 Tf (A) Tj ET Q";

    let result = convert_text_to_outlines(content, &fonts).unwrap();

    // 出力が空でないこと（q/Qとパスバイト列が含まれる）
    assert!(!result.is_empty(), "output should not be empty");

    let text = String::from_utf8_lossy(&result);
    // パスが生成されていること
    let has_moveto = text.split_whitespace().any(|token| token == "m");
    assert!(
        has_moveto,
        "should contain moveto (m) operators as PDF tokens"
    );
}

#[test]
fn test_multiple_text_blocks() {
    let fonts = load_sample_fonts();
    // 複数のBT...ETブロック（F4で同じ文字'A'を異なるサイズで描画）
    let content = b"BT /F4 12 Tf (A) Tj ET BT /F4 10 Tf (A) Tj ET";

    let result = convert_text_to_outlines(content, &fonts).unwrap();
    let text = String::from_utf8_lossy(&result);

    // 複数のグリフパスが生成される
    let m_count = text.matches(" m").count();
    assert!(
        m_count >= 2,
        "should have moveto for both glyphs, got {}",
        m_count
    );
}
