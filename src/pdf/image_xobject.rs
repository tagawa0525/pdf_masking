// Phase 7: 画像XObjectのデコード/再エンコード、重なり検出・塗りつぶし

use crate::error::PdfMaskError;
use crate::mrc::{jbig2, jpeg};
use crate::pdf::content_stream::BBox;
use flate2::read::ZlibDecoder;
use image::{DynamicImage, GrayImage, RgbImage};
use lopdf::Object;
use std::io::Read;

/// 2つのBBoxの重なりを判定する。
///
/// 辺が接しているだけの場合は重ならないと判定する（strict inequality）。
pub fn bbox_overlaps(a: &BBox, b: &BBox) -> bool {
    !(a.x_max <= b.x_min || b.x_max <= a.x_min || a.y_max <= b.y_min || b.y_max <= a.y_min)
}

/// 画像XObjectのメタデータ
#[derive(Debug, Clone)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub bits_per_component: u8,
    pub color_space: String,
    pub filter: Option<String>,
}

/// リダクション済み画像データ
#[derive(Debug)]
pub struct RedactedImage {
    pub data: Vec<u8>,
    pub filter: String,
    pub color_space: String,
    pub bits_per_component: u8,
}

/// 最適圧縮済み画像データ
#[derive(Debug)]
pub struct OptimizedImage {
    pub data: Vec<u8>,
    pub filter: &'static str,
    pub color_space: &'static str,
    pub bits_per_component: u8,
}

/// 画像XObjectのストリームから画像メタデータを読み取る。
fn read_image_meta(stream: &lopdf::Stream) -> crate::error::Result<ImageMeta> {
    let dict = &stream.dict;

    let width = dict_get_u32(dict, b"Width")?;
    let height = dict_get_u32(dict, b"Height")?;
    // BitsPerComponent: missing keyの場合のみデフォルト8、型エラーは伝播
    let bits_per_component = match dict.get(b"BitsPerComponent") {
        Ok(_) => dict_get_u32(dict, b"BitsPerComponent")? as u8,
        Err(_) => 8,
    };

    let color_space = match dict.get(b"ColorSpace") {
        Ok(obj) => match obj {
            Object::Name(name) => String::from_utf8_lossy(name).to_string(),
            _ => "DeviceRGB".to_string(),
        },
        Err(_) => "DeviceRGB".to_string(),
    };

    let filter = match dict.get(b"Filter") {
        Ok(Object::Name(name)) => Some(String::from_utf8_lossy(name).to_string()),
        Ok(Object::Array(arr)) => {
            // フィルタ連鎖: 最初のフィルタを取得
            arr.first().and_then(|obj| {
                if let Object::Name(name) = obj {
                    Some(String::from_utf8_lossy(name).to_string())
                } else {
                    None
                }
            })
        }
        _ => None,
    };

    Ok(ImageMeta {
        width,
        height,
        bits_per_component,
        color_space,
        filter,
    })
}

/// 辞書からu32値を取得するヘルパー（負の値はエラー）
fn dict_get_u32(dict: &lopdf::Dictionary, key: &[u8]) -> crate::error::Result<u32> {
    match dict.get(key) {
        Ok(Object::Integer(i)) => {
            let val = *i;
            if val < 0 || val > u32::MAX as i64 {
                Err(PdfMaskError::image_xobject(format!(
                    "Value out of u32 range for {:?}: {}",
                    String::from_utf8_lossy(key),
                    val
                )))
            } else {
                Ok(val as u32)
            }
        }
        Ok(Object::Real(f)) => {
            let val = *f;
            if val < 0.0 || val > u32::MAX as f32 {
                Err(PdfMaskError::image_xobject(format!(
                    "Value out of u32 range for {:?}: {}",
                    String::from_utf8_lossy(key),
                    val
                )))
            } else {
                Ok(val as u32)
            }
        }
        Ok(other) => Err(PdfMaskError::image_xobject(format!(
            "Expected integer for {:?}, got {:?}",
            String::from_utf8_lossy(key),
            other
        ))),
        Err(_) => Err(PdfMaskError::image_xobject(format!(
            "Missing required key: {:?}",
            String::from_utf8_lossy(key),
        ))),
    }
}

/// 画像XObjectのストリームデータをデコードしてDynamicImageに変換する。
///
/// 対応フィルタ:
/// - DCTDecode (JPEG)
/// - FlateDecode (raw pixels + zlib)
/// - 非圧縮 (raw pixels)
fn decode_image_stream(
    stream: &lopdf::Stream,
    meta: &ImageMeta,
) -> crate::error::Result<DynamicImage> {
    let raw = &stream.content;

    match meta.filter.as_deref() {
        Some("DCTDecode") => decode_jpeg(raw),
        Some("FlateDecode") => decode_flate(raw, meta),
        None => decode_raw(raw, meta),
        Some(other) => Err(PdfMaskError::image_xobject(format!(
            "Unsupported image filter: {}",
            other
        ))),
    }
}

/// JPEGデータをデコード
fn decode_jpeg(data: &[u8]) -> crate::error::Result<DynamicImage> {
    let reader = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| PdfMaskError::image_xobject(format!("JPEG decode error: {}", e)))?;
    reader
        .decode()
        .map_err(|e| PdfMaskError::image_xobject(format!("JPEG decode error: {}", e)))
}

/// FlateDecode (zlib) で圧縮されたraw pixelデータをデコード
fn decode_flate(data: &[u8], meta: &ImageMeta) -> crate::error::Result<DynamicImage> {
    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| PdfMaskError::image_xobject(format!("FlateDecode error: {}", e)))?;
    decode_raw(&decompressed, meta)
}

/// Raw pixelデータからDynamicImageを構築
fn decode_raw(data: &[u8], meta: &ImageMeta) -> crate::error::Result<DynamicImage> {
    let w = meta.width;
    let h = meta.height;

    match (meta.color_space.as_str(), meta.bits_per_component) {
        ("DeviceRGB", 8) => {
            let expected = (w as usize) * (h as usize) * 3;
            if data.len() < expected {
                return Err(PdfMaskError::image_xobject(format!(
                    "RGB data too short: expected {}, got {}",
                    expected,
                    data.len()
                )));
            }
            let img = RgbImage::from_raw(w, h, data[..expected].to_vec()).ok_or_else(|| {
                PdfMaskError::image_xobject("Failed to create RGB image from raw data")
            })?;
            Ok(DynamicImage::ImageRgb8(img))
        }
        ("DeviceGray", 8) => {
            let expected = (w as usize) * (h as usize);
            if data.len() < expected {
                return Err(PdfMaskError::image_xobject(format!(
                    "Gray data too short: expected {}, got {}",
                    expected,
                    data.len()
                )));
            }
            let img = GrayImage::from_raw(w, h, data[..expected].to_vec()).ok_or_else(|| {
                PdfMaskError::image_xobject("Failed to create Gray image from raw data")
            })?;
            Ok(DynamicImage::ImageLuma8(img))
        }
        (cs, bpc) => Err(PdfMaskError::image_xobject(format!(
            "Unsupported color space / BPC combination: {} / {}",
            cs, bpc
        ))),
    }
}

/// ページ座標のBBoxを画像ピクセル座標に変換する。
///
/// 画像は `image_placement` BBoxの範囲にマッピングされている。
/// `redact_bbox` をページ座標から画像ピクセル座標に変換する。
fn page_to_image_coords(
    redact_bbox: &BBox,
    image_placement: &BBox,
    img_width: u32,
    img_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let page_w = image_placement.x_max - image_placement.x_min;
    let page_h = image_placement.y_max - image_placement.y_min;

    if page_w <= 0.0 || page_h <= 0.0 {
        return None;
    }

    let scale_x = img_width as f64 / page_w;
    let scale_y = img_height as f64 / page_h;

    // ページ座標 → 画像ローカル座標（画像配置基準）
    let local_x_min = redact_bbox.x_min - image_placement.x_min;
    let local_x_max = redact_bbox.x_max - image_placement.x_min;
    // PDF Y軸は下から上、画像は上から下なので反転
    let local_y_min_pdf = redact_bbox.y_min - image_placement.y_min;
    let local_y_max_pdf = redact_bbox.y_max - image_placement.y_min;

    // ピクセル座標に変換（Y軸反転: PDF上端 = 画像上端）
    // min座標はfloor、max座標はceilでリダクション領域を確実にカバー
    let px_x_min = (local_x_min * scale_x).max(0.0).floor() as u32;
    let px_x_max = (local_x_max * scale_x).min(img_width as f64).ceil() as u32;
    // Y反転: PDF y_max → image y=0
    let px_y_min = ((page_h - local_y_max_pdf) * scale_y).max(0.0).floor() as u32;
    let px_y_max = ((page_h - local_y_min_pdf) * scale_y)
        .min(img_height as f64)
        .ceil() as u32;

    if px_x_min >= px_x_max || px_y_min >= px_y_max {
        return None;
    }

    let w = px_x_max - px_x_min;
    let h = px_y_max - px_y_min;

    Some((px_x_min, px_y_min, w, h))
}

/// 画像XObjectをデコードし、指定領域を白で塗りつぶして再エンコードする。
///
/// # Arguments
/// * `image_stream` - PDF画像XObjectのストリーム
/// * `redact_bboxes` - 白塗り対象領域（ページ座標）
/// * `image_placement` - 画像のページ上での配置BBox
///
/// # Returns
/// * `None` - 重なりなし（変更不要）
/// * `Some(RedactedImage)` - リダクション済み画像データ
pub fn redact_image_regions(
    image_stream: &lopdf::Stream,
    redact_bboxes: &[BBox],
    image_placement: &BBox,
) -> crate::error::Result<Option<RedactedImage>> {
    let meta = read_image_meta(image_stream)?;

    // 重なり判定: いずれかのredact_bboxが画像と重なるか
    let overlapping: Vec<&BBox> = redact_bboxes
        .iter()
        .filter(|rb| bbox_overlaps(rb, image_placement))
        .collect();

    if overlapping.is_empty() {
        return Ok(None);
    }

    // ピクセル領域に変換可能な重なりがあるか確認
    let pixel_regions: Vec<(u32, u32, u32, u32)> = overlapping
        .iter()
        .filter_map(|rb| page_to_image_coords(rb, image_placement, meta.width, meta.height))
        .collect();

    if pixel_regions.is_empty() {
        return Ok(None);
    }

    // 画像デコード
    let mut img = decode_image_stream(image_stream, &meta)?;

    // 各重なり領域を白で塗りつぶし
    for (x, y, w, h) in &pixel_regions {
        fill_white(&mut img, *x, *y, *w, *h);
    }

    // 元のフィルタ形式で再エンコード
    let (data, filter) = encode_image(&img, &meta)?;

    let color_space = if img.color().has_color() {
        "DeviceRGB".to_string()
    } else {
        "DeviceGray".to_string()
    };

    Ok(Some(RedactedImage {
        data,
        filter,
        color_space,
        bits_per_component: meta.bits_per_component,
    }))
}

/// 画像の指定領域を白で塗りつぶす
fn fill_white(img: &mut DynamicImage, x: u32, y: u32, w: u32, h: u32) {
    match img {
        DynamicImage::ImageRgb8(rgb) => {
            for py in y..y.saturating_add(h).min(rgb.height()) {
                for px in x..x.saturating_add(w).min(rgb.width()) {
                    rgb.put_pixel(px, py, image::Rgb([255, 255, 255]));
                }
            }
        }
        DynamicImage::ImageLuma8(gray) => {
            for py in y..y.saturating_add(h).min(gray.height()) {
                for px in x..x.saturating_add(w).min(gray.width()) {
                    gray.put_pixel(px, py, image::Luma([255]));
                }
            }
        }
        other => {
            // Fallback: RGBに変換して処理
            let mut rgb = other.to_rgb8();
            for py in y..y.saturating_add(h).min(rgb.height()) {
                for px in x..x.saturating_add(w).min(rgb.width()) {
                    rgb.put_pixel(px, py, image::Rgb([255, 255, 255]));
                }
            }
            *other = DynamicImage::ImageRgb8(rgb);
        }
    }
}

/// 画像をメタデータに基づいてエンコードする。
/// 返り値: (エンコード済みデータ, PDF Filter名)
fn encode_image(img: &DynamicImage, meta: &ImageMeta) -> crate::error::Result<(Vec<u8>, String)> {
    match meta.filter.as_deref() {
        Some("DCTDecode") => {
            let data = if meta.color_space == "DeviceGray" {
                jpeg::encode_gray_to_jpeg(&img.to_luma8(), 85)?
            } else {
                jpeg::encode_rgb_to_jpeg(&img.to_rgb8(), 85)?
            };
            Ok((data, "DCTDecode".to_string()))
        }
        Some("FlateDecode") => {
            let raw = if meta.color_space == "DeviceGray" {
                img.to_luma8().into_raw()
            } else {
                img.to_rgb8().into_raw()
            };
            let compressed = flate_encode(&raw)?;
            Ok((compressed, "FlateDecode".to_string()))
        }
        None => {
            // 元が非圧縮の場合はそのまま非圧縮で返す
            let raw = if meta.color_space == "DeviceGray" {
                img.to_luma8().into_raw()
            } else {
                img.to_rgb8().into_raw()
            };
            Ok((raw, String::new()))
        }
        Some(other) => Err(PdfMaskError::image_xobject(format!(
            "Cannot re-encode unsupported filter: {}",
            other
        ))),
    }
}

/// zlibで圧縮
fn flate_encode(data: &[u8]) -> crate::error::Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| PdfMaskError::image_xobject(format!("Flate encode error: {}", e)))?;
    encoder
        .finish()
        .map_err(|e| PdfMaskError::image_xobject(format!("Flate encode error: {}", e)))
}

/// 画像XObjectを複数形式でエンコードし、最小サイズの結果を返す。
///
/// # Arguments
/// * `decoded` - デコード済み画像
/// * `original_size` - 元のストリームサイズ（比較用）
/// * `quality` - JPEG品質 (1-100)
///
/// # Returns
/// * `None` - 元のサイズより小さくならない
/// * `Some(OptimizedImage)` - 最適圧縮済みデータ
pub fn optimize_image_encoding(
    decoded: &DynamicImage,
    original_size: usize,
    quality: u8,
) -> crate::error::Result<Option<OptimizedImage>> {
    if !(1..=100).contains(&quality) {
        return Err(PdfMaskError::image_xobject(format!(
            "JPEG quality must be 1-100, got {}",
            quality
        )));
    }

    let mut candidates: Vec<OptimizedImage> = Vec::new();
    let is_color = decoded.color().has_color();

    // 候補A: B&W JBIG2（グレースケール画像のみ。カラー画像のJBIG2化は意味的に不適切）
    if !is_color {
        let gray = decoded.to_luma8();
        let (w, h) = gray.dimensions();
        let rgba_for_binarize: Vec<u8> = gray
            .pixels()
            .flat_map(|p| [p.0[0], p.0[0], p.0[0], 255])
            .collect();
        if let Ok(pix) = crate::ffi::leptonica::Pix::from_raw_rgba(w, h, &rgba_for_binarize) {
            let sx = w.clamp(16, 2000);
            let sy = h.clamp(16, 2000);
            if let Ok(mut binary) = pix.otsu_adaptive_threshold(sx, sy)
                && let Ok(jbig2_data) = jbig2::encode_mask(&mut binary)
            {
                candidates.push(OptimizedImage {
                    data: jbig2_data,
                    filter: "JBIG2Decode",
                    color_space: "DeviceGray",
                    bits_per_component: 1,
                });
            }
        }
    }

    // 候補B: グレースケールJPEG
    let gray = decoded.to_luma8();
    if let Ok(gray_jpeg) = jpeg::encode_gray_to_jpeg(&gray, quality) {
        candidates.push(OptimizedImage {
            data: gray_jpeg,
            filter: "DCTDecode",
            color_space: "DeviceGray",
            bits_per_component: 8,
        });
    }

    // 候補C: RGB JPEG（元がカラーの場合）
    if is_color {
        let rgb = decoded.to_rgb8();
        if let Ok(rgb_jpeg) = jpeg::encode_rgb_to_jpeg(&rgb, quality) {
            candidates.push(OptimizedImage {
                data: rgb_jpeg,
                filter: "DCTDecode",
                color_space: "DeviceRGB",
                bits_per_component: 8,
            });
        }
    }

    // 最小サイズの候補を選択（元のサイズ以下のもの）
    candidates.sort_by_key(|c| c.data.len());

    Ok(candidates
        .into_iter()
        .find(|c| c.data.len() <= original_size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    /// テスト用: 指定サイズのRGB画像データを持つJPEGストリームを作成
    fn make_jpeg_stream(width: u32, height: u32, color: [u8; 3]) -> Stream {
        // RGB画像を作成してJPEGエンコード
        let mut rgb = RgbImage::new(width, height);
        for pixel in rgb.pixels_mut() {
            *pixel = image::Rgb(color);
        }
        let jpeg_data = jpeg::encode_rgb_to_jpeg(&rgb, 85).expect("encode test JPEG");

        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        };
        Stream::new(dict, jpeg_data)
    }

    /// テスト用: Flate圧縮されたRaw RGB画像ストリームを作成
    fn make_flate_rgb_stream(width: u32, height: u32, color: [u8; 3]) -> Stream {
        let pixel_count = (width as usize) * (height as usize);
        let mut raw = Vec::with_capacity(pixel_count * 3);
        for _ in 0..pixel_count {
            raw.extend_from_slice(&color);
        }
        let compressed = flate_encode(&raw).expect("compress test data");

        let dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        };
        Stream::new(dict, compressed)
    }

    // ============================================================
    // redact_image_regions テスト
    // ============================================================

    #[test]
    fn test_redact_no_overlap() {
        let stream = make_jpeg_stream(100, 100, [128, 64, 32]);
        let image_placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };
        let redact = vec![BBox {
            x_min: 200.0,
            y_min: 200.0,
            x_max: 300.0,
            y_max: 300.0,
        }];

        let result = redact_image_regions(&stream, &redact, &image_placement).expect("redact");
        assert!(result.is_none(), "No overlap should return None");
    }

    #[test]
    fn test_redact_with_overlap_jpeg() {
        let stream = make_jpeg_stream(100, 100, [128, 64, 32]);
        let image_placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };
        // 画像の左上25%を白塗り
        let redact = vec![BBox {
            x_min: 0.0,
            y_min: 50.0,
            x_max: 50.0,
            y_max: 100.0,
        }];

        let result = redact_image_regions(&stream, &redact, &image_placement).expect("redact");
        assert!(result.is_some(), "Overlap should return Some");
        let redacted = result.unwrap();
        assert_eq!(redacted.filter, "DCTDecode");
        assert_eq!(redacted.color_space, "DeviceRGB");
        assert!(!redacted.data.is_empty());
    }

    #[test]
    fn test_redact_with_overlap_flate() {
        let stream = make_flate_rgb_stream(100, 100, [128, 64, 32]);
        let image_placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };
        let redact = vec![BBox {
            x_min: 25.0,
            y_min: 25.0,
            x_max: 75.0,
            y_max: 75.0,
        }];

        let result = redact_image_regions(&stream, &redact, &image_placement).expect("redact");
        assert!(result.is_some(), "Overlap should return Some");
        let redacted = result.unwrap();
        assert_eq!(redacted.filter, "FlateDecode");
    }

    #[test]
    fn test_redact_multiple_regions() {
        let stream = make_jpeg_stream(200, 200, [100, 100, 100]);
        let image_placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 200.0,
            y_max: 200.0,
        };
        let redact = vec![
            BBox {
                x_min: 10.0,
                y_min: 10.0,
                x_max: 50.0,
                y_max: 50.0,
            },
            BBox {
                x_min: 100.0,
                y_min: 100.0,
                x_max: 150.0,
                y_max: 150.0,
            },
            BBox {
                x_min: 500.0,
                y_min: 500.0,
                x_max: 600.0,
                y_max: 600.0,
            }, // 重ならない
        ];

        let result = redact_image_regions(&stream, &redact, &image_placement).expect("redact");
        assert!(result.is_some(), "Should redact overlapping regions");
    }

    #[test]
    fn test_redact_empty_bboxes() {
        let stream = make_jpeg_stream(100, 100, [128, 64, 32]);
        let image_placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };
        let redact: Vec<BBox> = vec![];

        let result = redact_image_regions(&stream, &redact, &image_placement).expect("redact");
        assert!(result.is_none(), "Empty redact list should return None");
    }

    #[test]
    fn test_redact_verifies_white_fill() {
        // 赤い画像を作成し、全面を白塗り → デコード後全ピクセルが白
        let stream = make_jpeg_stream(10, 10, [255, 0, 0]);
        let image_placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
        };
        let redact = vec![BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
        }];

        let result = redact_image_regions(&stream, &redact, &image_placement)
            .expect("redact")
            .expect("should produce redacted image");

        // 結果をデコードして白であることを確認
        let reader = image::ImageReader::new(std::io::Cursor::new(&result.data))
            .with_guessed_format()
            .expect("guess format");
        let img = reader.decode().expect("decode result");
        let rgb = img.to_rgb8();

        // JPEGは非可逆なので完全な白(255)ではないが、ほぼ白であるべき
        for pixel in rgb.pixels() {
            assert!(
                pixel.0[0] > 240 && pixel.0[1] > 240 && pixel.0[2] > 240,
                "Pixel should be nearly white after redaction, got {:?}",
                pixel.0
            );
        }
    }

    // ============================================================
    // optimize_image_encoding テスト
    // ============================================================

    #[test]
    fn test_optimize_returns_none_if_larger() {
        // 非常に小さい画像 → 最適化しても元より小さくならない場合None
        let img = DynamicImage::ImageRgb8(RgbImage::new(2, 2));
        let result = optimize_image_encoding(&img, 1, 85).expect("optimize");
        assert!(
            result.is_none(),
            "Should return None if no candidate is smaller"
        );
    }

    #[test]
    fn test_optimize_returns_smallest() {
        // 大きめの画像でoriginal_sizeを十分大きく設定
        let mut rgb = RgbImage::new(100, 100);
        for pixel in rgb.pixels_mut() {
            *pixel = image::Rgb([200, 200, 200]);
        }
        let img = DynamicImage::ImageRgb8(rgb);

        let result = optimize_image_encoding(&img, 1_000_000, 85).expect("optimize");
        assert!(result.is_some(), "Should find a smaller encoding");
        let optimized = result.unwrap();
        assert!(optimized.data.len() <= 1_000_000);
    }

    // ============================================================
    // page_to_image_coords テスト
    // ============================================================

    #[test]
    fn test_page_to_image_coords_full_overlap() {
        let redact = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };
        let placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };

        let (x, y, w, h) = page_to_image_coords(&redact, &placement, 200, 200).unwrap();
        assert_eq!((x, y, w, h), (0, 0, 200, 200));
    }

    #[test]
    fn test_page_to_image_coords_partial() {
        let redact = BBox {
            x_min: 50.0,
            y_min: 50.0,
            x_max: 100.0,
            y_max: 100.0,
        };
        let placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };

        let (x, y, w, h) = page_to_image_coords(&redact, &placement, 100, 100).unwrap();
        assert_eq!((x, y), (50, 0)); // Y反転: PDF y=50-100 → image y=0-50
        assert_eq!((w, h), (50, 50));
    }

    #[test]
    fn test_page_to_image_coords_no_intersection() {
        let redact = BBox {
            x_min: 200.0,
            y_min: 200.0,
            x_max: 300.0,
            y_max: 300.0,
        };
        let placement = BBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 100.0,
            y_max: 100.0,
        };

        // この場合はクランプ後にw=0,h=0になりNone
        let result = page_to_image_coords(&redact, &placement, 100, 100);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_image_meta_jpeg() {
        let stream = make_jpeg_stream(50, 30, [0, 0, 0]);
        let meta = read_image_meta(&stream).expect("read meta");
        assert_eq!(meta.width, 50);
        assert_eq!(meta.height, 30);
        assert_eq!(meta.bits_per_component, 8);
        assert_eq!(meta.color_space, "DeviceRGB");
        assert_eq!(meta.filter.as_deref(), Some("DCTDecode"));
    }

    #[test]
    fn test_decode_jpeg_roundtrip() {
        let stream = make_jpeg_stream(20, 20, [128, 64, 32]);
        let meta = read_image_meta(&stream).expect("read meta");
        let img = decode_image_stream(&stream, &meta).expect("decode");
        assert_eq!(img.width(), 20);
        assert_eq!(img.height(), 20);
    }

    #[test]
    fn test_decode_flate_roundtrip() {
        let stream = make_flate_rgb_stream(30, 30, [100, 150, 200]);
        let meta = read_image_meta(&stream).expect("read meta");
        let img = decode_image_stream(&stream, &meta).expect("decode");
        assert_eq!(img.width(), 30);
        assert_eq!(img.height(), 30);
        // Raw pixelなので色が正確に保持される
        let rgb = img.to_rgb8();
        let pixel = rgb.get_pixel(0, 0);
        assert_eq!(pixel.0, [100, 150, 200]);
    }
}
