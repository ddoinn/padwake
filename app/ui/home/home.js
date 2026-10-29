// Home (design/ps5/1-home.md §1.3–1.5, motion §2): the top bar, the tile row, the hero and box art, the background,
// the game hub, the Options menu, and the launch and return transitions.
//
// Motion stays on the compositor (transform and opacity). Tiles are 168 px squares placed with translateX and scaled
// to 106 px, so the focused one draws at full resolution and its corner radius scales with it (19 → 12).
// - A tap slides the row with the PS5's measured spring as a CSS linear() easing, started one frame in so the press
//   frame already moves. The ring, hero and box art cut out on that frame; the ring grows back at +9 frames, the
//   background cross-fades once the slide has ended and the art is decoded, and the hero fades in 4 frames later.
// - A held direction (input.js) runs the row from JS with every tile small, at the run speed, then settles on the
//   tile its momentum reaches with the same spring (ω = 45/s), and the final tile grows into the slot.
// - The hub moves the hero block up 156 px (spring ω = 32/s), then scrolls the cards up 600 px (ω = 50/s). Each card
//   is one picture with its text on a dark fade at the bottom (§1.5). The focused trailer card plays muted; A opens a
//   screenshot or the trailer full screen from its card (the viewer), and B shrinks it back onto its card.
// - A running game (row A5): its tile is marked and its label says so, Play reads "Resume" (back to the game), and
//   Options has Close Game. Guide over the game wakes the app dark (Home is black while a game runs); Home comes in
//   when it is entered, with the game focused. When the game ends, Home returns with it first: the row is sorted by
//   last played.
// - The Library tile shows the Library's preview below the row (design/ps5/2-library.md §1.3): its count line and first
//   rows, 116 px below their Library spots, fading in once the slide has ended and the covers are decoded. Home →
//   Library starts from there, so its covers are already decoded when the Library first draws them.

import { runDistance, runSpeed } from '../input/input.js';
import { warmControlCenter } from '../control-center/notices.js'; // Control Center's notifications and toasts, kept from boot
import { toast } from '../control-center/toast.js';
import { sorted, SORTS } from '../library/library.js'; // the preview shows the Library's first rows in its order

const BIG = 168, SMALL = 106, SCALE = SMALL / BIG;
const FOCUS_X = 172, PREV_X = 50, NEXT_X = 356, PITCH = 114;
const FRAME = 1000 / 60;
const SLIDE_MS = 11 * FRAME;                 // the row spring's length: background and hero wait for it
const OMEGA = 45 / 1000;                     // the row spring, per ms
const RING_DELAY = 9 * FRAME, RING_MS = 7 * FRAME;
const LABEL_MS = 6 * FRAME;
const BG_MS = 337, HERO_DELAY = 4 * FRAME, HERO_MS = 300;
const GROW_OUT_MS = 6 * FRAME;               // a run shrinks the tile it starts from over 6 frames
const PAGE = 7;                              // LT / RT: the small tiles on screen right of the focus
const HUB_LIFT = 156, CARDS_UP = 600;        // hub Down 1 and Down 2
const CARD = 502, CARD_PITCH = 536, CARDS_X1 = 172, CARDS_X2 = 56, CARDS_Y2 = 274;
// the viewer: a step slides the next picture in this far while it fades in over the one shown; a held direction steps
// once per step's length, so every picture is seen
const VIEW_SHIFT = 96, VIEW_HELD_MS = 200;
// music.m4a is the trailer's own audio from its start; trailer.mp4 skips the first 6 s (src-tauri library/trailer.rs)
const TRAILER_AUDIO_AT = 6;
const EASE_OUT = 'cubic-bezier(0, 0, .2, 1)';
const SINE = 'cubic-bezier(.37, 0, .63, 1)';
// return to Home: the row fades in with this measured curve (13, 35, 54, 69, 80, 88, 94 % every 2 frames)
const RETURN_ROW = 'linear(0, .13 10%, .35 20%, .54 30%, .69 40%, .8 50%, .88 60%, .94 70%, .98 85%, 1)';

// a critically damped spring as a CSS linear() easing over ms; the transitions start it one frame in (delay -1 frame,
// as the row's), so the press frame already moves
function spring(omegaPerSec, ms) {
  const pts = [];
  for (let i = 0; i <= 24; i++) {
    const t = ((i / 24) * ms) / 1000;
    const v = i === 24 ? 1 : 1 - (1 + omegaPerSec * t) * Math.exp(-omegaPerSec * t);
    pts.push(`${v.toFixed(4)} ${((i / 24) * 100).toFixed(2)}%`);
  }
  return `linear(${pts.join(', ')})`;
}
const HUB_SLOW = { ms: 290, ease: spring(32, 290) };  // Down 1 / Up 2: 99 % at ≈207 ms
const HUB_FAST = { ms: 190, ease: spring(50, 190) };  // Down 2 / Up 1: 99 % at ≈133 ms

const ICONS = {
  search: '<svg viewBox="0 0 30 30"><circle cx="12.5" cy="12.5" r="9.5" fill="none" stroke="currentColor" stroke-width="3.2"/><path d="M19.6 19.6 28 28" stroke="currentColor" stroke-width="3.6" stroke-linecap="round"/></svg>',
  settings: '<svg viewBox="0 0 32 32"><circle cx="16" cy="16" r="12.6" fill="none" stroke="currentColor" stroke-width="5.4" stroke-dasharray="4.95 4.95"/><path fill="currentColor" fill-rule="evenodd" d="M16 5.4a10.6 10.6 0 1 1 0 21.2 10.6 10.6 0 0 1 0-21.2Zm0 6.1a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9Z"/></svg>',
  power: '<svg viewBox="0 0 32 32"><path d="M10.2 8.6a10.4 10.4 0 1 0 11.6 0" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"/><path d="M16 4.5v11" stroke="currentColor" stroke-width="3" stroke-linecap="round"/></svg>',
  library: '<svg viewBox="0 0 64 56"><g fill="#fff"><rect width="13" height="13" rx="2"/><rect x="18" width="13" height="13" rx="2"/><rect x="36" width="13" height="13" rx="2"/><rect y="18" width="13" height="13" rx="2"/><rect y="36" width="13" height="13" rx="2"/><path d="M27 24h26a9 9 0 0 1 8.7 6.8l2 9.2a6 6 0 0 1-10.4 5.2L49 41H31l-4.3 4.2a6 6 0 0 1-10.4-5.2l2-9.2A9 9 0 0 1 27 24Zm2.5 6v3.5H26v3h3.5V40h3v-3.5H36v-3h-3.5V30Zm19 2a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm5 3.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z"/></g></svg>',
  play: '<svg viewBox="0 0 36 36"><circle cx="18" cy="18" r="18" fill="#fff"/><path d="M14.5 11.5v13l10-6.5Z" fill="#111"/></svg>',
  pause: '<svg viewBox="0 0 36 36"><circle cx="18" cy="18" r="18" fill="#fff"/><path d="M13 11h3.6v14H13Zm6.4 0H23v14h-3.6Z" fill="#111"/></svg>',
};
const TOPS = [['search', 'Search'], ['settings', 'Settings'], ['power', 'Power']];

let app, host, root;
const $ = {};                 // named elements
let tab = 'games';
let games = [], media = [];
let items = [];               // the row: {kind: 'game' | 'library' | 'media', id, name, tile, g}
let tiles = [];
let index = 0;
let zone = 'row';             // row | top | hub1 | hub2 | viewer | menu | info | busy
let topIndex = 0, heroIndex = 0, cardIndex = 0, menuIndex = 0;
let menuFrom = 'row', menuItems = [], cards = [], hubGame = null;
let running = null;           // the running game's id ('game' events)
let dark = false;             // blacked out since a launch: comes in (reveal) when Home is next entered
let onTop = false;
// motion state
let moveFrom = 0, moveT = -1e9;
let run = null;
let ringAnim, heroAnim, bgAnim, labelAnims = [], lateTimer, launchTimer, launchAnims = [];
let front = 0, labelOn = 0, bgToken = 0;
const pending = new Set();    // animations a reset must cancel

export default {
  async mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="bgwrap"><div class="bg"><div class="layer"></div><div class="layer"></div></div><div class="scrim"></div><div class="dim"></div></div>
      <div class="navy"></div>
      <div class="ui">
        <div class="top">
          <span class="tab sel" data-nav="tab:games">Games</span><span class="tab" data-nav="tab:media">Media</span>
          ${TOPS.map(([id, name]) => `<div class="icon ${id}" data-nav="top:${id}"><i></i>${ICONS[id]}<span class="tip">${name}</span></div>`).join('')}
          <span class="clock"></span>
        </div>
        <div class="rowwrap">
          <div class="row"></div>
          <div class="ring"></div>
          <div class="label"><span></span><span></span></div>
        </div>
        <div class="preview"><div class="pcount"><span></span><span></span></div><div class="pgrid"></div></div>
        <div class="hubhead"><div class="icon"></div><div class="title"></div></div>
        <div class="content">
          <div class="herowrap"><div class="hero">
            <div class="hero-text"><div class="logo"></div><div class="tagline"></div></div>
            <div class="play" data-nav="hero:play"><span>Play</span></div>
            <div class="more" data-nav="hero:more"><i></i><i></i><i></i></div>
            <div class="box"><div class="cover"></div><div class="chip"></div><div class="last"></div></div>
          </div></div>
          <div class="cards"><div class="section"></div><div class="strip"></div></div>
        </div>
        <div class="menu"></div>
        <div class="info"></div>
        <div class="viewer" data-nav="viewer"><div class="vdim"></div><div class="vpic"><img alt=""></div><div class="vpic"><img alt=""></div>
          <div class="vpic"><img alt=""></div><div class="vvid"></div><div class="vhead"><span class="vname"></span><span class="vcount"></span></div></div>
      </div>
      <audio class="music"></audio>`;
    for (const k of ['bgwrap', 'dim', 'navy', 'ui', 'top', 'rowwrap', 'row', 'ring', 'hubhead', 'content', 'herowrap',
      'hero', 'play', 'more', 'box', 'cards', 'section', 'strip', 'menu', 'info', 'music', 'preview', 'pgrid',
      'viewer', 'vdim', 'vvid', 'vhead', 'vname', 'vcount']) $[k] = el.querySelector(`.${k}`);
    slots.push(...[...el.querySelectorAll('.vpic')].map(s => ({ el: s, img: s.firstElementChild, k: -1, ready: null })));
    [$.pcount, $.psort] = el.querySelectorAll('.pcount span');
    $.labels = [...el.querySelectorAll('.label span')];
    $.layers = [...el.querySelectorAll('.bg .layer')];
    $.icons = TOPS.map(([id]) => el.querySelector(`.top .icon.${id}`));
    $.tabs = [...el.querySelectorAll('.top .tab')];
    clock(el.querySelector('.clock'));

    host.on('library', list => setGames(list));
    host.on('game', gameEvent);
    host.on('media', list => { media = list ?? []; if (tab === 'media') setItems(); });
    document.addEventListener('visibilitychange', () => {
      if (document.hidden && zone === 'busy') blackout();
      if (document.hidden) stopVideo(); // the app hides while a game runs: no video may keep running
    });
    host.media.list().then(list => { media = list ?? []; if (tab === 'media') setItems(); }).catch(() => {});
    // read-only state for tests (tools/app/ui-probe.mjs eval steps)
    window.__home = { state: () => ({ zone, tab, index, item: items[index]?.id, card: cardIndex, menu: menuItems.map(m => m[0]), running, run: !!run, dark, play: $.play.textContent.trim(),
      view: view && { kind: view.kind, shot: view.shot, closing: view.closing }, videos: [...root.querySelectorAll('video')].map(v => ({ src: !!v.getAttribute('src'), playing: !v.paused, inViewer: v.parentNode === $.vvid })),
      music: !$.music.paused }) };
    setGames(await host.library.list());
  },

  enter({ focus, back } = {}) {
    onTop = true;
    const i = items.findIndex(it => it.id === focus);
    if (i >= 0 && i !== index) { index = i; showNow(); }
    // woken over a running game (Guide hold, or Home from Control Center over it): Home comes in from black. Not when
    // Control Center closes back to the game (the app sleeps).
    if (dark && !(back && running)) { resetZones(); showNow(); reveal(); }
    else if (zone === 'hub2') trailerSoon();
  },

  // another screen on top (Control Center, Search) or a game: the viewer closes and the trailer stops
  leave() {
    onTop = false;
    if (run) { stopRun(); showNow(); }
    stopVideo();
  },

  action(name, e = {}) {
    if (zone === 'busy') return false;
    if (zone === 'viewer') return viewerAction(name, e);
    if (name === 'y') { app.push('search'); return true; }
    if (zone === 'menu') return menuAction(name, e);
    if (zone === 'info') return name === 'b' || name === 'a' || name === 'click' ? closeInfo() : false;
    if (name === 'point' || name === 'click') return pointer(name, e);
    if (name === 'release') return run && !run.settle ? (release(), true) : false;
    if (name === 'lb' || name === 'rb') return (zone === 'row' || zone === 'top') && switchTab(name === 'lb' ? 'games' : 'media');
    if (zone === 'row') return rowAction(name, e);
    if (zone === 'top') return topAction(name, e);
    if (zone === 'hub1') return hub1Action(name, e);
    if (zone === 'hub2') return hub2Action(name, e);
    return false;
  },
};

// ---------- data ----------
function setGames(list) {
  games = list ?? [];
  if (hubGame) hubGame = games.find(g => g.id === hubGame.id) ?? hubGame;
  if (tab === 'games') setItems();
}

const tabFocus = {}; // each tab keeps its focus
function setItems(focused = items[index]?.id) {
  // the Library's "Recently played" order, so a game that just ended leads the row
  const played = sorted(games, 'played');
  items = tab === 'games'
    ? [...played.map(g => ({ kind: 'game', id: g.id, name: g.name, tile: g.art?.tile, g })),
      { kind: 'library', id: '#library', name: 'Game Library' }]
    : media.map(m => ({ kind: 'media', id: m.id, name: m.name, tile: m.art?.tile, g: m }));
  index = Math.max(0, items.findIndex(it => it.id === focused));
  stopRun();
  tiles = items.map(tileEl);
  $.row.replaceChildren(...tiles);
  showNow();
}

function tileEl(it, i) {
  const t = document.createElement('div');
  t.className = 'tile';
  t.dataset.nav = `tile:${i}`;
  if (it.kind === 'library') {
    t.classList.add('lib');
    t.innerHTML = ICONS.library;
  } else if (it.tile) {
    const src = host.fileUrl(it.tile);
    t.style.backgroundImage = `url("${src}")`;
    ifBroken(src, () => placeholder(t, it.id, it.name));
  } else placeholder(t, it.id, it.name);
  t.classList.toggle('running', it.kind === 'game' && it.id === running);
  return t;
}

// the running game: its tile's mark, its label and Play's "Resume"
function markRunning() {
  items.forEach((it, i) => tiles[i]?.classList.toggle('running', it.kind === 'game' && it.id === running));
  setLabel($.labels[labelOn], items[index]);
  playLabel(items[index]);
}

function setLabel(el, it) {
  el.textContent = it?.name ?? '';
  if (it?.kind === 'game' && it.id === running) {
    el.append(Object.assign(document.createElement('em'), { textContent: ' │ Running' }));
  }
}

function playLabel(it) {
  $.play.querySelector('span').textContent = it?.kind === 'media' ? 'Open' : it?.kind === 'game' && it.id === running ? 'Resume' : 'Play';
}

// a stable colour per entry when it has no art
function tint(id, light, dark, angle) {
  let h = 0;
  for (const c of id) h = (h * 31 + c.codePointAt(0)) % 360;
  return `linear-gradient(${angle}deg, hsl(${h} 45% ${light}%), hsl(${(h + 25) % 360} 50% ${dark}%))`;
}

// the tinted placeholder with the name, for an entry without art or whose art file is missing
function placeholder(el, id, name) {
  el.style.background = tint(id, 34, 16, 145);
  el.replaceChildren(Object.assign(document.createElement('span'), { textContent: name }));
}

// calls `fallback` if `src` doesn't load (a deleted file, a data dir that moved): CSS backgrounds have no onerror
function ifBroken(src, fallback) {
  const probe = new Image();
  probe.onerror = fallback;
  probe.src = src;
}

const focusedGame = () => (items[index]?.kind === 'game' ? items[index].g : null);

// ---------- the row ----------
// where tile k sits with the row at position p (the focus, fractional while moving). grow 1 is the rest layout
// (focus 168 px at 172, previous at 50, next at 356, then the 114 px pitch); grow 0 keeps every tile small on one
// even pitch, as during a run.
function slot(d, grow) {
  const rest = d >= 1 ? NEXT_X + (d - 1) * PITCH : d >= 0 ? FOCUS_X + d * (NEXT_X - FOCUS_X)
    : d >= -1 ? FOCUS_X + d * (FOCUS_X - PREV_X) : PREV_X + (d + 1) * PITCH;
  const even = FOCUS_X + d * PITCH;
  return [even + (rest - even) * grow, SCALE + (1 - SCALE) * Math.max(0, 1 - Math.abs(d)) * grow];
}

function place(p = index, grow = 1) {
  for (let k = 0; k < tiles.length; k++) {
    const [x, s] = slot(k - p, grow);
    tiles[k].style.transform = `translateX(${+x.toFixed(2)}px) scale(${+s.toFixed(4)})`;
  }
}

// the row's position now, while a tap's spring is still running
function rowPos(now = performance.now()) {
  if (run) return run.p;
  const t = (now - moveT + FRAME) / 1000 * 45;
  const s = t > 12 ? 1 : 1 - (1 + t) * Math.exp(-t);
  return moveFrom + (index - moveFrom) * s;
}

const clampIndex = i => Math.max(0, Math.min(items.length - 1, i));

// the settled state at once, with no motion: first paint, a jump, a new list
function showNow() {
  stopRun();
  for (const a of [ringAnim, heroAnim, bgAnim, ...labelAnims, ...pending]) a?.cancel();
  pending.clear();
  clearTimeout(lateTimer);
  root.classList.add('instant');
  place();
  root.offsetWidth; // apply the jump before motion comes back
  root.classList.remove('instant');
  moveT = -1e9;
  const it = items[index];
  setLabel($.labels[labelOn], it);
  $.labels[1 - labelOn].textContent = '';
  $.labels[labelOn].style.opacity = 1;
  $.labels[1 - labelOn].style.opacity = 0;
  $.ring.style.opacity = it ? 1 : 0;
  fillHero(it);
  $.hero.style.opacity = it ? 1 : 0;
  bgToken++;
  paintLayer($.layers[front], it);
  $.layers[front].style.opacity = 1;
  $.layers[front].style.zIndex = 1;
  $.layers[1 - front].style.opacity = 0;
  $.layers[1 - front].style.zIndex = 0;
  if (it?.kind === 'library') showPreview(false);
  else hidePreview();
  prepareSoon(it);
}

function move(to) {
  const next = clampIndex(to);
  if (next === index || !items.length) return false;
  if (run) stopRun(true);
  moveFrom = rowPos();
  moveT = performance.now();
  index = next;
  pressFrame();
  place();
  swapLabel(items[index]);
  afterMove(moveT);
  return true;
}

// the press frame: ring and hero (with the box art) cut out
function pressFrame() {
  clearTimeout(prepTimer);
  ringAnim?.cancel();
  $.ring.style.opacity = 0;
  heroAnim?.cancel();
  $.hero.style.opacity = 0;
  hidePreview(); // the PS5 removes it at once (§2 "focus leaves the Library tile")
  clearTimeout(lateTimer);
}

// after a slide that started at t0: the ring grows out from the art edge, then (slide over, art decoded) the
// background cross-fades and the hero fades in
function afterMove(t0) {
  const it = items[index];
  const since = performance.now() - t0;
  ringAnim = $.ring.animate(
    [{ opacity: 0, transform: `scale(${(BIG + 6) / 180})` }, { opacity: 1, transform: 'scale(1)' }],
    { duration: RING_MS, delay: RING_DELAY - since, easing: EASE_OUT, fill: 'both' },
  );
  ringAnim.onfinish = () => { $.ring.style.opacity = 1; ringAnim.cancel(); };
  lateTimer = setTimeout(async () => {
    const token = ++bgToken;
    fillHero(it);
    await artReady(it);
    if (token !== bgToken) return;
    crossFade(it);
    if (it?.kind === 'library') showPreview(true);
    heroAnim = $.hero.animate([{ opacity: 0 }, { opacity: 1 }], { duration: HERO_MS, delay: HERO_DELAY, easing: EASE_OUT, fill: 'both' });
    heroAnim.onfinish = () => {
      $.hero.style.opacity = 1;
      heroAnim.cancel();
      prepareSoon(it);
    };
  }, Math.max(0, SLIDE_MS - since));
}

// an element's opacity now, from its running fade or its style: no style recalculation in the key handler
function opacityNow(el, anim) {
  if (anim?.effect?.target === el && anim.playState === 'running') {
    const [a, b] = anim.effect.getKeyframes().map(k => +k.opacity);
    return a + (b - a) * (anim.effect.getComputedTiming().progress ?? 0);
  }
  return el.style.opacity === '' ? 1 : +el.style.opacity;
}

function swapLabel(it) {
  const old = $.labels[labelOn];
  const from = opacityNow(old, labelAnims.find(a => a.effect.target === old));
  labelAnims.forEach(a => a.cancel());
  labelOn = 1 - labelOn;
  const now = $.labels[labelOn];
  setLabel(now, it);
  old.style.opacity = 0;
  now.style.opacity = 1;
  const timing = { duration: LABEL_MS, delay: -FRAME, easing: 'linear' };
  labelAnims = [old.animate([{ opacity: from }, { opacity: 0 }], timing), now.animate([{ opacity: 0 }, { opacity: 1 }], timing)];
}

// ---------- the held run ----------
function startRun(dir, runStart) {
  const p = rowPos();
  run = { dir, t0: runStart, p, p0: p, v: 0, grow: 1, settle: null, raf: 0 };
  pressFrame();
  labelAnims.forEach(a => a.cancel());
  for (const l of $.labels) l.style.opacity = 0;
  root.classList.add('running');
  run.raf = requestAnimationFrame(runFrame);
}

function runFrame() {
  const r = run;
  if (!r) return;
  const now = performance.now();
  if (!r.settle) {
    const t = Math.max(0, now - r.t0);
    const v = runSpeed(t) / PITCH;
    let p = r.p0 + r.dir * runDistance(t) / PITCH;
    r.grow = Math.max(0, 1 - t / GROW_OUT_MS);
    r.v = r.dir * v;
    // near the end of the row: stop where the momentum would carry it, without passing the last tile
    const end = r.dir > 0 ? items.length - 1 : 0;
    if ((end - p) * r.dir <= v / OMEGA) {
      p = Math.min(Math.max(p, Math.min(end, r.p0)), Math.max(end, r.p0));
      r.p = p;
      settle(end);
    } else {
      r.p = p;
      index = r.dir > 0 ? Math.ceil(p - 1e-6) : Math.floor(p + 1e-6);
      place(p, r.grow);
    }
  }
  if (r.settle) {
    const s = r.settle;
    const t = now - s.t0;
    const e = (s.A + (OMEGA * s.A - s.v0) * t) * Math.exp(-OMEGA * t);
    const k = OMEGA * t;
    r.p = s.target - e;
    r.grow = s.grow0 + (1 - s.grow0) * (1 - (1 + k) * Math.exp(-k));
    if (t >= SLIDE_MS && Math.abs(e) < 0.002) { stopRun(); return; }
    place(r.p, r.grow);
  }
  r.raf = requestAnimationFrame(runFrame);
}

// release: settle on the first tile at or past where the momentum stops (critically damped, so no overshoot)
function release() {
  const r = run;
  const coast = r.p + r.v / OMEGA;
  settle(clampIndex(r.dir > 0 ? Math.ceil(coast - 1e-6) : Math.floor(coast + 1e-6)));
}

function settle(target) {
  const r = run;
  r.settle = { t0: performance.now(), target, A: target - r.p, v0: r.v, grow0: r.grow };
  index = target;
  const now = $.labels[labelOn];
  setLabel(now, items[index]);
  labelAnims = [now.animate([{ opacity: 0 }, { opacity: 1 }], { duration: LABEL_MS, delay: 3 * FRAME, easing: 'linear', fill: 'backwards' })];
  now.style.opacity = 1;
  $.labels[1 - labelOn].style.opacity = 0;
  afterMove(r.settle.t0);
}

// ends a run: at rest (settled), or handing the tiles to the CSS spring from where they are (a tap mid-settle)
function stopRun(handOver = false) {
  if (!run) return;
  cancelAnimationFrame(run.raf);
  const r = run;
  run = null;
  if (handOver) {
    moveFrom = r.p;
    root.classList.remove('running');
    return;
  }
  root.classList.add('instant');
  place();
  root.offsetWidth;
  root.classList.remove('instant', 'running');
}

// ---------- zones ----------
function rowAction(name, e) {
  if (name === 'left' || name === 'right') {
    const d = name === 'left' ? -1 : 1;
    if (!e.held) return move(index + d);
    if (e.step === 1) {
      if (clampIndex(index + d) === index) return false;
      startRun(d, e.runStart);
      return true;
    }
    return !!run && !run.settle;
  }
  if (e.held) return false;
  if (name === 'lt' || name === 'rt') return move(index + (name === 'lt' ? -PAGE : PAGE));
  if (name === 'wheel') return move(index + (e.dy || e.dx));
  if (name === 'up') return focusTop(0);
  if (name === 'down') return focusedGame() ? enterHub1() : false;
  if (name === 'a') return activate();
  if (name === 'menu') return openMenu('row');
  return false;
}

function activate() {
  const it = items[index];
  if (!it) return false;
  if (it.kind === 'library') { holdPreview(); app.go('library'); return true; }
  if (it.kind === 'media') return host.media.open ? (host.media.open(it.id).catch(e => console.warn('media', e)), true) : false;
  return launch(it.g);
}

function focusTop(i) {
  zone = 'top';
  topIndex = i;
  $.icons.forEach((el, k) => el.classList.toggle('focus', k === i));
  return true;
}

function topAction(name, e) {
  if (name === 'left' || name === 'right') {
    const i = Math.max(0, Math.min(TOPS.length - 1, topIndex + (name === 'left' ? -1 : 1)));
    return i !== topIndex && focusTop(i);
  }
  if (e.held) return false;
  if (name === 'down' || name === 'b') { $.icons.forEach(el => el.classList.remove('focus')); zone = 'row'; return true; }
  if (name === 'a') return topActivate(TOPS[topIndex][0]);
  return false;
}

function topActivate(id) {
  if (id === 'search') app.push('search');
  else if (id === 'settings') app.push('settings');
  else app.push('control-center', { power: true });
  return true;
}

function switchTab(t) {
  if (t === tab) return false;
  tabFocus[tab] = items[index]?.id;
  tab = t;
  $.tabs.forEach(el => el.classList.toggle('sel', el.dataset.nav === `tab:${t}`));
  zone = 'row';
  $.icons.forEach(el => el.classList.remove('focus'));
  setItems(tabFocus[t]);
  for (const el of [$.rowwrap, $.herowrap]) track(el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 150, easing: EASE_OUT }));
  return true;
}

// ---------- hero and background ----------
function fillHero(it) {
  const g = it?.kind === 'game' ? it.g : null;
  const logo = $.hero.querySelector('.logo');
  logo.replaceChildren();
  const logoArt = g?.art?.logo;
  if (logoArt) {
    const src = host.fileUrl(logoArt);
    const img = Object.assign(document.createElement('img'), { crossOrigin: 'anonymous', src, alt: '' });
    // a missing logo file: the name as a wordmark, never a broken image
    img.onerror = () => {
      if (img.parentNode !== logo) return;
      logo.textContent = it.name;
      logo.classList.add('wordmark');
    };
    const box = logoBoxes.get(src);
    if (box) img.style.objectViewBox = box;
    else logoBox(src).then(b => { if (b) img.style.objectViewBox = b; });
    logo.append(img);
  }
  else logo.textContent = it && it.kind !== 'library' ? it.name : '';
  logo.classList.toggle('wordmark', !logoArt);
  $.hero.querySelector('.tagline').textContent = g?.tagline ?? (it?.kind === 'media' ? it.g?.tagline ?? '' : '');
  $.hero.classList.toggle('none', !it || it.kind === 'library');
  $.hero.classList.toggle('media', it?.kind === 'media');
  playLabel(it);
  // box art: the cover, with playtime and last played where the PS5 has its chip and price
  $.box.hidden = !g;
  if (g) {
    const cover = $.box.querySelector('.cover');
    cover.replaceChildren();
    cover.style.background = '';
    const art = g.art?.cover ?? g.art?.tile;
    if (art) {
      const src = host.fileUrl(art);
      cover.style.backgroundImage = `url("${src}")`;
      ifBroken(src, () => { if (cover.style.backgroundImage.includes(src)) placeholder(cover, g.id, g.name); });
    } else placeholder(cover, g.id, g.name);
    $.box.querySelector('.chip').textContent = g.playtimeSec ? playtime(g.playtimeSec) : '';
    $.box.querySelector('.chip').hidden = !g.playtimeSec;
    $.box.querySelector('.last').textContent = g.lastPlayed ? `Last played ${ago(g.lastPlayed)}` : 'Not played yet';
  }
}

function playtime(sec = 0) {
  if (!sec) return 'New';
  const h = Math.floor(sec / 3600), m = Math.round((sec % 3600) / 60);
  return h ? `${h} h ${m ? `${m} min ` : ''}played` : `${Math.max(1, m)} min played`;
}

function ago(t) {
  const ms = t < 1e12 ? t * 1000 : t;
  const days = Math.floor((startOfDay(Date.now()) - startOfDay(ms)) / 864e5);
  if (days <= 0) return 'today';
  if (days === 1) return 'yesterday';
  if (days < 7) return `${days} days ago`;
  return new Date(ms).toLocaleDateString([], { day: 'numeric', month: 'short', year: 'numeric' });
}
const startOfDay = ms => { const d = new Date(ms); d.setHours(0, 0, 0, 0); return d.getTime(); };

function bgSource(it) {
  if ((it?.kind === 'game' || it?.kind === 'media') && it.g?.art?.background) return host.fileUrl(it.g.art.background);
  return null;
}

// resolves once the item's background art and logo are decoded (at once for tinted placeholders)
const decoded = new Map();
function decode(src, cors = false) {
  if (!decoded.has(src)) {
    const img = new Image();
    if (cors) img.crossOrigin = 'anonymous'; // logos: readable on a canvas (the asset protocol allows our origin)
    img.src = src;
    decoded.set(src, img.decode().catch(() => {}).then(() => img));
    if (decoded.size > 24) decoded.delete(decoded.keys().next().value);
  }
  return decoded.get(src);
}
function artReady(it) {
  if (it?.kind === 'library') return previewReady();
  const logo = it?.kind === 'game' && it.g.art?.logo ? host.fileUrl(it.g.art.logo) : null;
  return Promise.all([bgSource(it) && decode(bgSource(it)), logo && logoBox(logo)]);
}

// Logos often come with wide transparent margins. Their ink box, found once on a small copy, crops them
// (object-view-box), so the logo's left edge sits at x 172 and its bottom above the tagline.
const logoBoxes = new Map();
function logoBox(src) {
  if (logoBoxes.has(src)) return Promise.resolve(logoBoxes.get(src));
  return decode(src, true).then(img => {
    const w = img.naturalWidth, h = img.naturalHeight;
    if (!w || !h) return null;
    const k = Math.min(1, 256 / w);
    const c = Object.assign(document.createElement('canvas'), { width: Math.ceil(w * k), height: Math.ceil(h * k) });
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(img, 0, 0, c.width, c.height);
    let a;
    try { a = g.getImageData(0, 0, c.width, c.height).data; } catch { return null; }
    let x0 = c.width, y0 = c.height, x1 = -1, y1 = -1;
    for (let y = 0; y < c.height; y++) for (let x = 0; x < c.width; x++) {
      if (a[(y * c.width + x) * 4 + 3] > 24) { if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y; }
    }
    const box = x1 < 0 ? null : `inset(${Math.max(0, Math.floor(y0 / k))}px ${Math.max(0, Math.floor(w - (x1 + 1) / k))}px ${Math.max(0, Math.floor(h - (y1 + 1) / k))}px ${Math.max(0, Math.floor(x0 / k))}px)`;
    logoBoxes.set(src, box);
    return box;
  });
}

function paintLayer(layer, it) {
  const src = bgSource(it);
  layer.replaceChildren();
  const tinted = () => {
    layer.style.background = it && it.kind !== 'library'
      ? tint(it.id, 15, 3, 110).replace('linear-gradient(110deg,', 'radial-gradient(ellipse at 78% 40%,') : '#000';
  };
  if (src) {
    layer.style.background = '#000';
    const img = Object.assign(document.createElement('img'), { src, alt: '', decoding: 'sync' });
    img.onerror = () => { if (img.parentNode === layer) { img.remove(); tinted(); } };
    layer.append(img);
  } else tinted();
}

function crossFade(it) {
  if (bgAnim?.playState === 'running') bgAnim.finish();
  $.layers[1 - front].style.opacity = 0; // under the current one, so hiding it shows nothing
  front = 1 - front;
  const top = $.layers[front];
  top.style.zIndex = 1;
  $.layers[1 - front].style.zIndex = 0;
  paintLayer(top, it);
  top.style.opacity = 1;
  bgAnim = top.animate([{ opacity: 0 }, { opacity: 1 }], { duration: BG_MS, easing: SINE, fill: 'backwards' });
}

// ---------- the Library preview (design/ps5/2-library.md §1.3) ----------
// The Library's "All: N", "Sort by: …" and its first three rows of 296 px covers (the third below the screen), each 116
// px below its Library spot, in the Library's sort. Same art, size and fit as the Library's covers, so the decoded
// images are the ones the Library draws: its first opening used to drop 3–4 frames decoding 12 covers at once (A8).
const PREVIEW = { cols: 5, rows: 3, pitch: 320, x: 172, y: 454, dy: 116 };
const PREVIEW_IN = 10 * FRAME, PREVIEW_RISE = 9;   // §2 "library preview appears": 10 frames, rising ≈8–10 px
// library.js's Home → Library morph (measured per frame, 14 frames, started one frame in)
const MORPH = 'linear(0, .092, .283, .467, .625, .739, .826, .886, .924, .951, .967, .978, .989, .995, 1)';
let previewKey = '', previewDecoded = Promise.resolve(), previewAnim = null, holdTimer = 0;

const coverArt = g => g.art?.tile ?? g.art?.cover;

function buildPreview() {
  let sort = 'played';
  try { sort = localStorage.getItem('library.sort') || sort; } catch {}
  if (!SORTS.some(s => s.id === sort)) sort = 'played';
  const list = sorted(games, sort).slice(0, PREVIEW.cols * PREVIEW.rows);
  const key = `${sort}|${games.length}|${list.map(g => `${g.id}:${coverArt(g) ?? ''}`).join('|')}`;
  if (key === previewKey) return;
  previewKey = key;
  $.pcount.textContent = `All: ${games.length}`;
  $.psort.textContent = `Sort by: ${SORTS.find(s => s.id === sort).label}`;
  const imgs = [];
  $.pgrid.replaceChildren(...list.map((g, i) => {
    const el = document.createElement('div');
    el.className = 'pcover';
    el.style.transform = `translate(${PREVIEW.x + (i % PREVIEW.cols) * PREVIEW.pitch}px, ${PREVIEW.y + Math.floor(i / PREVIEW.cols) * PREVIEW.pitch}px)`;
    const art = coverArt(g);
    if (art) {
      const img = Object.assign(new Image(), { alt: '', decoding: 'async', src: host.fileUrl(art) });
      img.onerror = () => { img.remove(); placeholder(el, g.id, g.name); };
      if (!g.art?.tile) img.classList.add('portrait');
      imgs.push(img);
      el.append(img);
    } else {
      el.style.background = tint(g.id, 34, 16, 145);
      el.append(Object.assign(document.createElement('span'), { textContent: g.name }));
    }
    return el;
  }));
  previewDecoded = Promise.all(imgs.map(img => img.decode().catch(() => {})));
}

// resolves once the preview's covers are decoded (they are built now if the list or sort changed)
function previewReady() {
  buildPreview();
  return previewDecoded;
}

function showPreview(animate) {
  buildPreview();
  previewAnim?.cancel();
  $.preview.style.opacity = 1;
  if (animate) {
    previewAnim = $.preview.animate([{ opacity: 0, transform: `translateY(${PREVIEW_RISE}px)` }, { opacity: 1, transform: 'none' }],
      { duration: PREVIEW_IN, easing: EASE_OUT, fill: 'backwards' });
  }
}

function hidePreview() {
  previewAnim?.cancel();
  $.preview.style.opacity = 0;
}

// Home → Library: the Library draws its own grid rising from here while it fades in (library.js); the preview stays on
// screen under it (Home itself is hidden) and rises with it on the same curve, so the covers never dim. The Library's
// background is opaque after 300 ms, then the preview goes back to Home.
function holdPreview() {
  clearTimeout(holdTimer);
  if (+$.preview.style.opacity !== 1) return;
  previewAnim?.cancel();
  $.preview.style.visibility = 'visible';
  previewAnim = $.preview.animate([{ transform: 'none' }, { transform: `translateY(${-PREVIEW.dy}px)` }],
    { duration: 14 * FRAME, delay: -FRAME, easing: MORPH, fill: 'forwards' });
  holdTimer = setTimeout(() => { previewAnim?.cancel(); $.preview.style.visibility = ''; }, 320);
}

// ---------- hub ----------
function setStyle(el, prop, value, ms, ease = EASE_OUT, delay = 0) {
  el.style.transition = ms ? `${prop === 'transform' ? 'transform' : prop} ${ms}ms ${ease} ${delay}ms` : 'none';
  el.style[prop] = value;
}

function enterHub1() {
  const g = focusedGame();
  if (!g) return false;
  const fromRow = zone === 'row';
  zone = 'hub1';
  heroIndex = 0;
  if (fromRow) {
    prepareHub(g);
    // the hero may still be on its way in after a move: bring it now
    clearTimeout(lateTimer);
    const shown = opacityNow($.hero, heroAnim);
    if (shown < 1) {
      bgToken++;
      fillHero(items[index]);
      heroAnim?.cancel();
      crossFade(items[index]);
      heroAnim = $.hero.animate([{ opacity: shown }, { opacity: 1 }], { duration: 6 * FRAME, easing: EASE_OUT });
      $.hero.style.opacity = 1;
    }
    setStyle($.content, 'transform', `translateY(${-HUB_LIFT}px)`, HUB_SLOW.ms, HUB_SLOW.ease, -FRAME);
    for (const el of [$.top, $.rowwrap]) setStyle(el, 'opacity', 0, 8 * FRAME, EASE_OUT, 0);
    for (const el of [$.hubhead, $.cards]) setStyle(el, 'opacity', 1, 8 * FRAME, EASE_OUT, 0);
  } else {
    // Up 1: back from the cards
    releaseTrailer();
    setStyle($.content, 'transform', `translateY(${-HUB_LIFT}px)`, HUB_FAST.ms, HUB_FAST.ease, -FRAME);
    setStyle($.strip, 'transform', `translateX(${CARDS_X1}px)`, HUB_FAST.ms, HUB_FAST.ease, -FRAME);
    setStyle($.herowrap, 'opacity', 1, 6 * FRAME, EASE_OUT, 0);
    setStyle($.dim, 'opacity', 0, HUB_FAST.ms, EASE_OUT, 0);
    cardFocus(-1);
    sectionFor(0);
  }
  heroFocus(0);
  return true;
}

function leaveHub() {
  zone = 'row';
  heroFocus(-1);
  cardFocus(-1);
  releaseTrailer();
  releaseShots();
  stopMusic();
  setStyle($.content, 'transform', 'translateY(0px)', HUB_SLOW.ms, HUB_SLOW.ease, -FRAME);
  for (const el of [$.top, $.rowwrap]) setStyle(el, 'opacity', 1, 12 * FRAME, EASE_OUT, 0);
  for (const el of [$.hubhead, $.cards]) setStyle(el, 'opacity', 0.001, 8 * FRAME, EASE_OUT, 0);
  return true;
}

function enterHub2() {
  if (!cards.length) return false;
  zone = 'hub2';
  heroFocus(-1);
  setStyle($.content, 'transform', `translateY(${-HUB_LIFT - CARDS_UP}px)`, HUB_FAST.ms, HUB_FAST.ease, -FRAME);
  setStyle($.herowrap, 'opacity', 0, 3 * FRAME, EASE_OUT, 0);
  setStyle($.dim, 'opacity', 0.65, HUB_FAST.ms, EASE_OUT, 0);
  cardFocus(0);
  return true;
}

function heroFocus(i) {
  heroIndex = i;
  $.play.classList.toggle('focus', i === 0);
  $.more.classList.toggle('focus', i === 1);
}

function hub1Action(name, e) {
  if (name === 'left' || name === 'right') {
    const i = name === 'left' ? 0 : 1;
    if (i === heroIndex) return false;
    heroFocus(i);
    return true;
  }
  if (e.held) return false;
  if (name === 'up' || name === 'b') return leaveHub();
  if (name === 'down') return enterHub2();
  if (name === 'a') return heroIndex === 0 ? launch(hubGame) : openMenu('hub1');
  if (name === 'menu') return openMenu('hub1');
  if (name === 'rt' || name === 'lt' || name === 'wheel') return false;
  return false;
}

function hub2Action(name, e) {
  // clamped here: cardFocus(-1) means "no focus" (leaving the cards)
  const to = d => cardFocus(Math.max(0, Math.min(cards.length - 1, cardIndex + d)));
  if (name === 'left' || name === 'right') return to(name === 'left' ? -1 : 1);
  if (name === 'lt' || name === 'rt') return to(name === 'lt' ? -3 : 3);
  if (name === 'wheel') return to(e.dy || e.dx);
  if (e.held) return false;
  if (name === 'up' || name === 'b') return enterHub1();
  if (name === 'a') return cardAct(cardIndex);
  if (name === 'menu') return openMenu('hub2');
  return false;
}

// the hub's header and cards for a game, built once the focus has rested 1 s (Home is idle then), so Down only starts
// transitions and a quick run of presses never pays for painting cards
let preparedFor = null, prepTimer;
function prepareSoon(it) {
  clearTimeout(prepTimer);
  if (it?.kind === 'game') prepTimer = setTimeout(() => { if (zone === 'row' && items[index] === it) prepareHub(it.g); }, 1000);
}

function prepareHub(g) {
  hubGame = g;
  if (preparedFor === g) return;
  preparedFor = g;
  $.hubhead.querySelector('.title').textContent = g.name;
  $.hubhead.querySelector('.icon').style.background = g.art?.tile ? `center / cover url("${host.fileUrl(g.art.tile)}")` : tint(g.id, 34, 16, 145);
  buildCards(g);
}

// Cards: trailer, screenshots, Official news, About, Soundtrack (whatever the game has). Each is one 502 px picture,
// cropped to fill it, with its text on a dark fade that is part of the picture (§1.5); screenshots are the picture
// alone. The label above the strip names the focused card's section, so the cards need no filler lines.
function buildCards(g) {
  const url = p => host.fileUrl(p);
  const art = g.art?.background ? url(g.art.background) : null;
  const shots = g.screenshots ?? [];
  // news has no pictures of its own: the key art, cropped at a different spot for each card, so they read as a set
  // and don't repeat the screenshots before them
  const crops = ['0%', '100%', '30%', '70%', '15%', '85%'];
  releaseTrailer();
  releaseShots();
  cards = [];
  if (g.trailer) {
    cards.push({ kind: 'trailer', section: 'Trailer', image: art ?? (shots[0] ? url(shots[0]) : null), title: 'Official trailer',
      sub: trailerLength.get(g.trailer) ?? '', glyph: 'play' });
  }
  shots.forEach((s, k) => cards.push({ kind: 'shot', section: 'Screenshots', image: url(s), shot: k }));
  (g.news ?? []).forEach((n, k) => cards.push({
    kind: 'news', section: 'Official news', title: n.title ?? 'News', sub: n.date ? capital(ago(n.date)) : (n.feedlabel ?? ''),
    image: n.image ? url(n.image) : art ?? (shots.length ? url(shots[shots.length - 1 - (k % shots.length)]) : null),
    focus: n.image || !art ? '' : `${crops[k % crops.length]} 40%`,
  }));
  if (g.description) cards.push({ kind: 'about', section: 'About', text: g.description });
  if (g.music) cards.push({ kind: 'music', section: 'Soundtrack', image: g.art?.tile ? url(g.art.tile) : art, title: 'Music from the trailer', sub: '', glyph: 'play' });
  for (const el of cardEls.values()) el.remove();
  cardEls.clear();
  cardIndex = 0;
  sectionFor(0);
  setStyle($.strip, 'transform', `translateX(${CARDS_X1}px)`, 0);
  showCards();
}

// Only the cards on screen and one either side are in the page (plus the ones leaving during a move): the strip is a
// moving layer, and Chromium paints a moving layer whole. At the end of the strip the focus is right of x 56, so the
// range follows the strip, not the focus.
const cardEls = new Map();
let pruneTimer;
function cardRange() {
  const x = stripX(cardIndex);
  return [Math.max(0, Math.floor((-x - CARD) / CARD_PITCH)), Math.min(cards.length - 1, Math.ceil((1920 - x) / CARD_PITCH))];
}
function showCards() {
  const [lo, hi] = cardRange();
  for (let i = lo; i <= hi; i++) cardEl(i);
  clearTimeout(pruneTimer);
  pruneTimer = setTimeout(() => {
    const [lo, hi] = cardRange();
    for (const [i, el] of cardEls) {
      if (i >= lo && i <= hi) continue;
      const v = el.querySelector('video');
      if (v) releaseVideo(v);
      el.remove();
      cardEls.delete(i);
    }
  }, HUB_FAST.ms + 50);
}

function cardEl(i) {
  if (cardEls.has(i)) return cardEls.get(i);
  const c = cards[i];
  const el = document.createElement('div');
  el.className = `card ${c.kind}`;
  el.dataset.nav = `card:${i}`;
  if (c.kind === 'about') {
    el.innerHTML = '<div class="alogo"></div><div class="atext"></div>';
    aboutLogo(el.querySelector('.alogo'));
    el.querySelector('.atext').textContent = c.text;
  } else {
    el.innerHTML = `<div class="pic"></div>${c.glyph ? '<div class="glyph"></div>' : ''}${c.title ? '<div class="t"></div><div class="s"></div>' : ''}`;
    const pic = el.querySelector('.pic');
    const blank = () => { pic.style.background = tint(`${hubGame?.id}${i}`, 24, 10, 135); };
    if (c.image) {
      // async decoding: a frame never waits for a picture (the cards fade in anyway)
      const img = Object.assign(new Image(), { src: c.image, alt: '', decoding: 'async' });
      if (c.focus) img.style.objectPosition = c.focus;
      img.onerror = () => { img.remove(); blank(); };
      pic.append(img);
    } else blank();
    if (c.kind === 'trailer') pic.append(trailerVideo(c));
    const playing = c.kind === 'music' && !$.music.paused && $.music.loop;
    if (c.glyph) el.querySelector('.glyph').innerHTML = ICONS[playing ? 'pause' : c.glyph];
    if (c.title) {
      el.querySelector('.t').textContent = c.title;
      el.querySelector('.s').textContent = playing ? 'Playing' : c.sub;
    }
  }
  el.style.transform = `translateX(${i * CARD_PITCH}px)`;
  el.classList.toggle('focus', zone === 'hub2' && i === cardIndex);
  cardEls.set(i, el);
  $.strip.append(el);
  return el;
}

// the About card is text: the game's logo (trimmed like the hero's) or its name, then the store description
function aboutLogo(box) {
  const src = hubGame?.art?.logo ? host.fileUrl(hubGame.art.logo) : null;
  const name = () => { box.replaceChildren(); box.textContent = hubGame?.name ?? ''; box.classList.add('name'); };
  if (!src) return name();
  const img = Object.assign(new Image(), { src, alt: '', decoding: 'async' });
  img.onerror = name;
  const crop = logoBoxes.get(src);
  if (crop) img.style.objectViewBox = crop;
  else logoBox(src).then(b => { if (b) img.style.objectViewBox = b; });
  box.append(img);
}

const capital = s => s.charAt(0).toUpperCase() + s.slice(1);

function sectionFor(i) {
  $.section.textContent = cards[i]?.section ?? '';
}

// where the strip sits with card i focused: the focused card at x 56 until the last cards fill the screen
function stripX(i) {
  const last = Math.max(0, cards.length * CARD_PITCH - (CARD_PITCH - CARD) - (1920 - 2 * CARDS_X2));
  return CARDS_X2 - Math.min(i * CARD_PITCH, last);
}

function cardFocus(i) {
  if (i < 0) {
    for (const el of cardEls.values()) el.classList.remove('focus');
    return false;
  }
  i = Math.max(0, Math.min(cards.length - 1, i));
  if (i === cardIndex && cardEls.get(i)?.classList.contains('focus')) return false;
  if (view) endViewer(); // a press while the viewer is still shrinking away: it lands at once
  cardIndex = i;
  showCards();
  for (const [k, el] of cardEls) el.classList.toggle('focus', k === i);
  sectionFor(i);
  setStyle($.strip, 'transform', `translateX(${stripX(i)}px)`, HUB_FAST.ms, HUB_FAST.ease, -FRAME);
  pauseTrailer();
  trailerSoon();
  return true;
}

// after the card move (starting a video or decoding a picture costs a frame): the focused trailer plays, a focused
// screenshot and its neighbours decode at full size for the viewer
let cardTimer;
function trailerSoon() {
  clearTimeout(cardTimer);
  const c = cards[cardIndex];
  if (c?.kind === 'trailer') cardTimer = setTimeout(playTrailer, HUB_FAST.ms);
  else if (c?.kind === 'shot') cardTimer = setTimeout(() => { if (zone === 'hub2' && !view) fillSlots(c.shot); }, HUB_FAST.ms);
}

function cardAct(i) {
  const c = cards[i];
  if (c?.kind === 'shot') return openShot(i);
  if (c?.kind === 'trailer') return openTrailer(i);
  if (c?.kind !== 'music') return false;
  const a = $.music;
  const card = cardEl(i);
  if (a.paused || !a.loop) {
    a.loop = true;
    a.src = host.fileUrl(hubGame.music);
    a.play().catch(e => console.warn('music', e));
    card.querySelector('.glyph').innerHTML = ICONS.pause;
    card.querySelector('.s').textContent = 'Playing';
  } else stopMusic();
  return true;
}

function stopMusic() {
  if ($.music.paused && !$.music.getAttribute('src')) return;
  $.music.pause();
  $.music.removeAttribute('src');
  $.music.load();
  const card = $.strip.querySelector('.card.music');
  if (card) {
    card.querySelector('.glyph').innerHTML = ICONS.play;
    card.querySelector('.s').textContent = '';
  }
}

// ---------- the trailer card ----------
// Muted and looping while focused, cropped to fill the card like its poster. The video stays invisible until its
// first frame is up, then fades in over the poster, so the swap never shows black. Focus leaving pauses it and fades
// back to the poster; leaving the cards, the hub or Home releases it.
const trailerLength = new Map(); // trailer path → "0:30", once known
function trailerVideo(c) {
  const v = Object.assign(document.createElement('video'), { muted: true, loop: true, playsInline: true, preload: 'none' });
  v.addEventListener('loadedmetadata', () => {
    if (!Number.isFinite(v.duration) || !hubGame?.trailer) return;
    const s = Math.round(v.duration);
    c.sub = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
    trailerLength.set(hubGame.trailer, c.sub);
    const sub = v.closest('.card')?.querySelector('.s');
    if (sub) sub.textContent = c.sub;
  });
  return v;
}

const cardVideo = () => $.strip.querySelector('.card.trailer video');

function playTrailer() {
  if (zone !== 'hub2' || view || cards[cardIndex]?.kind !== 'trailer') return;
  const v = cardVideo();
  if (!v || !hubGame?.trailer) return;
  if (!v.getAttribute('src')) v.src = host.fileUrl(hubGame.trailer);
  v.muted = true;
  v.loop = true;
  v.play().catch(() => {});
  const show = () => { if (!v.paused && v.parentNode !== $.vvid) v.classList.add('live'); };
  if (v.requestVideoFrameCallback) v.requestVideoFrameCallback(show);
  else v.addEventListener('playing', show, { once: true });
}

function pauseTrailer() {
  clearTimeout(cardTimer);
  const v = cardVideo();
  if (!v) return;
  v.pause();
  v.classList.remove('live');
}

function releaseVideo(v) {
  v.pause();
  v.classList.remove('live');
  if (v.getAttribute('src')) { v.removeAttribute('src'); v.load(); }
}

function releaseTrailer() {
  clearTimeout(cardTimer);
  const v = cardVideo();
  if (v) releaseVideo(v);
}

// leaving Home, a game starting, the app hiding: the viewer goes at once and no video or trailer sound stays alive
function stopVideo() {
  if (view) endViewer();
  releaseTrailer();
  if (!$.music.loop) stopMusic();
}

// ---------- the viewer: a screenshot or the trailer, full screen ----------
// A grows the picture out of its card: one uniform scale from the card's crop (502 px of the 1080 px height, centred)
// to the full screen, on the hub's slow spring (ω = 32/s, 99 % at ≈207 ms), while black fades in over Home as
// Control Center's dim does (11 frames, ease-out). Left / Right and LB / RB step through the game's screenshots: the
// next one slides in 96 px while it fades in over the one shown (the hub's fast spring), so there is never a gap or a
// flash. Three pictures stay decoded and drawn (the one shown and its neighbours, the neighbours at opacity .001),
// so a step never waits. B shrinks the picture back onto the card of the screenshot it ended on (the strip moves
// there under the black first) and fades it out as it lands. The trailer grows the playing card video itself (moved,
// not copied, so it never restarts), with its music in sync; it closes the same way at the end of the video.
let view = null;   // {kind: 'shot' | 'trailer', card, shot, anims, stepAt, closing}
const slots = [];  // the three screenshot layers: {el, img, k (screenshot index or -1), ready}

const shotCount = () => hubGame?.screenshots?.length ?? 0;
const slotOf = k => slots.find(s => s.k === k);
const cardOfShot = k => cards.findIndex(c => c.kind === 'shot' && c.shot === k);

// the screenshot k, its neighbours decoded in the other two layers
function fillSlots(k) {
  const want = [k, k - 1, k + 1].filter(j => j >= 0 && j < shotCount());
  const free = slots.filter(s => !want.includes(s.k));
  for (const j of want) {
    if (slotOf(j)) continue;
    const s = free.shift();
    s.k = j;
    s.img.src = host.fileUrl(hubGame.screenshots[j]);
    s.ready = s.img.decode().catch(() => {});
  }
}

function releaseShots() {
  for (const s of slots) {
    s.k = -1;
    s.ready = null;
    s.img.removeAttribute('src');
  }
}

// the transform that lays the 1920×1080 viewer layer over card i: the card shows the picture's centred square
function onCard(i) {
  const s = Math.max(CARD / 1920, CARD / 1080);
  const x = stripX(cardIndex) + i * CARD_PITCH + CARD / 2 - 960 * s, y = CARDS_Y2 + CARD / 2 - 540 * s;
  return `translate(${+x.toFixed(2)}px, ${+y.toFixed(2)}px) scale(${+s.toFixed(5)})`;
}

function viewerAction(name, e) {
  if (name === 'left' || name === 'right' || name === 'lb' || name === 'rb') return step(name === 'left' || name === 'lb' ? -1 : 1, e);
  if (name === 'wheel') return step(Math.sign(e.dy || e.dx) || 1, e);
  if (e.held) return false;
  if (name === 'b' || name === 'click') return closeViewer();
  return false;
}

function openShot(i) {
  const k = cards[i].shot;
  if (view) endViewer();
  zone = 'viewer';
  view = { kind: 'shot', card: i, shot: k, anims: [], stepAt: 0, closing: false };
  fillSlots(k);
  const s = slotOf(k);
  // decoded since the card was focused; if not yet, wait (≤ 250 ms) rather than grow a blank layer
  Promise.race([s.ready, new Promise(done => setTimeout(done, 250))]).then(() => {
    if (view?.kind === 'shot' && view.shot === k && !view.closing) growIn(s.el);
  });
  return true;
}

function openTrailer(i) {
  const v = cardVideo();
  if (!v || !hubGame?.trailer) return false;
  if (view) endViewer();
  clearTimeout(cardTimer);
  zone = 'viewer';
  view = { kind: 'trailer', card: i, shot: -1, anims: [], stepAt: 0, closing: false };
  if (!v.getAttribute('src')) v.src = host.fileUrl(hubGame.trailer);
  const ready = v.readyState >= 2 ? Promise.resolve()
    : Promise.race([new Promise(done => v.addEventListener('loadeddata', done, { once: true })), new Promise(done => setTimeout(done, 400))]);
  ready.then(() => {
    if (view?.kind !== 'trailer' || view.closing) return;
    $.vvid.append(v); // a move in one task: it keeps playing (a media element only pauses if it stays out of the page)
    v.classList.remove('live');
    v.muted = false;
    v.loop = false;
    v.onended = () => closeViewer();
    v.play().catch(() => {});
    trailerSound(v);
    growIn($.vvid);
  });
  return true;
}

// the trailer's sound: its music from the same moment (the video file has none)
function trailerSound(v) {
  if (!hubGame?.music) return;
  stopMusic();
  const a = $.music;
  a.loop = false;
  a.src = host.fileUrl(hubGame.music);
  a.currentTime = v.currentTime + TRAILER_AUDIO_AT;
  a.play().catch(e => console.warn('trailer sound', e));
  // the music starts a few frames after the picture (it loads and seeks first): catch up when it drifts (≈4 checks a
  // second while the video plays, nothing more)
  v.ontimeupdate = () => {
    if (a.paused || a.loop) return;
    const want = v.currentTime + TRAILER_AUDIO_AT;
    if (Math.abs(a.currentTime - want) > 0.1) a.currentTime = want;
  };
}

function growIn(el) {
  for (const s of slots) Object.assign(s.el.style, { zIndex: 0, opacity: 0.001, transform: 'none' });
  Object.assign(el.style, { zIndex: 1, opacity: 1, transform: 'none' });
  head();
  $.viewer.classList.add('on');
  $.vdim.style.opacity = 1;
  $.vhead.style.opacity = 1;
  view.anims = [
    el.animate([{ transform: onCard(view.card) }, { transform: 'none' }], { duration: HUB_SLOW.ms, delay: -FRAME, easing: HUB_SLOW.ease }),
    el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 4 * FRAME, delay: -FRAME, easing: EASE_OUT }),
    $.vdim.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 11 * FRAME, delay: -FRAME, easing: EASE_OUT }),
    $.vhead.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 9 * FRAME, delay: 10 * FRAME, easing: EASE_OUT, fill: 'backwards' }),
  ];
}

// the game and "3 of 8" sit over the picture's own corners (game HUDs live there): shown on open and on each step,
// gone after a moment
const HEAD_MS = 2500;
let headTimer, headAnim;
function head() {
  $.vname.textContent = hubGame?.name ?? '';
  $.vcount.textContent = view.kind === 'shot' ? `${view.shot + 1} of ${shotCount()}` : '';
  const shown = opacityNow($.vhead, headAnim);
  if (shown < 1 && $.viewer.classList.contains('on')) {
    headAnim?.cancel();
    headAnim = $.vhead.animate([{ opacity: shown }, { opacity: 1 }], { duration: 6 * FRAME, easing: EASE_OUT });
  }
  $.vhead.style.opacity = 1;
  clearTimeout(headTimer);
  headTimer = setTimeout(() => {
    headAnim = $.vhead.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 9 * FRAME, easing: EASE_OUT });
    $.vhead.style.opacity = 0;
  }, HEAD_MS);
}

function step(d, e) {
  if (view.kind !== 'shot' || view.closing) return false;
  const j = view.shot + d;
  if (j < 0 || j >= shotCount()) return false;
  const now = performance.now();
  if (e.held && now - view.stepAt < VIEW_HELD_MS) return 'none'; // still held: no step yet, and no sound
  view.stepAt = now;
  for (const a of view.anims) a.finish(); // a step during the last one: that one lands first
  const from = slotOf(view.shot);
  for (const s of slots) if (s !== from) s.el.style.opacity = 0.001;
  view.shot = j;
  head();
  fillSlots(j);
  const to = slotOf(j);
  const go = () => {
    if (view?.shot !== j || view.closing) return;
    for (const s of slots) s.el.style.zIndex = s === to ? 2 : s === from ? 1 : 0;
    Object.assign(to.el.style, { opacity: 1, transform: 'none' });
    view.anims = [
      to.el.animate([{ transform: `translateX(${d * VIEW_SHIFT}px)` }, { transform: 'none' }], { duration: HUB_FAST.ms, delay: -FRAME, easing: HUB_FAST.ease }),
      to.el.animate([{ opacity: 0.001 }, { opacity: 1 }], { duration: 9 * FRAME, delay: -FRAME, easing: EASE_OUT }),
    ];
    view.anims[0].onfinish = () => {
      if (view?.shot !== j) return;
      if (from !== to) from.el.style.opacity = 0.001;
      follow(j);
    };
  };
  to.ready.then(go);
  return true;
}

// the strip follows the screenshot shown, at once, under the black (after the step, so its new cards are made while
// nothing moves): B lands on its card with nothing else moving
function follow(k) {
  const i = Math.max(0, cardOfShot(k));
  view.card = i;
  if (i === cardIndex) return;
  cardIndex = i;
  showCards();
  for (const [n, c] of cardEls) c.classList.toggle('focus', n === i);
  sectionFor(i);
  setStyle($.strip, 'transform', `translateX(${stripX(i)}px)`, 0);
}

function closeViewer() {
  if (!view || view.closing) return false;
  const el = view.kind === 'trailer' ? $.vvid : slotOf(view.shot)?.el;
  if (view.kind === 'trailer') {
    const v = $.vvid.querySelector('video');
    if (v) { v.onended = v.ontimeupdate = null; v.muted = true; }
    stopMusic();
  }
  if (view.kind === 'shot') follow(view.shot); // done after each step; here only if B comes during one
  for (const a of view.anims) a.finish(); // B during the open or a step: that lands first
  for (const s of slots) if (s.el !== el) s.el.style.opacity = 0.001;
  view.closing = true;
  zone = 'hub2';
  if (!el) return endViewer(), true;
  view.anims = [
    el.animate([{ transform: 'none' }, { transform: onCard(view.card) }], { duration: HUB_SLOW.ms, delay: -FRAME, easing: HUB_SLOW.ease, fill: 'forwards' }),
    el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 6 * FRAME, delay: 7 * FRAME, easing: 'linear', fill: 'both' }),
    $.vdim.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 12 * FRAME, delay: -FRAME, easing: EASE_OUT, fill: 'forwards' }),
    $.vhead.animate([{ opacity: opacityNow($.vhead, headAnim) }, { opacity: 0 }], { duration: 4 * FRAME, easing: EASE_OUT, fill: 'forwards' }),
  ];
  clearTimeout(headTimer);
  headAnim?.cancel();
  view.anims[0].onfinish = endViewer;
  return 'back';
}

// the viewer gone, at once: the trailer video goes back into its card (playing on, muted, if the card has focus)
function endViewer() {
  if (!view) return;
  const kind = view.kind;
  for (const a of view.anims) a.cancel();
  clearTimeout(headTimer);
  headAnim?.cancel();
  view = null;
  $.viewer.classList.remove('on');
  for (const el of [$.vdim, $.vhead]) el.style.opacity = 0;
  for (const s of slots) Object.assign(s.el.style, { zIndex: 0, opacity: 0.001, transform: 'none' });
  $.vvid.style.transform = 'none';
  if (zone === 'viewer') zone = 'hub2';
  if (kind !== 'trailer') return;
  if (!$.music.loop) stopMusic();
  const v = $.vvid.querySelector('video');
  if (!v) return;
  v.onended = v.ontimeupdate = null;
  v.muted = true;
  v.loop = true;
  const pic = cardEls.get(cards.findIndex(c => c.kind === 'trailer'))?.querySelector('.pic');
  if (!pic || zone !== 'hub2' || cards[cardIndex]?.kind !== 'trailer') { releaseVideo(v); if (pic) pic.append(v); else v.remove(); return; }
  if (v.ended) v.currentTime = 0;
  pic.append(v);
  v.classList.add('live');
  v.play().catch(() => {});
}

// ---------- Options menu (☰ / •••) and Information ----------
function openMenu(from) {
  const g = from === 'row' ? focusedGame() : hubGame;
  if (!g) return false;
  menuFrom = from;
  menuItems = [
    ...(from === 'row' ? [['Game hub', () => { closeMenu(); enterHub1(); }]] : []),
    ['Information', () => showInfo(g)],
    ['Refresh art', () => { closeMenu(); host.library.refresh?.(g.id).catch(e => console.warn('refresh', e)); }],
    ...(running === g.id ? [['Close Game', () => { closeMenu(); host.games.close?.(g.id).catch(e => console.warn('close', e)); }]] : []),
  ];
  $.menu.replaceChildren(...menuItems.map(([label], i) => {
    const el = document.createElement('div');
    el.className = 'item';
    el.dataset.nav = `menu:${i}`;
    el.textContent = label;
    return el;
  }));
  // next to what it is for: under the focused tile's label, or beside •••
  $.menu.style.left = from === 'row' ? '172px' : '611px';
  $.menu.style.top = from === 'row' ? '318px' : from === 'hub1' ? `${836 - HUB_LIFT}px` : '318px';
  zone = 'menu';
  menuSelect(0);
  setStyle($.menu, 'opacity', 1, 6 * FRAME, EASE_OUT, 0);
  $.menu.style.visibility = 'visible';
  return true;
}

function menuSelect(i) {
  menuIndex = Math.max(0, Math.min(menuItems.length - 1, i));
  [...$.menu.children].forEach((el, k) => el.classList.toggle('focus', k === menuIndex));
}

function closeMenu() {
  zone = menuFrom;
  setStyle($.menu, 'opacity', 0, 4 * FRAME, EASE_OUT);
  $.menu.style.visibility = 'hidden';
  return true;
}

function menuAction(name, e) {
  if (name === 'up' || name === 'down') {
    const i = menuIndex + (name === 'up' ? -1 : 1);
    if (i < 0 || i >= menuItems.length) return false;
    menuSelect(i);
    return true;
  }
  if (e.held) return false;
  if (name === 'point' && e.target?.startsWith('menu:')) { const i = +e.target.slice(5); if (i === menuIndex) return false; menuSelect(i); return true; }
  if (name === 'click' && e.target?.startsWith('menu:')) { menuSelect(+e.target.slice(5)); name = 'a'; }
  if (name === 'a') { menuItems[menuIndex][1](); return true; }
  if (name === 'b' || name === 'menu' || name === 'click') return closeMenu();
  return false;
}

function showInfo(g) {
  $.menu.style.visibility = 'hidden';
  $.menu.style.opacity = 0;
  const rows = [
    ['Playtime', g.playtimeSec ? playtime(g.playtimeSec).replace(' played', '') : 'Not played yet'],
    ['Last played', g.lastPlayed ? capital(ago(g.lastPlayed)) : '—'],
    ['Added', g.addedAt ? capital(ago(g.addedAt)) : '—'],
    ['Folder', g.folder ?? '—'],
    ['Program', g.exe ? String(g.exe).split(/[\\/]/).pop() : '—'],
  ];
  $.info.innerHTML = `<div class="head"></div>${rows.map(() => '<div class="row"><span></span><b></b></div>').join('')}`;
  $.info.querySelector('.head').textContent = g.name;
  [...$.info.querySelectorAll('.row')].forEach((el, i) => {
    el.querySelector('span').textContent = rows[i][0];
    el.querySelector('b').textContent = rows[i][1];
  });
  zone = 'info';
  $.info.style.visibility = 'visible';
  setStyle($.info, 'opacity', 1, 8 * FRAME, EASE_OUT, 0);
}

function closeInfo() {
  setStyle($.info, 'opacity', 0, 4 * FRAME, EASE_OUT);
  $.info.style.visibility = 'hidden';
  zone = menuFrom;
  return true;
}

// ---------- mouse ----------
function pointer(kind, e) {
  const [what, arg] = (e.target ?? '').split(':');
  const click = kind === 'click';
  if (what === 'tile' && (zone === 'row' || zone === 'top')) {
    if (zone === 'top') topAction('down', {});
    const i = +arg;
    if (i !== index) return run ? false : move(i);
    return click ? activate() : false;
  }
  if (what === 'tab') return click ? switchTab(arg) : false;
  if (what === 'top' && (zone === 'row' || zone === 'top')) {
    const i = TOPS.findIndex(([id]) => id === arg);
    if (click) return topActivate(arg);
    return i === topIndex && zone === 'top' ? false : focusTop(i);
  }
  if (what === 'hero') {
    const i = arg === 'play' ? 0 : 1;
    if (zone === 'hub1') {
      if (click) return i === 0 ? launch(hubGame) : openMenu('hub1');
      if (i === heroIndex) return false;
      heroFocus(i);
      return true;
    }
    if (zone === 'row' && click) return i === 0 ? activate() : openMenu('row');
    return false;
  }
  if (what === 'card' && zone === 'hub2') {
    const i = +arg;
    if (click) return i === cardIndex ? cardAct(i) : cardFocus(i);
    return cardFocus(i);
  }
  return false;
}

// ---------- launch and return ----------
function track(a) {
  pending.add(a);
  a.onfinish = a.oncancel = () => pending.delete(a);
  return a;
}

// UI fades out (167 ms), the art holds (300 ms), the navy screen cross-fades in (200 ms), then the game starts
function launch(g) {
  if (!g) return false;
  if (running === g.id) return resume();
  if (running) {
    const other = games.find(x => x.id === running);
    toast({ title: other?.name ?? running, message: 'Close it to play another game.', icon: other?.art?.tile ? host.fileUrl(other.art.tile) : null });
    return 'back';
  }
  if (zone === 'menu') closeMenu();
  zone = 'busy';
  stopVideo();
  stopMusic();
  launchAnims = [
    $.ui.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 10 * FRAME, easing: EASE_OUT, fill: 'forwards' }),
    $.navy.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 12 * FRAME, delay: 28 * FRAME, easing: EASE_OUT, fill: 'forwards' }),
  ];
  // Guide over the game opens Control Center: have it drawn over this game while the screen holds still (the UI is
  // out, the art waits for the navy), so its first open doesn't compile the GPU's shaders (notices.js). The game
  // starts once that is done (at once after the first launch), as the app sleeps when it starts.
  let warm = Promise.resolve();
  launchTimer = setTimeout(() => {
    warm = warmControlCenter(app, g);
    launchTimer = setTimeout(() => warm.then(() => {
      if (zone === 'busy') host.games.launch(g.id).catch(err => { console.warn('launch', err); returnHome(); });
    }), 29 * FRAME);
  }, 11 * FRAME);
  return true;
}

// back to the running game (Resume): it comes to the front and the app sleeps; Home stays as it is
function resume() {
  if (zone === 'menu') closeMenu();
  stopVideo();
  stopMusic();
  host.games.resume().catch(e => console.warn('resume', e));
  return true;
}

// the window is hidden while the game runs: show black when it comes back, not the navy screen
function blackout() {
  stopVideo();
  for (const a of [...pending, ...launchAnims]) a.cancel();
  pending.clear();
  launchAnims = [];
  for (const el of [$.top, $.rowwrap, $.herowrap, $.bgwrap]) setStyle(el, 'opacity', 0, 0);
  $.ui.style.opacity = 1;
  $.navy.style.opacity = 0;
  dark = true;
}

// from black: the row and top bar (330 ms), the background (+230 ms) and the hero (+470 ms)
function reveal() {
  dark = false;
  const hold = 4 * FRAME;
  for (const el of [$.top, $.rowwrap]) setStyle(el, 'opacity', 1, 20 * FRAME, RETURN_ROW, hold);
  setStyle($.bgwrap, 'opacity', 1, 14 * FRAME, EASE_OUT, hold + 14 * FRAME);
  setStyle($.herowrap, 'opacity', 1, 14 * FRAME, EASE_OUT, hold + 28 * FRAME);
}

function focusItem(id) {
  const i = items.findIndex(it => it.id === id);
  if (i >= 0 && i !== index) { index = i; showNow(); }
}

function gameEvent({ id, state, front }) {
  if (state === 'launching' || state === 'running') { running = id; markRunning(); }
  // Guide over the game: Home (dark since the launch) shows it focused when it comes in
  if (state === 'over') {
    if (zone === 'busy') blackout();
    focusItem(id);
  }
  if (state === 'exited') {
    if (running === id) running = null;
    markRunning();
    returnHome(id, front === 'app');
  }
}

// the game ended: Home with it first and focused. From the game, black and then the return motion (§2 "Return to
// Home"); if Home was already on screen over it (Close Game), at once.
function returnHome(id, overGame = false) {
  clearTimeout(launchTimer);
  const onScreen = overGame && !dark && app.top?.el === root;
  if (app.top?.el !== root) app.go('home');
  if (!onScreen) blackout();
  resetZones();
  const i = items.findIndex(it => it.id === id);
  if (i >= 0) index = i;
  showNow();
  if (!onScreen) reveal();
}

function resetZones() {
  stopVideo();
  releaseShots();
  zone = 'row';
  $.icons.forEach(el => el.classList.remove('focus'));
  for (const [el, prop, v] of [[$.content, 'transform', 'translateY(0px)'], [$.strip, 'transform', `translateX(${CARDS_X1}px)`],
    [$.hubhead, 'opacity', 0.001], [$.cards, 'opacity', 0.001], [$.dim, 'opacity', 0], [$.menu, 'opacity', 0], [$.info, 'opacity', 0]]) setStyle(el, prop, v, 0);
  $.menu.style.visibility = $.info.style.visibility = 'hidden';
  heroFocus(-1);
  cardFocus(-1);
  hubGame = null;
}

function clock(el) {
  const tick = () => {
    el.textContent = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', hourCycle: 'h23' }); // 24 h, as the PS5
    setTimeout(tick, 60000 - (Date.now() % 60000) + 50);
  };
  tick();
}
