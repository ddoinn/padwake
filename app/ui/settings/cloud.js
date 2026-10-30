// Online and free, through GitHub (src-tauri cloud area): System → System Software (updates) and Saved Data (a
// backup of the time played and the journal in a private gist on the player's own GitHub account). Both follow the
// host's `update` and `backup` events while open; the shapes are in host.js.

import { date } from '../journal/shared.js';

const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
// "github.com/login/device": what a person types, without the scheme
const bare = uri => (uri ?? '').replace(/^https?:\/\//, '').replace(/\/$/, '');
const when = t => {
  if (!t) return '';
  const d = new Date(t), today = new Date();
  const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return d.toDateString() === today.toDateString() ? `Today, ${time}` : `${date(t)}, ${time}`;
};
// release notes as plain lines: list markers and Markdown emphasis dropped, at most 4 lines
function notesText(notes) {
  const lines = (notes ?? '').split(/\r?\n/).map(l => l.replace(/^\s*(?:[-*+]|\d+\.)\s+/, '').replace(/[*_`#>]/g, '').trim()).filter(Boolean);
  return lines.slice(0, 4).join('\n');
}

// ---------- System → System Software ----------

const updateValue = u => {
  switch (u?.state) {
    case 'checking': return 'Checking…';
    case 'downloading': return `Downloading ${Math.round((u.progress ?? 0) * 100)} %`;
    case 'ready': return `Version ${u.latest} is ready`;
    case 'latest': return 'Up to date';
    case 'error': return "Couldn't check";
    default: return u ? `Version ${u.current}` : '';
  }
};

// "Update and Restart?" for a ready update; resolves after the player answers
export async function offerInstall(ctx, u) {
  const notes = notesText(u.notes);
  const ok = await ctx.dialog({
    title: `Update to version ${u.latest}?`,
    text: `${notes ? `${notes}\n\n` : ''}Padwake restarts. It takes a few seconds.`,
    buttons: [{ label: 'Update and Restart', value: true }, { label: 'Not Now', value: false }],
  });
  if (ok) await ctx.host.cloud.installUpdate();
}

// the System page's first category; s is the page's state ({update, settings})
export function softwareCategory(ctx, s) {
  return {
    label: 'System Software',
    enter: () => ctx.host.cloud.update().then(u => { s.update = u; ctx.update(); }, () => {}),
    rows: () => [
      {
        kind: 'action', label: 'System Software Update', key: 'update',
        value: () => updateValue(s.update),
        desc: () => (s.update?.state === 'error' ? s.update.error
          : s.update?.state === 'ready' ? 'Select to install it now, or it installs the next time Padwake starts.'
            : 'Look for a new version of Padwake.'),
        run: async () => {
          if (s.update?.state === 'checking' || s.update?.state === 'downloading') return;
          if (s.update?.state !== 'ready') s.update = await ctx.host.cloud.checkUpdate();
          const u = s.update;
          if (u.state === 'ready') return offerInstall(ctx, u);
          if (u.state === 'latest') return ctx.dialog({ title: 'Padwake is up to date', text: `Version ${u.current}` });
          if (u.state === 'error') return ctx.dialog({ title: "Couldn't check for updates", text: u.error ?? '' });
        },
      },
      {
        kind: 'toggle', label: 'Install Updates Automatically', key: 'autoUpdate',
        desc: 'Download new versions in the background and install them the next time Padwake starts.',
        get: () => s.settings.autoUpdate !== false,
        set: async v => { s.settings = await ctx.host.settings.set({ autoUpdate: v }); },
      },
      { kind: 'info', label: 'Version', key: 'version', value: () => s.update?.current ?? '' },
    ],
  };
}

// ---------- Saved Data ----------

export function savedData(ctx) {
  const s = { backup: null, settings: {} };
  const b = () => s.backup ?? {};
  const busy = () => ['saving', 'restoring', 'signing-in'].includes(b().state);

  async function signIn() {
    const started = await ctx.host.cloud.signIn();
    s.backup = started;
    let off = () => {};
    const done = new Promise(resolve => {
      off = ctx.host.on('backup', v => { if (v.state !== 'signing-in') resolve(v); });
    });
    const answer = await ctx.dialog({
      title: 'Sign In with GitHub',
      text: `On your phone or computer, go to ${bare(started.code?.uri)} and enter this code.`,
      code: started.code?.userCode ?? '',
      buttons: [{ label: 'Cancel', value: 'cancel' }],
      until: done,
    });
    off();
    if (answer === 'cancel' || answer === null) {
      s.backup = await ctx.host.cloud.cancelSignIn();
      return;
    }
    s.backup = answer;
    ctx.update();
    if (answer.state === 'error') return ctx.dialog({ title: "Couldn't sign in", text: answer.error ?? '' });
    if (answer.found) return offerRestore(answer.found);
    return ctx.dialog({ title: `Signed in as ${answer.login}`, text: 'Your time played and journal back up to a private gist on your GitHub account after each game.' });
  }

  async function offerRestore(found) {
    const ok = await ctx.dialog({
      title: 'Restore your backup?',
      text: `GitHub has a backup from ${found.machine || 'another PC'}, ${date(found.savedAt)}, with ${plural(found.games, 'game')} you played or reviewed. ` +
        'Their time and journal come back. Nothing on this PC is lost.',
      buttons: [{ label: 'Restore', value: true }, { label: 'Not Now', value: false }],
    });
    if (!ok) {
      s.backup = await ctx.host.cloud.dismissFound();
      return;
    }
    return restore();
  }

  async function restore() {
    s.backup = await ctx.host.cloud.restore();
    const r = s.backup.restored ?? { updated: 0, added: 0 };
    const parts = [
      r.updated && `${plural(r.updated, 'game')} on this PC got ${r.updated === 1 ? 'its' : 'their'} time and journal back`,
      r.added && `${plural(r.added, 'game')} joined your Journal`,
    ].filter(Boolean);
    return ctx.dialog({ title: 'Restored', text: parts.length ? `${parts.join(', and ')}.` : 'This PC already had everything in the backup.' });
  }

  const backupValue = () => {
    if (b().state === 'saving') return 'Backing up…';
    if (b().state === 'error') return "Didn't back up";
    return b().savedAt ? when(b().savedAt) : 'Not yet';
  };

  return {
    title: 'Saved Data',
    layout: 'two',
    load: async () => {
      [s.backup, s.settings] = await Promise.all([ctx.host.cloud.backup().catch(() => null), ctx.host.settings.get()]);
    },
    events: {
      backup: v => { s.backup = v; ctx.update(); },
      settings: v => { s.settings = v; ctx.update(); },
    },
    cats: [
      {
        label: 'Cloud Backup',
        enter: () => ctx.host.cloud.backup().then(v => { s.backup = v; ctx.update(); }, () => {}),
        rows: () => {
          if (!b().available) {
            return [{
              kind: 'info', key: 'unavailable', label: 'GitHub Backup', value: 'Not set up', disabled: true,
              desc: "This version of Padwake can't sign in to GitHub yet.",
            }];
          }
          if (!b().signedIn) {
            return [
              {
                kind: 'action', key: 'sign-in', label: 'Sign In with GitHub',
                desc: 'Keep your time played, ratings, status and notes in a private gist on your own GitHub account. It is free, and a new PC gets them back.',
                run: () => (busy() ? null : signIn()),
              },
            ];
          }
          return [
            { kind: 'info', key: 'account', label: 'GitHub Account', value: () => b().login ?? '' },
            {
              kind: 'action', key: 'now', label: 'Back Up Now', value: backupValue,
              desc: () => (b().state === 'error' ? b().error : 'Last backup to your private gist.'),
              run: async () => { if (!busy()) s.backup = await ctx.host.cloud.backUpNow(); },
            },
            {
              kind: 'toggle', key: 'auto', label: 'Back Up Automatically',
              desc: 'After each game, and when you change your Journal.',
              get: () => s.settings.backupAuto !== false,
              set: async v => { s.settings = await ctx.host.settings.set({ backupAuto: v }); },
            },
            {
              kind: 'action', key: 'restore', label: 'Restore from Backup',
              desc: 'Bring back time played and your Journal from GitHub. The larger time and the notes on this PC stay.',
              run: async () => {
                if (busy()) return;
                if (b().found) return offerRestore(b().found);
                const ok = await ctx.dialog({
                  title: 'Restore from your backup?',
                  text: 'Time played and journal entries come back from GitHub. Nothing on this PC is lost.',
                  buttons: [{ label: 'Restore', value: true }, { label: 'Cancel', value: false }],
                });
                if (ok) return restore();
              },
            },
            {
              kind: 'action', key: 'sign-out', label: 'Sign Out',
              desc: 'This PC stops backing up. The backup stays on your GitHub account.',
              run: async () => {
                const ok = await ctx.dialog({
                  title: 'Sign out of GitHub?', text: 'Backups stop on this PC. Your backup stays on GitHub.',
                  buttons: [{ label: 'Sign Out', value: true }, { label: 'Cancel', value: false }], focus: 1,
                });
                if (ok) s.backup = await ctx.host.cloud.signOut();
              },
            },
          ];
        },
      },
    ],
  };
}
