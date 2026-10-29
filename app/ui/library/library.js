// Game Library (row A8): design/ps5/2-library.md, adapted to one games folder. The contract is at the top of app.js.
//
// Layout (§1.1, §1.2): header icon and "Game Library", the tab row (one tab until stores add "Installed"), "All: N"
// and "Sort by: …", the sort button, and a 5-column grid of 296 px covers (pitch 320) from x 172, y 338. A scrolled
// row is pinned with its top at y 280; the first row sits at 338. Nothing draws above y 260.
//
// Motion (§2), all on the compositor (transform and opacity):
//   Home → Library   every element moves from its Home spot (the focused Library tile, its label, the preview grid
//                    116 px lower) to its Library spot: 14 frames on the measured curve. The tabs appear at +400 ms
//                    and the ring fades in at the end. B runs it backwards (tabs and button out in 70 ms, then 13
//                    frames) and lands on Home with the Library tile focused.
//   row scroll       the measured exponential settle (29 % of the remaining distance per frame, 14 frames), started
//                    one frame in so the press frame moves. A held direction retargets it mid-flight.
//   focus move       the old ring and caption go in 1 frame; the new ring fades in after 140 ms over 140 ms; the
//                    bottom shade cross-fades over 200 ms. A long title scrolls once at 60 px/s after 1.9 s.
//   sort menu        dim to 65 % and the menu fade in over 150 ms; the menu closes in 1 frame, the dim fades out over
//                    150 ms.
// Covers load lazily per row, draw nothing until decoded, then fade in (§1.10).
// Buttons: A shows the game on Home, B goes back to Home's Library tile, Y opens Search, Menu or the sort button
// opens the sort menu, LT / RT jump 2 rows; Up from the first row reaches the tabs, Left from column 1 the button.

const FRAME = 1000 / 60;
const COLS = 5, PITCH = 320, X0 = 172, Y0 = 338;  // covers are 296 px (library.css)
const PIN_Y = 280, CLIP_Y = 260, VIEW_H = 1080;
const PAGE_ROWS = 2;                                  // LT / RT: the rows fully on screen once scrolled
// held Up / Down: the input layer steps per 114 px of its run; a row is 320, so take a row at most every 45 ms
// (the PS5 repeats every ≈50 ms and moves 100–140 px a frame, §2 "Held Down")
const HELD_ROW_MS = 45;
const HOME_DY = 116;                                  // Home's preview grid sits 116 px lower (§1.3)
// Home → Library, measured per frame (§2: the label's right edge); Library → Home runs the same curve in 13 frames
const MORPH = 'linear(0, .092, .283, .467, .625, .739, .826, .886, .924, .951, .967, .978, .989, .995, 1)';
// the tabs appear ≈400 ms and the ring fades in 139–339 ms after the press frame (§2); the waits are a frame or two
// short of that because a newly shown screen's animations start a frame late
const MORPH_MS = 14 * FRAME, BACK_MS = 13 * FRAME, BACK_FADE_MS = 4 * FRAME, TABS_AT = 380;
const RING_IN = { delay: 140, ms: 140 }, RING_AFTER_MORPH = { delay: 115, ms: 200 };
// one row scroll, measured per frame (§2): 0, 109, 189, 246, 288, 317, 338, 352, 361, 367, 371, 374, 376, 377, 378
const SCROLL_PTS = [0, .288, .5, .651, .762, .839, .894, .931, .955, .971, .981, .989, .995, .997, 1];
const SCROLL = `linear(${SCROLL_PTS.join(', ')})`, SCROLL_MS = 14 * FRAME;
const MARQUEE_DELAY = 1900, MARQUEE_PX_MS = 60 / 1000, MARQUEE_GAP = 64;
const MENU_MS = 150;
const BAR = { top: 338, len: 704, min: 168, hideAfter: 3000 };

export const SORTS = [
  { id: 'played', label: 'Recently played' },
  { id: 'added', label: 'Recently added' },
  { id: 'az', label: 'Name (A - Z)' },
  { id: 'za', label: 'Name (Z - A)' },
  { id: 'most', label: 'Most played' },
];
const TABS = [{ id: 'collection', label: 'Your Collection' }]; // stores add { id: 'installed', label: 'Installed' }

const ICONS = {
  library: '<svg viewBox="0 0 64 56"><g fill="#fff"><rect width="13" height="13" rx="2"/><rect x="18" width="13" height="13" rx="2"/><rect x="36" width="13" height="13" rx="2"/><rect y="18" width="13" height="13" rx="2"/><rect y="36" width="13" height="13" rx="2"/><path d="M27 24h26a9 9 0 0 1 8.7 6.8l2 9.2a6 6 0 0 1-10.4 5.2L49 41H31l-4.3 4.2a6 6 0 0 1-10.4-5.2l2-9.2A9 9 0 0 1 27 24Zm2.5 6v3.5H26v3h3.5V40h3v-3.5H36v-3h-3.5V30Zm19 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm5 3.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z"/></g></svg>',
  // a down arrow beside three lines (§1.1, 35 × 34)
  sort: '<svg viewBox="0 0 35 34" fill="none" stroke="currentColor" stroke-width="3.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 4v25M2.5 23.5 8 29.5l5.5-6"/><path d="M18 6h15M18 15h15M18 24h10"/></svg>',
  check: '<svg viewBox="0 0 22 16" fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M1.5 8.5l6 6L20.5 1.5"/></svg>',
};

// the header glyph is Home's Library tile glyph (76 × 67 in a 168 px tile at 172, 126), scaled to 41 px wide in the
// PS5's 39 × 36 slot at (69, 70)
const ICON_S = 41 / 76;
const ICON_HOME = 'translate(172px, 126px)';
const ICON_REST = `translate(${68 - 46 * ICON_S}px, ${72 - 50.5 * ICON_S}px) scale(${ICON_S})`; // ink 68–108 × 72–103: centred in the 39 × 36 slot
// "Game Library": Home's label (30 px, ink x 357 cap 253) → the header title (31 px, x 172 cap 77)
const TITLE_HOME = `translate(182px, 176px) scale(${30 / 31})`;

let app, host, root;
const $ = {};
let games = [];          // sorted
let sortId = 'played';
let gamesRoot = '';
let zone = 'grid';       // grid | tabs | button | menu | busy
let index = 0, menuIndex = 0, menuFrom = 'grid', tabIndex = 0, heldRowAt = 0;
let scrollY = 0;         // the content's scroll target
let scrollAnim = null, scrollFrom = 0;
let ringAnim = null, marquee = null, marqueeTimer = 0, barTimer = 0, backTimer = 0;
let onTop = false;
const els = new Map();   // index → cover element (rows near the view only), kept while its game and art are the same

try { sortId = localStorage.getItem('library.sort') || sortId; } catch {}
if (!SORTS.some(s => s.id === sortId)) sortId = 'played';

export default {
  async mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="lbg"></div>
      <div class="view"><div class="slide"><div class="content">
        <div class="countline"><span class="count"></span><span class="sortlabel"></span></div>
        <div class="grid"></div>
        <div class="ring"></div>
        <div class="caption"><div class="title"><span></span></div><div class="status"></div></div>
      </div></div></div>
      <div class="empty"><div class="msg">No games yet</div><div class="sub"></div></div>
      <div class="bar"></div>
      <div class="tabs">${TABS.map((t, i) => `<span class="tab${i ? '' : ' sel'}" data-nav="tab:${i}">${t.label}</span>`).join('')}<div class="tabbox"></div></div>
      <div class="dim"></div>
      <div class="head"><div class="hicon"><div class="sq"></div>${ICONS.library}</div><div class="htitle">Game Library</div></div>
      <div class="sortbtn" data-nav="sort">${ICONS.sort}</div>
      <div class="menu">${SORTS.map((s, i) => `<div class="opt" data-nav="sort:${i}" style="top:${3 + i * 72}px">${ICONS.check}<span>${s.label}</span></div>`).join('')}<div class="mring"></div></div>`;
    for (const k of ['lbg', 'view', 'slide', 'content', 'countline', 'count', 'sortlabel', 'grid', 'ring', 'caption', 'empty',
      'bar', 'tabs', 'tabbox', 'dim', 'head', 'hicon', 'sq', 'htitle', 'sortbtn', 'menu', 'mring']) $[k] = el.querySelector(`.${k}`);
    $.title = el.querySelector('.caption .title span');
    $.status = el.querySelector('.caption .status');
    $.tabEls = [...el.querySelectorAll('.tab')];
    $.opts = [...el.querySelectorAll('.menu .opt')];
    $.lbg.append(illustration());
    $.hicon.style.transform = ICON_REST;
    placeTabBox();

    host.on('library', list => setGames(list));
    host.on('settings', s => { gamesRoot = s?.gamesRoot ?? gamesRoot; paintEmpty(); });
    host.settings.get().then(s => { gamesRoot = s?.gamesRoot ?? ''; paintEmpty(); }).catch(() => {});
    // read-only state for tests (tools/app/ui-probe.mjs eval steps)
    window.__library = {
      state: () => ({ zone, index, id: games[index]?.id, sort: sortId, order: games.map(g => g.id), scroll: scrollY, menu: menuIndex }),
      sort: id => applySort(id),
    };
    setGames(await host.library.list());
  },

  enter(params = {}) {
    onTop = true;
    if (params.back) return;
    const booting = document.documentElement.dataset.ready !== '1';
    reset();
    if (!booting) morphIn();
  },

  leave() {
    onTop = false;
    clearTimeout(backTimer);
    closeMenu(true);
    // the morphs hold their end state (fill forwards) until the screen goes; cancel them so the next enter starts clean
    root.getAnimations({ subtree: true }).forEach(a => a.cancel());
    stopMarquee();
    if (zone === 'busy') zone = 'grid';
  },

  action(name, e = {}) {
    if (zone === 'busy') return 'none';
    if (name === 'release') return false;
    if (name === 'b' && zone !== 'menu') return back();
    if (name === 'y' && zone !== 'menu' && !e.held) { app.push('search'); return true; } // as on Home
    if (name === 'lb' || name === 'rb') return switchTab(tabIndex + (name === 'lb' ? -1 : 1));
    if (name === 'point' || name === 'click') return pointer(name, e);
    if (zone === 'menu') return menuAction(name, e);
    if (name === 'menu' && !e.held) return openMenu(zone === 'button' ? 'button' : 'grid');
    if (zone === 'tabs') return tabsAction(name, e);
    if (zone === 'button') return buttonAction(name, e);
    return gridAction(name, e);
  },
};

// ---------- data and sort ----------
function setGames(list) {
  const keep = games[index]?.id;
  games = sorted(list ?? [], sortId);
  index = Math.max(0, games.findIndex(g => g.id === keep));
  if (!games.length && zone === 'grid') zone = 'tabs';
  paintCounts();
  rebuild();
  paintZone();
}

// the sort orders; ties fall back to the name, A–Z
const byName = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
const desc = key => (a, b) => (b[key] ?? -Infinity) - (a[key] ?? -Infinity) || byName(a, b);
const ORDER = {
  // also Home's row (sorted(list, 'played')): last played, then added, newest first; never played after played
  played: (a, b) => (b.lastPlayed ?? -Infinity) - (a.lastPlayed ?? -Infinity) || desc('addedAt')(a, b),
  added: desc('addedAt'),
  az: byName,
  za: (a, b) => byName(b, a),
  most: (a, b) => (b.playtimeSec ?? 0) - (a.playtimeSec ?? 0) || desc('lastPlayed')(a, b),
};
export const sorted = (list, id) => [...list].sort(ORDER[id] ?? ORDER.played);

function applySort(id) {
  if (!ORDER[id]) return false;
  sortId = id;
  try { localStorage.setItem('library.sort', id); } catch {}
  games = sorted(games, id);
  index = 0;
  paintCounts();
  rebuild();
  return true;
}

function paintCounts() {
  $.count.textContent = `All: ${games.length}`;
  $.sortlabel.textContent = `Sort by: ${SORTS.find(s => s.id === sortId).label}`;
  root.classList.toggle('none', !games.length);
  paintEmpty();
}

function paintEmpty() {
  if (!$.empty) return;
  $.empty.querySelector('.sub').textContent =
    `Put each game in its own folder in ${gamesRoot || 'the games folder'}. New folders show up here a few minutes after they stop changing.`;
}

// ---------- the grid ----------
const rowOf = i => Math.floor(i / COLS);
const rowsCount = () => Math.ceil(games.length / COLS);
const coverX = i => X0 + (i % COLS) * PITCH;
const coverY = i => Y0 + rowOf(i) * PITCH;
const scrollFor = i => (rowOf(i) === 0 ? 0 : coverY(i) - PIN_Y); // the first row stays at 338, the rest pin at 280

function reset() {
  zone = games.length ? 'grid' : 'tabs';
  index = 0;
  closeMenu(true);
  scrollAnim?.cancel();
  scrollAnim = null;
  scrollY = 0;
  $.content.style.transform = '';
  rebuild();
  paintZone();
}

function rebuild() {
  if (!$.grid) return;
  index = Math.min(index, Math.max(0, games.length - 1));
  renderRows(scrollY, scrollY);
  placeFocus(null);
  paintBar(false);
}

// keep cover elements for the rows on screen at either end of a scroll, plus one row each way
function renderRows(fromY, toY) {
  const top = Math.min(fromY, toY) + CLIP_Y, bottom = Math.max(fromY, toY) + VIEW_H;
  const r0 = Math.max(0, Math.floor((top - Y0) / PITCH) - 1);
  const r1 = Math.min(rowsCount() - 1, Math.floor((bottom - Y0) / PITCH) + 1);
  for (const [i, el] of els) {
    if (i < games.length && rowOf(i) >= r0 - 1 && rowOf(i) <= r1 + 1 && el.dataset.key === key(i)) continue;
    el.remove();
    els.delete(i);
  }
  for (let i = r0 * COLS; i <= Math.min(games.length - 1, r1 * COLS + COLS - 1); i++) if (!els.has(i)) els.set(i, cover(i));
}

// a cover is redrawn only when its slot gets another game or new art (a library event while art arrives)
const art = g => g.art?.tile ?? g.art?.cover;
const key = i => `${games[i].id}|${art(games[i]) ?? ''}`;

function cover(i) {
  const g = games[i];
  const el = document.createElement('div');
  el.className = `cover${i === index && zone === 'grid' ? ' focus' : ''}`;
  el.dataset.nav = `cover:${i}`;
  el.dataset.key = key(i);
  el.style.transform = `translate(${coverX(i)}px, ${coverY(i)}px)`;
  const noArt = () => {
    el.classList.add('noart');
    el.style.background = tint(g.id);
    el.prepend(Object.assign(document.createElement('span'), { className: 'name', textContent: g.name }));
  };
  if (art(g)) {
    const img = new Image();
    img.alt = '';
    img.decoding = 'async';
    img.src = host.fileUrl(art(g));
    if (!g.art?.tile) img.classList.add('portrait');
    // a missing art file (deleted, or a data dir that moved): the tinted placeholder, never a broken image
    img.onerror = () => { img.remove(); noArt(); };
    // nothing draws until the art is decoded, then it fades in (§1.10)
    img.decode().then(() => img.classList.add('in'), () => img.classList.add('in'));
    el.append(img);
  } else noArt();
  el.append(Object.assign(document.createElement('div'), { className: 'shade' }));
  $.grid.append(el);
  return el;
}

// a stable colour per entry without art (as Home's tiles)
function tint(id) {
  let h = 0;
  for (const c of id) h = (h * 31 + c.codePointAt(0)) % 360;
  return `linear-gradient(145deg, hsl(${h} 45% 34%), hsl(${(h + 25) % 360} 50% 16%))`;
}

function gridAction(name, e) {
  const n = games.length;
  if (!n) return false;
  const col = index % COLS;
  if ((name === 'up' || name === 'down') && e.held) {
    const now = performance.now();
    if (now - heldRowAt < HELD_ROW_MS) return (name === 'up' ? rowOf(index) > 0 : rowOf(index) < rowsCount() - 1) && 'none';
    heldRowAt = now;
  }
  if (name === 'left') return col > 0 ? focus(index - 1) : (!e.held && focusButton());
  if (name === 'right') return col < COLS - 1 && index + 1 < n && focus(index + 1);
  if (name === 'up') return rowOf(index) > 0 ? focus(index - COLS) : (!e.held && focusTabs());
  if (name === 'down') return rowOf(index) < rowsCount() - 1 && focus(Math.min(n - 1, index + COLS));
  if (e.held) return false;
  if (name === 'lt' || name === 'rt') {
    const row = Math.max(0, Math.min(rowsCount() - 1, rowOf(index) + (name === 'lt' ? -PAGE_ROWS : PAGE_ROWS)));
    return row !== rowOf(index) && focus(Math.min(n - 1, row * COLS + col));
  }
  if (name === 'wheel') return e.dy ? gridAction(e.dy > 0 ? 'down' : 'up', {}) || 'none' : 'none';
  if (name === 'a') return open(index);
  return false;
}

function open(i) {
  const g = games[i];
  if (!g) return false;
  app.go('home', { focus: g.id });
  return true;
}

// move focus to cover i: the old ring and caption go now, the new ring fades in later; the grid scrolls if the row
// changed
function focus(i, ring = RING_IN) {
  if (i < 0 || i >= games.length) return false;
  const prev = index;
  index = i;
  zone = 'grid';
  paintZone();
  if (rowOf(i) !== rowOf(prev) || scrollY !== scrollFor(i)) scrollTo(scrollFor(i));
  placeFocus(ring);
  return true;
}

function placeFocus(ring) {
  stopMarquee();
  ringAnim?.cancel();
  for (const [i, el] of els) el.classList.toggle('focus', i === index && zone === 'grid');
  const g = games[index];
  const on = !!g && zone === 'grid';
  $.ring.hidden = $.caption.hidden = !on;
  if (!on) return;
  const x = coverX(index), y = coverY(index);
  $.ring.style.transform = `translate(${x - 6}px, ${y - 6}px)`;
  $.caption.style.transform = `translate(${x}px, ${y}px)`;
  $.title.textContent = g.name;
  $.title.style.transform = '';
  $.status.textContent = status(g);
  if (ring) {
    ringAnim = $.ring.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ring.ms, delay: ring.delay, easing: 'linear', fill: 'backwards' });
    if (ring !== RING_IN) $.caption.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ring.ms, delay: ring.delay, easing: 'linear', fill: 'backwards' });
  }
  startMarquee();
}

// a long title scrolls left once, 1.9 s after focus, at 60 px/s, and ends where it started (a copy follows it in)
function startMarquee() {
  if (!onTop) return;
  const span = $.title, box = span.parentElement;
  const over = span.scrollWidth - box.clientWidth;
  if (over <= 0) return;
  marqueeTimer = setTimeout(() => {
    const dist = span.scrollWidth + MARQUEE_GAP;
    span.dataset.copy = span.textContent;
    span.classList.add('loop');
    marquee = span.animate([{ transform: 'translateX(0)' }, { transform: `translateX(${-dist}px)` }],
      { duration: dist / MARQUEE_PX_MS, easing: 'linear' });
    marquee.onfinish = () => span.classList.remove('loop');
  }, MARQUEE_DELAY);
}

function stopMarquee() {
  clearTimeout(marqueeTimer);
  marquee?.cancel();
  marquee = null;
  $.title?.classList.remove('loop');
}

function status(g) {
  if (!g.lastPlayed && !g.playtimeSec) return 'Not played yet';
  return [g.playtimeSec ? playtime(g.playtimeSec) : null, g.lastPlayed ? ago(g.lastPlayed) : null].filter(Boolean).join(' · ');
}

function playtime(sec) {
  const h = Math.floor(sec / 3600), m = Math.round((sec % 3600) / 60);
  return h ? `${h} h${m ? ` ${m} min` : ''}` : `${Math.max(1, m)} min`;
}

function ago(t) {
  const ms = t < 1e12 ? t * 1000 : t;
  const day = d => new Date(d).setHours(0, 0, 0, 0);
  const days = Math.round((day(Date.now()) - day(ms)) / 864e5);
  if (days <= 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days < 7) return `${days} days ago`;
  return new Date(ms).toLocaleDateString([], { day: 'numeric', month: 'short', year: 'numeric' });
}

// ---------- scrolling ----------
// the measured settle as a compositor animation; a retarget starts again from where the grid is now
function scrollTo(y) {
  let from = scrollY;
  if (scrollAnim?.playState === 'running') {
    const t = (Math.max(0, scrollAnim.currentTime ?? 0) + FRAME) / SCROLL_MS; // it started one frame in
    from = scrollFrom + (scrollY - scrollFrom) * ease(SCROLL_PTS, t);
  }
  scrollAnim?.cancel();
  scrollFrom = from;
  scrollY = y;
  renderRows(from, y);
  $.content.style.transform = `translateY(${-y}px)`;
  scrollAnim = $.content.animate([{ transform: `translateY(${-from}px)` }, { transform: `translateY(${-y}px)` }],
    { duration: SCROLL_MS, delay: -FRAME, easing: SCROLL });
  paintBar(true);
}

function ease(pts, t) {
  if (t >= 1) return 1;
  const f = Math.max(0, t) * (pts.length - 1), i = Math.floor(f);
  return pts[i] + (pts[i + 1] - pts[i]) * (f - i);
}

// the scrollbar thumb at x 1760: shown while scrolling, hidden 3 s after the last scroll (§2)
function paintBar(scrolled) {
  const rows = rowsCount();
  const max = rows > 1 ? scrollFor((rows - 1) * COLS) : 0;
  $.bar.hidden = rows <= 2;
  if ($.bar.hidden) return;
  const len = Math.min(BAR.len, Math.max(BAR.min, BAR.len * Math.min(1, 2.5 / rows)));
  const y = BAR.top + (BAR.len - len) * (max ? scrollY / max : 0);
  $.bar.style.height = `${len}px`;
  $.bar.style.transform = `translateY(${y}px)`;
  $.bar.classList.remove('off');
  clearTimeout(barTimer);
  barTimer = setTimeout(() => $.bar.classList.add('off'), BAR.hideAfter + (scrolled ? SCROLL_MS : 0));
}

// ---------- tabs, the sort button and the sort menu ----------
function focusTabs() {
  zone = 'tabs';
  placeFocus(null);
  paintZone();
  return true;
}

function focusButton() {
  zone = 'button';
  placeFocus(null);
  paintZone();
  return true;
}

// the tab box fades in 300 ms after a 170 ms delay when focus comes to the tabs (§2 "Tab box moves")
function paintZone() {
  const tabs = zone === 'tabs';
  if (tabs && !root.classList.contains('on-tabs')) $.tabbox.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 300, delay: 170, easing: 'linear', fill: 'backwards' });
  root.classList.toggle('on-tabs', tabs);
  $.sortbtn.classList.toggle('focus', zone === 'button');
}

// the box sits 30 px either side of the selected tab's text
function placeTabBox() {
  const t = $.tabEls[tabIndex];
  $.tabbox.style.transform = `translateX(${t.offsetLeft - 30}px)`;
  $.tabbox.style.width = `${t.offsetWidth + 60}px`;
}

function tabsAction(name, e) {
  if (name === 'down') return games.length ? focus(index) : false;
  if (name === 'left' || name === 'right') return switchTab(tabIndex + (name === 'left' ? -1 : 1));
  if (e.held) return false;
  if (name === 'a') return 'none';
  return false;
}

// one tab until stores add "Installed"; a second tab slots in here
function switchTab(i) {
  if (i < 0 || i >= TABS.length || i === tabIndex) return false;
  tabIndex = i;
  $.tabEls.forEach((el, k) => el.classList.toggle('sel', k === i));
  placeTabBox();
  paintZone();
  return true;
}

function buttonAction(name, e) {
  if (name === 'right' || name === 'down') return games.length ? focus(rowOf(index) * COLS) : false;
  if (name === 'up') return !e.held && focusTabs();
  if (e.held) return false;
  if (name === 'a') return openMenu('button');
  return false;
}

function openMenu(from) {
  menuFrom = from;
  zone = 'menu';
  placeFocus(null);
  paintZone();
  menuIndex = Math.max(0, SORTS.findIndex(s => s.id === sortId));
  $.opts.forEach((el, i) => el.classList.toggle('cur', SORTS[i].id === sortId));
  paintMenu();
  root.classList.add('menu-open');
  $.dim.getAnimations().forEach(a => a.cancel());
  $.dim.animate([{ opacity: 0 }, { opacity: 1 }], { duration: MENU_MS, easing: 'linear' });
  $.menu.animate([{ opacity: 0 }, { opacity: 1 }], { duration: MENU_MS, easing: 'linear' });
  return true;
}

// the menu goes in 1 frame; the dim fades out over 150 ms (§2 "Filter panel close")
function closeMenu(instant) {
  if (!root?.classList.contains('menu-open')) return false;
  root.classList.remove('menu-open');
  $.menu.getAnimations().forEach(a => a.cancel());
  $.dim.getAnimations().forEach(a => a.cancel());
  if (!instant) $.dim.animate([{ opacity: 1 }, { opacity: 0 }], { duration: MENU_MS, easing: 'linear' });
  if (zone === 'menu') {
    zone = menuFrom === 'button' || !games.length ? 'button' : 'grid';
    if (zone === 'grid') placeFocus(RING_IN);
    paintZone();
  }
  return true;
}

function paintMenu() {
  $.mring.style.transform = `translateY(${menuIndex * 72}px)`;
  $.opts.forEach((el, i) => el.classList.toggle('focus', i === menuIndex));
}

function menuAction(name, e) {
  if (name === 'up' || name === 'down') {
    const i = menuIndex + (name === 'up' ? -1 : 1);
    if (i < 0 || i >= SORTS.length) return false;
    menuIndex = i;
    paintMenu();
    return true;
  }
  if (e.held) return false;
  if (name === 'a') {
    const chosen = SORTS[menuIndex].id;
    closeMenu(false);
    if (chosen !== sortId) {
      applySort(chosen);
      if (zone === 'grid') placeFocus(RING_IN);
    }
    return true;
  }
  if (name === 'b' || name === 'menu') { closeMenu(false); return 'back'; }
  return name === 'left' || name === 'right' ? 'none' : false;
}

// ---------- the mouse ----------
function pointer(name, e) {
  const [kind, n] = (e.target ?? '').split(':');
  const i = Number(n);
  if (zone === 'menu') {
    if (kind !== 'sort' || n === undefined) return name === 'click' ? (closeMenu(false), 'back') : 'none';
    menuIndex = i;
    paintMenu();
    return name === 'click' ? menuAction('a', {}) : 'none';
  }
  if (kind === 'cover') {
    if (name === 'click') return open(i);
    return i === index && zone === 'grid' ? 'none' : (focus(i), 'none');
  }
  if (kind === 'sort') return name === 'click' ? openMenu('button') : (zone !== 'button' && focusButton(), 'none');
  if (kind === 'tab') return name === 'click' ? switchTab(i) || 'none' : (zone !== 'tabs' && focusTabs(), 'none');
  return 'none';
}

// ---------- Home ↔ Library ----------
function morphIn() {
  const o = { duration: MORPH_MS, delay: -FRAME, easing: MORPH };
  $.hicon.animate([{ transform: ICON_HOME }, { transform: ICON_REST }], o);
  $.sq.animate([{ opacity: 1 }, { opacity: 0 }], o);
  $.htitle.animate([{ transform: TITLE_HOME }, { transform: 'none' }], o);
  $.slide.animate([{ transform: `translateY(${HOME_DY}px)`, opacity: 0 }, { transform: 'none', opacity: 1 }], o);
  $.empty.animate([{ opacity: 0 }, { opacity: 1 }], o);
  $.sortbtn.animate([{ opacity: 0 }, { opacity: 1 }], o);
  $.bar.animate([{ opacity: 0 }, { opacity: 1 }], o);
  $.lbg.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 300, easing: 'linear' });
  $.tabs.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 1, delay: TABS_AT, fill: 'backwards' });
  if (zone === 'grid') placeFocus(RING_AFTER_MORPH);
  zone = zone === 'grid' ? 'busy' : zone;
  if (zone === 'busy') backTimer = setTimeout(() => { if (zone === 'busy') zone = 'grid'; }, MORPH_MS);
}

// B: the tabs and button fade out, then everything morphs back to its Home spot; Home takes over with its Library
// tile focused
function back() {
  closeMenu(true);
  const was = zone;
  zone = 'busy';
  $.ring.hidden = $.caption.hidden = true;
  stopMarquee();
  const fade = { duration: BACK_FADE_MS, easing: 'linear', fill: 'forwards' };
  $.tabs.animate([{ opacity: 1 }, { opacity: 0 }], fade);
  $.sortbtn.animate([{ opacity: 1 }, { opacity: 0 }], fade);
  $.bar.animate([{ opacity: 1 }, { opacity: 0 }], fade);
  const o = { duration: BACK_MS, delay: BACK_FADE_MS, easing: MORPH, fill: 'forwards' };
  $.hicon.animate([{ transform: ICON_REST }, { transform: ICON_HOME }], o);
  $.sq.animate([{ opacity: 0 }, { opacity: 1 }], o);
  $.htitle.animate([{ transform: 'none' }, { transform: TITLE_HOME }], o);
  $.slide.animate([{ transform: 'none', opacity: 1 }, { transform: `translateY(${HOME_DY}px)`, opacity: 0 }], o);
  $.empty.animate([{ opacity: 1 }, { opacity: 0 }], o);
  $.lbg.animate([{ opacity: 1 }, { opacity: 0 }], o);
  backTimer = setTimeout(() => {
    if (zone !== 'busy') return;
    zone = was;
    app.go('home', { focus: '#library' });
  }, BACK_FADE_MS + BACK_MS);
  return 'back';
}

// ---------- background: our own line drawing in the PS5's place (§1.1: dim fine lines on the right, static) ----------
// A twisted ribbon of fine lines down the right side, cool white with a warm band, and a scatter of faint points.
// Drawn once into a canvas; it never moves, so it costs nothing at idle.
function illustration() {
  const W = 1920, H = 1080;
  const c = Object.assign(document.createElement('canvas'), { width: W, height: H });
  const g = c.getContext('2d');
  g.fillStyle = '#000004';
  g.fillRect(0, 0, W, H);
  g.lineWidth = 1;
  const ribbon = (cx, amp, top, len, width, twist, lines, alpha) => {
    for (let j = 0; j < lines; j++) {
      const u = (j / (lines - 1)) * 2 - 1;
      const warm = u > 0.25 && u < 0.6;
      const a = alpha * (1 - 0.6 * Math.abs(u));
      g.strokeStyle = warm ? `rgba(190, 100, 62, ${a * 1.6})` : `rgba(190, 200, 228, ${a})`;
      g.beginPath();
      for (let k = 0; k <= 40; k++) {
        const s = k / 40;
        const x0 = cx + amp * Math.sin(2.4 * s + 0.4), y0 = top + len * s;
        const w = width * Math.sin(Math.PI * s) * Math.cos(twist * s + 0.6);
        const x = x0 + u * (w + 22), y = y0 + u * w * 0.35; // + 22: the lines never meet in one bright point
        k ? g.lineTo(x, y) : g.moveTo(x, y);
      }
      g.stroke();
    }
  };
  ribbon(1500, 170, -80, 1240, 380, 2.7, 120, 0.075);
  ribbon(1690, -150, 60, 940, 210, 3.4, 70, 0.055);
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  for (let k = 0; k < 1400; k++) {
    const x = 1180 + rnd() * 740, y = rnd() * H;
    g.fillStyle = `rgba(210, 215, 235, ${0.03 + rnd() * 0.12})`;
    g.fillRect(x, y, 1.2, 1.2);
  }
  // the left stays #000004 so the text reads
  const fade = g.createLinearGradient(1120, 0, 1420, 0);
  fade.addColorStop(0, 'rgba(0, 0, 4, 1)');
  fade.addColorStop(1, 'rgba(0, 0, 4, 0)');
  g.fillStyle = fade;
  g.fillRect(0, 0, 1420, H);
  return c;
}
