// Edit transactions (docs/LIVE_API.md 6.31): "turn the lights down, turn the blue map, reverse the
// rotation" as three edits from three parallel agents (three MCP clients) in one transaction.
//
// Needs a running bridge (`evani-live-bridge serve --root <engine> --port <P> --game sandbox`);
// it never starts or stops one. Opens one muted headless Chrome (pointer lock, fullscreen and
// keyboard lock stubbed; the real mouse and keyboard are never used) and closes it.
//
//   node tools/live-check/edit-txn.mjs [--port 8102] [--root <engine>] [--bridge <exe>] [--out <dir>]
//
//   1. reference looks, applied with param_set: base, light down, sky blue, both;
//   2. together: three agents stage at the same moment (and then staggered), the page is sampled
//      all the way through stage and commit, and an in-page loop reads the three values every
//      animation frame: nothing shows before commit, every reading and every sample is either all
//      old or all new (no in-between state), all three history entries share one frame, and the
//      commit's telemetry holds 60 fps;
//   3. stream: the same three edits each go live as their agent stages it, a fourth with a wrong
//      name fails alone; bake_export (dry run) writes them, history_undo takes back one alone;
//   4. a clash: two agents set the light (darker, brighter): both are conflict, neither stays
//      applied, then edit_resolve keep applies the winner; the same clash the other way round,
//      then edit_resolve drop applies neither.
// The sandbox spinner has no negative range, so "reverse the rotation" is a different spin speed
// here; true reversal (a negative speed) is proved by the native test edit_tests.rs.
// Writes PNGs and report.json to --out. Exit 0 when every check passed, 1 when one failed, 2 when
// it could not run.

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
let PORT = Number(args.port ?? 8102);
const BRIDGE = resolve(args.bridge ?? join(ROOT, 'target', 'debug', 'evani-live-bridge.exe'));
const OUT = resolve(args.out ?? join(ROOT, '.evani-shot-edit-txn'));
const log = (...m) => console.error('[edit-txn]', ...m);
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
  const profile = mkdtempSync(join(tmpdir(), 'evani-edit-txn-'));
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
      const r = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'edit-txn-check', version: '1' } });
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
// the canvas and the "applying…" tag only. The game is not touched.
const CANVAS_ONLY = `(() => {
  const s = document.createElement('style');
  s.id = 'edit-txn-check-canvas-only';
  s.textContent = '*{visibility:hidden!important} canvas,#evani-trial-mark{visibility:visible!important}';
  document.head.appendChild(s);
})()`;

// ---------- The three edits ----------

const LIGHT = 'render.light.intensity', SKY = 'render.clear_color', SPIN = 'kit.example.spin_speed';
const NAMES = [LIGHT, SKY, SPIN];
const REQUEST = 'turn the lights down, turn the blue map, reverse the rotation';
const NEW = { [LIGHT]: 0.4, [SKY]: [0.1, 0.25, 0.8], [SPIN]: 6.0 };
const NOTE = (t) => `[edit-txn check] ${t}`;
const stageArgs = (txn, edit, name, note) => ({ txn, edit, cmd: 'param.set', params: { values: { [name]: NEW[name] } }, note: NOTE(note) });
const PLAN = [
  { id: 'lights', what: 'turn the lights down', name: LIGHT },
  { id: 'sky', what: 'turn the map (sky) blue', name: SKY },
  { id: 'spin', what: 'change the rotation', name: SPIN },
];

/** Look features of a sample: blueness of the top rows (sky) and luma of the centre (lit scene). */
function features(img) {
  let nb = 0, blue = 0, nc = 0, centre = 0;
  for (let y = 0; y < img.h; y++) {
    for (let x = 0; x < img.w; x++) {
      if (inTag(x, y, img.w, img.h)) continue;
      const i = (y * img.w + x) * 3;
      const r = img.rgb[i], g = img.rgb[i + 1], b = img.rgb[i + 2];
      if (y < img.h * 0.12) {
        nb++;
        blue += (b - r) / 255;
      }
      if (x > img.w * 0.3 && x < img.w * 0.7 && y > img.h * 0.3 && y < img.h * 0.7) {
        nc++;
        centre += (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
      }
    }
  }
  return { blue: blue / nb, centre: centre / nc };
}

/** The nearest reference look (base, light, sky, both) to a sample. */
function classify(f, refs) {
  let best = null, bd = Infinity;
  for (const k of ['base', 'light', 'sky', 'both']) {
    const r = refs[k];
    const d = Math.hypot((f.blue - r.blue) / refs.scale.blue, (f.centre - r.centre) / refs.scale.centre);
    if (d < bd) {
      bd = d;
      best = k;
    }
  }
  return best;
}

const near = (a, b) => (Array.isArray(a) ? Array.isArray(b) && a.every((x, i) => Math.abs(x - b[i]) < 1e-3) : Math.abs(a - b) < 1e-3);

// In-page reader: one param.get per animation frame, through the page's own live API. Each reply is
// the engine state at the tick that handled it.
const READER_START = `(() => {
  window.__edRead = []; window.__edOn = true;
  const names = ${JSON.stringify(NAMES)};
  const loop = () => {
    if (!window.__edOn) return;
    const t = performance.now();
    window.evani.live.call('param.get', { names }).then((r) => window.__edRead.push({ t, v: r.values })).catch(() => {});
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  return true;
})()`;
const READER_STOP = `(async () => { window.__edOn = false; await new Promise((r) => setTimeout(r, 300)); return window.__edRead; })()`;

/** Each per-frame reading as 'old', 'new' or 'mixed:<which are new>'. */
function readState(v, base) {
  const isNew = NAMES.map((n) => near(v[n], NEW[n]));
  const isOld = NAMES.map((n) => near(v[n], base[n]));
  if (isNew.every(Boolean)) return 'new';
  if (isOld.every(Boolean)) return 'old';
  return 'mixed:' + NAMES.filter((_, i) => isNew[i]).join('+');
}

async function proof() {
  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, '.gitignore'), '*\n');
  const report = {};
  let page = null;
  const agents = [];
  const save = (name, s) => writeFileSync(join(OUT, name), s.png);
  const settle = async (ms = 700) => {
    await sleep(ms);
    return sample(page);
  };
  const values = async (names = NAMES) => (await agents[0].call('param_get', { names })).json?.values;
  try {
    page = await openLivePage();
    await page.evaluate(CANVAS_ONLY);
    for (let i = 0; i < 3; i++) {
      const c = mcpClient();
      await c.init();
      agents.push(c);
    }
    const lead = agents[0];
    const base = await values();
    if (!base || NAMES.some((n) => base[n] === undefined)) throw new Error('the sandbox lacks one of ' + NAMES.join(', '));
    report.base = base;
    const snap = await lead.call('snapshot_save', { name: 'edit-txn-start', note: NOTE('start of the edit transaction check') });
    if (snap.isError) throw new Error('snapshot_save: ' + snap.text.join(' '));
    const restore = async () => {
      const r = await lead.call('snapshot_restore', { snapshot: 'edit-txn-start', note: NOTE('back to the start') });
      if (r.isError) throw new Error('snapshot_restore: ' + r.text.join(' '));
      await sleep(500);
    };

    // ---- 1. reference looks, applied for real ----
    const refs = {};
    for (const [k, vals] of [['base', {}], ['light', { [LIGHT]: NEW[LIGHT] }], ['sky', { [SKY]: NEW[SKY] }], ['both', { [LIGHT]: NEW[LIGHT], [SKY]: NEW[SKY] }]]) {
      await restore();
      if (Object.keys(vals).length) {
        const r = await lead.call('param_set', { values: vals, note: NOTE(`reference look: ${k}`) });
        if (r.isError) throw new Error('param_set: ' + r.text.join(' '));
      }
      const s = await settle();
      save(`0-reference-${k}.png`, s);
      refs[k] = features(s.img);
    }
    await restore();
    refs.scale = {
      blue: Math.max(0.02, Math.abs(refs.sky.blue - refs.base.blue)),
      centre: Math.max(0.01, Math.abs(refs.light.centre - refs.base.centre)),
    };
    report.reference = refs;
    const sep = (a, b) => Math.hypot((refs[a].blue - refs[b].blue) / refs.scale.blue, (refs[a].centre - refs[b].centre) / refs.scale.centre);
    const minSep = Math.min(sep('base', 'light'), sep('base', 'sky'), sep('light', 'both'), sep('sky', 'both'), sep('light', 'sky'));
    check('the four reference looks are told apart (light down darkens the centre, blue sky shows in the top rows)', minSep > 0.5 && refs.sky.blue > refs.base.blue + 0.02 && refs.light.centre < refs.base.centre - 0.01, { minSep: +minSep.toFixed(2) });

    // ---- 2. together: three agents, one frame ----
    const begin = await lead.call('edit_begin', { request: REQUEST, edits: PLAN.map(({ id, what }) => ({ id, what })), mode: 'together' });
    const txn = begin.json?.txn;
    check('edit_begin returns the transaction and the plan of atomic edits', txn && begin.json.mode === 'together' && begin.json.edits?.length === 3 && begin.json.edits.every((e) => e.state === 'pending'), begin.isError ? begin.text[0] : { txn, mode: begin.json?.mode, edits: begin.json?.edits?.map((e) => `${e.edit}:${e.state}`) });
    await page.evaluate(READER_START);
    let stopSampling;
    const sampling = new Promise((r) => (stopSampling = r));
    const samplesP = sampleWhile(page, sampling, 120000);
    await sleep(600);
    const tStage = Date.now();
    // Three agents at the same moment, each staging its own edit.
    const staged = await Promise.all(PLAN.map((p, i) => agents[i].call('edit_stage', stageArgs(txn, p.id, p.name, p.what))));
    report.together_stage_ms = Date.now() - tStage;
    check('three agents stage at once and each edit is staged (nothing applied yet)', staged.every((r) => !r.isError && r.json?.state === 'staged'), staged.map((r) => (r.isError ? r.text[0]?.slice(0, 200) : r.json?.state)));
    await sleep(1500);
    const mid = await values();
    check('before commit the values are unchanged', mid && NAMES.every((n) => near(mid[n], base[n])), mid);
    const tCommit = Date.now();
    const commit = await lead.call('edit_commit', { txn, note: NOTE('commit the three edits together') });
    const tCommitted = Date.now();
    await sleep(1200);
    stopSampling();
    const samples = await samplesP;
    const reads = await page.evaluate(READER_STOP);
    const cj = commit.json ?? {};
    const tState = cj.transaction ?? cj;
    const edits = tState.edits ?? [];
    report.together_commit = {
      error: commit.isError ? commit.text[0]?.slice(0, 300) : null,
      state: tState.state,
      counts: tState.counts,
      telemetry: cj.telemetry && { frames: cj.telemetry.frames, window_ms: cj.telemetry.window_ms, frame_ms: cj.telemetry.frame_ms },
      keys: Object.keys(cj),
    };
    check('edit_commit applies all three', !commit.isError && edits.length === 3 && edits.every((e) => e.state === 'applied'), report.together_commit);
    const after = await values();
    check('after commit all three values are the new ones', after && NAMES.every((n) => near(after[n], NEW[n])), after);

    const cls = samples.map((s) => ({ t: s.t, held: s.held, k: classify(features(s.img), refs), s }));
    const pre = cls.filter((c) => c.t < tCommit);
    const post = cls.filter((c) => c.t > tCommitted + 300);
    const between = cls.filter((c) => c.k === 'light' || c.k === 'sky');
    report.together_samples = { total: cls.length, before_commit: pre.length, after_commit: post.length, kinds: cls.map((c) => c.k).join(' '), in_between: between.length };
    if (pre.length) save('1-together-staged.png', pre[pre.length - 1].s);
    if (post.length) save('1-together-committed.png', post[post.length - 1].s);
    check('canvas: nothing shows while the edits are staged (every sample before commit is the start look)', pre.length >= 3 && pre.every((c) => c.k === 'base'), report.together_samples);
    check('canvas: after commit every sample shows both looks (lights down and blue sky together)', post.length >= 2 && post.every((c) => c.k === 'both'), report.together_samples);
    check('canvas: no sample shows an in-between state (only one of the two looks)', between.length === 0, report.together_samples);

    const rs = (reads ?? []).map((r) => readState(r.v, base));
    const firstNew = rs.indexOf('new');
    const mixed = rs.filter((x) => x.startsWith('mixed'));
    report.together_reads = {
      readings: rs.length,
      old: rs.filter((x) => x === 'old').length,
      new: rs.filter((x) => x === 'new').length,
      mixed: mixed.length,
      mixed_kinds: [...new Set(mixed)],
      back_to_old_after_new: firstNew >= 0 && rs.slice(firstNew).some((x) => x === 'old'),
    };
    check('per-frame readings: every frame is all old or all new, never a mix', rs.length > 60 && mixed.length === 0 && firstNew > 0 && !report.together_reads.back_to_old_after_new, report.together_reads);

    const hist = (await lead.call('history_list', { limit: 30 })).json?.entries ?? [];
    const mine = hist.filter((e) => e.txn === txn);
    report.together_history = mine.map((e) => ({ tx: e.tx, edit: e.edit, frame: e.frame, state: e.state, note: e.note }));
    check('history: one undoable entry per edit, each with its note, all on the same frame', mine.length === 3 && new Set(mine.map((e) => e.frame)).size === 1 && mine.every((e) => e.state === 'applied' && e.note), report.together_history);
    const tel = cj.telemetry;
    check('the render loop holds 60 fps through the commit (telemetry after the change)', tel && tel.frames >= 55 * ((tel.window_ms ?? 1000) / 1000) && tel.frame_ms?.avg <= 17.5, report.together_commit.telemetry);
    await restore();

    // ---- 3. stream (the default): each edit goes live as its agent stages it ----
    const sBegin = await lead.call('edit_begin', { request: REQUEST, edits: [...PLAN.map(({ id, what }) => ({ id, what })), { id: 'typo', what: 'a fourth agent with a wrong name' }] });
    const stxn = sBegin.json?.txn;
    check('without a mode the transaction streams (the default)', sBegin.json?.mode === 'stream', sBegin.json?.mode);
    const sStaged = await Promise.all([
      ...PLAN.map((p, i) => sleep(i * 700).then(() => agents[i].call('edit_stage', stageArgs(stxn, p.id, p.name, p.what)))),
      sleep(300).then(() => agents[0].call('edit_stage', { txn: stxn, edit: 'typo', cmd: 'param.set', params: { values: { 'render.light.intensty': 0.4 } }, note: NOTE('misspelt parameter') })),
    ]);
    const sStates = sStaged.map((r) => (r.isError ? 'error: ' + r.text[0]?.slice(0, 120) : r.json?.state));
    report.stream_stage = sStates;
    check('stream: each of the three edits is applied as soon as it is staged', sStates.slice(0, 3).every((s) => s === 'applied'), sStates);
    check('stream: the bad edit fails on its own with the reason, the others still apply', sStaged[3].json?.state === 'failed' && sStaged[3].json?.error?.code, sStaged[3].json?.error ?? sStates[3]);
    const sCommit = await lead.call('edit_commit', { txn: stxn });
    const sState = sCommit.json?.transaction ?? sCommit.json ?? {};
    report.stream_commit = { state: sState.state, counts: sState.counts, error: sCommit.isError ? sCommit.text[0]?.slice(0, 200) : null };
    check('stream: commit closes the transaction with every edit\'s result', sState.state === 'closed' && sState.counts?.applied === 3 && sState.counts?.failed === 1, report.stream_commit);
    const sAfter = await values();
    check('stream: all three values are live', sAfter && NAMES.every((n) => near(sAfter[n], NEW[n])), sAfter);
    save('2-stream-done.png', await settle(500));

    // Bake and undo treat transaction edits like any change.
    const bake = await lead.call('bake_export', { dry_run: true });
    const bakeText = JSON.stringify(bake.json ?? {});
    report.bake = { error: bake.isError ? bake.text[0]?.slice(0, 200) : null, files: (bake.json?.written ?? []).map((f) => f.path) };
    check('bake_export (dry run) writes the transaction edits and their notes like any change', !bake.isError && bakeText.includes('spin_speed') && bakeText.includes('light.intensity') && bakeText.includes('change the rotation'), report.bake);
    const undo = await lead.call('history_undo', { note: NOTE('undo the last transaction edit') });
    const uAfter = await values();
    report.undo = { error: undo.isError ? undo.text[0]?.slice(0, 200) : null, values: uAfter };
    check('history_undo takes back one transaction edit on its own (the spin); the other two stay', !undo.isError && uAfter && near(uAfter[SPIN], base[SPIN]) && near(uAfter[LIGHT], NEW[LIGHT]) && near(uAfter[SKY], NEW[SKY]), report.undo);
    await restore();

    // ---- 4. a clash: darker and brighter on the same light ----
    const cBegin = await lead.call('edit_begin', { request: 'make the light darker; make the light brighter', edits: [{ id: 'darker', what: 'light darker' }, { id: 'brighter', what: 'light brighter' }] });
    const ctxn = cBegin.json?.txn;
    const cStaged = await Promise.all([
      agents[0].call('edit_stage', { txn: ctxn, edit: 'darker', cmd: 'param.set', params: { values: { [LIGHT]: 0.4 } }, note: NOTE('darker') }),
      agents[1].call('edit_stage', { txn: ctxn, edit: 'brighter', cmd: 'param.set', params: { values: { [LIGHT]: 6.0 } }, note: NOTE('brighter') }),
    ]);
    const cStatus = (await lead.call('edit_status', { txn: ctxn })).json ?? {};
    const cv = (await values([LIGHT]))?.[LIGHT];
    report.clash = { stage: cStaged.map((r) => r.json?.state ?? r.text[0]?.slice(0, 120)), status: cStatus.edits?.map((e) => `${e.edit}:${e.state}`), light: cv };
    check('clash: both edits to the same light are reported as conflict, naming each other', cStatus.edits?.length === 2 && cStatus.edits.every((e) => e.state === 'conflict' && (e.conflict_with ?? []).length), report.clash);
    check('clash: neither stays applied (the light is back at its start value)', near(cv, base[LIGHT]), cv);
    const resolved = await lead.call('edit_resolve', { txn: ctxn, keep: 'darker', note: NOTE('the user chose darker') });
    const rv = (await values([LIGHT]))?.[LIGHT];
    report.clash.resolved = { error: resolved.isError ? resolved.text[0]?.slice(0, 200) : null, light: rv };
    check('clash: edit_resolve keep applies the winner the user chose', !resolved.isError && near(rv, 0.4), report.clash.resolved);
    await lead.call('edit_commit', { txn: ctxn });
    await restore();

    // The same clash with the edits arriving the other way round: the same outcome.
    const rBegin = await lead.call('edit_begin', { request: 'make the light brighter; make the light darker', edits: [{ id: 'brighter', what: 'light brighter' }, { id: 'darker', what: 'light darker' }] });
    const rtxn = rBegin.json?.txn;
    const r1 = await agents[1].call('edit_stage', { txn: rtxn, edit: 'brighter', cmd: 'param.set', params: { values: { [LIGHT]: 6.0 } }, note: NOTE('brighter') });
    const r2 = await agents[0].call('edit_stage', { txn: rtxn, edit: 'darker', cmd: 'param.set', params: { values: { [LIGHT]: 0.4 } }, note: NOTE('darker') });
    const rStatus = (await lead.call('edit_status', { txn: rtxn })).json ?? {};
    const rv2 = (await values([LIGHT]))?.[LIGHT];
    report.clash_reverse = { stage: [r1, r2].map((r) => r.json?.state ?? r.text[0]?.slice(0, 120)), status: rStatus.edits?.map((e) => `${e.edit}:${e.state}`), light: rv2, with: r2.json?.conflict?.with };
    check('clash, reverse order: both are conflict, the second names the first, neither stays applied', rStatus.edits?.length === 2 && rStatus.edits.every((e) => e.state === 'conflict') && JSON.stringify(r2.json?.conflict?.with) === '["brighter"]' && near(rv2, base[LIGHT]), report.clash_reverse);
    const dropped = await lead.call('edit_resolve', { txn: rtxn, drop: 'brighter', note: NOTE('the user wants neither') });
    const dv = (await values([LIGHT]))?.[LIGHT];
    const dStatus = (await lead.call('edit_status', { txn: rtxn })).json ?? {};
    report.clash_reverse.dropped = { error: dropped.isError ? dropped.text[0]?.slice(0, 200) : null, light: dv, status: dStatus.edits?.map((e) => `${e.edit}:${e.state}`) };
    check('clash, reverse order: edit_resolve drop applies neither', !dropped.isError && near(dv, base[LIGHT]) && dStatus.edits?.every((e) => e.state === 'dropped'), report.clash_reverse.dropped);
    await lead.call('edit_commit', { txn: rtxn });
    await restore();
    save('3-final.png', await settle(500));
    report.console = page.consoleLines.slice(0, 20);
  } finally {
    for (const a of agents) a.close();
    if (page) await page.close();
    writeFileSync(join(OUT, 'report.json'), JSON.stringify({ ...report, checks: results }, null, 2) + '\n');
  }
  const failed = results.filter((r) => !r.ok).length;
  log(`${results.length - failed}/${results.length} checks passed; written to ${OUT}`);
  process.exit(failed ? 1 : 0);
}

proof().catch((e) => {
  log('could not run:', e.stack ?? e);
  process.exit(2);
});
