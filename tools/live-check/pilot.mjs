// Evani.Dev pilot driver: starts this worktree's live bridge for a game (or
// reuses it), opens the page in ONE headless Chrome (pointer lock,
// fullscreen and keyboard lock stubbed; the real mouse and keyboard are never
// used), then runs a plan of steps from a JSON file and writes every result
// into --out.
//
//   node tools/live-check/pilot.mjs --game kart-racer --port 8093 --plan plan.json --out <dir>
//        [--width 1920 --height 1080] [--keep-bridge]
//
// Plan: a JSON array of steps, run in order:
//   { "cmd": "bench.run", "params": {...}, "save": "name" }   live command; result -> name.json
//                                                             (png_base64 fields are written as PNGs)
//   { "page": "name.png", "hide_overlay": true }              page screenshot (canvas + DOM HUD)
//   { "eval": "js expression", "save": "name" }              Runtime.evaluate (returnByValue)
//   { "sleep": 500 }                                          wait (ms)
//   { "video": "name", "seconds": 20, "fps": 15 }             CDP screencast frames -> name/ (and
//                                                             name.webm when ffmpeg is on PATH)
//   { "key": "KeyW", "down": true }                           synthetic key event to the page (CDP)
//
// Only the processes it started are stopped at the end.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) {
    const n = process.argv[i + 1];
    if (n === undefined || n.startsWith('--')) args[a.slice(2)] = true;
    else args[a.slice(2)] = process.argv[++i];
  }
}
const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const OUT = resolve(ROOT, args.out ?? 'target/pilot');
const GAME = args.game ?? 'kart-racer';
const W = Number(args.width ?? 1920);
const H = Number(args.height ?? 1080);
const PLAN = JSON.parse(readFileSync(resolve(args.plan), 'utf8'));
const log = (...m) => console.error('[pilot]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const norm = (p) => resolve(p).replace(/\\/g, '/').replace(/\/$/, '').toLowerCase();

function findChrome() {
  const c = [args.chrome, process.env.EVANI_CHROME];
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]) {
    if (base) c.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  }
  return c.find((x) => x && existsSync(x));
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    this.handlers = [];
    ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id !== undefined) {
        const p = this.pending.get(msg.id);
        if (p) {
          this.pending.delete(msg.id);
          if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.message}`));
          else p.resolve(msg.result);
        }
      } else for (const h of this.handlers) h(msg);
    });
  }
  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => {
      ws.addEventListener('open', res, { once: true });
      ws.addEventListener('error', () => rej(new Error('cannot connect ' + url)), { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}, sessionId, ms = 120000) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      const t = setTimeout(() => (this.pending.delete(id), rej(new Error(method + ': timeout'))), ms);
      this.pending.set(id, { resolve: (v) => (clearTimeout(t), res(v)), reject: (e) => (clearTimeout(t), rej(e)), method });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
}

const STUBS = `(() => {
  const noop = () => Promise.resolve();
  try { Element.prototype.requestPointerLock = noop; } catch (e) {}
  try { Element.prototype.requestFullscreen = noop; } catch (e) {}
  try { Document.prototype.exitPointerLock = function () {}; } catch (e) {}
  try { if (typeof Keyboard !== 'undefined') Keyboard.prototype.lock = noop; } catch (e) {}
  try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
})();`;

async function status(port) {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/live/status`, { signal: AbortSignal.timeout(2000) });
    return await r.json();
  } catch {
    return null;
  }
}

let n = 0;
async function cmd(port, token, name, params, ms = 300000, note) {
  const body = { type: 'cmd', v: 1, id: `pilot-${++n}`, cmd: name, params, ...(note ? { note } : {}) };
  const r = await fetch(`http://127.0.0.1:${port}/live/v1/cmd`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(ms),
  });
  const v = await r.json();
  if (!v.ok) throw new Error(`${name}: ${v.error?.code}: ${v.error?.message}`);
  return v.result;
}

/** Write png_base64 fields as files, return the JSON without them. */
function stripPngs(v, base, list = []) {
  if (Array.isArray(v)) return v.map((x, i) => stripPngs(x, `${base}-${i + 1}`, list));
  if (v && typeof v === 'object') {
    const o = {};
    for (const [k, x] of Object.entries(v)) {
      if (k.endsWith('png_base64') && typeof x === 'string') {
        const f = `${base}${k === 'png_base64' ? '' : '-' + k.replace('_png_base64', '')}.png`;
        writeFileSync(join(OUT, f), Buffer.from(x, 'base64'));
        list.push(f);
        o[k.replace('png_base64', 'file')] = f;
      } else o[k] = stripPngs(x, base, list);
    }
    return o;
  }
  return v;
}

/** The session file of a bridge this script starts (never the Evani.dev tab's). */
const ownSession = (port) => join(ROOT, '.evani', 'live', 'review', String(port), 'session.json');

/** The token of the bridge on `port`: from its own session file, else the tab's if it is that port's. */
function sessionToken(port) {
  for (const f of [ownSession(port), join(ROOT, '.evani', 'live', 'session.json')]) {
    try {
      const s = JSON.parse(readFileSync(f, 'utf8'));
      if (s.http === `http://127.0.0.1:${port}` && s.token) return s.token;
    } catch {}
  }
  throw new Error(`no session file for the bridge on ${port}`);
}

async function main() {
  const port = Number(args.port ?? 8093);
  const started = [];
  let chrome = null;
  let chromeExited = true;
  let cdp = null;
  let profile = null;
  mkdirSync(OUT, { recursive: true });
  const results = [];
  try {
    const st = await status(port);
    if (st && !(st.type === 'live_hello' && norm(st.root ?? '') === norm(ROOT) && st.game === GAME)) {
      throw new Error(`port ${port} is used by something else`);
    }
    if (!st) {
      log('building the bridge...');
      const b = spawnSync('cargo', ['build', '-j', '4', '-p', 'evani-live-bridge'], { cwd: ROOT, stdio: ['ignore', 'ignore', 'inherit'] });
      if (b.status !== 0) throw new Error('bridge build failed');
      const exe = join(ROOT, 'target', 'debug', 'evani-live-bridge.exe');
      const env = { ...process.env, CARGO_BUILD_JOBS: '4' };
      delete env.NO_COLOR;
      // Its own session file: never overwrite or delete the Evani.dev tab's.
      const bridge = spawn(exe, ['serve', '--root', ROOT, '--port', String(port), '--game', GAME, '--session', ownSession(port), '--quiet'], {
        cwd: ROOT,
        stdio: ['ignore', 'ignore', 'inherit'],
        windowsHide: true,
        env,
      });
      if (!args['keep-bridge']) started.push(bridge);
      else bridge.unref();
    }
    const t0 = Date.now();
    let last = '';
    for (;;) {
      const s = await status(port);
      if (s && s.phase !== last) log('bridge:', (last = s.phase));
      if (s?.phase === 'idle') break;
      if (s?.phase === 'error') throw new Error('game build failed:\n' + s.error);
      if (Date.now() - t0 > 30 * 60000) throw new Error('build took over 30 min');
      await sleep(1000);
    }
    const token = sessionToken(port);

    const chromePath = findChrome();
    if (!chromePath) throw new Error('no Chrome');
    profile = mkdtempSync(join(tmpdir(), 'evani-pilot-'));
    chrome = spawn(
      chromePath,
      [
        '--headless=new',
        `--user-data-dir=${profile}`,
        '--remote-debugging-port=0',
        '--no-first-run',
        '--no-default-browser-check',
        '--enable-unsafe-webgpu',
        '--enable-webgpu-developer-features',
        '--disable-background-timer-throttling',
        '--disable-renderer-backgrounding',
        '--disable-backgrounding-occluded-windows',
        `--window-size=${W},${H}`,
        'about:blank',
      ],
      { stdio: 'ignore', windowsHide: true },
    );
    chromeExited = false;
    chrome.on('exit', () => (chromeExited = true));
    let wsUrl = null;
    for (let i = 0; i < 200 && !wsUrl && !chromeExited; i++) {
      const f = join(profile, 'DevToolsActivePort');
      if (existsSync(f)) {
        const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
        if (p && path) wsUrl = `ws://127.0.0.1:${p}${path}`;
      }
      if (!wsUrl) await sleep(100);
    }
    if (!wsUrl) throw new Error('Chrome did not start');
    cdp = await Cdp.connect(wsUrl);
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    const s = (m, p, ms) => cdp.send(m, p, sessionId, ms);
    const consoleLines = [];
    const frames = [];
    cdp.handlers.push((m) => {
      if (m.sessionId !== sessionId) return;
      if (m.method === 'Runtime.exceptionThrown') consoleLines.push('exception: ' + (m.params.exceptionDetails?.exception?.description ?? ''));
      if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') consoleLines.push('error: ' + m.params.args.map((a) => a.value ?? '').join(' ').slice(0, 300));
      if (m.method === 'Page.screencastFrame') {
        frames.push(m.params.data);
        cdp.send('Page.screencastFrameAck', { sessionId: m.params.sessionId }, sessionId).catch(() => {});
      }
    });
    await s('Page.enable');
    await s('Runtime.enable');
    await s('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
    await s('Page.navigate', { url: `http://127.0.0.1:${port}/?live=1` });
    const t1 = Date.now();
    while (!(await status(port))?.engine_connected) {
      if (Date.now() - t1 > 90000) throw new Error('page did not connect\n' + consoleLines.join('\n'));
      await sleep(500);
    }
    log('page connected');
    await sleep(3000);

    for (const [i, step] of PLAN.entries()) {
      const label = step.save ?? step.page ?? step.video ?? step.cmd ?? `step${i + 1}`;
      try {
        if (step.cmd) {
          const r = await cmd(port, token, step.cmd, step.params ?? {}, step.ms ?? 600000, step.note);
          const clean = stripPngs(r, step.save ?? `step${i + 1}`);
          if (step.save) writeFileSync(join(OUT, step.save + '.json'), JSON.stringify(clean, null, 2));
          results.push({ step: label, ok: true, result: step.save ? undefined : clean });
          log(`${step.cmd}: ok`);
        } else if (step.page) {
          if (step.hide_overlay !== false) {
            await s('Runtime.evaluate', { expression: `(()=>{const e=document.getElementById('evani-overlay'); if(e) e.style.display='none';})()` });
          }
          const shot = await s('Page.captureScreenshot', { format: 'png' });
          writeFileSync(join(OUT, step.page), Buffer.from(shot.data, 'base64'));
          results.push({ step: label, ok: true });
          log(`page -> ${step.page}`);
        } else if (step.eval) {
          const r = await s('Runtime.evaluate', { expression: step.eval, returnByValue: true, awaitPromise: true });
          results.push({ step: label, ok: true, value: r.result?.value });
          if (step.save) writeFileSync(join(OUT, step.save + '.json'), JSON.stringify(r.result?.value ?? null, null, 2));
        } else if (step.sleep) {
          await sleep(step.sleep);
        } else if (step.key) {
          const code = step.key;
          const key = code.startsWith('Key') ? code.slice(3).toLowerCase() : code;
          await s('Input.dispatchKeyEvent', { type: step.down === false ? 'keyUp' : 'rawKeyDown', code, key, windowsVirtualKeyCode: step.vk ?? 0 });
        } else if (step.video) {
          const dir = join(OUT, step.video);
          mkdirSync(dir, { recursive: true });
          frames.length = 0;
          await s('Page.startScreencast', { format: 'jpeg', quality: 80, maxWidth: step.max_width ?? 1280, everyNthFrame: step.nth ?? 2 });
          await sleep((step.seconds ?? 10) * 1000);
          await s('Page.stopScreencast');
          frames.forEach((f, k) => writeFileSync(join(dir, `f${String(k).padStart(5, '0')}.jpg`), Buffer.from(f, 'base64')));
          const fps = step.fps ?? Math.max(1, Math.round(frames.length / (step.seconds ?? 10)));
          const ff = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-framerate', String(fps), '-i', join(dir, 'f%05d.jpg'), '-c:v', 'libvpx-vp9', '-b:v', '1500k', '-pix_fmt', 'yuv420p', join(OUT, step.video + '.webm')], { stdio: ['ignore', 'ignore', 'inherit'] });
          results.push({ step: label, ok: true, frames: frames.length, fps, webm: ff.status === 0 });
          log(`video ${step.video}: ${frames.length} frames at ${fps} fps, webm ${ff.status === 0}`);
        }
      } catch (e) {
        results.push({ step: label, ok: false, error: e.message });
        log(`${label}: ${e.message}`);
        if (step.required) throw e;
      }
    }
    writeFileSync(join(OUT, 'pilot.json'), JSON.stringify({ port, game: GAME, results, console: consoleLines }, null, 2));
    process.stdout.write(JSON.stringify({ results, console: consoleLines }, null, 2) + '\n');
  } finally {
    if (cdp) {
      try {
        await cdp.send('Browser.close', {}, undefined, 5000);
      } catch {}
      cdp.ws.close();
    }
    for (let i = 0; i < 50 && !chromeExited; i++) await sleep(100);
    if (chrome && !chromeExited) spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    for (const p of started) {
      if (p.exitCode === null) spawnSync('taskkill', ['/PID', String(p.pid), '/T', '/F'], { stdio: 'ignore' });
      try {
        const f = ownSession(port);
        if (JSON.parse(readFileSync(f, 'utf8')).pid === p.pid) rmSync(f, { force: true });
      } catch {}
    }
    if (profile) try { rmSync(profile, { recursive: true, force: true }); } catch {}
  }
}

main().catch((e) => {
  console.error('[pilot] error:', e.message ?? e);
  process.exit(2);
});
