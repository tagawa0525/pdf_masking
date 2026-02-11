use std::collections::HashMap;

use lopdf::content::Content;

use crate::pdf::content_stream::{Matrix, operand_to_f64};
use crate::pdf::font::{FontEncoding, ParsedFont};

/// fill colorの状態（テキスト描画用）
#[derive(Debug, Clone)]
pub enum FillColor {
    Gray(f64),
    Rgb(f64, f64, f64),
    Cmyk(f64, f64, f64, f64),
}

impl FillColor {
    pub(crate) fn default_black() -> Self {
        FillColor::Gray(0.0)
    }
}

/// テキスト描画コマンド（1回のTj/TJ/'/"呼び出しに対応）
#[derive(Debug, Clone)]
pub struct TextDrawCommand {
    pub char_codes: Vec<u16>,
    pub tj_array: Option<Vec<TjArrayEntry>>,
    pub font_name: String,
    pub font_size: f64,
    pub text_matrix: Matrix,
    pub ctm: Matrix,
    pub fill_color: FillColor,
    pub char_spacing: f64,
    pub word_spacing: f64,
    pub horizontal_scaling: f64,
    pub text_rise: f64,
}

/// TJ配列の要素（文字列または位置調整値）
#[derive(Debug, Clone)]
pub enum TjArrayEntry {
    Text(Vec<u16>),
    Adjustment(f64),
}

/// コンテンツストリーム解析結果
#[derive(Debug)]
pub struct ContentOperations {
    /// BT...ETブロック内から抽出したテキスト描画コマンド
    pub text_commands: Vec<TextDrawCommand>,
    /// BT...ET外のオペレータ（そのまま出力に含める）
    pub non_text_operations: Vec<lopdf::content::Operation>,
}

/// BT...ET内のテキスト状態
pub(crate) struct TextState {
    pub(crate) font_name: String,
    pub(crate) font_size: f64,
    pub(crate) char_spacing: f64,
    pub(crate) word_spacing: f64,
    pub(crate) horizontal_scaling: f64,
    pub(crate) text_rise: f64,
    pub(crate) text_leading: f64,
    pub(crate) text_matrix: Matrix,
    pub(crate) text_line_matrix: Matrix,
}

impl TextState {
    pub(crate) fn new() -> Self {
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

    /// T* オペレータ: 0 -TL Td と等価
    pub(crate) fn apply_t_star(&mut self) {
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
    pub(crate) fn advance_by_glyph(&mut self, glyph_width: f64, font_size: f64) {
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
    pub(crate) fn advance_by_tj_adjustment(&mut self, adjustment: f64, font_size: f64) {
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

    /// テキスト状態オペレータ（Tf, Tm, Td, TD, TL, T*, Tc, Tw, Tz, Ts, Tr）を適用する。
    /// 処理されたら true を返す。
    pub(crate) fn apply_text_state_op(
        &mut self,
        operator: &str,
        operands: &[lopdf::Object],
    ) -> crate::error::Result<bool> {
        match operator {
            "Tf" => {
                if operands.len() == 2 {
                    if let Ok(name_bytes) = operands[0].as_name() {
                        self.font_name = String::from_utf8_lossy(name_bytes).into_owned();
                    }
                    if let Ok(size) = operand_to_f64(&operands[1]) {
                        self.font_size = size;
                    }
                }
                Ok(true)
            }
            "Tm" => {
                if operands.len() == 6 {
                    let vals: Vec<f64> = operands
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
                    self.text_matrix = m.clone();
                    self.text_line_matrix = m;
                }
                Ok(true)
            }
            "Td" => {
                if operands.len() == 2 {
                    let tx = operand_to_f64(&operands[0])?;
                    let ty = operand_to_f64(&operands[1])?;
                    let translate = Matrix {
                        a: 1.0,
                        b: 0.0,
                        c: 0.0,
                        d: 1.0,
                        e: tx,
                        f: ty,
                    };
                    self.text_line_matrix = translate.multiply(&self.text_line_matrix);
                    self.text_matrix = self.text_line_matrix.clone();
                }
                Ok(true)
            }
            "TD" => {
                if operands.len() == 2 {
                    let tx = operand_to_f64(&operands[0])?;
                    let ty = operand_to_f64(&operands[1])?;
                    self.text_leading = -ty;
                    let translate = Matrix {
                        a: 1.0,
                        b: 0.0,
                        c: 0.0,
                        d: 1.0,
                        e: tx,
                        f: ty,
                    };
                    self.text_line_matrix = translate.multiply(&self.text_line_matrix);
                    self.text_matrix = self.text_line_matrix.clone();
                }
                Ok(true)
            }
            "TL" => {
                if operands.len() == 1 {
                    self.text_leading = operand_to_f64(&operands[0])?;
                }
                Ok(true)
            }
            "T*" => {
                self.apply_t_star();
                Ok(true)
            }
            "Tc" => {
                if operands.len() == 1 {
                    self.char_spacing = operand_to_f64(&operands[0])?;
                }
                Ok(true)
            }
            "Tw" => {
                if operands.len() == 1 {
                    self.word_spacing = operand_to_f64(&operands[0])?;
                }
                Ok(true)
            }
            "Tz" => {
                if operands.len() == 1 {
                    self.horizontal_scaling = operand_to_f64(&operands[0])?;
                }
                Ok(true)
            }
            "Ts" => {
                if operands.len() == 1 {
                    self.text_rise = operand_to_f64(&operands[0])?;
                }
                Ok(true)
            }
            "Tr" => Ok(true),
            _ => Ok(false),
        }
    }
}

/// コンテンツストリームを解析し、テキスト描画コマンドと非テキストオペレータに分離する。
///
/// `fonts` を指定すると、フォントエンコーディングに応じた文字コード抽出を行う
/// （IdentityH CIDフォントの2バイトペア解釈など）。
/// `None` の場合は従来の1バイト=1文字コード変換を使用する。
pub fn parse_content_operations(
    content_bytes: &[u8],
    fonts: Option<&HashMap<String, ParsedFont>>,
) -> crate::error::Result<ContentOperations> {
    if content_bytes.is_empty() {
        return Ok(ContentOperations {
            text_commands: Vec::new(),
            non_text_operations: Vec::new(),
        });
    }

    let content = Content::decode(content_bytes)
        .map_err(|e| crate::error::PdfMaskError::content_stream(e.to_string()))?;

    let mut text_commands: Vec<TextDrawCommand> = Vec::new();
    let mut non_text_operations: Vec<lopdf::content::Operation> = Vec::new();

    let mut ctm_stack: Vec<Matrix> = vec![Matrix::identity()];
    let mut fill_color_stack: Vec<FillColor> = vec![FillColor::default_black()];

    let mut in_text = false;
    let mut ts = TextState::new();

    for op in &content.operations {
        match op.operator.as_str() {
            // --- グラフィックス状態（BT内外共通） ---
            "q" | "Q" | "cm" => {
                apply_graphics_state_op(
                    op,
                    &mut ctm_stack,
                    &mut fill_color_stack,
                    in_text,
                    &mut non_text_operations,
                )?;
            }

            // --- Fill color ---
            "rg" | "g" | "k" | "sc" | "scn" => {
                apply_color_op(op, &mut fill_color_stack, in_text, &mut non_text_operations);
            }

            // --- テキストブロック ---
            "BT" => {
                in_text = true;
                ts = TextState::new();
            }
            "ET" => {
                in_text = false;
            }

            // --- テキスト配置オペレータ（BT内のみ有効） ---
            "Td" | "TD" | "Tm" | "T*" | "TL" if in_text => {
                apply_text_positioning_op(op, &mut ts)?;
            }

            // --- テキスト状態オペレータ（BT内のみ有効） ---
            "Tf" | "Tc" | "Tw" | "Tz" | "Ts" | "Tr" if in_text => {
                apply_text_state_op(op, &mut ts)?;
            }

            // --- テキスト描画オペレータ ---
            "Tj" | "TJ" | "'" | "\"" if in_text => {
                apply_text_show_op(
                    op,
                    &mut ts,
                    fonts,
                    &ctm_stack,
                    &fill_color_stack,
                    &mut text_commands,
                );
            }

            // --- BT内のその他のオペレータは無視（色設定は上で処理済み） ---
            _ if in_text => {}

            // --- BT外のオペレータはそのまま保持 ---
            _ => {
                non_text_operations.push(op.clone());
            }
        }
    }

    Ok(ContentOperations {
        text_commands,
        non_text_operations,
    })
}

/// グラフィックス状態オペレータ (q/Q/cm) を適用する。
fn apply_graphics_state_op(
    op: &lopdf::content::Operation,
    ctm_stack: &mut Vec<Matrix>,
    fill_color_stack: &mut Vec<FillColor>,
    in_text: bool,
    non_text_operations: &mut Vec<lopdf::content::Operation>,
) -> crate::error::Result<()> {
    match op.operator.as_str() {
        "q" => {
            let current_ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
            ctm_stack.push(current_ctm);
            let current_fc = fill_color_stack
                .last()
                .cloned()
                .unwrap_or_else(FillColor::default_black);
            fill_color_stack.push(current_fc);
        }
        "Q" => {
            if ctm_stack.len() > 1 {
                ctm_stack.pop();
            }
            if fill_color_stack.len() > 1 {
                fill_color_stack.pop();
            }
        }
        "cm" => {
            if op.operands.len() == 6 {
                let vals: Vec<f64> = op
                    .operands
                    .iter()
                    .map(operand_to_f64)
                    .collect::<Result<Vec<_>, _>>()?;
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
        }
        _ => {}
    }
    if !in_text {
        non_text_operations.push(op.clone());
    }
    Ok(())
}

/// Fill colorオペレータ (rg/g/k/sc/scn) を適用する。
fn apply_color_op(
    op: &lopdf::content::Operation,
    fill_color_stack: &mut [FillColor],
    in_text: bool,
    non_text_operations: &mut Vec<lopdf::content::Operation>,
) {
    match op.operator.as_str() {
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
        }
        "g" => {
            if op.operands.len() == 1
                && let Ok(gray) = operand_to_f64(&op.operands[0])
                && let Some(fc) = fill_color_stack.last_mut()
            {
                *fc = FillColor::Gray(gray);
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
        }
        _ => {}
    }
    if !in_text {
        non_text_operations.push(op.clone());
    }
}

/// テキスト配置オペレータ (Td/TD/Tm/T*/TL) を適用する。
fn apply_text_positioning_op(
    op: &lopdf::content::Operation,
    ts: &mut TextState,
) -> crate::error::Result<()> {
    match op.operator.as_str() {
        "Td" => {
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
        "TD" => {
            // tx ty TD = -ty TL tx ty Td
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
        "Tm" => {
            if op.operands.len() == 6 {
                let vals: Vec<f64> = op
                    .operands
                    .iter()
                    .map(operand_to_f64)
                    .collect::<Result<Vec<_>, _>>()?;
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
        "T*" => {
            // T* = 0 -TL Td
            ts.apply_t_star();
        }
        "TL" => {
            if op.operands.len() == 1 {
                ts.text_leading = operand_to_f64(&op.operands[0])?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// テキスト状態オペレータ (Tf/Tc/Tw/Tz/Ts/Tr) を適用する。
fn apply_text_state_op(
    op: &lopdf::content::Operation,
    ts: &mut TextState,
) -> crate::error::Result<()> {
    match op.operator.as_str() {
        "Tf" => {
            if op.operands.len() == 2 {
                if let Ok(name_bytes) = op.operands[0].as_name() {
                    ts.font_name = String::from_utf8_lossy(name_bytes).into_owned();
                }
                if let Ok(size) = operand_to_f64(&op.operands[1]) {
                    ts.font_size = size;
                }
            }
        }
        "Tc" => {
            if op.operands.len() == 1 {
                ts.char_spacing = operand_to_f64(&op.operands[0])?;
            }
        }
        "Tw" => {
            if op.operands.len() == 1 {
                ts.word_spacing = operand_to_f64(&op.operands[0])?;
            }
        }
        "Tz" => {
            if op.operands.len() == 1 {
                ts.horizontal_scaling = operand_to_f64(&op.operands[0])?;
            }
        }
        "Ts" => {
            if op.operands.len() == 1 {
                ts.text_rise = operand_to_f64(&op.operands[0])?;
            }
        }
        "Tr" => {
            // rendering mode: 追跡するが現時点では使わない
        }
        _ => {}
    }
    Ok(())
}

/// テキスト描画オペレータ (Tj/TJ/'/") を適用する。
fn apply_text_show_op(
    op: &lopdf::content::Operation,
    ts: &mut TextState,
    fonts: Option<&HashMap<String, ParsedFont>>,
    ctm_stack: &[Matrix],
    fill_color_stack: &[FillColor],
    text_commands: &mut Vec<TextDrawCommand>,
) {
    match op.operator.as_str() {
        "Tj" => {
            if let Some(operand) = op.operands.first() {
                let encoding = lookup_encoding(&ts.font_name, fonts);
                let codes = extract_char_codes_for_encoding(operand, encoding);
                let cmd = build_text_command(ts, codes, None, ctm_stack, fill_color_stack);
                text_commands.push(cmd);
            }
        }
        "TJ" => {
            if let Some(operand) = op.operands.first() {
                let encoding = lookup_encoding(&ts.font_name, fonts);
                let (codes, tj_array) = extract_tj_array_for_encoding(operand, encoding);
                let cmd =
                    build_text_command(ts, codes, Some(tj_array), ctm_stack, fill_color_stack);
                text_commands.push(cmd);
            }
        }
        "'" => {
            // ' = T* string Tj
            ts.apply_t_star();
            if let Some(operand) = op.operands.first() {
                let encoding = lookup_encoding(&ts.font_name, fonts);
                let codes = extract_char_codes_for_encoding(operand, encoding);
                let cmd = build_text_command(ts, codes, None, ctm_stack, fill_color_stack);
                text_commands.push(cmd);
            }
        }
        "\"" => {
            // aw ac string " = aw Tw ac Tc T* string Tj
            if op.operands.len() == 3 {
                if let Ok(aw) = operand_to_f64(&op.operands[0]) {
                    ts.word_spacing = aw;
                }
                if let Ok(ac) = operand_to_f64(&op.operands[1]) {
                    ts.char_spacing = ac;
                }
                ts.apply_t_star();
                let encoding = lookup_encoding(&ts.font_name, fonts);
                let codes = extract_char_codes_for_encoding(&op.operands[2], encoding);
                let cmd = build_text_command(ts, codes, None, ctm_stack, fill_color_stack);
                text_commands.push(cmd);
            }
        }
        _ => {}
    }
}

fn build_text_command(
    ts: &TextState,
    char_codes: Vec<u16>,
    tj_array: Option<Vec<TjArrayEntry>>,
    ctm_stack: &[Matrix],
    fill_color_stack: &[FillColor],
) -> TextDrawCommand {
    TextDrawCommand {
        char_codes,
        tj_array,
        font_name: ts.font_name.clone(),
        font_size: ts.font_size,
        text_matrix: ts.text_matrix.clone(),
        ctm: ctm_stack.last().cloned().unwrap_or_else(Matrix::identity),
        fill_color: fill_color_stack
            .last()
            .cloned()
            .unwrap_or_else(FillColor::default_black),
        char_spacing: ts.char_spacing,
        word_spacing: ts.word_spacing,
        horizontal_scaling: ts.horizontal_scaling,
        text_rise: ts.text_rise,
    }
}

/// フォント名からエンコーディングを取得する。フォントマップが None または
/// フォントが見つからない場合は WinAnsi をデフォルトとする。
pub(crate) fn lookup_encoding<'a>(
    font_name: &str,
    fonts: Option<&'a HashMap<String, ParsedFont>>,
) -> &'a FontEncoding {
    static DEFAULT: std::sync::LazyLock<FontEncoding> =
        std::sync::LazyLock::new(|| FontEncoding::WinAnsi {
            differences: HashMap::new(),
        });
    fonts
        .and_then(|f| f.get(font_name))
        .map(|f| f.encoding())
        .unwrap_or(&DEFAULT)
}

/// エンコーディングに応じて lopdf::Object からバイト列を文字コード列に変換する。
pub fn extract_char_codes_for_encoding(obj: &lopdf::Object, encoding: &FontEncoding) -> Vec<u16> {
    match obj {
        lopdf::Object::String(bytes, _) => encoding.bytes_to_char_codes(bytes),
        _ => Vec::new(),
    }
}

/// TJ配列をエンコーディングに応じて解析し、全文字コードとTjArrayEntryの列を返す。
pub(crate) fn extract_tj_array_for_encoding(
    obj: &lopdf::Object,
    encoding: &FontEncoding,
) -> (Vec<u16>, Vec<TjArrayEntry>) {
    let mut all_codes = Vec::new();
    let mut entries = Vec::new();

    if let lopdf::Object::Array(arr) = obj {
        for item in arr {
            match item {
                lopdf::Object::String(bytes, _) => {
                    let codes = encoding.bytes_to_char_codes(bytes);
                    all_codes.extend_from_slice(&codes);
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

    (all_codes, entries)
}
