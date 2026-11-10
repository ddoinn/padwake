// A recap (row B4): one month or one year of a player's play as full-screen pages, from their own log.
// Pushed over the Library's Journal tab (A on a Recaps card): app.push('recap', {key: '2026-11' | '2026'}).
// Left / Right or LB / RB step through the pages, A goes on (and closes on the last), B closes.
// The numbers come from journal/recaps.js; a page with nothing to show isn't there (no screenshot, no media, …).
//
// Layout (stage px), the journal page's: the page's game art under its scrims (the only colour on screen, as on Home);
// the header (the top game's tile 72 px at (56, 52), the period x 176 cap 77, "3 / 8" right at x 1748); each page's
// content from x 172, 1576 wide, between y 200 and 940. White and the journal's greys only; our cup for trophies.
// Motion: the journal page's opening (12 frames, opacity and 24 px up) for the screen and for each page; the art
// cross-fades in 340 ms as there. Nothing else moves.

import { btn } from '../input/glyphs.js';
import { build, load, span, period, WEEKDAYS, MONTHS, PARTS } from './recaps.js';
import { squareArt, tint } from './shared.js';
import { TIER_COLOR, CUP } from './trophies.js';

const FRAME = 1000 / 60;
const IN = { duration: 12 * FRAME, easing: 'cubic-bezier(0, 0, .2, 1)' }; // as journal.js

let app, host, root;
const $ = {};
let data = null, recap = null, pages = [], at = 0, onTop = false;

const plural = (n, one, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
const pct = x => `${Math.round(x * 100)}%`;
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const dayName = t => WEEKDAYS[(new Date(t).getDay() + 6) % 7];
const dayDate = t => { const d = new Date(t); return `${dayName(t)} ${d.getDate()} ${MONTHS[d.getMonth()]}`; };
const clock = t => { const d = new Date(t); return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`; };

export default {
  mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `
      <div class="rart"></div>
      <div class="rscrim"></div>
      <div class="rhead"><div class="icon"></div><div class="t"></div><div class="n"></div></div>
      <div class="rpages"></div>`;
    for (const k of ['rart', 'rhead', 'rpages']) $[k] = el.querySelector(`.${k}`);
    $.icon = $.rhead.querySelector('.icon');
    $.title = $.rhead.querySelector('.t');
    $.count = $.rhead.querySelector('.n');
    window.__recap = {
      state: () => ({ key: recap?.period.key, page: pages[at]?.id, pages: pages.map(p => p.id), index: at }),
      recap: () => recap && structuredClone({ ...recap, period: { ...recap.period } }),
    };
  },

  async enter(params = {}) {
    onTop = true;
    if (params.back) return;
    root.classList.remove('ready');
    $.rpages.replaceChildren();
    data = await load(host);
    if (!onTop) return;
    const p = period(params.key ?? '');
    recap = build(p, data.log, data.games, data.captures);
    pages = PAGES.map(([id, fn]) => ({ id, html: fn(recap) })).filter(x => x.html);
    $.title.textContent = p.current ? `${p.name} so far` : p.title;
    $.icon.replaceChildren();
    const lead = recap.topGames[0];
    if (lead) { $.icon.dataset.game = lead.id; pictures($.rhead); }
    root.classList.add('ready');
    root.animate([{ opacity: 0 }, { opacity: 1 }], IN);
    at = 0;
    show(0);
  },

  leave() {
    onTop = false;
  },

  action(name, e = {}) {
    if (name === 'b') { app.pop(); return 'back'; }
    if (!pages.length) return false;
    if (e.held && name !== 'left' && name !== 'right') return false;
    if (name === 'right' || name === 'rb' || name === 'click') return step(1);
    if (name === 'left' || name === 'lb') return step(-1);
    if (name === 'a') return at === pages.length - 1 ? (app.pop(), 'back') : step(1);
    if (name === 'wheel') return e.dy ? step(e.dy > 0 ? 1 : -1) || 'none' : 'none';
    return false;
  },
};

function step(d) {
  const i = at + d;
  if (i < 0 || i >= pages.length) return false;
  show(i);
  return true;
}

// ---------- drawing ----------
// a page replaces the last one at once and comes in as the journal page does
function show(i) {
  $.rpages.replaceChildren();
  at = i;
  const page = pages[i];
  const el = document.createElement('div');
  el.className = `rpage p-${page.id}`;
  el.innerHTML = page.html;
  $.rpages.append(el);
  art(el.firstElementChild?.dataset.art || null);
  pictures(el);
  el.animate([{ opacity: 0, transform: 'translateY(24px)' }, { opacity: 1, transform: 'none' }], IN);
  $.count.textContent = `${i + 1} / ${pages.length}`;
}

// the page's game art behind it, cross-faded (the journal page's 340 ms opacity transition)
let artPath = null;
function art(id) {
  const g = id ? data.games.get(id) : null;
  const path = g?.art?.background ?? g?.art?.tile ?? null;
  if (path === artPath) return;
  artPath = path;
  for (const img of $.rart.querySelectorAll('img')) {
    img.classList.remove('in');
    setTimeout(() => img.remove(), 340);
  }
  if (!path) return;
  const img = new Image();
  img.alt = '';
  img.src = host.fileUrl(path);
  $.rart.append(img);
  img.decode().then(() => { if (artPath === path) img.classList.add('in'); }, () => {});
}

// square art for [data-game] boxes: the tile, else the top of the cover, else the game's colour and name
function pictures(el) {
  for (const box of el.querySelectorAll('[data-game]')) {
    const g = data.games.get(box.dataset.game);
    const src = g && squareArt(g);
    const none = () => {
      box.style.background = tint(box.dataset.game);
      box.append(Object.assign(document.createElement('span'), { className: 'name', textContent: g?.name ?? box.dataset.name ?? '' }));
    };
    if (!src) { none(); continue; }
    const img = new Image();
    img.alt = '';
    img.decoding = 'async';
    img.src = host.fileUrl(src);
    if (!g.art?.tile) img.classList.add('portrait');
    img.onerror = () => { img.remove(); none(); };
    box.append(img);
  }
}

// ---------- the pages: [id, recap → html, or null when it has nothing to show] ----------
const box = (id, name, cls = '') => `<div class="rtile ${cls}" data-game="${esc(id)}" data-name="${esc(name)}"></div>`;
const big = (sec, cls = '') => `<span class="${cls}">${span(sec, true)}</span>`;

const PAGES = [
  ['cover', r => {
    const p = r.period, lead = r.topGames[0];
    if (!r.sessions) {
      return `<div data-art="">
        <div class="kicker">${p.current ? (p.kind === 'year' ? 'Your year so far' : 'Your month so far') : 'Your recap'}</div>
        <div class="huge">${esc(p.current ? `${p.name} so far` : p.title)}</div>
        <div class="lede">Nothing played yet${p.current ? ` this ${p.kind}` : ''}.</div>
        <div class="note">Play a game and your recap starts filling in.</div></div>`;
    }
    const sub = [plural(r.sessions, 'session'), plural(r.days, 'day'), plural(r.games, 'game')].join('  ·  ');
    return `<div data-art="${esc(lead?.id ?? '')}">
      <div class="kicker">${p.current ? (p.kind === 'year' ? 'Your year so far' : 'Your month so far') : p.kind === 'year' ? 'Your year in games' : 'Your month in games'}</div>
      <div class="huge">${esc(p.name)}${p.kind === 'month' ? `<small> ${p.year}</small>` : ''}</div>
      <div class="lede">${big(r.secs, 'em')} played</div>
      <div class="sub">${sub}</div>
      ${r.since ? `<div class="note">Counted from ${dayDate(r.since).replace(/^\w+ /, '')}, when Padwake began keeping your log.</div>` : ''}
    </div>`;
  }],
  ['time', r => {
    if (!r.sessions) return null;
    const avg = r.secs / r.sessions;
    return `<div data-art="${esc(r.topGames[0]?.id ?? '')}">
      <div class="kicker">Time played</div>
      <div class="giant">${big(r.secs)}</div>
      <div class="lede">over ${plural(r.sessions, 'session')} on ${plural(r.days, 'day')}</div>
      <div class="facts">
        <div class="fact"><div class="v">${span(avg)}</div><div class="l">An average session</div></div>
        ${r.front ? `<div class="fact"><div class="v"><span>${pct(r.front.share)}</span></div>
          <div class="l">With the game in front</div><div class="meter"><i style="width:${(r.front.share * 100).toFixed(1)}%"></i></div></div>` : ''}
        ${r.front ? `<div class="fact"><div class="v">${span(r.front.secs)}</div><div class="l">${r.front.sessions === r.sessions ? 'Really playing' : `Really playing, in ${plural(r.front.sessions, 'session')} timed`}</div></div>` : ''}
      </div></div>`;
  }],
  ['top', r => {
    if (!r.topGames.length) return null;
    const [first, ...rest] = r.topGames;
    const item = (g, n, cls) => `<div class="tg ${cls}">${box(g.id, g.name)}<div class="rank">#${n}</div>
      <div class="gn">${esc(g.name)}</div><div class="gt">${span(g.secs)}  ·  ${plural(g.sessions, 'session')}</div>
      <div class="meter"><i style="width:${(g.share * 100).toFixed(1)}%"></i></div><div class="gs">${pct(g.share)} of your time</div></div>`;
    return `<div data-art="${esc(first.id)}">
      <div class="kicker">${r.topGames.length === 1 ? 'Your game' : `Your top ${r.topGames.length}`}</div>
      <div class="tgs">${item(first, 1, 'first')}${rest.map((g, k) => item(g, k + 2, 'rest')).join('')}</div></div>`;
  }],
  ['longest', r => {
    if (!r.longest || r.sessions < 2) return null;
    const l = r.longest;
    return `<div data-art="${esc(l.id)}">
      <div class="kicker">Your longest session</div>
      <div class="giant">${big(l.secs)}</div>
      <div class="lede">of ${esc(l.name)}</div>
      <div class="sub">${dayDate(l.start)}, from ${clock(l.start)}</div>
      ${box(l.id, l.name, 'hero')}</div>`;
  }],
  ['rhythm', r => {
    if (!r.secs) return null;
    const max = Math.max(...r.weekdays, 1), part = r.busiestPart;
    const cols = r.weekdays.map((s, k) => `<div class="col${k === r.busiestDay ? ' top' : ''}"><div class="h">${s ? span(s) : ''}</div>
      <div class="well"><i style="height:${((s / max) * 100).toFixed(1)}%"></i></div><div class="d">${WEEKDAYS[k].slice(0, 3)}</div></div>`).join('');
    const partSecs = Object.values(r.parts).reduce((a, b) => a + b, 0) || 1;
    return `<div data-art="">
      <div class="kicker">When you played</div>
      <div class="lede big">You were ${part.who}.</div>
      <div class="sub">${WEEKDAYS[r.busiestDay]}s were your busiest day.</div>
      <div class="cols">${cols}</div>
      <div class="parts">${PARTS.map(x => `<div class="part${x.id === part.id ? ' top' : ''}"><div class="v">${pct(r.parts[x.id] / partSecs)}</div><div class="l">${x.label}</div></div>`).join('')}</div>
    </div>`;
  }],
  ['months', r => {
    if (!r.months || r.months.filter(Boolean).length < 2) return null;
    const max = Math.max(...r.months, 1), best = r.months.indexOf(Math.max(...r.months));
    return `<div data-art="">
      <div class="kicker">Month by month</div>
      <div class="lede big">${MONTHS[best]} was your biggest month.</div>
      <div class="sub">${span(r.months[best])} played</div>
      <div class="cols months">${r.months.map((s, k) => `<div class="col${k === best ? ' top' : ''}"><div class="h">${s ? span(s) : ''}</div>
        <div class="well"><i style="height:${((s / max) * 100).toFixed(1)}%"></i></div><div class="d">${MONTHS[k].slice(0, 3)}</div></div>`).join('')}</div>
    </div>`;
  }],
  ['firsts', r => {
    if (!r.firsts.length && !r.finished.length) return null;
    const list = (items, line) => items.slice(0, 4).map(g => `<div class="gl">${box(g.id, g.name, 'small')}<div class="gn">${esc(g.name)}</div><div class="gt">${line(g)}</div></div>`).join('');
    const more = n => (n > 4 ? `<div class="more">and ${n - 4} more</div>` : '');
    return `<div data-art="${esc((r.finished[0] ?? r.firsts[0]).id)}">
      <div class="kicker">${r.firsts.length && r.finished.length ? 'Firsts and finishes' : r.firsts.length ? 'New to you' : 'Finished'}</div>
      <div class="two">
        ${r.firsts.length ? `<div class="side"><div class="head"><b>${r.firsts.length}</b> ${r.firsts.length === 1 ? 'game' : 'games'} played for the first time</div>
          ${list(r.firsts, g => `First played ${dayDate(g.at)}`)}${more(r.firsts.length)}</div>` : ''}
        ${r.finished.length ? `<div class="side"><div class="head"><b>${r.finished.length}</b> ${r.finished.length === 1 ? 'game' : 'games'} finished</div>
          ${list(r.finished, g => `${g.status === 'completed' ? '100% complete' : 'Finished'} ${dayDate(g.at)}`)}${more(r.finished.length)}</div>` : ''}
      </div></div>`;
  }],
  ['trophies', r => {
    if (!r.trophies.length && !r.achievements) return null;
    const tiers = ['gold', 'silver', 'bronze'].map(t => [t, r.trophies.filter(x => x.tier === t).length]).filter(([, n]) => n);
    return `<div data-art="${esc(r.trophies[0]?.game ?? '')}">
      <div class="kicker">Trophies</div>
      ${r.trophies.length ? `<div class="giant"><span>${r.trophies.length}</span><small> ${r.trophies.length === 1 ? 'trophy' : 'trophies'} earned</small></div>
        <div class="tiers">${tiers.map(([t, n]) => `<span class="tier" style="color:${TIER_COLOR[t]}">${CUP}<b>${n}</b> ${t[0].toUpperCase() + t.slice(1)}</span>`).join('')}</div>
        <div class="cups">${r.trophies.slice(0, 4).map(t => `<div class="cup"><div class="c" style="color:${TIER_COLOR[t.tier]}">${CUP}</div>
          <div class="cn">${esc(t.name)}</div><div class="cg">${esc(t.gameName)}  ·  ${esc(t.goal)}</div></div>`).join('')}</div>` : ''}
      ${r.achievements ? `<div class="ach"><b>${r.achievements}</b> ${r.achievements === 1 ? 'achievement' : 'achievements'} unlocked in your games</div>` : ''}
    </div>`;
  }],
  ['moment', r => {
    if (!r.shot || !r.sessions) return null;
    const s = r.shot;
    return `<div data-art="">
      <div class="kicker">A moment from ${esc(r.period.name)}</div>
      <div class="shot"><img alt="" src="${esc(host.fileUrl(s.path))}"></div>
      <div class="cap"><div class="gn">${esc(data.games.get(s.game)?.name ?? s.name ?? '')}</div><div class="gt">${dayDate(s.takenAt)}, ${clock(s.takenAt)}${r.shots > 1 ? `  ·  1 of ${plural(r.shots, 'screenshot')}` : ''}</div></div>
    </div>`;
  }],
  ['media', r => {
    if (!r.media) return null;
    const app = id => data.media?.find(m => m.id === id);
    return `<div data-art="">
      <div class="kicker">Also on Padwake</div>
      <div class="giant">${big(r.media.secs)}</div>
      <div class="lede">of watching and listening</div>
      <div class="apps">${r.media.apps.map(a => `<div class="app"><div class="ai">${app(a.id)?.art?.tile ? `<img alt="" src="${esc(host.fileUrl(app(a.id).art.tile))}">` : ''}</div>
        <div class="gn">${esc(app(a.id)?.name ?? a.id)}</div><div class="gt">${span(a.secs)}  ·  ${plural(a.sessions, 'time')}</div></div>`).join('')}</div>
    </div>`;
  }],
  ['summary', r => {
    if (!r.sessions) return null;
    const p = r.period, lead = r.topGames[0];
    const cell = (v, l) => `<div class="sc"><div class="v">${v}</div><div class="l">${l}</div></div>`;
    return `<div data-art="${esc(lead.id)}">
      <div class="kicker">${p.current ? `${p.name} so far` : `That was ${p.name}`}</div>
      <div class="card">
        <div class="ch">${box(lead.id, lead.name, 'lead')}<div><div class="cl">Your game</div><div class="cn">${esc(lead.name)}</div><div class="ct">${span(lead.secs)}</div></div></div>
        <div class="cells">
          ${cell(span(r.secs, true), 'Played')}${cell(r.sessions.toLocaleString(), r.sessions === 1 ? 'Session' : 'Sessions')}
          ${cell(r.games.toLocaleString(), r.games === 1 ? 'Game' : 'Games')}${cell(r.longest ? span(r.longest.secs) : '—', 'Longest session')}
          ${cell(WEEKDAYS[r.busiestDay] ?? '—', 'Busiest day')}${cell(r.busiestPart?.label ?? '—', 'Favourite time')}
          ${cell(r.trophies.length.toLocaleString(), r.trophies.length === 1 ? 'Trophy' : 'Trophies')}${cell(r.finished.length.toLocaleString(), 'Finished')}
        </div>
      </div>
      <div class="note">${btn('a')} to close</div>
    </div>`;
  }],
];
