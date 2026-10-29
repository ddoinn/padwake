// Search (row A8), in the PS5 Search's look: the spotlight background, category tabs at the top-left (All, Games,
// Media; no Players, as there are no users), a full-width field, and the pad keyboard (ui/keyboard) under the field's
// left end. The contract is at the top of app.js.
//
// The field is typed with the pad keyboard or a physical one, and results update as you type: entries whose words
// start with what you typed (any order, any case), best match first. Games are the library; Media are the host's
// media apps (host.media, as Home's Media tab). With nothing typed it shows the recently played (Media: every app).
// While typing, results fill the free area right of the keyboard (3 × 3). Done (RT, Menu, Enter, the Done key, or Up
// from the top row of keys) closes the keyboard and moves to the results, now a 7-wide grid; LB / RB switch tabs
// there (on the keyboard they move the caret, as on the PS5). A on a game shows it on Home; A on an app opens it.
// B goes back.

import * as keyboard from '../keyboard/keyboard.js';
import { sorted } from '../library/library.js'; // "Recently played" in the same order as Home and the Library
import { mount as spotlight } from '../background/spotlight.js'; // the moving spotlight (3-settings-a.md §1.1)

const COLS = 7, PITCH = 224, X0 = 172, Y0 = 300, ROWS = 3;  // results are 200 px (search.css)
const TYPING = { cols: 3, x: 1100, y: 272, max: 9 };       // while typing: right of the keyboard panel (x 172–1042)
const TABS = [['all', 'All'], ['games', 'Games'], ['media', 'Media']];
const PLACEHOLDER = 'Search for games and apps';
const RING_IN = { delay: 140, ms: 140 };          // as the Library's ring (2-library.md §2)
const IN_MS = 150;
const ICON = '<svg viewBox="0 0 30 30"><circle cx="12.5" cy="12.5" r="9.5" fill="none" stroke="currentColor" stroke-width="3.2"/><path d="M19.6 19.6 28 28" stroke="currentColor" stroke-width="3.6" stroke-linecap="round"/></svg>';

let app, host, root;
const $ = {};
let games = [];
let media = [];
let mediaSet = new Set();
let tab = 0;
let query = '';
let results = [];
let zone = 'keys';       // keys (the keyboard is open) | results | field
let index = 0;
let kb = null;
let ringAnim = null;

// ---------- matching ----------
const words = s => s.normalize('NFKD').replace(/\p{M}/gu, '').replace(/['’]/g, '').toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);

// rank 0: the name starts with the query; 1: every query word starts a word and the first starts the name; 2: the
// query run together starts at a word ("spiderman", "godofwar"); 3: every query word starts some word. Then
// recently played first, then A–Z. No match: null.
export function rank(name, q) {
  const n = words(name), qs = words(q);
  if (!qs.length || !n.length) return null;
  const joined = qs.join('');
  const tail = i => n.slice(i).join('');
  if (tail(0).startsWith(joined) && n[0].startsWith(qs[0])) return 0;
  const hits = qs.map(w => n.findIndex(x => x.startsWith(w)));
  if (hits.every(h => h >= 0)) return hits[0] === 0 ? 1 : 3 + Math.min(...hits) / 100;
  const at = n.findIndex((_, i) => tail(i).startsWith(joined));
  return at >= 0 ? 2 + at / 100 : null;
}

export function search(list, q) {
  const recent = sorted(list, 'played');
  if (!words(q).length) return recent.slice(0, COLS);
  const order = new Map(recent.map((g, i) => [g, i]));
  return list.map(g => [g, rank(g.name, q)]).filter(([, r]) => r !== null)
    .sort(([a, ra], [b, rb]) => ra - rb || order.get(a) - order.get(b)).map(([g]) => g).slice(0, COLS * ROWS);
}

export default {
  async mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="tabs">${TABS.map(([id, label], i) => `<span class="tab" data-nav="tab:${i}">${label}</span>`).join('')}</div>
      <div class="field" data-nav="field"><div class="glyph">${ICON}</div><span class="q"></span><span class="ph">${PLACEHOLDER}</span></div>
      <div class="label"></div>
      <div class="results"></div>
      <div class="ring"></div>`;
    for (const k of ['field', 'label', 'results', 'ring']) $[k] = el.querySelector(`.${k}`);
    spotlight(el);
    $.q = el.querySelector('.field .q');
    $.tabs = [...el.querySelectorAll('.tab')];
    host.on('library', list => { games = list ?? []; paint(); });
    host.on('media', list => setMedia(list));
    window.__search = { state: () => ({ zone, tab: TABS[tab][0], query, index, results: results.map(g => g.id), keyboard: !!kb?.isOpen }) };
    host.media.list().then(setMedia).catch(() => {});
    games = (await host.library.list()) ?? [];
  },

  enter(params = {}) {
    if (params.back) {
      if (zone === 'keys' && !kb?.isOpen) openKeyboard();
      return;
    }
    query = '';
    index = 0;
    tab = 0;
    root.animate([{ opacity: 0 }, { opacity: 1 }], { duration: IN_MS, easing: 'linear' });
    openKeyboard();
  },

  leave() {
    kb?.close(); // silently: a Control Center over Search comes back to the same zone (enter with back)
    kb = null;
  },

  action(name, e = {}) {
    // a click on a result or a tab works while typing too
    if (name === 'click' && /^res:/.test(e.target ?? '')) return open(Number(e.target.slice(4)));
    if (name === 'click' && /^tab:/.test(e.target ?? '')) return switchTab(Number(e.target.slice(4))) || 'none';
    if (kb?.isOpen) {
      if (name === 'up' && !e.held && kb.row === 0) { kb.finish(); return true; } // above the keys are the results
      return kb.action(name, e);
    }
    if (name === 'release') return false;
    if (name === 'b') { back(); return 'back'; }
    if (name === 'lb' || name === 'rb') return switchTab(tab + (name === 'lb' ? -1 : 1));
    if (name === 'point' || name === 'click') return pointer(name, e);
    if (zone === 'field') return fieldAction(name, e);
    return resultsAction(name, e);
  },
};

function setMedia(list) {
  media = list ?? [];
  mediaSet = new Set(media);
  paint();
}

function back() {
  kb?.close();
  kb = null;
  if (app.stack.length > 1) app.pop();
  else app.go('home');
}

function switchTab(i) {
  if (i < 0 || i >= TABS.length || i === tab) return false;
  tab = i;
  index = 0;
  if (zone === 'results' && !find().length) zone = 'field';
  paint(zone === 'results');
  return true;
}

// ---------- the keyboard ----------
function openKeyboard() {
  zone = 'keys';
  kb = keyboard.open({
    title: '',
    value: query,
    placeholder: PLACEHOLDER,
    maxLength: 60,
    parent: root,
    onInput: q => { query = q; index = 0; paint(); },
    onDone: q => {
      kb = null;
      query = q;
      zone = results.length ? 'results' : 'field';
      index = 0;
      paint(true);
    },
    onCancel: () => { kb = null; back(); },
  });
  paint();
}

// ---------- drawing ----------
function find() {
  const id = TABS[tab][0];
  if (id === 'media' && !words(query).length) return media.slice(0, COLS * ROWS); // the host's order, as Home's tab
  return search(id === 'games' ? games : id === 'media' ? media : [...games, ...media], query);
}

function paint(ring = false) {
  results = find();
  const typing = zone === 'keys';
  root.classList.toggle('typing', typing);
  $.tabs.forEach((el, i) => el.classList.toggle('sel', i === tab));
  $.q.textContent = query;
  root.classList.toggle('empty-q', !query);
  $.field.classList.toggle('focus', zone === 'field');
  const q = query.trim(), id = TABS[tab][0];
  $.label.textContent = !q
    ? (id === 'media' ? 'Media apps' : results.some(g => g.lastPlayed) ? 'Recently played' : 'Your games')
    : results.length ? `${results.length} result${results.length === 1 ? '' : 's'}`
    : id === 'games' ? `No games match "${q}"` : id === 'media' ? `No apps match "${q}"` : `No results for "${q}"`;
  index = Math.min(index, Math.max(0, results.length - 1));
  $.results.replaceChildren(...results.map(cover));
  placeRing(ring);
}

// where result i sits: 3 a row right of the keyboard while typing, else 7 a row under the field
function at(i) {
  const g = zone === 'keys' ? TYPING : { cols: COLS, x: X0, y: Y0 };
  return [g.x + (i % g.cols) * PITCH, g.y + Math.floor(i / g.cols) * PITCH];
}

function cover(g, i) {
  const el = document.createElement('div');
  el.className = `res${i >= TYPING.max ? ' more' : ''}`;
  el.dataset.nav = `res:${i}`;
  const [x, y] = at(i);
  el.style.transform = `translate(${x}px, ${y}px)`;
  const art = g.art?.tile ?? g.art?.cover;
  if (art) {
    const img = new Image();
    img.alt = '';
    img.decoding = 'async';
    img.src = host.fileUrl(art);
    if (!g.art?.tile) img.classList.add('portrait');
    img.decode().then(() => img.classList.add('in'), () => img.classList.add('in'));
    el.append(img);
  } else {
    el.style.background = tint(g.id);
    el.append(Object.assign(document.createElement('span'), { className: 'name', textContent: g.name }));
  }
  el.insertAdjacentHTML('beforeend', '<div class="shade"></div><div class="title"></div>');
  el.querySelector('.title').textContent = g.name;
  return el;
}

function tint(id) {
  let h = 0;
  for (const c of id) h = (h * 31 + c.codePointAt(0)) % 360;
  return `linear-gradient(145deg, hsl(${h} 45% 34%), hsl(${(h + 25) % 360} 50% 16%))`;
}

// the Library's focus: the ring 3 px outside the cover fades in after 140 ms; the title shows at once
function placeRing(fade) {
  ringAnim?.cancel();
  const on = zone === 'results' && results.length > 0;
  [...$.results.children].forEach((el, i) => el.classList.toggle('focus', on && i === index));
  $.ring.hidden = !on;
  if (!on) return;
  const [x, y] = at(index);
  $.ring.style.transform = `translate(${x - 6}px, ${y - 6}px)`;
  if (fade) ringAnim = $.ring.animate([{ opacity: 0 }, { opacity: 1 }], { duration: RING_IN.ms, delay: RING_IN.delay, easing: 'linear', fill: 'backwards' });
}

// ---------- moving ----------
function resultsAction(name, e) {
  const n = results.length, col = index % COLS;
  const to = i => { index = i; placeRing(true); return true; };
  if (name === 'left') return col > 0 && to(index - 1);
  if (name === 'right') return col < COLS - 1 && index + 1 < n && to(index + 1);
  if (name === 'up') return index >= COLS ? to(index - COLS) : (!e.held && focusField());
  if (name === 'down') return index + COLS < n ? to(index + COLS) : (Math.floor(index / COLS) < Math.floor((n - 1) / COLS) && to(n - 1));
  if (e.held) return false;
  if (name === 'a') return open(index);
  if (name === 'y') { openKeyboard(); return true; }
  if (name === 'wheel') return e.dy ? resultsAction(e.dy > 0 ? 'down' : 'up', {}) || 'none' : 'none';
  return false;
}

function focusField() {
  zone = 'field';
  paint();
  return true;
}

function fieldAction(name, e) {
  if (name === 'down') return results.length > 0 && (zone = 'results', index = 0, paint(true), true);
  if (e.held) return false;
  if (name === 'a' || name === 'y') { openKeyboard(); return true; }
  return false;
}

// a game shows on Home; an app opens (the host hides the window, and Home comes back when it closes)
function open(i) {
  const g = results[i];
  if (!g) return false;
  kb?.close();
  kb = null;
  if (mediaSet.has(g)) {
    zone = results.length ? 'results' : 'field';
    index = i;
    paint();
    host.media.open(g.id).catch(err => console.warn('media', err));
    return true;
  }
  app.go('home', { focus: g.id });
  return true;
}

function pointer(name, e) {
  const t = e.target ?? '';
  if (t === 'field') {
    if (name === 'click') { openKeyboard(); return true; }
    return zone === 'field' ? 'none' : (focusField(), 'none');
  }
  if (t.startsWith('res:')) {
    const i = Number(t.slice(4));
    if (zone === 'results' && i === index) return 'none';
    zone = 'results';
    index = i;
    paint(true);
    return 'none';
  }
  return 'none';
}
