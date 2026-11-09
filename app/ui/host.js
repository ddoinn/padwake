// The only place the UI talks to the Rust host.
//   host.<area>.<command>(...)  invoke('plugin:<area>|<command>', args). One block per area below; a row adds its
//                               commands to its own block (and a mock for each in MOCK further down).
//   host.on(event, fn)          host → UI events; fn gets the payload. Returns an unsubscribe function.
//                                 input    pad state on change: a b x y lb rb lt rt view menu guide up down left right
//                                          l3 r3 (booleans), lx ly rx ry (-1..1 in 0.05 steps, y down)
//                                 guide    {kind: 'tap' | 'hold'}
//                                 game     {id, kind: 'game' | 'media', state: 'launching' | 'running' | 'ready' (its
//                                          window came up) | 'over' (the app woke over it after Guide; show:
//                                          'control-center' | 'home', then the guide event follows) | 'exited' (front:
//                                          'app' if Home already showed over it, else 'game')}
//                                 mouse    {on}: mouse mode started or ended (the pad drives the cursor meanwhile)
//                                 library  the full list after any change (a journal review sends it too)
//                                 lists    your own lists after any change (host.library.lists)
//                                 settings the whole settings object after any change (host.settings.set)
//                                 display  a display-mode trial: {state: 'trial' | 'kept' | 'reverted', mode}
//                                 update   the update status (host.cloud.update) after any change
//                                 backup   the backup status (host.cloud.backup) after any change
//                                 users    {users, current} after any change (host.users)
//   host.fileUrl(path)          an absolute file path in the data dir (art, media) as a URL the page can load.
// Without window.__TAURI__ (a plain browser, headless Edge) this is a mock host with fake data. The keyboard drives
// input; window.__mock.emit(event, payload) sends host events from a test (pad state, a game exiting).

const T = window.__TAURI__;
export const isMock = !T;

const call = (area, command, args = {}) =>
  T ? T.core.invoke(`plugin:${area}|${command}`, args) : mock(area, command, args);

export const host = {
  isMock,

  on(event, fn) {
    if (!T) {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event).add(fn);
      return () => listeners.get(event).delete(fn);
    }
    const unlisten = T.event.listen(event, e => fn(e.payload));
    return () => unlisten.then(off => off());
  },

  fileUrl: path => (!path ? '' : T ? T.core.convertFileSrc(path) : path),

  input: {
    rumble: (strength, ms) => call('input', 'rumble', { strength, ms }),
  },
  // Game sessions (rows A1, A5). One at a time; the app sleeps while the game is in front.
  games: {
    // from: where the player launched it (home | library | search | switch), for their session log
    launch: (id, from) => call('games', 'launch', { id, from }),
    close: id => call('games', 'close', { id }),  // like its X, then ended after 5 s; resolves after that; `game` exited follows
    resume: () => call('games', 'resume'),        // back to the running game: it comes to the front and the app sleeps
    session: () => call('games', 'session'),      // {id, kind, name, front: 'game' | 'app', startedAt, ready} or null
    overReady: () => call('games', 'over_ready'), // the still is on screen: the host may show the window (Guide tap over a game)
    backdrop: size => call('games', 'backdrop', { size }), // ArrayBuffer: a BMP of the screen as Guide was pressed over the game; size 'small' (half) | 'full'
    // screenshots taken with Guide + View while a game was in front, newest first: [{game, name, path, thumb (480 px), takenAt}]
    captures: () => call('games', 'captures'),
    deleteCapture: path => call('games', 'delete_capture', { path }),
    screen: id => call('games', 'screen', { id }), // the top screen changed (app.js): over a game, where the time goes
  },
  // A game: {id, name, folder, exe, store (a game installed with a launcher: {name: steam | epic | gog | xbox | ea, id,
  // uri}; null for a folder in the games folder), art: {tile, background, logo, cover}, tagline, description, screenshots[], trailer,
  // music, news: [{title, url, date, contents}], playtimeSec, lastPlayed, firstPlayed, sessions, journal: {rating 1–5,
  // status: playing | finished | completed (100 %) | paused | dropped, note}, addedAt, steamAppId,
  // achievements ({unlocked, total}, read from the game's files; null when it recorded none), hltbMainSec (the main
  // story's usual length),
  // and removedAt (a journal entry whose folder is gone: no screenshots, trailer, music or news; restoredAt too if it came
  // from a GitHub backup). Art and media are file
  // paths for fileUrl (square tile, 16:9 background without scrims, transparent logo, portrait cover, 1280 px shots,
  // 30 s muted trailer, music); description and news contents are plain text (about 300 characters for news); times
  // are Unix ms. Anything but id, name, folder and exe may be null or empty (no Steam match, or not fetched yet).
  library: {
    list: () => call('library', 'list'),
    rescan: () => call('library', 'rescan'), // check the games folder now instead of within 30 s
    refresh: id => call('library', 'refresh', { id }), // fetch one game's info and art again; resolves with the game
    journal: () => call('library', 'journal'), // played or reviewed games, then removed ones (removedAt)
    review: (id, journal) => call('library', 'review', { id, journal }), // sets the whole journal; resolves with the game
    forget: id => call('library', 'forget', { id }), // a removed game leaves the journal, art and all
    // your own lists (row B3), shared by everyone on the PC: [{id, name, games: [game ids, in the order added],
    // createdAt}], oldest first. A list keeps a game whose folder is gone. Names are trimmed, up to 32 characters and
    // not taken (any case): a refused one rejects with the reason. Changes resolve with every list (createList: with the
    // new one) and the `lists` event follows.
    lists: () => call('library', 'lists'),
    createList: (name, game) => call('library', 'list_create', { name, game }), // game: put it in the new list too
    renameList: (id, name) => call('library', 'list_rename', { id, name }),
    deleteList: id => call('library', 'list_delete', { id }),
    toggleList: (id, game, on) => call('library', 'list_toggle', { id, game, on }),
    // the current user's logs for their recaps (row B4): {sessions: the session log, oldest first, each game session
    // with `trophies` (the Padwake trophies it earned, replayed from today's totals); events: journal status changes
    // [{at, type: 'status', game, status}]}. Lines are as library/sessions.rs writes them; fields from row B1b may be
    // missing on older lines.
    recap: () => call('library', 'recap'),
  },
  // Settings (rows A9, A10): settings.json. set() merges a patch, saves and emits `settings` with the whole object.
  settings: {
    get: () => call('settings', 'get'),
    set: patch => call('settings', 'set', { patch }),
    folderExists: path => call('settings', 'folder_exists', { path }),
  },
  // Windows itself, for Settings and Control Center (A7). Events: `display` {state: 'trial' (seconds) | 'kept' |
  // 'reverted' (reason 'timeout' | 'asked'), mode}.
  system: {
    audioOutputs: () => call('system', 'audio_outputs'),          // [{id, name, isDefault}] active outputs
    setAudioOutput: id => call('system', 'set_audio_output', { id }), // the Windows default output
    volume: () => call('system', 'volume'),                         // {level 0..1, muted}
    setVolume: level => call('system', 'set_volume', { level }),
    // [{slot, kind: wired|alkaline|nimh|unknown, level 0..3 (when kind isn't wired), connection: wired|wireless|receiver
    //   (receiver: a 2.4 GHz USB receiver, no battery reported), name?}]
    battery: () => call('system', 'battery'),
    power: action => call('system', 'power', { action }),           // sleep | restart | shutdown | exit
    about: () => call('system', 'about'),                           // {version, dataDir, exe}
    display: () => call('system', 'display'),  // {device, name, current: {width, height, hz}, modes: [..], hdr: {supported, enabled}, trial}
    tryDisplayMode: ({ width, height, hz }) => call('system', 'try_display_mode', { width, height, hz }), // reverts in 15 s
    keepDisplayMode: () => call('system', 'keep_display_mode'),
    revertDisplayMode: () => call('system', 'revert_display_mode'),
    setHdr: on => call('system', 'set_hdr', { on }),
    network: () => call('system', 'network'), // {kind: wifi|ethernet|none, connected, adapter, ssid, signal, ip, wifi: on|off|none}
    wifiNetworks: () => call('system', 'wifi_networks'), // [{ssid, signal 0..100, secure, auth, connected, saved}], starts a scan
    wifiConnect: (ssid, password) => call('system', 'wifi_connect', { ssid, password }),
    mouseMode: on => call('system', 'mouse_mode', { on }), // Control Center (A7): the pad drives the cursor; B or Guide ends it
  },
  startup: {
    withWindows: () => call('startup', 'with_windows'),             // the "Padwake" logon task exists
    setWithWindows: on => call('startup', 'set_with_windows', { on }), // resolves with the new state
    info: () => call('startup', 'info'), // {restarted}: true when the app restarted mid-sitting: no start screen
    note: text => call('startup', 'note', { text }), // a line for the log (what dismissed the start screen, and when)
  },
  media: {
    list: () => call('media', 'list'),
    refresh: () => call('media', 'refresh'),
    open: (id, from) => call('media', 'open', { id, from }),
  },
  // Online and free, through GitHub (cloud area): system software updates from the project's releases, and a backup
  // of the library's time and journal in a private gist on the player's own account.
  cloud: {
    // {current, state: idle | checking | downloading | ready | latest | error, latest, notes (Markdown), progress 0..1,
    //  checkedAt, error, updatedFrom (this start came from an update), auto}
    update: () => call('cloud', 'update_status'),
    checkUpdate: () => call('cloud', 'update_check'),     // resolves with the status after; a newer version downloads
    installUpdate: () => call('cloud', 'update_install'), // the app restarts into the ready version
    // {available (this build can sign in), signedIn, login, state: idle | signing-in | saving | restoring | error,
    //  code: {userCode, uri, expiresAt} while signing in, savedAt, auto, error, found: {savedAt, machine, games} (a backup
    //  found at sign-in, not restored yet), restored: {updated, added}}
    backup: () => call('cloud', 'backup_status'),
    signIn: () => call('cloud', 'backup_sign_in'),        // resolves with the code to show; `backup` follows when done
    cancelSignIn: () => call('cloud', 'backup_cancel_sign_in'),
    signOut: () => call('cloud', 'backup_sign_out'),
    backUpNow: () => call('cloud', 'backup_now'),
    restore: () => call('cloud', 'backup_restore'),       // merges the backup in: nothing here is lost
    dismissFound: () => call('cloud', 'backup_dismiss'),
  },
  // Local users (users area). Each has their own time played, journal, trophies, screenshots and GitHub backup; the
  // games, art and settings are shared. {users: [{id, name, avatar (ui/users/avatars.js), avatarName, image (the
  // picture of a char: or pic: avatar), createdAt}], current}. A new user's avatar 'random' becomes a popular
  // character soon after (a `users` event).
  // A switch sends `library` (the new user's time and journal) and `backup` (their status), then `users`.
  users: {
    list: () => call('users', 'list'),
    add: (name, avatar) => call('users', 'add', { name, avatar }), // resolves with the user; no switch
    edit: (id, patch) => call('users', 'edit', { id, ...patch }),   // patch {name?, avatar?}; resolves with the user
    remove: id => call('users', 'remove', { id }),                  // not the current user; their things on this PC go
    switch: id => call('users', 'switch', { id }),                  // refused while a game runs
    // characters (AniList): [{id, name, from, image (a local picture)}]; avatar 'char:<id>' with avatarName = name
    featured: () => call('users', 'featured'),                      // the popular ones, Berserk's among them
    search: query => call('users', 'search', { query }),            // by name, most loved first
    // your pictures from data\avatars\mine, Pictures, Downloads and Desktop, newest first: [{path, name, place,
    // modified, thumb}]; usePicture makes one a user's avatar (resolves with the user)
    pictures: () => call('users', 'pictures'),
    usePicture: (id, path) => call('users', 'use_picture', { id, path }),
  },
};

// ---------- mock host ----------
const listeners = new Map();
const emit = (event, payload) => listeners.get(event)?.forEach(fn => fn(structuredClone(payload)));
const query = new URLSearchParams(location.search);
// ?today=2026-11-10T20:00: the mock's clock starts there and runs on (Date.now only), so recaps and their notice are the
// same on any day (tools/app/checks/recap.json). window.__mock.today(iso) moves it later in a run.
const realNow = Date.now;
let clockOffset = 0;
const setToday = iso => { clockOffset = new Date(iso).getTime() - realNow(); };
if (isMock && query.get('today')) {
  setToday(query.get('today'));
  Date.now = () => realNow() + clockOffset;
  // each run starts with no recap notice shown (it is remembered in the page's storage, which a probe profile keeps)
  try { for (const k of Object.keys(localStorage)) if (k.startsWith('recap.noticed.')) localStorage.removeItem(k); } catch {}
}

const slug = name => name.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'game';
const mockGame = name => ({
  id: slug(name), name, folder: `W:\\games-test\\${name}`, exe: `W:\\games-test\\${name}\\${name.replace(/[^A-Za-z0-9]/g, '')}.exe`,
  store: null, art: { tile: null, background: null, logo: null, cover: null }, tagline: null, description: null, screenshots: [],
  trailer: null, music: null, news: [], playtimeSec: 0, lastPlayed: null, firstPlayed: null, sessions: 0,
  journal: { rating: null, status: null, note: null }, addedAt: null, steamAppId: null,
});
// ---- the journal (Library → Journal): reviews on some stand-ins, and games whose folders are gone
const mockKept = [];
const inJournal = g => g.playtimeSec > 0 || g.lastPlayed != null || Object.values(g.journal ?? {}).some(v => v != null);
function cleanJournal(j = {}) {
  const note = typeof j.note === 'string' ? j.note.trim().slice(0, 280) : '';
  return {
    rating: j.rating > 0 ? Math.min(5, Math.round(j.rating)) : null,
    status: ['playing', 'finished', 'completed', 'paused', 'dropped'].includes(j.status) ? j.status : null,
    note: note || null,
  };
}
// the stand-in library from tools/make-test-games.ps1, in the order the Rust host lists it (folder name, A–Z)
const mockLibrary = ['Celeste', 'Cyberpunk 2077', 'Dark Souls Remastered', 'Elden Ring', 'Ghost of Tsushima DC', 'God of War',
  'Hades', 'Hollow Knight', 'Horizon Zero Dawn', 'Red Dead Redemption 2', 'Sekiro - Shadows Die Twice', 'Spider-man Remastered',
  'Stand-in Long'].map(mockGame);
// ?stores=1: games installed with store launchers too (library/stores.rs), after the folders; settings.storesOff hides
// a store's games, as a rescan does on the PC
const mockStoreGames = query.get('stores') === '1' ? [
  ['Portal 2', 'steam', '620', 'C:\\Program Files (x86)\\Steam\\steamapps\\common\\Portal 2', 'portal2.exe', 'steam://rungameid/620'],
  ['Alan Wake 2', 'epic', 'Iris', 'C:\\Program Files\\Epic Games\\AlanWake2', 'AlanWake2.exe', 'com.epicgames.launcher://apps/dc9d%3Aa9d%3AIris?action=launch&silent=true'],
  ['Disco Elysium', 'gog', '1771589310', 'C:\\GOG Games\\Disco Elysium', 'disco.exe', null],
  ['Halo Infinite', 'xbox', 'Microsoft.254428597CFE2_8wekyb3d8bbwe', 'D:\\XboxGames\\Halo Infinite\\Content', 'HaloInfinite.exe', 'shell:AppsFolder\\Microsoft.254428597CFE2_8wekyb3d8bbwe!Game'],
  ['Mass Effect Legendary Edition', 'ea', 'Origin.OFR.50.0004096', 'C:\\Program Files\\EA Games\\Mass Effect Legendary Edition', 'MassEffectLauncher.exe', null],
].map(([name, store, id, folder, exe, uri]) => ({ ...mockGame(name), folder, exe: `${folder}\\${exe}`, store: { name: store, id, uri }, steamAppId: store === 'steam' ? +id : null })) : [];
mockLibrary.push(...mockStoreGames);
function mockSyncStores() {
  const off = mockSettings.storesOff ?? [];
  const keep = mockLibrary.filter(g => !g.store);
  mockLibrary.splice(0, mockLibrary.length, ...keep, ...mockStoreGames.filter(g => !off.includes(g.store.name)));
}
const mockSettings = { gamesRoot: 'W:\\games-test', rumble: true, uiSounds: true, music: true };
let mockRunning = null;
let mockSession = null;
// what the page told the host for the session log (tools/app/checks/play-log.json): launches with `from`, top screens
const mockPlayLog = { launches: [], screens: [] };
function mockExit(id) {
  if (mockRunning !== id) return;
  const g = mockLibrary.find(x => x.id === id);
  if (g) {
    g.playtimeSec += Math.round((Date.now() - mockSession.startedAt) / 1000);
    g.lastPlayed = Date.now();
    g.sessions += 1;
    g.firstPlayed ??= mockSession.startedAt;
  }
  mockRunning = null;
  // its line in the player's log, as the host appends it
  const secs = Math.round((Date.now() - mockSession.startedAt) / 1000);
  mockRecaps[mockUsers.current]?.sessions.push(mockSession.kind === 'media'
    ? { start: mockSession.startedAt, end: Date.now(), secs, kind: 'media', app: id }
    : { game: id, start: mockSession.startedAt, end: Date.now(), secs });
  emit('library', mockLibrary);
  emit('game', { id, kind: mockSession?.kind ?? 'game', state: 'exited', front: mockSession?.front ?? 'game',
    played: Math.round((Date.now() - mockSession.startedAt) / 1000) });
}
// as the host does on Guide over a running game: 'over', then the guide event (tests: window.__mock.guideOverGame('tap'))
function mockGuideOverGame(kind = 'tap') {
  if (!mockRunning) return false;
  mockSession.front = 'app';
  emit('game', { id: mockRunning, kind: mockSession.kind ?? 'game', state: 'over', show: kind === 'tap' ? 'control-center' : 'home' });
  emit('guide', { kind });
  return true;
}

// ---- input and Home (rows A3, A4): hub data for every stand-in (tagline, description, news, screenshots, playtime)
// and art for three, so Home's image paths (decode, cross-fade, logo) run too. Pictures are small JPEGs drawn on a
// canvas (raster, decoded off the main thread like real art), made only in the mock.
const DAY = 864e5;
const mockRumbles = [];
function picture(w, h, hue, dark = false, label = '') {
  const c = Object.assign(document.createElement('canvas'), { width: w, height: h });
  const g = c.getContext('2d');
  drawPicture(g, w, h, hue, dark, label);
  return c.toDataURL('image/jpeg', 0.85);
}
function drawPicture(g, w, h, hue, dark, label, x = 0.7) {
  const grad = g.createLinearGradient(0, 0, w, h);
  grad.addColorStop(0, `hsl(${hue} 45% ${dark ? 6 : 38}%)`);
  grad.addColorStop(1, `hsl(${(hue + 60) % 360} 55% ${dark ? 14 : 10}%)`);
  g.fillStyle = grad;
  g.fillRect(0, 0, w, h);
  g.fillStyle = `hsl(${(hue + 30) % 360} 60% ${dark ? 30 : 60}% / .35)`;
  g.beginPath();
  g.arc(w * x, h * 0.42, h * 0.3, 0, 7);
  g.fill();
  // screenshots carry their number, so a test can see which one is shown
  if (label) {
    g.fillStyle = 'rgba(255, 255, 255, .85)';
    g.font = `600 ${Math.round(h * 0.3)}px sans-serif`;
    g.fillText(label, w * 0.08, h * 0.88);
  }
}
// ?trailer=1: a 3 s WebM recorded from a canvas at start-up becomes the trailer of every game that has one (the library
// event follows), so the trailer card and its full-screen player play a real video. Off by default: recording takes
// 3 s of the page's time.
function mockTrailer(games) {
  const c = Object.assign(document.createElement('canvas'), { width: 640, height: 360 });
  const g = c.getContext('2d');
  const rec = new MediaRecorder(c.captureStream(30), { mimeType: 'video/webm' });
  const chunks = [];
  rec.ondataavailable = e => chunks.push(e.data);
  const t0 = performance.now();
  const draw = setInterval(() => drawPicture(g, 640, 360, 200, false, `${((performance.now() - t0) / 1000).toFixed(1)} s`, 0.3 + ((performance.now() - t0) / 3000) * 0.5), 33);
  rec.onstop = () => {
    clearInterval(draw);
    const url = URL.createObjectURL(new Blob(chunks, { type: 'video/webm' }));
    for (const x of games) if (x.trailer) x.trailer = url;
    emit('library', games);
  };
  rec.start();
  setTimeout(() => rec.stop(), 3000);
}
function wordmark(text) {
  const c = Object.assign(document.createElement('canvas'), { width: 560, height: 150 });
  const g = c.getContext('2d');
  g.font = 'bold 110px sans-serif';
  g.fillStyle = '#fff';
  g.fillText(text.toUpperCase(), 0, 120, 560);
  return c.toDataURL('image/png');
}
if (isMock) {
  const shots = [200, 20, 120, 280, 330].map((h, n) => picture(1280, 720, h, false, String(n + 1)));
  mockLibrary.forEach((g, i) => Object.assign(g, {
    tagline: i % 4 === 3 ? null : i % 2 ? `${g.name.split(' ')[0]} awaits. Fight your way through a world that remembers every choice you make.` : `The journey of ${g.name.split(' ')[0]} begins here.`,
    description: `${g.name} is a stand-in from tools/make-test-games.ps1. In the real library this card holds the store description: what the game is, how it plays and why it is worth your evening. It wraps across several lines and is cut off when it runs long.`,
    screenshots: shots.map((_, n) => shots[(n + i) % 5]),
    news: [0, 1, 4].map((d, n) => ({ title: ['Patch notes: performance and fixes', 'Weekend event starts Friday', 'Developer diary #3'][n], date: Math.floor((Date.now() - d * DAY) / 1000), feedlabel: 'Community Announcements' })),
    playtimeSec: (i * 7919) % 90000,
    lastPlayed: (i * 7919) % 90000 ? Date.now() - ((i * 3) % 20) * DAY : null,
    addedAt: Date.now() - (40 + i) * DAY,
    trailer: i % 2 ? null : `${g.folder}\\trailer.mp4`,
    music: i % 3 ? null : './assets/sounds/start.wav',
  }));
  [['celeste', 330], ['god-of-war', 10], ['hades', 345]].forEach(([id, h]) => {
    const g = mockLibrary.find(x => x.id === id);
    g.art = { tile: picture(512, 512, h), background: picture(960, 540, (h + 40) % 360, true), logo: wordmark(g.name) };
  });
  if (query.get('trailer') === '1') mockTrailer(mockLibrary);
  // ?journal=0: an empty journal
  if (query.get('journal') !== '0') {
    const review = (id, rating, status, note = null, sessions = 0) => {
      const g = mockLibrary.find(x => x.id === id);
      Object.assign(g, { sessions: sessions || Math.max(1, Math.round(g.playtimeSec / 5400)), firstPlayed: g.lastPlayed && g.lastPlayed - 30 * DAY,
        journal: { rating, status, note } });
    };
    review('hades', 5, 'playing', 'One more run. Always one more run. Beat Hades with the Twin Fists on heat 4.');
    review('celeste', 4, 'finished', 'Chapter 7 broke me, in a good way.');
    review('god-of-war', 5, 'completed');
    review('hollow-knight', 3, 'paused');
    review('cyberpunk-2077', 2, 'dropped', 'Come back after the next big patch.');
    // achievements (library/progress.rs): Hades 47 %, God of War all of them, Hollow Knight 32 %; Elden Ring has no
    // unlock file, only the main story's length
    Object.assign(mockLibrary.find(x => x.id === 'hades'), { achievements: { unlocked: 23, total: 49 }, hltbMainSec: 22 * 3600 });
    Object.assign(mockLibrary.find(x => x.id === 'god-of-war'), { achievements: { unlocked: 37, total: 37 } });
    Object.assign(mockLibrary.find(x => x.id === 'hollow-knight'), { achievements: { unlocked: 20, total: 63 } });
    Object.assign(mockLibrary.find(x => x.id === 'elden-ring'), { hltbMainSec: 60 * 3600 });
    const kept = (name, hours, days, sessions, journal, hue) => ({
      ...mockGame(name), playtimeSec: Math.round(hours * 3600), lastPlayed: Date.now() - (days + 3) * DAY, firstPlayed: Date.now() - (days + 90) * DAY,
      sessions, addedAt: Date.now() - (days + 120) * DAY, removedAt: Date.now() - days * DAY, journal: cleanJournal(journal),
      tagline: `${name}: in your journal since you removed it.`,
      art: hue == null ? { tile: null, background: null, logo: null, cover: null }
        : { tile: picture(512, 512, hue), background: picture(960, 540, (hue + 40) % 360, true), logo: null, cover: null },
    });
    mockKept.push(
      kept('The Witcher 3 Wild Hunt', 118.4, 21, 64, { rating: 5, status: 'completed', note: 'The Bloody Baron is still the best quest I have played.' }, 140),
      kept('Inside', 4.2, 63, 3, { rating: 4, status: 'finished' }, 20),
      kept('Death Stranding', 31.7, 140, 22, { rating: 3, status: 'dropped' }, 200),
      kept('Portal 2', 9.1, 300, 7, { rating: 5, status: 'finished' }, null),
    );
  }
}
const mockMedia = ['YouTube', 'Netflix', 'Spotify', 'Twitch', 'Plex', 'Crunchyroll'].map((name, i) => ({
  id: slug(name), name, url: ['https://www.youtube.com/', 'https://www.netflix.com/', 'https://open.spotify.com/',
    'https://www.twitch.tv/', 'https://app.plex.tv/', 'https://www.crunchyroll.com/'][i],
  tagline: ['Watch videos from your favourite creators.', 'Watch films, series and new favourites.',
    'Listen to music, podcasts and playlists.', 'Watch live streams and join the conversation.',
    'Watch films, live TV and your personal collection.', 'Watch anime and find your next favourite.'][i],
  art: { tile: `./media/${slug(name)}.svg`, background: `./media/${slug(name)}-background.svg` },
}));

const MOCK = {
  input: {
    rumble: ({ strength, ms }) => (mockRumbles.push([strength, ms]), null),
  },
  media: {
    list: () => mockMedia,
    refresh: () => emit('media', mockMedia),
    open: ({ id, from }) => {
      mockPlayLog.launches.push({ id, from, kind: 'media' });
      const entry = mockMedia.find(m => m.id === id);
      if (!entry) throw new Error(`no media ${id}`);
      if (mockRunning === id && mockSession.kind === 'media') { mockSession.front = 'game'; return null; }
      if (mockRunning) throw new Error(`${mockRunning} is already running`);
      mockRunning = id;
      mockSession = { id, kind: 'media', name: entry.name, startedAt: Date.now(), front: 'game' };
      emit('game', { id, kind: 'media', state: 'launching' });
      emit('game', { id, kind: 'media', state: 'running' });
      setTimeout(() => mockRunning === id && emit('game', { id, kind: 'media', state: 'ready' }), 500);
      const ms = Number(query.get('gameMs') ?? 3000);
      if (ms) mockSession.timer = setTimeout(() => mockExit(id), ms);
      return null;
    },
  },
  games: {
    // the real host hides the window while the game runs; here the game "runs" for ?gameMs= (default 3 s)
    // ?gameMs=0 keeps it running until close; a running game ends with playtime and last played, as the real host's
    launch({ id, from }) {
      if (!mockLibrary.some(g => g.id === id)) throw new Error(`no game ${id}`);
      mockPlayLog.launches.push({ id, from });
      if (mockRunning) throw new Error(`${mockRunning} is already running`);
      mockRunning = id;
      mockSession = { id, startedAt: Date.now(), front: 'game' };
      emit('game', { id, kind: 'game', state: 'launching' });
      emit('game', { id, kind: 'game', state: 'running' });
      setTimeout(() => mockRunning === id && emit('game', { id, kind: 'game', state: 'ready' }), 500);
      const ms = Number(query.get('gameMs') ?? 3000);
      if (ms) mockSession.timer = setTimeout(() => mockExit(id), ms);
      return null;
    },
    close: ({ id }) => {
      if (mockRunning !== id) throw new Error(`${id} is not running`);
      clearTimeout(mockSession.timer);
      return new Promise(done => setTimeout(() => { mockExit(id); done(null); }, 300));
    },
    resume: () => { if (!mockRunning) throw new Error('nothing is running'); mockSession.front = 'game'; return null; },
    over_ready: () => null,
    screen: ({ id }) => { mockPlayLog.screens.push(id); return null; },
    session: () => (mockRunning ? { id: mockRunning, kind: mockSession.kind ?? 'game', name: mockSession.name ?? mockLibrary.find(g => g.id === mockRunning)?.name, front: mockSession.front, startedAt: mockSession.startedAt, ready: true } : null),
    // a picture standing in for the game's screen
    backdrop: ({ size }) => {
      if (!mockRunning) throw new Error('no still');
      const [, b64] = (size === 'small' ? picture(960, 540, 200) : picture(1920, 1080, 200)).split(',');
      return new Blob([Uint8Array.from(atob(b64), c => c.charCodeAt(0))], { type: 'image/jpeg' });
    },
    // ?captures=0: none yet; else ten screenshots of four games over the last days (made once, on first use)
    captures: () => {
      if (!mockCaptures) {
        const games = [['hades', 'Hades', 350], ['celeste', 'Celeste', 200], ['elden-ring', 'Elden Ring', 40], ['hollow-knight', 'Hollow Knight', 230]];
        const n = Number(query.get('captures') ?? 10);
        mockCaptures = Array.from({ length: n }, (_, k) => {
          const [game, name, hue] = games[k % games.length];
          const path = picture(1280, 720, (hue + k * 9) % 360, false, `${k + 1}`);
          return { game, name, path: `${path}#${k}`, thumb: path, takenAt: Date.now() - k * 7.3 * 3600e3 };
        });
      }
      return mockCaptures;
    },
    delete_capture: ({ path }) => {
      const i = mockCaptures?.findIndex(c => c.path === path) ?? -1;
      if (i < 0) throw new Error('not a capture');
      mockCaptures.splice(i, 1);
      return null;
    },
  },
  library: {
    list: () => mockLibrary,
    rescan: () => (emit('library', mockLibrary), mockLibrary),
    refresh: ({ id }) => {
      const g = mockLibrary.find(x => x.id === id);
      if (!g) throw new Error(`no game ${id}`);
      setTimeout(() => emit('library', mockLibrary), 300);
      return g;
    },
    journal: () => [...mockLibrary, ...mockKept].filter(inJournal),
    review: ({ id, journal }) => {
      const g = mockLibrary.find(x => x.id === id) ?? mockKept.find(x => x.id === id);
      if (!g) throw new Error(`no game ${id}`);
      g.journal = cleanJournal(journal);
      emit('library', mockLibrary);
      return g;
    },
    // ?lists=0: none yet; else "Couch Co-op" with two games and one whose folder is gone
    lists: () => mockLists,
    list_create: ({ name, game }) => {
      const n = mockListName(name);
      let t = Date.now();
      while (mockLists.some(l => l.id === `l${t.toString(36)}`)) t++;
      const list = { id: `l${t.toString(36)}`, name: n, games: game ? [game] : [], createdAt: Date.now() };
      mockLists.push(list);
      emit('lists', mockLists);
      return list;
    },
    list_rename: ({ id, name }) => {
      const l = mockList(id);
      l.name = mockListName(name, id);
      emit('lists', mockLists);
      return mockLists;
    },
    list_delete: ({ id }) => {
      mockList(id);
      mockLists.splice(0, mockLists.length, ...mockLists.filter(l => l.id !== id));
      emit('lists', mockLists);
      return mockLists;
    },
    list_toggle: ({ id, game, on }) => {
      const l = mockList(id);
      if (on && !l.games.includes(game)) l.games.push(game);
      if (!on) l.games = l.games.filter(g => g !== game);
      emit('lists', mockLists);
      return mockLists;
    },
    forget: ({ id }) => {
      const i = mockKept.findIndex(x => x.id === id);
      if (i < 0) throw new Error(`${id} is not a removed game`);
      mockKept.splice(i, 1);
      emit('library', mockLibrary);
      return null;
    },
  },
  settings: {
    get: () => mockSettings,
    set: ({ patch }) => {
      Object.assign(mockSettings, patch);
      emit('settings', mockSettings);
      if ('storesOff' in patch) mockSyncStores();
      return mockSettings;
    },
    folder_exists: ({ path }) => /^[a-z]:\\/i.test(path) && !/missing/i.test(path),
  },
  // ---- Settings (rows A9, A10): a living-room PC with two outputs, one pad, a 1080p display and Wi-Fi
  system: {
    audio_outputs: () => mockSystem.outputs,
    set_audio_output: ({ id }) => {
      if (!mockSystem.outputs.some(o => o.id === id)) throw new Error(`no output ${id}`);
      mockSystem.outputs.forEach(o => { o.isDefault = o.id === id; });
      return null;
    },
    volume: () => mockSystem.volume,
    set_volume: ({ level }) => { mockSystem.volume.level = Math.min(1, Math.max(0, level)); return null; },
    battery: () => mockSystem.pads,
    power: ({ action }) => { mockSystem.powerLog.push(action); return null; },
    about: () => ({ version: '0.1.0', dataDir: 'W:\\system\\tmp\\ch-target\\release\\data', exe: 'W:\\system\\tmp\\ch-target\\release\\padwake.exe' }),
    display: () => ({ ...mockSystem.display, trial: !!mockSystem.trial }),
    try_display_mode: mode => {
      const d = mockSystem.display;
      if (!d.modes.some(m => m.width === mode.width && m.height === mode.height && m.hz === mode.hz)) throw new Error('no such mode');
      mockSystem.trial ??= { from: d.current };
      clearTimeout(mockSystem.trial.timer);
      d.current = { width: mode.width, height: mode.height, hz: mode.hz };
      mockSystem.trial.timer = setTimeout(() => mockRevert('timeout'), Number(query.get('trialMs') ?? 15000));
      emit('display', { state: 'trial', seconds: 15, mode: d.current });
      return null;
    },
    keep_display_mode: () => {
      if (!mockSystem.trial) throw new Error('no display trial');
      clearTimeout(mockSystem.trial.timer);
      mockSystem.trial = null;
      emit('display', { state: 'kept', mode: mockSystem.display.current });
      return null;
    },
    revert_display_mode: () => (mockRevert('asked'), null),
    set_hdr: ({ on }) => {
      if (!mockSystem.display.hdr.supported) throw new Error('HDR is not supported on this display');
      mockSystem.display.hdr.enabled = on;
      return null;
    },
    network: () => mockSystem.network,
    wifi_networks: () => mockSystem.wifi,
    wifi_connect: ({ ssid, password }) => {
      const n = mockSystem.wifi.find(w => w.ssid === ssid);
      if (!n) throw new Error(`${ssid} is out of range`);
      if (n.secure && !n.saved && password.length < 8) throw new Error('The password must be at least 8 characters');
      setTimeout(() => {
        mockSystem.wifi.forEach(w => { w.connected = w === n; });
        n.saved = true;
        Object.assign(mockSystem.network, { kind: 'wifi', connected: true, ssid, signal: n.signal });
      }, 800);
      return null;
    },
    // Control Center (A7): mouse mode only logs here
    mouse_mode: ({ on }) => { mockSystem.mouseLog.push(!!on); emit('mouse', { on: !!on }); return null; },
  },
  startup: {
    with_windows: () => mockSystem.withWindows,
    set_with_windows: ({ on }) => (mockSystem.withWindows = !!on),
    info: () => ({ restarted: query.get('restarted') === '1' }), // ?restarted=1: as after a crash
    note: ({ text }) => { (window.__notes ??= []).push(text); return null; },
  },
  // ---- updates and backup: ?update=ready (a downloaded 0.2.0) | latest | error, ?updated=1 (this start came from an
  // update), ?backup=signed-in | unavailable, ?signInMs= (how long the player takes to type the code; default 4 s)
  cloud: {
    update_status: () => mockCloud.update,
    update_check: async () => {
      const u = mockCloud.update;
      if (u.state === 'ready') return u;
      mockUpdate({ state: 'checking', error: null });
      await sleep(500);
      if (query.get('update') === 'latest') return mockUpdate({ state: 'latest', checkedAt: Date.now() });
      if (query.get('update') === 'error') return mockUpdate({ state: 'error', error: 'GitHub answered 503', checkedAt: Date.now() });
      for (let p = 0; p <= 1; p += 0.25) { mockUpdate({ state: 'downloading', latest: '0.2.0', progress: p }); await sleep(150); }
      return mockUpdate({ state: 'ready', latest: '0.2.0', progress: null, notes: MOCK_NOTES, checkedAt: Date.now() });
    },
    update_install: () => {
      if (mockCloud.update.state !== 'ready') throw new Error('No update is ready.');
      mockCloud.installed = true;
      return null;
    },
    backup_status: () => mockCloud.backup,
    backup_sign_in: () => {
      if (!mockCloud.backup.available) throw new Error("GitHub sign-in isn't set up in this version of Padwake.");
      clearTimeout(mockCloud.signIn);
      mockCloud.signIn = setTimeout(() => mockBackup({
        state: 'idle', code: null, signedIn: true, login: 'player',
        found: { savedAt: Date.now() - 3 * DAY, machine: 'LIVING-ROOM', games: 9 },
      }), Number(query.get('signInMs') ?? 4000));
      return mockBackup({ state: 'signing-in', error: null, code: { userCode: '8F2C-71QD', uri: 'https://github.com/login/device', expiresAt: Date.now() + 900e3 } });
    },
    backup_cancel_sign_in: () => (clearTimeout(mockCloud.signIn), mockBackup({ state: 'idle', code: null })),
    backup_sign_out: () => mockBackup({ signedIn: false, login: null, savedAt: null, found: null, restored: null, state: 'idle' }),
    backup_now: async () => {
      if (!mockCloud.backup.signedIn) throw new Error('Sign in to GitHub first.');
      mockBackup({ state: 'saving' });
      await sleep(600);
      return mockBackup({ state: 'idle', savedAt: Date.now() });
    },
    backup_restore: async () => {
      if (!mockCloud.backup.signedIn) throw new Error('Sign in to GitHub first.');
      mockBackup({ state: 'restoring' });
      await sleep(700);
      for (const [name, hours, rating, status] of [['Gris', 6, 5, 'finished'], ['Outer Wilds', 21, 5, 'completed']]) {
        if (mockKept.some(k => k.name === name)) continue;
        mockKept.push({ ...mockGame(name), playtimeSec: hours * 3600, sessions: hours, lastPlayed: Date.now() - 40 * DAY,
          removedAt: Date.now(), restoredAt: Date.now(), journal: cleanJournal({ rating, status }) });
      }
      emit('library', mockLibrary);
      return mockBackup({ state: 'idle', found: null, restored: { updated: 3, added: 2 } });
    },
    backup_dismiss: () => mockBackup({ found: null }),
  },
};

let mockCaptures = null;
// your own lists, as library/collections.rs keeps them
const mockLists = query.get('lists') === '0' ? []
  : [{ id: 'l1', name: 'Couch Co-op', games: ['celeste', 'gone-game', 'hades'], createdAt: Date.now() - 20 * DAY }];
const mockList = id => mockLists.find(l => l.id === id) ?? (() => { throw new Error("That list isn't there any more."); })();
function mockListName(name, except) {
  const n = String(name ?? '').trim().split(/\s+/).join(' ');
  if (!n) throw new Error('Type a name for the list.');
  if ([...n].length > 32) throw new Error('A list name can be up to 32 characters.');
  if (mockLists.some(l => l.id !== except && l.name.toLowerCase() === n.toLowerCase())) throw new Error(`There's already a list called ${n}.`);
  return n;
}
const sleep = ms => new Promise(r => setTimeout(r, ms));
const MOCK_NOTES = '- Saved Data: back up your time played and journal to GitHub, free\n- Updates download by themselves and install the next time Padwake starts\n- The Journal shows games restored from a backup';
const mockCloud = {
  update: {
    current: '0.1.0', state: query.get('update') === 'ready' ? 'ready' : 'idle', latest: query.get('update') === 'ready' ? '0.2.0' : null,
    notes: query.get('update') === 'ready' ? MOCK_NOTES : null, progress: null, checkedAt: null, error: null,
    updatedFrom: query.get('updated') === '1' ? '0.0.9' : null, auto: true,
  },
  backup: {
    available: query.get('backup') !== 'unavailable', signedIn: query.get('backup') === 'signed-in',
    login: query.get('backup') === 'signed-in' ? 'player' : null, state: 'idle', code: null,
    savedAt: query.get('backup') === 'signed-in' ? Date.now() - 2 * 3600e3 : null, auto: true, error: null, found: null, restored: null,
  },
  installed: false,
};
const mockUpdate = patch => (Object.assign(mockCloud.update, patch, { auto: mockSettings.autoUpdate !== false }), emit('update', mockCloud.update), structuredClone(mockCloud.update));
const mockBackup = patch => (Object.assign(mockCloud.backup, patch, { auto: mockSettings.backupAuto !== false }), emit('backup', mockCloud.backup), structuredClone(mockCloud.backup));

const mockModes = [[1920, 1080, [144, 120, 60]], [1680, 1050, [60]], [1600, 900, [60]], [1366, 768, [60]], [1280, 1024, [75, 60]],
  [1280, 720, [60]], [1024, 768, [60]], [800, 600, [60]]].flatMap(([width, height, rates]) => rates.map(hz => ({ width, height, hz })));
const mockSystem = {
  outputs: [
    { id: '{0.0.0.00000000}.{a1}', name: 'Speakers (Realtek High Definition Audio)', isDefault: false },
    { id: '{0.0.0.00000000}.{b2}', name: 'LG TV (NVIDIA High Definition Audio)', isDefault: true },
    { id: '{0.0.0.00000000}.{c3}', name: 'Headphones (Xbox Wireless Headset)', isDefault: false },
  ],
  volume: { level: 0.62, muted: false },
  pads: [{ slot: 0, kind: 'wired', level: 3, connection: 'receiver', name: '8BitDo Ultimate 2C Wireless Controller' }],
  powerLog: [],
  display: {
    device: '\\\\.\\DISPLAY1', name: 'LG TV SSCR2', current: { width: 1920, height: 1080, hz: 60 }, modes: mockModes,
    hdr: { supported: query.get('hdr') !== '0', enabled: false },
  },
  trial: null,
  network: { kind: 'wifi', connected: true, adapter: 'Intel(R) Wi-Fi 6 AX201 160MHz', ssid: 'Living Room 5G', signal: 84, ip: '192.168.1.23', wifi: 'on' },
  wifi: [
    ['Living Room 5G', 84, 'wpa2', true, true], ['Living Room', 71, 'wpa2', false, true], ['DIRECT-7F-HP OfficeJet', 58, 'wpa2', false, false],
    ['Pixel_4412', 49, 'wpa3', false, false], ['Neighbours', 37, 'wpa2', false, false], ['Cafe Guest', 22, 'open', false, false],
  ].map(([ssid, signal, auth, connected, saved]) => ({ ssid, signal, secure: auth !== 'open', auth, connected, saved })),
  withWindows: false,
  mouseLog: [],
};
function mockRevert(reason) {
  const t = mockSystem.trial;
  if (!t) return;
  clearTimeout(t.timer);
  mockSystem.trial = null;
  mockSystem.display.current = t.from;
  emit('display', { state: 'reverted', reason, mode: t.from });
}

// ---- users: ?users=3 starts with three (Madhav has the journal above, Asha played two games, Kabir nothing yet),
// else one. A switch puts the next user's time and journal on the stand-ins, as the real host does.
const PLAY = ['playtimeSec', 'sessions', 'firstPlayed', 'lastPlayed', 'journal', 'restoredAt'];
const blankPlay = () => ({ playtimeSec: 0, sessions: 0, firstPlayed: null, lastPlayed: null, journal: { rating: null, status: null, note: null }, restoredAt: null });
const mockUsers = { users: [{ id: 'u1', name: 'Madhav', avatar: 'ember', createdAt: Date.now() - 90 * DAY }], current: 'u1' };
const mockPlays = new Map(); // user id → {game id: play} for users other than the current one
if (Number(query.get('users') ?? 1) >= 3) {
  mockUsers.users.push({ id: 'u2', name: 'Asha', avatar: 'moon', createdAt: Date.now() - 40 * DAY },
    { id: 'u3', name: 'Kabir', avatar: 'bolt', createdAt: Date.now() - 2 * DAY });
  mockPlays.set('u2', {
    'hollow-knight': { ...blankPlay(), playtimeSec: 41 * 3600, sessions: 30, lastPlayed: Date.now() - 3600e3, firstPlayed: Date.now() - 30 * DAY,
      journal: { rating: 5, status: 'playing', note: 'Asha: the Mantis Lords!' } },
    celeste: { ...blankPlay(), playtimeSec: 2 * 3600, sessions: 3, lastPlayed: Date.now() - 2 * DAY, firstPlayed: Date.now() - 9 * DAY },
  });
  mockPlays.set('u3', {});
}
function mockWear(id) {
  const all = [...mockLibrary, ...mockKept];
  mockPlays.set(mockUsers.current, Object.fromEntries(all.map(g => [g.id, Object.fromEntries(PLAY.map(k => [k, g[k] ?? null]))])));
  const plays = mockPlays.get(id) ?? {};
  for (const g of all) Object.assign(g, blankPlay(), structuredClone(plays[g.id] ?? {}));
  mockPlays.delete(id);
  mockUsers.current = id;
}
// characters and pictures: canvas portraits with a name on them (the mock can't reach AniList or the PC's folders)
let mockChars = null, mockExtra = null, mockPics = null;
const portrait = (name, hue) => picture(256, 256, hue, false, name.split(' ').map(w => w[0]).join(''));
function mockCharacters() {
  return mockChars ??= [['Cloud Strife', 'Final Fantasy VII', 210], ['Tifa Lockhart', 'Final Fantasy VII', 330], ['2B', 'NieR:Automata', 0],
    ['Dante', 'Devil May Cry', 355], ['Master Chief', 'Halo', 110], ['Link', 'The Legend of Zelda', 140], ['Pikachu', 'Pokemon', 55],
    ['Sonic the Hedgehog', 'Sonic X', 225], ['Guts', 'Berserk', 20], ['Griffith', 'Berserk', 260], ['Casca', 'Berserk', 300],
    ['The Skull Knight', 'Berserk', 190]].map(([name, from, hue], i) => ({ id: 1000 + i, name, from, image: portrait(name, hue) }));
}
function mockSearchExtra() {
  return mockExtra ??= [['Zodd', 'Berserk', 15], ['Puck', 'Berserk', 160], ['Serpico', 'Berserk', 90]]
    .map(([name, from, hue], i) => ({ id: 2000 + i, name, from, image: portrait(name, hue) }));
}
function mockPictures() {
  return mockPics ??= [['me at the beach', 'Downloads', 30], ['cat', 'Pictures', 80], ['Screenshot 2026-11-01', 'Screenshots', 200],
    ['kratos', 'Padwake', 5]].map(([name, place, hue], k) => {
    const thumb = picture(160, 160, hue, false, name[0].toUpperCase());
    return { path: `C:\\Users\\player\\${place}\\${name}.jpg`, name, place, modified: Date.now() - k * 36e5, thumb };
  });
}
const mockUserName = (name, except) => {
  const n = String(name ?? '').trim().split(/\s+/).join(' ');
  if (!n) throw new Error('Type a name.');
  if ([...n].length > 16) throw new Error('A name can be up to 16 characters.');
  if (mockUsers.users.some(u => u.id !== except && u.name.toLowerCase() === n.toLowerCase())) throw new Error(`There's already a user called ${n}.`);
  return n;
};
MOCK.users = {
  list: () => mockUsers,
  add: ({ name, avatar }) => {
    const u = { id: `u${Date.now().toString(36)}`, name: mockUserName(name), avatar, createdAt: Date.now() };
    mockUsers.users.push(u);
    mockPlays.set(u.id, {});
    emit('users', mockUsers);
    // as the host: a random popular character nobody has, once "AniList" answers
    if (avatar === 'random') setTimeout(() => {
      const free = mockCharacters().filter(c => !mockUsers.users.some(x => x.avatar === `char:${c.id}`));
      const c = free[0] ?? mockCharacters()[0];
      Object.assign(u, { avatar: `char:${c.id}`, avatarName: c.name, image: c.image });
      emit('users', mockUsers);
    }, 300);
    return u;
  },
  featured: async () => { await sleep(150); return mockCharacters(); },
  search: async ({ query }) => {
    await sleep(250);
    const q = String(query ?? '').trim().toLowerCase();
    if (q.length < 2) throw new Error('Type at least two letters.');
    return mockCharacters().concat(mockSearchExtra()).filter(c => c.name.toLowerCase().includes(q) || c.from?.toLowerCase().includes(q));
  },
  pictures: async () => { await sleep(150); return query.get('pictures') === '0' ? [] : mockPictures(); },
  use_picture: ({ id, path }) => {
    const pic = mockPictures().find(x => x.path === path);
    if (!pic) throw new Error('Pick a picture from the list.');
    const u = mockUsers.users.find(x => x.id === id);
    Object.assign(u, { avatar: `pic:${pic.name}`, avatarName: pic.name, image: pic.thumb });
    emit('users', mockUsers);
    return u;
  },
  edit: ({ id, name, avatar, avatarName }) => {
    const u = mockUsers.users.find(x => x.id === id);
    if (!u) throw new Error("That user isn't on this PC any more.");
    if (name != null) u.name = mockUserName(name, id);
    if (avatar != null) {
      u.avatar = avatar;
      const c = mockCharacters().concat(mockSearchExtra()).find(x => `char:${x.id}` === avatar);
      u.avatarName = /^(char|pic):/.test(avatar) ? avatarName ?? c?.name ?? null : null;
      u.image = c?.image ?? (avatar.startsWith('pic:') ? u.image : null);
    }
    emit('users', mockUsers);
    return u;
  },
  remove: ({ id }) => {
    if (id === mockUsers.current) throw new Error('Switch to another user first.');
    mockUsers.users = mockUsers.users.filter(u => u.id !== id);
    mockPlays.delete(id);
    emit('users', mockUsers);
    return mockUsers;
  },
  switch: async ({ id }) => {
    if (mockRunning) throw new Error('Close the game first. Its time goes to whoever started it.');
    if (!mockUsers.users.some(u => u.id === id)) throw new Error("That user isn't on this PC any more.");
    if (id !== mockUsers.current) {
      mockWear(id);
      emit('library', mockLibrary);
      emit('backup', mockCloud.backup);
    }
    emit('users', mockUsers);
    return mockUsers;
  },
};

// ---- recaps (row B4): with ?today=, each user's session log over last month and this one, as the host sends it
// (trophies already replayed). Madhav's log begins on the 3rd of last month with lines from before row B1b (no front,
// achievements or media); this month's lines have them. Asha played this month only; Kabir nothing. Without ?today=
// every log is empty, so other checks see no recap notice.
const mockRecaps = { u1: { sessions: [], events: [] }, u2: { sessions: [], events: [] }, u3: { sessions: [], events: [] } };
function mockRecapLogs() {
  const now = new Date(Date.now());
  const at = (back, day, h, m = 0) => new Date(now.getFullYear(), now.getMonth() - back, day, h, m).getTime();
  const line = (game, back, day, h, m, mins, more = {}) => {
    const start = at(back, day, h, m);
    return { game, start, end: start + mins * 60e3, secs: mins * 60, ...more };
  };
  const b1b = (front, more = {}) => ({ v: '0.4.0', from: 'home', ended: 'quit', front, ...more });
  const media = (app, back, day, h, m, mins) => ({ ...line('', back, day, h, m, mins), game: undefined, kind: 'media', app, v: '0.4.0' });
  const status = (game, back, day, h, s) => ({ at: at(back, day, h), type: 'status', game, status: s });
  const madhav = [
    line('hades', 1, 3, 21, 0, 120), line('hades', 1, 4, 20, 30, 180), line('celeste', 1, 7, 19, 0, 60),
    line('hades', 1, 11, 22, 0, 90), line('celeste', 1, 14, 19, 30, 45), line('hades', 1, 18, 21, 30, 240, { trophies: ['invested'] }),
    line('elden-ring', 1, 20, 23, 0, 120, { trophies: ['first-steps'] }), line('hades', 1, 25, 14, 0, 60),
    line('hades', 0, 1, 20, 0, 120, b1b({ game: 6600, home: 240, controlCenter: 120, other: 240 }, { achievements: [21, 23] })),
    media('youtube', 0, 2, 19, 0, 45),
    line('hollow-knight', 0, 3, 21, 0, 90, b1b({ game: 5100, home: 0, controlCenter: 300, other: 0 }, { achievements: [18, 20] })),
    line('celeste', 0, 6, 22, 30, 60, b1b({ game: 3300, home: 300, controlCenter: 0, other: 0 })),
    line('ghost-of-tsushima-dc', 0, 7, 20, 0, 60, b1b({ game: 3400, home: 200, controlCenter: 0, other: 0 }, { trophies: ['first-steps'], from: 'library' })),
    line('god-of-war', 0, 8, 15, 0, 180, b1b({ game: 10200, home: 300, controlCenter: 0, other: 300 }, { achievements: [36, 37], trophies: ['regular'], screenshots: 2 })),
    media('netflix', 0, 9, 21, 0, 120),
  ];
  const asha = [
    line('hollow-knight', 0, 2, 22, 0, 120, b1b({ game: 7000, home: 200, controlCenter: 0, other: 0 })),
    line('hollow-knight', 0, 5, 23, 30, 60, b1b({ game: 3600, home: 0, controlCenter: 0, other: 0 })),
    line('celeste', 0, 6, 18, 0, 60, b1b({ game: 3500, home: 100, controlCenter: 0, other: 0 }, { trophies: ['first-steps'] })),
  ];
  const past = l => l.filter(s => s.start < Date.now());
  mockRecaps.u1 = { sessions: past(madhav), events: [status('elden-ring', 1, 20, 23, 'playing'), status('celeste', 1, 26, 10, 'finished'),
    status('hollow-knight', 0, 4, 12, 'paused'), status('god-of-war', 0, 8, 18, 'completed')].filter(e => e.at < Date.now()) };
  mockRecaps.u2 = { sessions: past(asha), events: [] };
  mockRecaps.u3 = { sessions: [], events: [] };
  // first plays as the library knows them: the log's games were played before it began, but for these two
  const first = { hades: at(2, 1, 20), celeste: at(3, 9, 20), 'hollow-knight': at(3, 15, 20), 'god-of-war': at(4, 2, 20),
    'elden-ring': at(1, 20, 23), 'ghost-of-tsushima-dc': at(0, 7, 20) };
  for (const [id, t] of Object.entries(first)) { const g = mockLibrary.find(x => x.id === id); if (g) g.firstPlayed = t; }
}
if (isMock && query.get('today')) mockRecapLogs();
MOCK.library.recap = () => mockRecaps[mockUsers.current] ?? { sessions: [], events: [] };

async function mock(area, command, args) {
  const fn = MOCK[area]?.[command];
  if (!fn) throw new Error(`mock host has no ${area}|${command}`);
  return structuredClone(await fn(args));
}

if (isMock) window.__mock = { emit, library: mockLibrary, kept: mockKept, settings: mockSettings, rumbles: mockRumbles, lists: mockLists };
if (isMock) window.__mock.system = mockSystem; // Settings tests read the power log and the display trial
if (isMock) window.__mock.cloud = mockCloud; // update and backup tests
if (isMock) window.__mock.users = mockUsers; // users tests
if (isMock) window.__mock.guideOverGame = mockGuideOverGame; // Control Center tests: Guide over a running game
if (isMock) window.__mock.playLog = mockPlayLog; // the session log's launch places and top screens
if (isMock) window.__mock.recaps = mockRecaps; // recap tests: each user's log
if (isMock) window.__mock.today = iso => { setToday(iso); Date.now = () => realNow() + clockOffset; mockRecapLogs(); }; // move the clock
