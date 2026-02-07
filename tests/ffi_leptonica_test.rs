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
    let mut data = Vec::new();

    // Generate RGBA data: red, green, blue, white
    data.push(255);
    data.push(0);
    data.push(0);
    data.push(255); // Red
    data.push(0);
    data.push(255);
    data.push(0);
    data.push(255); // Green
    data.push(0);
    data.push(0);
    data.push(255);
    data.push(255); // Blue
    data.push(255);
    data.push(255);
    data.push(255);
    data.push(255); // White

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
    let mut data = Vec::new();
    for _ in 0..(expected_bytes / 2) {
        data.push(255);
    }

    let pix = Pix::from_raw_rgba(width, height, &data);
    assert!(pix.is_err());
}
