# Task alerts and v1 follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tell the owner when a Claude Code session needs them or finished a long reply (notch + toast), and close the six v1 review follow-ups.

**Architecture:** Claude Code hooks (installed by a Settings switch into the active install's `settings.json`) write one JSON file per event into `%APPDATA%\com.hanifmuhd.claudenotch\events`; a 1 s Rust loop reads them into a pure session-state machine kept in `AppState`, which feeds the existing `ViewModel` and toast path. Follow-ups are local changes in `source.rs`, `usage.rs`, `alerts.rs`, `app_state.rs`, `view.rs`, `main.rs` and `tauri.conf.json`.

**Tech Stack:** Tauri v2, Rust (chrono, serde_json, `windows` 0.62), plain HTML/CSS/JS webview.

**Spec:** `docs/superpowers/specs/2026-11-04-task-alerts-and-follow-ups-design.md`; payload facts in `docs/spikes/hook-payloads.md`.

## Global Constraints

- NEVER read, copy or send Claude's OAuth token, `~/.claude/.credentials.json` or any cookie. Data comes only from the official `claude` command and hook stdin **(owner, hard rule)**.
- Never lose or corrupt the owner's Claude `settings.json`: keep every entry the app did not write (Orca hooks exist); refuse to edit invalid JSON; one-time backup `settings.json.notch-backup`; write via `.tmp` + rename.
- Hook marker string: `claude-notch` (in every command the app writes; removal matches only it).
- Hooked events: `Notification`, `PermissionRequest`, `Stop`, `UserPromptSubmit`, `SessionEnd`.
- "Needs you" = `Notification` with `notification_type` `permission_prompt` or `elicitation_dialog`, or any `PermissionRequest`. `idle_prompt` and others are ignored.
- A `Stop` toasts / flashes only if ≥ 60 s since that session's last `UserPromptSubmit` (no prompt seen = long).
- Finished entries clear when the panel is opened then closed, or after 30 min. Pill flash lasts 6 s.
- Events folder files older than 10 min at app start are dropped; newer ones enter the list silently. Any file older than 1 day is deleted.
- Usage call adds `--settings '{"disableAllHooks":true}'`; WSL side wrapped in `timeout 55`; app deadline stays 60 s.
- Copy (exact): toast titles `Claude needs you` / `Claude finished`; logged-out text `Log in to Claude Code: run claude, then /login`; idle text `Starts when you next use Claude`; disabled-switch hints `Install or find Claude Code first` / `Start WSL to change this`; invalid JSON `Couldn't read Claude's settings.json, so nothing was changed`.
- Run Rust tests only from Windows: `powershell.exe -NoProfile -Command '$env:Path = [Environment]::GetEnvironmentVariable("Path","User") + ";" + [Environment]::GetEnvironmentVariable("Path","Machine"); $env:CARGO_TARGET_DIR = "C:\Users\hanif\code\notch-target"; Set-Location \\wsl.localhost\Ubuntu\home\hanif\own\notch-project\src-tauri; cargo test'` (below: **`WINTEST`**, optionally followed by a test filter). Never run cargo from Linux. Never `wsl --shutdown`.
- Work only in `~/own/notch-project` on branch `build/task-alerts`; stage only files you changed.

## Review Focus

1. Claude `settings.json` with hooks in the other shape (`hooks.Stop` missing, `hooks` missing, an entry with a `matcher`) must still merge and remove cleanly, leaving foreign entries byte-equal in value. → Task 7 tests `install_into_empty_and_foreign`, `remove_keeps_foreign_matcher_group`.
2. A hook payload missing `session_id` or with a non-JSON body (partial write, other tool) must be skipped, never panic or block later files. → Task 8 test `bad_payloads_are_skipped`.
3. Two terminals in the same folder are two sessions (keyed by `session_id`, not project). → Task 8 test `same_project_two_sessions`.
4. Re-installing when already installed must not duplicate entries; removing when absent is a no-op. → Task 7 test `install_is_idempotent`.
5. A crash between writes of `alerts.json` or a corrupt file must fall back to today's behaviour, not block start. → Task 4 test `corrupt_alerts_file_is_default`.

---

### Task 1: Usage call fires no hooks and leaves no process behind

**Files:**
- Modify: `src-tauri/src/source.rs` (`usage_command`, `SystemRunner::run`)
- Modify: `src-tauri/Cargo.toml` (add `Win32_System_JobObjects`, `Win32_System_Threading`, `Win32_Security` features if needed)

**Interfaces:**
- Produces: `usage_command(&Source) -> (String, Vec<String>)` (same signature, new args).

- [ ] **Step 1: Update the shape tests first**

```rust
// wsl_command_shape
assert_eq!(a, ["-d", "Ubuntu", "--", "bash", "-lc",
  "timeout 55 claude -p --settings '{\"disableAllHooks\":true}' --output-format json /usage"]);
// windows_command_shape
assert_eq!(a, ["/d", "/c", "claude", "-p", "--settings", "{\"disableAllHooks\":true}", "--output-format", "json", "/usage"]);
```

- [ ] **Step 2: Run** `WINTEST command_shape` → both FAIL.
- [ ] **Step 3: Change `usage_command`** to produce exactly those args.
- [ ] **Step 4: Job Object in `SystemRunner::run` (Windows only).** After `spawn`, create a job (`CreateJobObjectW`), set `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `AssignProcessToJobObject` with the child's handle (`child.as_raw_handle()`); on timeout call `TerminateJobObject` before `child.kill()`; close the job handle on every return path (a small RAII guard struct). If any job call fails, carry on without it (log in debug). Add a Windows-only test:

```rust
#[cfg(windows)] #[test]
fn timeout_kills_grandchildren() {
    // parent spawns a child powershell that would create a marker file after 3 s
    let marker = std::env::temp_dir().join(format!("notch-job-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let script = format!("Start-Process -NoNewWindow powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 3; New-Item -ItemType File -Path \"{}\"'; Start-Sleep 10", marker.display());
    let r = SystemRunner.run("powershell", &["-NoProfile", "-Command", &script], Duration::from_secs(1));
    assert_eq!(r, Err(RunError::Timeout));
    std::thread::sleep(Duration::from_secs(4));
    assert!(!marker.exists(), "grandchild survived the timeout");
}
```

- [ ] **Step 5: Run** `WINTEST source::` → all PASS (incl. the existing `runner_timeout_is_error`, `exited_child_with_held_pipe_times_out`).
- [ ] **Step 6: Commit** `Run the usage check without hooks and stop all of its processes on timeout`.

### Task 2: Idle session without a reset time

**Files:**
- Modify: `src-tauri/src/usage.rs` (`Limit.resets_at: Option<DateTime<Utc>>`, `parse_limit`)
- Modify: `src-tauri/src/alerts.rs` (`step`), `src-tauri/src/view.rs` (`limit_view`, `pill_text` callers, `toast_text`), `src-tauri/src/alerts.rs` `AlertEvent` keeps `DateTime<Utc>` (only fired when a reset exists)
- Test: in-module tests of the same files

**Interfaces:**
- Produces: `pub struct Limit { pub used_pct: f64, pub resets_at: Option<DateTime<Utc>> }`.

- [ ] **Step 1: Failing tests**

```rust
// usage.rs
#[test] fn session_without_reset_is_idle() {
    let r = raw(&format!("Current session: 0% used\n{WEEK_LINE}"));
    let got = parse_usage(&r, t0()).unwrap();
    assert_eq!(got.session, Limit { used_pct: 0.0, resets_at: None });
    assert!(got.week.resets_at.is_some());
}
// view.rs
#[test] fn idle_limit_view() {
    let now = local(28, 10, 0);
    let lv = limit_view(&Limit { used_pct: 0.0, resets_at: None }, 90, SESSION_WINDOW, now);
    assert_eq!(lv.reset_text, "Starts when you next use Claude");
    assert_eq!(lv.pace_text, ""); assert_eq!(lv.pace_pct, 0.0); assert_eq!(lv.runs_out_text, None);
}
// alerts.rs
#[test] fn idle_limit_never_fires() { /* reading with session {95.0, None} → no events, state unchanged for session */ }
```

- [ ] **Step 2: Run** `WINTEST` → new tests fail to compile/FAIL.
- [ ] **Step 3: Implement.** `parse_limit`: if there is no `·`, parse `N% used` alone → `resets_at: None`; a `·` part that fails to parse is still `BadTime`. `alerts::step`: `None` → return the armed state unchanged, no event. `limit_view`: `None` → idle text above, `pace_pct 0`, empty `pace_text`, no runs-out; level still from `used_pct`. Fix every other `resets_at` use (`view::build` full-pill text uses `.unwrap_or(now)` is NOT allowed — Full with no reset shows `"{name} full"`).
- [ ] **Step 4: Run** `WINTEST` → all PASS.
- [ ] **Step 5: Commit** `Accept an idle session that has no reset time`.

### Task 3: Logged-out hint

**Files:**
- Modify: `src-tauri/src/usage.rs` (`UsageError::LoggedOut`), `src-tauri/src/app_state.rs` (`fetch`, `finish_refresh`, `Inner.logged_out`), `src-tauri/src/view.rs` (`PillState::LoggedOut`), `src/notch.js` (`renderPill`), `src-tauri/src/main.rs` (`refresh`)

**Interfaces:**
- Produces: `pub enum Fetched { Reading(UsageReading), LoggedOut, Failed }`; `fetch(&AppState, &dyn Runner) -> Fetched`; `AppState::finish_refresh(&self, Fetched) -> Vec<AlertEvent>`; `PillState::LoggedOut` serialises `"logged_out"`.

- [ ] **Step 1: Failing tests**

```rust
// usage.rs
#[test] fn login_error_is_logged_out() {
    let r = json!({"is_error": true, "result": "Invalid API key · Please run /login"}).to_string();
    assert_eq!(parse_usage(&r, t0()), Err(UsageError::LoggedOut));
}
// app_state.rs (FakeRunner returning RunError::Exit(1, "Please run /login".into()) for the usage command)
#[test] fn exit_with_login_text_is_logged_out() { assert!(matches!(fetch(&s, &r), Fetched::LoggedOut)); }
// view.rs
#[test] fn logged_out_pill() { /* build(..) with logged_out=true → pill LoggedOut, updated_text "Log in to Claude Code: run claude, then /login" */ }
```

- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.** Logged out = (`is_error: true` or non-zero exit) AND the text (result or stderr), lower-cased, contains `/login` or `log in` or `logged out` or `invalid api key`. `finish_refresh(LoggedOut)` keeps the old reading but sets `logged_out = true`; any `Reading` clears it. `view::build` gains a `logged_out: bool` parameter and puts LoggedOut ahead of everything except NoClaude. JS: `logged_out` renders the same muted label style as `no_claude` with text `Log in to Claude Code`.
- [ ] **Step 4: Run** `WINTEST` → PASS.
- [ ] **Step 5: Commit** `Say when Claude Code is logged out instead of showing old numbers`.

### Task 4: Fired alerts survive a restart

**Files:**
- Modify: `src-tauri/src/alerts.rs` (derive `Serialize, Deserialize` on `Armed`, `AlertState`), `src-tauri/src/app_state.rs` (`AppState::new` loads, `run_alerts` saves), `src-tauri/src/settings.rs` (make the `.tmp`+rename writer reusable: `pub fn write_atomic(path: &Path, text: &str) -> io::Result<()>`)

**Interfaces:**
- Produces: `alerts::load(path: &Path) -> AlertState`, `alerts::save(path: &Path, s: &AlertState) -> io::Result<()>`; file = `settings_path.with_file_name("alerts.json")`.

- [ ] **Step 1: Failing tests:** `alert_state_round_trips` (save then load equals), `corrupt_alerts_file_is_default` (garbage text → `AlertState::default()`), and in app_state `restart_does_not_refire` (fire at 92 %, new `AppState` on same dir, same reading → no events).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement;** save only when the state changed; a save error is logged, never fatal.
- [ ] **Step 4: Run** `WINTEST` → PASS.
- [ ] **Step 5: Commit** `Remember fired usage alerts across restarts`.

### Task 5: Content security policy

**Files:**
- Modify: `src-tauri/tauri.conf.json` (`app.security.csp`)

- [ ] **Step 1:** Set `"csp": "default-src 'self'; connect-src ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'"`.
- [ ] **Step 2:** `WINTEST` still PASS, then run the app (`cargo tauri dev` through the same PowerShell prefix) and open devtools (debug build): no CSP violations in the console; hover, click, settings, refresh, sliders all work; fonts render. If Tauri needs another source, add only that and note it in the commit body.
- [ ] **Step 3: Commit** `Lock the window to the app's own files with a content security policy`.

### Task 6: Friendly monitor names

**Files:**
- Create: `src-tauri/src/monitors.rs`
- Modify: `src-tauri/src/main.rs` (`list_screens` label), `src-tauri/Cargo.toml` (`Win32_Devices_Display` feature)

**Interfaces:**
- Produces: `monitors::labels(names: &[Option<String>], primary: &[bool]) -> Vec<String>` (pure); `#[cfg(windows)] monitors::friendly_names() -> HashMap<String, String>` keyed by GDI device name (`\\.\DISPLAY1`), via `GetDisplayConfigBufferSizes` + `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)` + `DisplayConfigGetDeviceInfo` for `DISPLAYCONFIG_SOURCE_DEVICE_NAME` and `DISPLAYCONFIG_TARGET_DEVICE_NAME.monitorFriendlyDeviceName`.

- [ ] **Step 1: Failing test**

```rust
#[test] fn labels_fallback_and_dedupe() {
    let l = labels(&[Some("DELL U2720Q".into()), None, Some("DELL U2720Q".into()), Some("".into())], &[true, false, false, false]);
    assert_eq!(l, ["DELL U2720Q (main)", "Screen 2", "DELL U2720Q 2", "Screen 4"]);
}
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; `list_screens` keeps `id` unchanged and uses `labels`. **Step 4: Run** `WINTEST` → PASS; check by hand that Settings → Show on lists real names.
- [ ] **Step 5: Commit** `Show monitor names in the screen picker`.

### Task 7: Install and remove the hooks

**Files:**
- Create: `src-tauri/src/hooks.rs`
- Modify: `src-tauri/Cargo.toml` (`serde_json = { version = "1", features = ["preserve_order"] }`), `src-tauri/src/settings.rs` (`task_finished: bool`, default `true`), `src-tauri/src/main.rs` (commands), `src-tauri/src/app_state.rs` (source change moves hooks)

**Interfaces:**
- Produces (pure): `pub const MARKER: &str = "claude-notch"`; `pub const EVENTS: [&str; 5]`; `hook_command(events_dir_in_shell: &str) -> String`; `install(json: &str, command: &str) -> Result<String, HookError>`; `remove(json: &str) -> Result<String, HookError>`; `is_installed(json: &str) -> bool`; `git_bash_path(win: &str) -> String` (`C:\Users\a b` → `/c/Users/a b`); `enum HookError { InvalidJson, Io(String) }`.
- Produces (IO): `enum Target { Windows, Wsl { distro: String } }`; `settings_path(target, &dyn Runner) -> Result<PathBuf, HookError>` (WSL: `\\wsl.localhost\<distro>` + `$HOME` from `wsl.exe -d <distro> -- bash -lc 'printf %s "$HOME"'`); `events_dir_in_shell(target, win_dir, &dyn Runner)` (WSL: `wsl.exe -d <distro> -- wslpath -u '<win_dir>'`; Windows: `git_bash_path`); `apply(target, on: bool, win_events_dir, &dyn Runner) -> Result<(), HookError>` (read or `{}` if missing; backup once; install/remove; `write_atomic`).
- Produces (commands): `get_task_alerts() -> TaskAlerts { on: bool, available: bool, hint: Option<String> }`, `set_task_alerts(on: bool) -> Result<TaskAlerts, String>`.

- [ ] **Step 1: Failing tests** (pure, in `hooks.rs`):

```rust
const CMD: &str = "d='/x'; true # claude-notch";
#[test] fn hook_command_shape() {
    assert_eq!(hook_command("/mnt/c/U/events"),
      r#"d='/mnt/c/U/events'; [ -d "$d" ] && { f="$d/$(date +%s%N)-$$"; cat > "$f.tmp" && mv "$f.tmp" "$f.json"; }; true # claude-notch"#);
}
#[test] fn install_into_empty_and_foreign() {
    let out = install("{}", CMD).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    for e in EVENTS { assert_eq!(v["hooks"][e][0]["hooks"][0]["command"], CMD); }
    let foreign = r#"{"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"orca"}]}]}}"#;
    let v: serde_json::Value = serde_json::from_str(&install(foreign, CMD).unwrap()).unwrap();
    assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "orca");
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    assert_eq!(v["model"], "x");
}
#[test] fn install_is_idempotent() { let a = install("{}", CMD).unwrap(); assert_eq!(install(&a, CMD).unwrap(), a); assert_eq!(remove("{}").unwrap(), "{}"); }
#[test] fn remove_keeps_foreign_matcher_group() {
    let start = r#"{"hooks":{"Notification":[{"matcher":"x","hooks":[{"type":"command","command":"orca"}]}]}}"#;
    let back: serde_json::Value = serde_json::from_str(&remove(&install(start, CMD).unwrap()).unwrap()).unwrap();
    assert_eq!(back, serde_json::from_str::<serde_json::Value>(start).unwrap());
}
#[test] fn invalid_json_is_refused() { assert!(matches!(install("{nope", CMD), Err(HookError::InvalidJson))); assert!(!is_installed("{nope")); }
#[test] fn git_bash_paths() { assert_eq!(git_bash_path(r"C:\Users\a b\x"), "/c/Users/a b/x"); }
```
Also an `apply` test with a temp dir and `FakeRunner`-free `Target::Windows` variant taking an explicit path (`apply_at(path, on, shell_dir)`): backup is created once and not overwritten on a second call.

- [ ] **Step 2: Run** `WINTEST hooks::` → FAIL.
- [ ] **Step 3: Implement.** `install` = `remove` first, then append one `{"hooks":[{"type":"command","command":cmd}]}` group per event (this makes it idempotent and lets a changed path replace the old one). `remove` drops hook objects whose `command` contains `MARKER`, then empty groups, empty event arrays, and an empty `hooks` object. Output pretty-printed with 2-space indent. `install`/`remove` refuse a top level that is not an object.
- [ ] **Step 4: Commands and availability.** `get_task_alerts` maps the current source: `None`/`NotFound` → unavailable, hint `Install or find Claude Code first`; WSL with `start_wsl` off and distro not running → unavailable, hint `Start WSL to change this`; otherwise read the file and report `is_installed`. `set_task_alerts` errors map to the exact invalid-JSON copy. The app creates `<app_config_dir>\events` at start. In `save_settings`, when the source changed and the old install had the hooks, remove them there and install them into the new one (best effort, errors logged). Run on a blocking thread (`spawn_blocking`), never on the main thread.
- [ ] **Step 5: Run** `WINTEST` → PASS.
- [ ] **Step 6: Commit** `Add a Task alerts switch that installs Claude Code hooks safely`.

### Task 8: Read events into session state

**Files:**
- Create: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/app_state.rs` (`Inner.sessions: Sessions`), `src-tauri/src/main.rs` (1 s intake thread, toasts, emit)

**Interfaces:**
- Produces: `enum Kind { NeedsYou(String /*message*/), Finished, Prompt, End }`; `struct HookEvent { session_id: String, project: String, kind: Kind }`; `parse_event(json: &str) -> Option<HookEvent>` (project = last segment of `cwd`, `/` or `\`); `struct Sessions` with `apply(&mut self, e: HookEvent, at: DateTime<Utc>, quiet: bool, finished_on: bool) -> Option<Notice>`, `seen(&mut self)`, `expire(&mut self, now)`, `rows(&self) -> Vec<SessionRow>`, `attention(&self) -> usize`; `struct Notice { title: String, body: String, flash: String }`; `#[derive(Serialize)] struct SessionRow { project: String, status: &'static str /*"needs_you"|"finished"*/, since_ms: i64 }`; `take_files(dir: &Path, started: DateTime<Utc>, now) -> Vec<(HookEvent, DateTime<Utc>, bool /*quiet*/)>` reads `*.json` sorted by file name, deletes each, skips `.tmp`, deletes files > 1 day, marks files whose name time is before `started - 10 min` dropped and before `started` quiet.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn needs_you_types() {
    let n = |t: &str| parse_event(&format!(r#"{{"session_id":"s","cwd":"/home/h/own","hook_event_name":"Notification","message":"m","notification_type":"{t}"}}"#));
    assert!(matches!(n("permission_prompt").unwrap().kind, Kind::NeedsYou(_)));
    assert!(matches!(n("elicitation_dialog").unwrap().kind, Kind::NeedsYou(_)));
    assert!(n("idle_prompt").is_none());
    let p = parse_event(r#"{"session_id":"s","cwd":"C:\\w\\proj","hook_event_name":"PermissionRequest","tool_name":"Write"}"#).unwrap();
    assert_eq!(p.project, "proj");
    assert!(matches!(p.kind, Kind::NeedsYou(ref m) if m == "Claude wants to use Write"));
}
#[test] fn short_reply_is_quiet_long_reply_toasts() { /* Prompt at t, Finished at t+59s → None, row finished; Prompt at t, Finished at t+60s → Some(Notice{title:"Claude finished", body:"own", flash:"own finished"}) */ }
#[test] fn needs_you_toasts_once() { /* two NeedsYou in a row → Some then None; title "Claude needs you", flash "own needs you" */ }
#[test] fn prompt_and_end_clear() { /* NeedsYou then Prompt → rows empty; Finished then End → rows empty */ }
#[test] fn finished_clears_on_seen_and_after_30_min() {}
#[test] fn finished_off_drops_stop() { /* finished_on=false → Finished ignored, no row */ }
#[test] fn quiet_events_never_notice() {}
#[test] fn same_project_two_sessions() { /* sessions "a","b" same cwd → 2 rows, attention 2 */ }
#[test] fn bad_payloads_are_skipped() { /* "", "{", r#"{"hook_event_name":"Stop"}"# (no session_id) → None */ }
#[test] fn take_files_order_age_and_cleanup() { /* temp dir: old (>1 day) deleted+ignored, x.tmp kept, pre-start-10min dropped, pre-start quiet, new loud; returned oldest first; .json files deleted */ }
```

- [ ] **Step 2: Run** `WINTEST events::` → FAIL.
- [ ] **Step 3: Implement** `events.rs` to pass. Rows newest first, max 5. `attention()` counts all rows.
- [ ] **Step 4: Wire up in `main.rs`.** Record `started = Utc::now()` at setup. A thread every 1 s: `take_files`, apply each under the `AppState` lock (`finished_on = settings.task_finished`), `expire(now)`; if anything changed, `emit`; for each `Notice` show a toast (`app.notification()`, same as `on_alert`) and put `flash` into the emitted `ViewModel`. New command `sessions_seen()` calls `Sessions::seen` and emits.
- [ ] **Step 5: Run** `WINTEST` → PASS.
- [ ] **Step 6: Commit** `Turn Claude Code hook events into session alerts and toasts`.

### Task 9: Show sessions in the notch and the Task alerts settings

**Files:**
- Modify: `src-tauri/src/view.rs` (`ViewModel` gains `sessions: Vec<SessionRow>`, `attention: usize`, `flash: Option<String>`), `src/index.html`, `src/notch.js`, `src/notch.css`

**Interfaces:**
- Consumes: Task 8 `SessionRow`, Task 7 `get_task_alerts` / `set_task_alerts`, `sessions_seen`, `Settings.task_finished`.

- [ ] **Step 1: Failing Rust test** `view_carries_sessions` (build with two rows → `attention == 2`, rows passed through; `flash` defaults `None`).
- [ ] **Step 2: Run** → FAIL; **Step 3:** implement; **Step 4:** `WINTEST` → PASS.
- [ ] **Step 5: Webview.** Load the design skills named in `~/own/CLAUDE.md` (antislop-ui, ui-ux-pro-max, make-interfaces-feel-better, accessibility) before markup; reuse existing tokens and classes, no new colours except reuse `--warn` for "needs you".
  - Pill: when `attention > 0`, append a Lucide `bell` symbol (`#i-bell`, 15 px, `aria-hidden`) and the count as a `.stat`. When a payload has `flash`, replace the pill content with the flash text for 6 s (newer flash restarts the timer), and announce it through `#announce`.
  - Panel: a `Sessions` block under the week limit, hidden when empty: one row per `SessionRow` with project, `needs you` (amber) or `finished`, and age from `since_ms` (`now`, `2m`, `1h`), refreshed every 30 s while open.
  - When the panel closes after having been open, call `sessions_seen`.
  - Settings: a `Task alerts` group: switch-style checkbox `Show when Claude needs you` bound to `get_task_alerts`/`set_task_alerts` (disabled with the hint when unavailable; error copy shown inline under it), and checkbox `Also when a task finishes` bound to `task_finished`.
- [ ] **Step 6: Check by hand in `cargo tauri dev`:** keyboard reaches every new control with a visible focus ring; Esc still closes; the window height follows the Sessions block; reduced motion has no flash animation beyond the text swap.
- [ ] **Step 7: Commit** `Show waiting and finished sessions in the notch and add Task alerts settings`.

### Task 10: README and end-to-end check

**Files:**
- Modify: `README.md` (acceptance checklist: new steps 21+ for task alerts and each follow-up; note "turn Task alerts off before uninstalling"; keep step 20 open)

- [ ] **Step 1:** Write the steps: switch on → `settings.json` gains 5 `claude-notch` entries and keeps Orca's; backup exists; in a WSL terminal ask Claude something needing permission → toast `Claude needs you` + pill flash + bell count; a reply > 1 min → `Claude finished` toast; a quick reply → row only; open/close panel clears finished; switch off → entries gone, file otherwise unchanged; restart while at 92 % → no repeat toast; Show on lists monitor names; usage poll does not create finished rows.
- [ ] **Step 2:** `WINTEST` full suite PASS; `cargo tauri build` through the PowerShell prefix succeeds with 0 warnings.
- [ ] **Step 3:** Run the new README steps that can run here; mark which ones need the owner.
- [ ] **Step 4: Commit** `Document task alerts and the follow-ups in the acceptance checklist`.
