// Welcome Back: Who's using this controller? The user select screen, and its Add User screen.
//   app.push('users', {over: 'start', onPick})  at boot, after "Press any button" (start.js) when there are two or more
//                                               users: drawn over the start screen's light, B does nothing, and onPick
//                                               runs once the switch is done (the start screen then reveals Home)
//   app.push('users')                           Switch User from Home: its own spotlight; B goes back
//   app.push('users', {add: true})              straight to Add User (Settings → Users); done or cancelled, it goes back
// Layout (from the PS5's): the clock top right; the title and question centred at the top; Add User then everyone in a
// row with the focused one in the middle, larger, ringed, the controller glyph over it; a power button under the row
// (Control Center's power menu); "Select" bottom right. A switch is the host's (src-tauri users/mod.rs): Home and the
// Journal follow its `library` event.
// Add User: a big avatar (a random popular character; A shows another), the name (A types it on the on-screen
// keyboard), Cancel and OK. Settings → Users changes the avatar later, from characters, your pictures or ours.

import * as keyboard from '../keyboard/keyboard.js';
import { mount as spotlight } from '../background/spotlight.js';
import { avatarHtml, nextAvatar } from './avatars.js';
import * as store from './store.js';

const BIG = 300, SMALL_K = 218 / 300, NEAR = 358, PITCH = 336, CX = 960;
const EASE_OUT = 'cubic-bezier(0, 0, .2, 1)';
const PLUS = '<svg viewBox="0 0 64 64" style="width:30%;height:30%"><path d="M29 10h6v19h19v6H35v19h-6V35H10v-6h19Z" fill="#fff"/></svg>';
const PAD = '<svg viewBox="0 0 64 64"><path fill="currentColor" d="M20 20h24c7.7 0 13 6.4 13.8 15.2l.9 9.6c.4 4.5-2.9 7.2-6.3 7.2-2.5 0-4.4-1.4-5.8-3.4L42.8 43H21.2l-3.8 5.6c-1.4 2-3.3 3.4-5.8 3.4-3.4 0-6.7-2.7-6.3-7.2l.9-9.6C7 26.4 12.3 20 20 20Zm-1.5 8v4h-4v4h4v4h4v-4h4v-4h-4v-4Zm25.5 0a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2Zm-5.5 5.5a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2Zm11 0a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2Z"/></svg>';
const POWER = '<svg viewBox="0 0 32 32"><path d="M10.2 8.6a10.4 10.4 0 1 0 11.6 0" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"/><path d="M16 4.5v11" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"/></svg>';
const CHECK = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="11" fill="#fff"/><path d="m7 12.5 3.2 3.2L17.5 8.5" fill="none" stroke="#111" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const A_GLYPH = '<i class="ua">A</i>';

let app, host, root, $ = {}, spot = null;
let index = 0, cards = [], params = {}, kb = null, busy = false;
let zone = 'row';            // row | power | add
// Add User: what is focused, the name, the avatar shown, and the popular characters to show
let add = null;              // {focus: 'avatar' | 'name' | 'cancel' | 'ok', name, pick: character | null, chars, k}

export default {
  mount(el, a) {
    app = a;
    host = a.host;
    root = el;
    el.innerHTML = `<div class="ushade"></div><span class="uclock"></span>
      <div class="upick">
        <h1 class="utitle"></h1><p class="usub">Who's using this controller?</p>
        <div class="upad">${PAD}</div>
        <div class="urow"></div>
        <div class="upower" data-nav="upower">${POWER}</div>
      </div>
      <div class="uadd">
        <h1 class="utitle">Add User</h1>
        <div class="uav" data-nav="add:avatar"><div class="udisc"></div><span class="uavhint">${A_GLYPH} Another character</span></div>
        <div class="ufield" data-nav="add:name"><label>Name</label><span class="uval"></span><i class="ucheck">${CHECK}</i></div>
        <div class="ubtns"><div class="upill" data-nav="add:cancel">Cancel</div><div class="upill" data-nav="add:ok">OK</div></div>
      </div>
      <div class="umsg"></div>
      <div class="uhint">${A_GLYPH}<span>Select</span></div>`;
    for (const k of ['upick', 'urow', 'upad', 'upower', 'uadd', 'umsg', 'uclock']) $[k] = el.querySelector(`.${k}`);
    $.title = el.querySelector('.upick .utitle');
    $.disc = el.querySelector('.uav .udisc');
    $.val = el.querySelector('.ufield .uval');
    $.field = el.querySelector('.ufield');
    $.pills = [...el.querySelectorAll('.upill')];
    $.av = el.querySelector('.uav');
    clock($.uclock);
    store.onUsers(() => { if (root.classList.contains('on') && !busy && zone !== 'add') draw(); });
    // read-only state for tests (tools/app/ui-probe.mjs eval steps)
    window.__users = { state: () => ({ index, zone, cards: cards.map(c => c.user?.name ?? 'add'), over: params.over ?? null, kb: !!kb,
      msg: $.umsg.textContent, add: add && { focus: add.focus, name: add.name, pick: add.pick?.name ?? null } }) };
  },

  async enter(p = {}) {
    if (p.back) return;
    params = p;
    busy = false;
    add = null;
    msg('');
    root.classList.toggle('over-start', p.over === 'start');
    if (p.over !== 'start' && !spot) spot = spotlight(root); // over the start screen, its light shows through
    root.getAnimations().forEach(a => a.cancel()); // the last pick's fade-out
    $.title.textContent = p.over === 'start' ? 'Welcome Back to Padwake' : 'Switch User';
    await store.ready;
    if (p.add) return openAdd();
    showPick();
    draw(store.me()?.id);
    root.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 250, easing: EASE_OUT });
    $.urow.animate([{ opacity: 0, transform: 'translateY(24px)' }, { opacity: 1, transform: 'none' }], { duration: 400, delay: 80, easing: EASE_OUT, fill: 'backwards' });
  },

  leave() {
    kb?.close();
    kb = null;
  },

  action(name, e = {}) {
    if (kb?.isOpen) return kb.action(name, e);
    if (busy) return 'none';
    if (zone === 'add') return addAction(name, e);
    const t = e.target ?? '';
    if (name === 'point') {
      if (t.startsWith('user:')) { const i = +t.slice(5); zoneRow(); return i === index ? false : focus(i); }
      if (t === 'upower') return zone === 'power' ? false : zonePower();
      return 'none';
    }
    if (name === 'click') {
      if (t.startsWith('user:')) { zoneRow(); focus(+t.slice(5)); name = 'a'; }
      else if (t === 'upower') { zonePower(); name = 'a'; }
      else return 'none';
    }
    if (zone === 'power') {
      if (name === 'up') return zoneRow();
      if (e.held) return false;
      if (name === 'a') { app.push('control-center', { power: true }); return 'select'; }
      if (name === 'b') return back();
      return 'none';
    }
    if (name === 'left' || name === 'right') return focus(index + (name === 'left' ? -1 : 1)) || (e.held ? false : 'none');
    if (name === 'down') return zonePower();
    if (e.held) return false;
    if (name === 'a') { pick(); return 'select'; }
    if (name === 'b') return back();
    return 'none';
  },
};

const msg = text => { $.umsg.textContent = text; };
const back = () => (params.over === 'start' ? 'none' : (app.pop(), 'back'));

// ---------- the row ----------
function showPick() {
  zone = 'row';
  $.upick.style.display = '';
  $.uadd.style.display = 'none';
  $.upower.classList.remove('focus');
}

function draw(focusId) {
  const games = store.libraryGames();
  const keep = focusId ?? cards[index]?.user?.id;
  cards = [{ user: null }, ...store.users().map(user => ({ user }))];
  $.urow.replaceChildren(...cards.map((c, i) => {
    const el = document.createElement('div');
    el.className = `ucard${c.user ? '' : ' add'}`;
    el.dataset.nav = `user:${i}`;
    el.innerHTML = `<div class="udisc">${c.user ? avatarHtml(c.user, BIG, { games, host }) : `<div class="avatar">${PLUS}</div>`}<i class="uring"><b></b></i></div><div class="uname"></div>`;
    el.querySelector('.uname').textContent = c.user ? c.user.name : 'Add User';
    return el;
  }));
  const i = cards.findIndex(c => c.user && c.user.id === keep);
  focus(i >= 0 ? i : Math.min(1, cards.length - 1), true);
}

// the focused card sits in the middle, full size; the others shrink and line up on either side
function focus(i, now = false) {
  if (i < 0 || i >= cards.length || (i === index && !now)) return false;
  index = i;
  [...$.urow.children].forEach((el, k) => {
    const d = k - i;
    const x = d === 0 ? CX : CX + Math.sign(d) * (NEAR + (Math.abs(d) - 1) * PITCH);
    el.classList.toggle('focus', d === 0);
    el.style.transform = `translateX(${x - BIG / 2}px)`;
    el.querySelector('.udisc').style.transform = d === 0 ? 'none' : `scale(${SMALL_K})`;
  });
  if (!now) $.upad.animate([{ opacity: 0, transform: 'translateY(8px)' }, { opacity: 1, transform: 'none' }], { duration: 200, easing: EASE_OUT });
  msg('');
  return true;
}

function zoneRow() {
  if (zone === 'row') return false;
  zone = 'row';
  $.upower.classList.remove('focus');
  $.urow.classList.remove('away');
  return true;
}

function zonePower() {
  zone = 'power';
  $.upower.classList.add('focus');
  $.urow.classList.add('away');
  return true;
}

async function pick() {
  const c = cards[index];
  if (!c) return;
  if (!c.user) return openAdd();
  busy = true;
  try {
    await host.users.switch(c.user.id);
  } catch (err) {
    busy = false;
    msg(String(err?.message ?? err));
    return;
  }
  const done = params.onPick;
  if (done) {
    root.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 133, easing: EASE_OUT, fill: 'forwards' }).finished
      .then(() => { app.pop(); done(); }, () => {});
  } else app.pop();
}

// ---------- Add User ----------
const takenChar = c => store.users().some(u => u.avatar === `char:${c.id}`);

function freeName() {
  const names = new Set(store.users().map(u => u.name.toLowerCase()));
  for (let n = store.users().length + 1; ; n++) if (!names.has(`player ${n}`)) return `Player ${n}`;
}

async function openAdd() {
  zone = 'add';
  add = { focus: 'name', name: freeName(), pick: null, chars: [], k: 0 };
  $.upick.style.display = 'none';
  $.uadd.style.display = '';
  paintAdd();
  $.uadd.animate([{ opacity: 0, transform: 'scale(.97)' }, { opacity: 1, transform: 'none' }], { duration: 250, easing: EASE_OUT });
  const mine = add;
  try {
    const chars = await host.users.featured();
    if (add !== mine) return;
    // a random one nobody has first, then the rest in turn
    const free = chars.filter(c => !takenChar(c));
    const pool = free.length ? free : chars;
    add.chars = pool;
    add.k = Math.floor(Math.random() * pool.length);
    add.pick = pool[add.k] ?? null;
    paintAdd();
  } catch (err) {
    console.warn('featured', err); // offline: our own avatar with the initial; the host picks a character later
  }
}

function paintAdd() {
  const preview = add.pick ? { name: add.name, avatar: `char:${add.pick.id}`, image: add.pick.image }
    : { name: add.name, avatar: nextAvatar(store.users()) };
  $.disc.innerHTML = avatarHtml(preview, 260, { host });
  $.disc.title = add.pick?.name ?? '';
  $.val.textContent = add.name;
  const ok = valid(add.name);
  $.field.classList.toggle('valid', ok);
  $.av.classList.toggle('focus', add.focus === 'avatar');
  $.av.classList.toggle('can', add.chars.length > 1);
  $.field.classList.toggle('focus', add.focus === 'name');
  $.pills[0].classList.toggle('focus', add.focus === 'cancel');
  $.pills[1].classList.toggle('focus', add.focus === 'ok');
  $.pills[1].classList.toggle('off', !ok);
}

const valid = name => {
  const n = name.trim();
  return n.length > 0 && [...n].length <= 16 && !store.users().some(u => u.name.toLowerCase() === n.toLowerCase());
};

const ORDER = ['avatar', 'name', 'buttons'];
function addAction(name, e) {
  const t = e.target ?? '';
  if ((name === 'point' || name === 'click') && t.startsWith('add:')) {
    const f = t.slice(4);
    if (name === 'point') { if (f === add.focus) return false; add.focus = f; paintAdd(); return true; }
    add.focus = f;
    name = 'a';
  } else if (name === 'point' || name === 'click') return 'none';
  const row = add.focus === 'cancel' || add.focus === 'ok' ? 'buttons' : add.focus;
  if (name === 'up' || name === 'down') {
    const r = ORDER.indexOf(row) + (name === 'up' ? -1 : 1);
    if (r < 0 || r >= ORDER.length) return e.held ? false : 'none';
    add.focus = ORDER[r] === 'buttons' ? 'ok' : ORDER[r];
    paintAdd();
    return true;
  }
  if (name === 'left' || name === 'right') {
    if (row !== 'buttons') return 'none';
    const f = name === 'left' ? 'cancel' : 'ok';
    if (f === add.focus) return e.held ? false : 'none';
    add.focus = f;
    paintAdd();
    return true;
  }
  if (e.held) return false;
  if (name === 'b') { closeAdd(null); return 'back'; }
  if (name !== 'a') return 'none';
  if (add.focus === 'avatar') return another();
  if (add.focus === 'name') { typeName(); return 'select'; }
  if (add.focus === 'cancel') { closeAdd(null); return 'back'; }
  create();
  return 'select';
}

function another() {
  if (add.chars.length < 2) return 'none';
  add.k = (add.k + 1) % add.chars.length;
  add.pick = add.chars[add.k];
  paintAdd();
  $.disc.animate([{ transform: 'scale(.92)', opacity: 0.4 }, { transform: 'none', opacity: 1 }], { duration: 220, easing: EASE_OUT });
  return 'select';
}

async function typeName() {
  kb = keyboard.open({ title: 'Name', value: add.name, maxLength: 16, parent: root });
  const name = await kb.done;
  kb = null;
  if (name == null || !add) return;
  add.name = name.trim().split(/\s+/).join(' ');
  add.focus = valid(add.name) ? 'ok' : 'name';
  msg(valid(add.name) || !add.name ? '' : `There's already a user called ${add.name}.`);
  paintAdd();
}

async function create() {
  if (!valid(add.name)) {
    msg(add.name.trim() ? `There's already a user called ${add.name.trim()}.` : 'Type a name.');
    add.focus = 'name';
    paintAdd();
    return;
  }
  busy = true;
  try {
    const p = add.pick;
    let user = await host.users.add(add.name, p ? `char:${p.id}` : 'random');
    if (p) user = await host.users.edit(user.id, { avatar: `char:${p.id}`, avatarName: p.name });
    await store.refresh();
    busy = false;
    closeAdd(user);
  } catch (err) {
    busy = false;
    msg(String(err?.message ?? err));
  }
}

function closeAdd(user) {
  add = null;
  msg('');
  if (params.add) { app.pop(); return; }
  showPick();
  draw(user?.id);
}

function clock(el) {
  const tick = () => {
    el.textContent = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', hourCycle: 'h23' }); // as Home's
    setTimeout(tick, 60000 - (Date.now() % 60000) + 50);
  };
  tick();
}
