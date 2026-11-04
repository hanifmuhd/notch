# Prayer Times Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (owner prefers native execution). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show JAKIM prayer times for a chosen zone in the notch panel, and alert with a notch banner plus a Windows notification at each prayer time and N minutes before.

**Architecture:** A pure `prayer.rs` parses JAKIM's month JSON and answers "next / previous prayer" and "which alert is due"; a small fetcher downloads the month with Windows `curl.exe` through the existing `Runner` and caches it beside settings.json. A prayer thread in main.rs sleeps until the next alert moment (capped at 60 s, so sleep and clock changes are caught), then emits the view and fires alerts through the existing flash + toast path; the webview renders a Prayer section and a longer banner.

**Tech Stack:** Tauri v2, Rust (chrono, chrono-tz, serde_json), plain HTML/CSS/JS.

**Spec:** `docs/superpowers/specs/2026-11-04-prayer-times-design.md`

## Global Constraints

- Windows only; no new crate. Download only with `curl.exe` via `source::Runner` (timeout 30 s).
- API: `https://www.e-solat.gov.my/index.php?r=esolatApi/TakwimSolat&period=month&zone=<ZONE>`; fields `date` (`04-Nov-2026`), `fajr`, `dhuhr`, `asr`, `maghrib`, `isha` as `HH:MM:SS`, Malaysia time (UTC+8, `Asia/Kuala_Lumpur`).
- Prayers shown and alerted (exact names): Subuh, Zohor, Asar, Maghrib, Isyak.
- Cache file: `prayer-<ZONE>-<YYYY-MM>.json` beside settings.json; retry a missing month hourly.
- Settings (exact): `prayer_on` (default false), `prayer_zone` (default `"WLY01"`), `prayer_remind` minutes in {0, 5, 10, 15} (default 10).
- Copy (exact): banner `<Prayer> · It's time to pray · <HH:MM>` (60 s); reminder `<Prayer> in <N> min · <HH:MM>` (20 s); toast titles `<Prayer> — It's time to pray` / `<Prayer> in <N> min`, body `<HH:MM> · <zone place>`; panel heading `Prayer · <zone place>`; no data `Prayer times unavailable — will retry`; settings labels `Prayer times`, `Zone`, `Remind before`.
- Each alert fires once per prayer per day, also across a restart (saved with the alert memory).
- No new nonstop animation; idle CPU must not rise (measure before and after, 30 s windows, all notch processes).
- Only the zone code leaves the PC; README Privacy updated.
- Run Rust tests through PowerShell (`cargo test`, CARGO_TARGET_DIR `C:\Users\hanif\code\notch-target`); never cargo from Linux.

## Review Focus

1. After Isyak, the next prayer is tomorrow's Subuh, also across a month end (needs next month's file). → Task 2 test `next_after_isyak_is_tomorrow_subuh` and `month_end_without_next_month_is_none`.
2. The PC was asleep through a prayer: waking 2 h later must not fire a stale "time to pray". → Task 2 test `missed_alert_older_than_window_is_skipped`.
3. Zone changed mid-month: old cache must not be shown for the new zone. → Task 3 test `cache_path_includes_zone_and_month`.
4. JAKIM down or offline at month start: panel says unavailable, nothing alerts with old data. → Task 3 test `today_missing_is_unavailable`.
5. Malformed JSON or `status` not `OK!`: no panic, treated as missing. → Task 2 test `bad_month_json_is_none`.

---

### Task 1: Zone list (spike S2)

**Files:** Create `src-tauri/src/prayer_zones.rs` (`pub const ZONES: &[(&str, &str)]` code → place names), `docs/spikes/jakim-zones.md` (source and date).

- [ ] Fetch the e-solat zone list (the zone `<select>` on the e-solat site or the API's zone listing), keep code + place text, record the source URL and count in the spike note.
- [ ] Test `zones_include_wly01`: `ZONES` contains `("WLY01", <text containing "Kuala Lumpur">)`, codes unique, at least 50 zones.
- [ ] Commit `Add the JAKIM prayer zone list`.

### Task 2: Pure prayer logic

**Files:** Create `src-tauri/src/prayer.rs`.

**Interfaces:**
- Produces: `pub enum Prayer { Subuh, Zohor, Asar, Maghrib, Isyak }` with `name()`; `pub struct Day { date: NaiveDate, times: [DateTime<Utc>; 5] }`; `pub fn parse_month(json: &str) -> Option<Vec<Day>>`; `pub fn next(days: &[Day], now: DateTime<Utc>) -> Option<(Prayer, DateTime<Utc>)>`; `pub fn previous(days: &[Day], now) -> Option<DateTime<Utc>>`; `pub fn today<'a>(days: &'a [Day], now) -> Option<&'a Day>` (Malaysia date); `#[derive(Serialize, Deserialize, Default)] pub struct Fired { keys: Vec<String> }`; `pub enum Alert { Time(Prayer, DateTime<Utc>), Remind(Prayer, DateTime<Utc>, u32) }`; `pub fn due(days: &[Day], now, remind_min: u32, fired: &mut Fired) -> Vec<Alert>`; `pub fn next_wake(days: &[Day], now, remind_min: u32) -> Option<DateTime<Utc>>` (the next prayer or reminder moment after `now`) (key `YYYY-MM-DD-<prayer>-<time|remind>`, window: fire if `now` is within [moment, moment + 2 min], else skip; keep only the last 3 days of keys).

- [ ] Failing tests: `parses_jakim_month` (fixture from the S1 response saved as `src-tauri/tests/fixtures/jakim-WLY01.json`: 4 Nov Maghrib = 18:58 +08), `bad_month_json_is_none` (garbage, `status` not `OK!`, missing field), `next_after_isyak_is_tomorrow_subuh`, `month_end_without_next_month_is_none`, `due_fires_once` (at Maghrib → Time once; again → nothing), `reminder_fires_n_minutes_before` (10 → Remind at 18:48; 0 → none), `missed_alert_older_than_window_is_skipped` (now = Maghrib + 2 h → nothing), `fired_keeps_three_days`, `next_wake_is_next_reminder_or_prayer` (17:00 with remind 10 → 18:48; 18:50 → 18:58).
- [ ] Implement; `WINTEST prayer::` → PASS.
- [ ] Commit `Parse JAKIM prayer times and decide which alert is due`.

### Task 3: Settings, download and cache

**Files:** Modify `src-tauri/src/settings.rs` (3 fields), create fetch/cache fns in `prayer.rs`, modify `app_state.rs` (prayer days + Fired saved with alerts.json or a `prayer-fired.json`).

**Interfaces:** `pub fn cache_path(dir: &Path, zone: &str, month: (i32, u32)) -> PathBuf`; `pub fn fetch_month(runner: &dyn Runner, zone: &str) -> Option<String>` (curl `-s -f -m 30 -A ClaudeNotch <url>`; returns body only when `parse_month` succeeds); `pub fn load_days(dir, zone, now) -> Vec<Day>` (this month + next month if cached).

- [ ] Failing tests: `cache_path_includes_zone_and_month`, `fetch_uses_curl_with_zone` (FakeRunner sees `curl.exe -s -f -m 30 -A ClaudeNotch https://...zone=WLY02`), `fetch_rejects_bad_body`, `today_missing_is_unavailable` (no cache → `today()` None), `old_settings_get_prayer_defaults`.
- [ ] Implement; `WINTEST` → PASS.
- [ ] Commit `Download and cache JAKIM prayer times by zone`.

### Task 4: Loop, alerts and view model

**Files:** Modify `src-tauri/src/main.rs` (prayer thread: sleep until `next_wake()`, at most 60 s; when `prayer_on`, ensure this and next month cached (fetch on miss, retry hourly), `due()` → toast + flash text, emit), `src-tauri/src/view.rs` (`ViewModel.prayer: Option<PrayerView { place, next_name, next_time, next_in, progress_pct, times: Vec<(name, HH:MM, state "past"|"next"|"later")>, unavailable: bool }>` and `banner: Option<(String, u32 /*seconds*/)>`).

- [ ] Failing tests (view.rs): `prayer_view_next_and_progress` (now 18:00 on 4 Nov → next Maghrib 18:58, `in 58m`, progress between Asar 16:20 and Maghrib), `prayer_view_unavailable`.
- [ ] Implement the loop (no work when `prayer_on` is off); toasts via the existing notification builder; banner text per Global Constraints.
- [ ] `WINTEST` → PASS; commit `Alert at prayer times and show the next prayer in the view`.

### Task 5: Panel section, banner and settings UI

**Files:** `src/index.html`, `src/notch.js`, `src/notch.css`.

- [ ] Prayer section between the limit lines and Sessions (hidden when `prayer` is null): heading, next prayer large + time + "in …", thin progress bar, row of five times (next highlighted with `--week`, past muted). Banner: the closed notch grows to show a moon icon + banner text for the given seconds (reuse flash; taskbar mode shows `<Prayer> · <HH:MM>` within 150 px). Settings: `Prayer times` switch, `Zone` select from `ZONES` (a `get_zones` command), `Remind before` segmented Off / 5 / 10 / 15.
- [ ] CDP check: panel section and banner screenshots (top and taskbar), settings reachable by keyboard, no console errors; CPU before/after (30 s, all processes) unchanged.
- [ ] Commit `Show prayer times in the panel with a prayer banner and settings`.

### Task 6: README, release, live check

- [ ] README: feature, settings, Privacy (only the zone code goes to e-solat.gov.my, once a month), acceptance steps.
- [ ] Release build, owner's notch on it; live: turn on with WLY01, confirm today's five times match e-solat, force a near-future alert via a test-only clock offset env var `NOTCH_PRAYER_TEST_OFFSET_MIN` (debug builds only) to see banner + toast.
- [ ] Commit `Document prayer times`; then final whole-branch review and fix pass.
