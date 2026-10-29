// Network (row A10): connection status, nearby Wi-Fi, and joining a network with a password typed on the on-screen
// keyboard. The host builds a WPA2/WPA3-personal profile and asks Windows to connect (src-tauri/src/system/network.rs).

import { bars, LOCK } from '../icons.js';

const wait = ms => new Promise(r => setTimeout(r, ms));
const RESCAN_MS = 4000; // a Wi-Fi scan takes ~2–4 s; the list is read again once it has finished

export function network(ctx) {
  const s = { net: null, wifi: [], scanned: false, connecting: null, timer: 0 };
  const sys = ctx.host.system;
  const load = async () => {
    [s.net, s.wifi] = await Promise.all([sys.network().catch(() => null), sys.wifiNetworks().catch(() => [])]);
  };
  const rescan = () => {
    clearTimeout(s.timer);
    s.timer = setTimeout(async () => {
      [s.net, s.wifi] = await Promise.all([sys.network().catch(() => s.net), sys.wifiNetworks().catch(() => s.wifi)]);
      s.scanned = true;
      ctx.update();
    }, RESCAN_MS);
  };

  const status = () => {
    const n = s.net;
    if (!n) return [{ kind: 'info', label: 'Connection', value: 'Unknown' }];
    const rows = [{ kind: 'info', label: 'Connection', value: !n.connected ? 'Not connected' : n.kind === 'wifi' ? 'Wi-Fi' : 'Ethernet' }];
    if (n.kind === 'wifi' && n.connected) {
      rows.push({ kind: 'info', label: 'Network Name', value: n.ssid ?? '' }, { kind: 'info', label: 'Signal Strength', value: `${n.signal ?? 0}%` });
    }
    rows.push(
      { kind: 'info', label: 'IP Address', value: n.ip ?? '-' },
      { kind: 'info', label: 'Adapter', value: n.adapter || '-' },
      { kind: 'info', label: 'Wi-Fi', value: n.wifi === 'on' ? 'On' : n.wifi === 'off' ? 'Off' : 'No Wi-Fi adapter' },
    );
    return rows;
  };

  const wifiRows = () => {
    if (s.net?.wifi === 'none') return [{ kind: 'info', label: 'No Wi-Fi adapter', disabled: true }];
    if (s.net?.wifi === 'off') {
      return [{ kind: 'info', label: 'Wi-Fi is turned off', disabled: true, desc: 'Turn on Wi-Fi in Windows to see networks here.' }];
    }
    if (!s.wifi.length) return [{ kind: 'info', label: s.scanned ? 'No networks found' : 'Looking for networks…', disabled: true }];
    return s.wifi.map(n => ({
      kind: 'action', key: `wifi:${n.ssid}`, label: n.ssid,
      glyph: `${n.secure ? LOCK : ''}${bars(n.signal)}`,
      value: () => (n.connected ? 'Connected' : s.connecting === n.ssid ? 'Connecting…' : n.saved ? 'Saved' : ''),
      run: () => connect(n),
    }));
  };

  async function connect(n) {
    if (n.connected || s.connecting) return;
    if (n.auth === 'enterprise' || n.auth === 'other') {
      await ctx.dialog({ title: `Join ${n.ssid} in Windows`, text: 'This network needs a sign-in that only Windows settings can set up.' });
      return;
    }
    let password = '';
    if (n.secure && !n.saved) {
      password = await ctx.type({ title: `Password for ${n.ssid}`, secret: true, maxLength: 63 });
      if (password == null) return;
      if (password.length < 8) {
        await ctx.dialog({ title: 'That password is too short', text: 'Wi-Fi passwords have at least 8 characters.' });
        return;
      }
    }
    s.connecting = n.ssid;
    ctx.update();
    try {
      await sys.wifiConnect(n.ssid, password);
      // Windows connects in the background; wait up to 20 s for it to say so
      for (let i = 0; i < 20 && !(s.net?.connected && s.net.ssid === n.ssid); i++) {
        await wait(1000);
        s.net = await sys.network().catch(() => s.net);
      }
      s.wifi = await sys.wifiNetworks().catch(() => s.wifi);
      if (!(s.net?.connected && s.net.ssid === n.ssid)) {
        await ctx.dialog({ title: `Couldn't connect to ${n.ssid}`, text: 'Check the password and try again.' });
      }
    } catch (e) {
      await ctx.dialog({ title: `Couldn't connect to ${n.ssid}`, text: String(e?.message ?? e) });
    } finally {
      s.connecting = null;
      ctx.update();
    }
  }

  return {
    title: 'Network',
    layout: 'two',
    load: async () => { await load(); rescan(); },
    leave: () => clearTimeout(s.timer),
    cats: [
      { label: 'Connection Status', enter: () => sys.network().then(n => { s.net = n; ctx.update(); }, () => {}), rows: status },
      { label: 'Wi-Fi', enter: rescan, rows: wifiRows },
    ],
  };
}
