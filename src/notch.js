// Renders the usage ViewModel from Rust (view.rs) and drives the panel and settings (spec 5.2-5.4, 5.7).
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (sel) => document.querySelector(sel);
const notch = $('#notch');
const pill = $('#pill');
const panel = $('#panel');
const views = { usage: $('[data-view="usage"]'), settings: $('[data-view="settings"]') };
const LEVEL_CLASS = { normal: '', alert: 'is-warn', full: 'is-danger' };
const CLOSE_DELAY = 150; // hover-out grace (spec 5.2)
const FADE_OUT = 120; // matches the panel's closing transition

const call = (cmd, args) => invoke(cmd, args).catch((e) => console.error(cmd, e));

// ---------- rendering ----------

const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

const stat = (label, value, cls = '') => {
  const s = el('span', 'stat');
  s.append(el('span', 'stat__label', label), el('span', ('stat__value ' + cls).trim(), value));
  return s;
};

const alertIcon = (cls) => {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('class', 'icon ' + cls);
  svg.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
  use.setAttribute('href', '#i-alert');
  svg.append(use);
  return svg;
};

/** "Session full, back in 1h 20m" -> ["Session full, back in", "1h 20m"]. */
const splitFull = (text) => {
  const m = /^(.*? back(?: in)?) (.+)$/.exec(text);
  return m ? [m[1], m[2]] : [text, ''];
};

const bellIcon = () => {
  const svg = alertIcon('');
  svg.querySelector('use').setAttribute('href', '#i-bell');
  return svg;
};

// A new session notice replaces the pill's content for 6 s (spec 5.3); a newer one restarts it.
const FLASH_MS = 6000;
let flashText = null;
let flashTimer;
let lastVm = null;

function renderPill(vm) {
  const parts = [];
  if (flashText) {
    parts.push(el('span', 'stat__value', flashText));
  } else if (vm.pill === 'no_claude') {
    parts.push(el('span', 'stat__label', 'Install Claude Code to see your limits'));
  } else if (vm.pill === 'logged_out') {
    parts.push(el('span', 'stat__label', 'Log in to Claude Code'));
  } else if (vm.pill === 'no_data' && vm.refreshing) {
    parts.push(el('span', 'stat__label', 'Updating…'));
  } else {
    if (vm.pill === 'alert' || vm.pill === 'full') parts.push(alertIcon(vm.pill === 'full' ? 'is-danger' : 'is-warn'));
    const fullKey = vm.pill === 'full' ? (vm.session.level === 'full' ? 'session' : 'week') : null;
    for (const [key, name] of [['session', 'Session'], ['week', 'Week']]) {
      if (key === fullKey) {
        const [label, value] = splitFull(vm.pill_text || '');
        parts.push(stat(label, value, 'is-danger'));
      } else {
        parts.push(stat(name, vm[key].value_text, LEVEL_CLASS[vm[key].level]));
      }
    }
    if (vm.pill === 'stale') parts.push(stat('As of', vm.updated_text.replace(/^As of /, '')));
  }
  if (!flashText && vm.attention > 0) {
    const s = stat('', String(vm.attention));
    s.firstChild.replaceWith(bellIcon());
    s.append(el('span', 'sr-only', vm.attention === 1 ? ' session needs a look' : ' sessions need a look'));
    parts.push(s);
  }
  pill.replaceChildren(...parts);
  pill.classList.toggle('is-stale', ['stale', 'no_data', 'no_claude', 'logged_out'].includes(vm.pill));
}

function renderLimit(key, lv) {
  const root = $(`[data-limit="${key}"]`);
  const f = (name) => root.querySelector(`[data-f="${name}"]`);
  const cls = LEVEL_CLASS[lv.level];
  const hasData = lv.value_text !== '–'; // an idle limit has a reading but no pace
  f('value').textContent = lv.value_text;
  f('value').className = ('limit__value ' + cls).trim();
  f('fill').className = ('meter__fill ' + cls).trim();
  f('fill').style.width = lv.used_pct + '%';
  f('pace').hidden = lv.pace_text === '';
  f('pace').style.left = lv.pace_pct + '%';
  f('meter').setAttribute('aria-valuenow', String(Math.round(lv.used_pct)));
  f('meter').setAttribute('aria-valuetext', hasData ? `${lv.value_text} used, ${lv.pace_text || lv.reset_text}` : 'No reading yet');
  f('reset').textContent = lv.reset_text;
  f('pace-text').textContent = lv.pace_text;
  f('runs').textContent = lv.runs_out_text || '';
  f('runs').hidden = !lv.runs_out_text;
}

/** "now" under a minute, then "2m", then "1h". */
const ageText = (sinceMs) => {
  const mins = Math.floor((Date.now() - sinceMs) / 60000);
  return mins < 1 ? 'now' : mins < 60 ? `${mins}m` : `${Math.floor(mins / 60)}h`;
};

function renderSessions(vm) {
  const rows = vm.sessions || [];
  $('#sessions').hidden = rows.length === 0;
  $('#session-list').replaceChildren(...rows.map((r) => {
    const li = el('li', 'session');
    const needs = r.status === 'needs_you';
    li.append(
      el('span', 'session__project', r.project || 'Claude'),
      el('span', 'session__status' + (needs ? ' is-warn' : ''), needs ? 'needs you' : 'finished'),
      el('span', 'session__age', ageText(r.since_ms)),
    );
    return li;
  }));
}

function render(vm) {
  lastVm = vm;
  renderPill(vm);
  renderSessions(vm);
  fitClosed();
  const noClaude = vm.pill === 'no_claude';
  // Logged out hides the old numbers too; the footer text carries the log-in hint.
  const hideLimits = noClaude || vm.pill === 'logged_out';
  $('#notice').hidden = !noClaude;
  for (const key of ['session', 'week']) {
    $(`[data-limit="${key}"]`).hidden = hideLimits;
    renderLimit(key, vm[key]);
  }
  $('#updated').textContent = vm.refreshing ? 'Updating…' : noClaude ? '' : vm.updated_text;
  const refresh = $('#refresh');
  refresh.setAttribute('aria-label', vm.refreshing ? 'Refreshing' : 'Refresh now');
  if (vm.refreshing) refresh.setAttribute('aria-busy', 'true');
  else refresh.removeAttribute('aria-busy');
}

// Only the usage-updated event may speak; get_view and save_settings also carry `announce`.
function announce(text) {
  const live = $('#announce');
  live.textContent = '';
  requestAnimationFrame(() => { live.textContent = text; });
}

// ---------- open / close ----------

let isOpen = false;
let pinned = false;
let menuOpen = false; // a native <select> popup is showing; it can blur the page in WebView2
let closeTimer;
let fadeTimer;

let closing = false;
let ageTimer;
let sentWidth = 0;

const sizePanel = () => call('set_panel_open', { open: true, height: Math.ceil(panel.offsetHeight) });

// Closed, the window is exactly the pill, so clicks beside it reach the app below (spec 5.7).
function fitClosed() {
  if (isOpen || closing) return;
  const width = Math.ceil(parseFloat(getComputedStyle(pill).width));
  if (width === sentWidth) return;
  sentWidth = width;
  call('set_panel_open', { open: false, height: 36, width });
}

async function setOpen(open) {
  clearTimeout(closeTimer);
  if (open === isOpen) return;
  isOpen = open;
  pill.setAttribute('aria-expanded', String(open));
  clearTimeout(fadeTimer);
  closing = !open;
  clearInterval(ageTimer);
  if (open) {
    await sizePanel(); // grow the window before the panel shows
    if (isOpen) notch.classList.add('is-open');
    ageTimer = setInterval(() => { if (lastVm) renderSessions(lastVm); }, 30000);
  } else {
    notch.classList.remove('is-open');
    call('sessions_seen'); // closing after a look clears finished rows (spec 5.2)
    fadeTimer = setTimeout(() => {
      showView('usage');
      closing = false;
      sentWidth = 0;
      fitClosed();
    }, FADE_OUT);
  }
}

function showView(name, focusEl) {
  for (const [k, v] of Object.entries(views)) v.hidden = k !== name;
  if (focusEl) focusEl.focus();
}

// The settings view is taller than the usage view, and both change with data.
new ResizeObserver(() => { if (isOpen) sizePanel(); }).observe(panel);

pill.addEventListener('click', () => { pinned = !pinned; setOpen(pinned); });
// Hover opens the panel over the pill, so a click inside it is the "opened by click" pin.
panel.addEventListener('pointerdown', () => { pinned = true; });
notch.addEventListener('mouseenter', () => setOpen(true));
notch.addEventListener('mouseleave', () => {
  if (!pinned) closeTimer = setTimeout(() => setOpen(false), CLOSE_DELAY);
});
// Clicking another window unpins, but not while a select's own popup takes the focus.
window.addEventListener('blur', () => {
  if (pinned && !menuOpen) { pinned = false; setOpen(false); }
});
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  pinned = false;
  setOpen(false);
  pill.focus();
  call('focus_previous');
});

$('#open-settings').addEventListener('click', () => { showView('settings', $('#close-settings')); loadTaskAlerts(); });
$('#close-settings').addEventListener('click', () => showView('usage', $('#open-settings')));
$('#refresh').addEventListener('click', () => call('refresh_now'));
$('#install').addEventListener('click', () => call('open_install_page'));

// ---------- settings ----------

let settings = null;
let distros = [];
const screenSel = $('#s-screen');
const sourceSel = $('#s-source');
const MENU_KEYS = [' ', 'Enter', 'F4'];

for (const sel of [screenSel, sourceSel]) {
  sel.addEventListener('pointerdown', () => { menuOpen = true; });
  sel.addEventListener('keydown', (e) => {
    if (MENU_KEYS.includes(e.key) || (e.altKey && e.key === 'ArrowDown')) menuOpen = true;
    else if (e.key === 'Escape') menuOpen = false;
  });
  sel.addEventListener('change', () => { menuOpen = false; });
  sel.addEventListener('blur', () => { menuOpen = false; });
}

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
  $('#s-start-wsl').checked = settings.start_wsl;
  $('#s-start-wsl-row').hidden = settings.source.kind !== 'wsl';
  $('#s-task-finished').checked = settings.task_finished;
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
    await invoke('save_settings', { settings }); // the view arrives through usage-updated
  } catch (e) {
    console.error('save_settings', e);
    settings = await call('get_settings') || settings;
    renderSettings();
  }
}

screenSel.addEventListener('change', () => save({ screen: screenSel.value }));
for (const r of document.querySelectorAll('input[name="position"]')) {
  r.addEventListener('change', () => save({ position: r.value }));
}
sourceSel.addEventListener('change', () => {
  const v = sourceSel.value;
  save({ source: v.startsWith('wsl:') ? { kind: 'wsl', distro: v.slice(4) } : { kind: v } });
});
$('#s-start-wsl').addEventListener('change', (e) => save({ start_wsl: e.target.checked }));
$('#s-task-finished').addEventListener('change', (e) => save({ task_finished: e.target.checked }));

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
for (const input of document.querySelectorAll('[data-threshold]')) {
  const key = input.dataset.threshold;
  input.addEventListener('input', () => { $(`[data-out="${key}"]`).textContent = input.value + '%'; });
  input.addEventListener('change', () => save({ [key + '_threshold']: Number(input.value) }));
}

// ---------- start ----------

(async () => {
  // The pill is measured, so every weight must be loaded first (500 is only used when stale).
  await Promise.all([400, 500, 600].map((w) => document.fonts.load(`${w} 14px "Public Sans"`))).catch(() => {});
  // Listen before the first fetch, so a refresh that finishes meanwhile is neither missed nor its announcement lost.
  await listen('usage-updated', (e) => {
    if (e.payload.flash) {
      flashText = e.payload.flash; // the same text arrives in `announce`, so it is spoken once
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => { flashText = null; if (lastVm) render(lastVm); }, FLASH_MS);
    }
    render(e.payload);
    if (e.payload.announce) announce(e.payload.announce);
  });
  // The tray can change show_notch; take the file's settings so the next save does not undo it.
  await listen('settings-changed', async () => {
    const s = await call('get_settings');
    if (!s) return; // not loaded yet, or the call failed; keep what is shown
    settings = s;
    renderSettings();
  });
  const vm = await call('get_view');
  if (vm) render(vm);
  const [s, screens] = await Promise.all([call('get_settings'), call('list_screens')]);
  if (s) {
    settings = s;
    renderSettings(screens || []);
  }
  // `wsl -l` can take seconds, so the distro list fills in afterwards.
  distros = (await call('list_distros')) || [];
  if (settings) renderSettings();
})();
