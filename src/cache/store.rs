// Phase 8: ファイルシステムキャッシュ: hash → MRC層バイト列
//
// Stores and retrieves PageOutput on disk, keyed by SHA-256 hash.
// MRC entries: mask.jbig2, foreground.jpg, background.jpg, metadata.json
// BW entries: mask.jbig2, metadata.json

use crate::config::job::ColorMode;
use crate::error::PdfMaskError;
use crate::mrc::{BwLayers, MrcLayers, PageOutput};
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};

/// MRC用キャッシュエントリの必須ファイル。
const MRC_CACHE_FILES: &[&str] = &[
    "mask.jbig2",
    "foreground.jpg",
    "background.jpg",
    "metadata.json",
];

/// BW用キャッシュエントリの必須ファイル。
const BW_CACHE_FILES: &[&str] = &["mask.jbig2", "metadata.json"];

/// ファイルシステムベースのキャッシュストア。
///
/// `<cache_dir>/<hex_hash>/` 以下に MRC レイヤーファイルを格納する。
pub struct CacheStore {
    cache_dir: PathBuf,
}

/// metadata.json に保存する画像のメタデータ。
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMetadata {
    cache_key: String,
    width: u32,
    height: u32,
    #[serde(default = "default_color_mode")]
    color_mode: String,
}

fn default_color_mode() -> String {
    "rgb".to_string()
}

/// キャッシュキーが有効な SHA-256 hex 文字列であることを検証する。
///
/// 有効なキーは正確に64文字の小文字16進数([0-9a-f])である必要がある。
/// パストラバーサルや不正なディレクトリアクセスを防止する。
fn validate_cache_key(key: &str) -> crate::error::Result<()> {
    if key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        Err(PdfMaskError::cache(format!(
            "invalid cache key: expected 64-character lowercase hex string, got '{}'",
            key
        )))
    }
}

fn color_mode_to_str(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Rgb => "rgb",
        ColorMode::Grayscale => "grayscale",
        ColorMode::Bw => "bw",
        ColorMode::Skip => "skip",
    }
}

impl CacheStore {
    /// 指定されたディレクトリをキャッシュルートとして新しい CacheStore を作成する。
    pub fn new(cache_dir: impl AsRef<Path>) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().to_path_buf(),
        }
    }

    /// キャッシュキーからディレクトリパスを計算する。
    fn key_dir(&self, key: &str) -> crate::error::Result<PathBuf> {
        validate_cache_key(key)?;
        Ok(self.cache_dir.join(key))
    }

    /// PageOutput をキャッシュに保存する。
    ///
    /// キャッシュディレクトリが存在しない場合は自動的に作成する。
    /// 書き込みはアトミック: 一時ディレクトリにファイルを書き込み、
    /// 最後にrenameで最終パスに移動する。
    pub fn store(&self, key: &str, output: &PageOutput) -> crate::error::Result<()> {
        // キャッシュキーの検証（Skip含め全ケースで一貫性を保つ）
        validate_cache_key(key)?;

        let (mask_jbig2, fg, bg, width, height, mode) = match output {
            PageOutput::Mrc(layers) => (
                &layers.mask_jbig2,
                Some(&layers.foreground_jpeg),
                Some(&layers.background_jpeg),
                layers.width,
                layers.height,
                layers.color_mode,
            ),
            PageOutput::BwMask(layers) => (
                &layers.mask_jbig2,
                None,
                None,
                layers.width,
                layers.height,
                ColorMode::Bw,
            ),
            PageOutput::Skip(_) => return Ok(()), // Skipはキャッシュ不要
        };

        let dir = self.key_dir(key)?;
        let tmp_dir = dir.with_extension("tmp");

        if tmp_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_dir);
        }
        fs::create_dir_all(&tmp_dir).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        fs::write(tmp_dir.join("mask.jbig2"), mask_jbig2)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        if let Some(fg_data) = fg {
            fs::write(tmp_dir.join("foreground.jpg"), fg_data)
                .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        }
        if let Some(bg_data) = bg {
            fs::write(tmp_dir.join("background.jpg"), bg_data)
                .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        }

        let metadata = CacheMetadata {
            cache_key: key.to_string(),
            width,
            height,
            color_mode: color_mode_to_str(mode).to_string(),
        };
        let metadata_json = serde_json::to_string(&metadata)?;
        fs::write(tmp_dir.join("metadata.json"), metadata_json.as_bytes())
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        if dir.exists() {
            let _ = fs::remove_dir_all(&dir);
        }

        fs::rename(&tmp_dir, &dir).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        Ok(())
    }

    /// キャッシュから PageOutput を取得する。キャッシュミスの場合は None を返す。
    pub fn retrieve(
        &self,
        key: &str,
        expected_mode: ColorMode,
    ) -> crate::error::Result<Option<PageOutput>> {
        let dir = self.key_dir(key)?;
        if !dir.exists() {
            return Ok(None);
        }

        let metadata_str = fs::read_to_string(dir.join("metadata.json"))
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        let metadata: CacheMetadata = serde_json::from_str(&metadata_str)?;

        if metadata.cache_key != key {
            return Err(PdfMaskError::cache(format!(
                "cache key mismatch: expected '{}', found '{}'",
                key, metadata.cache_key
            )));
        }

        // カラーモードが一致しない場合はキャッシュミス
        if metadata.color_mode != color_mode_to_str(expected_mode) {
            return Ok(None);
        }

        let mask_jbig2 =
            fs::read(dir.join("mask.jbig2")).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        match expected_mode {
            ColorMode::Bw => Ok(Some(PageOutput::BwMask(BwLayers {
                mask_jbig2,
                width: metadata.width,
                height: metadata.height,
            }))),
            mode => {
                let foreground_jpeg = fs::read(dir.join("foreground.jpg"))
                    .map_err(|e| PdfMaskError::cache(e.to_string()))?;
                let background_jpeg = fs::read(dir.join("background.jpg"))
                    .map_err(|e| PdfMaskError::cache(e.to_string()))?;

                Ok(Some(PageOutput::Mrc(MrcLayers {
                    mask_jbig2,
                    foreground_jpeg,
                    background_jpeg,
                    width: metadata.width,
                    height: metadata.height,
                    color_mode: mode,
                })))
            }
        }
    }

    /// キャッシュキーが存在するか確認する。
    pub fn contains(&self, key: &str) -> bool {
        let dir = match self.key_dir(key) {
            Ok(d) => d,
            Err(_) => return false,
        };

        // メタデータがあれば、そのモードに応じたファイル群をチェック
        let metadata_path = dir.join("metadata.json");
        if !metadata_path.exists() {
            return false;
        }

        let metadata_str = match fs::read_to_string(&metadata_path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let metadata: CacheMetadata = match serde_json::from_str(&metadata_str) {
            Ok(m) => m,
            Err(_) => return false,
        };

        let required_files = if metadata.color_mode == "bw" {
            BW_CACHE_FILES
        } else {
            MRC_CACHE_FILES
        };

        required_files.iter().all(|f| dir.join(f).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_cache_key_rejects_uppercase_hex() {
        let uppercase_key = "a".repeat(58) + "ABCDEF";
        assert_eq!(uppercase_key.len(), 64);
        assert!(validate_cache_key(&uppercase_key).is_err());
    }

    #[test]
    fn test_validate_cache_key_accepts_lowercase_hex() {
        let lowercase_key = "a".repeat(64);
        assert!(validate_cache_key(&lowercase_key).is_ok());
    }

    #[test]
    fn test_validate_cache_key_rejects_wrong_length() {
        let short_key = "a".repeat(63);
        assert!(validate_cache_key(&short_key).is_err());
    }
}
