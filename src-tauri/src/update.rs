//! Updates from the latest GitHub release, with Windows' own curl like the prayer download.

use crate::source::Runner;
use std::path::Path;
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/hanifmuhd/notch/releases/latest";
const SETUP: &str = "https://github.com/hanifmuhd/notch/releases/latest/download/ClaudeNotch-setup.exe";

/// The latest release's version ("1.3.1" from tag "v1.3.1") when it is newer than `current`.
pub fn newer(body: &str, current: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let latest = json["tag_name"].as_str()?.trim_start_matches('v');
    let parts = |v: &str| v.split('.').map(|n| n.parse::<u32>().ok()).collect::<Option<Vec<_>>>();
    (parts(latest)? > parts(current)?).then(|| latest.to_string())
}

pub fn check(runner: &dyn Runner) -> Option<String> {
    let body = runner
        .run("curl.exe", &["-s", "-f", "-m", "20", "-A", "ClaudeNotch", LATEST], Duration::from_secs(25))
        .ok()?;
    newer(&body, env!("CARGO_PKG_VERSION"))
}

/// Downloads the newest installer into `dir` and runs it in update mode: it closes the notch,
/// installs over it and starts it again (/R), keeping settings and the events folder (/UPDATE).
pub fn install(runner: &dyn Runner, dir: &Path) -> Result<(), String> {
    let setup = dir.join("ClaudeNotch-setup.exe");
    let out = setup.to_string_lossy();
    runner
        .run("curl.exe", &["-s", "-f", "-L", "-m", "300", "-A", "ClaudeNotch", "-o", &out, SETUP], Duration::from_secs(310))
        .map_err(|e| format!("download failed: {e:?}"))?;
    std::process::Command::new(&setup).args(["/S", "/UPDATE", "/R"]).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_numbers_not_text() {
        let tag = |t: &str| format!(r#"{{"tag_name":"{t}","assets":[]}}"#);
        assert_eq!(newer(&tag("v1.3.1"), "1.3.0"), Some("1.3.1".into()));
        assert_eq!(newer(&tag("v1.10.0"), "1.9.2"), Some("1.10.0".into()));
        assert_eq!(newer(&tag("v1.3.0"), "1.3.0"), None);
        assert_eq!(newer(&tag("v1.2.9"), "1.3.0"), None);
        assert_eq!(newer(&tag("nightly"), "1.3.0"), None);
        assert_eq!(newer("Not Found", "1.3.0"), None);
    }
}
