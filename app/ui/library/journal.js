// Library → Journal (the second tab): every game played here or reviewed, and the ones whose folder is gone, with the
// time spent, status, stars and progress. library.js owns the header, tabs, sort button and menu, and routes input
// here while this tab is on.
//
// Layout (stage px): "Played: N" and "Sort by: …" in the count line's slot (cap 288); a band of totals at y 338; then
// 132 px rows at x 172, 1576 wide, from y 522 (pitch 156). The focused row pins its top at y 436, with the one before
// it above; the first stays where it is. Motion is the Library's: the same row scroll settle, the ring fading in after
// 140 ms. Rows near the view are drawn only, art fades in once decoded.
// Buttons: A opens the game's journal page (ui/journal), Up from the first row goes to the tabs, Left to the sort
// button, LT / RT jump 4 rows.

import { statusOf, done, hours, ago, date, stars, tint, squareArt, STAR } from '../journal/shared.js';

export const SORTS = [
  { id: 'played', label: 'Recently played' },
  { id: 'most', label: 'Most played' },
  { id: 'rating', label: 'Highest rated' },
  { id: 'progress', label: 'Most complete' },
  { id: 'az', label: 'Name (A - Z)' },
];
const X0 = 172, W = 1576, ROW_H = 132, PITCH = 156, LIST_Y = 522, PIN_Y = 436, CLIP_Y = 260, VIEW_H = 1080;
const PAGE = 4;
const RING_IN = { delay: 140, ms: 140 };
const TAB_FADE = { duration: 315, delay: 200, easing: 'cubic-bezier(.33, 0, .67, 1)' }; // §2 "Tab switch content"

let ctx, host, $ = {};
let entries = [];
let sortId = 'played';
let index = 0, focused = false, scrollY = 0, scrollFrom = 0, scrollAnim = null, ringAnim = null;
const rows = new Map(); // index → row element (near the view only)

try { sortId = localStorage.getItem('journal.sort') || sortId; } catch {}
if (!SORTS.some(s => s.id === sortId)) sortId = 'played';

// ctx: {app, host, root, scroll: {curve, ms, frame, ease(t)}, toTabs(), toButton()}
export function mount(c) {
  ctx = c;
  host = c.host;
  const el = document.createElement('div');
  el.className = 'journal';
  el.innerHTML = `
    <div class="jview"><div class="jslide"><div class="jcontent">
      <div class="countline"><span class="jcount"></span><span class="jsortlabel"></span></div>
      <div class="jstats"></div>
      <div class="jlist"></div>
      <div class="jring"></div>
    </div></div></div>
    <div class="jempty"><div class="msg">Your journal is empty</div><div class="sub">Games you play show up here with the time
      you gave them, and they stay after you remove them. Rate a game, say how far you got and leave a note from its ☰
      menu on Home, or here.</div></div>`;
  c.root.querySelector('.bar').before(el);
  for (const k of ['jview', 'jcontent', 'jcount', 'jsortlabel', 'jstats', 'jlist', 'jring', 'jempty']) $[k] = el.querySelector(`.${k}`);
  $.el = el;
  host.on('library', () => reload());
  reload();
  window.__journal = { state: () => ({ index, focused, sort: sortId, order: entries.map(g => g.id), scroll: scrollY }), sort: id => applySort(id) };
}

export const sort = () => sortId;
export const count = () => entries.length;

export async function reload() {
  const keep = entries[index]?.id;
  let list = [];
  try { list = await host.library.journal(); } catch (e) { console.warn('journal', e); }
  entries = sorted(list ?? [], sortId);
  index = Math.max(0, entries.findIndex(g => g.id === keep));
  // a review from the journal page can move its game (sorted by rating): the list follows it
  if (focused && scrollY !== scrollFor(index)) scrollTo(scrollFor(index), true);
  paint();
}

export function applySort(id) {
  if (!SORTS.some(s => s.id === id)) return false;
  sortId = id;
  try { localStorage.setItem('journal.sort', id); } catch {}
  entries = sorted(entries, id);
  index = 0;
  scrollTo(0, true);
  paint();
  return true;
}

// the tab comes on (§2: the page stays empty 200 ms, then fades in) or goes (cut)
export function show(on) {
  $.el.classList.toggle('on', on);
  $.el.getAnimations().forEach(a => a.cancel());
  if (on) {
    scrollTo(0, true);
    index = 0;
    reload();
    $.el.animate([{ opacity: 0 }, { opacity: 1 }], { ...TAB_FADE, fill: 'backwards' });
  } else blur();
}

export function focus(ring = RING_IN) {
  if (!entries.length) return false;
  focused = true;
  placeFocus(ring);
  return true;
}

export function blur() {
  focused = false;
  placeFocus(null);
}

// the morph back to Home: fade with the rest
export function fadeOut(o) {
  $.el.animate([{ opacity: 1 }, { opacity: 0 }], o);
}

export function action(name, e) {
  const n = entries.length;
  if (!n) return name === 'left' ? ctx.toButton() : name === 'up' ? ctx.toTabs() : false;
  if (name === 'up') return index > 0 ? move(index - 1) : (!e.held && ctx.toTabs());
  if (name === 'down') return index < n - 1 && move(index + 1);
  if (name === 'left') return !e.held && ctx.toButton();
  if (name === 'right') return false;
  if (e.held) return false;
  if (name === 'lt' || name === 'rt') {
    const i = Math.max(0, Math.min(n - 1, index + (name === 'lt' ? -PAGE : PAGE)));
    return i !== index && move(i);
  }
  if (name === 'wheel') return e.dy ? action(e.dy > 0 ? 'down' : 'up', {}) || 'none' : 'none';
  if (name === 'a') return open(index);
  return false;
}

// the mouse: library.js passes `jrow:<i>` targets on
export function pointer(name, i) {
  if (!entries[i]) return 'none';
  if (name === 'click') return open(i);
  if (i !== index || !focused) { focused = true; move(i); }
  return 'none';
}

function open(i) {
  const g = entries[i];
  if (!g) return false;
  ctx.app.push('journal', { id: g.id });
  return true;
}

// ---------- order and totals ----------
const byName = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
const lastSeen = g => g.lastPlayed ?? g.removedAt ?? g.addedAt ?? -Infinity;
const ORDER = {
  played: (a, b) => lastSeen(b) - lastSeen(a) || byName(a, b),
  most: (a, b) => (b.playtimeSec ?? 0) - (a.playtimeSec ?? 0) || byName(a, b),
  rating: (a, b) => (b.journal?.rating ?? 0) - (a.journal?.rating ?? 0) || (b.playtimeSec ?? 0) - (a.playtimeSec ?? 0) || byName(a, b),
  progress: (a, b) => (b.journal?.progress ?? -1) - (a.journal?.progress ?? -1) || byName(a, b),
  az: byName,
};
const sorted = (list, id) => [...list].sort(ORDER[id] ?? ORDER.played);

function totals() {
  const sec = entries.reduce((s, g) => s + (g.playtimeSec ?? 0), 0);
  const rated = entries.map(g => g.journal?.rating).filter(Boolean);
  const h = Math.floor(sec / 3600);
  return [
    [h ? `${h.toLocaleString()}<small> h</small>` : `${Math.round(sec / 60)}<small> min</small>`, 'Time played'],
    [String(entries.length), entries.length === 1 ? 'Game' : 'Games'],
    [String(entries.filter(g => done(g.journal)).length), 'Finished'],
    [rated.length ? `${(rated.reduce((s, r) => s + r, 0) / rated.length).toFixed(1)}<small class="st">${STAR}</small>` : '—', 'Average rating'],
  ];
}

// ---------- drawing ----------
const rowY = i => LIST_Y + i * PITCH;
const scrollFor = i => (i === 0 ? 0 : Math.max(0, rowY(i) - PIN_Y));

function paint() {
  $.el.classList.toggle('none', !entries.length);
  $.jcount.textContent = `Played: ${entries.length}`;
  $.jsortlabel.textContent = `Sort by: ${SORTS.find(s => s.id === sortId).label}`;
  $.jstats.innerHTML = totals().map(([v, l]) => `<div class="st"><div class="v">${v}</div><div class="l">${l}</div></div>`).join('');
  index = Math.min(index, Math.max(0, entries.length - 1));
  if (!entries.length) focused = false;
  for (const el of rows.values()) el.remove();
  rows.clear();
  renderRows(scrollY, scrollY);
  placeFocus(null);
}

function renderRows(fromY, toY) {
  const top = Math.min(fromY, toY) + CLIP_Y, bottom = Math.max(fromY, toY) + VIEW_H;
  const r0 = Math.max(0, Math.floor((top - LIST_Y) / PITCH) - 1);
  const r1 = Math.min(entries.length - 1, Math.floor((bottom - LIST_Y) / PITCH) + 1);
  for (const [i, el] of rows) if (i < r0 - 1 || i > r1 + 1) { el.remove(); rows.delete(i); }
  for (let i = r0; i <= r1; i++) if (!rows.has(i)) rows.set(i, row(i));
}

function row(i) {
  const g = entries[i], j = g.journal ?? {};
  const el = document.createElement('div');
  el.className = `jrow${g.removedAt ? ' removed' : ''}${focused && i === index ? ' focus' : ''}`;
  el.dataset.nav = `jrow:${i}`;
  el.style.transform = `translate(${X0}px, ${rowY(i)}px)`;
  const st = statusOf(j.status);
  const meta = [
    g.playtimeSec ? hours(g.playtimeSec) : 'No time recorded',
    g.sessions ? `${g.sessions} session${g.sessions === 1 ? '' : 's'}` : null,
    g.restoredAt ? (g.lastPlayed ? `Last played ${lower(ago(g.lastPlayed))}` : null) : g.removedAt ? `Removed ${date(g.removedAt)}` : g.lastPlayed ? `Last played ${lower(ago(g.lastPlayed))}` : null,
  ].filter(Boolean).join('  ·  ');
  el.innerHTML = `
    <div class="jart"></div>
    <div class="jtitle"><span class="n"></span>${g.removedAt ? `<span class="tag">${g.restoredAt ? 'From backup' : 'Removed'}</span>` : ''}</div>
    <div class="jmeta"></div>
    <div class="jstatus${j.status ? '' : ' unset'}"><b style="background:${st.color}"></b><span>${st.label}</span></div>
    <div class="jstars">${stars(j.rating)}</div>
    <div class="jprog${j.progress == null ? ' unset' : ''}${j.progress === 100 ? ' full' : ''}"><div class="track"><div class="fill" style="width:${j.progress ?? 0}%"></div></div><span>${j.progress == null ? '— %' : `${j.progress} %`}</span></div>`;
  el.querySelector('.jtitle .n').textContent = g.name;
  el.querySelector('.jmeta').textContent = meta;
  const art = el.querySelector('.jart');
  const noArt = () => {
    art.style.background = tint(g.id);
    art.append(Object.assign(document.createElement('span'), { className: 'name', textContent: g.name }));
  };
  const src = squareArt(g);
  if (src) {
    const img = new Image();
    img.alt = '';
    img.decoding = 'async';
    img.src = host.fileUrl(src);
    if (!g.art?.tile) img.classList.add('portrait');
    img.onerror = () => { img.remove(); noArt(); };
    img.decode().then(() => img.classList.add('in'), () => img.classList.add('in'));
    art.append(img);
  } else noArt();
  $.jlist.append(el);
  return el;
}

const lower = s => (/^(Today|Yesterday)$/.test(s) ? s.toLowerCase() : s);

function move(i) {
  if (i < 0 || i >= entries.length) return false;
  index = i;
  focused = true;
  if (scrollY !== scrollFor(i)) scrollTo(scrollFor(i));
  placeFocus(RING_IN);
  return true;
}

function placeFocus(ring) {
  ringAnim?.cancel();
  for (const [i, el] of rows) el.classList.toggle('focus', focused && i === index);
  const on = focused && !!entries[index];
  $.jring.hidden = !on;
  if (!on) return;
  $.jring.style.transform = `translate(${X0 - 6}px, ${rowY(index) - 6}px)`;
  if (ring) ringAnim = $.jring.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ring.ms, delay: ring.delay, easing: 'linear', fill: 'backwards' });
}

// the Library's row settle (library.js passes its curve); a retarget starts from where the list is now
function scrollTo(y, instant = false) {
  const { curve, ms, frame, ease } = ctx.scroll;
  let from = scrollY;
  if (scrollAnim?.playState === 'running') from = scrollFrom + (scrollY - scrollFrom) * ease((Math.max(0, scrollAnim.currentTime ?? 0) + frame) / ms);
  scrollAnim?.cancel();
  scrollAnim = null;
  scrollFrom = instant ? y : from;
  scrollY = y;
  renderRows(scrollFrom, y);
  $.jcontent.style.transform = `translateY(${-y}px)`;
  if (!instant) scrollAnim = $.jcontent.animate([{ transform: `translateY(${-from}px)` }, { transform: `translateY(${-y}px)` }],
    { duration: ms, delay: -frame, easing: curve });
}
