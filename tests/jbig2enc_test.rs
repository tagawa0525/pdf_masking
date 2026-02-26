#![cfg(feature = "mrc")]

use jbig2enc::encoder::encode_generic;
use leptonica::{Pix, PixMut, PixelDepth};

/// Encode a white 1-bit PIX and verify we get non-empty JBIG2 data back.
#[test]
fn test_encode_generic_basic() {
    // Create a 100x100 white 1-bit image (all zeros = white in leptonica)
    let pix = Pix::new(100, 100, PixelDepth::Bit1).expect("failed to create 1-bit Pix");

    let result = encode_generic(&pix, false, 0, 0, true);
    assert!(result.is_ok(), "encode_generic failed: {:?}", result.err());

    let data = result.unwrap();
    assert!(!data.is_empty(), "encoded data should not be empty");
}

/// Encode a 1-bit PIX that has some black pixels set.
/// Verifies encoding works for non-trivial content.
#[test]
fn test_encode_generic_with_content() {
    // Create a 64x64 1-bit image and set all black pixels
    let pix = Pix::new(64, 64, PixelDepth::Bit1).expect("failed to create 1-bit Pix");
    let mut pix_mut: PixMut = pix.try_into_mut().unwrap();

    // Set all pixels to black
    pix_mut
        .set_all_arbitrary(1)
        .expect("failed to set all pixels");

    let pix_immut: Pix = pix_mut.into();
    let result = encode_generic(&pix_immut, false, 0, 0, true);
    assert!(result.is_ok(), "encode_generic failed: {:?}", result.err());

    let data = result.unwrap();
    assert!(!data.is_empty(), "encoded data should not be empty");
}

/// Verify the encoded data has a reasonable size.
/// A 100x100 white image should compress very small with JBIG2.
#[test]
fn test_encode_result_has_reasonable_size() {
    let pix = Pix::new(100, 100, PixelDepth::Bit1).expect("failed to create 1-bit Pix");

    let data = encode_generic(&pix, false, 0, 0, true).expect("encode failed");

    // A 100x100 1-bit image uncompressed is ~1250 bytes (100*100/8).
    // JBIG2 should produce something smaller, but at least a few bytes.
    assert!(
        data.len() >= 4,
        "encoded data too small: {} bytes",
        data.len()
    );
    assert!(
        data.len() < 100 * 100, // should be much smaller than raw
        "encoded data suspiciously large: {} bytes",
        data.len()
    );
}

/// Encoding a larger image should succeed and produce valid output.
#[test]
fn test_encode_generic_larger_image() {
    let pix = Pix::new(612, 792, PixelDepth::Bit1).expect("failed to create letter-size Pix");

    let result = encode_generic(&pix, false, 0, 0, true);
    assert!(
        result.is_ok(),
        "encode_generic failed on letter-size image: {:?}",
        result.err()
    );

    let data = result.unwrap();
    assert!(!data.is_empty(), "encoded data should not be empty");
}
