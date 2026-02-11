use std::path::{Path, PathBuf};

use image::{DynamicImage, RgbaImage};
use lopdf::dictionary;
use pdf_masking::cache::hash::CacheSettings;
use pdf_masking::cache::store::CacheStore;
use pdf_masking::config::job::ColorMode;
use pdf_masking::mrc::PageOutput;
use pdf_masking::mrc::compositor::MrcConfig;
use pdf_masking::pipeline::job_runner::JobConfig;
use pdf_masking::pipeline::orchestrator::run_all_jobs;
use pdf_masking::pipeline::page_processor::{process_page, process_page_outlines};

#[test]
fn test_process_page_cache_miss() {
    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"q 100 0 0 100 0 0 cm /Im1 Do Q";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        None,
        595.276,
        841.89,
    );
    assert!(
        result.is_ok(),
        "process_page should succeed: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    assert_eq!(processed.page_index, 0);
    assert!(!processed.cache_key.is_empty());
    assert_eq!(processed.cache_key.len(), 64); // SHA-256 hex
    assert!(
        matches!(&processed.output, PageOutput::TextMasked(_)),
        "expected PageOutput::TextMasked"
    );
}

#[test]
fn test_process_page_cache_hit() {
    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let cache_store = CacheStore::new(tmp_dir.path());

    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"q 100 0 0 100 0 0 cm /Im1 Do Q";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    // First call: cache miss, should compose and store
    let result1 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        None,
        595.276,
        841.89,
    );
    assert!(result1.is_ok());
    let processed1 = result1.unwrap();

    // Verify cache now contains the entry
    assert!(cache_store.contains(&processed1.cache_key));

    // Second call: cache hit, should return cached layers
    let result2 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        None,
        595.276,
        841.89,
    );
    assert!(result2.is_ok());
    let processed2 = result2.unwrap();

    // Same cache key
    assert_eq!(processed1.cache_key, processed2.cache_key);
    // Both should be TextMasked
    assert!(
        matches!(&processed1.output, PageOutput::TextMasked(_)),
        "expected TextMasked for first result"
    );
    assert!(
        matches!(&processed2.output, PageOutput::TextMasked(_)),
        "expected TextMasked for second result"
    );
}

/// image_streams=Some (非空) の場合、TextMaskedモードに分岐することを検証
#[test]
fn test_process_page_text_masked_with_image_streams() {
    use std::collections::HashMap;

    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"BT /F1 12 Tf (Hello) Tj ET";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    // 画像XObjectを持つストリームマップ
    let mut image_streams = HashMap::new();
    let img_stream = lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 50,
            "Height" => 50,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        vec![0u8; 4],
    );
    image_streams.insert("Im1".to_string(), img_stream);

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        Some(&image_streams),
        595.276,
        841.89,
    );
    assert!(
        result.is_ok(),
        "process_page with image_streams should succeed: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    match &processed.output {
        PageOutput::TextMasked(data) => {
            // BT...ETがすべて除去されたストリーム（元がBT...ETのみなので空になる）
            assert_eq!(data.page_index, 0);
            assert_eq!(data.color_mode, ColorMode::Rgb);
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// image_streams=None の場合もTextMaskedモードを使用する。
/// 画像XObjectが無いページでもテキスト領域のみをJPEG化する方がMRCより効率的。
#[test]
fn test_process_page_text_masked_without_image_streams() {
    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"BT /F1 12 Tf (Hello) Tj ET";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        None, // image_streams=None でもTextMaskedモードになるべき
        595.276,
        841.89,
    );
    assert!(
        result.is_ok(),
        "process_page should succeed: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    match &processed.output {
        PageOutput::TextMasked(data) => {
            assert_eq!(data.page_index, 0);
            assert_eq!(data.color_mode, ColorMode::Rgb);
            assert!(data.modified_images.is_empty());
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

#[test]
fn test_job_config_creation() {
    use std::collections::HashMap;

    let mut overrides = HashMap::new();
    overrides.insert(2, ColorMode::Bw);
    overrides.insert(3, ColorMode::Skip);

    let config = JobConfig {
        input_path: PathBuf::from("input.pdf"),
        output_path: PathBuf::from("output.pdf"),
        default_color_mode: ColorMode::Rgb,
        color_mode_overrides: overrides.clone(),
        dpi: 300,
        bg_quality: 50,
        fg_quality: 30,
        cache_dir: Some(PathBuf::from(".cache")),
    };

    assert_eq!(config.input_path, Path::new("input.pdf"));
    assert_eq!(config.output_path, Path::new("output.pdf"));
    assert_eq!(config.default_color_mode, ColorMode::Rgb);
    assert_eq!(config.color_mode_overrides.len(), 2);
    assert_eq!(config.color_mode_overrides.get(&2), Some(&ColorMode::Bw));
    assert_eq!(config.color_mode_overrides.get(&3), Some(&ColorMode::Skip));
    assert_eq!(config.dpi, 300);
    assert_eq!(config.bg_quality, 50);
    assert_eq!(config.fg_quality, 30);
    assert_eq!(config.cache_dir, Some(PathBuf::from(".cache")));
}

/// TextMaskedモードでキャッシュが効くことを検証（store → 2回目でcache hit）
#[test]
fn test_process_page_text_masked_cache_roundtrip() {
    use std::collections::HashMap;

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let cache_store = CacheStore::new(tmp_dir.path());

    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"BT /F1 12 Tf (Hello) Tj ET";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    let mut image_streams = HashMap::new();
    let img_stream = lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 50,
            "Height" => 50,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        vec![0u8; 4],
    );
    image_streams.insert("Im1".to_string(), img_stream);

    // 1回目: cache miss → TextMasked生成 + キャッシュ保存
    let result1 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        Some(&image_streams),
        595.276,
        841.89,
    );
    assert!(
        result1.is_ok(),
        "first call should succeed: {:?}",
        result1.err()
    );
    let processed1 = result1.unwrap();
    assert!(matches!(&processed1.output, PageOutput::TextMasked(_)));
    assert!(cache_store.contains(&processed1.cache_key));

    // 2回目: cache hit → キャッシュから復元
    let result2 = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        Some(&cache_store),
        Path::new("test.pdf"),
        Some(&image_streams),
        595.276,
        841.89,
    );
    assert!(
        result2.is_ok(),
        "second call should succeed: {:?}",
        result2.err()
    );
    let processed2 = result2.unwrap();

    // 同じcache keyであること
    assert_eq!(processed1.cache_key, processed2.cache_key);
    // TextMaskedとして復元されること
    match (&processed1.output, &processed2.output) {
        (PageOutput::TextMasked(d1), PageOutput::TextMasked(d2)) => {
            assert_eq!(d1.page_index, d2.page_index);
            assert_eq!(d1.color_mode, d2.color_mode);
            assert_eq!(d1.text_regions.len(), d2.text_regions.len());
            assert_eq!(d1.modified_images.len(), d2.modified_images.len());
        }
        _ => panic!("expected TextMasked for both calls"),
    }
}

/// process_page_outlines: ビットマップなしでテキスト→パス変換が成功する
#[test]
fn test_process_page_outlines_produces_text_masked() {
    use pdf_masking::pdf::font::ParsedFont;
    use std::collections::HashMap;

    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts: HashMap<String, ParsedFont> =
        pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // F4はWinAnsiEncoding（'A'のアウトラインあり）
    let content_stream = b"BT /F4 12 Tf (A) Tj ET";
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    let result = process_page_outlines(
        0,
        content_stream,
        &cache_settings,
        None,
        Path::new("sample/pdf_test.pdf"),
        None,
        &fonts,
        595.276,
        841.89,
    );
    assert!(
        result.is_ok(),
        "process_page_outlines should succeed: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    assert_eq!(processed.page_index, 0);
    assert!(!processed.cache_key.is_empty());
    match &processed.output {
        PageOutput::TextMasked(data) => {
            // テキストがパスに変換されるのでtext_regionsは空
            assert!(data.text_regions.is_empty());
            // コンテンツストリームにパス演算子が含まれること
            let text = String::from_utf8_lossy(&data.stripped_content_stream);
            let has_moveto = text.split_whitespace().any(|token| token == "m");
            assert!(
                has_moveto,
                "should contain moveto (m) operators as PDF tokens"
            );
            assert_eq!(data.color_mode, ColorMode::Rgb);
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// process_page_outlines: Bwモードでテキストがパス変換される（force_bw=true）
#[test]
fn test_process_page_outlines_accepts_bw_mode() {
    use pdf_masking::pdf::font::ParsedFont;
    use std::collections::HashMap;

    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts: HashMap<String, ParsedFont> =
        pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    // F4はWinAnsiEncoding（'A'のアウトラインあり）
    let content_stream = b"BT /F4 12 Tf (A) Tj ET";
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Bw,
    };

    let result = process_page_outlines(
        0,
        content_stream,
        &cache_settings,
        None,
        Path::new("sample/pdf_test.pdf"),
        None,
        &fonts,
        595.276,
        841.89,
    );
    assert!(
        result.is_ok(),
        "Bw mode should be accepted for text_to_outlines: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    match &processed.output {
        PageOutput::TextMasked(data) => {
            let text = String::from_utf8_lossy(&data.stripped_content_stream);
            // BW モードではテキストがパスに変換される
            let has_moveto = text.split_whitespace().any(|token| token == "m");
            assert!(has_moveto, "BW mode should produce glyph paths");
            // BW モードでは色が gray (0 g) になる
            assert!(
                text.contains("0 g"),
                "BW mode should force black color, got: {}",
                text
            );
        }
        other => panic!(
            "expected PageOutput::TextMasked, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// process_page_outlines: フォント不足でエラーを返す
#[test]
fn test_process_page_outlines_error_on_missing_font() {
    use pdf_masking::pdf::font::ParsedFont;
    use std::collections::HashMap;

    let fonts: HashMap<String, ParsedFont> = HashMap::new();
    let content_stream = b"BT /F99 12 Tf (Hello) Tj ET";
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    let result = process_page_outlines(
        0,
        content_stream,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        None,
        &fonts,
        595.276,
        841.89,
    );
    assert!(result.is_err(), "should fail when font is missing");
}

/// process_page_outlines: キャッシュが効くことを検証
#[test]
fn test_process_page_outlines_cache_roundtrip() {
    use pdf_masking::pdf::font::ParsedFont;
    use std::collections::HashMap;

    let tmp_dir = tempfile::tempdir().expect("create temp dir");
    let cache_store = CacheStore::new(tmp_dir.path());

    let doc = lopdf::Document::load("sample/pdf_test.pdf").expect("load PDF");
    let fonts: HashMap<String, ParsedFont> =
        pdf_masking::pdf::font::parse_page_fonts(&doc, 1).expect("parse fonts");

    let content_stream = b"BT /F4 12 Tf (A) Tj ET";
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    // 1回目: cache miss
    let result1 = process_page_outlines(
        0,
        content_stream,
        &cache_settings,
        Some(&cache_store),
        Path::new("sample/pdf_test.pdf"),
        None,
        &fonts,
        595.276,
        841.89,
    );
    assert!(result1.is_ok(), "first call: {:?}", result1.err());
    let processed1 = result1.unwrap();
    assert!(cache_store.contains(&processed1.cache_key));

    // 2回目: cache hit
    let result2 = process_page_outlines(
        0,
        content_stream,
        &cache_settings,
        Some(&cache_store),
        Path::new("sample/pdf_test.pdf"),
        None,
        &fonts,
        595.276,
        841.89,
    );
    assert!(result2.is_ok(), "second call: {:?}", result2.err());
    let processed2 = result2.unwrap();
    assert_eq!(processed1.cache_key, processed2.cache_key);
    assert!(matches!(&processed2.output, PageOutput::TextMasked(_)));
}

/// compose_text_masked はフォールバックチェーンの主経路として常に試行される。
/// ゴミデータのコンテンツストリームでも process_page が成功することを検証。
#[test]
fn test_process_page_succeeds_with_any_content_stream() {
    let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));
    let content_stream = b"\xff\xfe\xfd";
    let mrc_config = MrcConfig {
        bg_quality: 50,
        fg_quality: 30,
    };
    let cache_settings = CacheSettings {
        dpi: 300,
        fg_dpi: 100,
        bg_quality: 50,
        fg_quality: 30,
        color_mode: ColorMode::Rgb,
    };

    let result = process_page(
        0,
        &img,
        content_stream,
        &mrc_config,
        &cache_settings,
        None,
        Path::new("test.pdf"),
        None,
        595.276,
        841.89,
    );
    assert!(
        result.is_ok(),
        "process_page should succeed even with garbage content stream: {:?}",
        result.err()
    );

    let processed = result.unwrap();
    // compose_text_masked が成功すれば TextMasked、失敗すれば Mrc (フォールバック)
    assert!(
        matches!(
            &processed.output,
            PageOutput::TextMasked(_) | PageOutput::Mrc(_)
        ),
        "expected TextMasked or Mrc"
    );
}

#[test]
fn test_run_all_jobs_empty() {
    let jobs: Vec<JobConfig> = vec![];
    let results = run_all_jobs(&jobs);
    assert!(results.is_empty());
}
