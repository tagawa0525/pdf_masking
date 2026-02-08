// Phase 5: image crate: fg/bg -> JPEG bytes

use crate::error::PdfMaskError;
use image::{DynamicImage, RgbImage, RgbaImage};
use std::io::Cursor;

/// Encode raw RGBA pixel data to JPEG bytes.
///
/// Converts RGBA to RGB (dropping the alpha channel) and compresses with
/// the specified quality (1-100).
///
/// # Arguments
/// * `rgba_data` - Raw RGBA pixel data (4 bytes per pixel)
/// * `width`     - Image width in pixels
/// * `height`    - Image height in pixels
/// * `quality`   - JPEG quality (1 = worst, 100 = best)
pub fn encode_rgba_to_jpeg(
    rgba_data: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> crate::error::Result<Vec<u8>> {
    if !(1..=100).contains(&quality) {
        return Err(PdfMaskError::jpeg_encode(format!(
            "JPEG quality must be 1-100, got {}",
            quality
        )));
    }

    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|wh| wh.checked_mul(4))
        .ok_or_else(|| {
            PdfMaskError::jpeg_encode(format!(
                "Overflow computing buffer size for {}x{} RGBA image",
                width, height
            ))
        })?;

    if rgba_data.len() != expected_len {
        return Err(PdfMaskError::jpeg_encode(format!(
            "RGBA data size mismatch: expected {} bytes, got {}",
            expected_len,
            rgba_data.len()
        )));
    }

    let img = RgbaImage::from_raw(width, height, rgba_data.to_vec())
        .ok_or_else(|| PdfMaskError::jpeg_encode("Failed to create image from RGBA data"))?;

    let dynamic = DynamicImage::ImageRgba8(img);
    let rgb = dynamic.to_rgb8();

    encode_rgb_to_jpeg(&rgb, quality)
}

/// Encode an already-converted RGB image to JPEG bytes.
///
/// This is an internal helper used when the caller has already performed the
/// RGBA-to-RGB conversion and wants to encode at a given quality without
/// repeating that conversion.
pub(crate) fn encode_rgb_to_jpeg(rgb: &RgbImage, quality: u8) -> crate::error::Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    rgb.write_with_encoder(encoder)?;

    Ok(buf.into_inner())
}
