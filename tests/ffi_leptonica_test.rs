use pdf_masking::ffi::leptonica::Pix;

#[test]
fn test_pix_create_and_drop() {
    let pix = Pix::create(100, 100, 8);
    assert!(pix.is_ok());
    let _pix = pix.unwrap();
    // Pix should be dropped safely here
}

#[test]
fn test_pix_from_raw_rgba() {
    // Create a simple RGBA image: 2x2, 32-bit color
    let width = 2;
    let height = 2;
    // RGBA data: red, green, blue, white
    let data = vec![
        255, 0, 0, 255, // Red
        0, 255, 0, 255, // Green
        0, 0, 255, 255, // Blue
        255, 255, 255, 255, // White
    ];

    let pix = Pix::from_raw_rgba(width, height, &data);
    assert!(pix.is_ok());
}

#[test]
fn test_pix_dimensions() {
    let pix = Pix::create(50, 75, 8);
    assert!(pix.is_ok());

    let pix = pix.unwrap();
    assert_eq!(pix.get_width(), 50);
    assert_eq!(pix.get_height(), 75);
}

#[test]
fn test_pix_binarize() {
    let pix = Pix::create(100, 100, 8);
    assert!(pix.is_ok());

    let pix = pix.unwrap();
    let result = pix.otsu_adaptive_threshold(50, 50);
    assert!(result.is_ok());
}

#[test]
fn test_pix_get_region_masks() {
    let pix = Pix::create(100, 100, 1); // Binary image
    assert!(pix.is_ok());

    let pix = pix.unwrap();
    let result = pix.get_region_masks();
    assert!(result.is_ok());

    // RegionMasks has named fields; each may be None for empty images
    let masks = result.unwrap();
    // For a blank 1-bit image, textline may or may not be detected
    // Just verify the struct is accessible
    let _ = masks.halftone;
    let _ = masks.textline;
    let _ = masks.block;
}

// Fix #2, #10: Renamed from test_pix_clone_independence to
// test_pix_clone_lifecycle. pixClone creates a refcounted alias, not an
// independent deep copy. The test verifies that dropping one alias leaves
// the other valid.
#[test]
fn test_pix_clone_lifecycle() {
    let pix1 = Pix::create(50, 50, 8);
    assert!(pix1.is_ok());

    let pix1 = pix1.unwrap();
    let pix2_result = pix1.leptonica_clone();
    assert!(pix2_result.is_ok());

    let pix2 = pix2_result.unwrap();

    // Verify both aliases report the same dimensions
    assert_eq!(pix1.get_width(), pix2.get_width());
    assert_eq!(pix1.get_height(), pix2.get_height());

    // Drop the original; the refcounted alias should remain valid
    drop(pix1);
    assert_eq!(pix2.get_width(), 50);
    assert_eq!(pix2.get_height(), 50);
}

#[test]
fn test_pix_from_raw_rgba_length_mismatch() {
    // Create a simple RGBA image: 2x2, but provide wrong-sized data
    let width = 2;
    let height = 2;
    let expected_bytes = width * height * 4;

    // Provide only half the expected data
    let data = vec![255; (expected_bytes / 2) as usize];

    let pix = Pix::from_raw_rgba(width, height, &data);
    assert!(pix.is_err());
}

#[test]
fn test_pix_clip_rectangle() {
    // Create a 100x100 1-bit Pix and fill a 20x10 region
    let mut pix = Pix::create(100, 100, 1).expect("create 1-bit Pix");
    for y in 30..40 {
        for x in 20..40 {
            pix.set_pixel(x, y, 1).expect("set pixel");
        }
    }

    // Clip the 20x10 region
    let clipped = pix.clip_rectangle(20, 30, 20, 10).expect("clip_rectangle");

    // Verify dimensions
    assert_eq!(clipped.get_width(), 20, "clipped width should be 20");
    assert_eq!(clipped.get_height(), 10, "clipped height should be 10");
    assert_eq!(clipped.get_depth(), 1, "clipped depth should be 1");
}

#[test]
fn test_pix_clip_rectangle_out_of_bounds() {
    let pix = Pix::create(50, 50, 1).expect("create 1-bit Pix");

    // Clip region that exceeds bounds should fail
    let result = pix.clip_rectangle(40, 40, 20, 20);
    assert!(result.is_err(), "should fail when clipping out of bounds");
}

#[test]
fn test_pix_clip_rectangle_non_1bit() {
    let pix = Pix::create(100, 100, 8).expect("create 8-bit Pix");

    // clip_rectangle should work for non-1-bit images too
    let result = pix.clip_rectangle(10, 10, 30, 30);
    assert!(
        result.is_ok(),
        "clip_rectangle should work for 8-bit images"
    );
}
