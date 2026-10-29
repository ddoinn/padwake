// Settings (rows A9, A10): the PS5 Settings look (design/ps5/3-settings-a.md, 4-settings-b.md) over real PC settings.
// Pages are plain descriptions (pages.js, display/display.js, network/network.js); this file draws them, moves focus,
// runs the page cross-fade, dropdowns, dialogs and the on-screen keyboard. Rows and the focus light are list.js.
//
// A page: {title, layout, load?, rows? | cats? | mount?, events?, centred?}
//   layout 'root'   the root list (icons, ring x 298–1620)        rows: [row]
//          'two'    categories left, rows right (§1.5)            cats: [{label, rows: [row] | () => [row]}]
//          'full'   one centred list (4-settings-b §2.1)          rows
//          'info'   a read-only list; up / down scroll 48 px      rows
//          'custom' mount(el, ctx) → {action(name, e), leave()}
//   load()  async, before the page shows (data from the host); events: {hostEvent: fn} while the page is open.
// Rows are list.js rows. ctx (given to page factories): host, open(page), back(), dialog(opts), type(opts), update().
//
// Motion (3-settings-a.md §3; timings re-measured from the video, see PLAN row A9):
//   page change   A: ring out in 3 frames + flash; 150 ms later (100 ms on B) the page shrinks to 0.97 and fades in
//                 10 frames about the pressed row's centre; 100 ms after that starts, the new page grows from 0.97
//                 (21 frames) and fades in (17 frames). The title cross-fades in place; the background never changes.
//   category      the right column is cut, stays empty 100 ms, fades in over 200 ms (row 11)
//   column        categories dim to 63 % over 150 ms when focus goes right (row 12)
//   dropdown      ring off; page to 60 % and background to 32 % over 150 ms; options, then panel, then the focused
//                 option fade in (row 18, 217 ms). Close: panel gone in 1 frame, undim 183 ms, ring returns (row 19).

import * as keyboard from '../keyboard/keyboard.js';
import { List, GEOM, FRAME, focusable } from './list.js';
import { CHECK } from './icons.js';
import { root } from './pages.js';
import { mount as spotlight } from '../background/spotlight.js';

// a timer plus an animation's first (progress 0) frame add ≈2 frames, so the waits are 2 frames short of the measured
// 150 ms (press → first outgoing frame) and 100 ms (B); the incoming starts 7 frames after the outgoing
const PRESS_TO_OUT = 7 * 1000 / 60, BACK_TO_OUT = 4 * 1000 / 60, IN_AFTER = 7 * 1000 / 60;
const OUT_MS = 10 * FRAME, IN_FADE_MS = 17 * FRAME, IN_SCALE_MS = 21 * FRAME, SHRINK = 0.97;
const OUT_FADE = 'linear(0, .29, .49, .64, .74, .82, .87, .92, .94, .98, 1)';
const OUT_SCALE = 'cubic-bezier(.33, 1, .68, 1)';
const IN_FADE = 'linear(0, .13, .25, .36, .47, .57, .64, .72, .78, .82, .87, .9, .94, .96, .97, .98, .99, 1)';
const IN_SCALE = 'linear(0, .133, .267, .367, .467, .533, .6, .667, .733, .783, .833, .867, .9, .913, .933, .95, .967, .973, .983, .99, .997, 1)';
const RING_AFTER_IN = IN_AFTER + 200; // the new page's ring fades in once its content is mostly in (4-settings-b §3)
const VOLUME_STEP = 0.02;

let app, host, el, bgEl, pagesEl;
let stack = [];        // page views; the last is on screen
let overlay = null;    // the open dropdown or dialog: {action(name, e)}
let kb = null;         // the open on-screen keyboard
let busy = false;      // between a press and its page change
let rootFocus = 0;
const lists = new WeakMap(); // list element → List, for the mouse
// a display-mode change moves the pointer, which reads as hover; ignore hover for a moment after a resize so focus
// stays where the pad put it (the Keep button of the display check)
let resizedAt = -1e9;
addEventListener('resize', () => { resizedAt = performance.now(); });

const wait = ms => new Promise(r => setTimeout(r, ms));
const read = v => (typeof v === 'function' ? v() : v);
const top = () => stack.at(-1);

const ctx = {
  get host() { return host; },
  get app() { return app; },
  open: (factory, origin) => push(factory(ctx), origin),
  back: () => back(),
  dialog: opts => dialog(opts),
  type: opts => {
    kb = keyboard.open({ ...opts, parent: el });
    const done = kb.done;
    done.finally(() => { kb = null; });
    return done;
  },
  update: () => top()?.refresh(),
};

export default {
  mount(screenEl, a) {
    app = a;
    host = a.host;
    el = screenEl;
    el.innerHTML = '<div class="spages"></div>';
    bgEl = spotlight(el); // the living spotlight background (background/spotlight.js); the dims fade it (settings.css)
    pagesEl = el.querySelector('.spages');
    window.__settings = { ctx, get stack() { return stack; }, get overlay() { return overlay; }, get kb() { return kb; } };
  },

  async enter(params = {}) {
    if (params.back && stack.length) return;
    closeAll();
    stack.forEach(v => v.close());
    stack = [];
    // opening from Home (row 1): the background fades in, then the title and list, then the ring on the remembered row
    bgEl.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 250, easing: 'ease-out' });
    const view = await makeView(root(ctx));
    view.list.focus(rootFocus);
    stack.push(view);
    pagesEl.append(view.el);
    view.el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 10 * FRAME, delay: 250, easing: 'ease-out', fill: 'backwards' });
    view.enter(null, 250 + 10 * FRAME);
    const path = new URLSearchParams(location.search).get('settings');
    if (path && host.isMock) await openPath(path); // test hook: ?settings=sound/1 opens a page and a category
  },

  leave() {
    closeAll();
    if (stack[0]) rootFocus = stack[0].list.index;
  },

  action(name, e = {}) {
    if (name === 'point' && performance.now() - resizedAt < 1500) return 'none';
    if (kb?.isOpen) return kb.action(name, e);
    if (overlay) return overlay.action(name, e);
    if (busy) return 'none';
    const v = top();
    if (!v) return false;
    if (name === 'point' || name === 'click') return pointer(v, name, e);
    if (name === 'wheel') return e.dy ? v.action(e.dy > 0 ? 'down' : 'up', e) : 'none';
    return v.action(name, e);
  },
};

function closeAll() {
  kb?.close();
  kb = null;
  overlay?.close();
  overlay = null;
  busy = false;
}

// ---------- pages ----------
async function makeView(def) {
  const v = def.layout === 'two' ? new TwoColumns(def) : def.layout === 'custom' ? new Custom(def) : new OneList(def);
  await def.load?.();
  v.build();
  return v;
}

class View {
  constructor(def) {
    this.def = def;
    this.el = document.createElement('div');
    this.el.className = `spage${def.centred ? ' centred' : ''}`;
    this.el.innerHTML = '<div class="stitle"></div><div class="scontent"></div>';
    this.el.querySelector('.stitle').textContent = def.title;
    this.content = this.el.querySelector('.scontent');
    this.offs = Object.entries(def.events ?? {}).map(([name, fn]) => host.on(name, fn));
  }
  close() {
    this.offs.forEach(off => off());
    this.def.leave?.();
    this.el.remove();
  }
  refresh() {}
  // the scale origin of a page change: the focused row's centre (measured: the pressed row, or the row regaining focus)
  origin() {
    const l = this.focusList();
    if (!l) return [960, 540];
    return [(l.g.x + l.g.r) / 2, l.rowRect().centre];
  }
  focusList() { return this.list; }
}

class OneList extends View {
  build() {
    const g = this.def.layout === 'root' ? GEOM.root : this.def.layout === 'info' ? GEOM.info : GEOM.full;
    this.list = new List(this.content, g, read(this.def.rows));
    lists.set(this.list.el, this.list);
    if (this.def.layout === 'info') this.list.el.classList.add('info');
  }
  enter(dir, ringAt) {
    if (this.def.layout === 'info') return this.list.showBar();
    this.list.setActive(true, dir, ringAt);
  }
  refresh() { this.list.refresh(); }
  action(name, e) {
    const l = this.list;
    if (this.def.layout === 'info') {
      if (name === 'up' || name === 'down') return l.scrollBy(name === 'down' ? 48 : -48) || (e.held ? false : 'none');
      return name === 'b' ? back() : 'none';
    }
    if (name === 'up' || name === 'down') return l.move(name === 'down' ? 1 : -1, e) || (e.held ? false : 'none');
    if (name === 'b') return back();
    return rowAction(l, name, e);
  }
}

class TwoColumns extends View {
  build() {
    this.cat = 0;
    this.col = 'left';
    this.left = new List(this.content, GEOM.left, this.def.cats.map(c => ({ kind: 'cat', label: c.label })));
    this.left.el.classList.add('left');
    this.right = new List(this.content, GEOM.right, this.rowsOf(0));
    this.right.el.classList.add('right');
    lists.set(this.left.el, this.left);
    lists.set(this.right.el, this.right);
    this.mark();
    if (this.def.startCat) {
      this.left.focus(this.def.startCat);
      this.setCat(this.def.startCat, false);
    }
  }
  get list() { return this.col === 'left' ? this.left : this.right; }
  focusList() { return this.list; }
  rowsOf(i) { return read(this.def.cats[i].rows) ?? []; }
  mark() { this.left.els.forEach((r, i) => r.classList.toggle('sel', i === this.cat)); }
  enter(dir, ringAt) { this.list.setActive(true, dir, ringAt); }
  refresh() {
    this.left.refresh();
    const rows = this.rowsOf(this.cat);
    if (rows.length !== this.right.rows.length || rows.some((r, i) => (r.key ?? r.label) !== (this.right.rows[i].key ?? this.right.rows[i].label))) {
      this.right.setRows(rows);
    } else {
      this.right.rows = rows;
      this.right.refresh();
    }
  }
  // row 11: the right column is cut, stays empty ≈100 ms, then fades in over 200 ms
  setCat(i, animate = true) {
    this.cat = i;
    this.mark();
    this.right.setRows(this.rowsOf(i), false);
    this.right.scroll = 0;
    this.right.scrollTo(false);
    this.def.cats[i].enter?.();
    if (animate) this.right.el.animate([{ opacity: 0 }, { opacity: 0, offset: 1 / 3 }, { opacity: 1 }], { duration: 300, easing: 'linear' });
  }
  goRight() {
    if (!this.right.rows.some(focusable)) return 'none';
    this.col = 'right';
    this.left.setActive(false);
    this.left.el.classList.add('away');
    this.right.setActive(true, null);
    return true;
  }
  goLeft() {
    this.col = 'left';
    this.right.setActive(false);
    this.left.el.classList.remove('away');
    this.left.setActive(true, null);
    return true;
  }
  action(name, e) {
    if (this.col === 'left') {
      if (name === 'up' || name === 'down') {
        if (!this.left.move(name === 'down' ? 1 : -1, e)) return e.held ? false : 'none';
        this.setCat(this.left.index);
        return true;
      }
      if (name === 'right' || name === 'a') return this.goRight();
      if (name === 'b') return back();
      return 'none';
    }
    if (name === 'up' || name === 'down') return this.right.move(name === 'down' ? 1 : -1, e) || (e.held ? false : 'none');
    if (name === 'b' || (name === 'left' && this.right.row.kind !== 'slider')) return this.goLeft();
    return rowAction(this.right, name, e);
  }
}

class Custom extends View {
  build() { this.page = this.def.mount(this.content, ctx); }
  enter() {}
  close() { this.page?.leave?.(); super.close(); }
  action(name, e) { return this.page.action(name, e) ?? 'none'; }
}

// A, X and left / right on the focused row of list l
function rowAction(l, name, e) {
  const row = l.row, i = l.index;
  if (!row) return 'none';
  if (row.kind === 'slider' && (name === 'left' || name === 'right')) {
    if (read(row.disabled)) return 'none';
    const step = row.step ?? VOLUME_STEP;
    const now = read(row.get) ?? 0;
    const next = Math.round(Math.min(1, Math.max(0, now + (name === 'right' ? step : -step))) * 1000) / 1000;
    if (next === now) return e.held ? false : 'none';
    row.set(next);
    l.refresh();
    return true;
  }
  if (name !== 'a') return 'none';
  if (read(row.disabled) && row.kind !== 'link') return 'none';
  switch (row.kind) {
    case 'link':
      l.press();
      return openFrom(row.open, l, i);
    case 'toggle': {
      const v = !read(row.get);
      Promise.resolve(row.set(v)).catch(err => dialog({ title: `${read(row.label)} didn't change`, text: String(err) }))
        .finally(() => l.refresh());
      l.els[i].querySelector('.tog')?.classList.toggle('on', v);
      return true;
    }
    case 'select':
      openDropdown(l, i);
      return true;
    case 'action':
      l.press();
      Promise.resolve(row.run(ctx, l)).catch(err => dialog({ title: 'Something went wrong', text: String(err?.message ?? err) }))
        .finally(() => { l.refresh(); if (l.active && !overlay && !kb) l.arrive(null, 0); });
      return true;
    default:
      return 'none';
  }
}

async function openFrom(factory, l, i) {
  busy = true;
  const origin = [(l.g.x + l.g.r) / 2, l.rowRect(i).centre];
  try {
    await push(factory(ctx), origin, PRESS_TO_OUT);
  } finally {
    busy = false;
  }
  return true;
}

// a new page over the current one: the outgoing page shrinks and fades, then the new one grows in
async function push(def, origin = top()?.origin() ?? [960, 540], delay = 0) {
  const from = top();
  const [view] = await Promise.all([makeView(def), wait(delay)]);
  if (from && stack.at(-1) !== from) { view.close(); return; } // something else changed the page meanwhile
  if (from?.def.layout === 'root') rootFocus = from.list.index;
  stack.push(view);
  pagesEl.append(view.el);
  swap(from, view, origin);
  view.enter(null, RING_AFTER_IN);
}

function back() {
  if (stack.length < 2) {
    if (stack[0]) rootFocus = stack[0].list.index;
    if (app.stack.length > 1) app.pop(); else app.go('home');
    return true;
  }
  busy = true;
  setTimeout(() => {
    busy = false;
    const from = stack.pop();
    const to = top();
    to.el.hidden = false;
    swap(from, to, to.origin(), () => from.close());
    to.refresh();
    if (to.list?.active) to.list.arrive(null, RING_AFTER_IN);
  }, BACK_TO_OUT);
  return true;
}

function swap(from, to, [ox, oy], done) {
  const origin = `${ox}px ${oy}px`;
  if (from) {
    from.content.style.transformOrigin = origin;
    from.el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: OUT_MS, easing: OUT_FADE, fill: 'forwards' });
    from.content.animate([{ transform: 'scale(1)' }, { transform: `scale(${SHRINK})` }], { duration: OUT_MS, easing: OUT_SCALE, fill: 'forwards' })
      .finished.then(() => {
        from.el.getAnimations({ subtree: true }).forEach(a => { if (a.effect?.target === from.el || a.effect?.target === from.content) a.cancel(); });
        from.el.hidden = true;
        done?.();
      }, () => {});
  }
  to.content.style.transformOrigin = origin;
  to.el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: IN_FADE_MS, delay: from ? IN_AFTER : 0, easing: IN_FADE, fill: 'backwards' });
  to.content.animate([{ transform: `scale(${SHRINK})` }, { transform: 'scale(1)' }], { duration: IN_SCALE_MS, delay: from ? IN_AFTER : 0, easing: IN_SCALE, fill: 'backwards' });
}

// test hook (mock host only): ?settings=sound opens Sound, sound/1 also picks its second category, sound/1/r focuses
// the right column, sound/1/r/2 its third row
async function openPath(path) {
  const [page, cat, col, row] = path.split('/');
  const r = stack[0].list.rows.findIndex(x => x.label.toLowerCase() === page);
  if (r < 0) return;
  stack[0].list.focus(r);
  const def = stack[0].list.rows[r].open(ctx);
  if (cat) def.startCat = +cat;
  const view = await makeView(def);
  stack[0].el.hidden = true;
  stack.push(view);
  pagesEl.append(view.el);
  if (col === 'r' && view.goRight) {
    view.left.setActive(false);
    view.col = 'right';
    view.left.el.classList.add('away');
    if (row) view.right.index = +row;
    view.right.setActive(true, null, 0);
  } else view.enter(null, 0);
}

// ---------- mouse: hover focuses a row, a click acts ----------
function pointer(v, name, e) {
  const m = /^row:(\d+)$/.exec(e.target ?? '');
  const l = m && lists.get(e.el.closest('.slist'));
  if (!l || !focusable(l.rows[+m[1]])) return 'none';
  if (v instanceof TwoColumns) {
    if (l === v.left && v.col === 'right') v.goLeft();
    if (l === v.right && v.col === 'left') v.goRight();
    if (l === v.left && +m[1] !== v.cat) { l.focus(+m[1]); v.setCat(+m[1]); }
  }
  if (l.index !== +m[1]) l.focus(+m[1], null);
  if (name === 'click' && l !== v.left) return rowAction(l, 'a', e);
  return 'none';
}

// ---------- dropdown (§1.10, rows 18 and 19) ----------
const measureCtx = document.createElement('canvas').getContext('2d');
function textWidth(text) {
  measureCtx.font = '300 31px "Open Sans"';
  return measureCtx.measureText(text).width;
}

function openDropdown(l, i) {
  const row = l.rows[i];
  const options = read(row.options) ?? [];
  if (!options.length) return;
  const checked = Math.max(0, options.findIndex(o => o.value === read(row.get)));
  const MAX = 11, PITCH = 72;
  const n = options.length, vis = Math.min(n, MAX);
  // the checked option sits on its row's label line; the panel extends up or down from there and stays on screen
  const w = Math.ceil(78 + Math.max(...options.map(o => textWidth(o.label))) + 56);
  const right = l.g.dd ?? l.g.r - 9, left = right - w;
  let start = Math.min(Math.max(0, checked - Math.floor(vis / 2)), n - vis); // first visible option
  if (n <= MAX) start = 0;
  const capC = l.rowRect(i).capC;
  // the panel's edges sit 42 px beyond the first and last option centres (t0306.00: 389–617 for 3 options on 431)
  let panelTop = capC - (checked - start) * PITCH - 42;
  const h = (vis - 1) * PITCH + 84;
  panelTop = Math.max(56, Math.min(panelTop, 1030 - h));

  l.press();
  l.els[i].classList.add('owner');
  el.classList.add('dim');
  const dd = document.createElement('div');
  dd.className = 'dd';
  dd.style.cssText = `left:${left}px;top:${panelTop}px;width:${w}px;height:${h}px`;
  dd.innerHTML = '<div class="dd-panel"></div><div class="dd-opts"><div class="dd-list"></div><div class="dd-ring"><i></i></div></div>';
  const listEl = dd.querySelector('.dd-list');
  const ringEl = dd.querySelector('.dd-ring');
  listEl.append(...options.map((o, k) => {
    const opt = document.createElement('div');
    opt.className = 'dd-opt';
    opt.style.top = `${k * PITCH}px`;
    opt.dataset.nav = `opt:${k}`;
    opt.innerHTML = `${k === checked ? CHECK : ''}<span></span>`;
    opt.querySelector('span').textContent = o.label;
    return opt;
  }));
  el.append(dd);
  const panel = dd.querySelector('.dd-panel');
  const optsEl = listEl;
  // row 18: options fade in at their places (+1–5 frames), then the panel (+5–11), then the focused option (+9–13)
  optsEl.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 4 * FRAME, delay: FRAME, fill: 'backwards' });
  panel.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 6 * FRAME, delay: 5 * FRAME, fill: 'backwards' });

  let focus = checked;
  const place = (animate, first) => {
    if (focus < start) start = focus;
    if (focus >= start + vis) start = focus - vis + 1;
    // option k's centre is k * 72 + 36 in the list and (k - start) * 72 + 42 in the panel
    listEl.style.transform = `translateY(${6 - start * PITCH}px)`;
    ringEl.style.transform = `translateY(${(focus - start) * PITCH + 42 - 41.5}px)`;
    ringEl.getAnimations({ subtree: true }).forEach(a => a.cancel());
    if (first) {
      ringEl.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 4 * FRAME, delay: 9 * FRAME, fill: 'both' });
      ringEl.firstChild.animate([{ opacity: 0.12 }, { opacity: 0 }], { duration: 300, delay: 9 * FRAME, easing: 'ease-out', fill: 'both' });
    } else if (animate) {
      ringEl.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 6 * FRAME, easing: 'ease-out', fill: 'both' });
    } else ringEl.style.opacity = '1';
  };
  place(false, true);

  return new Promise(resolve => {
    const close = value => {
      overlay = null;
      dd.remove();                           // row 19: the panel goes in one frame
      el.classList.remove('dim');            // the page undims over 183 ms
      l.els[i].classList.remove('owner');
      l.arrive('down', 9 * FRAME);           // and the ring comes back through the streak
      resolve(value);
    };
    overlay = {
      close: () => close(undefined),
      action(name, e = {}) {
        if (name === 'up' || name === 'down') {
          const f = focus + (name === 'down' ? 1 : -1);
          if (f < 0 || f >= n) return e.held ? false : 'none';
          focus = f;
          place(true);
          return true;
        }
        if (name === 'point' || name === 'click') {
          const m = /^opt:(\d+)$/.exec(e.target ?? '');
          if (!m) return 'none';
          if (+m[1] !== focus) { focus = +m[1]; place(true); }
          if (name === 'point') return 'none';
          name = 'a';
        }
        if (name === 'a') {
          const o = options[focus];
          close(o.value);
          if (o.value !== read(row.get)) {
            Promise.resolve(row.set(o.value)).catch(err => dialog({ title: `${read(row.label)} didn't change`, text: String(err?.message ?? err) }))
              .finally(() => l.refresh());
          }
          return true;
        }
        if (name === 'b') { close(undefined); return true; }
        return 'none';
      },
    };
  });
}

// ---------- dialogs: a centred question with pill buttons ----------
// opts: {title, text (string, or fn of the seconds left), buttons: [{label, value}], focus, countdown (s), until}
// Resolves with the chosen button's value, null for B, 'timeout' when the countdown ends, or what `until` resolves to.
export function dialog({ title, text = '', buttons = [{ label: 'OK', value: true }], focus = 0, countdown = 0, until } = {}) {
  overlay?.close();
  const d = document.createElement('div');
  d.className = 'sdlg';
  d.innerHTML = '<h2></h2><p></p><div class="btns"></div>';
  d.querySelector('h2').textContent = title;
  const p = d.querySelector('p');
  const btns = buttons.map((b, k) => {
    const e = document.createElement('div');
    e.className = 'pill';
    e.dataset.nav = `btn:${k}`;
    e.textContent = b.label;
    return e;
  });
  d.querySelector('.btns').append(...btns);
  el.append(d);
  el.classList.add('dlg');
  d.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 150, easing: 'ease-out' });
  let left = countdown;
  const paint = () => {
    p.textContent = typeof text === 'function' ? text(left) : text;
    btns.forEach((b, k) => b.classList.toggle('focus', k === focus));
  };
  paint();
  return new Promise(resolve => {
    let timer = 0;
    const close = value => {
      if (overlay !== api) return;
      clearInterval(timer);
      overlay = null;
      el.classList.remove('dlg');
      d.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 120, easing: 'ease-out' }).finished.then(() => d.remove(), () => d.remove());
      const l = top()?.list;
      if (l?.active) l.arrive(null, 6 * FRAME);
      resolve(value);
    };
    if (countdown) {
      timer = setInterval(() => {
        left = Math.max(0, left - 1);
        paint();
        if (left === 0) close('timeout');
      }, 1000);
    }
    until?.then(v => close(v));
    const api = overlay = {
      close: () => close(null),
      action(name, e = {}) {
        if (name === 'left' || name === 'right') {
          const f = focus + (name === 'right' ? 1 : -1);
          if (f < 0 || f >= btns.length) return 'none';
          focus = f;
          paint();
          return true;
        }
        if (name === 'point' || name === 'click') {
          const m = /^btn:(\d+)$/.exec(e.target ?? '');
          if (!m) return 'none';
          focus = +m[1];
          paint();
          if (name === 'point') return 'none';
          name = 'a';
        }
        if (name === 'a') { close(buttons[focus].value); return true; }
        if (name === 'b') { close(null); return true; }
        return 'none';
      },
    };
    top()?.list?.ringOff();
  });
}
