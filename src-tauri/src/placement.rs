//! Where the notch sits on a screen (spec 5.7): pure geometry, wired up in main.rs.

use crate::settings::Position;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Gap from the left edge and from the right edge (room for the caption buttons), logical px.
const LEFT_GAP: f64 = 8.0;
const RIGHT_GAP: f64 = 140.0;

/// Top-left physical origin of a notch `width` physical px wide, hanging from the monitor's top edge.
pub fn notch_origin(monitor: Rect, scale: f64, pos: Position, width: i32) -> (i32, i32) {
    let x = match pos {
        Position::Centre => monitor.x + (monitor.w - width) / 2,
        Position::Left => monitor.x + (LEFT_GAP * scale).round() as i32,
        Position::Right => monitor.x + monitor.w - (RIGHT_GAP * scale).round() as i32 - width,
    };
    (x, monitor.y)
}

/// The wanted screen by id, else the primary one, else the first. `screens` must not be empty.
pub fn pick_monitor<'a>(screens: &'a [(String, Rect, bool)], wanted: Option<&str>) -> &'a (String, Rect, bool) {
    wanted
        .and_then(|w| screens.iter().find(|s| s.0 == w))
        .or_else(|| screens.iter().find(|s| s.2))
        .unwrap_or(&screens[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    const M0: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

    #[test]
    fn centre_on_1920() {
        assert_eq!(notch_origin(M0, 1.0, Position::Centre, 220), (850, 0));
    }

    #[test]
    fn left_gap_scaled() {
        assert_eq!(notch_origin(M0, 1.5, Position::Left, 220).0, 12);
        assert_eq!(notch_origin(M0, 1.0, Position::Left, 220).0, 8);
    }

    #[test]
    fn right_gap_scaled() {
        assert_eq!(notch_origin(M0, 1.25, Position::Right, 220), (1525, 0));
    }

    #[test]
    fn second_monitor_offset() {
        let m1 = Rect { x: 1920, y: 0, w: 1920, h: 1080 };
        assert_eq!(notch_origin(m1, 1.0, Position::Centre, 220), (2770, 0));
        assert_eq!(notch_origin(m1, 1.0, Position::Left, 220).0, 1928);
    }

    #[test]
    fn monitor_top_is_kept() {
        let m = Rect { x: 0, y: -1080, w: 1920, h: 1080 };
        assert_eq!(notch_origin(m, 1.0, Position::Centre, 220).1, -1080);
    }

    fn two() -> Vec<(String, Rect, bool)> {
        vec![
            ("A".into(), M0, true),
            ("B".into(), Rect { x: 1920, y: 0, w: 1280, h: 720 }, false),
        ]
    }

    #[test]
    fn wanted_screen_is_used() {
        assert_eq!(pick_monitor(&two(), Some("B")).0, "B");
    }

    #[test]
    fn missing_screen_falls_back_to_primary() {
        let mut s = two();
        s.reverse();
        assert_eq!(pick_monitor(&s, Some("gone")).0, "A");
        assert_eq!(pick_monitor(&s, None).0, "A");
    }

    #[test]
    fn no_primary_flag_falls_back_to_first() {
        let mut s = two();
        s[0].2 = false;
        assert_eq!(pick_monitor(&s, None).0, "A");
    }
}
