# Claude Notch

Claude Notch is a small black pill at the top edge of your Windows screen. It shows how much of your Claude 5-hour session limit and weekly limit you have used. Hover over it to see reset times and whether you are using Claude faster than the clock. It warns you before you run out and hides itself while a full-screen app is on the same screen.

## Requirements

- Windows 10 or 11 with WebView2 (already part of Windows 11).
- Claude Code installed and logged in, either on Windows or inside WSL.

If Claude Code is not found, the notch says so and links to the install page. It has no other way to read your limits.

## Install

1. Run `Claude Notch_0.1.0_x64-setup.exe`. It installs for your user only and needs no admin rights.
2. Windows SmartScreen may warn that the app is unrecognised, because the installer is not signed. Choose **More info**, then **Run anyway**.
3. The notch appears at the top of your screen when the install ends.

## Using it

- Hover over the pill, or click it, or press Enter, to open the panel. Esc closes it.
- The panel shows a bar for the session and the week, when each resets, and a blue tick for how much of that time has passed. If the fill is past the tick, you are using Claude faster than the clock, and it says when you would run out.
- The gear opens settings. The refresh button updates now; it also updates by itself every 5 minutes.
- Old numbers turn grey and show "As of" with the time. This happens when WSL is stopped, you are offline, or Claude returns an error.

## Settings

Changes are saved as you make them.

- **Show on**: which screen the notch sits on. If that screen is unplugged, it moves to your main screen.
- **Position**: left, centre or right. Right leaves room for the window buttons.
- **Get usage from**: Auto looks on Windows first, then in each WSL distro. Or pick Windows, or one WSL distro.
- **Start WSL if it's stopped**: only for a WSL source. When off, the notch will not start WSL and shows old numbers instead.
- **Session alert at** and **Week alert at**: the percentage that triggers an amber warning and one Windows notification, from 50 to 99. At 100 percent it always alerts as full.

## Tray menu

Right-click the tray icon. Its tooltip shows the current numbers.

- **Hide notch** or **Show notch**
- **Start with Windows** (off by default)
- **Refresh now**
- **Quit**

## Privacy

Never reads your Claude login; only runs `claude /usage`.

The app runs `claude -p --output-format json /usage` (inside WSL when needed), reads the percentages and reset times from the answer, and keeps them on your PC. It does not open your Claude credentials file and does not contact claude.ai or api.anthropic.com itself.

## Build from source

Needs Rust (MSVC toolchain) and `tauri-cli` on Windows. The code can stay in WSL. From a Linux shell, run:

```
powershell.exe -NoProfile -Command '$env:Path = [Environment]::GetEnvironmentVariable("Path","User") + ";" + [Environment]::GetEnvironmentVariable("Path","Machine"); $env:CARGO_TARGET_DIR = "C:\Users\hanif\code\notch-target"; Set-Location \\wsl.localhost\Ubuntu\home\hanif\own\notch-project\src-tauri; cargo tauri build'
```

The installer lands in `C:\Users\hanif\code\notch-target\release\bundle\nsis\`. Use `cargo test` instead of `cargo tauri build` to run the tests. Always run cargo through PowerShell, not from Linux.

## Acceptance checklist

Tick each step after you try it on your own PC. Write down anything that fails before you release.

### Look and window

1. [ ] Install and start it. You see a black pill about 220 by 36 pixels at the top of your screen, with transparent rounded corners.
2. [ ] The pill stays above other windows and has no taskbar entry.

### Panel and click-through

3. [ ] Hover over the pill. The panel opens in place. Move away and it closes.
4. [ ] Click on the screen just beside the pill. The click reaches the app underneath.
5. [ ] Open the panel, open a settings drop-down, then click another app. The pinned panel closes.
6. [ ] With no Claude Code available, click "Install Claude Code to see your limits". The Claude Code setup page opens in your browser.

### Cold start timing

7. [ ] In Windows PowerShell, run `wsl --shutdown`, then right away:
   `Measure-Command { cmd /c 'wsl.exe -d Ubuntu -- bash -lc "claude -p --output-format json /usage" < NUL' }`
   Write the time in `docs/spikes/usage-output.md` under "Cold timing".

### Screens and position

8. [ ] With two monitors, set "Show on" to each screen in turn. The notch moves there. Check the screen numbers against Settings, System, Display, Identify.
9. [ ] On each screen, try Left, Centre and Right. The notch lands at the left edge, middle, and right with a gap for the window buttons.
10. [ ] Unplug the screen you chose. The notch moves to your main screen.
11. [ ] On a screen with scaling above 100 percent, the notch and its gaps scale and nothing is cut off.

### Full screen

12. [ ] Run `cargo run --example fullscreen_probe` as written in `docs/spikes/fullscreen.md` and fill in the Observed column: F11 in a browser, YouTube full screen, a game, a maximised window, and the desktop.
13. [ ] Full screen on the notch's screen (F11, YouTube full screen, a game) hides the notch. Leaving it shows the notch again.
14. [ ] A maximised window does not hide the notch.
15. [ ] Full screen on the other screen does not hide the notch.

### Alerts and tray

16. [ ] In settings, lower a threshold below your current usage. You get exactly one Windows notification, and the value turns amber.
17. [ ] Right-click the tray icon. You see Hide notch, Start with Windows, Refresh now and Quit. Hide notch hides the pill and the item becomes Show notch.
18. [ ] Turn on Start with Windows, sign out and back in. The notch is there and the item is still ticked.
19. [ ] Quit from the tray closes the app.
