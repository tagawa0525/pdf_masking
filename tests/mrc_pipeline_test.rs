// Phase 5: MRC pipeline integration tests
//
// Tests for the MRC pipeline: segmenter, jbig2, jpeg, compositor.
// Each test verifies a specific component of the MRC layer generation pipeline.

use pdf_masking::ffi::leptonica::Pix;
use pdf_masking::mrc::{compositor, jbig2, jpeg, segmenter};

/// Generate a synthetic 200x200 RGBA test image.
/// Upper half: black (text-like region), lower half: white (background).
fn create_test_rgba_image() -> (Vec<u8>, u32, u32) {
    let width: u32 = 200;
    let height: u32 = 200;
    let mut data = vec![0u8; (width * height * 4) as usize];

    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            if y < height / 2 {
                // Upper half: black pixels (simulating text)
                data[offset] = 0; // R
                data[offset + 1] = 0; // G
                data[offset + 2] = 0; // B
                data[offset + 3] = 255; // A
            } else {
                // Lower half: white pixels (background)
                data[offset] = 255; // R
                data[offset + 1] = 255; // G
                data[offset + 2] = 255; // B
                data[offset + 3] = 255; // A
            }
        }
    }

    (data, width, height)
}

// ---- segmenter.rs tests ----

/// Test that segment_text_mask produces a Pix from RGBA input.
#[test]
fn test_segment_creates_text_mask() {
    let (data, width, height) = create_test_rgba_image();

    let result = segmenter::segment_text_mask(&data, width, height);
    assert!(
        result.is_ok(),
        "segment_text_mask failed: {:?}",
        result.err()
    );

    let mask = result.unwrap();
    assert_eq!(mask.get_width(), width);
    assert_eq!(mask.get_height(), height);
}

/// Test that the text mask is 1-bit depth.
#[test]
fn test_segment_mask_is_1bit() {
    let (data, width, height) = create_test_rgba_image();

    let mask = segmenter::segment_text_mask(&data, width, height)
        .expect("segment_text_mask should succeed");

    assert_eq!(mask.get_depth(), 1, "Text mask should be 1-bit depth");
}

// ---- jbig2.rs tests ----

/// Test encoding a 1-bit mask to JBIG2 format.
#[test]
fn test_encode_mask_to_jbig2() {
    // Create a simple 1-bit mask
    let mut mask = Pix::create(100, 100, 1).expect("failed to create 1-bit Pix");
    mask.set_all_pixels(1).expect("failed to set pixels");

    let result = jbig2::encode_mask(&mut mask);
    assert!(result.is_ok(), "encode_mask failed: {:?}", result.err());
}

/// Test that JBIG2 encoding produces non-empty output.
#[test]
fn test_encode_returns_non_empty() {
    let mut mask = Pix::create(100, 100, 1).expect("failed to create 1-bit Pix");

    let data = jbig2::encode_mask(&mut mask).expect("encode_mask should succeed");
    assert!(!data.is_empty(), "JBIG2 encoded data should not be empty");
}

// ---- jpeg.rs tests ----

/// Test encoding a background RGBA image to JPEG format.
#[test]
fn test_encode_background_jpeg() {
    let (data, width, height) = create_test_rgba_image();

    let result = jpeg::encode_rgba_to_jpeg(&data, width, height, 75);
    assert!(
        result.is_ok(),
        "encode_rgba_to_jpeg failed: {:?}",
        result.err()
    );

    let jpeg_data = result.unwrap();
    assert!(!jpeg_data.is_empty(), "JPEG data should not be empty");
    // JPEG files start with FF D8
    assert_eq!(
        &jpeg_data[..2],
        &[0xFF, 0xD8],
        "JPEG data should start with FF D8 marker"
    );
}

/// Test encoding a foreground RGBA image to JPEG format.
#[test]
fn test_encode_foreground_jpeg() {
    // Create a mostly-white image (foreground with text removed)
    let width: u32 = 100;
    let height: u32 = 100;
    let data = vec![255u8; (width * height * 4) as usize];

    let result = jpeg::encode_rgba_to_jpeg(&data, width, height, 50);
    assert!(
        result.is_ok(),
        "encode_rgba_to_jpeg for foreground failed: {:?}",
        result.err()
    );

    let jpeg_data = result.unwrap();
    assert!(!jpeg_data.is_empty(), "Foreground JPEG should not be empty");
}

// ---- compositor.rs tests ----

/// Test the full MRC pipeline: RGBA bitmap + config -> MrcLayers.
#[test]
fn test_compose_mrc_layers() {
    let (data, width, height) = create_test_rgba_image();
    let config = compositor::MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };

    let result = compositor::compose(&data, width, height, &config);
    assert!(result.is_ok(), "compose failed: {:?}", result.err());

    let layers = result.unwrap();
    assert_eq!(layers.width, width);
    assert_eq!(layers.height, height);
}

/// Test that all three MRC layers are non-empty.
#[test]
fn test_mrc_layers_has_all_components() {
    let (data, width, height) = create_test_rgba_image();
    let config = compositor::MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };

    let layers =
        compositor::compose(&data, width, height, &config).expect("compose should succeed");

    assert!(
        !layers.mask_jbig2.is_empty(),
        "JBIG2 mask layer should not be empty"
    );
    assert!(
        !layers.foreground_jpeg.is_empty(),
        "Foreground JPEG layer should not be empty"
    );
    assert!(
        !layers.background_jpeg.is_empty(),
        "Background JPEG layer should not be empty"
    );
}
