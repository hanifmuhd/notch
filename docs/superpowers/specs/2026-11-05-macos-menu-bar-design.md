# macOS menu-bar version (experimental, untested on a real Mac)

Date: 2026-11-05. Approved by the owner on 2026-11-05 ("yes proceed").

## Why

Mac users ask for the app. Nobody can test on a Mac yet, so the first Mac version is the part
that is simple enough to trust without seeing it: a menu bar item, no notch window.

## Goals

- **Menu bar:** the ring icon with "18% · 33m" beside it. Its menu lists the session and week
  lines with reset times, the next prayer when Prayer times is on, "Update to x.y.z" when one is
  out, Settings…, Refresh now and Quit.
- **Settings window:** as on Windows, with Windows-only groups and rows hidden.
- **Alerts:** usage alerts and prayer times as Mac notifications (their normal sound).
- **Install and updates without an Apple developer account:** an `install.sh` one-liner using
  `curl` (no quarantine mark, so no Gatekeeper prompt); the Update item runs the same script.
  A `.dmg` is also attached to releases, with the README's "Open Anyway" step.
- **Build:** GitHub Actions on macOS builds a universal (Apple Silicon + Intel) app on release
  and attaches `ClaudeNotch-mac.zip` and `ClaudeNotch-mac.dmg`.
- README marks the Mac version as experimental and untested.

## Non-goals (Mac, for now)

- The notch window, Taskbar mode, full-screen hiding.
- Bell and azan sounds (Core Audio cannot play the .ogg bells; nobody can hear-test).
- Task alerts (Claude Code hooks): hidden on Mac until someone can test them.
- WSL sources (Mac has none).

## Anti-goals

- Any change in behaviour on Windows.
- Publishing a Mac file from a failed build.

## Approaches

**Mac UI** (owner picked, given no tester): (a) full notch port — needs a tester; (b) **menu bar
only** — picked; (c) nothing until a tester exists.

**Compile checks:** a local cross-check from Windows is impossible (objc2 needs Apple's
toolchain), so GitHub's macOS runners are the only build check; work happens on a pushed
feature branch with a manually triggered workflow.

## Design calls (builder's, for the owner to check)

- `source::Source::Windows` becomes `Source::Local` ("Claude on this computer"). On macOS it is
  found and run through a login shell (`/bin/zsh -lc …`), because apps started from Finder do
  not get the terminal's PATH.
- `curl.exe` becomes `curl` on non-Windows for prayer times and updates.
- On macOS the app is a menu-bar-only app (no Dock icon) and the notch window stays hidden; its
  page still runs the update check every 6 hours.
- The latest found update version is kept in Rust so the tray can show "Update to x.y.z".
- Settings on Mac show Prayer, Alert levels and About.
