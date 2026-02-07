use pdf_masking::ffi::leptonica::Pix;

/// Create a synthetic RGBA test image with distinct black and white regions.
/// Upper-left quadrant has a black rectangle (simulating text), rest is white.
fn create_test_image_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![255u8; (width * height * 4) as usize]; // white background, alpha=255
    // Place a black rectangle in the upper portion to simulate text
    for y in 0..height / 4 {
        for x in width / 4..width * 3 / 4 {
            let idx = ((y * width + x) * 4) as usize;
            data[idx] = 0; // R
            data[idx + 1] = 0; // G
            data[idx + 2] = 0; // B
            // A stays 255
        }
    }
    data
}

#[test]
fn test_pix_create_and_drop() {
    // Pix::new should create a valid PIX and Drop should release it without panic.
    let pix = Pix::new(200, 150, 32).expect("pixCreate should succeed");
    assert_eq!(pix.width(), 200);
    assert_eq!(pix.height(), 150);
    assert_eq!(pix.depth(), 32);
    // pix is dropped here; Drop calls pixDestroy
}

#[test]
fn test_pix_from_raw_rgba() {
    let width = 100;
    let height = 80;
    let data = create_test_image_rgba(width, height);

    let pix = Pix::from_raw_rgba(&data, width, height).expect("from_raw_rgba should succeed");
    assert_eq!(pix.width(), width);
    assert_eq!(pix.height(), height);
    assert_eq!(pix.depth(), 32);
}

#[test]
fn test_pix_from_raw_rgba_length_mismatch() {
    // Data too short should return an error
    let data = vec![0u8; 100];
    let result = Pix::from_raw_rgba(&data, 200, 200);
    assert!(result.is_err());
}

#[test]
fn test_pix_dimensions() {
    let pix = Pix::new(320, 240, 8).expect("pixCreate should succeed");
    assert_eq!(pix.width(), 320);
    assert_eq!(pix.height(), 240);
    assert_eq!(pix.depth(), 8);
}

#[test]
fn test_pix_binarize() {
    let width = 200;
    let height = 200;
    let data = create_test_image_rgba(width, height);
    let pix = Pix::from_raw_rgba(&data, width, height).expect("from_raw_rgba should succeed");

    let binary = pix.binarize().expect("binarize should succeed");
    assert_eq!(binary.depth(), 1, "binarized image should be 1-bit");
    assert_eq!(binary.width(), width);
    assert_eq!(binary.height(), height);
}

#[test]
fn test_pix_get_region_masks() {
    let width = 200;
    let height = 200;
    let data = create_test_image_rgba(width, height);
    let pix = Pix::from_raw_rgba(&data, width, height).expect("from_raw_rgba should succeed");

    // pixGetRegionsBinary accepts various depth inputs (it handles conversion internally)
    let (halftone, textline, block) = pix
        .get_region_masks()
        .expect("get_region_masks should succeed");

    // All masks should be 1-bit
    assert_eq!(halftone.depth(), 1, "halftone mask should be 1-bit");
    assert_eq!(textline.depth(), 1, "textline mask should be 1-bit");
    assert_eq!(block.depth(), 1, "block mask should be 1-bit");
}

#[test]
fn test_pix_clone_independence() {
    // pixClone is actually reference-counted, so "clone" shares the same data.
    // This test verifies that our safe wrapper handles the lifecycle correctly:
    // dropping one Pix (clone) should not invalidate the other.
    let pix = Pix::new(100, 100, 32).expect("pixCreate should succeed");
    let cloned = pix.clone_pix().expect("clone_pix should succeed");

    assert_eq!(cloned.width(), 100);
    assert_eq!(cloned.height(), 100);

    // Drop the clone first; the original should still be usable
    drop(cloned);
    assert_eq!(pix.width(), 100);
}
