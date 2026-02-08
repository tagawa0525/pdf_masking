// Phase 5: leptonica segmentation: bitmap -> mask/fg/bg separation

use crate::ffi::leptonica::Pix;

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
