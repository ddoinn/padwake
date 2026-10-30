// Toasts (design/ps5/1-home.md §1.9; motion §2 "Toast in / on screen / out"): a box at the top right that fades in
// (the text in 4 frames, the panel in 15), stays 5.3 s from the start and fades out in 11 frames. No slide.
// Any screen may import it:
//   toast({title, message, icon})   title: the small grey line (a game's name); message: the big white line;
//                                   icon: an image URL, or 'glyph:<name>' (mouse, library, power) for one of ours.
// Toasts queue: one at a time, each for its full time. The same module draws the toast in its own small window while
// the app sleeps (toast.html, opened by the host's overlay_toast).

const FRAME = 1000 / 60;
export const TOAST = { inText: 4 * FRAME, inPanel: 15 * FRAME, on: 5300, out: 11 * FRAME };
const BOX = { x: 1236, y: 30 };

const GLYPHS = {
  mouse: '<svg viewBox="0 0 32 32"><path d="M9 4.5v20.5l5.6-5.4 3.6 8.2 3.5-1.5-3.6-8.1 7.7-.2Z" fill="#fff" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/></svg>',
  library: '<svg viewBox="0 0 32 32"><g fill="#fff"><rect x="5" y="5" width="9" height="9" rx="2"/><rect x="18" y="5" width="9" height="9" rx="2"/><rect x="5" y="18" width="9" height="9" rx="2"/><path d="M22.5 17.5v10M17.5 22.5h10" stroke="#fff" stroke-width="3" stroke-linecap="round"/></g></svg>',
  update: '<svg viewBox="0 0 32 32"><g fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M16 4.5v15M10 13.5l6 6 6-6"/><path d="M6 21v4.5h20V21"/></g></svg>',
  cloud: '<svg viewBox="0 0 32 32"><g fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9.3 25h14a6.2 6.2 0 0 0 1-12.3A8.4 8.4 0 0 0 8 11.9 6.5 6.5 0 0 0 9.3 25z"/><path d="M16 21.5V14M12.7 17.2 16 14l3.3 3.2"/></g></svg>',
  power: '<svg viewBox="0 0 32 32"><path d="M10.2 8.6a10.4 10.4 0 1 0 11.6 0" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round"/><path d="M16 4.5v11" stroke="#fff" stroke-width="3" stroke-linecap="round"/></svg>',
};
// our badge where the PS5 has its PS disc: a white disc with the app's mark (a play triangle in a rounded square)
const BADGE = '<svg viewBox="0 0 36 36"><circle cx="18" cy="18" r="18" fill="#fff"/><rect x="10" y="10" width="16" height="16" rx="4" fill="none" stroke="#111" stroke-width="2.4"/><path d="M15.6 14.2v7.6l6-3.8Z" fill="#111"/></svg>';

let layer = null;
let origin = BOX;
const queue = [];
let showing = false;
export const toastLog = []; // [{title, message, shownAt, outAt}] for tests

function css() {
  if (document.querySelector('link[data-toast]')) return;
  const link = Object.assign(document.createElement('link'), { rel: 'stylesheet', href: new URL('./toast.css', import.meta.url).href });
  link.dataset.toast = '';
  document.head.append(link);
}

/// Where toasts go: by default the app's 1920×1080 stage, at the PS5 position. toast.html puts them at (0, 0) of its
/// own small window.
export function mountToasts(el, at = BOX) {
  css();
  layer = el;
  origin = at;
}

export function toast(t) {
  queue.push(t);
  if (!showing) next();
}

function next() {
  const t = queue.shift();
  if (!t) { showing = false; return; }
  showing = true;
  if (!layer) mountToasts(document.getElementById('stage') ?? document.body);
  const el = document.createElement('div');
  el.className = 'toast';
  el.style.left = `${origin.x}px`;
  el.style.top = `${origin.y}px`;
  el.innerHTML = '<div class="toast-bg"></div><div class="toast-fg"><div class="toast-icon"></div><div class="toast-title"></div><div class="toast-msg"></div><div class="toast-badge"></div></div>';
  const icon = el.querySelector('.toast-icon');
  if (t.icon?.startsWith('glyph:')) {
    icon.classList.add('glyph');
    icon.innerHTML = GLYPHS[t.icon.slice(6)] ?? '';
  } else if (t.icon) icon.style.backgroundImage = `url("${t.icon}")`;
  else icon.classList.add('none');
  el.querySelector('.toast-title').textContent = t.title ?? '';
  el.querySelector('.toast-msg').textContent = t.message ?? '';
  el.querySelector('.toast-badge').innerHTML = BADGE;
  layer.append(el);
  const entry = { title: t.title, message: t.message, shownAt: performance.now(), outAt: null };
  toastLog.push(entry);
  const ease = 'cubic-bezier(0, 0, .2, 1)';
  el.querySelector('.toast-bg').animate([{ opacity: 0 }, { opacity: 1 }], { duration: TOAST.inPanel, easing: ease, fill: 'backwards' });
  el.querySelector('.toast-fg').animate([{ opacity: 0 }, { opacity: 1 }], { duration: TOAST.inText, easing: ease, fill: 'backwards' });
  setTimeout(() => {
    entry.outAt = performance.now();
    const out = el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: TOAST.out, easing: 'cubic-bezier(.42, 0, .58, 1)', fill: 'forwards' });
    out.onfinish = () => { el.remove(); next(); };
  }, TOAST.on);
}
