//! Which parts of the fixed-size notch window take the mouse (spec 4, approach A). The webview
//! reports the rectangles of the visible shape; everywhere else clicks pass to the app below.

use serde::Deserialize;

/// A rectangle in CSS px, relative to the window's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// True when the point lies in any rectangle (left/top edges inside, right/bottom outside).
pub fn hit(rects: &[HitRect], px: f64, py: f64) -> bool {
    rects.iter().any(|r| px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_rects() {
        let rects = [HitRect { x: 100.0, y: 0.0, w: 200.0, h: 32.0 }, HitRect { x: 320.0, y: 4.0, w: 120.0, h: 26.0 }];
        assert!(hit(&rects, 100.0, 0.0));
        assert!(hit(&rects, 299.5, 31.5));
        assert!(hit(&rects, 400.0, 10.0));
        assert!(!hit(&rects, 99.5, 10.0)); // just left
        assert!(!hit(&rects, 300.0, 10.0)); // right edge is outside
        assert!(!hit(&rects, 310.0, 10.0)); // gap between
        assert!(!hit(&rects, 200.0, 32.0)); // just below
        assert!(!hit(&rects, -5.0, -5.0));
        assert!(!hit(&[], 150.0, 10.0));
    }
}
