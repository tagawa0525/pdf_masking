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

    // q: グラフィックス状態を保存
    writeln!(buf, "q").unwrap();

    // 塗り色を設定
    match params.fill_color {
        FillColor::Gray(g) => {
            writeln!(buf, "{} g", format_f64(*g)).unwrap();
        }
        FillColor::Rgb(r, g, b) => {
            writeln!(
                buf,
                "{} {} {} rg",
                format_f64(*r),
                format_f64(*g),
                format_f64(*b)
            )
            .unwrap();
        }
        FillColor::Cmyk(c, m, y, k) => {
            writeln!(
                buf,
                "{} {} {} {} k",
                format_f64(*c),
                format_f64(*m),
                format_f64(*y),
                format_f64(*k)
            )
            .unwrap();
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
                writeln!(buf, "{} {} m", format_f64(px), format_f64(py)).unwrap();
            }
            PathOp::LineTo(x, y) => {
                current_x = *x;
                current_y = *y;
                let (px, py) = transform(*x, *y);
                writeln!(buf, "{} {} l", format_f64(px), format_f64(py)).unwrap();
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
                writeln!(
                    buf,
                    "{} {} {} {} {} {} c",
                    format_f64(px1),
                    format_f64(py1),
                    format_f64(px2),
                    format_f64(py2),
                    format_f64(px3),
                    format_f64(py3)
                )
                .unwrap();
            }
            PathOp::CubicTo(_x1, _y1, _x2, _y2, _x, _y) => {
                // TODO: CubicTo rendering not yet implemented
            }
            PathOp::Close => {
                writeln!(buf, "h").unwrap();
            }
        }
    }

    // fill
    writeln!(buf, "f").unwrap();

    // Q: グラフィックス状態を復元
    writeln!(buf, "Q").unwrap();

    buf.into_bytes()
}

fn format_f64(v: f64) -> String {
    // 不要な末尾ゼロを除去しつつ適切な精度で出力
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    // -0 を 0 に正規化
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}
