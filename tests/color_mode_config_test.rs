// Phase 1: ColorMode enum と YAML パーステスト (RED)

use pdf_masking::config::job::{ColorMode, JobFile};
use pdf_masking::config::merged::MergedConfig;
use pdf_masking::config::settings::Settings;

// ============================================================
// 1. ColorMode デシリアライゼーション
// ============================================================

#[test]
fn test_color_mode_deserialize_rgb() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: rgb
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse rgb");
    assert_eq!(job_file.jobs[0].color_mode, Some(ColorMode::Rgb));
}

#[test]
fn test_color_mode_deserialize_grayscale() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: grayscale
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse grayscale");
    assert_eq!(job_file.jobs[0].color_mode, Some(ColorMode::Grayscale));
}

#[test]
fn test_color_mode_deserialize_bw() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: bw
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse bw");
    assert_eq!(job_file.jobs[0].color_mode, Some(ColorMode::Bw));
}

#[test]
fn test_color_mode_deserialize_skip() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: skip
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse skip");
    assert_eq!(job_file.jobs[0].color_mode, Some(ColorMode::Skip));
}

#[test]
fn test_color_mode_optional() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse without color_mode");
    assert_eq!(job_file.jobs[0].color_mode, None);
}

// ============================================================
// 2. ページ別モード指定フィールドのパース
// ============================================================

#[test]
fn test_parse_rgb_pages_string() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    rgb_pages: "5-8, 100-120"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse rgb_pages");
    let pages = job_file.jobs[0]
        .rgb_pages
        .as_ref()
        .expect("rgb_pages should be Some");
    assert!(pages.contains(&5));
    assert!(pages.contains(&8));
    assert!(pages.contains(&100));
    assert!(pages.contains(&120));
}

#[test]
fn test_parse_grayscale_pages_array() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    grayscale_pages: [50, 51, 52, 53, 54, 55]
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse grayscale_pages");
    let pages = job_file.jobs[0]
        .grayscale_pages
        .as_ref()
        .expect("grayscale_pages should be Some");
    assert_eq!(pages, &vec![50, 51, 52, 53, 54, 55]);
}

#[test]
fn test_parse_bw_pages() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    bw_pages: "1-10"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse bw_pages");
    let pages = job_file.jobs[0]
        .bw_pages
        .as_ref()
        .expect("bw_pages should be Some");
    assert_eq!(pages.len(), 10);
    assert!(pages.contains(&1));
    assert!(pages.contains(&10));
}

#[test]
fn test_parse_skip_pages() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    skip_pages: [200]
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse skip_pages");
    let pages = job_file.jobs[0]
        .skip_pages
        .as_ref()
        .expect("skip_pages should be Some");
    assert_eq!(pages, &vec![200]);
}

// ============================================================
// 3. resolve_page_modes() のテスト
// ============================================================

#[test]
fn test_resolve_page_modes_default_only() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: bw
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse");
    let job = &job_file.jobs[0];

    let page_modes = job
        .resolve_page_modes(ColorMode::Rgb)
        .expect("should resolve");

    // デフォルトはbw、オーバーライドなし
    assert_eq!(page_modes.len(), 0, "no overrides -> empty map");
}

#[test]
fn test_resolve_page_modes_with_overrides() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: bw
    rgb_pages: "5-8"
    grayscale_pages: [10]
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse");
    let job = &job_file.jobs[0];

    let page_modes = job
        .resolve_page_modes(ColorMode::Bw)
        .expect("should resolve");

    assert_eq!(page_modes.get(&5), Some(&ColorMode::Rgb));
    assert_eq!(page_modes.get(&6), Some(&ColorMode::Rgb));
    assert_eq!(page_modes.get(&7), Some(&ColorMode::Rgb));
    assert_eq!(page_modes.get(&8), Some(&ColorMode::Rgb));
    assert_eq!(page_modes.get(&10), Some(&ColorMode::Grayscale));

    // ページ1はオーバーライドなし → Noneが返る（デフォルト使用）
    assert_eq!(page_modes.get(&1), None);
}

#[test]
fn test_resolve_page_modes_conflict_detection() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: bw
    rgb_pages: "5-8"
    grayscale_pages: "7-10"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse");
    let job = &job_file.jobs[0];

    let result = job.resolve_page_modes(ColorMode::Bw);
    assert!(result.is_err(), "should detect conflict on page 7 and 8");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("conflict")
            || err_msg.contains("Conflict")
            || err_msg.contains("multiple")
    );
}

#[test]
fn test_resolve_page_modes_all_modes() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    color_mode: rgb
    bw_pages: [1]
    grayscale_pages: [2]
    skip_pages: [3]
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse");
    let job = &job_file.jobs[0];

    let page_modes = job
        .resolve_page_modes(ColorMode::Rgb)
        .expect("should resolve");

    assert_eq!(page_modes.get(&1), Some(&ColorMode::Bw));
    assert_eq!(page_modes.get(&2), Some(&ColorMode::Grayscale));
    assert_eq!(page_modes.get(&3), Some(&ColorMode::Skip));
}

// ============================================================
// 4. Settings にデフォルト color_mode を追加
// ============================================================

#[test]
fn test_settings_default_color_mode_is_rgb() {
    let settings = Settings::default();
    assert_eq!(settings.color_mode, ColorMode::Rgb);
}

#[test]
fn test_settings_parse_color_mode_from_yaml() {
    let yaml = r#"
color_mode: bw
dpi: 300
"#;
    let settings = Settings::from_yaml(yaml).expect("should parse");
    assert_eq!(settings.color_mode, ColorMode::Bw);
}

// ============================================================
// 5. MergedConfig にカラーモードを追加
// ============================================================

#[test]
fn test_merged_config_job_color_mode_overrides_settings() {
    let settings_yaml = "color_mode: rgb\ndpi: 300";
    let settings = Settings::from_yaml(settings_yaml).expect("parse settings");

    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
    color_mode: bw
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse job");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);

    assert_eq!(merged.color_mode, ColorMode::Bw);
}

#[test]
fn test_merged_config_uses_settings_color_mode_if_job_none() {
    let settings_yaml = "color_mode: grayscale\ndpi: 300";
    let settings = Settings::from_yaml(settings_yaml).expect("parse settings");

    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse job");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);

    assert_eq!(merged.color_mode, ColorMode::Grayscale);
}
