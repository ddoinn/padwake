// Controller test: our own vector hardware drawing, using the same monochrome Settings palette.
// Input remains raw: no deadzone here, so a resting stick's offset is visible. Triggers are digital in the host.
const BUTTONS = {
  a: 'A', b: 'B', x: 'X', y: 'Y', lb: 'LB', rb: 'RB', lt: 'LT', rt: 'RT',
  up: 'D-pad Up', down: 'D-pad Down', left: 'D-pad Left', right: 'D-pad Right',
  l3: 'Left Stick', r3: 'Right Stick', view: 'View', menu: 'Menu', guide: 'Guide',
};
const axis = value => Number.isFinite(value) ? Math.max(-1, Math.min(1, value)) : 0;
const signed = value => `${value > 0 ? '+' : ''}${value.toFixed(2)}`;
const key = (id, shape, glyph) => `<g class="pad-key" data-button="${id}">${shape}${glyph}</g>`;
const round = (id, x, y, radius, label) => key(id,
  `<circle class="pad-surface" cx="${x}" cy="${y}" r="${radius}"/>`,
  `<text x="${x}" y="${y}">${label}</text>`);
const shoulder = (id, x, y, label) => key(id,
  `<rect class="pad-surface" x="${x}" y="${y}" width="154" height="48" rx="16"/>`,
  `<text x="${x + 77}" y="${y + 24}">${label}</text>`);
const stick = (id, x, y, label) => `<circle class="pad-well" cx="${x}" cy="${y}" r="71"/>
  <path class="pad-axis" d="M${x - 59} ${y}h118 M${x} ${y - 59}v118"/>
  <g data-stick="${id}">${key(id, `<circle class="pad-surface" cx="${x}" cy="${y}" r="47"/>
    <circle class="pad-thumb" cx="${x}" cy="${y}" r="36"/>`, `<text x="${x}" y="${y}">${label}</text>`)}</g>`;
const direction = (id, rotation) => `<g transform="translate(397 402) rotate(${rotation})">${key(id,
  '<path class="pad-surface" d="M-23-22V-61Q-23-67-17-67H17Q23-67 23-61V-22L0-7Z"/>',
  '<path class="pad-symbol" d="m-7-43 7-7 7 7"/>')}</g>`;

const drawing = `<svg class="pad-drawing" viewBox="0 0 1000 680" role="img" aria-label="Controller with live button and stick positions">
  <defs><linearGradient id="pad-shell" x2="0" y2="1">
    <stop stop-color="#41434d"/><stop offset=".5" stop-color="#292b34"/><stop offset="1" stop-color="#1c1e26"/>
  </linearGradient></defs>
  ${shoulder('lt', 219, 36, 'LT')}${shoulder('rt', 627, 36, 'RT')}
  ${shoulder('lb', 219, 97, 'LB')}${shoulder('rb', 627, 97, 'RB')}
  <path class="pad-body" d="M298 160C247 155 207 177 182 221C150 280 110 432 90 550C78 617 109 651 153 642C188 636 213 592 248 540L307 457C352 481 407 490 500 490S648 481 693 457L752 540C787 592 812 636 847 642C891 651 922 617 910 550C890 432 850 280 818 221C793 177 753 155 702 160C635 166 568 176 500 176S365 166 298 160Z"/>
  <path class="pad-seam" d="M182 270C190 387 213 433 287 451M818 270C810 387 787 433 713 451"/>
  ${stick('l3', 286, 280, 'L')}${stick('r3', 612, 402, 'R')}
  <circle class="pad-well" cx="397" cy="402" r="78"/>
  ${direction('up', 0)}${direction('right', 90)}${direction('down', 180)}${direction('left', 270)}
  <circle class="pad-dpad-centre" cx="397" cy="402" r="8"/>
  ${round('guide', 500, 227, 27, 'G')}
  ${key('view', '<circle class="pad-surface" cx="425" cy="291" r="22"/>',
    '<path class="pad-symbol" d="M416 287v-6h14v13h-6 M421 287h14v13h-14Z"/>')}
  ${key('menu', '<circle class="pad-surface" cx="575" cy="291" r="22"/>',
    '<path class="pad-symbol" d="M566 284h18M566 291h18M566 298h18"/>')}
  ${round('y', 731, 218, 29, 'Y')}${round('x', 670, 279, 29, 'X')}
  ${round('b', 792, 279, 29, 'B')}${round('a', 731, 340, 29, 'A')}
  <text class="pad-caption" x="425" y="252">View</text><text class="pad-caption" x="575" y="252">Menu</text>
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
          <div class="pad-reading pad-triggers"><h3>Triggers</h3><p><span data-trigger="lt">LT <b>Released</b></span><span data-trigger="rt">RT <b>Released</b></span></p></div>
          <p class="pad-help">Guide opens Control Center.<br>Triggers show pressed or released.</p>
        </section>
        <div class="pad-footer"><div class="pad-back"><span class="pad-back-key">B<svg viewBox="0 0 48 48"><circle cx="24" cy="24" r="22"/></svg></span><span>Hold to go back <small>1 second</small></span></div>
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
        reading.buttons.textContent = Object.entries(BUTTONS).filter(([id]) => pad[id]).map(([, label]) => label).join(' + ') || 'None pressed';
        reading.buttons.classList.toggle('active', Object.keys(BUTTONS).some(id => pad[id]));
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
