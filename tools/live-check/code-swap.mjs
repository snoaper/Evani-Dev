// Evani.Dev live code swapping check (docs/LIVE_API.md 10.4): one headless
// Chrome on a game served by the live bridge (`serve`, live code swapping
// on), then a list of source edits made while the game keeps running. For
// each edit it records whether the new code was swapped in with no reload,
// how long that took from the save, and the game's state before and after
// (page identity, frame counter, HUD text, live params), plus screenshots.
//
//   node tools/live-check/code-swap.mjs --url http://127.0.0.1:8160/?live=1 \
//        --plan tools/live-check/code-swap-kart.json --out .vibedeck/screenshots/live-code/kart
//
// Plan: { "params": ["name", ...],          // read before/after every trial
//         "setup": [ {"click": "css"}, {"wait_ms": 4000}, {"param": {"name": value}} ],
//         "trials": [ {"name", "file", "from", "to", "expect": "patched" | "error",
//                      "fix": true, "timeout_s": 300} ] }
// Every edit is an exact string replace; `fix` (default true) puts the
// original back as a second trial (for an `error` trial that is the fix,
// which must then patch). Every edited file is restored on exit, even on a
// failure. Pointer lock, fullscreen and keyboard lock are stubbed; Chrome
// runs muted; the real mouse and keyboard are never used.
//
// Grew out of the spike's tools/spike-subsecond/harness.mjs + patch-trial.mjs
// (branch spike-subsecond); CDP and launch flags as tools/live-check/pilot.mjs.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = {};
for (let i = 2; i < process.argv.length; i++) if (process.argv[i].startsWith('--')) args[process.argv[i].slice(2)] = process.argv[++i];
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const URL_ = args.url ?? 'http://127.0.0.1:8160/?live=1';
const OUT = resolve(args.out ?? join(ROOT, '.vibedeck', 'screenshots', 'live-code'));
const PLAN = JSON.parse(readFileSync(resolve(args.plan), 'utf8'));
const W = Number(args.width ?? 960), H = Number(args.height ?? 540);
const ONLY = args.only ? new Set(args.only.split(',')) : null;
mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...m) => console.log('[code-swap]', ...m);

const STUBS = `(() => {
  const noop = () => Promise.resolve();
  try { Element.prototype.requestPointerLock = noop; } catch (e) {}
  try { Element.prototype.requestFullscreen = noop; } catch (e) {}
  try { Document.prototype.exitPointerLock = function () {}; } catch (e) {}
  try { if (typeof Keyboard !== 'undefined') Keyboard.prototype.lock = noop; } catch (e) {}
  try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
  window.__rafs = 0;
  const tick = () => { window.__rafs++; requestAnimationFrame(tick); };
  requestAnimationFrame(tick);
})();`;

class Cdp {
  constructor(ws) {
    this.ws = ws; this.id = 0; this.pending = new Map(); this.handlers = [];
    ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id !== undefined) {
        const p = this.pending.get(msg.id);
        if (p) { this.pending.delete(msg.id); msg.error ? p.reject(new Error(msg.error.message)) : p.resolve(msg.result); }
      } else for (const h of this.handlers) h(msg);
    });
  }
  send(method, params = {}, sessionId, ms = 60000) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      const t = setTimeout(() => (this.pending.delete(id), rej(new Error(method + ': timeout'))), ms);
      this.pending.set(id, { resolve: (v) => (clearTimeout(t), res(v)), reject: (e) => (clearTimeout(t), rej(e)) });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
}

// ---- edits, always restored -------------------------------------------------------
const originals = new Map();
function edit(file, from, to) {
  const path = resolve(ROOT, file);
  const src = readFileSync(path, 'utf8');
  if (!originals.has(path)) originals.set(path, src);
  if (!src.includes(from)) throw new Error(`edit target not found in ${file}: ${from}`);
  writeFileSync(path, src.replace(from, to));
}
function restoreAll() {
  for (const [path, src] of originals) {
    try { if (readFileSync(path, 'utf8') !== src) { writeFileSync(path, src); log('restored', path); } } catch (e) { console.error('cannot restore', path, e); }
  }
}
process.on('exit', restoreAll);
process.on('SIGINT', () => process.exit(130));

// ---- Chrome ---------------------------------------------------------------------------
const chromePath = [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]
  .filter(Boolean).map((b) => join(b, 'Google', 'Chrome', 'Application', 'chrome.exe')).find(existsSync);
const profile = mkdtempSync(join(tmpdir(), 'evani-code-swap-'));
const chrome = spawn(chromePath, [
  '--headless=new', '--mute-audio', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run',
  '--no-default-browser-check', '--enable-unsafe-webgpu', '--enable-webgpu-developer-features',
  '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
  '--disable-backgrounding-occluded-windows', `--window-size=${W},${H}`, 'about:blank',
], { stdio: 'ignore', windowsHide: true });
function closeChrome() { try { chrome.kill(); } catch {} try { rmSync(profile, { recursive: true, force: true }); } catch {} }
process.on('exit', closeChrome);

let wsUrl = null;
for (let i = 0; i < 200 && !wsUrl; i++) {
  const f = join(profile, 'DevToolsActivePort');
  if (existsSync(f)) { const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/); if (p && path) wsUrl = `ws://127.0.0.1:${p}${path}`; }
  if (!wsUrl) await sleep(100);
}
if (!wsUrl) { console.error('Chrome did not start'); process.exit(2); }
const ws = new WebSocket(wsUrl);
await new Promise((r) => ws.addEventListener('open', r, { once: true }));
const cdp = new Cdp(ws);
const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
const s = (m, p, ms) => cdp.send(m, p, sessionId, ms);
const consoleLines = [];
let navigations = 0;
cdp.handlers.push((m) => {
  if (m.sessionId !== sessionId) return;
  const t = new Date().toISOString().slice(11, 23);
  if (m.method === 'Runtime.exceptionThrown') consoleLines.push(`${t} exception: ${m.params.exceptionDetails?.exception?.description ?? m.params.exceptionDetails?.text}`);
  if (m.method === 'Runtime.consoleAPICalled') consoleLines.push(`${t} ${m.params.type}: ${m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 600)}`);
  if (m.method === 'Page.frameNavigated' && !m.params.frame.parentId) { navigations++; consoleLines.push(`${t} NAVIGATED ${m.params.frame.url}`); }
});
await s('Page.enable'); await s('Runtime.enable');
await s('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
await s('Page.navigate', { url: URL_ });
log('page opened', URL_);

async function ev(expr) {
  const r = await s('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }, 120000);
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
}
async function shot(name) {
  const r = await s('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(OUT, name), Buffer.from(r.data, 'base64'));
  return name;
}

const PARAMS = JSON.stringify(PLAN.params ?? []);
const SNAP = `(async () => {
  const c = window.__evani_code || {};
  let params = null;
  try { params = ${PARAMS}.length ? (await window.evani.live.call('param.get', { names: ${PARAMS} })).values : null; } catch (e) { params = String(e); }
  const t = document.getElementById('evani-code-toast');
  return {
    timeOrigin: performance.timeOrigin, now: Math.round(performance.now()), rafs: window.__rafs,
    engine: window.__engine ? { started: window.__engine.started, error: window.__engine.error } : null,
    code: { hot: !!c.hot, applied: c.applied || 0, failed: c.failed || 0, active: c.active || 0, last: c.last || null },
    toast: t && t.dataset.shown === '1' ? { kind: t.dataset.kind, text: t.textContent.slice(0, 600) } : null,
    hud: (document.getElementById('hud0')?.innerText || '').replace(/\\s+/g, ' ').trim().slice(0, 200) || null,
    params,
  };
})()`;

const results = [];
let failed = false;
try {
  // 1. The game is up and takes code patches.
  const t0 = Date.now();
  let ready = null;
  while (Date.now() - t0 < 240000) {
    try { ready = await ev(`({ started: !!(window.__engine && window.__engine.started), hot: !!(window.__evani_code && window.__evani_code.hot) })`); } catch { ready = null; }
    if (ready?.started && ready?.hot) break;
    await sleep(500);
  }
  if (!ready?.started) throw new Error('the game did not start: ' + JSON.stringify(ready));
  if (!ready?.hot) throw new Error('the page does not take code patches (no window.__evani_code.hot)');
  log('game running, code patches on');
  // 2. Setup steps.
  for (const step of PLAN.setup ?? []) {
    if (step.click) await ev(`(() => { const b = document.querySelector(${JSON.stringify(step.click)}); if (!b) throw new Error('no ' + ${JSON.stringify(step.click)}); b.click(); return true; })()`);
    if (step.wait_ms) await sleep(step.wait_ms);
    if (step.param) {
      const r = await ev(`window.evani.live.call('param.set', { values: ${JSON.stringify(step.param)} }, { note: 'code-swap check: a live value that must survive every code swap', telemetry_ms: 0 }).then((r) => r, (e) => ({ error: String(e) }))`);
      log('param set', JSON.stringify(step.param), r?.error ?? 'ok');
    }
    if (step.key) {
      for (const type of ['keyDown', 'keyUp']) await s('Input.dispatchKeyEvent', { type, code: step.key, key: step.key });
    }
  }
  await shot('setup.png');

  // 3. Trials.
  const trials = [];
  for (const t of PLAN.trials) {
    if (ONLY && !ONLY.has(t.name)) continue;
    trials.push(t);
    if (t.fix !== false) trials.push({ ...t, name: `${t.name}-fix`, from: t.to, to: t.from, expect: 'patched', fixes: t.name });
  }
  for (const t of trials) {
    log(`trial ${t.name}: ${t.file}`);
    const before = await ev(SNAP);
    await shot(`${t.name}-before.png`);
    const save = Date.now();
    edit(t.file, t.from, t.to);
    const deadline = save + 1000 * (t.timeout_s ?? 300);
    let outcome = 'timeout', at = null, seen = null;
    while (Date.now() < deadline) {
      await sleep(100);
      let a;
      try { a = await ev(`({ o: performance.timeOrigin, c: window.__evani_code || {} })`); } catch { continue; }
      if (a.o !== before.timeOrigin) { outcome = 'reloaded'; at = Date.now(); break; }
      if ((a.c.applied || 0) > before.code.applied) { outcome = 'patched'; at = Date.now(); seen = a.c.last; break; }
      if ((a.c.failed || 0) > before.code.failed) { outcome = 'failed'; at = Date.now(); seen = a.c.last; break; }
      if (a.c.last && a.c.last.state === 'error' && JSON.stringify(a.c.last) !== JSON.stringify(before.code.last)) { outcome = 'error'; at = Date.now(); seen = a.c.last; break; }
    }
    // Let the game run a little on the new (or kept) code.
    await sleep(t.settle_ms ?? 2500);
    const after = await ev(SNAP);
    await shot(`${t.name}-after.png`);
    const r = {
      name: t.name, file: t.file, from: t.from, to: t.to, expect: t.expect, outcome,
      ok: outcome === t.expect && after.timeOrigin === before.timeOrigin,
      save_to_outcome_ms: at ? at - save : null,
      page_reported_ms: seen?.ms ?? null,
      same_page: after.timeOrigin === before.timeOrigin,
      frames_advanced: (after.rafs ?? 0) - (before.rafs ?? 0),
      params_kept: JSON.stringify(after.params) === JSON.stringify(before.params),
      hud_before: before.hud, hud_after: after.hud,
      toast_after: after.toast,
      before, after,
      shots: [`${t.name}-before.png`, `${t.name}-after.png`],
    };
    results.push(r);
    writeFileSync(join(OUT, `${t.name}.json`), JSON.stringify(r, null, 2));
    log(`  ${r.ok ? 'OK' : 'NOT OK'}: ${outcome} in ${r.save_to_outcome_ms} ms, same page ${r.same_page}, frames +${r.frames_advanced}, params kept ${r.params_kept}`);
    if (!r.ok) failed = true;
    if (outcome === 'reloaded' || outcome === 'timeout') break;
  }
} catch (e) {
  failed = true;
  console.error('[code-swap] stopped:', e.message);
  results.push({ error: e.message });
} finally {
  restoreAll();
  let status = null;
  try { status = await (await fetch(new URL('/live/status', URL_))).json(); } catch {}
  const summary = {
    url: URL_, when: new Date().toISOString(), ok: !failed, navigations,
    trials: results.map((r) => r.error ? r : ({ name: r.name, expect: r.expect, outcome: r.outcome, ok: r.ok, save_to_outcome_ms: r.save_to_outcome_ms,
      same_page: r.same_page, frames_advanced: r.frames_advanced, params_kept: r.params_kept, hud_before: r.hud_before, hud_after: r.hud_after,
      toast: r.toast_after?.text?.split('\n')[0] ?? null })),
    bridge_code_status: status?.code ?? null,
    console: consoleLines.slice(-60),
  };
  writeFileSync(join(OUT, 'summary.json'), JSON.stringify(summary, null, 2));
  log('summary', join(OUT, 'summary.json'), summary.ok ? 'ALL OK' : 'NOT ALL OK');
  try { await cdp.send('Browser.close'); } catch {}
  await sleep(500);
  closeChrome();
  process.exit(failed ? 1 : 0);
}
