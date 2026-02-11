// Phase 10: ページ単位処理: キャッシュ確認 → MRC合成 → キャッシュ保存

use std::collections::HashMap;
use std::path::Path;

use image::DynamicImage;

use crate::cache::hash::{CacheSettings, compute_cache_key};
use crate::cache::store::CacheStore;
use crate::config::job::ColorMode;
use crate::mrc::compositor::{
    MrcConfig, TextMaskedParams, TextOutlinesParams, compose, compose_bw, compose_text_masked,
    compose_text_outlines,
};
use crate::mrc::{PageOutput, SkipData};
use crate::pdf::font::ParsedFont;

/// Single page processing result.
pub struct ProcessedPage {
    pub page_index: u32,
    pub output: PageOutput,
    pub cache_key: String,
}

/// Parameters for [`process_page_outlines`].
pub struct ProcessPageOutlinesParams<'a> {
    pub page_index: u32,
    pub content_stream: &'a [u8],
    pub cache_settings: &'a CacheSettings,
    pub cache_store: Option<&'a CacheStore>,
    pub pdf_path: &'a Path,
    pub image_streams: Option<&'a HashMap<String, lopdf::Stream>>,
    pub fonts: &'a HashMap<String, ParsedFont>,
}

impl ProcessPageOutlinesParams<'_> {
    /// Process a page using text_to_outlines without bitmap rendering.
    ///
    /// This path skips pdfium rendering entirely. It converts text to vector outlines
    /// and performs image redaction using only PDF-level data.
    ///
    /// Returns `Err` if text-to-outlines conversion fails (e.g. missing fonts).
    pub fn process(&self) -> crate::error::Result<ProcessedPage> {
        let color_mode = self.cache_settings.color_mode;

        // text_to_outlinesはRGB/Grayscale/Bwに対応
        if !matches!(
            color_mode,
            ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw
        ) {
            return Err(crate::error::PdfMaskError::config(format!(
                "unsupported color mode for process_page_outlines: {:?} (supported: Rgb, Grayscale, Bw)",
                color_mode
            )));
        }

        let cache_key = compute_cache_key(
            self.content_stream,
            self.cache_settings,
            self.pdf_path,
            self.page_index,
        );

        // Check cache (no bitmap dimensions for outlines path)
        if let Some(store) = self.cache_store
            && let Some(cached) = store.retrieve(&cache_key, color_mode, None)?
            && !matches!(&cached, PageOutput::Skip(_))
        {
            return Ok(ProcessedPage {
                page_index: self.page_index,
                output: cached,
                cache_key,
            });
        }

        // Run compose_text_outlines (no bitmap needed)
        let empty_streams = HashMap::new();
        let streams = self.image_streams.unwrap_or(&empty_streams);
        let outlines_params = TextOutlinesParams {
            content_bytes: self.content_stream,
            fonts: self.fonts,
            image_streams: streams,
            color_mode,
            page_index: self.page_index,
        };
        let data = compose_text_outlines(&outlines_params)?;
        let output = PageOutput::TextMasked(data);

        // Store in cache
        if let Some(store) = self.cache_store {
            store.store(&cache_key, &output, None)?;
        }

        Ok(ProcessedPage {
            page_index: self.page_index,
            output,
            cache_key,
        })
    }
}

/// Process a page using text_to_outlines without bitmap rendering (backward-compatible wrapper).
///
/// Prefer constructing [`ProcessPageOutlinesParams`] and calling
/// [`ProcessPageOutlinesParams::process`].
#[allow(clippy::too_many_arguments)]
pub fn process_page_outlines(
    page_index: u32,
    content_stream: &[u8],
    cache_settings: &CacheSettings,
    cache_store: Option<&CacheStore>,
    pdf_path: &Path,
    image_streams: Option<&HashMap<String, lopdf::Stream>>,
    fonts: &HashMap<String, ParsedFont>,
) -> crate::error::Result<ProcessedPage> {
    let params = ProcessPageOutlinesParams {
        page_index,
        content_stream,
        cache_settings,
        cache_store,
        pdf_path,
        image_streams,
        fonts,
    };
    params.process()
}

/// Parameters for [`process_page`].
pub struct ProcessPageParams<'a> {
    pub page_index: u32,
    pub bitmap: &'a DynamicImage,
    pub content_stream: &'a [u8],
    pub mrc_config: &'a MrcConfig,
    pub cache_settings: &'a CacheSettings,
    pub cache_store: Option<&'a CacheStore>,
    pub pdf_path: &'a Path,
    pub image_streams: Option<&'a HashMap<String, lopdf::Stream>>,
}

impl ProcessPageParams<'_> {
    /// Process a single page: check cache -> mode-specific composition -> store in cache.
    ///
    /// Cache key is computed from content_stream, settings, pdf_path, and page_index.
    /// If cache hits and dimensions match the bitmap, return cached output.
    /// Otherwise, perform mode-specific composition:
    ///
    /// - Skip: Return empty ProcessedPage without MRC encoding
    /// - Bw: Full-page JBIG2 encoding via compose_bw
    /// - Rgb/Grayscale: Try compose_text_masked (text-only JPEG); fallback to compose (full-page MRC) on failure
    pub fn process(&self) -> crate::error::Result<ProcessedPage> {
        let color_mode = self.cache_settings.color_mode;

        // Skip モードはMRC処理不要
        if color_mode == ColorMode::Skip {
            return Ok(ProcessedPage {
                page_index: self.page_index,
                output: PageOutput::Skip(SkipData {
                    page_index: self.page_index,
                }),
                cache_key: String::new(),
            });
        }

        let cache_key = compute_cache_key(
            self.content_stream,
            self.cache_settings,
            self.pdf_path,
            self.page_index,
        );

        let bitmap_width = self.bitmap.width();
        let bitmap_height = self.bitmap.height();

        // Check cache first (retrieve checks bitmap dimensions internally)
        if let Some(store) = self.cache_store
            && let Some(cached) =
                store.retrieve(&cache_key, color_mode, Some((bitmap_width, bitmap_height)))?
        {
            match &cached {
                PageOutput::Skip(_) => {}
                _ => {
                    return Ok(ProcessedPage {
                        page_index: self.page_index,
                        output: cached,
                        cache_key,
                    });
                }
            }
        }

        // Cache miss: run MRC composition
        let rgba_image = self.bitmap.to_rgba8();
        let (width, height) = (rgba_image.width(), rgba_image.height());
        let rgba_data = rgba_image.into_raw();

        let output = match color_mode {
            ColorMode::Bw => {
                let bw_layers = compose_bw(&rgba_data, width, height)?;
                PageOutput::BwMask(bw_layers)
            }
            mode @ (ColorMode::Rgb | ColorMode::Grayscale) => {
                let empty_streams = HashMap::new();
                let streams = self.image_streams.unwrap_or(&empty_streams);

                let page_width_pts = width as f64 * 72.0 / self.cache_settings.dpi as f64;
                let page_height_pts = height as f64 * 72.0 / self.cache_settings.dpi as f64;
                let params = TextMaskedParams {
                    content_bytes: self.content_stream,
                    rgba_data: &rgba_data,
                    bitmap_width: width,
                    bitmap_height: height,
                    page_width_pts,
                    page_height_pts,
                    image_streams: streams,
                    color_mode: mode,
                    page_index: self.page_index,
                };

                match compose_text_masked(&params) {
                    Ok(data) => PageOutput::TextMasked(data),
                    Err(e) => {
                        tracing::warn!(
                            "page {}: compose_text_masked failed, falling back to full MRC: {}",
                            self.page_index,
                            e
                        );
                        let mrc_layers = compose(&rgba_data, width, height, self.mrc_config, mode)?;
                        PageOutput::Mrc(mrc_layers)
                    }
                }
            }
            ColorMode::Skip => unreachable!("Skip handled above"),
        };

        // Store in cache if available
        if let Some(store) = self.cache_store {
            store.store(&cache_key, &output, Some((bitmap_width, bitmap_height)))?;
        }

        Ok(ProcessedPage {
            page_index: self.page_index,
            output,
            cache_key,
        })
    }
}

/// Process a single page (backward-compatible wrapper).
///
/// Prefer constructing [`ProcessPageParams`] and calling [`ProcessPageParams::process`].
#[allow(clippy::too_many_arguments)]
pub fn process_page(
    page_index: u32,
    bitmap: &DynamicImage,
    content_stream: &[u8],
    mrc_config: &MrcConfig,
    cache_settings: &CacheSettings,
    cache_store: Option<&CacheStore>,
    pdf_path: &Path,
    image_streams: Option<&HashMap<String, lopdf::Stream>>,
) -> crate::error::Result<ProcessedPage> {
    let params = ProcessPageParams {
        page_index,
        bitmap,
        content_stream,
        mrc_config,
        cache_settings,
        cache_store,
        pdf_path,
        image_streams,
    };
    params.process()
}
