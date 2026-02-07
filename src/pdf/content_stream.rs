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
