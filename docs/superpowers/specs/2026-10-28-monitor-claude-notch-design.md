# Claude usage notch for Windows: design

Date: 2026-10-28. Status: draft for the owner's approval.
Mockup: `docs/design/notch-mockup.html` (open in a browser; the first frame is interactive). Product brief: `docs/design/PRODUCT.md`.

Marked **(owner)** = decided by the owner in the brainstorm. Marked **(Claude's call)** = proposed by Claude, not yet challenged; change freely.

## 1. Intent

The owner uses Claude (Claude Code in WSL, plus the Claude desktop app) all day on a Pro/Max subscription and keeps getting surprised by the usage limits. They want to see, at a glance and without opening anything, how much of the **5-hour session limit** and the **weekly limit** is used and when each resets, and to be warned before they run out.

Success: the owner never hits a limit by surprise, and never has to open Claude's `/usage` to know where they stand.

Audience: the owner first, but the app must also install and work for **other Windows users**, whose Claude Code may be on Windows, in WSL, or not installed at all **(owner)**.

## 2. Goals, non-goals, anti-goals

Goals
- A small black "notch" pill at the top edge of a Windows screen, always visible, showing session % and week % **(owner)**.
- Hover, click or Enter opens a panel in place with a bar per limit, reset times and the pace tick **(owner: panel; Claude's call: pace tick, accepted by owner)**.
- Alerts at a threshold the owner sets, separately for session and week **(owner)**, plus a Windows notification when one fires **(owner accepted suggestion)**.
- "Runs out around HH:MM" when usage is ahead of the clock **(owner accepted suggestion)**.
- Hides itself while a full-screen app is on the same screen **(owner: required)**.
- Choice of screen and of position (left, centre, right) **(owner)**.
- Tray menu: show/hide notch, start with Windows, quit **(owner accepted)**.

Non-goals (this version)
- The Amazfit / Zepp watch (dropped by the owner).
- Usage history or charts over days (later, once the owner knows they want it).
- macOS or Linux builds.
- Session alerts ("Claude needs your input", "task finished"): **step 2, after the usage notch works (owner)**; see section 8. v1 only leaves room for them in the pill.
- Token counts per request, cost tracking.

Anti-goals (would make it a failure even if it works)
- **Reading or sending the owner's Claude OAuth token or claude.ai cookie.** Anthropic's consumer terms reportedly forbid using subscription tokens outside Claude Code and claude.ai, and the owner fears losing the account. Our code must never open `~/.claude/.credentials.json`, never call `api.anthropic.com` or `claude.ai` itself, and never store a token. The Claude CLI does all fetching.
- Spending usage to measure usage: a refresh must not consume meaningful session quota (see spike S1).
- A number that looks fresh but is old: stale data must always look stale.
- Covering things the owner needs: games and full-screen video, and the window close buttons.

## 3. Constraints

- Windows 11 host; the app runs on Windows. The owner's Claude Code is **inside WSL2**; other users may have it **natively on Windows**, in WSL, or **not at all** (then the app cannot show limits, see 5.1).
- Owner is a web developer, new to desktop apps: prefer plain HTML/CSS/JS in the webview, keep Rust to the parts that need Windows APIs.
- WSL may be stopped; the PC may be offline. Both must degrade to the stale state, never crash.
- WSL2 networking is NAT (no `.wslconfig`); not relevant unless a local server is added (none planned).

## 4. Approaches considered

**Data source** (owner picked 1)
1. **Ask the Claude CLI every 5 minutes**: the app runs `wsl.exe claude -p --output-format json /usage` and parses the result. Covers the whole account (desktop app, phone, CLI), works while idle. Costs a WSL + CLI start per refresh; JSON form untested. **Chosen.**
2. Claude Code status line: Claude Code passes `rate_limits` (five_hour / seven_day `used_percentage`, `resets_at`) to a status-line script, which would write a file the app reads. Official and free, but only updates while a Claude Code session is active. **Fallback if spike S1 fails; the owner accepts it, knowing the notch will often be stale (owner).** If used: the app adds its script to Claude Code's `settings.json` by merging (back up the file first; if a status line already exists, chain to it so the user's own line still shows), and a reading older than 10 minutes counts as stale **(Claude's call)**.
3. Both (status line live, CLI when older than 5 minutes). Most accurate, twice the code. Rejected for v1.
Rejected outright: calling the usage endpoint with the OAuth token, or claude.ai with the session cookie (terms risk, anti-goal).

**App shell** (owner picked 1)
1. **Tauri v2 tray app**: small (about 3-10 MB), webview UI in HTML/CSS/JS, Rust for Windows APIs. **Chosen.**
2. Fork `claude-usage-widget` (Electron, MIT): faster start, but 80-150 MB and built on the claude.ai cookie (anti-goal).
3. Windows 11 Widgets Board: needs Adaptive Cards and a packaged app; cannot be an always-visible notch.

**Placement** (owner picked 1)
1. **Top-edge notch pill**, always on top. **Chosen.** Cost: it covers part of the top edge of maximised windows.
2. Tray icon with popup: never covers anything, but easy to miss.
3. Floating draggable card: most information, most screen space.

## 5. What was decided

### 5.1 Data and refresh
- **Finding Claude Code (Claude's call, owner agreed to the approach):** on first run and on demand, check in order: `claude` on the Windows PATH; then each installed WSL distro for `claude`. Use the first found ("Auto"). Settings can override: Windows, or WSL with a chosen distro.
- **No Claude Code found:** the notch shows `Install Claude Code to see your limits` with a link to the official install page; no other data source is used (anti-goal).
- **WSL stopped:** setting `Start WSL if it's stopped`, shown only for a WSL source, **default On (owner wants it on; Claude's call to make it a setting for other users)**. When Off, check `wsl.exe -l --running` first and go stale instead of booting WSL.
- Refresh every **5 minutes** and on demand (refresh button, clicking the pill opens the panel but does not force a refresh) **(Claude's call)**.
- Command: `claude -p --output-format json /usage` (Windows source) or `wsl.exe -d <distro> -- claude -p --output-format json /usage` (WSL source; check whether `claude` needs a login shell to be on PATH), run with no window (no console flash). Parse session %, session reset time, week %, week reset time. If the CLI also returns Sonnet/Opus weekly sub-limits, ignore them in v1.
- Keep the last good reading with its timestamp. If a refresh fails (WSL stopped, CLI error, offline, unparseable output), keep showing the last reading in the **stale** state and keep retrying on the normal schedule.
- Never more than one refresh in flight; a manual refresh while one runs is ignored.

### 5.2 The notch (collapsed pill)
- Black pill hanging from the top edge, bottom corners rounded 14px, about 36px tall. Content: `Session 42%   Week 68%` (labels muted, values white, tabular figures). See mockup.
- States, in priority order:
  - **Full** (a limit at 100%): red Lucide `triangle-alert` icon, `Session full, back in 1h 20m` (or `Week full, back in Fri 09:00`), the other limit as normal.
  - **Alert** (a limit at or above its threshold): amber `triangle-alert` icon, that value in amber.
  - **Stale** (last refresh failed): all values grey, plus `As of 10:51`.
  - **Normal**.
  - **No data yet** (first run, nothing fetched): **(builder's call)**; suggest `Session –   Week –` in grey.
- Hover opens the panel; moving off closes it after 150ms unless it was opened by click (then it stays until clicked again or Esc). Enter/Space opens, Esc closes and returns focus to the pill.

### 5.3 The panel
- Opens in place from the pill (fade + slight scale, 180ms; close 120ms; reduced-motion: fade only).
- Per limit: name (`Session, 5 hours` / `Week, 7 days`), value, bar, reset (`Resets in 2h 13m` / `Resets Fri 09:00`), pace text (`56% of time passed`).
- **Pace tick**: a 1px blue line on each bar at the share of that window's time already passed. Fill past the tick = using faster than the clock.
  - Session elapsed % = (5h − time to reset) / 5h. Week elapsed % = (7d − time to reset) / 7d.
- **Runs-out projection** (new): when used % > elapsed %, show `Runs out around 15:40` under that bar, computed as now + (100 − used) × (elapsed time / used). Show only if that moment is before the reset; otherwise show nothing extra. **(Claude's call on wording)**
- Footer: `Updated 10:51`, Settings (gear) button, Refresh button (spins while refreshing; `Updating…`).

### 5.4 Settings view (inside the panel; gear opens it, back arrow returns)
- **Show on**: list of connected screens, labelled with number and name; default the primary screen; if the chosen screen is disconnected, fall back to primary **(owner)**.
- **Position**: Left / Centre / Right, default Centre **(owner)**. Segmented control; the selected option carries a 1px blue line under its label. Left = 8px from the screen's left edge. Right = leaves a gap for window caption buttons (about 140px at 100% scaling, multiplied by the screen's scale factor) **(Claude's call)**.
- **Get usage from**: Auto / Windows / WSL (distro list), with `Start WSL if it's stopped` under it for WSL (5.1).
- **Session alert at** / **Week alert at**: sliders 50–99%, default 90% each **(owner: separate; Claude's call: range and default)**. Full is always 100% and not configurable.
- Saved as changed (no Save button); persists across restarts in a small JSON settings file in the app's config folder.
- Hint line: `Saved as you change it. Full always alerts at 100%.`

### 5.5 Windows notifications
- One toast when a limit **crosses** its threshold, one when it becomes **full** **(owner accepted; Claude's call on rules)**. Example: `Session at 91%. Resets in 1h 30m.`
- No repeat for the same limit until that window resets; only a reset re-arms it. Raising the threshold above the current value clears the alert state silently; lowering it below the current value fires the toast once.
- Use the Tauri notification plugin.

### 5.6 Hide in full screen (required by owner)
- Every ~1 second, check the **foreground window**: if it covers its whole monitor and that monitor is the notch's screen, hide the notch; show it again when that stops. Exclude the desktop and shell (`Progman`, `WorkerW`, `Shell_TrayWnd`) and the notch itself.
- Windows' `SHQueryUserNotificationState` (busy / D3D full screen / presentation mode) cannot tell which monitor, so use it only as a hint on single-monitor setups **(Claude's call)**.
- Tauri has no built-in API for this; implement in Rust with the `windows` crate (Win32 `GetForegroundWindow`, `GetWindowRect`, `MonitorFromWindow`, `GetMonitorInfoW`). Chromium's `ui/base/fullscreen_win.cc` is a reference.
- Toasts still fire while hidden.

### 5.7 Window behaviour
- Frameless, transparent background, always on top, not in the taskbar or Alt+Tab, no focus stolen on show. **Keyboard (Claude's call):** no global hotkey in v1. Clicking the notch gives its window focus; from then on Tab, Enter/Space, arrow keys and Esc work as in the mockup, and Esc also hands focus back to the previous window. A global hotkey can come later. Position recomputed on display changes (resolution, scaling, monitor plugged/unplugged).
- The window is only as large as the pill when closed (so clicks outside the pill reach the app below) and grows to the panel size when open. **(builder decides how: resize the window, or a fixed panel-sized window with click-through outside the pill)**.

### 5.8 Tray menu
- `Show notch` / `Hide notch` (toggle), `Start with Windows` (checkbox, default off), `Refresh now`, `Quit`. Tray icon tooltip shows the current numbers.

### 5.8b Build and install (Claude's call)
- Build on the **Windows host** (Rust with the MSVC toolchain, Node, WebView2 already on Windows 11); Tauri's cross-compiling from Linux is not reliable enough. The source can live in WSL or Windows; the plan's first phase sets up the toolchain step by step for a first-time desktop developer.
- Ship as Tauri's default **NSIS installer**, per-user (no admin). Unsigned: the owner accepts one SmartScreen "More info → Run anyway" on first install. "Start with Windows" uses the Tauri autostart plugin against the installed path.

### 5.9 Visual system (from the design pass; see mockup)
- Direction: taste-skill app style "data-terminal" (seed 1480439416), checked with hallmark, impeccable, ui-ux-pro-max, WCAG AA.
- Tokens (primitive → semantic → component) are in the mockup's `:root`. Key values: notch `#000000`, ink `#ffffff`, muted `#8a8c92`, track `#3a3a3e`, fill `#d6d7da`, pace/focus `#4a8ad6`, warn `#d9a441`, danger `#e5566f`, hover `#1e1e21`. Font: Public Sans (bundle the font files in the app; do not load from Google at runtime), `tabular-nums` on all numbers.
- Icons: Lucide `triangle-alert`, `settings`, `chevron-left`, `rotate-cw`, inline SVG.
- Accessibility: pill is a `button` with `aria-expanded`; bars are `role="meter"` with `aria-valuetext`; one `role="status"` message when a limit crosses its threshold or fills (never on every refresh); focus ring inset on the pill; all text ≥ 4.5:1.

## 6. Spikes to run first (they can change the plan)

The mockup's settings view shows only the alert sliders; Show on, Position and Get usage from follow the same style (segmented control or select, 1px blue selection line).


- **S1 /usage from Windows**: does `wsl.exe claude -p --output-format json /usage` return parseable JSON with both limits and reset times? How long does it take cold (WSL stopped) and warm? Does it consume session quota (compare `/usage` before and after 10 runs)? If it fails or costs quota, switch to approach 2 (status line).
- **S2 Full-screen detection**: Rust check from 5.6 against: a game, YouTube full screen in Chrome/Edge, F11 browser, a maximised (not full-screen) window (must NOT hide), on one and two monitors.
- **S1b Windows-native Claude Code**: same as S1 with `claude` installed on Windows, if available to test.
- **S3 Monitors**: how Tauri lists the screens versus Windows Display settings numbering; what name to show.

## 7. Left to the builder

- Project layout, Rust/JS split, build tooling, how the webview and Rust talk (Tauri commands/events).
- Exact parsing of the CLI output (after S1).
- How to size and position the window for click-through (5.7).
- Retry/backoff details beyond "keep the 5-minute schedule".
- The "no data yet" look.
- Tests: unit tests for the pace %, runs-out time, alert crossing and re-arm rules, and the full-screen rectangle check; manual checks for the rest.

## 8. Step 2 (after v1 works): session alerts

Not built in v1; recorded so v1 leaves room.
- Goal: with several Claude Code terminals open, see which sessions **need the owner's input or permission** and which **finished**, without checking each terminal **(owner)**.
- Source: Claude Code's official hooks, `Notification` (needs permission/input) and `Stop` (finished a reply), installed into each Claude Code `settings.json` (Windows and/or WSL) by merging with a backup. The hook appends one JSON line (session id, project folder, event, time) to a file the app watches. Hook names and payload fields must be verified against current Claude Code docs first.
- UI: a third pill item such as `2 waiting` with a Lucide `bell` icon; the panel lists sessions (`notch-project, needs your input, 2m`); a Windows toast per event. Jumping to the terminal tab is out of scope (Windows Terminal cannot target a tab reliably).
