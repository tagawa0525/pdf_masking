pub mod job;
pub mod merged;
pub mod settings;

use settings::Settings;
use std::path::Path;

/// ジョブファイルのパスからsettings.yamlを自動検出して読み込む。
///
/// ジョブファイルと同じディレクトリに `settings.yaml` が存在すれば読み込み、
/// 存在しなければデフォルト設定を返す。
pub fn load_settings_for_job(job_file_path: &Path) -> crate::error::Result<Settings> {
    let dir = job_file_path
        .parent()
        .ok_or_else(|| crate::error::PdfMaskError::config("Cannot determine job file directory"))?;

    let settings_path = dir.join("settings.yaml");

    if settings_path.exists() {
        Settings::from_file(&settings_path)
    } else {
        Ok(Settings::default())
    }
}
