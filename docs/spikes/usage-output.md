# Spike S1/S1b: what `claude /usage` returns

Date: 2026-10-28. Claude Code 2.1.312, WSL2 distro `Ubuntu`, run from Windows PowerShell through `wsl.exe` (the same path the app will use).

## Verdict: PASS (with caveats below)

Both limits and both reset times are present and parseable, and 10 extra calls moved neither percentage. The numbers are **not** separate JSON fields: they sit inside the human-readable `result` string, so the parser must read text. Cold timing is still pending (owner runs it).

## Exact working command (WSL source)

```
wsl.exe -d Ubuntu -- bash -lc "claude -p --output-format json /usage"
```

- Exit code 0, JSON on stdout, UTF-8. A single JSON object, no leading text.
- **`bash -lc` is required.** Without it, `wsl.exe -d Ubuntu -- claude -p ...` fails with `/bin/bash: line 1: claude: command not found` (the non-login shell PATH has no `~/.local/bin`). Resolves spec 5.1's open question: use the login shell.
- **Close stdin** (Rust: `Stdio::null()`). When stdin is an open console/pipe, `claude` waits 3 s and prints `Warning: no stdin data received in 3s, proceeding without it.` to stderr. With stdin at NUL the call is about 2.3 s faster (see timings).
- Capture stdout only; stderr can carry that warning.
- Windows source: `claude` is **not** on the Windows PATH on the owner's machine (`Get-Command claude` returned nothing), so there is no `usage-windows.txt` fixture and the native Windows command is untested.

## JSON shape

Fixture: `src-tauri/tests/fixtures/usage-wsl.txt` (raw stdout, unedited). Top-level keys: `is_error`, `duration_api_ms`, `num_turns`, `stop_reason`, `session_id`, `total_cost_usd`, `usage`, `modelUsage`, `permission_denials`, `fast_mode_state`, `fast_mode_disabled_reason`, `subagent_stats`, `safety_stops`, `subtype`, `result`, `local_command`, `type`, `duration_ms`, `uuid`, `queued_turn_count`, `result_index`.

Fields the parser needs:

| What | Where |
|---|---|
| Success check | `is_error == false`, `subtype == "success"`, `local_command == "usage"` |
| Everything else | `result` (one string, `\n` separated) |
| Session % | `result`, line starting `Current session: `, `<N>% used` |
| Session reset | same line, after `resets ` |
| Week % | `result`, line starting `Current week (all models): `, `<N>% used` |
| Week reset | same line, after `resets ` |

The two lines in the fixture:

```
Current session: 46% used · resets Oct 28, 8pm (Asia/Kuala_Lumpur)
Current week (all models): 72% used · resets Oct 31, 10am (Asia/Kuala_Lumpur)
```

Suggested regexes: `Current session: (\d+)% used · resets (.+)` and `Current week \(all models\): (\d+)% used · resets (.+)`. The separator is U+00B7 (`·`).

**Timestamp format:** `Mon D, h[:mm]am|pm (IANA zone)`, for example `Oct 28, 8pm (Asia/Kuala_Lumpur)`. Local wall-clock time in the user's IANA zone, **no year**, 12-hour clock, no ISO. Minutes were absent on the hour; whether `8:30pm` appears for non-round hours is unverified. The parser must infer the year (next occurrence at or after now) and resolve the zone itself.

The rest of `result` (usage-contributor breakdown: "Last 24h", "Last 7d", top skills and plugins) is ignored. No Sonnet/Opus weekly sub-limit line appeared in this reading; none was parsed. All the other fields (`usage`, `modelUsage`, costs) are zero or empty because `/usage` is a local command with no model call.

## Expected values (read off the fixture)

| Field | Value in fixture | Parsed meaning |
|---|---|---|
| Session % | `46` | 46 |
| Session reset | `Oct 28, 8pm (Asia/Kuala_Lumpur)` | 2026-10-28 20:00 +08:00 |
| Week % | `72` | 72 |
| Week reset | `Oct 31, 10am (Asia/Kuala_Lumpur)` | 2026-10-31 10:00 +08:00 |

The year (2026) is inferred from the capture date, not present in the text.

## Timings (Windows PowerShell stopwatch around the full `wsl.exe` call, WSL already running)

| Variant | Runs | Result |
|---|---|---|
| Brief's command, stdin left open | 10 (runs 2 to 11) | 8370 to 8610 ms, typically 8.45 s; first ever call 7.3 s |
| Same, stdin from NUL | 3 | 6031, 6111, 6183 ms (about 6.1 s) |

Roughly 5 to 5.6 s of that is `claude` itself starting up inside WSL (measured natively in WSL: 5.1 s with `bash -lc`, 5.6 s direct); `bash -lc` and the `wsl.exe` hop add well under 1 s. The CLI's own `duration_ms` reports about 1.3 s, so the rest is startup (plugin and settings loading). Plan for a 6 to 9 s call and a timeout well above that.

**Cold timing: pending, owner runs it.** It needs `wsl --shutdown`, which would kill the session the spike ran in. Owner, from a Windows PowerShell:

```
wsl --shutdown
Measure-Command { wsl.exe -d Ubuntu -- bash -lc "claude -p --output-format json /usage" < NUL }
```

Run the second line once right after the first, and add the number here.

## Quota cost

The brief asked for an interactive `/usage` reading; the controller changed this to use the JSON from the runs themselves.

| Reading | Session | Week |
|---|---|---|
| Run 1 (fixture capture) | 46% | 72% |
| Run 11 (after 10 more calls) | 46% | 72% |
| Difference | 0 | 0 |

Every later run, including three more with stdin at NUL, also showed 46% and 72%, with the same reset times. `/usage` runs as a local command (`num_turns` 0, `total_cost_usd` 0, no token usage), so it does not appear to spend quota. Caveats: the percentages are whole numbers, and other Claude Code sessions were running on the same account during the test, so a sub-1% effect cannot be fully ruled out. It is still within the pass limit of 1%. Possible server-side caching of the numbers is not ruled out either, as nothing moved even with other activity; the reading is at least stable across calls.

## Security and redaction

The fixture was checked for tokens, emails, org and account IDs: none present. It contains a random `session_id` and `uuid` (not account identifiers) and the user's plugin and skill names in the breakdown text. Nothing was redacted.

## Consequences for later tasks

- Parser (Task 4): parse text in `result`, not JSON fields; infer year; resolve IANA zone from the parenthesis; treat any missing line as "unparseable, keep the last reading".
- Runner: use `bash -lc` for WSL, `Stdio::null()` for stdin, generous timeout (at least 30 s), read stdout only.
- Windows source stays unverified until someone with native Claude Code on Windows captures `usage-windows.txt`.
