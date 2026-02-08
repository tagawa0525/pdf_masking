// Phase 6: pdfium-render wrapper: page -> DynamicImage (in-memory only)

use image::DynamicImage;
use pdfium_render::prelude::*;
use std::path::PathBuf;

/// Resolves the path to the pdfium shared library.
///
/// Search order:
/// 1. `PDFIUM_DYNAMIC_LIB_PATH` environment variable
/// 2. `vendor/pdfium/lib/` relative to the project root (for development)
fn resolve_pdfium_lib_path() -> crate::error::Result<PathBuf> {
    // 1. Check environment variable
    if let Ok(path) = std::env::var("PDFIUM_DYNAMIC_LIB_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
        return Err(crate::error::PdfMaskError::render(format!(
            "PDFIUM_DYNAMIC_LIB_PATH is set to '{}' but the path does not exist",
            path
        )));
    }

    // 2. Fallback: vendor/pdfium/lib/ relative to project root
    //    In development, CARGO_MANIFEST_DIR points to the project root.
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let vendor_path = PathBuf::from(&manifest_dir).join("vendor/pdfium/lib");
        if vendor_path.exists() {
            return Ok(vendor_path);
        }
    }

    Err(crate::error::PdfMaskError::render(
        "pdfium library not found: set PDFIUM_DYNAMIC_LIB_PATH or place libpdfium.so in vendor/pdfium/lib/",
    ))
}

/// Creates a new Pdfium instance by dynamically loading the shared library.
fn create_pdfium() -> crate::error::Result<Pdfium> {
    let lib_path = resolve_pdfium_lib_path()?;
    let lib_path_str = lib_path.to_str().ok_or_else(|| {
        crate::error::PdfMaskError::render("pdfium library path contains non-UTF-8 characters")
    })?;
    let bindings =
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(lib_path_str))
            .map_err(|e| crate::error::PdfMaskError::render(e.to_string()))?;
    Ok(Pdfium::new(bindings))
}

/// Renders a PDF page at the specified DPI and returns a DynamicImage.
///
/// The PDF is loaded from disk, the specified page is rendered to an in-memory
/// bitmap at the given DPI, and the result is returned as a `DynamicImage`.
/// No intermediate files are created.
///
/// # Arguments
/// * `pdf_path` - Path to the PDF file
/// * `page_index` - 0-indexed page number
/// * `dpi` - Resolution in dots per inch (72 DPI = 1 point per pixel)
///
/// # Errors
/// Returns `PdfMaskError::RenderError` if:
/// - The pdfium library cannot be initialized
/// - The PDF file cannot be opened
/// - The page index is out of range
/// - Rendering fails
pub fn render_page(
    pdf_path: &str,
    page_index: u32,
    dpi: u32,
) -> crate::error::Result<DynamicImage> {
    let pdfium = create_pdfium()?;

    let document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| crate::error::PdfMaskError::render(e.to_string()))?;

    let page_index_u16 = u16::try_from(page_index)
        .map_err(|_| crate::error::PdfMaskError::render("page index exceeds u16 range"))?;

    let page = document
        .pages()
        .get(page_index_u16)
        .map_err(|e| crate::error::PdfMaskError::render(e.to_string()))?;

    // PDF default user unit: 1 point = 1/72 inch
    // At the given DPI, each point maps to (dpi / 72) pixels
    let width_pts = page.width().value;
    let height_pts = page.height().value;
    let width_px = (width_pts * dpi as f32 / 72.0).round() as i32;
    let height_px = (height_pts * dpi as f32 / 72.0).round() as i32;

    let config = PdfRenderConfig::new()
        .set_target_width(width_px)
        .set_target_height(height_px);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| crate::error::PdfMaskError::render(e.to_string()))?;

    Ok(bitmap.as_image())
}
