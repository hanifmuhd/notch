//! Hide the notch while a full-screen app covers its screen (spec 5.6). The decisions are pure
//! and tested; the Win32 reading of the foreground window lives in `win`.

use crate::placement::Rect;

/// Shell windows that cover a whole monitor without being a full-screen app.
pub const IGNORED_CLASSES: [&str; 3] = ["Progman", "WorkerW", "Shell_TrayWnd"];

/// True when `window` contains the whole `monitor`; borderless windows often overhang a few px.
pub fn covers_monitor(window: Rect, monitor: Rect) -> bool {
    window.x <= monitor.x
        && window.y <= monitor.y
        && window.x + window.w >= monitor.x + monitor.w
        && window.y + window.h >= monitor.y + monitor.h
}

/// A maximised captioned window overhangs by 8 px and, with an auto-hide taskbar, covers the
/// monitor too (R13). F11 browsers and borderless games drop the caption.
pub fn is_fullscreen(covers: bool, zoomed: bool, has_caption: bool) -> bool {
    covers && !(zoomed && has_caption)
}

pub fn should_show(show_notch: bool, fullscreen: bool) -> bool {
    show_notch && !fullscreen
}

#[cfg(windows)]
pub use win::{foreground_fullscreen_on, quns_d3d_fullscreen};
#[cfg(all(windows, debug_assertions))]
pub use win::foreground_class;

#[cfg(windows)]
mod win {
    use super::{covers_monitor, is_fullscreen, IGNORED_CLASSES};
    use crate::placement::Rect;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsZoomed, GWL_STYLE, WS_CAPTION,
    };

    fn name(raw: &[u16]) -> String {
        String::from_utf16_lossy(&raw[..raw.iter().position(|&c| c == 0).unwrap_or(raw.len())])
    }

    fn class_of(hwnd: HWND) -> String {
        let mut cls = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd, &mut cls) }.max(0) as usize;
        name(&cls[..n])
    }

    fn rect(r: RECT) -> Rect {
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }

    /// Class of the foreground window, for the debug log.
    #[cfg(debug_assertions)]
    pub fn foreground_class() -> String {
        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() { String::new() } else { class_of(fg) }
    }

    /// True when the foreground window (not the notch, not a shell window) is a full-screen
    /// window on the monitor named `monitor_device` (GDI name, as Tauri reports it).
    pub fn foreground_fullscreen_on(monitor_device: &str, own_hwnd: isize) -> bool {
        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() || fg.0 as isize == own_hwnd || IGNORED_CLASSES.contains(&class_of(fg).as_str()) {
            return false;
        }
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let mut wr = RECT::default();
        unsafe {
            let m = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            if !GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO).as_bool() || GetWindowRect(fg, &mut wr).is_err() {
                return false;
            }
        }
        if name(&mi.szDevice) != monitor_device {
            return false;
        }
        let covers = covers_monitor(rect(wr), rect(mi.monitorInfo.rcMonitor));
        let caption = WS_CAPTION.0 as isize;
        let has_caption = unsafe { GetWindowLongPtrW(fg, GWL_STYLE) } & caption == caption;
        is_fullscreen(covers, unsafe { IsZoomed(fg) }.as_bool(), has_caption)
    }

    /// QUNS_RUNNING_D3D_FULL_SCREEN; only a hint, used on single-monitor setups (spec 5.6).
    pub fn quns_d3d_fullscreen() -> bool {
        unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s.0 == 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M0: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

    #[test]
    fn exact_cover_true() {
        assert!(covers_monitor(M0, M0));
    }

    #[test]
    fn bigger_than_monitor_true() {
        assert!(covers_monitor(Rect { x: -8, y: -8, w: 1936, h: 1096 }, M0));
    }

    #[test]
    fn maximised_with_taskbar_false() {
        assert!(!covers_monitor(Rect { x: -8, y: -8, w: 1936, h: 1080 - 48 + 8 }, M0));
        assert!(!covers_monitor(Rect { x: 0, y: 0, w: 1920, h: 1080 - 48 }, M0));
    }

    #[test]
    fn other_monitor_false() {
        assert!(!covers_monitor(Rect { x: 1920, y: 0, w: 1920, h: 1080 }, M0));
    }

    #[test]
    fn maximised_captioned_not_fullscreen() {
        assert!(!is_fullscreen(true, true, true));
        assert!(is_fullscreen(true, true, false));
        assert!(is_fullscreen(true, false, true));
        assert!(!is_fullscreen(false, false, false));
    }

    #[test]
    fn should_show_needs_setting_and_no_fullscreen() {
        assert!(should_show(true, false));
        assert!(!should_show(true, true));
        assert!(!should_show(false, false));
        assert!(!should_show(false, true));
    }
}
