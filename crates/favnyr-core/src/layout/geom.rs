use super::*;

/// Cuts `area` in two along `dir`, reserving `gap` for the separator between
/// the halves. Returns the first child's rectangle, the separator's, and the
/// second child's.
///
/// Shared by the flattening and by the equalizer: both need to know where a
/// child actually lands, and two copies of this arithmetic would eventually
/// disagree.
pub(super) fn split_area(area: Rect, dir: SplitDir, ratio: f32, gap: f32) -> (Rect, Rect, Rect) {
    let r = ratio.clamp(0.0, 1.0);
    match dir {
        SplitDir::Row => {
            let avail = (area.w - gap).max(0.0);
            let w1 = avail * r;
            (
                Rect { w: w1, ..area },
                Rect {
                    x: area.x + w1,
                    y: area.y,
                    w: gap,
                    h: area.h,
                },
                Rect {
                    x: area.x + w1 + gap,
                    w: avail - w1,
                    ..area
                },
            )
        }
        SplitDir::Column => {
            let avail = (area.h - gap).max(0.0);
            let h1 = avail * r;
            (
                Rect { h: h1, ..area },
                Rect {
                    x: area.x,
                    y: area.y + h1,
                    w: area.w,
                    h: gap,
                },
                Rect {
                    y: area.y + h1 + gap,
                    h: avail - h1,
                    ..area
                },
            )
        }
    }
}
