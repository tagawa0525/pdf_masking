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
use crate::pipeline::page_processor::{ProcessedPage, process_page};
use crate::render::pdfium::render_page;

/// Configuration for a single job.
pub struct JobConfig {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    /// 0-based page index and its color mode, sorted by page index.
    pub page_modes: Vec<(u32, ColorMode)>,
    pub dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub preserve_images: bool,
    pub cache_dir: Option<PathBuf>,
}

/// Result of processing a single job.
pub struct JobResult {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub pages_processed: usize,
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

    // Validate all page indices are within range
    for &(page_idx, _) in &config.page_modes {
        let page_num = page_idx + 1;
        if page_num > page_count {
            return Err(PdfMaskError::pdf_read(format!(
                "page index {} out of range (document has {} pages)",
                page_idx, page_count
            )));
        }
    }

    // --- Phase A: Content stream analysis (sequential) ---
    // Skip pages don't need content streams or rendering.
    let non_skip: Vec<(u32, ColorMode)> = config
        .page_modes
        .iter()
        .filter(|(_, mode)| *mode != ColorMode::Skip)
        .copied()
        .collect();

    let mut content_streams: Vec<(u32, ColorMode, Vec<u8>)> = Vec::new();
    for &(page_idx, mode) in &non_skip {
        let page_num = page_idx + 1;
        let content = reader.page_content_stream(page_num)?;
        content_streams.push((page_idx, mode, content));
    }

    // --- Phase B: Page rendering (sequential) ---
    let mut pages_data: Vec<(u32, ColorMode, image::DynamicImage, Vec<u8>)> = Vec::new();
    for (page_idx, mode, content) in content_streams {
        let bitmap = render_page(&config.input_path, page_idx, config.dpi)?;
        pages_data.push((page_idx, mode, bitmap, content));
    }

    // --- Phase C: MRC processing (rayon parallel) ---
    let mrc_config = MrcConfig {
        bg_quality: config.bg_quality,
        fg_quality: config.fg_quality,
    };
    let cache_store = config.cache_dir.as_ref().map(CacheStore::new);

    let processed: Vec<crate::error::Result<ProcessedPage>> = pages_data
        .par_iter()
        .map(|(page_idx, mode, bitmap, content_stream)| {
            let cache_settings = CacheSettings {
                dpi: config.dpi,
                fg_dpi: config.dpi,
                bg_quality: config.bg_quality,
                fg_quality: config.fg_quality,
                preserve_images: config.preserve_images,
                color_mode: *mode,
            };
            process_page(
                *page_idx,
                bitmap,
                content_stream,
                &mrc_config,
                &cache_settings,
                cache_store.as_ref(),
                &config.input_path,
                *mode,
            )
        })
        .collect();

    // Collect MRC/BW results
    let mut successful_pages: Vec<ProcessedPage> = Vec::new();
    for result in processed {
        successful_pages.push(result?);
    }

    // Add skip pages directly (no rendering or MRC processing needed)
    for &(page_idx, mode) in &config.page_modes {
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
