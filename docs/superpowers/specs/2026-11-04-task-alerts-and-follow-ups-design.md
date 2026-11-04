# Task alerts (step 2) and v1 follow-ups: design

Date: 2026-11-04. Status: draft for the owner's approval.
Builds on `2026-10-28-monitor-claude-notch-design.md` (v1, shipped as `ffbd574`). Section 8 of that doc sketched step 2; this doc replaces that sketch.

Marked **(owner)** = decided by the owner. Marked **(Claude's call)** = proposed by Claude; change freely.

## 1. Intent

v1 shows Claude usage limits. The owner runs several Claude Code terminals at once and wants to know, without checking each one, which session **needs their input or permission** and which **finished**. The same release closes the v1 review follow-ups that must be fixed before other people use the app.

Success: the owner never leaves a session waiting unnoticed, and the app is fit to hand to other Windows users.

The visual redesign and the new "Show in: Top of screen / Taskbar" setting **(owner)** are a separate, later piece of work with its own mockup. This work keeps the current look.

## 2. Goals, non-goals, anti-goals

Goals
- Task alerts: the notch and a Windows toast tell the owner when a session needs them or finished **(owner: notch + toast)**.
- A "Task alerts" switch in Settings adds or removes the app's own hook entries in Claude Code's `settings.json` **(owner)**.
- Follow-ups from the v1 final review:
  1. A timed-out `/usage` call leaves no Claude process running.
  2. Fired usage alerts survive a restart, so a restart does not toast again.
  3. A content security policy.
  4. Friendly monitor names in "Show on".
  5. The parser accepts an idle session whose line has no `resets …` part.
  6. A clear hint when Claude Code is logged out.
- The three v1 changes from the mockup stay as built **(owner)**: clicking elsewhere closes a pinned panel; the title is "Settings"; screen labels (now with monitor names, see 5.4).

Non-goals
- Jumping to the terminal or tab that needs you (Windows Terminal cannot target a tab reliably).
- Hooks for more than one Claude Code install at once: the hook goes into the install the app reads usage from.
- A real app icon: it belongs to the redesign, so it matches the new look **(Claude's call)**.

Anti-goals
- Breaking the owner's existing hooks. `~/.claude/settings.json` in WSL already holds Orca hooks; the app must keep every entry it did not write.
- Losing or corrupting `settings.json`. Invalid JSON there means the app refuses to edit it and says so.
- Reading Claude's OAuth token or cookies (unchanged rule from v1).
- Toasts that repeat on restart or flood the screen.

## 3. Constraints

- Claude Code hooks run a shell command with a JSON payload on stdin. For WSL Claude the command runs in Linux; for Windows Claude, Claude Code runs it through Git Bash. Event names used: `Notification`, `Stop`, `UserPromptSubmit`, `SessionEnd`. Payload fields (`session_id`, `cwd`, `hook_event_name`, `message`, and `notification_type` where present) must be checked against a real payload first (spike S1).
- WSL2 does not reliably forward `localhost` to Windows, so the hook hands events over through **files**, not a local port.
- Tauri v2 + Rust, the `windows` crate already in use. No new crates except turning on serde_json's `preserve_order` feature (keeps the key order of `settings.json`).

## 4. Approaches considered

How a hook reaches the app:
- **A. One file per event in a folder the app polls (chosen, Claude's call).** The hook writes `<events>/<time>-<pid>.json.tmp` and renames it to `.json`, so the app never reads half a file. The app reads and deletes new files once a second. Works the same from WSL (`/mnt/c/...`) and Git Bash (`/c/...`). Tiny delay (≤ 1 s).
- B. Append to one shared `.jsonl` file. Two terminals writing at once can interleave lines; payloads may contain newlines. Rejected.
- C. Local HTTP port + `curl`. WSL2's `localhost` to Windows needs mirrored networking; firewall prompts. Rejected.

Who installs the hook: **a Settings switch (owner)** rather than README steps.

## 5. What was decided

### 5.1 Hook install (Settings switch)

- Settings gets a **Task alerts** section: a switch "Show when Claude needs you" (installs/removes hooks) and a checkbox "Also when a task finishes" (default on; filters `Stop` in the app, the hook stays) **(Claude's call)**.
- The `Notification` hook uses a matcher so it fires only for permission prompts and questions to the user (`permission_prompt|elicitation_dialog`, names confirmed in S1). The ~60 s "waiting for your input" idle notice is not hooked **(owner)**.
- Reading the switch state is a user action, but it must not boot a stopped distro when "Start WSL" is off: then the switch is disabled with the hint "Start WSL to change this" **(Claude's call)**.
- When the usage source changes while task alerts are on, the app removes its entries from the old install (best effort) and adds them to the new one **(Claude's call)**.
- Target file: for source Windows, `%USERPROFILE%\.claude\settings.json`; for source WSL distro X, `\\wsl.localhost\X\<home>\.claude\settings.json`, where `<home>` comes from `wsl.exe -d X -- bash -lc 'printf %s "$HOME"'`. Undetected or not found: the switch is disabled with the hint "Install or find Claude Code first".
- Editing rules: read; if the file is missing, start from `{}`; if it is not valid JSON, change nothing and show "Couldn't read Claude's settings.json, so nothing was changed". Before the first edit, copy it to `settings.json.notch-backup` (once; never overwrite an existing backup). Add one hook entry per event under `hooks.<Event>`, each command containing the marker `claude-notch`. Removing deletes only entries whose command contains the marker, and drops arrays/objects the app emptied. Write via `.tmp` + rename. Re-running is idempotent (no duplicates).
- The switch state shown is read from the file (marker present), not stored, so manual edits stay truthful.
- Hook command (bash, one line; `<dir>` is the events folder as the hook's shell sees it, via `wslpath -u` for WSL or `/c/...` form for Git Bash):
  `d='<dir>'; [ -d "$d" ] && { f="$d/$(date +%s%N)-$$"; cat > "$f.tmp" && mv "$f.tmp" "$f.json"; }; true # claude-notch`
  No `mkdir`: the app creates the folder, so after an uninstall that removes app data the hooks do nothing. It must finish fast, never block Claude, and always exit 0. README tells users to turn task alerts off before uninstalling **(Claude's call)**.
- The app's own `/usage` call runs with hooks disabled (`--settings '{"disableAllHooks":true}'`, checked in S1), so the 5-minute poll never fires `Stop` or anyone else's hooks.

### 5.2 Event intake and session state

- Events folder: `<app config dir>\events\`. A background loop (1 s) reads `*.json` oldest first, deletes each after reading, ignores `*.tmp`, deletes unreadable files and anything older than 1 day.
- Events written while the app was closed: at start, files older than 10 minutes are dropped; newer ones go into the Sessions list without a toast or pill flash **(Claude's call)**.
- Session state, keyed by `session_id`, project name = last folder of `cwd` **(Claude's call)**:
  - `Notification` → **needs you** (with the message text).
  - `Stop` → **finished** (dropped if "Also when a task finishes" is off). It toasts and flashes the pill only when the reply took **1 minute or more**, measured from that session's last `UserPromptSubmit`; shorter replies appear quietly in the Sessions list **(owner)**. No prompt seen for that session (e.g. app started mid-task) = treat as long.
  - `UserPromptSubmit` → removed (you are back in that session); its time is kept for the 1-minute rule.
  - `SessionEnd` → removed.
  - A **finished** entry also clears when you open the panel and close it again (seen), or after 30 minutes.
  - A **needs you** entry after a permission prompt stays until that session's next `Stop`, prompt or end (no hook fires when you approve; known limit, accepted for v1.1 unless S1 shows a cheap signal).
- Kept in memory only; a restart starts empty.

### 5.3 What the owner sees

- **Pill:** while any session needs you or finished unseen, a Lucide `bell` icon and a count appear after the limits (`Session 46%  Week 72%  🔔 2`). When a new event arrives, the pill briefly shows the line itself for 6 s (`notch-project needs you`) and the screen reader announces it, then it settles back to the count.
- **Panel:** a "Sessions" list under the limits, newest first, max 5 rows: project name, "needs you" (amber) or "finished", and age (`2m`). Hidden when empty.
- **Toast:** one per `Notification`, and one per long `Stop` (5.2), title "Claude needs you" / "Claude finished", body the project name (plus the message for `Notification`). No toast for `UserPromptSubmit`/`SessionEnd`.
- **Full screen:** the notch hides as in v1; toasts still show (Windows' own Focus/Do Not Disturb rules apply).

### 5.4 Follow-ups

1. **No leftover Claude after a timeout.** Windows: put the child in a Job Object with kill-on-close and terminate the job on timeout, so `cmd.exe`'s `claude`/`node` descendants die too. WSL: killing `wsl.exe` cannot reach Linux processes, so the Linux side is wrapped in `timeout 55 claude …` (before the app's 60 s deadline).
2. **Fired alerts survive restart.** Save the alert memory (per limit: fired-alert, fired-full, window reset time) to `alerts.json` beside `settings.json` whenever it changes, using the same `.tmp` + rename write; load it at start. Missing/corrupt file = today's behaviour.
3. **CSP.** Replace `"csp": null` with a policy allowing only the app's own files and Tauri IPC (start from `default-src 'self'; connect-src ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data:`). Check there are no CSP errors in the console and every button still works.
4. **Monitor names.** Labels become the monitor's own name from Windows display config (e.g. `DELL U2720Q (main)`), matched to each monitor's device name. Fallback to `Screen N` when Windows gives no name; duplicate names get ` 2`, ` 3`. The saved screen id is unchanged, so existing settings keep working.
5. **Idle session without a reset time.** A limit line `… N% used` with no `· resets …` parses as "no active window": the panel shows the % with "Starts when you next use Claude", no pace tick and no runs-out line; alerts skip it. (Exact idle text is unconfirmed; README step 20 stays open to capture it.)
6. **Logged out.** When the CLI exits with an error or its JSON says `is_error: true` and the text mentions logging in, the panel shows "Log in to Claude Code: run `claude`, then `/login`" instead of stale numbers.

## 6. Spikes to run first

- **S1. Real hook payloads.** In WSL, temporarily add a hook that copies stdin to a file for `Notification`, `Stop`, `UserPromptSubmit`, `SessionEnd`; trigger each once; record field names, the `notification_type` values and matcher behaviour, and a sample in `docs/spikes/hook-payloads.md`. Also confirm `claude -p --settings '{"disableAllHooks":true}' --output-format json /usage` fires no hooks and still prints usage. Then remove the temporary hook (owner's settings.json must end byte-identical to before). Changes 5.1/5.2 if anything differs.
- **S2. Event file from WSL.** Confirm the hook command writes into `C:\Users\<user>\AppData\Roaming\com.hanifmuhd.claudenotch\events` from WSL within a second and the app sees it.
- Windows-native Claude (Git Bash path form) cannot be tested on the owner's PC; test the path conversion with a unit test and note it as untested on real hardware.

## 7. Left to the builder

- Module layout (suggested: `hooks.rs` for settings.json editing, `events.rs` for intake + session state), exact Rust types, test names.
- Copy for hints and errors, within the meanings above.
- Whether the events loop shares the existing 1 s full-screen loop or has its own thread.
- Tests: pure logic gets unit tests (settings.json merge/remove keeping foreign hooks, idempotency, invalid JSON refusal; session state transitions; parser idle case; alert state round-trip; monitor-name de-duplication). Job Object and toasts are checked by hand on Windows.
- README: add acceptance steps for task alerts and the follow-ups.
