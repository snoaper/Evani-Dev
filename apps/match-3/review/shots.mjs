// Puff's Sweet Grid review shots: plays the live build in headless Chrome
// through synthetic mouse or touch events (never the real ones) and saves
// full-page screenshots (canvas plus DOM HUD) of the kit's signature
// moments, named as in kits/match-3/target/moments.md: title, a match
// popping, a cascade with its callout, a Pop Bomb and a Prism going off,
// Sugar Rush, the win screen and the lose screen (plus the level intro), on a
// 1080p desktop and on a phone.
//
//   node apps/match-3/review/shots.mjs [--port 8091] [--out .vibedeck/review/match-3/moments] [--only fit]
//
// It reuses this project's live bridge on --port if one serves match-3,
// else starts one (which builds the game with `live`) and stops it after.
// Each run forces its quality tier through the saved tier key before the
// page loads; shots.json records the tier, viewport and input of every shot.
// Pointer lock, fullscreen and keyboard lock are stubbed. Only the processes
// this script started are stopped.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i];
}
const ROOT = resolve(fileURLToPath(new URL('../../..', import.meta.url)));
const OUT = resolve(ROOT, args.out ?? '.vibedeck/review/match-3/moments');
const PORT = Number(args.port ?? 8091);
const GAME = 'match-3';
const log = (...m) => console.error('[shots]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const norm = (p) => resolve(p).replace(/\\/g, '/').replace(/\/$/, '').toLowerCase();

function findChrome() {
  const c = [args.chrome, process.env.EVANI_CHROME];
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]) {
    if (base) c.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  }
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)']]) {
    if (base) c.push(join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  }
  return c.find((p) => p && existsSync(p));
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id === undefined) return;
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      if (msg.error) p.rej(new Error(`${p.method}: ${msg.error.message}`));
      else p.res(msg.result);
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
  send(method, params = {}, sessionId, ms = 60000) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      const t = setTimeout(() => {
        this.pending.delete(id);
        rej(new Error(`${method}: timeout`));
      }, ms);
      this.pending.set(id, { res: (v) => { clearTimeout(t); res(v); }, rej: (e) => { clearTimeout(t); rej(e); }, method });
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

/** One page of a run: viewport, input, tier. */
async function openPage(cdp, run) {
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (m, p, ms) => cdp.send(m, p, sessionId, ms);
  await s('Page.enable');
  await s('Runtime.enable');
  await s('Emulation.setDeviceMetricsOverride', { width: run.w, height: run.h, deviceScaleFactor: run.dpr, mobile: run.touch });
  if (run.touch) await s('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
  // The saved tier key (SaveData prefix evani:<game>:) and a fresh save.
  await s('Page.addScriptToEvaluateOnNewDocument', {
    source: `try { if (!sessionStorage.getItem('shots')) { localStorage.clear(); sessionStorage.setItem('shots', '1'); }
      localStorage.setItem('evani:${GAME}:render.tier', ${JSON.stringify(run.tier)}); } catch (e) {}`,
  });
  await s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` });
  return { s, targetId };
}

async function attr(s, name) {
  const r = await s('Runtime.evaluate', { expression: `document.getElementById('ui')?.getAttribute(${JSON.stringify(name)}) ?? ''`, returnByValue: true });
  return r.result.value ?? '';
}

async function evalv(s, expr) {
  const r = await s('Runtime.evaluate', { expression: expr, returnByValue: true });
  return r.result.value;
}

async function waitFor(s, pred, ms, what) {
  const t0 = Date.now();
  for (;;) {
    if (await pred()) return;
    if (Date.now() - t0 > ms) throw new Error('timed out waiting for ' + what);
    await sleep(40);
  }
}

async function shot(s, run, name, meta, shots) {
  const r = await s('Page.captureScreenshot', { format: 'png' }, 30000);
  const file = join(OUT, `${run.id}-${name}.png`);
  writeFileSync(file, Buffer.from(r.data, 'base64'));
  const tier = (await evalv(s, `localStorage.getItem('evani:${GAME}:render.tier') ?? '?'`)) || '?';
  // The HUD panel's fit: its height, the bottom of its last shown child
  // (CSS px from the panel's top; with the rows spread out this is the
  // panel's inner bottom), whether anything spills out, and (in
  // landscape) `need`: the height its shown children take stacked with
  // their gaps, padding and borders, ignoring the spare room the panel
  // spreads between its rows; `spare` is the panel's height minus `need`
  // (the content fits when it is 0 or more).
  const hud = await evalv(s, `(() => { const b = document.getElementById('hud-body'); if (!b || !b.classList.contains('on')) return null;
    const r = b.getBoundingClientRect(); let bottom = 0, sum = 0, n = 0;
    for (const c of b.children) { const cr = c.getBoundingClientRect(); if (cr.height > 0) { bottom = Math.max(bottom, cr.bottom - r.top); sum += cr.height; n++; } }
    const st = getComputedStyle(b), px = (v) => parseFloat(v) || 0;
    const need = b.classList.contains('portrait') ? null : Math.round(sum + Math.max(0, n - 1) * px(st.rowGap) + px(st.paddingTop) + px(st.paddingBottom) + px(st.borderTopWidth) + px(st.borderBottomWidth));
    return { height: Math.round(r.height), content: Math.round(bottom), need, spare: need === null ? null : Math.round(r.height) - need, unit: px(getComputedStyle(document.getElementById('hud')).getPropertyValue('--u')), spill: b.scrollHeight > b.clientHeight + 1 }; })()`);
  shots.push({ file, run: run.id, moment: name, tier, viewport: `${run.w}x${run.h}@${run.dpr}x`, input: run.touch ? 'touch' : 'mouse', hud, ...meta });
  log(`${run.id}: ${name} (${tier}) -> ${file}`);
}

/** The pause button's tap target: every point of a 48 x 48 CSS px square
 *  centred on the drawn button (inside the page) must hit the button
 *  (document.elementFromPoint). Records the drawn size; throws if it misses. */
async function pauseHit(s, run, shots) {
  const r = await evalv(s, `(() => { const b = [...document.querySelectorAll('[data-cmd="pause"]')].find(e => e.offsetParent !== null);
    if (!b) return null; const q = b.getBoundingClientRect(); const cx = q.x + q.width / 2, cy = q.y + q.height / 2;
    let n = 0, miss = 0; const hits = {}; for (let y = -24; y <= 24; y += 4) for (let x = -24; x <= 24; x += 4) {
      const px = Math.min(Math.max(cx + x, 0), innerWidth - 1), py = Math.min(Math.max(cy + y, 0), innerHeight - 1);
      n++; const e = document.elementFromPoint(px, py); if (!e || e.closest('[data-cmd="pause"]') !== b) {
        miss++; const k = e ? e.tagName.toLowerCase() + (e.id ? '#' + e.id : '') + (e.className ? '.' + String(e.className).split(' ').join('.') : '') : 'none';
        (hits[k] ??= []).push([x, y]); } }
    return { drawn: [Math.round(q.width), Math.round(q.height)], at: [Math.round(q.x), Math.round(q.y)], points: n, miss, missed: hits }; })()`);
  if (!r) throw new Error(run.id + ': no visible pause button');
  shots.push({ run: run.id, probe: 'pause hit 48x48', viewport: `${run.w}x${run.h}`, ...r });
  log(`${run.id}: pause button drawn ${r.drawn.join('x')} px, 48x48 hit area: ${r.points - r.miss}/${r.points} points`);
  if (r.miss > 0) throw new Error(`${run.id}: ${r.miss} of ${r.points} points of the 48 x 48 pause target miss the button: ${JSON.stringify(r.missed)}`);
}

/** Press on a, move to b, release: a drag (mouse) or a swipe (touch). */
async function drag(s, run, a, b) {
  const steps = 6;
  if (run.touch) {
    await s('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: a[0], y: a[1], id: 1 }] });
    for (let i = 1; i <= steps; i++) {
      const k = i / steps;
      await s('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: a[0] + (b[0] - a[0]) * k, y: a[1] + (b[1] - a[1]) * k, id: 1 }] });
      await sleep(12);
    }
    await s('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  } else {
    await s('Input.dispatchMouseEvent', { type: 'mouseMoved', x: a[0], y: a[1] });
    await s('Input.dispatchMouseEvent', { type: 'mousePressed', x: a[0], y: a[1], button: 'left', buttons: 1, clickCount: 1 });
    for (let i = 1; i <= steps; i++) {
      const k = i / steps;
      await s('Input.dispatchMouseEvent', { type: 'mouseMoved', x: a[0] + (b[0] - a[0]) * k, y: a[1] + (b[1] - a[1]) * k, button: 'left', buttons: 1 });
      await sleep(12);
    }
    await s('Input.dispatchMouseEvent', { type: 'mouseReleased', x: b[0], y: b[1], button: 'left', buttons: 0, clickCount: 1 });
  }
}

/** Tap a page button by its data-cmd, through the run's pointer. */
async function tapButton(s, run, cmd) {
  const r = await evalv(s, `(() => { const b = [...document.querySelectorAll('[data-cmd="${cmd}"]')].find(e => e.offsetParent !== null);
    if (!b) return null; const q = b.getBoundingClientRect(); return [q.x + q.width / 2, q.y + q.height / 2]; })()`);
  if (!r) throw new Error('no visible button ' + cmd);
  if (run.touch) {
    await s('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: r[0], y: r[1], id: 2 }] });
    await sleep(40);
    await s('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  } else {
    await s('Input.dispatchMouseEvent', { type: 'mouseMoved', x: r[0], y: r[1] });
    await s('Input.dispatchMouseEvent', { type: 'mousePressed', x: r[0], y: r[1], button: 'left', buttons: 1, clickCount: 1 });
    await s('Input.dispatchMouseEvent', { type: 'mouseReleased', x: r[0], y: r[1], button: 'left', buttons: 0, clickCount: 1 });
  }
}

/** Play a level with the best (or weakest) moves; shoot the moments. */
async function play(s, run, shots, { pick = 'data-best', want = [] } = {}) {
  const got = new Set();
  let moves = 0;
  for (let guard = 0; guard < 80; guard++) {
    const phase = await attr(s, 'data-phase');
    if (phase === 'won' || phase === 'lost') return phase;
    // Sugar Rush: the left-over moves fire as Streaks after a win.
    if (phase === 'rush' && want.includes('sugar_rush') && !got.has('sugar_rush') && (Number(await attr(s, 'data-blast')) || 0) & 1) {
      got.add('sugar_rush');
      await shot(s, run, 'sugar_rush', { move: moves }, shots);
    }
    if (phase !== 'play' || (await attr(s, 'data-idle')) !== '1') {
      await sleep(60);
      guard--;
      continue;
    }
    const m = (await attr(s, pick)).split(',').map(Number);
    if (m.length !== 4 || m.some((v) => !Number.isFinite(v))) {
      await sleep(60);
      continue;
    }
    await drag(s, run, [m[0], m[1]], [m[2], m[3]]);
    moves++;
    // Watch the move resolve; shoot each moment once.
    const t0 = Date.now();
    let shotMatch = false;
    while (Date.now() - t0 < 3500) {
      const blast = Number(await attr(s, 'data-blast')) || 0;
      const cascade = Number(await attr(s, 'data-cascade')) || 0;
      const callout = await evalv(s, `!!document.querySelector('.float.on.callout')`);
      if (want.includes('prism') && blast & 4 && !got.has('prism')) {
        got.add('prism');
        await shot(s, run, 'prism', { move: moves }, shots);
      } else if (want.includes('bomb') && blast & 2 && !got.has('bomb')) {
        got.add('bomb');
        await shot(s, run, 'bomb', { move: moves }, shots);
      } else if (want.includes('cascade') && cascade >= 2 && callout && !got.has('cascade')) {
        got.add('cascade');
        await shot(s, run, 'cascade', { move: moves, cascade }, shots);
      } else if (want.includes('match') && !shotMatch && !got.has('match') && Date.now() - t0 > 210) {
        shotMatch = true;
        got.add('match');
        await shot(s, run, 'match', { move: moves }, shots);
      }
      if ((await attr(s, 'data-idle')) === '1' && Date.now() - t0 > 400) break;
      const ph = await attr(s, 'data-phase');
      if (ph !== 'play') break;
      await sleep(30);
    }
  }
  throw new Error('the level did not end in 80 moves');
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  const started = [];
  let chrome = null;
  let chromeExited = true;
  let cdp = null;
  let profile = null;
  const shots = [];
  try {
    let st = await status(PORT);
    if (st && !(st.type === 'live_hello' && norm(st.root ?? '') === norm(ROOT) && st.game === GAME)) {
      throw new Error(`port ${PORT} is used by something else`);
    }
    if (!st) {
      // Build jobs: CARGO_BUILD_JOBS when set (a shared PC), else 4.
      const jobs = process.env.CARGO_BUILD_JOBS || '4';
      log(`building the bridge (cargo build -j ${jobs})...`);
      const b = spawnSync('cargo', ['build', '-j', jobs, '-p', 'evani-live-bridge'], { cwd: ROOT, stdio: ['ignore', 'ignore', 'inherit'] });
      if (b.status !== 0) throw new Error('the bridge did not build');
      const exe = join(ROOT, 'target', 'debug', process.platform === 'win32' ? 'evani-live-bridge.exe' : 'evani-live-bridge');
      const env = { ...process.env, CARGO_BUILD_JOBS: jobs };
      delete env.NO_COLOR;
      log(`starting the bridge on ${PORT} (it builds ${GAME} with live)...`);
      started.push(spawn(exe, ['serve', '--root', ROOT, '--port', String(PORT), '--game', GAME, '--quiet'], { cwd: ROOT, stdio: ['ignore', 'ignore', 'inherit'], windowsHide: true, env }));
    } else {
      log(`reusing this project's bridge on ${PORT}`);
    }
    const t0 = Date.now();
    for (;;) {
      st = await status(PORT);
      if (st?.phase === 'idle') break;
      if (st?.phase === 'error') throw new Error('the game did not build:\n' + st.error);
      if (Date.now() - t0 > 20 * 60000) throw new Error('the game build took over 20 minutes');
      await sleep(1000);
    }
    const chromePath = findChrome();
    if (!chromePath) throw new Error('no Chrome found (set EVANI_CHROME or pass --chrome)');
    profile = mkdtempSync(join(tmpdir(), 'evani-m3-shots-'));
    chrome = spawn(chromePath, [
      '--headless=new', '--mute-audio', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check',
      '--enable-unsafe-webgpu', '--enable-webgpu-developer-features', '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--window-size=1920,1080', 'about:blank',
    ], { stdio: 'ignore', windowsHide: true });
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
    if (!wsUrl) throw new Error('Chrome did not open its debugging port');
    cdp = await Cdp.connect(wsUrl);

    const runs = [
      // Desktop 1080p, mouse, Ultra: the signature moments and the win.
      { id: 'desk-ultra', w: 1920, h: 1080, dpr: 1, touch: false, tier: 'ultra', level: 0, pick: 'data-best', want: ['match', 'cascade', 'bomb', 'prism', 'sugar_rush'], title: true },
      // Phone portrait, touch, Low: the HUD on a phone, swipes, and a loss.
      { id: 'phone-low', w: 390, h: 844, dpr: 3, touch: true, tier: 'low', level: 0, pick: 'data-worst', want: ['match'], title: true },
      // Desktop 1080p, mouse, Medium: level 2 (frosting) to the end.
      { id: 'desk-medium', w: 1920, h: 1080, dpr: 1, touch: false, tier: 'medium', level: 1, pick: 'data-best', want: ['cascade', 'bomb', 'prism', 'sugar_rush'] },
    ];
    // `--only fit` skips the play runs and only measures the HUD fit.
    // `--run <id>` plays only that run (and skips the fit pass).
    for (const run of args.only === 'fit' ? [] : runs.filter((r) => !args.run || r.id === args.run)) {
      const { s, targetId } = await openPage(cdp, run);
      await waitFor(s, async () => (await attr(s, 'data-phase')) === 'title', 60000, 'the title screen');
      await sleep(2500);
      if (run.title) await shot(s, run, 'title', {}, shots);
      if (run.level === 0) {
        await tapButton(s, run, 'play');
      } else {
        await tapButton(s, run, 'levels');
        await sleep(300);
        await tapButton(s, run, `level:${run.level}`);
      }
      await waitFor(s, async () => (await attr(s, 'data-phase')) === 'intro', 10000, 'the level intro');
      await sleep(700);
      await shot(s, run, 'intro', {}, shots);
      if (run.touch) {
        await waitFor(s, async () => (await attr(s, 'data-phase')) === 'play', 10000, 'play');
        await pauseHit(s, run, shots);
      }
      const end = await play(s, run, shots, { pick: run.pick, want: run.want });
      await waitFor(s, async () => !!(await evalv(s, `document.getElementById('result')?.classList.contains('on')`)), 15000, 'the result screen');
      await sleep(700);
      await shot(s, run, end === 'won' ? 'win' : 'lose', {}, shots);
      const errors = await evalv(s, `(document.querySelector('#evani-overlay')?.textContent ?? '').slice(0, 300)`);
      shots.push({ run: run.id, end, overlay: errors });
      try {
        await cdp.send('Target.closeTarget', { targetId });
      } catch {
        // gone
      }
    }
    // The HUD panel's fit on landscape screens, big to small: level 3 (the
    // widest goal chips) with the banner showing ("Few moves left!" is put
    // in by the probe, as the game shows it near the end). Each shot's `hud`
    // records the panel height, the content's bottom and any spill.
    for (const [w, h] of args.run ? [] : [[1920, 1080], [1366, 768], [1280, 720], [844, 390], [740, 360], [667, 375]]) {
      const run = { id: `fit-${w}x${h}`, w, h, dpr: 1, touch: false, tier: 'low' };
      const { s, targetId } = await openPage(cdp, run);
      await waitFor(s, async () => (await attr(s, 'data-phase')) === 'title', 60000, 'the title screen');
      // The title menu and its keys hint keep clear of Puff.
      await sleep(1500);
      await shot(s, run, 'title', {}, shots);
      await tapButton(s, run, 'levels');
      await sleep(300);
      await tapButton(s, run, 'level:2');
      await waitFor(s, async () => (await attr(s, 'data-phase')) === 'play', 15000, 'play');
      await sleep(500);
      await evalv(s, `(() => { const b = document.getElementById('banner'); b.textContent = 'Few moves left!'; b.className = 'banner ink warn on'; })()`);
      await sleep(200);
      await shot(s, run, 'hud', { probe: 'banner on' }, shots);
      await pauseHit(s, run, shots);
      try {
        await cdp.send('Target.closeTarget', { targetId });
      } catch {
        // gone
      }
    }
    writeFileSync(join(OUT, 'shots.json'), JSON.stringify(shots, null, 2));
    process.stdout.write(JSON.stringify(shots.filter((x) => x.file).map((x) => `${x.run} ${x.moment} tier=${x.tier} ${x.viewport} ${x.input}`), null, 2) + '\n');
  } finally {
    writeFileSync(join(OUT, 'shots.json'), JSON.stringify(shots, null, 2));
    if (cdp) {
      try {
        await cdp.send('Browser.close', {}, undefined, 5000);
      } catch {
        // closing drops the socket
      }
      cdp.ws.close();
    }
    for (let i = 0; i < 50 && !chromeExited; i++) await sleep(100);
    if (chrome && !chromeExited) spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    for (const p of started) {
      if (p.exitCode === null) spawnSync('taskkill', ['/PID', String(p.pid), '/T', '/F'], { stdio: 'ignore' });
      try {
        const f = join(ROOT, '.evani', 'live', 'session.json');
        if (JSON.parse(readFileSync(f, 'utf8')).pid === p.pid) rmSync(f, { force: true });
      } catch {
        // no session file
      }
    }
    if (profile) {
      try {
        rmSync(profile, { recursive: true, force: true });
      } catch {
        // temp leftovers are harmless
      }
    }
  }
}

main().catch((e) => {
  console.error('[shots] error:', e.message ?? e);
  process.exit(2);
});
