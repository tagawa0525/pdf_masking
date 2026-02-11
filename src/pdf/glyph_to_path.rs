use std::fmt::Write;

use crate::pdf::content_stream::Matrix;
use crate::pdf::font::PathOp;
use crate::pdf::text_state::FillColor;

/// グリフ→PDFパス変換のパラメータ
pub struct GlyphPathParams<'a> {
    pub outline: &'a [PathOp],
    pub font_size: f64,
    pub units_per_em: u16,
    pub text_matrix: &'a Matrix,
    pub ctm: &'a Matrix,
    pub fill_color: &'a FillColor,
    /// Tz演算子の値（100.0 = デフォルト、50.0 = 半分の幅）
    pub horizontal_scaling: f64,
    /// Ts演算子の値（0.0 = デフォルト、上付き/下付きのオフセット量）
    pub text_rise: f64,
    /// BW強制モード: trueの場合、fill colorを輝度→閾値0.5で0/1に変換
    pub force_bw: bool,
}

/// グリフアウトラインをPDFパス演算子のバイト列に変換する。
///
/// 座標変換（PDF仕様 §9.4.4 テキストレンダリング行列に準拠）:
///   フォント単位 → テキスト空間(÷units_per_em × font_size, Tz/Ts適用) → ページ空間(text_matrix × ctm)
pub fn glyph_to_pdf_path(params: &GlyphPathParams) -> Vec<u8> {
    if params.outline.is_empty() {
        return Vec::new();
    }

    let combined = params.text_matrix.multiply(params.ctm);
    let scale = params.font_size / params.units_per_em as f64;
    let tz = params.horizontal_scaling / 100.0;
    let text_rise = params.text_rise;

    let transform = |x: f64, y: f64| -> (f64, f64) {
        // PDF §9.4.4: Trm = [Tfs×Th 0 0; 0 Tfs 0; 0 Trise 1] × Tm × CTM
        let sx = x * scale * tz;
        let sy = y * scale + text_rise;
        let px = combined.a * sx + combined.c * sy + combined.e;
        let py = combined.b * sx + combined.d * sy + combined.f;
        (px, py)
    };

    let mut buf = String::new();

    // NOTE: `String` への `fmt::Write` は失敗しないため、以下の `.unwrap()` は安全。

    // q: グラフィックス状態を保存
    buf.push_str("q\n");

    // 塗り色を設定
    if params.force_bw {
        // BWモード: 輝度→閾値0.5で0/1に変換
        let luminance = fill_color_luminance(params.fill_color);
        let bw = if luminance >= 0.5 { 1.0 } else { 0.0 };
        write_f64(&mut buf, bw);
        buf.push_str(" g\n");
    } else {
        match params.fill_color {
            FillColor::Gray(g) => {
                write_f64(&mut buf, *g);
                buf.push_str(" g\n");
            }
            FillColor::Rgb(r, g, b) => {
                write_f64(&mut buf, *r);
                buf.push(' ');
                write_f64(&mut buf, *g);
                buf.push(' ');
                write_f64(&mut buf, *b);
                buf.push_str(" rg\n");
            }
            FillColor::Cmyk(c, m, y, k) => {
                write_f64(&mut buf, *c);
                buf.push(' ');
                write_f64(&mut buf, *m);
                buf.push(' ');
                write_f64(&mut buf, *y);
                buf.push(' ');
                write_f64(&mut buf, *k);
                buf.push_str(" k\n");
            }
        }
    }

    // パス演算子を生成（current pointを追跡してQuad→Cubic変換に使用）
    let mut current_x = 0.0_f64;
    let mut current_y = 0.0_f64;

    for op in params.outline {
        match op {
            PathOp::MoveTo(x, y) => {
                current_x = *x;
                current_y = *y;
                let (px, py) = transform(*x, *y);
                write_point_op(&mut buf, px, py, "m");
            }
            PathOp::LineTo(x, y) => {
                current_x = *x;
                current_y = *y;
                let (px, py) = transform(*x, *y);
                write_point_op(&mut buf, px, py, "l");
            }
            PathOp::QuadTo(x1, y1, x2, y2) => {
                // 二次ベジェ(p0, p1, p2) → 三次ベジェ(p0, cp1, cp2, p2)
                // cp1 = p0 + 2/3 * (p1 - p0)
                // cp2 = p2 + 2/3 * (p1 - p2)
                let p0x = current_x;
                let p0y = current_y;
                let cp1x = p0x + 2.0 / 3.0 * (*x1 - p0x);
                let cp1y = p0y + 2.0 / 3.0 * (*y1 - p0y);
                let cp2x = *x2 + 2.0 / 3.0 * (*x1 - *x2);
                let cp2y = *y2 + 2.0 / 3.0 * (*y1 - *y2);

                current_x = *x2;
                current_y = *y2;

                let (px1, py1) = transform(cp1x, cp1y);
                let (px2, py2) = transform(cp2x, cp2y);
                let (px3, py3) = transform(*x2, *y2);
                write_curve_op(&mut buf, px1, py1, px2, py2, px3, py3, "c");
            }
            PathOp::CubicTo(x1, y1, x2, y2, x, y) => {
                // CFF/CFF2の3次ベジェ: そのままPDF `c` 演算子に出力（変換不要）
                let (px1, py1) = transform(*x1, *y1);
                let (px2, py2) = transform(*x2, *y2);
                let (px3, py3) = transform(*x, *y);
                current_x = *x;
                current_y = *y;
                write_curve_op(&mut buf, px1, py1, px2, py2, px3, py3, "c");
            }
            PathOp::Close => {
                buf.push_str("h\n");
            }
        }
    }

    // fill
    buf.push_str("f\n");

    // Q: グラフィックス状態を復元
    buf.push_str("Q\n");

    buf.into_bytes()
}

/// FillColor の輝度を計算する (BW閾値判定用)
fn fill_color_luminance(color: &FillColor) -> f64 {
    match color {
        FillColor::Gray(g) => *g,
        FillColor::Rgb(r, g, b) => 0.299 * *r + 0.587 * *g + 0.114 * *b,
        FillColor::Cmyk(c, m, y, k) => {
            // CMYK → RGB → 輝度
            let r = (1.0 - *c) * (1.0 - *k);
            let g = (1.0 - *m) * (1.0 - *k);
            let b = (1.0 - *y) * (1.0 - *k);
            0.299 * r + 0.587 * g + 0.114 * b
        }
    }
}

/// `v` を小数4桁でフォーマットし、末尾ゼロを除去して `buf` に直接書き込む。
///
/// `String` への `fmt::Write` は失敗しないため `.unwrap()` は安全。
fn write_f64(buf: &mut String, v: f64) {
    let start = buf.len();
    write!(buf, "{v:.4}").unwrap();
    // 末尾の不要なゼロと小数点を除去
    let trimmed_len = buf[start..]
        .trim_end_matches('0')
        .trim_end_matches('.')
        .len();
    // -0 を 0 に正規化
    if buf[start..start + trimmed_len] == *"-0" {
        buf.truncate(start);
        buf.push('0');
    } else {
        buf.truncate(start + trimmed_len);
    }
}

/// 2つの座標値とPDF演算子を `buf` に書き込むヘルパー。
fn write_point_op(buf: &mut String, x: f64, y: f64, op: &str) {
    write_f64(buf, x);
    buf.push(' ');
    write_f64(buf, y);
    buf.push(' ');
    buf.push_str(op);
    buf.push('\n');
}

/// 6つの座標値とPDF演算子を `buf` に書き込むヘルパー。
#[allow(clippy::too_many_arguments)]
fn write_curve_op(
    buf: &mut String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    op: &str,
) {
    write_f64(buf, x1);
    buf.push(' ');
    write_f64(buf, y1);
    buf.push(' ');
    write_f64(buf, x2);
    buf.push(' ');
    write_f64(buf, y2);
    buf.push(' ');
    write_f64(buf, x3);
    buf.push(' ');
    write_f64(buf, y3);
    buf.push(' ');
    buf.push_str(op);
    buf.push('\n');
}
