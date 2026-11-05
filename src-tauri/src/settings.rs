use serde::{Deserialize, Deserializer, Serialize};
use std::io;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Left,
    Centre,
    Right,
}

/// Where the notch lives (redesign spec 5.5).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShowIn {
    #[default]
    Top,
    Taskbar,
}

/// The built-in bells (src/sounds/<name>.ogg).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Bell {
    #[default]
    Chime,
    Ding,
    Soft,
    Double,
}

/// What an alert plays: the chosen bell, the owner's own file, or nothing.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum SoundChoice {
    #[default]
    Bell,
    File,
    Off,
}

/// An unknown value resets only its own field instead of making the whole file unreadable.
fn or_default<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
    Ok(T::deserialize(serde_json::Value::deserialize(d)?).unwrap_or_default())
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
    /// Task alerts also show finished tasks (the Stop hook stays installed either way).
    pub task_finished: bool,
    pub show_in: ShowIn,
    /// Keep the notch below other windows so a maximised window covers it (spec 5.6).
    pub stay_behind: bool,
    /// Prayer times (prayer spec 5.4): off by default; JAKIM zone; reminder minutes 0/5/10/15.
    pub prayer_on: bool,
    pub prayer_zone: String,
    pub prayer_remind: u32,
    /// Sounds (settings window spec): the bell, and what needs-you and prayer alerts play.
    #[serde(deserialize_with = "or_default")]
    pub bell: Bell,
    #[serde(deserialize_with = "or_default")]
    pub needs_sound: SoundChoice,
    #[serde(deserialize_with = "or_default")]
    pub prayer_sound: SoundChoice,
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
            task_finished: true,
            show_in: ShowIn::Top,
            stay_behind: false,
            prayer_on: false,
            prayer_zone: "WLY01".into(),
            prayer_remind: 10,
            bell: Bell::Chime,
            needs_sound: SoundChoice::Bell,
            prayer_sound: SoundChoice::Bell,
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
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    write_bytes_atomic(path, text.as_bytes())
}

pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    write_atomic(path, &serde_json::to_string_pretty(s).map_err(io::Error::other)?)
}

pub fn clamp(mut s: Settings) -> Settings {
    s.session_threshold = s.session_threshold.clamp(50, 99);
    s.week_threshold = s.week_threshold.clamp(50, 99);
    if ![0, 5, 10, 15].contains(&s.prayer_remind) {
        s.prayer_remind = 10;
    }
    if !crate::prayer_zones::ZONES.iter().any(|(code, _)| *code == s.prayer_zone) {
        s.prayer_zone = Settings::default().prayer_zone;
    }
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
            task_finished: false,
            show_in: ShowIn::Taskbar,
            stay_behind: true,
            prayer_on: true,
            prayer_zone: "SGR01".into(),
            prayer_remind: 5,
            bell: Bell::Soft,
            needs_sound: SoundChoice::Off,
            prayer_sound: SoundChoice::File,
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
        assert!(s.task_finished);
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

    #[test]
    fn old_settings_file_gets_new_defaults() {
        let dir = test_dir("old_file");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"session_threshold":80,"week_threshold":90,"position":"right","screen":null,"source":{"kind":"auto"},"start_wsl":true,"show_notch":true,"task_finished":true}"#).unwrap();
        let s = load(&path);
        assert_eq!(s.session_threshold, 80);
        assert_eq!(s.show_in, ShowIn::Top);
        assert!(!s.stay_behind);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_new_fields() {
        let dir = test_dir("new_fields");
        let path = dir.join("settings.json");
        let s = Settings { show_in: ShowIn::Taskbar, stay_behind: true, ..Settings::default() };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        assert!(fs::read_to_string(&path).unwrap().contains("\"show_in\": \"taskbar\""));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_settings_get_prayer_defaults() {
        let dir = test_dir("prayer_defaults");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"session_threshold":90,"show_in":"top","stay_behind":false}"#).unwrap();
        let s = load(&path);
        assert!(!s.prayer_on);
        assert_eq!(s.prayer_zone, "WLY01");
        assert_eq!(s.prayer_remind, 10);
        // Only Off / 5 / 10 / 15 are allowed.
        fs::write(&path, r#"{"prayer_remind":7}"#).unwrap();
        assert_eq!(load(&path).prayer_remind, 10);
        // A zone that is not in JAKIM's list falls back to the default.
        fs::write(&path, r#"{"prayer_zone":"XYZ99"}"#).unwrap();
        assert_eq!(load(&path).prayer_zone, "WLY01");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sound_settings_default_and_tolerate_unknown_values() {
        let dir = test_dir("sounds");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"session_threshold":80}"#).unwrap();
        let s = load(&path);
        assert_eq!((s.bell, s.needs_sound, s.prayer_sound), (Bell::Chime, SoundChoice::Bell, SoundChoice::Bell));
        // An unknown value resets only that field; the rest of the file still loads.
        fs::write(&path, r#"{"session_threshold":80,"needs_sound":"loud","bell":"gong"}"#).unwrap();
        let s = load(&path);
        assert_eq!((s.session_threshold, s.needs_sound, s.bell), (80, SoundChoice::Bell, Bell::Chime));
        let s = Settings { bell: Bell::Double, needs_sound: SoundChoice::Off, prayer_sound: SoundChoice::File, ..Settings::default() };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        let _ = fs::remove_dir_all(&dir);
    }
}
