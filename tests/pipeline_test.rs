use image::{DynamicImage, RgbaImage};
use pdf_masking::cache::hash::CacheSettings;
use pdf_masking::cache::store::CacheStore;
use pdf_masking::mrc::compositor::MrcConfig;
use pdf_masking::pipeline::job_runner::JobConfig;
use pdf_masking::pipeline::orchestrator::run_all_jobs;
use pdf_masking::pipeline::page_processor::process_page;

#[test]
fn test_process_page_cache_miss() {
    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"q 100 0 0 100 0 0 cm /Im1 Do Q";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: true,
    };

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        "test.pdf",
    );
    assert!(
        result.is_ok(),
        "process_page should succeed: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    assert_eq!(processed.page_index, 0);
    assert!(!processed.cache_key.is_empty());
    assert_eq!(processed.cache_key.len(), 64); // SHA-256 hex
    assert!(!processed.mrc_layers.mask_jbig2.is_empty());
    assert!(!processed.mrc_layers.background_jpeg.is_empty());
    assert!(!processed.mrc_layers.foreground_jpeg.is_empty());
    assert_eq!(processed.mrc_layers.width, 100);
    assert_eq!(processed.mrc_layers.height, 100);
}

#[test]
fn test_process_page_cache_hit() {
    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let cache_store = CacheStore::new(tmp_dir.path());

    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"q 100 0 0 100 0 0 cm /Im1 Do Q";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: true,
    };

    // First call: cache miss, should compose and store
    let result1 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        "test.pdf",
    );
    assert!(result1.is_ok());
    let processed1 = result1.unwrap();

    // Verify cache now contains the entry
    assert!(cache_store.contains(&processed1.cache_key));

    // Second call: cache hit, should return cached layers
    let result2 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        "test.pdf",
    );
    assert!(result2.is_ok());
    let processed2 = result2.unwrap();

    // Same cache key
    assert_eq!(processed1.cache_key, processed2.cache_key);
    // Same layer dimensions
    assert_eq!(processed1.mrc_layers.width, processed2.mrc_layers.width);
    assert_eq!(processed1.mrc_layers.height, processed2.mrc_layers.height);
}

#[test]
fn test_job_config_creation() {
    let config = JobConfig {
        input_path: "input.pdf".to_string(),
        output_path: "output.pdf".to_string(),
        pages: vec![0, 1, 2],
        dpi: 300,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: true,
        cache_dir: Some(".cache".to_string()),
    };

    assert_eq!(config.input_path, "input.pdf");
    assert_eq!(config.output_path, "output.pdf");
    assert_eq!(config.pages, vec![0, 1, 2]);
    assert_eq!(config.dpi, 300);
    assert_eq!(config.bg_quality, 50);
    assert_eq!(config.fg_quality, 30);
    assert!(config.preserve_images);
    assert_eq!(config.cache_dir, Some(".cache".to_string()));
}

#[test]
fn test_run_all_jobs_empty() {
    let jobs: Vec<JobConfig> = vec![];
    let results = run_all_jobs(&jobs);
    assert!(results.is_empty());
}
