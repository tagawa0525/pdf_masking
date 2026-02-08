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
/// PHASE 7+ TODO: Placeholder implementation. Actual JPEG decode ->
/// white fill -> re-encode logic will be implemented in Phase 7+.
/// Currently only used in tests.
#[allow(dead_code)]
pub(crate) fn redact_region_placeholder() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_region_placeholder() {
        assert!(
            redact_region_placeholder(),
            "placeholder should return true"
        );
    }
}
