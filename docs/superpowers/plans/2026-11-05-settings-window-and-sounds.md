# Settings Window, Sounds and Banners That Stay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move settings into their own window, add bell/azan sounds with the owner's own files, and keep the prayer banner as a small moon until the notch is opened.

**Architecture:** A second Tauri window (`settings`) loads `src/settings.html`; both pages share `src/tokens.css` and talk to the same commands, and `save_settings` broadcasts `settings-changed` so each page reloads its copy. Sounds play through an `<audio>` element in the notch's webview: built-in bells from `src/sounds/`, user files read from the app folder through a binary command and played from a blob URL. The moon is notch-side state that the existing banner flow feeds.

**Tech Stack:** Tauri 2 (Rust), plain HTML/CSS/JS, `tauri-plugin-notification` (unchanged), ffmpeg (only to generate the bell files once).

**Spec:** `docs/superpowers/specs/2026-11-05-settings-window-and-sounds-design.md`

## Global Constraints

- Builds and tests run on Windows through PowerShell from WSL (README "Build from source"); `cargo test` must stay green (195 tests today).
- No new Rust or JS dependencies. ffmpeg is a dev-time tool only; the generated `.ogg` files are committed.
- Settings save as you change them; the notch updates live.
- One sound at a time; the prayer sound wins over a bell.
- Windows notifications stay as they are: they are already silent (notify-rust passes no sound unless one is named), so no double sound.
- No nonstop animation anywhere (each costs ~9 % of a core).
- User sound files: mp3, wav, ogg, m4a; at most 10 MB; copied to `<app data>/sounds/needs.<ext>` or `sounds/azan.<ext>`.
- Every UI task uses the CLAUDE.md design skills (frontend-design, antislop-ui, ui-ux-pro-max, make-interfaces-feel-better, impeccable, accessibility review, design-critique) and is checked live over CDP before it is called done; WCAG AA contrast, visible focus, keyboard reachable, Esc closes the settings window.
- Commit after each task; push and release only when the owner says so.

## Review Focus

1. A prayer time and a new needs-you in the same minute → only the prayer sound plays (Task 5 test `prayer_wins_over_bell`).
2. The user's file is unplayable (deleted, corrupt, odd codec) → the bell plays instead, once, no error loop (Task 5 live check with a renamed `.txt`).
3. Gear and tray both open settings → one window, focused (Task 1 live check).
4. Tray "Hide notch" while the settings window is open → the window reflects it and a later save does not undo it (Task 2 live check).
5. Sound while the notch is hidden (`show_notch` off) or a full-screen app hides it → still plays (Task 5 live check).

---

### Task 1: Settings window shell, tray item and broadcast

**Files:**
- Modify: `src-tauri/src/main.rs` (new command `open_settings`, register it; `save_settings` emits `settings-changed`)
- Modify: `src-tauri/src/tray.rs` (menu item `settings` "Settings…" above "Refresh now"; remove the now-redundant `settings-changed` emit in `toggle_notch`)
- Modify: `src-tauri/capabilities/default.json` (`"windows": ["notch", "settings"]`)
- Create: `src/settings.html` (title "Claude Notch settings"; placeholder body filled in Task 2)

**Interfaces:**
- Produces: `pub fn open_settings(app: AppHandle) -> Result<(), String>` (Tauri command and called by the tray). Creates `WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))` with title "Claude Notch settings", inner size 640 × 460, min size 560 × 400, centred, decorations on, `skip_taskbar(false)`; if a window labelled `settings` exists, `show()` + `unminimize()` + `set_focus()` instead.
- Produces: event `settings-changed` (no payload) after every successful `save_settings`.

- [ ] **Step 1:** Implement `open_settings` and the tray item; register the command.
- [ ] **Step 2:** Build debug and check live: gear call `invoke('open_settings')` twice and tray "Settings…" → exactly one window (`Get-Process` shows one webview window titled "Claude Notch settings"), focused.
- [ ] **Step 3:** `cargo test` → all pass. Commit "Open settings in their own window from the gear and the tray".

### Task 2: Move every setting into the settings window

**Files:**
- Create: `src/tokens.css` (the `@font-face` and the primitive + semantic tokens now in `notch.css` `:root`, with `color-scheme: dark`)
- Create: `src/settings.css`, `src/settings.js`; fill `src/settings.html`
- Create: `src/update.js` (shared by both pages)
- Modify: `src/index.html` (remove the `data-view="settings"` view; gear keeps its place and label "Settings"), `src/notch.css` (link order: tokens.css first; drop settings-only rules), `src/notch.js` (gear → `call('open_settings')`; remove the settings, task-alerts-switch and settings-view code; keep `notch.dataset.align` from `get_settings` on start and on `settings-changed`; `fitPanel` scrolls only `#session-list` again)

**Interfaces:**
- Consumes: `open_settings`, `settings-changed` (Task 1); existing commands `get_settings`, `save_settings`, `list_screens`, `list_distros`, `get_zones`, `get_task_alerts`, `set_task_alerts`, `check_update`, `install_update`.
- Produces: `src/update.js` defines `findUpdate(): Promise<string|null>` (throws when GitHub cannot be reached) and `installUpdate(): Promise<void>`; the notch's top link and the window's About row both use them.
- Produces: settings page groups with ids `g-display`, `g-claude`, `g-prayer`, `g-sounds` (empty until Task 4), `g-alerts`, `g-about`; left nav is a list of buttons with `aria-current="page"` on the shown group.

- [ ] **Step 1:** Move tokens to `tokens.css`; link it from both pages; confirm the notch looks identical (CDP screenshot before/after, same pixels in the closed pill and open panel).
- [ ] **Step 2:** Build the window: left group list, right pane; move each control with its existing id and save handler from `notch.js` into `settings.js` (same `save(patch)` behaviour, same STATES zone grouping, same task-alerts switch). About shows "Version x.y.z" from `window.__TAURI__.app.getVersion()` and the update button with the texts already in use ("Check for updates", "Checking…", "You have the newest version", "Could not check. Try again", "Update to x.y.z", "Downloading x.y.z…", "Installing x.y.z…", "Update failed. Try again").
- [ ] **Step 3:** Live check over CDP on both webviews: each control saves (settings.json changes), the notch moves when Position changes, Show in switches mode, tray "Hide notch" with the window open then a Position change does not show the notch again; Tab reaches every control, focus visible, Esc closes the window; screenshots of every group.
- [ ] **Step 4:** `cargo test` → pass. Commit "Move settings into the settings window".

### Task 3: Sound settings and the user's sound files (Rust)

**Files:**
- Modify: `src-tauri/src/settings.rs` (fields + clamp + tests)
- Create: `src-tauri/src/sounds.rs` (file handling + tests)
- Modify: `src-tauri/src/main.rs` (commands `save_sound`, `remove_sound`, `read_sound`, `sound_files`; `mod sounds;`)

**Interfaces:**
- Produces in `settings.rs`: `pub enum Bell { Chime, Ding, Soft, Double }` and `pub enum SoundChoice { Bell, File, Off }` (serde lowercase); `Settings` gains `bell: Bell` (default `Chime`), `needs_sound: SoundChoice` (default `Bell`), `prayer_sound: SoundChoice` (default `Bell`). Unknown values in the file fall back to defaults (serde default per field).
- Produces in `sounds.rs`: `pub enum Kind { Needs, Azan }` (`"needs"`, `"azan"`); `pub const MAX_BYTES: usize = 10 * 1024 * 1024;` `pub fn ext_of(name: &str) -> Option<&'static str>` (case-insensitive mp3|wav|ogg|m4a); `pub fn save(dir: &Path, kind: Kind, name: &str, bytes: &[u8]) -> Result<(), String>` (rejects bad ext with "Choose an mp3, wav, ogg or m4a file." and size with "Choose a file under 10 MB."; deletes any other `sounds/<kind>.*`, writes atomically); `pub fn find(dir: &Path, kind: Kind) -> Option<PathBuf>`; `pub fn remove(dir: &Path, kind: Kind)`.
- Produces commands: `save_sound(request: tauri::ipc::Request) -> Result<(), String>` reading the raw body as bytes and headers `kind` and `name`; `remove_sound(kind: String)`; `read_sound(kind: String) -> Result<tauri::ipc::Response, String>` (bytes, Err "no file" when none); `sound_files() -> SoundFiles { needs: Option<String>, azan: Option<String> }` (file names for display).

- [ ] **Step 1: Failing tests** in `sounds.rs`: `ext_of_accepts_audio_only` (`"Azan.MP3"`→`Some("mp3")`, `"a.m4a"`→`Some("m4a")`, `"a.exe"`, `"mp3"`, `""`→`None`); `save_replaces_the_old_file` (save `x.wav` then `y.ogg` for `Azan` → only `sounds/azan.ogg` exists, `find` returns it); `save_rejects_big_and_wrong_files` (`MAX_BYTES + 1` bytes → the size message; `"a.txt"` → the type message; nothing written); `remove_deletes_only_that_kind`. In `settings.rs` extend `old_settings_get_prayer_defaults` with the three new defaults and `{"needs_sound":"loud"}` → `Bell`.
- [ ] **Step 2:** Run `cargo test sounds settings` → fail.
- [ ] **Step 3:** Implement; register commands.
- [ ] **Step 4:** `cargo test` → all pass. Commit "Store sound choices and the user's own sound files".

### Task 4: Built-in bells and the Sounds group

**Files:**
- Create: `tools/make_bells.sh` (ffmpeg `aevalsrc` + `loudnorm=I=-18:TP=-1.5`, Vorbis q4, 1–2 s each) and its outputs `src/sounds/chime.ogg`, `ding.ogg`, `soft.ogg`, `double.ogg`
- Modify: `src/settings.html`, `src/settings.js`, `src/settings.css` (group `g-sounds`)
- Modify: `src-tauri/tauri.conf.json` CSP: add `media-src 'self' blob:`

**Interfaces:**
- Consumes: Task 3 settings fields and commands.
- Produces: rows "Bell" (four choices with Play), "Claude needs you" (Bell / My file / Off) and "Prayer" (Bell / My azan / Off), each file row with "Choose file" (`<input type="file" accept=".mp3,.wav,.ogg,.m4a,audio/*">`, sent with `invoke('save_sound', bytes, { headers: { kind, name } })`), the stored file name, "Remove", and Play. Choosing My file with no file opens the picker; errors from `save_sound` show under the row.

- [ ] **Step 1:** Generate the four bells; listen-check by rendering each to the owner's Downloads only if asked; commit the script and files.
- [ ] **Step 2:** Build the group; Play uses `new Audio(...)` in the settings window.
- [ ] **Step 3:** Live check: choose each bell (settings.json `bell` changes), upload a 1 MB mp3 (file appears in `sounds/`), upload a 12 MB file (size message, nothing saved), Remove (file gone, choice back to Bell), keyboard and focus as in Task 2.
- [ ] **Step 4:** Commit "Add built-in bells and the Sounds settings".

### Task 5: Play the sounds in the notch

**Files:**
- Modify: `src-tauri/src/view.rs` (`Banner` gains `pub kind: &'static str` = `"time"` or `"remind"`), `src-tauri/src/main.rs` (`prayer_texts` sets it; test)
- Modify: `src/notch.js` (a `sound` section), `src-tauri/tauri.conf.json` only if Step 2 shows autoplay is blocked

**Interfaces:**
- Consumes: Task 3 settings and `read_sound`; Task 4 bell files.
- Produces in `notch.js`: `playAlert(kind: 'needs' | 'remind' | 'time')`, `stopSound()`. Rules: `time` → azan file if `prayer_sound === 'file'` and readable, else the chosen bell, nothing if `'off'`; `remind` → the bell unless `prayer_sound === 'off'`; `needs` → file if `needs_sound === 'file'`, else bell, nothing if `'off'`. A `time` sound stops any bell; a bell is skipped while a `time` sound plays. A failed file play falls back to the bell once. `setOpen(true)` calls `stopSound()`.
- Triggers: `usage-updated` with `banner` → `playAlert(banner.kind)`; `attention` rising above the last seen value (not on the first view) → `playAlert('needs')`.

- [ ] **Step 1: Failing Rust test** `banner_kind_is_time_or_remind` (an `Alert::Time` gives `kind == "time"`, an `Alert::Remind` gives `"remind"`).
- [ ] **Step 2: Autoplay check:** in the notch over CDP, `new Audio('sounds/chime.ogg').play()` with no click resolves → no config change. If it rejects with NotAllowedError, set the notch window's `additionalBrowserArgs` to `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required` and re-check.
- [ ] **Step 3:** Implement the sound section with a pure helper `pickSound(kind, settings, playingTime) -> 'azan' | 'needs-file' | 'bell' | null` and an in-page self-check `prayer_wins_over_bell` run over CDP: `pickSound('needs', s, true) === null`, `pickSound('time', {prayer_sound:'file'}, false) === 'azan'`, `pickSound('remind', {prayer_sound:'off'}, false) === null`.
- [ ] **Step 4:** Live checks with `NOTCH_PRAYER_TEST_OFFSET_MIN` (back up and restore `prayer-fired.json` as before): time with no azan → bell; with an azan file → azan; with a `.txt` renamed `azan.mp3` in `sounds/` → bell once; open the notch mid-azan → silence; `show_notch` off → still plays.
- [ ] **Step 5:** `cargo test` → pass. Commit "Play the bell and the azan for alerts".

### Task 6: The moon that stays

**Files:**
- Modify: `src/notch.js` (`takeFlash`, `renderPill`, `setOpen`), `src/notch.css` (`.ear__moon` like `.ear__bell`)

**Interfaces:**
- Consumes: `banner` from `usage-updated`/`get_view`.
- Produces: notch state `moon` (bool). After a prayer banner's `seconds` run out, `moon = true` instead of nothing; `renderPill` shows a moon icon in the right ear (blue, like the bell's placement) while `moon`; in `taskbar_mini` the moon replaces the ring; `setOpen(true)` sets `moon = false`; a new banner replaces it. Screen readers: the pill label gains "; a prayer time has passed" while `moon`.

- [ ] **Step 1:** Implement; keep session flashes unchanged.
- [ ] **Step 2:** Live check in top and taskbar modes with the test offset: banner full for its time, then moon beside the numbers; with a bell count of 1 the capsule still fits 150 px with nothing cut (measure as in the capsule fix); open → moon gone.
- [ ] **Step 3:** Commit "Keep a moon in the notch until it is opened after a prayer alert".

### Task 7: Docs, version and review

**Files:**
- Modify: `README.md` (Settings section now describes the window and its groups; Sounds; the moon; checklist "Settings window and sounds (v1.5)" steps 44+), `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` version `1.5.0`, README release example

- [ ] **Step 1:** Update the README; bump the version.
- [ ] **Step 2:** `cargo test`, `cargo tauri build`, install the build over the owner's copy only with their go-ahead; otherwise run the release exe from the target folder for the live pass of Review Focus 1–5.
- [ ] **Step 3:** Whole-branch review (superpowers:requesting-code-review), fix what it finds, commit "Document the settings window and sounds; version 1.5.0". Ask the owner before push and release.
