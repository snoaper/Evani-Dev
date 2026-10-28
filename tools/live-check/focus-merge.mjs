// Merge check for asset focus on the full kart racer: starts a race, focuses the item box
// (two tunables changed, before/after), the asset-view commands the Evani.dev tab uses (orbit
// drag), the kart, then back to the race. Needs a running live server on --port (no default).
//   node tools/live-check/focus-merge.mjs --port <free port> [--out dir]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const args = {};
for (let i = 2; i < process.argv.length; i++) { const a = process.argv[i]; if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i]; }
const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const PORT = Number(args.port);
if (!PORT) { console.error("give --port"); process.exit(1); }
const OUT = resolve(args.out ?? join(ROOT, '.vibedeck', 'screenshots', 'focus-merge'));
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
  const profile = mkdtempSync(join(tmpdir(), 'evani-focus-merge-'));
  const chromePath = findChrome();
  const isShell = /headless_shell|chrome-headless-shell/.test(chromePath);
  const flags = [`--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--enable-unsafe-webgpu',
    '--enable-webgpu-developer-features', '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
    '--window-size=1280,720', 'about:blank'];
  if (!isShell) flags.unshift('--headless=new');
  const chrome = spawn(chromePath, flags, { stdio: 'ignore', windowsHide: true });
  const out = { steps: [] };
  const check = (name, ok, extra) => { out.steps.push({ name, ok: !!ok, ...(extra !== undefined ? { extra } : {}) }); log(ok ? 'ok  ' : 'FAIL', name, extra ?? ''); };
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
    const errors = [];
    cdp.on((msg) => {
      if (msg.method === 'Runtime.exceptionThrown') errors.push(msg.params.exceptionDetails?.exception?.description ?? msg.params.exceptionDetails?.text);
      if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') errors.push(msg.params.args.map((a) => a.value ?? a.description).join(' '));
    });
    await s('Page.enable'); await s('Runtime.enable');
    await s('Emulation.setDeviceMetricsOverride', { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
    await s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` });
    for (let i = 0; ; i++) {
      const st = await fetch(`http://127.0.0.1:${PORT}/live/status`).then((r) => r.json()).catch(() => null);
      if (st?.engine_connected) break;
      if (i > 240) throw new Error('page did not connect');
      await sleep(500);
    }
    await sleep(2500);
    const page = async (name) => { const r = await s('Page.captureScreenshot', { format: 'png' }); writeFileSync(join(OUT, name), Buffer.from(r.data, 'base64')); };
    const frame = async (name) => { const r = await call('capture.frame', {}); writeFileSync(join(OUT, name), Buffer.from(r.png_base64, 'base64')); };
    const ev = (expr) => s('Runtime.evaluate', { expression: expr, returnByValue: true }).then((r) => r.result.value);
    const focusState = async () => (await call('focus.list', {})).focus;

    // The full game: a one-player race, a few seconds in.
    await ev(`(document.querySelector('[data-cmd=start1]') || {click(){}}).click(), 1`);
    await sleep(9000);
    await page('01-race.png');
    const list = await call('focus.list', {});
    check('focus.list lists five assets', list.assets.length === 5, list.assets.map((a) => a.id).join(','));
    check('nothing focused at first', list.focus.active === null);

    // Item box: before, then two tunables, after.
    await call('focus.set', { asset: 'item_box' });
    await sleep(2500);
    const fs1 = await focusState();
    check('item box focus drew', fs1.active === 'item_box' && fs1.drawn === true, fs1.frame);
    const hidden = await ev(`document.documentElement.dataset.evaniFocus + '|' + getComputedStyle(document.getElementById('ui') || document.body).display`);
    check('page UI hidden while focused', String(hidden).startsWith('item_box|none'), hidden);
    await page('02-item-box-before.png');
    await frame('02-item-box-before-canvas.png');
    const sch = await call('schema.get', {});
    check('focused tunables come first', (sch.params ?? [])[0]?.name?.startsWith('kit.kart_racer.item_box_'), (sch.params ?? [])[0]?.name);
    await call('param.set', { values: { 'kit.kart_racer.item_box_size': 3.0 } }, 'merge check: bigger box');
    await call('param.set', { values: { 'kit.kart_racer.item_box_saturation': 0 } }, 'merge check: grey box');
    await sleep(1500);
    await page('03-item-box-after.png');
    await frame('03-item-box-after-canvas.png');
    const fs2 = await focusState();
    check('bigger box framed from further away', fs2.frame && fs1.frame && fs2.frame.distance > fs1.frame.distance * 1.3, { before: fs1.frame?.distance, after: fs2.frame?.distance });
    await call('param.set', { values: { 'kit.kart_racer.item_box_size': 1.8 } }, 'merge check: restore');
    await call('param.set', { values: { 'kit.kart_racer.item_box_saturation': 1.0 } }, 'merge check: restore');

    // The asset view the Evani.dev tab uses.
    const al = await call('asset.list', { limit: 1000 });
    check('asset.list', al.assets.length === 5 && al.focused === 'item_box' && al.assets.every((a) => a.path.startsWith('kits/')), al.focused);
    const af = await call('asset.focus', { id: 'kart', view: 'orbit' }, 'merge check');
    check('asset.focus kart orbit', af.id === 'kart' && af.view === 'orbit' && af.name === 'Kart with driver', af);
    await sleep(1500);
    const o0 = await focusState();
    check('orbit holds the camera still', o0.spin_deg_s === 0, o0.yaw_deg);
    // A drag and a wheel turn in the page (synthetic browser events).
    const mouse = (type, x, y, extra = {}) => s('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1, ...extra });
    await mouse('mouseMoved', 600, 360, { buttons: 0, button: 'none' });
    await mouse('mousePressed', 600, 360);
    for (let i = 1; i <= 10; i++) await mouse('mouseMoved', 600 + i * 15, 360 + i * 4);
    await mouse('mouseReleased', 750, 400);
    await s('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 640, y: 360, deltaX: 0, deltaY: -300 });
    await sleep(800);
    const o1 = await focusState();
    check('dragging turned the camera', Math.abs(o1.yaw_deg - o0.yaw_deg) > 20 && o1.pitch_deg !== o0.pitch_deg, { yaw: [o0.yaw_deg, o1.yaw_deg], pitch: [o0.pitch_deg, o1.pitch_deg] });
    check('wheel zoomed in', (o1.distance ?? 99) < (o0.frame?.distance ?? 0) * 0.9, { from: o0.frame?.distance, to: o1.distance });
    await page('04-kart-orbit.png');
    await call('asset.focus', { id: 'kart', view: 'turntable' }, 'merge check');
    await sleep(2000);
    const k = await focusState();
    check('kart turntable turns', k.active === 'kart' && k.spin_deg_s > 0 && k.drawn);
    await page('05-kart-turntable.png');
    await frame('05-kart-turntable-canvas.png');

    // Back to the race.
    const cleared = await call('focus.clear', {});
    check('focus.clear', cleared.active === null && cleared.was === 'kart', cleared);
    await sleep(2500);
    const after = await call('session.status', {});
    check('the page UI is back', await ev(`document.documentElement.dataset.evaniFocus === undefined`));
    check('session.status focus null', after.focus.active === null && after.focused === null);
    await page('06-race-again.png');
    await frame('06-race-again-canvas.png');
    check('no page errors', errors.length === 0, errors.slice(0, 5));
  } finally {
    spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    await sleep(500);
    try { rmSync(profile, { recursive: true, force: true }); } catch {}
  }
  console.log(JSON.stringify(out, null, 1).slice(0, 6000));
  if (out.steps.some((x) => !x.ok)) process.exit(1);
}
main().catch((e) => { console.error(e); process.exit(2); });
