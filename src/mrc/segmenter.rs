// Phase 5: leptonica segmentation: bitmap -> mask/fg/bg separation

use crate::ffi::leptonica::Pix;

/// テキスト領域のピクセル座標バウンディングボックス。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelBBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// テキストマスクから矩形領域を抽出する。
///
/// 1-bit テキストマスクの connected components を検出し、
/// 近接する矩形をマージして XObject 数を削減する。
/// 4x4px 未満の矩形はノイズとして除外する。
///
/// # Arguments
/// * `text_mask` - 1-bit テキストマスク（`segment_text_mask` の出力）
/// * `merge_distance` - この距離以下の矩形をマージする（ピクセル単位）
///
/// # Returns
/// マージ済みのテキスト領域矩形リスト
pub fn extract_text_bboxes(
    text_mask: &Pix,
    merge_distance: u32,
) -> crate::error::Result<Vec<PixelBBox>> {
    // Connected components のバウンディングボックスを取得
    let raw_boxes = text_mask.connected_component_bboxes(4)?;

    // PixelBBox に変換
    let mut bboxes: Vec<PixelBBox> = raw_boxes
        .into_iter()
        .map(|(x, y, w, h)| PixelBBox {
            x,
            y,
            width: w,
            height: h,
        })
        .collect();

    // 最小面積閾値: 4x4px 未満を除外
    bboxes.retain(|b| b.width >= 4 && b.height >= 4);

    // 近接矩形のマージ
    if merge_distance > 0 {
        bboxes = merge_nearby_bboxes(bboxes, merge_distance);
    }

    Ok(bboxes)
}

/// 近接する矩形をマージする。
///
/// 2つの矩形の間のギャップが `distance` 以下の場合、
/// 両方を包含する矩形にマージする。収束するまで繰り返す。
fn merge_nearby_bboxes(mut bboxes: Vec<PixelBBox>, distance: u32) -> Vec<PixelBBox> {
    loop {
        let mut merged = false;
        let mut result: Vec<PixelBBox> = Vec::new();

        for bbox in bboxes {
            let mut was_merged = false;
            for existing in &mut result {
                if bboxes_are_nearby(existing, &bbox, distance) {
                    // マージ: 両方を包含する矩形に拡張
                    let x_min = existing.x.min(bbox.x);
                    let y_min = existing.y.min(bbox.y);
                    let x_max = (existing.x + existing.width).max(bbox.x + bbox.width);
                    let y_max = (existing.y + existing.height).max(bbox.y + bbox.height);
                    existing.x = x_min;
                    existing.y = y_min;
                    existing.width = x_max - x_min;
                    existing.height = y_max - y_min;
                    was_merged = true;
                    merged = true;
                    break;
                }
            }
            if !was_merged {
                result.push(bbox);
            }
        }

        bboxes = result;
        if !merged {
            break;
        }
    }

    bboxes
}

/// 2つの矩形が distance 以下のギャップで近接しているか判定。
fn bboxes_are_nearby(a: &PixelBBox, b: &PixelBBox, distance: u32) -> bool {
    let a_right = a.x + a.width;
    let a_bottom = a.y + a.height;
    let b_right = b.x + b.width;
    let b_bottom = b.y + b.height;

    // 拡張した矩形同士が重なるかチェック
    let d = distance as i64;
    let gap_x = (b.x as i64 - a_right as i64).max(a.x as i64 - b_right as i64);
    let gap_y = (b.y as i64 - a_bottom as i64).max(a.y as i64 - b_bottom as i64);

    gap_x <= d && gap_y <= d
}

/// Segment an RGBA bitmap into a 1-bit text mask using Otsu binarization.
///
/// Returns a 1-bit `Pix` where text regions are set (1) and non-text
/// regions are clear (0).  When no text is detected the mask is all-zero.
///
/// # Arguments
/// * `rgba_data` - Raw RGBA pixel data (4 bytes per pixel)
/// * `width`     - Image width in pixels
/// * `height`    - Image height in pixels
pub fn segment_text_mask(rgba_data: &[u8], width: u32, height: u32) -> crate::error::Result<Pix> {
    // 1. RGBA -> leptonica 32-bit Pix
    let pix = Pix::from_raw_rgba(width, height, rgba_data)?;

    // 2. Convert 32-bit RGBA to 8-bit grayscale (Otsu requires 8 bpp)
    let gray = pix.convert_to_gray()?;

    // 3. Otsu adaptive threshold -> 1-bit binary image
    //    Tile size is capped at the image dimension (min 16px to avoid
    //    degenerate tiles) so it adapts to both small and large images.
    let tile_sx = width.clamp(16, 2000);
    let tile_sy = height.clamp(16, 2000);
    let binary = gray.otsu_adaptive_threshold(tile_sx, tile_sy)?;

    // 4. Extract region masks from the binary image
    let masks = binary.get_region_masks()?;

    // Use the textline mask if leptonica detected text regions.
    // Otherwise return an empty (all-zero) 1-bit mask.
    match masks.textline {
        Some(textline_mask) => Ok(textline_mask),
        None => Pix::create(width, height, 1),
    }
}
