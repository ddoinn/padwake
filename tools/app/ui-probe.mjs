#!/usr/bin/env node
// Checks the UI in headless Edge with the mock host: serves app/ui, opens index.html at 1920×1080, runs a scenario
// over raw CDP (Node's built-in WebSocket, no dependencies), samples elements every animation frame and saves
// screenshots. Later rows use it to check motion against design/ps5/ within ±1 frame.
//
//   node tools/app/ui-probe.mjs [--query "scene=home&focus=hades"] [--out W:\tmp\ui-probe] [--steps '<json>' | file.json]
//
// A scenario is a JSON array of steps, run in order:
//   {"wait": 500}                                   wait in ms
//   {"key": "ArrowRight"}                           press and release a key (ArrowLeft/Right/Up/Down, Enter, Escape, a-z)
//   {"shot": "home"}                                save <out>/home.png (1920×1080)
//   {"sample": "move", "select": {"focus": "#screen-home .tile:nth-child(2)", "ring": "#screen-home .ring"},
//    "ms": 400, "keys": ["ArrowRight"]}           sample every frame for ms, pressing keys 2 frames in; writes
//                                                   <out>/move.json: {keyT, frames: [{t, focus: {x, y, w, h, o}, ...}]}
//                                                   (t and keyT in ms from the first frame; x y w h = rect, o = opacity)
//   {"emit": "input", "payload": {...}}             send a mock host event (pad state, a game event)
//   {"eval": "document.title"}                      print the value of an expression
//   {"assert": "expr", "why": "..."}                fail the run (exit 1) unless the expression is truthy
// With no scenario it waits for the page and saves <out>/home.png.

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir, rm } from 'node:fs/promises';
import { existsSync, readFileSync } from 'node:fs';
import { extname, join, normalize, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args.splice(i, 2)[1] : def; };
const EDGE = opt('edge', process.env.EDGE || 'C:\\Program Files (x86)\\Microsoft\\EdgeCore\\156.0.4314.50\\msedge.exe');
const PROFILE = opt('profile', 'W:\\tmp\\edge-probe');
const OUT = resolve(opt('out', 'W:\\tmp\\ui-probe'));
const QUERY = opt('query', 'scene=home');
const STEPS = opt('steps', null);
const UI = resolve(dirname(fileURLToPath(import.meta.url)), '../../app/ui');
const scenario = STEPS ? JSON.parse(STEPS) : args[0] ? JSON.parse(readFileSync(args[0], 'utf8')) : [{ shot: 'home' }];

const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json',
  '.ttf': 'font/ttf', '.woff2': 'font/woff2', '.wav': 'audio/wav', '.mp3': 'audio/mpeg', '.mp4': 'video/mp4', '.png': 'image/png',
  '.jpg': 'image/jpeg', '.svg': 'image/svg+xml', '.webp': 'image/webp' };
const server = createServer(async (req, res) => {
  const path = normalize(join(UI, decodeURIComponent(new URL(req.url, 'http://x').pathname)));
  if (!path.startsWith(UI)) return res.writeHead(403).end();
  try {
    const body = await readFile(path.endsWith('\\') || path === UI ? join(path, 'index.html') : path);
    res.writeHead(200, { 'content-type': MIME[extname(path)] || 'application/octet-stream', 'cache-control': 'no-store' }).end(body);
  } catch { res.writeHead(404).end(); }
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html?${QUERY}`;

await mkdir(OUT, { recursive: true });
await rm(join(PROFILE, 'DevToolsActivePort'), { force: true });
const edge = spawn(EDGE, ['--headless=new', `--user-data-dir=${PROFILE}`, '--remote-debugging-port=0', '--window-size=1920,1080',
  '--hide-scrollbars', '--mute-audio', '--no-first-run', '--no-default-browser-check', 'about:blank'], { stdio: 'ignore' });
const sleep = ms => new Promise(r => setTimeout(r, ms));

let ws, nextId = 0;
const pending = new Map();
function cdp(method, params = {}) {
  const id = ++nextId;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((ok, fail) => pending.set(id, { ok, fail, method }));
}
async function evaluate(expression) {
  const r = await cdp('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(`${expression.slice(0, 60)}…: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
  return r.result.value;
}

const VK = { ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Enter: 13, Escape: 27, Backspace: 8, ' ': 32, Tab: 9 };
async function press(key) {
  const vk = VK[key] ?? key.toUpperCase().charCodeAt(0);
  const text = key === 'Enter' ? '\r' : key.length === 1 ? key : undefined;
  const code = key.length === 1 ? (/[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : key === ' ' ? 'Space' : key) : key;
  await cdp('Input.dispatchKeyEvent', { type: text ? 'keyDown' : 'rawKeyDown', key, code, windowsVirtualKeyCode: vk, text });
  await cdp('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: vk });
}

// runs in the page: one row per animation frame with each element's rect and own opacity
const SAMPLER = `(select, ms) => new Promise(done => {
  const frames = []; let t0 = null, keyT = null;
  addEventListener('keydown', () => { keyT ??= performance.now() - t0; }, { capture: true, once: true });
  const els = Object.fromEntries(Object.entries(select).map(([k, s]) => [k, document.querySelector(s)]));
  function frame(ts) {
    t0 ??= ts;
    const row = { t: +(ts - t0).toFixed(2) };
    for (const [k, el] of Object.entries(els)) {
      if (!el) { row[k] = null; continue; }
      const r = el.getBoundingClientRect();
      row[k] = { x: +r.x.toFixed(1), y: +r.y.toFixed(1), w: +r.width.toFixed(1), h: +r.height.toFixed(1), o: +(+getComputedStyle(el).opacity).toFixed(3) };
    }
    frames.push(row);
    if (ts - t0 < ms) requestAnimationFrame(frame); else done({ keyT: keyT === null ? null : +keyT.toFixed(2), frames });
  }
  requestAnimationFrame(frame);
})`;

try {
  const portFile = join(PROFILE, 'DevToolsActivePort');
  for (let i = 0; !existsSync(portFile); i++) { if (i > 150) throw new Error('Edge did not start'); await sleep(100); }
  const port = readFileSync(portFile, 'utf8').split('\n')[0].trim();
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
  ws.onmessage = m => {
    const msg = JSON.parse(m.data);
    const p = pending.get(msg.id);
    if (!p) return;
    pending.delete(msg.id);
    msg.error ? p.fail(new Error(`${p.method}: ${msg.error.message}`)) : p.ok(msg.result);
  };
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  await cdp('Runtime.enable');
  await cdp('Emulation.setDeviceMetricsOverride', { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
  await cdp('Page.navigate', { url });
  for (let i = 0; !(await evaluate(`document.documentElement.dataset.ready === '1'`).catch(() => false)); i++) {
    if (i > 100) throw new Error(`page not ready: ${url}`);
    await sleep(100);
  }
  console.log(`ready: ${url}`);

  for (const step of scenario) {
    if (step.wait) await sleep(step.wait);
    else if (step.key) await press(step.key);
    else if (step.emit) await evaluate(`window.__mock.emit(${JSON.stringify(step.emit)}, ${JSON.stringify(step.payload ?? null)})`);
    else if (step.eval) console.log(step.eval, '→', JSON.stringify(await evaluate(step.eval)));
    else if (step.assert) {
      const v = await evaluate(step.assert);
      console.log(`${v ? 'ok' : 'FAILED'}: ${step.why ?? step.assert}`);
      if (!v) throw new Error(`assert failed: ${step.assert}`);
    }
    else if (step.shot) {
      const { data } = await cdp('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: 1920, height: 1080, scale: 1 } });
      const file = join(OUT, `${step.shot}.png`);
      await writeFile(file, Buffer.from(data, 'base64'));
      console.log(`shot ${file}`);
    } else if (step.sample) {
      const run = evaluate(`(${SAMPLER})(${JSON.stringify(step.select)}, ${step.ms ?? 400})`);
      await sleep(2 * 16.7);
      for (const k of step.keys ?? []) await press(k);
      const result = await run;
      const file = join(OUT, `${step.sample}.json`);
      await writeFile(file, JSON.stringify(result, null, 1));
      console.log(`sample ${file}: ${result.frames.length} frames, key at ${result.keyT} ms`);
      for (const f of result.frames) {
        const cols = Object.keys(step.select).map(k => f[k] ? `${k} x${f[k].x} y${f[k].y} w${f[k].w} o${f[k].o}` : `${k} -`);
        console.log(`  ${String(f.t).padStart(7)}  ${cols.join('  ')}`);
      }
    } else throw new Error(`unknown step ${JSON.stringify(step)}`);
  }
  await cdp('Browser.close').catch(() => {});
} catch (e) {
  console.error(e.message);
  process.exitCode = 1;
} finally {
  ws?.close();
  server.close();
  setTimeout(() => edge.kill(), 1000).unref();
}
