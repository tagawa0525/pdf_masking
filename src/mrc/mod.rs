pub mod compositor;
pub mod jbig2;
pub mod jpeg;
pub mod segmenter;

pub struct MrcLayers {
    pub mask_jbig2: Vec<u8>,
    pub foreground_jpeg: Vec<u8>,
    pub background_jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
