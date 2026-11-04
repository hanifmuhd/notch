//! Real monitor names for the "Show on" picker. The saved screen id stays the GDI device name.

use std::collections::HashMap;

/// Picker labels in screen order: the monitor's name, or "Screen N" when it has none; a name seen
/// again gets " 2", " 3"; the primary screen gets " (main)".
pub fn labels(names: &[Option<String>], primary: &[bool]) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    names
        .iter()
        .zip(primary)
        .enumerate()
        .map(|(i, (name, &main))| {
            let mut label = match name.as_deref().filter(|n| !n.is_empty()) {
                Some(n) => {
                    let c = seen.entry(n).or_insert(0);
                    *c += 1;
                    if *c == 1 { n.to_string() } else { format!("{n} {c}") }
                }
                None => format!("Screen {}", i + 1),
            };
            if main {
                label.push_str(" (main)");
            }
            label
        })
        .collect()
}

/// Active monitors' friendly names keyed by GDI device name (`\\.\DISPLAY1`); empty on any failure.
#[cfg(windows)]
pub fn friendly_names() -> HashMap<String, String> {
    use windows::Win32::Devices::Display::*;

    fn text(w: &[u16]) -> String {
        String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
    }
    let mut out = HashMap::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).is_err() {
            return out;
        }
        for p in &paths[..np as usize] {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of_val(&src) as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of_val(&tgt) as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 || DisplayConfigGetDeviceInfo(&mut tgt.header) != 0 {
                continue;
            }
            let name = text(&tgt.monitorFriendlyDeviceName);
            if !name.is_empty() {
                out.insert(text(&src.viewGdiDeviceName), name);
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn friendly_names() -> HashMap<String, String> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_fallback_and_dedupe() {
        let l = labels(
            &[Some("DELL U2720Q".into()), None, Some("DELL U2720Q".into()), Some("".into())],
            &[true, false, false, false],
        );
        assert_eq!(l, ["DELL U2720Q (main)", "Screen 2", "DELL U2720Q 2", "Screen 4"]);
    }

    #[cfg(windows)]
    #[test]
    #[ignore]
    fn print_real_names() {
        println!("{:?}", friendly_names());
    }
}
