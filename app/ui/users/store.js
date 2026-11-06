// The users, kept from boot: who is playing (me()), everyone (users()), and the library's games for game-art avatars.
// onUsers(fn) calls fn after any change (a switch, an add, a rename, a new avatar, a delete).

import { host } from '../host.js';

let data = { users: [], current: null };
let games = [];
const subs = new Set();

function set(d) {
  if (!d) return;
  data = d;
  subs.forEach(fn => fn(data));
}

host.on('users', set);
host.on('library', list => { games = list ?? []; });
export const ready = Promise.all([
  host.users.list().then(set),
  host.library.list().then(list => { games = list ?? []; }),
]).catch(e => console.warn('users', e));

export const users = () => data.users;
export const me = () => data.users.find(u => u.id === data.current) ?? data.users[0] ?? null;
export const libraryGames = () => games;
export const onUsers = fn => { subs.add(fn); return () => subs.delete(fn); };
// after an add, rename or delete from the page: the host's `users` event comes too, but this doesn't wait for it
export const refresh = () => host.users.list().then(set);
