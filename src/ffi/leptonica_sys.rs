// Phase 3: Hand-written FFI bindings for leptonica.
// We avoid leptonica-sys because bindgen fails on NixOS with __mbstate_t_union_.
// Only the minimal set of functions needed for MRC segmentation is exposed here.

use libc::{c_float, c_int, c_void};

/// Opaque representation of leptonica's PIX structure.
/// We never access internal fields; all interaction goes through the C API.
#[repr(C)]
pub struct PIX {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    // --- Creation and destruction ---

    pub fn pixCreate(width: c_int, height: c_int, depth: c_int) -> *mut PIX;
    pub fn pixDestroy(ppix: *mut *mut PIX);
    pub fn pixClone(pix: *mut PIX) -> *mut PIX;

    // --- Properties ---

    pub fn pixGetWidth(pix: *const PIX) -> c_int;
    pub fn pixGetHeight(pix: *const PIX) -> c_int;
    pub fn pixGetDepth(pix: *const PIX) -> c_int;
    pub fn pixGetWpl(pix: *const PIX) -> c_int;
    pub fn pixGetData(pix: *mut PIX) -> *mut u32;

    // --- Binarization ---

    pub fn pixOtsuAdaptiveThreshold(
        pixs: *const PIX,
        sx: c_int,
        sy: c_int,
        smoothx: c_int,
        smoothy: c_int,
        scorefract: c_float,
        ppixth: *mut *mut PIX,
        ppixd: *mut *mut PIX,
    ) -> c_int;

    // --- Region segmentation ---

    pub fn pixGetRegionsBinary(
        pixs: *const PIX,
        ppixhm: *mut *mut PIX,
        ppixtext: *mut *mut PIX,
        ppixb: *mut *mut PIX,
        pixadb: *mut c_void,
    ) -> c_int;

    // --- Colour conversion ---

    pub fn pixConvertRGBToGray(
        pixs: *const PIX,
        rwt: c_float,
        gwt: c_float,
        bwt: c_float,
    ) -> *mut PIX;
}
