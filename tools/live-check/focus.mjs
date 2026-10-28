// Focus-mode check: opens ?live=1&focus=item_box in headless Chrome, frames the
// asset, changes two settings, saves before/after and a focus.clear shot.
//   node tools/live-check/focus.mjs [--port 8104] [--asset item_box] [--out dir]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const args = {};
for (let i = 2; i < process.argv.length; i++) { const a = process.argv[i]; if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i]; }
const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const PORT = Number(args.port ?? 8104);
const ASSET = args.asset ?? 'item_box';
const OUT = resolve(args.out ?? join(ROOT, '.vibedeck', 'screenshots', 'itembox-live'));
const log = (...m) => console.error('[focus-check]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function findChrome() {
  const candidates = [args.chrome, process.env.EVANI_CHROME];
  const pf = [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA];
  for (const base of pf) {
    if (base) candidates.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  }
  for (const base of pf) {
    if (base) candidates.push(join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  }
  const pw = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (existsSync(pw)) {
    const dirs = readdirSync(pw).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse();
    for (const d of dirs) candidates.push(join(pw, d, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe'));
  }
  return candidates.find((c) => c && existsSync(c));
}

/** Minimal CDP client over one WebSocket (same as tools/bench-runner). */
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
      } else {
        for (const h of this.handlers) h(msg);
      }
    });
    ws.addEventListener('close', () => {
      for (const p of this.pending.values()) p.reject(new Error('browser connection closed'));
      this.pending.clear();
    });
  }
  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => {
      ws.addEventListener('open', res, { once: true });
      ws.addEventListener('error', () => rej(new Error('cannot connect to ' + url)), { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}, sessionId, timeoutMs = 60000) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        rej(new Error(`${method}: no answer from the browser in ${Math.round(timeoutMs / 1000)} s`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (v) => { clearTimeout(timer); res(v); },
        reject: (e) => { clearTimeout(timer); rej(e); },
        method,
      });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  on(handler) {
    this.handlers.push(handler);
  }
}

// The page must never grab the real pointer or keyboard in a test run.
const STUBS = `(() => {
  const noop = () => Promise.resolve();
  try { Element.prototype.requestPointerLock = noop; } catch (e) {}
  try { Element.prototype.requestFullscreen = noop; } catch (e) {}
  try { Document.prototype.exitPointerLock = function () {}; } catch (e) {}
  try { if (typeof Keyboard !== 'undefined') Keyboard.prototype.lock = noop; } catch (e) {}
  try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
})();`;
let cmdN = 0;
async function cmd(port, token, name, params, timeoutMs = 120000, note = undefined, opts = undefined) {
  const body = { type: 'cmd', v: 1, id: `lc-${++cmdN}`, cmd: name, params, ...(note ? { note } : {}), ...(opts ? { opts } : {}) };
  const r = await fetch(`http://127.0.0.1:${port}/live/v1/cmd`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(timeoutMs),
  });
  const v = await r.json();
  if (!v.ok) throw new Error(`${name}: ${v.error?.code}: ${v.error?.message}`);
  return v.result;
}
async function main() {
  mkdirSync(OUT, { recursive: true });
  const token = JSON.parse(readFileSync(join(ROOT, '.evani', 'live', 'session.json'), 'utf8')).token;
  const call = (n, p, note) => cmd(PORT, token, n, p, 120000, note);
  const profile = mkdtempSync(join(tmpdir(), 'evani-focus-check-'));
  const chromePath = findChrome();
  const isShell = /headless_shell|chrome-headless-shell/.test(chromePath);
  const flags = [`--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--enable-unsafe-webgpu',
    '--enable-webgpu-developer-features', '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
    '--window-size=1280,720', 'about:blank'];
  if (!isShell) flags.unshift('--headless=new');
  const chrome = spawn(chromePath, flags, { stdio: 'ignore', windowsHide: true });
  const summary = {};
  try {
    let wsUrl = null;
    for (let i = 0; i < 200 && !wsUrl; i++) {
      const f = join(profile, 'DevToolsActivePort');
      if (existsSync(f)) { const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/); if (p && path) wsUrl = `ws://127.0.0.1:${p}${path}`; }
      if (!wsUrl) await sleep(100);
    }
    const cdp = await Cdp.connect(wsUrl);
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    const s = (m, p, ms) => cdp.send(m, p, sessionId, ms);
    await s('Page.enable'); await s('Runtime.enable');
    await s('Emulation.setDeviceMetricsOverride', { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
    await s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1&focus=${ASSET}` });
    for (let i = 0; ; i++) {
      const st = await fetch(`http://127.0.0.1:${PORT}/live/status`).then((r) => r.json()).catch(() => null);
      if (st?.engine_connected) break;
      if (i > 120) throw new Error('page did not connect');
      await sleep(500);
    }
    await sleep(3000);
    const shot = async (name) => { const r = await call('capture.frame', {}); writeFileSync(join(OUT, name), Buffer.from(r.png_base64, 'base64')); return { camera: r.camera ?? null }; };
    summary.list = await call('focus.list', {});
    summary.status = (await call('session.status', {})).focus ?? null;
    summary.before = await shot('before.png');
    const sch = await call('schema.get', {});
    summary.firstParams = (sch.params ?? sch).slice?.(0, 4)?.map((p) => p.id ?? p.name);
    await call('param.set', { values: { 'kit.kart_racer.item_box_size': 3.0 } }, 'bigger box to compare');
    await call('param.set', { values: { 'kit.kart_racer.item_box_saturation': 0 } }, 'grey box to compare');
    await sleep(1000);
    summary.after = await shot('after.png');
    await call('param.set', { values: { 'kit.kart_racer.item_box_size': 1.0 } }, 'restore');
    await call('param.set', { values: { 'kit.kart_racer.item_box_saturation': 1.0 } }, 'restore');
    summary.clear = await call('focus.clear', {});
    await sleep(2000);
    summary.full = await shot('full-game.png');
  } finally {
    spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    await sleep(500);
    try { rmSync(profile, { recursive: true, force: true }); } catch {}
  }
  console.log(JSON.stringify(summary, null, 1).slice(0, 4000));
}
main().catch((e) => { console.error(e); process.exit(2); });
