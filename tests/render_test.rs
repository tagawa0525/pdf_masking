// Phase 6: Render integration tests
//
// Tests for rendering PDF pages to DynamicImage using pdfium-render.
// Test PDFs are dynamically generated with lopdf to avoid fixture files.

use std::path::PathBuf;

use pdf_masking::render::pdfium::render_page;

/// Create a minimal 1-page PDF (Letter size: 612x792 points) using lopdf.
/// Returns the path to a temporary file containing the PDF.
fn create_test_pdf(dir: &tempfile::TempDir) -> PathBuf {
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.4");

    // Empty content stream
    let content_stream = Stream::new(dictionary! {}, Vec::new());
    let content_id = doc.add_object(content_stream);

    // Page object: Letter size (612 x 792 points)
    let page = dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ],
        "Contents" => content_id,
        "Resources" => dictionary! {},
    };
    let page_id = doc.add_object(page);

    // Pages object
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => Object::Integer(1),
    };
    let pages_id = doc.add_object(pages);

    // Set Parent reference on page
    if let Ok(Object::Dictionary(dict)) = doc.get_object_mut(page_id) {
        dict.set("Parent", pages_id);
    }

    // Catalog
    let catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);

    let path = dir.path().join("test.pdf");
    doc.save(&path).expect("failed to save test PDF");

    path
}

// ---- Test 1: Basic rendering ----

/// Render page 0 of a single-page PDF and verify we get a DynamicImage.
#[test]
fn test_render_page_basic() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let pdf_path = create_test_pdf(&dir);

    let result = render_page(&pdf_path, 0, 72);
    assert!(
        result.is_ok(),
        "render_page should succeed for a valid PDF, got: {:?}",
        result.err()
    );

    let image = result.unwrap();
    assert!(image.width() > 0, "rendered image width should be > 0");
    assert!(image.height() > 0, "rendered image height should be > 0");
}

// ---- Test 2: Dimensions match DPI ----

/// At 72 DPI, a Letter-size page (612x792 pt) should render to ~612x792 pixels.
#[test]
fn test_render_page_dimensions() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let pdf_path = create_test_pdf(&dir);

    let image = render_page(&pdf_path, 0, 72).expect("render_page should succeed");

    // Letter size at 72 DPI: 612 x 792 pixels (1 pt = 1 px at 72 DPI)
    assert_eq!(image.width(), 612, "width at 72 DPI should be 612");
    assert_eq!(image.height(), 792, "height at 72 DPI should be 792");
}

// ---- Test 3: Different DPI produces different dimensions ----

/// Rendering at 144 DPI should produce an image twice the size of 72 DPI.
#[test]
fn test_render_page_at_different_dpi() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let pdf_path = create_test_pdf(&dir);

    let image_72 = render_page(&pdf_path, 0, 72).expect("render at 72 DPI should succeed");
    let image_144 = render_page(&pdf_path, 0, 144).expect("render at 144 DPI should succeed");

    // At 144 DPI, dimensions should be double those at 72 DPI
    assert_eq!(
        image_144.width(),
        image_72.width() * 2,
        "144 DPI width should be 2x of 72 DPI"
    );
    assert_eq!(
        image_144.height(),
        image_72.height() * 2,
        "144 DPI height should be 2x of 72 DPI"
    );
}

// ---- Test 4: Nonexistent file ----

/// Rendering a nonexistent file should return an error.
#[test]
fn test_render_nonexistent_file() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let nonexistent = dir.path().join("nonexistent_file.pdf");
    let result = render_page(&nonexistent, 0, 72);
    assert!(
        result.is_err(),
        "render_page should fail for a nonexistent file"
    );
}

// ---- Test 5: Zero DPI ----

/// Rendering with dpi=0 should return an error.
#[test]
fn test_render_zero_dpi() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let pdf_path = create_test_pdf(&dir);

    let result = render_page(&pdf_path, 0, 0);
    assert!(result.is_err(), "render_page should fail when dpi is 0");
}

// ---- Test 6: Invalid page index ----

/// Rendering a page index beyond the document's page count should return an error.
#[test]
fn test_render_invalid_page_index() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let pdf_path = create_test_pdf(&dir);

    // The test PDF has only 1 page (index 0), so page 99 should fail
    let result = render_page(&pdf_path, 99, 72);
    assert!(
        result.is_err(),
        "render_page should fail for an out-of-range page index"
    );
}
