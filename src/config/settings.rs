use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dpi: u32,
    pub fg_dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub parallel_workers: usize,
    pub cache_dir: PathBuf,
    pub preserve_images: bool,
    pub linearize: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dpi: 300,
            fg_dpi: 100,
            bg_quality: 50,
            fg_quality: 30,
            parallel_workers: 0,
            cache_dir: PathBuf::from(".cache"),
            preserve_images: true,
            linearize: true,
        }
    }
}

impl Settings {
    pub fn from_yaml(yaml: &str) -> crate::error::Result<Self> {
        serde_yml::from_str(yaml).map_err(|e| {
            crate::error::PdfMaskError::config(format!("Failed to parse settings YAML: {e}"))
        })
    }

    pub fn from_file(path: &Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }
}
