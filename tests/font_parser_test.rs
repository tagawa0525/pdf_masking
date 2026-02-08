// 埋込フォント解析テスト (RED phase)

use pdf_masking::pdf::font::{FontEncoding, ParsedFont};

// ============================================================
// 1. サンプルPDFからのフォント解析
// ============================================================

#[test]
fn test_parse_font_from_sample_pdf() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");

    // ページ1のフォントリソースを取得
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // サンプルPDFにはフォントが含まれるはず
    assert!(!fonts.is_empty(), "should find at least one font");
}

#[test]
fn test_parsed_font_has_glyph_outlines() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // 少なくとも1つのフォントでグリフアウトラインが取得できる
    let mut found_outline = false;
    for (_name, font) in &fonts {
        // 'A' (0x41) のグリフを試す
        if let Some(gid) = font.char_code_to_glyph_id(0x41) {
            if font.glyph_outline(gid).is_some() {
                found_outline = true;
                break;
            }
        }
    }
    assert!(
        found_outline,
        "should get glyph outline for at least one font"
    );
}

// ============================================================
// 2. エンコーディング
// ============================================================

#[test]
fn test_winansii_char_code_to_glyph() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // WinAnsiEncoding のフォントを探す
    for (_name, font) in &fonts {
        if matches!(font.encoding(), FontEncoding::WinAnsi { .. }) {
            // 'A' = 0x41 はWinAnsiで有効な文字コード
            let gid = font.char_code_to_glyph_id(0x41);
            assert!(gid.is_some(), "WinAnsi font should resolve 'A'");
            return;
        }
    }
    // WinAnsiフォントが無い場合はスキップ（CIDフォントのみの場合）
}

// ============================================================
// 3. グリフ幅
// ============================================================

#[test]
fn test_glyph_width_positive() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    for (_name, font) in &fonts {
        // 何らかの文字コードの幅が正の値であること
        let width = font.glyph_width(0x41); // 'A'
        if width > 0.0 {
            return; // OK
        }
    }
    // 少なくとも1つのフォントで幅が取得できるはず
    panic!("no font returned positive width for 'A'");
}

// ============================================================
// 4. units_per_em
// ============================================================

#[test]
fn test_units_per_em() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    for (_name, font) in &fonts {
        let upem = font.units_per_em();
        assert!(upem > 0, "units_per_em should be positive");
        // TrueTypeは一般的に1000または2048
        assert!(
            upem == 1000 || upem == 2048,
            "unexpected units_per_em: {}",
            upem
        );
        return;
    }
}

// ============================================================
// 5. PathOp
// ============================================================

#[test]
fn test_glyph_outline_contains_path_ops() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    for (_name, font) in &fonts {
        if let Some(gid) = font.char_code_to_glyph_id(0x41) {
            if let Some(ops) = font.glyph_outline(gid) {
                // アウトラインにはMoveTo, LineTo/QuadTo, Closeが含まれるはず
                assert!(!ops.is_empty(), "outline should not be empty for 'A'");
                return;
            }
        }
    }
}

// ============================================================
// 6. エッジケース
// ============================================================

#[test]
fn test_nonexistent_page_returns_error() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let result = pdf_masking::pdf::font::parse_page_fonts(&doc, 999);
    assert!(result.is_err(), "page 999 should not exist");
}
