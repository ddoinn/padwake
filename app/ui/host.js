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
//                                 library  the full list after any change
//                                 settings the whole settings object after any change (host.settings.set)
//                                 display  a display-mode trial: {state: 'trial' | 'kept' | 'reverted', mode}
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
    launch: id => call('games', 'launch', { id }),
    close: id => call('games', 'close', { id }),  // like its X, then ended after 5 s; resolves after that; `game` exited follows
    resume: () => call('games', 'resume'),        // back to the running game: it comes to the front and the app sleeps
    session: () => call('games', 'session'),      // {id, kind, name, front: 'game' | 'app', startedAt, ready} or null
    overReady: () => call('games', 'over_ready'), // the still is on screen: the host may show the window (Guide tap over a game)
    backdrop: size => call('games', 'backdrop', { size }), // ArrayBuffer: a BMP of the screen as Guide was pressed over the game; size 'small' (half) | 'full'
  },
  // A game: {id, name, folder, exe, art: {tile, background, logo, cover}, tagline, description, screenshots[], trailer,
  // music, news: [{title, url, date, contents}], playtimeSec, lastPlayed, addedAt, steamAppId}. Art and media are file
  // paths for fileUrl (square tile, 16:9 background without scrims, transparent logo, portrait cover, 1280 px shots,
  // 30 s muted trailer, music); description and news contents are plain text (about 300 characters for news); times
  // are Unix ms. Anything but id, name, folder and exe may be null or empty (no Steam match, or not fetched yet).
  library: {
    list: () => call('library', 'list'),
    rescan: () => call('library', 'rescan'), // check the games folder now instead of within 30 s
    refresh: id => call('library', 'refresh', { id }), // fetch one game's info and art again; resolves with the game
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
    info: () => call('startup', 'info'), // {restarted}: true when the watchdog started the app again after a crash
  },
  media: {
    list: () => call('media', 'list'),
    refresh: () => call('media', 'refresh'),
    open: id => call('media', 'open', { id }),
  },
};

// ---------- mock host ----------
const listeners = new Map();
const emit = (event, payload) => listeners.get(event)?.forEach(fn => fn(structuredClone(payload)));
const query = new URLSearchParams(location.search);

const slug = name => name.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'game';
const mockGame = name => ({
  id: slug(name), name, folder: `W:\\games-test\\${name}`, exe: `W:\\games-test\\${name}\\${name.replace(/[^A-Za-z0-9]/g, '')}.exe`,
  art: { tile: null, background: null, logo: null, cover: null }, tagline: null, description: null, screenshots: [],
  trailer: null, music: null, news: [], playtimeSec: 0, lastPlayed: null, addedAt: null, steamAppId: null,
});
// the stand-in library from tools/make-test-games.ps1, in the order the Rust host lists it (folder name, A–Z)
const mockLibrary = ['Celeste', 'Cyberpunk 2077', 'Dark Souls Remastered', 'Elden Ring', 'Ghost of Tsushima DC', 'God of War',
  'Hades', 'Hollow Knight', 'Horizon Zero Dawn', 'Red Dead Redemption 2', 'Sekiro - Shadows Die Twice', 'Spider-man Remastered',
  'Stand-in Long'].map(mockGame);
const mockSettings = { gamesRoot: 'W:\\games-test', rumble: true, uiSounds: true, music: true };
let mockRunning = null;
let mockSession = null;
function mockExit(id) {
  if (mockRunning !== id) return;
  const g = mockLibrary.find(x => x.id === id);
  if (g) { g.playtimeSec += Math.round((Date.now() - mockSession.startedAt) / 1000); g.lastPlayed = Date.now(); }
  mockRunning = null;
  emit('library', mockLibrary);
  emit('game', { id, kind: mockSession?.kind ?? 'game', state: 'exited', front: mockSession?.front ?? 'game' });
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
    open: ({ id }) => {
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
    launch({ id }) {
      if (!mockLibrary.some(g => g.id === id)) throw new Error(`no game ${id}`);
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
    session: () => (mockRunning ? { id: mockRunning, kind: mockSession.kind ?? 'game', name: mockSession.name ?? mockLibrary.find(g => g.id === mockRunning)?.name, front: mockSession.front, startedAt: mockSession.startedAt, ready: true } : null),
    // a picture standing in for the game's screen
    backdrop: ({ size }) => {
      if (!mockRunning) throw new Error('no still');
      const [, b64] = (size === 'small' ? picture(960, 540, 200) : picture(1920, 1080, 200)).split(',');
      return new Blob([Uint8Array.from(atob(b64), c => c.charCodeAt(0))], { type: 'image/jpeg' });
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
  },
  settings: {
    get: () => mockSettings,
    set: ({ patch }) => (Object.assign(mockSettings, patch), emit('settings', mockSettings), mockSettings),
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
    about: () => ({ version: '0.1.0', dataDir: 'W:\\tmp\\ch-target\\release\\data', exe: 'W:\\tmp\\ch-target\\release\\padwake.exe' }),
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
  },
};

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

async function mock(area, command, args) {
  const fn = MOCK[area]?.[command];
  if (!fn) throw new Error(`mock host has no ${area}|${command}`);
  return structuredClone(await fn(args));
}

if (isMock) window.__mock = { emit, library: mockLibrary, settings: mockSettings, rumbles: mockRumbles };
if (isMock) window.__mock.system = mockSystem; // Settings tests read the power log and the display trial
if (isMock) window.__mock.guideOverGame = mockGuideOverGame; // Control Center tests: Guide over a running game
