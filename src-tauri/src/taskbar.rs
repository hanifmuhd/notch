//! Taskbar mode (redesign spec 5.5): where the capsule can sit in the taskbar, and whether it
//! fits there at full size, as rings only, or not at all (then it shows at the top).

/// How the notch shows when "Show in: Taskbar" is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    Full,
    Mini,
    Top,
}

/// `gap` is the free taskbar space between the last app button and the tray; widths and the
/// margin kept on each side are in the same (physical) px.
pub fn fit(gap: i32, full_w: i32, mini_w: i32, margin: i32) -> Fit {
    if gap >= full_w + 2 * margin {
        Fit::Full
    } else if gap >= mini_w + 2 * margin {
        Fit::Mini
    } else {
        Fit::Top
    }
}

/// The main taskbar as Windows reports it, in physical screen px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Taskbar {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    /// Right edge of the last app button (or of Start/Search when no app is pinned).
    pub apps_right: i32,
    /// Left edge of the tray (hidden icons, network, clock...).
    pub tray_left: i32,
    pub auto_hide: bool,
}

impl Taskbar {
    /// A horizontal taskbar along the bottom of the screen is the only kind taskbar mode uses.
    pub fn usable(&self) -> bool {
        !self.auto_hide && self.right - self.left > self.bottom - self.top && self.top > 0
    }
}

/// Reads the main taskbar with Win32 and UI Automation (spike S2, docs/spikes/taskbar-gap.md).
/// None when anything is missing (e.g. Windows 10 layouts or Explorer restarting).
#[cfg(windows)]
pub fn measure() -> Option<Taskbar> {
    use windows::core::w;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, TreeScope_Descendants};
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABS_AUTOHIDE, APPBARDATA};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, GetWindowRect};
    unsafe {
        let tb = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
        let mut r = RECT::default();
        GetWindowRect(tb, &mut r).ok()?;
        let tray = FindWindowExW(Some(tb), None, w!("TrayNotifyWnd"), None).ok()?;
        let mut tr = RECT::default();
        GetWindowRect(tray, &mut tr).ok()?;
        let mut abd = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
        let auto_hide = (SHAppBarMessage(ABM_GETSTATE, &mut abd) as u32) & ABS_AUTOHIDE != 0;

        // Safe to call on every poll: a second init on the same thread just returns S_FALSE.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let root = uia.ElementFromHandle(tb).ok()?;
        let all = root.FindAll(TreeScope_Descendants, &uia.CreateTrueCondition().ok()?).ok()?;
        let mut apps_right = r.left;
        for i in 0..all.Length().ok()? {
            let Ok(e) = all.GetElement(i) else { continue };
            let (Ok(class), Ok(id), Ok(b)) = (e.CurrentClassName(), e.CurrentAutomationId(), e.CurrentBoundingRectangle()) else { continue };
            let (class, id) = (class.to_string(), id.to_string());
            let left_item = class == "Taskbar.TaskListButtonAutomationPeer" || id == "StartButton" || id == "SearchButton";
            if left_item && b.right <= tr.left {
                apps_right = apps_right.max(b.right);
            }
        }
        Some(Taskbar { left: r.left, top: r.top, right: r.right, bottom: r.bottom, apps_right, tray_left: tr.left, auto_hide })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_decision() {
        assert_eq!(fit(804, 150, 44, 8), Fit::Full);
        assert_eq!(fit(166, 150, 44, 8), Fit::Full); // exactly full + 2 margins
        assert_eq!(fit(165, 150, 44, 8), Fit::Mini);
        assert_eq!(fit(60, 150, 44, 8), Fit::Mini); // exactly mini + 2 margins
        assert_eq!(fit(59, 150, 44, 8), Fit::Top);
        assert_eq!(fit(0, 150, 44, 8), Fit::Top);
        assert_eq!(fit(-40, 150, 44, 8), Fit::Top); // icons already past the tray
    }

    #[test]
    fn only_a_bottom_horizontal_visible_taskbar_is_usable() {
        let t = Taskbar { left: 0, top: 1032, right: 1920, bottom: 1080, apps_right: 865, tray_left: 1669, auto_hide: false };
        assert!(t.usable());
        assert!(!Taskbar { auto_hide: true, ..t }.usable());
        assert!(!Taskbar { top: 0, bottom: 48, ..t }.usable()); // at the top
        assert!(!Taskbar { right: 48, bottom: 1080, top: 0, ..t }.usable()); // vertical
    }

    #[cfg(windows)]
    #[test]
    #[ignore] // reads the real taskbar: cargo test print_taskbar -- --ignored --nocapture
    fn print_taskbar() {
        println!("{:?}", measure());
    }
}
