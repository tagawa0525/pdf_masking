// Phase 8: ファイルシステムキャッシュ: hash → MRC層バイト列
//
// Stores and retrieves MrcLayers on disk, keyed by SHA-256 hash.
// Each cache entry is a directory containing mask.jbig2, foreground.jpg,
// background.jpg, and metadata.json.

use crate::error::PdfMaskError;
use crate::mrc::MrcLayers;
use serde_json;
use std::fs;
use std::path::PathBuf;

/// ファイルシステムベースのキャッシュストア。
///
/// `<cache_dir>/<hex_hash>/` 以下に MRC レイヤーファイルを格納する。
#[allow(dead_code)]
pub struct CacheStore {
    cache_dir: PathBuf,
}

/// metadata.json に保存する画像のメタデータ。
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMetadata {
    width: u32,
    height: u32,
}

#[allow(dead_code)]
impl CacheStore {
    /// 指定されたディレクトリをキャッシュルートとして新しい CacheStore を作成する。
    pub fn new(cache_dir: &str) -> Self {
        Self {
            cache_dir: PathBuf::from(cache_dir),
        }
    }

    /// キャッシュキーからディレクトリパスを計算する。
    fn key_dir(&self, key: &str) -> PathBuf {
        self.cache_dir.join(key)
    }

    /// MrcLayers をキャッシュに保存する。
    ///
    /// キャッシュディレクトリが存在しない場合は自動的に作成する。
    pub fn store(&self, key: &str, layers: &MrcLayers) -> crate::error::Result<()> {
        let dir = self.key_dir(key);
        fs::create_dir_all(&dir).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        fs::write(dir.join("mask.jbig2"), &layers.mask_jbig2)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        fs::write(dir.join("foreground.jpg"), &layers.foreground_jpeg)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        fs::write(dir.join("background.jpg"), &layers.background_jpeg)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        let metadata = CacheMetadata {
            width: layers.width,
            height: layers.height,
        };
        let metadata_json =
            serde_json::to_string(&metadata).map_err(|e| PdfMaskError::cache(e.to_string()))?;
        fs::write(dir.join("metadata.json"), metadata_json.as_bytes())
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        Ok(())
    }

    /// キャッシュから MrcLayers を取得する。キャッシュミスの場合は None を返す。
    pub fn retrieve(&self, key: &str) -> crate::error::Result<Option<MrcLayers>> {
        let dir = self.key_dir(key);
        if !dir.exists() {
            return Ok(None);
        }

        let mask_jbig2 =
            fs::read(dir.join("mask.jbig2")).map_err(|e| PdfMaskError::cache(e.to_string()))?;
        let foreground_jpeg =
            fs::read(dir.join("foreground.jpg")).map_err(|e| PdfMaskError::cache(e.to_string()))?;
        let background_jpeg =
            fs::read(dir.join("background.jpg")).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        let metadata_str = fs::read_to_string(dir.join("metadata.json"))
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        let metadata: CacheMetadata =
            serde_json::from_str(&metadata_str).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        Ok(Some(MrcLayers {
            mask_jbig2,
            foreground_jpeg,
            background_jpeg,
            width: metadata.width,
            height: metadata.height,
        }))
    }

    /// キャッシュキーが存在するか確認する。
    pub fn contains(&self, key: &str) -> bool {
        self.key_dir(key).exists()
    }
}
