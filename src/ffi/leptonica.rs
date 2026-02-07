// Phase 3: 安全ラッパー（Pix型、RAII Drop）

use super::leptonica_sys::{
    pixClone, pixCreate, pixDestroy, pixGetData, pixGetDepth, pixGetHeight, pixGetRegionsBinary,
    pixGetWidth, pixGetWpl, pixOtsuAdaptiveThreshold, PIX,
};
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
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    /// * `depth` - Bits per pixel (1, 8, 32, etc.)
    ///
    /// # Returns
    /// `Ok(Pix)` if successful, `Err(String)` on failure
    pub fn create(width: u32, height: u32, depth: u32) -> Result<Self, String> {
        unsafe {
            let ptr = pixCreate(width as i32, height as i32, depth as i32);
            if ptr.is_null() {
                Err(format!(
                    "Failed to create Pix image ({}x{}x{})",
                    width, height, depth
                ))
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
    /// `Ok(Pix)` if successful, `Err(String)` on failure
    pub fn from_raw_rgba(width: u32, height: u32, data: &[u8]) -> Result<Self, String> {
        let expected_size = (width * height * 4) as usize;
        if data.len() != expected_size {
            return Err(format!(
                "Data size mismatch: expected {} bytes, got {}",
                expected_size,
                data.len()
            ));
        }

        unsafe {
            // Create 32-bit Pix from RGBA data
            let ptr = pixCreate(width as i32, height as i32, 32);
            if ptr.is_null() {
                return Err(format!(
                    "Failed to create Pix image from RGBA ({}x{})",
                    width, height
                ));
            }

            // Get the Pix data buffer and copy RGBA data
            let pix_data = pixGetData(ptr);
            if !pix_data.is_null() {
                ptr::copy_nonoverlapping(data.as_ptr() as *const u32, pix_data, expected_size / 4);
            }

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
    /// `Ok(Pix)` containing the binary result, `Err(String)` on failure
    pub fn otsu_adaptive_threshold(&self, sx: u32, sy: u32) -> Result<Pix, String> {
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
                Err("Failed to apply Otsu adaptive threshold".to_string())
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
    /// `Ok(Vec<Pix>)` containing mask for each region, `Err(String)` on failure
    pub fn get_region_masks(&self) -> Result<Vec<Pix>, String> {
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

            if result != 0 {
                return Err("Failed to get region masks".to_string());
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

    /// Clone this Pix image (wrapper around leptonica's pixClone)
    ///
    /// # Returns
    /// `Ok(Pix)` containing the cloned image, `Err(String)` on failure
    pub fn leptonica_clone(&self) -> Result<Pix, String> {
        unsafe {
            let ptr = pixClone(self.ptr);
            if ptr.is_null() {
                Err("Failed to clone Pix".to_string())
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

// Pix is not Clone at the Rust level, but we support leptonica's clone operation
// The Rust Clone trait would require two independent Pix instances
// For now, users should use the .clone() method to clone leptonica images

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
}
