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
use crate::pipeline::page_processor::{ProcessedPage, process_page, process_page_outlines};
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
struct ContentStreamData {
    page_idx: u32,
    mode: ColorMode,
    content: Vec<u8>,
    image_streams: Option<std::collections::HashMap<String, lopdf::Stream>>,
    fonts: Option<std::collections::HashMap<String, crate::pdf::font::ParsedFont>>,
}

/// Intermediate data for a page after rendering (Phase B).
struct PageRenderData {
    page_idx: u32,
    mode: ColorMode,
    bitmap: image::DynamicImage,
    content: Vec<u8>,
    image_streams: Option<std::collections::HashMap<String, lopdf::Stream>>,
}

/// Run a single PDF masking job through the 4-phase pipeline.
///
/// Phase A: Content stream analysis (sequential, skip Skip pages)
/// Phase B: Page rendering (sequential, skip Skip pages)
/// Phase C: MRC processing (rayon parallel)
/// Phase D: PDF assembly + optimization (sequential)
pub fn run_job(config: &JobConfig) -> crate::error::Result<JobResult> {
    let reader = PdfReader::open(&config.input_path)?;
    let page_count = reader.page_count();

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

    // --- Phase A: Content stream analysis (sequential) ---
    // Skip pages don't need content streams or rendering.
    let non_skip: Vec<(u32, ColorMode)> = page_modes
        .iter()
        .filter(|(_, mode)| *mode != ColorMode::Skip)
        .copied()
        .collect();

    let mut content_streams: Vec<ContentStreamData> = Vec::new();
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

        content_streams.push(ContentStreamData {
            page_idx,
            mode,
            content,
            image_streams,
            fonts,
        });
    }

    // --- Phase A2: Text-to-outlines (no rendering required) ---
    let cache_store = config.cache_dir.as_ref().map(CacheStore::new);
    let mut outlines_pages: Vec<ProcessedPage> = Vec::new();
    let mut needs_rendering: Vec<ContentStreamData> = Vec::new();

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
            let result = process_page_outlines(
                cs.page_idx,
                &cs.content,
                &cache_settings,
                cache_store.as_ref(),
                &config.input_path,
                cs.image_streams.as_ref(),
                cs.fonts.as_ref().unwrap(),
            );
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

    // --- Phase B: Page rendering (sequential, only pages needing bitmap) ---
    let mut pages_data: Vec<PageRenderData> = Vec::new();
    for cs in needs_rendering {
        let bitmap = render_page(&config.input_path, cs.page_idx, config.dpi)?;
        pages_data.push(PageRenderData {
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
            process_page(
                pd.page_idx,
                &pd.bitmap,
                &pd.content,
                &mrc_config,
                &cache_settings,
                cache_store.as_ref(),
                &config.input_path,
                pd.image_streams.as_ref(),
            )
        })
        .collect();

    // Collect all results
    let mut successful_pages: Vec<ProcessedPage> = outlines_pages;
    for result in processed {
        successful_pages.push(result?);
    }

    // Add skip pages directly (no rendering or MRC processing needed)
    for &(page_idx, mode) in &page_modes {
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

    let pages_processed = successful_pages.len();

    // --- Phase D: PDF assembly + optimization (sequential) ---
    let mut writer = MrcPageWriter::new();
    let mut masked_page_ids: Vec<lopdf::ObjectId> = Vec::new();
    for page in &successful_pages {
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
