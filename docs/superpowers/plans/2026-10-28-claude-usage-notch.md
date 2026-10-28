# Claude Usage Notch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Windows tray app that shows Claude session and weekly usage in a black notch at the top of the screen, with alerts, notifications, pace tick, run-out time, full-screen hiding and screen/position settings.

**Architecture:** Tauri v2 app. All logic (parsing, pace maths, alert rules, settings, source detection, placement, full-screen check, view strings) lives in small pure Rust modules with `cargo test` unit tests; `main.rs` wires them to a refresh loop, Tauri commands/events, tray, notifications and the window. The webview is plain HTML/CSS/JS (no bundler, no npm build) that only renders a `ViewModel` event and calls commands.

**Tech Stack:** Rust (stable, MSVC), Tauri 2 + `tauri-cli` 2, `serde`/`serde_json`, `chrono`, `windows` crate (Win32), `tauri-plugin-notification`, `tauri-plugin-autostart`; Public Sans font files bundled; Lucide icons inline SVG.

**Spec:** `docs/superpowers/specs/2026-10-28-monitor-claude-notch-design.md` (read it with this plan). Visual reference: `docs/design/notch-mockup.html`.

## Global Constraints

- Never open `~/.claude/.credentials.json`, never call `api.anthropic.com` or `claude.ai`, never store a token or cookie. The Claude CLI does all fetching.
- Build and run on the Windows host. Working copy lives at `C:\Users\hanif\code\notch` (`/mnt/c/Users/hanif/code/notch` from WSL); all `cargo` commands run there with the Windows toolchain. Repo-local git email: `173446283+hanifmuhd@users.noreply.github.com`.
- Refresh every 5 minutes and from the refresh button; never more than one refresh in flight; CLI call timeout 60 s.
- Thresholds: session and week separately, 50–99 %, default 90 %. Full is always 100 %, not configurable.
- Session window 5 h; week window 7 days.
- Toasts: once when a limit crosses its threshold, once when it becomes full; only a window reset re-arms; lowering a threshold below the current value fires once; raising it above clears silently.
- Status-line fallback (only if S1 fails): reading older than 10 minutes is stale.
- Placement: Centre default; Left = 8 px × scale from the left edge; Right = 140 px × scale gap from the right edge. Default screen = primary; missing screen falls back to primary.
- Full-screen check polled every 1 s; hide only when the foreground window covers the notch's monitor; exclude window classes `Progman`, `WorkerW`, `Shell_TrayWnd` and the notch itself.
- Window: frameless, transparent, always on top, skip taskbar, no focus on show; no global hotkey in v1.
- UI copy and tokens exactly as the mockup and spec §5.2–5.4, §5.9 (`#000000`, `#ffffff`, `#8a8c92`, `#3a3a3e`, `#d6d7da`, `#4a8ad6`, `#d9a441`, `#e5566f`, `#1e1e21`); Public Sans bundled, `font-variant-numeric: tabular-nums`; hover close delay 150 ms; open 180 ms, close 120 ms; reduced motion = fade only.
- Ship as Tauri NSIS installer, per-user, unsigned.

## Review Focus

1. CLI prints warnings or extra lines around the JSON (or WSL prints a banner) → parser still finds the JSON object; covered by `parse_tolerates_noise` in Task 4.
2. Reset time already in the past (window just reset, clock skew) → elapsed clamps to 100 %, no run-out time, reset text `Resets now`; covered by `elapsed_clamps_when_reset_passed` (Task 5) and `reset_text_past_is_now` (Task 9).
3. `used_pct` of 0 or above 100 → no run-out time at 0, clamp to 100 and treat as full above 100; covered by `runs_out_none_when_unused` (Task 5) and `over_100_is_full` (Task 6).
4. Settings file missing, empty or corrupt → defaults, app starts, file rewritten on next save; covered by `load_corrupt_returns_defaults` (Task 7).
5. CLI hangs (WSL booting, login prompt) → refresh times out after 60 s and goes stale, next tick tries again; covered by `runner_timeout_is_error` (Task 8).

---

### Task 1: Windows toolchain and empty notch window

**Files:**
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/icons/*` (from `cargo tauri icon`), `src/index.html`
- Modify: `.gitignore` (add `src-tauri/target/`, `src-tauri/gen/`)

**Interfaces:**
- Produces: window label `notch`; `frontendDist: "../src"`; `app.withGlobalTauri: true` (JS uses `window.__TAURI__`).

- [ ] **Step 1: Install tools (owner runs, in Windows PowerShell)**
  - Visual Studio Build Tools 2022 with workload "Desktop development with C++": `winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`
  - Rust: `winget install Rustlang.Rustup`, then new PowerShell: `rustup default stable-msvc`
  - Tauri CLI: `cargo install tauri-cli --version "^2" --locked`
  - Expected: `cargo --version`, `cargo tauri --version` (2.x) both print.
- [ ] **Step 2: Clone on Windows and set the no-reply email**
  `git clone https://github.com/hanifmuhd/notch.git $env:USERPROFILE\code\notch; cd $env:USERPROFILE\code\notch; git config user.email 173446283+hanifmuhd@users.noreply.github.com`
  Expected: `git config user.email` prints the no-reply address. From here on, work only in this copy (`~/own/notch-project` is retired; leave a one-line `MOVED.md` pointing here or delete it after the owner agrees).
- [ ] **Step 3: Create the app skeleton** with window `notch`: `decorations: false`, `transparent: true`, `alwaysOnTop: true`, `skipTaskbar: true`, `focus: false`, `resizable: false`, size 220×36, `visible: true`. `src/index.html` shows a black 220×36 box with `Session –   Week –`.
- [ ] **Step 4: Run** `cargo tauri dev` in `src-tauri`'s parent. Expected: a black box appears on top of other windows; Ctrl+C stops it.
- [ ] **Step 5: Commit** `Add Tauri skeleton with an always-on-top notch window`.

### Task 2: Spike S1/S1b — what `/usage` returns

**Files:**
- Create: `docs/spikes/usage-output.md`, `src-tauri/tests/fixtures/usage-wsl.txt`, `src-tauri/tests/fixtures/usage-windows.txt` (only if Claude Code on Windows exists)

- [ ] **Step 1: Capture raw output, warm WSL:** `wsl.exe -d Ubuntu -- bash -lc "claude -p --output-format json /usage" > src-tauri\tests\fixtures\usage-wsl.txt; Measure-Command { wsl.exe -d Ubuntu -- bash -lc "claude -p --output-format json /usage" }`. Also try without `bash -lc` (`wsl.exe -d Ubuntu -- claude ...`) and record whether `claude` is found.
- [ ] **Step 2: Cold timing:** `wsl --shutdown`, then the same `Measure-Command`. Record both times.
- [ ] **Step 3: Quota cost:** note session % from `/usage` in an interactive Claude Code, run Step 1's command 10 times, check again. Record the difference.
- [ ] **Step 4: Write `docs/spikes/usage-output.md`** with: exact working command, the JSON shape (field names and where session %, session reset, week %, week reset live; timestamp format), an **Expected values** table read off the fixture (session %, session reset, week %, week reset), timings, quota cost, verdict.
- [ ] **Step 5: Decide.** Pass = parseable, both limits and resets present, quota change ≤ 1 %. **If it fails, stop and tell the owner**: the plan switches to the status-line fallback (spec §4 approach 2), and Tasks 4, 8 and 10 are re-planned before continuing.
- [ ] **Step 6: Commit** `Record what claude /usage returns from Windows`.

### Task 3: Spike S2/S3 — full-screen and monitors

**Files:**
- Create: `src-tauri/examples/fullscreen_probe.rs`, `docs/spikes/fullscreen.md`
- Modify: `src-tauri/Cargo.toml` (`windows` crate features `Win32_Foundation`, `Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_UI_Shell`)

- [ ] **Step 1: Write the probe**: every 1 s print foreground window class, its rect, its monitor's device name and rect, "covers=true/false", and `SHQueryUserNotificationState` value; at start print all monitors (`EnumDisplayMonitors`) with device name, rect, primary flag.
- [ ] **Step 2: Run** `cargo run --example fullscreen_probe` and fill this matrix in `docs/spikes/fullscreen.md` (expected covers): a maximised window (false), F11 browser (true), YouTube full screen in Edge (true), a game full screen (true), desktop clicked (false: class `Progman`/`WorkerW`), each on screen 1 and screen 2 if available.
- [ ] **Step 3: Compare monitor names** with Windows Settings → Display numbering; record which name to show in "Show on" (spec S3).
- [ ] **Step 4: Commit** `Record full-screen and monitor probe results`.

### Task 4: Usage parser

**Files:**
- Create: `src-tauri/src/usage.rs`; Modify: `src-tauri/src/main.rs` (`mod usage;`)

**Interfaces:**
- Produces: `pub struct Limit { pub used_pct: f64, pub resets_at: DateTime<Utc> }`, `pub struct UsageReading { pub session: Limit, pub week: Limit, pub fetched_at: DateTime<Utc> }`, `pub fn parse_usage(raw: &str, fetched_at: DateTime<Utc>) -> Result<UsageReading, UsageError>`, `pub enum UsageError { NoJson, MissingField(&'static str), BadTime(String) }`.

- [ ] **Step 1: Write failing tests** in `usage.rs` `#[cfg(test)]`:
  - `parses_wsl_fixture`: `parse_usage(include_str!("../tests/fixtures/usage-wsl.txt"), t0)` equals the Expected values table in `docs/spikes/usage-output.md` (assert each of the four values).
  - `parse_tolerates_noise`: same fixture with `"wsl: banner line\n"` prepended and `"\ntrailing"` appended parses to the same values.
  - `accepts_epoch_and_iso_reset`: the same reading with `resets_at` as epoch seconds and as ISO-8601 gives equal `resets_at`.
  - `missing_week_is_error`: fixture with the week object removed → `Err(UsageError::MissingField("week"))`.
  - `empty_is_no_json`: `parse_usage("", t0)` → `Err(UsageError::NoJson)`.
- [ ] **Step 2: Run** `cargo test usage` → FAIL (not defined).
- [ ] **Step 3: Implement**: find the first `{` … matching last `}` and `serde_json::from_str` into `serde_json::Value`; read the field paths recorded in Task 2.
- [ ] **Step 4: Run** `cargo test usage` → PASS.
- [ ] **Step 5: Commit** `Parse claude /usage output into session and week limits`.

### Task 5: Pace and run-out maths

**Files:** Create `src-tauri/src/pace.rs`

**Interfaces:**
- Produces: `pub const SESSION_WINDOW: Duration` (5 h), `pub const WEEK_WINDOW: Duration` (7 d), `pub fn elapsed_pct(window: Duration, now: DateTime<Utc>, resets_at: DateTime<Utc>) -> f64`, `pub fn runs_out_at(used_pct: f64, window: Duration, now: DateTime<Utc>, resets_at: DateTime<Utc>) -> Option<DateTime<Utc>>`.

- [ ] **Step 1: Write failing tests:**
  - `session_elapsed_matches_mockup`: resets in 2 h 13 m → `elapsed_pct` ≈ 55.67 (mockup shows 56 %).
  - `week_elapsed_matches_mockup`: now Wed 10:51, resets Fri 09:00 same week → ≈ 72.5.
  - `elapsed_clamps_when_reset_passed`: `resets_at < now` → 100.0; `resets_at > now + window` → 0.0.
  - `runs_out_when_ahead`: used 80 %, elapsed 50 % of 5 h (2 h 30 m in) → `Some(now + 37.5 min)` (rate 32 %/h, 20 % left).
  - `runs_out_none_when_behind`: used 42, elapsed 56 → `None`.
  - `runs_out_none_after_reset`: projected moment later than `resets_at` → `None`.
  - `runs_out_none_when_unused`: used 0 → `None`.
- [ ] **Step 2: Run** `cargo test pace` → FAIL.
- [ ] **Step 3: Implement** per spec §5.3: elapsed = (window − (resets_at − now)) / window, clamped 0–100; run-out = now + (100 − used) × (elapsed time / used), only if used > elapsed % and before `resets_at`.
- [ ] **Step 4: Run** `cargo test pace` → PASS.
- [ ] **Step 5: Commit** `Add pace tick and run-out time calculations`.

### Task 6: Alert rules

**Files:** Create `src-tauri/src/alerts.rs`

**Interfaces:**
- Consumes: `UsageReading`, `Limit` (Task 4).
- Produces: `pub enum LimitKind { Session, Week }`, `pub enum Level { Normal, Alert, Full }`, `pub struct Thresholds { pub session: u8, pub week: u8 }`, `pub struct AlertState { session: Armed, week: Armed }` (private `Armed { fired_alert: bool, fired_full: bool, resets_at: DateTime<Utc> }`), `pub enum AlertEvent { Crossed(LimitKind, f64, DateTime<Utc>), Full(LimitKind, DateTime<Utc>) }`, `pub fn level(used_pct: f64, threshold: u8) -> Level`, `pub fn evaluate(state: &AlertState, reading: &UsageReading, t: &Thresholds) -> (AlertState, Vec<AlertEvent>)`, `impl Default for AlertState`.

- [ ] **Step 1: Write failing tests:**
  - `crossing_fires_once`: 85 → 91 (threshold 90) gives one `Crossed(Session, 91.0, _)`; 91 → 93 gives none.
  - `full_fires_once`: 99 → 100 gives `Full(Session, _)`; 100 → 100 none.
  - `reset_rearms`: after firing, a reading with a later `resets_at` and 92 % fires `Crossed` again.
  - `lower_threshold_fires`: at 70 %, threshold 90 → 60 fires once.
  - `raise_threshold_clears_silently`: at 92 % fired, threshold → 95 gives no event and `level` Normal.
  - `over_100_is_full`: `level(104.0, 90)` → `Full`.
  - `week_independent`: session firing does not affect week state.
- [ ] **Step 2: Run** `cargo test alerts` → FAIL.
- [ ] **Step 3: Implement** with the rules in Global Constraints.
- [ ] **Step 4: Run** `cargo test alerts` → PASS.
- [ ] **Step 5: Commit** `Add alert threshold and notification rules`.

### Task 7: Settings

**Files:** Create `src-tauri/src/settings.rs`

**Interfaces:**
- Produces: `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct Settings { pub session_threshold: u8, pub week_threshold: u8, pub position: Position, pub screen: Option<String>, pub source: SourcePref, pub start_wsl: bool, pub show_notch: bool }` with `#[serde(default)]`; `pub enum Position { Left, Centre, Right }`; `pub enum SourcePref { Auto, Windows, Wsl { distro: String } }`; `impl Default` (90, 90, Centre, None, Auto, true, true); `pub fn load(path: &Path) -> Settings`; `pub fn save(path: &Path, s: &Settings) -> io::Result<()>`; `pub fn clamp(s: Settings) -> Settings` (thresholds to 50..=99).

- [ ] **Step 1: Write failing tests:** `load_missing_returns_defaults`, `load_corrupt_returns_defaults` (file contains `{not json`), `round_trip` (save then load equal), `clamp_thresholds` (10 → 50, 100 → 99), `unknown_fields_ignored` (old file with an extra key loads).
- [ ] **Step 2: Run** `cargo test settings` → FAIL.
- [ ] **Step 3: Implement**; `save` writes to `settings.json.tmp` then renames (no half-written file). Path at runtime: `app.path().app_config_dir()/settings.json`.
- [ ] **Step 4: Run** `cargo test settings` → PASS.
- [ ] **Step 5: Commit** `Add settings with defaults and safe saving`.

### Task 8: Finding Claude Code and running it

**Files:** Create `src-tauri/src/source.rs`

**Interfaces:**
- Consumes: `SourcePref` (Task 7).
- Produces: `pub enum Source { Windows, Wsl { distro: String }, NotFound }`; `pub trait Runner { fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<String, RunError>; }`; `pub enum RunError { Timeout, Exit(i32, String), Spawn(String) }`; `pub struct SystemRunner;` (uses `std::process::Command` with `CREATE_NO_WINDOW`, kills the child on timeout); `pub fn detect(runner: &dyn Runner, pref: &SourcePref) -> Source`; `pub fn usage_command(source: &Source) -> (String, Vec<String>)`; `pub fn wsl_running(runner: &dyn Runner, distro: &str) -> bool`; `pub const CLI_TIMEOUT: Duration` (60 s).

- [ ] **Step 1: Write failing tests** with a `FakeRunner` mapping command lines to results:
  - `prefers_windows_when_present`: `where claude` succeeds → `Source::Windows`.
  - `falls_back_to_wsl_distro`: `where` fails; `wsl.exe -l -q` returns `"Ubuntu\r\n"` (UTF-16 output decoded) and `claude` exists there → `Wsl { distro: "Ubuntu" }`.
  - `not_found`: both fail → `NotFound`.
  - `pref_overrides_auto`: `SourcePref::Wsl{distro:"Debian"}` → `Wsl{distro:"Debian"}` without probing Windows.
  - `wsl_command_shape`: `usage_command(&Wsl{distro:"Ubuntu"})` equals the exact command recorded in Task 2 Step 4.
  - `runner_timeout_is_error`: `SystemRunner.run("powershell", &["-c","Start-Sleep 5"], 1s)` → `Err(RunError::Timeout)` within ~1.5 s.
  - `wsl_running_parses_list`: `wsl.exe -l --running -q` output containing `Ubuntu` → true; empty → false.
- [ ] **Step 2: Run** `cargo test source` → FAIL.
- [ ] **Step 3: Implement.** `wsl.exe -l` prints UTF-16LE: decode before matching.
- [ ] **Step 4: Run** `cargo test source` → PASS.
- [ ] **Step 5: Commit** `Detect Claude Code on Windows or WSL and run it with a timeout`.

### Task 9: View model and copy

**Files:** Create `src-tauri/src/view.rs`

**Interfaces:**
- Consumes: Tasks 4–7.
- Produces: `#[derive(Serialize)] pub struct ViewModel { pub pill: PillState, pub session: LimitView, pub week: LimitView, pub updated_text: String, pub refreshing: bool }`; `pub enum PillState { NoClaude, NoData, Normal, Alert, Full, Stale }`; `pub struct LimitView { pub value_text: String, pub used_pct: f64, pub pace_pct: f64, pub level: Level, pub reset_text: String, pub pace_text: String, pub runs_out_text: Option<String> }`; `pub fn build(reading: Option<&UsageReading>, last_ok: bool, source: &Source, settings: &Settings, now: DateTime<Local>) -> ViewModel`; `pub fn reset_text(now, resets_at) -> String`.

- [ ] **Step 1: Write failing tests** (exact copy from spec/mockup):
  - `reset_text_session`: 2 h 13 m ahead → `"Resets in 2h 13m"`; under 1 h → `"Resets in 45m"`.
  - `reset_text_week`: more than 24 h ahead, Fri 09:00 → `"Resets Fri 09:00"`.
  - `reset_text_past_is_now`: past → `"Resets now"`.
  - `pace_text`: 55.67 → `"56% of time passed"`.
  - `runs_out_text`: `Some` time 15:40 → `"Runs out around 15:40"`.
  - `stale_state_and_text`: `last_ok=false` with a reading fetched 10:51 → `PillState::Stale`, `updated_text == "As of 10:51"`.
  - `full_beats_alert_beats_stale`: session 100, week 95, stale → `Full` (priority Full > Alert > Stale > Normal per spec §5.2).
  - `no_claude`: `Source::NotFound` → `PillState::NoClaude` and `updated_text == "Install Claude Code to see your limits"` (the UI links it to the official Claude Code install page).
  - `no_data`: `None` reading → `NoData`, values `"–"`.
- [ ] **Step 2: Run** `cargo test view` → FAIL.
- [ ] **Step 3: Implement** using local time for display.
- [ ] **Step 4: Run** `cargo test view` → PASS.
- [ ] **Step 5: Commit** `Build the notch view model and its text`.

### Task 10: Refresh loop, commands and events

**Files:** Create `src-tauri/src/app_state.rs`; Modify `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: Tasks 4–9.
- Produces: Tauri event `usage-updated` (payload `ViewModel`); commands `get_view() -> ViewModel`, `refresh_now()`, `get_settings() -> Settings`, `save_settings(settings: Settings) -> ViewModel`, `list_screens() -> Vec<ScreenInfo>` (`ScreenInfo { id: String, label: String, primary: bool }`, label from Task 3's naming), `set_panel_open(open: bool)`, `focus_previous()`; internal `fn on_alert(events: Vec<AlertEvent>)` (hook for Task 14).

- [ ] **Step 1: Write failing test** `single_flight` in `app_state.rs`: two `try_start_refresh()` calls without `finish_refresh()` → first `true`, second `false`.
- [ ] **Step 2: Run** `cargo test app_state` → FAIL; implement `AppState` (`Mutex` with last reading, last_ok, alert state, settings, source, in-flight flag); run → PASS.
- [ ] **Step 3: Wire the loop**: on start load settings, detect source, refresh immediately, then every 5 minutes on Tauri's async runtime (run the CLI in `spawn_blocking`). If the source is WSL and `start_wsl` is false and `wsl_running` is false, skip the call and mark stale. After each refresh: `alerts::evaluate`, `on_alert`, emit `usage-updated`.
- [ ] **Step 4: Manual check** with `cargo tauri dev` and a temporary `console.log` of the payload in devtools: numbers match `/usage`; stopping WSL with the setting off leads to stale within one tick.
- [ ] **Step 5: Commit** `Refresh usage on a schedule and send it to the notch`.

### Task 11: Notch UI in the webview

**Files:** Create `src/notch.css` (tokens and rules from `docs/design/notch-mockup.html`), `src/notch.js`, `src/fonts/PublicSans-{400,500,600}.woff2` (+ OFL licence); Modify `src/index.html`

**Interfaces:**
- Consumes: event `usage-updated`, commands from Task 10.

- [ ] **Step 1: Port the mockup** markup for pill, panel, usage view and settings view; drop the demo sheet, Google Fonts link and sample numbers; `@font-face` points at `src/fonts`.
- [ ] **Step 2: Render** `ViewModel` into the DOM (values, bar widths, pace tick `left`, state classes `is-warn`/`is-danger`/`is-stale`, alert icon colour, runs-out line, `Updated …`/`Updating…`). One `role="status"` message only when a toast-worthy event happened (payload flag), never every refresh.
- [ ] **Step 3: Settings view** adds, in mockup style above the sliders: `Show on` (select from `list_screens`), `Position` (Left / Centre / Right segmented, selected one with a 1px `#4a8ad6` line under its label), `Get usage from` (Auto / Windows / WSL distro), `Start WSL if it's stopped` (checkbox, only for WSL). Every change calls `save_settings`. Hint line stays: `Saved as you change it. Full always alerts at 100%.`
- [ ] **Step 4: Open/close**: hover opens, leave closes after 150 ms unless click-pinned; Enter/Space toggle; Esc closes, then `focus_previous`. Opening calls `set_panel_open(true)` before showing the panel; closing calls it with `false` after the 120 ms fade.
- [ ] **Step 5: Run the lints** on `src/index.html`: `node <hallmark>/scripts/sloplint.mjs src/index.html --scope component` (0 FAIL) and `node <taste-skill>/scripts/lint.mjs src` (0 FAIL).
- [ ] **Step 6: Manual check** with `cargo tauri dev`: each state from the mockup appears with real data or by temporarily editing thresholds; keyboard path Tab → gear → sliders works after one click on the notch.
- [ ] **Step 7: Commit** `Render the notch, panel and settings from live data`.

### Task 12: Placement and screens

**Files:** Create `src-tauri/src/placement.rs`; Modify `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: `Position`, `Settings.screen` (Task 7), `set_panel_open` (Task 10).
- Produces: `pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }`, `pub fn notch_origin(monitor: Rect, scale: f64, pos: Position, width: i32) -> (i32, i32)`, `pub fn pick_monitor<'a>(screens: &'a [(String, Rect, bool)], wanted: Option<&str>) -> &'a (String, Rect, bool)`.

- [ ] **Step 1: Write failing tests:** `centre_on_1920` (monitor 0,0,1920×1080, width 220 → x 850, y 0), `left_gap_scaled` (scale 1.5 → x = 12), `right_gap_scaled` (scale 1.25, width 220 → x = 1920 − 175 − 220 = 1525), `second_monitor_offset` (monitor at x 1920 → centre x 2770), `missing_screen_falls_back_to_primary`.
- [ ] **Step 2: Run** `cargo test placement` → FAIL; implement; run → PASS.
- [ ] **Step 3: Wire**: closed window = pill size, open = panel size (320 wide), both anchored at the same top-centre point of the notch; recompute on `save_settings` and on Tauri's scale/monitor change events.
- [ ] **Step 4: Manual check** on two monitors if available: each Position on each screen; unplug the chosen screen → notch moves to primary.
- [ ] **Step 5: Commit** `Place the notch on the chosen screen and position`.

### Task 13: Hide in full screen

**Files:** Create `src-tauri/src/fullscreen.rs`; Modify `src-tauri/src/main.rs`

**Interfaces:**
- Consumes: `Rect` (Task 12), probe findings (Task 3).
- Produces: `pub fn covers_monitor(window: Rect, monitor: Rect) -> bool`, `pub const IGNORED_CLASSES: [&str; 3]`, `#[cfg(windows)] pub fn foreground_fullscreen_on(monitor_device: &str, own_hwnd: isize) -> bool`.

- [ ] **Step 1: Write failing tests:** `exact_cover_true`, `bigger_than_monitor_true` (borderless windows often overhang by a few px), `maximised_with_taskbar_false` (height = monitor − 48), `other_monitor_false`.
- [ ] **Step 2: Run** `cargo test fullscreen` → FAIL; implement `covers_monitor`; run → PASS.
- [ ] **Step 3: Wire** a 1 s loop: if `foreground_fullscreen_on(notch monitor)` hide the window, else show it (when `show_notch` is true). On a single-monitor setup only, also hide when `SHQueryUserNotificationState` returns `QUNS_RUNNING_D3D_FULL_SCREEN` (3) (spec §5.6 hint). Toasts keep working while hidden.
- [ ] **Step 4: Manual check** with Task 3's matrix: hides for F11, YouTube full screen and a game on the notch's screen; stays for maximised windows and for full screen on the other screen.
- [ ] **Step 5: Commit** `Hide the notch while a full-screen app is on its screen`.

### Task 14: Notifications, tray and start with Windows

**Files:** Modify `src-tauri/src/main.rs`, `src-tauri/Cargo.toml`, `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: `on_alert` (Task 10), `AlertEvent` (Task 6), `reset_text` (Task 9).

- [ ] **Step 1: Write failing test** `toast_text` in `view.rs`: `Crossed(Session, 91.0, resets in 1h 30m)` → title `"Session at 91%"`, body `"Resets in 1h 30m"`; `Full(Week, Fri 09:00)` → `"Week limit full"`, `"Back Fri 09:00"`. Implement `pub fn toast_text(e: &AlertEvent, now) -> (String, String)`; run → PASS.
- [ ] **Step 2: Notifications**: add `tauri-plugin-notification`; `on_alert` shows one toast per event.
- [ ] **Step 3: Tray**: icon with tooltip built from the view model, e.g. `Session 42%, week 68%`, menu `Show notch`/`Hide notch`, `Start with Windows` (checkbox, `tauri-plugin-autostart`, default off), `Refresh now`, `Quit`.
- [ ] **Step 4: Manual check**: lower a threshold below current usage → one toast; tray items all work; autostart survives a sign-out/sign-in.
- [ ] **Step 5: Commit** `Add alert notifications, tray menu and start with Windows`.

### Task 15: Installer and acceptance

**Files:** Modify `src-tauri/tauri.conf.json` (`bundle.targets: ["nsis"]`, `bundle.windows.nsis.installMode: "currentUser"`, product name `Claude Notch`); Create `README.md`

- [ ] **Step 1: Build** `cargo tauri build`. Expected: `src-tauri/target/release/bundle/nsis/*.exe` exists.
- [ ] **Step 2: Install** it as the owner (SmartScreen: More info → Run anyway); run through the spec's goals list (§2) and states (§5.2) once each; note any failure as an issue before release.
- [ ] **Step 3: README**: what it does, needs Claude Code (Windows or WSL) installed and logged in, install steps incl. SmartScreen, settings, privacy line ("never reads your Claude login; only runs `claude /usage`").
- [ ] **Step 4: Commit** `Package the notch as a per-user installer and document it`; push to `origin main` only when the owner asks.
