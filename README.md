# Claude Notch

Claude Notch is a small black notch joined to the top edge of your Windows screen (or a capsule in your taskbar). A double ring shows how much of your Claude 5-hour session limit (outside) and weekly limit (inside) you have used, next to the session % and the time until the session resets. Hover over it to see more. It warns you before you run out, shows when a Claude Code session needs you, and hides itself while a full-screen app is on the same screen.

## Requirements

- Windows 10 or 11 with WebView2 (already part of Windows 11).
- Claude Code installed and logged in, either on Windows or inside WSL.

If Claude Code is not found, the notch says so and links to the install page. It has no other way to read your limits.

## Install

Either way installs for your user only, needs no admin rights, and also updates an older version.

**With one command.** Open PowerShell (Start, type `PowerShell`) and paste:

```
irm https://raw.githubusercontent.com/hanifmuhd/notch/main/install.ps1 | iex
```

It downloads the newest installer, installs it quietly and starts the notch.

**With the installer.** Download `ClaudeNotch-setup.exe` from the [latest release](https://github.com/hanifmuhd/notch/releases/latest) and run it. Windows SmartScreen may warn that the app is unrecognised, because the installer is not signed. Choose **More info**, then **Run anyway**. The notch appears at the top of your screen when the install ends.

### On a Mac (experimental, untested)

The Mac version is new and nobody has tried it on a real Mac yet. For now it is a menu bar item, not a notch: the ring with your session % and countdown, and a menu with the session and week lines, the next prayer, updates, settings and Quit. Alerts and prayer times come as Mac notifications. Task alerts and the bell and azan sounds are not on the Mac yet. Please report what works and what does not.

**With one command.** Open Terminal and paste:

```
curl -fsSL https://raw.githubusercontent.com/hanifmuhd/notch/main/install.sh | sh
```

It downloads the newest version into Applications and opens it, with no security prompt. Run it again, or choose **Update to x.y.z** in the menu, to update.

**With the disk image.** Download `ClaudeNotch-mac.dmg` from the [latest release](https://github.com/hanifmuhd/notch/releases/latest) and drag the app to Applications. The app is not signed by Apple, so the first time macOS says it cannot check it: open it once, then go to **System Settings → Privacy & Security** and click **Open Anyway**.

## Using it

- Closed: the double ring and your session % on the left, the time until the session resets on the right (`1:32`, or `45m` under an hour). Amber means you passed your alert level, red means full (`back in 1:32`), `ready` means no session is running.
- Rest the pointer on the notch for a moment, or click it, or press Enter, to open the panel; it grows out of the notch. Esc closes it. Moving past the notch on the way to your browser tabs does not open it.
- The panel shows the rings large, one line per limit with its reset time, and "Runs out around" when you are using Claude faster than the clock.
- The gear opens the settings window (also in the tray menu). The refresh button updates now; it also updates by itself every 5 minutes.
- When a newer version is out, Windows shows one notification ("Claude Notch 1.4.1 is ready"), and the panel shows **Update to 1.4.1** (or whichever version) at the top. Click it and the notch downloads the new version, installs it and starts again by itself, with your settings kept.
- Old numbers turn grey and show "As of" with the time. This happens when WSL is stopped, you are offline, or Claude returns an error.
- If Claude Code is logged out, the notch says "Log in to Claude Code" and hides the old numbers until you run `claude`, then `/login`.
- With **Task alerts** on, the notch tells you when a Claude Code session needs your permission or answer, and when a reply that took a minute or more finishes. The notch shows the line for a few seconds, then a bell with a count; the panel lists the sessions. Each also gives a Windows notification. Closing the panel clears the finished ones.
- A session that is working shows as a small pod beside the notch with how long it has been running. It turns amber with a bell when it needs you, and a soft amber glow breathes around the notch until you answer. A session that stops sending news (Esc, a crash, a closed terminal) disappears after 10 minutes.
- With **Prayer times** on, the panel shows the next prayer for your JAKIM zone, its time and how long until it, a line since the previous prayer, and today's five times (Subuh, Zohor, Asar, Maghrib, Isyak). At each prayer time the notch shows a moon with "Maghrib · It's time to pray · 18:58" for a minute, and before it "Maghrib in 10 min · 18:58" for 20 seconds; each also gives a Windows notification. After that a small blue moon stays beside the numbers until you open the notch. If the times cannot be downloaded, the panel says "Prayer times unavailable — will retry" and nothing alerts.
- **Sounds**: when more sessions need you, and before a prayer, the notch plays a bell; at the prayer time it plays your own azan if you added one, or the bell. The prayer sound wins over a bell, and opening the notch stops any sound.

## Settings

The gear in the panel, or **Settings…** in the tray menu, opens the settings window. Changes are saved as you make them, and Esc closes the window. Groups:

- **Display**: Show in, Show on, Position, Stay behind windows.
- **Claude**: Get usage from, Start WSL, Show when Claude needs you, Also when a task finishes.
- **Prayer**: Prayer times, Zone, Remind before.
- **Sounds**: the bell, what Claude needs you plays, what the prayer time plays.
- **Alert levels**: Session alert at, Week alert at.
- **About**: your version, with **Check for updates** (or **Update to x.y.z** when one is out).

- **Show in**: Top of screen (default) or Taskbar. In the taskbar the notch is a capsule just left of the tray that opens upward. When the taskbar gets crowded it shrinks to the rings, and when there is no room at all it moves to the top until room returns. An auto-hiding or vertical taskbar uses the top.
- **Show on**: which screen the notch sits on, by the monitor's own name. If that screen is unplugged, it moves to your main screen.
- **Position**: left, centre or right. Right leaves room for the window buttons.
- **Stay behind windows**: off by default. On, the notch stays under other windows, so a maximised window covers it instead of the notch covering its title bar. Show on, Position and this setting apply to Top only.
- **Get usage from**: Auto looks on Windows first, then in each WSL distro. Or pick Windows, or one WSL distro.
- **Start WSL if it's stopped**: only for a WSL source. When off, the notch will not start WSL and shows old numbers instead.
- **Show when Claude needs you** (Task alerts): adds the notch's own hooks to Claude Code's `settings.json` for the install it reads usage from, and removes them when you turn it off. Your other hooks stay as they are, and the first change keeps a copy as `settings.json.notch-backup`. It is greyed out until Claude Code is found, or while WSL is stopped and "Start WSL" is off.
- **Also when a task finishes**: on by default. Off means only "needs you" alerts.
- **Prayer times**: off by default. **Zone** is your JAKIM zone (default WLY01, Kuala Lumpur and Putrajaya). **Remind before**: Off, 5, 10 (default) or 15 minutes. Zone and Remind before show while Prayer times is on.
- **Bell**: Chime (default), Ding, Soft or Double. **Play** lets you hear it.
- **Claude needs you**: Bell (default), My file or Off. **Prayer time**: Bell (default), My azan or Off; the reminder before a prayer plays the bell unless this is Off. **Choose file** takes an mp3, wav, ogg or m4a up to 10 MB and keeps a copy in the app's folder, so moving the original does not matter; **Remove** deletes the copy and goes back to the bell.
- **Session alert at** and **Week alert at**: the percentage that triggers an amber warning and one Windows notification, from 50 to 99. At 100 percent it always alerts as full.

## Tray menu

Right-click the tray icon. Its tooltip shows the current numbers.

- **Hide notch** or **Show notch**
- **Start with Windows** (off by default)
- **Settings…**
- **Refresh now**
- **Quit**

## Privacy

Never reads your Claude login; only runs `claude /usage`.

The app runs `claude -p --settings '{"disableAllHooks":true}' --output-format json /usage` (inside WSL when needed), reads the percentages and reset times from the answer, and keeps them on your PC. It does not open your Claude credentials file and does not contact claude.ai or api.anthropic.com itself.

Task alerts use Claude Code's own hooks. Each hook copies the whole event Claude Code hands it into a file in the app's folder on your PC. That copy can include your prompt, Claude's last reply, or the command or file it asked permission for. The app reads each file and deletes it within about a second; files written while the app is closed are deleted at its next start (after a day at most). Nothing is sent anywhere.

Prayer times, when on, download your zone's times for this month from JAKIM's e-solat (`www.e-solat.gov.my`) with Windows' own `curl.exe`, and next month's on the last day of the month, so they are in before Maghrib. It also downloads when you change zone. Only the zone code and the month's first and last dates are sent. The times are kept beside the settings file. Last month's times are deleted after Subuh on the 1st, and another zone's times as soon as you change zone.

To look for updates, the notch asks GitHub (`api.github.com`) for the newest release when it starts and every 6 hours, with Windows' own `curl.exe`. Nothing about you is sent. Clicking **Update** downloads the installer from the project's GitHub releases page.

## Uninstall

Turn **Show when Claude needs you** off first, so Claude Code's `settings.json` no longer lists the notch's hooks. If you forget, uninstalling still deletes the app's `events` folder, and the leftover hooks then do nothing; you can remove the lines containing `claude-notch` by hand.

## Build from source

Needs Rust (MSVC toolchain) and `tauri-cli` on Windows. The code can stay in WSL. From a Linux shell, run:

```
powershell.exe -NoProfile -Command '$env:Path = [Environment]::GetEnvironmentVariable("Path","User") + ";" + [Environment]::GetEnvironmentVariable("Path","Machine"); $env:CARGO_TARGET_DIR = "C:\Users\hanif\code\notch-target"; Set-Location \\wsl.localhost\Ubuntu\home\hanif\own\notch-project\src-tauri; cargo tauri build'
```

The installer lands in `C:\Users\hanif\code\notch-target\release\bundle\nsis\`. Use `cargo test` instead of `cargo tauri build` to run the tests. Always run cargo through PowerShell, not from Linux.

The Mac version is built by GitHub Actions (`.github/workflows/mac.yml`) on macOS runners: publishing a release attaches `ClaudeNotch-mac.zip` and `ClaudeNotch-mac.dmg` to it.

To release, raise `version` in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, build, then attach the installer under the fixed name `ClaudeNotch-setup.exe`, which the install command and the download link expect:

```
cp "/mnt/c/Users/hanif/code/notch-target/release/bundle/nsis/Claude Notch_1.5.1_x64-setup.exe" /tmp/ClaudeNotch-setup.exe
gh release create v1.5.1 /tmp/ClaudeNotch-setup.exe --title "Claude Notch 1.5.1" --notes "What changed"
```

## Acceptance checklist

Try each step on your own PC before a release, and write down anything that fails.

### Look and window

1. Install and start it. You see a black notch joined to the top of your screen, its sides curving into the edge, with the double ring, your session % and the countdown.
2. The notch stays above other windows and has no taskbar entry. The installer, tray and notifications show the ring icon.

### Panel and click-through

3. Rest the pointer on the notch. After a moment the panel grows out of it with a small bounce. Move away and it closes. Sweep the pointer quickly across the notch: it does not open.
4. Click on the screen just beside the notch, and below it where the open panel would be. The click reaches the app underneath.
5. Open the panel, open a settings drop-down, then click another app. The pinned panel closes.
6. With no Claude Code available, click "Install Claude Code to see your limits". The Claude Code setup page opens in your browser.

### Cold start timing

7. In Windows PowerShell, run `wsl --shutdown`, then right away:
   `Measure-Command { cmd /c 'wsl.exe -d Ubuntu -- bash -lc "claude -p --output-format json /usage" < NUL' }`
   Write the time in `docs/spikes/usage-output.md` under "Cold timing".

### Screens and position

8. With two monitors, set "Show on" to each screen in turn. The notch moves there. Check the screen numbers against Settings, System, Display, Identify.
9. On each screen, try Left, Centre and Right. The notch lands at the left edge, middle, and right with a gap for the window buttons.
10. Unplug the screen you chose. The notch moves to your main screen.
11. On a screen with scaling above 100 percent, the notch and its gaps scale and nothing is cut off.

### Full screen

12. Run `cargo run --example fullscreen_probe` as written in `docs/spikes/fullscreen.md` and fill in the Observed column: F11 in a browser, YouTube full screen, a game, a maximised window, and the desktop.
13. Full screen on the notch's screen (F11, YouTube full screen, a game) hides the notch. Leaving it shows the notch again.
14. A maximised window does not hide the notch.
15. Full screen on the other screen does not hide the notch.

### Alerts and tray

16. In settings, lower a threshold below your current usage. You get exactly one Windows notification, and the value turns amber.
17. Right-click the tray icon. You see Hide notch, Start with Windows, Refresh now and Quit. Hide notch hides the notch and the item becomes Show notch.
18. Turn on Start with Windows, sign out and back in. The notch is there and the item is still ticked.
19. Quit from the tray closes the app.

### Idle session

20. After at least 5 hours with no Claude Code use (for example first thing in the morning), run the `/usage` command from step 7 once and save the output. If the session line has no `resets …` part, tell Claude so the parser can accept it.

### Task alerts and follow-ups (v1.1)

21. Turn on **Show when Claude needs you**. Claude Code's `settings.json` now has five entries containing `claude-notch`, your other hooks are unchanged, and `settings.json.notch-backup` exists.
22. In a WSL terminal, ask Claude something that needs permission (for example to create a file). You get a "Claude needs you" notification, the notch shows "<project> needs you" for a few seconds, then a bell with 1.
23. Ask something that takes over a minute. When it finishes you get "Claude finished". A quick reply only adds a quiet row in the panel.
24. Open and close the panel. Finished rows are gone; "needs you" rows stay until you answer in that terminal.
25. The 5-minute usage refresh never adds a "finished" row.
26. Turn the switch off. The `claude-notch` entries are gone and the rest of the file is unchanged.
27. Lower a threshold below your usage (one notification), then quit and restart the app. No second notification.
28. **Show on** lists your monitor by name (for example "MSI MP252 (main)").
29. If you have Claude Code on Windows (not WSL), choose it as the source and check the numbers still load. This checks the Windows command quoting, which could not be tested here.
30. Log out of Claude Code, refresh. The notch says "Log in to Claude Code" and hides the numbers. Log back in.

### Redesign (v1.2)

31. Turn off "Animation effects" in Windows (Settings, Accessibility, Visual effects). The panel opens without the bounce or blur, and the needs-you glow becomes a still amber outline.
32. With Task alerts on, start a Claude Code reply in a WSL terminal. A pod shows "<project> working" beside the notch with a timer. When Claude asks permission it turns amber with a bell and the notch glows; approve it and the amber goes away within a few seconds.
33. Press Esc in the middle of a Claude reply. The pod disappears within 10 minutes.
34. Choose Show in: Taskbar. The capsule sits just left of the tray and opens upward. Open many apps until the taskbar is crowded: it shrinks to the rings, then moves to the top when there is no room; close apps and it comes back.
35. In Taskbar mode, click an empty part of the taskbar. The capsule stays visible.
36. In Top mode, turn on Stay behind windows and maximise a window. The window covers the notch; turn it off and the notch is on top again.

### Prayer times (v1.3)

37. Turn on Prayer times with zone WLY01. The panel shows "Prayer · Kuala Lumpur, Putrajaya" and today's five times match e-solat.gov.my for that zone.
38. Wait for a reminder (or a prayer time) with the notch closed. The notch shows the moon and the banner line, and one Windows notification arrives. Quit and restart within the minute: no second notification.
39. Choose Show in: Taskbar and wait for the next alert. The capsule shows the short form, for example "Maghrib · 18:58", without cutting it off.
40. Change the zone. The panel's place and times change to the new zone.
41. Turn off Wi-Fi, delete the `prayer-*.json` files in the app's folder and restart. The panel says "Prayer times unavailable — will retry"; turn Wi-Fi back on and the times appear within an hour.

### Updates (v1.4)

42. With 1.4.0 installed and a newer release on GitHub, open the panel within 6 hours of the release. It shows "Update to <version>"; click it. The notch closes, comes back within a minute without the Update line, Windows Settings → Apps lists the new version, and your settings are the same.
43. With six or more sessions listed, open the panel. Only the session list scrolls; the rings, prayer times and the footer stay in view.

### Settings window and sounds (v1.5)

44. Click the gear, then Settings… in the tray: one settings window, brought to the front. Esc closes it.
45. Change Position and Show in from the window: the notch moves at once. Hide the notch from the tray with the window open, change any setting: the notch stays hidden.
46. Sounds: choose each bell and press Play. Choose a 12 MB file: "Choose a file under 10 MB." and nothing changes. Add an azan mp3: My azan turns on and Play plays it. Remove: back to Bell.
47. Wait for a prayer reminder and a prayer time with the notch closed: the bell for the reminder, your azan (or the bell) at the time. Open the notch while it plays: it stops. A small moon stays after the banner until you open the notch.
48. In the taskbar, with a bell and the moon showing, the capsule shows "34%  4:01  bell 1  moon" without cutting anything.
