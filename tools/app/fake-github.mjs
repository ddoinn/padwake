#!/usr/bin/env node
// A stand-in for the parts of GitHub the cloud area uses, so the real app's updater and backup can be tested on the
// dev laptop without a GitHub account, a public release or the network. Point the app at it:
//   PADWAKE_GITHUB_API=http://127.0.0.1:<port>  PADWAKE_GITHUB_WEB=http://127.0.0.1:<port>
//   PADWAKE_GITHUB_CLIENT_ID=test
//
//   node tools/app/fake-github.mjs [--port 47801] [--release <dir> --tag v0.1.1] [--seed backup.json] [--polls 2]
//     --release   a release folder (tools/app/release.ps1 -DryRun makes one: the zip, padwake.exe,
//                 SHA256SUMS.txt, notes.md); every file but notes.md is an asset of the latest release of any repo
//     --seed      a backup file put in a "Padwake backup" gist before sign-in (a restore test)
//     --polls     sign-in is pending for this many token polls, then approved (as a person typing the code)
// Every request is logged; GET /state shows the gists.

import { createServer } from 'node:http';
import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : def; };
const PORT = Number(opt('port', 47801));
const RELEASE = opt('release', null);
const TAG = opt('tag', 'v0.1.1');
const POLLS = Number(opt('polls', 2));
const TOKEN = 'fake-token';
const base = `http://127.0.0.1:${PORT}`;

const gists = new Map();
let nextGist = 1, polls = 0;
const seed = opt('seed', null);
if (seed) {
  gists.set('g1', { id: 'g1', description: 'Padwake backup', public: false, files: { 'padwake-backup.json': { content: readFileSync(seed, 'utf8') } } });
  nextGist = 2;
}

const body = req => new Promise(r => { let s = ''; req.on('data', c => { s += c; }); req.on('end', () => r(s ? JSON.parse(s) : {})); });
const send = (res, status, v) => res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(v));
const authed = req => req.headers.authorization === `Bearer ${TOKEN}`;
const gistOut = g => ({ id: g.id, description: g.description, public: g.public, files: Object.fromEntries(Object.entries(g.files).map(([n, f]) => [n, { filename: n, content: f.content, truncated: false }])) });

createServer(async (req, res) => {
  const url = new URL(req.url, base);
  const p = url.pathname;
  console.log(new Date().toISOString().slice(11, 23), req.method, p + url.search);
  try {
    if (req.method === 'GET' && /^\/repos\/[^/]+\/[^/]+\/releases\/latest$/.test(p)) {
      if (!RELEASE) return send(res, 404, { message: 'Not Found' });
      const notes = existsSync(join(RELEASE, 'notes.md')) ? readFileSync(join(RELEASE, 'notes.md'), 'utf8') : '';
      const asset = name => ({ name, browser_download_url: `${base}/dl/${name}`, size: statSync(join(RELEASE, name)).size });
      return send(res, 200, { tag_name: TAG, body: notes, assets: readdirSync(RELEASE).filter(n => n !== 'notes.md').map(asset) });
    }
    if (req.method === 'GET' && p.startsWith('/dl/')) {
      const file = join(RELEASE ?? '.', decodeURIComponent(p.slice(4)));
      if (!RELEASE || !existsSync(file)) return send(res, 404, { message: 'Not Found' });
      const data = readFileSync(file);
      return res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': data.length }).end(data);
    }
    if (req.method === 'POST' && p === '/login/device/code') {
      const b = await body(req);
      if (!b.client_id) return send(res, 400, { error: 'unauthorized_client', error_description: 'no client_id' });
      polls = 0;
      return send(res, 200, { device_code: 'dev-code', user_code: 'TEST-1234', verification_uri: 'https://github.com/login/device', expires_in: 900, interval: 1 });
    }
    if (req.method === 'POST' && p === '/login/oauth/access_token') {
      await body(req);
      return send(res, 200, ++polls > POLLS ? { access_token: TOKEN, token_type: 'bearer', scope: 'gist' } : { error: 'authorization_pending' });
    }
    if (req.method === 'GET' && p === '/user') return authed(req) ? send(res, 200, { login: 'test-user' }) : send(res, 401, { message: 'Bad credentials' });
    if (p === '/gists' || p.startsWith('/gists/')) {
      if (!authed(req)) return send(res, 401, { message: 'Bad credentials' });
      const id = p.split('/')[2];
      if (req.method === 'GET' && !id) return send(res, 200, url.searchParams.get('page') === '1' || !url.searchParams.get('page') ? [...gists.values()].map(gistOut) : []);
      if (req.method === 'POST' && !id) {
        const b = await body(req);
        const g = { id: `g${nextGist++}`, description: b.description, public: !!b.public, files: b.files };
        gists.set(g.id, g);
        return send(res, 201, gistOut(g));
      }
      const g = gists.get(id);
      if (!g) return send(res, 404, { message: 'Not Found' });
      if (req.method === 'GET') return send(res, 200, gistOut(g));
      if (req.method === 'PATCH') {
        const b = await body(req);
        Object.assign(g.files, b.files ?? {});
        return send(res, 200, gistOut(g));
      }
    }
    if (p === '/state') return send(res, 200, [...gists.values()].map(gistOut));
    send(res, 404, { message: 'Not Found' });
  } catch (e) {
    console.log('error', e);
    send(res, 500, { message: String(e) });
  }
}).listen(PORT, '127.0.0.1', () => console.log(`fake GitHub on ${base}${RELEASE ? `, release ${TAG} from ${RELEASE}` : ''}`));
