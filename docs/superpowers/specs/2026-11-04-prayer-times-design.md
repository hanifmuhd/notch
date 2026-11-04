# Prayer times in the notch: design

Date: 2026-11-04. Status: draft for the owner's approval.
Inspiration: the owner's screen recording of a Mac notch app (prayer times panel with a sun arc, and a "Maghrib · It's time to pray · 17:48" banner). Taken as inspiration only. Robot eyes and the Floating style from that video are **not** wanted (owner).

Marked **(owner)** = decided by the owner. Marked **(Claude's call)** = proposed by Claude; change freely.

## 1. Intent

The owner, in Malaysia, wants the notch to show the day's prayer times and to alert them when it is time to pray, so they do not miss a prayer while working. Success: every prayer is announced on time with the official JAKIM time, without opening anything.

## 2. Goals, non-goals, anti-goals

Goals
- Official JAKIM times by zone (e-solat.gov.my), e.g. WLY01 Kuala Lumpur **(owner)**.
- A Prayer section in the panel: zone name, the next prayer large with its time and "in 1h 8m", a progress bar since the previous prayer, and the five prayers in a row (Subuh, Zohor, Asar, Maghrib, Isyak) with the next one highlighted **(Claude's call, from the video)**.
- At each prayer time: the notch grows into a banner with a moon icon, "Maghrib · It's time to pray · 18:58", for 60 s, plus a Windows notification **(owner)**.
- A reminder before each prayer: setting Off / 5 / 10 / 15 min, default 10 **(owner)**; same banner style with "Maghrib in 10 min · 18:58" and a notification.
- Settings: Prayer times on/off, Zone (all JAKIM zones), Reminder minutes.

Non-goals
- Robot eyes, Floating style, Imsak/Syuruk/Dhuha alerts, adhan audio, qibla, Hijri calendar view, non-Malaysian methods.

Anti-goals
- Breaking the usage notch or task alerts.
- Constant animation or polling that costs CPU (owner asked; idle cost must stay ~0.2–0.7 % of a core).
- Sending anything about the owner: only the zone code goes to JAKIM.
- Alerting with a wrong time silently (stale or missing data must be visible).

## 3. Constraints

- Windows only, Tauri v2, no new runtime crate. Downloads use Windows' own `curl.exe` through the existing `Runner` (as the app already runs `claude`).
- API (spike, 2026-11-04): `GET https://www.e-solat.gov.my/index.php?r=esolatApi/TakwimSolat&period=month&zone=<ZONE>` → `{status:"OK!", zone, prayerTime:[{date:"04-Nov-2026", hijri, imsak, fajr, syuruk, dhuha, dhuhr, asr, maghrib, isha}]}` with times `HH:MM:SS`, Malaysia time (UTC+8).
- First network use of the app: README Privacy must say so.

## 4. Approaches considered

How to download:
- **A. Windows `curl.exe` via the existing Runner (chosen, Claude's call).** No new dependency, same timeout and process clean-up as the `claude` call, testable with the fake runner.
- B. An HTTP crate (e.g. ureq). Adds a dependency and TLS stack for one request a month.
- C. `fetch()` in the webview. Needs CSP and cross-origin changes; network access from the page.

## 5. What was decided

### 5.1 Data
- Month cache file `prayer-<ZONE>-<YYYY-MM>.json` beside settings.json. Download when the zone changes, when the month has no cache, and retry hourly while missing. Keep the previous month's file until the new one arrives.
- Parse the five prayers per day as Malaysia local times (UTC+8) **(Claude's call)**. A day with a missing or unparsable time is skipped for alerts.
- Zones: the full JAKIM zone list (code + place names) is built into the app (spike S2 fetches it once from e-solat to make the list) **(Claude's call)**.

### 5.2 Panel
- Section "Prayer · <zone place>" between the limit lines and Sessions: next prayer name large, its time, "in 1h 8m", a thin progress bar from the previous prayer to the next, then the five prayers in a row with times, next one highlighted, past ones muted.
- After Isyak the next prayer is tomorrow's Subuh.
- No data: "Prayer times unavailable — will retry" with the reason (offline / zone not found). Data older than the current day is never shown as today.

### 5.3 Alerts
- At each prayer time (± the 1 s loop) and at "reminder minutes" before: banner in the notch (60 s for the prayer, 20 s for the reminder) using the existing flash path, plus a Windows notification ("Maghrib — It's time to pray · 18:58" / "Maghrib in 10 min · 18:58").
- Each alert fires once per prayer per day; a restart within the minute does not repeat it (alert memory like the usage alerts) **(Claude's call)**.
- In full screen the notch is hidden; the notification still shows.
- In taskbar mode the capsule is capped at 150 px, so the banner text shortens to "Maghrib · 18:58" **(Claude's call)**.

### 5.4 Settings
- "Prayer times" switch (default off for new installs; the owner turns it on) **(Claude's call)**, "Zone" dropdown, "Remind before" Off / 5 / 10 / 15 min (default 10).

### 5.5 Cost
- Times are computed once per minute from the cached month; no animation runs except the existing banner grow. Measure idle CPU before and after (target unchanged).

## 6. Spikes to run first
- S1 (done): API shape above.
- S2: the zone list (codes and names) from e-solat, stored in the repo as data.

## 7. Left to the builder
- Module layout (suggested `prayer.rs`: parse, next/previous, alert schedule, cache path; fetch through `Runner`).
- Exact copy within the meanings above; tests for parsing, next-prayer across midnight and month end, alert-once, and stale-day handling; README Privacy and acceptance steps.
