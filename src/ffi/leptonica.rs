// Phase 3: 安全ラッパー（Pix型、RAII Drop）

use super::leptonica_sys::{
    BOX, BOXA, L_CLONE, PIX, boxDestroy, boxGetGeometry, boxaDestroy, boxaGetBox, boxaGetCount,
    pixClone, pixConnCompBB, pixConvertRGBToGray, pixCreate, pixDestroy, pixGetData, pixGetDepth,
    pixGetHeight, pixGetRegionsBinary, pixGetWidth, pixGetWpl, pixOtsuAdaptiveThreshold, pixSetAll,
};
use crate::error::{PdfMaskError, Result};
use std::ptr;

/// Named result from `pixGetRegionsBinary`.
///
/// Each field is `None` when leptonica returned a NULL pointer for that
/// region type (e.g. the image contains no halftone regions).
pub struct RegionMasks {
    /// Halftone (image) region mask
    pub halftone: Option<Pix>,
    /// Textline region mask
    pub textline: Option<Pix>,
    /// Text block region mask
    pub block: Option<Pix>,
}

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
                    pixDestroy(&mut pix_threshold);
                }
                Ok(Pix { ptr: pix_result })
            }
        }
    }

    /// Get region masks from binary image
    ///
    /// Returns a [`RegionMasks`] with named fields for each mask type.
    /// Each field is `None` when leptonica returns a NULL pointer for that
    /// region (e.g., no halftone regions detected).
    ///
    /// # Returns
    /// `Ok(RegionMasks)` on success, `Err` on failure
    pub fn get_region_masks(&self) -> Result<RegionMasks> {
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

            let halftone = if pix_hm.is_null() {
                None
            } else {
                Some(Pix { ptr: pix_hm })
            };
            let textline = if pix_tm.is_null() {
                None
            } else {
                Some(Pix { ptr: pix_tm })
            };
            let block = if pix_tb.is_null() {
                None
            } else {
                Some(Pix { ptr: pix_tb })
            };

            Ok(RegionMasks {
                halftone,
                textline,
                block,
            })
        }
    }

    /// Extract bounding boxes of connected components from a 1-bit image.
    ///
    /// Wraps leptonica's `pixConnCompBB`. Returns a list of `(x, y, w, h)`
    /// rectangles, one per connected component.
    ///
    /// # Arguments
    /// * `connectivity` - 4 or 8 (4-connected or 8-connected)
    ///
    /// # Errors
    /// Returns an error if the image is not 1 bpp or if leptonica fails.
    pub fn connected_component_bboxes(
        &self,
        connectivity: i32,
    ) -> Result<Vec<(u32, u32, u32, u32)>> {
        if self.get_depth() != 1 {
            return Err(PdfMaskError::segmentation(format!(
                "connected_component_bboxes requires 1-bit image, got {}-bit",
                self.get_depth()
            )));
        }

        unsafe {
            let boxa: *mut BOXA = pixConnCompBB(self.ptr, connectivity);
            if boxa.is_null() {
                return Err(PdfMaskError::segmentation("pixConnCompBB returned null"));
            }

            let count = boxaGetCount(boxa);
            let mut result = Vec::with_capacity(count as usize);

            for i in 0..count {
                let box_ptr: *mut BOX = boxaGetBox(boxa, i, L_CLONE as i32);
                if box_ptr.is_null() {
                    boxaDestroy(&mut (boxa as *mut BOXA));
                    return Err(PdfMaskError::segmentation(format!(
                        "boxaGetBox returned null for index {}",
                        i
                    )));
                }

                let mut x: i32 = 0;
                let mut y: i32 = 0;
                let mut w: i32 = 0;
                let mut h: i32 = 0;
                let ret = boxGetGeometry(box_ptr, &mut x, &mut y, &mut w, &mut h);
                boxDestroy(&mut (box_ptr as *mut BOX));

                if ret != 0 {
                    boxaDestroy(&mut (boxa as *mut BOXA));
                    return Err(PdfMaskError::segmentation(format!(
                        "boxGetGeometry failed for index {}",
                        i
                    )));
                }

                result.push((x as u32, y as u32, w as u32, h as u32));
            }

            boxaDestroy(&mut (boxa as *mut BOXA));
            Ok(result)
        }
    }

    /// Return the raw mutable pointer to the underlying PIX.
    ///
    /// # Safety
    /// - The caller must not use this pointer after the `Pix` is dropped.
    /// - The caller must ensure no aliasing violations occur while the
    ///   pointer is in use.
    pub(crate) unsafe fn as_mut_ptr(&mut self) -> *mut PIX {
        self.ptr
    }

    /// Set all pixels in the image to the given value.
    ///
    /// Only 1-bit images are supported. `value = 1` sets all pixels to black
    /// (foreground) via leptonica's `pixSetAll`. `value = 0` is a no-op because
    /// PIX data is zero-initialized at creation time.
    ///
    /// # Errors
    /// Returns an error if `depth != 1` or `value > 1`.
    pub fn set_all_pixels(&mut self, value: u32) -> Result<()> {
        if self.get_depth() != 1 {
            return Err(PdfMaskError::segmentation(format!(
                "set_all_pixels only supports 1-bit images, got {}-bit",
                self.get_depth()
            )));
        }
        if value > 1 {
            return Err(PdfMaskError::segmentation(format!(
                "set_all_pixels value must be 0 or 1 for 1-bit images, got {}",
                value
            )));
        }
        if value == 0 {
            // PIX is zero-initialized at creation; nothing to do
            return Ok(());
        }
        unsafe {
            let ret = pixSetAll(self.ptr);
            if ret != 0 {
                return Err(PdfMaskError::segmentation("pixSetAll failed"));
            }
        }
        Ok(())
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

    /// Convert a 32-bit RGB(A) Pix to an 8-bit grayscale Pix.
    ///
    /// Uses default luminance weights (0.3 R, 0.59 G, 0.11 B) when all
    /// weight parameters are zero.
    ///
    /// # Errors
    /// Returns an error if the input image is not 32 bpp or if the
    /// underlying leptonica conversion fails.
    ///
    /// # Returns
    /// `Ok(Pix)` containing the 8-bit grayscale image, `Err` on failure
    pub fn convert_to_gray(&self) -> Result<Pix> {
        if self.get_depth() != 32 {
            return Err(PdfMaskError::segmentation(format!(
                "convert_to_gray requires 32-bit input, got {}-bit",
                self.get_depth()
            )));
        }
        unsafe {
            let ptr = pixConvertRGBToGray(self.ptr, 0.0, 0.0, 0.0);
            if ptr.is_null() {
                Err(PdfMaskError::segmentation(
                    "Failed to convert Pix to grayscale",
                ))
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
