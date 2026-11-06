// Settings → Users: + Add User (the Add User screen, ui/users), then everyone on this PC with their avatar; per user
// Name, Avatar, Switch and Delete (src-tauri users/mod.rs). Each user's time played, journal, trophies, screenshots and
// GitHub backup are their own; the games and every other setting are shared.
// The avatar page is a grid, in sections:
//   Search            A types a name; the characters that match (AniList) fill the section under it
//   Popular           famous characters, Berserk's among them (AniList; their pictures are kept on this PC)
//   Your Pictures     images in data\avatars\mine, Pictures, Downloads and Desktop, newest first. To use your own
//                     picture, save or copy it into one of those folders (a browser download lands in Downloads).
//   Padwake           ours, drawn by the app
//   From Your Games   the library's tiles

import { AVATARS, avatarHtml, avatarName } from '../users/avatars.js';
import * as store from '../users/store.js';

const errText = err => String(err?.message ?? err);
const PLUS = '<svg viewBox="0 0 36 36" fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round"><path d="M18 7v22M7 18h22"/></svg>';

export function usersPage(ctx) {
  const icon = u => avatarHtml(u, 44, { games: store.libraryGames(), host: ctx.host });
  return {
    title: 'Users',
    layout: 'full',
    cls: 'users',
    load: () => store.ready,
    events: { users: () => ctx.update() },
    rows: () => [
      { kind: 'header', label: 'Add a new user, or select someone to change or delete.' },
      { kind: 'action', label: 'Add User', icon: PLUS, run: () => ctx.app.push('users', { add: true }) },
      ...store.users().map(u => ({
        kind: 'link', label: u.name, icon: icon(u), value: u.id === store.me()?.id ? 'Playing now' : '',
        open: c => userPage(c, u.id),
      })),
    ],
  };
}

function userPage(ctx, id) {
  const u = () => store.users().find(x => x.id === id) ?? { id, name: '', avatar: '' };
  const isMe = () => store.me()?.id === id;
  return {
    title: u().name,
    layout: 'full',
    cls: 'users',
    events: { users: () => ctx.update() },
    rows: () => [
      {
        kind: 'action', label: 'Name', value: u().name,
        run: async () => {
          const name = await ctx.type({ title: 'Name', value: u().name, maxLength: 16 });
          if (name == null || name.trim() === u().name) return;
          try {
            await ctx.host.users.edit(id, { name });
            await store.refresh();
            ctx.update();
          } catch (err) {
            ctx.dialog({ title: "The name didn't change", text: errText(err) });
          }
        },
      },
      {
        kind: 'link', label: 'Avatar', icon: avatarHtml(u(), 44, { games: store.libraryGames(), host: ctx.host }),
        value: avatarName(u(), store.libraryGames()), desc: 'A character, one of your own pictures, or one of ours.',
        open: c => avatarPage(c, id),
      },
      {
        kind: 'action', label: `Switch to ${u().name}`, disabled: isMe,
        desc: () => (isMe() ? `${u().name} is playing now.` : 'Home and the Journal show their games and time.'),
        run: async () => {
          try {
            await ctx.host.users.switch(id);
            await store.refresh();
            ctx.update();
          } catch (err) {
            ctx.dialog({ title: "Couldn't switch", text: errText(err) });
          }
        },
      },
      {
        kind: 'action', label: 'Delete User', disabled: isMe,
        desc: () => (isMe() ? 'Switch to another user first.' : 'Deletes their time played, journal, trophies and screenshots on this PC.'),
        run: async () => {
          const ok = await ctx.dialog({
            title: `Delete ${u().name}?`,
            text: 'Their time played, journal, trophies and screenshots on this PC are deleted. A GitHub backup stays on their account.',
            buttons: [{ label: 'Cancel', value: false }, { label: 'Delete', value: true }],
          });
          if (ok !== true) return;
          try {
            await ctx.host.users.remove(id);
            await store.refresh();
            ctx.back();
          } catch (err) {
            ctx.dialog({ title: "The user wasn't deleted", text: errText(err) });
          }
        },
      },
    ],
  };
}

// ---------- the avatar grid ----------
const COLS = 7, SIZE = 150, PITCH_X = 196, PITCH_Y = 214, HEAD = 70;
const LEFT = 960 - ((COLS - 1) * PITCH_X) / 2 - SIZE / 2, TOP = 236, BOTTOM = 1000;
const SEARCH = '<svg viewBox="0 0 30 30" style="width:40%;height:40%"><circle cx="12.5" cy="12.5" r="9.5" fill="none" stroke="#fff" stroke-width="3"/><path d="M19.6 19.6 28 28" stroke="#fff" stroke-width="3.4" stroke-linecap="round"/></svg>';
const FOLDER = '<svg viewBox="0 0 36 36" style="width:40%;height:40%" fill="none" stroke="#fff" stroke-width="2.4" stroke-linejoin="round"><path d="M4 9h10l3 3h15v16H4z"/></svg>';

function avatarPage(ctx, id) {
  return {
    title: 'Avatar',
    layout: 'custom',
    mount(el) {
      const user = () => store.users().find(x => x.id === id);
      const games = store.libraryGames().filter(g => g.art?.tile);
      const s = { results: null, searching: '', featured: null, pictures: null, note: {} };
      const box = document.createElement('div');
      box.className = 'avgrid';
      box.innerHTML = '<div class="avscroll"></div>';
      el.append(box);
      const scroller = box.querySelector('.avscroll');
      let items = [], index = 0, scroll = 0, busy = false;

      // the sections, each a heading and its tiles: {avatar, label, image?, act?}
      const sections = () => [
        { head: s.searching ? `Search: ${s.searching}` : 'Search', note: s.note.search,
          tiles: [{ label: 'Search Characters', html: SEARCH, act: search }, ...(s.results ?? []).map(charTile)] },
        { head: 'Popular Characters', note: s.featured ? (s.featured.length ? '' : 'Connect to the internet to see characters.') : 'Loading…',
          tiles: (s.featured ?? []).map(charTile) },
        { head: 'Your Pictures', note: s.pictures && !s.pictures.length ? 'Save a picture in Pictures or Downloads on this PC, or copy it into data\\avatars\\mine, and it shows here.' : s.pictures ? '' : 'Loading…',
          tiles: [{ label: 'How to add pictures', html: FOLDER, act: howTo }, ...(s.pictures ?? []).map(p => ({ label: p.name, avatar: 'pic', image: p.thumb, path: p.path }))] },
        { head: 'Padwake', tiles: AVATARS.map(a => ({ avatar: a.id, label: a.name })) },
        ...(games.length ? [{ head: 'From Your Games', tiles: games.map(g => ({ avatar: `game:${g.id}`, label: g.name })) }] : []),
      ];
      const tileKey = t => t?.path ?? t?.avatar ?? t?.label;
      const charTile = c => ({ avatar: `char:${c.id}`, label: c.from ? `${c.name} · ${c.from}` : c.name, image: c.image, name: c.name });

      function draw(keep) {
        const u = user();
        const html = [];
        items = [];
        let y = TOP;
        for (const sec of sections()) {
          html.push(`<div class="avhead" style="top:${y - 64}px"></div>`);
          if (sec.note) html.push(`<div class="avnote" style="top:${y - 30}px"></div>`);
          const row0 = items.length ? items.at(-1).row + 1 : 0;
          sec.tiles.forEach((t, k) => {
            const row = row0 + Math.floor(k / COLS), col = k % COLS;
            items.push({ ...t, row, col, x: LEFT + col * PITCH_X, y: y + Math.floor(k / COLS) * PITCH_Y });
          });
          y += Math.max(1, Math.ceil(sec.tiles.length / COLS)) * PITCH_Y + HEAD + (sec.note ? 26 : 0);
          if (!sec.tiles.length) items.push({ row: row0, col: -1, spacer: true }); // an empty section still takes a row
        }
        items = items.filter(t => !t.spacer);
        const heads = sections();
        scroller.innerHTML = html.join('') + items.map((t, i) => {
          const face = t.html ? `<div class="avatar avtool" style="width:${SIZE}px;height:${SIZE}px">${t.html}</div>`
            : avatarHtml({ name: u?.name, avatar: t.avatar === 'pic' ? 'pic:x' : t.avatar, image: t.image }, SIZE, { games, host: ctx.host });
          return `<div class="avitem${t.act ? ' tool' : ''}" data-nav="av:${i}" style="left:${t.x}px;top:${t.y}px">${face}<span></span></div>`;
        }).join('');
        [...scroller.querySelectorAll('.avhead')].forEach((h, k) => { h.textContent = heads[k].head; });
        const notes = heads.filter(h => h.note);
        [...scroller.querySelectorAll('.avnote')].forEach((n, k) => { n.textContent = notes[k].note; });
        [...scroller.querySelectorAll('.avitem')].forEach((it, i) => { it.querySelector('span').textContent = items[i].label; });
        // keep: a tile index, or a tile's key (a section that loaded moves the tiles under it)
        const i = typeof keep === 'number' ? keep : keep ? items.findIndex(t => tileKey(t) === keep) : items.findIndex(t => t.avatar && t.avatar === u?.avatar);
        focus(Math.max(0, Math.min(items.length - 1, i < 0 ? 0 : i)));
      }

      function focus(i) {
        index = i;
        [...scroller.querySelectorAll('.avitem')].forEach((it, k) => it.classList.toggle('focus', k === i));
        const y = items[i]?.y ?? TOP;
        if (y - scroll + SIZE + 70 > BOTTOM) scroll = y + SIZE + 70 - BOTTOM;
        if (y - scroll < TOP) scroll = Math.max(0, y - TOP);
        scroller.style.transform = `translateY(${-scroll}px)`;
        return true;
      }

      function step(dRow, dCol) {
        const p = items[index];
        if (!p) return false;
        if (dCol) {
          const k = items.findIndex(t => t.row === p.row && t.col === p.col + dCol);
          return k >= 0 && focus(k);
        }
        // the next row up or down: the same column, or that row's last tile when it is shorter
        let row = p.row + dRow;
        while (row >= 0 && row <= items.at(-1).row) {
          const inRow = items.map((t, k) => [t, k]).filter(([t]) => t.row === row);
          if (inRow.length) return focus((inRow.find(([t]) => t.col === p.col) ?? inRow.at(-1))[1]);
          row += dRow;
        }
        return false;
      }

      async function search() {
        const q = await ctx.type({ title: 'Search characters, e.g. Guts, Cloud, 2B', value: s.searching, maxLength: 40 });
        if (q == null || !q.trim()) return;
        s.searching = q.trim();
        s.note.search = 'Searching…';
        s.results = [];
        draw(0);
        try {
          s.results = await ctx.host.users.search(s.searching);
          s.note.search = s.results.length ? '' : `No characters called ${s.searching}.`;
        } catch (err) {
          s.note.search = errText(err);
        }
        draw(s.results.length ? 1 : 0);
      }

      function howTo() {
        ctx.dialog({
          title: 'Use your own picture',
          text: 'Save a JPEG or PNG in Pictures, Downloads or Desktop on this PC (a browser download lands in Downloads), or copy one into data\\avatars\\mine beside Padwake. It shows under Your Pictures, newest first.',
        });
      }

      async function choose() {
        const t = items[index];
        if (!t) return;
        if (t.act) return t.act();
        busy = true;
        try {
          if (t.path) await ctx.host.users.usePicture(id, t.path);
          else await ctx.host.users.edit(id, { avatar: t.avatar, avatarName: t.name ?? null });
          await store.refresh();
          ctx.back();
        } catch (err) {
          ctx.dialog({ title: "The avatar didn't change", text: errText(err) });
        } finally {
          busy = false;
        }
      }

      draw();
      ctx.host.users.featured().then(list => { s.featured = list; }, () => { s.featured = []; }).then(() => draw(tileKey(items[index])));
      ctx.host.users.pictures().then(list => { s.pictures = list; }, () => { s.pictures = []; }).then(() => draw(tileKey(items[index])));
      // tests (tools/app/ui-probe.mjs): the state, and focus by label
      window.__avatars = { state: () => ({ index, label: items[index]?.label, count: items.length, sections: sections().map(x => [x.head, x.tiles.length]) }),
        focusLabel: label => focus(Math.max(0, items.findIndex(t => t.label.startsWith(label)))) };
      return {
        action(name, e = {}) {
          if (busy) return 'none';
          if (name === 'up' || name === 'down') return step(name === 'up' ? -1 : 1, 0) || (e.held ? false : 'none');
          if (name === 'left' || name === 'right') return step(0, name === 'left' ? -1 : 1) || (e.held ? false : 'none');
          const m = /^av:(\d+)$/.exec(e.target ?? '');
          if (name === 'point') return m && +m[1] !== index ? focus(+m[1]) : 'none';
          if (name === 'wheel') return e.dy ? step(e.dy > 0 ? 1 : -1, 0) || 'none' : 'none';
          if (e.held) return false;
          if (name === 'click' && m) { focus(+m[1]); choose(); return 'select'; }
          if (name === 'a') { choose(); return 'select'; }
          if (name === 'y') { search(); return 'select'; }
          if (name === 'b') { ctx.back(); return 'back'; }
          return 'none';
        },
      };
    },
  };
}
