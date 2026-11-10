// Controller test: our own vector hardware drawing, using the same monochrome Settings palette.
// Input remains raw: no deadzone here, so a resting stick's offset is visible. Triggers are digital in the host.
// Labels and names follow the pad used last (row C1): the drawing holds each family's labels, base.css shows one.
import { btn, btnSvg, family, FAMILIES } from '../input/glyphs.js';

const COMMON = { up: 'D-pad Up', down: 'D-pad Down', left: 'D-pad Left', right: 'D-pad Right', l3: 'Left Stick', r3: 'Right Stick' };
const NAMES = {
  xbox: { a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'LB', rb: 'RB', lt: 'LT', rt: 'RT', view: 'View', menu: 'Menu', guide: 'Guide' },
  playstation: { a: 'Cross', b: 'Circle', x: 'Square', y: 'Triangle', lb: 'L1', rb: 'R1', lt: 'L2', rt: 'R2', view: 'Create', menu: 'Options', guide: 'Home' },
  switch: { a: 'B', b: 'A', x: 'Y', y: 'X', lb: 'L', rb: 'R', lt: 'ZL', rt: 'ZR', view: 'Minus', menu: 'Plus', guide: 'Home' },
};
// the order buttons are listed in when several are down
const ORDER = ['a', 'b', 'x', 'y', 'lb', 'rb', 'lt', 'rt', 'up', 'down', 'left', 'right', 'l3', 'r3', 'view', 'menu', 'guide'];
// the state names its pad: this listener may run before input.js puts the family on <html>
const nameOf = (id, f) => NAMES[FAMILIES.includes(f) ? f : family()][id] ?? COMMON[id];
// one SVG label per family, centred on (x, y); `words` is [xbox, playstation, switch]
const svgWords = (x, y, words, cls = '') => ['xbox', 'playstation', 'switch']
  .map((f, i) => `<g class="pf-${f}"><text${cls ? ` class="${cls}"` : ''} x="${x}" y="${y}">${words[i]}</text></g>`).join('');
const words = id => [NAMES.xbox[id], NAMES.playstation[id], NAMES.switch[id]];
// a home glyph for the PS / Home button, a line icon like the rest
const home = (x, y) => `<path class="pad-symbol" data-glyph="home" d="M${x - 9} ${y + 1}l9-8 9 8M${x - 6} ${y - 1}v9h12v-9"/>`;
const axis = value => Number.isFinite(value) ? Math.max(-1, Math.min(1, value)) : 0;
const signed = value => `${value > 0 ? '+' : ''}${value.toFixed(2)}`;
const key = (id, shape, glyph) => `<g class="pad-key" data-button="${id}">${shape}${glyph}</g>`;
const round = (id, x, y, radius, glyph) => key(id,
  `<circle class="pad-surface" cx="${x}" cy="${y}" r="${radius}"/>`, glyph);
const face = (id, x, y) => round(id, x, y, 29, btnSvg(id, x, y, 26));
const shoulder = (id, x, y) => key(id,
  `<rect class="pad-surface" x="${x}" y="${y}" width="154" height="48" rx="16"/>`, svgWords(x + 77, y + 24, words(id)));
const stick = (id, x, y, label) => `<circle class="pad-well" cx="${x}" cy="${y}" r="71"/>
  <path class="pad-axis" d="M${x - 59} ${y}h118 M${x} ${y - 59}v118"/>
  <g data-stick="${id}">${key(id, `<circle class="pad-surface" cx="${x}" cy="${y}" r="47"/>
    <circle class="pad-thumb" cx="${x}" cy="${y}" r="36"/>`, `<text x="${x}" y="${y}">${label}</text>`)}</g>`;
const direction = (id, rotation) => `<g transform="translate(397 402) rotate(${rotation})">${key(id,
  '<path class="pad-surface" d="M-23-22V-61Q-23-67-17-67H17Q23-67 23-61V-22L0-7Z"/>',
  '<path class="pad-symbol" d="m-7-43 7-7 7 7"/>')}</g>`;

const drawing = `<svg class="pad-drawing" viewBox="0 0 1000 680" role="img" aria-label="Controller with live button and stick positions">
  <defs><linearGradient id="pad-shell" x2="0" y2="1">
    <stop stop-color="#464646"/><stop offset=".5" stop-color="#2d2d2d"/><stop offset="1" stop-color="#202020"/>
  </linearGradient></defs>
  ${shoulder('lt', 219, 36)}${shoulder('rt', 627, 36)}
  ${shoulder('lb', 219, 97)}${shoulder('rb', 627, 97)}
  <path class="pad-body" d="M298 160C247 155 207 177 182 221C150 280 110 432 90 550C78 617 109 651 153 642C188 636 213 592 248 540L307 457C352 481 407 490 500 490S648 481 693 457L752 540C787 592 812 636 847 642C891 651 922 617 910 550C890 432 850 280 818 221C793 177 753 155 702 160C635 166 568 176 500 176S365 166 298 160Z"/>
  <path class="pad-seam" d="M182 270C190 387 213 433 287 451M818 270C810 387 787 433 713 451"/>
  ${stick('l3', 286, 280, 'L')}${stick('r3', 612, 402, 'R')}
  <circle class="pad-well" cx="397" cy="402" r="78"/>
  ${direction('up', 0)}${direction('right', 90)}${direction('down', 180)}${direction('left', 270)}
  <circle class="pad-dpad-centre" cx="397" cy="402" r="8"/>
  ${round('guide', 500, 227, 27, `<g class="pf-xbox"><text x="500" y="227">G</text></g><g class="pf-playstation pf-switch">${home(500, 227)}</g>`)}
  ${key('view', '<circle class="pad-surface" cx="425" cy="291" r="22"/>',
    `<g class="pf-xbox"><path class="pad-symbol" d="M416 287v-6h14v13h-6 M421 287h14v13h-14Z"/></g>
    <g class="pf-playstation"><path class="pad-symbol" d="M425 293v-12M420 286l5-5 5 5M418 291v7h14v-7"/></g>
    <g class="pf-switch"><path class="pad-symbol" d="M417 291h16"/></g>`)}
  ${key('menu', '<circle class="pad-surface" cx="575" cy="291" r="22"/>',
    `<g class="pf-xbox pf-playstation"><path class="pad-symbol" d="M566 284h18M566 291h18M566 298h18"/></g>
    <g class="pf-switch"><path class="pad-symbol" d="M567 291h16M575 283v16"/></g>`)}
  ${face('y', 731, 218)}${face('x', 670, 279)}
  ${face('b', 792, 279)}${face('a', 731, 340)}
  ${svgWords(425, 252, words('view'), 'pad-caption')}${svgWords(575, 252, words('menu'), 'pad-caption')}
  <text class="pad-caption" x="500" y="537">Press a stick to test its click</text>
</svg>`;

export function padTest(ctx) {
  return {
    title: 'Test Buttons', layout: 'custom',
    mount(el) {
      const box = document.createElement('div');
      box.className = 'pad-test';
      box.innerHTML = `<p class="pad-intro">Press a button or move a stick to see it respond.</p>
        ${drawing}
        <section class="pad-readings" aria-label="Live controller input">
          <h2>Live input</h2>
          <div class="pad-reading pad-pressed"><h3>Buttons</h3><p data-reading="buttons">None pressed</p></div>
          <div class="pad-reading"><h3>Left Stick <span>Click L</span></h3><p data-reading="left">X&nbsp; 0.00&nbsp;&nbsp;&nbsp; Y&nbsp; 0.00</p></div>
          <div class="pad-reading"><h3>Right Stick <span>Click R</span></h3><p data-reading="right">X&nbsp; 0.00&nbsp;&nbsp;&nbsp; Y&nbsp; 0.00</p></div>
          <div class="pad-reading pad-triggers"><h3>Triggers</h3><p><span data-trigger="lt">${btn('lt')} <b>Released</b></span><span data-trigger="rt">${btn('rt')} <b>Released</b></span></p></div>
          <p class="pad-help">${btn('guide')} opens Control Center.<br>Triggers show pressed or released.</p>
        </section>
        <div class="pad-footer"><div class="pad-back"><span class="pad-back-key">${btn('b')}<svg viewBox="0 0 48 48"><circle cx="24" cy="24" r="22"/></svg></span><span>Hold to go back <small>1 second</small></span></div>
          <span class="pad-escape">Esc <span>Back</span></span></div>`;
      el.append(box);
      const parts = [...box.querySelectorAll('[data-button]')];
      const sticks = [...box.querySelectorAll('[data-stick]')];
      const reading = Object.fromEntries([...box.querySelectorAll('[data-reading]')].map(e => [e.dataset.reading, e]));
      const triggers = [...box.querySelectorAll('[data-trigger]')];
      let timer = null, closed = false, bDown = false;
      const cancelBack = () => {
        clearTimeout(timer);
        timer = null;
        box.classList.remove('holding-back');
      };
      const leave = () => { cancelBack(); ctx.back(); };
      const off = ctx.host.on('input', pad => {
        // Guide may have put another screen on top; never let a background B hold navigate Settings.
        if (closed || ctx.app.top?.id !== 'settings') { bDown = !!pad.b; cancelBack(); return; }
        for (const part of parts) part.classList.toggle('on', !!pad[part.dataset.button]);
        for (const part of sticks) {
          const left = part.dataset.stick === 'l3';
          const x = axis(pad[left ? 'lx' : 'rx']), y = axis(pad[left ? 'ly' : 'ry']);
          part.setAttribute('transform', `translate(${x * 23} ${y * 23})`);
          reading[left ? 'left' : 'right'].textContent = `X  ${signed(x)}     Y  ${signed(y)}`;
        }
        reading.buttons.textContent = ORDER.filter(id => pad[id]).map(id => nameOf(id, pad.family)).join(' + ') || 'None pressed';
        reading.buttons.classList.toggle('active', ORDER.some(id => pad[id]));
        for (const trigger of triggers) {
          const on = !!pad[trigger.dataset.trigger];
          trigger.classList.toggle('on', on);
          trigger.querySelector('b').textContent = on ? 'Pressed' : 'Released';
        }
        if (pad.b && !bDown) {
          box.classList.add('holding-back');
          timer = setTimeout(() => {
            if (ctx.app.top?.id === 'settings' && !closed) leave(); else cancelBack();
          }, 1000);
        } else if (!pad.b) cancelBack();
        bDown = !!pad.b;
      });
      // Focus loss cancels a pending exit; it must never finish while the user is in another window.
      window.addEventListener('blur', cancelBack);
      return {
        action: (name, e) => name === 'b' && e.source !== 'pad' ? (leave(), 'back') : 'none',
        leave() {
          closed = true;
          off();
          cancelBack();
          window.removeEventListener('blur', cancelBack);
        },
      };
    },
  };
}
