// Control Center (design/ps5/1-home.md §1.6–1.8, motion §2): Guide tap over Home, any screen, or a running game.
// - Over a screen: it dims (art to ≈35 % with the PS5's per-frame curve, Home's UI to ≈10 %, the bottom darker), the
//   bar rises 20 px while fading in, then the cards. Closing fades the bar and cards first, then un-dims linearly.
// - Over a running game: the host copied the screen as Guide was pressed (our window is opaque), so that still is the
//   backdrop at full brightness, with the PS5's top and bottom scrims; the scrims fade in, then the bar and cards.
//   B, Guide or "Back to game" fade them out and the game comes back to the front (host.games.resume).
// - The bar: Home, the game (running: Back to game / Close Game; else the last played), Notifications, Sound,
//   Controller, Network, Mouse mode, Power. Left/Right move along it, A opens the item's panel, Up goes to the cards.
//   A panel rises from its bottom edge above its icon while the other icons dim and the cards fade (§1.7).
// - Cards (bottom left): the game and the notifications. The current one is grown (432×520) as in §1.6; it gets the
//   ring when the cards have focus.
// - Power from Home's power icon: app.push('control-center', {power: true}) opens straight to the Power panel.
// Read-only state for tests: window.__cc.state().

import { notices, session, takeOver, onNotices, library, dropPreStill } from './notices.js';
import { toast } from './toast.js';

const FRAME = 1000 / 60;
const EASE_OUT = 'cubic-bezier(0, 0, .2, 1)';
// §2 "Control Center open (Home)": the dim's measured alpha per frame, 26 … 99 % over 11 frames (183 ms)
const DIM = 'linear(0, .26 9.09%, .49 18.18%, .64 27.27%, .75 36.36%, .85 45.45%, .89 54.55%, .94 63.64%, .96 72.73%, .97 81.82%, .99 90.91%, 1)';
// open: the dim starts one frame in (the first frame already shows 26 %), the bar a frame later, the cards after it
const OPEN = { dim: 11 * FRAME, bar: 7 * FRAME, barDelay: 0, cards: 8 * FRAME, cardsDelay: FRAME, grow: 10 * FRAME };
// close: bar and cards out in 8 frames, then the un-dim, linear, in 6
const CLOSE = { out: 8 * FRAME, undim: 6 * FRAME };
// over a game (§2 "Control Center over game", ≈130 ms): scrims first, the bar from +3, the cards from +5
const OVER = { scrims: 4 * FRAME, bar: 4 * FRAME, barDelay: 3 * FRAME, cards: 3 * FRAME, cardsDelay: 5 * FRAME, out: 6 * FRAME, reveal: 10 * FRAME };
// §2 "Sound panel open": the icons dim and the cards fade (8 frames), then the panel grows up from its bottom (13)
const PANEL = { wait: 8 * FRAME, grow: 13 * FRAME, out: 6 * FRAME };
const BAR = { y: 1011, pitch: 112 };
const CARD = { x: 84, w: 360, h: 400, W: 432, H: 520, gap: 19 }; // bottom 889 (control-center.css)
const PANEL_BOTTOM = 880;

const S = 'fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"';
const ICONS = {
  home: `<svg viewBox="0 0 32 32"><path ${S} d="M4.5 15 16 5.5 27.5 15"/><path ${S} d="M8.5 12.5V26.5h5.5v-7.5h4v7.5h5.5V12.5"/></svg>`,
  game: `<svg viewBox="0 0 32 32"><rect ${S} x="4" y="10" width="17" height="17" rx="3.5"/><path ${S} d="M10 5.5h13.5a3.5 3.5 0 0 1 3.5 3.5v13.5"/><path fill="currentColor" d="M10.5 14.5v8l6-4Z"/></svg>`,
  notifications: `<svg viewBox="0 0 32 32"><path ${S} d="M16 4.5a7.5 7.5 0 0 0-7.5 7.5v6L6 22.5h20L23.5 18v-6A7.5 7.5 0 0 0 16 4.5Z"/><path ${S} d="M12.8 26a3.3 3.3 0 0 0 6.4 0"/></svg>`,
  sound: `<svg viewBox="0 0 32 32"><path fill="currentColor" d="M4.5 12h5.2l7.3-6.3v20.6L9.7 20H4.5Z"/><path ${S} d="M21 11.5a6.4 6.4 0 0 1 0 9M24.6 8a11.4 11.4 0 0 1 0 16"/></svg>`,
  controller: `<svg viewBox="0 0 32 32"><path ${S} d="M10 9.5h12a5.6 5.6 0 0 1 5.4 4.2l1.7 6.6a3.4 3.4 0 0 1-5.9 3L20.6 20h-9.2l-2.6 3.3a3.4 3.4 0 0 1-5.9-3l1.7-6.6A5.6 5.6 0 0 1 10 9.5Z"/><path ${S} d="M10.5 13v5M8 15.5h5"/><circle fill="currentColor" cx="21.5" cy="14" r="1.6"/><circle fill="currentColor" cx="24" cy="17" r="1.6"/></svg>`,
  network: `<svg viewBox="0 0 32 32"><path ${S} d="M3.8 12.4a17.3 17.3 0 0 1 24.4 0M7.8 16.6a11.6 11.6 0 0 1 16.4 0M11.9 20.7a5.8 5.8 0 0 1 8.2 0"/><circle fill="currentColor" cx="16" cy="25" r="2.2"/></svg>`,
  mouse: `<svg viewBox="0 0 32 32"><rect ${S} x="8.5" y="3.5" width="15" height="25" rx="7.5"/><path ${S} d="M16 4v8"/></svg>`,
  power: `<svg viewBox="0 0 32 32"><path ${S} stroke-width="3" d="M10.2 8.6a10.4 10.4 0 1 0 11.6 0"/><path ${S} stroke-width="3" d="M16 4.5v11"/></svg>`,
};
const ITEMS = ['home', 'game', 'notifications', 'sound', 'controller', 'network', 'mouse', 'power'];
const LABELS = { home: 'Home', notifications: 'Notifications', sound: 'Sound', controller: 'Controller', network: 'Network', mouse: 'Mouse mode', power: 'Power' };
const POWER = [
  ['sleep', 'Sleep', 'Put the PC to sleep?'],
  ['restart', 'Restart', 'Restart the PC?'],
  ['shutdown', 'Shut down', 'Shut down the PC?'],
  ['exit', 'Exit to desktop', 'Exit to the desktop?'],
];

let app, host, root;
const $ = {};
let mode = 'home';      // 'home': dimming the screen below | 'game': the still of the running game
let zone = 'bar';       // bar | cards | panel
let barIndex = 0, cardIndex = 0;
let cards = [];         // [{id: 'game' | 'notifications', run()}]
let panel = null;       // {id, title, rows: [row], focus, back?}; row: {label, value?, kind, run?, step?, on?}
let powerOnly = false;  // opened from Home's power icon: B on the Power panel closes Control Center
let open = false;
let anims = [];
let closing = null;     // the close in progress: {done}
let underUi = null;     // Home's UI layer, dimmed further than its art
const games = () => library.games;
let clockTimer = 0;
let openedAt = 0;
let stillUrl = null, fullUrl = null;

export default {
  async mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="blank"></div><img class="still" alt=""><img class="still full" alt="">
      <div class="dim"></div><div class="shade"></div><div class="scrim"></div>
      <div class="clock"></div>
      <div class="cards"></div>
      <div class="panel"><div class="head"></div><div class="rows"></div></div>
      <div class="bar">${ITEMS.map((id, i) => `<div class="item" data-nav="cc:bar:${i}" style="left:${barX(i)}px"><i class="disc"></i>${ICONS[id]}<span class="label"></span></div>`).join('')}</div>`;
    for (const k of ['blank', 'still', 'dim', 'shade', 'scrim', 'clock', 'cards', 'panel', 'bar']) $[k] = el.querySelector(`.${k}`);
    $.full = el.querySelector('.still.full');
    $.head = $.panel.querySelector('.head');
    $.rows = $.panel.querySelector('.rows');
    $.items = [...el.querySelectorAll('.bar .item')];

    host.on('library', () => { if (open && zone !== 'panel') buildCards(); });
    onNotices(() => { if (open && zone !== 'panel') buildCards(); });
    window.__cc = { state: () => ({ open, mode, zone, item: ITEMS[barIndex], card: cards[cardIndex]?.id, panel: panel?.id, rows: panel?.rows.map(r => r.label), focus: panel?.focus, openedAt }) };
  },

  enter(params = {}) {
    if (params.back) return;
    finishClose();
    cancel();
    // warm-up (notices.warmControlCenter): drawn at a hair's opacity as it will be over the game being launched
    // (params.warm: {id, kind}), then popped; no motion, no input
    warming = params.warm ?? null;
    root.classList.toggle('warm', warming);
    if (warming) {
      // the layers it draws over a game (a BMP still, the scrims) are the ones a first open finds cold
      root.dataset.mode = 'game';
      $.still.src = $.full.src = tinyBmp();
      $.still.style.opacity = $.full.style.opacity = 1;
      buildCards();
      focusBar(0);
      tickClock();
      return;
    }
    const over = takeOver();
    mode = over ? 'game' : 'home';
    root.dataset.mode = mode;
    powerOnly = !!params.power;
    open = true;
    openedAt = performance.now();
    const below = app.stack.at(-2);
    underUi = mode === 'home' && below?.id === 'home' ? below.el.querySelector(':scope > .ui') : null;
    panel = null;
    $.panel.classList.remove('on');
    root.classList.remove('paneled');
    zone = 'bar';
    cardIndex = 0;
    buildCards();
    focusBar(powerOnly ? ITEMS.indexOf('power') : 0);
    tickClock();
    if (mode === 'game') openOverGame(over);
    else openOverScreen();
    if (powerOnly && mode === 'home') openPanel('power', true);
  },

  leave() {
    if (warming) {
      warming = null;
      root.classList.remove('warm');
      $.still.style.opacity = $.full.style.opacity = 0;
      clearTimeout(clockTimer);
      return;
    }
    if (!open) return;
    open = false;
    clearTimeout(clockTimer);
    // pop (B, Guide, Back to game) takes Control Center off the stack before this; go (Home, Guide hold) doesn't.
    // Back to game resumes however Control Center opened (also over Home over the game, where mode is 'home')
    const popped = !app.stack.some(s => s.el === root);
    const resume = popped && (leaving === 'resume' || (mode === 'game' && !leaving));
    leaving = null;
    close(resume);
  },

  action(name, e = {}) {
    if (!open || closing) return false;
    if (name === 'point' || name === 'click') return pointer(name, e);
    if (name === 'release' || name === 'wheel') return false;
    if (zone === 'panel') return panelAction(name, e);
    if (zone === 'cards') return cardsAction(name, e);
    return barAction(name, e);
  },
};

let leaving = null; // why Control Center is closing itself: 'stay' (no resume: Home, mouse mode, power, close game)
                    // or 'resume' (Back to game)
let warming = null;       // the warm-up's session ({id, kind}) while it draws

// a 2×2 grey BMP as an object URL, for the warm-up
function tinyBmp() {
  const b = new Uint8Array(54 + 16);
  const v = new DataView(b.buffer);
  b.set([0x42, 0x4d]);
  v.setUint32(2, b.length, true);
  v.setUint32(10, 54, true);
  v.setUint32(14, 40, true);
  v.setInt32(18, 2, true);
  v.setInt32(22, -2, true);
  v.setUint16(26, 1, true);
  v.setUint16(28, 32, true);
  b.fill(80, 54);
  return URL.createObjectURL(new Blob([b], { type: 'image/bmp' }));
}

// ---------- open and close ----------
function track(a) {
  anims.push(a);
  return a;
}

function cancel() {
  for (const a of anims) a.cancel();
  anims = [];
}

function openOverScreen() {
  const dim = { duration: OPEN.dim, easing: DIM, delay: -FRAME, fill: 'both' };
  for (const el of [$.dim, $.shade, $.clock]) track(el.animate([{ opacity: 0 }, { opacity: 1 }], dim));
  if (underUi) track(underUi.animate([{ opacity: 1 }, { opacity: 0.3 }], { ...dim, fill: 'forwards' }));
  track($.bar.animate([{ opacity: 0, transform: 'translateY(20px)' }, { opacity: 1, transform: 'none' }],
    { duration: OPEN.bar, easing: EASE_OUT, delay: OPEN.barDelay, fill: 'both' }));
  track($.cards.animate([{ opacity: 0 }, { opacity: 1 }], { duration: OPEN.cards, easing: EASE_OUT, delay: OPEN.cardsDelay, fill: 'both' }));
  growCurrent(OPEN.cardsDelay);
}

function openOverGame(over) {
  // black until the still is decoded (the screen below is dark while a game runs), then the still at full brightness:
  // half size first, the full one on top once it is decoded
  $.blank.style.opacity = 1;
  $.still.style.opacity = 0;
  $.full.style.opacity = 0;
  for (const el of [$.scrim, $.clock, $.bar, $.cards]) track(el.animate([{ opacity: 0 }, { opacity: 0 }], { duration: 1, fill: 'forwards' }));
  const token = openedAt;
  const start = url => {
    $.still.style.opacity = url ? 1 : 0;
    cancel();
    window.__cc.stillAt = performance.now();
    // when each step happened (Unix ms), to compare with the host's log: the over event, the still fetched, on stage
    window.__cc.times = { over: over.wall, enter: Date.now() - (performance.now() - openedAt), fetched: session.stillFetched, staged: session.preStillAt, started: Date.now() };
    const t = (ms, delay) => ({ duration: ms, easing: EASE_OUT, delay, fill: 'both' });
    for (const el of [$.scrim, $.clock]) track(el.animate([{ opacity: 0 }, { opacity: 1 }], t(OVER.scrims, -FRAME)));
    track($.bar.animate([{ opacity: 0, transform: 'translateY(20px)' }, { opacity: 1, transform: 'none' }], t(OVER.bar, OVER.barDelay)));
    track($.cards.animate([{ opacity: 0 }, { opacity: 1 }], t(OVER.cards, OVER.cardsDelay)));
    growCurrent(OVER.cardsDelay);
  };
  over.full.then(async url => {
    if (!url) return;
    if (fullUrl) URL.revokeObjectURL(fullUrl);
    fullUrl = url;
    $.full.src = url;
    await $.full.decode().catch(() => {});
    if (token !== openedAt || !open) return;
    $.full.style.opacity = 1;
    window.__cc.fullAt = performance.now();
  });
  // the still the page put on the stage before the window showed: take it over, decoded, and start at once
  const pre = session.preStill;
  if (pre) {
    session.preStill = null;
    pre.removeAttribute('style');
    pre.className = 'still';
    $.still.replaceWith(pre);
    $.still = pre;
    if (stillUrl && stillUrl !== pre.src) URL.revokeObjectURL(stillUrl);
    stillUrl = pre.src;
    start(stillUrl);
    return;
  }
  over.still.then(async url => {
    if (url && url !== stillUrl) {
      if (stillUrl) URL.revokeObjectURL(stillUrl);
      stillUrl = url;
      $.still.src = url;
      await $.still.decode().catch(() => {});
    }
    if (token !== openedAt || !open) return;
    dropPreStill();
    start(url);
  });
}

// the current opacity of an element (its running animation's, or 1)
const opacityOf = el => +getComputedStyle(el).opacity;

function close(resume) {
  root.classList.add('closing'); // the router hid the layer; keep it while it fades
  const now = el => [{ opacity: opacityOf(el) }, { opacity: 0 }];
  const panelOn = $.panel.classList.contains('on');
  const front = [$.bar, $.cards, ...(panelOn ? [$.panel] : [])];
  const froms = front.map(el => now(el));
  const fades = [];
  let total;
  if (mode === 'game') {
    const scrimFrom = now($.scrim);
    cancel();
    for (const [i, el] of front.entries()) fades.push(track(el.animate(froms[i], { duration: OVER.out, delay: -FRAME, easing: EASE_OUT, fill: 'forwards' })));
    fades.push(track($.scrim.animate(scrimFrom, { duration: OVER.out, delay: -FRAME, easing: EASE_OUT, fill: 'forwards' })));
    if (!resume) {
      // to Home over the game: the still gives way to Home coming in
      for (const el of [$.still, $.full, $.blank, $.clock]) fades.push(track(el.animate(now(el), { duration: OVER.reveal, easing: EASE_OUT, fill: 'forwards' })));
    }
    total = resume ? OVER.out : OVER.reveal;
  } else {
    const dims = [$.dim, $.shade, $.clock].map(el => [el, opacityOf(el)]);
    const uiFrom = underUi ? opacityOf(underUi) : 1;
    cancel();
    // the press frame already shows the fade (as the open's first frame shows the dim)
    for (const [i, el] of front.entries()) fades.push(track(el.animate(froms[i], { duration: CLOSE.out, delay: -FRAME, easing: EASE_OUT, fill: 'forwards' })));
    // then the screen below un-dims, linear
    const undim = { duration: CLOSE.undim, delay: CLOSE.out - FRAME, easing: 'linear', fill: 'both' };
    for (const [el, o] of dims) fades.push(track(el.animate([{ opacity: o }, { opacity: 0 }], undim)));
    if (underUi) fades.push(track(underUi.animate([{ opacity: uiFrom }, { opacity: 1 }], undim)));
    total = CLOSE.out + CLOSE.undim - FRAME;
  }
  const c = { done: false };
  closing = c;
  window.__cc.closedAt = performance.now();
  setTimeout(() => {
    if (closing !== c) return;
    if (resume) {
      // the game comes to the front above us, then the app sleeps; tidy up after that so no black frame shows
      host.games.resume().catch(e => console.warn('resume', e)).finally(finishClose);
    } else finishClose();
  }, total + FRAME);
}

function finishClose() {
  if (!closing && !root.classList.contains('closing')) return;
  closing = null;
  cancel();
  root.classList.remove('closing', 'paneled');
  $.panel.classList.remove('on');
  $.blank.style.opacity = 0;
  $.still.style.opacity = 0;
  $.full.style.opacity = 0;
}

// ---------- bar ----------
function barX(i) {
  return Math.round(960 + BAR.pitch * (i - (ITEMS.length - 1) / 2));
}

// the running session's entry: a game, or a Media entry (same card and panel; Close Game ends either)
function runningGame() {
  const s = session.id ? session : warming ?? {};
  if (!s.id) return null;
  const list = s.kind === 'media' ? library.media : games();
  return list.find(g => g.id === s.id) ?? { id: s.id, name: s.id };
}

function lastPlayed() {
  return games().filter(g => g.lastPlayed).sort((a, b) => b.lastPlayed - a.lastPlayed)[0] ?? null;
}

function label(id) {
  if (id !== 'game') return LABELS[id];
  return runningGame()?.name ?? lastPlayed()?.name ?? 'Games';
}

function focusBar(i) {
  barIndex = Math.max(0, Math.min(ITEMS.length - 1, i));
  $.items.forEach((el, k) => {
    el.classList.toggle('focus', zone !== 'cards' && k === barIndex);
    el.querySelector('.label').textContent = k === barIndex ? label(ITEMS[k]) : '';
  });
  return true;
}

function barAction(name, e) {
  if (name === 'left' || name === 'right') {
    const i = barIndex + (name === 'left' ? -1 : 1);
    if (i < 0 || i >= ITEMS.length) return false;
    return focusBar(i);
  }
  if (e.held) return false;
  if (name === 'up') return cards.length ? focusCards(cardIndex) : false;
  if (name === 'a') return activate(ITEMS[barIndex]);
  if (name === 'b') { app.pop(); return 'back'; }
  return false;
}

function activate(id) {
  if (id === 'home') return goHome();
  openPanel(id);
  return true;
}

function goHome(focus) {
  if (app.stack.length === 2 && app.stack[0].id === 'home' && mode === 'home' && !focus) { app.pop(); return true; }
  leaving = 'stay';
  app.go('home', focus ? { focus } : {});
  return true;
}

// ---------- cards ----------
function buildCards() {
  const g = runningGame() ?? lastPlayed();
  const n = notices[0];
  cards = [
    { id: 'game', kicker: runningGame() ? 'Now playing' : g ? 'Last played' : 'Games', title: g?.name ?? 'No game played yet',
      image: g?.art?.background ?? g?.art?.tile, badge: g?.art?.tile, glyph: 'game' },
    { id: 'notifications', kicker: 'Notifications', title: n ? `${n.name} was added` : 'No new notifications', image: n?.tile, glyph: 'notifications' },
  ];
  cardIndex = Math.min(cardIndex, cards.length - 1);
  $.cards.replaceChildren(...cards.map((c, i) => {
    const el = document.createElement('div');
    el.className = 'card';
    el.dataset.nav = `cc:card:${i}`;
    el.innerHTML = '<div class="pic"></div><div class="badge"></div><div class="kicker"></div><div class="title"></div>';
    const pic = el.querySelector('.pic');
    if (c.image) pic.style.backgroundImage = `url("${host.fileUrl(c.image)}")`;
    else pic.classList.add('plain');
    const badge = el.querySelector('.badge');
    if (c.badge) badge.style.backgroundImage = `url("${host.fileUrl(c.badge)}")`;
    else badge.innerHTML = ICONS[c.glyph];
    el.querySelector('.kicker').textContent = c.kicker;
    el.querySelector('.title').textContent = c.title;
    return el;
  }));
  layoutCards(false);
}

// the current card is grown (432×520, bottom and left edges fixed); the rest follow at the PS5's 19 px gap
function layoutCards(animate, delay = 0) {
  let x = CARD.x;
  [...$.cards.children].forEach((el, i) => {
    const big = i === cardIndex;
    el.style.transition = animate ? `left ${OPEN.grow}ms ${EASE_OUT} ${delay}ms, width ${OPEN.grow}ms ${EASE_OUT} ${delay}ms, height ${OPEN.grow}ms ${EASE_OUT} ${delay}ms` : 'none';
    el.style.left = `${x}px`;
    el.style.width = `${big ? CARD.W : CARD.w}px`;
    el.style.height = `${big ? CARD.H : CARD.h}px`;
    el.classList.toggle('focus', zone === 'cards' && big);
    x += (big ? CARD.W : CARD.w) + CARD.gap;
  });
}

// on open the current card grows from the small size, as the PS5's does over ≈10 frames
function growCurrent(delay) {
  const el = $.cards.children[cardIndex];
  if (!el) return;
  el.style.transition = 'none';
  el.style.width = `${CARD.w}px`;
  el.style.height = `${CARD.h}px`;
  el.offsetWidth;
  layoutCards(true, delay);
}

function focusCards(i) {
  zone = 'cards';
  cardIndex = Math.max(0, Math.min(cards.length - 1, i));
  focusBar(barIndex);
  layoutCards(true);
  return true;
}

function cardsAction(name, e) {
  if (name === 'left' || name === 'right') {
    const i = cardIndex + (name === 'left' ? -1 : 1);
    if (i < 0 || i >= cards.length) return false;
    return focusCards(i);
  }
  if (e.held) return false;
  if (name === 'down') { zone = 'bar'; layoutCards(true); return focusBar(barIndex); }
  if (name === 'a') { openPanel(cards[cardIndex].id); return true; }
  if (name === 'b') { app.pop(); return 'back'; }
  return false;
}

// ---------- panels ----------
function panelWidth(id) {
  return id === 'sound' ? 780 : 646;
}

function openPanel(id, now = false) {
  zone = 'panel';
  panel = { id, focus: 0, ...content(id) };
  render();
  const i = ITEMS.indexOf(id);
  if (i >= 0) focusBar(i);
  const w = panelWidth(id);
  const cx = barX(i >= 0 ? i : barIndex);
  $.panel.style.width = `${w}px`;
  $.panel.style.left = `${Math.max(84, Math.min(1836 - w, cx - w / 2))}px`;
  $.panel.classList.add('on');
  root.classList.add('paneled');
  layoutCards(false);
  // the other icons dim and the cards fade, then the panel grows up from its bottom edge while fading in
  const wait = now ? FRAME : PANEL.wait;
  track($.cards.animate([{ opacity: opacityOf($.cards) }, { opacity: 0 }], { duration: PANEL.wait, easing: EASE_OUT, fill: 'forwards' }));
  track($.panel.animate([
    { opacity: 0, clipPath: 'inset(100% 0 0 0 round 12px)' },
    { opacity: 1, clipPath: 'inset(0 0 0 0 round 12px)' },
  ], { duration: PANEL.grow, delay: wait, easing: EASE_OUT, fill: 'both' }));
  window.__cc.panelAt = performance.now();
  return true;
}

function closePanel() {
  if (panel?.back) { const back = panel.back; panel = { ...back, focus: back.focus }; render(); return true; }
  if (powerOnly && panel?.id === 'power') { app.pop(); return 'back'; }
  panel = null;
  zone = 'bar';
  root.classList.remove('paneled');
  track($.panel.animate([{ opacity: opacityOf($.panel) }, { opacity: 0 }], { duration: PANEL.out, easing: EASE_OUT, fill: 'forwards' }));
  track($.cards.animate([{ opacity: opacityOf($.cards) }, { opacity: 1 }], { duration: PANEL.out, delay: PANEL.out, easing: EASE_OUT, fill: 'forwards' }));
  setTimeout(() => { if (!panel) $.panel.classList.remove('on'); }, PANEL.out);
  buildCards();
  return focusBar(barIndex);
}

// a sub-list or a confirm question in the same panel; B goes back to where it came from
function subPanel(p) {
  panel = { ...p, focus: p.focus ?? 0, back: { ...panel } };
  render();
  return true;
}

const row = (label, run, extra = {}) => ({ label, kind: 'action', run, ...extra });
const info = (label, value) => ({ label, value, kind: 'info' });
const text = label => ({ label, kind: 'text' });

function content(id) {
  const loading = [text('…')];
  switch (id) {
    case 'game': {
      const g = runningGame();
      if (g) {
        return { title: g.name, rows: [
          row('Back to game', () => backToGame()),
          row('Close Game', () => confirm(`Close ${g.name}?`, 'Unsaved progress will be lost.', 'Close Game', () => closeGame(g))),
        ] };
      }
      const last = lastPlayed();
      return { title: 'Last played', rows: last ? [row(last.name, () => goHome(last.id), { value: ago(last.lastPlayed) })] : [text('No game played yet')] };
    }
    case 'notifications':
      return { title: 'Notifications', rows: notices.length
        ? notices.slice(0, 6).map(n => row(`${n.name} was added`, () => goHome(n.id), { value: ago(n.at) }))
        : [text('No new notifications')] };
    case 'sound':
      loadSound();
      return { title: 'Sound', rows: loading };
    case 'controller':
      loadController();
      return { title: 'Controller', rows: loading };
    case 'network':
      loadNetwork();
      return { title: 'Network', rows: loading };
    case 'mouse':
      return { title: 'Mouse mode', rows: [
        row('Start mouse mode', () => startMouse()),
        text('The right stick moves the pointer.'),
        text('A clicks, X right-clicks.'),
        text('The left stick scrolls.'),
        text('B or the Guide button ends it.'),
      ] };
    case 'power':
      return { title: 'Power', rows: POWER.map(([action, label, question]) => row(label, () => confirm(question, null, label, () => power(action)))) };
    default:
      return { title: '', rows: [] };
  }
}

function confirm(question, note, yes, run) {
  return subPanel({ id: panel.id, title: question, rows: [...(note ? [text(note)] : []), row(yes, run), row('Cancel', () => closePanel())], focus: note ? 1 : 0 });
}

// fills a panel's rows once its data arrives, if that panel is still the open one
function fill(id, rows) {
  if (panel?.id !== id || panel.back) return;
  panel.rows = rows;
  panel.focus = Math.max(0, Math.min(panel.focus, rows.length - 1));
  render();
}

async function loadSound() {
  const [vol, outs] = await Promise.all([host.system.volume().catch(() => null), host.system.audioOutputs().catch(() => [])]);
  const cur = outs.find(o => o.isDefault);
  const volume = { label: 'Volume', kind: 'slider', level: vol?.level ?? 0, step: d => setVolume(d) };
  fill('sound', [
    volume,
    row('Output device', () => subPanel({ id: 'sound', title: 'Output device', rows: outs.map(o => row(o.name, () => setOutput(o), { check: o.isDefault })), focus: Math.max(0, outs.indexOf(cur)) }),
      { value: cur?.name ?? 'None' }),
  ]);
}

function setVolume(d) {
  const r = panel.rows[panel.focus];
  const level = Math.round(Math.max(0, Math.min(1, r.level + d * 0.05)) * 100) / 100;
  if (level === r.level) return false;
  r.level = level;
  render();
  host.system.setVolume(level).catch(e => console.warn('volume', e));
  return 'move';
}

async function setOutput(o) {
  try {
    await host.system.setAudioOutput(o.id);
  } catch (e) {
    toast({ title: 'Sound', message: 'That output could not be used.', icon: null });
  }
  closePanel();
  loadSound();
}

async function loadController() {
  const [pads, settings] = await Promise.all([host.system.battery().catch(() => []), host.settings.get().catch(() => ({}))]);
  const level = ['Empty', 'Low', 'Medium', 'Full'];
  // as Settings › Controllers says it: a 2.4 GHz receiver looks wired to XInput and reports no battery
  const connection = { wired: 'Wired (USB)', wireless: 'Wireless', receiver: 'Wireless (2.4 GHz receiver)' };
  const line = p => {
    const link = p.connection ?? (p.kind === 'wired' ? 'wired' : 'wireless');
    return link === 'wireless' ? `Wireless · Battery: ${level[p.level] ?? 'Unknown'}` : connection[link] ?? 'Unknown';
  };
  const rows = pads.length ? pads.map(p => info(`Controller ${p.slot + 1}`, line(p))) : [info('Controller', 'None connected')];
  rows.push({ label: 'Vibration', kind: 'toggle', on: settings.rumble !== false, run: () => toggleRumble() });
  fill('controller', rows);
}

function toggleRumble() {
  const r = panel.rows[panel.focus];
  r.on = !r.on;
  render();
  host.settings.set({ rumble: r.on }).catch(e => console.warn('rumble', e));
  return true;
}

async function loadNetwork() {
  const n = await host.system.network().catch(() => null);
  if (!n) return fill('network', [text('No network information')]);
  const kind = n.kind === 'wifi' ? 'Wi-Fi' : n.kind === 'ethernet' ? 'Ethernet' : 'None';
  fill('network', [
    info('Connection', n.connected ? `${kind}${n.ssid ? `: ${n.ssid}` : ''}` : 'Not connected'),
    ...(n.kind === 'wifi' && n.signal != null ? [info('Signal', `${n.signal} %`)] : []),
    ...(n.ip ? [info('IP address', n.ip)] : []),
    info('Wi-Fi', n.wifi === 'on' ? 'On' : n.wifi === 'off' ? 'Off' : 'Not available'),
  ]);
}

const focusable = r => r.kind !== 'text';

function render() {
  $.head.textContent = panel.title;
  $.rows.replaceChildren(...panel.rows.map((r, i) => {
    const el = document.createElement('div');
    el.className = `row ${r.kind}`;
    if (focusable(r)) el.dataset.nav = `cc:row:${i}`;
    el.innerHTML = '<span class="l"></span><span class="v"></span>';
    el.querySelector('.l').textContent = r.label;
    const v = el.querySelector('.v');
    if (r.kind === 'slider') {
      v.innerHTML = '<b></b><span class="track"><i></i><em></em></span>';
      v.querySelector('b').textContent = `${Math.round(r.level * 100)}`;
      v.querySelector('i').style.width = `${r.level * 100}%`;
      v.querySelector('em').style.left = `${r.level * 100}%`;
    } else if (r.kind === 'toggle') {
      v.innerHTML = `<span class="switch${r.on ? ' on' : ''}"><i></i></span>`;
    } else if (r.check) {
      v.textContent = '✓';
    } else v.textContent = r.value ?? '';
    el.classList.toggle('focus', i === panel.focus);
    return el;
  }));
  if (!focusable(panel.rows[panel.focus] ?? {})) panel.focus = panel.rows.findIndex(focusable);
  [...$.rows.children].forEach((el, i) => el.classList.toggle('focus', i === panel.focus));
}

function panelAction(name, e) {
  const r = panel.rows[panel.focus];
  if (name === 'up' || name === 'down') {
    const d = name === 'up' ? -1 : 1;
    let i = panel.focus + d;
    while (i >= 0 && i < panel.rows.length && !focusable(panel.rows[i])) i += d;
    if (i < 0 || i >= panel.rows.length) return false;
    panel.focus = i;
    [...$.rows.children].forEach((el, k) => el.classList.toggle('focus', k === i));
    return true;
  }
  if (name === 'left' || name === 'right') return r?.kind === 'slider' ? r.step(name === 'left' ? -1 : 1) : false;
  if (e.held) return false;
  if (name === 'a') return r?.run ? (r.run() ?? true) : false;
  if (name === 'b') return closePanel() && 'back';
  return false;
}

// ---------- actions ----------
function backToGame() {
  if (session.id) leaving = 'resume';
  app.pop();
  return true;
}

function closeGame(g) {
  host.games.close(g.id).catch(e => console.warn('close', e));
  return goHome(g.id);
}

function startMouse() {
  leaving = 'stay';
  app.pop();
  host.system.mouseMode(true).catch(e => console.warn('mouse mode', e));
  return true;
}

function power(action) {
  leaving = 'stay';
  app.pop();
  // after the close, so a dry run or a failed action leaves the screen tidy
  setTimeout(() => host.system.power(action).catch(e => toast({ title: 'Power', message: String(e).slice(0, 60), icon: 'glyph:power' })), CLOSE.out + CLOSE.undim);
  return true;
}

// ---------- mouse pointer ----------
function pointer(kind, e) {
  const [, what, arg] = (e.target ?? '').split(':');
  const i = +arg;
  const click = kind === 'click';
  if (what === 'bar') {
    if (zone === 'panel' && !click) return false;
    if (zone === 'panel') closePanel();
    zone = 'bar';
    layoutCards(true);
    focusBar(i);
    return click ? activate(ITEMS[i]) : true;
  }
  if (what === 'card' && zone !== 'panel') {
    if (i !== cardIndex || zone !== 'cards') focusCards(i);
    return click ? (openPanel(cards[i].id), true) : true;
  }
  if (what === 'row' && zone === 'panel' && panel.rows[i]) {
    panel.focus = i;
    [...$.rows.children].forEach((el, k) => el.classList.toggle('focus', k === i));
    return click ? panelAction('a', {}) : true;
  }
  return false;
}

// ---------- clock and text ----------
function tickClock() {
  $.clock.textContent = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', hourCycle: 'h23' });
  clearTimeout(clockTimer);
  clockTimer = setTimeout(tickClock, 60000 - (Date.now() % 60000) + 50);
}

function ago(t) {
  if (!t) return '';
  const ms = t < 1e12 ? t * 1000 : t;
  const min = Math.round((Date.now() - ms) / 60000);
  if (min < 1) return 'Just now';
  if (min < 60) return `${min} min ago`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  return d === 1 ? 'Yesterday' : `${d} days ago`;
}
