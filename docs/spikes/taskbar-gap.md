# Spike S2: taskbar gap via UI Automation (2026-11-04)

Owner's PC, Windows 11 build 26300, taskbar at the bottom, icons left-aligned, 1920x1080.

- `Shell_TrayWnd` rect: 0,1032 -> 1920,1080 (48 px tall).
- App buttons: UIA class `Taskbar.TaskListButtonAutomationPeer`, 44 px wide each; 16 buttons, the last (Terminal) at x=821, so app buttons end at **865**.
- Tray: `TrayNotifyWnd` starts at **1669** (Show hidden icons, input, network, volume, power, clock, show desktop).
- Gap: 804 px. A full capsule (~150 px) fits with lots of room.
- 31 UIA elements; a full descendant search is quick enough for a 2 s poll on a background thread.

Effect on the plan: measure app-button right edge with UIA (`ClassName == Taskbar.TaskListButtonAutomationPeer`), tray left with `FindWindowEx(Shell_TrayWnd, TrayNotifyWnd)` + `GetWindowRect`.
