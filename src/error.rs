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

impl PdfMaskError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::ConfigError(msg.into())
    }

    pub fn pdf_read(msg: impl Into<String>) -> Self {
        Self::PdfReadError(msg.into())
    }

    pub fn pdf_write(msg: impl Into<String>) -> Self {
        Self::PdfWriteError(msg.into())
    }

    pub fn content_stream(msg: impl Into<String>) -> Self {
        Self::ContentStreamError(msg.into())
    }

    pub fn render(msg: impl Into<String>) -> Self {
        Self::RenderError(msg.into())
    }

    pub fn segmentation(msg: impl Into<String>) -> Self {
        Self::SegmentationError(msg.into())
    }

    pub fn jbig2_encode(msg: impl Into<String>) -> Self {
        Self::Jbig2EncodeError(msg.into())
    }

    pub fn jpeg_encode(msg: impl Into<String>) -> Self {
        Self::JpegEncodeError(msg.into())
    }

    pub fn image_xobject(msg: impl Into<String>) -> Self {
        Self::ImageXObjectError(msg.into())
    }

    pub fn cache(msg: impl Into<String>) -> Self {
        Self::CacheError(msg.into())
    }

    pub fn linearize(msg: impl Into<String>) -> Self {
        Self::LinearizeError(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, PdfMaskError>;
