use std::path::{Path, PathBuf};

use image::{DynamicImage, RgbaImage};
use lopdf::dictionary;
use pdf_masking::cache::hash::CacheSettings;
use pdf_masking::cache::store::CacheStore;
use pdf_masking::config::job::ColorMode;
use pdf_masking::mrc::PageOutput;
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
        color_mode: ColorMode::Rgb,
    };

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        None,
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
    match &processed.output {
        PageOutput::Mrc(layers) => {
            assert!(!layers.mask_jbig2.is_empty());
            assert!(!layers.background_jpeg.is_empty());
            assert!(!layers.foreground_jpeg.is_empty());
            assert_eq!(layers.width, 100);
            assert_eq!(layers.height, 100);
        }
        _ => panic!("expected PageOutput::Mrc"),
    }
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
        color_mode: ColorMode::Rgb,
    };

    // First call: cache miss, should compose and store
    let result1 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        None,
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
        Path::new("test.pdf"),
        None,
    );
    assert!(result2.is_ok());
    let processed2 = result2.unwrap();

    // Same cache key
    assert_eq!(processed1.cache_key, processed2.cache_key);
    // Same layer dimensions
    match (&processed1.output, &processed2.output) {
        (PageOutput::Mrc(layers1), PageOutput::Mrc(layers2)) => {
            assert_eq!(layers1.width, layers2.width);
            assert_eq!(layers1.height, layers2.height);
        }
        _ => panic!("expected PageOutput::Mrc for both results"),
    }
}

/// image_streams=Some (非空) の場合、TextMaskedモードに分岐することを検証
#[test]
fn test_process_page_text_masked_with_image_streams() {
    use std::collections::HashMap;

    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"BT /F1 12 Tf (Hello) Tj ET";
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
        color_mode: ColorMode::Rgb,
    };

    // 画像XObjectを持つストリームマップ
    let mut image_streams = HashMap::new();
    let img_stream = lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 50,
            "Height" => 50,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        vec![0u8; 4],
    );
    image_streams.insert("Im1".to_string(), img_stream);

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        Some(&image_streams),
    );
    assert!(
        result.is_ok(),
        "process_page with image_streams should succeed: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    match &processed.output {
        PageOutput::TextMasked(data) => {
            // BT...ETがすべて除去されたストリーム（元がBT...ETのみなので空になる）
            assert_eq!(data.page_index, 0);
            assert_eq!(data.color_mode, ColorMode::Rgb);
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// image_streams=None の場合、preserve_images=true でもMRCフォールバックすることを検証
#[test]
fn test_process_page_mrc_fallback_without_image_streams() {
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
        color_mode: ColorMode::Rgb,
    };

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        None, // image_streams=None → MRCフォールバック
    );
    assert!(result.is_ok());

    let processed = result.unwrap();
    match &processed.output {
        PageOutput::Mrc(layers) => {
            assert_eq!(layers.width, 100);
            assert_eq!(layers.height, 100);
        }
        other => panic!(
            "expected PageOutput::Mrc, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

#[test]
fn test_job_config_creation() {
    use std::collections::HashMap;

    let mut overrides = HashMap::new();
    overrides.insert(2, ColorMode::Bw);
    overrides.insert(3, ColorMode::Skip);

    let config = JobConfig {
        input_path: PathBuf::from("input.pdf"),
        output_path: PathBuf::from("output.pdf"),
        default_color_mode: ColorMode::Rgb,
        color_mode_overrides: overrides.clone(),
        dpi: 300,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: true,
        cache_dir: Some(PathBuf::from(".cache")),
    };

    assert_eq!(config.input_path, Path::new("input.pdf"));
    assert_eq!(config.output_path, Path::new("output.pdf"));
    assert_eq!(config.default_color_mode, ColorMode::Rgb);
    assert_eq!(config.color_mode_overrides.len(), 2);
    assert_eq!(config.color_mode_overrides.get(&2), Some(&ColorMode::Bw));
    assert_eq!(config.color_mode_overrides.get(&3), Some(&ColorMode::Skip));
    assert_eq!(config.dpi, 300);
    assert_eq!(config.bg_quality, 50);
    assert_eq!(config.fg_quality, 30);
    assert!(config.preserve_images);
    assert_eq!(config.cache_dir, Some(PathBuf::from(".cache")));
}

/// TextMaskedモードでキャッシュが効くことを検証（store → 2回目でcache hit）
#[test]
fn test_process_page_text_masked_cache_roundtrip() {
    use std::collections::HashMap;

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let cache_store = CacheStore::new(tmp_dir.path());

    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"BT /F1 12 Tf (Hello) Tj ET";
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
        color_mode: ColorMode::Rgb,
    };

    let mut image_streams = HashMap::new();
    let img_stream = lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 50,
            "Height" => 50,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        vec![0u8; 4],
    );
    image_streams.insert("Im1".to_string(), img_stream);

    // 1回目: cache miss → TextMasked生成 + キャッシュ保存
    let result1 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        Some(&image_streams),
    );
    assert!(
        result1.is_ok(),
        "first call should succeed: {:?}",
        result1.err()
    );
    let processed1 = result1.unwrap();
    assert!(matches!(&processed1.output, PageOutput::TextMasked(_)));
    assert!(cache_store.contains(&processed1.cache_key));

    // 2回目: cache hit → キャッシュから復元
    let result2 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        Some(&image_streams),
    );
    assert!(
        result2.is_ok(),
        "second call should succeed: {:?}",
        result2.err()
    );
    let processed2 = result2.unwrap();

    // 同じcache keyであること
    assert_eq!(processed1.cache_key, processed2.cache_key);
    // TextMaskedとして復元されること
    match (&processed1.output, &processed2.output) {
        (PageOutput::TextMasked(d1), PageOutput::TextMasked(d2)) => {
            assert_eq!(d1.page_index, d2.page_index);
            assert_eq!(d1.color_mode, d2.color_mode);
            assert_eq!(d1.text_regions.len(), d2.text_regions.len());
            assert_eq!(d1.modified_images.len(), d2.modified_images.len());
        }
        _ => panic!("expected TextMasked for both calls"),
    }
}

#[test]
fn test_run_all_jobs_empty() {
    let jobs: Vec<JobConfig> = vec![];
    let results = run_all_jobs(&jobs);
    assert!(results.is_empty());
}
