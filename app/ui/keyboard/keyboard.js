// On-screen keyboard: a pad-driven text field used by Settings (games folder, Wi-Fi password) and Search (row A8).
//
//   import * as keyboard from '../keyboard/keyboard.js';
//   const kb = keyboard.open({
//     title: 'Wi-Fi Password',  // line above the field
//     value: '',                // starting text
//     secret: false,            // dots instead of text; View (or the eye key) shows or hides them
//     placeholder: '',          // grey text while empty
//     maxLength: 256,
//     parent: stageOrScreenEl,  // where it draws (default #stage); it covers the parent with a dim layer
//     onInput(value) {},        // every change (Search: results as you type)
//     onDone(value) {},         // Done (RT, Menu, Enter, or the Done key)
//     onCancel() {},            // B or Esc
//   });
//   kb.action(name, e)  forward the screen's input actions while kb.isOpen; always truthy while open
//   kb.value            the text now;  kb.isOpen;  kb.row (0 = the top row of keys);  kb.close() closes without
//                       calling onDone or onCancel
//   kb.done             a Promise: the final text, or null if cancelled
//
// The look is the PS5's keyboard: a title where Search has its tabs (cap 84), a full-width field at y 150–221, and
// under it at its left a 870 × 441 panel (a blank strip, a 10 × 5 grid of plain glyphs, a function row and a last
// row). The focused key is a lit cell with a ring 2 px outside it. Tiny pad hints sit in the function keys' corners.
// Search lays it over its own field and hides the dim and title (search.css).
//
// Pad: A types the focused key, B cancels, X deletes, Y types a space, LB / RB move the caret, LT is Shift, RT or Menu
// is Done, View shows or hides a secret. Held directions repeat (input layer, e.held). The mouse points and clicks keys.
// A physical keyboard types straight into the field: it is a real <input>, which the input layer leaves alone.

const css = Object.assign(document.createElement('link'), { rel: 'stylesheet', href: './keyboard/keyboard.css' });
document.head.append(css);

// Open Sans has no ⇧ ⌫ ◀ ▶ or eye, so those are drawn
const svg = (w, h, d) => `<svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${d}</svg>`;
const G = {
  shift: svg(26, 26, '<path d="M13 3 3 13.5h5.5V23h9v-9.5H23z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>'),
  space: svg(38, 12, '<path d="M2 2v8h34V2" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linejoin="round"/>'),
  back: svg(38, 22, '<mask id="osk-x"><path d="M0 0h38v22H0z" fill="#fff"/><path d="M18 6l10 10M28 6 18 16" stroke="#000" stroke-width="2.6" stroke-linecap="round"/></mask><path d="M11 1h25v20H11L1 11z" fill="currentColor" mask="url(#osk-x)"/>'),
  left: svg(14, 16, '<path d="M13 1v14L1 8z" fill="currentColor"/>'),
  right: svg(14, 16, '<path d="M1 1v14l12-7z" fill="currentColor"/>'),
  eye: svg(36, 22, '<path d="M2 11C7 3.5 12 1.5 18 1.5S29 3.5 34 11c-5 7.5-10 9.5-16 9.5S7 18.5 2 11z" fill="none" stroke="currentColor" stroke-width="2.2"/><circle cx="18" cy="11" r="4.6" fill="currentColor"/>'),
  eyeOff: svg(36, 22, '<path d="M2 11C7 3.5 12 1.5 18 1.5S29 3.5 34 11c-5 7.5-10 9.5-16 9.5S7 18.5 2 11z" fill="none" stroke="currentColor" stroke-width="2.2"/><circle cx="18" cy="11" r="4.6" fill="currentColor"/><path d="M5 20 31 2" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>'),
};

// the grid: 10 × 5 plain glyphs per layer
const LAYERS = {
  abc: ['1234567890', 'qwertyuiop', "asdfghjkl'", 'zxcvbnm,.?', '-_/\\:@&!()'],
  sym: ['1234567890', '!@#$%^&*()', '-_=+[]{}\\|', ';:\'",.<>/?', '~`€£¥¢°§¿¡'],
};
// panel geometry (stage px, from the panel's top-left): strip 0–63, grid 63–329 (3 px pad, rows 52, cells 83 from
// x 20), the function row 331–384 and the last row 386–441 (a 2 px line above each), cells 3 px apart
const W = 870, CELL_W = 83, CELL_H = 52, GRID_X = 20, GRID_Y = 66, GAP = 3;
const FN_Y = 331, FN_H = 53, LAST_Y = 386, LAST_H = 55;

let current = null;

export function open(options = {}) {
  current?.close();
  current = new Keyboard(options);
  return current;
}

export const isOpen = () => !!current?.isOpen;

class Keyboard {
  constructor({ title = '', value = '', secret = false, placeholder = '', maxLength = 256, parent, onInput, onDone, onCancel }) {
    this.opts = { secret, maxLength, onInput, onDone, onCancel };
    this.layer = 'abc';
    this.shift = false;
    this.row = 1;
    this.col = 0;
    this.isOpen = true;
    this.done = new Promise(resolve => { this.resolve = resolve; });

    const el = this.el = document.createElement('div');
    el.className = 'osk';
    el.innerHTML = `
      <div class="osk-dim"></div>
      <div class="osk-title"></div>
      <div class="osk-field"><input spellcheck="false" autocomplete="off"></div>
      <div class="osk-panel"><div class="osk-strip"></div><div class="osk-grid"></div><div class="osk-keys"></div></div>`;
    el.querySelector('.osk-title').textContent = title;
    this.input = el.querySelector('input');
    this.input.value = value.slice(0, maxLength);
    this.input.maxLength = maxLength;
    this.input.placeholder = placeholder;
    this.input.type = secret ? 'password' : 'text';
    this.keysEl = el.querySelector('.osk-keys');
    // the physical keyboard edits the field natively; Enter and Esc finish here
    this.input.addEventListener('keydown', e => {
      if (e.key === 'Enter') { e.preventDefault(); this.finish(); }
      else if (e.key === 'Escape') { e.preventDefault(); this.cancel(); }
    });
    this.input.addEventListener('input', () => this.changed());
    this.input.addEventListener('blur', () => this.isOpen && requestAnimationFrame(() => this.isOpen && this.input.focus()));
    (parent ?? document.getElementById('stage')).append(el);
    this.render();
    this.input.focus();
    this.input.setSelectionRange(this.input.value.length, this.input.value.length);
  }

  get value() { return this.input.value; }

  // rows of keys: {label (text or svg), cmd, char, hint, x, y, w, h}
  layout() {
    const grid = LAYERS[this.layer].map((chars, r) => [...chars].map((char, c) => ({
      char, x: GRID_X + c * CELL_W, y: GRID_Y + r * CELL_H, w: CELL_W, h: CELL_H,
    })));
    const strip = (y, h, cells) => {
      let x = 0;
      return cells.map(([cmd, label, w, hint]) => {
        const k = { cmd, label, hint, x, y, w, h };
        x += w + GAP;
        return k;
      });
    };
    const secret = this.opts.secret;
    const mid = W - 116 - 115 - 290 - GAP * (secret ? 4 : 3); // Clear (and the eye) take what ◀ ▶ and Done leave
    const eyeOn = this.input.type === 'text';
    return [
      ...grid,
      strip(FN_Y, FN_H, [['shift', G.shift, 116, 'LT'], ['abc', 'ABC', 115], ['sym', '@#:', 115], ['space', G.space, 367, 'Y'], ['back', G.back, 145, 'X']]),
      strip(LAST_Y, LAST_H, [['left', G.left, 116, 'LB'], ['right', G.right, 115, 'RB'], ['clear', 'Clear', secret ? Math.floor(mid / 2) : mid],
        ...(secret ? [['eye', eyeOn ? G.eyeOff : G.eye, Math.ceil(mid / 2), 'View']] : []), ['done', 'Done', 290, 'RT']]),
    ];
  }

  render() {
    this.keys = this.layout();
    this.keysEl.replaceChildren(...this.keys.flatMap((row, r) => row.map((k, c) => {
      const b = document.createElement('div');
      b.className = k.cmd ? `osk-key fn ${k.cmd}` : 'osk-key';
      b.style.cssText = `left:${k.x}px;top:${k.y}px;width:${k.w}px;height:${k.h}px`;
      b.dataset.nav = `osk:${r}:${c}`;
      if (k.char) b.textContent = this.shift ? k.char.toUpperCase() : k.char;
      else b.innerHTML = `<span class="g">${k.label}</span>${k.hint ? `<span class="hint">${k.hint}</span>` : ''}`;
      k.el = b;
      return b;
    })));
    this.keysEl.querySelector('.shift').classList.toggle('on', this.shift);
    this.keysEl.querySelector(`.${this.layer}`).classList.add('on');
    this.col = Math.min(this.col, this.keys[this.row].length - 1);
    this.paint();
  }

  paint() {
    for (const k of this.keys.flat()) k.el.classList.remove('focus');
    this.keys[this.row][this.col].el.classList.add('focus');
  }

  // up and down keep the column nearest by x, as wide keys span several
  moveTo(row, col) {
    this.row = row;
    this.col = col;
    this.paint();
    return true;
  }

  move(dir) {
    const row = this.keys[this.row], k = row[this.col];
    if (dir === 'left') return this.col > 0 && this.moveTo(this.row, this.col - 1);
    if (dir === 'right') return this.col < row.length - 1 && this.moveTo(this.row, this.col + 1);
    const r = this.row + (dir === 'down' ? 1 : -1);
    if (r < 0 || r >= this.keys.length) return false;
    const mid = k.x + k.w / 2;
    const best = this.keys[r].reduce((b, c, i) => (Math.abs(c.x + c.w / 2 - mid) < Math.abs(this.keys[r][b].x + this.keys[r][b].w / 2 - mid) ? i : b), 0);
    return this.moveTo(r, best);
  }

  press(k = this.keys[this.row][this.col]) {
    k.el.animate([{ opacity: 0.55 }, { opacity: 1 }], { duration: 120, easing: 'ease-out' });
    if (k.char) {
      this.insert(this.shift ? k.char.toUpperCase() : k.char);
      if (this.shift) { this.shift = false; this.render(); }
    } else if (k.cmd === 'back') this.erase();
    else if (k.cmd === 'space') this.insert(' ');
    else if (k.cmd === 'shift') this.toggleShift();
    else if (k.cmd === 'abc' || k.cmd === 'sym') {
      this.layer = k.cmd;
      this.shift = false;
      this.render();
    } else if (k.cmd === 'left') this.caret(-1);
    else if (k.cmd === 'right') this.caret(1);
    else if (k.cmd === 'clear') this.clear();
    else if (k.cmd === 'eye') this.toggleSecret();
    else if (k.cmd === 'done') this.finish();
    return true;
  }

  toggleShift() {
    this.shift = !this.shift;
    this.render();
  }

  toggleSecret() {
    if (!this.opts.secret) return;
    this.input.type = this.input.type === 'password' ? 'text' : 'password';
    this.render();
  }

  insert(text) {
    const i = this.input;
    if (i.value.length >= this.opts.maxLength) return;
    i.setRangeText(text, i.selectionStart, i.selectionEnd, 'end');
    this.changed();
  }

  erase() {
    const i = this.input;
    const s = i.selectionStart, e = i.selectionEnd;
    if (s !== e) i.setRangeText('', s, e, 'end');
    else if (s > 0) i.setRangeText('', s - 1, s, 'end');
    this.changed();
  }

  clear() {
    if (!this.input.value) return;
    this.input.value = '';
    this.changed();
  }

  caret(d) {
    const i = this.input;
    const p = Math.max(0, Math.min(i.value.length, i.selectionStart + d));
    i.setSelectionRange(p, p);
  }

  changed() {
    this.opts.onInput?.(this.input.value);
  }

  // every action is used while the keyboard is open, so nothing reaches the screen under it
  action(name, e = {}) {
    if (!this.isOpen) return false;
    switch (name) {
      case 'up': case 'down': case 'left': case 'right': return this.move(name) || (e.held ? false : 'none');
      case 'a': return this.press();
      case 'b': this.cancel(); return 'back';
      case 'x': this.erase(); return true;
      case 'y': this.insert(' '); return true;
      case 'lb': this.caret(-1); return true;
      case 'rb': this.caret(1); return true;
      case 'lt': this.toggleShift(); return true;
      case 'rt': case 'menu': this.finish(); return true;
      case 'view': this.toggleSecret(); return true;
      case 'point': case 'click': {
        const m = /^osk:(\d+):(\d+)$/.exec(e.target ?? '');
        if (!m) return 'none';
        this.moveTo(+m[1], +m[2]);
        return name === 'click' ? this.press() : 'none';
      }
      default: return 'none';
    }
  }

  finish() {
    const v = this.input.value;
    this.resolve(v);
    this.close();
    this.opts.onDone?.(v);
  }

  cancel() {
    this.resolve(null);
    this.close();
    this.opts.onCancel?.();
  }

  close() {
    if (!this.isOpen) return;
    this.isOpen = false;
    this.input.blur();
    this.el.remove();
    if (current === this) current = null;
    this.resolve(null);
  }
}
