// Button hints that follow the pad used last (row C1): Xbox letters, PlayStation shapes, Switch letters. The host's
// pad state carries `family` ('xbox' | 'playstation' | 'switch'); input.js puts it on <html data-pad>. Every hint
// holds all three versions and base.css shows the one for that family, so nothing re-renders when the pad changes.
// Buttons go by position: the south button confirms on every pad (Cross; B on a Switch pad), the east one goes back.
// The shapes are our own line icons, in the 2.6 px round-stroke style of settings/icons.js.

export const FAMILIES = ['xbox', 'playstation', 'switch'];

const svg = body => `<svg class="pb-shape" viewBox="0 0 24 24" aria-hidden="true">${body}</svg>`;
export const SHAPES = {
  a: svg('<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>'),       // cross
  b: svg('<circle cx="12" cy="12" r="6.5"/>'),                // circle
  x: svg('<rect x="6" y="6" width="12" height="12" rx="1"/>'), // square
  y: svg('<path d="M12 5.5l7 12.5H5z"/>'),                    // triangle
};

// what each family calls a button, by position: [xbox, playstation, switch]
const SAY = {
  a: ['A', 'Cross', 'B'], b: ['B', 'Circle', 'A'], x: ['X', 'Square', 'Y'], y: ['Y', 'Triangle', 'X'],
  lb: ['LB', 'L1', 'L'], rb: ['RB', 'R1', 'R'], lt: ['LT', 'L2', 'ZL'], rt: ['RT', 'R2', 'ZR'],
  view: ['View', 'Create', '−'], menu: ['Menu', 'Options', '+'], guide: ['Guide', 'Home', 'Home'],
};

/** The family on screen now. */
export function family() {
  const f = document.documentElement.dataset.pad;
  return FAMILIES.includes(f) ? f : 'xbox';
}

export function setFamily(f) {
  if (FAMILIES.includes(f) && document.documentElement.dataset.pad !== f) document.documentElement.dataset.pad = f;
}

/** A button's name in a family, as text (the controller test's readings). */
export function nameOf(button, f = family()) {
  return SAY[button]?.[FAMILIES.indexOf(f)] ?? null;
}

const variants = parts => parts.map((p, i) => `<span class="pf-${FAMILIES[i]}">${p}</span>`).join('');

/** A button in running text or a small hint: the face buttons as their letter or shape, the rest as their word.
 *  `menu` reads ☰ on an Xbox pad, as the Library's hints always said it. */
export function btn(button) {
  const say = SAY[button];
  if (!say) return '';
  if (SHAPES[button]) return `<span class="pb">${variants([say[0], SHAPES[button], say[2]])}</span>`;
  return `<span class="pb">${variants(button === 'menu' ? ['☰', say[1], say[2]] : say)}</span>`;
}

/** The same as SVG for the controller test's drawing: a face button's letter or shape centred on (x, y). */
export function btnSvg(button, x, y, size = 24) {
  const say = SAY[button];
  if (!say) return '';
  const text = s => `<text x="${x}" y="${y}">${s}</text>`;
  const s = size / 24;
  const shape = SHAPES[button]
    ? `<g class="pad-symbol" transform="translate(${x - size / 2} ${y - size / 2}) scale(${s})">${SHAPES[button].replace(/^<svg[^>]*>|<\/svg>$/g, '')}</g>`
    : text(say[1]);
  return `<g class="pf-xbox">${text(say[0])}</g><g class="pf-playstation">${shape}</g><g class="pf-switch">${text(say[2])}</g>`;
}
