// The settings window: loads the settings, saves each change at once, and reloads when another
// page or the tray saves (settings-changed). The notch follows through the same event.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (sel) => document.querySelector(sel);
const call = (cmd, args) => invoke(cmd, args).catch((e) => console.error(cmd, e));
const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

// ---------- groups ----------

const navItems = [...document.querySelectorAll('.nav__item')];
function showGroup(name) {
  for (const b of navItems) {
    const on = b.dataset.group === name;
    if (on) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
    $(`#g-${b.dataset.group}`).hidden = !on;
  }
  if (name === 'claude') loadTaskAlerts();
}
for (const b of navItems) b.addEventListener('click', () => showGroup(b.dataset.group));
document.addEventListener('keydown', (e) => {
  // Esc closes the window, unless it is closing an open select list.
  if (e.key === 'Escape' && !(e.target instanceof HTMLSelectElement)) window.__TAURI__.window.getCurrentWindow().close();
});

// ---------- settings ----------

let settings = null;
let distros = [];
let zones = [];
const screenSel = $('#s-screen');
const sourceSel = $('#s-source');
const zoneSel = $('#s-zone');
// JAKIM zone codes start with the state's code.
const STATES = { JHR: 'Johor', KDH: 'Kedah', KTN: 'Kelantan', MLK: 'Melaka', NGS: 'Negeri Sembilan', PHG: 'Pahang',
  PLS: 'Perlis', PNG: 'Pulau Pinang', PRK: 'Perak', SBH: 'Sabah', SGR: 'Selangor', SWK: 'Sarawak', TRG: 'Terengganu', WLY: 'Wilayah Persekutuan' };

const sourceValue = (src) => (src.kind === 'wsl' ? 'wsl:' + src.distro : src.kind);

function fillSelect(sel, options, value) {
  sel.replaceChildren(...options.map(([v, label]) => {
    const o = el('option', null, label);
    o.value = v;
    return o;
  }));
  sel.value = value;
}

function renderSettings(screens) {
  if (screens) {
    const primary = screens.find((s) => s.primary) || screens[0];
    const chosen = screens.some((s) => s.id === settings.screen) ? settings.screen : primary && primary.id;
    fillSelect(screenSel, screens.map((s) => [s.id, s.label]), chosen);
  }
  const names = [...distros];
  if (settings.source.kind === 'wsl' && !names.includes(settings.source.distro)) names.push(settings.source.distro);
  fillSelect(sourceSel, [['auto', 'Auto'], ['windows', 'Windows'], ...names.map((d) => ['wsl:' + d, 'WSL: ' + d])], sourceValue(settings.source));
  for (const r of document.querySelectorAll('input[name="position"]')) r.checked = r.value === settings.position;
  for (const r of document.querySelectorAll('input[name="show_in"]')) r.checked = r.value === settings.show_in;
  for (const row of document.querySelectorAll('[data-top-only]')) row.hidden = settings.show_in === 'taskbar';
  $('#s-taskbar-hint').hidden = settings.show_in !== 'taskbar';
  $('#s-stay-behind').checked = settings.stay_behind;
  $('#s-start-wsl').checked = settings.start_wsl;
  $('#s-start-wsl-row').hidden = settings.source.kind !== 'wsl';
  $('#s-task-finished').checked = settings.task_finished;
  $('#s-prayer-on').checked = settings.prayer_on;
  for (const row of document.querySelectorAll('[data-prayer-only]')) row.hidden = !settings.prayer_on;
  for (const r of document.querySelectorAll('input[name="prayer_remind"]')) r.checked = Number(r.value) === settings.prayer_remind;
  if (zones.length && zoneSel.options.length === 0) {
    const groups = new Map();
    for (const [code, place] of zones) {
      const state = STATES[code.slice(0, 3)] || code.slice(0, 3);
      if (!groups.has(state)) groups.set(state, el('optgroup'));
      const g = groups.get(state);
      g.label = state;
      const o = el('option', null, `${code} · ${place}`);
      o.value = code;
      g.append(o);
    }
    zoneSel.replaceChildren(...groups.values());
  }
  zoneSel.value = settings.prayer_zone;
  for (const name of ['bell', 'needs_sound', 'prayer_sound']) {
    for (const r of document.querySelectorAll(`input[name="${name}"]`)) r.checked = r.value === settings[name];
  }
  renderSounds();
  for (const key of ['session', 'week']) {
    $(`#t-${key}`).value = settings[key + '_threshold'];
    $(`[data-out="${key}"]`).textContent = settings[key + '_threshold'] + '%';
  }
}

async function save(patch) {
  if (!settings) return;
  settings = { ...settings, ...patch };
  renderSettings();
  try {
    await invoke('save_settings', { settings });
  } catch (e) {
    console.error('save_settings', e);
    settings = (await call('get_settings')) || settings;
    renderSettings();
  }
}

screenSel.addEventListener('change', () => save({ screen: screenSel.value }));
for (const r of document.querySelectorAll('input[name="position"]')) r.addEventListener('change', () => save({ position: r.value }));
for (const r of document.querySelectorAll('input[name="show_in"]')) r.addEventListener('change', () => save({ show_in: r.value }));
sourceSel.addEventListener('change', () => {
  const v = sourceSel.value;
  save({ source: v.startsWith('wsl:') ? { kind: 'wsl', distro: v.slice(4) } : { kind: v } });
});
$('#s-stay-behind').addEventListener('change', (e) => save({ stay_behind: e.target.checked }));
$('#s-start-wsl').addEventListener('change', (e) => save({ start_wsl: e.target.checked }));
$('#s-task-finished').addEventListener('change', (e) => save({ task_finished: e.target.checked }));
$('#s-prayer-on').addEventListener('change', (e) => save({ prayer_on: e.target.checked }));
zoneSel.addEventListener('change', () => save({ prayer_zone: zoneSel.value }));
for (const r of document.querySelectorAll('input[name="prayer_remind"]')) r.addEventListener('change', () => save({ prayer_remind: Number(r.value) }));
for (const input of document.querySelectorAll('[data-threshold]')) {
  const key = input.dataset.threshold;
  input.addEventListener('input', () => { $(`[data-out="${key}"]`).textContent = input.value + '%'; });
  input.addEventListener('change', () => save({ [key + '_threshold']: Number(input.value) }));
}

// ---------- sounds ----------

const SOUND_KEY = { needs: 'needs_sound', azan: 'prayer_sound' };
const MAX_BYTES = 10 * 1024 * 1024;
let files = { needs: null, azan: null }; // the stored file names
let preview = null;
let previewUrl = null;

function renderSounds() {
  for (const box of document.querySelectorAll('.sound')) {
    const kind = box.dataset.kind;
    box.querySelector('.file-row__name').textContent = files[kind] ? `Your file: ${files[kind]}` : 'No file added yet';
    box.querySelector('[data-remove]').hidden = !files[kind];
    box.querySelector('[data-play]').disabled = settings && settings[SOUND_KEY[kind]] === 'off';
  }
}

function soundError(kind, text) {
  const p = document.querySelector(`.sound[data-kind="${kind}"] .is-error`);
  p.textContent = text || '';
  p.hidden = !text;
}

async function play(kind) {
  if (preview) preview.pause();
  if (previewUrl) URL.revokeObjectURL(previewUrl);
  previewUrl = null;
  const choice = kind === 'bell' ? 'bell' : settings[SOUND_KEY[kind]];
  let src = `sounds/${settings.bell}.ogg`;
  if (choice === 'file' && files[kind]) src = previewUrl = URL.createObjectURL(new Blob([await invoke('read_sound', { kind })]));
  preview = new Audio(src);
  preview.play().catch((e) => { if (kind !== 'bell') soundError(kind, 'This file cannot be played. Choose another one.'); console.error(e); });
}

async function loadFiles() {
  files = (await call('sound_files')) || files;
  if (settings) renderSounds();
}

for (const name of ['bell', 'needs_sound', 'prayer_sound']) {
  for (const r of document.querySelectorAll(`input[name="${name}"]`)) {
    r.addEventListener('change', () => {
      const kind = name === 'needs_sound' ? 'needs' : 'azan';
      // My file with no file yet: ask for one first; the choice is saved once it is stored.
      if (r.value === 'file' && !files[kind]) {
        r.checked = false;
        document.querySelector(`.sound[data-kind="${kind}"] input[type="file"]`).click();
        renderSettings();
        return;
      }
      save({ [name]: r.value });
    });
  }
}

for (const box of document.querySelectorAll('.sound')) {
  const kind = box.dataset.kind;
  const picker = box.querySelector('input[type="file"]');
  box.querySelector('[data-choose]').addEventListener('click', () => picker.click());
  picker.addEventListener('change', async () => {
    const file = picker.files[0];
    picker.value = '';
    if (!file) return;
    if (file.size > MAX_BYTES) return soundError(kind, 'Choose a file under 10 MB.');
    try {
      await invoke('save_sound', new Uint8Array(await file.arrayBuffer()), { headers: { kind, name: encodeURIComponent(file.name) } });
      soundError(kind, '');
      await loadFiles();
      save({ [SOUND_KEY[kind]]: 'file' });
    } catch (e) {
      soundError(kind, String(e));
    }
  });
  box.querySelector('[data-remove]').addEventListener('click', async () => {
    await call('remove_sound', { kind });
    await loadFiles();
    if (settings[SOUND_KEY[kind]] === 'file') save({ [SOUND_KEY[kind]]: 'bell' });
    box.querySelector('[data-choose]').focus(); // the Remove button is gone
  });
}
for (const b of document.querySelectorAll('[data-play]')) b.addEventListener('click', () => play(b.dataset.play));

// ---------- task alerts switch (reads Claude's settings.json, may take seconds on WSL) ----------

const taskSwitch = $('#s-task-alerts');
const taskHint = $('#s-task-hint');

function showTaskAlerts(t, error) {
  taskSwitch.checked = !!(t && t.on);
  taskSwitch.disabled = !(t && t.available);
  const hint = error || (t && t.hint) || '';
  taskHint.textContent = hint;
  taskHint.hidden = !hint;
}

async function loadTaskAlerts() {
  taskSwitch.disabled = true;
  try { showTaskAlerts(await invoke('get_task_alerts')); } catch (e) { showTaskAlerts(null, String(e)); }
}

taskSwitch.addEventListener('change', async () => {
  const on = taskSwitch.checked;
  taskSwitch.disabled = true; // one edit at a time
  try { showTaskAlerts(await invoke('set_task_alerts', { on })); } catch (e) {
    taskSwitch.checked = !on;
    taskSwitch.disabled = false;
    taskHint.textContent = String(e);
    taskHint.hidden = false;
  }
});

// ---------- about: version and updates ----------

const updateBtn = $('#s-update');
let updateTo = null;
updateBtn.addEventListener('click', async () => {
  updateBtn.disabled = true;
  if (updateTo) {
    updateBtn.textContent = `Downloading ${updateTo}…`;
    try {
      await invoke('install_update'); // the installer then closes the app and starts the new one
      updateBtn.textContent = `Installing ${updateTo}…`;
    } catch (e) {
      console.error('install_update', e);
      updateBtn.textContent = 'Update failed. Try again';
      updateBtn.disabled = false;
    }
    return;
  }
  updateBtn.textContent = 'Checking…';
  try {
    updateTo = await invoke('check_update');
    updateBtn.textContent = updateTo ? `Update to ${updateTo}` : 'You have the newest version';
  } catch {
    updateBtn.textContent = 'Could not check. Try again';
  }
  updateBtn.disabled = false;
});
window.__TAURI__.app.getVersion().then((v) => { $('#s-version').textContent = `Version ${v}`; }).catch(() => {});

// ---------- start ----------

(async () => {
  // The tray or the notch may save too; reload so a later save here does not undo theirs.
  await listen('settings-changed', async () => {
    const s = await call('get_settings');
    if (s) { settings = s; renderSettings(); }
  });
  const [s, screens, z, f] = await Promise.all([call('get_settings'), call('list_screens'), call('get_zones'), call('sound_files')]);
  files = f || files;
  zones = z || [];
  if (s) {
    settings = s;
    renderSettings(screens || []);
  }
  // `wsl -l` can take seconds, so the distro list fills in afterwards.
  distros = (await call('list_distros')) || [];
  if (settings) renderSettings();
})();
