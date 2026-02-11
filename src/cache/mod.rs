pub mod hash;
pub mod store;

use crate::config::job::ColorMode;

/// ColorMode を文字列に変換する。
pub(crate) fn color_mode_to_str(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Rgb => "rgb",
        ColorMode::Grayscale => "grayscale",
        ColorMode::Bw => "bw",
        ColorMode::Skip => "skip",
    }
}

/// 文字列を ColorMode に変換する。
pub(crate) fn str_to_color_mode(s: &str) -> Option<ColorMode> {
    match s {
        "rgb" => Some(ColorMode::Rgb),
        "grayscale" => Some(ColorMode::Grayscale),
        "bw" => Some(ColorMode::Bw),
        "skip" => Some(ColorMode::Skip),
        _ => None,
    }
}
