// A game's journal page: what you gave it and what you thought of it. Pushed over the Library's Journal tab (A on a
// row) or Home (☰ Options → Journal): app.push('journal', {id}); B goes back. Removed games open here too.
//
// Layout (stage px): the game's background art under left and bottom scrims; the hub's header (tile 56–127 × 52–123,
// "Journal" x 176 cap 77); its logo or name at x 172 down to y 340, then the tagline (or when it was removed); four
// editable rows from y 452 (900 wide): Your rating, Status, How far you got, Your note; Forget this game for a removed
// one. On the right a card (x 1288–1748) with the square art and the facts the app recorded: time played, sessions,
// first and last played, added, removed.
// Buttons: Up / Down move between rows; Left / Right change the rating (0–5 stars), the status, or the progress in
// 5 % steps (LT / RT: 1 %); A on the rating or status steps it, on the note opens the keyboard, on Forget asks once
// more. Each change is saved 300 ms after the last one, and at once when the page closes.

import * as keyboard from '../keyboard/keyboard.js';
import { STATUSES, statusOf, EMPTY, NOTE_MAX, hours, date, ago, stars, tint, squareArt } from './shared.js';

const FRAME = 1000 / 60;
const ROWS_Y = 452, ROW_H = 88, PITCH = 100, NOTE_H = 136, ROW_W = 900, X0 = 172;
const SAVE_MS = 300, FORGET_MS = 4000;
const IN = { duration: 12 * FRAME, easing: 'cubic-bezier(0, 0, .2, 1)' };
const HINTS = {
  rating: 'Left and right set the stars',
  status: 'Left and right change the status',
  progress: 'Left and right move 5 %  ·  LT and RT move 1 %',
  note: 'A writes a note',
  forget: 'Your time and notes for this game go too',
};

let app, host, root;
const $ = {};
let game = null, journal = { ...EMPTY };
let rows = [], at = 0;
let saveTimer = 0, forgetArmed = 0, kb = null, onTop = false;

export default {
  mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="jpbg"><img alt=""><div class="scrim"></div></div>
      <div class="jpage">
        <div class="jphead"><div class="icon"></div><div class="t">Journal</div></div>
        <div class="jpname"><img class="logo" alt=""><div class="text"></div></div>
        <div class="jpline"></div>
        <div class="jprows"></div>
        <div class="jpring"></div>
        <div class="jphint"></div>
        <div class="jpcard"><div class="art"></div><div class="facts"></div></div>
      </div>`;
    for (const k of ['jpbg', 'jpage', 'jphead', 'jpname', 'jpline', 'jprows', 'jpring', 'jphint', 'jpcard']) $[k] = el.querySelector(`.${k}`);
    $.bgimg = $.jpbg.querySelector('img');
    $.icon = $.jphead.querySelector('.icon');
    $.logo = $.jpname.querySelector('.logo');
    $.nameText = $.jpname.querySelector('.text');
    $.art = $.jpcard.querySelector('.art');
    $.facts = $.jpcard.querySelector('.facts');
    // read-only state for tests (tools/app/ui-probe.mjs eval steps)
    window.__journalPage = { state: () => ({ id: game?.id, row: rows[at]?.id, rows: rows.map(r => r.id), journal: { ...journal }, keyboard: !!kb?.isOpen, forgetArmed: !!forgetArmed }) };
  },

  async enter(params = {}) {
    onTop = true;
    if (params.back) return;
    const g = await find(params.id);
    if (!onTop) return;
    if (!g) { console.warn('journal: no game', params.id); app.pop(); return; }
    show(g);
    root.animate([{ opacity: 0 }, { opacity: 1 }], IN);
    $.jpage.animate([{ transform: 'translateY(24px)' }, { transform: 'none' }], IN);
  },

  leave() {
    onTop = false;
    kb?.close();
    kb = null;
    flush();
    disarm();
  },

  action(name, e = {}) {
    if (kb?.isOpen) return kb.action(name, e);
    if (!game) return false;
    if (name === 'b') { flush(); app.pop(); return 'back'; }
    if (name === 'point' || name === 'click') return pointer(name, e);
    if (name === 'up' || name === 'down') return focusRow(at + (name === 'up' ? -1 : 1));
    if (name === 'wheel') return e.dy ? focusRow(at + (e.dy > 0 ? 1 : -1)) || 'none' : 'none';
    const row = rows[at];
    if (name === 'left' || name === 'right') return row.step?.(name === 'left' ? -1 : 1, e) ?? false;
    if (e.held) return false;
    if (name === 'lt' || name === 'rt') return row.fine?.(name === 'lt' ? -1 : 1) ?? false;
    if (name === 'a') return row.press?.() ?? false;
    return false;
  },
};

async function find(id) {
  const inJournal = (await host.library.journal().catch(() => [])) ?? [];
  return inJournal.find(g => g.id === id) ?? ((await host.library.list().catch(() => [])) ?? []).find(g => g.id === id) ?? null;
}

// ---------- the page ----------
function show(g) {
  game = g;
  journal = { ...EMPTY, ...(g.journal ?? {}) };
  forgetArmed = 0;
  root.classList.toggle('removed', !!g.removedAt);
  // background: the game's art, else a tint
  const bg = g.art?.background;
  $.bgimg.classList.remove('in');
  $.jpbg.style.background = bg ? '#000' : tint(g.id);
  if (bg) {
    $.bgimg.src = host.fileUrl(bg);
    $.bgimg.decode().then(() => $.bgimg.classList.add('in'), () => {});
  } else $.bgimg.removeAttribute('src');
  // header icon and the square art in the card
  const sq = squareArt(g);
  for (const box of [$.icon, $.art]) {
    box.replaceChildren();
    box.style.background = tint(g.id);
    if (!sq) continue;
    const img = new Image();
    img.alt = '';
    img.src = host.fileUrl(sq);
    if (!g.art?.tile) img.classList.add('portrait');
    img.onerror = () => img.remove();
    box.append(img);
  }
  // logo, else the name
  const logo = g.art?.logo;
  $.jpname.classList.toggle('haslogo', !!logo);
  $.nameText.textContent = g.name;
  if (logo) { $.logo.src = host.fileUrl(logo); $.logo.onerror = () => $.jpname.classList.remove('haslogo'); }
  else $.logo.removeAttribute('src');
  $.jpline.textContent = g.restoredAt
    ? `From your GitHub backup. Put the game in the games folder and its time comes with it.`
    : g.removedAt
    ? `Removed from this PC ${date(g.removedAt)}. Its time and your notes stay here.`
    : g.tagline ?? '';
  paintFacts();
  buildRows();
  focusRow(0, true);
}

function paintFacts() {
  const g = game;
  const facts = [
    ['Time played', g.playtimeSec ? hours(g.playtimeSec) : 'None yet'],
    ['Sessions', g.sessions ? String(g.sessions) : '—'],
    ['First played', g.firstPlayed ? date(g.firstPlayed) : '—'],
    ['Last played', g.lastPlayed ? ago(g.lastPlayed) : '—'],
    ['Added', g.addedAt ? date(g.addedAt) : '—'],
    g.restoredAt ? ['Restored', date(g.restoredAt)] : g.removedAt ? ['Removed', date(g.removedAt)] : ['On this PC', 'Installed'],
  ];
  $.facts.innerHTML = facts.map(() => '<div class="f"><div class="l"></div><div class="v"></div></div>').join('');
  [...$.facts.children].forEach((el, i) => {
    el.querySelector('.l').textContent = facts[i][0];
    el.querySelector('.v').textContent = facts[i][1];
  });
}

// ---------- the rows ----------
function buildRows() {
  rows = [
    {
      id: 'rating', label: 'Your rating',
      draw: el => { el.innerHTML = `<div class="jpstars">${stars(journal.rating)}</div><span class="v">${journal.rating ? `${journal.rating} / 5` : 'Not rated'}</span>`; },
      step: d => set({ rating: clamp((journal.rating ?? 0) + d, 0, 5) || null }),
      press: () => set({ rating: (journal.rating ?? 0) >= 5 ? null : (journal.rating ?? 0) + 1 }) || true,
    },
    {
      id: 'status', label: 'Status',
      draw: el => {
        const s = statusOf(journal.status);
        el.innerHTML = `<span class="chev l">‹</span><b style="background:${s.color}"></b><span class="v${journal.status ? '' : ' unset'}">${s.label}</span><span class="chev r">›</span>`;
      },
      step: (d, e) => (e?.held ? false : setStatus(d)),
      press: () => setStatus(1, true),
    },
    {
      id: 'progress', label: 'How far you got',
      draw: el => {
        const p = journal.progress;
        el.innerHTML = `<div class="track${p === 100 ? ' full' : ''}"><div class="fill" style="width:${p ?? 0}%"></div></div><span class="v${p == null ? ' unset' : ''}">${p == null ? 'Not set' : `${p} %`}</span>`;
      },
      // the first step from "not set" lands on 0 % or 5 %
      step: d => set({ progress: journal.progress == null ? (d > 0 ? 5 : 0) : clamp(Math.round((journal.progress + d * 5) / 5) * 5, 0, 100) }),
      fine: d => set({ progress: clamp((journal.progress ?? 0) + d, 0, 100) }),
    },
    {
      id: 'note', label: 'Your note', tall: true,
      draw: el => {
        el.innerHTML = '<div class="note"></div>';
        const n = el.querySelector('.note');
        n.textContent = journal.note ?? 'Add a note: what you thought, where you left off, what to try next time.';
        n.classList.toggle('unset', !journal.note);
      },
      press: () => openNote(),
    },
  ];
  if (game.removedAt) rows.push({ id: 'forget', label: '', button: true, draw: drawForget, press: () => forget() });
  let y = ROWS_Y;
  $.jprows.replaceChildren(...rows.map((r, i) => {
    const el = document.createElement('div');
    el.className = `jprow ${r.id}${r.tall ? ' tall' : ''}${r.button ? ' button' : ''}`;
    el.dataset.nav = `jprow:${i}`;
    el.style.top = `${y}px`;
    r.y = y;
    r.h = r.button ? 70 : r.tall ? NOTE_H : ROW_H;
    y += r.button ? r.h + 30 : (r.tall ? NOTE_H : ROW_H) + (PITCH - ROW_H);
    el.innerHTML = `${r.button ? '' : '<span class="label"></span>'}<div class="ctl"></div>`;
    if (!r.button) el.querySelector('.label').textContent = r.label;
    r.el = el;
    r.draw(el.querySelector('.ctl'));
    return el;
  }));
}

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const redraw = id => { const r = rows.find(x => x.id === id); r?.draw(r.el.querySelector('.ctl')); };

function setStatus(d, wrap = false) {
  const i = STATUSES.findIndex(s => s.id === (journal.status ?? null));
  let n = i + d;
  if (wrap) n = (n + STATUSES.length) % STATUSES.length;
  if (n < 0 || n >= STATUSES.length) return false;
  const status = STATUSES[n].id;
  // 100 % complete means all of it
  const patch = status === 'completed' ? { status, progress: 100 } : { status };
  return set(patch);
}

// a change: redrawn now, saved shortly; false (no sound) when nothing changed
function set(patch) {
  const next = { ...journal, ...patch };
  if (Object.keys(patch).every(k => next[k] === journal[k])) return false;
  journal = next;
  for (const k of Object.keys(patch)) redraw(k);
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, SAVE_MS);
  return true;
}

function flush() {
  if (!saveTimer) return;
  clearTimeout(saveTimer);
  save();
}

function save() {
  saveTimer = 0;
  if (!game) return;
  const id = game.id;
  host.library.review(id, { ...journal }).then(g => { if (game?.id === id && g) Object.assign(game, g); }).catch(e => console.warn('journal save', e));
}

// ---------- focus ----------
function focusRow(i, instant = false) {
  if (i < 0 || i >= rows.length) return false;
  const dir = i > at ? 'dn' : 'up';
  at = i;
  if (rows[at].id !== 'forget') disarm();
  rows.forEach((r, k) => r.el.classList.toggle('focus', k === at));
  const r = rows[at];
  $.jpring.className = `jpring ${instant ? '' : dir}${r.button ? ' pill' : ''}`;
  $.jpring.style.transform = r.button ? `translate(${X0 - 7}px, ${r.y - 7}px)` : `translate(${X0 - 20}px, ${r.y}px)`;
  $.jpring.style.height = `${r.button ? r.h + 14 : r.h}px`;
  $.jpring.style.width = `${r.button ? r.el.offsetWidth + 14 : ROW_W + 40}px`;
  $.jpring.getAnimations().forEach(a => a.cancel());
  if (!instant) $.jpring.animate([{ opacity: 0.35 }, { opacity: 1 }], { duration: 8 * FRAME, easing: 'linear' });
  $.jphint.textContent = HINTS[r.id] ?? '';
  return true;
}

function pointer(name, e) {
  const [kind, n] = (e.target ?? '').split(':');
  if (kind !== 'jprow') return 'none';
  const i = Number(n);
  if (i !== at) focusRow(i);
  if (name === 'click') return rows[at].press?.() ?? rows[at].step?.(1) ?? 'none';
  return 'none';
}

// ---------- the note ----------
function openNote() {
  kb = keyboard.open({
    title: `Your note on ${game.name}`,
    value: journal.note ?? '',
    placeholder: 'What you thought, where you left off',
    maxLength: NOTE_MAX,
    parent: root,
    onDone(v) { kb = null; set({ note: v.trim() || null }); flush(); },
    onCancel() { kb = null; },
  });
  return true;
}

// ---------- forgetting a removed game: A, then A again within 4 s ----------
function drawForget(el) {
  el.innerHTML = `<span>${forgetArmed ? 'Press A again to forget it' : 'Forget this game'}</span>`;
}

function disarm() {
  if (!forgetArmed) return;
  clearTimeout(forgetArmed);
  forgetArmed = 0;
  redraw('forget');
  if (rows[at]?.id === 'forget') focusRow(at, true);
}

function forget() {
  if (!forgetArmed) {
    forgetArmed = setTimeout(disarm, FORGET_MS);
    redraw('forget');
    focusRow(at, true);
    return true;
  }
  clearTimeout(forgetArmed);
  forgetArmed = 0;
  clearTimeout(saveTimer);
  saveTimer = 0;
  const id = game.id;
  game = null;
  host.library.forget(id).catch(e => console.warn('forget', e)).finally(() => app.pop());
  return 'back';
}
