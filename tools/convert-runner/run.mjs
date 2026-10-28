// Evani.Dev quality-gate runner (STAGE3_PLAN S3-6): renders models in the
// engine's own renderer (apps/model-view in gate mode) in one headless,
// muted Chrome at below-normal priority, and hands the pixels back to the
// converter (tools/evani-convert, `cargo xtask convert --gate`). Node
// built-ins only; the Chrome finder and CDP client are the bench runner's.
//
// Gate mode (the converter starts it):
//   node tools/convert-runner/run.mjs --dist <model-view gate build> --serve <folder>
//   stdout, first line: {"ready":true,"port":..,"chrome":..,"version":..}
//   stdin, one JSON job per line: {id, tier, width, height, model, place, views, out}
//     model: a URL path under /m/ (the --serve folder); views: camera poses
//     [{eye,target,up,fov_y_deg}]; out: the folder for view<i>.rgba.
//   stdout, one line per job: {id, ok, w, h, n, ms, error, console}
//   Closing stdin stops Chrome and the server.
//
// Calibration mode (Chris's blind test, STAGE3_PLAN decision 4):
//   node tools/convert-runner/run.mjs --calibrate <folder>
//   Serves calibrate.html with the folder's pairs (key.json; its scores never
//   reach the page) and saves each answer to <folder>/answers.json.
//
// Ports 8230 to 8239 (the first free one) unless --port is given.
// Safety: only the Chrome it started is ever stopped, by its own PID.

import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';

import { STUBS, sleep, startChrome } from '../bench-runner/chrome.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[++i] : '1';
}
const log = (...m) => console.error('[convert-runner]', ...m);
const say = (o) => process.stdout.write(JSON.stringify(o) + '\n');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.glb': 'model/gltf-binary',
  '.gltf': 'model/gltf+json',
  '.bin': 'application/octet-stream',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.ktx2': 'image/ktx2',
  '.css': 'text/css; charset=utf-8',
};

/** A file under `base` for a URL path, or null (never outside `base`). */
function fileUnder(base, urlPath) {
  let p;
  try {
    p = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  const full = normalize(join(base, p));
  const root = resolve(base);
  if (full !== root && !full.startsWith(root + sep)) return null;
  try {
    const st = statSync(full);
    if (st.isDirectory()) return existsSync(join(full, 'index.html')) ? join(full, 'index.html') : null;
    return full;
  } catch {
    return null;
  }
}

function send(res, code, body, type = 'text/plain; charset=utf-8') {
  res.writeHead(code, { 'content-type': type, 'cache-control': 'no-store' });
  res.end(body);
}

function readBody(req, limit = 512 * 1024 * 1024) {
  return new Promise((resolveBody, reject) => {
    const parts = [];
    let n = 0;
    req.on('data', (c) => {
      n += c.length;
      if (n > limit) {
        reject(new Error('body too large'));
        req.destroy();
      } else parts.push(c);
    });
    req.on('end', () => resolveBody(Buffer.concat(parts)));
    req.on('error', reject);
  });
}

async function listen(handler) {
  const ports = args.port ? [Number(args.port)] : [8230, 8231, 8232, 8233, 8234, 8235, 8236, 8237, 8238, 8239];
  for (const port of ports) {
    const server = createServer((req, res) => {
      handler(req, res).catch((e) => send(res, 500, String(e.message ?? e)));
    });
    const ok = await new Promise((r) => {
      server.once('error', () => r(false));
      server.listen(port, '127.0.0.1', () => r(true));
    });
    if (ok) return { server, port };
  }
  throw new Error(`no free port in ${ports[0]}..${ports[ports.length - 1]}`);
}

// ---------------------------------------------------------------- gate mode

async function gate() {
  const dist = args.dist;
  const serveDir = args.serve;
  if (!dist || !existsSync(join(dist, 'index.html'))) throw new Error('--dist must be the model-view gate build (no index.html in ' + dist + ')');
  if (!serveDir) throw new Error('--serve <folder> is required');
  let current = null; // the job whose shots are being uploaded
  const { server, port } = await listen(async (req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    if (req.method === 'POST' && url.pathname.startsWith('/put/')) {
      const [, , token, idx] = url.pathname.split('/');
      if (!current || token !== current.token || !/^\d+$/.test(idx)) return send(res, 409, 'no such job');
      const body = await readBody(req);
      writeFileSync(join(current.out, `view${idx}.rgba`), body);
      return send(res, 200, 'ok');
    }
    if (req.method !== 'GET') return send(res, 405, 'method');
    const f = url.pathname.startsWith('/m/') ? fileUnder(serveDir, url.pathname.slice(3)) : fileUnder(dist, url.pathname);
    if (!f) return send(res, 404, 'not found');
    res.writeHead(200, { 'content-type': MIME[extname(f).toLowerCase()] ?? 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(readFileSync(f));
  });
  let chrome = null;
  const stopAll = async () => {
    if (chrome) await chrome.stop();
    chrome = null;
    server.close();
  };
  try {
    chrome = await startChrome({ chrome: args.chrome, width: 1920, height: 1080, prefix: 'evani-gate-' });
    log('chrome:', chrome.path, chrome.version);
    say({ ready: true, port, chrome: chrome.path, version: chrome.version });
    const rl = createInterface({ input: process.stdin });
    for await (const line of rl) {
      if (!line.trim()) continue;
      let job;
      try {
        job = JSON.parse(line);
      } catch (e) {
        say({ ok: false, error: 'bad job line: ' + e.message });
        continue;
      }
      const t0 = Date.now();
      try {
        mkdirSync(job.out, { recursive: true });
        current = { token: Math.random().toString(36).slice(2), out: job.out };
        const r = await renderJob(chrome.cdp, port, job, current.token);
        say({ id: job.id, ok: true, ...r, ms: Date.now() - t0 });
      } catch (e) {
        say({ id: job.id, ok: false, error: String(e.message ?? e), console: e.console ?? [], ms: Date.now() - t0 });
      } finally {
        current = null;
      }
    }
  } finally {
    await stopAll();
  }
}

async function renderJob(cdp, port, job, token) {
  const timeoutMs = Number(job.timeout_ms ?? 180000);
  const consoleLines = [];
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (method, params, ms) => cdp.send(method, params, sessionId, ms);
  let crashed = false;
  const onMsg = (m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === 'Inspector.targetCrashed') crashed = true;
    if (m.method === 'Runtime.exceptionThrown' && consoleLines.length < 20) {
      consoleLines.push('exception: ' + (m.params.exceptionDetails?.exception?.description ?? m.params.exceptionDetails?.text));
    }
    if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error' && consoleLines.length < 20) {
      consoleLines.push('console.error: ' + m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
    }
  };
  cdp.on(onMsg);
  try {
    await s('Page.enable');
    await s('Runtime.enable');
    await s('Inspector.enable');
    await s('Emulation.setDeviceMetricsOverride', { width: job.width, height: job.height, deviceScaleFactor: 1, mobile: false });
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
    const q = new URLSearchParams({ gate: '1', tier: job.tier, model: job.model, views: JSON.stringify(job.views) });
    if (job.place) q.set('place', JSON.stringify(job.place));
    if (job.look) q.set('look', job.look);
    await s('Page.navigate', { url: `http://127.0.0.1:${port}/?${q}` });
    const start = Date.now();
    let st = {};
    for (;;) {
      if (crashed) throw new Error('the page crashed');
      if (Date.now() - start > timeoutMs) throw new Error(`timed out after ${Math.round(timeoutMs / 1000)} s (${st.progress ?? 'no progress'})`);
      const r = await s('Runtime.evaluate', {
        expression: `JSON.stringify((() => { const g = window.__gate; const e = window.__engine; return g ? { done: !!g.done, error: g.error || null, progress: g.progress || '', w: g.w, h: g.h, n: g.n } : { engineError: e ? e.error : null }; })())`,
        returnByValue: true,
      });
      st = JSON.parse(r.result?.value ?? '{}');
      if (st.error) throw new Error('gate: ' + st.error);
      if (st.done) break;
      if (st.engineError && Date.now() - start > 5000) throw new Error('engine error: ' + st.engineError);
      await sleep(150);
    }
    // Upload every shot to the server (raw RGBA, no base64 round trip).
    const up = await s(
      'Runtime.evaluate',
      {
        expression: `(async () => { const s = window.__gateShots || []; for (let i = 0; i < s.length; i++) { const r = await fetch('/put/${token}/' + i, { method: 'POST', body: s[i] }); if (!r.ok) throw new Error('upload ' + i + ': ' + r.status); } return s.length; })()`,
        awaitPromise: true,
        returnByValue: true,
      },
      120000,
    );
    if (up.exceptionDetails) throw new Error('upload: ' + (up.exceptionDetails.exception?.description ?? up.exceptionDetails.text));
    if (up.result?.value !== st.n) throw new Error(`upload: ${up.result?.value} of ${st.n} shots`);
    return { w: st.w, h: st.h, n: st.n };
  } catch (e) {
    e.console = consoleLines;
    throw e;
  } finally {
    cdp.off(onMsg);
    try {
      await cdp.send('Target.closeTarget', { targetId });
    } catch {
      // already gone
    }
  }
}

// --------------------------------------------------------- calibration mode

async function calibrate() {
  const dir = resolve(args.calibrate);
  const keyFile = join(dir, 'key.json');
  if (!existsSync(keyFile)) throw new Error(`${keyFile}: no pairs yet (make them with cargo xtask convert <model> --gate-pairs ${dir})`);
  const page = readFileSync(join(here, 'calibrate.html'));
  // Blind: the page only ever sees a random token per pair and pictures
  // "1" and "2" behind it (a random side per pair, per start). The real
  // pair ids, file names (which say which side is the original, and which
  // pair is the control) and scores stay here.
  const key = JSON.parse(readFileSync(keyFile, 'utf8'));
  const byTok = new Map();
  const tokOf = new Map();
  for (const p of key.pairs ?? []) {
    const tok = randomBytes(6).toString('hex');
    byTok.set(tok, { pair: p, swap: Math.random() < 0.5 });
    tokOf.set(p.id, tok);
  }
  // Picture n (1 or 2) of a token: the original (a) or the converted one (b).
  const side = (e, n) => ((n === 1) !== e.swap ? 'a' : 'b');
  const { port } = await listen(async (req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    if (req.method === 'GET' && (url.pathname === '/' || url.pathname === '/index.html')) return send(res, 200, page, MIME['.html']);
    if (req.method === 'GET' && url.pathname === '/pairs') {
      const pairs = [...byTok.keys()].map((tok) => ({ id: tok, a: `${tok}-1.png`, b: `${tok}-2.png` }));
      return send(res, 200, JSON.stringify({ pairs }), MIME['.json']);
    }
    if (req.method === 'GET' && url.pathname === '/answers') {
      // The answers so far (tokens only), so the test can resume.
      const f = join(dir, 'answers.json');
      if (!existsSync(f)) return send(res, 200, JSON.stringify({ answers: [] }), MIME['.json']);
      const a = JSON.parse(readFileSync(f, 'utf8'));
      const answers = (a.answers ?? []).filter((x) => tokOf.has(x.id)).map((x) => ({ id: tokOf.get(x.id), answer: x.answer }));
      return send(res, 200, JSON.stringify({ answers }), MIME['.json']);
    }
    if (req.method === 'GET' && url.pathname.startsWith('/img/')) {
      const m = /^([0-9a-f]{12})-([12])\.png$/.exec(url.pathname.slice(5));
      const e = m && byTok.get(m[1]);
      if (!e) return send(res, 404, 'not found');
      const name = e.pair[side(e, Number(m[2]))];
      const f = typeof name === 'string' && /^[A-Za-z0-9_.-]+\.png$/.test(name) ? fileUnder(dir, name) : null;
      if (!f) return send(res, 404, 'not found');
      res.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'no-store' });
      return res.end(readFileSync(f));
    }
    if (req.method === 'POST' && url.pathname === '/save') {
      const body = JSON.parse((await readBody(req, 1024 * 1024)).toString('utf8'));
      if (!Array.isArray(body.answers)) return send(res, 400, 'answers must be a list');
      // Back to the real ids; shown_first says which real picture came first.
      const answers = body.answers
        .filter((a) => a && byTok.has(a.id) && (a.answer === 'same' || a.answer === 'different'))
        .map((a) => {
          const e = byTok.get(a.id);
          return { id: e.pair.id, answer: a.answer, shown_first: side(e, a.shown_first === 'b' ? 2 : 1), ms: Number(a.ms) || 0 };
        });
      // An answer is given once: a pair already in the file (an earlier
      // session, resumed) keeps its saved record.
      const f = join(dir, 'answers.json');
      const before = existsSync(f) ? (JSON.parse(readFileSync(f, 'utf8')).answers ?? []) : [];
      for (const a of answers) {
        const k = before.findIndex((b) => b.id === a.id);
        if (k < 0) before.push(a);
      }
      answers.length = 0;
      answers.push(...before);
      writeFileSync(join(dir, 'answers.json'), JSON.stringify({ format: 1, saved: new Date().toISOString(), answers }, null, 2) + '\n');
      return send(res, 200, JSON.stringify({ saved: answers.length }), MIME['.json']);
    }
    return send(res, 404, 'not found');
  });
  const url = `http://127.0.0.1:${port}/`;
  log('blind test page:', url, '(answers go to', join(dir, 'answers.json') + ')');
  say({ calibrate: true, url, answers: join(dir, 'answers.json') });
  // Serve until stopped (Ctrl+C, or the caller closes stdin).
  await new Promise((r) => {
    process.stdin.on('end', r);
    process.stdin.on('close', r);
    process.on('SIGINT', r);
    process.stdin.resume();
  });
  process.exit(0);
}

// Below-normal priority for this process (Chrome inherits it on Windows).
if (process.platform === 'win32') {
  try {
    spawnSync('powershell', ['-NoProfile', '-Command', `(Get-Process -Id ${process.pid}).PriorityClass = 'BelowNormal'`], { stdio: 'ignore', windowsHide: true });
  } catch {
    // run-low.mjs or the caller may already have lowered it
  }
}

(args.calibrate ? calibrate() : gate()).catch((e) => {
  console.error('[convert-runner] error:', e.message ?? e);
  say({ ready: false, error: String(e.message ?? e) });
  process.exit(2);
});
