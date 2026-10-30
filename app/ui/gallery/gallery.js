// Media Gallery (row A17): the screenshots taken with View while a game ran (host.games.captures), newest first. View
// on Home opens it over Home (app.push); B goes back to where Home was.
// Layout (stage px), as the Game Library: the glyph at (68, 70), "Media Gallery" x 172 cap 77; "All: N" x 172 cap 288;
// 16:9 thumbnails four across, 376 × 212 at x 172 + 400 c, y 338 + 236 r; the focus ring 3 px, 3 px outside, with
// the game and the date on a shade inside the focused thumbnail. Rows scroll so the focused one stays in view.
// A shows the screenshot full screen: it grows out of its thumbnail (the hub viewer's motion), Left / Right and
// LB / RB step, B shrinks it back. X or ☰ deletes (asks first). LT / RT jump two rows.
// Only rows near the view have elements; thumbnails fade in once decoded. Nothing animates while idle.

const X0 = 172, Y0 = 338, COLS = 4, W = 376, H = 212, PX = 400, PY = 236, VIEW_TOP = 260, PAGE_ROWS = 2;
const FRAME = 1000 / 60;
const EASE = 'cubic-bezier(0.20, 0.80, 0.45, 1)'; // Settings' expand curve
const EASE_OUT = 'cubic-bezier(0, 0, .2, 1)';
const GROW_MS = 13 * FRAME, STEP_MS = 10 * FRAME, SHIFT = 96, HEAD_MS = 2500, HELD_MS = 160;
const ICON = '<svg viewBox="0 0 41 36" fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">' +
  '<rect x="2" y="7" width="31" height="23" rx="3"/><path d="M7 25l7-8 5 5 4-4 6 7"/><circle cx="24.5" cy="14" r="2.4"/>' +
  '<path d="M8 3h28a3 3 0 0 1 3 3v19"/></svg>';

let app, host, root, $ = {};
let caps = [];          // [{game, name, path, thumb, takenAt}]
let index = 0, scrollY = 0, onTop = false;
let view = null;        // the full-screen viewer: {i, anims, stepAt, closing}
let dlg = null;         // the delete question: {focus, path, fromViewer}
const cards = new Map(); // index → element (rows near the view only)

const rowOf = i => Math.floor(i / COLS);
const rowsCount = () => Math.ceil(caps.length / COLS);
const cardX = i => X0 + (i % COLS) * PX;
const cardY = i => Y0 + rowOf(i) * PY;
const when = t => {
  if (!t) return '';
  const time = new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return `${new Date(t).toLocaleDateString([], { day: 'numeric', month: 'short', year: 'numeric' })}, ${time}`;
};

export default {
  mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="gview"><div class="gcontent">
        <div class="gcount"></div>
        <div class="ggrid"></div>
        <div class="gring"></div>
      </div></div>
      <div class="gempty"><div class="msg">No captures yet</div>
        <div class="sub">Press View on your controller while you play to save a screenshot of the game. They show up here.</div></div>
      <div class="ghead"><div class="gicon">${ICON}</div><div class="gtitle">Media Gallery</div></div>
      <div class="gv" data-nav="viewer"><div class="gdim"></div><div class="gpic"><img alt=""></div><div class="gpic"><img alt=""></div>
        <div class="gvhead"><span class="gvname"></span><span class="gvcount"></span></div></div>
      <div class="gdlg"><h2>Delete this screenshot?</h2><p></p><div class="btns">
        <div class="pill" data-nav="btn:0">Delete</div><div class="pill" data-nav="btn:1">Cancel</div></div></div>`;
    for (const k of ['gview', 'gcontent', 'gcount', 'ggrid', 'gring', 'gempty', 'gv', 'gdim', 'gvhead', 'gvname', 'gvcount', 'gdlg'])
      $[k] = el.querySelector(`.${k}`);
    $.pics = [...el.querySelectorAll('.gpic')].map(p => ({ el: p, img: p.querySelector('img'), i: -1 }));
    $.dlgText = $.gdlg.querySelector('p');
    $.pills = [...$.gdlg.querySelectorAll('.pill')];
    window.__gallery = {
      state: () => ({ index, count: caps.length, scroll: scrollY, viewer: view && { i: view.i, closing: view.closing }, dialog: !!dlg, paths: caps.map(c => c.path) }),
    };
  },

  async enter(params = {}) {
    onTop = true;
    if (params.back) return;
    endViewer();
    closeDialog();
    await reload();
    index = 0;
    scrollTo(0, false);
    draw();
    // in, as the Library opens: the content rises 40 px and fades in
    $.gview.animate([{ opacity: 0, transform: 'translateY(40px)' }, { opacity: 1, transform: 'none' }], { duration: 300, easing: EASE });
  },

  leave() {
    onTop = false;
    endViewer();
    closeDialog();
  },

  action(name, e = {}) {
    if (dlg) return dialogAction(name, e);
    if (view) return viewerAction(name, e);
    if (name === 'point' || name === 'click') return pointer(name, e);
    if (name === 'b') return back();
    if (!caps.length) return name === 'release' ? false : 'none';
    return gridAction(name, e);
  },
};

async function reload() {
  caps = await host.games.captures().catch(() => []);
  index = Math.min(index, Math.max(0, caps.length - 1));
  for (const c of cards.values()) c.remove();
  cards.clear();
  root.classList.toggle('none', !caps.length);
  $.gcount.textContent = `All: ${caps.length}`;
}

function back() {
  if (app.stack.length > 1) app.pop();
  else app.go('home');
  return 'back';
}

// ---------- the grid ----------
function gridAction(name, e) {
  const col = index % COLS, n = caps.length;
  if (name === 'left') return col > 0 && focus(index - 1);
  if (name === 'right') return col < COLS - 1 && index + 1 < n && focus(index + 1);
  if (name === 'up') return rowOf(index) > 0 && focus(index - COLS);
  if (name === 'down') return rowOf(index) < rowsCount() - 1 && focus(Math.min(n - 1, index + COLS));
  if (e.held) return false;
  if (name === 'lt' || name === 'rt') {
    const row = Math.max(0, Math.min(rowsCount() - 1, rowOf(index) + (name === 'lt' ? -PAGE_ROWS : PAGE_ROWS)));
    return row !== rowOf(index) && focus(Math.min(n - 1, row * COLS + col));
  }
  if (name === 'wheel') return e.dy ? gridAction(e.dy > 0 ? 'down' : 'up', {}) || 'none' : 'none';
  if (name === 'a') return openViewer(index);
  if (name === 'x' || name === 'menu') return askDelete(index, false);
  return false;
}

function focus(i, animate = true) {
  if (i < 0 || i >= caps.length) return false;
  index = i;
  // the focused row is the first in view, or the second once there is one above it
  const r = rowOf(i);
  const max = Math.max(0, (rowsCount() - 1) * PY - PY);
  scrollTo(Math.min(max, Math.max(0, (r - 1) * PY)), animate);
  draw();
  return true;
}

function scrollTo(y, animate) {
  if (y === scrollY && animate) return;
  const from = scrollY;
  scrollY = y;
  $.gcontent.style.transform = `translateY(${-y}px)`;
  if (animate && from !== y) {
    $.gcontent.animate([{ transform: `translateY(${-from}px)` }, { transform: `translateY(${-y}px)` }], { duration: 300, easing: EASE });
  }
}

// cards for the rows near the view; the ring and caption on the focused one
function draw() {
  const first = Math.max(0, Math.floor(scrollY / PY) - 1), last = first + 6;
  for (const [i, c] of cards) {
    if (rowOf(i) < first || rowOf(i) > last || i >= caps.length) { c.remove(); cards.delete(i); }
  }
  for (let i = first * COLS; i < Math.min(caps.length, (last + 1) * COLS); i++) {
    if (!cards.has(i)) cards.set(i, card(i));
  }
  for (const [i, c] of cards) c.classList.toggle('focus', i === index);
  const has = caps.length > 0;
  $.gring.style.display = has ? '' : 'none';
  if (has) $.gring.style.transform = `translate(${cardX(index) - 6}px, ${cardY(index) - 6}px)`;
}

function card(i) {
  const c = caps[i];
  const el = document.createElement('div');
  el.className = 'gcard';
  el.dataset.nav = `cap:${i}`;
  el.style.transform = `translate(${cardX(i)}px, ${cardY(i)}px)`;
  el.innerHTML = '<img alt=""><div class="shade"></div><div class="cap"><div class="n"></div><div class="d"></div></div>';
  el.querySelector('.n').textContent = c.name;
  el.querySelector('.d').textContent = when(c.takenAt);
  const img = el.querySelector('img');
  img.decoding = 'async';
  img.src = host.fileUrl(c.thumb ?? c.path);
  img.decode().then(() => img.classList.add('in'), () => {});
  $.ggrid.append(el);
  return el;
}

function pointer(name, e) {
  const m = /^cap:(\d+)$/.exec(e.target ?? '');
  if (!m) return 'none';
  const i = +m[1];
  if (i !== index) focus(i);
  return name === 'click' ? openViewer(i) : 'none';
}

// ---------- the viewer ----------
// the transform that lays the 1920×1080 layer over thumbnail i (both 16:9)
function onCard(i) {
  const s = W / 1920;
  return `translate(${cardX(i)}px, ${cardY(i) - scrollY}px) scale(${s.toFixed(5)})`;
}

function load(slot, i) {
  if (slot.i === i) return slot.ready;
  slot.i = i;
  const c = caps[i];
  // the thumbnail first (decoded already), then the full picture swapped in when it is decoded
  slot.img.src = host.fileUrl(c.thumb ?? c.path);
  const full = new Image();
  full.src = host.fileUrl(c.path);
  full.decode().then(() => { if (slot.i === i) slot.img.src = full.src; }, () => {});
  slot.ready = slot.img.decode().catch(() => {});
  return slot.ready;
}

function openViewer(i) {
  if (!caps[i]) return false;
  const [a, b] = $.pics;
  view = { i, anims: [], stepAt: 0, closing: false, shown: a };
  Object.assign(b.el.style, { opacity: 0, zIndex: 0, transform: 'none' });
  Object.assign(a.el.style, { opacity: 1, zIndex: 1, transform: 'none' });
  load(a, i).then(() => {
    if (!view || view.closing) return;
    $.gv.classList.add('on');
    view.anims = [
      a.el.animate([{ transform: onCard(i) }, { transform: 'none' }], { duration: GROW_MS, easing: EASE }),
      $.gdim.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 11 * FRAME, easing: EASE_OUT, fill: 'both' }),
    ];
    head();
  });
  return true;
}

function viewerAction(name, e) {
  if (name === 'left' || name === 'right' || name === 'lb' || name === 'rb') return step(name === 'left' || name === 'lb' ? -1 : 1, e);
  if (name === 'wheel') return step(Math.sign(e.dy || e.dx) || 1, e);
  if (e.held) return false;
  if (name === 'b' || name === 'click') return closeViewer();
  if (name === 'x' || name === 'menu') return askDelete(view.i, true);
  return name === 'release' ? false : 'none';
}

function step(d, e) {
  if (view.closing) return false;
  const j = view.i + d;
  if (j < 0 || j >= caps.length) return false;
  const now = performance.now();
  if (e.held && now - view.stepAt < HELD_MS) return 'none';
  view.stepAt = now;
  view.anims.forEach(a => a.finish());
  const from = view.shown, to = $.pics.find(p => p !== from);
  view.i = j;
  focus(j, false); // the grid follows under the black, so B lands on this thumbnail
  head();
  load(to, j).then(() => {
    if (view?.i !== j || view.closing) return;
    from.el.style.zIndex = 0;
    Object.assign(to.el.style, { zIndex: 1, opacity: 1, transform: 'none' });
    view.shown = to;
    view.anims = [
      to.el.animate([{ transform: `translateX(${d * SHIFT}px)` }, { transform: 'none' }], { duration: STEP_MS, easing: EASE }),
      to.el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 9 * FRAME, easing: EASE_OUT }),
    ];
    view.anims[0].onfinish = () => { if (view?.shown === to) from.el.style.opacity = 0; };
  });
  return true;
}

function closeViewer() {
  if (!view || view.closing) return false;
  view.closing = true;
  view.anims.forEach(a => a.finish());
  const el = view.shown.el;
  const anims = [
    el.animate([{ transform: 'none' }, { transform: onCard(view.i) }], { duration: GROW_MS, easing: EASE, fill: 'forwards' }),
    el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 4 * FRAME, delay: GROW_MS - 4 * FRAME, fill: 'forwards' }),
    $.gdim.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 11 * FRAME, easing: EASE_OUT, fill: 'both' }),
    $.gvhead.animate([{ opacity: getComputedStyle($.gvhead).opacity }, { opacity: 0 }], { duration: 4 * FRAME, fill: 'forwards' }),
  ];
  anims[0].finished.then(() => endViewer(), () => {});
  return 'back';
}

// the viewer gone, at once
function endViewer() {
  if (!view) return;
  view.anims.forEach(a => a.cancel());
  for (const p of $.pics) {
    p.el.getAnimations().forEach(a => a.cancel());
    p.el.style.opacity = 0;
    p.i = -1;
    p.img.removeAttribute('src');
  }
  $.gdim.getAnimations().forEach(a => a.cancel());
  $.gvhead.getAnimations().forEach(a => a.cancel());
  $.gvhead.style.opacity = 0;
  clearTimeout(view.headTimer);
  $.gv.classList.remove('on');
  view = null;
}

// the game, the date and "3 of 12", low on a soft fade, gone after a moment (game HUDs live in the corners)
function head() {
  const c = caps[view.i];
  $.gvname.textContent = `${c.name}  ·  ${when(c.takenAt)}`;
  $.gvcount.textContent = `${view.i + 1} of ${caps.length}`;
  $.gvhead.getAnimations().forEach(a => a.cancel());
  $.gvhead.style.opacity = 1;
  clearTimeout(view.headTimer);
  view.headTimer = setTimeout(() => {
    if (!view) return;
    $.gvhead.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 9 * FRAME, easing: EASE_OUT });
    $.gvhead.style.opacity = 0;
  }, HEAD_MS);
}

// ---------- delete ----------
function askDelete(i, fromViewer) {
  const c = caps[i];
  if (!c) return false;
  dlg = { focus: 1, path: c.path, fromViewer };
  $.dlgText.textContent = `${c.name}, ${when(c.takenAt)}`;
  paintDialog();
  root.classList.add('dlg');
  $.gdlg.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 150, easing: 'ease-out' });
  return true;
}

function paintDialog() {
  $.pills.forEach((p, k) => p.classList.toggle('focus', k === dlg.focus));
}

function closeDialog() {
  dlg = null;
  root?.classList.remove('dlg');
}

function dialogAction(name, e) {
  if (name === 'left' || name === 'right') {
    const f = name === 'left' ? 0 : 1;
    if (f === dlg.focus) return 'none';
    dlg.focus = f;
    paintDialog();
    return true;
  }
  if (name === 'point' || name === 'click') {
    const m = /^btn:(\d)$/.exec(e.target ?? '');
    if (!m) return 'none';
    dlg.focus = +m[1];
    paintDialog();
    if (name === 'point') return 'none';
    name = 'a';
  }
  if (name === 'b' || (name === 'a' && dlg.focus === 1)) { closeDialog(); return 'back'; }
  if (name === 'a') {
    const { path, fromViewer } = dlg;
    closeDialog();
    remove(path, fromViewer);
    return true;
  }
  return 'none';
}

async function remove(path, fromViewer) {
  try {
    await host.games.deleteCapture(path);
  } catch (err) {
    console.warn('delete capture', err);
  }
  const at = view?.i ?? index;
  if (fromViewer) endViewer();
  await reload();
  if (!onTop) return;
  focus(Math.min(at, caps.length - 1), false);
  draw();
  if (fromViewer && caps.length) openViewer(index);
}
