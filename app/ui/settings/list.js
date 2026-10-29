// A Settings list: 98 px rows with labels, values, toggles, sliders, descriptions and section headers, the focus ring
// ("focus light"), scrolling that keeps focus in view, and the scrollbar. design/ps5/3-settings-a.md §1.4–1.10,
// motion §3 rows 2–5, 13, 14 and 17; 4-settings-b.md §2.1.
//
// Rows (plain objects; a field may be a function, read again on refresh()):
//   {kind: 'link',   label, desc?, value?, open}           opens a page (A)
//   {kind: 'action', label, desc?, value?, glyph?, run}    does something (A)
//   {kind: 'toggle', label, desc?, get, set}               A flips it
//   {kind: 'select', label, desc?, options, get, set}      A opens a dropdown; options: [{value, label}]
//   {kind: 'slider', label, desc?, get, set, step?}        left / right change it (0..1)
//   {kind: 'info',   label, value, desc?}                  focusable, does nothing
//   {kind: 'header', label}                                a section header, not focusable
//   {kind: 'cat',    label}                                a left-column category
// Any row may be disabled (dimmed, still focusable). Root rows have an icon (SVG markup).
//
// Motion is transform and opacity only, so it stays on the compositor: rows slide with translateY, the scroller with
// translateY, the ring is placed with translateY and fades with opacity.

export const FRAME = 1000 / 60;
export const EXPAND = 'cubic-bezier(0.20, 0.80, 0.45, 1)'; // rows 13 and 14: description open, one-step scroll
const STREAK = [40, 32, 24, 20, 16, 12, 10, 8, 6, 4, 2, 0]; // row 2: streak edge offsets from the leading edge, per frame
const LIGHT = 3400;      // ring lifecycle: fade in, brighten, settle glow (rows 2–4)
const BAR_IDLE = 3300, BAR_FADE = 500; // row 17

// Open Sans at line-height 1: the cap centre sits 0.531 em below the box top (ascent 1.069, cap 0.714, content 1.362)
export const capTop = (centre, size) => centre - 0.531 * size;

// Stage px. x..r: the ring's outer edges; text: label ink; value: right edge of values; sep: separator span; top: first
// row top; clip: rows above are cut off; bottom: focus stays above this; bar: scrollbar x; dd: dropdown right edge;
// capC: label cap centre in the row. Ring edges follow the keyframes (t0279.50 root 298–1621; t0302.50, t0466.50 and
// t0490.50 right column 651–1752, a few px wider than §1.5's 652–1748).
export const GEOM = {
  root: { x: 298, r: 1622, text: 396, value: 1600, sep: [396, 1600], top: 185, clip: 180, bottom: 990, bar: 1628, icon: 330, dd: 1611 },
  right: { x: 650, r: 1754, text: 672, value: 1731, sep: [672, 1731], top: 185, clip: 180, bottom: 990, bar: 1760, header: 656, dd: 1739 },
  full: { x: 296, r: 1624, text: 322, value: 1597, sep: [322, 1600], top: 183, clip: 178, bottom: 990, bar: 1628, header: 306, dd: 1615 },
  left: { x: 166, r: 565, text: 196, top: 180.5, pitch: 104, capC: 53, ring: [0.5, 3.5], clip: 170, bottom: 1000, noSep: true },
  // read-only info list (§2.3, Video Output Information): no ring, pitch 96, up / down scroll the page 48 px
  info: { x: 304, r: 1616, text: 320, value: 1600, sep: [304, 1616], top: 186, pitch: 96, capC: 48, clip: 180, bottom: 1000, bar: 1630, header: 306 },
};
const PITCH = 98, CAPC = 50, HEADER = 144, DESC_LINE = 34, DESC_PAD = 8;

const read = v => (typeof v === 'function' ? v() : v);
export const focusable = row => row && row.kind !== 'header';

export class List {
  constructor(parent, g, rows, hooks = {}) {
    this.g = { pitch: PITCH, capC: CAPC, ring: [-2, 3], ...g };
    this.hooks = hooks;     // onActivate(row, i), onMove(i), onEdge(dir) → used
    this.index = 0;
    this.active = false;
    this.scroll = 0;
    this.el = document.createElement('div');
    this.el.className = 'slist';
    const { x, r, clip } = this.g;
    this.el.style.cssText = `--x:${x}px;--w:${r - x}px;--tx:${this.g.text - x}px;--vr:${r - (this.g.value ?? r)}px;--capc:${this.g.capC}px;` +
      `--sx:${(this.g.sep?.[0] ?? x) - x}px;--sr:${r - (this.g.sep?.[1] ?? r)}px;--hx:${(this.g.header ?? this.g.text - 16) - x}px;` +
      `--ix:${(this.g.icon ?? x) - x}px`;
    this.el.innerHTML = `<div class="sview" style="top:${clip}px"><div class="sscroll"><div class="sglow"></div><div class="sring"></div></div></div>` +
      (this.g.bar ? `<div class="sbar" style="left:${this.g.bar}px"></div>` : '');
    this.scroller = this.el.querySelector('.sscroll');
    this.scroller.style.transform = `translateY(${-clip}px)`;
    this.ring = this.el.querySelector('.sring');
    this.glow = this.el.querySelector('.sglow');
    this.bar = this.el.querySelector('.sbar');
    parent.append(this.el);
    this.setRows(rows);
  }

  get row() { return this.rows[this.index]; }

  // replaces the rows; keeps focus on the same row (by key or label) if it is still there
  setRows(rows, keep = true) {
    const was = keep && this.rows ? this.rows[this.index] : null;
    const key = r => r && (r.key ?? r.label);
    this.rows = rows;
    this.els?.forEach(e => e.remove());
    this.els = rows.map((row, i) => this.rowEl(row, i));
    this.scroller.prepend(...this.els);
    const again = was ? rows.findIndex(r => key(r) === key(was)) : -1;
    this.index = again >= 0 ? again : this.firstFocusable();
    this.measure();
    this.place(false);
    this.scrollTo(false);
    if (this.active) {
      this.showDesc(false);
      this.ringAt(true);
    }
  }

  firstFocusable() {
    const i = this.rows.findIndex(focusable);
    return i < 0 ? 0 : i;
  }

  rowEl(row, i) {
    const el = document.createElement('div');
    el.className = `srow k-${row.kind}`;
    el.dataset.nav = `row:${i}`;
    if (row.kind === 'header') {
      el.innerHTML = '<span class="label"></span>';
    } else {
      el.innerHTML = `${row.icon ? `<i class="icon">${row.icon}</i>` : ''}<span class="label"></span>` +
        `${row.glyph ? '<i class="glyph"></i>' : ''}<span class="value"></span>` +
        `${row.kind === 'toggle' ? '<span class="tog"><i></i></span>' : ''}` +
        `${row.kind === 'slider' ? '<span class="sld"><b></b><i></i></span>' : ''}` +
        `${row.desc ? '<div class="desc"></div>' : ''}${this.g.noSep ? '' : '<i class="sep"></i>'}`;
    }
    this.fill(el, row);
    return el;
  }

  // writes a row's current state into its element
  fill(el, row) {
    el.querySelector('.label').textContent = read(row.label);
    el.classList.toggle('disabled', !!read(row.disabled));
    const value = el.querySelector('.value');
    if (value) {
      let v = read(row.value);
      if (v == null && row.kind === 'select') v = read(row.options)?.find(o => o.value === read(row.get))?.label;
      value.textContent = v ?? '';
    }
    const glyph = el.querySelector('.glyph');
    if (glyph) glyph.innerHTML = read(row.glyph) ?? '';
    el.querySelector('.tog')?.classList.toggle('on', !!read(row.get));
    const sld = el.querySelector('.sld');
    if (sld) sld.style.setProperty('--p', `${Math.round(Math.min(1, Math.max(0, read(row.get) ?? 0)) * 1000) / 10}%`);
    const desc = el.querySelector('.desc');
    if (desc) desc.textContent = read(row.desc) ?? '';
  }

  // re-reads every row's fields (values, toggles, sliders, disabled) without moving anything
  refresh() {
    this.rows.forEach((row, i) => this.fill(this.els[i], row));
    const h = this.descH.slice();
    this.measure();
    if (h.some((v, i) => v !== this.descH[i])) this.place(true), this.scrollTo(true);
    if (this.active) this.ringAt(false);
  }

  measure() {
    this.descH = this.els.map(el => {
      const d = el.querySelector('.desc');
      if (!d || !d.textContent) return 0;
      return Math.max(1, Math.round(d.scrollHeight / DESC_LINE)) * DESC_LINE + DESC_PAD;
    });
  }

  // tops and heights for the current focus: only the focused row of the active list shows its description
  layout() {
    let y = this.g.top;
    const ys = [], hs = [];
    this.rows.forEach((row, i) => {
      const h = row.kind === 'header' ? HEADER : this.g.pitch + (this.active && i === this.index ? this.descH[i] : 0);
      ys.push(y);
      hs.push(h);
      y += h;
    });
    this.els.forEach((el, i) => {
      const nextHeader = this.rows[i + 1]?.kind === 'header' || i === this.rows.length - 1;
      el.classList.toggle('last', nextHeader);
    });
    return { ys, hs, end: y };
  }

  // moves rows to the layout; animated = row 13's slide (6-frame delay, 12 frames)
  place(animate, delay = 6 * FRAME) {
    const { ys, hs, end } = this.layout();
    this.ys = ys;
    this.hs = hs;
    this.end = end;
    this.els.forEach((el, i) => {
      el.style.height = `${hs[i]}px`;
      slideY(el, ys[i], animate ? { duration: 12 * FRAME, delay, easing: EXPAND } : null);
    });
  }

  // keeps the focused row inside clip..bottom (row 14), and sizes the scrollbar thumb
  scrollTo(animate) {
    const { clip, top, bottom } = this.g;
    const y = this.ys[this.index], h = this.hs[this.index];
    let s = this.scroll;
    if (y + h - s > bottom) s = y + h - bottom;
    if (y - s < top) s = y - top;
    s = Math.max(0, Math.min(s, Math.max(0, this.end - bottom)));
    this.scroll = s;
    slideY(this.scroller, -clip - s, animate ? { duration: 12 * FRAME, easing: EXPAND } : null);
    if (this.bar) {
      const track = [186, (this.g.bottom ?? 990)], view = track[1] - top, total = this.end - top;
      const long = total > view + 1;
      this.bar.hidden = !long;
      if (long) {
        const len = Math.max(60, (view / total) * (track[1] - track[0]));
        const at = track[0] + (s / (total - view)) * (track[1] - track[0] - len);
        this.bar.style.height = `${len}px`;
        slideY(this.bar, at, animate ? { duration: 12 * FRAME, easing: EXPAND } : null);
      }
    }
  }

  showBar() {
    if (!this.bar || this.bar.hidden) return;
    this.barAnim?.cancel();
    this.barAnim = this.bar.animate([{ opacity: 1 }, { opacity: 1, offset: BAR_IDLE / (BAR_IDLE + BAR_FADE) }, { opacity: 0 }],
      { duration: BAR_IDLE + BAR_FADE, fill: 'forwards' });
  }

  // stage y of a row (after scrolling): {top, h, centre}
  rowRect(i = this.index) {
    const top = this.ys[i] - this.scroll;
    return { top, h: this.hs[i], centre: top + this.hs[i] / 2, capC: top + this.g.capC };
  }

  // ---------- focus ----------
  setActive(on, dir = null, start = null) {
    if (on === this.active) return;
    this.active = on;
    if (!on) {
      this.ringOff();
      this.hideDesc();
      this.place(true, 0);
      this.scrollTo(true);
      return;
    }
    if (!this.rows.some(focusable)) return;
    const expands = this.descH[this.index] > 0;
    this.place(true);
    this.scrollTo(true);
    this.showDesc(true);
    this.arrive(dir, start ?? (expands ? 18 * FRAME : 9 * FRAME));
    this.showBar();
  }

  // info pages: up / down move the content 48 px (row 14); false at either end
  scrollBy(dy) {
    const max = Math.max(0, this.end - this.g.bottom);
    const s = Math.max(0, Math.min(max, this.scroll + dy));
    if (s === this.scroll) return false;
    this.scroll = s;
    slideY(this.scroller, -this.g.clip - s, { duration: 12 * FRAME, easing: EXPAND });
    this.showBar();
    const track = [186, this.g.bottom], view = track[1] - this.g.top, total = this.end - this.g.top;
    if (this.bar && !this.bar.hidden) {
      const len = Math.max(60, (view / total) * (track[1] - track[0]));
      slideY(this.bar, track[0] + (s / max) * (track[1] - track[0] - len), { duration: 12 * FRAME, easing: EXPAND });
    }
    return true;
  }

  // one step: true if focus moved (the input layer then ticks and keeps repeating a held direction)
  move(delta, e = {}) {
    let i = this.index + delta;
    while (i >= 0 && i < this.rows.length && !focusable(this.rows[i])) i += delta;
    if (i < 0 || i >= this.rows.length) return false;
    this.focus(i, delta > 0 ? 'down' : 'up', e);
    return true;
  }

  focus(i, dir = null) {
    if (i === this.index && this.active) return;
    const wasExpanded = this.active && this.descH[this.index] > 0;
    this.hideDesc();
    this.index = i;
    if (!this.active) { this.place(false); this.scrollTo(false); return; }
    const expands = this.descH[i] > 0;
    this.ringOff();
    this.place(wasExpanded || expands);
    this.scrollTo(true);
    this.showDesc(true);
    // the ring arrives once the row stops: +150 ms, or at the end of the description opening
    this.arrive(dir, expands ? 18 * FRAME : 9 * FRAME);
    this.showBar();
    this.hooks.onMove?.(i);
  }

  hideDesc() {
    const d = this.els[this.index]?.querySelector('.desc');
    if (!d) return;
    d.getAnimations().forEach(a => a.cancel());
    d.classList.remove('open');
  }

  showDesc(animate) {
    const d = this.els[this.index]?.querySelector('.desc');
    if (!d || !this.active || !this.descH[this.index]) return;
    d.classList.add('open');
    // the new description fades in over ≈100 ms once its row has opened halfway (row 13)
    if (animate) d.animate([{ opacity: 0 }, { opacity: 0, offset: 0.5 }, { opacity: 1 }], { duration: 12 * FRAME + 6 * FRAME, delay: 6 * FRAME, easing: 'linear', fill: 'backwards' });
  }

  ringOff() {
    this.ring.getAnimations().forEach(a => a.cancel());
    this.glow.getAnimations().forEach(a => a.cancel());
    this.ring.style.opacity = '0';
    this.glow.style.opacity = '0';
    this.streak?.remove();
  }

  // puts the ring on the focused row (no motion)
  ringAt(visible) {
    const { top, h } = { top: this.ys[this.index], h: this.hs[this.index] };
    const [dy, dh] = this.g.ring;
    this.ring.style.transform = this.glow.style.transform = `translateY(${top + dy}px)`;
    this.ring.style.height = this.glow.style.height = `${h + dh}px`;
    if (visible) this.ring.style.opacity = '0.75';
  }

  // the ring lifecycle (rows 2–4): a streak runs to the leading edge, the ring fades in from `start` ms, brightens to
  // its peak by 1.2 s, glows once at +2.2 s and rests at ≈65 %
  arrive(dir, start = 9 * FRAME) {
    this.ringOff();
    if (!this.rows[this.index] || !focusable(this.rows[this.index])) return;
    this.ringAt(false);
    this.ring.classList.toggle('dn', dir === 'down');
    this.ring.classList.toggle('up', dir === 'up');
    const at = ms => ms / LIGHT;
    this.ring.animate([
      { opacity: 0, offset: 0 }, { opacity: 0, offset: at(start) }, { opacity: 0.75, offset: at(start + 100), easing: 'ease-out' },
      { opacity: 1, offset: at(1200) }, { opacity: 1, offset: at(2200), easing: 'ease-in-out' }, { opacity: 0.65, offset: 1 },
    ], { duration: LIGHT, fill: 'forwards' });
    this.glow.animate([
      { opacity: 0, offset: 0 }, { opacity: 0, offset: at(2200), easing: 'ease-in-out' }, { opacity: 0.15, offset: at(2750), easing: 'ease-in-out' },
      { opacity: 0, offset: 1 },
    ], { duration: LIGHT, fill: 'forwards' });
    if (dir === 'up' || dir === 'down') this.runStreak(dir);
  }

  // a soft bar runs from the row's middle to its leading edge and fades (row 2)
  runStreak(dir) {
    const s = this.streak = document.createElement('i');
    s.className = `streak ${dir}`;
    this.els[this.index].append(s);
    const sign = dir === 'down' ? -1 : 1;
    const frames = STREAK.map((o, k) => ({ transform: `translateY(${sign * o}px)`, opacity: 0.25 - (0.21 * k) / 11 }));
    frames.push({ transform: 'translateY(0)', opacity: 0 });
    s.animate(frames, { duration: 13 * FRAME, delay: FRAME, easing: 'linear', fill: 'both' }).finished
      .then(() => s.remove(), () => s.remove());
  }

  // A on a row (row 5): the ring fades out in 3 frames and the row flashes ≈5 % white for 170 ms
  press() {
    const o = +getComputedStyle(this.ring).opacity;
    this.ring.getAnimations().forEach(a => a.cancel());
    this.glow.getAnimations().forEach(a => a.cancel());
    this.ring.animate([{ opacity: o }, { opacity: 0 }], { duration: 3 * FRAME, fill: 'forwards' });
    this.glow.animate([{ opacity: 0.05 }, { opacity: 0 }], { duration: 170, easing: 'ease-out', fill: 'forwards' });
  }
}

// translateY from wherever the element is drawn now (mid-animation included) to y
export function slideY(el, y, anim) {
  if (anim && el.style.transform === `translateY(${y}px)`) return; // already going there: let it run
  const from = new DOMMatrixReadOnly(getComputedStyle(el).transform).m42;
  el.getAnimations().filter(a => a.id === 'y').forEach(a => a.cancel());
  el.style.transform = `translateY(${y}px)`;
  if (!anim || Math.abs(from - y) < 0.5) return;
  const a = el.animate([{ transform: `translateY(${from}px)` }, { transform: `translateY(${y}px)` }], { ...anim, fill: 'backwards' });
  a.id = 'y';
}
