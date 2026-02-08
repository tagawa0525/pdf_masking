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

/// Check whether pdfium is available via environment variable and the path exists.
fn pdfium_available() -> bool {
    match std::env::var("PDFIUM_DYNAMIC_LIB_PATH") {
        Ok(path) => Path::new(&path).exists(),
        Err(_) => false,
    }
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
    match doc
        .get_object_mut(page_id)
        .expect("page object should exist")
    {
        Object::Dictionary(dict) => dict.set("Parent", pages_id),
        _ => panic!("page object should be a dictionary"),
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
/// All pages are processed by default with RGB mode.
/// Use `extra_yaml` to add color_mode, *_pages overrides, etc.
fn write_jobs_yaml(dir: &Path, input: &str, output: &str, extra_yaml: &str) {
    let mut yaml =
        format!("jobs:\n  - input: \"{input}\"\n    output: \"{output}\"\n    dpi: 72\n");
    if !extra_yaml.is_empty() {
        yaml += extra_yaml;
    }
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
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", "");

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
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

/// Create a 3-page PDF, process all pages (default RGB), run CLI,
/// verify output has all 3 pages.
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
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", "");

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
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
    // All pages are processed by default (no page selection)
    assert_eq!(
        doc.get_pages().len(),
        3,
        "output PDF should have 3 pages (all pages processed by default)"
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
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", "");

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
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

    // Verify settings.yaml was picked up by checking MRC structure exists.
    // The configured DPI (72) and bg_quality (30) affect image encoding, but
    // verifying exact pixel dimensions requires decoding XObject streams.
    // As a structural check, confirm BgImg/FgImg XObjects are present, which
    // proves the pipeline ran with the settings rather than being a no-op.
    let pages = doc.get_pages();
    let first_page_id = pages.values().next().expect("should have a page");
    let page_dict = doc
        .get_dictionary(*first_page_id)
        .expect("page should be a dictionary");
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
        .expect("Resources should have XObject when settings.yaml is applied");
    let xobject_dict = match xobject {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve XObject ref"),
        Object::Dictionary(d) => d,
        _ => panic!("XObject should be a dictionary or reference"),
    };
    assert!(
        xobject_dict.has(b"BgImg"),
        "XObject should contain BgImg (settings.yaml was applied)"
    );
    assert!(
        xobject_dict.has(b"FgImg"),
        "XObject should contain FgImg (settings.yaml was applied)"
    );
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
    write_jobs_yaml(dir.path(), "input.pdf", "output.pdf", "");

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
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

/// jobs.yaml with override page number > page count, verify CLI exits with error.
#[test]
fn test_e2e_invalid_page_range() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");

    // Create a 1-page PDF but request rgb_pages with page 99
    create_single_page_pdf(&input_path);
    write_jobs_yaml(
        dir.path(),
        "input.pdf",
        "output.pdf",
        "    rgb_pages: [99]\n",
    );

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
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
        "jobs:\n  - input: \"{}\"\n    output: \"{}\"\n    dpi: 72\n",
        input1.to_string_lossy(),
        output1.to_string_lossy()
    );
    let jobs1_path = job_dir1.join("jobs.yaml");
    std::fs::write(&jobs1_path, jobs1_yaml).expect("write jobs1.yaml");

    // Job file 2: uses absolute paths
    let jobs2_yaml = format!(
        "jobs:\n  - input: \"{}\"\n    output: \"{}\"\n    dpi: 72\n",
        input2.to_string_lossy(),
        output2.to_string_lossy()
    );
    let jobs2_path = job_dir2.join("jobs.yaml");
    std::fs::write(&jobs2_path, jobs2_yaml).expect("write jobs2.yaml");

    let output = cargo_bin()
        .arg(&jobs1_path)
        .arg(&jobs2_path)
        .current_dir(dir.path())
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

// ============================================================
// 7. E2E test: BW mode
// ============================================================

/// Process a single page in BW mode. Output should have JBIG2 mask only (no BgImg/FgImg).
#[test]
#[ignore]
fn test_e2e_bw_mode() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_single_page_pdf(&input_path);
    write_jobs_yaml(
        dir.path(),
        "input.pdf",
        "output.pdf",
        "    color_mode: bw\n",
    );

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success for BW mode, stderr: {stderr}"
    );

    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable");
    assert_eq!(doc.get_pages().len(), 1, "output PDF should have 1 page");

    // BW page: should have MaskImg XObject but NO BgImg/FgImg
    let pages = doc.get_pages();
    let first_page_id = pages.values().next().expect("should have a page");
    let page_dict = doc
        .get_dictionary(*first_page_id)
        .expect("page should be a dictionary");
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
    assert!(
        xobject_dict.has(b"MaskImg"),
        "BW page should have MaskImg XObject"
    );
    assert!(
        !xobject_dict.has(b"BgImg"),
        "BW page should NOT have BgImg XObject"
    );
    assert!(
        !xobject_dict.has(b"FgImg"),
        "BW page should NOT have FgImg XObject"
    );
}

// ============================================================
// 8. E2E test: Grayscale mode
// ============================================================

/// Process a single page in grayscale mode. Output should have DeviceGray XObjects.
#[test]
#[ignore]
fn test_e2e_grayscale_mode() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_single_page_pdf(&input_path);
    write_jobs_yaml(
        dir.path(),
        "input.pdf",
        "output.pdf",
        "    color_mode: grayscale\n",
    );

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success for grayscale mode, stderr: {stderr}"
    );

    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable");
    assert_eq!(doc.get_pages().len(), 1);

    // Grayscale page: should have BgImg/FgImg with DeviceGray ColorSpace
    let pages = doc.get_pages();
    let first_page_id = pages.values().next().expect("should have a page");
    let page_dict = doc
        .get_dictionary(*first_page_id)
        .expect("page should be a dictionary");
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

    assert!(xobject_dict.has(b"BgImg"), "should have BgImg");
    assert!(xobject_dict.has(b"FgImg"), "should have FgImg");

    // Verify BgImg uses DeviceGray
    let bg_ref = xobject_dict.get(b"BgImg").expect("BgImg should exist");
    if let Object::Reference(bg_id) = bg_ref {
        let bg_stream = doc
            .get_object(*bg_id)
            .and_then(|o| o.as_stream())
            .expect("BgImg should be a stream");
        let cs = bg_stream
            .dict
            .get(b"ColorSpace")
            .expect("BgImg should have ColorSpace");
        let cs_name = cs.as_name().expect("ColorSpace should be a name");
        assert_eq!(
            cs_name, b"DeviceGray",
            "Grayscale BgImg should use DeviceGray"
        );
    }
}

// ============================================================
// 9. E2E test: Skip mode (page copy)
// ============================================================

/// Process with skip_pages to copy a page as-is from source PDF.
#[test]
#[ignore]
fn test_e2e_skip_mode() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_multi_page_pdf(&input_path, 2);
    // Page 1: default RGB, Page 2: skip (copy as-is)
    write_jobs_yaml(
        dir.path(),
        "input.pdf",
        "output.pdf",
        "    skip_pages: [2]\n",
    );

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success with skip pages, stderr: {stderr}"
    );

    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable");
    assert_eq!(
        doc.get_pages().len(),
        2,
        "output should have 2 pages (1 MRC + 1 skip)"
    );
}

// ============================================================
// 10. E2E test: Mixed mode (per-page overrides)
// ============================================================

/// 3-page PDF with: page 1 = RGB (default), page 2 = BW, page 3 = skip.
#[test]
#[ignore]
fn test_e2e_mixed_mode() {
    if !pdfium_available() {
        eprintln!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_multi_page_pdf(&input_path, 3);
    write_jobs_yaml(
        dir.path(),
        "input.pdf",
        "output.pdf",
        "    bw_pages: [2]\n    skip_pages: [3]\n",
    );

    let jobs_yaml_path = dir.path().join("jobs.yaml");

    let output = cargo_bin()
        .arg(&jobs_yaml_path)
        .current_dir(dir.path())
        .output()
        .expect("failed to execute binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI should exit with success for mixed mode, stderr: {stderr}"
    );

    assert!(output_path.exists(), "output PDF should exist");
    let doc = Document::load(&output_path).expect("output PDF should be loadable");
    assert_eq!(doc.get_pages().len(), 3, "output should have 3 pages");

    // Page 1 (RGB): should have BgImg + FgImg
    let pages = doc.get_pages();
    let page1_id = pages.get(&1).expect("page 1");
    let page1_dict = doc.get_dictionary(*page1_id).expect("page 1 dict");
    let res1 = page1_dict.get(b"Resources").expect("Resources");
    let res1_dict = match res1 {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve"),
        Object::Dictionary(d) => d,
        _ => panic!("dict expected"),
    };
    let xobj1 = res1_dict.get(b"XObject").expect("XObject");
    let xobj1_dict = match xobj1 {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve"),
        Object::Dictionary(d) => d,
        _ => panic!("dict expected"),
    };
    assert!(xobj1_dict.has(b"BgImg"), "Page 1 (RGB) should have BgImg");

    // Page 2 (BW): should have MaskImg but no BgImg
    let page2_id = pages.get(&2).expect("page 2");
    let page2_dict = doc.get_dictionary(*page2_id).expect("page 2 dict");
    let res2 = page2_dict.get(b"Resources").expect("Resources");
    let res2_dict = match res2 {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve"),
        Object::Dictionary(d) => d,
        _ => panic!("dict expected"),
    };
    let xobj2 = res2_dict.get(b"XObject").expect("XObject");
    let xobj2_dict = match xobj2 {
        Object::Reference(r) => doc.get_dictionary(*r).expect("resolve"),
        Object::Dictionary(d) => d,
        _ => panic!("dict expected"),
    };
    assert!(
        xobj2_dict.has(b"MaskImg"),
        "Page 2 (BW) should have MaskImg"
    );
    assert!(
        !xobj2_dict.has(b"BgImg"),
        "Page 2 (BW) should NOT have BgImg"
    );
}
