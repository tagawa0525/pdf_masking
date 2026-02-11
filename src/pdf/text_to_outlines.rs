use std::collections::HashMap;

use lopdf::content::Content;

use crate::error::{PdfMaskError, Result};
use crate::pdf::content_stream::{Matrix, operand_to_f64};
use crate::pdf::font::{FontEncoding, ParsedFont};
use crate::pdf::glyph_to_path::{GlyphPathParams, glyph_to_pdf_path};
use crate::pdf::text_state::{
    FillColor, TextState, TjArrayEntry, extract_tj_array_for_encoding, lookup_encoding,
};

// Re-export for backward compatibility (tests import from here)
pub use crate::pdf::text_state::extract_char_codes_for_encoding;

/// BT...ETブロックをベクターパスに変換したコンテンツストリームを返す。
///
/// フォントが見つからない場合はErrを返し、呼び出し元でpdfiumフォールバックに切り替える。
pub fn convert_text_to_outlines(
    content_bytes: &[u8],
    fonts: &HashMap<String, ParsedFont>,
    force_bw: bool,
) -> Result<Vec<u8>> {
    if content_bytes.is_empty() {
        return Ok(Vec::new());
    }

    let content =
        Content::decode(content_bytes).map_err(|e| PdfMaskError::content_stream(e.to_string()))?;

    let mut output_ops: Vec<lopdf::content::Operation> = Vec::new();
    let mut path_bytes: Vec<u8> = Vec::new();

    // テキスト状態追跡
    let mut ctm_stack: Vec<Matrix> = vec![Matrix::identity()];
    let mut fill_color_stack: Vec<FillColor> = vec![FillColor::default_black()];
    let mut in_text = false;
    let mut ts = TextState::new();

    // BT...ETブロック内のパスバイトをバッファリングし、ETで出力に挿入
    let mut text_path_buf: Vec<u8> = Vec::new();

    for op in &content.operations {
        match op.operator.as_str() {
            // --- グラフィックス状態 ---
            "q" => {
                let current_ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                ctm_stack.push(current_ctm);
                let current_fc = fill_color_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(FillColor::default_black);
                fill_color_stack.push(current_fc);
                if !in_text {
                    output_ops.push(op.clone());
                }
            }
            "Q" => {
                if ctm_stack.len() > 1 {
                    ctm_stack.pop();
                }
                if fill_color_stack.len() > 1 {
                    fill_color_stack.pop();
                }
                if !in_text {
                    output_ops.push(op.clone());
                }
            }
            "cm" => {
                if op.operands.len() == 6 {
                    let vals: Vec<f64> = op
                        .operands
                        .iter()
                        .map(operand_to_f64)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    let cm = Matrix {
                        a: vals[0],
                        b: vals[1],
                        c: vals[2],
                        d: vals[3],
                        e: vals[4],
                        f: vals[5],
                    };
                    if let Some(current) = ctm_stack.last_mut() {
                        *current = current.multiply(&cm);
                    }
                }
                if !in_text {
                    output_ops.push(op.clone());
                }
            }

            // --- Fill color ---
            "rg" | "g" | "k" | "sc" | "scn" => {
                apply_fill_color(op.operator.as_str(), &op.operands, &mut fill_color_stack);
                if !in_text {
                    output_ops.push(op.clone());
                }
            }

            // --- テキストブロック ---
            "BT" => {
                in_text = true;
                ts = TextState::new();
                text_path_buf.clear();
            }
            "ET" => {
                in_text = false;
                // BT...ETブロック内で生成されたパスバイトを出力に追加
                if !text_path_buf.is_empty() {
                    path_bytes.extend_from_slice(&text_path_buf);
                    text_path_buf.clear();
                }
            }

            // --- テキスト状態オペレータ ---
            "Tf" | "Tm" | "Td" | "TD" | "TL" | "T*" | "Tc" | "Tw" | "Tz" | "Ts" | "Tr"
                if in_text =>
            {
                ts.apply_text_state_op(op.operator.as_str(), &op.operands)?;
            }

            // --- テキスト描画オペレータ ---
            "Tj" if in_text => {
                if let Some(operand) = op.operands.first() {
                    render_show_text(
                        operand,
                        &mut ts,
                        &ctm_stack,
                        &fill_color_stack,
                        fonts,
                        &mut text_path_buf,
                        force_bw,
                    )?;
                }
            }
            "TJ" if in_text => {
                if let Some(operand) = op.operands.first() {
                    render_show_text_array(
                        operand,
                        &mut ts,
                        &ctm_stack,
                        &fill_color_stack,
                        fonts,
                        &mut text_path_buf,
                        force_bw,
                    )?;
                }
            }
            "'" if in_text => {
                ts.apply_t_star();
                if let Some(operand) = op.operands.first() {
                    render_show_text(
                        operand,
                        &mut ts,
                        &ctm_stack,
                        &fill_color_stack,
                        fonts,
                        &mut text_path_buf,
                        force_bw,
                    )?;
                }
            }
            "\"" if in_text => {
                if op.operands.len() == 3 {
                    if let Ok(aw) = operand_to_f64(&op.operands[0]) {
                        ts.word_spacing = aw;
                    }
                    if let Ok(ac) = operand_to_f64(&op.operands[1]) {
                        ts.char_spacing = ac;
                    }
                    ts.apply_t_star();
                    render_show_text(
                        &op.operands[2],
                        &mut ts,
                        &ctm_stack,
                        &fill_color_stack,
                        fonts,
                        &mut text_path_buf,
                        force_bw,
                    )?;
                }
            }

            // BT内のその他のオペレータは無視
            _ if in_text => {}

            // BT外のオペレータはそのまま保持
            _ => {
                output_ops.push(op.clone());
            }
        }
    }

    // 非テキストオペレータをエンコード
    let non_text_content = Content {
        operations: output_ops,
    };
    let mut result = non_text_content.encode().map_err(|e| {
        PdfMaskError::content_stream(format!("failed to encode non-text content: {}", e))
    })?;

    // パスバイト列を追加
    result.extend_from_slice(&path_bytes);

    Ok(result)
}

/// Fill colorオペレータを適用する。
fn apply_fill_color(
    operator: &str,
    operands: &[lopdf::Object],
    fill_color_stack: &mut [FillColor],
) {
    let Some(fc) = fill_color_stack.last_mut() else {
        return;
    };
    match operator {
        "rg" => {
            if operands.len() == 3
                && let (Ok(r), Ok(g), Ok(b)) = (
                    operand_to_f64(&operands[0]),
                    operand_to_f64(&operands[1]),
                    operand_to_f64(&operands[2]),
                )
            {
                *fc = FillColor::Rgb(r, g, b);
            }
        }
        "g" => {
            if operands.len() == 1
                && let Ok(gray) = operand_to_f64(&operands[0])
            {
                *fc = FillColor::Gray(gray);
            }
        }
        "k" => {
            if operands.len() == 4
                && let (Ok(c), Ok(m), Ok(y), Ok(k)) = (
                    operand_to_f64(&operands[0]),
                    operand_to_f64(&operands[1]),
                    operand_to_f64(&operands[2]),
                    operand_to_f64(&operands[3]),
                )
            {
                *fc = FillColor::Cmyk(c, m, y, k);
            }
        }
        "sc" | "scn" => match operands.len() {
            1 => {
                if let Ok(gray) = operand_to_f64(&operands[0]) {
                    *fc = FillColor::Gray(gray);
                }
            }
            3 => {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    operand_to_f64(&operands[0]),
                    operand_to_f64(&operands[1]),
                    operand_to_f64(&operands[2]),
                ) {
                    *fc = FillColor::Rgb(r, g, b);
                }
            }
            4 => {
                if let (Ok(c), Ok(m), Ok(y), Ok(k)) = (
                    operand_to_f64(&operands[0]),
                    operand_to_f64(&operands[1]),
                    operand_to_f64(&operands[2]),
                    operand_to_f64(&operands[3]),
                ) {
                    *fc = FillColor::Cmyk(c, m, y, k);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// Tj型テキスト描画（文字列からコードを抽出してレンダリング）
fn render_show_text(
    operand: &lopdf::Object,
    ts: &mut TextState,
    ctm_stack: &[Matrix],
    fill_color_stack: &[FillColor],
    fonts: &HashMap<String, ParsedFont>,
    output: &mut Vec<u8>,
    force_bw: bool,
) -> Result<()> {
    let encoding = lookup_encoding(&ts.font_name, Some(fonts));
    let codes = extract_char_codes_for_encoding(operand, encoding);
    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
    let fill_color = fill_color_stack
        .last()
        .cloned()
        .unwrap_or_else(FillColor::default_black);
    render_text_codes(&codes, ts, &ctm, &fill_color, fonts, output, force_bw)
}

/// TJ型テキスト描画（配列からコードと位置調整を処理）
fn render_show_text_array(
    operand: &lopdf::Object,
    ts: &mut TextState,
    ctm_stack: &[Matrix],
    fill_color_stack: &[FillColor],
    fonts: &HashMap<String, ParsedFont>,
    output: &mut Vec<u8>,
    force_bw: bool,
) -> Result<()> {
    let encoding = lookup_encoding(&ts.font_name, Some(fonts));
    let (_, entries) = extract_tj_array_for_encoding(operand, encoding);
    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
    let fill_color = fill_color_stack
        .last()
        .cloned()
        .unwrap_or_else(FillColor::default_black);
    for entry in &entries {
        match entry {
            TjArrayEntry::Text(codes) => {
                render_text_codes(codes, ts, &ctm, &fill_color, fonts, output, force_bw)?;
            }
            TjArrayEntry::Adjustment(val) => {
                ts.advance_by_tj_adjustment(*val, ts.font_size);
            }
        }
    }
    Ok(())
}

/// 文字コード列をグリフパスに変換して出力バッファに追加
fn render_text_codes(
    codes: &[u16],
    ts: &mut TextState,
    ctm: &Matrix,
    fill_color: &FillColor,
    fonts: &HashMap<String, ParsedFont>,
    output: &mut Vec<u8>,
    force_bw: bool,
) -> Result<()> {
    let font = fonts
        .get(&ts.font_name)
        .ok_or_else(|| PdfMaskError::content_stream(format!("font not found: {}", ts.font_name)))?;

    for &code in codes {
        // グリフ解決
        if let Some(glyph_id) = font.char_code_to_glyph_id(code)
            && let Some(outline) = font.glyph_outline(glyph_id)
        {
            let path_bytes = glyph_to_pdf_path(&GlyphPathParams {
                outline: &outline,
                font_size: ts.font_size,
                units_per_em: font.units_per_em(),
                text_matrix: &ts.text_matrix,
                ctm,
                fill_color,
                horizontal_scaling: ts.horizontal_scaling,
                text_rise: ts.text_rise,
                force_bw,
            });
            output.extend_from_slice(&path_bytes);
        }

        // グリフ幅で位置を進める
        let width = font.glyph_width(code);
        ts.advance_by_glyph(width, ts.font_size);

        // スペース文字の場合はword_spacingも追加
        if code == 0x20 {
            let tw = ts.word_spacing * (ts.horizontal_scaling / 100.0);
            let translate = Matrix {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e: tw,
                f: 0.0,
            };
            ts.text_matrix = translate.multiply(&ts.text_matrix);
        }
    }

    Ok(())
}

/// TJ配列をエンコーディングに応じてパースしてTjArrayEntryの列を返す。
pub fn parse_tj_entries_for_encoding(
    obj: &lopdf::Object,
    encoding: &FontEncoding,
) -> Vec<TjArrayEntry> {
    extract_tj_array_for_encoding(obj, encoding).1
}
