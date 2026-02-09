// Phase 5: pipeline integration: bitmap + config -> MrcLayers

use std::collections::HashMap;

use super::{
    BwLayers, ImageModification, MrcLayers, TextMaskedData, TextRegionCrop, jbig2, jpeg, segmenter,
};
use crate::config::job::ColorMode;
use crate::error::PdfMaskError;
use crate::mrc::segmenter::PixelBBox;
use crate::pdf::content_stream::{
    extract_white_fill_rects, extract_xobject_placements, pixel_to_page_coords,
    strip_text_operators,
};
use crate::pdf::font::ParsedFont;
use crate::pdf::image_xobject::{bbox_overlaps, redact_image_regions};
use image::{DynamicImage, RgbaImage};

/// Configuration for MRC layer generation.
pub struct MrcConfig {
    /// JPEG quality for the background layer (1-100)
    pub bg_quality: u8,
    /// JPEG quality for the foreground layer (1-100)
    pub fg_quality: u8,
}

/// Generate MRC layers from an RGBA bitmap.
///
/// Pipeline:
/// 1. Segment text regions into a 1-bit mask
/// 2. Encode the mask as JBIG2
/// 3. Convert RGBA to RGB/Gray
/// 4. Encode the background as JPEG
/// 5. Encode the foreground as JPEG
///
/// # Arguments
/// * `rgba_data` - Raw RGBA pixel data (4 bytes per pixel)
/// * `width`     - Image width in pixels
/// * `height`    - Image height in pixels
/// * `config`    - Quality settings for the output layers
/// * `color_mode` - RGB or Grayscale
pub fn compose(
    rgba_data: &[u8],
    width: u32,
    height: u32,
    config: &MrcConfig,
    color_mode: ColorMode,
) -> crate::error::Result<MrcLayers> {
    // 1. Segment: RGBA -> 1-bit text mask
    let mut text_mask = segmenter::segment_text_mask(rgba_data, width, height)?;

    // 2. Mask layer: JBIG2-encode the 1-bit mask
    let mask_jbig2 = jbig2::encode_mask(&mut text_mask)?;

    // 3. Convert RGBA -> image
    let img = RgbaImage::from_raw(width, height, rgba_data.to_vec())
        .ok_or_else(|| PdfMaskError::jpeg_encode("Failed to create image from RGBA data"))?;
    let dynamic = DynamicImage::ImageRgba8(img);

    let (background_jpeg, foreground_jpeg) = match color_mode {
        ColorMode::Grayscale => {
            let gray = dynamic.to_luma8();
            let bg = jpeg::encode_gray_to_jpeg(&gray, config.bg_quality)?;
            let fg = jpeg::encode_gray_to_jpeg(&gray, config.fg_quality)?;
            (bg, fg)
        }
        _ => {
            // Rgb (default)
            let rgb = dynamic.to_rgb8();
            let bg = jpeg::encode_rgb_to_jpeg(&rgb, config.bg_quality)?;
            let fg = jpeg::encode_rgb_to_jpeg(&rgb, config.fg_quality)?;
            (bg, fg)
        }
    };

    Ok(MrcLayers {
        mask_jbig2,
        foreground_jpeg,
        background_jpeg,
        width,
        height,
        color_mode,
    })
}

/// テキスト領域をビットマップからクロップし、JPEG化する。
///
/// 各 PixelBBox 領域をクロップして、指定のカラーモードとクオリティで
/// JPEG エンコードする。
///
/// # Arguments
/// * `bitmap` - レンダリング済みのページビットマップ
/// * `bboxes` - テキスト領域の矩形リスト（ピクセル座標）
/// * `quality` - JPEG 品質 (1-100)
/// * `color_mode` - RGB または Grayscale
///
/// # Returns
/// `(jpeg_data, bbox)` のペアリスト
pub fn crop_text_regions(
    bitmap: &DynamicImage,
    bboxes: &[PixelBBox],
    quality: u8,
    color_mode: ColorMode,
) -> crate::error::Result<Vec<(Vec<u8>, PixelBBox)>> {
    if !(1..=100).contains(&quality) {
        return Err(PdfMaskError::jpeg_encode(format!(
            "JPEG quality must be 1-100, got {}",
            quality
        )));
    }

    let mut results = Vec::with_capacity(bboxes.len());

    for bbox in bboxes {
        // クロップ
        let cropped = bitmap.crop_imm(bbox.x, bbox.y, bbox.width, bbox.height);

        // カラーモードに応じてエンコード
        let jpeg_data = match color_mode {
            ColorMode::Grayscale => {
                let gray = cropped.to_luma8();
                jpeg::encode_gray_to_jpeg(&gray, quality)?
            }
            ColorMode::Rgb => {
                let rgb = cropped.to_rgb8();
                jpeg::encode_rgb_to_jpeg(&rgb, quality)?
            }
            ColorMode::Bw | ColorMode::Skip => {
                return Err(PdfMaskError::jpeg_encode(format!(
                    "crop_text_regions does not support {:?} color mode",
                    color_mode
                )));
            }
        };

        results.push((jpeg_data, bbox.clone()));
    }

    Ok(results)
}

/// BWモード: segmenter + JBIG2のみ。JPEG層なし。
pub fn compose_bw(rgba_data: &[u8], width: u32, height: u32) -> crate::error::Result<BwLayers> {
    let mut text_mask = segmenter::segment_text_mask(rgba_data, width, height)?;
    let mask_jbig2 = jbig2::encode_mask(&mut text_mask)?;

    Ok(BwLayers {
        mask_jbig2,
        width,
        height,
    })
}

/// テキスト選択的ラスタライズの入力パラメータ。
pub struct TextMaskedParams<'a> {
    /// 元のコンテンツストリーム
    pub content_bytes: &'a [u8],
    /// レンダリング済みRGBAビットマップ
    pub rgba_data: &'a [u8],
    /// ビットマップ幅(px)
    pub bitmap_width: u32,
    /// ビットマップ高さ(px)
    pub bitmap_height: u32,
    /// ページ幅(pt)
    pub page_width_pts: f64,
    /// ページ高さ(pt)
    pub page_height_pts: f64,
    /// XObject名 → lopdf::Stream のマップ
    pub image_streams: &'a HashMap<String, lopdf::Stream>,
    /// JPEG品質 (1-100)
    pub quality: u8,
    /// RGB or Grayscale
    pub color_mode: ColorMode,
    /// ページ番号(0-based)
    pub page_index: u32,
}

/// テキスト選択的ラスタライズ: テキストのみ画像化し、画像XObjectは保持。
///
/// # Pipeline
/// 1. コンテンツストリームからBT...ETブロックを除去
/// 2. 白色fill矩形を検出
/// 3. 画像XObject配置を取得
/// 4. 白色矩形と重なる画像をリダクション
/// 5. ビットマップからテキスト領域を抽出・JPEG化
pub fn compose_text_masked(params: &TextMaskedParams) -> crate::error::Result<TextMaskedData> {
    // 1. テキスト除去済みコンテンツストリーム
    let stripped_content_stream = strip_text_operators(params.content_bytes)?;

    // 2. 白色fill矩形を検出
    let white_rects = extract_white_fill_rects(params.content_bytes)?;

    // 3. 画像XObject配置を取得
    let placements = extract_xobject_placements(params.content_bytes)?;

    // 4. 白色矩形と重なる画像をリダクション
    let mut modified_images: HashMap<String, ImageModification> = HashMap::new();
    for placement in &placements {
        if let Some(stream) = params.image_streams.get(&placement.name) {
            let overlapping: Vec<_> = white_rects
                .iter()
                .filter(|wr| bbox_overlaps(wr, &placement.bbox))
                .cloned()
                .collect();

            if !overlapping.is_empty()
                && let Some(redacted) = redact_image_regions(stream, &overlapping, &placement.bbox)?
            {
                modified_images.insert(
                    placement.name.clone(),
                    ImageModification {
                        data: redacted.data,
                        filter: redacted.filter,
                        color_space: redacted.color_space,
                        bits_per_component: redacted.bits_per_component,
                    },
                );
            }
        }
    }

    // 5. ビットマップからテキスト領域を抽出・JPEG化
    /// テキスト領域のマージ距離（px）。近接する矩形を結合してXObject数を削減する。
    const TEXT_BBOX_MERGE_DISTANCE: u32 = 5;

    let text_mask =
        segmenter::segment_text_mask(params.rgba_data, params.bitmap_width, params.bitmap_height)?;
    let bboxes = segmenter::extract_text_bboxes(&text_mask, TEXT_BBOX_MERGE_DISTANCE)?;

    // テキスト領域が無い場合はビットマップコピーを回避して早期リターン
    if bboxes.is_empty() {
        return Ok(TextMaskedData {
            stripped_content_stream,
            text_regions: Vec::new(),
            modified_images,
            page_index: params.page_index,
            color_mode: params.color_mode,
        });
    }

    let bitmap = RgbaImage::from_raw(
        params.bitmap_width,
        params.bitmap_height,
        params.rgba_data.to_vec(),
    )
    .ok_or_else(|| PdfMaskError::jpeg_encode("Failed to create bitmap from RGBA data"))?;
    let dynamic = DynamicImage::ImageRgba8(bitmap);

    let crops = crop_text_regions(&dynamic, &bboxes, params.quality, params.color_mode)?;

    let text_regions: Vec<TextRegionCrop> = crops
        .into_iter()
        .map(|(jpeg_data, pixel_bbox)| {
            let bbox_points = pixel_to_page_coords(
                &pixel_bbox,
                params.page_width_pts,
                params.page_height_pts,
                params.bitmap_width,
                params.bitmap_height,
            )?;
            Ok(TextRegionCrop {
                jpeg_data,
                bbox_points,
                pixel_width: pixel_bbox.width,
                pixel_height: pixel_bbox.height,
            })
        })
        .collect::<crate::error::Result<Vec<_>>>()?;

    Ok(TextMaskedData {
        stripped_content_stream,
        text_regions,
        modified_images,
        page_index: params.page_index,
        color_mode: params.color_mode,
    })
}

/// テキスト→アウトライン変換の入力パラメータ。
pub struct TextOutlinesParams<'a> {
    /// 元のコンテンツストリーム
    pub content_bytes: &'a [u8],
    /// ページのフォントマップ
    pub fonts: &'a HashMap<String, ParsedFont>,
    /// XObject名 → lopdf::Stream のマップ
    pub image_streams: &'a HashMap<String, lopdf::Stream>,
    /// RGB or Grayscale
    pub color_mode: ColorMode,
    /// ページ番号(0-based)
    pub page_index: u32,
}

/// テキスト→アウトライン変換: BT...ETをベクターパスに変換し、画像リダクションも行う。
///
/// `compose_text_masked` と異なり、テキストをJPEG画像化せずベクターパスとして
/// コンテンツストリームに残す。text_regionsは空になる。
pub fn compose_text_outlines(_params: &TextOutlinesParams) -> crate::error::Result<TextMaskedData> {
    todo!("compose_text_outlines: GREEN phase で実装")
}
