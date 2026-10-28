use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Left,
    Centre,
    Right,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourcePref {
    Auto,
    Windows,
    Wsl { distro: String },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub session_threshold: u8,
    pub week_threshold: u8,
    pub position: Position,
    pub screen: Option<String>,
    pub source: SourcePref,
    pub start_wsl: bool,
    pub show_notch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            session_threshold: 90,
            week_threshold: 90,
            position: Position::Centre,
            screen: None,
            source: SourcePref::Auto,
            start_wsl: true,
            show_notch: true,
        }
    }
}

/// Missing, unreadable, empty or corrupt files all fall back to defaults.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .map(clamp)
        .unwrap_or_default()
}

/// Writes to `<path>.tmp` then renames over the target, so a crash never leaves a half-written file.
pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(s).map_err(io::Error::other)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

pub fn clamp(mut s: Settings) -> Settings {
    s.session_threshold = s.session_threshold.clamp(50, 99);
    s.week_threshold = s.week_threshold.clamp(50, 99);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "notch-settings-test-{}-{}",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn load_missing_returns_defaults() {
        let dir = test_dir("load_missing_returns_defaults");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(load(&dir.join("settings.json")), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_corrupt_returns_defaults() {
        let dir = test_dir("load_corrupt_returns_defaults");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip() {
        let dir = test_dir("round_trip");
        let path = dir.join("settings.json");
        let s = Settings {
            session_threshold: 75,
            week_threshold: 80,
            position: Position::Right,
            screen: Some("\\\\.\\DISPLAY2".into()),
            source: SourcePref::Wsl {
                distro: "Ubuntu".into(),
            },
            start_wsl: false,
            show_notch: false,
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clamp_thresholds() {
        let low = clamp(Settings {
            session_threshold: 10,
            ..Settings::default()
        });
        assert_eq!(low.session_threshold, 50);
        let high = clamp(Settings {
            week_threshold: 100,
            ..Settings::default()
        });
        assert_eq!(high.week_threshold, 99);
    }

    #[test]
    fn unknown_fields_ignored() {
        let dir = test_dir("unknown_fields_ignored");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{"session_threshold":75,"week_threshold":80,"position":"left","screen":"DISPLAY1","source":{"kind":"wsl","distro":"Ubuntu"},"start_wsl":false,"old_key":1}"#,
        )
        .unwrap();
        let s = load(&path);
        assert_eq!(s.session_threshold, 75);
        assert_eq!(s.week_threshold, 80);
        assert_eq!(s.position, Position::Left);
        assert_eq!(s.screen, Some("DISPLAY1".into()));
        assert_eq!(
            s.source,
            SourcePref::Wsl {
                distro: "Ubuntu".into()
            }
        );
        assert!(!s.start_wsl);
        assert!(s.show_notch);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wsl_source_json_shape() {
        let json = serde_json::to_string(&SourcePref::Wsl {
            distro: "Ubuntu".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"wsl","distro":"Ubuntu"}"#);
    }

    #[test]
    fn load_clamps_out_of_range() {
        let dir = test_dir("load_clamps_out_of_range");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"session_threshold":10}"#).unwrap();
        assert_eq!(load(&path).session_threshold, 50);
        let _ = fs::remove_dir_all(&dir);
    }
}
