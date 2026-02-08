// Phase 11: Linearization integration tests
//
// Tests for PDF linearization via qpdf CLI wrapper.
// Each test verifies a specific aspect of the linearize module.

use lopdf::{Document, Object, Stream, dictionary};
use pdf_masking::error::PdfMaskError;
use pdf_masking::linearize::{linearize, linearize_in_place};
use tempfile::tempdir;

/// Check whether qpdf is available on PATH.
fn qpdf_available() -> bool {
    std::process::Command::new("qpdf")
        .arg("--version")
        .output()
        .is_ok()
}

/// Create a minimal valid PDF for testing.
fn create_test_pdf(path: &str) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    // Add an empty content stream so the page is fully formed
    let content_stream = Stream::new(dictionary! {}, Vec::new());
    let content_id = doc.add_object(content_stream);

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ],
        "Contents" => content_id,
        "Resources" => dictionary! {},
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).expect("save test PDF");
}

/// Linearize creates an output file at the specified path.
#[test]
fn test_linearize_creates_output() {
    if !qpdf_available() {
        eprintln!("Skipping: qpdf not found in PATH (run inside `nix develop`)");
        return;
    }
    let dir = tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_test_pdf(input_path.to_str().unwrap());

    linearize(input_path.to_str().unwrap(), output_path.to_str().unwrap())
        .expect("linearize should succeed");

    assert!(output_path.exists(), "output file should exist");
    assert!(
        std::fs::metadata(&output_path).unwrap().len() > 0,
        "output file should not be empty"
    );
}

/// Linearize in-place replaces the original file and keeps it valid.
#[test]
fn test_linearize_in_place() {
    if !qpdf_available() {
        eprintln!("Skipping: qpdf not found in PATH (run inside `nix develop`)");
        return;
    }
    let dir = tempdir().expect("create temp dir");
    let pdf_path = dir.path().join("inplace.pdf");

    create_test_pdf(pdf_path.to_str().unwrap());

    linearize_in_place(pdf_path.to_str().unwrap()).expect("linearize in-place should succeed");

    assert!(
        pdf_path.exists(),
        "file should still exist after in-place linearization"
    );
    assert!(
        std::fs::metadata(&pdf_path).unwrap().len() > 0,
        "file should not be empty after linearization"
    );
    // Verify the output is a loadable, valid 1-page PDF
    let doc = Document::load(pdf_path.to_str().unwrap())
        .expect("in-place linearized PDF should be loadable by lopdf");
    assert_eq!(
        doc.get_pages().len(),
        1,
        "linearized PDF should have 1 page"
    );
}

/// Linearize returns an error for a nonexistent input file.
#[test]
fn test_linearize_nonexistent_input() {
    if !qpdf_available() {
        eprintln!("Skipping: qpdf not found in PATH (run inside `nix develop`)");
        return;
    }
    let dir = tempdir().expect("create temp dir");
    let input_path = dir.path().join("nonexistent.pdf");
    let output_path = dir.path().join("output.pdf");

    let result = linearize(input_path.to_str().unwrap(), output_path.to_str().unwrap());

    assert!(
        result.is_err(),
        "linearize should fail for nonexistent input"
    );
    match result.unwrap_err() {
        PdfMaskError::LinearizeError(_) => {} // expected
        other => panic!("expected LinearizeError, got: {}", other),
    }
}

/// Linearized output can be loaded by lopdf as a valid PDF.
#[test]
fn test_linearize_output_is_valid_pdf() {
    if !qpdf_available() {
        eprintln!("Skipping: qpdf not found in PATH (run inside `nix develop`)");
        return;
    }
    let dir = tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("linearized.pdf");

    create_test_pdf(input_path.to_str().unwrap());

    linearize(input_path.to_str().unwrap(), output_path.to_str().unwrap())
        .expect("linearize should succeed");

    let doc = Document::load(output_path.to_str().unwrap())
        .expect("linearized PDF should be loadable by lopdf");

    // A linearized PDF should still have at least one page
    let page_count = doc.get_pages().len();
    assert_eq!(page_count, 1, "linearized PDF should have 1 page");
}
