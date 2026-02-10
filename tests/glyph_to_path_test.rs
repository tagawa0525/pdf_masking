// グリフ→PDFパス変換テスト

use pdf_masking::pdf::content_stream::Matrix;
use pdf_masking::pdf::font::PathOp;
use pdf_masking::pdf::glyph_to_path::{GlyphPathParams, glyph_to_pdf_path};
use pdf_masking::pdf::text_state::FillColor;

/// 出力ストリームから最初のmoveto演算子の座標を抽出するヘルパー
fn first_moveto_coords(text: &str) -> (f32, f32) {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let m_pos = tokens
        .iter()
        .position(|&t| t == "m")
        .expect("path should contain move-to operator 'm'");
    assert!(
        m_pos >= 2,
        "move-to operator should have two preceding operands"
    );
    let x = tokens[m_pos - 2]
        .parse::<f32>()
        .expect("x coordinate before 'm' should be a number");
    let y = tokens[m_pos - 1]
        .parse::<f32>()
        .expect("y coordinate before 'm' should be a number");
    (x, y)
}

/// PDFテキストに指定されたオペレータが含まれているかチェックするヘルパー
fn assert_has_pdf_operator(text: &str, operator: &str) {
    assert!(
        text.split_whitespace().any(|token| token == operator),
        "should contain PDF operator: {}",
        operator
    );
}

// ============================================================
// 1. 基本的なパス変換
// ============================================================

#[test]
fn test_simple_outline_produces_pdf_operators() {
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(500.0, 0.0),
        PathOp::LineTo(250.0, 500.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 12.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    assert_has_pdf_operator(&text, "m");
    assert_has_pdf_operator(&text, "l");
    assert_has_pdf_operator(&text, "h");
    assert_has_pdf_operator(&text, "f");
}

#[test]
fn test_quad_to_cubic_conversion() {
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::QuadTo(500.0, 1000.0, 1000.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 12.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    assert!(
        text.contains(" c"),
        "quad should be converted to cubic bezier"
    );
    assert!(!text.contains(" v"), "should not use v operator");
    assert!(!text.contains(" y"), "should not use y operator");
}

// ============================================================
// 2. 座標変換
// ============================================================

#[test]
fn test_font_units_scaled_by_font_size() {
    let outline = vec![
        PathOp::MoveTo(1000.0, 1000.0),
        PathOp::LineTo(0.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 12.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    // 1000 * (12/1000) = 12 にスケーリングされる
    let (x, y) = first_moveto_coords(&text);
    let epsilon = 0.001_f32;
    assert!(
        (x - 12.0).abs() < epsilon,
        "x coordinate should be scaled to 12.0, got {}",
        x
    );
    assert!(
        (y - 12.0).abs() < epsilon,
        "y coordinate should be scaled to 12.0, got {}",
        y
    );
}

#[test]
fn test_text_matrix_applied() {
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(1000.0, 0.0),
        PathOp::Close,
    ];

    let text_matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 100.0,
        f: 200.0,
    };

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 10.0,
        units_per_em: 1000,
        text_matrix: &text_matrix,
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    // (0,0) がtext_matrixで(100,200)に変換される
    let (x, y) = first_moveto_coords(&text);
    assert!(
        (x - 100.0).abs() < 0.01,
        "expected translated x ≈ 100, got {}",
        x
    );
    assert!(
        (y - 200.0).abs() < 0.01,
        "expected translated y ≈ 200, got {}",
        y
    );
}

// ============================================================
// 3. 塗り色の設定
// ============================================================

#[test]
fn test_fill_color_rgb() {
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(100.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 12.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Rgb(1.0, 0.0, 0.0),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    assert!(text.contains("rg"), "should set RGB fill color");
}

#[test]
fn test_fill_color_gray() {
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(100.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 12.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.5),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    assert!(text.contains(" g"), "should set gray fill color");
}

// ============================================================
// 4. エッジケース
// ============================================================

#[test]
fn test_empty_outline() {
    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &[],
        font_size: 12.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 100.0,
        text_rise: 0.0,
    });

    assert!(
        result.is_empty(),
        "empty outline should produce empty output"
    );
}

// ============================================================
// 5. horizontal_scaling / text_rise
// ============================================================

#[test]
fn test_horizontal_scaling_applies() {
    // Tz=50 → x座標が半分になる
    let outline = vec![
        PathOp::MoveTo(1000.0, 0.0),
        PathOp::LineTo(0.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 10.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 50.0,
        text_rise: 0.0,
    });

    let text = String::from_utf8_lossy(&result);
    let (x, _y) = first_moveto_coords(&text);
    // 1000 * (10/1000) * (50/100) = 5.0
    assert!(
        (x - 5.0).abs() < 0.01,
        "x should be scaled by horizontal_scaling, got {}",
        x
    );
}

#[test]
fn test_text_rise_applies() {
    // Ts=3.0 → y座標に3.0が加算される
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(1000.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(&GlyphPathParams {
        outline: &outline,
        font_size: 10.0,
        units_per_em: 1000,
        text_matrix: &Matrix::identity(),
        ctm: &Matrix::identity(),
        fill_color: &FillColor::Gray(0.0),
        horizontal_scaling: 100.0,
        text_rise: 3.0,
    });

    let text = String::from_utf8_lossy(&result);
    let (_x, y) = first_moveto_coords(&text);
    // y = 0 * (10/1000) + 3.0 = 3.0
    assert!(
        (y - 3.0).abs() < 0.01,
        "y should include text_rise offset, got {}",
        y
    );
}
