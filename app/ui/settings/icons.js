// Our own line icons for Settings (no Sony art): 36×36 slots, white strokes (3-settings-a.md §1.4).
const svg = body => `<svg viewBox="0 0 36 36" fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

export const ICONS = {
  display: svg('<rect x="3" y="6" width="30" height="19" rx="2"/><path d="M18 25v5M11 31h14"/>'),
  sound: svg('<path d="M4 13.5h6l7-6v21l-7-6H4z" fill="#fff"/><path d="M22.5 13a7 7 0 0 1 0 10M26.5 9a12.5 12.5 0 0 1 0 18"/>'),
  controllers: svg('<path d="M11 10h14c4.5 0 7 3 8 8l1.2 6.5c.6 3.6-3.4 5.6-5.8 3L24.8 24H11.2l-3.6 3.5c-2.4 2.6-6.4.6-5.8-3L3 18c1-5 3.5-8 8-8z"/>' +
    '<path d="M10.5 14.5v5M8 17h5"/><circle cx="24.5" cy="15.5" r="1.3" fill="#fff" stroke="none"/><circle cx="27.5" cy="18.5" r="1.3" fill="#fff" stroke="none"/>'),
  network: svg('<circle cx="18" cy="18" r="14"/><ellipse cx="18" cy="18" rx="6" ry="14"/><path d="M4.5 13.5h27M4.5 22.5h27"/>'),
  library: svg('<rect x="4" y="4" width="12" height="12" rx="2"/><rect x="20" y="4" width="12" height="12" rx="2"/>' +
    '<rect x="4" y="20" width="12" height="12" rx="2"/><rect x="20" y="20" width="12" height="12" rx="2"/>'),
  savedData: svg('<path d="M10.5 28h15.5a7 7 0 0 0 1.2-13.9A9.5 9.5 0 0 0 9 13.4 7.3 7.3 0 0 0 10.5 28z"/><path d="M18 24v-8.5M14.2 19.3 18 15.5l3.8 3.8"/>'),
  power: svg('<path d="M18 4v13"/><path d="M10.2 9.2a12 12 0 1 0 15.6 0"/>'),
  system: svg('<path d="M5 9h26M5 18h26M5 27h26"/><circle cx="12" cy="9" r="3.2" fill="#2a262c"/><circle cx="24" cy="18" r="3.2" fill="#2a262c"/><circle cx="15" cy="27" r="3.2" fill="#2a262c"/>'),
};

export const CHECK = '<svg viewBox="0 0 22 16"><path d="M1.5 8.5l6 6L20.5 1.5"/></svg>';

// Wi-Fi strength as four bars (0..100 → 1..4 lit) and a padlock for secured networks
export function bars(signal) {
  const lit = signal >= 75 ? 4 : signal >= 50 ? 3 : signal >= 25 ? 2 : 1;
  const bar = (k, h) => `<rect x="${k * 7 + 1}" y="${26 - h}" width="4" height="${h}" rx="1" fill="${k < lit ? '#c7c7d5' : 'rgba(255,255,255,.18)'}" stroke="none"/>`;
  return `<svg viewBox="0 0 28 28">${bar(0, 7)}${bar(1, 12)}${bar(2, 18)}${bar(3, 24)}</svg>`;
}
export const LOCK = '<svg viewBox="0 0 22 28"><rect x="2" y="12" width="18" height="14" rx="2"/><path d="M6 12V8a5 5 0 0 1 10 0v4"/></svg>';
