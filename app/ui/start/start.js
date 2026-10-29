// The start screen (row A11): the light show, "Press any button on your controller", then the Home reveal.
//
// At boot it puts Home under itself (stack [home, start]), so the library and the first art are ready before the
// reveal, and it stays the top screen until the reveal ends. So no press reaches Home early: an A on the prompt never
// launches a game, and a held direction stops repeating (this screen doesn't take repeats).
// - Light show: assets/intro.mp4 (4.6 s, the mockup's boot rendered by tools/render-intro.sh). Its sparkle burst fades
//   out over 1.7–3.0 s while the live bokeh (sparkle.js) fades in over the same span, read off the video's clock. The
//   prompt fades in at 3.3 s over the beam; meanwhile the video cross-fades (1.3 s) to the living spotlight
//   (background/spotlight.js: the PS5's near-black right side and floor haze) and is unloaded. A waiting prompt is
//   then the spotlight's 2 Hz steps plus the bokeh's 8 Hz canvas: ≈7 % GPU, 1.3 % CPU on the Avita (sparkle.js).
// - Any press during the light show jumps to the prompt (the video to 3.3 s). A press on the prompt, from 300 ms after
//   it shows, plays the start chord and reveals Home.
// - The prompt follows design/ps5/1-home.md §1.1: 30 px Light, centred, cap 235–255; a white Ø86 disc at (960, 563)
//   with our controller glyph; a glow out to r≈80 that flares and decays over 1.4 s every 3.3 s, 4 times only (an
//   endless loop would cost all the time the prompt waits).
// - The reveal follows §2 "User select → Home": the prompt fades out (8 frames), then the row zooms in from the right
//   (bunched at x 810–1320, spreading out and scaling up to its place, 400 ms ease-out), the ring and label come as
//   after a move, the background fades in (+530 ms, 270 ms), then the top bar (+930 ms, 80 ms) and the hero
//   (+1300 ms). Home's own elements carry Web Animations that end with the reveal, so Home is left exactly as it
//   draws itself, and this screen sits behind Home meanwhile, so the row comes in over the spotlight.
// - Then this screen leaves the stack: the bokeh, the spotlight and the video are dropped, and nothing of it runs.
// A restart after a crash (host.startup.info().restarted) goes straight to Home.

import { sparkle } from './sparkle.js';
import { mount as spotlight } from '../background/spotlight.js';

const FRAME = 1000 / 60;
const PROMPT_AT = 3300;                   // ms into the light show: the beam is lit
const PROMPT_IN = 700, PROMPT_SKIP_IN = 300;
const PRESS_GUARD = 300;                  // a press this soon after the prompt shows is the one that showed it
const PROMPT_OUT = 8 * FRAME;
const ROW_AT = PROMPT_OUT, ROW_MS = 400;
const RING_AT = ROW_AT + ROW_MS - 2 * FRAME, RING_MS = 7 * FRAME, LABEL_MS = 6 * FRAME;
const BG_AT = ROW_AT + 530, BG_MS = 270;
const TOP_AT = ROW_AT + 930, TOP_MS = 80;
const HERO_AT = ROW_AT + 1300, HERO_MS = 300;
const END = HERO_AT + HERO_MS + FRAME;
const ZOOM = { k: (1320 - 810) / (1320 - 150), origin: '1320px 200px' }; // 810–1320 spreads to 150–1320
const EASE_OUT = 'cubic-bezier(0, 0, .2, 1)';
const PULSE = { period: 3300, flare: 1400, times: 4 };
// the intro video's own sparkles fade out over 1.7–3.0 s (the mockup's drawBoot); the live bokeh fades in over the same
const SPARK_IN = [1700, 3000];
const HANDOFF_MS = 1300; // from the prompt (3.3 s) to the video's end (4.6 s)

const GLYPH = '<svg viewBox="0 0 64 64"><path fill="#16181f" d="M20 20h24c7.7 0 13 6.4 13.8 15.2l.9 9.6c.4 4.5-2.9 7.2-6.3 7.2-2.5 0-4.4-1.4-5.8-3.4L42.8 43H21.2l-3.8 5.6c-1.4 2-3.3 3.4-5.8 3.4-3.4 0-6.7-2.7-6.3-7.2l.9-9.6C7 26.4 12.3 20 20 20z"/><rect x="16.5" y="28" width="4" height="12" rx="1" fill="#fff"/><rect x="12.5" y="32" width="12" height="4" rx="1" fill="#fff"/><circle cx="44" cy="30" r="2.6" fill="#fff"/><circle cx="49.5" cy="35.5" r="2.6" fill="#fff"/><circle cx="38.5" cy="35.5" r="2.6" fill="#fff"/><circle cx="44" cy="41" r="2.6" fill="#fff"/></svg>';

let app, host, root, video, prompt, glow, spark, spot;
let phase = 'new';                        // new | boot | intro | prompt | reveal | done
let promptAt = 0, revealAt = 0, videoUrl = null, noVideo = false;
let timers = [], anims = [];

const later = (fn, ms) => timers.push(setTimeout(fn, ms));

export default {
  mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.dataset.nav = 'start'; // a mouse click anywhere is a press
    el.innerHTML = `
      <video class="intro" muted playsinline preload="auto"></video>
      <div class="prompt">
        <div class="text">Press any button on your controller.</div>
        <div class="glow"></div><div class="disc">${GLYPH}</div>
      </div>`;
    video = el.querySelector('video');
    prompt = el.querySelector('.prompt');
    glow = el.querySelector('.glow');
    spot = spotlight(el); // under the video; the held prompt shows it instead of the video's last frame
    // read-only state for tests (tools/app/ui-probe.mjs eval steps)
    window.__start = {
      state: () => ({ phase, video: +video.currentTime.toFixed(3), promptAt, revealAt,
        running: anims.filter(a => a.playState === 'running').length, spark: !!spark }),
    };
  },

  enter(params = {}) {
    if (phase === 'new') boot(params);
  },

  leave() {
    if (phase === 'boot') return; // the boot puts this screen back on top of Home
    // an overlay over this screen (Control Center) keeps it; leaving the stack (pop, go) ends it
    queueMicrotask(() => { if (!root.classList.contains('on')) stop(); });
  },

  action(name, e = {}) {
    const press = !e.held && !['release', 'point', 'wheel'].includes(name);
    if (press && phase === 'intro') { showPrompt(PROMPT_SKIP_IN, true); return 'none'; }
    if (press && phase === 'prompt' && performance.now() - promptAt >= PRESS_GUARD) { reveal(); return 'start'; }
    return name === 'b' ? 'none' : false; // B too, or the input layer would pop this screen
  },
};

async function boot(params) {
  phase = 'boot';
  const info = await host.startup.info().catch(() => ({}));
  await app.go('home', params);
  if (info?.restarted) { stop(); return; }
  root.parentNode.append(root); // above Home, under any screen loaded later (Control Center)
  await app.push('start');
  phase = 'intro';
  play();
  // the bokeh over the video, under the prompt; it comes in as the video's own sparkles go (by the video's clock)
  spark = sparkle(root);
  root.append(prompt);
  const ss = (a, b, v) => { const k = Math.min(1, Math.max(0, (v - a) / (b - a))); return k * k * (3 - 2 * k); };
  spark.run(() => (phase === 'intro' && !noVideo ? ss(...SPARK_IN, video.currentTime * 1000) : 1));
}

async function play() {
  try {
    // a blob, so seeking works whatever the page is served from
    const blob = await (await fetch('./assets/intro.mp4')).blob();
    if (noVideo || phase === 'done') return; // pressed through and handed off while it was loading
    videoUrl = URL.createObjectURL(blob);
    video.src = videoUrl;
    await video.play();
  } catch (e) {
    console.warn('intro', e);
    noVideo = true; // the bokeh carries the screen alone
    return showPrompt(PROMPT_SKIP_IN, false);
  }
  if (phase === 'intro') later(() => showPrompt(PROMPT_IN, false), PROMPT_AT - video.currentTime * 1000);
  else if (phase === 'prompt') skipVideo(); // pressed while it was loading
}

function skipVideo() {
  if (video.readyState >= 1 && video.currentTime * 1000 < PROMPT_AT) video.currentTime = PROMPT_AT / 1000;
}

function showPrompt(ms, skipped) {
  if (phase !== 'intro') return;
  phase = 'prompt';
  clearTimers();
  if (skipped) skipVideo();
  promptAt = performance.now();
  prompt.style.opacity = 1;
  anims.push(prompt.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ms, easing: EASE_OUT }));
  const f = PULSE.flare / PULSE.period;
  anims.push(glow.animate([
    { opacity: 0, transform: 'scale(.86)', easing: 'cubic-bezier(.3, 0, .2, 1)' },
    { opacity: 1, transform: 'scale(1)', offset: f * 0.18, easing: 'cubic-bezier(.4, 0, .6, 1)' },
    { opacity: 0, transform: 'scale(1.06)', offset: f },
    { opacity: 0, transform: 'scale(1.06)' },
  ], { duration: PULSE.period, iterations: PULSE.times, delay: ms * 0.6 }));
  handOff();
}

// the video's beam cross-fades to the living spotlight under it (the PS5's darker right side and floor haze), then the
// video is dropped: the waiting prompt is the spotlight's 2 Hz steps plus the bokeh
function handOff() {
  if (noVideo) return;
  const fade = video.animate([{ opacity: 1 }, { opacity: 0 }], { duration: HANDOFF_MS, easing: 'ease-in-out', fill: 'forwards' });
  fade.finished.then(() => {
    if (phase === 'done') return;
    noVideo = true;
    unloadVideo();
  }, () => {});
}

function unloadVideo() {
  video.pause();
  video.style.visibility = 'hidden';
  video.removeAttribute('src');
  video.load(); // lets the decoder go
  if (videoUrl) URL.revokeObjectURL(videoUrl);
  videoUrl = null;
}

function reveal() {
  phase = 'reveal';
  revealAt = performance.now();
  clearTimers();
  cancelAll();
  root.classList.add('under'); // behind Home: the row comes in over the spotlight
  prompt.style.opacity = 0;
  anims.push(prompt.animate([{ opacity: 1 }, { opacity: 0 }], { duration: PROMPT_OUT, easing: EASE_OUT }));
  const home = app.stack.find(s => s.id === 'home')?.el;
  const el = sel => home?.querySelector(sel);
  const run = (target, keyframes, at, ms) => {
    if (target) anims.push(target.animate(keyframes, { duration: ms, delay: at, easing: EASE_OUT, fill: 'backwards' }));
  };
  const fade = (target, at, ms) => run(target, [{ opacity: 0 }, { opacity: 1 }], at, ms);
  run(el('.row'), [
    { opacity: 0, transform: `scale(${ZOOM.k.toFixed(4)})`, transformOrigin: ZOOM.origin },
    { opacity: 1, offset: 0.3 },
    { opacity: 1, transform: 'scale(1)', transformOrigin: ZOOM.origin },
  ], ROW_AT, ROW_MS);
  const ring = el('.ring'); // grows out from the art edge, as after a move
  const grow = [{ opacity: 0, transform: `scale(${174 / 180})` }, { opacity: 1, transform: 'scale(1)' }];
  if (ring && ring.style.opacity !== '0') run(ring, grow, RING_AT, RING_MS);
  fade(el('.label'), RING_AT, LABEL_MS);
  fade(el('.bgwrap'), BG_AT, BG_MS);
  fade(el('.top'), TOP_AT, TOP_MS);
  fade(el('.herowrap'), HERO_AT, HERO_MS);
  later(finish, END);
}

// the reveal is over: hand the stack to Home
function finish() {
  cancelAll();
  if (app.top?.el === root) return app.pop(); // leave() then stops this screen
  // an overlay (Control Center) opened during the reveal: take this screen out from under it
  const i = app.stack.findIndex(s => s.el === root);
  if (i >= 0) app.stack.splice(i, 1);
  root.classList.remove('on');
  stop();
}

function stop() {
  phase = 'done';
  spark?.stop();
  spark = null;
  clearTimers();
  cancelAll();
  root.classList.remove('under');
  unloadVideo();
  spot?.remove(); // its clock already stopped with the screen (.on gone)
  spot = null;
}

function cancelAll() {
  for (const a of anims) a.cancel();
  anims = [];
}

function clearTimers() {
  timers.forEach(clearTimeout);
  timers = [];
}
