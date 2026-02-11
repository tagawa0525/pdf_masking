// Phase 13: E2E integration tests
//
// End-to-end tests that verify the complete pipeline from CLI invocation
// to output PDF generation. All test PDFs are dynamically generated with
// lopdf (no committed fixtures).

use std::path::Path;
use std::process::Command;

use lopdf::{Document, Object, Stream, content::Content, dictionary};
use tracing::warn;

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

/// Check if content stream contains path painting/construction operators.
/// Parses the stream via lopdf::content::Content::decode to avoid
/// whitespace-dependent string matching.
fn content_has_path_operators(content_bytes: &[u8]) -> bool {
    let operations = match Content::decode(content_bytes) {
        Ok(content) => content.operations,
        Err(_) => return false,
    };

    // パス構築・描画オペレータを検出
    const PATH_OPS: &[&str] = &[
        "m", "l", "c", "v", "y", "h", "re", "f", "F", "f*", "B", "B*", "b", "b*", "s", "S",
    ];

    operations
        .iter()
        .any(|op| PATH_OPS.contains(&op.operator.as_str()))
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

/// Create a 1-page PDF with simple text (Type1 Helvetica) for testing settings application.
fn create_pdf_with_text(path: &Path) {
    let mut doc = Document::with_version("1.4");

    // Font: Helvetica (Type1, standard 14 fonts)
    let font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    };
    let font_id = doc.add_object(font);

    // Content stream with text
    let content = b"BT /F1 24 Tf 100 700 Td (Settings Test) Tj ET";
    let content_stream = Stream::new(dictionary! {}, content.to_vec());
    let content_id = doc.add_object(content_stream);

    // Resources with font
    let resources = dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    };

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
        "Resources" => resources,
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
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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
/// verify settings are applied (output contains XObjects proving MRC processing occurred).
#[test]
fn test_e2e_with_settings_yaml() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_pdf_with_text(&input_path);
    // settings.yaml sets dpi=72, bg_quality=30
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

    // Verify settings were applied: output should contain XObjects (TxtRgn* or BgImg/FgImg)
    // proving MRC processing occurred with the specified DPI and quality settings
    let pages = doc.get_pages();
    let page_id = pages.get(&1).expect("page 1 should exist");
    let page_obj = doc.get_object(*page_id).expect("page object should exist");
    let page_dict = page_obj.as_dict().expect("page should be a dictionary");

    let resources_ref = page_dict
        .get(b"Resources")
        .and_then(|r| r.as_reference())
        .expect("Page should have Resources reference");
    let resources = doc
        .get_object(resources_ref)
        .and_then(|obj| obj.as_dict())
        .expect("Resources should be a dictionary");

    let has_xobjects = resources
        .get(b"XObject")
        .and_then(|x| x.as_reference())
        .is_ok();

    assert!(
        has_xobjects,
        "Output should contain XObject dictionary (TxtRgn*/BgImg/FgImg), proving settings were applied"
    );
}

// ============================================================
// 5. E2E test: invalid page range
// ============================================================

/// jobs.yaml with override page number > page count, verify CLI exits with error.
#[test]
fn test_e2e_invalid_page_range() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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

/// Process a single page in BW mode. Output should contain BwImg or TxtRgn* XObjects,
/// proving BW processing occurred. 自動フォールバックチェーンにより、
/// text_to_outlines が成功すれば TextMasked、失敗すれば compose_bw (BwImg) が使われる。
#[test]
fn test_e2e_bw_mode() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let input_path = dir.path().join("input.pdf");
    let output_path = dir.path().join("output.pdf");

    create_pdf_with_text(&input_path);
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

    // Verify BW processing occurred: output should contain BwImg or TxtRgn* XObjects
    let pages = doc.get_pages();
    let page_id = pages.get(&1).expect("page 1 should exist");
    let page_obj = doc.get_object(*page_id).expect("page object should exist");
    let page_dict = page_obj.as_dict().expect("page should be a dictionary");

    let resources_ref = page_dict
        .get(b"Resources")
        .and_then(|r| r.as_reference())
        .expect("Page should have Resources reference");
    let resources = doc
        .get_object(resources_ref)
        .and_then(|obj| obj.as_dict())
        .expect("Resources should be a dictionary");

    // BW処理が行われたことを確認:
    // 1. text_to_outlinesが成功した場合: XObject辞書が空、コンテンツストリームにパス描画オペレータ
    // 2. text_to_outlinesが失敗した場合: BwImgまたはTxtRgn* XObjectが生成される
    let xobj_result = resources.get(b"XObject");

    if xobj_result.is_ok() {
        // XObjectがある場合: BwImgまたはTxtRgn*を確認
        let xobjects = xobj_result
            .and_then(|xobj| {
                xobj.as_dict().or_else(|_| {
                    xobj.as_reference()
                        .and_then(|r| doc.get_object(r)?.as_dict())
                })
            })
            .expect("XObject should be a dictionary or reference to dictionary");

        let has_bw_or_text = xobjects
            .iter()
            .any(|(k, _)| k.starts_with(b"BwImg") || k.starts_with(b"TxtRgn"));

        if !has_bw_or_text {
            // XObjectが空の場合、text_to_outlinesが成功した可能性がある
            // コンテンツストリームにパス描画オペレータが含まれることを確認
            let contents_ref = page_dict
                .get(b"Contents")
                .expect("Page should have Contents");
            let contents_obj = match contents_ref {
                Object::Reference(id) => doc.get_object(*id).expect("Contents object should exist"),
                other => other,
            };
            let mut contents_stream = contents_obj
                .as_stream()
                .expect("Contents should be a stream")
                .clone();
            // コンテンツストリームを解凍
            contents_stream
                .decompress()
                .expect("Failed to decompress content stream");
            let content_bytes = &contents_stream.content;

            // パス描画オペレータの存在を確認（lopdf::content::Contentでパース）
            assert!(
                content_has_path_operators(content_bytes),
                "BW mode output with empty XObject should contain path drawing operators (text_to_outlines succeeded)"
            );
        }
    } else {
        // XObjectがない場合: text_to_outlinesが成功したはず
        // コンテンツストリームにパス描画オペレータが含まれることを確認
        let contents_ref = page_dict
            .get(b"Contents")
            .expect("Page should have Contents");
        let contents_obj = match contents_ref {
            Object::Reference(id) => doc.get_object(*id).expect("Contents object should exist"),
            other => other,
        };
        let mut contents_stream = contents_obj
            .as_stream()
            .expect("Contents should be a stream")
            .clone();
        // コンテンツストリームを解凍
        contents_stream
            .decompress()
            .expect("Failed to decompress content stream");
        let content_bytes = &contents_stream.content;

        // パス描画オペレータの存在を確認（lopdf::content::Contentでパース）
        assert!(
            content_has_path_operators(content_bytes),
            "BW mode output without XObject should contain path drawing operators (text_to_outlines succeeded)"
        );
    }
}

// ============================================================
// 8. E2E test: Grayscale mode
// ============================================================

/// Process a single page in grayscale mode.
/// Output should be a valid PDF with 1 page.
#[test]
fn test_e2e_grayscale_mode() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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
}

// ============================================================
// 9. E2E test: Skip mode (page copy)
// ============================================================

/// Process with skip_pages to copy a page as-is from source PDF.
#[test]
fn test_e2e_skip_mode() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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

/// 3-page PDF with: page 1 = RGB, page 2 = BW, page 3 = skip.
#[test]
fn test_e2e_mixed_mode() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    if !pdfium_available() {
        warn!("Skipping: PDFIUM_DYNAMIC_LIB_PATH not set (run inside `nix develop`)");
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
}
