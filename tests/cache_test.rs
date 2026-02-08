// Phase 8: Cache integration tests
//
// Tests for cache key computation (hash.rs) and file-system cache store (store.rs).
// Each test verifies a specific aspect of the caching layer.

use std::collections::HashMap;
use std::path::Path;

use pdf_masking::cache::hash::{CacheSettings, compute_cache_key};
use pdf_masking::cache::store::CacheStore;
use pdf_masking::config::job::ColorMode;
use pdf_masking::mrc::{ImageModification, MrcLayers, PageOutput, TextMaskedData, TextRegionCrop};
use pdf_masking::pdf::content_stream::BBox;
use tempfile::tempdir;

// ---- hash.rs tests ----

/// Test that compute_cache_key produces a valid SHA-256 hex string.
#[test]
fn test_compute_cache_key() {
    let content = b"page content stream bytes";
    let settings = CacheSettings {
        dpi: 300,
        fg_dpi: 150,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: false,
        color_mode: ColorMode::Rgb,
    };

    let key = compute_cache_key(content, &settings, Path::new("test.pdf"), 0);

    // SHA-256 produces a 64-character hex string
    assert_eq!(key.len(), 64, "Cache key should be 64 hex characters");
    assert!(
        key.chars().all(|c: char| c.is_ascii_hexdigit()),
        "Cache key should contain only hex characters"
    );
}

/// Test that the same inputs always produce the same cache key.
#[test]
fn test_cache_key_deterministic() {
    let content = b"deterministic test content";
    let settings = CacheSettings {
        dpi: 300,
        fg_dpi: 150,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: false,
        color_mode: ColorMode::Rgb,
    };

    let key1 = compute_cache_key(content, &settings, Path::new("test.pdf"), 0);
    let key2 = compute_cache_key(content, &settings, Path::new("test.pdf"), 0);

    assert_eq!(key1, key2, "Same inputs should produce the same cache key");
}

/// Test that different content produces a different cache key.
#[test]
fn test_cache_key_differs_with_different_content() {
    let settings = CacheSettings {
        dpi: 300,
        fg_dpi: 150,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: false,
        color_mode: ColorMode::Rgb,
    };

    let key_a = compute_cache_key(b"content A", &settings, Path::new("test.pdf"), 0);
    let key_b = compute_cache_key(b"content B", &settings, Path::new("test.pdf"), 0);

    assert_ne!(
        key_a, key_b,
        "Different content should produce different cache keys"
    );
}

/// Test that different settings produce a different cache key.
#[test]
fn test_cache_key_differs_with_different_settings() {
    let content = b"same content";

    let settings_a = CacheSettings {
        dpi: 300,
        fg_dpi: 150,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: false,
        color_mode: ColorMode::Rgb,
    };
    let settings_b = CacheSettings {
        dpi: 600,
        fg_dpi: 300,
        bg_quality: 80,
        fg_quality: 60,
        preserve_images: true,
        color_mode: ColorMode::Rgb,
    };

    let key_a = compute_cache_key(content, &settings_a, Path::new("test.pdf"), 0);
    let key_b = compute_cache_key(content, &settings_b, Path::new("test.pdf"), 0);

    assert_ne!(
        key_a, key_b,
        "Different settings should produce different cache keys"
    );
}

/// Test that different pdf_path produces a different cache key.
#[test]
fn test_cache_key_differs_with_different_pdf_path() {
    let content = b"same content";
    let settings = CacheSettings {
        dpi: 300,
        fg_dpi: 150,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: false,
        color_mode: ColorMode::Rgb,
    };

    let key_a = compute_cache_key(content, &settings, Path::new("file_a.pdf"), 0);
    let key_b = compute_cache_key(content, &settings, Path::new("file_b.pdf"), 0);

    assert_ne!(
        key_a, key_b,
        "Different pdf_path should produce different cache keys"
    );
}

/// Test that different page_index produces a different cache key.
#[test]
fn test_cache_key_differs_with_different_page_index() {
    let content = b"same content";
    let settings = CacheSettings {
        dpi: 300,
        fg_dpi: 150,
        bg_quality: 50,
        fg_quality: 30,
        preserve_images: false,
        color_mode: ColorMode::Rgb,
    };

    let key_a = compute_cache_key(content, &settings, Path::new("test.pdf"), 0);
    let key_b = compute_cache_key(content, &settings, Path::new("test.pdf"), 1);

    assert_ne!(
        key_a, key_b,
        "Different page_index should produce different cache keys"
    );
}

// ---- store.rs tests ----

/// A valid 64-character hex string for use as a cache key in tests.
const TEST_KEY: &str = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";

/// Helper to create sample MrcLayers for testing.
fn sample_layers() -> MrcLayers {
    MrcLayers {
        mask_jbig2: vec![0x00, 0x01, 0x02, 0x03],
        foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
        background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
        width: 200,
        height: 300,
        color_mode: ColorMode::Rgb,
    }
}

/// Test storing and retrieving MrcLayers.
#[test]
fn test_store_and_retrieve() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());
    let layers = sample_layers();

    let expected_mask = layers.mask_jbig2.clone();
    let expected_fg = layers.foreground_jpeg.clone();
    let expected_bg = layers.background_jpeg.clone();
    let expected_width = layers.width;
    let expected_height = layers.height;

    store
        .store(TEST_KEY, &PageOutput::Mrc(layers), None)
        .expect("store should succeed");
    let retrieved = store
        .retrieve(TEST_KEY, ColorMode::Rgb, None)
        .expect("retrieve should succeed")
        .expect("should return Some for stored key");

    match retrieved {
        PageOutput::Mrc(ref mrc) => {
            assert_eq!(mrc.mask_jbig2, expected_mask);
            assert_eq!(mrc.foreground_jpeg, expected_fg);
            assert_eq!(mrc.background_jpeg, expected_bg);
            assert_eq!(mrc.width, expected_width);
            assert_eq!(mrc.height, expected_height);
        }
        _ => panic!("expected PageOutput::Mrc"),
    }
}

/// Test that retrieving a non-existent key returns None.
#[test]
fn test_cache_miss() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());
    let nonexistent_key = "0000000000000000000000000000000000000000000000000000000000000000";

    let result = store
        .retrieve(nonexistent_key, ColorMode::Rgb, None)
        .expect("retrieve should succeed even on miss");

    assert!(
        result.is_none(),
        "Retrieving a non-existent key should return None"
    );
}

/// Test that contains returns true after storing.
#[test]
fn test_cache_hit_after_store() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());
    let layers = sample_layers();

    assert!(
        !store.contains(TEST_KEY),
        "Key should not exist before storing"
    );

    store
        .store(TEST_KEY, &PageOutput::Mrc(layers), None)
        .expect("store should succeed");

    assert!(store.contains(TEST_KEY), "Key should exist after storing");
}

/// Test that the cache directory is created automatically.
#[test]
fn test_cache_dir_creation() {
    let dir = tempdir().expect("failed to create temp dir");
    let cache_path = dir.path().join("nested").join("cache").join("dir");
    let store = CacheStore::new(&cache_path);
    let layers = sample_layers();

    // The nested directory doesn't exist yet
    assert!(!cache_path.exists());

    store
        .store(TEST_KEY, &PageOutput::Mrc(layers), None)
        .expect("store should create directories and succeed");

    // After storing, the cache directory and key subdirectory should exist
    assert!(cache_path.join(TEST_KEY).exists());
}

/// Test that invalid cache keys are rejected by store.
#[test]
fn test_store_rejects_invalid_key() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());
    let output = PageOutput::Mrc(sample_layers());

    // Too short
    assert!(store.store("abc123", &output, None).is_err());
    // Path traversal attempt
    assert!(store.store("../etc/passwd", &output, None).is_err());
    // Contains non-hex characters
    assert!(
        store
            .store(
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
                &output,
                None
            )
            .is_err()
    );
}

/// Test that invalid cache keys are rejected by retrieve.
#[test]
fn test_retrieve_rejects_invalid_key() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());

    assert!(
        store
            .retrieve("../etc/passwd", ColorMode::Rgb, None)
            .is_err()
    );
    assert!(store.retrieve("short", ColorMode::Rgb, None).is_err());
}

/// Test that contains returns false for invalid keys.
#[test]
fn test_contains_returns_false_for_invalid_key() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());

    assert!(!store.contains("../etc/passwd"));
    assert!(!store.contains("short_key"));
}

/// Test that contains returns false when cache entry is incomplete (missing files).
#[test]
fn test_contains_returns_false_for_incomplete_entry() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path());

    // Manually create a directory with only some files (simulating interrupted store)
    let key_dir = dir.path().join(TEST_KEY);
    std::fs::create_dir_all(&key_dir).unwrap();
    std::fs::write(key_dir.join("mask.jbig2"), b"data").unwrap();
    // Missing foreground.jpg, background.jpg, metadata.json

    assert!(
        !store.contains(TEST_KEY),
        "contains should return false when cache entry is incomplete"
    );
}

/// Test that CacheStore::new accepts Path directly (not just &str).
#[test]
fn test_new_accepts_path() {
    let dir = tempdir().expect("failed to create temp dir");
    // Pass Path directly - this would not compile with the old &str signature
    let store = CacheStore::new(dir.path());
    let layers = sample_layers();

    store
        .store(TEST_KEY, &PageOutput::Mrc(layers), None)
        .expect("store should succeed");
    assert!(store.contains(TEST_KEY));
}

// ---- TextMasked cache tests ----

const TEST_KEY_TM: &str = "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3";

fn sample_text_masked_data() -> TextMaskedData {
    TextMaskedData {
        stripped_content_stream: b"q 100 0 0 100 0 0 cm /Im1 Do Q".to_vec(),
        text_regions: vec![TextRegionCrop {
            data: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10],
            filter: "DCTDecode".to_string(),
            color_space: "DeviceRGB".to_string(),
            bits_per_component: 8,
            bbox_points: BBox {
                x_min: 72.0,
                y_min: 600.0,
                x_max: 200.0,
                y_max: 700.0,
            },
            pixel_width: 128,
            pixel_height: 100,
        }],
        modified_images: HashMap::new(),
        page_index: 0,
        color_mode: ColorMode::Rgb,
    }
}

/// TextMaskedDataをstore→retrieveし、全フィールドが一致することを検証。
#[test]
fn test_store_and_retrieve_text_masked() {
    let dir = tempdir().expect("create temp dir");
    let store = CacheStore::new(dir.path());
    let data = sample_text_masked_data();

    store
        .store(TEST_KEY_TM, &PageOutput::TextMasked(data), Some((100, 100)))
        .expect("store TextMasked should succeed");

    let retrieved = store
        .retrieve(TEST_KEY_TM, ColorMode::Rgb, None)
        .expect("retrieve should succeed")
        .expect("should return Some for stored TextMasked key");

    match retrieved {
        PageOutput::TextMasked(tm) => {
            assert_eq!(
                tm.stripped_content_stream,
                b"q 100 0 0 100 0 0 cm /Im1 Do Q"
            );
            assert_eq!(tm.page_index, 0);
            assert_eq!(tm.color_mode, ColorMode::Rgb);
            assert_eq!(tm.text_regions.len(), 1);
            let region = &tm.text_regions[0];
            assert_eq!(region.data, vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]);
            assert_eq!(region.pixel_width, 128);
            assert_eq!(region.pixel_height, 100);
            assert!((region.bbox_points.x_min - 72.0).abs() < f64::EPSILON);
            assert!((region.bbox_points.y_max - 700.0).abs() < f64::EPSILON);
            assert!(tm.modified_images.is_empty());
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// modified_imagesを含むTextMaskedDataをstore→retrieveし、画像データが復元されることを検証。
#[test]
fn test_store_and_retrieve_text_masked_with_modified_images() {
    let dir = tempdir().expect("create temp dir");
    let store = CacheStore::new(dir.path());

    let mut modified_images = HashMap::new();
    modified_images.insert(
        "Im1".to_string(),
        ImageModification {
            data: vec![0xBB; 200],
            filter: "DCTDecode".to_string(),
            color_space: "DeviceGray".to_string(),
            bits_per_component: 8,
        },
    );

    let data = TextMaskedData {
        stripped_content_stream: b"q Q".to_vec(),
        text_regions: vec![],
        modified_images,
        page_index: 2,
        color_mode: ColorMode::Grayscale,
    };

    store
        .store(TEST_KEY_TM, &PageOutput::TextMasked(data), Some((100, 100)))
        .expect("store should succeed");

    let retrieved = store
        .retrieve(TEST_KEY_TM, ColorMode::Grayscale, None)
        .expect("retrieve should succeed")
        .expect("should return Some");

    match retrieved {
        PageOutput::TextMasked(tm) => {
            assert_eq!(tm.page_index, 2);
            assert_eq!(tm.color_mode, ColorMode::Grayscale);
            assert_eq!(tm.stripped_content_stream, b"q Q");
            assert!(tm.text_regions.is_empty());
            assert_eq!(tm.modified_images.len(), 1);
            let img = tm.modified_images.get("Im1").expect("Im1 should exist");
            assert_eq!(img.data, vec![0xBB; 200]);
            assert_eq!(img.filter, "DCTDecode");
            assert_eq!(img.color_space, "DeviceGray");
            assert_eq!(img.bits_per_component, 8);
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// TextMaskedDataをstore後にcontainsがtrueを返すことを検証。
#[test]
fn test_contains_text_masked() {
    let dir = tempdir().expect("create temp dir");
    let store = CacheStore::new(dir.path());

    assert!(!store.contains(TEST_KEY_TM));

    let data = sample_text_masked_data();
    store
        .store(TEST_KEY_TM, &PageOutput::TextMasked(data), Some((100, 100)))
        .expect("store should succeed");

    assert!(
        store.contains(TEST_KEY_TM),
        "contains should return true after storing TextMasked"
    );
}

/// TextMaskedキャッシュのディスク上のファイル構造を検証。
#[test]
fn test_text_masked_cache_files_on_disk() {
    let dir = tempdir().expect("create temp dir");
    let store = CacheStore::new(dir.path());

    let mut modified_images = HashMap::new();
    modified_images.insert(
        "Im1".to_string(),
        ImageModification {
            data: vec![0xCC; 50],
            filter: "FlateDecode".to_string(),
            color_space: "DeviceRGB".to_string(),
            bits_per_component: 8,
        },
    );

    let data = TextMaskedData {
        stripped_content_stream: b"q Q".to_vec(),
        text_regions: vec![TextRegionCrop {
            data: vec![0xFF, 0xD8],
            bbox_points: BBox {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 100.0,
                y_max: 100.0,
            },
            pixel_width: 50,
            pixel_height: 50,
            filter: "DCTDecode".to_string(),
            color_space: "DeviceRGB".to_string(),
            bits_per_component: 8,
        }],
        modified_images,
        page_index: 0,
        color_mode: ColorMode::Rgb,
    };

    store
        .store(TEST_KEY_TM, &PageOutput::TextMasked(data), Some((100, 100)))
        .expect("store should succeed");

    let key_dir = dir.path().join(TEST_KEY_TM);
    assert!(key_dir.join("metadata.json").exists());
    assert!(key_dir.join("stripped_content.bin").exists());
    assert!(key_dir.join("region_0.jpg").exists());
    assert!(key_dir.join("modified_Im1.bin").exists());
}
