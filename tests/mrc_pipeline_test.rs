// Phase 5: MRC pipeline integration tests
//
// Tests for the MRC pipeline: segmenter, jbig2, jpeg, compositor.
// Each test verifies a specific component of the MRC layer generation pipeline.

use std::collections::HashMap;

use pdf_masking::config::job::ColorMode;
#[cfg(feature = "mrc")]
use pdf_masking::ffi::leptonica::Pix;
use pdf_masking::mrc::compositor;
use pdf_masking::mrc::jpeg;
#[cfg(feature = "mrc")]
use pdf_masking::mrc::{jbig2, segmenter};
use pdf_masking::pdf::font::ParsedFont;

fn load_sample_fonts() -> HashMap<String, ParsedFont> {
    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts")
}

/// Generate a synthetic 200x200 RGBA test image.
/// Upper half: black (text-like region), lower half: white (background).
#[cfg(feature = "mrc")]
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
#[cfg(feature = "mrc")]
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
#[cfg(feature = "mrc")]
#[test]
fn test_segment_mask_is_1bit() {
    let (data, width, height) = create_test_rgba_image();

    let mask = segmenter::segment_text_mask(&data, width, height)
        .expect("segment_text_mask should succeed");

    assert_eq!(mask.get_depth(), 1, "Text mask should be 1-bit depth");
}

// ---- jbig2.rs tests ----

/// Test encoding a 1-bit mask to JBIG2 format.
#[cfg(feature = "mrc")]
#[test]
fn test_encode_mask_to_jbig2() {
    // Create a simple 1-bit mask
    let mut mask = Pix::create(100, 100, 1).expect("failed to create 1-bit Pix");
    mask.set_all_pixels(1).expect("failed to set pixels");

    let result = jbig2::encode_mask(&mut mask);
    assert!(result.is_ok(), "encode_mask failed: {:?}", result.err());
}

/// Test that JBIG2 encoding produces non-empty output.
#[cfg(feature = "mrc")]
#[test]
fn test_encode_returns_non_empty() {
    let mut mask = Pix::create(100, 100, 1).expect("failed to create 1-bit Pix");

    let data = jbig2::encode_mask(&mut mask).expect("encode_mask should succeed");
    assert!(!data.is_empty(), "JBIG2 encoded data should not be empty");
}

// ---- jpeg.rs tests ----

/// Test encoding a background RGBA image to JPEG format.
#[test]
#[cfg(feature = "mrc")]
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
#[cfg(feature = "mrc")]
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
#[cfg(feature = "mrc")]
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

// ---- compositor.rs tests ----

/// Test the full MRC pipeline: RGBA bitmap + config -> MrcLayers.
#[test]
fn test_compose_mrc_layers() {
    let (data, width, height) = create_test_rgba_image();
    let config = compositor::MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };

    let result = compositor::compose(
        &data,
        width,
        height,
        595.276,
        841.89,
        &config,
        ColorMode::Rgb,
    );
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

    let layers = compositor::compose(
        &data,
        width,
        height,
        595.276,
        841.89,
        &config,
        ColorMode::Rgb,
    )
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

// ---- compose_text_masked tests ----

/// Test compose_text_masked with empty content stream.
#[test]
fn test_compose_text_masked_empty_content() {
    let (data, width, height) = create_test_rgba_image();
    let image_streams = std::collections::HashMap::new();

    let params = compositor::TextMaskedParams {
        content_bytes: b"",
        rgba_data: &data,
        bitmap_width: width,
        bitmap_height: height,
        page_width_pts: 612.0,
        page_height_pts: 792.0,
        image_streams: &image_streams,
        color_mode: ColorMode::Rgb,
        page_index: 0,
    };

    let result = compositor::compose_text_masked(&params);
    assert!(
        result.is_ok(),
        "compose_text_masked failed: {:?}",
        result.err()
    );

    let text_data = result.unwrap();
    assert_eq!(text_data.page_index, 0);
    assert!(matches!(text_data.color_mode, ColorMode::Rgb));
    assert!(text_data.stripped_content_stream.is_empty());
    assert!(text_data.modified_images.is_empty());
}

/// Test compose_text_masked strips BT...ET blocks from content.
#[test]
fn test_compose_text_masked_strips_text() {
    let content = b"BT /F1 12 Tf (Hello) Tj ET";
    let (data, width, height) = create_test_rgba_image();
    let image_streams = std::collections::HashMap::new();

    let params = compositor::TextMaskedParams {
        content_bytes: content,
        rgba_data: &data,
        bitmap_width: width,
        bitmap_height: height,
        page_width_pts: 612.0,
        page_height_pts: 792.0,
        image_streams: &image_streams,
        color_mode: ColorMode::Rgb,
        page_index: 2,
    };

    let result = compositor::compose_text_masked(&params);
    assert!(
        result.is_ok(),
        "compose_text_masked failed: {:?}",
        result.err()
    );

    let text_data = result.unwrap();
    // BT...ET should be stripped from the content stream
    assert!(text_data.stripped_content_stream.is_empty());
    assert_eq!(text_data.page_index, 2);
}

/// Test compose_text_masked in grayscale mode.
#[test]
fn test_compose_text_masked_grayscale() {
    let (data, width, height) = create_test_rgba_image();
    let image_streams = std::collections::HashMap::new();

    let params = compositor::TextMaskedParams {
        content_bytes: b"",
        rgba_data: &data,
        bitmap_width: width,
        bitmap_height: height,
        page_width_pts: 100.0,
        page_height_pts: 100.0,
        image_streams: &image_streams,
        color_mode: ColorMode::Grayscale,
        page_index: 1,
    };

    let result = compositor::compose_text_masked(&params);
    assert!(result.is_ok(), "grayscale failed: {:?}", result.err());

    let text_data = result.unwrap();
    assert!(matches!(text_data.color_mode, ColorMode::Grayscale));
    // Text regions should have valid JBIG2 data
    for region in &text_data.text_regions {
        assert!(!region.jbig2_data.is_empty());
        assert!(region.pixel_width > 0);
        assert!(region.pixel_height > 0);
    }
}

/// Test that text regions have valid PDF bounding boxes.
#[test]
fn test_compose_text_masked_valid_bboxes() {
    let (data, width, height) = create_test_rgba_image();
    let image_streams = std::collections::HashMap::new();

    let params = compositor::TextMaskedParams {
        content_bytes: b"",
        rgba_data: &data,
        bitmap_width: width,
        bitmap_height: height,
        page_width_pts: 200.0,
        page_height_pts: 200.0,
        image_streams: &image_streams,
        color_mode: ColorMode::Rgb,
        page_index: 0,
    };

    let result = compositor::compose_text_masked(&params).expect("should succeed");

    // Each text region should have a valid PDF BBox
    for region in &result.text_regions {
        assert!(
            region.bbox_points.x_min >= 0.0,
            "x_min should be >= 0: {}",
            region.bbox_points.x_min
        );
        assert!(
            region.bbox_points.y_min >= 0.0,
            "y_min should be >= 0: {}",
            region.bbox_points.y_min
        );
        assert!(
            region.bbox_points.x_max > region.bbox_points.x_min,
            "x_max should > x_min"
        );
        assert!(
            region.bbox_points.y_max > region.bbox_points.y_min,
            "y_max should > y_min"
        );
    }
}

// ---- compose_text_outlines tests ----

/// テキスト→アウトライン変換: BT...ETがベクターパスに変換されること
#[test]
fn test_compose_text_outlines_replaces_text_with_paths() {
    let fonts = load_sample_fonts();
    // F4はWinAnsiEncoding（'A'のアウトラインあり）
    let content = b"BT /F4 12 Tf (A) Tj ET";
    let image_streams = HashMap::new();

    let params = compositor::TextOutlinesParams {
        content_bytes: content,
        fonts: &fonts,
        image_streams: &image_streams,
        page_width_pts: 595.276,
        page_height_pts: 841.89,
        color_mode: ColorMode::Rgb,
        page_index: 0,
    };

    let result = compositor::compose_text_outlines(&params);
    assert!(result.is_ok(), "should succeed: {:?}", result.err());

    let data = result.unwrap();
    // text_regionsは空（テキストはパスとしてコンテンツストリーム内にある）
    assert!(
        data.text_regions.is_empty(),
        "text_regions should be empty when text is converted to outlines"
    );
    // コンテンツストリームにパス演算子が含まれること
    let text = String::from_utf8_lossy(&data.stripped_content_stream);
    let has_moveto = text.split_whitespace().any(|token| token == "m");
    assert!(
        has_moveto,
        "should contain moveto (m) operators as PDF tokens"
    );
    let has_fill = text.split_whitespace().any(|token| token == "f");
    assert!(has_fill, "should contain fill (f) operators as PDF tokens");
    // 元のテキスト演算子が除去されていること
    assert!(!text.contains(" BT"), "should not contain BT");
    assert!(!text.contains(" Tf"), "should not contain Tf");
}

/// フォント未発見時はErrを返すこと
#[test]
fn test_compose_text_outlines_returns_err_on_missing_font() {
    let fonts = HashMap::new(); // 空のフォントマップ
    let content = b"BT /F99 12 Tf (Hello) Tj ET";
    let image_streams = HashMap::new();

    let params = compositor::TextOutlinesParams {
        content_bytes: content,
        fonts: &fonts,
        image_streams: &image_streams,
        page_width_pts: 595.276,
        page_height_pts: 841.89,
        color_mode: ColorMode::Rgb,
        page_index: 0,
    };

    let result = compositor::compose_text_outlines(&params);
    assert!(result.is_err(), "should fail with missing font");
}

/// テキストがない場合もOkを返すこと
#[test]
fn test_compose_text_outlines_no_text() {
    let fonts = load_sample_fonts();
    let content = b"q 1 0 0 1 0 0 cm /Im1 Do Q";
    let image_streams = HashMap::new();

    let params = compositor::TextOutlinesParams {
        content_bytes: content,
        fonts: &fonts,
        image_streams: &image_streams,
        page_width_pts: 595.276,
        page_height_pts: 841.89,
        color_mode: ColorMode::Rgb,
        page_index: 0,
    };

    let result = compositor::compose_text_outlines(&params);
    assert!(result.is_ok(), "should succeed: {:?}", result.err());

    let data = result.unwrap();
    assert!(data.text_regions.is_empty());
    // Doオペレータが保持されること
    let text = String::from_utf8_lossy(&data.stripped_content_stream);
    assert!(text.contains("Do"), "should preserve Do operator");
}

// ---- crop_text_regions_jbig2 tests ----

/// Test cropping a single text region as JBIG2 from a 1-bit mask.
#[test]
fn test_crop_text_regions_jbig2_single() {
    let mut mask = Pix::create(200, 200, 1).expect("create 1-bit Pix");

    // Create a 50x50 black region at (50, 50)
    for y in 50..100 {
        for x in 50..100 {
            mask.set_pixel(x, y, 1).expect("set pixel");
        }
    }

    let bboxes = vec![segmenter::PixelBBox {
        x: 50,
        y: 50,
        width: 50,
        height: 50,
    }];

    let result = compositor::crop_text_regions_jbig2(&mask, &bboxes);
    assert!(
        result.is_ok(),
        "crop_text_regions_jbig2 failed: {:?}",
        result.err()
    );

    let crops = result.unwrap();
    assert_eq!(crops.len(), 1, "should have 1 crop");
    // JBIG2 data should be non-empty
    assert!(!crops[0].0.is_empty(), "JBIG2 data should not be empty");
    // BBox should be preserved
    assert_eq!(crops[0].1, bboxes[0], "bbox should be preserved");
}

/// Test cropping multiple regions from a 1-bit mask.
#[test]
fn test_crop_text_regions_jbig2_multiple() {
    let mut mask = Pix::create(300, 200, 1).expect("create 1-bit Pix");

    // Create two separate regions
    for y in 20..50 {
        for x in 20..70 {
            mask.set_pixel(x, y, 1).expect("set pixel");
        }
    }
    for y in 100..150 {
        for x in 100..150 {
            mask.set_pixel(x, y, 1).expect("set pixel");
        }
    }

    let bboxes = vec![
        segmenter::PixelBBox {
            x: 20,
            y: 20,
            width: 50,
            height: 30,
        },
        segmenter::PixelBBox {
            x: 100,
            y: 100,
            width: 50,
            height: 50,
        },
    ];

    let result = compositor::crop_text_regions_jbig2(&mask, &bboxes);
    assert!(result.is_ok(), "should succeed: {:?}", result.err());

    let crops = result.unwrap();
    assert_eq!(crops.len(), 2, "should have 2 crops");
    for (jbig2_data, _) in &crops {
        assert!(
            !jbig2_data.is_empty(),
            "each JBIG2 crop should be non-empty"
        );
    }
}

/// Test cropping with empty bboxes returns empty.
#[test]
fn test_crop_text_regions_jbig2_empty() {
    let mask = Pix::create(200, 200, 1).expect("create 1-bit Pix");
    let result = compositor::crop_text_regions_jbig2(&mask, &[]);
    assert!(result.is_ok());
    let crops = result.unwrap();
    assert!(crops.is_empty(), "empty bboxes should yield empty crops");
}
