// Phase 1: 設定ファイル解析テスト (RED)

use std::io::Write;
use std::path::Path;

use pdf_masking::config::job::{JobFile, parse_page_range};
use pdf_masking::config::load_settings_for_job;
use pdf_masking::config::merged::MergedConfig;
use pdf_masking::config::settings::Settings;

// ============================================================
// 1. ページ範囲パーサ
// ============================================================

#[test]
fn test_parse_page_range_single_range() {
    let result = parse_page_range("5-10").expect("should parse range");
    assert_eq!(result, vec![5, 6, 7, 8, 9, 10]);
}

#[test]
fn test_parse_page_range_single_page() {
    let result = parse_page_range("1").expect("should parse single page");
    assert_eq!(result, vec![1]);
}

#[test]
fn test_parse_page_range_mixed() {
    let result = parse_page_range("1, 3, 5-10, 15").expect("should parse mixed");
    assert_eq!(result, vec![1, 3, 5, 6, 7, 8, 9, 10, 15]);
}

#[test]
fn test_parse_page_range_invalid_text() {
    let result = parse_page_range("abc");
    assert!(result.is_err(), "should fail on non-numeric input");
}

#[test]
fn test_parse_page_range_reversed_range() {
    let result = parse_page_range("10-5");
    assert!(result.is_err(), "should fail when start > end");
}

#[test]
fn test_parse_page_range_empty_string() {
    let result = parse_page_range("");
    assert!(result.is_err(), "should fail on empty string");
}

// ============================================================
// 2. Settings 構造体のデシリアライズ
// ============================================================

#[test]
fn test_settings_full_yaml() {
    let yaml = r#"
dpi: 600
fg_dpi: 200
bg_quality: 80
fg_quality: 60
parallel_workers: 4
cache_dir: "/tmp/cache"
preserve_images: false
linearize: false
"#;
    let settings = Settings::from_yaml(yaml).expect("should parse full YAML");
    assert_eq!(settings.dpi, 600);
    assert_eq!(settings.fg_dpi, 200);
    assert_eq!(settings.bg_quality, 80);
    assert_eq!(settings.fg_quality, 60);
    assert_eq!(settings.parallel_workers, 4);
    assert_eq!(settings.cache_dir, Path::new("/tmp/cache"));
    assert!(!settings.preserve_images);
    assert!(!settings.linearize);
}

#[test]
fn test_settings_empty_yaml() {
    // 空YAML（"{}" はserde_ymlで空のマッピングを意味する）
    let settings = Settings::from_yaml("{}").expect("should use defaults for empty YAML");
    assert_eq!(settings.dpi, 300);
    assert_eq!(settings.fg_dpi, 100);
    assert_eq!(settings.bg_quality, 50);
    assert_eq!(settings.fg_quality, 30);
    assert_eq!(settings.parallel_workers, 0);
    assert_eq!(settings.cache_dir, Path::new(".cache"));
    assert!(settings.preserve_images);
    assert!(settings.linearize);
}

#[test]
fn test_settings_partial_yaml() {
    let yaml = r#"
dpi: 150
"#;
    let settings = Settings::from_yaml(yaml).expect("should fill missing with defaults");
    assert_eq!(settings.dpi, 150);
    // 残りはデフォルト値
    assert_eq!(settings.fg_dpi, 100);
    assert_eq!(settings.bg_quality, 50);
    assert_eq!(settings.fg_quality, 30);
    assert_eq!(settings.parallel_workers, 0);
    assert_eq!(settings.cache_dir, Path::new(".cache"));
    assert!(settings.preserve_images);
    assert!(settings.linearize);
}

// ============================================================
// 3. Job 構造体のデシリアライズ
// ============================================================

#[test]
fn test_job_required_fields_only() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse required fields");
    assert_eq!(job_file.jobs.len(), 1);
    let job = &job_file.jobs[0];
    assert_eq!(job.input, "input.pdf");
    assert_eq!(job.output, "output.pdf");
    assert!(job.dpi.is_none());
    assert!(job.fg_dpi.is_none());
    assert!(job.bg_quality.is_none());
    assert!(job.fg_quality.is_none());
    assert!(job.preserve_images.is_none());
    assert!(job.linearize.is_none());
}

#[test]
fn test_job_with_optional_fields() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    dpi: 600
    preserve_images: false
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse with optional fields");
    let job = &job_file.jobs[0];
    assert_eq!(job.dpi, Some(600));
    assert_eq!(job.preserve_images, Some(false));
}

#[test]
fn test_job_missing_required_field() {
    // inputが欠損
    let yaml = r#"
jobs:
  - output: "output.pdf"
"#;
    let result: Result<JobFile, _> = serde_yml::from_str(yaml);
    assert!(
        result.is_err(),
        "should fail when required field is missing"
    );
}

#[test]
fn test_job_multiple_jobs() {
    let yaml = r#"
jobs:
  - input: "a.pdf"
    output: "a_out.pdf"
  - input: "b.pdf"
    output: "b_out.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("should parse multiple jobs");
    assert_eq!(job_file.jobs.len(), 2);
    assert_eq!(job_file.jobs[0].input, "a.pdf");
    assert_eq!(job_file.jobs[1].input, "b.pdf");
}

// ============================================================
// 4. 設定マージロジック
// ============================================================

#[test]
fn test_merge_job_dpi_overrides_settings() {
    let settings = Settings::from_yaml("dpi: 300").expect("parse settings");
    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
    dpi: 600
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse job");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);
    assert_eq!(merged.dpi, 600, "job dpi should override settings dpi");
}

#[test]
fn test_merge_job_no_dpi_uses_settings() {
    let settings = Settings::from_yaml("dpi: 300").expect("parse settings");
    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse job");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);
    assert_eq!(merged.dpi, 300, "should fall back to settings dpi");
}

#[test]
fn test_merge_no_settings_uses_defaults() {
    let settings = Settings::default();
    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse job");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);
    assert_eq!(merged.dpi, 300);
    assert_eq!(merged.fg_dpi, 100);
    assert_eq!(merged.bg_quality, 50);
    assert_eq!(merged.fg_quality, 30);
    assert_eq!(merged.parallel_workers, 0);
    assert_eq!(merged.cache_dir, Path::new(".cache"));
    assert!(merged.preserve_images);
    assert!(merged.linearize);
}

// ============================================================
// 5. text_to_outlines フラグ
// ============================================================

#[test]
fn test_settings_text_to_outlines_default_false() {
    let settings = Settings::from_yaml("{}").expect("should use defaults");
    assert!(
        !settings.text_to_outlines,
        "text_to_outlines should default to false"
    );
}

#[test]
fn test_settings_text_to_outlines_explicit_true() {
    let yaml = "text_to_outlines: true";
    let settings = Settings::from_yaml(yaml).expect("should parse");
    assert!(settings.text_to_outlines);
}

#[test]
fn test_job_text_to_outlines_none_by_default() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("parse");
    assert!(job_file.jobs[0].text_to_outlines.is_none());
}

#[test]
fn test_job_text_to_outlines_explicit() {
    let yaml = r#"
jobs:
  - input: "input.pdf"
    output: "output.pdf"
    text_to_outlines: true
"#;
    let job_file: JobFile = serde_yml::from_str(yaml).expect("parse");
    assert_eq!(job_file.jobs[0].text_to_outlines, Some(true));
}

#[test]
fn test_merge_text_to_outlines_job_overrides_settings() {
    let settings = Settings::from_yaml("text_to_outlines: false").expect("parse");
    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
    text_to_outlines: true
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);
    assert!(merged.text_to_outlines, "job should override settings");
}

#[test]
fn test_merge_text_to_outlines_falls_back_to_settings() {
    let settings = Settings::from_yaml("text_to_outlines: true").expect("parse");
    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);
    assert!(merged.text_to_outlines, "should fall back to settings");
}

#[test]
fn test_merge_text_to_outlines_default() {
    let settings = Settings::default();
    let job_yaml = r#"
jobs:
  - input: "in.pdf"
    output: "out.pdf"
"#;
    let job_file: JobFile = serde_yml::from_str(job_yaml).expect("parse");
    let merged = MergedConfig::new(&settings, &job_file.jobs[0]);
    assert!(!merged.text_to_outlines, "should default to false");
}

// ============================================================
// 6. settings.yaml自動検出
// ============================================================

#[test]
fn test_auto_detect_settings_yaml_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let settings_path = dir.path().join("settings.yaml");
    let job_path = dir.path().join("jobs.yaml");

    let mut f = std::fs::File::create(&settings_path).expect("create settings.yaml");
    f.write_all(b"dpi: 450\n").expect("write settings");

    // ジョブファイルもダミーで作成（パスの解決に必要）
    std::fs::File::create(&job_path).expect("create jobs.yaml");

    let settings = load_settings_for_job(&job_path).expect("should load settings");
    assert_eq!(settings.dpi, 450);
}

#[test]
fn test_auto_detect_settings_yaml_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let job_path = dir.path().join("jobs.yaml");
    std::fs::File::create(&job_path).expect("create jobs.yaml");

    let settings = load_settings_for_job(&job_path).expect("should return defaults");
    assert_eq!(
        settings.dpi, 300,
        "should use default when settings.yaml absent"
    );
}
