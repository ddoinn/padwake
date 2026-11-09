// Row B4: each user's monthly and yearly recaps, worked out from their session log (rows B1, B1b) as the host sends it
// (host.library.recap: {sessions, events}, each game session with the Padwake trophies it earned, replayed by the
// host from today's totals: library/sessions.rs `replay`). The Journal tab's Recaps shelf (library/journal.js), the
// recap pages (journal/recap.js) and the new-month notice (control-center/notices.js) all read it here.
// - Periods: "This month so far" always, then every past month with a session, then every year with one (newest
//   first). Nothing comes from before the log began: a period counts logged sessions only, and a game is "new" only
//   when its first play is in the log's time.
// - Times are local. A session counts in the month (and year) it started in. Weekday and time of day split a session
//   over the hours it ran, so a night that goes past midnight counts where its time went.
// - Lines from before row B1b have no `front`, `achievements` or media fields: what they lack is left out, never
//   guessed. Time in front of the game is shown as a share of the sessions that know it.

import { TROPHIES } from './trophies.js';

export const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
export const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
// the parts of a day, by the hour they start (local): night runs 22:00–05:00
export const PARTS = [
  { id: 'morning', label: 'Morning', from: 5, to: 12, who: 'an early riser' },
  { id: 'afternoon', label: 'Afternoon', from: 12, to: 17, who: 'an afternoon player' },
  { id: 'evening', label: 'Evening', from: 17, to: 22, who: 'an evening player' },
  { id: 'night', label: 'Night', from: 22, to: 29, who: 'a night owl' },
];
// each recap has its colour: a hue per month (a year takes its own)
export const hueOf = p => (p.kind === 'year' ? 265 : [210, 330, 150, 95, 45, 15, 355, 285, 25, 190, 300, 230][p.month]);
// the recap mark: a calendar with a star
export const MARK = '<svg viewBox="0 0 32 32"><g fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="7" width="22" height="20" rx="3"/><path d="M5 13h22M11 4.5v5M21 4.5v5"/></g><path fill="#fff" d="M16 16.2l1.3 2.7 2.9.4-2.1 2 .5 2.9-2.6-1.4-2.6 1.4.5-2.9-2.1-2 2.9-.4Z"/></svg>';

const partOf = h => PARTS.find(p => (h < 5 ? h + 24 : h) >= p.from && (h < 5 ? h + 24 : h) < p.to).id;

const isGame = s => !s.kind && !!s.game;
const isMedia = s => s.kind === 'media';
const monthKey = t => { const d = new Date(t); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`; };
const dayKey = t => { const d = new Date(t); return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`; };

// The period a key names: '2026-11' (a month) or '2026' (a year), local time, with its bounds in Unix ms
export function period(key, now = Date.now()) {
  const [y, m] = key.split('-').map(Number);
  const start = new Date(y, m ? m - 1 : 0, 1).getTime();
  const end = m ? new Date(y, m, 1).getTime() : new Date(y + 1, 0, 1).getTime();
  const current = now >= start && now < end;
  const kind = m ? 'month' : 'year';
  const name = m ? MONTHS[m - 1] : String(y);
  return { key, kind, start, end, current, name, year: y, month: m ? m - 1 : null,
    title: m ? `${name} ${y}` : name, label: current ? `${name} so far` : m ? `${name} ${y}` : `${y}` };
}

// "This month so far", every past month with a session, then the years with one (newest first)
export function periods(log, now = Date.now()) {
  const played = (log?.sessions ?? []).filter(s => isGame(s) || isMedia(s));
  const months = new Set(played.map(s => monthKey(s.start)));
  const thisMonth = monthKey(now);
  months.delete(thisMonth);
  const years = new Set(played.map(s => String(new Date(s.start).getFullYear())));
  return [thisMonth, ...[...months].filter(k => k < thisMonth).sort().reverse(), ...[...years].filter(y => y <= thisMonth.slice(0, 4)).sort().reverse()]
    .map(k => period(k, now));
}

// The month a notice is due for, as the first Home of a new month sees it: {month, year} periods (either null) for the
// month before `now` and, in January, the year before, when they have sessions
export function due(log, now = Date.now()) {
  const d = new Date(now);
  const prev = period(monthKey(new Date(d.getFullYear(), d.getMonth() - 1, 1).getTime()), now);
  const has = p => (log?.sessions ?? []).some(s => isGame(s) && s.start >= p.start && s.start < p.end);
  const year = d.getMonth() === 0 ? period(String(d.getFullYear() - 1), now) : null;
  return { key: prev.key, month: has(prev) ? prev : null, year: year && has(year) ? year : null };
}

// Splits [from, to) at each local hour, calling fn(hourStart, ms)
function eachHour(from, to, fn) {
  for (let t = from; t < to;) {
    const d = new Date(t);
    const next = Math.min(to, new Date(d.getFullYear(), d.getMonth(), d.getDate(), d.getHours() + 1).getTime());
    fn(t, next - t);
    t = next;
  }
}

const top = (map, n) => [...map.entries()].sort((a, b) => b[1].secs - a[1].secs || a[0].localeCompare(b[0])).slice(0, n);

// Everything a recap shows, for period p. games: id → library or journal entry (for names, art and first play);
// captures: the user's screenshots [{game, path, thumb, takenAt}].
export function build(p, log, games = new Map(), captures = []) {
  const all = log?.sessions ?? [];
  const logStart = all.length ? Math.min(...all.map(s => s.start)) : null;
  const inP = s => s.start >= p.start && s.start < p.end;
  const lines = all.filter(inP);
  const sessions = lines.filter(isGame);
  const media = lines.filter(isMedia);
  const secs = sessions.reduce((n, s) => n + (s.secs ?? 0), 0);
  const name = id => games.get(id)?.name ?? id.split('-').map(w => w[0]?.toUpperCase() + w.slice(1)).join(' ');

  // by game
  const byGame = new Map();
  for (const s of sessions) {
    const g = byGame.get(s.game) ?? { secs: 0, sessions: 0 };
    g.secs += s.secs ?? 0;
    g.sessions += 1;
    byGame.set(s.game, g);
  }
  const topGames = top(byGame, 3).map(([id, g]) => ({ id, name: name(id), secs: g.secs, sessions: g.sessions, share: secs ? g.secs / secs : 0 }));

  // the longest session
  const longest = sessions.reduce((a, s) => (!a || s.secs > a.secs ? s : a), null);

  // when: weekday and part of day, by where the time went
  const weekdays = Array(7).fill(0);
  const parts = Object.fromEntries(PARTS.map(x => [x.id, 0]));
  const months = Array(12).fill(0);
  for (const s of sessions) {
    eachHour(s.start, s.start + (s.secs ?? 0) * 1000, (t, ms) => {
      const d = new Date(t);
      weekdays[(d.getDay() + 6) % 7] += ms / 1000;
      parts[partOf(d.getHours())] += ms / 1000;
    });
    months[new Date(s.start).getMonth()] += s.secs ?? 0;
  }
  const busiestDay = secs ? weekdays.indexOf(Math.max(...weekdays)) : null;
  const busiestPart = secs ? PARTS.reduce((a, x) => (parts[x.id] > parts[a.id] ? x : a), PARTS[0]) : null;
  const days = new Set(sessions.map(s => dayKey(s.start))).size;

  // where the time went (row B1b lines only): the share with the game in front
  const timed = sessions.filter(s => s.front);
  const timedSecs = timed.reduce((n, s) => n + (s.secs ?? 0), 0);
  const frontSecs = timed.reduce((n, s) => n + (s.front.game ?? 0), 0);
  const front = timed.length && timedSecs ? { secs: frontSecs, share: Math.min(1, frontSecs / timedSecs), sessions: timed.length, of: sessions.length } : null;

  // firsts: a game's first play is in this period (and in the log's time)
  const firstOf = new Map();
  for (const s of all.filter(isGame)) if (!firstOf.has(s.game) || s.start < firstOf.get(s.game)) firstOf.set(s.game, s.start);
  const firsts = [...byGame.keys()].filter(id => {
    const lib = games.get(id)?.firstPlayed;
    const first = Math.min(firstOf.get(id), lib ?? Infinity);
    return first >= p.start && first < p.end && first >= logStart;
  }).sort((a, b) => firstOf.get(a) - firstOf.get(b)).map(id => ({ id, name: name(id), at: firstOf.get(id) }));

  // finished: set to Finished or 100% complete in this period, and not set to something else after it in the period
  const last = new Map();
  for (const e of (log?.events ?? []).filter(e => e.type === 'status' && e.at >= p.start && e.at < p.end).sort((a, b) => a.at - b.at)) last.set(e.game, e);
  const finished = [...last.values()].filter(e => e.status === 'finished' || e.status === 'completed')
    .map(e => ({ id: e.game, name: name(e.game), status: e.status, at: e.at }));

  // trophies (replayed by the host) and achievements (B1b: before → after, when the game has an unlock file)
  const trophies = sessions.flatMap(s => (s.trophies ?? []).map(id => ({ id, game: s.game, gameName: name(s.game), at: s.end, ...TROPHIES.find(t => t.id === id) })))
    .filter(t => t.name);
  const achievements = sessions.reduce((n, s) => n + (s.achievements ? Math.max(0, s.achievements[1] - s.achievements[0]) : 0), 0);

  // media: by app
  const byApp = new Map();
  for (const s of media) {
    const a = byApp.get(s.app) ?? { secs: 0, sessions: 0 };
    a.secs += s.secs ?? 0;
    a.sessions += 1;
    byApp.set(s.app, a);
  }
  const mediaSecs = media.reduce((n, s) => n + (s.secs ?? 0), 0);

  // one screenshot: the newest of the top game, else the newest
  const shots = captures.filter(c => c.takenAt >= p.start && c.takenAt < p.end).sort((a, b) => b.takenAt - a.takenAt);
  const shot = shots.find(c => c.game === topGames[0]?.id) ?? shots[0] ?? null;

  return {
    period: p, secs, sessions: sessions.length, days, games: byGame.size, topGames,
    longest: longest && { id: longest.game, name: name(longest.game), secs: longest.secs, start: longest.start },
    weekdays, busiestDay, parts, busiestPart, months: p.kind === 'year' ? months : null, front,
    firsts, finished, trophies, achievements,
    media: mediaSecs ? { secs: mediaSecs, apps: top(byApp, 3).map(([id, a]) => ({ id, secs: a.secs, sessions: a.sessions })) } : null,
    shot, shots: shots.length,
    // the log began inside this period: the recap starts there
    since: logStart != null && logStart > p.start && logStart < p.end ? logStart : null,
  };
}

// "3 h 20 min", "25 min"; round: "42 h" from 10 h up (a recap's big numbers)
export function span(sec, round = false) {
  const min = Math.round(sec / 60), h = Math.floor(min / 60), m = min % 60;
  if (round && h >= 10) return `${Math.round(sec / 3600)} h`;
  return h ? `${h} h${m ? ` ${m} min` : ''}` : `${Math.max(sec ? 1 : 0, min)} min`;
}

// Everything the pages and the shelf need, from the host: the log, the games (library and journal) and the captures
export async function load(host) {
  const [log, list, journal, captures, media] = await Promise.all([
    host.library.recap().catch(() => null),
    host.library.list().catch(() => []),
    host.library.journal().catch(() => []),
    host.games.captures().catch(() => []),
    host.media?.list?.().catch(() => []) ?? [],
  ]);
  const games = new Map();
  for (const g of [...(journal ?? []), ...(list ?? [])]) games.set(g.id, g);
  return { log: log ?? { sessions: [], events: [] }, games, captures: captures ?? [], media: media ?? [] };
}

