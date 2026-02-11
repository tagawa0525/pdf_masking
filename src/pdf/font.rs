use std::collections::HashMap;
use std::sync::LazyLock;

use lopdf::{Document, Object, ObjectId};
use tracing::{debug, warn};
use ttf_parser::GlyphId;

use crate::error::PdfMaskError;

/// グリフアウトラインのパス操作
#[derive(Debug, Clone)]
pub enum PathOp {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    QuadTo(f64, f64, f64, f64),
    /// 3次ベジェ曲線（CFF/CFF2フォント用）: (x1, y1, x2, y2, x, y)
    CubicTo(f64, f64, f64, f64, f64, f64),
    Close,
}

/// フォントエンコーディング
#[derive(Debug, Clone)]
pub enum FontEncoding {
    WinAnsi { differences: HashMap<u8, String> },
    IdentityH,
}

impl FontEncoding {
    /// バイト列をエンコーディングに応じて文字コード列に変換する。
    pub fn bytes_to_char_codes(&self, bytes: &[u8]) -> Vec<u16> {
        match self {
            FontEncoding::IdentityH => {
                if !bytes.len().is_multiple_of(2) {
                    warn!(
                        "IdentityH encoded string has odd length ({} bytes); \
                         trailing byte will be ignored",
                        bytes.len()
                    );
                }
                bytes
                    .chunks_exact(2)
                    .map(|pair| ((pair[0] as u16) << 8) | pair[1] as u16)
                    .collect()
            }
            FontEncoding::WinAnsi { .. } => bytes.iter().map(|&b| b as u16).collect(),
        }
    }
}

/// 解析済みフォント
pub struct ParsedFont {
    font_data: Vec<u8>,
    face_index: u32,
    encoding: FontEncoding,
    widths: HashMap<u16, f64>,
    default_width: f64,
    units_per_em: u16,
}

impl ParsedFont {
    pub fn encoding(&self) -> &FontEncoding {
        &self.encoding
    }

    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// 文字コード→グリフIDを解決
    pub fn char_code_to_glyph_id(&self, code: u16) -> Option<GlyphId> {
        let face = ttf_parser::Face::parse(&self.font_data, self.face_index).ok()?;
        match &self.encoding {
            FontEncoding::WinAnsi { differences } => {
                // Differences配列: グリフ名→cmapでUnicode→GID
                if let Some(glyph_name) = differences.get(&(code as u8))
                    && let Some(unicode) = glyph_name_to_unicode(glyph_name)
                {
                    return face.glyph_index(unicode);
                }
                // WinAnsi: char_code → Unicode → cmap lookup
                let unicode_char = win_ansi_to_unicode(code as u8)?;
                face.glyph_index(unicode_char)
            }
            FontEncoding::IdentityH => {
                // Identity-H + CIDToGIDMap=Identity: CID = GID
                Some(GlyphId(code))
            }
        }
    }

    /// 文字コードの幅を返す（1/1000テキスト空間単位）
    pub fn glyph_width(&self, code: u16) -> f64 {
        self.widths
            .get(&code)
            .copied()
            .unwrap_or(self.default_width)
    }

    /// グリフIDからアウトラインを取得
    pub fn glyph_outline(&self, glyph_id: GlyphId) -> Option<Vec<PathOp>> {
        let face = ttf_parser::Face::parse(&self.font_data, self.face_index).ok()?;
        let mut builder = OutlineBuilder::new();
        face.outline_glyph(glyph_id, &mut builder)?;
        Some(builder.ops)
    }
}

/// ttf-parserのOutlineBuilderコールバック
struct OutlineBuilder {
    ops: Vec<PathOp>,
}

impl OutlineBuilder {
    fn new() -> Self {
        OutlineBuilder { ops: Vec::new() }
    }
}

impl ttf_parser::OutlineBuilder for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.ops.push(PathOp::MoveTo(x as f64, y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.ops.push(PathOp::LineTo(x as f64, y as f64));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.ops
            .push(PathOp::QuadTo(x1 as f64, y1 as f64, x as f64, y as f64));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.ops.push(PathOp::CubicTo(
            x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64, y as f64,
        ));
    }

    fn close(&mut self) {
        self.ops.push(PathOp::Close);
    }
}

/// システムフォントデータベース（一度だけ初期化）
static SYSTEM_FONT_DB: LazyLock<fontdb::Database> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db
});

/// PostScript フォント名からファミリ名とスタイルを推定して fontdb::Query を構築
fn parse_ps_name_to_query(ps_name: &str) -> (String, fontdb::Weight, bool) {
    let mut family = ps_name.to_string();
    let mut weight = fontdb::Weight::NORMAL;
    let mut is_italic = false;

    // MT (Mac) / PS (PostScript) サフィックスを先に除去
    // "-BoldMT" → "-Bold", "-ItalicMT" → "-Italic" にしてからスタイル解析
    family = family.trim_end_matches("MT").to_string();

    // サフィックスの解析 (-Bold, -Italic, -BoldItalic, -Oblique)
    if family.ends_with("-BoldItalic") {
        family = family.strip_suffix("-BoldItalic").unwrap().to_string();
        weight = fontdb::Weight::BOLD;
        is_italic = true;
    } else if family.ends_with("-Bold") {
        family = family.strip_suffix("-Bold").unwrap().to_string();
        weight = fontdb::Weight::BOLD;
    } else if family.ends_with("-Italic") {
        family = family.strip_suffix("-Italic").unwrap().to_string();
        is_italic = true;
    } else if family.ends_with("-Oblique") {
        family = family.strip_suffix("-Oblique").unwrap().to_string();
        is_italic = true;
    }

    // 残りの PS サフィックスを除去
    family = family.trim_end_matches("PS").to_string();

    // CamelCase をスペース区切りに展開: TimesNewRoman → Times New Roman
    let mut result = String::new();
    for (i, ch) in family.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            result.push(' ');
        }
        result.push(ch);
    }

    (result, weight, is_italic)
}

/// 非埋め込みフォントをシステムフォントから解決
/// Returns: (font_data, face_index)
fn resolve_system_font(base_font_name: &str) -> crate::error::Result<(Vec<u8>, u32)> {
    let db = &*SYSTEM_FONT_DB;

    // Helper to load font data from face info
    let load_font_data = |face_info: &fontdb::FaceInfo| -> Option<(Vec<u8>, u32)> {
        let font_data = match &face_info.source {
            fontdb::Source::File(path) => std::fs::read(path).ok()?,
            fontdb::Source::SharedFile(path, _) => std::fs::read(path).ok()?,
            fontdb::Source::Binary(_) => {
                // Memory-resident fonts (e.g., embedded in the binary)
                return None;
            }
        };
        Some((font_data, face_info.index))
    };

    // 1. PostScript 名で完全一致検索
    for face_info in db.faces() {
        if face_info.post_script_name == base_font_name
            && let Some((font_data, face_index)) = load_font_data(face_info)
        {
            return Ok((font_data, face_index));
        }
    }

    // 2. ファミリ名とスタイルから検索
    let (family, weight, is_italic) = parse_ps_name_to_query(base_font_name);

    let query = fontdb::Query {
        families: &[fontdb::Family::Name(&family)],
        weight,
        stretch: fontdb::Stretch::Normal,
        style: if is_italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
    };

    if let Some(id) = db.query(&query)
        && let Some(face_info) = db.face(id)
        && let Some((font_data, face_index)) = load_font_data(face_info)
    {
        return Ok((font_data, face_index));
    }

    // 3. Linux での代替フォント検索
    let fallback_family = match family.as_str() {
        "Times New Roman" => "Liberation Serif",
        "Arial" | "Helvetica" => "Liberation Sans",
        "Courier" => "Liberation Mono",
        _ => {
            return Err(PdfMaskError::pdf_read(format!(
                "system font not found: {}",
                base_font_name
            )));
        }
    };

    let fallback_query = fontdb::Query {
        families: &[fontdb::Family::Name(fallback_family)],
        weight,
        stretch: fontdb::Stretch::Normal,
        style: if is_italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
    };

    if let Some(id) = db.query(&fallback_query)
        && let Some(face_info) = db.face(id)
        && let Some((font_data, face_index)) = load_font_data(face_info)
    {
        return Ok((font_data, face_index));
    }

    Err(PdfMaskError::pdf_read(format!(
        "system font not found: {}",
        base_font_name
    )))
}

/// フォント辞書から BaseFont を取得してシステムフォント解決
fn resolve_system_font_from_dict(
    font_dict: &lopdf::Dictionary,
) -> crate::error::Result<(Vec<u8>, u32)> {
    let base_font = font_dict
        .get(b"BaseFont")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .ok_or_else(|| PdfMaskError::pdf_read("no BaseFont in font dictionary"))?;

    resolve_system_font(&base_font)
}

/// ページのフォントリソースを解析し、ParsedFontのマップを返す。
/// 埋込フォントデータが無いフォントはシステムフォントから解決を試みる。
pub fn parse_page_fonts(
    doc: &Document,
    page_num: u32,
) -> crate::error::Result<HashMap<String, ParsedFont>> {
    if page_num == 0 {
        return Err(PdfMaskError::pdf_read("page_num must be >= 1 (1-based)"));
    }

    let page_id = doc
        .page_iter()
        .nth((page_num - 1) as usize)
        .ok_or_else(|| PdfMaskError::pdf_read(format!("page {} not found", page_num)))?;

    let font_dict = get_font_dict(doc, page_id)?;
    let mut fonts = HashMap::new();

    for (name_bytes, font_ref) in &font_dict {
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        match parse_single_font(doc, font_ref) {
            Ok(parsed) => {
                fonts.insert(name, parsed);
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("FontFile2")
                    || msg.contains("FontDescriptor")
                    || msg.contains("system font not found")
                    || msg.contains("unsupported font subtype")
                {
                    // 埋込データなし、システムフォント未検出、非対応形式はスキップ
                    // 呼び出し元が不足フォントを処理する（例: pdfium フォールバック）
                    continue;
                }
                return Err(e);
            }
        }
    }

    debug!(page = page_num, count = fonts.len(), "parsed page fonts");
    Ok(fonts)
}

/// ページのフォントリソース辞書を取得
fn get_font_dict(
    doc: &Document,
    page_id: ObjectId,
) -> crate::error::Result<HashMap<Vec<u8>, Object>> {
    let page_obj = doc
        .get_object(page_id)
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    let page_dict = page_obj
        .as_dict()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    // Resources を取得（ページ直接 or 親からの継承）
    let resources = get_resources(doc, page_dict)?;
    let resources_dict = resources
        .as_dict()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    // /Font 辞書を取得
    let font_obj = match resources_dict.get(b"Font") {
        Ok(obj) => {
            doc.dereference(obj)
                .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
                .1
        }
        Err(_) => return Ok(HashMap::new()),
    };

    let font_dict = font_obj
        .as_dict()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    Ok(font_dict
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

/// Resources辞書を取得（ページ直接またはPages親から継承）
fn get_resources<'a>(
    doc: &'a Document,
    page_dict: &'a lopdf::Dictionary,
) -> crate::error::Result<&'a Object> {
    if let Ok(res) = page_dict.get(b"Resources") {
        return match res {
            Object::Reference(id) => doc
                .get_object(*id)
                .map_err(|e| PdfMaskError::pdf_read(e.to_string())),
            _ => Ok(res),
        };
    }

    // 親Pagesノードから継承
    if let Ok(parent_ref) = page_dict.get(b"Parent")
        && let Object::Reference(parent_id) = parent_ref
    {
        let parent = doc
            .get_object(*parent_id)
            .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;
        if let Ok(parent_dict) = parent.as_dict() {
            return get_resources(doc, parent_dict);
        }
    }

    Err(PdfMaskError::pdf_read("no Resources found"))
}

/// 単一フォント辞書からParsedFontを構築
fn parse_single_font(doc: &Document, font_ref: &Object) -> crate::error::Result<ParsedFont> {
    let font_obj = match font_ref {
        Object::Reference(id) => doc
            .get_object(*id)
            .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?,
        other => other,
    };

    let font_dict = font_obj
        .as_dict()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    let subtype = font_dict
        .get(b"Subtype")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();

    match subtype.as_str() {
        "TrueType" | "Type1" | "MMType1" => parse_truetype_font(doc, font_dict),
        "Type0" => parse_type0_font(doc, font_dict),
        _ => Err(PdfMaskError::pdf_read(format!(
            "unsupported font subtype: {}",
            subtype
        ))),
    }
}

/// フォントfaceからグリフ幅を導出（Widths省略時用）
/// ttf_parserのhorizontal advanceを1000単位に正規化して返す
fn derive_widths_from_font_face(
    face: &ttf_parser::Face,
    encoding: &FontEncoding,
    units_per_em: u16,
) -> HashMap<u16, f64> {
    let mut widths = HashMap::new();
    let scale = 1000.0 / units_per_em as f64;

    // エンコーディングに応じて文字コード範囲を決定
    let char_codes: Vec<u16> = match encoding {
        FontEncoding::WinAnsi { .. } => (0x00..=0xFF).collect(),
        FontEncoding::IdentityH => {
            // IdentityHの場合は全グリフを対象とする（0x0000-0xFFFF）
            // 実際にはCIDフォントでは使わないが、念のため実装
            (0x0000..=0xFFFF).collect()
        }
    };

    for code in char_codes {
        // 文字コード→グリフID解決（WinAnsi/IdentityH両対応）
        let glyph_id = match encoding {
            FontEncoding::WinAnsi { differences } => {
                // Differences配列: グリフ名→Unicode→GID
                if let Some(glyph_name) = differences.get(&(code as u8))
                    && let Some(unicode) = glyph_name_to_unicode(glyph_name)
                {
                    face.glyph_index(unicode)
                } else {
                    // WinAnsi: char_code → Unicode → cmap lookup
                    win_ansi_to_unicode(code as u8).and_then(|ch| face.glyph_index(ch))
                }
            }
            FontEncoding::IdentityH => Some(GlyphId(code)),
        };

        // グリフIDからhorizontal advanceを取得して1000単位に正規化
        if let Some(gid) = glyph_id
            && let Some(advance) = face.glyph_hor_advance(gid)
        {
            let width = advance as f64 * scale;
            widths.insert(code, width);
        }
    }

    widths
}

/// TrueTypeフォントの解析
fn parse_truetype_font(
    doc: &Document,
    font_dict: &lopdf::Dictionary,
) -> crate::error::Result<ParsedFont> {
    // 埋め込みフォントデータが無ければシステムフォント解決
    let (font_data, face_index) = extract_font_file2(doc, font_dict)
        .map(|data| (data, 0u32))
        .or_else(|_| {
            debug!("embedded font data not found, trying system font resolution");
            resolve_system_font_from_dict(font_dict)
        })?;

    let encoding = parse_encoding(doc, font_dict)?;
    let mut widths = parse_truetype_widths(doc, font_dict)?;

    let face = ttf_parser::Face::parse(&font_data, face_index)
        .map_err(|e| PdfMaskError::pdf_read(format!("failed to parse TrueType: {}", e)))?;
    let units_per_em = face.units_per_em();

    // Widths が省略されている場合（Type1標準14フォント等）、システムフォントから導出
    if widths.is_empty() {
        widths = derive_widths_from_font_face(&face, &encoding, units_per_em);
    }

    Ok(ParsedFont {
        font_data,
        face_index,
        encoding,
        widths,
        default_width: 1000.0,
        units_per_em,
    })
}

/// Type0 (CIDFont) フォントの解析
fn parse_type0_font(
    doc: &Document,
    font_dict: &lopdf::Dictionary,
) -> crate::error::Result<ParsedFont> {
    // DescendantFonts 配列を取得
    let descendants = font_dict
        .get(b"DescendantFonts")
        .map_err(|_| PdfMaskError::pdf_read("Type0 font missing DescendantFonts"))?;
    let descendants = doc
        .dereference(descendants)
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
        .1;
    let desc_array = descendants
        .as_array()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    if desc_array.is_empty() {
        return Err(PdfMaskError::pdf_read("DescendantFonts array is empty"));
    }

    let cid_font_obj = match &desc_array[0] {
        Object::Reference(id) => doc
            .get_object(*id)
            .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?,
        other => other,
    };

    let cid_font_dict = cid_font_obj
        .as_dict()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    // CIDToGIDMapの検証: Identity以外は未対応
    if let Ok(cid_to_gid) = cid_font_dict.get(b"CIDToGIDMap") {
        let cid_to_gid = doc
            .dereference(cid_to_gid)
            .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
            .1;
        match cid_to_gid {
            Object::Name(name) if name == b"Identity" => {
                // OK: CID = GID
            }
            Object::Stream(_) => {
                return Err(PdfMaskError::pdf_read(
                    "CIDToGIDMap stream not supported (only Identity)",
                ));
            }
            _ => {}
        }
    }

    // 埋め込みフォントデータが無ければシステムフォント解決
    let (font_data, face_index) = extract_font_file2(doc, cid_font_dict)
        .map(|data| (data, 0u32))
        .or_else(|_| {
            debug!("embedded CID font data not found, trying system font resolution");
            resolve_system_font_from_dict(cid_font_dict)
        })?;

    let widths = parse_cid_widths(doc, cid_font_dict)?;
    let default_width = cid_font_dict
        .get(b"DW")
        .ok()
        .and_then(|o| match o {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(*r as f64),
            _ => None,
        })
        .unwrap_or(1000.0);

    let face = ttf_parser::Face::parse(&font_data, face_index)
        .map_err(|e| PdfMaskError::pdf_read(format!("failed to parse CID TrueType: {}", e)))?;
    let units_per_em = face.units_per_em();

    Ok(ParsedFont {
        font_data,
        face_index,
        encoding: FontEncoding::IdentityH,
        widths,
        default_width,
        units_per_em,
    })
}

/// FontDescriptorからFontFile2ストリームを取得・解凍
fn extract_font_file2(
    doc: &Document,
    font_dict: &lopdf::Dictionary,
) -> crate::error::Result<Vec<u8>> {
    let descriptor_obj = font_dict
        .get(b"FontDescriptor")
        .map_err(|_| PdfMaskError::pdf_read("no FontDescriptor"))?;
    let descriptor_obj = doc
        .dereference(descriptor_obj)
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
        .1;
    let descriptor = descriptor_obj
        .as_dict()
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    let font_file2_ref = descriptor
        .get(b"FontFile2")
        .map_err(|_| PdfMaskError::pdf_read("no FontFile2 in FontDescriptor"))?;

    let font_file2_id = match font_file2_ref {
        Object::Reference(id) => *id,
        _ => return Err(PdfMaskError::pdf_read("FontFile2 is not a reference")),
    };

    let stream_obj = doc
        .get_object(font_file2_id)
        .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;

    match stream_obj {
        Object::Stream(stream) => {
            let mut stream = stream.clone();
            stream.decompress().map_err(|e| {
                PdfMaskError::pdf_read(format!("FontFile2 decompress failed: {}", e))
            })?;
            Ok(stream.content)
        }
        _ => Err(PdfMaskError::pdf_read("FontFile2 is not a stream")),
    }
}

/// TrueTypeフォントの/Widths配列を解析
fn parse_truetype_widths(
    doc: &Document,
    font_dict: &lopdf::Dictionary,
) -> crate::error::Result<HashMap<u16, f64>> {
    let mut result = HashMap::new();

    let first_char = match font_dict.get(b"FirstChar").ok() {
        None => 0u16,
        Some(Object::Integer(i)) => {
            let v = *i;
            if v < 0 || v > u16::MAX as i64 {
                return Err(PdfMaskError::pdf_read(format!(
                    "FirstChar out of range: {}",
                    v
                )));
            }
            v as u16
        }
        Some(_) => 0u16,
    };

    let widths_obj = match font_dict.get(b"Widths") {
        Ok(obj) => {
            doc.dereference(obj)
                .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
                .1
        }
        Err(_) => return Ok(result),
    };

    if let Ok(arr) = widths_obj.as_array() {
        for (i, obj) in arr.iter().enumerate() {
            let obj = doc
                .dereference(obj)
                .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
                .1;
            let w = match obj {
                Object::Integer(i) => *i as f64,
                Object::Real(r) => *r as f64,
                _ => continue,
            };
            let code = first_char + i as u16;
            result.insert(code, w);
        }
    }

    Ok(result)
}

/// CIDFont の /W (Widths) 配列を解析
fn parse_cid_widths(
    doc: &Document,
    cid_font_dict: &lopdf::Dictionary,
) -> crate::error::Result<HashMap<u16, f64>> {
    let mut result = HashMap::new();

    let w_obj = match cid_font_dict.get(b"W") {
        Ok(obj) => {
            doc.dereference(obj)
                .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?
                .1
        }
        Err(_) => return Ok(result),
    };

    let arr = match w_obj.as_array() {
        Ok(a) => a,
        Err(_) => return Ok(result),
    };

    // /W 配列: [ cid [w1 w2 ...] ] or [ cid_first cid_last w ]
    let mut i = 0;
    while i < arr.len() {
        let cid_start = match &arr[i] {
            Object::Integer(n) => *n as u16,
            _ => {
                i += 1;
                continue;
            }
        };
        i += 1;

        if i >= arr.len() {
            break;
        }

        match &arr[i] {
            Object::Array(widths) => {
                // [ cid [w1 w2 w3 ...] ]
                for (j, w_obj) in widths.iter().enumerate() {
                    let w = match w_obj {
                        Object::Integer(n) => *n as f64,
                        Object::Real(r) => *r as f64,
                        _ => continue,
                    };
                    result.insert(cid_start + j as u16, w);
                }
                i += 1;
            }
            Object::Integer(cid_end) => {
                // [ cid_first cid_last w ]
                let cid_end = *cid_end as u16;
                i += 1;
                if i >= arr.len() {
                    break;
                }
                let w = match &arr[i] {
                    Object::Integer(n) => *n as f64,
                    Object::Real(r) => *r as f64,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                for cid in cid_start..=cid_end {
                    result.insert(cid, w);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    Ok(result)
}

/// エンコーディングの解析
fn parse_encoding(
    doc: &Document,
    font_dict: &lopdf::Dictionary,
) -> crate::error::Result<FontEncoding> {
    let enc_obj = match font_dict.get(b"Encoding") {
        Ok(obj) => obj,
        Err(_) => {
            return Ok(FontEncoding::WinAnsi {
                differences: HashMap::new(),
            });
        }
    };

    match enc_obj {
        Object::Name(name) => {
            let name_str = String::from_utf8_lossy(name);
            if name_str == "WinAnsiEncoding" {
                Ok(FontEncoding::WinAnsi {
                    differences: HashMap::new(),
                })
            } else if name_str == "Identity-H" {
                Ok(FontEncoding::IdentityH)
            } else {
                // MacRomanEncoding等は WinAnsi として近似
                Ok(FontEncoding::WinAnsi {
                    differences: HashMap::new(),
                })
            }
        }
        Object::Reference(id) => {
            let obj = doc
                .get_object(*id)
                .map_err(|e| PdfMaskError::pdf_read(e.to_string()))?;
            if let Ok(dict) = obj.as_dict() {
                parse_encoding_dict(doc, dict)
            } else {
                Ok(FontEncoding::WinAnsi {
                    differences: HashMap::new(),
                })
            }
        }
        Object::Dictionary(dict) => parse_encoding_dict(doc, dict),
        _ => Ok(FontEncoding::WinAnsi {
            differences: HashMap::new(),
        }),
    }
}

/// エンコーディング辞書の解析（Differences配列を含む）
fn parse_encoding_dict(
    _doc: &Document,
    _dict: &lopdf::Dictionary,
) -> crate::error::Result<FontEncoding> {
    // Differences配列のグリフ名→GIDマッピングは将来対応
    // 現時点ではWinAnsiのベースエンコーディングのみ
    Ok(FontEncoding::WinAnsi {
        differences: HashMap::new(),
    })
}

/// グリフ名→Unicode変換（Adobe Glyph Listの主要エントリ）
fn glyph_name_to_unicode(name: &str) -> Option<char> {
    // 主要なグリフ名のみ対応（完全なAGLは数千エントリ）
    match name {
        "space" => Some(' '),
        "exclam" => Some('!'),
        "quotedbl" => Some('"'),
        "numbersign" => Some('#'),
        "dollar" => Some('$'),
        "percent" => Some('%'),
        "ampersand" => Some('&'),
        "quotesingle" => Some('\''),
        "parenleft" => Some('('),
        "parenright" => Some(')'),
        "asterisk" => Some('*'),
        "plus" => Some('+'),
        "comma" => Some(','),
        "hyphen" | "minus" => Some('-'),
        "period" => Some('.'),
        "slash" => Some('/'),
        "zero" => Some('0'),
        "one" => Some('1'),
        "two" => Some('2'),
        "three" => Some('3'),
        "four" => Some('4'),
        "five" => Some('5'),
        "six" => Some('6'),
        "seven" => Some('7'),
        "eight" => Some('8'),
        "nine" => Some('9'),
        "colon" => Some(':'),
        "semicolon" => Some(';'),
        "less" => Some('<'),
        "equal" => Some('='),
        "greater" => Some('>'),
        "question" => Some('?'),
        "at" => Some('@'),
        _ if name.len() == 1 => name.chars().next(),
        _ if name.starts_with("uni") && name.len() == 7 => u32::from_str_radix(&name[3..], 16)
            .ok()
            .and_then(char::from_u32),
        _ => {
            // A-Z, a-z の名前は直接文字に対応
            if name.len() == 1 {
                name.chars().next()
            } else {
                None
            }
        }
    }
}

/// WinAnsi文字コード→Unicode変換（基本ラテン文字のみ）
fn win_ansi_to_unicode(code: u8) -> Option<char> {
    // 0x20-0x7E: ASCII直接対応
    if (0x20..=0x7E).contains(&code) {
        return Some(code as char);
    }

    // Windows-1252 の上位バイトマッピング
    match code {
        0x80 => Some('\u{20AC}'),          // €
        0x82 => Some('\u{201A}'),          // ‚
        0x83 => Some('\u{0192}'),          // ƒ
        0x84 => Some('\u{201E}'),          // „
        0x85 => Some('\u{2026}'),          // …
        0x86 => Some('\u{2020}'),          // †
        0x87 => Some('\u{2021}'),          // ‡
        0x88 => Some('\u{02C6}'),          // ˆ
        0x89 => Some('\u{2030}'),          // ‰
        0x8A => Some('\u{0160}'),          // Š
        0x8B => Some('\u{2039}'),          // ‹
        0x8C => Some('\u{0152}'),          // Œ
        0x8E => Some('\u{017D}'),          // Ž
        0x91 => Some('\u{2018}'),          // '
        0x92 => Some('\u{2019}'),          // '
        0x93 => Some('\u{201C}'),          // "
        0x94 => Some('\u{201D}'),          // "
        0x95 => Some('\u{2022}'),          // •
        0x96 => Some('\u{2013}'),          // –
        0x97 => Some('\u{2014}'),          // —
        0x98 => Some('\u{02DC}'),          // ˜
        0x99 => Some('\u{2122}'),          // ™
        0x9A => Some('\u{0161}'),          // š
        0x9B => Some('\u{203A}'),          // ›
        0x9C => Some('\u{0153}'),          // œ
        0x9E => Some('\u{017E}'),          // ž
        0x9F => Some('\u{0178}'),          // Ÿ
        0xA0..=0xFF => Some(code as char), // Latin-1 Supplement直接対応
        _ => None,
    }
}
