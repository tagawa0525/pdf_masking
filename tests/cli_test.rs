// Phase 12: CLI entry point tests (RED)

use std::process::Command;

fn cargo_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdf_masking"))
}

// ============================================================
// 1. No arguments shows usage and exits with failure
// ============================================================

#[test]
fn test_main_no_args_shows_usage() {
    let output = cargo_bin().output().expect("failed to execute binary");

    assert!(
        !output.status.success(),
        "should exit with failure when no args given"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage"),
        "stderr should contain 'Usage', got: {stderr}"
    );
}

// ============================================================
// 2. --help flag shows usage and exits with success
// ============================================================

#[test]
fn test_main_help_flag() {
    let output = cargo_bin()
        .arg("--help")
        .output()
        .expect("failed to execute binary");

    assert!(
        output.status.success(),
        "should exit with success for --help"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage"),
        "stderr should contain 'Usage', got: {stderr}"
    );
}

// ============================================================
// 3. --version flag shows version and exits with success
// ============================================================

#[test]
fn test_main_version_flag() {
    let output = cargo_bin()
        .arg("--version")
        .output()
        .expect("failed to execute binary");

    assert!(
        output.status.success(),
        "should exit with success for --version"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        stderr.contains(version),
        "stderr should contain version '{version}', got: {stderr}"
    );
}

// ============================================================
// 4. Nonexistent job file produces error
// ============================================================

#[test]
fn test_main_nonexistent_job_file() {
    let unique_path = std::env::temp_dir().join(format!(
        "nonexistent_job_file_{}.yaml",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock error")
            .as_nanos()
    ));
    let output = cargo_bin()
        .arg(unique_path.as_os_str())
        .output()
        .expect("failed to execute binary");

    assert!(
        !output.status.success(),
        "should exit with failure for nonexistent file"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERROR") || stderr.contains("error") || stderr.contains("Error"),
        "stderr should contain error message, got: {stderr}"
    );
}
