# Spike S1: Claude Code hook payloads (2026-11-04)

Claude Code 2.1.318 in WSL (Ubuntu). A temporary hook copied stdin to a file for each event; the owner's `~/.claude/settings.json` was restored byte-identical afterwards (sha256 checked).

## Payloads (stdin, one JSON object)

Common fields: `session_id`, `transcript_path`, `cwd`, `prompt_id`, `hook_event_name`, often `permission_mode`.

| Event | Extra fields seen |
|---|---|
| `UserPromptSubmit` | `prompt` |
| `Stop` | `stop_hook_active`, `last_assistant_message`, `effort`, `background_tasks`, `session_crons` |
| `SessionEnd` | `reason` (`"other"` for a `-p` run) |
| `Notification` | `message` (`"Claude is waiting for your input"`), `notification_type` (`"idle_prompt"`) |
| `PermissionRequest` | `tool_name`, `tool_input`, `permission_suggestions` |

Sample `Notification` (idle, the one we ignore):
`{"session_id":"…","cwd":"/home/hanif/own","hook_event_name":"Notification","message":"Claude is waiting for your input","notification_type":"idle_prompt"}`

## Findings

- A `-p` run fires `UserPromptSubmit`, `Stop`, `SessionEnd` in that order (~2 s apart for a tiny reply).
- The idle notice is a `Notification` with `notification_type: "idle_prompt"`, about 60 s after a reply.
- A permission prompt fires `PermissionRequest` (seen in `-p` mode). The `Notification` with `notification_type: "permission_prompt"` could not be captured without an interactive session; it is documented by Claude Code but unconfirmed here.
- `claude -p --settings '{"disableAllHooks":true}' --output-format json /usage` still prints the usage lines, fires no hooks, and reports `total_cost_usd: 0`.
- `cwd` is a Linux path; the project name is its last folder.

## Effect on the design

- Hook `Notification` without a matcher and filter in the app: "needs you" = `notification_type` is `permission_prompt` or `elicitation_dialog`; anything else (`idle_prompt`, `auth_success`, missing) is ignored. App-side filtering does not depend on matcher support.
- Also hook `PermissionRequest` as a second "needs you" signal, de-duplicated per session (a session already in "needs you" does not toast again).
- The usage call gains `--settings '{"disableAllHooks":true}'`.
