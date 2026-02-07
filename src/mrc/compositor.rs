// Phase 5: pipeline integration: bitmap + config -> MrcLayers

use super::{jbig2, jpeg, segmenter, MrcLayers};

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
/// 3. Encode the background as JPEG
/// 4. Encode the foreground as JPEG
///
/// In v1, the background and foreground layers use the original image
/// without text-aware inpainting or masking.
///
/// # Arguments
/// * `rgba_data` - Raw RGBA pixel data (4 bytes per pixel)
/// * `width`     - Image width in pixels
/// * `height`    - Image height in pixels
/// * `config`    - Quality settings for the output layers
pub fn compose(
    rgba_data: &[u8],
    width: u32,
    height: u32,
    config: &MrcConfig,
) -> crate::error::Result<MrcLayers> {
    // 1. Segment: RGBA -> 1-bit text mask
    let mut text_mask = segmenter::segment_text_mask(rgba_data, width, height)?;

    // 2. Mask layer: JBIG2-encode the 1-bit mask
    let mask_jbig2 = jbig2::encode_mask(&mut text_mask)?;

    // 3. Background layer: encode original image as JPEG
    //    (v1: no inpainting; text regions are left as-is)
    let background_jpeg = jpeg::encode_rgba_to_jpeg(rgba_data, width, height, config.bg_quality)?;

    // 4. Foreground layer: encode original image as JPEG
    //    (v1: no text-only extraction; full image at fg_quality)
    let foreground_jpeg = jpeg::encode_rgba_to_jpeg(rgba_data, width, height, config.fg_quality)?;

    Ok(MrcLayers {
        mask_jbig2,
        foreground_jpeg,
        background_jpeg,
        width,
        height,
    })
}
