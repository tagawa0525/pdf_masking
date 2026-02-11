// Phase 10: ジョブ単位: PDF読込 -> 並列ページ処理 -> 出力PDF組立

use std::path::PathBuf;

use rayon::prelude::*;

use crate::cache::hash::CacheSettings;
use crate::cache::store::CacheStore;
use crate::config::job::ColorMode;
use crate::error::PdfMaskError;
use crate::mrc::compositor::MrcConfig;
use crate::mrc::{PageOutput, SkipData};
use crate::pdf::reader::PdfReader;
use crate::pdf::writer::MrcPageWriter;
use crate::pipeline::page_processor::{
    ProcessPageOutlinesParams, ProcessPageParams, ProcessedPage,
};
use crate::render::pdfium::render_page;

/// Configuration for a single job.
pub struct JobConfig {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    /// Default color mode for pages not in overrides map.
    pub default_color_mode: ColorMode,
    /// 1-based page overrides (from resolve_page_modes).
    pub color_mode_overrides: std::collections::HashMap<u32, ColorMode>,
    pub dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub cache_dir: Option<PathBuf>,
}

/// Result of processing a single job.
pub struct JobResult {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub pages_processed: usize,
}

/// Intermediate data for a page after content stream analysis (Phase A).
struct AnalysisResult {
    page_idx: u32,
    mode: ColorMode,
    content: Vec<u8>,
    image_streams: Option<std::collections::HashMap<String, lopdf::Stream>>,
    fonts: Option<std::collections::HashMap<String, crate::pdf::font::ParsedFont>>,
}

/// Intermediate data for a page after rendering (Phase B).
struct RenderResult {
    page_idx: u32,
    mode: ColorMode,
    bitmap: image::DynamicImage,
    content: Vec<u8>,
    image_streams: Option<std::collections::HashMap<String, lopdf::Stream>>,
}

/// Run a single PDF masking job through the 4-phase pipeline.
///
/// Phase A: Content stream analysis (sequential, skip Skip pages)
/// Phase A2: Text-to-outlines conversion (skip if not eligible)
/// Phase B+C: Page rendering + MRC processing (rayon parallel)
/// Phase D: PDF assembly + optimization (sequential)
pub fn run_job(config: &JobConfig) -> crate::error::Result<JobResult> {
    let reader = PdfReader::open(&config.input_path)?;
    let page_count = reader.page_count();

    tracing::debug!(
        input = %config.input_path.display(),
        pages = page_count,
        "starting job"
    );

    // Validate override page numbers are within range
    for &page_num in config.color_mode_overrides.keys() {
        if page_num < 1 || page_num > page_count {
            return Err(PdfMaskError::pdf_read(format!(
                "override page {} out of range (document has {} pages)",
                page_num, page_count
            )));
        }
    }

    // Build page_modes for all pages (convert 1-based to 0-based)
    let page_modes: Vec<(u32, ColorMode)> = (1..=page_count)
        .map(|p| {
            let mode = config
                .color_mode_overrides
                .get(&p)
                .copied()
                .unwrap_or(config.default_color_mode);
            (p - 1, mode)
        })
        .collect();

    let cache_store = config.cache_dir.as_ref().map(CacheStore::new);

    // Phase A: Content stream analysis
    tracing::debug!("phase A: analyzing content streams");
    let content_streams = phase_a_analyze(&reader, &page_modes)?;

    // Phase A2: Text-to-outlines conversion
    tracing::debug!("phase A2: text-to-outlines conversion");
    let (outlines_pages, needs_rendering) =
        phase_a2_text_to_outlines(content_streams, config, cache_store.as_ref())?;

    // Phase B+C: Rendering and MRC composition
    tracing::debug!(
        rendering = needs_rendering.len(),
        outlines = outlines_pages.len(),
        "phase B+C: rendering and MRC composition"
    );
    let successful_pages = phase_bc_render_and_mrc(
        needs_rendering,
        outlines_pages,
        &page_modes,
        config,
        cache_store.as_ref(),
    )?;

    let pages_processed = successful_pages.len();

    // Phase D: PDF output assembly
    tracing::debug!("phase D: PDF assembly");
    phase_d_write(&reader, &successful_pages, config, pages_processed)
}

/// Phase A: Content stream analysis (sequential).
///
/// Reads content streams, image streams, and fonts for all non-Skip pages.
fn phase_a_analyze(
    reader: &PdfReader,
    page_modes: &[(u32, ColorMode)],
) -> crate::error::Result<Vec<AnalysisResult>> {
    let non_skip: Vec<(u32, ColorMode)> = page_modes
        .iter()
        .filter(|(_, mode)| *mode != ColorMode::Skip)
        .copied()
        .collect();

    let mut content_streams: Vec<AnalysisResult> = Vec::new();
    for &(page_idx, mode) in &non_skip {
        let page_num = page_idx + 1;
        let content = reader.page_content_stream(page_num)?;
        let image_streams = if matches!(mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw)
        {
            let streams = reader.page_image_streams(page_num)?;
            if streams.is_empty() {
                None
            } else {
                Some(streams)
            }
        } else {
            None
        };
        let fonts = if matches!(mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw) {
            crate::pdf::font::parse_page_fonts(reader.document(), page_num).ok()
        } else {
            None
        };

        content_streams.push(AnalysisResult {
            page_idx,
            mode,
            content,
            image_streams,
            fonts,
        });
    }
    Ok(content_streams)
}

/// Phase A2: Text-to-outlines conversion.
///
/// Attempts text-to-outlines for eligible pages. Pages that fail or are
/// ineligible are returned in `needs_rendering` for bitmap-based processing.
fn phase_a2_text_to_outlines(
    content_streams: Vec<AnalysisResult>,
    config: &JobConfig,
    cache_store: Option<&CacheStore>,
) -> crate::error::Result<(Vec<ProcessedPage>, Vec<AnalysisResult>)> {
    let mut outlines_pages: Vec<ProcessedPage> = Vec::new();
    let mut needs_rendering: Vec<AnalysisResult> = Vec::new();

    for cs in content_streams {
        let eligible = matches!(
            cs.mode,
            ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Bw
        ) && cs.fonts.is_some();

        if eligible {
            let cache_settings = CacheSettings {
                dpi: config.dpi,
                fg_dpi: config.dpi,
                bg_quality: config.bg_quality,
                fg_quality: config.fg_quality,
                color_mode: cs.mode,
            };
            let params = ProcessPageOutlinesParams {
                page_index: cs.page_idx,
                content_stream: &cs.content,
                cache_settings: &cache_settings,
                cache_store,
                pdf_path: &config.input_path,
                image_streams: cs.image_streams.as_ref(),
                fonts: cs.fonts.as_ref().unwrap(),
            };
            let result = params.process();
            match result {
                Ok(page) => {
                    outlines_pages.push(page);
                    continue;
                }
                Err(_) => {
                    needs_rendering.push(cs);
                }
            }
        } else {
            needs_rendering.push(cs);
        }
    }
    Ok((outlines_pages, needs_rendering))
}

/// Phase B+C: Page rendering (sequential) and MRC processing (rayon parallel).
///
/// Renders pages that need bitmaps, then runs MRC composition in parallel.
/// Skip pages are appended with no processing.
fn phase_bc_render_and_mrc(
    needs_rendering: Vec<AnalysisResult>,
    outlines_pages: Vec<ProcessedPage>,
    page_modes: &[(u32, ColorMode)],
    config: &JobConfig,
    cache_store: Option<&CacheStore>,
) -> crate::error::Result<Vec<ProcessedPage>> {
    // --- Phase B: Page rendering (sequential, only pages needing bitmap) ---
    let mut pages_data: Vec<RenderResult> = Vec::new();
    for cs in needs_rendering {
        let bitmap = render_page(&config.input_path, cs.page_idx, config.dpi)?;
        pages_data.push(RenderResult {
            page_idx: cs.page_idx,
            mode: cs.mode,
            bitmap,
            content: cs.content,
            image_streams: cs.image_streams,
        });
    }

    // --- Phase C: MRC processing (rayon parallel, outlines already handled) ---
    let mrc_config = MrcConfig {
        bg_quality: config.bg_quality,
        fg_quality: config.fg_quality,
    };

    let processed: Vec<crate::error::Result<ProcessedPage>> = pages_data
        .par_iter()
        .map(|pd| {
            let cache_settings = CacheSettings {
                dpi: config.dpi,
                fg_dpi: config.dpi,
                bg_quality: config.bg_quality,
                fg_quality: config.fg_quality,
                color_mode: pd.mode,
            };
            let params = ProcessPageParams {
                page_index: pd.page_idx,
                bitmap: &pd.bitmap,
                content_stream: &pd.content,
                mrc_config: &mrc_config,
                cache_settings: &cache_settings,
                cache_store,
                pdf_path: &config.input_path,
                image_streams: pd.image_streams.as_ref(),
            };
            params.process()
        })
        .collect();

    // Collect all results
    let mut successful_pages: Vec<ProcessedPage> = outlines_pages;
    for result in processed {
        successful_pages.push(result?);
    }

    // Add skip pages directly (no rendering or MRC processing needed)
    for &(page_idx, mode) in page_modes {
        if mode == ColorMode::Skip {
            successful_pages.push(ProcessedPage {
                page_index: page_idx,
                output: PageOutput::Skip(SkipData {
                    page_index: page_idx,
                }),
                cache_key: String::new(),
            });
        }
    }

    // Sort by page index for deterministic output
    successful_pages.sort_by_key(|p| p.page_index);

    Ok(successful_pages)
}

/// Phase D: PDF assembly + optimization (sequential).
///
/// Writes all processed pages into a new PDF document and optimizes it.
fn phase_d_write(
    reader: &PdfReader,
    successful_pages: &[ProcessedPage],
    config: &JobConfig,
    pages_processed: usize,
) -> crate::error::Result<JobResult> {
    let mut writer = MrcPageWriter::new();
    let mut masked_page_ids: Vec<lopdf::ObjectId> = Vec::new();
    for page in successful_pages {
        match &page.output {
            PageOutput::Mrc(layers) => {
                let page_id = writer.write_mrc_page(layers)?;
                masked_page_ids.push(page_id);
            }
            PageOutput::BwMask(bw) => {
                let page_id = writer.write_bw_page(bw)?;
                masked_page_ids.push(page_id);
            }
            PageOutput::Skip(_) => {
                let page_num = page.page_index + 1; // 1-based
                writer.copy_page_from(reader.document(), page_num)?;
                // Skip pages are NOT added to masked_page_ids (no font optimization)
            }
            PageOutput::TextMasked(data) => {
                let page_num = page.page_index + 1;
                let page_id = writer.write_text_masked_page(reader.document(), page_num, data)?;
                masked_page_ids.push(page_id);
            }
        }
    }

    // Run optimization on the assembled document
    crate::pdf::optimizer::optimize(writer.document_mut(), &masked_page_ids)?;

    let pdf_bytes = writer.save_to_bytes()?;
    std::fs::write(&config.output_path, pdf_bytes)?;

    Ok(JobResult {
        input_path: config.input_path.clone(),
        output_path: config.output_path.clone(),
        pages_processed,
    })
}
