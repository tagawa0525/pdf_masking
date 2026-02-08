// Phase 10: ジョブ単位: PDF読込 -> 並列ページ処理 -> 出力PDF組立

use rayon::prelude::*;

use crate::cache::hash::CacheSettings;
use crate::cache::store::CacheStore;
use crate::error::PdfMaskError;
use crate::mrc::compositor::MrcConfig;
use crate::pdf::reader::PdfReader;
use crate::pdf::writer::MrcPageWriter;
use crate::pipeline::page_processor::{ProcessedPage, process_page};
use crate::render::pdfium::render_page;

/// Configuration for a single job.
#[allow(dead_code)]
pub struct JobConfig {
    pub input_path: String,
    pub output_path: String,
    pub pages: Vec<u32>,
    pub dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub preserve_images: bool,
    pub cache_dir: Option<String>,
}

/// Result of processing a single job.
#[allow(dead_code)]
pub struct JobResult {
    pub input_path: String,
    pub output_path: String,
    pub pages_processed: usize,
}

/// Run a single PDF masking job through the 4-phase pipeline.
///
/// Phase A: Content stream analysis (sequential)
/// Phase B: Page rendering (sequential)
/// Phase C: MRC processing (rayon parallel)
/// Phase D: PDF assembly + optimization (sequential)
#[allow(dead_code)]
pub fn run_job(config: &JobConfig) -> crate::error::Result<JobResult> {
    // --- Phase A: Content stream analysis (sequential) ---
    // PdfReader uses 1-indexed pages, but config.pages are 0-indexed.
    let reader = PdfReader::open(&config.input_path)?;
    let page_count = reader.page_count();

    let mut content_streams: Vec<(u32, Vec<u8>)> = Vec::new();
    for &page_idx in &config.pages {
        let page_num = page_idx + 1; // Convert 0-indexed to 1-indexed
        if page_num > page_count {
            return Err(PdfMaskError::pdf_read(format!(
                "page index {} out of range (document has {} pages)",
                page_idx, page_count
            )));
        }
        let content = reader.page_content_stream(page_num)?;
        content_streams.push((page_idx, content));
    }

    // --- Phase B: Page rendering (sequential) ---
    // render_page loads the PDF independently, so we pass the path and page index.
    let mut pages_data: Vec<(u32, image::DynamicImage, Vec<u8>)> = Vec::new();
    for (page_idx, content) in content_streams {
        let bitmap = render_page(&config.input_path, page_idx, config.dpi)?;
        pages_data.push((page_idx, bitmap, content));
    }

    // --- Phase C: MRC processing (rayon parallel) ---
    let mrc_config = MrcConfig {
        bg_quality: config.bg_quality,
        fg_quality: config.fg_quality,
    };
    let cache_settings = CacheSettings {
        dpi: config.dpi,
        fg_dpi: config.dpi, // Use same DPI for fg in v1
        bg_quality: config.bg_quality,
        fg_quality: config.fg_quality,
        preserve_images: config.preserve_images,
    };
    let cache_store = config
        .cache_dir
        .as_ref()
        .map(|dir| CacheStore::new(dir.as_str()));

    let processed: Vec<crate::error::Result<ProcessedPage>> = pages_data
        .par_iter()
        .map(|(page_idx, bitmap, content_stream)| {
            process_page(
                *page_idx,
                bitmap,
                content_stream,
                &mrc_config,
                &cache_settings,
                cache_store.as_ref(),
            )
        })
        .collect();

    // Collect results, failing on first error
    let mut successful_pages: Vec<ProcessedPage> = Vec::new();
    for result in processed {
        successful_pages.push(result?);
    }

    // Sort by page index for deterministic output
    successful_pages.sort_by_key(|p| p.page_index);

    let pages_processed = successful_pages.len();

    // --- Phase D: PDF assembly + optimization (sequential) ---
    let mut writer = MrcPageWriter::new();
    for page in &successful_pages {
        writer.write_mrc_page(&page.mrc_layers)?;
    }

    let pdf_bytes = writer.save_to_bytes()?;
    std::fs::write(&config.output_path, pdf_bytes)?;

    Ok(JobResult {
        input_path: config.input_path.clone(),
        output_path: config.output_path.clone(),
        pages_processed,
    })
}
