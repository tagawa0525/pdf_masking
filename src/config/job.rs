use serde::Deserialize;
use serde::de::{self, SeqAccess, Visitor};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct JobFile {
    pub jobs: Vec<Job>,
}

/// カラーモード: MRC処理の種類を指定
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Rgb,
    Grayscale,
    Bw,
    Skip,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub input: String,
    pub output: String,
    pub color_mode: Option<ColorMode>,
    #[serde(default, deserialize_with = "deserialize_optional_pages")]
    pub bw_pages: Option<Vec<u32>>,
    #[serde(default, deserialize_with = "deserialize_optional_pages")]
    pub grayscale_pages: Option<Vec<u32>>,
    #[serde(default, deserialize_with = "deserialize_optional_pages")]
    pub rgb_pages: Option<Vec<u32>>,
    #[serde(default, deserialize_with = "deserialize_optional_pages")]
    pub skip_pages: Option<Vec<u32>>,
    pub dpi: Option<u32>,
    pub fg_dpi: Option<u32>,
    pub bg_quality: Option<u8>,
    pub fg_quality: Option<u8>,
    pub preserve_images: Option<bool>,
    pub linearize: Option<bool>,
    pub text_to_outlines: Option<bool>,
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

/// YAML配列の各要素（文字列または整数）を表す中間型。
///
/// 整数はそのままページ番号に、文字列は `parse_page_range` でパースされる。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PageElement {
    Integer(u32),
    Range(String),
}

/// serdeのdeserialize_withで使用するページ範囲デシリアライザ。
///
/// 以下の両形式を受け付ける:
/// - 文字列形式: `pages: "1, 3, 5-10"`
/// - YAML配列形式: `pages: [1, 3, "5-10", 15]`
fn deserialize_pages<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PagesVisitor;

    impl<'de> Visitor<'de> for PagesVisitor {
        type Value = Vec<u32>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a page range string (e.g. \"1, 3, 5-10\") or a YAML sequence (e.g. [1, 3, \"5-10\"])")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<u32>, E>
        where
            E: de::Error,
        {
            parse_page_range(value).map_err(de::Error::custom)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<u32>, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut pages = Vec::new();
            while let Some(elem) = seq.next_element::<PageElement>()? {
                match elem {
                    PageElement::Integer(n) => pages.push(n),
                    PageElement::Range(s) => {
                        let parsed = parse_page_range(&s).map_err(de::Error::custom)?;
                        pages.extend(parsed);
                    }
                }
            }

            if pages.is_empty() {
                return Err(de::Error::custom("Page sequence cannot be empty"));
            }

            pages.sort();
            pages.dedup();
            Ok(pages)
        }
    }

    deserializer.deserialize_any(PagesVisitor)
}

/// Optional<Vec<u32>>用のデシリアライザ（Noneを許容）
fn deserialize_optional_pages<'de, D>(deserializer: D) -> Result<Option<Vec<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptionalPagesVisitor;

    impl<'de> Visitor<'de> for OptionalPagesVisitor {
        type Value = Option<Vec<u32>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an optional page range string or YAML sequence")
        }

        fn visit_none<E>(self) -> Result<Option<Vec<u32>>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Option<Vec<u32>>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserialize_pages(deserializer).map(Some)
        }

        fn visit_unit<E>(self) -> Result<Option<Vec<u32>>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_option(OptionalPagesVisitor)
}

impl Job {
    /// ページ→カラーモードのオーバーライドマップを構築する。
    ///
    /// - bw_pages, grayscale_pages, rgb_pages, skip_pages からオーバーライドを収集
    /// - 同一ページが複数リストに含まれる場合はエラー
    /// - リストに含まれないページは HashMap に含まれない
    ///   （= デフォルトカラーモードは呼び出し側で決定・適用する）
    ///
    /// # Returns
    /// ページ番号(1-based) → ColorMode のマップ。
    /// マップに含まれないページには、呼び出し側で用意したデフォルトモードを使うこと。
    pub fn resolve_page_modes(&self) -> crate::error::Result<HashMap<u32, ColorMode>> {
        let mut page_to_mode: HashMap<u32, ColorMode> = HashMap::new();

        // 各 *_pages リストを処理
        let lists = [
            (ColorMode::Bw, self.bw_pages.as_deref()),
            (ColorMode::Grayscale, self.grayscale_pages.as_deref()),
            (ColorMode::Rgb, self.rgb_pages.as_deref()),
            (ColorMode::Skip, self.skip_pages.as_deref()),
        ];

        for (mode, pages_opt) in lists {
            if let Some(pages) = pages_opt {
                for &page in pages {
                    if let Some(existing_mode) = page_to_mode.insert(page, mode) {
                        return Err(crate::error::PdfMaskError::config(format!(
                            "Page {} specified in multiple mode lists: {:?} and {:?}",
                            page, existing_mode, mode
                        )));
                    }
                }
            }
        }

        Ok(page_to_mode)
    }
}
