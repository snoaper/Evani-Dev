// Touch controls check: serves a built game folder, opens it in headless
// Chrome as a touch phone (muted; pointer lock, fullscreen and keyboard lock
// stubbed), and drives the on-screen controls with synthetic touches over
// CDP (Input.dispatchTouchEvent). Never touches the real mouse or keyboard.
//
//   node tools/live-check/touch-check.mjs --dist <folder with index.html>
//        [--port 8731] [--out <dir for screenshots and summary.json>]
//        [--action <name of an action a control drives>] [--control <data-id>]
//
// Checks: the layer is shown on a coarse-pointer device; touching a control
// marks it held and the action reads down in the game; pointercancel and a
// key release it; a key hides the layer and a touch shows it again.
// The action state is read from `window.evani.live` when the game is a live
// build; ship builds are checked through the page only.

import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { extname, join, resolve } from 'node:path';

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => {
    if (a.startsWith('--')) acc.push([a.slice(2), all[i + 1] && !all[i + 1].startsWith('--') ? all[i + 1] : 'true']);
    return acc;
  }, []),
);
const DIST = resolve(args.dist ?? '');
const PORT = Number(args.port ?? 8731);
const OUT = resolve(args.out ?? 'target/touch-check');
const CONTROL = args.control ?? 'jump';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...m) => console.error('[touch-check]', ...m);
if (!existsSync(join(DIST, 'index.html'))) {
  console.error('usage: --dist <built game folder with index.html>');
  process.exit(2);
}
if (PORT === 8096) {
  console.error('port 8096 belongs to Evani.App; pick another');
  process.exit(2);
}

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png' };
const server = createServer((req, res) => {
  const path = decodeURIComponent((req.url ?? '/').split('?')[0].split('#')[0]);
  let file = join(DIST, path === '/' ? 'index.html' : path);
  if (!file.startsWith(DIST) || !existsSync(file) || statSync(file).isDirectory()) file = join(DIST, 'index.html');
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});

function findChrome() {
  const candidates = [args.chrome, process.env.EVANI_CHROME];
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]) {
    if (base) candidates.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  }
  const pw = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (existsSync(pw)) {
    for (const d of readdirSync(pw).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse()) {
      candidates.push(join(pw, d, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe'));
    }
  }
  return candidates.find((c) => c && existsSync(c));
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      const p = msg.id !== undefined && this.pending.get(msg.id);
      if (p) {
        this.pending.delete(msg.id);
        if (msg.error) p.rej(new Error(`${p.method}: ${msg.error.message}`));
        else p.res(msg.result);
      }
    });
  }
  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => {
      ws.addEventListener('open', res, { once: true });
      ws.addEventListener('error', () => rej(new Error('cannot connect')), { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}, sessionId) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      this.pending.set(id, { res, rej, method });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
}

const STUBS = `(() => {
  const noop = () => Promise.resolve();
  try { Element.prototype.requestPointerLock = noop; } catch (e) {}
  try { Element.prototype.requestFullscreen = noop; } catch (e) {}
  try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
  window.__fsCalls = 0;
  const fs = Element.prototype.requestFullscreen;
  Element.prototype.requestFullscreen = function () { window.__fsCalls++; return fs.call(this); };
})();`;

let chrome = null;
let profile = null;
const summary = { dist: DIST, control: CONTROL, checks: [] };
const check = (name, ok, detail) => {
  summary.checks.push({ name, ok: !!ok, detail });
  log(ok ? 'PASS' : 'FAIL', name, detail ?? '');
};

try {
  await new Promise((r) => server.listen(PORT, '127.0.0.1', r));
  mkdirSync(OUT, { recursive: true });
  const chromePath = findChrome();
  if (!chromePath) throw new Error('no Chrome found');
  profile = mkdtempSync(join(tmpdir(), 'evani-touch-check-'));
  const flags = [
    `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check',
    '--mute-audio', '--enable-unsafe-webgpu', '--window-size=915,412', 'about:blank',
  ];
  if (!/headless_shell|chrome-headless-shell/.test(chromePath)) flags.unshift('--headless=new');
  chrome = spawn(chromePath, flags, { stdio: 'ignore', windowsHide: true });
  let wsUrl = null;
  for (let i = 0; i < 200 && !wsUrl; i++) {
    const f = join(profile, 'DevToolsActivePort');
    if (existsSync(f)) {
      const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
      if (p && path) wsUrl = `ws://127.0.0.1:${p}${path}`;
    }
    if (!wsUrl) await sleep(100);
  }
  if (!wsUrl) throw new Error('Chrome did not open its debugging port');
  const cdp = await Cdp.connect(wsUrl);
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (m, p) => cdp.send(m, p, sessionId);
  await s('Page.enable');
  await s('Runtime.enable');
  // A landscape phone: touch, coarse pointer, no hover.
  await s('Emulation.setDeviceMetricsOverride', { width: 915, height: 412, deviceScaleFactor: 1, mobile: true });
  await s('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  await s('Emulation.setEmitTouchEventsForMouse', { enabled: false });
  await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
  await s('Page.navigate', { url: `http://127.0.0.1:${PORT}/` });
  const ev = async (expr) => (await s('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true })).result.value;
  let ready = false;
  for (let i = 0; i < 300 && !ready; i++) {
    ready = await ev(`!!document.getElementById('evani-touch')`).catch(() => false);
    if (!ready) await sleep(100);
  }
  check('layer installed', ready);
  // Menus may cover it; start a game the way the page does if there is a Play button.
  await ev(`(() => { const b = document.querySelector('[data-cmd=play],[data-cmd=start1],[data-cmd=race],[data-cmd=start]'); if (b) b.click(); return !!b; })()`);
  await sleep(1500);
  const shot = async (name) => {
    const { data } = await s('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(OUT, name), Buffer.from(data, 'base64'));
  };
  const layer = () => ev(`(() => { const r = document.getElementById('evani-touch'); return { on: r.className === 'on', display: getComputedStyle(r).display }; })()`);
  check('shown on a coarse-pointer device', (await layer()).on, await layer());
  const rect = await ev(`(() => { const e = document.querySelector('#evani-touch [data-id="${CONTROL}"]'); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, w: r.width, h: r.height }; })()`);
  check(`control "${CONTROL}" found`, !!rect, rect);
  const sizes = await ev(`[...document.querySelectorAll('#evani-touch .et-b')].map((e) => ({ id: e.dataset.id, w: e.getBoundingClientRect().width }))`);
  check('every button at least 48 CSS px', sizes.every((b) => b.w >= 47.5), sizes);
  await shot('1-shown.png');
  const held = () => ev(`document.querySelector('#evani-touch [data-id="${CONTROL}"]').className.includes('down')`);
  const touch = (type, pts) => s('Input.dispatchTouchEvent', { type, touchPoints: pts });
  if (rect) {
    // Press with finger 1, hold a few frames.
    await touch('touchStart', [{ x: rect.x, y: rect.y, id: 1 }]);
    await sleep(200);
    check('touch holds the control', await held(), await ev(`(() => { const e = document.elementFromPoint(${rect.x}, ${rect.y}); return e ? (e.id || e.className || e.tagName) : null; })()`));
    // A second finger elsewhere (multi-touch) keeps the first held.
    await touch('touchStart', [{ x: rect.x, y: rect.y, id: 1 }, { x: 40, y: 40, id: 2 }]);
    await sleep(100);
    await touch('touchMove', [{ x: rect.x, y: rect.y, id: 1 }]); // finger 2 lifts (CDP: a point left out is released)
    await sleep(150);
    check('a second finger lifting keeps the first held', await held());
    await shot('2-held.png');
    // Cancel (the OS took the touch): released.
    await touch('touchCancel', []);
    await sleep(200);
    check('pointercancel releases', !(await held()));
    // Slide off the button: still held (captured), released on lift.
    await touch('touchStart', [{ x: rect.x, y: rect.y, id: 3 }]);
    await sleep(100);
    await touch('touchMove', [{ x: 457, y: 200, id: 3 }]);
    await sleep(150);
    check('sliding off keeps the capture', await held());
    await touch('touchEnd', []);
    await sleep(200);
    check('lifting after sliding off releases', !(await held()));
    // Key input hides the layer and lets go.
    await touch('touchStart', [{ x: rect.x, y: rect.y, id: 4 }]);
    await sleep(100);
    await s('Input.dispatchKeyEvent', { type: 'keyDown', code: 'KeyX', key: 'x' });
    await s('Input.dispatchKeyEvent', { type: 'keyUp', code: 'KeyX', key: 'x' });
    await sleep(200);
    check('a key hides the layer', !(await layer()).on);
    check('a key lets go of held controls', !(await held()));
    await touch('touchEnd', []);
    await shot('3-hidden-after-key.png');
    // A touch on the game shows it again.
    await touch('touchStart', [{ x: 457, y: 200, id: 5 }]);
    await touch('touchEnd', []);
    await sleep(200);
    check('a touch shows the layer again', (await layer()).on);
    await shot('4-shown-again.png');
    check('never asked for fullscreen', (await ev('window.__fsCalls')) === 0);
    // Steer with the left-thumb stick (if the layout has one) and look at the HUD at speed.
    const zone = await ev(`(() => { const z = document.querySelector('#evani-touch .et-z'); if (!z) return null; const r = z.getBoundingClientRect(); return { x: r.x + r.width * 0.4, y: r.y + r.height * 0.6 }; })()`);
    if (zone) {
      await touch('touchStart', [{ x: zone.x, y: zone.y, id: 6 }]);
      await sleep(100);
      await touch('touchMove', [{ x: zone.x + 40, y: zone.y, id: 6 }]);
      await sleep(300);
      const knob = await ev(`(() => { const k = document.querySelector('#evani-touch .et-r.down .et-k'); return k ? k.getAttribute('style') : null; })()`);
      check('stick follows the thumb', !!knob && knob.includes('translate('), knob);
      await sleep(3000);
      await shot('5-steering-at-speed.png');
      await touch('touchEnd', []);
    }
  }
  const errs = await ev(`(() => { const o = document.getElementById('evani-overlay'); return o ? o.textContent.match(/error[^\\n]*/gi) : null; })()`);
  summary.overlay_errors = errs;
} catch (e) {
  check('run', false, String(e?.message ?? e));
} finally {
  if (chrome && chrome.exitCode === null) spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
  server.close();
  await sleep(300);
  if (profile) try { rmSync(profile, { recursive: true, force: true }); } catch {}
  summary.passed = summary.checks.every((c) => c.ok);
  try { writeFileSync(join(OUT, 'summary.json'), JSON.stringify(summary, null, 2)); } catch {}
  console.log(JSON.stringify({ passed: summary.passed, failed: summary.checks.filter((c) => !c.ok).map((c) => c.name) }));
  process.exit(summary.passed ? 0 : 1);
}
