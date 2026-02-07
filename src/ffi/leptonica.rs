// Phase 3: 安全ラッパー（Pix型、RAII Drop）

use super::leptonica_sys::{
    pixClone, pixCreate, pixDestroy, pixGetData, pixGetDepth, pixGetHeight, pixGetRegionsBinary,
    pixGetWidth, pixGetWpl, pixOtsuAdaptiveThreshold, PIX,
};
use crate::error::{PdfMaskError, Result};
use std::ptr;

/// Safe wrapper around leptonica's PIX structure
/// Implements RAII pattern for automatic memory management
pub struct Pix {
    ptr: *mut PIX,
}

impl Pix {
    /// Create a new Pix image with specified dimensions and depth
    ///
    /// # Arguments
    /// * `width` - Image width in pixels (must fit in i32)
    /// * `height` - Image height in pixels (must fit in i32)
    /// * `depth` - Bits per pixel (1, 8, 32, etc.; must fit in i32)
    ///
    /// # Returns
    /// `Ok(Pix)` if successful, `Err` on failure
    pub fn create(width: u32, height: u32, depth: u32) -> Result<Self> {
        // Fix #5: Validate u32 values fit in i32 before casting
        if width > i32::MAX as u32 || height > i32::MAX as u32 || depth > i32::MAX as u32 {
            return Err(PdfMaskError::segmentation(format!(
                "Pix dimensions exceed i32::MAX (width={}, height={}, depth={})",
                width, height, depth
            )));
        }

        unsafe {
            let ptr = pixCreate(width as i32, height as i32, depth as i32);
            if ptr.is_null() {
                Err(PdfMaskError::segmentation(format!(
                    "Failed to create Pix image ({}x{}x{})",
                    width, height, depth
                )))
            } else {
                Ok(Pix { ptr })
            }
        }
    }

    /// Create a Pix from raw RGBA data
    ///
    /// # Arguments
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    /// * `data` - Raw RGBA data (4 bytes per pixel)
    ///
    /// # Returns
    /// `Ok(Pix)` if successful, `Err` on failure
    pub fn from_raw_rgba(width: u32, height: u32, data: &[u8]) -> Result<Self> {
        // Fix #3: Use checked_mul to prevent u32 overflow
        let expected_size = width
            .checked_mul(height)
            .and_then(|wh| wh.checked_mul(4))
            .ok_or_else(|| {
                PdfMaskError::segmentation(format!(
                    "Overflow computing buffer size for {}x{} RGBA image",
                    width, height
                ))
            })? as usize;

        if data.len() != expected_size {
            return Err(PdfMaskError::segmentation(format!(
                "Data size mismatch: expected {} bytes, got {}",
                expected_size,
                data.len()
            )));
        }

        // Fix #5: Validate u32 values fit in i32 before casting
        if width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(PdfMaskError::segmentation(format!(
                "Pix dimensions exceed i32::MAX (width={}, height={})",
                width, height
            )));
        }

        unsafe {
            // Create 32-bit Pix from RGBA data
            let ptr = pixCreate(width as i32, height as i32, 32);
            if ptr.is_null() {
                return Err(PdfMaskError::segmentation(format!(
                    "Failed to create Pix image from RGBA ({}x{})",
                    width, height
                )));
            }

            // Fix #4: Check pixGetData for null and destroy PIX on failure
            let pix_data = pixGetData(ptr);
            if pix_data.is_null() {
                let mut ptr_to_destroy = ptr;
                pixDestroy(&mut ptr_to_destroy);
                return Err(PdfMaskError::segmentation(
                    "pixGetData returned null for newly created Pix",
                ));
            }

            // Fix #11: Use byte-level copy to avoid unaligned u32 cast UB
            ptr::copy_nonoverlapping(data.as_ptr(), pix_data as *mut u8, expected_size);

            Ok(Pix { ptr })
        }
    }

    /// Get the width of the image in pixels
    pub fn get_width(&self) -> u32 {
        unsafe { pixGetWidth(self.ptr) as u32 }
    }

    /// Get the height of the image in pixels
    pub fn get_height(&self) -> u32 {
        unsafe { pixGetHeight(self.ptr) as u32 }
    }

    /// Get the depth (bits per pixel) of the image
    pub fn get_depth(&self) -> u32 {
        unsafe { pixGetDepth(self.ptr) as u32 }
    }

    /// Get the words per line (stride) of the image
    pub fn get_wpl(&self) -> u32 {
        unsafe { pixGetWpl(self.ptr) as u32 }
    }

    /// Apply Otsu adaptive thresholding to create a binary image
    ///
    /// # Arguments
    /// * `sx` - Size of local region in x direction
    /// * `sy` - Size of local region in y direction
    ///
    /// # Returns
    /// `Ok(Pix)` containing the binary result, `Err` on failure
    pub fn otsu_adaptive_threshold(&self, sx: u32, sy: u32) -> Result<Pix> {
        unsafe {
            let mut pix_threshold: *mut PIX = ptr::null_mut();
            let mut pix_result: *mut PIX = ptr::null_mut();
            let result = pixOtsuAdaptiveThreshold(
                self.ptr,
                sx as i32,
                sy as i32,
                0,   // smoothx
                0,   // smoothy
                0.0, // scorefract
                &mut pix_threshold,
                &mut pix_result,
            );

            if result != 0 || pix_result.is_null() {
                if !pix_threshold.is_null() {
                    pixDestroy(&mut pix_threshold);
                }
                Err(PdfMaskError::segmentation(
                    "Failed to apply Otsu adaptive threshold",
                ))
            } else {
                // Clean up the threshold intermediate result
                if !pix_threshold.is_null() {
                    let mut pix_th = pix_threshold;
                    pixDestroy(&mut pix_th);
                }
                Ok(Pix { ptr: pix_result })
            }
        }
    }

    /// Get region masks from binary image
    ///
    /// # Returns
    /// `Ok(Vec<Pix>)` containing mask for each region, `Err` on failure
    pub fn get_region_masks(&self) -> Result<Vec<Pix>> {
        unsafe {
            let mut pix_hm: *mut PIX = ptr::null_mut();
            let mut pix_tm: *mut PIX = ptr::null_mut();
            let mut pix_tb: *mut PIX = ptr::null_mut();
            let result = pixGetRegionsBinary(
                self.ptr,
                &mut pix_hm,
                &mut pix_tm,
                &mut pix_tb,
                ptr::null_mut(),
            );

            // Fix #12: Clean up partially created PIX on failure
            if result != 0 {
                if !pix_hm.is_null() {
                    pixDestroy(&mut pix_hm);
                }
                if !pix_tm.is_null() {
                    pixDestroy(&mut pix_tm);
                }
                if !pix_tb.is_null() {
                    pixDestroy(&mut pix_tb);
                }
                return Err(PdfMaskError::segmentation("Failed to get region masks"));
            }

            // Collect the three output masks if they were created
            let mut masks = Vec::new();
            if !pix_hm.is_null() {
                masks.push(Pix { ptr: pix_hm });
            }
            if !pix_tm.is_null() {
                masks.push(Pix { ptr: pix_tm });
            }
            if !pix_tb.is_null() {
                masks.push(Pix { ptr: pix_tb });
            }

            Ok(masks)
        }
    }

    /// Create a refcounted alias of this Pix via leptonica's pixClone.
    ///
    /// This does **not** perform a deep copy. The returned `Pix` shares the
    /// same underlying pixel data through leptonica's internal reference count.
    /// Each `Pix` (original and alias) must still be dropped independently;
    /// leptonica will free the backing memory only when the last reference is
    /// destroyed.
    ///
    /// # Returns
    /// `Ok(Pix)` containing the refcounted alias, `Err` on failure
    pub fn leptonica_clone(&self) -> Result<Pix> {
        unsafe {
            let ptr = pixClone(self.ptr);
            if ptr.is_null() {
                Err(PdfMaskError::segmentation("Failed to clone Pix"))
            } else {
                Ok(Pix { ptr })
            }
        }
    }
}

impl Drop for Pix {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                pixDestroy(&mut self.ptr);
            }
        }
    }
}

// Fix #7: Pix is not Clone at the Rust level, but we support leptonica's
// refcounted clone via the `leptonica_clone()` method. The Rust Clone trait
// would imply an independent deep copy, which pixClone does not provide.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pix_create_basic() {
        let result = Pix::create(100, 100, 8);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pix_get_width_height() {
        let pix = Pix::create(123, 456, 8).unwrap();
        assert_eq!(pix.get_width(), 123);
        assert_eq!(pix.get_height(), 456);
    }

    #[test]
    fn test_pix_create_overflow_width() {
        // u32::MAX exceeds i32::MAX
        let result = Pix::create(u32::MAX, 100, 8);
        assert!(result.is_err());
    }

    #[test]
    fn test_pix_from_raw_rgba_overflow() {
        // Dimensions that would overflow u32 when multiplied
        let result = Pix::from_raw_rgba(u32::MAX, 2, &[]);
        assert!(result.is_err());
    }
}
