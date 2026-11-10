// Avatars. A user's `avatar` is one of:
//   one of AVATARS     our own, drawn here: six show the user's initial on a grey, six a glyph. Grey because Padwake has
//                      no hue of its own (row B5); a game's art, a character or a picture brings colour. The ids are
//                      the old colour presets', so saved users keep their choice.
//   game:<id>          a game's square tile from the library
//   char:<id>          a character (AniList, src-tauri users/characters.rs): user.image is its picture
//   pic:<key>          one of the player's own pictures (users/pictures.rs): user.image
//   random             a popular character the host hasn't picked yet (offline): drawn as the first of ours
// avatarHtml() styles itself inline, so any screen can draw one without loading CSS.

const G = d => `<svg viewBox="0 0 64 64" style="width:56%;height:56%;display:block"><path fill="#fff" d="${d}"/></svg>`;
const GLYPHS = {
  pad: G('M20 20h24c7.7 0 13 6.4 13.8 15.2l.9 9.6c.4 4.5-2.9 7.2-6.3 7.2-2.5 0-4.4-1.4-5.8-3.4L42.8 43H21.2l-3.8 5.6c-1.4 2-3.3 3.4-5.8 3.4-3.4 0-6.7-2.7-6.3-7.2l.9-9.6C7 26.4 12.3 20 20 20Zm-1.5 8v4h-4v4h4v4h4v-4h4v-4h-4v-4Zm25.5 0a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2Zm-5.5 5.5a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2Zm11 0a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2Z'),
  star: G('M32 7l7.6 16.2 17.7 2.1-13 12.2 3.4 17.5L32 46.3 16.3 55l3.4-17.5-13-12.2 17.7-2.1Z'),
  bolt: G('M37 5 13 36h15l-4 23 26-33H35Z'),
  moon: G('M41 8a24 24 0 1 0 15 38A19 19 0 0 1 41 8Z'),
  flame: G('M32 5c2.5 11 15 17 15 31a15 15 0 0 1-30 0c0-7.5 4-12 6.5-15 1 5.5 3.3 8 6.5 9-2.3-10.5.2-18 2-25Z'),
  leaf: G('M54 9C31 9 11 21 11 40c0 5 1.7 9 4 12.5C19 40 27 31 40 25 29 33 23 42 20.5 55c3 1.6 7 2.5 11 2.5C51 57.5 56 34 54 9Z'),
};

export const AVATARS = [
  { id: 'ember', name: 'Graphite', from: '#5a5a5a', to: '#262626' },
  { id: 'tide', name: 'Silver', from: '#8c8c8c', to: '#4a4a4a' },
  { id: 'grove', name: 'Charcoal', from: '#3e3e3e', to: '#161616' },
  { id: 'dusk', name: 'Stone', from: '#727272', to: '#363636' },
  { id: 'bloom', name: 'Ash', from: '#666666', to: '#2e2e2e' },
  { id: 'sun', name: 'Ink', from: '#2e2e2e', to: '#0c0c0c' },
  { id: 'pad', name: 'Controller', from: '#4a4a4a', to: '#1c1c1c', glyph: 'pad' },
  { id: 'star', name: 'Star', from: '#4a4a4a', to: '#1c1c1c', glyph: 'star' },
  { id: 'bolt', name: 'Bolt', from: '#4a4a4a', to: '#1c1c1c', glyph: 'bolt' },
  { id: 'moon', name: 'Moon', from: '#4a4a4a', to: '#1c1c1c', glyph: 'moon' },
  { id: 'flame', name: 'Flame', from: '#4a4a4a', to: '#1c1c1c', glyph: 'flame' },
  { id: 'leaf', name: 'Leaf', from: '#4a4a4a', to: '#1c1c1c', glyph: 'leaf' },
];

const byId = new Map(AVATARS.map(a => [a.id, a]));
const esc = t => String(t).replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const initial = name => [...String(name ?? '').trim()][0]?.toUpperCase() ?? '?';

// the avatar's name in Settings: a character's or picture's, a game's, or ours
export function avatarName(user, games = []) {
  const id = user?.avatar ?? '';
  if (/^(char|pic):/.test(id)) return user.avatarName || (id.startsWith('pic:') ? 'Your picture' : 'Character');
  if (id.startsWith('game:')) return games.find(g => g.id === id.slice(5))?.name ?? 'Game art';
  if (id === 'random') return 'Choosing one for you';
  return byId.get(id)?.name ?? AVATARS[0].name;
}

// a free preset for a new user: the first nobody uses yet
export const nextAvatar = users => (AVATARS.find(a => !users.some(u => u.avatar === a.id)) ?? AVATARS[users.length % AVATARS.length]).id;

// a round avatar size px across. games: the library, for game:<id> avatars; host: for fileUrl.
export function avatarHtml(user, size, { games = [], host } = {}) {
  const box = `width:${size}px;height:${size}px;border-radius:50%;overflow:hidden;flex:none;display:grid;place-items:center;`;
  const id = user?.avatar ?? '';
  if (/^(char|pic):/.test(id) && user.image && host) {
    return `<div class="avatar" style="${box}background:#222 center/cover url('${esc(host.fileUrl(user.image))}')"></div>`;
  }
  if (id.startsWith('game:')) {
    const tile = games.find(g => g.id === id.slice(5))?.art?.tile;
    if (tile && host) return `<div class="avatar" style="${box}background:#222 center/cover url('${esc(host.fileUrl(tile))}')"></div>`;
  }
  const a = byId.get(id) ?? AVATARS[0];
  const inner = a.glyph ? GLYPHS[a.glyph]
    : `<span style="font-size:${Math.round(size * 0.46)}px;line-height:1;font-weight:600;color:#fff;text-shadow:0 2px 8px rgba(0,0,0,.25)">${esc(initial(user?.name))}</span>`;
  return `<div class="avatar" style="${box}background:linear-gradient(145deg, ${a.from}, ${a.to})">${inner}</div>`;
}
