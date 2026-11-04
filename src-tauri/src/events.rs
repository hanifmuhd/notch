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

/// A finished entry clears itself after this (spec 5.2).
const FINISHED_FOR: Duration = Duration::minutes(30);
/// A reply at least this long toasts when it finishes (spec 5.2).
const LONG_REPLY: Duration = Duration::seconds(60);
/// Files written this long before app start are dropped (spec 5.2).
const START_GRACE: Duration = Duration::minutes(10);
/// Any file older than this is deleted unread.
const MAX_FILE_AGE: Duration = Duration::days(1);
const MAX_ROWS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status {
    NeedsYou,
    Finished,
}

struct Entry {
    session_id: String,
    project: String,
    status: Status,
    since: DateTime<Utc>,
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
        _ => return None,
    };
    Some(HookEvent { session_id, project, kind })
}

impl Sessions {
    /// Applies one event at time `at`; returns the toast to show, if any. `quiet` events
    /// (written before the app started) update the list but never notify.
    pub fn apply(&mut self, e: HookEvent, at: DateTime<Utc>, quiet: bool, finished_on: bool) -> Option<Notice> {
        let id = e.session_id;
        let waiting = self.entries.iter().any(|x| x.session_id == id && x.status == Status::NeedsYou);
        if waiting && matches!(e.kind, Kind::NeedsYou(_)) {
            return None; // already waiting on you: no second toast, keeps its age
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
                self.prompts.insert(id, at);
                return None;
            }
            Kind::End => {
                self.prompts.remove(&id);
                return None;
            }
        };
        self.entries.push(Entry { session_id: id, project: p, status, since: at });
        notice.filter(|_| !quiet)
    }

    /// The panel was opened and closed: finished entries have been seen.
    pub fn seen(&mut self) {
        self.entries.retain(|x| x.status != Status::Finished);
    }

    /// Drops finished entries older than 30 min; true if any went.
    pub fn expire(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.entries.len();
        self.entries.retain(|x| x.status != Status::Finished || now - x.since < FINISHED_FOR);
        // Sessions that never sent SessionEnd must not grow this map forever.
        self.prompts.retain(|_, t| now - *t < MAX_FILE_AGE);
        self.entries.len() != before
    }

    /// Newest first, at most 5.
    #[allow(dead_code)] // shown by the panel (Task 9)
    pub fn rows(&self) -> Vec<SessionRow> {
        let mut rows: Vec<&Entry> = self.entries.iter().collect();
        rows.sort_by(|a, b| b.since.cmp(&a.since));
        rows.into_iter()
            .take(MAX_ROWS)
            .map(|x| SessionRow {
                project: x.project.clone(),
                status: match x.status {
                    Status::NeedsYou => "needs_you",
                    Status::Finished => "finished",
                },
                since_ms: x.since.timestamp_millis(),
            })
            .collect()
    }

    /// Every entry counts, including ones past the 5 shown.
    #[allow(dead_code)] // shown by the pill (Task 9)
    pub fn attention(&self) -> usize {
        self.entries.len()
    }
}

/// Time in a file name `<nanoseconds since epoch>-<pid>.json`, or None if it does not parse.
fn name_time(name: &str) -> Option<DateTime<Utc>> {
    let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().ok().map(DateTime::from_timestamp_nanos)
}

/// Reads and deletes every `*.json` event in `dir`, oldest name first, as (event, time, quiet).
/// Skips `.tmp` files still being written; deletes anything older than a day unread. Files
/// from more than 10 min before `started` are dropped; the rest from before it are quiet.
pub fn take_files(dir: &Path, started: DateTime<Utc>, now: DateTime<Utc>) -> Vec<(HookEvent, DateTime<Utc>, bool)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let path = dir.join(&name);
        let at = name_time(&name).unwrap_or(now);
        if now - at > MAX_FILE_AGE {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        if !name.ends_with(".json") {
            continue;
        }
        let text = std::fs::read_to_string(&path);
        // Unreadable or not, the file is done with; a stuck file would be re-read every second.
        let _ = std::fs::remove_file(&path);
        if at < started - START_GRACE {
            continue;
        }
        if let Some(e) = text.ok().as_deref().and_then(parse_event) {
            out.push((e, at, at < started));
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
        assert_eq!(parse_event(r#"{"session_id":"s","cwd":"/a/b/","hook_event_name":"Stop"}"#).unwrap().project, "b");
    }

    #[test]
    fn short_reply_is_quiet_long_reply_toasts() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", Kind::Prompt), t, false, true), None);
        assert_eq!(s.apply(ev("a", Kind::Finished), t + Duration::seconds(59), false, true), None);
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "finished");
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Prompt), t, false, true);
        let n = s.apply(ev("a", Kind::Finished), t + Duration::seconds(60), false, true);
        assert_eq!(
            n,
            Some(Notice { title: "Claude finished".into(), body: "own".into(), flash: "own finished".into() })
        );
        // No prompt seen (app started mid-task) counts as long.
        let mut s = Sessions::default();
        assert!(s.apply(ev("b", Kind::Finished), t, false, true).is_some());
    }

    #[test]
    fn needs_you_toasts_once() {
        let t = Utc::now();
        let mut s = Sessions::default();
        let n = s.apply(ev("a", needs()), t, false, true).unwrap();
        assert_eq!(n.title, "Claude needs you");
        assert_eq!(n.flash, "own needs you");
        assert!(n.body.starts_with("own"));
        assert!(n.body.contains('m'));
        assert_eq!(s.apply(ev("a", needs()), t + Duration::seconds(5), false, true), None);
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "needs_you");
        assert_eq!(s.rows()[0].since_ms, t.timestamp_millis());
    }

    #[test]
    fn prompt_and_end_clear() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", needs()), t, false, true);
        s.apply(ev("a", Kind::Prompt), t, false, true);
        assert!(s.rows().is_empty());
        s.apply(ev("a", Kind::Finished), t, false, true);
        s.apply(ev("a", Kind::End), t, false, true);
        assert!(s.rows().is_empty());
        // Stop after needs-you replaces it with finished.
        s.apply(ev("b", needs()), t, false, true);
        s.apply(ev("b", Kind::Finished), t, false, true);
        assert_eq!(s.rows()[0].status, "finished");
    }

    #[test]
    fn finished_clears_on_seen_and_after_30_min() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", Kind::Finished), t, false, true);
        s.apply(ev("b", needs()), t, false, true);
        s.seen();
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "needs_you");
        s.apply(ev("a", Kind::Finished), t, false, true);
        assert!(!s.expire(t + Duration::minutes(29)));
        assert!(s.expire(t + Duration::minutes(30)));
        assert_eq!(s.rows().len(), 1);
        assert_eq!(s.rows()[0].status, "needs_you"); // needs-you never times out
    }

    #[test]
    fn finished_off_drops_stop() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", Kind::Finished), t, false, false), None);
        assert!(s.rows().is_empty());
        assert!(s.apply(ev("a", needs()), t, false, false).is_some());
    }

    #[test]
    fn quiet_events_never_notice() {
        let t = Utc::now();
        let mut s = Sessions::default();
        assert_eq!(s.apply(ev("a", needs()), t, true, true), None);
        assert_eq!(s.apply(ev("b", Kind::Finished), t, true, true), None);
        assert_eq!(s.rows().len(), 2);
    }

    #[test]
    fn same_project_two_sessions() {
        let t = Utc::now();
        let mut s = Sessions::default();
        s.apply(ev("a", needs()), t, false, true);
        s.apply(ev("b", Kind::Finished), t + Duration::seconds(1), false, true);
        assert_eq!(s.rows().len(), 2);
        assert_eq!(s.attention(), 2);
        assert_eq!(s.rows()[0].status, "finished"); // newest first
    }

    #[test]
    fn rows_are_capped_at_five() {
        let t = Utc::now();
        let mut s = Sessions::default();
        for i in 0..7 {
            s.apply(ev(&i.to_string(), needs()), t + Duration::seconds(i), false, true);
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
    fn take_files_order_age_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("notch-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let started = Utc::now() - Duration::minutes(30);
        let now = Utc::now();
        let ns = |t: DateTime<Utc>| t.timestamp_nanos_opt().unwrap();
        let body = |name: &str| format!(r#"{{"session_id":"{name}","cwd":"/p","hook_event_name":"Stop"}}"#);
        let write = |file: String, sid: &str| std::fs::write(dir.join(file), body(sid)).unwrap();
        write(format!("{}-1.json", ns(now - Duration::days(2))), "old");
        write(format!("{}-1.json", ns(started - Duration::minutes(11))), "dropped");
        write(format!("{}-1.json", ns(now - Duration::seconds(5))), "new");
        write(format!("{}-1.json", ns(started - Duration::minutes(5))), "quiet");
        write(format!("{}-2.tmp", ns(now)), "tmp");
        write("bad.json".into(), "noname");
        std::fs::write(dir.join(format!("{}-3.json", ns(now - Duration::seconds(1)))), "{").unwrap();

        let got = take_files(&dir, started, now);
        let ids: Vec<(&str, bool)> = got.iter().map(|(e, _, q)| (e.session_id.as_str(), *q)).collect();
        assert_eq!(ids, vec![("quiet", true), ("new", false), ("noname", false)]);
        assert_eq!(got[1].1.timestamp_nanos_opt().unwrap() / 1000, ns(now - Duration::seconds(5)) / 1000);
        assert_eq!(got[2].1, now); // unparseable name counts as now
        let left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec![format!("{}-2.tmp", ns(now))]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
