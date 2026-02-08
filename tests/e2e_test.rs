// Phase 13: E2E integration tests
//
// End-to-end tests that verify the complete pipeline from CLI invocation
// to output PDF generation. All test PDFs are dynamically generated with
// lopdf (no committed fixtures).

use std::path::Path;
use std::process::Command;

use lopdf::{Document, Object, Stream, dictionary};

// ============================================================
// Guards and helpers
// ============================================================

/// Check whether pdfium is available via environment variable.
fn pdfium_available() -> bool {
    std::env::var("PDFIUM_DYNAMIC_LIB_PATH").is_ok()
}

/// Build a Command pointing to the compiled binary.
fn cargo_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdf_masking"))
}

/// Create a minimal valid 1-page PDF (Letter size: 612x792 points) using lopdf.
fn create_single_page_pdf(path: &Path) {
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

    doc.save(path).expect("failed to save test PDF");
}

/// Create a multi-page PDF with the specified number of pages.
fn create_multi_page_pdf(path: &Path, num_pages: usize) {
    let mut doc = Document::with_version("1.4");

    let pages_id = doc.new_object_id();

    let mut kids: Vec<Object> = Vec::new();

    for _ in 0..num_pages {
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

        kids.push(page_id.into());
    }

    // Pages node
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => kids,
        "Count" => Object::Integer(num_pages as i64),
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    // Catalog
    let catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);

    doc.save(path).expect("failed to save multi-page test PDF");
}

/// Write a jobs.yaml file for testing.
///
/// `pages` are 1-based page numbers (as expected in the YAML format).
fn write_jobs_yaml(dir: &Path, input: &str, output: &str, pages: &[u32]) {
    let pages_str = pages
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let yaml = format!(
        "jobs:\n  - input: \"{input}\"\n    output: \"{output}\"\n    pages: \"{pages_str}\"\n    dpi: 72\n"
    );
    std::fs::write(dir.join("jobs.yaml"), yaml).expect("failed to write jobs.yaml");
}

/// Write a settings.yaml file for testing.
fn write_settings_yaml(dir: &Path, dpi: u32, bg_quality: u8) {
    let yaml = format!("dpi: {dpi}\nbg_quality: {bg_quality}\nlinearize: false\n");
    std::fs::write(dir.join("settings.yaml"), yaml).expect("failed to write settings.yaml");
}

// ============================================================
// 1. Single-page PDF E2E test
// ============================================================

/// Create a 1-page PDF, write jobs.yaml targeting page 1, run CLI,
/// verify output PDF exists and is valid (loadable by lopdf, has 1 page).
#[test]
fn test_e2e_single_page_pdf() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_single_page_pdf(&input_path);
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", &[1]);

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(jobs_yaml_path.to_str().unwrap())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success, stderr: {stderr}"
    );

    // Verify output PDF exists and is loadable
    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable by lopdf");
    assert_eq!(doc.get_pages().len(), 1, "output PDF should have 1 page");
}

// ============================================================
// 2. Multi-page PDF E2E test
// ============================================================

/// Create a 3-page PDF, write jobs.yaml targeting pages 1 and 3, run CLI,
/// verify output exists and is valid.
#[test]
fn test_e2e_multi_page_pdf() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_multi_page_pdf(&input_path, 3);
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", &[1, 3]);

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(jobs_yaml_path.to_str().unwrap())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success, stderr: {stderr}"
    );

    // Verify output PDF exists and is loadable
    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable by lopdf");
    // The pipeline creates a new PDF with only the processed pages
    assert_eq!(
        doc.get_pages().len(),
        2,
        "output PDF should have 2 pages (pages 1 and 3 from input)"
    );
}

// ============================================================
// 3. E2E test with settings.yaml
// ============================================================

/// Create PDF + settings.yaml + jobs.yaml in same dir, run CLI,
/// verify settings are respected (verify output exists and is loadable).
#[test]
fn test_e2e_with_settings_yaml() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_single_page_pdf(&input_path);
    write_settings_yaml(dir.path(), 72, 30);
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", &[1]);

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(jobs_yaml_path.to_str().unwrap())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success when settings.yaml is present, stderr: {stderr}"
    );

    // Verify output PDF exists and is loadable
    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable by lopdf");
    assert_eq!(doc.get_pages().len(), 1, "output PDF should have 1 page");
}

// ============================================================
// 4. E2E test: output has MRC structure
// ============================================================

/// Run pipeline, verify output PDF has XObject resources (BgImg, FgImg)
/// on processed pages.
#[test]
fn test_e2e_output_has_mrc_structure() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_single_page_pdf(&input_path);
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", &[1]);

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(jobs_yaml_path.to_str().unwrap())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success, stderr: {stderr}"
    );

    // Load output PDF and inspect MRC structure
    let doc = Document::load(&output_path).expect("output PDF should be loadable");
    let pages = doc.get_pages();
    assert!(!pages.is_empty(), "output should have at least one page");

    // Check first page for BgImg and FgImg XObject resources
    let first_page_id = pages.values().next().expect("should have a page");
    let page_dict = doc
        .get_dictionary(*first_page_id)
        .expect("page should be a dictionary");

    // Get Resources -> XObject dictionary
    let resources = page_dict
        .get(b"Resources")
        .expect("page should have Resources");
    let resources_dict = match resources {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve Resources ref"),
        Object::Dictionary(d) => d,
        _ => panic!("Resources should be a dictionary or reference"),
    };

    let xobject = resources_dict
        .get(b"XObject")
        .expect("Resources should have XObject");
    let xobject_dict = match xobject {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve XObject ref"),
        Object::Dictionary(d) => d,
        _ => panic!("XObject should be a dictionary or reference"),
    };

    assert!(xobject_dict.has(b"BgImg"), "XObject should contain BgImg");
    assert!(xobject_dict.has(b"FgImg"), "XObject should contain FgImg");
}

// ============================================================
// 5. E2E test: invalid page range
// ============================================================

/// jobs.yaml with page number > page count, verify CLI exits with error.
#[test]
fn test_e2e_invalid_page_range() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");

    // Create a 1-page PDF but request page 99
    create_single_page_pdf(&input_path);
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", &[99]);

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(jobs_yaml_path.to_str().unwrap())
        .output()
        .expect("failed to execute binary");

    assert!(
        !output.status.success(),
        "CLI should exit with failure for invalid page range"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERROR") || stderr.contains("error") || stderr.contains("Error"),
        "stderr should contain an error message, got: {stderr}"
    );
}

// ============================================================
// 6. E2E test: multiple job files
// ============================================================

/// Two separate job files, both processed in one invocation.
#[test]
fn test_e2e_multiple_job_files() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");

    // Create two input PDFs
    let input1 = dir.path().join("input1.pdf");
    let input2 = dir.path().join("input2.pdf");
    let output1 = dir.path().join("output1.pdf");
    let output2 = dir.path().join("output2.pdf");

    create_single_page_pdf(&input1);
    create_single_page_pdf(&input2);

    // Create two separate job files in subdirectories to avoid settings collision
    let job_dir1 = dir.path().join("job1");
    let job_dir2 = dir.path().join("job2");
    std::fs::create_dir_all(&job_dir1).expect("create job1 dir");
    std::fs::create_dir_all(&job_dir2).expect("create job2 dir");

    // Job file 1: uses absolute paths since job dir differs from PDF dir
    let jobs1_yaml = format!(
        "jobs:\n  - input: \"{}\"\n    output: \"{}\"\n    pages: \"1\"\n    dpi: 72\n",
        input1.to_string_lossy(),
        output1.to_string_lossy()
    );
    let jobs1_path = job_dir1.join("jobs.yaml");
    std::fs::write(&jobs1_path, jobs1_yaml).expect("write jobs1.yaml");

    // Job file 2: uses absolute paths
    let jobs2_yaml = format!(
        "jobs:\n  - input: \"{}\"\n    output: \"{}\"\n    pages: \"1\"\n    dpi: 72\n",
        input2.to_string_lossy(),
        output2.to_string_lossy()
    );
    let jobs2_path = job_dir2.join("jobs.yaml");
    std::fs::write(&jobs2_path, jobs2_yaml).expect("write jobs2.yaml");

    let output = cargo_bin()
        .arg(jobs1_path.to_str().unwrap())
        .arg(jobs2_path.to_str().unwrap())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success for multiple job files, stderr: {stderr}"
    );

    // Both outputs should exist and be valid PDFs
    assert!(output1.exists(), "output1.pdf should exist");
    assert!(output2.exists(), "output2.pdf should exist");

    let doc1 = Document::load(&output1).expect("output1 should be loadable");
    let doc2 = Document::load(&output2).expect("output2 should be loadable");
    assert_eq!(doc1.get_pages().len(), 1, "output1 should have 1 page");
    assert_eq!(doc2.get_pages().len(), 1, "output2 should have 1 page");
}
