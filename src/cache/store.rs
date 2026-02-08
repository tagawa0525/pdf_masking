// Phase 8: ファイルシステムキャッシュ: hash → MRC層バイト列
//
// Stores and retrieves MrcLayers on disk, keyed by SHA-256 hash.
// Each cache entry is a directory containing mask.jbig2, foreground.jpg,
// background.jpg, and metadata.json.

use crate::error::PdfMaskError;
use crate::mrc::MrcLayers;
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};

/// Expected files in a complete cache entry.
const CACHE_ENTRY_FILES: &[&str] = &[
    "mask.jbig2",
    "foreground.jpg",
    "background.jpg",
    "metadata.json",
];

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
    cache_key: String,
    width: u32,
    height: u32,
}

/// キャッシュキーが有効な SHA-256 hex 文字列であることを検証する。
///
/// 有効なキーは正確に64文字の小文字16進数([0-9a-f])である必要がある。
/// パストラバーサルや不正なディレクトリアクセスを防止する。
fn validate_cache_key(key: &str) -> crate::error::Result<()> {
    if key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(PdfMaskError::cache(format!(
            "invalid cache key: expected 64-character hex string, got '{}'",
            key
        )))
    }
}

#[allow(dead_code)]
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

    /// MrcLayers をキャッシュに保存する。
    ///
    /// キャッシュディレクトリが存在しない場合は自動的に作成する。
    /// 書き込みはアトミック: 一時ディレクトリにファイルを書き込み、
    /// 最後にrenameで最終パスに移動する。これにより、並列アクセス時に
    /// 部分的なエントリが見えることを防止する。
    pub fn store(&self, key: &str, layers: &MrcLayers) -> crate::error::Result<()> {
        let dir = self.key_dir(key)?;
        let tmp_dir = dir.with_extension("tmp");

        // 一時ディレクトリに書き込む
        // 既存のtmpディレクトリがあれば削除（前回の失敗したstore等）
        if tmp_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_dir);
        }
        fs::create_dir_all(&tmp_dir).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        fs::write(tmp_dir.join("mask.jbig2"), &layers.mask_jbig2)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        fs::write(tmp_dir.join("foreground.jpg"), &layers.foreground_jpeg)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;
        fs::write(tmp_dir.join("background.jpg"), &layers.background_jpeg)
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        let metadata = CacheMetadata {
            cache_key: key.to_string(),
            width: layers.width,
            height: layers.height,
        };
        let metadata_json =
            serde_json::to_string(&metadata).map_err(|e| PdfMaskError::cache(e.to_string()))?;
        fs::write(tmp_dir.join("metadata.json"), metadata_json.as_bytes())
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        // 既存のキャッシュエントリがあれば削除
        if dir.exists() {
            let _ = fs::remove_dir_all(&dir);
        }

        // アトミックにrename（同一ファイルシステム上）
        fs::rename(&tmp_dir, &dir).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        Ok(())
    }

    /// キャッシュから MrcLayers を取得する。キャッシュミスの場合は None を返す。
    pub fn retrieve(&self, key: &str) -> crate::error::Result<Option<MrcLayers>> {
        let dir = self.key_dir(key)?;
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

        if metadata.cache_key != key {
            return Err(PdfMaskError::cache(format!(
                "cache key mismatch: expected '{}', found '{}'",
                key, metadata.cache_key
            )));
        }

        Ok(Some(MrcLayers {
            mask_jbig2,
            foreground_jpeg,
            background_jpeg,
            width: metadata.width,
            height: metadata.height,
        }))
    }

    /// キャッシュキーが存在するか確認する。
    ///
    /// 無効なキーの場合は false を返す。
    /// ディレクトリだけでなく、すべての必要なファイルが存在するかを確認する。
    pub fn contains(&self, key: &str) -> bool {
        let dir = match self.key_dir(key) {
            Ok(d) => d,
            Err(_) => return false,
        };
        CACHE_ENTRY_FILES.iter().all(|f| dir.join(f).exists())
    }
}
