pub mod compositor;
pub mod jbig2;
pub mod jpeg;
pub mod segmenter;

use crate::config::job::ColorMode;

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

/// ページ処理結果
#[derive(Debug)]
pub enum PageOutput {
    /// RGB/Grayscale 3層MRC
    Mrc(MrcLayers),
    /// JBIG2マスクのみ（BWモード）
    BwMask(BwLayers),
    /// 元ページをそのままコピー
    Skip(SkipData),
}
