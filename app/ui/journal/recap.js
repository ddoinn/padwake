// A recap (row B4): one month or one year of a player's play as full-screen pages, a console wrap-up of their own log.
// Pushed over the Library's Journal tab (A on a Recaps card): app.push('recap', {key: '2026-11' | '2026'}).
// Left / Right or LB / RB step through the pages, A goes on (and closes on the last), B closes.
// The numbers come from journal/recaps.js; a page with nothing to show isn't there (no screenshot, no media, …).
//
// Layout (stage px): the period's colour field (two soft glows, its hue from the month) under the page's game art,
// dimmed; the header (the recap mark 72 px at (56, 52), the period x 176 cap 77, the page count right at x 1748); each
// page's content from x 172, 1576 wide, between y 200 and 940; progress segments at y 1004 (1576 wide, 6 px gaps).
// Motion: a page leaves in 180 ms (fade, 48 px aside), the next comes in over 420 ms from the other side, its parts
// rising 24 px one after another (70 ms apart); big numbers count up over 700 ms. Transform and opacity only.

import { build, load, span, period, hueOf, WEEKDAYS, MONTHS, PARTS, MARK } from './recaps.js';
import { squareArt, tint } from './shared.js';
import { TIER_COLOR, CUP } from './trophies.js';

const FRAME = 1000 / 60;
const OUT = { duration: 180, easing: 'cubic-bezier(.4, 0, 1, 1)', fill: 'forwards' };
const IN = { duration: 420, easing: 'cubic-bezier(0, 0, .2, 1)' };
const RISE = { duration: 460, easing: 'cubic-bezier(.16, 1, .3, 1)' };
const STAGGER = 70, SIDE = 48, COUNT_MS = 700;

let app, host, root;
const $ = {};
let data = null, recap = null, pages = [], at = 0, onTop = false, busy = 0, counters = [];

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
      <div class="rfield"><div class="glow g1"></div><div class="glow g2"></div></div>
      <div class="rart"></div>
      <div class="rscrim"></div>
      <div class="rhead"><div class="icon">${MARK}</div><div class="t"></div><div class="n"></div></div>
      <div class="rpages"></div>
      <div class="rnav prev"><svg viewBox="0 0 24 24"><path d="M15 4l-8 8 8 8"/></svg></div>
      <div class="rnav next"><svg viewBox="0 0 24 24"><path d="M9 4l8 8-8 8"/></svg></div>
      <div class="rsegs"></div>`;
    for (const k of ['rfield', 'rart', 'rhead', 'rpages', 'rsegs']) $[k] = el.querySelector(`.${k}`);
    $.title = $.rhead.querySelector('.t');
    $.count = $.rhead.querySelector('.n');
    $.prev = el.querySelector('.rnav.prev');
    $.next = el.querySelector('.rnav.next');
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
    root.style.setProperty('--hue', hueOf(p));
    $.title.textContent = p.current ? `${p.name} so far` : p.title;
    root.classList.add('ready');
    root.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 260, easing: 'linear' });
    at = 0;
    paintSegs();
    show(0, 0);
  },

  leave() {
    onTop = false;
    counters.forEach(c => cancelAnimationFrame(c));
    counters = [];
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
  show(i, d);
  return true;
}

// ---------- drawing ----------
function show(i, dir) {
  const old = $.rpages.querySelector('.rpage.cur');
  if (old) {
    old.classList.remove('cur');
    old.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: `translateX(${-dir * SIDE}px)` }], OUT).finished
      .then(() => old.remove(), () => old.remove());
  }
  at = i;
  const page = pages[i];
  const el = document.createElement('div');
  el.className = `rpage p-${page.id} cur`;
  el.innerHTML = page.html;
  $.rpages.append(el);
  art(el.dataset.art ?? el.firstElementChild?.dataset.art ?? null);
  pictures(el);
  const delay = old ? 120 : 0;
  if (dir) el.animate([{ transform: `translateX(${dir * SIDE}px)` }, { transform: 'none' }], { ...IN, delay, fill: 'backwards' });
  el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 260, delay, easing: 'linear', fill: 'backwards' });
  [...el.querySelectorAll('.rise')].forEach((x, k) => x.animate([{ opacity: 0, transform: 'translateY(24px)' }, { opacity: 1, transform: 'none' }],
    { ...RISE, delay: delay + 60 + k * STAGGER, fill: 'backwards' }));
  [...el.querySelectorAll('.bar i, .col i')].forEach((x, k) => x.animate([{ transform: 'scaleY(0)' }, { transform: 'none' }],
    { duration: 520, delay: delay + 200 + k * 35, easing: 'cubic-bezier(.16, 1, .3, 1)', fill: 'backwards' }));
  [...el.querySelectorAll('.meter i')].forEach(x => x.animate([{ transform: 'scaleX(0)' }, { transform: 'none' }],
    { duration: 700, delay: delay + 260, easing: 'cubic-bezier(.16, 1, .3, 1)', fill: 'backwards' }));
  count(el, delay + 120);
  $.count.textContent = `${i + 1} / ${pages.length}`;
  $.prev.classList.toggle('on', i > 0);
  $.next.classList.toggle('on', i < pages.length - 1);
  paintSegs();
}

function paintSegs() {
  $.rsegs.innerHTML = pages.map((_, k) => `<b class="${k < at ? 'done' : k === at ? 'cur' : ''}"></b>`).join('');
}

// numbers count up: [data-count] holds the final value; data-fmt how to write it (span: seconds, pct: 0..1)
function count(el, delay) {
  for (const n of el.querySelectorAll('[data-count]')) {
    const to = Number(n.dataset.count), fmt = n.dataset.fmt;
    const write = v => { n.textContent = fmt === 'span' ? span(v, true) : fmt === 'pct' ? pct(v) : Math.round(v).toLocaleString(); };
    write(0);
    const t0 = performance.now() + delay;
    const tick = t => {
      const k = Math.min(1, Math.max(0, (t - t0) / COUNT_MS));
      write(to * (1 - (1 - k) ** 3));
      if (k < 1 && onTop && n.isConnected) counters.push(requestAnimationFrame(tick));
    };
    counters.push(requestAnimationFrame(tick));
  }
}

// the page's game art behind it, cross-faded
let artPath = null;
function art(id) {
  const g = id ? data.games.get(id) : null;
  const path = g?.art?.background ?? g?.art?.tile ?? null;
  if (path === artPath) return;
  artPath = path;
  for (const img of $.rart.querySelectorAll('img')) {
    img.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 340, easing: 'linear', fill: 'forwards' }).finished.then(() => img.remove(), () => img.remove());
  }
  if (!path) return;
  const img = new Image();
  img.alt = '';
  img.src = host.fileUrl(path);
  img.decode().then(() => {
    if (artPath !== path) return;
    $.rart.append(img);
    img.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 340, easing: 'linear' });
  }, () => {});
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
const big = (sec, cls = '') => `<span class="${cls}" data-count="${sec}" data-fmt="span">${span(sec, true)}</span>`;

const PAGES = [
  ['cover', r => {
    const p = r.period, lead = r.topGames[0];
    if (!r.sessions) {
      return `<div data-art="">
        <div class="kicker rise">${p.current ? (p.kind === 'year' ? 'Your year so far' : 'Your month so far') : 'Your recap'}</div>
        <div class="huge rise">${esc(p.current ? `${p.name} so far` : p.title)}</div>
        <div class="lede rise">Nothing played yet${p.current ? ` this ${p.kind}` : ''}.</div>
        <div class="note rise">Play a game and your recap starts filling in.</div></div>`;
    }
    const sub = [plural(r.sessions, 'session'), plural(r.days, 'day'), plural(r.games, 'game')].join('  ·  ');
    return `<div data-art="${esc(lead?.id ?? '')}">
      <div class="kicker rise">${p.current ? (p.kind === 'year' ? 'Your year so far' : 'Your month so far') : p.kind === 'year' ? 'Your year in games' : 'Your month in games'}</div>
      <div class="huge rise">${esc(p.name)}${p.kind === 'month' ? `<small> ${p.year}</small>` : ''}</div>
      <div class="lede rise">${big(r.secs, 'em')} played</div>
      <div class="sub rise">${sub}</div>
      ${r.since ? `<div class="note rise">Counted from ${dayDate(r.since).replace(/^\w+ /, '')}, when Padwake began keeping your log.</div>` : ''}
    </div>`;
  }],
  ['time', r => {
    if (!r.sessions) return null;
    const avg = r.secs / r.sessions;
    return `<div data-art="${esc(r.topGames[0]?.id ?? '')}">
      <div class="kicker rise">Time played</div>
      <div class="giant rise">${big(r.secs)}</div>
      <div class="lede rise">over ${plural(r.sessions, 'session')} on ${plural(r.days, 'day')}</div>
      <div class="facts">
        <div class="fact rise"><div class="v">${span(avg)}</div><div class="l">An average session</div></div>
        ${r.front ? `<div class="fact rise"><div class="v"><span data-count="${r.front.share}" data-fmt="pct">${pct(r.front.share)}</span></div>
          <div class="l">With the game in front</div><div class="meter"><i style="width:${(r.front.share * 100).toFixed(1)}%"></i></div></div>` : ''}
        ${r.front ? `<div class="fact rise"><div class="v">${span(r.front.secs)}</div><div class="l">${r.front.sessions === r.sessions ? 'Really playing' : `Really playing, in ${plural(r.front.sessions, 'session')} timed`}</div></div>` : ''}
      </div></div>`;
  }],
  ['top', r => {
    if (!r.topGames.length) return null;
    const [first, ...rest] = r.topGames;
    const item = (g, n, cls) => `<div class="tg ${cls} rise">${box(g.id, g.name)}<div class="rank">${n}</div>
      <div class="gn">${esc(g.name)}</div><div class="gt">${span(g.secs)}  ·  ${plural(g.sessions, 'session')}</div>
      <div class="meter"><i style="width:${(g.share * 100).toFixed(1)}%"></i></div><div class="gs">${pct(g.share)} of your time</div></div>`;
    return `<div data-art="${esc(first.id)}">
      <div class="kicker rise">${r.topGames.length === 1 ? 'Your game' : `Your top ${r.topGames.length}`}</div>
      <div class="tgs">${item(first, 1, 'first')}${rest.map((g, k) => item(g, k + 2, 'rest')).join('')}</div></div>`;
  }],
  ['longest', r => {
    if (!r.longest || r.sessions < 2) return null;
    const l = r.longest;
    return `<div data-art="${esc(l.id)}">
      <div class="kicker rise">Your longest session</div>
      <div class="giant rise">${big(l.secs)}</div>
      <div class="lede rise">of ${esc(l.name)}</div>
      <div class="sub rise">${dayDate(l.start)}, from ${clock(l.start)}</div>
      ${box(l.id, l.name, 'hero rise')}</div>`;
  }],
  ['rhythm', r => {
    if (!r.sessions) return null;
    const max = Math.max(...r.weekdays, 1), part = r.busiestPart;
    const cols = r.weekdays.map((s, k) => `<div class="col${k === r.busiestDay ? ' top' : ''}"><div class="h">${s ? span(s) : ''}</div>
      <div class="well"><i style="height:${((s / max) * 100).toFixed(1)}%"></i></div><div class="d">${WEEKDAYS[k].slice(0, 3)}</div></div>`).join('');
    const partSecs = Object.values(r.parts).reduce((a, b) => a + b, 0) || 1;
    return `<div data-art="">
      <div class="kicker rise">When you played</div>
      <div class="lede big rise">You were ${part.who}.</div>
      <div class="sub rise">${WEEKDAYS[r.busiestDay]}s were your busiest day.</div>
      <div class="cols rise">${cols}</div>
      <div class="parts rise">${PARTS.map(x => `<div class="part${x.id === part.id ? ' top' : ''}"><div class="v">${pct(r.parts[x.id] / partSecs)}</div><div class="l">${x.label}</div></div>`).join('')}</div>
    </div>`;
  }],
  ['months', r => {
    if (!r.months || r.months.filter(Boolean).length < 2) return null;
    const max = Math.max(...r.months, 1), best = r.months.indexOf(Math.max(...r.months));
    return `<div data-art="">
      <div class="kicker rise">Month by month</div>
      <div class="lede big rise">${MONTHS[best]} was your biggest month.</div>
      <div class="sub rise">${span(r.months[best])} played</div>
      <div class="cols months rise">${r.months.map((s, k) => `<div class="col${k === best ? ' top' : ''}"><div class="h">${s ? span(s) : ''}</div>
        <div class="well"><i style="height:${((s / max) * 100).toFixed(1)}%"></i></div><div class="d">${MONTHS[k].slice(0, 3)}</div></div>`).join('')}</div>
    </div>`;
  }],
  ['firsts', r => {
    if (!r.firsts.length && !r.finished.length) return null;
    const list = (items, line) => items.slice(0, 4).map(g => `<div class="gl rise">${box(g.id, g.name, 'small')}<div class="gn">${esc(g.name)}</div><div class="gt">${line(g)}</div></div>`).join('');
    const more = n => (n > 4 ? `<div class="more rise">and ${n - 4} more</div>` : '');
    return `<div data-art="${esc((r.finished[0] ?? r.firsts[0]).id)}">
      <div class="kicker rise">${r.firsts.length && r.finished.length ? 'Firsts and finishes' : r.firsts.length ? 'New to you' : 'Finished'}</div>
      <div class="two">
        ${r.firsts.length ? `<div class="side"><div class="head rise"><b data-count="${r.firsts.length}">${r.firsts.length}</b> ${r.firsts.length === 1 ? 'game' : 'games'} played for the first time</div>
          ${list(r.firsts, g => `First played ${dayDate(g.at)}`)}${more(r.firsts.length)}</div>` : ''}
        ${r.finished.length ? `<div class="side"><div class="head rise"><b data-count="${r.finished.length}">${r.finished.length}</b> ${r.finished.length === 1 ? 'game' : 'games'} finished</div>
          ${list(r.finished, g => `${g.status === 'completed' ? '100% complete' : 'Finished'} ${dayDate(g.at)}`)}${more(r.finished.length)}</div>` : ''}
      </div></div>`;
  }],
  ['trophies', r => {
    if (!r.trophies.length && !r.achievements) return null;
    const tiers = ['gold', 'silver', 'bronze'].map(t => [t, r.trophies.filter(x => x.tier === t).length]).filter(([, n]) => n);
    return `<div data-art="${esc(r.trophies[0]?.game ?? '')}">
      <div class="kicker rise">Trophies</div>
      ${r.trophies.length ? `<div class="giant rise"><span data-count="${r.trophies.length}">${r.trophies.length}</span><small> ${r.trophies.length === 1 ? 'trophy' : 'trophies'} earned</small></div>
        <div class="tiers rise">${tiers.map(([t, n]) => `<span class="tier" style="color:${TIER_COLOR[t]}">${CUP}<b>${n}</b> ${t[0].toUpperCase() + t.slice(1)}</span>`).join('')}</div>
        <div class="cups">${r.trophies.slice(0, 4).map(t => `<div class="cup rise"><div class="c" style="color:${TIER_COLOR[t.tier]}">${CUP}</div>
          <div class="cn">${esc(t.name)}</div><div class="cg">${esc(t.gameName)}  ·  ${esc(t.goal)}</div></div>`).join('')}</div>` : ''}
      ${r.achievements ? `<div class="ach rise"><b data-count="${r.achievements}">${r.achievements}</b> ${r.achievements === 1 ? 'achievement' : 'achievements'} unlocked in your games</div>` : ''}
    </div>`;
  }],
  ['moment', r => {
    if (!r.shot) return null;
    const s = r.shot;
    return `<div data-art="">
      <div class="kicker rise">A moment from ${esc(r.period.name)}</div>
      <div class="shot rise"><img alt="" src="${esc(host.fileUrl(s.path))}"></div>
      <div class="cap rise"><div class="gn">${esc(data.games.get(s.game)?.name ?? s.name ?? '')}</div><div class="gt">${dayDate(s.takenAt)}, ${clock(s.takenAt)}${r.shots > 1 ? `  ·  1 of ${plural(r.shots, 'screenshot')}` : ''}</div></div>
    </div>`;
  }],
  ['media', r => {
    if (!r.media) return null;
    const app = id => data.media?.find(m => m.id === id);
    return `<div data-art="">
      <div class="kicker rise">Also on Padwake</div>
      <div class="giant rise">${big(r.media.secs)}<small> of watching and listening</small></div>
      <div class="apps">${r.media.apps.map(a => `<div class="app rise"><div class="ai">${app(a.id)?.art?.tile ? `<img alt="" src="${esc(host.fileUrl(app(a.id).art.tile))}">` : ''}</div>
        <div class="gn">${esc(app(a.id)?.name ?? a.id)}</div><div class="gt">${span(a.secs)}  ·  ${plural(a.sessions, 'time')}</div></div>`).join('')}</div>
    </div>`;
  }],
  ['summary', r => {
    if (!r.sessions) return null;
    const p = r.period, lead = r.topGames[0];
    const cell = (v, l) => `<div class="sc"><div class="v">${v}</div><div class="l">${l}</div></div>`;
    return `<div data-art="${esc(lead.id)}">
      <div class="kicker rise">${p.current ? `${p.name} so far` : `That was ${p.name}`}</div>
      <div class="card rise">
        <div class="ch">${box(lead.id, lead.name, 'lead')}<div><div class="cl">Your game</div><div class="cn">${esc(lead.name)}</div><div class="ct">${span(lead.secs)}</div></div></div>
        <div class="cells">
          ${cell(span(r.secs, true), 'Played')}${cell(r.sessions.toLocaleString(), r.sessions === 1 ? 'Session' : 'Sessions')}
          ${cell(r.games.toLocaleString(), r.games === 1 ? 'Game' : 'Games')}${cell(r.longest ? span(r.longest.secs) : '—', 'Longest session')}
          ${cell(WEEKDAYS[r.busiestDay], 'Busiest day')}${cell(r.busiestPart.label, 'Favourite time')}
          ${cell(r.trophies.length.toLocaleString(), r.trophies.length === 1 ? 'Trophy' : 'Trophies')}${cell(r.finished.length.toLocaleString(), 'Finished')}
        </div>
      </div>
      <div class="note rise">A to close</div>
    </div>`;
  }],
];
