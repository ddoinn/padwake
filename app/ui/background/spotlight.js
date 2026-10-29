// The spotlight background (design/ps5/3-settings-a.md §1.1 and motion row 21): a dark warm-grey left, a rose-beige
// spotlight from above x ≈300, floor haze bottom-left, near-black navy on the right. It is alive, as on the PS5.
//
//   mount(el) → root   prepends <div class="spot"> to el (a screen) and returns it; style dims on the root (opacity).
//                      It moves only while el's screen is shown (.screen.on) and the window is visible, and picks up
//                      where it stopped. Page changes inside the screen never touch it.
//
// What the recording shows (331–365 s, Video Output Information, a still page; frame minus the 34 s mean):
//   - the cone breathes: core luma 71 ↔ 89 (x 160–560, y 0–300), peaks ≈26 s apart; falls over ≈7 s, rises in steps
//   - broad soft beams sweep outward (to the right) about a point above x ≈300 at ≈4°/s, from ≈20° to ≈75° right of
//     straight down, one sweep per ≈26 s; a thin shaft trails the sheet by ≈24°. The fine rays inside the cone stay.
//   - the floor (x 160–640, y 900–1080) wanders 63 ↔ 66 on its own.
//
// Cost decides how: on the Avita a composited frame of this screen costs ≈0.5 % GPU per frame per second (the glow
// alone at 60 fps: 21 % GPU, 2.2 % CPU; everything at 60 fps: 28 % GPU). So the layers are pre-drawn once into small
// images and their Web Animations are stepped by a timer RATE times a second: no layout, paint or raster, only
// opacity and transform on promoted layers (measured at 2 Hz: +0.1 % CPU, +1.2 % GPU). At 2 Hz each step is ≈1 luma,
// below what the eye sees, so nothing moves by position except the very soft haze; the sweep is five fixed soft beams
// whose overlapping fades carry the light outward.
const RATE = 2;
const SINE = 'cubic-bezier(.37, 0, .63, 1)';

const TONE = `
2b262c 726160 705f5e 3a3236 1a1921 0e0f1a 090b16 070814 050513 030311 01010f 000010
2a242a 473d40 55494b 453c40 2e2930 201f27 161722 0e0f1b 090916 060614 020211 000010
252128 352e33 3d353a 393237 2d292f 211f26 17161f 0e0f19 090915 050513 030311 010110
211f26 2b272d 312c32 322d33 2b282e 232128 1a1920 12121b 0b0b17 070714 030311 010110
27272e 2f2e35 333339 333238 2d2c33 25242c 1e1d24 16151e 0f0f19 090a16 050513 020210
36353d 3d3c44 403f46 3c3c43 36353c 2d2c33 25232b 1c1c24 14151e 0f0f1a 0b0b17 070714`;
const grid = TONE.trim().split('\n').map(line => line.trim().split(/\s+/).map(h => [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16))));

// Catmull-Rom through the cell centres (160×180 px cell averages at 438.5 s), clamped at the edges; x, y in stage px
function cubic(p0, p1, p2, p3, t) {
  return p1 + 0.5 * t * (p2 - p0 + t * (2 * p0 - 5 * p1 + 4 * p2 - p3 + t * (3 * (p1 - p2) + p3 - p0)));
}
function tone(x, y, ch) {
  const rows = grid.length, cols = grid[0].length;
  const fx = (x / 1920) * cols - 0.5, fy = (y / 1080) * rows - 0.5;
  const ix = Math.floor(fx), iy = Math.floor(fy), tx = fx - ix, ty = fy - iy;
  const at = (cx, cy) => grid[Math.min(rows - 1, Math.max(0, cy))][Math.min(cols - 1, Math.max(0, cx))][ch];
  const row = cy => cubic(at(ix - 1, cy), at(ix, cy), at(ix + 1, cy), at(ix + 2, cy), tx);
  return cubic(row(iy - 1), row(iy), row(iy + 1), row(iy + 2), ty);
}

const DEG = 180 / Math.PI;
const smooth = (a, b, v) => { const t = Math.min(1, Math.max(0, (v - a) / (b - a))); return t * t * (3 - 2 * t); };
const bump = [{ offset: 0, opacity: 0, easing: SINE }, { offset: 0.13, opacity: 1, easing: SINE }, { offset: 0.26, opacity: 0 }, { offset: 1, opacity: 0 }];
const breathe = [{ opacity: 1, easing: SINE }, { opacity: 0, easing: SINE }, { opacity: 1 }];

// Layers: a box in stage px (drawn at a fifth of it), a colour, an alpha at a stage point, and animations
// [keyframes, period ms, ms into the period at start].
const GLOW = { box: [0, 0, 1300, 1080], rgb: [214, 186, 180],
  // a cone from (310, -120), axis 6° right of straight down, brightest at the top
  a(x, y) { const dx = x - 310, dy = y + 120, th = Math.atan2(dx, dy) * DEG, r = Math.hypot(dx, dy);
    return 0.46 * Math.exp(-(((th - 6) / 20) ** 2) - ((r / 620) ** 2)); },
  anim: [[breathe, 26000, 2000]] };
const HAZE = { box: [-100, 760, 1500, 320], rgb: [124, 122, 132],
  a(x, y) { return 0.07 * Math.exp(-(((x - 480) / 520) ** 2) - (((y - 1010) / 120) ** 2)); },
  anim: [[breathe, 19000, 5000], [[{ transform: 'translateX(-70px)', easing: SINE }, { transform: 'translateX(70px)', easing: SINE },
    { transform: 'translateX(-70px)' }], 23000, 9000]] };
// the sweep: soft sheets from (300, -160), 12° apart, each fading in and out over 7 s, 3 s after the one before (4°/s)
const SRC = [300, -160], SWEEP = 27000, SWEEP_AT = 21000;
const polar = (x, y) => [Math.atan2(x - SRC[0], y - SRC[1]) * DEG, Math.hypot(x - SRC[0], y - SRC[1])];
const reach = r => smooth(80, 320, r) * Math.exp(-r / 1700);
const sheet = (c, k) => ({ wedge: [c - 14, c + 6], rgb: [205, 186, 186],
  a(x, y) { const [th, r] = polar(x, y); return 0.06 * smooth(c - 12, c - 3, th) * (1 - smooth(c, c + 4, th)) * reach(r); },
  anim: [[bump, SWEEP, mod(SWEEP_AT - 3000 * k, SWEEP)]] });
const SHAFT = { wedge: [48, 56], rgb: [205, 186, 186],
  a(x, y) { const [th, r] = polar(x, y); return 0.09 * Math.exp(-(((th - 52) / 1.4) ** 2)) * reach(r); },
  anim: [[bump, SWEEP, mod(SWEEP_AT - 13500, SWEEP)]] };
const BEAMS = [22, 34, 46, 58, 70].map(sheet).concat(SHAFT);
function mod(v, m) { return ((v % m) + m) % m; }

// the on-screen bounding box of a wedge from SRC, so each beam layer covers only its own area
function wedgeBox([a, b]) {
  let x0 = 1920, y0 = 1080, x1 = 0, y1 = 0;
  for (let th = a; th <= b; th += 0.5) {
    for (let r = 0; r < 2400; r += 20) {
      const x = SRC[0] + r * Math.sin(th / DEG), y = SRC[1] + r * Math.cos(th / DEG);
      if (x < 0 || x > 1920 || y < 0 || y > 1080) continue;
      x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
    }
  }
  const m = 40, l = Math.max(0, Math.floor(x0 - m)), t = Math.max(0, Math.floor(y0 - m));
  return [l, t, Math.min(1920, Math.ceil(x1 + m)) - l, Math.min(1080, Math.ceil(y1 + m)) - t];
}
for (const B of BEAMS) B.box = wedgeBox(B.wedge);

// a little ordered dither so the dark gradients don't band when scaled up
const dither = (x, y) => (((x * 7 + y * 13) % 8) - 3.5) / 8;
function draw(iw, ih, px) {
  const c = Object.assign(document.createElement('canvas'), { width: iw, height: ih });
  const g = c.getContext('2d');
  const img = g.createImageData(iw, ih);
  for (let j = 0; j < ih; j++) {
    for (let i = 0; i < iw; i++) {
      const o = (j * iw + i) * 4, [r, gr, b, a] = px(i + 0.5, j + 0.5), d = dither(i, j);
      img.data[o] = r + d; img.data[o + 1] = gr + d; img.data[o + 2] = b + d; img.data[o + 3] = a * 255 + d;
    }
  }
  g.putImageData(img, 0, 0);
  return c.toDataURL('image/png');
}
// a layer image: its colour, with the alpha sampled at each pixel's centre, at a fifth of the box
function layer(L) {
  const [x, y, w, h] = L.box, iw = Math.ceil(w / 5), ih = Math.ceil(h / 5);
  return draw(iw, ih, (i, j) => [...L.rgb, L.a(x + (i / iw) * w, y + (j / ih) * h)]);
}
// the base: the tone map with the haze, then the glow, each at half alpha, undone (out = base·(1 − a) + rgb·a), so
// with both at their mid point the screen is the spec's tone map
function base(x, y) {
  const ag = GLOW.a(x, y) / 2, ah = HAZE.a(x, y) / 2;
  return [0, 1, 2].map(ch => ((tone(x, y, ch) - ag * GLOW.rgb[ch]) / (1 - ag) - ah * HAZE.rgb[ch]) / (1 - ah)).concat(1);
}

const LAYERS = [HAZE, GLOW, ...BEAMS];
let images = null;
const paint = () => images ??= { base: draw(384, 216, (i, j) => base((i / 384) * 1920, (j / 216) * 1080)), layers: LAYERS.map(layer) };

export function mount(el) {
  const im = paint();
  const root = document.createElement('div');
  root.className = 'spot';
  const base = Object.assign(document.createElement('div'), { className: 'spot-base' });
  base.style.backgroundImage = `url(${im.base})`;
  root.append(base);
  const anims = [];
  LAYERS.forEach((L, n) => {
    const img = Object.assign(document.createElement('img'), { alt: '', src: im.layers[n] });
    const [x, y, w, h] = L.box;
    Object.assign(img.style, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${h}px` });
    root.append(img);
    for (const [frames, period, at] of L.anim) {
      const a = img.animate(frames, { duration: period, iterations: Infinity });
      a.pause();
      anims.push([a, at]);
    }
  });
  el.prepend(root);

  // the clock: runs only while shown; a long gap (a suspended window) counts as one step
  const screen = el.closest('.screen') ?? el;
  let t = 0, last = 0, timer = 0;
  const step = () => {
    const now = performance.now();
    t += Math.min(now - last, 2000 / RATE);
    last = now;
    for (const [a, at] of anims) a.currentTime = t + at;
  };
  const sync = () => {
    const shown = screen.classList.contains('on') && !document.hidden;
    if (shown && !timer) { last = performance.now(); timer = setInterval(step, 1000 / RATE); }
    else if (!shown && timer) { clearInterval(timer); timer = 0; }
  };
  step();
  new MutationObserver(sync).observe(screen, { attributes: true, attributeFilter: ['class'] });
  document.addEventListener('visibilitychange', sync);
  sync();
  return root;
}
