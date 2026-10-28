# Spike S2/S3: full-screen detection and monitor names

Probe: `src-tauri/examples/fullscreen_probe.rs`. Once per second it prints the foreground window class, its rect, the monitor it sits on (device name, rect), `covers=`, `excluded=` (class is `Progman`, `WorkerW` or `Shell_TrayWnd`), `fullscreen=` (covers and not excluded) and the `SHQueryUserNotificationState` value (`quns`). At start it lists every monitor from `EnumDisplayMonitors` (device name, rect, work area, primary flag). `covers` is true when the window rect contains the whole monitor rect (the window may overhang).

Run it (stops by itself with `--seconds N`, default runs until Ctrl+C):

```
cd C:\Users\hanif\code\notch\src-tauri
cargo run --example fullscreen_probe
```

## S2 matrix (owner to fill in)

The builder could not press F11, play video or start a game. **Owner:** run the command above, switch to each case below for a few seconds, and write down the `fullscreen=` (and raw `covers=`) and `quns=` values it prints (`quns`: 2 busy, 3 D3D full screen, 4 presentation mode, 5 = normal/accepts notifications).

| Case | Expected fullscreen | Expected class | Observed |
|---|---|---|---|
| Maximised window (not full screen), screen 1 | false | any app | pending (owner) |
| Maximised window, screen 2 | false | any app | pending (owner) |
| F11 in a browser, screen 1 | true | `Chrome_WidgetWin_1` | pending (owner) |
| F11 in a browser, screen 2 | true | `Chrome_WidgetWin_1` | pending (owner) |
| YouTube full screen in Edge, screen 1 | true | `Chrome_WidgetWin_1` | pending (owner) |
| YouTube full screen in Edge, screen 2 | true | `Chrome_WidgetWin_1` | pending (owner) |
| A game, full screen, screen 1 | true | game's own class | pending (owner) |
| A game, full screen, screen 2 | true | game's own class | pending (owner) |
| Desktop clicked, screen 1 | false (raw covers=true, excluded=true) | `Progman` or `WorkerW` | pending (owner) |
| Desktop clicked, screen 2 | false (raw covers=true, excluded=true) | `Progman` or `WorkerW` | pending (owner) |

Classes are the expected values from the brief and Chromium's reference; the Observed column will confirm them.

## What the probe really reported (builder run, 5 s, this PC)

```
-- monitors (EnumDisplayMonitors) --
monitor \\.\DISPLAY2 rect=(0,0)-(1920,1080) 1920x1080 work=(0,0)-(1920,1032) 1920x1032 primary=true
-- foreground, once per second --
class=CASCADIA_HOSTING_WINDOW_CLASS rect=(-8,-8)-(1928,1040) 1936x1048 monitor=\\.\DISPLAY2 (0,0)-(1920,1080) 1920x1080 covers=false excluded=false fullscreen=false quns=5
(same line x5)
```

Findings:
- One active monitor right now, and it is `\\.\DISPLAY2`, not `DISPLAY1`.
- A maximised window (Windows Terminal here) overhangs the monitor by 8 px on the left/top/right but stops above the taskbar (bottom 1040 < 1080), so `covers=false`. The "contains the monitor rect" rule therefore does not mistake a maximised window for full screen, as long as the taskbar is visible. With an auto-hidden taskbar a maximised window would reach the bottom edge; that case is part of the owner's maximised row and may need a check on the work area (`rcWork`) too.
- `quns=5` (QUNS_ACCEPTS_NOTIFICATIONS) for normal use, as expected.

## S3 monitor names

Device names from `EnumDisplayMonitors` / `GetMonitorInfoW` are GDI names (`\\.\DISPLAYn`). The `n` is a display-adapter output number, not the number shown in Windows Settings, and it is not stable across plug/unplug. On this PC the laptop panel is switched off and the only active screen is `\\.\DISPLAY2`, so GDI says 2 where Settings very likely says 1 (owner to confirm).

Friendly names (`Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID`, decoding `UserFriendlyName`):

| WMI InstanceName | UserFriendlyName | Note |
|---|---|---|
| `DISPLAY\MSI30C4\...` | `MSI MP252` | the active external monitor |
| `DISPLAY\LEN40A9\...` | (blank) | laptop panel, currently off; panels often report no name |

Joining GDI to WMI works through the hardware id: `EnumDisplayDevices(\\.\DISPLAY2, 1)` returns `DeviceID = MONITOR\MSI30C4\{...}\0001`, and `MSI30C4` is the middle part of the WMI `InstanceName`. (Its `DeviceString` is only "Generic PnP Monitor", so use WMI for the name.)

**Recommendation for "Show on":** label each entry `Screen N (Friendly name)`, e.g. `Screen 1 (MSI MP252)`, or just `Screen N` when the friendly name is blank, where
- the friendly name comes from `WmiMonitorID` matched by hardware id as above; if blank or missing (laptop panels), use plain `Screen N`;
- N should be the Windows Settings number, which neither GDI nor Tauri gives directly; the mapping below is an unverified guess. Until the owner confirms how it lines up, use the position in the list of active monitors ordered left to right then top to bottom (the order Settings usually shows, unverified), not the GDI `DISPLAYn` number;
- store the choice by monitor position/hardware id plus rect, not by the label, so a renamed or renumbered screen still resolves.

**Owner to confirm:** open Settings -> System -> Display, click Identify, and check that the number shown matches the `N` in the Show on list (and that the MSI MP252 is the screen it numbers). Not checked by the builder: how Tauri's own monitor list (`available_monitors`) numbers or names these screens; that needs a window and belongs to the Task 13 work.
