// Phase 6: pdfium-render wrapper: page -> DynamicImage (in-memory only)

use image::DynamicImage;

/// Renders a PDF page at the specified DPI and returns a DynamicImage.
///
/// # Arguments
/// * `pdf_path` - Path to the PDF file
/// * `page_index` - 0-indexed page number
/// * `dpi` - Resolution in dots per inch
///
/// # Errors
/// Returns `PdfMaskError::RenderError` on failure.
pub fn render_page(
    _pdf_path: &str,
    _page_index: u32,
    _dpi: u32,
) -> crate::error::Result<DynamicImage> {
    // TODO: Implement in GREEN phase
    Err(crate::error::PdfMaskError::render("not yet implemented"))
}
