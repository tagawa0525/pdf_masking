// Phase 8: SHA-256（コンテンツストリーム + 設定）
//
// Computes a cache key from a page's content stream bytes and MRC-relevant settings.
// The key is a SHA-256 hash encoded as a lowercase hexadecimal string.

use sha2::{Digest, Sha256};

/// MRC処理に影響する設定パラメータ。
///
/// キャッシュキー計算時にハッシュに含める設定値のみを保持する。
/// dpi, fg_dpi, bg_quality, fg_quality, preserve_images がMRC出力に影響する。
pub struct CacheSettings {
    pub dpi: u32,
    pub fg_dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub preserve_images: bool,
}

/// 設定を正規化JSON形式に変換する（キーはアルファベット順で固定）。
fn settings_to_canonical_json(settings: &CacheSettings) -> String {
    format!(
        "{{\"bg_quality\":{},\"dpi\":{},\"fg_dpi\":{},\"fg_quality\":{},\"preserve_images\":{}}}",
        settings.bg_quality,
        settings.dpi,
        settings.fg_dpi,
        settings.fg_quality,
        settings.preserve_images
    )
}

/// コンテンツストリームと設定からキャッシュキー（SHA-256ハッシュ）を計算する。
///
/// ハッシュ入力: `pdf_path || page_index || content_stream || settings_canonical_json`
/// PDFパスとページインデックスを含めることで、異なるPDF間のキー衝突を防止する。
/// 設定は正規化されたJSON形式（キーのアルファベット順）で結合される。
pub fn compute_cache_key(
    content_stream: &[u8],
    settings: &CacheSettings,
    pdf_path: &str,
    page_index: u32,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pdf_path.as_bytes());
    hasher.update(page_index.to_le_bytes());
    hasher.update(content_stream);

    // 設定を正規化JSON形式で追加（キーはアルファベット順で固定）
    let settings_json = settings_to_canonical_json(settings);
    hasher.update(settings_json.as_bytes());

    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_json_is_sorted_by_key() {
        let settings = CacheSettings {
            dpi: 300,
            fg_dpi: 150,
            bg_quality: 50,
            fg_quality: 30,
            preserve_images: false,
        };

        let json = settings_to_canonical_json(&settings);

        // Verify the exact JSON output
        assert_eq!(
            json,
            "{\"bg_quality\":50,\"dpi\":300,\"fg_dpi\":150,\"fg_quality\":30,\"preserve_images\":false}"
        );

        // Verify keys are in alphabetical order by extracting them
        let keys: Vec<&str> = json
            .trim_matches(|c| c == '{' || c == '}')
            .split(',')
            .map(|pair| {
                let key = pair.split(':').next().unwrap();
                key.trim_matches('"')
            })
            .collect();

        let mut sorted_keys = keys.clone();
        sorted_keys.sort();
        assert_eq!(keys, sorted_keys, "JSON keys must be in alphabetical order");
    }

    #[test]
    fn test_settings_json_with_different_values() {
        let settings = CacheSettings {
            dpi: 600,
            fg_dpi: 300,
            bg_quality: 80,
            fg_quality: 60,
            preserve_images: true,
        };

        let json = settings_to_canonical_json(&settings);

        assert_eq!(
            json,
            "{\"bg_quality\":80,\"dpi\":600,\"fg_dpi\":300,\"fg_quality\":60,\"preserve_images\":true}"
        );
    }
}
