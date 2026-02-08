// Phase 5: MRC pipeline integration tests
//
// Tests for the MRC pipeline: segmenter, jbig2, jpeg, compositor.
// Each test verifies a specific component of the MRC layer generation pipeline.

use pdf_masking::config::job::ColorMode;
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
    assert!(
        jpeg_data.starts_with(&[0xFF, 0xD8]),
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

/// Test encoding a grayscale image to JPEG format.
#[test]
fn test_encode_gray_to_jpeg() {
    let width: u32 = 100;
    let height: u32 = 100;
    let gray_data = vec![128u8; (width * height) as usize];
    let gray_img = image::GrayImage::from_raw(width, height, gray_data).expect("create GrayImage");

    let result = jpeg::encode_gray_to_jpeg(&gray_img, 75);
    assert!(
        result.is_ok(),
        "encode_gray_to_jpeg failed: {:?}",
        result.err()
    );

    let jpeg_data = result.unwrap();
    assert!(!jpeg_data.is_empty(), "Grayscale JPEG should not be empty");
    // JPEG files start with FF D8
    assert!(
        jpeg_data.starts_with(&[0xFF, 0xD8]),
        "Grayscale JPEG should start with FF D8 marker"
    );
}

/// Test that grayscale JPEG is smaller than RGB JPEG for same dimensions.
#[test]
fn test_gray_jpeg_smaller_than_rgb() {
    let width: u32 = 200;
    let height: u32 = 200;

    // Create RGB image (all gray pixels, but 3 channels)
    let rgb_data: Vec<u8> = (0..width * height)
        .flat_map(|_| vec![128u8, 128, 128])
        .collect();
    let rgb_img = image::RgbImage::from_raw(width, height, rgb_data).expect("create RgbImage");
    let rgb_jpeg = jpeg::encode_rgb_to_jpeg(&rgb_img, 50).expect("encode RGB");

    // Create grayscale image (same content, 1 channel)
    let gray_data = vec![128u8; (width * height) as usize];
    let gray_img = image::GrayImage::from_raw(width, height, gray_data).expect("create GrayImage");
    let gray_jpeg = jpeg::encode_gray_to_jpeg(&gray_img, 50).expect("encode gray");

    assert!(
        gray_jpeg.len() < rgb_jpeg.len(),
        "Grayscale JPEG ({} bytes) should be smaller than RGB JPEG ({} bytes)",
        gray_jpeg.len(),
        rgb_jpeg.len()
    );
}

// ---- segmenter::extract_text_bboxes tests ----

/// Test that extract_text_bboxes returns bboxes for a mask with content.
#[test]
fn test_extract_text_bboxes_with_content() {
    let w: u32 = 100;
    let h: u32 = 100;
    let mut mask = Pix::create(w, h, 1).expect("create 1-bit Pix");

    // Create a 20x10 block (large enough to pass the 4x4 filter)
    for y in 10..20 {
        for x in 20..40 {
            mask.set_pixel(x, y, 1).expect("set pixel");
        }
    }

    let bboxes = segmenter::extract_text_bboxes(&mask, 0).expect("extract_text_bboxes");
    assert_eq!(bboxes.len(), 1, "Should find exactly one bbox");
    assert_eq!(bboxes[0].x, 20);
    assert_eq!(bboxes[0].y, 10);
    assert_eq!(bboxes[0].width, 20);
    assert_eq!(bboxes[0].height, 10);

    // All bboxes should be within image bounds
    for bbox in &bboxes {
        assert!(bbox.x + bbox.width <= w);
        assert!(bbox.y + bbox.height <= h);
    }
}

/// Test that extract_text_bboxes returns empty for an all-zero mask.
#[test]
fn test_extract_text_bboxes_empty_mask() {
    let mask = Pix::create(100, 100, 1).expect("create 1-bit Pix");
    // All-zero mask → no connected components
    let bboxes = segmenter::extract_text_bboxes(&mask, 0).expect("extract_text_bboxes");
    assert!(bboxes.is_empty(), "Empty mask should yield no bboxes");
}

/// Test that small bboxes (< 4x4) are filtered out.
#[test]
fn test_extract_text_bboxes_filters_small() {
    // Create a 1-bit mask with only a tiny 2x2 connected component
    let mut mask = Pix::create(100, 100, 1).expect("create 1-bit Pix");
    // Set a 2x2 block of pixels (too small, should be filtered)
    mask.set_pixel(10, 10, 1).expect("set pixel");
    mask.set_pixel(11, 10, 1).expect("set pixel");
    mask.set_pixel(10, 11, 1).expect("set pixel");
    mask.set_pixel(11, 11, 1).expect("set pixel");

    // Verify connected component exists before filtering
    let raw_bboxes = mask
        .connected_component_bboxes(4)
        .expect("connected_component_bboxes");
    assert_eq!(raw_bboxes.len(), 1, "Should have one small component");
    assert_eq!(raw_bboxes[0], (10, 10, 2, 2), "Component should be 2x2");

    // extract_text_bboxes should filter it out (< 4x4)
    let bboxes = segmenter::extract_text_bboxes(&mask, 0).expect("extract_text_bboxes");
    assert!(
        bboxes.is_empty(),
        "2x2 component should be filtered out, got {} bboxes",
        bboxes.len()
    );
}

/// Test merge_distance parameter merges nearby bboxes.
#[test]
fn test_extract_text_bboxes_merge() {
    let mut mask = Pix::create(200, 100, 1).expect("create 1-bit Pix");

    // Create two separate 10x10 blocks with a 5px gap between them
    for y in 10..20 {
        for x in 10..20 {
            mask.set_pixel(x, y, 1).expect("set pixel");
        }
    }
    for y in 10..20 {
        for x in 25..35 {
            mask.set_pixel(x, y, 1).expect("set pixel");
        }
    }

    // Without merging: 2 separate bboxes
    let bboxes_no_merge = segmenter::extract_text_bboxes(&mask, 0).expect("no merge");
    assert_eq!(bboxes_no_merge.len(), 2, "Should find 2 unmerged bboxes");

    // With merge distance of 10 (gap is 5): should merge into 1
    let bboxes_merged = segmenter::extract_text_bboxes(&mask, 10).expect("with merge");
    assert_eq!(bboxes_merged.len(), 1, "Should merge into 1 bbox");

    // Merged bbox should encompass both regions
    assert_eq!(bboxes_merged[0].x, 10);
    assert_eq!(bboxes_merged[0].width, 25); // 35 - 10
}

/// Test connected_component_bboxes FFI wrapper directly.
#[test]
fn test_connected_component_bboxes_empty() {
    let mask = Pix::create(50, 50, 1).expect("create 1-bit Pix");
    let bboxes = mask
        .connected_component_bboxes(4)
        .expect("connected_component_bboxes");
    assert!(
        bboxes.is_empty(),
        "Empty 1-bit image should have no connected components"
    );
}

/// Test connected_component_bboxes with all-set mask.
#[test]
fn test_connected_component_bboxes_full() {
    let mut mask = Pix::create(50, 50, 1).expect("create 1-bit Pix");
    mask.set_all_pixels(1).expect("set all pixels");
    let bboxes = mask
        .connected_component_bboxes(4)
        .expect("connected_component_bboxes");
    // All-set image → one big connected component
    assert_eq!(bboxes.len(), 1, "All-set image should be one component");
    assert_eq!(bboxes[0], (0, 0, 50, 50));
}

/// Test connected_component_bboxes rejects non-1-bit images.
#[test]
fn test_connected_component_bboxes_wrong_depth() {
    let mask = Pix::create(50, 50, 8).expect("create 8-bit Pix");
    let result = mask.connected_component_bboxes(4);
    assert!(result.is_err(), "Should reject non-1-bit image");
}

// ---- compositor::crop_text_regions tests ----

/// Test cropping a single text region from a bitmap.
#[test]
fn test_crop_text_regions_single() {
    let (data, width, height) = create_test_rgba_image();
    let img = image::RgbaImage::from_raw(width, height, data).expect("create image");
    let dynamic = image::DynamicImage::ImageRgba8(img);

    let bboxes = vec![segmenter::PixelBBox {
        x: 10,
        y: 10,
        width: 50,
        height: 50,
    }];

    let result =
        compositor::crop_text_regions(&dynamic, &bboxes, 75, ColorMode::Rgb).expect("crop");
    assert_eq!(result.len(), 1);
    // JPEG data should be non-empty and start with FF D8
    assert!(!result[0].0.is_empty());
    assert!(result[0].0.starts_with(&[0xFF, 0xD8]));
    // BBox should be preserved
    assert_eq!(result[0].1, bboxes[0]);
}

/// Test cropping multiple regions.
#[test]
fn test_crop_text_regions_multiple() {
    let (data, width, height) = create_test_rgba_image();
    let img = image::RgbaImage::from_raw(width, height, data).expect("create image");
    let dynamic = image::DynamicImage::ImageRgba8(img);

    let bboxes = vec![
        segmenter::PixelBBox {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        },
        segmenter::PixelBBox {
            x: 50,
            y: 100,
            width: 80,
            height: 40,
        },
    ];

    let result =
        compositor::crop_text_regions(&dynamic, &bboxes, 50, ColorMode::Rgb).expect("crop");
    assert_eq!(result.len(), 2);
    for (jpeg_data, _) in &result {
        assert!(!jpeg_data.is_empty());
        assert!(jpeg_data.starts_with(&[0xFF, 0xD8]));
    }
}

/// Test cropping in grayscale mode.
#[test]
fn test_crop_text_regions_grayscale() {
    let (data, width, height) = create_test_rgba_image();
    let img = image::RgbaImage::from_raw(width, height, data).expect("create image");
    let dynamic = image::DynamicImage::ImageRgba8(img);

    let bboxes = vec![segmenter::PixelBBox {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    }];

    let result =
        compositor::crop_text_regions(&dynamic, &bboxes, 75, ColorMode::Grayscale).expect("crop");
    assert_eq!(result.len(), 1);
    assert!(!result[0].0.is_empty());
    assert!(result[0].0.starts_with(&[0xFF, 0xD8]));
}

/// Test cropping with empty bboxes returns empty.
#[test]
fn test_crop_text_regions_empty() {
    let (data, width, height) = create_test_rgba_image();
    let img = image::RgbaImage::from_raw(width, height, data).expect("create image");
    let dynamic = image::DynamicImage::ImageRgba8(img);

    let result = compositor::crop_text_regions(&dynamic, &[], 75, ColorMode::Rgb).expect("crop");
    assert!(result.is_empty());
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

    let result = compositor::compose(&data, width, height, &config, ColorMode::Rgb);
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

    let layers = compositor::compose(&data, width, height, &config, ColorMode::Rgb)
        .expect("compose should succeed");

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
