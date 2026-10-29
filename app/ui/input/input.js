// Input layer (row A3): keys, the mouse and the host's pad state become actions for the top screen, with the PS5's
// tap/hold feel, a rumble tick per move and a sound per action. Every screen gets the same layer.
//
// A screen's action(name, e) returns truthy if it used the action; the layer then plays the action's sound (and a
// rumble tick for a pad move). Return a sound name ('move' 'select' 'back' 'start') to pick another, or 'none'.
//   up down left right   e.held false: a press, one move. e.held true: a repeat step while held (see Hold)
//   release              e.dir: a held direction was let go after at least one repeat step
//   a b x y lb rb lt rt menu view
//   point, click         the mouse moved onto / clicked an element with data-nav="<target>": e.target, e.el. No point
//                        while the pointer hasn't really moved, nor for 1.5 s after a resize or the window showing or hiding
//   wheel                e.dx, e.dy: mouse wheel notches (+1 = down / right)
// e.source is 'key' | 'pad' | 'stick' | 'mouse'. Guide never reaches a screen: a tap opens Control Center (a second
// tap closes it), a hold goes Home. B that the top screen doesn't use closes it if it is an overlay.
//
// Hold (design/ps5/1-home.md §3): a direction held HOLD_MS (the stick: HOLD_STICK_MS) becomes a run. The run's
// speed starts at RUN.v0 px/ms and rises +200 px/s every 50 ms (RUN.accel) up to RUN.max. Repeat steps arrive when
// a run at that speed would pass the next PITCH-px item: step 1 the moment the hold starts, then about every
// 55 ms, faster and faster. e.runStart is the run's start (performance.now() time), e.speed the speed now. A list
// moves one item per step; a screen that draws the run itself (Home's row) calls runDistance(now - e.runStart) each
// frame and settles on release.
//
// Keyboard: arrows = D-pad, Enter or Space = A, Esc or Backspace = B, X = X, Y = Y, Q / E = LB / RB,
// PageUp / PageDown = LT / RT, M or the context-menu key = Menu, V = View, Home = Guide (held 600 ms = Guide hold).

export const HOLD_MS = 120;          // longer than a tap (keys and buttons are down 60-110 ms per tap)
export const HOLD_STICK_MS = 200;    // a stick flick lasts longer than a button tap
export const PITCH = 114;            // Home's tile pitch: the item size the step timing assumes
// The run resumes the PS5's measured run (1.6 → 2.5 px/ms, +≈200 px/s every 50 ms) where it passed its first
// tile, since our first tile is the tap's spring move.
export const RUN = { v0: 2.4, accel: 0.004, max: 4.0 };
export const runSpeed = ms => Math.min(RUN.max, RUN.v0 + RUN.accel * ms);
const capT = (RUN.max - RUN.v0) / RUN.accel;
export function runDistance(ms) {
  const t = Math.min(ms, capT);
  return RUN.v0 * t + RUN.accel * t * t / 2 + Math.max(0, ms - capT) * RUN.max;
}
function runTime(px) { // inverse of runDistance
  const t = (-RUN.v0 + Math.sqrt(RUN.v0 * RUN.v0 + 2 * RUN.accel * px)) / RUN.accel;
  return t <= capT ? t : capT + (px - runDistance(capT)) / RUN.max;
}

const GUIDE_HOLD_MS = 600; // as the host's Guide (src-tauri/src/input/xinput.rs)
// the stick works like the D-pad: past ON it presses, back under OFF it lets go, and switching axes needs a clear
// lead, so a stick resting near a threshold or a diagonal doesn't flicker
const STICK = { dead: 0.25, on: 0.5, off: 0.35, lead: 0.15 };

const KEYS = {
  ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down', Enter: 'a', ' ': 'a', Escape: 'b',
  Backspace: 'b', x: 'x', y: 'y', q: 'lb', e: 'rb', PageUp: 'lt', PageDown: 'rt', m: 'menu', ContextMenu: 'menu',
  v: 'view', Home: 'guide',
};
const DIRS = ['up', 'down', 'left', 'right'];
const PAD_BUTTONS = ['a', 'b', 'x', 'y', 'lb', 'rb', 'lt', 'rt', 'menu', 'view'];
const MOVES = new Set([...DIRS, 'lb', 'rb', 'lt', 'rt', 'point']);
const SOUND_OF = { a: 'select', click: 'select', menu: 'select', x: 'select', y: 'select', view: 'select', b: 'back' };
const SOUNDS = { move: ['navigation', 1, 0.7], select: ['activation', 1, 0.8], back: ['navigation', 0.8, 0.7], start: ['start', 1, 0.8] };

let app, host;
const settings = { rumble: true, uiSounds: true };
export const inputState = { sounds: 0, rumbles: 0, lastSound: null }; // counters for tests

export function startInput(h, a) {
  host = h;
  app = a;
  host.settings.get().then(s => Object.assign(settings, s)).catch(() => {});
  host.on('settings', s => Object.assign(settings, s));
  startAudio();
  startKeys();
  startPad();
  startMouse();
  host.on('guide', ({ kind }) => guide(kind));
  window.__input = { state: inputState, settings, runDistance, RUN, HOLD_MS };
}

// ---------- dispatch and feedback ----------
function fire(name, e) {
  const used = app.top?.mod.action?.(name, e);
  if (!used && name === 'b' && app.stack.length > 1) { app.pop(); feedback('b', e, true); return true; }
  if (used) feedback(name, e, used);
  return used;
}

let lastTick = 0;
function feedback(name, e, used) {
  if (name === 'release') return;
  const now = performance.now();
  const move = MOVES.has(name) || name === 'wheel';
  if (move && e.held && now - lastTick < 45) return; // a fast run ticks at most every 45 ms
  if (move) lastTick = now;
  playSound(typeof used === 'string' ? used : move ? 'move' : SOUND_OF[name] ?? 'select');
  if (move && (e.source === 'pad' || e.source === 'stick') && settings.rumble !== false) {
    inputState.rumbles++;
    host.input.rumble(e.held ? 0.2 : 0.3, 30).catch(() => {});
  }
}

function guide(kind) {
  if (kind === 'hold') return void app.go('home');
  if (app.top?.id === 'control-center') app.pop();
  else app.push('control-center');
  playSound('select');
}

// ---------- directions: tap and hold ----------
const holds = new Map(); // id ('key:ArrowRight', 'pad:right', 'stick') → {dir, source, timer, steps, runStart}
let current = null;      // the hold that repeats: the direction pressed last

function pressDir(id, dir, source) {
  if (holds.has(id)) return;
  if (current) stopHold(current, true);
  const h = { id, dir, source, steps: 0, runStart: 0 };
  holds.set(id, h);
  current = h;
  fire(dir, { source, held: false });
  h.timer = setTimeout(() => { h.runStart = performance.now(); step(h); }, source === 'stick' ? HOLD_STICK_MS : HOLD_MS);
}

function step(h) {
  if (current !== h) return;
  h.steps++;
  const now = performance.now();
  const used = fire(h.dir, { source: h.source, held: true, step: h.steps, speed: runSpeed(now - h.runStart), runStart: h.runStart });
  if (!used) return; // nothing further that way: no more steps until it's pressed again
  const next = h.runStart + runTime(h.steps * PITCH);
  h.timer = setTimeout(() => step(h), Math.max(0, next - performance.now()));
}

function releaseDir(id) {
  const h = holds.get(id);
  if (!h) return;
  holds.delete(id);
  if (current === h) stopHold(h, false);
  clearTimeout(h.timer);
}

function stopHold(h, replaced) {
  clearTimeout(h.timer);
  if (current === h) current = null;
  if (replaced) holds.delete(h.id); // a new direction takes over; this one must be pressed again
  if (h.steps) fire('release', { source: h.source, dir: h.dir });
}

function releaseAll() {
  for (const id of [...holds.keys()]) releaseDir(id);
}

// ---------- keyboard ----------
function startKeys() {
  let guideTimer = null;
  addEventListener('keydown', e => {
    const name = KEYS[e.key] ?? KEYS[e.key.toLowerCase()];
    if (!name || e.ctrlKey || e.altKey || e.metaKey || e.target.closest?.('input, textarea')) return;
    e.preventDefault();
    usingMouse(false);
    if (e.repeat) return; // the layer does its own repeat
    if (name === 'guide') {
      guideTimer = setTimeout(() => { guideTimer = null; guide('hold'); }, GUIDE_HOLD_MS);
    } else if (DIRS.includes(name)) pressDir(`key:${e.key}`, name, 'key');
    else fire(name, { source: 'key' });
  });
  addEventListener('keyup', e => {
    if (KEYS[e.key] === 'guide' && guideTimer) { clearTimeout(guideTimer); guideTimer = null; guide('tap'); }
    releaseDir(`key:${e.key}`);
  });
  addEventListener('blur', releaseAll);
  document.addEventListener('visibilitychange', () => document.hidden && releaseAll());
}

// ---------- pad (the host sends the whole state on every change) ----------
function startPad() {
  let prev = {};
  let stickDir = null;
  host.on('input', pad => {
    usingMouse(false);
    for (const b of PAD_BUTTONS) if (pad[b] && !prev[b]) fire(b, { source: 'pad' });
    for (const d of DIRS) {
      if (pad[d] && !prev[d]) pressDir(`pad:${d}`, d, 'pad');
      else if (!pad[d] && prev[d]) releaseDir(`pad:${d}`);
    }
    const s = stickDirection(pad.lx ?? 0, pad.ly ?? 0, stickDir);
    if (s !== stickDir) {
      if (stickDir) releaseDir('stick');
      stickDir = s;
      if (s) pressDir('stick', s, 'stick');
    }
    prev = pad;
  });
}

// the stick as a D-pad: deadzone, on/off hysteresis and a lead needed to switch axes
export function stickDirection(x, y, was) {
  const ax = Math.abs(x), ay = Math.abs(y);
  if (Math.hypot(x, y) < STICK.dead) return null;
  const along = d => (d === 'left' ? -x : d === 'right' ? x : d === 'up' ? -y : y);
  const strongest = ax >= ay ? (x < 0 ? 'left' : 'right') : (y < 0 ? 'up' : 'down');
  if (was) {
    if (along(was) < STICK.off) return along(strongest) > STICK.on ? strongest : null;
    const other = strongest !== was && along(strongest) > along(was) + STICK.lead;
    return other ? strongest : was;
  }
  return along(strongest) > STICK.on ? strongest : null;
}

// ---------- mouse: focus on hover, act on click ----------
let mouse = false;
function usingMouse(on) {
  if (on === mouse) return;
  mouse = on;
  document.documentElement.style.cursor = on ? '' : 'none';
}

// A display-mode change, or the window showing or hiding, moves the pointer under a still mouse; that isn't a person,
// so hover doesn't take focus for HOVER_QUIET_MS after one
export const HOVER_QUIET_MS = 1500;
function startMouse() {
  usingMouse(false);
  let over = null;
  let quietUntil = 0;
  const quiet = () => { quietUntil = performance.now() + HOVER_QUIET_MS; };
  addEventListener('resize', quiet);
  document.addEventListener('visibilitychange', quiet);
  const target = e => e.target.closest?.('[data-nav]');
  addEventListener('pointermove', e => {
    if (!e.movementX && !e.movementY) return; // the page moved under a still pointer: not a person
    if (performance.now() < quietUntil) return;
    usingMouse(true);
    const el = target(e);
    if (el === over) return;
    over = el;
    if (el) fire('point', { source: 'mouse', target: el.dataset.nav, el });
  });
  addEventListener('pointerdown', e => {
    if (e.button !== 0 && e.button !== 2) return;
    usingMouse(true);
    const el = target(e);
    if (e.button === 2) fire('menu', { source: 'mouse', target: el?.dataset.nav, el });
    else if (el) fire('click', { source: 'mouse', target: el.dataset.nav, el });
  });
  let wheelT = 0;
  addEventListener('wheel', e => {
    const now = performance.now();
    if (now - wheelT < 90) return; // one notch per move, however fast the wheel spins
    wheelT = now;
    usingMouse(true);
    fire('wheel', { source: 'mouse', dx: Math.sign(e.deltaX), dy: Math.sign(e.deltaY) });
  }, { passive: true });
}

// ---------- sounds: Web Audio, decoded up front; the context sleeps when quiet so idle costs nothing ----------
let ctx = null;
const buffers = {};
let sleepTimer = 0;

function startAudio() {
  try {
    ctx = new AudioContext({ latencyHint: 'interactive' });
  } catch (e) {
    console.warn('no Web Audio', e);
    return;
  }
  for (const name of ['navigation', 'activation', 'start']) {
    fetch(`./assets/sounds/${name}.wav`).then(r => r.arrayBuffer()).then(b => ctx.decodeAudioData(b))
      .then(buf => { buffers[name] = buf; }).catch(e => console.warn('sound', name, e));
  }
  ctx.suspend().catch(() => {});
}

export function playSound(kind) {
  if (kind === 'none' || settings.uiSounds === false || !ctx) return;
  const [name, rate, gain] = SOUNDS[kind] ?? SOUNDS.select;
  const buf = buffers[name];
  if (!buf) return;
  inputState.sounds++;
  inputState.lastSound = kind;
  if (ctx.state !== 'running') ctx.resume().catch(() => {});
  const src = ctx.createBufferSource();
  src.buffer = buf;
  src.playbackRate.value = rate;
  const g = ctx.createGain();
  g.gain.value = gain;
  src.connect(g).connect(ctx.destination);
  src.start();
  clearTimeout(sleepTimer);
  sleepTimer = setTimeout(() => ctx.suspend().catch(() => {}), 3000 + buf.duration * 1000);
}
