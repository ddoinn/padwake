// What Control Center knows before it first opens, kept from boot (Home imports this module):
// - notifications: games added, from `library` events (plus the last three days' additions at start, without toasts)
// - the running session, from `game` events, and whether the app just woke over it after Guide (`over`)
// - the toasts: "<game> / Was added to your library." and "Ready to play." when the app shows over a game that came up;
//   "System Software" when an update is ready, and after the app was updated
// Control Center reads `notices`, `session` and `takeOver()`.

import { host } from '../host.js';
import { toast } from './toast.js';

const DAY = 864e5;
export const notices = [];                 // newest first: {id, name, tile, at}
export const session = { id: null, kind: null, over: null, still: null };
const listeners = new Set();
export const onNotices = fn => (listeners.add(fn), () => listeners.delete(fn));

let known = null; // library ids seen so far; null until the first list

function seen(list) {
  known = new Set(list.map(g => g.id));
  const recent = list.filter(g => g.addedAt && Date.now() - g.addedAt < 3 * DAY).sort((a, b) => b.addedAt - a.addedAt);
  for (const g of recent.slice(0, 6)) notices.push({ id: g.id, name: g.name, tile: g.art?.tile ?? null, at: g.addedAt });
}

function added(list = []) {
  if (!known) return seen(list);
  const added = list.filter(g => !known.has(g.id));
  known = new Set(list.map(g => g.id));
  if (!added.length) return;
  for (const g of added) notices.unshift({ id: g.id, name: g.name, tile: g.art?.tile ?? null, at: Date.now() });
  notices.splice(20);
  if (added.length <= 2) {
    for (const g of added) toast({ title: g.name, message: 'Was added to your library.', icon: g.art?.tile ? host.fileUrl(g.art.tile) : 'glyph:library' });
  } else {
    toast({ title: 'Game Library', message: `${added.length} games were added.`, icon: 'glyph:library' });
  }
  listeners.forEach(fn => fn());
}

function game({ id, kind, state, show }) {
  if (state === 'launching' || state === 'running') Object.assign(session, { id, kind });
  if (state === 'over') {
    session.over = { show, at: performance.now(), wall: Date.now() };
    // Control Center over the game: fetch the still now (the host sends this before the window shows): half size
    // first, then full size
    session.still = show === 'control-center' ? still('small') : null;
    // the full size only once the window is up and Control Center has opened: 8 MB queued ahead of over_ready would
    // hold the window back
    const staged = session.still?.then(preStill);
    session.stillFull = staged?.then(() => new Promise(r => setTimeout(r, 300))).then(() => still('full')) ?? null;
  }
  if (state === 'ready' && document.visibilityState === 'visible') {
    // the app shows over the game as it came up (with the app asleep, the host shows this toast in its own window)
    const g = kind === 'media' ? library.media.find(m => m.id === id) : gameById(id);
    toast({ title: g?.name ?? id, message: 'Ready to play.', icon: g?.art?.tile ? host.fileUrl(g.art.tile) : null });
  }
  if (state === 'exited') { dropPreStill(); Object.assign(session, { id: null, kind: null, over: null, still: null, stillFull: null }); }
}

// the library as last sent, and the Media entries (row A12: {id, name, art}), for Control Center (it may open before
// its own list call returns)
export const library = { games: [], media: [] };
const mediaList = () => host.media?.list?.().then(list => { library.media = list ?? []; }).catch(() => {});
const gameById = id => library.games.find(g => g.id === id);

// the still the host took as Guide was pressed, as an object URL (null without one)
async function still(size) {
  try {
    const buf = await host.games.backdrop(size);
    if (size === 'small') session.stillFetched = Date.now();
    return buf ? URL.createObjectURL(new Blob([buf], { type: buf.type || 'image/bmp' })) : null;
  } catch {
    return null;
  }
}

// The window is still hidden: decode the still and put it on the stage above every screen, then tell the host it may
// show the window. Its first frame is then the game, not black; Control Center adopts this image as it opens.
async function preStill(url) {
  dropPreStill();
  if (url) {
    const img = Object.assign(new Image(), { src: url, alt: '' });
    await img.decode().catch(() => {});
    img.className = 'pre-still';
    img.style.cssText = 'position:absolute;left:0;top:0;width:1920px;height:1080px;object-fit:fill;z-index:30';
    document.getElementById('stage')?.append(img);
    session.preStill = img;
    session.preStillAt = Date.now();
    // if Control Center never takes it, it must not cover the screen
    setTimeout(() => { if (session.preStill === img) dropPreStill(); }, 2000);
  }
  await host.games.overReady?.().catch(() => {});
}

export function dropPreStill() {
  session.preStill?.remove();
  session.preStill = null;
}

/// The pending "Guide over the game" for Control Center, once: {show, at, still, full} (still and full: Promises of an
/// object URL or null) or null.
export function takeOver() {
  const o = session.over;
  session.over = null;
  if (!o || o.show !== 'control-center' || performance.now() - o.at > 3000) return null;
  return { ...o, still: session.still ?? Promise.resolve(null), full: session.stillFull ?? Promise.resolve(null) };
}

// Control Center loads on first use; over a game that first use is the Guide press itself, so its module and styles
// are loaded ahead: soon after boot, and again as a game launches (the page sleeps right after)
let warmed = false;
function warm() {
  if (warmed) return;
  warmed = true;
  const href = './control-center/control-center.css';
  if (![...document.styleSheets].some(s => s.href?.endsWith('/control-center/control-center.css'))) {
    document.head.append(Object.assign(document.createElement('link'), { rel: 'stylesheet', href }));
  }
  import('./control-center.js').catch(() => { warmed = false; });
}
setTimeout(warm, 3000);
host.on('game', ({ state }) => { if (state === 'launching') warm(); });

// Control Center's first open over a game rasters its layers for the first time, and the GPU process compiles the
// shader programs for them (the cards, the scrims): 250–330 ms with every frame held, so the first Guide tap after a
// start showed the bar at ≈650 ms instead of ≈300 (row A7; Chromium trace: RasterDecoderImpl::DoEndRasterCHROMIUM
// waiting on ANGLE's shader compiles). The programs stay compiled for the life of the GPU process.
// Home's launch transition calls this while the screen holds still (the UI is out, the art waits for the navy):
// Control Center draws at a hair's opacity exactly as it will over this game (its card, the notifications, the
// scrims), held until its frames run smooth again, so the raster and the compiles are done. Once per kind of card
// (with or without art) per start. Resolves when it is popped.
const warmedCards = new Set();
export function warmControlCenter(app, g) {
  const key = `${!!(g?.art?.background ?? g?.art?.tile)}|${!!g?.art?.tile}|${!!notices[0]?.tile}`;
  if (!g || warmedCards.has(key) || app.top?.id !== 'home') return Promise.resolve();
  warmedCards.add(key);
  return app.push('control-center', { warm: { id: g.id, kind: 'game' } })
    .then(() => new Promise(done => {
      // at least 6 frames (the raster runs 2–4 frames in), then 3 in a row under 24 ms; 40 frames at most
      let n = 0, smooth = 0, last = performance.now();
      const frame = t => {
        smooth = t - last < 24 ? smooth + 1 : 0;
        last = t;
        if ((++n >= 6 && smooth >= 3) || n >= 40) done();
        else requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
    }))
    .catch(() => warmedCards.delete(key))
    .finally(() => { if (app.top?.id === 'control-center' && app.top.el.classList.contains('warm')) app.pop(); });
}

// updates (cloud area): a toast once a new version has downloaded, and on the first start after an update
let updateState = null;
host.on('update', u => {
  if (u.state === 'ready' && updateState !== 'ready') {
    toast({ title: 'System Software', message: `Version ${u.latest} is ready to install.`, icon: 'glyph:update' });
  }
  updateState = u.state;
});
// (a start installs a waiting update, so one that is ready now came in while the start screen showed: toast it too)
host.cloud?.update().then(u => {
  const was = updateState;
  updateState = u.state;
  if (u.updatedFrom) setTimeout(() => toast({ title: 'System Software', message: `Updated to version ${u.current}.`, icon: 'glyph:update' }), 1500);
  else if (u.state === 'ready' && was !== 'ready') toast({ title: 'System Software', message: `Version ${u.latest} is ready to install.`, icon: 'glyph:update' });
}).catch(() => {});

host.on('library', list => { library.games = list ?? []; added(library.games); });
host.on('game', game);
host.on('game', ({ kind, state }) => { if (kind === 'media' && state === 'launching') mediaList(); });
mediaList();
host.library.list().then(list => { if (!known) { library.games = list ?? []; seen(library.games); } }).catch(() => {});
host.games.session?.().then(s => { if (s && !session.id) Object.assign(session, { id: s.id, kind: s.kind }); }).catch(() => {});
