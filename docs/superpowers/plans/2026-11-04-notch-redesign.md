# Notch Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The owner chose **native** execution (superpowers:executing-plans).

**Goal:** Restyle the notch to concentric rings with a countdown, smooth spring motion, a session pod with a heartbeat hook, hover intent, a Top/Taskbar setting, Stay behind windows, and a ring app icon (Windows only).

**Architecture:** The notch window becomes a fixed-size transparent window; the webview animates the black shape with CSS and reports its visible rectangles, and a Rust pointer loop turns mouse input on only over those rectangles. Rust keeps computing all text and state (view model); the webview draws rings from percentages. A new `taskbar.rs` measures the taskbar gap and decides full / mini / top.

**Tech Stack:** Tauri v2, Rust (chrono, serde_json, `windows` 0.62 incl. UI Automation), plain HTML/CSS/JS, Python + Pillow for icon rasterising (dev only).

**Spec:** `docs/superpowers/specs/2026-11-04-notch-redesign-design.md` (mockups in `docs/design/redesign/`).

## Global Constraints

- Windows only this release (owner).
- NEVER read Claude's token, `.credentials.json` or cookies; data only from the official `claude` command and hook stdin (owner hard rule).
- Tokens (exact): notch `#000`; ring track `#ffffff24`; session ring `#f3f1ee`; week ring `#9fb8ff`; amber `#f0a541`; red `#ff6b6b`; muted text `#f3f1eebf`; icon square `#121214`.
- Font: Manrope 500/600/700 woff2 bundled under `src/fonts/` (OFL), `font-variant-numeric: tabular-nums`; Public Sans removed.
- Shape: shoulders radius 14 px; closed height 32 px, bottom radius 16 px; open bottom radius 30 px.
- Motion: size spring `cubic-bezier(.34,1.4,.55,1)` ≈500 ms; content fades ≈250 ms with `blur(6px)`; `prefers-reduced-motion`: no spring, no blur, no breathing (still amber 1.5 px outline), fades ≤120 ms.
- Hover intent: open after the pointer rests 300 ms; click / Enter open at once; Esc closes.
- Countdown copy: ≥1 h `h:mm` (`1:32`); <1 h `45m`; past `now`; idle `ready`; full `back in 1:32`. Panel lines: `in 1h 32m`, beyond 24 h `in 3d 16h`.
- Heartbeat: hooked events become `Notification`, `PermissionRequest`, `Stop`, `UserPromptSubmit`, `SessionEnd`, `PostToolUse`; working session dropped after 10 min without events.
- Copy (exact): `Turn on Task alerts to see sessions`; settings labels `Show in`, `Top of screen`, `Taskbar`, `Stay behind windows`.
- No new runtime crates except `windows` features; CSP unchanged (`font-src 'self'`).
- Tests: Rust only via PowerShell (plan v1.1 `WINTEST`): `powershell.exe -NoProfile -Command '$env:Path = [Environment]::GetEnvironmentVariable("Path","User") + ";" + [Environment]::GetEnvironmentVariable("Path","Machine"); $env:CARGO_TARGET_DIR = "C:\Users\hanif\code\notch-target"; Set-Location \\wsl.localhost\Ubuntu\home\hanif\own\notch-project\src-tauri; cargo test'`. Visual checks through the DevTools protocol (WebView2 `--remote-debugging-port=9222`, Windows node `--experimental-websocket`); the owner's running notch must be restarted after any check.
- Design skills (owner's CLAUDE.md) are loaded before the webview tasks (Task 5): `antislop:antislop` + `antislop-ui`, `ui-ux-pro-max:ui-ux-pro-max`, `ecc:make-interfaces-feel-better`, `ecc:accessibility`, `impeccable:impeccable` (polish/harden + critique); name them in the report.
- Branch `build/redesign`; stage only files you changed.

## Review Focus

1. Pointer over the transparent part of the big window must reach the app below at all times, including mid-animation and right after closing. → Task 3 test `hit_test_rects` + live CDP/cursor check.
2. A session that is interrupted (Esc), crashes, or whose terminal closes must not keep a pod forever. → Task 2 test `working_expires_after_10_min`.
3. Approving a permission must stop the amber glow without waiting for the reply to end. → Task 2 test `heartbeat_clears_needs_you`.
4. Taskbar so full that even rings-only do not fit must never cover an app icon. → Task 7 test `fit_decision`.
5. Existing installs with task alerts on must gain the new hook without the owner touching the switch, and keep foreign hooks. → Task 2 test `reinstall_adds_post_tool_use`.

---

### Task 1: Countdown and limit texts

**Files:** Modify `src-tauri/src/view.rs` (LimitView, new pure fns, tests).

**Interfaces:**
- Produces: `pub fn countdown_text(now: DateTime<Local>, resets_at: Option<DateTime<Utc>>) -> String`; `pub fn span_text(now: DateTime<Local>, resets_at: DateTime<Utc>) -> String`; `LimitView` gains `countdown_text: String` (closed notch) and `span_text: String` (panel line, `""` when idle).

- [ ] **Step 1: Failing tests**
```rust
#[test] fn countdown_formats() {
    let now = local(28, 10, 0);
    assert_eq!(countdown_text(now, Some(utc(now) + Duration::minutes(92))), "1:32");
    assert_eq!(countdown_text(now, Some(utc(now) + Duration::minutes(45))), "45m");
    assert_eq!(countdown_text(now, Some(utc(now) + Duration::seconds(30))), "1m");
    assert_eq!(countdown_text(now, Some(utc(now) - Duration::minutes(1))), "now");
    assert_eq!(countdown_text(now, None), "ready");
    assert_eq!(countdown_text(now, Some(utc(now) + Duration::minutes(600))), "10:00");
}
#[test] fn span_formats() {
    let now = local(28, 10, 0);
    assert_eq!(span_text(now, utc(now) + Duration::minutes(92)), "in 1h 32m");
    assert_eq!(span_text(now, utc(now) + Duration::hours(88)), "in 3d 16h");
}
#[test] fn limit_view_carries_countdown() { /* limit_view session 68% resets +92m → countdown_text "1:32", span_text "in 1h 32m"; idle → "ready", "" */ }
```
- [ ] **Step 2:** `WINTEST view::` → FAIL (missing fns/fields).
- [ ] **Step 3:** Implement; minutes round up like `when_text`. Full pill text becomes `back in {countdown}` (keep `PillState::Full`).
- [ ] **Step 4:** `WINTEST` → PASS (fix the existing full-pill test to the new copy).
- [ ] **Step 5:** Commit `Add a session countdown and short reset spans to the view`.

### Task 2: Working sessions, heartbeat and the pod

**Files:** Modify `src-tauri/src/events.rs`, `src-tauri/src/hooks.rs` (EVENTS), `src-tauri/src/app_state.rs`, `src-tauri/src/view.rs` (ViewModel.pod), `src-tauri/src/main.rs` (re-install at start).

**Interfaces:**
- Produces: `Kind::Heartbeat` (from `PostToolUse`); `Kind::Prompt` now creates/refreshes a **working** entry; `Status::Working`; `Sessions::pod(&self) -> Option<Pod>` with `#[derive(Serialize)] pub struct Pod { project: String, status: &'static str /*"working"|"needs_you"*/, since_ms: i64, more: usize }`; `SessionRow.status` may be `"working"`; `ViewModel.pod: Option<Pod>`; `hooks::EVENTS` has 6 entries ending with `"PostToolUse"`.

- [ ] **Step 1: Failing tests** (events.rs unless noted)
```rust
#[test] fn prompt_makes_working() { /* Prompt → rows()[0].status == "working", pod().unwrap().status == "working" */ }
#[test] fn heartbeat_clears_needs_you() { /* Prompt, NeedsYou, Heartbeat → status "working", no Notice from heartbeat */ }
#[test] fn heartbeat_unknown_session_is_ignored() { /* Heartbeat for unseen id → no row */ }
#[test] fn working_expires_after_10_min() { /* Prompt at t; expire(t+9m59s) keeps; expire(t+10m) drops; a Heartbeat at t+9m pushes it to t+19m */ }
#[test] fn pod_prefers_needs_you_and_counts_more() { /* a working, b needs-you, c working → pod.project == b, more == 2; finished rows never become the pod */ }
#[test] fn post_tool_use_parses_as_heartbeat() { /* {"hook_event_name":"PostToolUse","session_id":"s","cwd":"/x/p"} → Kind::Heartbeat */ }
// hooks.rs
#[test] fn reinstall_adds_post_tool_use() { /* install with the old 5-event JSON present (+ an Orca PostToolUse entry) → 6 claude-notch entries, Orca entry kept, no duplicates */ }
```
- [ ] **Step 2:** `WINTEST events:: hooks::` → FAIL.
- [ ] **Step 3:** Implement. Long-reply rule keeps using the prompt time. Needs-you dedupe unchanged. `expire` drops working entries whose last event (seen-time) is ≥10 min old. At app start, if the current install `is_installed`, call `apply(..., on=true, ...)` on a blocking thread (idempotent; adds `PostToolUse`). `attention()` counts needs-you + finished only (working is not "a look needed") — update `view_carries_sessions` accordingly.
- [ ] **Step 4:** `WINTEST` → PASS.
- [ ] **Step 5:** Commit `Track working Claude sessions with a heartbeat hook and pick a pod`.

### Task 3: Fixed window with pointer hit-testing (spike S1, then real)

**Files:** Create `src-tauri/src/hit.rs`; modify `src-tauri/src/main.rs` (window size, remove resize-on-open, pointer loop, new command), `src-tauri/src/placement.rs`, `src-tauri/tauri.conf.json` (initial size), `src/notch.js` (report rects; stop calling `set_panel_open`).

**Interfaces:**
- Produces: `#[derive(Deserialize, Clone, Copy)] pub struct HitRect { x: f64, y: f64, w: f64, h: f64 }` (CSS px, window-relative); `pub fn hit(rects: &[HitRect], px: f64, py: f64) -> bool`; command `set_hit_rects(rects: Vec<HitRect>)`; constants `WINDOW_TOP: (f64, f64) = (860.0, 340.0)` logical px (open panel 340 + shoulders + pod room each side + glow margin). `placement::notch_origin` keeps its signature; the window is centred on the notch's x for Position Centre/Left/Right (Left/Right keep the notch's own edge gaps by offsetting the window so the shape, not the window, sits at the gap).
- Removes: `set_panel_open` (and `CLOSED_SIZE`, `OPEN_WIDTH`, `LAST_SIZE`/`remember_size` if unused).

- [ ] **Step 1: Failing test** `hit_test_rects`: two rects; points inside each → true; just outside, in the gap between, and negative → false; empty list → false.
- [ ] **Step 2:** `WINTEST hit::` → FAIL. **Step 3:** implement `hit`.
- [ ] **Step 4: Pointer loop** (Windows): every 16 ms while the cursor is within the window's outer rect, else every 100 ms: `GetCursorPos` → window-relative logical px → `hit` → call `window.set_ignore_cursor_events(!inside)` only when the value changes. Start with ignore = true. JS sends rects on load, on every `transitionend`/`animationend`, from a `ResizeObserver` on the shape and pod, and once more when an open/close starts (send the union of start and end rects while animating). Keep focus behaviour (`SW_SHOWNOACTIVATE`, `focus_previous`).
- [ ] **Step 5: Live check (S1)** with the CDP script + PowerShell `[System.Windows.Forms.Cursor]::Position`: cursor over shape → webview receives `mouseenter`; cursor 30 px beside the shape → a test window underneath (e.g. Notepad positioned there) gets the click; idle CPU of the app < 1 % over 30 s (`Get-Process claude-notch | % CPU` delta). If S1 fails, ledger `Ruling:` and fall back to v1's resize-then-show (spec fallback), keeping later tasks' visuals.
- [ ] **Step 6:** `WINTEST` → PASS. Commit `Keep the notch window a fixed size and pass clicks through outside the shape`.

### Task 4: Fonts, tokens and the ring icon

**Files:** Add `src/fonts/Manrope-500.woff2`, `-600`, `-700`, keep `src/fonts/OFL.txt` updated for Manrope; remove Public Sans files; create `src-tauri/icons/source-ring.png` (1024 px) via a dev script `tools/make_icon.py` (Pillow); regenerate bundle icons with `cargo tauri icon icons/source-ring.png` (PowerShell prefix).

- [ ] **Step 1:** Fetch Manrope latin woff2 for 500/600/700 from the Google Fonts CSS2 API (`https://fonts.googleapis.com/css2?family=Manrope:wght@500;600;700`, take the `latin` `src` URLs) and OFL text from google/fonts. Verify each file starts with `wOF2`.
- [ ] **Step 2:** `tools/make_icon.py`: 1024² RGBA, rounded square `#121214` radius 224; outer ring r=336 stroke 96, track `#ffffff26`, arc `#f3f1ee` 68 % from 12 o'clock with round caps; inner ring r=192 stroke 96, track + `#9fb8ff` 37 %. Run it; view the PNG (Read) and a 16 px downscale; at 16 px the inner ring should read as a dot — if not, draw a separate `icons/32x32.png` with inner dot.
- [ ] **Step 3:** `cargo tauri icon` → `src-tauri/icons/*` updated; `WINTEST` still PASS.
- [ ] **Step 4:** Commit `Bundle Manrope and replace the app icon with the ring mark`.

### Task 5: Webview redesign (closed notch, panel, pod, glow, motion)

**Files:** Rewrite `src/notch.css`; modify `src/index.html`, `src/notch.js`.

**Interfaces:** Consumes Task 1 (`countdown_text`, `span_text`), Task 2 (`pod`, `"working"` rows), Task 3 (`set_hit_rects`), existing view model, settings and task-alert commands.

- [ ] **Step 1:** Load the design skills listed in Global Constraints; build the look from `docs/design/redesign/notch-top.html` and `states-and-icon.html` (structure: `.wrap > .shape` with shoulder pseudo-elements, `.ears` closed row, `.body` open panel, `.pod` sibling). Rings are one inline SVG built by a JS function `ring(sessionPct, weekPct, size, state)` (outer r/inner r ratio as mockup; outer amber at `alert`, solid red at `full`).
- [ ] **Step 2:** States: normal, alert, full (`back in …`), idle (`0%` + `ready`), stale/no data/no claude/logged out (muted copy as v1), flash (6 s, inside the ears row), panel lines with `span_text` and the existing "Runs out around HH:MM" line under the session when present, pod working/needs-you with `+N`, breathing glow while any needs-you, `Turn on Task alerts to see sessions` when alerts are off and the sessions area would show.
- [ ] **Step 3:** Motion and input: spring sizes, blur fades, hover intent 300 ms (cancel on leave), click/Enter immediate, Esc closes, reduced-motion variants; keep `aria-expanded`, `#announce`, focus rings, keyboard reachability of every control.
- [ ] **Step 4:** Settings view restyled to the tokens (no behaviour change yet).
- [ ] **Step 5: Verify** through CDP on a debug build: no console/CSP errors; each state rendered by injecting view-model payloads (`window.render(vm)` is already global-reachable via the listener; add a debug-only hook if needed); screenshot each state (`Page.captureScreenshot`) at 100 % and 150 % scale and Read them; contrast of text ≥ 4.5:1; Tab order. Run impeccable critique on the screenshots; fix findings that are real.
- [ ] **Step 6:** Commit `Redesign the notch with concentric rings, a countdown, a session pod and smooth motion`.

### Task 6: Settings: Show in and Stay behind windows

**Files:** Modify `src-tauri/src/settings.rs` (`show_in: ShowIn`, `stay_behind: bool`), `src-tauri/src/main.rs` (apply z-order), `src/index.html`, `src/notch.js`.

**Interfaces:** Produces `#[serde(rename_all="lowercase")] pub enum ShowIn { Top, Taskbar }` (default `Top`), `Settings.stay_behind` (default `false`).

- [ ] **Step 1: Failing test** `old_settings_file_gets_new_defaults` (a v1.1 settings JSON without the fields loads with `show_in == Top`, `stay_behind == false`) and `settings_round_trip_new_fields`.
- [ ] **Step 2:** `WINTEST settings::` → FAIL; **Step 3:** implement.
- [ ] **Step 4:** Stay behind on (Top mode only): `set_always_on_top(false)` and `SetWindowPos(HWND_BOTTOM, …, SWP_NOACTIVATE|SWP_NOMOVE|SWP_NOSIZE)`, re-applied when the notch is shown or placed; off → always on top as today. Full-screen hiding unchanged.
- [ ] **Step 5:** UI: `Show in` segmented (`Top of screen` / `Taskbar`); `Show on` and `Position` hidden when Taskbar; `Stay behind windows` checkbox hidden when Taskbar. Live check: maximise Notepad with stay-behind on → notch covered; off → on top.
- [ ] **Step 6:** `WINTEST` → PASS; commit `Add Show in and Stay behind windows settings`.

### Task 7: Taskbar mode (spikes S2, S3)

**Files:** Create `src-tauri/src/taskbar.rs`; modify `src-tauri/src/main.rs` (placement by mode, re-assert above taskbar), `src-tauri/Cargo.toml` (`Win32_UI_Accessibility`, `Win32_System_Com` as needed), `src/notch.js`/`.css` (capsule variant opening upward, pod on the left, mini variant).

**Interfaces:** Produces `pub enum Fit { Full, Mini, Top }`; `pub fn fit(gap_px: i32, full_w: i32, mini_w: i32, margin: i32) -> Fit`; `#[cfg(windows)] pub fn measure() -> Option<Taskbar { rect: Rect, apps_right: i32, tray_left: i32, auto_hide: bool, bottom: bool }>`; view model field `ViewModel.mode: "top" | "taskbar" | "taskbar_mini"` set from settings + last `fit`.

- [ ] **Step 1: Failing test** `fit_decision`: gap ≥ full+2·margin → Full; ≥ mini+2·margin → Mini; less → Top; negative gap → Top.
- [ ] **Step 2:** `WINTEST taskbar::` → FAIL; **Step 3:** implement `fit`.
- [ ] **Step 4: S2:** `measure()` via `FindWindowW("Shell_TrayWnd")` for the rect, `SHAppBarMessage(ABM_GETSTATE)` for auto-hide, UI Automation for the right edge of the last taskbar app button and the left edge of the tray/clock area. Throwaway `#[ignore]` test prints the result; run it with 3 and then many apps open on the owner's PC; record values in `docs/spikes/taskbar-gap.md`. If it cannot be read reliably, ledger `Ruling:` → Taskbar mode deferred (spec fallback), finish Task 7 with only the setting hidden.
- [ ] **Step 5:** Placement: Taskbar + `Full`/`Mini` → window anchored so the capsule sits right-aligned at `tray_left - margin`, vertically centred in the taskbar, panel grows upward; `Top` (incl. auto-hide or non-bottom taskbar) → top placement. Re-measure every 2 s and on display change; animate between modes with the existing fade.
- [ ] **Step 6: S3:** after the taskbar is clicked, re-assert `HWND_TOPMOST` with `SWP_NOACTIVATE` (poll the foreground window ≤500 ms, the loop already exists); check no visible flicker and no focus steal live.
- [ ] **Step 7:** `WINTEST` → PASS; live CDP + screenshots; commit `Add taskbar mode that shrinks and moves to the top when the taskbar fills`.

### Task 8: README, checklist and release build

**Files:** Modify `README.md` (look, Show in, Stay behind, pod and heartbeat, new acceptance steps 31+).

- [ ] **Step 1:** Document new behaviour and settings; acceptance steps: smooth open, click-through beside the notch, hover intent, pod working→needs you→approve clears, Esc-interrupted session disappears within 10 min, Show in Taskbar full/mini/top, Stay behind with a maximised window, icon in toasts.
- [ ] **Step 2:** `WINTEST` full suite PASS, `cargo tauri build` 0 warnings; restart the owner's notch on the new build.
- [ ] **Step 3:** Commit `Document the redesign and its acceptance steps`.
