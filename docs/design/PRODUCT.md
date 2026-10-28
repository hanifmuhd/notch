# monitor-claude notch

Source: the user's answers in the 2026-10-28 brainstorm. Lines marked (inferred) are Claude's calls, not the user's.

- Product: a small always-on-top "notch" pill at the top centre of a Windows screen (Tauri tray app) that shows the user's Claude subscription limits.
- User: one person (the owner), web developer, uses Claude Code in WSL and the Claude desktop app all day.
- Job: glance to know how much of the 5-hour session and the weekly limit is used, and when each resets; never be surprised by hitting a limit.
- Data: `claude -p --output-format json /usage` every 5 minutes and from the refresh button (inferred cadence). Numbers are real, never invented.
- States the user asked for: normal, 90%+ alert with a Lucide `triangle-alert` icon, full, greyed when it cannot refresh.
- Mode: Operate (a tool glanced at), dark, quiet, precise. (inferred)
- Later features go in the expanded panel.
