// The start screen's gold bokeh (design/ps5/1-home.md §1.1): it keeps drifting and twinkling for as long as the
// prompt waits.
//
//   const s = sparkle(el)   adds the layer (a canvas over the band) to el; nothing runs yet
//   s.run(level)            starts stepping; level() → 0..1 is the layer's strength, read every step (the hand-off
//                           from the intro video's own sparkles)
//   s.stop()                stops for good and drops the layer
//
// What the footage shows (Timeline 1.mp4, 0–5.4 s; blob counts and block matching on the 60 fps frames):
//   - ≈450 particles in a band rising to the right: its middle at y≈750 by x 240 and y≈550 by x 1440, ±140 px
//     (10–90 %); densest right of centre (x 1200–1680), thinner in the middle and at the right edge.
//   - sizes: half are sharp specks (Ø 2–6), a quarter soft dots (Ø 6–12), a fifth out-of-focus discs (Ø 12–30), a few
//     up to Ø≈40, and a rare bright one. All ≈40 luma over the background at their peak; discs amber, specks paler.
//   - no common flow: each wanders on its own at 3–8 px/s (median ≈5, a few to 18), discs a little faster.
//   - twinkle: ≈15 % of them fade out or in within a second.
//
// Cost decides the rate (tools/app/cost.ps1 -Sec 60, release app on the prompt, the Avita): every composited frame
// costs ≈0.6 % GPU per frame per second, whatever changed. Spotlight alone (2 Hz): 1.2 % GPU, 0.4 % CPU. With this
// canvas at 4 / 6 / 8 / 10 / 20 / 60 Hz: 4.3 / 5.6 / 7.1 / 7.9 / 14.4 / 40.1 % GPU and 0.9 / 1.3 / 1.3 / 1.8 / 3.5 /
// 9.1 % CPU of the PC. One promoted element per particle at 10 Hz: 6.1 % GPU but 6.4 % CPU; a pre-rendered 15 fps loop
// video: 19.3 % GPU. So it steps RATE times a second: the motion is slow enough that most steps are under 1 px
// (median 0.6 px, the fastest soft discs ≈1.5 px). A prompt left waiting (the PC boots into it) drops to SLOW_RATE
// after FULL_MS: the same field, still moving, at 4.3 % GPU instead of 6.9 %.
const RATE = 8, SLOW_RATE = 4, FULL_MS = 10 * 60 * 1000;
const LOOP = 30000; // every motion has whole periods in LOOP ms, so t wraps without a jump
const TAU = Math.PI * 2;
const W = 1920, TOP = 260, H = 1080 - TOP; // the canvas: the band and everything under it

// a small seeded random, so the field is the same on every start
function rng(seed) {
  let s = seed >>> 0;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
}

// colours: amber, gold, a pale warm white (the footage's discs average rgb 31:23:15 over the background, specks 31:27:22)
const TINTS = [[255, 172, 96], [255, 202, 136], [255, 230, 190]];
// kinds: share, visible Ø range, alpha range, sprite, odds of each tint, speed factor (near ones move a little faster)
const KINDS = [
  { share: 0.56, d: [1.3, 3.4], a: [0.35, 0.95], sprite: 'speck', tint: [0.4, 0.4, 0.2], speed: 1 },
  { share: 0.25, d: [3.5, 8], a: [0.22, 0.45], sprite: 'soft', tint: [0.4, 0.45, 0.15], speed: 1.1 },
  { share: 0.14, d: [11, 24], a: [0.15, 0.3], sprite: 'disc', tint: [0.45, 0.45, 0.1], speed: 1.3 },
  { share: 0.03, d: [24, 40], a: [0.09, 0.18], sprite: 'disc', tint: [0.5, 0.4, 0.1], speed: 1.4 },
  { share: 0.02, d: [8, 16], a: [0.6, 0.95], sprite: 'hot', tint: [0, 0.3, 0.7], speed: 1.2 }, // the few bright ones
];
const COUNT = 980; // about a third are faded out at any time
// relative density per 240 px column (footage; the middle two lose half their band to the button)
const COLS = [0.3, 0.75, 0.85, 0.7, 0.7, 1.25, 1.25, 0.4];

function field(seed = 7) {
  const r = rng(seed), out = [];
  const gauss = () => { let u = 0; for (let i = 0; i < 4; i++) u += r(); return (u - 2) / 0.577; }; // ≈N(0, 1)
  const total = COLS.reduce((a, b) => a + b, 0);
  const pickX = () => {
    let v = r() * total, c = 0;
    while (v > COLS[c]) v -= COLS[c++];
    return (c + r()) * 240;
  };
  for (let i = 0; i < COUNT; i++) {
    let v = r(), k = 0;
    while (v > KINDS[k].share && k < KINDS.length - 1) v -= KINDS[k++].share;
    const K = KINDS[k];
    let u = r(), tint = 0;
    while (u > K.tint[tint] && tint < TINTS.length - 1) u -= K.tint[tint++];
    const x = pickX();
    const mid = 790 - 0.16 * (x - 240), spread = 160 - 75 * (x / W);
    const y = Math.min(1080 + 20, Math.max(TOP + 30, mid + gauss() * spread));
    const speed = (2.5 + 6 * r() ** 2) * K.speed; // px/s
    // wander: two sines per axis with whole periods in LOOP; amplitude from the speed (A·ω ≈ speed)
    const w1 = TAU * (1 + Math.floor(r() * 2)) / LOOP, w2 = TAU * (2 + Math.floor(r() * 3)) / LOOP;
    const dir = r() * TAU;
    out.push({
      sprite: k * TINTS.length + tint, size: (K.d[0] + (K.d[1] - K.d[0]) * r() ** 1.5) * SCALE[K.sprite],
      a: K.a[0] + (K.a[1] - K.a[0]) * r(), x, y,
      ax: Math.cos(dir) * speed * 0.8 / (w1 * 1000), ay: Math.sin(dir) * speed * 0.8 / (w1 * 1000), w1, p1: r() * TAU,
      bx: (r() - 0.5) * speed * 0.6 / (w2 * 1000), by: (r() - 0.5) * speed * 0.6 / (w2 * 1000), w2, p2: r() * TAU,
      // twinkle: a slow life cycle (5–15 s) that is off about a third of the time; specks also shimmer (1–2 s)
      wl: TAU * (2 + Math.floor(r() * 5)) / LOOP, pl: r() * TAU,
      ws: k === 0 ? TAU * (15 + Math.floor(r() * 16)) / LOOP : 0, ps: r() * TAU,
    });
  }
  return out;
}

const smooth = (a, b, v) => { const t = Math.min(1, Math.max(0, (v - a) / (b - a))); return t * t * (3 - 2 * t); };

// one particle at t ms: [x, y, alpha] in stage px
function at(p, t) {
  const x = p.x + p.ax * Math.sin(p.w1 * t + p.p1) + p.bx * Math.sin(p.w2 * t + p.p2);
  const y = p.y + p.ay * Math.cos(p.w1 * t + p.p1) + p.by * Math.cos(p.w2 * t + p.p2);
  const life = smooth(-0.25, 0.35, Math.sin(p.wl * t + p.pl));
  const shimmer = p.ws ? 0.72 + 0.28 * Math.sin(p.ws * t + p.ps) : 1;
  return [x, y, p.a * life * shimmer];
}

// soft round sprites, drawn once: a speck (a sharp core), a soft dot, an out-of-focus disc (flat, soft rim), a hot one
const SPRITE = 64;
const STOPS = {
  speck: [[0, 1], [0.18, 0.9], [0.4, 0.28], [0.7, 0.06], [1, 0]],
  soft: [[0, 1], [0.35, 0.75], [0.7, 0.2], [1, 0]],
  disc: [[0, 0.82], [0.6, 0.9], [0.78, 0.75], [0.9, 0.25], [1, 0]],
  hot: [[0, 1], [0.45, 0.95], [0.7, 0.55], [0.85, 0.12], [1, 0]],
};
// how much bigger than its visible Ø a sprite is drawn (the faint tail)
const SCALE = { speck: 2.4, soft: 1.6, disc: 1.15, hot: 1.4 };
function sprite(kind, rgb) {
  const c = Object.assign(document.createElement('canvas'), { width: SPRITE, height: SPRITE });
  const g = c.getContext('2d'), h = SPRITE / 2;
  const gr = g.createRadialGradient(h, h, 0, h, h, h);
  for (const [o, a] of STOPS[kind]) gr.addColorStop(o, `rgba(${rgb.join(',')},${a})`);
  g.fillStyle = gr;
  g.fillRect(0, 0, SPRITE, SPRITE);
  return c;
}

export function sparkle(el) {
  const parts = field();
  const sprites = KINDS.flatMap(K => TINTS.map(rgb => sprite(K.sprite, rgb)));
  const cv = Object.assign(document.createElement('canvas'), { className: 'spark', width: W, height: H });
  el.append(cv);
  const g = cv.getContext('2d');
  const draw = (t, k) => {
    g.clearRect(0, 0, W, H);
    g.globalCompositeOperation = 'lighter';
    for (const p of parts) {
      const [x, y, a] = at(p, t);
      if (a * k < 0.01) continue;
      g.globalAlpha = a * k;
      g.drawImage(sprites[p.sprite], x - p.size / 2, y - TOP - p.size / 2, p.size, p.size);
    }
    g.globalAlpha = 1;
  };

  let timer = 0, slowTimer = 0, t0 = 0, level = () => 1, last = -1;
  const step = () => {
    if (document.hidden) return; // the app hides and suspends its web view while a game runs
    const k = Math.min(1, Math.max(0, level()));
    if (k === 0 && last === 0) return;
    last = k;
    draw((performance.now() - t0) % LOOP, k);
  };
  return {
    run(fn) {
      if (fn) level = fn;
      if (timer) return;
      t0 = performance.now() - 4000; // start mid-life, so the field is already full
      step();
      timer = setInterval(step, 1000 / RATE);
      slowTimer = setTimeout(() => {
        if (!timer) return;
        clearInterval(timer);
        timer = setInterval(step, 1000 / SLOW_RATE);
      }, FULL_MS);
    },
    stop() {
      clearInterval(timer);
      clearTimeout(slowTimer);
      timer = 0;
      cv.remove();
    },
  };
}
