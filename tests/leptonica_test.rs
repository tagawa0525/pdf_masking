#![cfg(feature = "mrc")]

use leptonica::{Pix, PixMut, PixelDepth};

#[test]
fn test_pix_create_and_drop() {
    let pix = Pix::new(100, 100, PixelDepth::Bit8);
    assert!(pix.is_ok());
    let _pix = pix.unwrap();
    // Pix should be dropped safely here
}

#[test]
fn test_pix_from_raw_rgba() {
    use pdf_masking::mrc::segmenter::pix_from_raw_rgba;

    let width = 2;
    let height = 2;
    // RGBA data: red, green, blue, white
    let data = vec![
        255, 0, 0, 255, // Red
        0, 255, 0, 255, // Green
        0, 0, 255, 255, // Blue
        255, 255, 255, 255, // White
    ];

    let pix = pix_from_raw_rgba(width, height, &data);
    assert!(pix.is_ok());
}

#[test]
fn test_pix_dimensions() {
    let pix = Pix::new(50, 75, PixelDepth::Bit8);
    assert!(pix.is_ok());

    let pix = pix.unwrap();
    assert_eq!(pix.width(), 50);
    assert_eq!(pix.height(), 75);
}

#[test]
fn test_pix_binarize() {
    use leptonica::color::otsu_adaptive_threshold;

    let pix = Pix::new(100, 100, PixelDepth::Bit8);
    assert!(pix.is_ok());

    let pix = pix.unwrap();
    let result = otsu_adaptive_threshold(&pix, 50, 50, 0, 0, 0.0);
    assert!(result.is_ok());
}

#[test]
fn test_pix_get_region_masks() {
    use leptonica::recog::pageseg::{PageSegOptions, segment_regions};

    let pix = Pix::new(100, 100, PixelDepth::Bit1);
    assert!(pix.is_ok());

    let pix = pix.unwrap();
    let opts = PageSegOptions::default();
    let result = segment_regions(&pix, &opts);
    assert!(result.is_ok());

    // SegmentationResult has named fields; check they're accessible
    let masks = result.unwrap();
    // halftone_mask may be None for blank images
    let _ = masks.halftone_mask;
    let _ = masks.textline_mask;
    let _ = masks.textblock_mask;
}

#[test]
fn test_pix_clone_lifecycle() {
    let pix1 = Pix::new(50, 50, PixelDepth::Bit8);
    assert!(pix1.is_ok());

    let pix1 = pix1.unwrap();
    let pix2 = pix1.deep_clone();

    // Verify both images report the same dimensions
    assert_eq!(pix1.width(), pix2.width());
    assert_eq!(pix1.height(), pix2.height());

    // Drop the original; the deep clone should remain valid
    drop(pix1);
    assert_eq!(pix2.width(), 50);
    assert_eq!(pix2.height(), 50);
}

#[test]
fn test_pix_from_raw_rgba_length_mismatch() {
    use pdf_masking::mrc::segmenter::pix_from_raw_rgba;

    let width = 2;
    let height = 2;
    let expected_bytes = width * height * 4;

    // Provide only half the expected data
    let data = vec![255; (expected_bytes / 2) as usize];

    let pix = pix_from_raw_rgba(width, height, &data);
    assert!(pix.is_err());
}

#[test]
fn test_pix_clip_rectangle() {
    // Create a 100x100 1-bit Pix
    let pix = Pix::new(100, 100, PixelDepth::Bit1).expect("create 1-bit Pix");
    let mut pix_mut: PixMut = pix.try_into_mut().unwrap();

    for y in 30..40u32 {
        for x in 20..40u32 {
            pix_mut.set_pixel(x, y, 1).expect("set pixel");
        }
    }

    let pix_immut: Pix = pix_mut.into();

    // Clip the 20x10 region
    let clipped = pix_immut
        .clip_rectangle(20, 30, 20, 10)
        .expect("clip_rectangle");

    // Verify dimensions
    assert_eq!(clipped.width(), 20, "clipped width should be 20");
    assert_eq!(clipped.height(), 10, "clipped height should be 10");
    assert_eq!(
        clipped.depth(),
        PixelDepth::Bit1,
        "clipped depth should be 1-bit"
    );
}

#[test]
fn test_pix_clip_rectangle_out_of_bounds() {
    let pix = Pix::new(50, 50, PixelDepth::Bit1).expect("create 1-bit Pix");

    // Origin entirely outside image bounds should fail
    let result = pix.clip_rectangle(50, 50, 10, 10);
    assert!(
        result.is_err(),
        "should fail when origin is outside image bounds"
    );

    let result2 = pix.clip_rectangle(60, 10, 10, 10);
    assert!(
        result2.is_err(),
        "should fail when x origin is outside image bounds"
    );
}

#[test]
fn test_pix_clip_rectangle_non_1bit() {
    let pix = Pix::new(100, 100, PixelDepth::Bit8).expect("create 8-bit Pix");

    // clip_rectangle should work for non-1-bit images too
    let result = pix.clip_rectangle(10, 10, 30, 30);
    assert!(
        result.is_ok(),
        "clip_rectangle should work for 8-bit images"
    );
}

#[test]
fn test_pix_clip_rectangle_zero_size() {
    let pix = Pix::new(100, 100, PixelDepth::Bit1).expect("create 1-bit Pix");

    // Zero width should fail
    let result = pix.clip_rectangle(10, 10, 0, 20);
    assert!(result.is_err(), "should fail with zero width");

    // Zero height should fail
    let result = pix.clip_rectangle(10, 10, 20, 0);
    assert!(result.is_err(), "should fail with zero height");
}
