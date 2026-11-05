// Renders the usage ViewModel from Rust (view.rs) and drives the notch and its panel
// (redesign spec 5.1-5.4). Rust computes every text; this file draws rings and moves the shape.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (sel) => document.querySelector(sel);
const notch = $('#notch');
const pill = $('#pill');
const panel = $('#panel');
const pod = $('#pod');
const usageView = $('[data-view="usage"]');
const HOVER_INTENT = 300; // the pointer must rest this long before hover opens (spec 5.2)
const CLOSE_DELAY = 150; // hover-out grace
const MORPH = 500; // matches the size spring in notch.css
const FLASH_MS = 6000; // a new session notice shows in the notch this long

const call = (cmd, args) => invoke(cmd, args).catch((e) => console.error(cmd, e));
const reduced = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

// ---------- small builders ----------

const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};

const SVG = 'http://www.w3.org/2000/svg';
const svgEl = (tag, attrs) => {
  const n = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  return n;
};

const icon = (id, cls = 'icon') => {
  const s = svgEl('svg', { class: cls, 'aria-hidden': 'true' });
  s.append(svgEl('use', { href: '#' + id }));
  return s;
};

/**
 * The double ring: session outside, week inside (spec 5.1). `size` in px; percentages 0-100.
 * The outer ring turns amber at the alert level and solid red when full.
 */
function ring(vm, size, big = false) {
  // The dial keeps a wide centre for its text; the small ring is bolder so it reads at 20 px.
  const [ro, ri] = big ? [10.6, 8.5] : [9.6, 5.2];
  const s = svgEl('svg', { width: size, height: size, viewBox: '0 0 24 24', class: 'ring', 'aria-hidden': 'true' });
  const arc = (r, pct, cls) => {
    const c = 2 * Math.PI * r;
    s.append(svgEl('circle', { cx: 12, cy: 12, r, class: 'ring__track' }));
    if (pct > 0) {
      s.append(svgEl('circle', {
        cx: 12, cy: 12, r, class: 'ring__arc ' + cls,
        'stroke-dasharray': `${(Math.min(pct, 100) / 100) * c} ${c}`,
        transform: 'rotate(-90 12 12)',
      }));
    }
  };
  const lvl = vm.session.level;
  arc(ro, lvl === 'full' ? 100 : vm.session.used_pct, lvl === 'full' ? 'is-full' : lvl === 'alert' ? 'is-alert' : 'is-session');
  arc(ri, vm.week.used_pct, vm.week.level === 'full' ? 'is-full' : vm.week.level === 'alert' ? 'is-alert' : 'is-week');
  return s;
}

// ---------- state ----------

let lastVm = null;
let flash = null; // { text, icon, prayer }: a session notice or a prayer banner shown in the closed notch
let flashTimer;
let moon = false; // a prayer banner ran out while the notch was closed; a small moon stays until it is opened
let tasksOn = null; // task alerts switch, read lazily (it may take seconds over WSL)

/** "now" under a minute, then "2m", then "1h". */
const ageText = (sinceMs) => {
  const mins = Math.floor((Date.now() - sinceMs) / 60000);
  return mins < 1 ? 'now' : mins < 60 ? `${mins}m` : `${Math.floor(mins / 60)}h`;
};

const LEVEL = { normal: '', alert: 'is-alert', full: 'is-full' };
const hasNumbers = (vm) => !['no_claude', 'logged_out', 'no_data'].includes(vm.pill);

// ---------- closed notch: rings and % on the left ear, countdown on the right ----------

// In the taskbar there is no pod, so a still blue dot on the ring's corner says a session is
// working (still, not blinking: a nonstop animation costs ~9 % of a core).
const working = (vm) => vm.mode !== 'top' && vm.pod && vm.pod.status === 'working';
function pillRing(vm) {
  const r = ring(vm, 20);
  if (working(vm)) r.append(svgEl('circle', { cx: 19.6, cy: 4.4, r: 3.8, class: 'ring__busy' }));
  return r;
}

function renderPill(vm) {
  const left = el('span', 'ear ear--left');
  const right = el('span', 'ear ear--right');
  if (vm.mode === 'taskbar_mini') {
    // tight taskbar: rings only (a moon during a prayer banner); the label below still says it all
    left.append((flash && flash.prayer) || moon ? icon('i-moon', 'icon is-prayer') : pillRing(vm));
  } else if (flash) {
    const text = flash.prayer && vm.mode !== 'top' ? flash.short : flash.text; // the capsule is capped at 172 px
    left.append(icon(flash.icon, flash.prayer ? 'icon is-prayer' : 'icon is-alert'), el('span', 'ear__flash' + (flash.prayer ? ' is-prayer' : ''), text));
  } else if (!hasNumbers(vm)) {
    const msg = vm.pill === 'no_claude' ? 'Install Claude Code to see your limits'
      : vm.pill === 'logged_out' ? 'Log in to Claude Code' : vm.refreshing ? 'Updating…' : 'Claude usage';
    left.append(el('span', 'ear__muted', msg));
  } else {
    const s = vm.session;
    left.append(pillRing(vm), el('span', 'ear__pct ' + LEVEL[s.level], s.value_text));
    if (vm.pill === 'full' && vm.pill_text) right.append(el('span', 'ear__count is-full', vm.pill_text));
    else if (vm.pill === 'stale') right.append(el('span', 'ear__muted', vm.updated_text.replace(/^As of /, 'as of ')));
    else right.append(el('span', 'ear__count', s.countdown_text));
    if (vm.attention > 0) {
      const b = el('span', 'ear__bell');
      b.append(icon('i-bell'), el('span', null, String(vm.attention)),
        el('span', 'sr-only', vm.attention === 1 ? ' session needs a look' : ' sessions need a look'));
      right.append(b);
    }
    if (moon) right.append(icon('i-moon', 'icon ear__moon'));
  }
  pill.replaceChildren(left, right);
  pill.classList.toggle('is-quiet', !flash && (!hasNumbers(vm) || vm.pill === 'stale'));
  // Screen readers get the whole closed state in one label.
  pill.setAttribute('aria-label', (flash && flash.text) || (hasNumbers(vm)
    ? `Claude usage: session ${vm.session.value_text}, ${vm.session.span_text || vm.session.countdown_text}; week ${vm.week.value_text}${working(vm) ? '; a session is working' : ''}${moon ? '; a prayer alert is waiting' : ''}`
    : left.textContent));
}

// ---------- the pod: the session that is working or needs you (spec 5.4) ----------

function renderPod(vm) {
  const p = vm.pod;
  // No pod in the taskbar: it would grow over app icons (the capsule's bell and glow say it).
  pod.hidden = !p || flash != null || (vm.mode || 'top') !== 'top';
  notch.classList.toggle('is-needs', !!p && p.status === 'needs_you');
  if (!p) return;
  const needs = p.status === 'needs_you';
  pod.replaceChildren(
    needs ? icon('i-bell', 'icon is-alert') : el('span', 'pod__spin'),
    el('span', 'pod__project', p.project || 'Claude'),
    el('span', 'pod__state ' + (needs ? 'is-alert' : 'is-working'), needs ? 'needs you' : 'working'),
    el('span', 'pod__age', ageText(p.since_ms)),
    ...(p.more > 0 ? [el('span', 'pod__more', `+${p.more}`)] : []),
  );
}

// ---------- open panel ----------

function renderDial(vm) {
  const dial = $('#dial');
  const show = hasNumbers(vm);
  $('#dial-wrap').hidden = !show;
  if (!show) return;
  dial.replaceChildren(ring(vm, 136, true));
  $('#dial-pct').textContent = vm.session.value_text;
  $('#dial-pct').className = 'dial__pct ' + LEVEL[vm.session.level];
  const c = vm.session.countdown_text;
  $('#dial-sub').textContent = vm.pill === 'full' ? (vm.pill_text || '') : c === 'ready' ? 'ready' : c === 'now' ? 'resets now' : `resets in ${c}`;
  $('#dial-wrap').setAttribute('aria-label',
    `Session ${vm.session.value_text} used, ${vm.session.reset_text}. Week ${vm.week.value_text} used, ${vm.week.reset_text}.`);
}

function renderLine(key, lv) {
  const row = $(`[data-line="${key}"]`);
  row.querySelector('.line__pct').textContent = lv.value_text;
  row.querySelector('.line__pct').className = 'line__pct ' + LEVEL[lv.level];
  row.querySelector('.line__when').textContent = lv.span_text || lv.reset_text;
}

function renderSessions(vm) {
  const rows = vm.sessions || [];
  const off = tasksOn === false;
  $('#sessions').hidden = rows.length === 0 && !off;
  $('#sessions-off').hidden = !off || rows.length > 0;
  $('#session-list').replaceChildren(...rows.map((r) => {
    const li = el('li', 'session');
    const label = { needs_you: 'needs you', working: 'working', finished: 'finished' }[r.status] || r.status;
    li.append(
      el('span', 'session__project', r.project || 'Claude'),
      el('span', 'session__status is-' + r.status.replace('_', '-'), label),
      el('span', 'session__age', ageText(r.since_ms)),
    );
    return li;
  }));
}

function renderPrayer(vm) {
  const p = vm.prayer;
  $('#prayer').hidden = !p;
  if (!p) return;
  $('#prayer-title').textContent = `Prayer · ${p.place}`;
  $('#prayer-off').hidden = !p.unavailable;
  $('#prayer-body').hidden = p.unavailable;
  if (p.unavailable) return;
  $('#prayer-name').textContent = p.next_name;
  $('#prayer-time').textContent = p.next_time;
  $('#prayer-in').textContent = p.next_in;
  $('#prayer-bar').style.width = p.progress_pct + '%';
  $('#prayer-times').replaceChildren(...p.times.map(([name, time, state]) => {
    const li = el('li', 'is-' + state, name);
    li.append(' ', el('b', null, time)); // the space keeps "Subuh 05:48" two words for screen readers
    if (state === 'next') li.setAttribute('aria-current', 'time');
    return li;
  }));
}

function render(vm) {
  lastVm = vm;
  notch.dataset.mode = vm.mode || 'top';
  renderPill(vm);
  renderPod(vm);
  renderDial(vm);
  const show = hasNumbers(vm);
  $('#lines').hidden = !show;
  if (show) {
    renderLine('session', vm.session);
    renderLine('week', vm.week);
  }
  $('#runs').textContent = vm.session.runs_out_text || '';
  $('#runs').hidden = !show || !vm.session.runs_out_text;
  $('#notice').hidden = vm.pill !== 'no_claude';
  renderPrayer(vm);
  renderSessions(vm);
  $('#updated').textContent = vm.refreshing ? 'Updating…' : vm.pill === 'no_claude' ? '' : vm.updated_text;
  const refresh = $('#refresh');
  refresh.setAttribute('aria-label', vm.refreshing ? 'Refreshing' : 'Refresh now');
  if (vm.refreshing) refresh.setAttribute('aria-busy', 'true');
  else refresh.removeAttribute('aria-busy');
  fitPanel();
  reportHits();
}

// Only the usage-updated event may speak; get_view and save_settings also carry `announce`.
function announce(text) {
  const live = $('#announce');
  live.textContent = '';
  requestAnimationFrame(() => { live.textContent = text; });
}

// ---------- open / close: one black shape that grows (spec 5.1, 5.2) ----------

let isOpen = false;
let pinned = false;
let closeTimer;
let hoverTimer;
let settleTimer;
let morphing = false;
let ageTimer;

// The panel grows from the pill's size to its content size; both are CSS variables so the
// spring in notch.css animates width and height together.
// The height the usage view needs. Not scrollHeight: that never drops below the current height.
function contentH() {
  const cs = getComputedStyle($('#panel-inner'));
  return usageView.offsetHeight + parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
}

function fitPanel() {
  notch.style.setProperty('--pill-w', pill.offsetWidth + 'px');
  // Never taller than the window: only the session list scrolls; the rest stays in place.
  const room = innerHeight - ((notch.dataset.mode || 'top') === 'top' ? 4 : 12);
  const scroller = $('#session-list');
  scroller.style.maxHeight = '';
  const over = contentH() - room;
  if (over > 0) scroller.style.maxHeight = Math.max(scroller.scrollHeight - over, 24) + 'px';
  notch.style.setProperty('--panel-h', Math.min(contentH(), room) + 'px');
}

// The window is a fixed size; only the visible shape takes the mouse (spec 4, approach A).
// While the shape is open or morphing, its full open box counts, so the spring never runs
// past the area that takes clicks.
let sentHits = '';
function reportHits() {
  const box = (r) => ({ x: Math.floor(r.left), y: Math.floor(r.top), w: Math.ceil(r.width) + 1, h: Math.ceil(r.height) + 1 });
  const rects = [box(pill.getBoundingClientRect())];
  if (isOpen || morphing) {
    const r = panel.getBoundingClientRect();
    const w = parseFloat(getComputedStyle(notch).getPropertyValue('--open-w')) || panel.offsetWidth;
    const h = contentH();
    const align = notch.dataset.align || 'centre';
    if ((notch.dataset.mode || 'top') !== 'top') {
      // taskbar: the panel is anchored bottom-right and grows up
      rects.push(box({ left: r.right - w, top: r.bottom - Math.max(h, r.height), width: w, height: Math.max(h, r.height) }));
    } else {
      const left = align === 'left' ? r.left : align === 'right' ? r.right - w : r.left + r.width / 2 - w / 2;
      rects.push(box({ left, top: 0, width: w, height: Math.max(h, r.height) }));
    }
  }
  if (!pod.hidden && !isOpen) rects.push(box(pod.getBoundingClientRect()));
  const key = JSON.stringify(rects);
  if (key === sentHits) return;
  sentHits = key;
  window.__hits = rects; // for live checks
  call('set_hit_rects', { rects });
}

function setOpen(open) {
  clearTimeout(closeTimer);
  clearTimeout(hoverTimer);
  if (open) {
    stopSound(); // opening the notch is "seen": the sound stops and the moon goes
    if (moon) { moon = false; if (lastVm) renderPill(lastVm); }
  }
  if (open === isOpen) return;
  isOpen = open;
  pill.setAttribute('aria-expanded', String(open));
  clearInterval(ageTimer);
  clearTimeout(settleTimer);
  morphing = true;
  fitPanel();
  notch.classList.toggle('is-open', open);
  reportHits();
  settleTimer = setTimeout(() => {
    morphing = false;
    reportHits();
  }, reduced() ? 150 : MORPH + 100);
  if (open) {
    ageTimer = setInterval(() => { if (lastVm) { renderSessions(lastVm); renderPod(lastVm); } }, 30000);
    if (tasksOn === null) {
      invoke('get_task_alerts').then((t) => { tasksOn = !!t.on; if (lastVm) render(lastVm); }).catch(() => {});
    }
  } else {
    call('sessions_seen'); // closing after a look clears finished rows (spec 5.2)
  }
}

/** Hover asks to open; it only happens if the pointer rests (spec 5.2: hover intent). */
function hoverIn() {
  clearTimeout(closeTimer);
  if (isOpen) return;
  clearTimeout(hoverTimer);
  hoverTimer = setTimeout(() => setOpen(true), HOVER_INTENT);
}

function hoverOut() {
  clearTimeout(hoverTimer);
  if (!pinned && isOpen) { clearTimeout(closeTimer); closeTimer = setTimeout(() => setOpen(false), CLOSE_DELAY); }
}

// Content changes (sessions, prayer, update link) change the panel height.
const fitObserver = new ResizeObserver(() => { fitPanel(); reportHits(); });
[$('#panel-inner'), usageView].forEach((e) => fitObserver.observe(e));
new ResizeObserver(() => { fitPanel(); reportHits(); }).observe(pill);
new ResizeObserver(reportHits).observe(pod);
window.addEventListener('resize', reportHits);

pill.addEventListener('click', () => { pinned = !pinned; setOpen(pinned); });
// Hover opens the panel over the pill, so a click inside it is the "opened by click" pin.
panel.addEventListener('pointerdown', () => { pinned = true; });
notch.addEventListener('mouseenter', hoverIn);
notch.addEventListener('mouseleave', hoverOut);
pod.addEventListener('click', () => { pinned = true; setOpen(true); });
// Clicking another window unpins, but not while a select's own popup takes the focus.
window.addEventListener('blur', () => {
  if (pinned) { pinned = false; setOpen(false); }
});
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  pinned = false;
  setOpen(false);
  pill.focus();
  call('focus_previous');
});

$('#open-settings').addEventListener('click', () => call('open_settings'));
$('#refresh').addEventListener('click', () => call('refresh_now'));
$('#install').addEventListener('click', () => call('open_install_page'));

// ---------- update ----------

// The settings window has its own check; this is the link at the top of the panel.
const updateBtn = $('#update-btn');
let updateTo = null;
let installing = false;
const setUpdate = (text, busy) => { updateBtn.textContent = text; updateBtn.disabled = busy; };
async function checkUpdate() {
  if (installing) return;
  updateTo = await invoke('check_update').catch(() => null); // the timed check stays quiet
  if (updateTo) setUpdate(`Update to ${updateTo}`, false);
  $('#update').hidden = !updateTo;
  fitPanel();
}
updateBtn.addEventListener('click', async () => {
  installing = true;
  setUpdate(`Downloading ${updateTo}…`, true);
  try {
    await invoke('install_update'); // the installer then closes the notch and starts the new one
    setUpdate(`Installing ${updateTo}…`, true);
  } catch (e) {
    console.error('install_update', e);
    installing = false;
    setUpdate('Update failed. Try again', false);
  }
});
checkUpdate();
setInterval(checkUpdate, 6 * 60 * 60 * 1000);

// ---------- settings: the window has them; the notch only follows the position ----------

async function followSettings() {
  const s = await call('get_settings');
  if (!s) return; // not loaded yet, or the call failed; keep what is shown
  soundSettings = s;
  tasksOn = null; // the window may have switched Task alerts; read it again when the panel opens
  notch.dataset.align = s.position; // the window sits at that edge; the shape follows
  reportHits();
}

// ---------- sound: bells and the azan, one at a time; the prayer sound wins ----------

let soundSettings = null; // the settings the sounds follow (bell, needs_sound, prayer_sound)
let playing = null; // { audio, url, kind } from the moment an alert claims the speaker
let seenAttention = null; // sessions needing you at the last view; a rise plays the needs-you sound

/** What an alert plays: 'azan', 'needs-file', 'bell' or null. `timePlaying`: a prayer-time sound is on. */
function pickSound(kind, s, timePlaying) {
  if (!s) return null;
  if (kind === 'time') return s.prayer_sound === 'off' ? null : s.prayer_sound === 'file' ? 'azan' : 'bell';
  if (timePlaying) return null; // a bell never cuts into the prayer sound
  if (kind === 'remind') return s.prayer_sound === 'off' ? null : 'bell';
  return s.needs_sound === 'off' ? null : s.needs_sound === 'file' ? 'needs-file' : 'bell';
}

function stopSound() {
  if (!playing) return;
  if (playing.audio) playing.audio.pause();
  if (playing.url) URL.revokeObjectURL(playing.url); // an own file can be 10 MB; free it
  playing = null;
}

async function playAlert(kind) {
  const pick = pickSound(kind, soundSettings, !!playing && playing.kind === 'time');
  if (!pick) return;
  stopSound();
  // Claim the speaker before reading the file, so an alert or an open during the read sees it.
  const me = { audio: null, url: null, kind };
  playing = me;
  const bell = `sounds/${soundSettings.bell}.ogg`;
  let src = bell;
  if (pick !== 'bell') {
    const bytes = await invoke('read_sound', { kind: pick === 'azan' ? 'azan' : 'needs' }).catch(() => null);
    if (playing !== me) return; // stopped or replaced while the file was read
    if (bytes) src = me.url = URL.createObjectURL(new Blob([bytes]));
  }
  const start = (url) => {
    me.audio = new Audio(url);
    me.audio.addEventListener('ended', () => { if (playing === me) stopSound(); });
    return me.audio.play();
  };
  start(src).catch((e) => {
    console.error('sound', e);
    if (playing !== me || src === bell) return;
    URL.revokeObjectURL(me.url); // an unplayable own file falls back to the bell, once
    me.url = null;
    start(bell).catch(() => {});
  });
}

/** Sounds for a view: a new prayer banner, or more sessions needing you than before. */
function soundFor(vm) {
  if (vm.banner) playAlert(vm.banner.kind);
  const a = vm.attention || 0;
  if (seenAttention !== null && a > seenAttention && !vm.banner) playAlert('needs');
  seenAttention = a;
}

// ---------- start ----------

/** A prayer banner or a session notice to show in the closed notch for a while. */
function takeFlash(vm) {
  const b = vm.banner;
  const next = b ? { text: b.text, short: b.short, icon: 'i-moon', prayer: true, ms: b.seconds * 1000 }
    : vm.flash ? { text: vm.flash, icon: 'i-bell', ms: FLASH_MS } : null;
  if (!next) return;
  flash = next;
  if (next.prayer) moon = false; // the new banner shows in full first
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => {
    if (next.prayer && !isOpen) moon = true; // like the bell: it stays until the notch is opened
    flash = null;
    if (lastVm) render(lastVm);
  }, next.ms);
}

(async () => {
  // The shape is measured for hit-testing, so the font must be loaded first.
  await Promise.all([500, 600, 700].map((w) => document.fonts.load(`${w} 14px "Manrope"`))).catch(() => {});
  // Listen before the first fetch, so a refresh that finishes meanwhile is neither missed nor its announcement lost.
  await listen('usage-updated', (e) => {
    takeFlash(e.payload); // the same text arrives in `announce`, so it is spoken once
    soundFor(e.payload);
    render(e.payload);
    if (e.payload.announce) announce(e.payload.announce);
  });
  // Rust tells us when the pointer leaves the shape (input is then ignored, so no mouseleave).
  // It also says when the pointer arrives: a cursor that stops right after input is switched on
  // never sends mouseenter.
  await listen('pointer', (e) => { if (e.payload) hoverIn(); else hoverOut(); });
  // Any save (settings window or tray) may move the notch.
  await listen('settings-changed', followSettings);
  await followSettings(); // first, so a banner that fired before we listened has its sound settings
  const vm = await call('get_view');
  if (vm) {
    takeFlash(vm); // a prayer banner that fired before we listened
    soundFor(vm);
    render(vm);
  }
})();
