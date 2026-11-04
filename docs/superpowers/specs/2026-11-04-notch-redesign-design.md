# Notch redesign: rings, smooth motion, taskbar mode

Date: 2026-11-04. Status: draft for the owner's approval.
Builds on v1.1 (`main` at `e68e2a5`). Mockups (open in a browser): `docs/design/redesign/notch-top.html` (chosen top look, with the session pod), `notch-taskbar.html` (T1 is chosen), `taskbar-full.html`, `states-and-icon.html` (closed states and icon). Inspiration only, nothing copied: V-Notch (shape + spring), Venu (shoulders, satellite pods, hover intent, calm glow), a korus screen recording (soft cross-fades).

Marked **(owner)** = decided by the owner in the brainstorm. Marked **(Claude's call)** = proposed by Claude; change freely.

## 1. Intent

The owner finds v1's flat black pill dated and its open/close abrupt. They want a more modern, smoother notch that they can also put in the taskbar. Success: the notch looks like part of the screen, every change moves smoothly, and the usage plus "Claude needs you" reads at a glance in either place.

## 2. Goals, non-goals, anti-goals

Goals
- **Concentric rings (owner):** one double ring, session outside (warm white), week inside (soft blue), replaces the text numbers and bars.
- **Countdown (owner):** the closed notch shows session % on the left ear and time to the session reset on the right ear.
- **Attached notch with soft shoulders (owner):** black joined to the top edge; its sides curve into the edge.
- **Smooth motion (owner):** opening grows the same shape with a small spring; content swaps fade (with a light blur) instead of cutting.
- **Satellite pod (owner):** a Claude session that is working buds out as a small pod beside the notch; it turns amber with a bell when it needs you, and merges into the panel when the panel opens. A calm amber glow breathes around the notch while something needs you.
- **Hover intent (owner):** the notch opens only after the pointer rests on it, so sweeping to browser tabs does not open it.
- **Show in: Top of screen / Taskbar (owner).** Taskbar uses design T1: the same black capsule in the empty part of the taskbar, opening upward.
- **Taskbar full (owner):** the capsule shrinks to rings only when space is tight and moves to the top notch when there is no room, then comes back.
- **Stay behind windows (owner):** a setting, off by default, that keeps the notch below other windows so a maximised window covers it.
- **App icon (owner, from the mockup):** the double ring on a dark rounded square; used by the installer, the tray and toasts.

Non-goals
- Floating island shape (owner chose attached notch only).
- T2 native tray item.
- Glass, blur-behind or wallpaper effects (owner, v1 rule).
- Colour washes per state, a mascot, Allow/Deny buttons inside the notch, a live "running npm test" step line (owner preferred the earlier design; the per-tool hook would add work to every Claude step).
- Media, clipboard, calendar or other pages.
- A live ring drawn in the tray icon (later idea).

Anti-goals
- Covering the owner's taskbar app icons.
- Opening by accident when the pointer passes the top edge.
- A notch that flickers or fights the taskbar for the top spot in a visible way.
- Motion that ignores "reduce motion".
- Losing v1 behaviour: hide in full screen, click-through beside the notch, keyboard (Enter opens, Esc closes), screen reader announcements, settings, alerts, task alerts.

## 3. Constraints

- Tauri v2 + Rust + plain HTML/CSS/JS, `windows` 0.62. No new frontend framework or animation library: CSS transitions only.
- The webview window is transparent; anything outside the black shape must stay click-through.
- CSP `font-src 'self'`: fonts are bundled files.
- Windows 11 lets no app reserve space inside the taskbar; the taskbar raises itself when clicked.
- Must keep working on Windows 10 (taskbar internals differ; taskbar mode may fall back to top there).

## 4. Approaches considered

How to make open/close smooth (the main technical choice):
- **A. Fixed-size transparent window + click-through by hit test (chosen, Claude's call).** The window is always as big as the largest state (open panel plus pod plus glow margin). CSS animates the shape inside it. A Rust loop (~60 Hz, only while the pointer is near) checks whether the pointer is over the visible shape and turns the window's mouse input on or off (`set_ignore_cursor_events`). No window resizing during animation, so motion is fully smooth.
- B. Resize the window every animation frame. WebView2 repaints late on resize; it would stutter, which is the problem today.
- C. Shape the window with a Win32 region per frame. Smooth hit-testing but heavy, and regions do not anti-alias the curved shoulders.

Hover intent: a 300 ms rest before opening (Claude's call); click and Enter still open at once.

Taskbar free space:
- **Read button positions with UI Automation (chosen, Claude's call, needs spike S2):** find the taskbar's app buttons and the tray, measure the gap.
- Fixed position near the tray, ignore the icons. Rejected: covers icons, an anti-goal.

## 5. What was decided

### 5.1 Look (tokens)
- Notch black `#000`; ring track `#ffffff24`; session ring `#f3f1ee`; week ring `#9fb8ff`; alert amber `#f0a541`; full red `#ff6b6b`; muted text `#f3f1eebf`. Text contrast ≥ 4.5:1 on black.
- Type: Manrope 500/600/700, bundled as woff2 (OFL), tabular numbers; replaces Public Sans **(Claude's call, used in all mockups)**.
- Shoulders 14 px radius; closed notch 32 px tall, bottom radius 16 px; open panel bottom radius 30 px.
- Motion: spring `cubic-bezier(.34,1.4,.55,1)` ~500 ms for size; fades ~250 ms with a 6 px blur on content swaps; reduced motion: no spring, no blur, no breathing (a still amber outline instead), fades ≤ 120 ms.

### 5.2 Closed notch (top)
- Left ear: double ring + session % (`68%`). Right ear: countdown to session reset: `1:32` (h:mm) when ≥ 1 h, `45m` under 1 h.
- Alert: session ring and % turn amber. Full: outer ring solid red, text `back in 1:32`. Idle session (no reset time): `0%` + `ready`. Stale / logged out / no Claude: keep v1 meanings with muted text (`As of 10:51`, `Log in to Claude Code`, `Install Claude Code to see your limits`).
- When a session alert arrives, the existing 6 s flash shows inside the notch line (same copy as v1.1).

### 5.3 Open panel
- Big double ring with session % and `resets in 1:32` in the middle.
- One line per limit: dot, name, %, countdown (`in 1h 32m`, `in 3d 16h`). "Runs out around HH:MM" stays as a small line under the session when usage is ahead of the clock **(Claude's call, keeps v1 data)**.
- Sessions list (v1.1), footer (updated time, settings, refresh) and the settings view keep their content, restyled to the new tokens.

### 5.4 Session pod
- UserPromptSubmit now marks a session **working** (with its start time) instead of clearing it; Stop → finished (rules unchanged); needs-you unchanged.
- **Heartbeat (owner):** a sixth hook, `PostToolUse`, is added to the installed set. It shows no text; it only means "still alive": a needs-you session goes back to working (so the amber glow stops right after you approve), and a working session's last-seen time updates. A working session with no event for 10 minutes is dropped (covers Esc, crashes, closed terminals). On start, when task alerts are on, the app re-installs its hooks so existing installs gain the new event.
- Pod and glow need task alerts. With them off, the panel's sessions area shows "Turn on Task alerts to see sessions" **(Claude's call)**. The pod shows the most urgent session (needs you > working) with project name, state and elapsed time; `+N` when more sessions are active **(Claude's call)**.
- The pod appears beside the notch (right side at top; left side in the taskbar) and merges into the panel when it opens. Finished sessions do not get a pod; they show in the panel as today.
- While any session needs you, the amber glow breathes around the notch.

### 5.5 Show in: Top / Taskbar
- New setting `Show in`: `Top of screen` (default) / `Taskbar`. Position (Left/Centre/Right) and Show on stay for Top. Taskbar mode always uses the main taskbar (the one with the clock and tray); Show on and Position are hidden in that mode **(Claude's call)**.
- Taskbar: the capsule sits just left of the tray on the taskbar of the chosen screen, vertically centred; it opens upward with the same spring; the pod buds to its left.
- Every ~2 s (and on taskbar/size changes) the app measures the gap between the last app button and the tray: enough → full capsule; tight → rings only (hover still opens the full panel); none → it shows at the top until the gap returns. Auto-hide taskbar or Windows 10 → top.
- It re-asserts staying above the taskbar when the taskbar is activated, without stealing focus.

### 5.6 Stay behind windows
- Setting, off by default. On: the notch is not always-on-top and stays at the bottom of the window stack, so maximised windows cover it. Full-screen hiding still applies. In taskbar mode it is ignored (the taskbar is always on top).

### 5.7 Icon
- Double ring on a `#121214` rounded square (64/32/16 px; at 16 px the inner ring becomes a dot). Generated into all bundle sizes; toasts then show it.

## 6. Spikes to run first

- **S1. Fixed window + hit-test click-through.** A throwaway build: big transparent window, CSS shape, Rust pointer loop toggling `set_ignore_cursor_events`. Check: clicks beside the shape reach the app below, hover/Enter/Esc still work, no focus stealing, CPU when idle near 0.
- **S2. Taskbar gap via UI Automation** on the owner's Windows 11 (left-aligned icons): read app-button and tray rectangles; log them as windows open and close.
- **S3. Staying above the taskbar** after the owner clicks the taskbar, without flicker.

Fallbacks **(Claude's call)**: if S1 fails, keep today's resize-then-show approach with the new look and soft fades (the top redesign still ships); if S2 or S3 fails, ship Top only and move Taskbar mode to a follow-up release.

## 7. Left to the builder

- Exact spring curves and durations within 5.1; the pointer-loop rate and its idle back-off.
- Module layout (suggested `taskbar.rs` for gap and placement; existing `placement.rs` grows the shoulder margin).
- Tests: pure logic gets unit tests (countdown text, pod choice, gap → mode decision, placement maths); the visuals are checked live through the DevTools protocol as in v1.1 plus owner README steps.
