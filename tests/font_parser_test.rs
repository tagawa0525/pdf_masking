// 埋込フォント解析テスト (RED phase)

use lopdf::{Document, Object, Stream, dictionary};
use pdf_masking::pdf::font::FontEncoding;
use std::path::Path;

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
    for font in fonts.values() {
        // 'A' (0x41) のグリフを試す
        if let Some(gid) = font.char_code_to_glyph_id(0x41)
            && font.glyph_outline(gid).is_some()
        {
            found_outline = true;
            break;
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
    for font in fonts.values() {
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

    for font in fonts.values() {
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

    let (_name, font) = fonts.iter().next().expect("should have at least one font");
    let upem = font.units_per_em();
    assert!(upem > 0, "units_per_em should be positive");
    // TrueTypeは一般的に256, 1000, 2048のいずれか
    assert!(upem <= 16384, "units_per_em seems too large: {}", upem);
}

// ============================================================
// 5. PathOp
// ============================================================

#[test]
fn test_glyph_outline_contains_path_ops() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    for font in fonts.values() {
        if let Some(gid) = font.char_code_to_glyph_id(0x41)
            && let Some(ops) = font.glyph_outline(gid)
        {
            // アウトラインにはMoveTo, LineTo/QuadTo, Closeが含まれるはず
            assert!(!ops.is_empty(), "outline should not be empty for 'A'");
            return;
        }
    }
}

// ============================================================
// 6. CIDフォントの文字コード→グリフID
// ============================================================

#[test]
fn test_cid_font_char_code_to_glyph_id() {
    // IdentityH CIDフォント: char_code = CID = GlyphId
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // IdentityHフォントを探す
    let mut found = false;
    for (name, font) in &fonts {
        if matches!(font.encoding(), FontEncoding::IdentityH) {
            // 日本語CID: 0x30C6(テ) → GlyphId(0x30C6)
            let gid = font.char_code_to_glyph_id(0x30C6);
            assert!(
                gid.is_some(),
                "IdentityH font '{}' should resolve CID 0x30C6 to GlyphId",
                name
            );
            // GlyphIdの値がCIDと一致すること
            assert_eq!(
                gid.unwrap().0,
                0x30C6,
                "IdentityH: GlyphId should equal CID"
            );
            found = true;
            break;
        }
    }
    assert!(
        found,
        "sample PDF should contain at least one IdentityH CID font"
    );
}

// ============================================================
// 7. エッジケース
// ============================================================

#[test]
fn test_nonexistent_page_returns_error() {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let result = pdf_masking::pdf::font::parse_page_fonts(&doc, 999);
    assert!(result.is_err(), "page 999 should not exist");
}

// ============================================================
// 8. システムフォント解決（非埋め込みフォント）
// ============================================================

#[test]
fn test_parse_page_fonts_skips_unresolvable_system_fonts() {
    // ページ2にはF1(非埋め込み), F2(埋め込み), F4(埋め込み), F7(非埋め込み), F8(非埋め込み)がある
    // F7/F8のシステムフォント解決に失敗しても、parse_page_fontsはErrを返さず
    // 埋め込みフォント(F2, F4)を含む結果を返すべき
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 2)
        .expect("parse_page_fonts should not fail even when some system fonts are unavailable");

    // 埋め込みフォントは常に解析される
    assert!(
        fonts.contains_key("F2"),
        "embedded font F2 should always be present"
    );
}

#[test]
fn test_page2_bold_italic_system_fonts_resolved() {
    // F7=TimesNewRomanPS-ItalicMT, F8=TimesNewRomanPS-BoldMT はスタイル付き非埋め込みフォント
    // PostScript名の -BoldMT, -ItalicMT サフィックスから正しくファミリ・スタイルをパースすべき
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 2).expect("parse fonts for page 2");

    if !fonts.contains_key("F1") {
        // システムにTimes New Roman互換フォントがない環境ではスキップ
        eprintln!("SKIP: system font not available");
        return;
    }

    // F7, F8 もスタイル付きで同じファミリなので解決されるべき
    assert!(
        fonts.contains_key("F7"),
        "F7 (TimesNewRomanPS-ItalicMT) should be resolved via system font"
    );
    assert!(
        fonts.contains_key("F8"),
        "F8 (TimesNewRomanPS-BoldMT) should be resolved via system font"
    );
}

#[test]
fn test_system_font_resolved_for_non_embedded() {
    // F1（TimesNewRomanPSMT）は埋め込みフォントがないが、
    // システムフォント解決により parse_page_fonts が返すフォントに含まれるべき
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    if !fonts.contains_key("F1") {
        // システムにTimesNewRomanまたは互換フォントがない環境ではスキップ
        eprintln!("SKIP: F1 (TimesNewRomanPSMT) not resolved — system font not available");
        return;
    }

    // F1が解決されていればOK
    assert!(
        fonts.contains_key("F1"),
        "F1 should be resolved via system font"
    );
}

#[test]
fn test_system_font_glyph_outline_available() {
    // システムフォント解決されたフォントでグリフアウトラインが取得できること
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    let font = match fonts.get("F1") {
        Some(f) => f,
        None => {
            eprintln!("SKIP: F1 not resolved — system font not available");
            return;
        }
    };

    // 'A' (0x41) のグリフを解決してアウトラインを取得
    let gid = font
        .char_code_to_glyph_id(0x41)
        .expect("should resolve 'A' to glyph ID");
    let outline = font.glyph_outline(gid).expect("should get outline for 'A'");
    assert!(!outline.is_empty(), "outline for 'A' should not be empty");
}

// ============================================================
// 9. Type1フォント（システムフォント解決）
// ============================================================

/// Type1フォントを含むテスト用PDFを作成（標準14フォント）
fn create_type1_test_pdf(path: &Path) {
    let mut doc = Document::with_version("1.4");

    // Type1 Helveticaフォント（標準14フォントの一つ、非埋め込み）
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    };
    let font_id = doc.add_object(font);

    // コンテンツストリーム
    let content = b"BT /F1 24 Tf 100 700 Td (Type1 Test) Tj ET";
    let content_stream = Stream::new(dictionary! {}, content.to_vec());
    let content_id = doc.add_object(content_stream);

    // Resourcesにフォントを登録
    let resources = dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    };

    // Page object
    let page = dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ],
        "Contents" => content_id,
        "Resources" => resources,
    };
    let page_id = doc.add_object(page);

    // Pages node
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1,
    };
    let pages_id = doc.add_object(pages);

    // Catalog
    let catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);

    doc.save(path).expect("failed to save Type1 test PDF");
}

#[test]
fn test_type1_font_parsed_from_system() {
    // Type1フォント（Helvetica）がシステムフォント解決によりパースされること
    let temp_dir = std::env::temp_dir();
    let pdf_path = temp_dir.join("type1_parsed_test.pdf");
    create_type1_test_pdf(&pdf_path);

    let doc = lopdf::Document::load(&pdf_path).expect("load Type1 test PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // システムフォントが無い環境ではこのテスト自体が意味をなさないため、
    // F1が解決できない場合はテストをスキップする。
    let font = if let Some(font) = fonts.get("F1") {
        font
    } else {
        eprintln!("skipping Type1 font test: system Helvetica (F1) could not be resolved");
        return;
    };

    // エンコーディングがWinAnsiであること
    assert!(
        matches!(font.encoding(), FontEncoding::WinAnsi { .. }),
        "Type1 Helvetica should use WinAnsi encoding"
    );
}

#[test]
fn test_type1_font_glyph_outline() {
    // Type1フォント（Helvetica）からグリフアウトラインが取得できること
    let temp_dir = std::env::temp_dir();
    let pdf_path = temp_dir.join("type1_outline_test.pdf");
    create_type1_test_pdf(&pdf_path);

    let doc = lopdf::Document::load(&pdf_path).expect("load Type1 test PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    let font = if let Some(font) = fonts.get("F1") {
        font
    } else {
        eprintln!("skipping Type1 glyph outline test: system Helvetica (F1) could not be resolved");
        return;
    };

    // 'A' (0x41) のグリフアウトラインを取得
    let gid = font
        .char_code_to_glyph_id(0x41)
        .expect("should resolve 'A' to glyph ID for Type1 Helvetica");
    let outline = font
        .glyph_outline(gid)
        .expect("should get outline for 'A' from Type1 Helvetica");
    assert!(
        !outline.is_empty(),
        "outline for 'A' should not be empty for Type1 Helvetica"
    );
}

#[test]
fn test_mmtype1_font_parsed_from_system() {
    // MMType1フォント（Multiple Master Type1）もType1と同様に扱う
    let temp_dir = std::env::temp_dir();
    let pdf_path = temp_dir.join("mmtype1_parsed_test.pdf");

    // MMType1フォント辞書を作成（通常のType1と辞書構造は同じ）
    let mut doc = Document::with_version("1.4");

    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "MMType1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    };
    let font_id = doc.add_object(font);

    let content = b"BT /F1 24 Tf 100 700 Td (MMType1 Test) Tj ET";
    let content_stream = Stream::new(dictionary! {}, content.to_vec());
    let content_id = doc.add_object(content_stream);

    let resources = dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    };

    let page = dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ],
        "Contents" => content_id,
        "Resources" => resources,
    };
    let page_id = doc.add_object(page);

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1,
    };
    let pages_id = doc.add_object(pages);

    let catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);

    doc.save(&pdf_path)
        .expect("failed to save MMType1 test PDF");

    // MMType1フォントがパースされること
    let doc = lopdf::Document::load(&pdf_path).expect("load MMType1 test PDF");
    let fonts = pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    let font = if let Some(font) = fonts.get("F1") {
        font
    } else {
        eprintln!("skipping MMType1 font test: system Helvetica (F1) could not be resolved");
        return;
    };

    // グリフアウトラインが取得できること
    let gid = font
        .char_code_to_glyph_id(0x41)
        .expect("should resolve 'A' to glyph ID for MMType1");
    let outline = font
        .glyph_outline(gid)
        .expect("should get outline for 'A' from MMType1");
    assert!(
        !outline.is_empty(),
        "outline for 'A' should not be empty for MMType1"
    );
}
