use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct JobFile {
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub input: String,
    pub output: String,
    #[serde(deserialize_with = "deserialize_pages")]
    pub pages: Vec<u32>,
    pub dpi: Option<u32>,
    pub fg_dpi: Option<u32>,
    pub bg_quality: Option<u8>,
    pub fg_quality: Option<u8>,
    pub preserve_images: Option<bool>,
    pub linearize: Option<bool>,
}

/// ページ範囲文字列をパースしてページ番号のベクタに変換する。
///
/// 形式:
/// - 単一ページ: `"5"`
/// - 範囲: `"5-10"` (5, 6, 7, 8, 9, 10)
/// - 混合（カンマ区切り）: `"1, 3, 5-10, 15"`
///
/// 結果はソート済み・重複なし。
pub fn parse_page_range(s: &str) -> crate::error::Result<Vec<u32>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(crate::error::PdfMaskError::config(
            "Page range cannot be empty",
        ));
    }

    let mut pages = Vec::new();

    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some((start_str, end_str)) = part.split_once('-') {
            let start: u32 = start_str.trim().parse().map_err(|_| {
                crate::error::PdfMaskError::config(format!(
                    "Invalid page number in range: '{start_str}'"
                ))
            })?;
            let end: u32 = end_str.trim().parse().map_err(|_| {
                crate::error::PdfMaskError::config(format!(
                    "Invalid page number in range: '{end_str}'"
                ))
            })?;

            if start > end {
                return Err(crate::error::PdfMaskError::config(format!(
                    "Invalid page range: start ({start}) > end ({end})"
                )));
            }

            for page in start..=end {
                pages.push(page);
            }
        } else {
            let page: u32 = part.parse().map_err(|_| {
                crate::error::PdfMaskError::config(format!("Invalid page number: '{part}'"))
            })?;
            pages.push(page);
        }
    }

    if pages.is_empty() {
        return Err(crate::error::PdfMaskError::config(
            "Page range resolved to empty set",
        ));
    }

    pages.sort();
    pages.dedup();
    Ok(pages)
}

/// serdeのdeserialize_withで使用するページ範囲デシリアライザ
fn deserialize_pages<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse_page_range(&s).map_err(serde::de::Error::custom)
}
