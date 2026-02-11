use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfMaskError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("PDF read error: {0}")]
    PdfReadError(String),

    #[error("PDF write error: {0}")]
    PdfWriteError(String),

    #[error("Content stream error: {0}")]
    ContentStreamError(String),

    #[error("Render error: {0}")]
    RenderError(String),

    #[error("Segmentation error: {0}")]
    SegmentationError(String),

    #[error("JBIG2 encode error: {0}")]
    Jbig2EncodeError(String),

    #[error("JPEG encode error: {0}")]
    JpegEncodeError(String),

    #[error("Image XObject error: {0}")]
    ImageXObjectError(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("Linearize error: {0}")]
    LinearizeError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Generates factory methods for [`PdfMaskError`] variants that wrap a `String`.
macro_rules! error_constructors {
    ($(
        $(#[doc = $doc:expr])*
        $method:ident => $variant:ident
    ),* $(,)?) => {
        impl PdfMaskError {
            $(
                $(#[doc = $doc])*
                pub fn $method(msg: impl Into<String>) -> Self {
                    Self::$variant(msg.into())
                }
            )*
        }
    };
}

error_constructors! {
    /// Create a configuration error.
    config => ConfigError,
    /// Create a PDF read error.
    pdf_read => PdfReadError,
    /// Create a PDF write error.
    pdf_write => PdfWriteError,
    /// Create a content stream error.
    content_stream => ContentStreamError,
    /// Create a render error.
    render => RenderError,
    /// Create a segmentation error.
    segmentation => SegmentationError,
    /// Create a JBIG2 encode error.
    jbig2_encode => Jbig2EncodeError,
    /// Create a JPEG encode error.
    jpeg_encode => JpegEncodeError,
    /// Create an image XObject error.
    image_xobject => ImageXObjectError,
    /// Create a cache error.
    cache => CacheError,
    /// Create a linearize error.
    linearize => LinearizeError,
}

impl From<lopdf::Error> for PdfMaskError {
    fn from(e: lopdf::Error) -> Self {
        Self::PdfReadError(e.to_string())
    }
}

impl From<serde_json::Error> for PdfMaskError {
    fn from(e: serde_json::Error) -> Self {
        Self::CacheError(e.to_string())
    }
}

impl From<serde_yml::Error> for PdfMaskError {
    fn from(e: serde_yml::Error) -> Self {
        Self::ConfigError(e.to_string())
    }
}

#[cfg(feature = "mrc")]
impl From<pdfium_render::prelude::PdfiumError> for PdfMaskError {
    fn from(e: pdfium_render::prelude::PdfiumError) -> Self {
        Self::RenderError(e.to_string())
    }
}

impl From<image::ImageError> for PdfMaskError {
    fn from(e: image::ImageError) -> Self {
        Self::JpegEncodeError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PdfMaskError>;
