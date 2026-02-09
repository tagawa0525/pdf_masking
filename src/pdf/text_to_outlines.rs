use std::collections::HashMap;

use crate::pdf::font::ParsedFont;

/// BT...ETブロックをベクターパスに変換したコンテンツストリームを返す。
///
/// フォントが見つからない場合はErrを返し、呼び出し元でpdfiumフォールバックに切り替える。
pub fn convert_text_to_outlines(
    _content_bytes: &[u8],
    _fonts: &HashMap<String, ParsedFont>,
) -> crate::error::Result<Vec<u8>> {
    todo!("PR 5: implement text-to-outlines conversion")
}
