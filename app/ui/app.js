// Boot and screen router. Each screen is <id>/<id>.js plus <id>/<id>.css, loaded on first use. A screen exports:
//   mount(el, app)    once, before first use: build its DOM into el (a 1920×1080 layer)
//   enter(params)     it becomes the top screen (params from go/push; at boot, ?focus=)
//   leave()           it stops being the top screen
//   action(name, e)   an input action while on top (up down left right a b x y lb rb lt rt menu view, plus release
//                     point click wheel; e and holds: ui/input/input.js); truthy if used
// app.go(id, params) replaces the stack, app.push(id, params) opens a screen over the current one (overlays such as
// Control Center), app.pop() closes it. At boot ?scene=<id>&focus=<game id> jump to a state (used with the mock host).

import { host } from './host.js';
import { startInput } from './input/input.js';

const FIRST = 'start'; // the light show and "Press any button", which reveals Home (?scene= skips it)
const SCREENS = ['start', 'home', 'library', 'search', 'control-center', 'settings', 'media', 'journal', 'gallery', 'users', 'recap'];
// a screen whose files live in another screen's folder
const PATHS = { recap: 'journal/recap' };
const stage = document.getElementById('stage');
const loaded = new Map();
// the host hears the top screen when it changes: over a running game, that's where the session's time goes (row B1b)
let reported = null;
const report = () => {
  const id = app.top?.id;
  if (!id || id === reported) return;
  reported = id;
  Promise.resolve().then(() => host.games.screen(id)).catch(() => {});
};

// the design canvas is 1920×1080; scale it to the window (the Avita runs 1920×1080 at 150%, so 1280×720 CSS px)
function fit() {
  const s = Math.min(innerWidth / 1920, innerHeight / 1080);
  stage.style.transform = `translate(${(innerWidth - 1920 * s) / 2}px, ${(innerHeight - 1080 * s) / 2}px) scale(${s})`;
}

async function load(id) {
  if (loaded.has(id)) return loaded.get(id);
  if (!SCREENS.includes(id)) throw new Error(`no screen ${id}`);
  const path = PATHS[id] ?? `${id}/${id}`;
  const css = Object.assign(document.createElement('link'), { rel: 'stylesheet', href: `./${path}.css` });
  const cssReady = new Promise(done => { css.onload = css.onerror = done; });
  document.head.append(css);
  const [{ default: mod }] = await Promise.all([import(`./${path}.js`), cssReady]);
  const el = Object.assign(document.createElement('div'), { className: 'screen', id: `screen-${id}` });
  stage.append(el);
  const screen = { id, mod, el };
  loaded.set(id, screen);
  await mod.mount?.(el, app);
  return screen;
}

const app = {
  host,
  stack: [],
  get top() { return this.stack.at(-1); },
  async go(id, params = {}) {
    for (const s of this.stack.reverse()) { s.mod.leave?.(); s.el.classList.remove('on'); }
    this.stack = [];
    await this.push(id, params);
    report();
  },
  async push(id, params = {}) {
    const s = await load(id);
    this.top?.mod.leave?.();
    this.stack.push(s);
    s.el.classList.add('on');
    s.mod.enter?.(params);
    report();
  },
  pop() {
    if (this.stack.length < 2) return;
    const s = this.stack.pop();
    s.mod.leave?.();
    s.el.classList.remove('on');
    this.top.mod.enter?.({ back: true });
    report();
  },
};
window.__app = app;

addEventListener('resize', fit);
addEventListener('contextmenu', e => e.preventDefault());
fit();
startInput(host, app);

const query = new URLSearchParams(location.search);
await Promise.all(['300 36px "Open Sans"', '400 30px "Open Sans"', '600 30px "Open Sans"'].map(f => document.fonts.load(f)));
await app.go(query.get('scene') || FIRST, { focus: query.get('focus') });
document.documentElement.dataset.ready = '1';
// load the Library while idle: Home hides as it opens, so a first load would show black frames (row A8)
setTimeout(() => load('library').catch(e => console.warn('preload library', e)), 1500);
