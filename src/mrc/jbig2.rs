// Phase 5: jbig2enc wrapper: 1-bit mask -> JBIG2 bytes

use crate::ffi::jbig2enc;
use crate::ffi::leptonica::Pix;

/// Encode a 1-bit text mask into JBIG2 format.
///
/// Delegates to the jbig2enc FFI binding for generic-region encoding.
///
/// # Arguments
/// * `mask` - A mutable reference to a 1-bit `Pix` (required by the FFI layer)
pub fn encode_mask(mask: &mut Pix) -> crate::error::Result<Vec<u8>> {
    jbig2enc::encode_generic(mask)
}
