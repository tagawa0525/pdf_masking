use std::collections::HashMap;

use lopdf::content::Content;

use crate::error::{PdfMaskError, Result};
use crate::pdf::content_stream::{Matrix, operand_to_f64};
use crate::pdf::font::ParsedFont;
use crate::pdf::glyph_to_path::{GlyphPathParams, glyph_to_pdf_path};
use crate::pdf::text_state::{FillColor, TjArrayEntry};

/// BT...ETブロックをベクターパスに変換したコンテンツストリームを返す。
///
/// フォントが見つからない場合はErrを返し、呼び出し元でpdfiumフォールバックに切り替える。
pub fn convert_text_to_outlines(
    content_bytes: &[u8],
    fonts: &HashMap<String, ParsedFont>,
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
    let mut fill_color_stack: Vec<FillColor> = vec![FillColor::Gray(0.0)];
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
                    .unwrap_or(FillColor::Gray(0.0));
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
            "rg" => {
                if op.operands.len() == 3
                    && let (Ok(r), Ok(g), Ok(b)) = (
                        operand_to_f64(&op.operands[0]),
                        operand_to_f64(&op.operands[1]),
                        operand_to_f64(&op.operands[2]),
                    )
                    && let Some(fc) = fill_color_stack.last_mut()
                {
                    *fc = FillColor::Rgb(r, g, b);
                }
                if !in_text {
                    output_ops.push(op.clone());
                }
            }
            "g" => {
                if op.operands.len() == 1
                    && let Ok(gray) = operand_to_f64(&op.operands[0])
                    && let Some(fc) = fill_color_stack.last_mut()
                {
                    *fc = FillColor::Gray(gray);
                }
                if !in_text {
                    output_ops.push(op.clone());
                }
            }
            "k" => {
                if op.operands.len() == 4
                    && let (Ok(c), Ok(m), Ok(y), Ok(k)) = (
                        operand_to_f64(&op.operands[0]),
                        operand_to_f64(&op.operands[1]),
                        operand_to_f64(&op.operands[2]),
                        operand_to_f64(&op.operands[3]),
                    )
                    && let Some(fc) = fill_color_stack.last_mut()
                {
                    *fc = FillColor::Cmyk(c, m, y, k);
                }
                if !in_text {
                    output_ops.push(op.clone());
                }
            }
            "sc" | "scn" => {
                if let Some(fc) = fill_color_stack.last_mut() {
                    match op.operands.len() {
                        1 => {
                            if let Ok(gray) = operand_to_f64(&op.operands[0]) {
                                *fc = FillColor::Gray(gray);
                            }
                        }
                        3 => {
                            if let (Ok(r), Ok(g), Ok(b)) = (
                                operand_to_f64(&op.operands[0]),
                                operand_to_f64(&op.operands[1]),
                                operand_to_f64(&op.operands[2]),
                            ) {
                                *fc = FillColor::Rgb(r, g, b);
                            }
                        }
                        4 => {
                            if let (Ok(c), Ok(m), Ok(y), Ok(k)) = (
                                operand_to_f64(&op.operands[0]),
                                operand_to_f64(&op.operands[1]),
                                operand_to_f64(&op.operands[2]),
                                operand_to_f64(&op.operands[3]),
                            ) {
                                *fc = FillColor::Cmyk(c, m, y, k);
                            }
                        }
                        _ => {}
                    }
                }
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
            "Tf" if in_text => {
                if op.operands.len() == 2 {
                    if let Ok(name_bytes) = op.operands[0].as_name() {
                        ts.font_name = String::from_utf8_lossy(name_bytes).into_owned();
                    }
                    if let Ok(size) = operand_to_f64(&op.operands[1]) {
                        ts.font_size = size;
                    }
                }
            }
            "Tm" if in_text => {
                if op.operands.len() == 6 {
                    let vals: Vec<f64> = op
                        .operands
                        .iter()
                        .map(operand_to_f64)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    let m = Matrix {
                        a: vals[0],
                        b: vals[1],
                        c: vals[2],
                        d: vals[3],
                        e: vals[4],
                        f: vals[5],
                    };
                    ts.text_matrix = m.clone();
                    ts.text_line_matrix = m;
                }
            }
            "Td" if in_text => {
                if op.operands.len() == 2 {
                    let tx = operand_to_f64(&op.operands[0])?;
                    let ty = operand_to_f64(&op.operands[1])?;
                    let translate = Matrix {
                        a: 1.0,
                        b: 0.0,
                        c: 0.0,
                        d: 1.0,
                        e: tx,
                        f: ty,
                    };
                    ts.text_line_matrix = translate.multiply(&ts.text_line_matrix);
                    ts.text_matrix = ts.text_line_matrix.clone();
                }
            }
            "TD" if in_text => {
                if op.operands.len() == 2 {
                    let tx = operand_to_f64(&op.operands[0])?;
                    let ty = operand_to_f64(&op.operands[1])?;
                    ts.text_leading = -ty;
                    let translate = Matrix {
                        a: 1.0,
                        b: 0.0,
                        c: 0.0,
                        d: 1.0,
                        e: tx,
                        f: ty,
                    };
                    ts.text_line_matrix = translate.multiply(&ts.text_line_matrix);
                    ts.text_matrix = ts.text_line_matrix.clone();
                }
            }
            "TL" if in_text => {
                if op.operands.len() == 1 {
                    ts.text_leading = operand_to_f64(&op.operands[0])?;
                }
            }
            "T*" if in_text => {
                ts.apply_t_star();
            }
            "Tc" if in_text => {
                if op.operands.len() == 1 {
                    ts.char_spacing = operand_to_f64(&op.operands[0])?;
                }
            }
            "Tw" if in_text => {
                if op.operands.len() == 1 {
                    ts.word_spacing = operand_to_f64(&op.operands[0])?;
                }
            }
            "Tz" if in_text => {
                if op.operands.len() == 1 {
                    ts.horizontal_scaling = operand_to_f64(&op.operands[0])?;
                }
            }
            "Ts" if in_text => {
                if op.operands.len() == 1 {
                    ts.text_rise = operand_to_f64(&op.operands[0])?;
                }
            }
            "Tr" if in_text => {}

            // --- テキスト描画オペレータ ---
            "Tj" if in_text => {
                if let Some(operand) = op.operands.first() {
                    let codes = extract_char_codes(operand);
                    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                    let fill_color = fill_color_stack
                        .last()
                        .cloned()
                        .unwrap_or(FillColor::Gray(0.0));
                    render_text_codes(
                        &codes,
                        &mut ts,
                        &ctm,
                        &fill_color,
                        fonts,
                        &mut text_path_buf,
                    )?;
                }
            }
            "TJ" if in_text => {
                if let Some(operand) = op.operands.first() {
                    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                    let fill_color = fill_color_stack
                        .last()
                        .cloned()
                        .unwrap_or(FillColor::Gray(0.0));
                    render_tj_array(
                        operand,
                        &mut ts,
                        &ctm,
                        &fill_color,
                        fonts,
                        &mut text_path_buf,
                    )?;
                }
            }
            "'" if in_text => {
                ts.apply_t_star();
                if let Some(operand) = op.operands.first() {
                    let codes = extract_char_codes(operand);
                    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                    let fill_color = fill_color_stack
                        .last()
                        .cloned()
                        .unwrap_or(FillColor::Gray(0.0));
                    render_text_codes(
                        &codes,
                        &mut ts,
                        &ctm,
                        &fill_color,
                        fonts,
                        &mut text_path_buf,
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
                    let codes = extract_char_codes(&op.operands[2]);
                    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                    let fill_color = fill_color_stack
                        .last()
                        .cloned()
                        .unwrap_or(FillColor::Gray(0.0));
                    render_text_codes(
                        &codes,
                        &mut ts,
                        &ctm,
                        &fill_color,
                        fonts,
                        &mut text_path_buf,
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

/// テキスト状態（BT内で追跡）
struct TextState {
    font_name: String,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scaling: f64,
    text_rise: f64,
    text_leading: f64,
    text_matrix: Matrix,
    text_line_matrix: Matrix,
}

impl TextState {
    fn new() -> Self {
        TextState {
            font_name: String::new(),
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 100.0,
            text_rise: 0.0,
            text_leading: 0.0,
            text_matrix: Matrix::identity(),
            text_line_matrix: Matrix::identity(),
        }
    }

    fn apply_t_star(&mut self) {
        let translate = Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: -self.text_leading,
        };
        self.text_line_matrix = translate.multiply(&self.text_line_matrix);
        self.text_matrix = self.text_line_matrix.clone();
    }

    /// テキスト位置を1グリフ分進める（PDF §9.4.4）
    fn advance_by_glyph(&mut self, glyph_width: f64, font_size: f64) {
        // tx = ((w0 - Tj/1000) * Tfs + Tc + Tw) * Th
        // ここでは Tj=0, Tw はスペース文字の場合のみ適用（呼び出し側で処理）
        let tx = (glyph_width / 1000.0) * font_size + self.char_spacing;
        let tx = tx * (self.horizontal_scaling / 100.0);
        let translate = Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: 0.0,
        };
        self.text_matrix = translate.multiply(&self.text_matrix);
    }

    /// TJ配列の位置調整値を適用
    fn advance_by_tj_adjustment(&mut self, adjustment: f64, font_size: f64) {
        // tx = -(adjustment / 1000) * fontSize * Th
        let tx = -(adjustment / 1000.0) * font_size * (self.horizontal_scaling / 100.0);
        let translate = Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: 0.0,
        };
        self.text_matrix = translate.multiply(&self.text_matrix);
    }
}

/// 文字コード列をグリフパスに変換して出力バッファに追加
fn render_text_codes(
    codes: &[u16],
    ts: &mut TextState,
    ctm: &Matrix,
    fill_color: &FillColor,
    fonts: &HashMap<String, ParsedFont>,
    output: &mut Vec<u8>,
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

/// TJ配列を処理して出力バッファに追加
fn render_tj_array(
    obj: &lopdf::Object,
    ts: &mut TextState,
    ctm: &Matrix,
    fill_color: &FillColor,
    fonts: &HashMap<String, ParsedFont>,
    output: &mut Vec<u8>,
) -> Result<()> {
    let entries = parse_tj_entries(obj);

    for entry in &entries {
        match entry {
            TjArrayEntry::Text(codes) => {
                render_text_codes(codes, ts, ctm, fill_color, fonts, output)?;
            }
            TjArrayEntry::Adjustment(val) => {
                ts.advance_by_tj_adjustment(*val, ts.font_size);
            }
        }
    }

    Ok(())
}

/// TJ配列をパースしてTjArrayEntryの列を返す
fn parse_tj_entries(obj: &lopdf::Object) -> Vec<TjArrayEntry> {
    let mut entries = Vec::new();
    if let lopdf::Object::Array(arr) = obj {
        for item in arr {
            match item {
                lopdf::Object::String(bytes, _) => {
                    let codes: Vec<u16> = bytes.iter().map(|&b| b as u16).collect();
                    entries.push(TjArrayEntry::Text(codes));
                }
                lopdf::Object::Integer(n) => {
                    entries.push(TjArrayEntry::Adjustment(*n as f64));
                }
                lopdf::Object::Real(r) => {
                    entries.push(TjArrayEntry::Adjustment(*r as f64));
                }
                _ => {}
            }
        }
    }
    entries
}

/// lopdf::Object からバイト列を取得し、u16の文字コード列に変換する。
fn extract_char_codes(obj: &lopdf::Object) -> Vec<u16> {
    match obj {
        lopdf::Object::String(bytes, _) => bytes.iter().map(|&b| b as u16).collect(),
        _ => Vec::new(),
    }
}

/// エンコーディングに応じて lopdf::Object からバイト列を文字コード列に変換する。
///
/// - IdentityH: 2バイトBEペアとして解釈（CIDフォント）
/// - WinAnsi: 1バイト→u16変換（従来動作）
pub fn extract_char_codes_for_encoding(
    obj: &lopdf::Object,
    encoding: &crate::pdf::font::FontEncoding,
) -> Vec<u16> {
    // TODO: エンコーディング分岐は GREEN フェーズで実装
    let _ = encoding;
    extract_char_codes(obj)
}

/// TJ配列をエンコーディングに応じてパースしてTjArrayEntryの列を返す。
pub fn parse_tj_entries_for_encoding(
    obj: &lopdf::Object,
    encoding: &crate::pdf::font::FontEncoding,
) -> Vec<TjArrayEntry> {
    // TODO: エンコーディング分岐は GREEN フェーズで実装
    let _ = encoding;
    parse_tj_entries(obj)
}
