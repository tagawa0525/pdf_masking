pub mod compositor;
pub mod jbig2;
pub mod jpeg;
pub mod segmenter;

use std::collections::HashMap;

use crate::config::job::ColorMode;
use crate::pdf::content_stream::BBox;

#[derive(Debug)]
pub struct MrcLayers {
    pub mask_jbig2: Vec<u8>,
    pub foreground_jpeg: Vec<u8>,
    pub background_jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub color_mode: ColorMode,
}

/// JBIG2マスクのみ（BWモード用）
#[derive(Debug)]
pub struct BwLayers {
    pub mask_jbig2: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// スキップモード用データ
#[derive(Debug)]
pub struct SkipData {
    pub page_index: u32,
}

/// テキスト領域のクロップ結果
#[derive(Debug)]
pub struct TextRegionCrop {
    pub jpeg_data: Vec<u8>,
    pub bbox_points: BBox,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// 画像XObjectの変更内容
#[derive(Debug)]
pub struct ImageModification {
    pub data: Vec<u8>,
    pub filter: String,
    pub color_space: String,
    pub bits_per_component: u8,
}

/// テキスト選択的ラスタライズの処理結果
#[derive(Debug)]
pub struct TextMaskedData {
    pub stripped_content_stream: Vec<u8>,
    pub text_regions: Vec<TextRegionCrop>,
    pub modified_images: HashMap<String, ImageModification>,
    pub page_index: u32,
    pub color_mode: ColorMode,
}

/// ページ処理結果
#[derive(Debug)]
pub enum PageOutput {
    /// RGB/Grayscale 3層MRC
    Mrc(MrcLayers),
    /// JBIG2マスクのみ（BWモード）
    BwMask(BwLayers),
    /// 元ページをそのままコピー
    Skip(SkipData),
    /// テキストのみ画像化、画像XObjectは保持
    TextMasked(TextMaskedData),
}
