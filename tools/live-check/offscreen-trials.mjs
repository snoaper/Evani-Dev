// Trials run off-screen (docs/LIVE_API.md 6.30): while sweep_run or compare_ab runs, the visible
// canvas holds its last frame; only applied values show.
//
// Needs a running bridge (`evani-live-bridge serve --root <engine> --port <P> --game sandbox`);
// it never starts or stops one. Opens one muted headless Chrome (pointer lock, fullscreen and
// keyboard lock stubbed; the real mouse and keyboard are never used) and closes it.
//
//   node tools/live-check/offscreen-trials.mjs [--port 8101] [--root <engine>] [--bridge <exe>] [--out <dir>]
//   node tools/live-check/offscreen-trials.mjs --numbers [--port 8101 --root <engine>] [--base-port 8187 --base-root <old engine>] [--repeat 4] [--bridge <exe>] [--out <file>]
//
// Default mode, the proof:
//   1. sweep render.exposure (very dark, very bright) and sample the visible page
//      with Page.captureScreenshot all the way through: every sample taken while the page is marked
//      held must equal the first held sample and keep the start look;
//   2. the same for compare_ab with B brighter, kept (B shows at once after) and not kept; and a
//      failing sweep must release the hold;
//   3. freeze the page (Page.setWebLifecycleState frozen, as a hidden tab) mid-sweep, wake it,
//      and check the hold is released after the run ends however it ended;
//   4. param_set shows its change at once.
// Writes before / during / after PNGs and report.json to --out.
// `--numbers`: sweeps of fx.bloom.intensity [0.3, 0.6, 1.0] at 120 frames per value, alternating
// between the new and the old engine (see numbers() below).
// Exit 0 when every check passed, 1 when one failed, 2 when it could not run.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[++i] : true;
}
let ROOT = resolve(args.root ?? fileURLToPath(new URL('../..', import.meta.url)));
let PORT = Number(args.port ?? 8101);
const BRIDGE = resolve(args.bridge ?? join(ROOT, 'target', 'debug', 'evani-live-bridge.exe'));
const OUT = resolve(args.out ?? join(ROOT, '.vibedeck', 'screenshots', 'live-offscreen-trials'));
const log = (...m) => console.error('[offscreen]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok: !!ok, detail });
  log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${typeof detail === 'string' ? detail : JSON.stringify(detail)}` : ''}`);
  return !!ok;
};

// ---------- Chrome over CDP (as sweep.mjs; no npm packages) ----------

function findChrome() {
  const c = [args.chrome, process.env.EVANI_CHROME];
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]) {
    if (base) c.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'), join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
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
      const m = JSON.parse(ev.data);
      if (m.id !== undefined && this.pending.has(m.id)) {
        const p = this.pending.get(m.id);
        this.pending.delete(m.id);
        m.error ? p.reject(new Error(`${p.method}: ${m.error.message}`)) : p.resolve(m.result);
      } else for (const h of this.handlers) h(m);
    });
  }
  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => {
      ws.addEventListener('open', res, { once: true });
      ws.addEventListener('error', rej, { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}, sessionId, ms = 120000) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      const timer = setTimeout(() => rej(new Error(`${method} timed out`)), ms);
      this.pending.set(id, { resolve: (v) => (clearTimeout(timer), res(v)), reject: (e) => (clearTimeout(timer), rej(e)), method });
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

async function openChrome() {
  const exe = findChrome();
  if (!exe) throw new Error('no Chrome or Edge found (pass --chrome)');
  const profile = mkdtempSync(join(tmpdir(), 'evani-offscreen-'));
  const flags = ['--headless=new', '--mute-audio', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check', '--enable-unsafe-webgpu', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--window-size=1280,720'];
  const chrome = spawn(exe, [...flags, 'about:blank'], { stdio: 'ignore', windowsHide: true });
  log(`chrome pid ${chrome.pid}`);
  let ws = null;
  for (let i = 0; i < 200 && !ws; i++) {
    const f = join(profile, 'DevToolsActivePort');
    if (existsSync(f)) {
      const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
      if (p && path) ws = `ws://127.0.0.1:${p}${path}`;
    }
    if (!ws) await sleep(100);
  }
  if (!ws) throw new Error('Chrome did not open its debugging port');
  const cdp = await Cdp.connect(ws);
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (m, p, ms) => cdp.send(m, p, sessionId, ms);
  const consoleLines = [];
  cdp.handlers.push((m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === 'Runtime.exceptionThrown') consoleLines.push('exception: ' + (m.params.exceptionDetails?.exception?.description ?? m.params.exceptionDetails?.text));
    if (m.method === 'Runtime.consoleAPICalled' && ['error', 'warning'].includes(m.params.type)) consoleLines.push(`console.${m.params.type}: ` + m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
  });
  await s('Page.enable');
  await s('Runtime.enable');
  await s('Emulation.setDeviceMetricsOverride', { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
  await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
  const close = async () => {
    try {
      await cdp.send('Browser.close', {}, undefined, 3000);
    } catch {
      // closing drops the socket
    }
    cdp.ws.close();
    await sleep(500);
    if (chrome.exitCode === null) spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    await sleep(300);
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // Chrome may still hold a file for a moment
    }
  };
  const evaluate = async (expression, ms = 30000) => {
    const r = await s('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, ms);
    if (r.exceptionDetails) throw new Error('page: ' + (r.exceptionDetails.exception?.description ?? r.exceptionDetails.text));
    return r.result.value;
  };
  return { s, close, evaluate, consoleLines };
}

async function status() {
  try {
    return await (await fetch(`http://127.0.0.1:${PORT}/live/status`, { signal: AbortSignal.timeout(2000) })).json();
  } catch {
    return null;
  }
}

async function openLivePage() {
  for (const t0 = Date.now(); (await status())?.phase !== 'idle'; await sleep(2000)) {
    if (Date.now() - t0 > 30 * 60000) throw new Error(`no idle bridge on ${PORT}`);
  }
  const page = await openChrome();
  await page.s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` });
  for (const t0 = Date.now(); !(await status())?.engine_connected; await sleep(300)) {
    if (Date.now() - t0 > 180000) throw new Error('the page did not connect in 180 s\n' + page.consoleLines.join('\n'));
  }
  await sleep(4000);
  log('live page connected');
  return page;
}

// ---------- The bridge's MCP server on stdio ----------

function mcpClient() {
  const p = spawn(BRIDGE, ['mcp', '--root', ROOT], { stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true });
  let buf = '';
  let id = 0;
  const pending = new Map();
  p.stdout.on('data', (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (!line) continue;
      const m = JSON.parse(line);
      if (m.id !== undefined && pending.has(m.id)) {
        pending.get(m.id)(m);
        pending.delete(m.id);
      }
    }
  });
  const rpc = (method, params) =>
    new Promise((res) => {
      const mid = ++id;
      pending.set(mid, res);
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: mid, method, params }) + '\n');
    });
  return {
    async init() {
      const r = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'offscreen-check', version: '1' } });
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
      return r.result;
    },
    async call(name, a) {
      const r = await rpc('tools/call', { name, arguments: a });
      const res = r.result ?? { isError: true, content: [{ type: 'text', text: JSON.stringify(r.error) }] };
      const text = res.content.filter((c) => c.type === 'text').map((c) => c.text);
      const images = res.content.filter((c) => c.type === 'image');
      let json = null;
      try {
        json = JSON.parse(text[0]);
      } catch {
        // error text
      }
      return { isError: !!res.isError, json, text, images };
    },
    close() {
      p.stdin.end();
      setTimeout(() => p.exitCode === null && spawnSync('taskkill', ['/PID', String(p.pid), '/T', '/F'], { stdio: 'ignore' }), 1000);
    },
  };
}

// ---------- PNG (8-bit RGB/RGBA, non-interlaced) ----------

function decodePng(b) {
  let off = 8;
  let w = 0, h = 0, ct = 0;
  const idat = [];
  while (off < b.length) {
    const len = b.readUInt32BE(off);
    const type = b.toString('ascii', off + 4, off + 8);
    const data = b.subarray(off + 8, off + 8 + len);
    if (type === 'IHDR') {
      w = data.readUInt32BE(0);
      h = data.readUInt32BE(4);
      ct = data[9];
    } else if (type === 'IDAT') idat.push(data);
    off += 12 + len;
  }
  const bpp = { 2: 3, 6: 4 }[ct];
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const out = Buffer.alloc(w * h * 3);
  const prev = Buffer.alloc(stride);
  const cur = Buffer.alloc(stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    raw.copy(cur, 0, y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? cur[x - bpp] : 0;
      const up = prev[x];
      const c = x >= bpp ? prev[x - bpp] : 0;
      let v = cur[x];
      if (f === 1) v += a;
      else if (f === 2) v += up;
      else if (f === 3) v += (a + up) >> 1;
      else if (f === 4) {
        const p = a + up - c;
        const pa = Math.abs(p - a), pb = Math.abs(p - up), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? up : c;
      }
      cur[x] = v & 255;
    }
    for (let x = 0; x < w; x++) for (let k = 0; k < 3; k++) out[(y * w + x) * 3 + k] = cur[x * bpp + k];
    cur.copy(prev);
  }
  return { w, h, rgb: out };
}

// The page sampled at quarter size (320 x 180). The "measuring…" tag sits in the bottom-right
// corner; it is left out of every comparison (it is DOM, not canvas).
const SAMPLE_SCALE = 0.25;
const inTag = (x, y, w, h) => x > w * 0.8 && y > h * 0.85;

async function sample(page) {
  const held = await page.evaluate("document.documentElement.getAttribute('data-evani-trial')");
  const { data } = await page.s('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: 1280, height: 720, scale: SAMPLE_SCALE } }, 30000);
  const png = Buffer.from(data, 'base64');
  return { t: Date.now(), held, png, img: decodePng(png) };
}

/** Fraction of pixels (outside the tag) that differ by more than `tol` in any channel. */
function diff(a, b, tol = 6) {
  let n = 0, d = 0;
  for (let y = 0; y < a.h; y++) {
    for (let x = 0; x < a.w; x++) {
      if (inTag(x, y, a.w, a.h)) continue;
      n++;
      const i = (y * a.w + x) * 3;
      if (Math.abs(a.rgb[i] - b.rgb[i]) > tol || Math.abs(a.rgb[i + 1] - b.rgb[i + 1]) > tol || Math.abs(a.rgb[i + 2] - b.rgb[i + 2]) > tol) d++;
    }
  }
  return d / n;
}

/** Mean luma (0..1) of the canvas outside the tag. */
function luma(img) {
  let n = 0, l = 0;
  for (let y = 0; y < img.h; y++) {
    for (let x = 0; x < img.w; x++) {
      if (inTag(x, y, img.w, img.h)) continue;
      n++;
      const i = (y * img.w + x) * 3;
      l += 0.2126 * img.rgb[i] + 0.7152 * img.rgb[i + 1] + 0.0722 * img.rgb[i + 2];
    }
  }
  return l / n / 255;
}

/** Sample the page until `done` settles; returns the samples. */
async function sampleWhile(page, done, maxMs = 300000) {
  const out = [];
  let finished = false;
  done.finally(() => (finished = true));
  const t0 = Date.now();
  while (!finished && Date.now() - t0 < maxMs) {
    try {
      out.push(await sample(page));
    } catch (e) {
      log('sample failed:', e.message);
      await sleep(200);
    }
    await sleep(150);
  }
  return out;
}

// The page's DOM panels (stats overlay, help) change every frame; hide them so the samples show
// the canvas and the trial tag only. The game is not touched.
const CANVAS_ONLY = `(() => {
  const s = document.createElement('style');
  s.id = 'offscreen-check-canvas-only';
  s.textContent = '*{visibility:hidden!important} canvas,#evani-trial-mark{visibility:visible!important}';
  document.head.appendChild(s);
})()`;
const isHeld = (page) => page.evaluate("document.documentElement.getAttribute('data-evani-trial')");

// Exposure is the most obvious single change in every game: 0.15 darkens the whole view, 4 floods it.
const DARK = 0.15, BRIGHT = 4.0, KEEP = 2.5, SHOW = 0.3;
const NOTE = (t) => `[offscreen check] ${t}`;

async function proof() {
  mkdirSync(OUT, { recursive: true });
  const report = {};
  let page = null;
  let mcp = null;
  const save = (name, s) => writeFileSync(join(OUT, name), s.png);
  const settle = async (ms = 600) => {
    await sleep(ms);
    return sample(page);
  };
  try {
    page = await openLivePage();
    await page.evaluate(CANVAS_ONLY);
    mcp = mcpClient();
    await mcp.init();
    report.start = (await mcp.call('param_get', { prefix: 'render.exposure' })).json;
    await mcp.call('snapshot_save', { name: 'offscreen-start', note: 'start of the off-screen check' });

    // What the trial values look like when applied for real (so "not shown" means something).
    const base = await settle();
    const ref = { base: luma(base.img) };
    for (const [name, v] of [['dark', DARK], ['bright', BRIGHT]]) {
      const set = await mcp.call('param_set', { values: { 'render.exposure': v }, note: NOTE(`show ${name} for reference`) });
      if (set.isError) throw new Error('param_set render.exposure: ' + set.text.join(' '));
      const s = await settle();
      save(`0-reference-${name}.png`, s);
      ref[name] = luma(s.img);
    }
    report.reference_luma = ref;
    check('applied trial values are plainly visible (reference)', ref.dark < ref.base - 0.1 && ref.bright > ref.base + 0.1, ref);
    await mcp.call('snapshot_restore', { snapshot: 'offscreen-start', note: NOTE('back to the start') });

    // ---- 1. sweep ----
    const before = await settle();
    save('1-before.png', before);
    const thumbsDir = join('.vibedeck', 'screenshots', 'live-offscreen-trials', 'sweep-thumbs').replace(/\\/g, '/');
    const sweepP = mcp.call('sweep_run', { param: 'render.exposure', values: [DARK, BRIGHT, DARK, BRIGHT], frames_per_value: 90, thumbs: { w: 160, dir: thumbsDir } });
    const sw = await sampleWhile(page, sweepP);
    const sres = await sweepP;
    const after = await settle(400);
    save('1-after.png', after);
    const heldS = sw.filter((s) => s.held !== null);
    if (heldS.length) {
      save('1-during-first.png', heldS[0]);
      save('1-during-middle.png', heldS[Math.floor(heldS.length / 2)]);
      save('1-during-last.png', heldS[heldS.length - 1]);
    }
    check('sweep_run succeeds', !sres.isError && sres.json, sres.isError ? sres.text[0]?.slice(0, 300) : `${sres.json?.rows?.length} rows`);
    check('the page is marked held during the sweep', heldS.length >= 3, `${heldS.length} of ${sw.length} samples held`);
    const sDiffs = heldS.map((s) => diff(heldS[0].img, s.img));
    const sLuma = heldS.map((s) => luma(s.img));
    report.sweep = {
      samples: sw.length,
      held: heldS.length,
      max_diff_from_first_held: Math.max(0, ...sDiffs),
      held_luma_min: Math.min(...sLuma),
      held_luma_max: Math.max(...sLuma),
      before_luma: luma(before.img),
      after_luma: luma(after.img),
    };
    check('every held sample equals the first held frame', Math.max(0, ...sDiffs) < 0.002, report.sweep);
    check('no held sample shows a trial value (luma stays at the start look)', sLuma.every((l) => Math.abs(l - ref.base) < 0.04), report.sweep);
    // The off-screen rows did see their values: row luma follows exposure (table and thumbnails).
    const t = sres.json ?? { columns: [], rows: [] };
    const lc = t.columns.indexOf('mean_luma');
    report.sweep.rows_mean_luma = t.rows.map((r) => r[lc]);
    const thumbs = (t.thumbs ?? []).map((x) => x.path && resolve(ROOT, x.path)).filter((p) => p && existsSync(p));
    report.sweep.thumb_luma = thumbs.map((p) => +luma(decodePng(readFileSync(p))).toFixed(3));
    const rl = report.sweep.rows_mean_luma;
    check('the off-screen rows show the trial values (table: dark rows darker, bright rows brighter than row 0)', rl.length === 5 && rl[1] < rl[0] && rl[3] < rl[0] && rl[2] > rl[0] && rl[4] > rl[0], rl);
    const tl = report.sweep.thumb_luma;
    check('the thumbnails show the trial values too', tl.length === 5 && tl[1] < tl[0] && tl[2] > tl[0], tl);
    check('the hold is released after the sweep and the start look is back', (await isHeld(page)) === null && Math.abs(luma(after.img) - ref.base) < 0.04, report.sweep);
    const a2 = await settle(300);
    report.sweep.moving_after = diff(after.img, a2.img);

    // ---- 2. compare_ab, kept: held during, B shows at once after ----
    const cmpP = mcp.call('compare_ab', { b: { values: { 'render.exposure': KEEP } }, seconds: 1.5, warmup_s: 0.3, keep: true, note: NOTE('keep the brighter exposure') });
    const cs = await sampleWhile(page, cmpP);
    const cres = await cmpP;
    const cAfter = await sample(page);
    save('2-compare-after-kept.png', cAfter);
    const heldC = cs.filter((s) => s.held !== null);
    if (heldC.length) save('2-compare-during.png', heldC[Math.floor(heldC.length / 2)]);
    check('compare_ab succeeds', !cres.isError, cres.isError ? cres.text[0]?.slice(0, 300) : '');
    const cDiffs = heldC.map((s) => diff(heldC[0].img, s.img));
    const cLuma = heldC.map((s) => luma(s.img));
    report.compare = { samples: cs.length, held: heldC.length, max_diff_from_first_held: Math.max(0, ...cDiffs), held_luma_max: Math.max(...cLuma), after_kept_luma: luma(cAfter.img) };
    check('compare: held the whole run and never shows B', heldC.length >= 2 && Math.max(0, ...cDiffs) < 0.002 && cLuma.every((l) => Math.abs(l - ref.base) < 0.04), report.compare);
    check('compare: kept B shows at once after the run', cAfter.held === null && report.compare.after_kept_luma > ref.base + 0.08, report.compare);

    // ---- 2b. compare_ab not kept: held during, the unchanged state after ----
    await mcp.call('snapshot_restore', { snapshot: 'offscreen-start', note: NOTE('back to the start') });
    await settle(400);
    const cmp2 = await mcp.call('compare_ab', { b: { values: { 'render.exposure': BRIGHT } }, seconds: 1, warmup_s: 0.2 });
    const c2After = await sample(page);
    save('2b-compare-after-reverted.png', c2After);
    report.compare_reverted = { error: cmp2.isError ? cmp2.text[0]?.slice(0, 200) : null, after_luma: luma(c2After.img) };
    check('compare without keep: the unchanged state shows after the run', !cmp2.isError && c2After.held === null && Math.abs(report.compare_reverted.after_luma - ref.base) < 0.04, report.compare_reverted);

    // ---- 2c. a failing sweep (bad value on a later row) releases the hold ----
    const bad = await mcp.call('sweep_run', { param: 'render.exposure', values: [DARK, 'not a number'], frames_per_value: 30, look_at: [] });
    const badAfter = await settle(300);
    report.failed_sweep = { error: bad.isError ? bad.text[0]?.slice(0, 200) : 'no error', held_after: badAfter.held, luma_after: luma(badAfter.img) };
    check('a failed sweep errors and releases the hold with the start look', bad.isError && badAfter.held === null && Math.abs(report.failed_sweep.luma_after - ref.base) < 0.04, report.failed_sweep);

    // ---- 3. tab hidden (page frozen) mid-sweep ----
    const hidP = mcp.call('sweep_run', { param: 'render.exposure', values: [DARK, BRIGHT], frames_per_value: 90, look_at: [] });
    let sawHeld = false;
    for (const t0 = Date.now(); Date.now() - t0 < 20000 && !sawHeld; await sleep(100)) sawHeld = (await isHeld(page)) !== null;
    check('hidden tab: the sweep holds before the tab is hidden', sawHeld);
    // What a person sees while held: the page as is (panels shown), full size, with the tag.
    const fullShot = async (name) => {
      await page.evaluate("(document.getElementById('offscreen-check-canvas-only') || {}).remove?.()");
      const { data } = await page.s('Page.captureScreenshot', { format: 'png' }, 30000);
      writeFileSync(join(OUT, name), Buffer.from(data, 'base64'));
      const tag = await page.evaluate("(() => { const t = document.getElementById('evani-trial-mark'); if (!t) return null; const r = t.getBoundingClientRect(); return { text: t.textContent, x: r.x, y: r.y, w: r.width, h: r.height }; })()");
      await page.evaluate(CANVAS_ONLY);
      return tag;
    };
    report.tag_while_held = await fullShot('3-during-full-with-tag.png');
    check('while held the page shows the small "measuring…" tag in the bottom-right corner', report.tag_while_held?.text === 'measuring…' && report.tag_while_held.x + report.tag_while_held.w > 1240 && report.tag_while_held.y > 600 && report.tag_while_held.w < 160, report.tag_while_held);
    await page.s('Page.setWebLifecycleState', { state: 'frozen' });
    log('page frozen (as a hidden tab) for 5 s');
    await sleep(5000);
    await page.s('Page.setWebLifecycleState', { state: 'active' });
    for (const [m, p] of [['Page.bringToFront', {}], ['Emulation.setFocusEmulationEnabled', { enabled: true }]]) {
      try {
        await page.s(m, p);
      } catch (e) {
        log(`${m}: ${e.message}`);
      }
    }
    for (let i = 0; i < 4; i++) {
      log('after waking:', await page.evaluate('document.visibilityState + " " + document.hasFocus()'), JSON.stringify((await status())?.page ?? {}).slice(0, 200));
      await sleep(1000);
    }
    const hres = await hidP;
    report.hidden = { answer: hres.isError ? hres.text[0]?.slice(0, 200) : `ok, ${hres.json?.rows?.length} rows` };
    // Shown again, the run carries on where it stopped and ends; the hold goes with it.
    let released = false;
    const tw = Date.now();
    for (; Date.now() - tw < 60000 && !released; await sleep(250)) released = (await isHeld(page)) === null;
    report.hidden.ms_until_released_after_shown = Date.now() - tw;
    const hAfter = await settle(400);
    save('3-after-hidden-tab.png', hAfter);
    const hAfter2 = await settle(400);
    report.hidden.luma_after = luma(hAfter.img);
    report.hidden.moving_after = diff(hAfter.img, hAfter2.img);
    check('hidden tab: once shown again the run ends and the hold is released', released && hAfter.held === null && hAfter2.held === null, report.hidden);
    check('hidden tab: the start look is back, nothing frozen', Math.abs(report.hidden.luma_after - ref.base) < 0.04 && report.hidden.moving_after > 0, report.hidden);
    report.tag_after = await fullShot('3-after-full-no-tag.png');
    check('after the run the tag is gone', report.tag_after === null, report.tag_after);

    // ---- 4. param_set shows at once ----
    save('4-param-set-before.png', await sample(page));
    const ps = await mcp.call('param_set', { values: { 'render.exposure': SHOW }, note: NOTE('param_set shows at once') });
    const t0 = Date.now();
    let s4 = await sample(page);
    while (Date.now() - t0 < 3000 && luma(s4.img) > ref.base - 0.05) s4 = await sample(page);
    save('4-param-set-after.png', s4);
    report.param_set = { error: ps.isError ? ps.text[0]?.slice(0, 200) : null, luma: luma(s4.img), ms_until_shown: Date.now() - t0, held: s4.held };
    check('param_set shows its change at once (not held)', !ps.isError && report.param_set.luma < ref.base - 0.05 && s4.held === null && report.param_set.ms_until_shown < 1500, report.param_set);
    await mcp.call('snapshot_restore', { snapshot: 'offscreen-start', note: NOTE('back to the start') });
    save('5-final.png', await settle(500));
    report.console = page.consoleLines.slice(0, 20);
  } finally {
    if (mcp) mcp.close();
    if (page) await page.close();
    writeFileSync(join(OUT, 'report.json'), JSON.stringify({ ...report, checks: results }, null, 2) + '\n');
  }
  const failed = results.filter((r) => !r.ok).length;
  log(`${results.length - failed}/${results.length} checks passed; written to ${OUT}`);
  process.exit(failed ? 1 : 0);
}

// `--numbers`: the same bloom sweep, on this engine (--port, --root) and, with --base-port and
// --base-root, on the old one too, alternating A B B A ... with one fresh page per run, so drift on
// a shared machine falls on both sides alike. Writes every table plus a per-side summary.
async function numbers() {
  const outFile = resolve(args.out && args.out !== true ? args.out : join(ROOT, '.vibedeck', 'screenshots', 'live-offscreen-trials', 'numbers.json'));
  mkdirSync(dirname(outFile), { recursive: true });
  const sides = [{ name: 'new', port: PORT, root: ROOT }];
  if (args['base-port']) sides.push({ name: 'baseline', port: Number(args['base-port']), root: resolve(args['base-root']) });
  const runs = [];
  const repeat = Number(args.repeat ?? 2);
  for (let i = 0; i < repeat; i++) {
    const order = i % 2 ? [...sides].reverse() : sides;
    for (const side of order) {
      PORT = side.port;
      ROOT = side.root;
      let page = null;
      let mcp = null;
      try {
        page = await openLivePage();
        mcp = mcpClient();
        await mcp.init();
        const r = await mcp.call('sweep_run', { param: 'fx.bloom.intensity', values: [0.3, 0.6, 1.0], frames_per_value: 120 });
        if (r.isError) throw new Error('sweep_run: ' + r.text.join(' '));
        runs.push({ side: side.name, round: i, table: r.json });
        log(`${side.name} round ${i}: ` + r.json.rows.map((row) => JSON.stringify(row.slice(0, 4))).join(' '));
      } finally {
        if (mcp) mcp.close();
        if (page) await page.close();
        for (const t0 = Date.now(); Date.now() - t0 < 15000 && (await status())?.engine_connected; ) await sleep(250);
      }
    }
  }
  // Per side and row: mean, min and max of each cost column; the look columns as one set of values.
  const summary = {};
  for (const side of sides) {
    const mine = runs.filter((r) => r.side === side.name).map((r) => r.table);
    if (!mine.length) continue;
    const cols = mine[0].columns;
    const stat = (xs) => ({ mean: +(xs.reduce((a, b) => a + b, 0) / xs.length).toFixed(3), min: Math.min(...xs), max: Math.max(...xs) });
    summary[side.name] = mine[0].rows.map((_, ri) => {
      const row = {};
      for (const [ci, c] of cols.entries()) {
        const xs = mine.map((t) => t.rows[ri][ci]);
        row[c] = /ms$|^gpu\./.test(c) ? stat(xs) : [...new Set(xs.map((x) => JSON.stringify(x)))].map((x) => JSON.parse(x));
      }
      return row;
    });
  }
  writeFileSync(outFile, JSON.stringify({ sides, summary, runs }, null, 2) + '\n');
  for (const [name, rows] of Object.entries(summary)) {
    for (const r of rows) log(`${name} bloom ${r['fx.bloom.intensity']}: work ${JSON.stringify(r.work_ms)} gpu ${JSON.stringify(r.gpu_ms)} present ${JSON.stringify(r['gpu.present'])} mean_luma ${r.mean_luma} clipped ${r.clipped} bloom ${r.bloom}`);
  }
  log(`written to ${outFile}`);
  process.exit(0);
}

(args.numbers ? numbers : proof)().catch((e) => {
  log('could not run:', e.stack ?? e);
  process.exit(2);
});
