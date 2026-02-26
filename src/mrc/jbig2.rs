// Phase 5: jbig2enc wrapper: 1-bit mask -> JBIG2 bytes

#[cfg(feature = "mrc")]
use leptonica::Pix;

/// Encode a 1-bit text mask into JBIG2 format.
///
/// Uses pure Rust jbig2enc crate for generic-region encoding.
/// Parameters: `full_headers=false` (PDF fragment), `xres/yres=0` (auto),
/// `duplicate_line_removal=true` (TPGD).
///
/// # Arguments
/// * `mask` - A 1-bit `Pix` image
#[cfg(feature = "mrc")]
pub fn encode_mask(mask: &Pix) -> crate::error::Result<Vec<u8>> {
    use crate::error::PdfMaskError;
    use jbig2enc::encoder::encode_generic;

    encode_generic(mask, false, 0, 0, true).map_err(|e| PdfMaskError::jbig2_encode(e.to_string()))
}
