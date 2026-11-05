# macOS Menu-Bar Version Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An experimental macOS build: a menu bar item with usage and prayer lines, the settings window, notifications, and curl-based install and updates, built by GitHub Actions.

**Architecture:** Same Tauri app. Windows-only code stays behind `cfg(windows)`; macOS-only behaviour goes behind `cfg(target_os = "macos")`. Pure helpers (command shapes, menu texts) are plain functions tested on Windows. The macOS build is checked only by a GitHub Actions workflow on `macos-latest`.

**Tech Stack:** Tauri 2, Rust, plain JS, GitHub Actions (`macos-latest`, `npx @tauri-apps/cli@2`), POSIX `sh` for `install.sh`.

**Spec:** `docs/superpowers/specs/2026-11-05-macos-menu-bar-design.md`

## Global Constraints

- No behaviour change on Windows; `cargo test` on Windows stays green (201 today).
- No new dependencies.
- macOS artefacts: `ClaudeNotch-mac.zip` (the `.app`, made with `ditto -c -k --keepParent`) and `ClaudeNotch-mac.dmg`, universal (`--target universal-apple-darwin`).
- Nothing is pushed to `main` or released without the owner's word; the feature branch is pushed only to run the workflow.
- README says plainly that the Mac version is experimental and untested.

## Review Focus

1. Claude installed with Homebrew or `~/.local/bin` and the app opened from Finder → found (login shell PATH) (Task 2 test on the command shape).
2. Claude not installed on the Mac → "Install Claude Code" state, no crash (Task 2: `detect` returns `NotFound` when the probe fails).
3. `install.sh` run twice, or while the app is running → replaces the app and relaunches it (Task 4 script review).
4. The Windows tray, settings and notch are unchanged (Task 1–3: Windows build + full tests + live tray check).
5. A failed macOS build never uploads files to a release (Task 4: upload step runs only after a successful build).

---

### Task 1: The macOS build compiles (workflow + gates)

**Files:** Create `.github/workflows/mac.yml`; modify Windows-only call sites found by the build (main.rs, monitors.rs, fullscreen.rs, taskbar.rs, events.rs, source.rs as needed); `prayer.rs` and `update.rs` (`curl.exe` → `CURL`).

**Interfaces:** Produces `pub const CURL: &str` in `source.rs` (`"curl.exe"` on Windows, `"curl"` elsewhere).

- [ ] Step 1: workflow with `workflow_dispatch` and `release: published`; job builds `npx @tauri-apps/cli@2 build --target universal-apple-darwin --bundles app,dmg` after `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.
- [ ] Step 2: push the branch, run the workflow; fix each compile error with the smallest `cfg` gate; repeat until it builds.
- [ ] Step 3: Windows `cargo test` → 201 pass; commit "Build the app on macOS".

### Task 2: Find and run Claude on the Mac

**Files:** `source.rs` (rename `Source::Windows` → `Source::Local` everywhere; macOS detect/run through `/bin/zsh -lc`), callers.

**Interfaces:** Produces `pub fn login_shell(cmd: &str) -> (&'static str, Vec<String>)` returning `("/bin/zsh", ["-lc", cmd])`; macOS `detect_with` probes `command -v claude`; macOS usage run is `claude -p --settings '{"disableAllHooks":true}' --output-format json /usage` through it.

- [ ] Step 1: failing tests `login_shell_wraps_the_command` and `mac_usage_command_disables_hooks` (pure string checks, run on Windows).
- [ ] Step 2: implement; rename; Windows tests pass; mac workflow builds. Commit "Find and run Claude through a login shell on macOS".

### Task 3: Menu bar item, hidden notch, Mac settings

**Files:** `tray.rs` (macOS title + info items + update item), `main.rs` (macOS: `ActivationPolicy::Accessory`, hide the notch window; `LATEST_UPDATE` static set by `check_update`), `src/settings.js`/`settings.html` (hide Windows-only groups on Mac).

**Interfaces:** Produces `pub fn menu_title(vm: &ViewModel) -> String` (e.g. `"18% · 33m"`, or `""` without numbers) and `pub fn menu_lines(vm: &ViewModel) -> Vec<String>` (session, week, next prayer) in `tray.rs`, tested on Windows.

- [ ] Step 1: failing tests `menu_title_is_pct_and_countdown` and `menu_lines_include_next_prayer`.
- [ ] Step 2: implement; macOS-only wiring behind `cfg(target_os = "macos")`; settings page hides Display, Claude's source/WSL/Task alerts and Sounds when `navigator.userAgent` contains `Mac OS X`.
- [ ] Step 3: Windows tests + live Windows tray unchanged; mac workflow builds. Commit "Show usage in the macOS menu bar".

### Task 4: Install, update, release upload, README

**Files:** Create `install.sh`; `update.rs` (macOS `install`: run `curl -fsSL <install.sh> | sh` detached); `.github/workflows/mac.yml` (zip + dmg, `gh release upload --clobber` on release only); `README.md` (Mac section, experimental).

- [ ] Step 1: `install.sh`: download `ClaudeNotch-mac.zip` from the latest release with `curl -fL`, unzip to a temp dir, quit the running app (`pkill -x claude-notch`), replace `/Applications/Claude Notch.app` (or `~/Applications` when `/Applications` is not writable), `open` it; `sh -n install.sh` passes.
- [ ] Step 2: workflow uploads only after the build step succeeds; manual run attaches artefacts to the workflow run instead.
- [ ] Step 3: README Mac section; commit "Install and update the Mac version with one command".
- [ ] Step 4: whole-branch review; ask the owner before merge, push to main and release.
