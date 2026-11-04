//! Claude Code hook events (written by the hooks in `hooks.rs`) turned into session state
//! (spec 5.2-5.3). Pure logic plus one folder reader, so it is testable without Tauri.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    NeedsYou(String),
    Finished,
    Prompt,
    End,
    /// `PostToolUse`: the session is still alive (shows no text).
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookEvent {
    pub session_id: String,
    pub project: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub flash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionRow {
    pub project: String,
    pub status: &'static str,
    pub since_ms: i64,
}

/// The session the pod shows: needs you first, else the newest working one (spec 5.4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pod {
    pub project: String,
    pub status: &'static str,
    pub since_ms: i64,
    /// Other sessions that are working or need you.
    pub more: usize,
}

/// A working entry with no event for this long is dropped (Esc, crash, closed terminal).
const WORKING_FOR: Duration = Duration::minutes(10);
/// A finished entry clears itself after this (spec 5.2).
const FINISHED_FOR: Duration = Duration::minutes(30);
/// A reply at least this long toasts when it finishes (spec 5.2).
const LONG_REPLY: Duration = Duration::seconds(60);
/// Files already waiting at start and older than this are dropped (spec 5.2).
const START_GRACE: Duration = Duration::minutes(10);
/// Any file older than this is deleted unread.
const MAX_FILE_AGE: Duration = Duration::days(1);
const MAX_ROWS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status {
    NeedsYou,
    Working,
    Finished,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::NeedsYou => "needs_you",
            Status::Working => "working",
            Status::Finished => "finished",
        }
    }
}

struct Entry {
    session_id: String,
    project: String,
    status: Status,
    since: DateTime<Utc>,
    /// Seen-time of the session's latest event, for the working timeout.
    last: DateTime<Utc>,
}

/// In memory only; a restart starts empty.
#[derive(Default)]
pub struct Sessions {
    entries: Vec<Entry>,
    /// Last `UserPromptSubmit` time per session, for the 1-minute rule.
    prompts: HashMap<String, DateTime<Utc>>,
}

/// One hook payload, or None for events the app ignores and anything malformed.
pub fn parse_event(json: &str) -> Option<HookEvent> {
    let v: Value = serde_json::from_str(json).ok()?;
    let text = |k: &str| v.get(k).and_then(Value::as_str);
    let session_id = text("session_id").filter(|s| !s.is_empty())?.to_string();
    let cwd = text("cwd").unwrap_or_default();
    // Linux (WSL) or Windows path; the last non-empty folder names the project.
    let project = cwd.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(cwd).to_string();
    let kind = match text("hook_event_name")? {
        "Notification" => match text("notification_type")? {
            "permission_prompt" | "elicitation_dialog" => Kind::NeedsYou(text("message").unwrap_or_default().to_string()),
            _ => return None,
        },
        "PermissionRequest" => Kind::NeedsYou(format!("Claude wants to use {}", text("tool_name").unwrap_or("a tool"))),
        "Stop" => Kind::Finished,
        "UserPromptSubmit" => Kind::Prompt,
        "SessionEnd" => Kind::End,
        "PostToolUse" => Kind::Heartbeat,
        _ => return None,
    };
    Some(HookEvent { session_id, project, kind })
}

impl Sessions {
    /// Applies one event; returns the toast to show, if any. `at` is the file-name time (the
    /// writer's clock, maybe WSL's), used only for the 1-minute rule; `seen` is when the app read
    /// it, used for row age and expiry. `quiet` events update the list but never notify.
    pub fn apply(&mut self, e: HookEvent, at: DateTime<Utc>, seen: DateTime<Utc>, quiet: bool, finished_on: bool) -> Option<Notice> {
        let id = e.session_id;
        let waiting = self.entries.iter().any(|x| x.session_id == id && x.status == Status::NeedsYou);
        if waiting && matches!(e.kind, Kind::NeedsYou(_)) {
            return None; // already waiting on you: no second toast, keeps its age
        }
        if e.kind == Kind::Heartbeat {
            // Still alive: an answered prompt goes back to working; finished stays finished.
            if let Some(x) = self.entries.iter_mut().find(|x| x.session_id == id && x.status != Status::Finished) {
                if x.status == Status::NeedsYou {
                    x.status = Status::Working;
                    x.since = seen;
                }
                x.last = seen;
            }
            return None;
        }
        // Every other event replaces the session's entry (a Stop also ends a needs-you).
        self.entries.retain(|x| x.session_id != id);
        let p = e.project;
        let (status, notice) = match e.kind {
            Kind::NeedsYou(msg) => {
                let body = if msg.is_empty() { p.clone() } else { format!("{p}: {msg}") };
                let n = Notice { title: "Claude needs you".into(), body, flash: format!("{p} needs you") };
                (Status::NeedsYou, Some(n))
            }
            Kind::Finished if finished_on => {
                let long = self.prompts.get(&id).is_none_or(|t| at - *t >= LONG_REPLY);
                let n = Notice { title: "Claude finished".into(), body: p.clone(), flash: format!("{p} finished") };
                (Status::Finished, long.then_some(n))
            }
            Kind::Finished => return None, // "Also when a task finishes" is off
            Kind::Prompt => {
                self.prompts.insert(id.clone(), at);
                (Status::Working, None)
            }
            Kind::End => {
                self.prompts.remove(&id);
                return None;
            }
            Kind::Heartbeat => return None, // handled above
        };
        self.entries.push(Entry { session_id: id, project: p, status, since: seen, last: seen });
        notice.filter(|_| !quiet)
    }

    /// The panel was opened and closed: finished entries have been seen.
    pub fn seen(&mut self) {
        self.entries.retain(|x| x.status != Status::Finished);
    }

    /// Drops finished entries older than 30 min; true if any went.
    pub fn expire(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.entries.len();
        self.entries.retain(|x| match x.status {
            Status::Finished => now - x.since < FINISHED_FOR,
            Status::Working => now - x.last < WORKING_FOR,
            Status::NeedsYou => true,
        });
        // Sessions that never sent SessionEnd must not grow this map forever (rough: mixes clocks).
        self.prompts.retain(|_, t| now - *t < MAX_FILE_AGE);
        self.entries.len() != before
    }

    /// Newest first, at most 5.
    pub fn rows(&self) -> Vec<SessionRow> {
        // Entries are pushed as they are seen, so the newest are last.
        self.entries
            .iter()
            .rev()
            .take(MAX_ROWS)
            .map(|x| SessionRow {
                project: x.project.clone(),
                status: x.status.name(),
                since_ms: x.since.timestamp_millis(),
            })
            .collect()
    }

    /// Entries that need a look (needs you or finished), including ones past the 5 shown.
    pub fn attention(&self) -> usize {
        self.entries.iter().filter(|x| x.status != Status::Working).count()
    }

    pub fn pod(&self) -> Option<Pod> {
        let active: Vec<&Entry> = self.entries.iter().filter(|x| x.status != Status::Finished).collect();
        let pick = active
            .iter()
            .rev()
            .find(|x| x.status == Status::NeedsYou)
            .or_else(|| active.last())?;
        Some(Pod {
            project: pick.project.clone(),
            status: pick.status.name(),
            since_ms: pick.since.timestamp_millis(),
            more: active.len() - 1,
        })
    }
}

/// Time in a file name `<nanoseconds since epoch>-<pid>.json`, or None if it does not parse.
fn name_time(name: &str) -> Option<DateTime<Utc>> {
    let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().ok().map(DateTime::from_timestamp_nanos)
}

/// Reads and deletes every `*.json` event in `dir`, oldest name first, as (event, name time,
/// quiet). Skips `.tmp` files still being written. Ages use the file's modified time (the
/// Windows clock; name times come from the writer's clock, which can lag after sleep): anything
/// older than a day is deleted unread. On the `first` scan after start, files older than 10 min
/// are dropped and the rest are quiet; files found by later scans are loud.
pub fn take_files(dir: &Path, first: bool, now: DateTime<Utc>) -> Vec<(HookEvent, DateTime<Utc>, bool)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let path = dir.join(&name);
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).map_or(now, DateTime::<Utc>::from);
        if now - modified > MAX_FILE_AGE {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        if !name.ends_with(".json") {
            continue;
        }
        let text = std::fs::read_to_string(&path);
        // Applied only once deleted: a file that stays would otherwise toast every second.
        if std::fs::remove_file(&path).is_err() || (first && now - modified > START_GRACE) {
            continue;
        }
        if let Some(e) = text.ok().as_deref().and_then(parse_event) {
            out.push((e, name_time(&name).unwrap_or(now), first));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(session: &str, kind: Kind) -> HookEvent {
        HookEvent { session_id: session.into(), project: "own".into(), kind }
    }

    fn needs() -> Kind {
        Kind::NeedsYou("m".into())
    }

    #[test]
    fn needs_you_types() {
        let n = |t: &str| parse_event(&format!(r#"{{"session_id":"s","cwd":"/home/h/own","hook_event_name":"Notification","message":"m","notification_type":"{t}"}}"#));
        assert!(matches!(n("permission_prompt").unwrap().kind, Kind::NeedsYou(_)));
        assert!(matches!(n("elicitation_dialog").unwrap().kind, Kind::NeedsYou(_)));
        assert!(n("idle_prompt").is_none());
        assert_eq!(n("permission_prompt").unwrap().project, "own");
        let p = parse_event(r#"{"session_id":"s","cwd":"C:\\w\\proj","hook_event_name":"PermissionRequest","tool_name":"Write"}"#).unwrap();
        assert_eq!(p.project, "proj");
        assert!(matches!(p.kind, Kind::NeedsYou(ref m) if m == "Claude wants to use Write"));
        let k = |name: &str| parse_event(&format!(r#"{{"session_id":"s","cwd":"/a/b/","hook_event_name":"{name}"}}"#)).map(|e| e.kind);
        assert_eq!(k("Stop"), Some(Kind::Finished));
        assert_eq!(k("UserPromptSubmit"), Some(Kind::Prompt));
        assert_eq!(k("SessionEnd"), Some(Kind::End));
        assert_eq!(k("PreToolUse"), None);
        assert_eq!(k("PostToolUse"), Some(Kind::Heartbeat));
        assert_eq!(parse_event(r#"{"session_id":"s","cwd":"/a/b/","hook_event_name":"Stop"}"#).unwrap().project, "b");
    }

    #[test]
    fn short_reply_is_quiet_long_reply_toasts() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", Kind::Prompt), t, t, false, true), None);
        assert_eq!(s.apply(ev("a", Kind::Finished), t + Duration::seconds(59), t + Duration::seconds(59), false, true), None);
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "finished");
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Prompt), t, t, false, true);
        let n = s.apply(ev("a", Kind::Finished), t + Duration::seconds(60), t + Duration::seconds(60), false, true);
        assert_eq!(
            n,
            Some(Notice { title: "Claude finished".into(), body: "own".into(), flash: "own finished".into() })
        );
        // No prompt seen (app started mid-task) counts as long.
        let mut s = Sessions::default();
        assert!(s.apply(ev("b", Kind::Finished), t, t, false, true).is_some());
    }

    #[test]
    fn needs_you_toasts_once() {
        let t = Utc::now();
        let mut s = Sessions::default();
        let n = s.apply(ev("a", needs()), t, t, false, true).unwrap();
        assert_eq!(n.title, "Claude needs you");
        assert_eq!(n.flash, "own needs you");
        assert!(n.body.starts_with("own"));
        assert!(n.body.contains('m'));
        assert_eq!(s.apply(ev("a", needs()), t + Duration::seconds(5), t + Duration::seconds(5), false, true), None);
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "needs_you");
        assert_eq!(s.rows()[0].since_ms, t.timestamp_millis());
    }

    #[test]
    fn prompt_and_end_clear() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", needs()), t, t, false, true);
        s.apply(ev("a", Kind::Prompt), t, t, false, true);
        assert_eq!(s.rows()[0].status, "working"); // answering in the terminal sets it working
        s.apply(ev("a", Kind::Finished), t, t, false, true);
        s.apply(ev("a", Kind::End), t, t, false, true);
        assert!(s.rows().is_empty());
        // Stop after needs-you replaces it with finished.
        s.apply(ev("b", needs()), t, t, false, true);
        s.apply(ev("b", Kind::Finished), t, t, false, true);
        assert_eq!(s.rows()[0].status, "finished");
    }

    #[test]
    fn finished_clears_on_seen_and_after_30_min() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Finished), t, t, false, true);
        s.apply(ev("b", needs()), t, t, false, true);
        s.seen();
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "needs_you");
        s.apply(ev("a", Kind::Finished), t, t, false, true);
        assert!(!s.expire(t + Duration::minutes(29)));
        assert!(s.expire(t + Duration::minutes(30)));
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "needs_you"); // needs-you never times out
    }

    #[test]
    fn finished_off_drops_stop() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", Kind::Finished), t, t, false, false), None);
        assert!(s.rows().is_empty());
        assert!(s.apply(ev("a", needs()), t, t, false, false).is_some());
    }

    #[test]
    fn quiet_events_never_notice() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", needs()), t, t, true, true), None);
        assert_eq!(s.apply(ev("b", Kind::Finished), t, t, true, true), None);
        assert_eq!(s.rows().len(), 2);
    }

    #[test]
    fn same_project_two_sessions() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", needs()), t, t, false, true);
        s.apply(ev("b", Kind::Finished), t + Duration::seconds(1), t + Duration::seconds(1), false, true);
        assert_eq!(s.rows().len(), 2);
        assert_eq!(s.attention(), 2);
        assert_eq!(s.rows()[0].status, "finished"); // newest first
    }

    #[test]
    fn rows_are_capped_at_five() {
        let t = Utc::now();
        let mut s = Sessions::default();
        for i in 0..7 {
            s.apply(ev(&i.to_string(), needs()), t + Duration::seconds(i), t + Duration::seconds(i), false, true);
        }
        assert_eq!(s.rows().len(), 5);
        assert_eq!(s.rows()[0].since_ms, (t + Duration::seconds(6)).timestamp_millis());
        assert_eq!(s.attention(), 7);
    }

    #[test]
    fn bad_payloads_are_skipped() {
        assert!(parse_event("").is_none());
        assert!(parse_event("{").is_none());
        assert!(parse_event(r#"{"hook_event_name":"Stop"}"#).is_none());
        assert!(parse_event(r#"{"session_id":"","cwd":"/a","hook_event_name":"Stop"}"#).is_none());
        assert!(parse_event("[1]").is_none());
    }

    #[test]
    fn finished_age_and_expiry_use_the_time_seen() {
        // The WSL clock (file names) can lag Windows after sleep; only the 60 s rule uses it.
        let seen = Utc::now();
        let wsl = seen - Duration::hours(2);
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Prompt), wsl, seen, false, true);
        assert!(s.apply(ev("a", Kind::Finished), wsl + Duration::seconds(30), seen, false, true).is_none());
        assert_eq!(s.rows()[0].since_ms, seen.timestamp_millis());
        assert!(!s.expire(seen + Duration::minutes(29)));
        assert!(s.expire(seen + Duration::minutes(30)));
    }

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("notch-events-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        /// Writes a Stop event for `sid` with the given modified time.
        fn put(&self, file: &str, sid: &str, modified: DateTime<Utc>) {
            let path = self.0.join(file);
            std::fs::write(&path, format!(r#"{{"session_id":"{sid}","cwd":"/p","hook_event_name":"Stop"}}"#)).unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(modified.into()).unwrap();
        }
        fn left(&self) -> Vec<String> {
            let mut v: Vec<String> =
                std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ids(got: &[(HookEvent, DateTime<Utc>, bool)]) -> Vec<(&str, bool)> {
        got.iter().map(|(e, _, q)| (e.session_id.as_str(), *q)).collect()
    }

    #[test]
    fn take_files_order_age_and_cleanup() {
        let d = Scratch::new("order");
        let now = Utc::now();
        let ns = |t: DateTime<Utc>| t.timestamp_nanos_opt().unwrap();
        let name = |t: DateTime<Utc>, ext: &str| format!("{}-1.{ext}", ns(t));
        d.put(&name(now - Duration::seconds(9), "json"), "old", now - Duration::days(2));
        d.put(&name(now - Duration::seconds(8), "json"), "dropped", now - Duration::minutes(11));
        d.put(&name(now - Duration::seconds(5), "json"), "second", now - Duration::seconds(5));
        d.put(&name(now - Duration::seconds(7), "json"), "first", now - Duration::minutes(5));
        // Name time (WSL clock) far off but modified now: kept, not deleted as old.
        d.put(&name(now - Duration::days(3), "json"), "skewed", now);
        d.put(&name(now - Duration::days(3), "tmp"), "stale-tmp", now - Duration::days(2));
        d.put(&name(now, "tmp"), "tmp", now);
        d.put("bad.json", "noname", now);
        std::fs::write(d.0.join(name(now - Duration::seconds(1), "json")), "{").unwrap();

        // First scan after start: older than 10 min dropped, the rest quiet; oldest name first.
        let got = take_files(&d.0, true, now);
        let all_quiet = vec![("skewed", true), ("first", true), ("second", true), ("noname", true)];
        assert_eq!(ids(&got), all_quiet);
        assert_eq!(got[2].1.timestamp_nanos_opt().unwrap() / 1000, ns(now - Duration::seconds(5)) / 1000);
        assert_eq!(got[3].1, now); // unparseable name counts as now
        assert_eq!(d.left(), vec![name(now, "tmp")]);

        // Later scans: every new file is loud, however old its modified time.
        d.put(&name(now, "json"), "late", now - Duration::minutes(20));
        assert_eq!(ids(&take_files(&d.0, false, now)), vec![("late", false)]);
    }

    /// A file that cannot be deleted yet is not applied, so it cannot toast every second; it
    /// is applied once when the delete works. Windows only: the lock is a Windows share mode.
    #[cfg(windows)]
    #[test]
    fn undeletable_file_is_applied_once_deleted() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = Scratch::new("locked");
        d.put("1-1.json", "a", Utc::now());
        // Held open with read sharing only (FILE_SHARE_READ), as another process might.
        let lock = std::fs::OpenOptions::new().read(true).share_mode(1).open(d.0.join("1-1.json")).unwrap();
        assert!(take_files(&d.0, false, Utc::now()).is_empty());
        assert!(take_files(&d.0, false, Utc::now()).is_empty());
        drop(lock);
        assert_eq!(ids(&take_files(&d.0, false, Utc::now())), vec![("a", false)]);
        assert!(take_files(&d.0, false, Utc::now()).is_empty());
    }

    #[test]
    fn prompt_makes_working() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", Kind::Prompt), t, t, false, true), None);
        assert_eq!(s.rows()[0].status, "working");
        let pod = s.pod().unwrap();
        assert_eq!((pod.status, pod.project.as_str(), pod.more), ("working", "own", 0));
        assert_eq!(s.attention(), 0); // working needs no look
    }

    #[test]
    fn heartbeat_clears_needs_you() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Prompt), t, t, false, true);
        s.apply(ev("a", needs()), t, t, false, true);
        assert_eq!(s.pod().unwrap().status, "needs_you");
        assert_eq!(s.apply(ev("a", Kind::Heartbeat), t, t, false, true), None);
        assert_eq!(s.rows()[0].status, "working");
        // A finished reply is not reopened by a stray heartbeat.
        s.apply(ev("a", Kind::Finished), t, t, false, true);
        s.apply(ev("a", Kind::Heartbeat), t, t, false, true);
        assert_eq!(s.rows()[0].status, "finished");
    }

    #[test]
    fn heartbeat_unknown_session_is_ignored() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("x", Kind::Heartbeat), t, t, false, true), None);
        assert!(s.rows().is_empty());
        assert!(s.pod().is_none());
    }

    #[test]
    fn working_expires_after_10_min() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Prompt), t, t, false, true);
        assert!(!s.expire(t + Duration::seconds(599)));
        let mut s2 = Sessions::default();
        s2.apply(ev("a", Kind::Prompt), t, t, false, true);
        assert!(s2.expire(t + Duration::minutes(10)));
        assert!(s2.rows().is_empty());
        // A heartbeat at 9 min keeps it until 19 min.
        s.apply(ev("a", Kind::Heartbeat), t + Duration::minutes(9), t + Duration::minutes(9), false, true);
        assert!(!s.expire(t + Duration::minutes(18)));
        assert!(s.expire(t + Duration::minutes(19)));
    }

    #[test]
    fn pod_prefers_needs_you_and_counts_more() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(HookEvent { session_id: "a".into(), project: "pa".into(), kind: Kind::Prompt }, t, t, false, true);
        s.apply(HookEvent { session_id: "b".into(), project: "pb".into(), kind: needs() }, t, t, false, true);
        s.apply(HookEvent { session_id: "c".into(), project: "pc".into(), kind: Kind::Prompt }, t, t, false, true);
        s.apply(HookEvent { session_id: "d".into(), project: "pd".into(), kind: Kind::Finished }, t, t, false, true);
        let pod = s.pod().unwrap();
        assert_eq!((pod.project.as_str(), pod.status, pod.more), ("pb", "needs_you", 2));
        let mut only_done = Sessions::default();
        only_done.apply(ev("d", Kind::Finished), t, t, false, true);
        assert!(only_done.pod().is_none());
    }

    #[test]
    fn post_tool_use_parses_as_heartbeat() {
        let e = parse_event(r#"{"hook_event_name":"PostToolUse","session_id":"s","cwd":"/x/p"}"#).unwrap();
        assert_eq!((e.kind, e.project.as_str()), (Kind::Heartbeat, "p"));
    }
}
