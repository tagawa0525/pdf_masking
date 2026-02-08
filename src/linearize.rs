// Phase 11: PDF linearization via qpdf CLI wrapper

use std::path::Path;
use std::process::Command;

/// Linearize a PDF file using qpdf.
///
/// Creates a linearized copy of the input PDF at the output path.
/// If input_path == output_path, uses qpdf's --replace-input mode.
///
/// qpdf must be available in PATH (provided by nix develop environment).
pub fn linearize(input_path: &Path, output_path: &Path) -> crate::error::Result<()> {
    let output = if input_path == output_path {
        // In-place mode: qpdf --linearize --replace-input <path>
        Command::new("qpdf")
            .arg("--linearize")
            .arg("--replace-input")
            .arg(input_path)
            .output()
    } else {
        // Separate output: qpdf --linearize <input> <output>
        Command::new("qpdf")
            .arg("--linearize")
            .arg(input_path)
            .arg(output_path)
            .output()
    };

    match output {
        Ok(result) => {
            if result.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&result.stderr);
                Err(crate::error::PdfMaskError::linearize(format!(
                    "qpdf failed (exit code {}): {}",
                    result
                        .status
                        .code()
                        .map_or_else(|| "unknown".to_string(), |c| c.to_string()),
                    stderr.trim()
                )))
            }
        }
        Err(e) => Err(crate::error::PdfMaskError::linearize(format!(
            "failed to execute qpdf: {e}"
        ))),
    }
}

/// Linearize a PDF in-place (replaces the original file).
pub fn linearize_in_place(path: &Path) -> crate::error::Result<()> {
    linearize(path, path)
}
