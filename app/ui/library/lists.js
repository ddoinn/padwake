// Library → Lists (the third tab, row B3): smart lists, then your own lists, then New List, as tiles in the cover grid.
// library.js owns the header, tabs, menu, keyboard and the cover grid a list opens into, and routes input here while
// the tiles are on.
//
// Smart lists are each user's, worked out from the library every time they show (the library event carries the
// current user's time, sessions and journal) and hidden when empty. Your own lists are shared by everyone on the PC
// (library/collections.rs); a game whose folder is gone stays in its lists but isn't shown.
//
// Layout (design/ps5/2-library.md): "Lists: N" in the count line's slot (cap 288), then the cover grid: 296 px tiles,
// 5 columns at pitch 320 from (172, 338); a scrolled row pins its top at y 280. A tile is the system-app tile of §1.4:
// the charcoal gradient, here with up to four of the list's covers on it, its name and count bottom-left over the
// cover's bottom shade, and a small glyph top-right on a smart list. New List has a centred + (45 % of the tile).
// Motion is the Library's: the ring fades in after 140 ms over 140 ms, focus jumps (no glide), rows scroll on the
// measured settle, art fades in once decoded (§1.10), the tab fades in after the 200 ms empty page (§2).

import { tint, squareArt } from '../journal/shared.js';

const DAY = 864e5;
const COLS = 5, PITCH = 320, X0 = 172, Y0 = 338, PIN_Y = 280;
const RING_IN = { delay: 140, ms: 140 };
const TAB_FADE = { duration: 315, delay: 200, easing: 'cubic-bezier(.33, 0, .67, 1)' };
const ms = t => (t && t < 1e12 ? t * 1000 : t); // a few old entries keep seconds
const status = g => g.journal?.status ?? null;

// the smart lists, in the order they show
export const SMART = [
  { id: 'smart:playing', name: 'Playing', has: g => status(g) === 'playing' },
  { id: 'smart:never', name: 'Never Played', has: g => !g.sessions && !g.playtimeSec && !g.lastPlayed },
  { id: 'smart:finished', name: 'Finished', has: g => status(g) === 'finished' || status(g) === 'completed' },
  { id: 'smart:paused', name: 'On Hold', has: g => status(g) === 'paused' }, // the journal's name for "paused"
  { id: 'smart:short', name: 'Short Games', has: g => g.hltbMainSec > 0 && g.hltbMainSec <= 10 * 3600 },
  { id: 'smart:dusty', name: 'Not Played in 3 Months', has: (g, now) => !!g.lastPlayed && now - ms(g.lastPlayed) > 90 * DAY
    && !['finished', 'completed', 'dropped'].includes(status(g)) },
];

const ICONS = {
  // a filter: the list fills itself. A line icon, as Settings' and Control Center's (2.6 px at size, round ends).
  smart: '<svg viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h16l-6.2 7.6v5.9l-3.6 1.8v-7.7Z"/></svg>',
  plus: '<svg viewBox="0 0 134 134"><path d="M67 30v74M30 67h74" stroke="#fff" stroke-width="7" stroke-linecap="round"/></svg>',
};

let ctx, host, $ = {};
let all = [];     // the library, as library.js has it
let own = [];     // your lists: [{id, name, games, createdAt}]
let tiles = [];   // [{id, name, kind: smart | own | new, games}]
let index = 0, focused = false, scrollY = 0, scrollFrom = 0, scrollAnim = null, ringAnim = null;

// ctx: {app, host, root, scroll: {curve, ms, frame, ease(t)}, toTabs(), open(id), create(), options(id), changed()}
export function mount(c) {
  ctx = c;
  host = c.host;
  const el = document.createElement('div');
  el.className = 'lists';
  el.innerHTML = `
    <div class="lview"><div class="lslide"><div class="lcontent">
      <div class="countline"><span class="lcount"></span><span></span></div>
      <div class="lgrid"></div>
      <div class="lring"></div>
    </div></div></div>`;
  c.root.querySelector('.bar').before(el);
  for (const k of ['lcontent', 'lcount', 'lgrid', 'lring']) $[k] = el.querySelector(`.${k}`);
  $.el = el;
  host.on('lists', l => setOwn(l));
  Promise.resolve(host.library.lists()).then(setOwn, e => console.warn('lists', e));
  window.__lists = {
    state: () => ({ index, focused, id: tiles[index]?.id, tiles: tiles.map(t => t.name), counts: tiles.map(t => t.games?.length ?? null),
      games: tiles.map(t => t.games?.map(g => g.id) ?? null), scroll: scrollY }),
    own: () => own,
  };
}

// ---------- data ----------
export function setLibrary(list) {
  all = list ?? [];
  paint();
}

function setOwn(list) {
  own = list ?? [];
  paint();
  ctx.changed();
}

// a list's games from the library: a smart list's for whoever is playing now, yours in the order they were added
export function gamesOf(id) {
  const smart = SMART.find(s => s.id === id);
  if (smart) {
    const now = Date.now();
    return all.filter(g => smart.has(g, now));
  }
  const l = own.find(x => x.id === id);
  if (!l) return null;
  const byId = new Map(all.map(g => [g.id, g]));
  return l.games.map(g => byId.get(g)).filter(Boolean);
}

export const nameOf = id => SMART.find(s => s.id === id)?.name ?? own.find(l => l.id === id)?.name ?? null;
export const isOwn = id => own.some(l => l.id === id);
export const count = () => tiles.length;

function makeTiles() {
  const now = Date.now();
  const smart = SMART.map(s => ({ id: s.id, name: s.name, kind: 'smart', games: all.filter(g => s.has(g, now)) })).filter(t => t.games.length);
  return [...smart, ...own.map(l => ({ id: l.id, name: l.name, kind: 'own', games: gamesOf(l.id) })), { id: 'new', name: 'New List', kind: 'new' }];
}

// ---------- the tab ----------
export function show(on) {
  $.el.classList.toggle('on', on);
  $.el.getAnimations().forEach(a => a.cancel());
  if (on) {
    index = 0;
    scrollTo(0, true);
    paint();
    $.el.animate([{ opacity: 0 }, { opacity: 1 }], { ...TAB_FADE, fill: 'backwards' });
  } else blur();
}

// back from an open list: the tiles fade in as a tab does, focus on the list it came from
export function back(id) {
  const i = tiles.findIndex(t => t.id === id);
  if (i >= 0) index = i;
  if (scrollY !== scrollFor(index)) scrollTo(scrollFor(index), true);
  $.el.animate([{ opacity: 0 }, { opacity: 1 }], { ...TAB_FADE, fill: 'backwards' });
}

export function focus(ring = RING_IN) {
  if (!tiles.length) return false;
  focused = true;
  placeFocus(ring);
  return true;
}

export function blur() {
  focused = false;
  placeFocus(null);
}

// a list just made or renamed: focus it once it's drawn
export function focusOn(id) {
  const i = tiles.findIndex(t => t.id === id);
  if (i < 0) return false;
  return move(i);
}

export function fadeOut(o) {
  $.el.animate([{ opacity: 1 }, { opacity: 0 }], o);
}

export function action(name, e) {
  const n = tiles.length, col = index % COLS, row = rowOf(index);
  if (name === 'left') return col > 0 && move(index - 1);
  if (name === 'right') return col < COLS - 1 && index + 1 < n && move(index + 1);
  if (name === 'up') return row > 0 ? move(index - COLS) : (!e.held && ctx.toTabs());
  if (name === 'down') return row < rowOf(n - 1) && move(Math.min(n - 1, index + COLS));
  if (e.held) return false;
  if (name === 'wheel') return e.dy ? action(e.dy > 0 ? 'down' : 'up', {}) || 'none' : 'none';
  if (name === 'a') return activate(index);
  if (name === 'menu') return tiles[index]?.kind === 'own' ? ctx.options(tiles[index].id) : 'none';
  return false;
}

export function pointer(name, i) {
  if (!tiles[i]) return 'none';
  if (name === 'click') return activate(i);
  if (i !== index || !focused) move(i);
  return 'none';
}

function activate(i) {
  const t = tiles[i];
  if (!t) return false;
  return t.kind === 'new' ? ctx.create() : ctx.open(t.id);
}

// ---------- drawing ----------
const rowOf = i => Math.floor(i / COLS);
const tileX = i => X0 + (i % COLS) * PITCH;
const tileY = i => Y0 + rowOf(i) * PITCH;
const scrollFor = i => (rowOf(i) === 0 ? 0 : tileY(i) - PIN_Y);

function paint() {
  if (!$.el) return;
  const keep = tiles[index]?.id;
  tiles = makeTiles();
  const k = tiles.findIndex(t => t.id === keep);
  index = k >= 0 ? k : Math.min(index, tiles.length - 1);
  const lists = tiles.length - 1;
  $.lcount.textContent = `Lists: ${lists}`;
  // tiles are few (six smart lists at most, plus yours): all are drawn, each redrawn only when what it shows changed
  const old = new Map([...$.lgrid.children].map(el => [el.dataset.key, el]));
  const els = tiles.map((t, i) => {
    const key = tileKey(t);
    const el = old.get(key) ?? tile(t);
    old.delete(key);
    el.dataset.nav = `ltile:${i}`;
    el.style.transform = `translate(${tileX(i)}px, ${tileY(i)}px)`;
    return el;
  });
  old.forEach(el => el.remove());
  $.lgrid.replaceChildren(...els);
  if (scrollY > scrollFor(index)) scrollTo(scrollFor(index), true);
  placeFocus(null);
}

const art = g => squareArt(g);
const covers = t => (t.games ?? []).slice(0, 4);
const tileKey = t => `${t.kind}|${t.id}|${t.name}|${t.games?.length ?? ''}|${covers(t).map(g => `${g.id}:${art(g) ?? ''}`).join(',')}`;
const countText = n => (n === 1 ? '1 game' : `${n} games`);

function tile(t) {
  const el = document.createElement('div');
  el.className = `ltile ${t.kind}`;
  el.dataset.key = tileKey(t);
  if (t.kind === 'new') {
    el.innerHTML = `<div class="plus">${ICONS.plus}</div><div class="lname"></div>`;
    el.querySelector('.lname').textContent = t.name;
    return el;
  }
  const shown = covers(t);
  const pics = document.createElement('div');
  pics.className = `pics n${shown.length}`;
  for (const g of shown) {
    const cell = document.createElement('div');
    cell.className = 'cell';
    const noArt = () => { cell.style.background = tint(g.id); };
    if (art(g)) {
      const img = new Image();
      img.alt = '';
      img.decoding = 'async';
      img.src = host.fileUrl(art(g));
      if (!g.art?.tile) img.classList.add('portrait');
      img.onerror = () => { img.remove(); noArt(); };
      img.decode().then(() => img.classList.add('in'), () => img.classList.add('in'));
      cell.append(img);
    } else noArt();
    pics.append(cell);
  }
  el.append(pics);
  el.insertAdjacentHTML('beforeend', `<div class="lshade"></div><div class="lname"></div><div class="lnum"></div>${t.kind === 'smart' ? `<div class="spark">${ICONS.smart}</div>` : ''}`);
  el.querySelector('.lname').textContent = t.name;
  el.querySelector('.lnum').textContent = countText(t.games.length);
  return el;
}

function move(i) {
  if (i < 0 || i >= tiles.length) return false;
  index = i;
  focused = true;
  if (scrollY !== scrollFor(i)) scrollTo(scrollFor(i));
  placeFocus(RING_IN);
  return true;
}

function placeFocus(ring) {
  ringAnim?.cancel();
  [...$.lgrid.children].forEach((el, i) => el.classList.toggle('focus', focused && i === index));
  const on = focused && !!tiles[index];
  $.lring.hidden = !on;
  if (!on) return;
  $.lring.style.transform = `translate(${tileX(index) - 6}px, ${tileY(index) - 6}px)`;
  if (ring) ringAnim = $.lring.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ring.ms, delay: ring.delay, easing: 'linear', fill: 'backwards' });
}

// the Library's row settle (library.js passes its curve); a retarget starts from where the grid is now
function scrollTo(y, instant = false) {
  const { curve, ms: dur, frame, ease } = ctx.scroll;
  let from = scrollY;
  if (scrollAnim?.playState === 'running') from = scrollFrom + (scrollY - scrollFrom) * ease((Math.max(0, scrollAnim.currentTime ?? 0) + frame) / dur);
  scrollAnim?.cancel();
  scrollAnim = null;
  scrollFrom = instant ? y : from;
  scrollY = y;
  $.lcontent.style.transform = `translateY(${-y}px)`;
  if (!instant) scrollAnim = $.lcontent.animate([{ transform: `translateY(${-from}px)` }, { transform: `translateY(${-y}px)` }],
    { duration: dur, delay: -frame, easing: curve });
}
