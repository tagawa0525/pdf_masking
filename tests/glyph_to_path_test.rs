// グリフ→PDFパス変換テスト (RED phase)

use pdf_masking::pdf::content_stream::Matrix;
use pdf_masking::pdf::font::PathOp;
use pdf_masking::pdf::glyph_to_path::glyph_to_pdf_path;
use pdf_masking::pdf::text_state::FillColor;

// ============================================================
// 1. 基本的なパス変換
// ============================================================

#[test]
fn test_simple_outline_produces_pdf_operators() {
    // 簡単な三角形のアウトライン
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::LineTo(500.0, 0.0),
        PathOp::LineTo(250.0, 500.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(
        &outline,
        12.0,
        1000,
        &Matrix::identity(),
        &Matrix::identity(),
        &FillColor::Gray(0.0),
    );

    let text = String::from_utf8_lossy(&result);
    // PDFパス演算子が含まれること
    assert!(text.contains(" m"), "should contain moveto");
    assert!(text.contains(" l"), "should contain lineto");
    assert!(text.contains("\nh"), "should contain closepath");
    assert!(text.contains("\nf"), "should contain fill");
}

#[test]
fn test_quad_to_cubic_conversion() {
    // TrueTypeの二次ベジェ → PDFの三次ベジェ変換
    let outline = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::QuadTo(500.0, 1000.0, 1000.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(
        &outline,
        12.0,
        1000,
        &Matrix::identity(),
        &Matrix::identity(),
        &FillColor::Gray(0.0),
    );

    let text = String::from_utf8_lossy(&result);
    // PDFの三次ベジェ(c)演算子に変換されること
    assert!(
        text.contains(" c"),
        "quad should be converted to cubic bezier"
    );
    // 二次ベジェ(v/y)は使わない
    assert!(!text.contains(" v"), "should not use v operator");
    assert!(!text.contains(" y"), "should not use y operator");
}

// ============================================================
// 2. 座標変換
// ============================================================

#[test]
fn test_font_units_scaled_by_font_size() {
    // font_size=12, units_per_em=1000 → scale = 12/1000 = 0.012
    let outline = vec![
        PathOp::MoveTo(1000.0, 1000.0),
        PathOp::LineTo(0.0, 0.0),
        PathOp::Close,
    ];

    let result = glyph_to_pdf_path(
        &outline,
        12.0,
        1000,
        &Matrix::identity(),
        &Matrix::identity(),
        &FillColor::Gray(0.0),
    );

    let text = String::from_utf8_lossy(&result);
    // 1000 * (12/1000) = 12 にスケーリングされる
    assert!(
        text.contains("12"),
        "coordinates should be scaled to text space"
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

    let result = glyph_to_pdf_path(
        &outline,
        10.0,
        1000,
        &text_matrix,
        &Matrix::identity(),
        &FillColor::Gray(0.0),
    );

    let text = String::from_utf8_lossy(&result);
    // (0,0) がtext_matrixで(100,200)に変換される
    assert!(
        text.contains("100"),
        "text_matrix translation should be applied"
    );
    assert!(
        text.contains("200"),
        "text_matrix translation should be applied"
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

    let result = glyph_to_pdf_path(
        &outline,
        12.0,
        1000,
        &Matrix::identity(),
        &Matrix::identity(),
        &FillColor::Rgb(1.0, 0.0, 0.0),
    );

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

    let result = glyph_to_pdf_path(
        &outline,
        12.0,
        1000,
        &Matrix::identity(),
        &Matrix::identity(),
        &FillColor::Gray(0.5),
    );

    let text = String::from_utf8_lossy(&result);
    assert!(text.contains(" g"), "should set gray fill color");
}

// ============================================================
// 4. エッジケース
// ============================================================

#[test]
fn test_empty_outline() {
    let result = glyph_to_pdf_path(
        &[],
        12.0,
        1000,
        &Matrix::identity(),
        &Matrix::identity(),
        &FillColor::Gray(0.0),
    );

    assert!(
        result.is_empty(),
        "empty outline should produce empty output"
    );
}
