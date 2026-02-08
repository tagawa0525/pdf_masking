use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pdf_masking::config::job::JobFile;
use pdf_masking::config::merged::MergedConfig;
use pdf_masking::config::{self};
use pdf_masking::linearize;
use pdf_masking::pipeline::job_runner::JobConfig;
use pdf_masking::pipeline::orchestrator::run_all_jobs;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: pdf_masking <jobs.yaml>...");
        eprintln!("  Process PDF files according to job specifications.");
        return if args.is_empty() {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        };
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        eprintln!("pdf_masking {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    // Collect job configs and their linearize flags from all job files.
    let mut job_configs: Vec<JobConfig> = Vec::new();
    let mut linearize_flags: Vec<bool> = Vec::new();

    for job_file_arg in &args {
        let job_file_path = Path::new(job_file_arg);

        // Load settings from the same directory as the job file.
        let settings = match config::load_settings_for_job(job_file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ERROR: Failed to load settings for {job_file_arg}: {e}");
                return ExitCode::FAILURE;
            }
        };

        // Read and parse the job YAML file.
        let yaml_content = match std::fs::read_to_string(job_file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ERROR: Failed to read job file {job_file_arg}: {e}");
                return ExitCode::FAILURE;
            }
        };

        let job_file: JobFile = match serde_yml::from_str(&yaml_content) {
            Ok(jf) => jf,
            Err(e) => {
                eprintln!("ERROR: Failed to parse job file {job_file_arg}: {e}");
                return ExitCode::FAILURE;
            }
        };

        // Resolve job file directory for relative paths.
        let job_dir = job_file_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        // Merge settings with each job and construct JobConfig.
        for job in &job_file.jobs {
            let merged = MergedConfig::new(&settings, job);

            let input_path = resolve_path(&job_dir, &job.input);
            let output_path = resolve_path(&job_dir, &job.output);

            // Resolve per-page color mode overrides (1-based)
            let default_color_mode = merged.color_mode;
            let color_mode_overrides = match job.resolve_page_modes() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("ERROR: {e}");
                    return ExitCode::FAILURE;
                }
            };

            linearize_flags.push(merged.linearize);

            job_configs.push(JobConfig {
                input_path,
                output_path,
                default_color_mode,
                color_mode_overrides,
                dpi: merged.dpi,
                bg_quality: merged.bg_quality,
                fg_quality: merged.fg_quality,
                preserve_images: merged.preserve_images,
                cache_dir: Some(merged.cache_dir),
                text_to_outlines: merged.text_to_outlines,
            });
        }
    }

    // Run all jobs through the pipeline.
    let results = run_all_jobs(&job_configs);

    // Report results and optionally linearize.
    let mut has_error = false;
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(job_result) => {
                eprintln!(
                    "OK: {} -> {} ({} pages)",
                    job_result.input_path.display(),
                    job_result.output_path.display(),
                    job_result.pages_processed
                );

                // Linearize output if configured.
                if linearize_flags[i]
                    && let Err(e) = linearize::linearize_in_place(&job_result.output_path)
                {
                    eprintln!(
                        "ERROR: Failed to linearize {}: {e}",
                        job_result.output_path.display()
                    );
                    has_error = true;
                }
            }
            Err(e) => {
                eprintln!(
                    "ERROR: {} -> {}: {e}",
                    job_configs[i].input_path.display(),
                    job_configs[i].output_path.display()
                );
                has_error = true;
            }
        }
    }

    if has_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Resolve a potentially relative path against a base directory.
/// If the path is already absolute, return it as-is.
fn resolve_path(base_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    }
}
