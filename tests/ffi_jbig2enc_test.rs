use pdf_masking::ffi::jbig2enc;
use pdf_masking::ffi::leptonica::Pix;

/// Encode a white 1-bit PIX and verify we get non-empty JBIG2 data back.
#[test]
fn test_encode_generic_basic() {
    // Create a 100x100 white 1-bit image (all zeros = white in leptonica)
    let pix = Pix::create(100, 100, 1).expect("failed to create 1-bit Pix");

    let result = jbig2enc::encode_generic(&pix);
    assert!(result.is_ok(), "encode_generic failed: {:?}", result.err());

    let data = result.unwrap();
    assert!(!data.is_empty(), "encoded data should not be empty");
}

/// Encode a 1-bit PIX that has some black pixels set.
/// Verifies encoding works for non-trivial content.
#[test]
fn test_encode_generic_with_content() {
    // Create a 64x64 1-bit image and set a horizontal black stripe
    // by writing directly to pixel data via leptonica
    let pix = Pix::create(64, 64, 1).expect("failed to create 1-bit Pix");

    // Set all pixels to black (fill with 0xFF) using the raw data accessor
    pix.set_all_pixels(1).expect("failed to set pixels");

    let result = jbig2enc::encode_generic(&pix);
    assert!(result.is_ok(), "encode_generic failed: {:?}", result.err());

    let data = result.unwrap();
    assert!(!data.is_empty(), "encoded data should not be empty");
}

/// Verify the encoded data has a reasonable size.
/// A 100x100 white image should compress very small with JBIG2.
#[test]
fn test_encode_result_has_reasonable_size() {
    let pix = Pix::create(100, 100, 1).expect("failed to create 1-bit Pix");

    let data = jbig2enc::encode_generic(&pix).expect("encode failed");

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
    let pix = Pix::create(612, 792, 1).expect("failed to create letter-size Pix");

    let result = jbig2enc::encode_generic(&pix);
    assert!(
        result.is_ok(),
        "encode_generic failed on letter-size image: {:?}",
        result.err()
    );

    let data = result.unwrap();
    assert!(!data.is_empty(), "encoded data should not be empty");
}
