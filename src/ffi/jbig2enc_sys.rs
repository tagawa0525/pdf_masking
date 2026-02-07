// Phase 4: jbig2enc raw FFI declarations for the C shim
//
// The C++ function jbig2_encode_generic is wrapped by a thin C shim
// (csrc/jbig2enc_shim.cpp) that exposes it with extern "C" linkage.

use leptonica_sys::PIX;

unsafe extern "C" {
    /// Encode a 1-bit PIX using JBIG2 generic region encoding.
    ///
    /// This is the C-ABI wrapper compiled from csrc/jbig2enc_shim.cpp.
    ///
    /// # Arguments
    /// * `pix` - Pointer to a 1-bit leptonica PIX
    /// * `duplicate_line_removal` - Enable TPGD (0 or 1)
    /// * `tpl_x` - Template x position (-1 for auto)
    /// * `tpl_y` - Template y position (-1 for auto)
    /// * `use_refinement` - Enable refinement coding (0 or 1)
    /// * `length` - Output: length of returned buffer in bytes
    ///
    /// # Returns
    /// Pointer to a malloc'd buffer containing JBIG2 data, or NULL on failure.
    /// Caller must free() the returned buffer.
    pub fn jbig2enc_encode_generic_c(
        pix: *mut PIX,
        duplicate_line_removal: libc::c_int,
        tpl_x: libc::c_int,
        tpl_y: libc::c_int,
        use_refinement: libc::c_int,
        length: *mut libc::c_int,
    ) -> *mut u8;
}
