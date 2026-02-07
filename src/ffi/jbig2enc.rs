// Phase 4: Safe wrapper around jbig2enc FFI

use super::jbig2enc_sys;
use crate::error::PdfMaskError;
use crate::ffi::leptonica::Pix;

/// Encode a 1-bit PIX using JBIG2 generic region encoding.
///
/// The PIX must be 1-bit depth. The function uses duplicate line removal
/// (TPGD) and automatic template positioning for good compression.
///
/// # Arguments
/// * `pix` - A 1-bit leptonica Pix image
///
/// # Returns
/// `Ok(Vec<u8>)` containing the JBIG2 encoded data, or `Err` on failure.
pub fn encode_generic(pix: &Pix) -> crate::error::Result<Vec<u8>> {
    if pix.get_depth() != 1 {
        return Err(PdfMaskError::jbig2_encode(format!(
            "JBIG2 encoding requires 1-bit PIX, got {}-bit",
            pix.get_depth()
        )));
    }

    let mut length: libc::c_int = 0;

    let buf = unsafe {
        jbig2enc_sys::jbig2enc_encode_generic_c(
            pix.as_mut_ptr(),
            1,  // duplicate_line_removal (TPGD)
            -1, // tpl_x (auto)
            -1, // tpl_y (auto)
            0,  // use_refinement (off for generic encoding)
            &mut length,
        )
    };

    if buf.is_null() || length <= 0 {
        return Err(PdfMaskError::jbig2_encode(
            "jbig2_encode_generic returned NULL or zero-length buffer",
        ));
    }

    // Copy the malloc'd buffer into a Vec<u8> and free the original
    let result = unsafe {
        let slice = std::slice::from_raw_parts(buf, length as usize);
        let vec = slice.to_vec();
        libc::free(buf as *mut libc::c_void);
        vec
    };

    Ok(result)
}
