// Phase 7: 画像XObjectのデコード/再エンコード、重なり検出・塗りつぶし

use crate::pdf::content_stream::BBox;

/// 2つのBBoxの重なりを判定する。
///
/// 辺が接しているだけの場合は重ならないと判定する（strict inequality）。
pub fn bbox_overlaps(a: &BBox, b: &BBox) -> bool {
    !(a.x_max <= b.x_min || b.x_max <= a.x_min || a.y_max <= b.y_min || b.y_max <= a.y_min)
}

/// 画像データの指定領域を白で塗りつぶす（重なり領域のredaction）。
///
/// v1ではplaceholder実装。実際のJPEGデコード→白塗り→再エンコードは
/// Phase 7+で実装する。
pub fn redact_region_placeholder() -> bool {
    true
}
