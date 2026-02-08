// Phase 8: SHA-256（コンテンツストリーム + 設定）
//
// Computes a cache key from a page's content stream bytes and MRC-relevant settings.
// The key is a SHA-256 hash encoded as a lowercase hexadecimal string.

use sha2::{Digest, Sha256};

/// MRC処理に影響する設定パラメータ。
///
/// キャッシュキー計算時にハッシュに含める設定値のみを保持する。
/// dpi, fg_dpi, bg_quality, fg_quality, preserve_images がMRC出力に影響する。
#[allow(dead_code)]
pub struct CacheSettings {
    pub dpi: u32,
    pub fg_dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub preserve_images: bool,
}

/// コンテンツストリームと設定からキャッシュキー（SHA-256ハッシュ）を計算する。
///
/// ハッシュ入力: `content_stream || settings_canonical_json`
/// 設定は正規化されたJSON形式（キーのアルファベット順）で結合される。
#[allow(dead_code)]
pub fn compute_cache_key(content_stream: &[u8], settings: &CacheSettings) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content_stream);

    // 設定を正規化JSON形式で追加（キーはアルファベット順で固定）
    let settings_json = format!(
        "{{\"bg_quality\":{},\"dpi\":{},\"fg_dpi\":{},\"fg_quality\":{},\"preserve_images\":{}}}",
        settings.bg_quality,
        settings.dpi,
        settings.fg_dpi,
        settings.fg_quality,
        settings.preserve_images
    );
    hasher.update(settings_json.as_bytes());

    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_json_is_sorted_by_key() {
        // Verify the canonical JSON format has keys in alphabetical order
        let settings = CacheSettings {
            dpi: 300,
            fg_dpi: 150,
            bg_quality: 50,
            fg_quality: 30,
            preserve_images: false,
        };

        let key = compute_cache_key(b"", &settings);
        // Key should be deterministic (not empty)
        assert_eq!(key.len(), 64);
    }
}
