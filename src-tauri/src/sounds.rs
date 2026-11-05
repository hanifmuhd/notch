//! The owner's own sound files (settings window spec, Sounds): one per kind, copied into the
//! app's folder as `sounds/needs.<ext>` or `sounds/azan.<ext>`.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Needs,
    Azan,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "needs" => Some(Kind::Needs),
            "azan" => Some(Kind::Azan),
            _ => None,
        }
    }

    fn stem(self) -> &'static str {
        match self {
            Kind::Needs => "needs",
            Kind::Azan => "azan",
        }
    }
}

pub const MAX_BYTES: usize = 10 * 1024 * 1024;
const EXTS: [&str; 4] = ["mp3", "wav", "ogg", "m4a"];

/// The audio type of a file name, or None when it is not one we accept.
pub fn ext_of(name: &str) -> Option<&'static str> {
    let (_, ext) = name.rsplit_once('.')?;
    EXTS.into_iter().find(|e| e.eq_ignore_ascii_case(ext))
}

/// Stores `bytes` as this kind's file, replacing any earlier one.
pub fn save(dir: &Path, kind: Kind, name: &str, bytes: &[u8]) -> Result<(), String> {
    let ext = ext_of(name).ok_or("Choose an mp3, wav, ogg or m4a file.")?;
    if bytes.len() > MAX_BYTES {
        return Err("Choose a file under 10 MB.".into());
    }
    remove(dir, kind);
    let path = dir.join("sounds").join(format!("{}.{ext}", kind.stem()));
    crate::settings::write_bytes_atomic(&path, bytes).map_err(|e| e.to_string())
}

pub fn find(dir: &Path, kind: Kind) -> Option<PathBuf> {
    EXTS.iter().map(|e| dir.join("sounds").join(format!("{}.{e}", kind.stem()))).find(|p| p.is_file())
}

pub fn remove(dir: &Path, kind: Kind) {
    while let Some(p) = find(dir, kind) {
        if std::fs::remove_file(&p).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("notch-sounds-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn ext_of_accepts_audio_only() {
        assert_eq!(ext_of("Azan.MP3"), Some("mp3"));
        assert_eq!(ext_of("a.m4a"), Some("m4a"));
        assert_eq!(ext_of("my%20bell.wav"), Some("wav"));
        assert_eq!(ext_of("a.ogg"), Some("ogg"));
        assert_eq!(ext_of("a.exe"), None);
        assert_eq!(ext_of("mp3"), None);
        assert_eq!(ext_of(""), None);
        assert_eq!(Kind::parse("azan"), Some(Kind::Azan));
        assert_eq!(Kind::parse("needs"), Some(Kind::Needs));
        assert_eq!(Kind::parse("other"), None);
    }

    #[test]
    fn save_replaces_the_old_file() {
        let d = dir("replace");
        save(&d, Kind::Azan, "x.wav", b"one").unwrap();
        save(&d, Kind::Azan, "y.ogg", b"two").unwrap();
        let found = find(&d, Kind::Azan).unwrap();
        assert_eq!(found.file_name().unwrap(), "azan.ogg");
        assert_eq!(std::fs::read(&found).unwrap(), b"two");
        assert!(!d.join("sounds").join("azan.wav").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_rejects_big_and_wrong_files() {
        let d = dir("reject");
        assert_eq!(save(&d, Kind::Needs, "a.mp3", &vec![0; MAX_BYTES + 1]), Err("Choose a file under 10 MB.".into()));
        assert_eq!(save(&d, Kind::Needs, "a.txt", b"x"), Err("Choose an mp3, wav, ogg or m4a file.".into()));
        assert_eq!(find(&d, Kind::Needs), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn remove_deletes_only_that_kind() {
        let d = dir("remove");
        save(&d, Kind::Azan, "a.mp3", b"a").unwrap();
        save(&d, Kind::Needs, "n.wav", b"n").unwrap();
        remove(&d, Kind::Azan);
        assert_eq!(find(&d, Kind::Azan), None);
        assert!(find(&d, Kind::Needs).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}
