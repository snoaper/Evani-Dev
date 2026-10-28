// Live tuning helper for Puff's Sweet Grid (kit process step 7).
//
//   node apps/match-3/review/live-session.mjs hold [--port 8091] [--tier ultra] [--minutes 40]
//     starts (or reuses) this project's bridge on --port and keeps one headless
//     Chrome page on the live game until `stop` or the time runs out.
//   node apps/match-3/review/live-session.mjs cmd <command> '<params json>' [--note <why>] [--png <file>] [--port 8091]
//     sends one live command through the bridge (/live/v1/cmd) and prints the
//     result; any png_base64 in it is saved to --png (a, b for compare.ab).
//   node apps/match-3/review/live-session.mjs stop
//
// Headless only: pointer lock, fullscreen and keyboard lock are stubbed; the
// real mouse and keyboard are never used. Only processes it started are stopped.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../../..', import.meta.url)));
const [mode, ...rest] = process.argv.slice(2);
const args = {};
const pos = [];
for (let i = 0; i < rest.length; i++) {
  if (rest[i].startsWith('--')) args[rest[i].slice(2)] = rest[++i];
  else pos.push(rest[i]);
}
const PORT = Number(args.port ?? 8091);
const STOP = join(tmpdir(), `evani-m3-live-${PORT}.stop`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...m) => console.error('[live]', ...m);

async function status() {
  try {
    const r = await fetch(`http://127.0.0.1:${PORT}/live/status`, { signal: AbortSignal.timeout(2000) });
    return await r.json();
  } catch {
    return null;
  }
}

async function cmd(name, params, note) {
  const token = JSON.parse(readFileSync(join(ROOT, '.evani', 'live', 'session.json'), 'utf8')).token;
  const body = { type: 'cmd', v: 1, id: `m3-${Date.now()}`, cmd: name, params, ...(note ? { note, opts: { telemetry_ms: 0 } } : {}) };
  const r = await fetch(`http://127.0.0.1:${PORT}/live/v1/cmd`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(180000),
  });
  return r.json();
}

function savePngs(v, out) {
  let n = 0;
  const walk = (o, path) => {
    if (!o || typeof o !== 'object') return;
    for (const [k, val] of Object.entries(o)) {
      if (typeof val === 'string' && k.endsWith('png_base64')) {
        const f = n === 0 && k === 'png_base64' ? out : out.replace(/\.png$/, `-${path}${k.replace('_png_base64', '').replace('png_base64', '')}.png`);
        writeFileSync(f, Buffer.from(val, 'base64'));
        o[k] = `<saved ${f}>`;
        n++;
      } else walk(val, path + k + '.');
    }
  };
  walk(v, '');
}

async function hold() {
  const started = [];
  let chrome = null;
  let profile = null;
  rmSync(STOP, { force: true });
  try {
    let st = await status();
    if (!st) {
      const exe = join(ROOT, 'target', 'debug', 'evani-live-bridge.exe');
      if (!existsSync(exe)) {
        const b = spawnSync('cargo', ['build', '-j', '4', '-p', 'evani-live-bridge'], { cwd: ROOT, stdio: 'inherit' });
        if (b.status !== 0) throw new Error('bridge build failed');
      }
      const env = { ...process.env, CARGO_BUILD_JOBS: '4' };
      delete env.NO_COLOR;
      started.push(spawn(exe, ['serve', '--root', ROOT, '--port', String(PORT), '--game', 'match-3', '--quiet'], { cwd: ROOT, stdio: ['ignore', 'ignore', 'inherit'], windowsHide: true, env }));
      log('bridge started on', PORT);
    }
    for (const t0 = Date.now(); ; ) {
      st = await status();
      if (st?.phase === 'idle') break;
      if (st?.phase === 'error') throw new Error(st.error);
      if (Date.now() - t0 > 20 * 60000) throw new Error('build timeout');
      await sleep(1000);
    }
    const c = [process.env.EVANI_CHROME, join(process.env.ProgramFiles ?? '', 'Google', 'Chrome', 'Application', 'chrome.exe'), join(process.env.LOCALAPPDATA ?? '', 'Google', 'Chrome', 'Application', 'chrome.exe')].find((p) => p && existsSync(p));
    profile = mkdtempSync(join(tmpdir(), 'evani-m3-live-'));
    const tier = args.tier ?? 'ultra';
    // A tiny page that sets the saved tier, stubs the locks, then opens the game.
    chrome = spawn(c, ['--headless=new', '--mute-audio', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run',
      '--enable-unsafe-webgpu', '--enable-webgpu-developer-features', '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--window-size=1920,1080', 'about:blank'], { stdio: 'ignore', windowsHide: true });
    let ws = null;
    for (let i = 0; i < 200 && !ws; i++) {
      const f = join(profile, 'DevToolsActivePort');
      if (existsSync(f)) {
        const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
        if (p && path) ws = `ws://127.0.0.1:${p}${path}`;
      }
      if (!ws) await sleep(100);
    }
    const sock = new WebSocket(ws);
    await new Promise((r) => sock.addEventListener('open', r, { once: true }));
    let id = 0;
    const pending = new Map();
    sock.addEventListener('message', (e) => {
      const m = JSON.parse(e.data);
      if (m.id && pending.has(m.id)) {
        pending.get(m.id)(m);
        pending.delete(m.id);
      }
    });
    const send = (method, params = {}, sessionId) => new Promise((r) => {
      const i = ++id;
      pending.set(i, r);
      sock.send(JSON.stringify({ id: i, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
    const { result: { targetId } } = await send('Target.createTarget', { url: 'about:blank' });
    const { result: { sessionId } } = await send('Target.attachToTarget', { targetId, flatten: true });
    await send('Page.enable', {}, sessionId);
    await send('Emulation.setDeviceMetricsOverride', { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false }, sessionId);
    await send('Page.addScriptToEvaluateOnNewDocument', { source: `(() => { const noop = () => Promise.resolve();
      try { Element.prototype.requestPointerLock = noop; Element.prototype.requestFullscreen = noop; } catch (e) {}
      try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
      try { localStorage.setItem('evani:match-3:render.tier', ${JSON.stringify(tier)}); } catch (e) {} })();` }, sessionId);
    await send('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` }, sessionId);
    writeFileSync(STOP + '.ws', JSON.stringify({ ws, targetId }));
    log(`page open at tier ${tier}; waiting for stop (${STOP})`);
    const until = Date.now() + Number(args.minutes ?? 40) * 60000;
    while (Date.now() < until && !existsSync(STOP)) await sleep(500);
    try {
      await send('Browser.close');
    } catch {
      // gone
    }
  } finally {
    await sleep(500);
    if (chrome && chrome.exitCode === null) spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    for (const p of started) {
      if (p.exitCode === null) spawnSync('taskkill', ['/PID', String(p.pid), '/T', '/F'], { stdio: 'ignore' });
      try {
        const f = join(ROOT, '.evani', 'live', 'session.json');
        if (JSON.parse(readFileSync(f, 'utf8')).pid === p.pid) rmSync(f, { force: true });
      } catch {
        // none
      }
    }
    if (profile) rmSync(profile, { recursive: true, force: true });
    rmSync(STOP, { force: true });
    log('stopped');
  }
}

if (mode === 'hold') {
  hold().catch((e) => {
    console.error('[live] error:', e.message);
    process.exit(2);
  });
} else if (mode === 'zoom') {
  // zoom <x> <y> <w> <h> <scale> --png out: a clipped, scaled page screenshot.
  const { ws, targetId } = JSON.parse(readFileSync(STOP + '.ws', 'utf8'));
  const sock = new WebSocket(ws);
  await new Promise((r) => sock.addEventListener('open', r, { once: true }));
  let id = 0;
  const pending = new Map();
  sock.addEventListener('message', (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } });
  const send = (method, params = {}, sessionId) => new Promise((r) => { const i = ++id; pending.set(i, r); sock.send(JSON.stringify({ id: i, method, params, ...(sessionId ? { sessionId } : {}) })); });
  const { result: { sessionId } } = await send('Target.attachToTarget', { targetId, flatten: true });
  const [x, y, w, h, scale] = pos.map(Number);
  const r = await send('Page.captureScreenshot', { format: 'png', clip: { x, y, width: w, height: h, scale } }, sessionId);
  writeFileSync(resolve(args.png), Buffer.from(r.result.data, 'base64'));
  sock.close();
  console.log('saved', args.png);
} else if (mode === 'stop') {
  writeFileSync(STOP, '1');
} else if (mode === 'cmd') {
  const v = await cmd(pos[0], JSON.parse(pos[1] ?? '{}'), args.note);
  if (args.png) savePngs(v, resolve(args.png));
  process.stdout.write(JSON.stringify(v, null, 1).slice(0, Number(args.max ?? 6000)) + '\n');
} else {
  console.error('usage: hold | cmd <name> <json> [--png f] | stop');
  process.exit(1);
}
