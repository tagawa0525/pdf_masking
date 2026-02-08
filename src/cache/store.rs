// Phase 8: ファイルシステムキャッシュ: hash → MRC層バイト列
//
// Stores and retrieves PageOutput on disk, keyed by SHA-256 hash.
// MRC entries: mask.jbig2, foreground.jpg, background.jpg, metadata.json
// BW entries: mask.jbig2, metadata.json
// TextMasked entries: stripped_content.bin, region_*.jpg, modified_*.bin, metadata.json

use std::collections::HashMap;

use crate::config::job::ColorMode;
use crate::error::PdfMaskError;
use crate::mrc::{
    BwLayers, ImageModification, MrcLayers, PageOutput, TextMaskedData, TextRegionCrop,
};
use crate::pdf::content_stream::BBox;
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

/// metadata.json に保存するキャッシュエントリのメタデータ。
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMetadata {
    cache_key: String,
    #[serde(default)]
    cache_type: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default = "default_color_mode")]
    color_mode: String,
    #[serde(default)]
    page_index: u32,
    #[serde(default)]
    regions: Vec<TextRegionMeta>,
    #[serde(default)]
    modified_images: Vec<ModifiedImageMeta>,
}

/// テキスト領域のキャッシュメタデータ。
#[derive(serde::Serialize, serde::Deserialize)]
struct TextRegionMeta {
    bbox: BBox,
    pixel_width: u32,
    pixel_height: u32,
    file: String,
}

/// リダクション済み画像のキャッシュメタデータ。
#[derive(serde::Serialize, serde::Deserialize)]
struct ModifiedImageMeta {
    name: String,
    filter: String,
    color_space: String,
    bits_per_component: u8,
    file: String,
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

fn str_to_color_mode(s: &str) -> Option<ColorMode> {
    match s {
        "rgb" => Some(ColorMode::Rgb),
        "grayscale" => Some(ColorMode::Grayscale),
        "bw" => Some(ColorMode::Bw),
        "skip" => Some(ColorMode::Skip),
        _ => None,
    }
}

/// XObject名をファイル名として安全な文字列にエンコードする。
/// 英数字と `_`, `-` 以外は `%XX` (hex) に変換。
fn sanitize_xobject_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    result
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
    pub fn store(
        &self,
        key: &str,
        output: &PageOutput,
        bitmap_dims: Option<(u32, u32)>,
    ) -> crate::error::Result<()> {
        validate_cache_key(key)?;

        match output {
            PageOutput::Skip(_) => return Ok(()),
            PageOutput::TextMasked(data) => {
                let (w, h) = bitmap_dims.unwrap_or((0, 0));
                return self.store_text_masked(key, data, w, h);
            }
            _ => {}
        }

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
            PageOutput::Skip(_) | PageOutput::TextMasked(_) => unreachable!(),
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

        let cache_type = match output {
            PageOutput::BwMask(_) => "bw",
            _ => "mrc",
        };
        let metadata = CacheMetadata {
            cache_key: key.to_string(),
            cache_type: cache_type.to_string(),
            width,
            height,
            color_mode: color_mode_to_str(mode).to_string(),
            page_index: 0,
            regions: vec![],
            modified_images: vec![],
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

    /// TextMaskedData をキャッシュに保存する。
    fn store_text_masked(
        &self,
        key: &str,
        data: &TextMaskedData,
        bitmap_width: u32,
        bitmap_height: u32,
    ) -> crate::error::Result<()> {
        let dir = self.key_dir(key)?;
        let tmp_dir = dir.with_extension("tmp");

        if tmp_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_dir);
        }
        fs::create_dir_all(&tmp_dir).map_err(|e| PdfMaskError::cache(e.to_string()))?;

        // stripped_content.bin
        fs::write(
            tmp_dir.join("stripped_content.bin"),
            &data.stripped_content_stream,
        )
        .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        // region_*.jpg
        let mut region_metas = Vec::with_capacity(data.text_regions.len());
        for (i, region) in data.text_regions.iter().enumerate() {
            let filename = format!("region_{}.jpg", i);
            fs::write(tmp_dir.join(&filename), &region.jpeg_data)
                .map_err(|e| PdfMaskError::cache(e.to_string()))?;
            region_metas.push(TextRegionMeta {
                bbox: region.bbox_points.clone(),
                pixel_width: region.pixel_width,
                pixel_height: region.pixel_height,
                file: filename,
            });
        }

        // modified_*.bin (XObject名をサニタイズしてファイル名に使用)
        let mut modified_metas = Vec::with_capacity(data.modified_images.len());
        for (name, modification) in &data.modified_images {
            let safe_name = sanitize_xobject_name(name);
            let filename = format!("modified_{}.bin", safe_name);
            fs::write(tmp_dir.join(&filename), &modification.data)
                .map_err(|e| PdfMaskError::cache(e.to_string()))?;
            modified_metas.push(ModifiedImageMeta {
                name: name.clone(),
                filter: modification.filter.clone(),
                color_space: modification.color_space.clone(),
                bits_per_component: modification.bits_per_component,
                file: filename,
            });
        }

        let metadata = CacheMetadata {
            cache_key: key.to_string(),
            cache_type: "text_masked".to_string(),
            width: bitmap_width,
            height: bitmap_height,
            color_mode: color_mode_to_str(data.color_mode).to_string(),
            page_index: data.page_index,
            regions: region_metas,
            modified_images: modified_metas,
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
    ///
    /// `bitmap_dims` が指定された場合、キャッシュされたビットマップ寸法と比較し、
    /// 不一致ならキャッシュミス（None）を返す。
    pub fn retrieve(
        &self,
        key: &str,
        expected_mode: ColorMode,
        bitmap_dims: Option<(u32, u32)>,
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

        // ビットマップ寸法の一致チェック（指定された場合）
        if let Some((bw, bh)) = bitmap_dims {
            if metadata.width != bw || metadata.height != bh {
                return Ok(None);
            }
        }

        // TextMasked エントリの場合
        if metadata.cache_type == "text_masked" {
            return self.retrieve_text_masked(&dir, &metadata);
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

    /// TextMasked キャッシュエントリを読み込む。
    fn retrieve_text_masked(
        &self,
        dir: &Path,
        metadata: &CacheMetadata,
    ) -> crate::error::Result<Option<PageOutput>> {
        let stripped_content_stream = fs::read(dir.join("stripped_content.bin"))
            .map_err(|e| PdfMaskError::cache(e.to_string()))?;

        let mut text_regions = Vec::with_capacity(metadata.regions.len());
        for region_meta in &metadata.regions {
            let jpeg_data = fs::read(dir.join(&region_meta.file))
                .map_err(|e| PdfMaskError::cache(e.to_string()))?;
            text_regions.push(TextRegionCrop {
                jpeg_data,
                bbox_points: region_meta.bbox.clone(),
                pixel_width: region_meta.pixel_width,
                pixel_height: region_meta.pixel_height,
            });
        }

        let mut modified_images = HashMap::with_capacity(metadata.modified_images.len());
        for img_meta in &metadata.modified_images {
            let data = fs::read(dir.join(&img_meta.file))
                .map_err(|e| PdfMaskError::cache(e.to_string()))?;
            modified_images.insert(
                img_meta.name.clone(),
                ImageModification {
                    data,
                    filter: img_meta.filter.clone(),
                    color_space: img_meta.color_space.clone(),
                    bits_per_component: img_meta.bits_per_component,
                },
            );
        }

        let color_mode = str_to_color_mode(&metadata.color_mode).unwrap_or(ColorMode::Rgb);

        Ok(Some(PageOutput::TextMasked(TextMaskedData {
            stripped_content_stream,
            text_regions,
            modified_images,
            page_index: metadata.page_index,
            color_mode,
        })))
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

        if metadata.cache_type == "text_masked" {
            // TextMasked: stripped_content.bin + 全region + 全modified_image
            if !dir.join("stripped_content.bin").exists() {
                return false;
            }
            for region in &metadata.regions {
                if !dir.join(&region.file).exists() {
                    return false;
                }
            }
            for img in &metadata.modified_images {
                if !dir.join(&img.file).exists() {
                    return false;
                }
            }
            return true;
        }

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
