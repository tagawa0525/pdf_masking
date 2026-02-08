use std::path::PathBuf;

use super::job::{ColorMode, Job};
use super::settings::Settings;

#[derive(Debug, Clone)]
pub struct MergedConfig {
    pub color_mode: ColorMode,
    pub dpi: u32,
    pub fg_dpi: u32,
    pub bg_quality: u8,
    pub fg_quality: u8,
    pub parallel_workers: usize,
    pub cache_dir: PathBuf,
    pub preserve_images: bool,
    pub linearize: bool,
}

impl MergedConfig {
    /// JobのOption値がSomeならJobの値を、NoneならSettingsの値を使用する。
    pub fn new(settings: &Settings, job: &Job) -> Self {
        MergedConfig {
            color_mode: job.color_mode.unwrap_or(settings.color_mode),
            dpi: job.dpi.unwrap_or(settings.dpi),
            fg_dpi: job.fg_dpi.unwrap_or(settings.fg_dpi),
            bg_quality: job.bg_quality.unwrap_or(settings.bg_quality),
            fg_quality: job.fg_quality.unwrap_or(settings.fg_quality),
            parallel_workers: settings.parallel_workers,
            cache_dir: settings.cache_dir.clone(),
            preserve_images: job.preserve_images.unwrap_or(settings.preserve_images),
            linearize: job.linearize.unwrap_or(settings.linearize),
        }
    }
}
