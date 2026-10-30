// Settings pages (decision 9): Sound, Controllers, Library, Power and System here; Display and Network in their
// folders (row A10); Saved Data and System Software in cloud.js. Each page is a factory (ctx) → page description; the shapes are at the top of settings.js and
// list.js. Every setting reads and writes the real thing through ctx.host (src-tauri: settings, system, startup).

import { ICONS } from './icons.js';
import { display } from './display/display.js';
import { network } from './network/network.js';
import { padTest } from './controller-test.js';
export { padTest } from './controller-test.js';
import { savedData, softwareCategory } from './cloud.js';

export const root = () => ({
  title: 'Settings',
  layout: 'root',
  rows: [
    { kind: 'link', label: 'Display', icon: ICONS.display, open: display },
    { kind: 'link', label: 'Sound', icon: ICONS.sound, open: sound },
    { kind: 'link', label: 'Controllers', icon: ICONS.controllers, open: controllers },
    { kind: 'link', label: 'Network', icon: ICONS.network, open: network },
    { kind: 'link', label: 'Library', icon: ICONS.library, open: library },
    { kind: 'link', label: 'Saved Data', icon: ICONS.savedData, open: savedData },
    { kind: 'link', label: 'Power', icon: ICONS.power, open: power },
    { kind: 'link', label: 'System', icon: ICONS.system, open: system },
  ],
});

// a toggle bound to one key of settings.json (s: the page's copy of the settings)
const setting = (ctx, s, key, label, desc) => ({
  kind: 'toggle', label, desc,
  get: () => s.settings[key] !== false,
  set: async v => { s.settings = await ctx.host.settings.set({ [key]: v }); },
});

// ---------- Sound: output device, volume, home music, UI sounds ----------
export function sound(ctx) {
  const s = { outputs: [], volume: null, settings: {} };
  const load = async () => {
    [s.outputs, s.volume, s.settings] = await Promise.all([
      ctx.host.system.audioOutputs().catch(() => []),
      ctx.host.system.volume().catch(() => null),
      ctx.host.settings.get(),
    ]);
  };
  return {
    title: 'Sound',
    layout: 'two',
    load,
    events: { settings: v => { s.settings = v; ctx.update(); } },
    cats: [
      {
        label: 'Audio Output',
        rows: () => [
          {
            kind: 'select', label: 'Output Device', desc: 'Games, apps and Padwake play through this device.',
            disabled: () => !s.outputs.length,
            value: () => (s.outputs.length ? undefined : 'None found'),
            options: () => s.outputs.map(o => ({ value: o.id, label: o.name })),
            get: () => s.outputs.find(o => o.isDefault)?.id,
            set: async id => {
              await ctx.host.system.setAudioOutput(id);
              [s.outputs, s.volume] = await Promise.all([ctx.host.system.audioOutputs(), ctx.host.system.volume().catch(() => null)]);
            },
          },
          { kind: 'header', label: 'General' },
          setting(ctx, s, 'music', 'Home Screen Music'),
          setting(ctx, s, 'uiSounds', 'Sound Effects', 'Play a sound when you move around and select things.'),
        ],
      },
      {
        label: 'Volume',
        enter: () => ctx.host.system.volume().then(v => { s.volume = v; ctx.update(); }, () => {}),
        rows: () => [
          {
            kind: 'slider', label: 'Volume', key: 'volume', disabled: () => !s.volume,
            desc: () => (s.outputs.find(o => o.isDefault)?.name ?? ''),
            get: () => s.volume?.level ?? 0,
            set: v => {
              if (s.volume) s.volume.level = v;
              ctx.host.system.setVolume(v).catch(() => {});
            },
          },
        ],
      },
    ],
  };
}

// ---------- Controllers: vibration, button test, each pad's connection and battery ----------
const BATTERY = ['Empty', 'Low', 'Medium', 'Full'];
const CONNECTION = { wired: 'Wired (USB)', wireless: 'Wireless', receiver: 'Wireless (2.4 GHz receiver)' };
// a pad's rows. XInput reports a battery only for pads it sees as wireless; a 2.4 GHz receiver looks wired to it.
const padRows = p => {
  const link = p.connection ?? (p.kind === 'wired' ? 'wired' : 'wireless');
  const battery = link === 'wireless' ? BATTERY[p.level] ?? 'Unknown' : link === 'receiver' ? 'Not reported by this controller' : null;
  return [
    { kind: 'header', label: `Controller ${p.slot + 1}` },
    { kind: 'info', key: `pad${p.slot}`, label: 'Connection', value: CONNECTION[link] ?? 'Unknown' },
    ...(p.name ? [{ kind: 'info', key: `pad${p.slot}name`, label: 'Name', value: p.name }] : []),
    ...(battery ? [{ kind: 'info', key: `pad${p.slot}battery`, label: 'Battery', value: battery }] : []),
  ];
};
export function controllers(ctx) {
  const s = { pads: [], settings: {} };
  const load = async () => {
    [s.pads, s.settings] = await Promise.all([ctx.host.system.battery().catch(() => []), ctx.host.settings.get()]);
  };
  return {
    title: 'Controllers',
    layout: 'two',
    load,
    events: { settings: v => { s.settings = v; ctx.update(); } },
    cats: [
      {
        label: 'Controllers',
        enter: () => ctx.host.system.battery().then(p => { s.pads = p; ctx.update(); }, () => {}),
        rows: () => [
          setting(ctx, s, 'rumble', 'Vibration', 'A light tick on the controller each time you move.'),
          { kind: 'link', label: 'Test Buttons', desc: 'See each button and stick light up as you press it.', open: padTest },
          ...(s.pads.length
            ? s.pads.flatMap(padRows)
            : [{ kind: 'info', key: 'nopad', label: 'Controller', value: 'No controller connected', disabled: true }]),
        ],
      },
    ],
  };
}

// ---------- Library: games folder, check now, refresh art ----------
export function library(ctx) {
  const s = { settings: {}, refreshing: null, checked: null };
  return {
    title: 'Library',
    layout: 'two',
    load: async () => { s.settings = await ctx.host.settings.get(); },
    events: { settings: v => { s.settings = v; ctx.update(); } },
    cats: [
      {
        label: 'Games',
        rows: () => [
          {
            kind: 'action', label: 'Games Folder', value: () => s.settings.gamesRoot ?? '',
            desc: 'Each folder inside it is one game. Select to type a new path.',
            run: async () => {
              const path = await ctx.type({ title: 'Games Folder', value: s.settings.gamesRoot ?? '', placeholder: 'C:\\Games' });
              const v = path?.trim().replace(/[\\/]+$/, '');
              if (!v || v === s.settings.gamesRoot) return;
              if (!(await ctx.host.settings.folderExists(v))) {
                await ctx.dialog({ title: "That folder doesn't exist", text: v });
                return;
              }
              s.settings = await ctx.host.settings.set({ gamesRoot: v });
              ctx.host.library.rescan().catch(() => {});
            },
          },
          {
            kind: 'action', label: 'Check for New Games', value: () => s.checked ?? '',
            desc: 'Look in the games folder now instead of waiting.',
            run: async () => {
              const list = await ctx.host.library.rescan();
              s.checked = `${list.length} game${list.length === 1 ? '' : 's'}`;
            },
          },
          {
            kind: 'action', label: 'Refresh All Art', value: () => s.refreshing ?? '',
            desc: 'Fetch the store details, art and trailers for every game again.',
            run: async () => {
              if (s.refreshing && s.refreshing !== 'Done') return;
              const games = await ctx.host.library.list();
              let failed = 0;
              for (const [k, g] of games.entries()) {
                s.refreshing = `${k + 1} of ${games.length}`;
                ctx.update();
                await ctx.host.library.refresh(g.id).catch(() => { failed++; });
              }
              s.refreshing = failed ? `Done, ${failed} failed` : 'Done';
            },
          },
        ],
      },
    ],
  };
}

// ---------- Power: each action asks first ----------
const POWER = [
  ['sleep', 'Sleep', 'Put the PC to sleep?', 'Press any button on the controller or the power button to wake it.'],
  ['restart', 'Restart', 'Restart the PC?', 'Running games close without saving.'],
  ['shutdown', 'Shut Down', 'Shut down the PC?', 'Running games close without saving.'],
  ['exit', 'Exit to Desktop', 'Exit to the desktop?', 'Padwake closes and Windows shows its desktop.'],
];
export function power(ctx) {
  return {
    title: 'Power',
    layout: 'full',
    rows: POWER.map(([action, label, question, text]) => ({
      kind: 'action', label,
      desc: action === 'exit' ? 'Close Padwake and use Windows.' : undefined,
      run: async () => {
        const ok = await ctx.dialog({ title: question, text, buttons: [{ label, value: true }, { label: 'Cancel', value: false }], focus: 1 });
        if (ok) await ctx.host.system.power(action);
      },
    })),
  };
}

// ---------- System: system software (updates, cloud.js), start with Windows, about ----------
export function system(ctx) {
  const s = { withWindows: false, about: {}, update: null, settings: {} };
  return {
    title: 'System',
    layout: 'two',
    load: async () => {
      [s.withWindows, s.about, s.update, s.settings] = await Promise.all([
        ctx.host.startup.withWindows().catch(() => false),
        ctx.host.system.about().catch(() => ({})),
        ctx.host.cloud.update().catch(() => null),
        ctx.host.settings.get(),
      ]);
    },
    events: {
      update: v => { s.update = v; ctx.update(); },
      settings: v => { s.settings = v; ctx.update(); },
    },
    cats: [
      softwareCategory(ctx, s),
      {
        label: 'Start-up',
        rows: () => [{
          kind: 'toggle', label: 'Start with Windows', desc: 'Open Padwake when you sign in to Windows.',
          get: () => s.withWindows,
          set: async v => { s.withWindows = await ctx.host.startup.setWithWindows(v); },
        }],
      },
      {
        label: 'About',
        rows: () => [
          { kind: 'info', label: 'Version', value: s.about.version ?? '' },
          { kind: 'info', label: 'Data Folder', value: s.about.dataDir ?? '' },
          { kind: 'info', label: 'Program', value: s.about.exe ?? '' },
        ],
      },
    ],
  };
}
