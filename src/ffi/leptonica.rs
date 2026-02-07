// Phase 3: Safe Rust wrapper around leptonica's PIX type.
// All unsafe FFI calls are contained within this module.

use super::leptonica_sys as sys;
use crate::error::{PdfMaskError, Result};

/// RAII wrapper for leptonica's `PIX` type.
///
/// `Drop` automatically calls `pixDestroy`, which decrements the
/// reference count and frees memory when the count reaches zero.
pub struct Pix {
    ptr: *mut sys::PIX,
}

// PIX is internally reference-counted.  We only use it from a single thread,
// so Send is safe.  We do NOT implement Sync because concurrent mutation
// through the raw pointer would be undefined behaviour.
unsafe impl Send for Pix {}

impl Drop for Pix {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                sys::pixDestroy(&mut self.ptr);
            }
            // pixDestroy sets the pointer to null, but we do it explicitly
            // for defensive safety.
            self.ptr = std::ptr::null_mut();
        }
    }
}

impl Pix {
    // ------------------------------------------------------------------
    // Constructors
    // ------------------------------------------------------------------

    /// Create an empty PIX with the given dimensions and bit depth.
    pub fn new(width: u32, height: u32, depth: u32) -> Result<Self> {
        let ptr = unsafe { sys::pixCreate(width as i32, height as i32, depth as i32) };
        if ptr.is_null() {
            return Err(PdfMaskError::segmentation("Failed to create PIX"));
        }
        Ok(Self { ptr })
    }

    /// Build a 32-bit PIX from a packed RGBA byte buffer
    /// (`width * height * 4` bytes, row-major, R-G-B-A order).
    pub fn from_raw_rgba(data: &[u8], width: u32, height: u32) -> Result<Self> {
        let expected = (width as usize) * (height as usize) * 4;
        if data.len() != expected {
            return Err(PdfMaskError::segmentation(format!(
                "RGBA data length mismatch: expected {}, got {}",
                expected,
                data.len()
            )));
        }

        let pix = Self::new(width, height, 32)?;

        unsafe {
            let pix_data = sys::pixGetData(pix.ptr);
            let wpl = sys::pixGetWpl(pix.ptr as *const _) as usize;

            // Copy RGBA bytes into leptonica pixel words.
            // leptonica stores 32-bit pixels as 0xRRGGBBAA in a native-endian
            // u32 word.
            for y in 0..height as usize {
                let line = pix_data.add(y * wpl);
                for x in 0..width as usize {
                    let src_idx = (y * width as usize + x) * 4;
                    let r = data[src_idx] as u32;
                    let g = data[src_idx + 1] as u32;
                    let b = data[src_idx + 2] as u32;
                    let a = data[src_idx + 3] as u32;
                    let pixel = (r << 24) | (g << 16) | (b << 8) | a;
                    *line.add(x) = pixel;
                }
            }
        }

        Ok(pix)
    }

    // ------------------------------------------------------------------
    // Properties
    // ------------------------------------------------------------------

    /// Image width in pixels.
    pub fn width(&self) -> u32 {
        unsafe { sys::pixGetWidth(self.ptr as *const _) as u32 }
    }

    /// Image height in pixels.
    pub fn height(&self) -> u32 {
        unsafe { sys::pixGetHeight(self.ptr as *const _) as u32 }
    }

    /// Bit depth (1, 2, 4, 8, 16, or 32).
    pub fn depth(&self) -> u32 {
        unsafe { sys::pixGetDepth(self.ptr as *const _) as u32 }
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Perform Otsu adaptive binarisation and return a 1-bit PIX.
    ///
    /// The input should be an 8-bit grayscale image.  If the current PIX is
    /// 32-bit, it is first converted to grayscale internally.
    pub fn binarize(&self) -> Result<Pix> {
        // pixOtsuAdaptiveThreshold requires an 8-bit grayscale input.
        // If we are 32-bit, convert first.
        let gray: Option<Pix>;
        let src_ptr: *const sys::PIX;

        if self.depth() == 32 {
            let g = unsafe { sys::pixConvertRGBToGray(self.ptr as *const _, 0.0, 0.0, 0.0) };
            if g.is_null() {
                return Err(PdfMaskError::segmentation("pixConvertRGBToGray failed"));
            }
            gray = Some(Pix { ptr: g });
            src_ptr = gray.as_ref().unwrap().ptr as *const _;
        } else {
            gray = None;
            src_ptr = self.ptr as *const _;
        }

        let mut pix_threshold: *mut sys::PIX = std::ptr::null_mut();
        let mut pix_binary: *mut sys::PIX = std::ptr::null_mut();

        let ret = unsafe {
            sys::pixOtsuAdaptiveThreshold(
                src_ptr,
                2000,
                2000, // sx, sy: large tile = effectively full-page (min 16)
                0,
                0,   // smoothx, smoothy
                0.0, // scorefract
                &mut pix_threshold,
                &mut pix_binary,
            )
        };

        // Threshold image is not needed; free it immediately.
        if !pix_threshold.is_null() {
            unsafe {
                sys::pixDestroy(&mut pix_threshold);
            }
        }

        // Drop the temporary grayscale image (if created).
        drop(gray);

        if ret != 0 || pix_binary.is_null() {
            return Err(PdfMaskError::segmentation("Otsu binarization failed"));
        }

        Ok(Pix { ptr: pix_binary })
    }

    /// Run `pixGetRegionsBinary` and return
    /// `(halftone_mask, textline_mask, block_mask)`.
    ///
    /// Each mask is a 1-bit PIX.  If a particular mask is empty, an
    /// all-zero PIX of the same dimensions is returned instead.
    ///
    /// If the input is not 1-bpp, it is binarised first (leptonica
    /// requires a 1-bpp input for this function).
    pub fn get_region_masks(&self) -> Result<(Pix, Pix, Pix)> {
        // pixGetRegionsBinary requires 1-bpp input.
        let binary: Option<Pix>;
        let src_ptr: *const sys::PIX;

        if self.depth() != 1 {
            let b = self.binarize()?;
            src_ptr = b.ptr as *const _;
            binary = Some(b);
        } else {
            binary = None;
            src_ptr = self.ptr as *const _;
        }

        let mut pix_hm: *mut sys::PIX = std::ptr::null_mut();
        let mut pix_text: *mut sys::PIX = std::ptr::null_mut();
        let mut pix_block: *mut sys::PIX = std::ptr::null_mut();

        let ret = unsafe {
            sys::pixGetRegionsBinary(
                src_ptr,
                &mut pix_hm,
                &mut pix_text,
                &mut pix_block,
                std::ptr::null_mut(), // debug pixa -- not needed
            )
        };

        // Drop temporary binary image after the FFI call.
        drop(binary);

        if ret != 0 {
            // Clean up any partially-allocated masks.
            unsafe {
                if !pix_hm.is_null() {
                    sys::pixDestroy(&mut pix_hm);
                }
                if !pix_text.is_null() {
                    sys::pixDestroy(&mut pix_text);
                }
                if !pix_block.is_null() {
                    sys::pixDestroy(&mut pix_block);
                }
            }
            return Err(PdfMaskError::segmentation("pixGetRegionsBinary failed"));
        }

        // Any NULL result means "no mask found" -- substitute an empty PIX.
        let hm = if pix_hm.is_null() {
            Pix::new(self.width(), self.height(), 1)?
        } else {
            Pix { ptr: pix_hm }
        };
        let text = if pix_text.is_null() {
            Pix::new(self.width(), self.height(), 1)?
        } else {
            Pix { ptr: pix_text }
        };
        let block = if pix_block.is_null() {
            Pix::new(self.width(), self.height(), 1)?
        } else {
            Pix { ptr: pix_block }
        };

        Ok((hm, text, block))
    }

    /// Increment the reference count and return a new handle to the same PIX.
    ///
    /// This is a *shallow* clone: both handles share pixel data.  The
    /// underlying memory is freed only when the last handle is dropped.
    pub fn clone_pix(&self) -> Result<Pix> {
        let ptr = unsafe { sys::pixClone(self.ptr as *mut _) };
        if ptr.is_null() {
            return Err(PdfMaskError::segmentation("pixClone failed"));
        }
        Ok(Pix { ptr })
    }

    // ------------------------------------------------------------------
    // Raw-pointer access (for downstream FFI callers)
    // ------------------------------------------------------------------

    /// Return an immutable raw pointer to the underlying PIX.
    #[allow(dead_code)]
    pub fn as_ptr(&self) -> *const sys::PIX {
        self.ptr as *const _
    }

    /// Return a mutable raw pointer to the underlying PIX.
    #[allow(dead_code)]
    pub fn as_mut_ptr(&mut self) -> *mut sys::PIX {
        self.ptr
    }
}
