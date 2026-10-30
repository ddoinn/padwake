// What the Journal tab (library/journal.js) and a game's journal page (journal/journal.js) both draw: statuses,
// stars, times and dates. A journal is {rating 1–5, status, progress 0–100, note}, each null when not set.

export const STATUSES = [
  { id: null, label: 'Not set', color: 'rgba(255, 255, 255, .3)' },
  { id: 'playing', label: 'Playing', color: '#6cb4ff' },
  { id: 'finished', label: 'Finished', color: '#79d29a' },
  { id: 'completed', label: '100% complete', color: '#f2c75c' },
  { id: 'paused', label: 'On hold', color: '#c6c6cc' },
  { id: 'dropped', label: 'Dropped', color: '#ef8079' },
];
export const statusOf = id => STATUSES.find(s => s.id === (id ?? null)) ?? STATUSES[0];
export const done = j => j?.status === 'finished' || j?.status === 'completed';
export const EMPTY = { rating: null, status: null, progress: null, note: null };
export const NOTE_MAX = 280; // as the host (library::NOTE_MAX)

const STAR_PATH = 'M12 1.8l3.1 6.4 7 .9-5.1 4.9 1.3 6.9L12 17.5l-6.3 3.4 1.3-6.9L1.9 9.1l7-.9z';
export const STAR = `<svg viewBox="0 0 24 24"><path d="${STAR_PATH}"/></svg>`;
// five stars, the first n lit
export const stars = n => Array.from({ length: 5 }, (_, i) => `<i class="star${i < (n ?? 0) ? ' on' : ''}">${STAR}</i>`).join('');

// "42 h 10 min", "25 min"
export function hours(sec) {
  if (!sec) return '';
  const h = Math.floor(sec / 3600), m = Math.round((sec % 3600) / 60);
  return h ? `${h} h${m ? ` ${m} min` : ''}` : `${Math.max(1, m)} min`;
}

// "3 Oct 2026"
export const date = t => (t ? new Date(t).toLocaleDateString([], { day: 'numeric', month: 'short', year: 'numeric' }) : '');

// "Today", "Yesterday", "4 days ago", else the date
export function ago(t) {
  if (!t) return '';
  const day = d => new Date(d).setHours(0, 0, 0, 0);
  const days = Math.round((day(Date.now()) - day(t)) / 864e5);
  if (days <= 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days < 7) return `${days} days ago`;
  return date(t);
}

// a stable colour per entry without art (as the Library's covers)
export function tint(id) {
  let h = 0;
  for (const c of id) h = (h * 31 + c.codePointAt(0)) % 360;
  return `linear-gradient(145deg, hsl(${h} 45% 34%), hsl(${(h + 25) % 360} 50% 16%))`;
}

// square art for an entry: the tile, else the top of the cover
export const squareArt = g => g.art?.tile ?? g.art?.cover ?? null;
