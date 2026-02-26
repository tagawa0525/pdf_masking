// Phase 5: leptonica segmentation: bitmap -> mask/fg/bg separation

use tracing::debug;

#[cfg(feature = "mrc")]
use leptonica::{Pix, PixMut, PixelDepth};

/// テキスト領域のピクセル座標バウンディングボックス。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelBBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// RGBAデータから leptonica 32-bit Pix を作成するヘルパー。
///
/// `segmenter.rs` と `image_xobject.rs` の両方から使用する。
#[cfg(feature = "mrc")]
pub fn pix_from_raw_rgba(width: u32, height: u32, data: &[u8]) -> crate::error::Result<Pix> {
    use crate::error::PdfMaskError;

    let expected_size = width
        .checked_mul(height)
        .and_then(|wh| wh.checked_mul(4))
        .ok_or_else(|| {
            PdfMaskError::segmentation(format!(
                "Overflow computing buffer size for {}x{} RGBA image",
                width, height
            ))
        })? as usize;

    if data.len() != expected_size {
        return Err(PdfMaskError::segmentation(format!(
            "Data size mismatch: expected {} bytes, got {}",
            expected_size,
            data.len()
        )));
    }

    // 32-bit Pix を作成してピクセルデータをコピー
    let pix = Pix::new(width, height, PixelDepth::Bit32)
        .map_err(|e| PdfMaskError::segmentation(e.to_string()))?;

    let mut pix_mut: PixMut = pix.try_into_mut().unwrap_or_else(|p| p.to_mut());

    // RGBA データを 32-bit Pix のワード配列にコピー
    // leptonica の 32-bit レイアウト: 各ピクセルが 1 word (u32)
    // RGBA の各バイトをシフトして合成する
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let r = data[idx] as u32;
            let g = data[idx + 1] as u32;
            let b = data[idx + 2] as u32;
            let a = data[idx + 3] as u32;
            // leptonica 32-bit pixel layout: R<<24 | G<<16 | B<<8 | A
            let val = (r << 24) | (g << 16) | (b << 8) | a;
            pix_mut.set_pixel(x, y, val).map_err(|e| {
                PdfMaskError::segmentation(format!("Failed to set pixel at ({}, {}): {}", x, y, e))
            })?;
        }
    }

    Ok(pix_mut.into())
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
#[cfg(feature = "mrc")]
pub fn extract_text_bboxes(
    text_mask: &Pix,
    merge_distance: u32,
) -> crate::error::Result<Vec<PixelBBox>> {
    use crate::error::PdfMaskError;
    use leptonica::region::conncomp::{ConnectivityType, find_connected_components};

    // Connected components のバウンディングボックスを取得
    let components = find_connected_components(text_mask, ConnectivityType::FourWay)
        .map_err(|e| PdfMaskError::segmentation(e.to_string()))?;

    // PixelBBox に変換（負の値は 0 にクランプ）
    let mut bboxes: Vec<PixelBBox> = components
        .into_iter()
        .map(|cc| {
            let b = cc.bounds;
            PixelBBox {
                x: b.x.max(0) as u32,
                y: b.y.max(0) as u32,
                width: b.w.max(0) as u32,
                height: b.h.max(0) as u32,
            }
        })
        .collect();

    let before_filter = bboxes.len();

    // 最小面積閾値: 4x4px 未満を除外
    bboxes.retain(|b| b.width >= 4 && b.height >= 4);

    let after_filter = bboxes.len();

    // 近接矩形のマージ
    if merge_distance > 0 {
        bboxes = merge_nearby_bboxes(bboxes, merge_distance);
    }

    debug!(
        raw = before_filter,
        filtered = after_filter,
        merged = bboxes.len(),
        "extract_text_bboxes"
    );
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
#[cfg(feature = "mrc")]
pub fn segment_text_mask(rgba_data: &[u8], width: u32, height: u32) -> crate::error::Result<Pix> {
    use crate::error::PdfMaskError;
    use leptonica::color::otsu_adaptive_threshold;
    use leptonica::recog::pageseg::{PageSegOptions, segment_regions};

    // 1. RGBA -> leptonica 32-bit Pix
    let pix = pix_from_raw_rgba(width, height, rgba_data)?;

    // 2. Convert 32-bit RGBA to 8-bit grayscale (Otsu requires 8 bpp)
    let gray = pix
        .convert_rgb_to_gray(0.0, 0.0, 0.0)
        .map_err(|e| PdfMaskError::segmentation(e.to_string()))?;

    // 3. Otsu adaptive threshold -> 1-bit binary image
    //    Tile size is capped at the image dimension (min 16px to avoid
    //    degenerate tiles) so it adapts to both small and large images.
    let tile_sx = width.clamp(16, 2000);
    let tile_sy = height.clamp(16, 2000);
    let (binary, _threshold_map) = otsu_adaptive_threshold(&gray, tile_sx, tile_sy, 0, 0, 0.0)
        .map_err(|e| PdfMaskError::segmentation(e.to_string()))?;

    // 4. Extract region masks from the binary image
    let opts = PageSegOptions {
        detect_halftone: true,
        ..Default::default()
    };
    let result =
        segment_regions(&binary, &opts).map_err(|e| PdfMaskError::segmentation(e.to_string()))?;

    // Use the textline mask if it contains any foreground pixels.
    // If empty, return an all-zero 1-bit mask.
    let textline_mask = result.textline_mask;
    let has_text = has_foreground(&textline_mask);

    if has_text {
        Ok(textline_mask)
    } else {
        Pix::new(width, height, PixelDepth::Bit1)
            .map_err(|e| PdfMaskError::segmentation(e.to_string()))
    }
}

/// 1-bit Pix にフォアグラウンドピクセル（1）が存在するか確認する。
#[cfg(feature = "mrc")]
fn has_foreground(pix: &Pix) -> bool {
    let w = pix.width();
    let h = pix.height();
    for y in 0..h {
        for x in 0..w {
            if pix.get_pixel(x, y).unwrap_or(0) != 0 {
                return true;
            }
        }
    }
    false
}
