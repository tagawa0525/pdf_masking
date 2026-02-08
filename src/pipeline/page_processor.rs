// Phase 10: ページ単位処理: キャッシュ確認 → MRC合成 → キャッシュ保存

use image::DynamicImage;

use crate::cache::hash::{CacheSettings, compute_cache_key};
use crate::cache::store::CacheStore;
use crate::mrc::MrcLayers;
use crate::mrc::compositor::{MrcConfig, compose};

/// Single page MRC processing result.
#[allow(dead_code)]
pub struct ProcessedPage {
    pub page_index: u32,
    pub mrc_layers: MrcLayers,
    pub cache_key: String,
}

/// Process a single page: check cache -> MRC compose -> store in cache.
///
/// `content_stream` is used with settings to compute the cache key.
/// `pdf_path` and `page_index` are included in the cache key to prevent
/// collisions across different PDFs.
/// If cache hits and dimensions match the bitmap, return cached layers.
/// Otherwise run MRC compose.
#[allow(dead_code)]
pub fn process_page(
    page_index: u32,
    bitmap: &DynamicImage,
    content_stream: &[u8],
    mrc_config: &MrcConfig,
    cache_settings: &CacheSettings,
    cache_store: Option<&CacheStore>,
    pdf_path: &str,
) -> crate::error::Result<ProcessedPage> {
    let cache_key = compute_cache_key(content_stream, cache_settings, pdf_path, page_index);

    let bitmap_width = bitmap.width();
    let bitmap_height = bitmap.height();

    // Check cache first
    if let Some(store) = cache_store {
        if let Some(layers) = store.retrieve(&cache_key)? {
            // Validate cached layer dimensions against current bitmap
            if layers.width == bitmap_width && layers.height == bitmap_height {
                return Ok(ProcessedPage {
                    page_index,
                    mrc_layers: layers,
                    cache_key,
                });
            }
            // Dimension mismatch: treat as cache miss and recompose
        }
    }

    // Cache miss: run MRC composition
    let rgba_image = bitmap.to_rgba8();
    let (width, height) = (rgba_image.width(), rgba_image.height());
    let rgba_data = rgba_image.into_raw();

    let mrc_layers = compose(&rgba_data, width, height, mrc_config)?;

    // Store in cache if available
    if let Some(store) = cache_store {
        store.store(&cache_key, &mrc_layers)?;
    }

    Ok(ProcessedPage {
        page_index,
        mrc_layers,
        cache_key,
    })
}
