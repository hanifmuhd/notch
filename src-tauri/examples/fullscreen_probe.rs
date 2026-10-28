// Spike S2/S3: prints what Win32 reports about the foreground window and monitors.
// Run: cargo run --example fullscreen_probe [-- --seconds N]   (default: until Ctrl+C)
#[cfg(not(windows))]
fn main() {
    eprintln!("fullscreen_probe only runs on Windows");
}

#[cfg(windows)]
fn main() {
    use std::{thread::sleep, time::{Duration, Instant}};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    use windows::Win32::UI::WindowsAndMessaging::*;

    fn name(raw: &[u16]) -> String {
        String::from_utf16_lossy(&raw[..raw.iter().position(|&c| c == 0).unwrap_or(raw.len())])
    }
    fn rect(r: &RECT) -> String {
        format!("({},{})-({},{}) {}x{}", r.left, r.top, r.right, r.bottom, r.right - r.left, r.bottom - r.top)
    }
    fn info(m: HMONITOR) -> MONITORINFOEXW {
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        unsafe { let _ = GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO); }
        mi
    }
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, _: LPARAM) -> BOOL {
        let mi = info(m);
        let primary = mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0;
        println!("monitor {} rect={} work={} primary={}", name(&mi.szDevice), rect(&mi.monitorInfo.rcMonitor), rect(&mi.monitorInfo.rcWork), primary);
        BOOL(1)
    }

    let args: Vec<String> = std::env::args().collect();
    let limit = args.iter().position(|a| a == "--seconds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u64>().ok());
    let start = Instant::now();

    unsafe {
        println!("-- monitors (EnumDisplayMonitors) --");
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(0));
        println!("-- foreground, once per second --");
        while limit.map_or(true, |s| start.elapsed() < Duration::from_secs(s)) {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                println!("no foreground window");
            } else {
                let mut cls = [0u16; 256];
                let n = GetClassNameW(hwnd, &mut cls).max(0) as usize;
                let mut wr = RECT::default();
                let _ = GetWindowRect(hwnd, &mut wr);
                let mi = info(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST));
                let m = mi.monitorInfo.rcMonitor;
                // covers = window rect contains the whole monitor rect (window may overhang)
                let covers = wr.left <= m.left && wr.top <= m.top && wr.right >= m.right && wr.bottom >= m.bottom;
                let c = name(&cls[..n]);
                let excluded = ["Progman", "WorkerW", "Shell_TrayWnd"].contains(&c.as_str());
                let quns = SHQueryUserNotificationState().map(|s| s.0.to_string()).unwrap_or_else(|e| format!("err {e}"));
                println!("class={} rect={} monitor={} {} covers={} excluded={} fullscreen={} quns={}", c, rect(&wr), name(&mi.szDevice), rect(&m), covers, excluded, covers && !excluded, quns);
            }
            sleep(Duration::from_secs(1));
        }
    }
}
