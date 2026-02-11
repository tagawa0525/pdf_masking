// Phase 6: pdfium-render wrapper: page -> DynamicImage (in-memory only)

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use image::DynamicImage;
use pdfium_render::prelude::*;

/// Resolves the path to the pdfium shared library.
///
/// Uses `PDFIUM_DYNAMIC_LIB_PATH` environment variable (set by flake.nix).
fn resolve_pdfium_lib_path() -> crate::error::Result<PathBuf> {
    let path = std::env::var("PDFIUM_DYNAMIC_LIB_PATH").map_err(|_| {
        crate::error::PdfMaskError::render(
            "PDFIUM_DYNAMIC_LIB_PATH not set: run inside `nix develop`",
        )
    })?;
    let p = PathBuf::from(&path);
    if p.exists() {
        Ok(p)
    } else {
        Err(crate::error::PdfMaskError::render(format!(
            "PDFIUM_DYNAMIC_LIB_PATH='{}' does not exist",
            path
        )))
    }
}

/// Creates a new Pdfium instance by dynamically loading the shared library.
///
/// If `PDFIUM_DYNAMIC_LIB_PATH` points to a directory, the platform-specific
/// library name is resolved within that directory. If it points to a file, the
/// file path is used directly.
fn create_pdfium() -> crate::error::Result<Pdfium> {
    let lib_path = resolve_pdfium_lib_path()?;
    let lib_path_str = lib_path.to_str().ok_or_else(|| {
        crate::error::PdfMaskError::render("pdfium library path contains non-UTF-8 characters")
    })?;
    let lib_name = if lib_path.is_dir() {
        Pdfium::pdfium_platform_library_name_at_path(lib_path_str)
    } else {
        PathBuf::from(lib_path_str)
    };
    let bindings = Pdfium::bind_to_library(lib_name)?;
    Ok(Pdfium::new(bindings))
}

// Thread-local singleton for the Pdfium instance.
//
// `Pdfium` is neither `Send` nor `Sync` (its `PdfiumLibraryBindings` trait
// object lacks those bounds), so a global static is not possible. Instead we
// use `thread_local!` to lazily load the shared library once per thread,
// avoiding the overhead of reloading it on every `render_page` call.
thread_local! {
    static PDFIUM: RefCell<Option<Pdfium>> = const { RefCell::new(None) };
}

/// Runs the given closure with a reference to the thread-local [`Pdfium`]
/// instance, initializing it on first access.
fn with_pdfium<F, R>(f: F) -> crate::error::Result<R>
where
    F: FnOnce(&Pdfium) -> crate::error::Result<R>,
{
    PDFIUM.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(create_pdfium()?);
        }
        f(opt.as_ref().expect("just initialised above"))
    })
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
/// - The path contains non-UTF-8 characters (pdfium requires UTF-8 paths)
/// - The pdfium library cannot be initialized
/// - The PDF file cannot be opened
/// - The page index is out of range
/// - Rendering fails
pub fn render_page(
    pdf_path: &Path,
    page_index: u32,
    dpi: u32,
) -> crate::error::Result<DynamicImage> {
    if dpi == 0 {
        return Err(crate::error::PdfMaskError::render(
            "dpi must be greater than 0",
        ));
    }

    let pdf_path_str = pdf_path
        .to_str()
        .ok_or_else(|| {
            crate::error::PdfMaskError::render("PDF path contains non-UTF-8 characters")
        })?
        .to_owned();

    with_pdfium(|pdfium| {
        let document = pdfium.load_pdf_from_file(&pdf_path_str, None)?;

        // pdfium-render uses u16 for page indices, limiting documents to 65536 pages max.
        let page_index_u16 = u16::try_from(page_index)
            .map_err(|_| crate::error::PdfMaskError::render("page index exceeds u16 range"))?;

        let page = document.pages().get(page_index_u16)?;

        // PDF default user unit: 1 point = 1/72 inch
        // At the given DPI, each point maps to (dpi / 72) pixels
        let width_pts = page.width().value;
        let height_pts = page.height().value;
        let width_px = (width_pts * dpi as f32 / 72.0).round() as i32;
        let height_px = (height_pts * dpi as f32 / 72.0).round() as i32;

        let config = PdfRenderConfig::new()
            .set_target_width(width_px)
            .set_target_height(height_px);

        let bitmap = page.render_with_config(&config)?;

        Ok(bitmap.as_image())
    })
}
