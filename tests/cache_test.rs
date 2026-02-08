// Phase 8: Cache integration tests
//
// Tests for cache key computation (hash.rs) and file-system cache store (store.rs).
// Each test verifies a specific aspect of the caching layer.

use pdf_masking::cache::hash::{CacheSettings, compute_cache_key};
use pdf_masking::cache::store::CacheStore;
use pdf_masking::mrc::MrcLayers;
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
    };

    let key = compute_cache_key(content, &settings);

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
    };

    let key1 = compute_cache_key(content, &settings);
    let key2 = compute_cache_key(content, &settings);

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
    };

    let key_a = compute_cache_key(b"content A", &settings);
    let key_b = compute_cache_key(b"content B", &settings);

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
    };
    let settings_b = CacheSettings {
        dpi: 600,
        fg_dpi: 300,
        bg_quality: 80,
        fg_quality: 60,
        preserve_images: true,
    };

    let key_a = compute_cache_key(content, &settings_a);
    let key_b = compute_cache_key(content, &settings_b);

    assert_ne!(
        key_a, key_b,
        "Different settings should produce different cache keys"
    );
}

// ---- store.rs tests ----

/// Helper to create sample MrcLayers for testing.
fn sample_layers() -> MrcLayers {
    MrcLayers {
        mask_jbig2: vec![0x00, 0x01, 0x02, 0x03],
        foreground_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
        background_jpeg: vec![0xFF, 0xD8, 0xFF, 0xE1],
        width: 200,
        height: 300,
    }
}

/// Test storing and retrieving MrcLayers.
#[test]
fn test_store_and_retrieve() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path().to_str().unwrap());
    let key = "abc123def456";
    let layers = sample_layers();

    store.store(key, &layers).expect("store should succeed");
    let retrieved = store
        .retrieve(key)
        .expect("retrieve should succeed")
        .expect("should return Some for stored key");

    assert_eq!(retrieved.mask_jbig2, layers.mask_jbig2);
    assert_eq!(retrieved.foreground_jpeg, layers.foreground_jpeg);
    assert_eq!(retrieved.background_jpeg, layers.background_jpeg);
    assert_eq!(retrieved.width, layers.width);
    assert_eq!(retrieved.height, layers.height);
}

/// Test that retrieving a non-existent key returns None.
#[test]
fn test_cache_miss() {
    let dir = tempdir().expect("failed to create temp dir");
    let store = CacheStore::new(dir.path().to_str().unwrap());

    let result = store
        .retrieve("nonexistent_key")
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
    let store = CacheStore::new(dir.path().to_str().unwrap());
    let key = "hit_test_key";
    let layers = sample_layers();

    assert!(!store.contains(key), "Key should not exist before storing");

    store.store(key, &layers).expect("store should succeed");

    assert!(store.contains(key), "Key should exist after storing");
}

/// Test that the cache directory is created automatically.
#[test]
fn test_cache_dir_creation() {
    let dir = tempdir().expect("failed to create temp dir");
    let cache_path = dir.path().join("nested").join("cache").join("dir");
    let store = CacheStore::new(cache_path.to_str().unwrap());
    let key = "dir_creation_test";
    let layers = sample_layers();

    // The nested directory doesn't exist yet
    assert!(!cache_path.exists());

    store
        .store(key, &layers)
        .expect("store should create directories and succeed");

    // After storing, the cache directory and key subdirectory should exist
    assert!(cache_path.join(key).exists());
}
