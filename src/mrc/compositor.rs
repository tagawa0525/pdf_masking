// Phase 5: pipeline integration: bitmap + config -> MrcLayers

use super::{BwLayers, MrcLayers, jbig2, jpeg, segmenter};
use crate::config::job::ColorMode;
use crate::error::PdfMaskError;
use crate::mrc::segmenter::PixelBBox;
use image::{DynamicImage, RgbaImage};

/// Configuration for MRC layer generation.
pub struct MrcConfig {
    /// JPEG quality for the background layer (1-100)
    pub bg_quality: u8,
    /// JPEG quality for the foreground layer (1-100)
    pub fg_quality: u8,
}

/// Generate MRC layers from an RGBA bitmap.
///
/// Pipeline:
/// 1. Segment text regions into a 1-bit mask
/// 2. Encode the mask as JBIG2
/// 3. Convert RGBA to RGB/Gray
/// 4. Encode the background as JPEG
/// 5. Encode the foreground as JPEG
///
/// # Arguments
/// * `rgba_data` - Raw RGBA pixel data (4 bytes per pixel)
/// * `width`     - Image width in pixels
/// * `height`    - Image height in pixels
/// * `config`    - Quality settings for the output layers
/// * `color_mode` - RGB or Grayscale
pub fn compose(
    rgba_data: &[u8],
    width: u32,
    height: u32,
    config: &MrcConfig,
    color_mode: ColorMode,
) -> crate::error::Result<MrcLayers> {
    // 1. Segment: RGBA -> 1-bit text mask
    let mut text_mask = segmenter::segment_text_mask(rgba_data, width, height)?;

    // 2. Mask layer: JBIG2-encode the 1-bit mask
    let mask_jbig2 = jbig2::encode_mask(&mut text_mask)?;

    // 3. Convert RGBA -> image
    let img = RgbaImage::from_raw(width, height, rgba_data.to_vec())
        .ok_or_else(|| PdfMaskError::jpeg_encode("Failed to create image from RGBA data"))?;
    let dynamic = DynamicImage::ImageRgba8(img);

    let (background_jpeg, foreground_jpeg) = match color_mode {
        ColorMode::Grayscale => {
            let gray = dynamic.to_luma8();
            let bg = jpeg::encode_gray_to_jpeg(&gray, config.bg_quality)?;
            let fg = jpeg::encode_gray_to_jpeg(&gray, config.fg_quality)?;
            (bg, fg)
        }
        _ => {
            // Rgb (default)
            let rgb = dynamic.to_rgb8();
            let bg = jpeg::encode_rgb_to_jpeg(&rgb, config.bg_quality)?;
            let fg = jpeg::encode_rgb_to_jpeg(&rgb, config.fg_quality)?;
            (bg, fg)
        }
    };

    Ok(MrcLayers {
        mask_jbig2,
        foreground_jpeg,
        background_jpeg,
        width,
        height,
        color_mode,
    })
}

/// テキスト領域をビットマップからクロップし、JPEG化する。
///
/// 各 PixelBBox 領域をクロップして、指定のカラーモードとクオリティで
/// JPEG エンコードする。
///
/// # Arguments
/// * `bitmap` - レンダリング済みのページビットマップ
/// * `bboxes` - テキスト領域の矩形リスト（ピクセル座標）
/// * `quality` - JPEG 品質 (1-100)
/// * `color_mode` - RGB または Grayscale
///
/// # Returns
/// `(jpeg_data, bbox)` のペアリスト
pub fn crop_text_regions(
    bitmap: &DynamicImage,
    bboxes: &[PixelBBox],
    quality: u8,
    color_mode: ColorMode,
) -> crate::error::Result<Vec<(Vec<u8>, PixelBBox)>> {
    if !(1..=100).contains(&quality) {
        return Err(PdfMaskError::jpeg_encode(format!(
            "JPEG quality must be 1-100, got {}",
            quality
        )));
    }

    let mut results = Vec::with_capacity(bboxes.len());

    for bbox in bboxes {
        // クロップ
        let cropped = bitmap.crop_imm(bbox.x, bbox.y, bbox.width, bbox.height);

        // カラーモードに応じてエンコード
        let jpeg_data = match color_mode {
            ColorMode::Grayscale => {
                let gray = cropped.to_luma8();
                jpeg::encode_gray_to_jpeg(&gray, quality)?
            }
            ColorMode::Rgb => {
                let rgb = cropped.to_rgb8();
                jpeg::encode_rgb_to_jpeg(&rgb, quality)?
            }
            ColorMode::Bw | ColorMode::Skip => {
                return Err(PdfMaskError::jpeg_encode(format!(
                    "crop_text_regions does not support {:?} color mode",
                    color_mode
                )));
            }
        };

        results.push((jpeg_data, bbox.clone()));
    }

    Ok(results)
}

/// BWモード: segmenter + JBIG2のみ。JPEG層なし。
pub fn compose_bw(rgba_data: &[u8], width: u32, height: u32) -> crate::error::Result<BwLayers> {
    let mut text_mask = segmenter::segment_text_mask(rgba_data, width, height)?;
    let mask_jbig2 = jbig2::encode_mask(&mut text_mask)?;

    Ok(BwLayers {
        mask_jbig2,
        width,
        height,
    })
}
