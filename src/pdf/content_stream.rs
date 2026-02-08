use lopdf::content::Content;

/// 6要素アフィン変換行列 [a, b, c, d, e, f]
/// PDF仕様: [ a b 0 ]
///          [ c d 0 ]
///          [ e f 1 ]
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    /// 単位行列を返す。
    pub fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// self * other (行列の右乗算)
    pub fn multiply(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }
}

/// 矩形領域を表すバウンディングボックス。
#[derive(Debug, Clone)]
pub struct BBox {
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
}

/// 画像XObjectの配置情報。
#[derive(Debug, Clone)]
pub struct ImagePlacement {
    /// XObjectの名前 (e.g. "Im1")
    pub name: String,
    /// 描画時のCTM
    pub ctm: Matrix,
    /// CTMから計算したBBox
    pub bbox: BBox,
}

/// コンテンツストリームを解析し、全XObjectの配置情報を抽出する。
///
/// CTMスタック(q/Q)を追跡し、cmオペレータでCTMを更新する。
/// DoオペレータでXObject名とその時点のCTM・BBoxを記録する。
/// Image XObjectだけでなくForm XObjectも含む全XObjectの配置を返す。
pub fn extract_xobject_placements(
    content_bytes: &[u8],
) -> crate::error::Result<Vec<ImagePlacement>> {
    // 空バイト列の場合、lopdfのパーサがエラーを返す可能性があるため特別扱い
    if content_bytes.is_empty() {
        return Ok(Vec::new());
    }

    let content = Content::decode(content_bytes)
        .map_err(|e| crate::error::PdfMaskError::content_stream(e.to_string()))?;

    let mut ctm_stack: Vec<Matrix> = vec![Matrix::identity()];
    let mut placements: Vec<ImagePlacement> = Vec::new();

    let operations: &[lopdf::content::Operation] = content.operations.as_ref();
    for op in operations {
        match op.operator.as_str() {
            "q" => {
                // グラフィックス状態を保存: 現在のCTMをスタックにpush
                let current = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                ctm_stack.push(current);
            }
            "Q" => {
                // グラフィックス状態を復元: スタックからpop
                if ctm_stack.len() > 1 {
                    ctm_stack.pop();
                }
            }
            "cm" => {
                // CTMを更新: 現在のCTM = 現在のCTM × Matrix(a,b,c,d,e,f)
                if op.operands.len() == 6 {
                    let vals: Vec<f64> = op
                        .operands
                        .iter()
                        .map(operand_to_f64)
                        .collect::<Result<Vec<_>, _>>()?;
                    let cm_matrix = Matrix {
                        a: vals[0],
                        b: vals[1],
                        c: vals[2],
                        d: vals[3],
                        e: vals[4],
                        f: vals[5],
                    };
                    if let Some(current) = ctm_stack.last_mut() {
                        *current = current.multiply(&cm_matrix);
                    }
                }
            }
            "Do" => {
                // XObjectを描画: 名前とCTMを記録
                if let Some(operand) = op.operands.first() {
                    let name_bytes: &[u8] = operand
                        .as_name()
                        .map_err(|e| crate::error::PdfMaskError::content_stream(e.to_string()))?;
                    let name = String::from_utf8_lossy(name_bytes).into_owned();
                    let current_ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                    let bbox = ctm_to_bbox(&current_ctm);
                    placements.push(ImagePlacement {
                        name,
                        ctm: current_ctm,
                        bbox,
                    });
                }
            }
            _ => {
                // その他のオペレータは無視
            }
        }
    }

    Ok(placements)
}

/// lopdfのObjectから数値をf64として取得する。
fn operand_to_f64(obj: &lopdf::Object) -> crate::error::Result<f64> {
    match obj {
        lopdf::Object::Integer(i) => Ok(*i as f64),
        lopdf::Object::Real(r) => Ok(*r as f64),
        _ => Err(crate::error::PdfMaskError::content_stream(format!(
            "expected numeric operand, got {:?}",
            obj
        ))),
    }
}

/// CTMからBBoxを計算する。
/// 単位正方形 [0,0]-[1,1] の4頂点をCTMで変換し、min/maxを取る。
fn ctm_to_bbox(ctm: &Matrix) -> BBox {
    let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
    let transformed: Vec<(f64, f64)> = corners
        .iter()
        .map(|&(x, y)| {
            let x_prime = ctm.a * x + ctm.c * y + ctm.e;
            let y_prime = ctm.b * x + ctm.d * y + ctm.f;
            (x_prime, y_prime)
        })
        .collect();

    let x_min = transformed
        .iter()
        .map(|p| p.0)
        .fold(f64::INFINITY, f64::min);
    let y_min = transformed
        .iter()
        .map(|p| p.1)
        .fold(f64::INFINITY, f64::min);
    let x_max = transformed
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let y_max = transformed
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);

    BBox {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

/// コンテンツストリームからBT...ETブロック（テキストオブジェクト）を除去する。
///
/// BTオペレータでテキストブロックが開始され、ETオペレータで終了する。
/// このネスト深度を追跡し、深度>0の全オペレーションを除去する。
/// 非テキストオペレーション（グラフィックス、XObject描画等）はそのまま保持する。
///
/// # 引数
/// * `content_bytes` - 元のコンテンツストリームバイト列
///
/// # 戻り値
/// テキストオペレーションを除去したコンテンツストリーム
pub fn strip_text_operators(content_bytes: &[u8]) -> crate::error::Result<Vec<u8>> {
    // 空バイト列の場合は空を返す
    if content_bytes.is_empty() {
        return Ok(Vec::new());
    }

    let content = Content::decode(content_bytes)
        .map_err(|e| crate::error::PdfMaskError::content_stream(e.to_string()))?;

    let mut depth = 0_u32;
    let mut filtered_operations = Vec::new();

    for op in &content.operations {
        match op.operator.as_str() {
            "BT" => {
                // テキストブロック開始
                depth = depth.saturating_add(1);
                // BTオペレータ自体も除去
            }
            "ET" => {
                // テキストブロック終了
                depth = depth.saturating_sub(1);
                // ETオペレータ自体も除去
            }
            _ => {
                // 深度0（テキストブロック外）のオペレーションのみ保持
                if depth == 0 {
                    filtered_operations.push(op.clone());
                }
            }
        }
    }

    // 再エンコード
    let filtered_content = Content {
        operations: filtered_operations,
    };

    filtered_content
        .encode()
        .map_err(|e| crate::error::PdfMaskError::content_stream(e.to_string()))
}

/// ピクセル座標をPDFページ座標（ポイント）に変換する。
///
/// PDFの座標系は左下原点（Y軸上向き）、ビットマップは左上原点（Y軸下向き）。
/// この関数はY軸反転を含む変換を行う。
///
/// # Arguments
/// * `pixel_bbox` - ピクセル座標のバウンディングボックス
/// * `page_width_pts` - ページ幅（ポイント）
/// * `page_height_pts` - ページ高さ（ポイント）
/// * `bitmap_width_px` - ビットマップ幅（ピクセル）
/// * `bitmap_height_px` - ビットマップ高さ（ピクセル）
///
/// # Returns
/// PDFポイント座標のBBox
pub fn pixel_to_page_coords(
    pixel_bbox: &crate::mrc::segmenter::PixelBBox,
    page_width_pts: f64,
    page_height_pts: f64,
    bitmap_width_px: u32,
    bitmap_height_px: u32,
) -> crate::error::Result<BBox> {
    if bitmap_width_px == 0 || bitmap_height_px == 0 {
        return Err(crate::error::PdfMaskError::content_stream(
            "bitmap dimensions must be non-zero for coordinate conversion",
        ));
    }

    let scale_x = page_width_pts / bitmap_width_px as f64;
    let scale_y = page_height_pts / bitmap_height_px as f64;

    // u32オーバーフロー防止: u64で加算してからf64に変換
    let pixel_right = pixel_bbox.x as u64 + pixel_bbox.width as u64;
    let pixel_bottom = pixel_bbox.y as u64 + pixel_bbox.height as u64;

    // ピクセル座標 → PDFポイント座標（Y軸反転）
    let x_min = pixel_bbox.x as f64 * scale_x;
    let x_max = pixel_right as f64 * scale_x;

    // ビットマップのY=0（上端）はPDFのY=page_height_pts（上端）
    let y_max = page_height_pts - (pixel_bbox.y as f64 * scale_y);
    let y_min = page_height_pts - (pixel_bottom as f64 * scale_y);

    Ok(BBox {
        x_min,
        y_min,
        x_max,
        y_max,
    })
}

/// fill colorの状態
#[derive(Debug, Clone)]
struct FillColor {
    /// 白色 (RGB: 1,1,1 / Gray: 1 / CMYK: 0,0,0,0) かどうか
    is_white: bool,
}

impl FillColor {
    fn default_black() -> Self {
        FillColor { is_white: false }
    }
}

/// コンテンツストリームから白色fill矩形の位置を抽出する。
///
/// 追跡するオペレータ:
/// - 色設定: `rg`/`g`/`k`/`sc`/`scn` (fill color)
/// - パス構築: `re` (rectangle)
/// - fill: `f`/`F`/`f*`
/// - CTMスタック: `q`/`Q`/`cm`
///
/// # Arguments
/// * `content_bytes` - コンテンツストリームのバイト列
///
/// # Returns
/// CTM適用済みのページ座標BBoxリスト（白色fill矩形のみ）
pub fn extract_white_fill_rects(content_bytes: &[u8]) -> crate::error::Result<Vec<BBox>> {
    if content_bytes.is_empty() {
        return Ok(Vec::new());
    }

    let content = Content::decode(content_bytes)
        .map_err(|e| crate::error::PdfMaskError::content_stream(e.to_string()))?;

    let mut ctm_stack: Vec<Matrix> = vec![Matrix::identity()];
    let mut fill_color_stack: Vec<FillColor> = vec![FillColor::default_black()];
    let mut results: Vec<BBox> = Vec::new();

    // 現在のパス上の矩形（reオペレータで蓄積、fillで一括処理）
    let mut current_rects: Vec<(f64, f64, f64, f64)> = Vec::new();

    for op in &content.operations {
        match op.operator.as_str() {
            "q" => {
                let current_ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                ctm_stack.push(current_ctm);
                let current_fill = fill_color_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(FillColor::default_black);
                fill_color_stack.push(current_fill);
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
                    let cm_matrix = Matrix {
                        a: vals[0],
                        b: vals[1],
                        c: vals[2],
                        d: vals[3],
                        e: vals[4],
                        f: vals[5],
                    };
                    if let Some(current) = ctm_stack.last_mut() {
                        *current = current.multiply(&cm_matrix);
                    }
                }
            }
            // Fill color operators
            "rg" => {
                // RGB fill color: r g b rg
                if op.operands.len() == 3
                    && let (Ok(r), Ok(g), Ok(b)) = (
                        operand_to_f64(&op.operands[0]),
                        operand_to_f64(&op.operands[1]),
                        operand_to_f64(&op.operands[2]),
                    )
                    && let Some(fc) = fill_color_stack.last_mut()
                {
                    fc.is_white = is_white_rgb(r, g, b);
                }
            }
            "g" => {
                // Gray fill color: gray g
                if op.operands.len() == 1
                    && let Ok(gray) = operand_to_f64(&op.operands[0])
                    && let Some(fc) = fill_color_stack.last_mut()
                {
                    fc.is_white = is_white_gray(gray);
                }
            }
            "k" => {
                // CMYK fill color: c m y k k
                if op.operands.len() == 4
                    && let (Ok(c), Ok(m), Ok(y), Ok(k)) = (
                        operand_to_f64(&op.operands[0]),
                        operand_to_f64(&op.operands[1]),
                        operand_to_f64(&op.operands[2]),
                        operand_to_f64(&op.operands[3]),
                    )
                    && let Some(fc) = fill_color_stack.last_mut()
                {
                    fc.is_white = is_white_cmyk(c, m, y, k);
                }
            }
            "sc" | "scn" => {
                // Generic fill color: 値の数で判定
                if let Some(fc) = fill_color_stack.last_mut() {
                    fc.is_white = match op.operands.len() {
                        1 => operand_to_f64(&op.operands[0])
                            .map(is_white_gray)
                            .unwrap_or(false),
                        3 => {
                            if let (Ok(r), Ok(g), Ok(b)) = (
                                operand_to_f64(&op.operands[0]),
                                operand_to_f64(&op.operands[1]),
                                operand_to_f64(&op.operands[2]),
                            ) {
                                is_white_rgb(r, g, b)
                            } else {
                                false
                            }
                        }
                        4 => {
                            if let (Ok(c), Ok(m), Ok(y), Ok(k)) = (
                                operand_to_f64(&op.operands[0]),
                                operand_to_f64(&op.operands[1]),
                                operand_to_f64(&op.operands[2]),
                                operand_to_f64(&op.operands[3]),
                            ) {
                                is_white_cmyk(c, m, y, k)
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                }
            }
            // Path construction: rectangle
            "re" => {
                if op.operands.len() == 4
                    && let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                        operand_to_f64(&op.operands[0]),
                        operand_to_f64(&op.operands[1]),
                        operand_to_f64(&op.operands[2]),
                        operand_to_f64(&op.operands[3]),
                    )
                {
                    current_rects.push((x, y, w, h));
                }
            }
            // Non-rectangle path construction: path is no longer just rectangles
            "m" | "l" | "c" | "v" | "y" | "h" => {
                current_rects.clear();
            }
            // Fill operators
            "f" | "F" | "f*" => {
                let is_white = fill_color_stack
                    .last()
                    .map(|fc| fc.is_white)
                    .unwrap_or(false);
                if is_white {
                    let ctm = ctm_stack.last().cloned().unwrap_or_else(Matrix::identity);
                    for (x, y, w, h) in &current_rects {
                        let bbox = rect_to_bbox(&ctm, *x, *y, *w, *h);
                        results.push(bbox);
                    }
                }
                current_rects.clear();
            }
            // Path end without fill
            "S" | "s" | "B" | "B*" | "b" | "b*" | "n" | "W" | "W*" => {
                current_rects.clear();
            }
            _ => {}
        }
    }

    Ok(results)
}

/// 矩形(x, y, w, h)をCTMで変換しBBoxを返す。
fn rect_to_bbox(ctm: &Matrix, x: f64, y: f64, w: f64, h: f64) -> BBox {
    let corners = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)];
    let transformed: Vec<(f64, f64)> = corners
        .iter()
        .map(|&(px, py)| {
            let x_prime = ctm.a * px + ctm.c * py + ctm.e;
            let y_prime = ctm.b * px + ctm.d * py + ctm.f;
            (x_prime, y_prime)
        })
        .collect();

    let x_min = transformed
        .iter()
        .map(|p| p.0)
        .fold(f64::INFINITY, f64::min);
    let y_min = transformed
        .iter()
        .map(|p| p.1)
        .fold(f64::INFINITY, f64::min);
    let x_max = transformed
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let y_max = transformed
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);

    BBox {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

fn is_white_rgb(r: f64, g: f64, b: f64) -> bool {
    (r - 1.0).abs() < 1e-6 && (g - 1.0).abs() < 1e-6 && (b - 1.0).abs() < 1e-6
}

fn is_white_gray(gray: f64) -> bool {
    (gray - 1.0).abs() < 1e-6
}

fn is_white_cmyk(c: f64, m: f64, y: f64, k: f64) -> bool {
    c.abs() < 1e-6 && m.abs() < 1e-6 && y.abs() < 1e-6 && k.abs() < 1e-6
}
