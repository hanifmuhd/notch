//! Adds and removes the app's own Claude Code hooks in Claude's settings.json (spec 5.1).
//! Only hook objects whose command contains `MARKER` are ever touched.

use crate::settings::write_atomic;
use crate::source::{wsl_running, Runner, Source, CLI_TIMEOUT};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MARKER: &str = "claude-notch";
pub const EVENTS: [&str; 5] = ["Notification", "PermissionRequest", "Stop", "UserPromptSubmit", "SessionEnd"];
pub const HINT_NOT_FOUND: &str = "Install or find Claude Code first";
pub const HINT_START_WSL: &str = "Start WSL to change this";
const INVALID_JSON: &str = "Couldn't read Claude's settings.json, so nothing was changed";

#[derive(Debug, PartialEq)]
pub enum HookError {
    InvalidJson,
    Io(String),
}

impl HookError {
    /// Text for the Settings switch.
    pub fn message(&self) -> String {
        match self {
            HookError::InvalidJson => INVALID_JSON.to_string(),
            HookError::Io(e) => e.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Windows,
    Wsl { distro: String },
}

/// The Task alerts switch as Settings shows it.
#[derive(Serialize, Debug, PartialEq)]
pub struct TaskAlerts {
    pub on: bool,
    pub available: bool,
    pub hint: Option<String>,
}

/// One bash line that copies the hook's stdin into the events folder and always exits 0.
/// No mkdir: once the app's folder is gone, the hook does nothing.
pub fn hook_command(events_dir_in_shell: &str) -> String {
    let dir = events_dir_in_shell.replace('\'', r"'\''");
    format!(r#"d='{dir}'; [ -d "$d" ] && {{ f="$d/$(date +%s%N)-$$"; cat > "$f.tmp" && mv "$f.tmp" "$f.json"; }}; true # {MARKER}"#)
}

/// Only a JSON object is edited; anything else is refused as it is.
fn parse(json: &str) -> Result<Map<String, Value>, HookError> {
    match serde_json::from_str(json) {
        Ok(Value::Object(root)) => Ok(root),
        _ => Err(HookError::InvalidJson),
    }
}

fn pretty(root: Map<String, Value>) -> Result<String, HookError> {
    serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| HookError::Io(e.to_string()))
}

fn ours(hook: &Value) -> bool {
    hook.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(MARKER))
}

/// Drops our hook objects and the groups this left empty. Returns the events whose arrays
/// this left empty, for `prune`; containers that were empty before are not ours to drop.
fn strip(hooks: &mut Map<String, Value>) -> Vec<String> {
    let mut emptied = vec![];
    for (event, groups) in hooks.iter_mut() {
        let Value::Array(groups) = groups else { continue };
        let before = groups.len();
        groups.retain_mut(|group| {
            let Some(Value::Array(list)) = group.get_mut("hooks") else { return true };
            let n = list.len();
            list.retain(|h| !ours(h));
            !(list.is_empty() && n > 0)
        });
        if groups.is_empty() && before > 0 {
            emptied.push(event.clone());
        }
    }
    emptied
}

/// Drops the emptied event arrays that are still empty, then `hooks` if that emptied it.
fn prune(root: &mut Map<String, Value>, emptied: &[String]) {
    let Some(Value::Object(hooks)) = root.get_mut("hooks") else { return };
    for event in emptied {
        if hooks.get(event).and_then(Value::as_array).is_some_and(Vec::is_empty) {
            hooks.shift_remove(event);
        }
    }
    if hooks.is_empty() && !emptied.is_empty() {
        root.shift_remove("hooks");
    }
}

/// Replaces any old entries of ours with one group per event; everything else stays in place.
pub fn install(json: &str, command: &str) -> Result<String, HookError> {
    let mut root = parse(json)?;
    let Value::Object(hooks) = root.entry("hooks").or_insert_with(|| json!({})) else {
        return Err(HookError::InvalidJson);
    };
    let emptied = strip(hooks);
    for event in EVENTS {
        let Value::Array(groups) = hooks.entry(event).or_insert_with(|| json!([])) else {
            return Err(HookError::InvalidJson);
        };
        groups.push(json!({ "hooks": [{ "type": "command", "command": command }] }));
    }
    prune(&mut root, &emptied);
    pretty(root)
}

pub fn remove(json: &str) -> Result<String, HookError> {
    let mut root = parse(json)?;
    let emptied = match root.get_mut("hooks") {
        Some(Value::Object(hooks)) => strip(hooks),
        _ => vec![],
    };
    prune(&mut root, &emptied);
    pretty(root)
}

pub fn is_installed(json: &str) -> bool {
    let Ok(root) = parse(json) else { return false };
    root.get("hooks").and_then(Value::as_object).is_some_and(|hooks| {
        hooks
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|group| group.get("hooks")?.as_array())
            .flatten()
            .any(ours)
    })
}

/// `C:\Users\a b` → `/c/Users/a b`, the form Git Bash (Windows Claude Code's hook shell) reads.
pub fn git_bash_path(win: &str) -> String {
    let unix = win.replace('\\', "/");
    match unix.as_bytes() {
        [drive, b':', ..] if drive.is_ascii_alphabetic() => {
            format!("/{}{}", drive.to_ascii_lowercase() as char, &unix[2..])
        }
        _ => unix,
    }
}

/// Which install the switch edits, or the hint for a disabled switch. A stopped distro is
/// never booted when Start WSL is off.
pub fn target_for(source: Option<&Source>, start_wsl: bool, runner: &dyn Runner) -> Result<Target, &'static str> {
    match source {
        None | Some(Source::NotFound) => Err(HINT_NOT_FOUND),
        Some(Source::Windows) => Ok(Target::Windows),
        Some(Source::Wsl { distro }) if !start_wsl && !wsl_running(runner, distro) => Err(HINT_START_WSL),
        Some(Source::Wsl { distro }) => Ok(Target::Wsl { distro: distro.clone() }),
    }
}

pub fn settings_path(target: &Target, runner: &dyn Runner) -> Result<PathBuf, HookError> {
    let home = match target {
        Target::Windows => PathBuf::from(
            std::env::var_os("USERPROFILE").ok_or_else(|| HookError::Io("Couldn't find your user folder".into()))?,
        ),
        Target::Wsl { distro } => {
            let out = runner
                .run("wsl.exe", &["-d", distro, "--", "bash", "-lc", r#"printf %s "$HOME""#], CLI_TIMEOUT)
                .unwrap_or_default();
            // A login shell may print a banner first; printf's output is the last line.
            let home = out.trim_end().lines().last().unwrap_or("");
            if !home.starts_with('/') {
                return Err(HookError::Io("Couldn't find your home folder in WSL".into()));
            }
            PathBuf::from(format!(r"\\wsl.localhost\{distro}{}", home.replace('/', "\\")))
        }
    };
    Ok(home.join(".claude").join("settings.json"))
}

/// The events folder as the hook's shell sees it.
pub fn events_dir_in_shell(target: &Target, win_dir: &Path, runner: &dyn Runner) -> Result<String, HookError> {
    let win = win_dir.to_string_lossy();
    match target {
        Target::Windows => Ok(git_bash_path(&win)),
        Target::Wsl { distro } => {
            // --exec skips the Linux shell, which would eat the backslashes.
            let out = runner
                .run("wsl.exe", &["-d", distro, "--exec", "wslpath", "-u", &win], CLI_TIMEOUT)
                .unwrap_or_default();
            let dir = out.trim();
            if !dir.starts_with('/') {
                return Err(HookError::Io("Couldn't reach WSL to set up task alerts".into()));
            }
            Ok(dir.to_string())
        }
    }
}

/// Turns the hooks on or off in the settings.json at `path`. A missing file counts as `{}`;
/// invalid JSON is left alone. The file as it was before the first edit is kept once in
/// `settings.json.notch-backup`.
pub fn apply_at(path: &Path, on: bool, shell_dir: &str) -> Result<(), HookError> {
    // Switch flips and a source-change move run on separate threads and share one `.tmp`.
    static EDIT: Mutex<()> = Mutex::new(());
    let _edit = EDIT.lock().unwrap_or_else(|e| e.into_inner());
    let io_err = |e: io::Error| HookError::Io(format!("Couldn't update Claude's settings.json: {e}"));
    let old = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) if e.kind() == io::ErrorKind::InvalidData => return Err(HookError::InvalidJson),
        Err(e) => return Err(io_err(e)),
    };
    let current = old.as_deref().unwrap_or("{}");
    let mut next = if on { install(current, &hook_command(shell_dir))? } else { remove(current)? };
    if current.ends_with('\n') {
        next.push('\n');
    }
    // Off with nothing of ours must not reformat the owner's file.
    if next == current || (!on && !is_installed(current)) {
        return Ok(());
    }
    if let Some(text) = &old {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".notch-backup");
        let backup = PathBuf::from(backup);
        if !backup.try_exists().map_err(io_err)? {
            write_atomic(&backup, text).map_err(io_err)?;
        }
    }
    write_atomic(path, &next).map_err(io_err)
}

pub fn apply(target: &Target, on: bool, win_events_dir: &Path, runner: &dyn Runner) -> Result<(), HookError> {
    let path = settings_path(target, runner)?;
    let dir = if on { events_dir_in_shell(target, win_events_dir, runner)? } else { String::new() };
    apply_at(&path, on, &dir)
}

/// Read from the file each time, so manual edits show truthfully.
fn installed(target: &Target, runner: &dyn Runner) -> bool {
    settings_path(target, runner)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|text| is_installed(&text))
}

pub fn status(source: Option<&Source>, start_wsl: bool, runner: &dyn Runner) -> TaskAlerts {
    match target_for(source, start_wsl, runner) {
        Err(hint) => TaskAlerts { on: false, available: false, hint: Some(hint.into()) },
        Ok(target) => TaskAlerts { on: installed(&target, runner), available: true, hint: None },
    }
}

pub fn set(source: Option<&Source>, start_wsl: bool, on: bool, win_events_dir: &Path, runner: &dyn Runner) -> Result<TaskAlerts, String> {
    let target = target_for(source, start_wsl, runner)?;
    apply(&target, on, win_events_dir, runner).map_err(|e| e.message())?;
    Ok(TaskAlerts { on, available: true, hint: None })
}

/// After a source change: if the old install had our hooks, takes them out there and puts
/// them into the new one. Both halves run; the first error is returned.
pub fn move_hooks(old: Option<&Source>, new: Option<&Source>, start_wsl: bool, win_events_dir: &Path, runner: &dyn Runner) -> Result<(), HookError> {
    let Ok(from) = target_for(old, start_wsl, runner) else { return Ok(()) };
    let to = target_for(new, start_wsl, runner);
    if to.as_ref() == Ok(&from) || !installed(&from, runner) {
        return Ok(());
    }
    let removed = apply(&from, false, win_events_dir, runner);
    let added = match to {
        Ok(to) => apply(&to, true, win_events_dir, runner),
        Err(_) => Ok(()),
    };
    removed.and(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::RunError;
    use serde_json::Value;
    use std::cell::RefCell;
    use std::time::Duration;

    const CMD: &str = "d='/x'; true # claude-notch";

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn hook_command_shape() {
        assert_eq!(
            hook_command("/mnt/c/U/events"),
            r#"d='/mnt/c/U/events'; [ -d "$d" ] && { f="$d/$(date +%s%N)-$$"; cat > "$f.tmp" && mv "$f.tmp" "$f.json"; }; true # claude-notch"#
        );
        // A quote in a user name must not break out of the single-quoted folder.
        assert!(hook_command("/c/Users/O'Brien").starts_with(r#"d='/c/Users/O'\''Brien'; "#));
    }

    #[test]
    fn install_into_empty_and_foreign() {
        let v = parse(&install("{}", CMD).unwrap());
        for e in EVENTS {
            assert_eq!(v["hooks"][e][0]["hooks"][0]["command"], CMD);
            assert_eq!(v["hooks"][e][0]["hooks"][0]["type"], "command");
        }
        let foreign = r#"{"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"orca"}]}]}}"#;
        let v = parse(&install(foreign, CMD).unwrap());
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "orca");
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(v["model"], "x");
    }

    #[test]
    fn install_is_idempotent() {
        let a = install("{}", CMD).unwrap();
        assert_eq!(install(&a, CMD).unwrap(), a);
        assert_eq!(remove("{}").unwrap(), "{}");
    }

    #[test]
    fn new_folder_replaces_the_old_command() {
        let a = install("{}", CMD).unwrap();
        let v = parse(&install(&a, "d='/y'; true # claude-notch").unwrap());
        for e in EVENTS {
            assert_eq!(v["hooks"][e].as_array().unwrap().len(), 1);
            assert_eq!(v["hooks"][e][0]["hooks"][0]["command"], "d='/y'; true # claude-notch");
        }
    }

    #[test]
    fn remove_keeps_foreign_matcher_group() {
        let start = r#"{"hooks":{"Notification":[{"matcher":"x","hooks":[{"type":"command","command":"orca"}]}]}}"#;
        let back = parse(&remove(&install(start, CMD).unwrap()).unwrap());
        assert_eq!(back, parse(start));
    }

    /// Same shape as a real settings.json with another tool's hooks (values made up).
    const REALISTIC: &str = r#"{
  "model": "opus",
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "tool start", "timeout": 5 }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "tool prompt", "timeout": 5 }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "tool stop", "timeout": 5 }] }],
    "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "tool pre", "timeout": 5 }] }],
    "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "tool perm", "timeout": 5 }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "tool end", "timeout": 5 }] }]
  },
  "enabledPlugins": { "b@m": true, "a@m": false },
  "theme": "dark",
  "ratio": 1.0
}"#;

    #[test]
    fn realistic_round_trip_keeps_everything_and_its_order() {
        let on = install(REALISTIC, CMD).unwrap();
        assert!(is_installed(&on));
        let v = parse(&on);
        assert_eq!(v["hooks"]["PermissionRequest"][0]["matcher"], "*");
        assert_eq!(v["hooks"]["Notification"][0]["hooks"][0]["command"], CMD);
        let off = remove(&on).unwrap();
        assert!(!is_installed(&off));
        assert_eq!(off, serde_json::to_string_pretty(&parse(REALISTIC)).unwrap());
        let keys: Vec<_> = parse(&off).as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["model", "hooks", "enabledPlugins", "theme", "ratio"]);
        assert!(off.contains("\"ratio\": 1.0"));
    }

    #[test]
    fn our_hook_inside_a_foreign_group_goes_alone() {
        let start = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"orca"},{"type":"command","command":"x # claude-notch"}]}]}}"#;
        let v = parse(&remove(start).unwrap());
        assert_eq!(v, parse(r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"orca"}]}]}}"#));
    }

    #[test]
    fn invalid_json_is_refused() {
        assert!(matches!(install("{nope", CMD), Err(HookError::InvalidJson)));
        assert!(!is_installed("{nope"));
        assert_eq!(remove("[1]"), Err(HookError::InvalidJson));
        assert_eq!(install(r#"{"hooks":[]}"#, CMD), Err(HookError::InvalidJson));
        assert_eq!(install(r#"{"hooks":{"Stop":{}}}"#, CMD), Err(HookError::InvalidJson));
    }

    #[test]
    fn git_bash_paths() {
        assert_eq!(git_bash_path(r"C:\Users\a b\x"), "/c/Users/a b/x");
        assert_eq!(git_bash_path(r"D:\events"), "/d/events");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notch-hooks-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn apply_backs_up_once_and_round_trips() {
        let dir = scratch("apply");
        let path = dir.join("settings.json");
        let backup = dir.join("settings.json.notch-backup");
        std::fs::write(&path, REALISTIC).unwrap();
        apply_at(&path, true, "/mnt/c/events").unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), REALISTIC);
        assert!(is_installed(&std::fs::read_to_string(&path).unwrap()));
        assert!(!dir.join("settings.json.tmp").exists());
        apply_at(&path, false, "").unwrap();
        apply_at(&path, true, "/mnt/c/events").unwrap();
        // The backup still holds the very first file.
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), REALISTIC);
        apply_at(&path, false, "").unwrap();
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), parse(REALISTIC));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_on_missing_file_creates_it_and_off_leaves_nothing() {
        let dir = scratch("missing");
        let path = dir.join(".claude").join("settings.json");
        apply_at(&path, false, "").unwrap();
        assert!(!path.exists());
        apply_at(&path, true, "/e").unwrap();
        assert!(is_installed(&std::fs::read_to_string(&path).unwrap()));
        assert!(!dir.join(".claude").join("settings.json.notch-backup").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_refuses_invalid_json_and_leaves_it_alone() {
        let dir = scratch("invalid");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{nope").unwrap();
        assert_eq!(apply_at(&path, true, "/e"), Err(HookError::InvalidJson));
        assert_eq!(apply_at(&path, false, ""), Err(HookError::InvalidJson));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{nope");
        assert!(!dir.join("settings.json.notch-backup").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_then_off_gives_back_the_same_bytes() {
        let dir = scratch("bytes");
        let path = dir.join("settings.json");
        let start = "{\n  \"model\": \"x\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"orca\"\n          }\n        ]\n      }\n    ]\n  }\n}\n";
        std::fs::write(&path, start).unwrap();
        apply_at(&path, true, "/e").unwrap();
        let on = std::fs::read_to_string(&path).unwrap();
        assert!(on.ends_with("}\n"));
        apply_at(&path, true, "/e").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), on);
        apply_at(&path, false, "").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), start);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parallel_switch_flips_never_break_the_file() {
        let dir = scratch("parallel");
        let path = dir.join("settings.json");
        std::fs::write(&path, REALISTIC).unwrap();
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || (0..15).map(|i| apply_at(&path, (t + i) % 2 == 0, "/e")).collect::<Vec<_>>())
            })
            .collect();
        for t in threads {
            for r in t.join().unwrap() {
                assert_eq!(r, Ok(()));
            }
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(serde_json::from_str::<Value>(&text).is_ok());
        apply_at(&path, false, "").unwrap();
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()), parse(REALISTIC));
        apply_at(&path, true, "/e").unwrap();
        assert!(is_installed(&std::fs::read_to_string(&path).unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn off_without_our_hooks_does_not_rewrite_the_file() {
        let dir = scratch("untouched");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{\"a\":1}").unwrap();
        apply_at(&path, false, "").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":1}");
        assert!(!dir.join("settings.json.notch-backup").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct FakeRunner {
        map: Vec<(&'static str, Result<String, RunError>)>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(map: Vec<(&'static str, Result<String, RunError>)>) -> Self {
            FakeRunner { map, calls: RefCell::new(vec![]) }
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[&str], _t: Duration) -> Result<String, RunError> {
            let key = format!("{} {}", program, args.join(" "));
            self.calls.borrow_mut().push(key.clone());
            self.map.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()).unwrap_or(Err(RunError::Spawn("unknown".into())))
        }
    }

    #[test]
    fn target_and_hints() {
        let none = FakeRunner::new(vec![]);
        assert_eq!(target_for(None, true, &none), Err(HINT_NOT_FOUND));
        assert_eq!(target_for(Some(&Source::NotFound), true, &none), Err(HINT_NOT_FOUND));
        assert_eq!(target_for(Some(&Source::Windows), false, &none), Ok(Target::Windows));
        let wsl = Source::Wsl { distro: "Ubuntu".into() };
        let stopped = FakeRunner::new(vec![("wsl.exe -l --running -q", Ok(String::new()))]);
        assert_eq!(target_for(Some(&wsl), false, &stopped), Err(HINT_START_WSL));
        assert!(stopped.calls.borrow().iter().all(|c| !c.contains(" -d ")));
        let running = FakeRunner::new(vec![("wsl.exe -l --running -q", Ok("Ubuntu\r\n".into()))]);
        assert_eq!(target_for(Some(&wsl), false, &running), Ok(Target::Wsl { distro: "Ubuntu".into() }));
        assert_eq!(target_for(Some(&wsl), true, &none), Ok(Target::Wsl { distro: "Ubuntu".into() }));
    }

    #[test]
    fn wsl_paths_come_from_the_distro() {
        let t = Target::Wsl { distro: "Ubuntu".into() };
        let r = FakeRunner::new(vec![
            ("wsl.exe -d Ubuntu -- bash -lc printf %s \"$HOME\"", Ok("motd line\n/home/u".into())),
            (r"wsl.exe -d Ubuntu --exec wslpath -u C:\Users\u\events", Ok("/mnt/c/Users/u/events\n".into())),
        ]);
        assert_eq!(settings_path(&t, &r).unwrap(), PathBuf::from(r"\\wsl.localhost\Ubuntu\home\u\.claude\settings.json"));
        assert_eq!(events_dir_in_shell(&t, Path::new(r"C:\Users\u\events"), &r).unwrap(), "/mnt/c/Users/u/events");
        let bad = FakeRunner::new(vec![("wsl.exe -d Ubuntu -- bash -lc printf %s \"$HOME\"", Ok(String::new()))]);
        assert!(matches!(settings_path(&t, &bad), Err(HookError::Io(_))));
        assert!(matches!(events_dir_in_shell(&t, Path::new("C:\\e"), &bad), Err(HookError::Io(_))));
    }

    #[test]
    fn error_copy() {
        assert_eq!(HookError::InvalidJson.message(), INVALID_JSON);
    }
}

