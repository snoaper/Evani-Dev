// Evani.Dev milestone 1 acceptance driver (docs/M1-ACCEPTANCE.md).
//
// Needs a running bridge (`evani-live-bridge serve --root <engine> --port <P> --game sandbox`);
// it never starts or stops one. Each mode opens its own headless Chrome (pointer lock,
// fullscreen and keyboard lock stubbed; the real mouse and keyboard are never used) and
// closes it at the end.
//
//   node tools/live-check/accept.mjs ai     [--port 8098] [--root <engine>] [--out <dir>]
//        DONE 6 (headless part): drives the changes the way "Talk to the AI" does, through
//        the bridge's MCP server on stdio (evani-live-bridge mcp), while a separate /ctl
//        client watches for each change's session.mirror; then bloom, a tonemap rewrite and
//        a bake dry run. Reports frames and milliseconds from send to mirror.
//   node tools/live-check/accept.mjs safety [--port 8098] ...
//        DONE 7: broken WGSL rejected with the old shader kept; a heavy tonemap slot that
//        pushes frame time over budget is auto-reverted (time measured); undo/redo across
//        20 changes returns exactly to each recorded state.
//   node tools/live-check/accept.mjs tune   [--try '<json list of value sets>'] [--apply '<json>'] [--bake]
//        DONE 9, part 1: capture.frame at a fixed sim time for each candidate Bloom setting
//        (PNGs to --out), or apply one with a note and (with --bake) bake.export it.
//   node tools/live-check/accept.mjs frames --url <page> [--values '<json>'] [--frames 300] --png <file>
//        DONE 9, part 2: opens a page with a stepped clock (performance.now and
//        requestAnimationFrame replaced: exactly 1/60 s per frame), optionally sets params
//        in-page through window.evani.live, steps exactly N frames and screenshots the
//        canvas (overlay and panel hidden). Same N on two builds = same sim tick and camera.
//   node tools/live-check/accept.mjs diff a.png b.png
//        Per-pixel difference of two PNGs (mean abs, rmse, max, changed %, psnr).
//   node tools/live-check/accept.mjs quality --port 8092 --root <engine> --out <dir>
//        Sandbox review: vsync holds, MSAA, load sheds effects then scale to the floor,
//        everything comes back, bloom.composite at High, dev-build download label, log
//        times, command counter, no VR in the tier list.
//   node tools/live-check/accept.mjs session --port 8092 --root <engine> --out <dir> [--no-rebuild]
//        A recorded MCP session (Bloom, tonemap rewrite, rejected broken patch, bake dry run
//        and real bake) to live-session.json/.md, then the ship build rebuilt and compared
//        pixel for pixel with the session. The bake writes into <engine>'s look files.
//   node tools/live-check/accept.mjs talk --port 8097 --root <engine> --out <dir> [--no-rebuild] [--keep-bake]
//        Gate 1 "Talk to the AI" plus the look guard, end to end. Starts its own bridge on
//        --port and plays Evani.App's part with the public TEST key only (a temp APPDATA,
//        never the real Evani.App folder): Bloom, tonemap rewrite, two snapshots A/B
//        compared, a baseline approved, a flagged bake refused until "Chris" approves it, a
//        passing bake, then the ship build rebuilt and compared pixel for pixel. Puts the
//        look files back afterwards unless --keep-bake.
//
// Exit 0 when every check passed, 1 when one failed, 2 when it could not run.

import { spawn, spawnSync } from 'node:child_process';
import { createHash, createHmac, createPrivateKey, createPublicKey, randomBytes, sign as edSign } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

const MODE = process.argv[2];
const args = {};
const pos = [];
for (let i = 3; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) {
    const next = process.argv[i + 1];
    if (next === undefined || next.startsWith('--')) args[a.slice(2)] = true;
    else args[a.slice(2)] = process.argv[++i];
  } else pos.push(a);
}
const ROOT = resolve(args.root ?? fileURLToPath(new URL('../..', import.meta.url)));
const PORT = Number(args.port ?? 8098);
const OUT = resolve(args.out ?? join(ROOT, '.vibedeck', 'screenshots', 'm1-accept'));
const log = (...m) => console.error('[accept]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok: !!ok, detail });
  log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${typeof detail === 'string' ? detail : JSON.stringify(detail)}` : ''}`);
  return !!ok;
};

// ---------- Chrome over CDP (no npm packages) ----------

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

// A stepped clock: rAF callbacks only run when __clock.step() is called, and
// performance.now() advances exactly 1000/60 ms per step. Identical on every build.
const STEPPED_CLOCK = `(() => {
  const realNow = performance.now.bind(performance);
  const t0 = realNow();
  let now = t0; let queue = []; let nextId = 1; let frames = 0;
  performance.now = () => now;
  window.requestAnimationFrame = (cb) => { const id = nextId++; queue.push({ id, cb }); return id; };
  window.cancelAnimationFrame = (id) => { queue = queue.filter((q) => q.id !== id); };
  window.__clock = {
    get frames() { return frames; },
    get pending() { return queue.length; },
    step() { now += 1000 / 60; const q = queue; queue = []; for (const { cb } of q) cb(now); if (q.length) frames++; return q.length; },
  };
  const hide = document.createElement('style');
  hide.textContent = 'body > :not(canvas) { display: none !important; }';
  const put = () => (document.head || document.documentElement).appendChild(hide);
  if (document.documentElement) put(); else document.addEventListener('DOMContentLoaded', put);
})();`;

async function openChrome() {
  const exe = findChrome();
  if (!exe) throw new Error('no Chrome or Edge found (pass --chrome)');
  const profile = mkdtempSync(join(tmpdir(), 'evani-accept-'));
  const chrome = spawn(exe, ['--headless=new', '--mute-audio', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check', '--enable-unsafe-webgpu', '--enable-webgpu-developer-features', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--window-size=1280,720', 'about:blank'], { stdio: 'ignore', windowsHide: true });
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
    if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') consoleLines.push('console.error: ' + m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
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
  const evaluate = async (expression, ms = 120000) => {
    const r = await s('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, ms);
    if (r.exceptionDetails) throw new Error('page: ' + (r.exceptionDetails.exception?.description ?? r.exceptionDetails.text));
    return r.result.value;
  };
  return { s, close, evaluate, consoleLines };
}

// ---------- The bridge: session file, HTTP commands, a /ctl watcher, MCP on stdio ----------

const session = () => JSON.parse(readFileSync(join(ROOT, '.evani', 'live', 'session.json'), 'utf8'));
async function status() {
  try {
    return await (await fetch(`http://127.0.0.1:${PORT}/live/status`, { signal: AbortSignal.timeout(2000) })).json();
  } catch {
    return null;
  }
}
let cmdN = 0;
async function cmd(name, params = {}, note, opts, ms = 120000) {
  const body = { type: 'cmd', v: 1, id: `acc-${++cmdN}`, cmd: name, params, source: 'script', ...(note ? { note } : {}), ...(opts ? { opts } : {}) };
  const r = await fetch(`http://127.0.0.1:${PORT}/live/v1/cmd`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${session().token}` }, body: JSON.stringify(body), signal: AbortSignal.timeout(ms) });
  return r.json();
}
async function must(name, params, note, opts, ms) {
  const v = await cmd(name, params, note, opts, ms);
  if (!v.ok) throw new Error(`${name}: ${v.error?.code}: ${v.error?.message} ${JSON.stringify(v.error?.data ?? '').slice(0, 300)}`);
  return v.result;
}

/** A /ctl client that only listens (like the tab's control client). */
async function ctlWatcher() {
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/ctl`, { headers: { authorization: `Bearer ${session().token}` } });
  const events = [];
  ws.addEventListener('message', (ev) => {
    const m = JSON.parse(ev.data);
    if (m.type === 'event') events.push({ at: performance.now(), event: m.event, data: m.data });
  });
  await new Promise((res, rej) => {
    ws.addEventListener('open', res, { once: true });
    ws.addEventListener('error', () => rej(new Error('/ctl did not open')), { once: true });
  });
  return {
    events,
    async waitFor(pred, ms = 5000) {
      for (const t0 = performance.now(); performance.now() - t0 < ms; await sleep(2)) {
        const e = events.find(pred);
        if (e) return e;
      }
      return null;
    },
    close: () => ws.close(),
  };
}

/** The bridge's MCP server on stdio, as Claude Code runs it. */
function mcpClient() {
  const exe = join(ROOT, 'target', 'debug', 'evani-live-bridge.exe');
  const p = spawn(exe, ['mcp', '--root', ROOT], { stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true });
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
      const r = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'm1-accept', version: '1' } });
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
      return r.result;
    },
    async tools() {
      return (await rpc('tools/list', {})).result.tools;
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

async function openLivePage() {
  const st = await status();
  if (!st || st.phase !== 'idle') throw new Error(`no idle bridge on ${PORT} (start: evani-live-bridge serve --root ${ROOT} --port ${PORT} --game sandbox)`);
  const page = await openChrome();
  await page.s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` });
  for (const t0 = Date.now(); !(await status())?.engine_connected; await sleep(300)) {
    if (Date.now() - t0 > 90000) throw new Error('the page did not connect in 90 s\n' + page.consoleLines.join('\n'));
  }
  await sleep(3000);
  log('live page connected');
  return page;
}

const savePng = (name, b64) => {
  mkdirSync(OUT, { recursive: true });
  const f = join(OUT, name);
  writeFileSync(f, Buffer.from(b64, 'base64'));
  return f;
};

// ---------- PNG decode + diff (RGB, 8-bit, non-interlaced; what Chrome and evani-live write) ----------

function decodePng(file) {
  const b = readFileSync(file);
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
      if (data[8] !== 8 || data[12] !== 0) throw new Error(`${file}: only 8-bit non-interlaced PNGs`);
      ct = data[9];
    } else if (type === 'IDAT') idat.push(data);
    off += 12 + len;
  }
  const bpp = { 2: 3, 6: 4 }[ct];
  if (!bpp) throw new Error(`${file}: colour type ${ct} not supported`);
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

function diffPng(fa, fb) {
  const a = decodePng(fa);
  const b = decodePng(fb);
  if (a.w !== b.w || a.h !== b.h) throw new Error(`sizes differ: ${a.w}x${a.h} vs ${b.w}x${b.h}`);
  let sum = 0, sq = 0, max = 0, changed = 0, over8 = 0;
  for (let i = 0; i < a.w * a.h; i++) {
    let px = 0;
    for (let k = 0; k < 3; k++) {
      const d = Math.abs(a.rgb[i * 3 + k] - b.rgb[i * 3 + k]);
      sum += d;
      sq += d * d;
      if (d > max) max = d;
      if (d > px) px = d;
    }
    if (px > 0) changed++;
    if (px > 8) over8++;
  }
  const n = a.w * a.h * 3;
  const rmse = Math.sqrt(sq / n);
  return { width: a.w, height: a.h, mean_abs: +(sum / n).toFixed(4), rmse: +rmse.toFixed(4), max, changed_pct: +((100 * changed) / (a.w * a.h)).toFixed(3), over8_pct: +((100 * over8) / (a.w * a.h)).toFixed(3), psnr_db: rmse === 0 ? Infinity : +(20 * Math.log10(255 / rmse)).toFixed(2) };
}

// ---------- Modes ----------

const AI_NOTE = (t) => `[Evani.dev] acceptance: ${t}`;

async function modeAi() {
  const page = await openLivePage();
  const ctl = await ctlWatcher();
  const mcp = mcpClient();
  try {
    const info = await mcp.init();
    const tools = (await mcp.tools()).map((t) => t.name);
    check('MCP server answers on stdio and lists the live tools', tools.includes('param_set') && tools.includes('slot_set') && tools.includes('bake_export'), `${info?.serverInfo?.name ?? '?'}: ${tools.length} tools`);
    const st = await mcp.call('live_status', {});
    check('live_status: game connected', !st.isError && st.json?.engine_connected, st.text[0]?.slice(0, 160));

    // Each change: send over MCP, time the session.mirror at the /ctl watcher.
    const timed = async (label, tool, a) => {
      const before = await must('session.status', {});
      const head0 = before.head;
      const t0 = performance.now();
      const pr = mcp.call(tool, a);
      const ev = await ctl.waitFor((e) => e.event === 'session.mirror' && e.at >= t0 && e.data?.head > head0, 15000);
      const r = await pr;
      const tx = r.json?.tx;
      const entry = ev?.data?.history?.find((h) => h.tx === tx);
      const mirrorMs = ev ? +(ev.at - t0).toFixed(1) : null;
      const replyMs = +(performance.now() - t0).toFixed(1);
      const frameMs = r.json?.telemetry?.frame_ms?.avg ?? r.json?.telemetry?.baseline_frame_ms ?? 16.7;
      const row = { tool, tx, applied_frame: r.json?.frame, mirror_frame: entry?.frame, mirror_ms_after_send: mirrorMs, frame_ms: frameMs, reply_ms: replyMs, summary: entry?.summary, compile: r.json?.compile ?? null, applied: r.json?.applied, error: r.isError ? r.text.join(' ').slice(0, 300) : undefined, image: r.images.length ? savePng(`ai-${label}.png`, r.images[0].data) : null };
      check(`AI ${label}: applied and mirrored to the /ctl client`, !r.isError && !!ev && entry?.frame === r.json?.frame, row);
      return row;
    };

    const rows = [];
    const intensity = (await must('param.get', { names: ['fx.bloom.intensity'] })).values['fx.bloom.intensity'];
    rows.push(await timed('bloom-intensity', 'param_set', { values: { 'fx.bloom.intensity': 0.35 }, note: AI_NOTE('Bloom softer so the image is not washed out'), capture_width: 480 }));
    rows.push(await timed('bloom-off', 'effect_toggle', { effect: 'bloom', on: false, note: AI_NOTE('Bloom off to compare'), capture_width: 480 }));
    rows.push(await timed('bloom-on', 'effect_toggle', { effect: 'bloom', on: true, note: AI_NOTE('Bloom back on'), capture_width: 480 }));
    const g = await mcp.call('slot_get', { name: 'tonemap' });
    const oldBody = g.json?.source;
    const aces = 'let x = color * 0.9;\nlet a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;\nreturn clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));';
    rows.push(await timed('tonemap-rewrite', 'slot_set', { name: 'tonemap', source: aces, note: AI_NOTE('ACES-style filmic tonemap for more highlight contrast'), capture_width: 480 }));
    const after = await mcp.call('slot_get', { name: 'tonemap' });
    check('the tonemap slot now holds the AI body', after.json?.source?.trim() === aces.trim(), `hash ${g.json?.hash} -> ${after.json?.hash}`);

    const bake = await mcp.call('bake_export', { dry_run: true, title: 'M1 acceptance (dry run)' });
    const plan = bake.json?.written?.map((w) => `${w.action} ${w.path}`) ?? [];
    check('AI bake (dry run) plans the slot and Bloom into source files, writes nothing', !bake.isError && bake.json?.dry_run === true && plan.some((p) => p.includes('slots/tonemap.wgsl')) && plan.some((p) => p.includes('params.json')), plan.join(', ') || bake.text.join(' ').slice(0, 300));

    // Put everything back.
    const head = (await must('session.status', {})).head;
    await mcp.call('history_undo', { steps: head });
    const back = await must('slot.get', { name: 'tonemap' });
    const bi = (await must('param.get', { names: ['fx.bloom.intensity'] })).values['fx.bloom.intensity'];
    check('undo puts the session back', back.source === oldBody && bi === intensity, `head ${head} -> ${(await must('session.status', {})).head}`);
    return { rows, bake_plan: plan };
  } finally {
    mcp.close();
    ctl.close();
    await page.close();
  }
}

async function modeSafety() {
  const page = await openLivePage();
  const ctl = await ctlWatcher();
  const out = {};
  try {
    const head0 = (await must('session.status', {})).head;
    // 1. Broken WGSL is rejected with the compiler message; the old shader keeps running.
    const g = await must('slot.get', { name: 'tonemap' });
    const bad = await cmd('slot.set', { name: 'tonemap', source: 'let y = color * undefined_thing;\nreturn y;' }, 'acceptance: a broken patch');
    const keep = await must('slot.get', { name: 'tonemap' });
    const cap = await must('capture.frame', { width: 320 });
    savePng('safety-broken-slot-kept.png', cap.png_base64);
    out.broken = { code: bad.error?.code, kept: bad.error?.data?.kept, messages: bad.error?.data?.messages?.slice(0, 2) };
    check('broken WGSL rejected with the compiler message, old shader kept', !bad.ok && bad.error.code === 'compile_error' && keep.hash === g.hash && (bad.error.data?.messages ?? []).length > 0, out.broken);

    // 2. A change that pushes frame time > 25 % over target is auto-reverted.
    const target = (await must('session.status', {})).watch?.target_ms;
    let iters = Number(args.iters ?? 600);
    const heavy = (n) => `var acc = color;\nfor (var i = 0; i < ${n}; i = i + 1) { acc = sin(acc * 1.0001 + vec3<f32>(f32(i) * 0.0001)); }\nreturn clamp(color + acc * 0.000001, vec3<f32>(0.0), vec3<f32>(1.0));`;
    let rev = null;
    for (let attempt = 0; attempt < 4 && !rev; attempt++) {
      const t0 = performance.now();
      const r = await cmd('slot.set', { name: 'tonemap', source: heavy(iters) }, `acceptance: a deliberately heavy tonemap (${iters} iterations)`, { telemetry_ms: 0 });
      if (!r.ok) {
        out.heavy_error = r.error;
        break;
      }
      const applied = performance.now();
      const ev = await ctl.waitFor((e) => e.event === 'auto_revert' && e.at >= t0, 12000);
      const tel = ev ? null : await must('session.status', {});
      if (ev) {
        const after = await must('slot.get', { name: 'tonemap' });
        rev = { iters, tx: r.result.tx, watch: r.result.watch, seconds_after_apply: +((ev.at - applied) / 1000).toFixed(2), event: ev.data, restored: after.hash === g.hash };
      } else {
        log(`no auto-revert at ${iters} iterations (watch ${JSON.stringify(tel.watch)}); undoing and trying heavier`);
        await must('history.undo', { steps: 1 });
        await sleep(1500);
        iters *= 3;
      }
    }
    out.auto_revert = rev;
    check('over-budget change auto-reverted (restored the old shader)', !!rev && rev.event.reason === 'frame_time' && rev.restored && rev.seconds_after_apply <= 4.0, rev ?? out.heavy_error ?? 'no revert');
    await sleep(2000);

    // 3. Undo/redo across 20 changes returns exactly to each state.
    const names = ['fx.bloom.intensity', 'fx.bloom.threshold', 'fx.bloom.knee', 'render.exposure', 'render.saturation', 'fx.vignette.intensity'];
    const known = (await must('param.get', { names })).values;
    const use = names.filter((n) => known[n] !== undefined);
    const state = async () => JSON.stringify({ p: (await must('param.get', {})).values, s: (await must('slot.get', { name: 'tonemap' })).hash });
    const states = [await state()];
    const headStart = (await must('session.status', {})).head;
    for (let i = 0; i < 20; i++) {
      const name = use[i % use.length];
      const v = +(Number(known[name]) * (0.8 + 0.02 * i) + 0.01 * (i + 1)).toFixed(3);
      await must('param.set', { values: { [name]: v } }, `acceptance undo test ${i + 1}: ${name}`, { telemetry_ms: 0 });
      states.push(await state());
    }
    let undoOk = 0, redoOk = 0;
    for (let i = 20; i > 0; i--) {
      await must('history.undo', { steps: 1 }, undefined, { telemetry_ms: 0 });
      if ((await state()) === states[i - 1]) undoOk++;
    }
    for (let i = 1; i <= 20; i++) {
      await must('history.redo', { steps: 1 }, undefined, { telemetry_ms: 0 });
      if ((await state()) === states[i]) redoOk++;
    }
    out.undo_redo = { changes: 20, params: use, undo_exact: undoOk, redo_exact: redoOk, distinct_states: new Set(states).size, head_start: headStart, head_end: (await must('session.status', {})).head };
    check('undo/redo across 20 changes returns exactly to each state', undoOk === 20 && redoOk === 20 && new Set(states).size === 21, out.undo_redo);
    const steps = (await must('session.status', {})).head - head0;
    if (steps > 0) await must('history.undo', { steps }, undefined, { telemetry_ms: 0 });
    return out;
  } finally {
    ctl.close();
    await page.close();
  }
}

async function modeTune() {
  const page = await openLivePage();
  try {
    const T = Number(args.t ?? 6);
    const shots = [];
    if (args.try) {
      const sets = JSON.parse(args.try);
      for (const [i, values] of sets.entries()) {
        if (Object.keys(values).length) await must('param.set', { values }, `tune candidate ${i}`, { telemetry_ms: 0 });
        await sleep(300);
        const c = await must('capture.frame', { width: Number(args.width ?? 640), sim_time: T, camera: 'path', hide_overlay: true });
        const on = (await must('param.get', { names: ['fx.bloom.enabled'] })).values['fx.bloom.enabled'];
        shots.push({ i, values, bloom_on: on, file: savePng(`tune-${i}.png`, c.png_base64), gpu: c.stats?.gpu_ms?.passes });
        if (Object.keys(values).length) await must('history.undo', { steps: 1 }, undefined, { telemetry_ms: 0 });
      }
    }
    let bake = null;
    if (args.apply) {
      const values = JSON.parse(args.apply);
      const r = await must('param.set', { values }, args.note ?? 'Bloom tuned so it no longer washes the image out', { telemetry_ms: 1000 });
      const c = await must('capture.frame', { width: Number(args.width ?? 640), sim_time: T, camera: 'path', hide_overlay: true });
      shots.push({ applied: r.applied, file: savePng('tuned.png', c.png_base64), frame_ms: r.telemetry?.frame_ms });
      if (args.bake) {
        bake = await must('bake.export', { title: args.title ?? 'M1 acceptance: Bloom tuned' });
        check('bake.export wrote the tuned values', !bake.dry_run && bake.written.length > 0, bake.written.map((w) => `${w.action} ${w.path}`).join(', '));
      }
    }
    const history = args.history ? (await must('history.list', { limit: 50 })) : undefined;
    return { shots, bake, history };
  } finally {
    await page.close();
  }
}

async function modeFrames() {
  return shootFrames({ url: args.url, N: Number(args.frames ?? 300), values: args.values ? JSON.parse(args.values) : null, png: args.png });
}

/** Open `url` with the stepped clock; at frame 30 set `values` (params) and `slots` ({name: wgsl})
 *  in-page through window.evani.live; step to frame N and screenshot. */
async function shootFrames({ url, N, values, slots, png }) {
  const page = await openChrome();
  try {
    await page.s('Page.addScriptToEvaluateOnNewDocument', { source: STEPPED_CLOCK });
    await page.s('Page.navigate', { url });
    // Wait until the engine has asked for its first frame.
    for (const t0 = Date.now(); !(await page.evaluate('window.__clock ? window.__clock.pending : 0')); await sleep(100)) {
      if (Date.now() - t0 > 60000) throw new Error('the page never requested a frame\n' + page.consoleLines.join('\n'));
    }
    const stepTo = (n) => page.evaluate(`(async () => { while (__clock.frames < ${n}) { if (!__clock.step()) await new Promise((r) => setTimeout(r, 5)); await new Promise((r) => setTimeout(r, 4)); } return __clock.frames; })()`, 600000);
    let applied = null;
    // In-page live API (the same commands the bridge carries). The reply waits for frames, so step while waiting.
    const live = (c, p) =>
      page.evaluate(`(async () => {
        const p = window.evani.live.call(${JSON.stringify(c)}, ${JSON.stringify(p)}, { note: 'replay of the live session', telemetry_ms: 0 });
        let done = null; p.then((r) => (done = { ok: true, r }), (e) => (done = { ok: false, e: String(e) }));
        while (!done) { __clock.step(); await new Promise((r) => setTimeout(r, 8)); }
        return done.ok ? { frame: done.r.frame, applied: (done.r.applied || []).map((a) => [a.name, a.new]), hash: done.r.hash } : done;
      })()`);
    if (values || slots) {
      await stepTo(30);
      applied = [];
      if (values) applied.push(await live('param.set', { values }));
      for (const [name, source] of Object.entries(slots ?? {})) applied.push(await live('slot.set', { name, source }));
      if (applied.some((a) => a.ok === false)) throw new Error(`in-page replay failed: ${JSON.stringify(applied)}`);
    }
    const got = await stepTo(N);
    await sleep(500);
    const shot = await page.s('Page.captureScreenshot', { format: 'png' });
    const file = resolve(png ?? join(OUT, 'frames.png'));
    mkdirSync(resolve(file, '..'), { recursive: true });
    writeFileSync(file, Buffer.from(shot.data, 'base64'));
    const hasLive = await page.evaluate('!!(window.evani && window.evani.live)');
    return { url, frames: got, file, live_api_in_page: hasLive, applied, console: page.consoleLines.slice(0, 10) };
  } finally {
    await page.close();
  }
}

// Run a JSON list of [cmd, params, note?] on a fresh live page; print each reply (trimmed).
async function modeProbe() {
  const page = await openLivePage();
  try {
    const out = [];
    for (const [c, p, note] of JSON.parse(args.cmds)) {
      if (c === 'sleep') {
        await sleep(p);
        continue;
      }
      const r = await cmd(c, p ?? {}, note, { telemetry_ms: 0 });
      out.push({ cmd: c, params: p, ok: r.ok, result: r.ok ? r.result : r.error });
    }
    return out;
  } finally {
    await page.close();
  }
}

// A plain screenshot of a page after --wait ms (overlay and panel visible). With --nogpu the
// page sees a browser without WebGPU (navigator.gpu removed before any script runs).
async function modeShot() {
  const page = await openChrome();
  try {
    if (args.nogpu) await page.s('Page.addScriptToEvaluateOnNewDocument', { source: "try { Object.defineProperty(Navigator.prototype, 'gpu', { get: () => undefined, configurable: true }); } catch (e) {}" });
    await page.s('Page.navigate', { url: args.url });
    await sleep(Number(args.wait ?? 4000));
    const shot = await page.s('Page.captureScreenshot', { format: 'png' });
    const file = resolve(args.png ?? join(OUT, 'shot.png'));
    mkdirSync(resolve(file, '..'), { recursive: true });
    writeFileSync(file, Buffer.from(shot.data, 'base64'));
    const text = await page.evaluate('document.body ? document.body.innerText.slice(0, 600) : ""');
    return { url: args.url, nogpu: !!args.nogpu, has_gpu: await page.evaluate('!!navigator.gpu'), file, text, console: page.consoleLines.slice(0, 10) };
  } finally {
    await page.close();
  }
}

// ---------- Sandbox review: adaptive quality, MSAA, the overlay's command counter ----------

// What the page's overlay says (the same text a player sees), parsed.
function parseOverlay(text) {
  const num = (re, d = null) => {
    const m = text.match(re);
    return m ? Number(m[1]) : d;
  };
  const work = text.match(/work ([\d.]+)\/([\d.]+) ms/);
  const fxLine = text.split('\n').find((l) => /^fx: /.test(l)) ?? '';
  const effects = {};
  for (const part of fxLine.replace(/^fx: /, '').split(' · ')) {
    const m = part.match(/^([a-z_]+) (.+)$/);
    if (m && !/pts$/.test(part)) effects[m[1]] = m[2];
  }
  const paused = Object.entries(effects).filter(([, s]) => s.startsWith('on, paused')).map(([n]) => n);
  // `passes  forward 0.36  bloom.composite 0.17 ...` (GPU ms per pass).
  const passes = {};
  const passLine = text.split('\n').find((l) => /^passes /.test(l)) ?? '';
  for (const m of passLine.matchAll(/([a-z_.]+) (-?[\d.]+)/g)) passes[m[1]] = Number(m[2]);
  const lines = text.split('\n');
  return {
    passes,
    download: lines.find((l) => /^download /.test(l)) ?? null,
    // Log entries start "! " (error) or "· " (note).
    log: lines.filter((l) => /^[!·] /.test(l)),
    settling: /quality settling/.test(text),
    tier: text.match(/tier (\S+)/)?.[1] ?? null,
    fps: num(/fps ([\d.]+)/),
    work_ms: work ? Number(work[1]) : null,
    target_ms: work ? Number(work[2]) : null,
    scale: num(/scale x([\d.]+)/, 1),
    msaa: num(/msaa (\d+)x/, 1),
    commands: num(/(\d+) commands/),
    effects,
    paused,
    held: /quality holding/.test(text),
  };
}

// `accept.mjs quality --port 8092 --out <dir>`: Chris's sandbox review, notes 1 to 5.
async function modeQuality() {
  const page = await openLivePage();
  const ctl = await ctlWatcher();
  const out = { vsync: {}, load: {} };
  const overlay = () => page.evaluate("(document.getElementById('evani-overlay') || {}).innerText || ''");
  const snap = async () => parseOverlay(await overlay());
  const shot = async (name, clip) => savePng(name, (await page.s('Page.captureScreenshot', { format: 'png', ...(clip ? { clip } : {}) })).data);
  // A 160 x 90 patch of the spinner grid's edges, 4x enlarged.
  const ZOOM = { x: 560, y: 300, width: 160, height: 90, scale: 4 };
  try {
    out.commands_at_start = (await snap()).commands;

    // 1. At vsync, Medium and High hold 60 fps without cutting effects or scale.
    for (const tier of ['medium', 'high']) {
      await must('tier.set', { tier }, `review: ${tier} at vsync`, { telemetry_ms: 0 });
      await sleep(5000); // pipelines rebuilt, tier warm-up over
      const samples = [];
      for (let i = 0; i < 24; i++) {
        samples.push(await snap());
        await sleep(500);
      }
      const cut = samples.filter((s) => s.paused.length || s.scale < 1);
      const works = samples.map((s) => s.work_ms).filter((w) => w !== null);
      out.vsync[tier] = {
        samples: samples.length,
        fps: [Math.min(...samples.map((s) => s.fps)), Math.max(...samples.map((s) => s.fps))],
        work_ms: [Math.min(...works), Math.max(...works)],
        target_ms: samples.at(-1).target_ms,
        effects: samples.at(-1).effects,
        msaa: samples.at(-1).msaa,
        cut_samples: cut.length,
        file: await shot(`${tier}-vsync.png`),
      };
      check(`${tier} at vsync: no effect or scale cuts over 12 s`, cut.length === 0 && samples.every((s) => s.tier === tier), out.vsync[tier]);
      if (tier === 'medium') {
        out.vsync.medium.zoom = await shot('medium-msaa-zoom.png', ZOOM);
        check('MSAA renders at Medium (overlay "msaa 4x" on every sample)', samples.every((s) => s.msaa === 4), { msaa: samples.map((s) => s.msaa) });
      }
    }
    await must('tier.set', { tier: 'low' }, 'review: Low for contrast (no MSAA)', { telemetry_ms: 0 });
    await sleep(4000);
    const low = await snap();
    out.low = { msaa: low.msaa, scale_note: 'Low renders at its own lower scale', zoom: await shot('low-no-msaa-zoom.png', ZOOM) };
    check('Low draws one sample (no "msaa" in the overlay)', low.msaa === 1, out.low);

    // 2. Under real GPU load at High: effects are paused first, then the render scale
    //    steps down 0.05 at a time to the 80 % floor, never below.
    await must('tier.set', { tier: 'high' }, 'review: High for the load test', { telemetry_ms: 0 });
    await sleep(5000);
    const before = await snap();
    const before_hash = (await must('slot.get', { name: 'tonemap' })).hash;
    const onBefore = Object.entries(before.effects).filter(([, s]) => s === 'on').map(([n]) => n);
    await must('param.set', { values: { 'live.revert.over_pct': 200, 'live.revert.sustain_s': 30 } }, 'review: let a deliberate load run (auto-revert would undo it)', { telemetry_ms: 0 });
    const heavy = (n) => `var acc = color;\nfor (var i = 0; i < ${n}; i = i + 1) { acc = sin(acc * 1.0001 + vec3<f32>(f32(i) * 0.0001)); }\nreturn clamp(color + acc * 0.000001, vec3<f32>(0.0), vec3<f32>(1.0));`;
    let iters = Number(args.iters ?? 12000);
    const timeline = [];
    const t0 = performance.now();
    let key = '';
    const files = [];
    const record = async () => {
      const s = await snap();
      const k = `${s.paused.join(',')}|${s.scale.toFixed(2)}`;
      if (k !== key) {
        key = k;
        const row = { t_s: +((performance.now() - t0) / 1000).toFixed(2), paused: s.paused, scale: s.scale, work_ms: s.work_ms, target_ms: s.target_ms, fps: s.fps, iters };
        const label = s.scale < 1 ? `scale-${s.scale.toFixed(2)}` : s.paused.length ? `paused-${s.paused.join('-')}` : 'full';
        row.file = await shot(`load-${String(timeline.length).padStart(2, '0')}-${label}.png`);
        files.push(row.file);
        timeline.push(row);
        log(`load t=${row.t_s}s paused [${row.paused}] scale ${row.scale} work ${row.work_ms}/${row.target_ms}`);
      }
      return s;
    };
    await record();
    let r = await must('slot.set', { name: 'tonemap', source: heavy(iters) }, `review: deliberate GPU load (${iters} iterations in tonemap)`, { telemetry_ms: 0 });
    let slotTx = [r.tx];
    let floorSince = null;
    for (const tEnd = performance.now() + 60000; performance.now() < tEnd; await sleep(150)) {
      const s = await record();
      // Too light to push past the target even before cuts: make it heavier once.
      if (performance.now() - t0 > 6000 && !timeline.some((x) => x.paused.length || x.scale < 1) && iters < 100000) {
        iters *= 3;
        r = await must('slot.set', { name: 'tonemap', source: heavy(iters) }, `review: heavier GPU load (${iters} iterations)`, { telemetry_ms: 0 });
        slotTx.push(r.tx);
      }
      if (s.scale <= 0.8001) floorSince ??= performance.now();
      if (floorSince && performance.now() - floorSince > 4000) break;
    }
    const final = await snap();
    out.load = { effects_on_before: onBefore, iters, timeline, final: { paused: final.paused, scale: final.scale, work_ms: final.work_ms }, auto_reverts: ctl.events.filter((e) => e.event === 'auto_revert').map((e) => e.data) };
    const firstScale = timeline.findIndex((x) => x.scale < 1);
    const scales = [...new Set(timeline.map((x) => x.scale.toFixed(2)))].map(Number);
    const steps = scales.slice(1).map((v, i) => +(scales[i] - v).toFixed(3));
    check('under load every effect that was on is paused before the scale moves', firstScale > 0 && onBefore.every((n) => timeline[firstScale].paused.includes(n)) && timeline.slice(0, firstScale).every((x) => x.scale === 1), { firstScale, onBefore, at: timeline[firstScale] });
    check('scale steps down 0.05 at a time', steps.length >= 1 && steps.every((d) => Math.abs(d - 0.05) < 0.001), { scales, steps });
    check('scale stops at the 80 % floor and holds there', Math.min(...scales) === 0.8 && final.scale === 0.8, { min: Math.min(...scales), final: final.scale });
    check('no auto-revert during the load test', out.load.auto_reverts.length === 0, out.load.auto_reverts);

    // Take the load off again.
    out.undone = [];
    for (let i = 0; i < slotTx.length + 1; i++) out.undone.push((await must('history.undo', { steps: 1 })).applied);
    out.slot_after_undo = (await must('slot.get', { name: 'tonemap' })).hash;
    out.slot_before_load = before_hash;
    // Recovery runs in reverse (scale up first, then effects back), one step per upgrade window.
    const tUndo = performance.now();
    let rec = await snap();
    for (const tEnd = performance.now() + 40000; performance.now() < tEnd && (rec.scale < 1 || rec.paused.length); await sleep(500)) rec = await snap();
    out.after_load = { paused: rec.paused, scale: rec.scale, work_ms: rec.work_ms, file: await shot('after-load-recovered.png') };
    check('after the load is undone, quality recovers fully', out.slot_after_undo === before_hash && rec.scale === 1 && rec.paused.length === 0, out.after_load);
    out.after_load.recovered_s = +((performance.now() - tUndo) / 1000).toFixed(1);

    // 4. With bloom running again at High: bloom.composite GPU ms, averaged over a few seconds
    //    (the sandbox review measured 0.92 ms before the fixes).
    const comp = [];
    const bloomPasses = [];
    for (let i = 0; i < 16; i++) {
      const s = await snap();
      if (s.effects.bloom === 'on' && s.passes['bloom.composite'] !== undefined) {
        comp.push(s.passes['bloom.composite']);
        bloomPasses.push(s.passes);
      }
      await sleep(400);
    }
    const avg = (xs) => (xs.length ? +(xs.reduce((a, b) => a + b, 0) / xs.length).toFixed(3) : null);
    out.bloom_composite = { samples: comp.length, avg_ms: avg(comp), min_ms: comp.length ? Math.min(...comp) : null, max_ms: comp.length ? Math.max(...comp) : null, old_ms: 0.92, last_passes: bloomPasses.at(-1) ?? null };
    check('bloom runs at High again and bloom.composite is measured', comp.length >= 8, out.bloom_composite);

    // 5a. The dev build labels its download with the published size.
    const ov5 = await snap();
    out.download_line = ov5.download;
    check('the download line says dev build and shows the published size', /\(dev build; published ~[\d.]+ (KB|MB) gzip\)/.test(ov5.download ?? ''), ov5.download);
    // 5b. Log lines carry a time (m:ss.s since the page started).
    out.log_lines = ov5.log;
    check('every overlay log line starts with a time', ov5.log.length > 0 && ov5.log.every((l) => /^[!·] \d+:\d{2}\.\d /.test(l)), ov5.log.slice(-4));

    // 3. The overlay's live command counter counts what it handled.
    const end = await snap();
    out.commands_at_end = end.commands;
    out.overlay_text = await overlay();
    out.overlay_file = await shot('overlay-commands.png');
    check('the overlay command counter goes above 0 after a few commands', end.commands > 0 && end.commands > (out.commands_at_start ?? 0), { start: out.commands_at_start, end: end.commands });

    // 5c. In a normal tab (no XR session) the tier list has no VR. Open the list in place
    //     (size = option count) so the screenshot shows every option.
    const sel = await page.evaluate(`(() => {
      const s = document.querySelector('#evani-panel select');
      if (!s) return null;
      s.setAttribute('size', String(s.options.length));
      const r = document.getElementById('evani-panel').getBoundingClientRect();
      return { options: [...s.options].map((o) => o.value), labels: [...s.options].map((o) => o.text), rect: { x: r.x, y: r.y, width: r.width, height: r.height } };
    })()`);
    await sleep(300);
    out.tier_list = sel && { options: sel.options, labels: sel.labels, file: await shot('tier-list-no-vr.png', { x: Math.max(0, sel.rect.x - 8), y: 0, width: sel.rect.width + 8, height: sel.rect.y + sel.rect.height + 8, scale: 2 }) };
    check('the tier list in a normal tab has no VR option', !!sel && sel.options.length >= 4 && !sel.options.some((o) => /vr/i.test(o)), out.tier_list);
    await page.evaluate("(() => { const s = document.querySelector('#evani-panel select'); if (s) s.removeAttribute('size'); })()");
    out.console = page.consoleLines.slice(0, 10);
    writeFileSync(join(OUT, 'quality.json'), JSON.stringify(out, null, 2));
    return out;
  } finally {
    ctl.close();
    await page.close();
  }
}

// A static file server for a ship build's dist folder (no npm packages). Returns its port.
async function serveDir(dir) {
  const { createServer } = await import('node:http');
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.png': 'image/png', '.json': 'application/json' };
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    const file = join(dir, path === '/' ? 'index.html' : path);
    if (!file.startsWith(dir) || !existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': types[file.slice(file.lastIndexOf('.'))] ?? 'application/octet-stream' }).end(readFileSync(file));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { port: server.address().port, close: () => server.close() };
}

// `accept.mjs session --port 8092 --root <engine> --out <dir>`: Chris's follow-up 3 (Stage 0 gate).
// One recorded live session through the bridge's MCP server, the way the AI drives it: Bloom
// change, tonemap rewrite, a deliberately broken patch (rejected, old shader keeps running),
// bake dry run, real bake. Every command is logged with its time, reply and the overlay's
// command count to live-session.json and live-session.md. Then the ship build (live off) is
// rebuilt from the baked sources and compared pixel for pixel with the live session replayed on
// the pre-bake live build (stepped clock, frame --frames, as in M1 DONE 9). --no-rebuild skips
// the rebuild and compares against the ship build already in target/evani-sizes/sandbox-ship.
async function modeSession() {
  const page = await openLivePage();
  const mcp = mcpClient();
  const overlay = async () => parseOverlay(await page.evaluate("(document.getElementById('evani-overlay') || {}).innerText || ''"));
  // Frames the page has drawn (a rAF counter of its own, next to the engine's).
  await page.evaluate('(() => { window.__rafN = 0; const f = () => { window.__rafN++; requestAnimationFrame(f); }; requestAnimationFrame(f); })()');
  const rafN = () => page.evaluate('window.__rafN');
  const started = Date.now();
  const log = [];
  const trim = (v) => JSON.parse(JSON.stringify(v ?? null, (k, x) => (typeof x === 'string' && x.length > 400 ? `${x.slice(0, 400)}... (${x.length} chars)` : x)));
  const call = async (label, tool, a) => {
    const at = new Date();
    const t = performance.now();
    const r = await mcp.call(tool, a);
    const ms = +(performance.now() - t).toFixed(1);
    await sleep(500); // the overlay redraws its text a few times a second
    const ov = await overlay();
    const row = { step: log.length + 1, label, at: at.toISOString(), t_s: +((at - started) / 1000).toFixed(2), tool, args: trim(a), reply_ms: ms, ok: !r.isError, reply: trim(r.json ?? r.text.join('\n')), overlay_commands: ov.commands, image: r.images.length ? savePng(`session-${String(log.length + 1).padStart(2, '0')}-${label}.png`, r.images[0].data) : null };
    log.push(row);
    console.error(`[accept] ${row.at} ${tool} ${label}: ${row.ok ? 'ok' : 'error'} in ${ms} ms, overlay ${ov.commands} commands`);
    return { r, row };
  };
  const git = (...a) => spawnSync('git', ['-C', ROOT, ...a], { encoding: 'utf8' }).stdout.trim();
  const out = {};
  try {
    await mcp.init();
    const st = await call('status', 'live_status', {});
    check('session: game connected over MCP', st.row.ok && st.r.json?.engine_connected, st.r.text[0]?.slice(0, 120));
    const params0 = (await call('bloom-before', 'param_get', { names: ['fx.bloom.intensity', 'fx.bloom.threshold'] })).r.json;

    // 1. Bloom.
    const bloomValues = { 'fx.bloom.intensity': 0.4 };
    const b = await call('bloom', 'param_set', { values: bloomValues, note: AI_NOTE('Bloom a little stronger so the glow reads at a distance'), capture_width: 480 });
    check('session: Bloom change applied', b.row.ok && (b.r.json?.applied ?? []).some((x) => x.name === 'fx.bloom.intensity'), b.row.reply?.applied);

    // 2. Tonemap rewrite.
    const g0 = (await call('tonemap-before', 'slot_get', { name: 'tonemap' })).r.json;
    const aces = 'let x = color * 0.9;\nlet a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;\nreturn clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));';
    const tm = await call('tonemap', 'slot_set', { name: 'tonemap', source: aces, note: AI_NOTE('ACES-style filmic tonemap for more highlight contrast'), capture_width: 480 });
    const g1 = (await call('tonemap-after', 'slot_get', { name: 'tonemap' })).r.json;
    check('session: tonemap rewritten (new hash)', tm.row.ok && g1?.source?.trim() === aces.trim() && g1?.hash !== g0?.hash, { before: g0?.hash, after: g1?.hash });

    // 3. A deliberately broken patch: rejected, the old (rewritten) shader keeps running.
    const n0 = await rafN();
    const bad = await call('broken-patch', 'slot_set', { name: 'tonemap', source: 'let y = color * undefined_thing;\nreturn y;', note: AI_NOTE('a deliberately broken patch (must be rejected)') });
    const g2 = (await call('tonemap-kept', 'slot_get', { name: 'tonemap' })).r.json;
    const tel = await call('frames-after-broken', 'telemetry_sample', { seconds: 2 });
    const n1 = await rafN();
    const cap = await call('capture-after-broken', 'capture_frame', { width: 480 });
    out.broken = { rejected: bad.r.isError, message: bad.r.text.join(' ').slice(0, 300), hash_before: g1?.hash, hash_after: g2?.hash, page_frames_during: n1 - n0, telemetry: tel.row.reply };
    check('session: broken patch rejected, same shader hash, frames still advancing', bad.r.isError && /undefined_thing|compile/i.test(out.broken.message) && g2?.hash === g1?.hash && n1 - n0 > 60 && !!cap.row.image, out.broken);

    // 4. Bake: dry run writes nothing, then the real bake.
    const dirty0 = git('status', '--porcelain');
    const dry = await call('bake-dry-run', 'bake_export', { dry_run: true, title: 'Sandbox review live session (dry run)' });
    const plan = dry.r.json?.written?.map((w) => `${w.action} ${w.path}`) ?? [];
    const dirty1 = git('status', '--porcelain');
    check('session: bake dry run plans Bloom and the slot, writes nothing', dry.row.ok && dry.r.json?.dry_run === true && plan.some((p) => p.includes('slots/tonemap.wgsl')) && plan.some((p) => p.includes('params.json')) && dirty0 === dirty1, plan);
    const real = await call('bake', 'bake_export', { title: 'Sandbox review live session' });
    const wrote = real.r.json?.written?.map((w) => `${w.action} ${w.path}`) ?? [];
    out.bake_diff_stat = git('diff', '--stat');
    writeFileSync(join(OUT, 'live-session-bake.diff'), spawnSync('git', ['-C', ROOT, 'diff'], { encoding: 'utf8' }).stdout);
    check('session: real bake wrote the files', real.row.ok && real.r.json?.dry_run === false && wrote.length > 0 && out.bake_diff_stat.length > 0, { wrote, diff: out.bake_diff_stat });
    const counts = log.map((x) => x.overlay_commands);
    check('session: the overlay command count rises with the commands', counts.at(-1) > counts[0], counts);
    out.params_before = params0;
    out.slot = { before: g0?.hash, after: g1?.hash };
    out.values = bloomValues;
  } finally {
    mcp.close();
    await page.close();
  }

  // 5. Rebuild without live from the baked sources and compare pixels with the live session.
  const N = Number(args.frames ?? 360);
  if (!args['no-rebuild']) {
    log.push({ step: log.length + 1, label: 'rebuild-ship', at: new Date().toISOString(), tool: 'cargo xtask size-report sandbox (ship build, live off)' });
    const t = Date.now();
    // Run this script at below-normal priority on a shared PC; cargo inherits it.
    const b = spawnSync('cargo', ['xtask', 'size-report', 'sandbox'], { cwd: ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 64 << 20 });
    log.at(-1).reply = { exit: b.status, seconds: Math.round((Date.now() - t) / 1000), tail: (b.stdout + b.stderr).trim().split('\n').slice(-6) };
    if (b.status !== 0) throw new Error(`ship rebuild failed: ${b.stdout}\n${b.stderr}`);
  }
  const shipDir = join(ROOT, 'target', 'evani-sizes', 'sandbox-ship');
  const srv = await serveDir(shipDir);
  try {
    const aces = 'let x = color * 0.9;\nlet a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;\nreturn clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));';
    const liveShot = await shootFrames({ url: `http://127.0.0.1:${PORT}/`, N, values: out.values, slots: { tonemap: aces }, png: join(OUT, `session-live-f${N}.png`) });
    const shipShot = await shootFrames({ url: `http://127.0.0.1:${srv.port}/`, N, png: join(OUT, `session-ship-f${N}.png`) });
    const untuned = await shootFrames({ url: `http://127.0.0.1:${PORT}/`, N, png: join(OUT, `session-live-unchanged-f${N}.png`) });
    out.compare = { frame: N, live: liveShot.file, ship: shipShot.file, live_api_in_ship: shipShot.live_api_in_page, replay: liveShot.applied, live_vs_ship: diffPng(liveShot.file, shipShot.file), sensitivity_unchanged_vs_ship: diffPng(untuned.file, shipShot.file) };
    log.push({ step: log.length + 1, label: 'pixel-compare', at: new Date().toISOString(), tool: 'stepped clock, frame ' + N, reply: { live_vs_ship: out.compare.live_vs_ship, sensitivity: out.compare.sensitivity_unchanged_vs_ship } });
    check('session: rebuild without live is pixel-identical to the live session', out.compare.live_api_in_ship === false && out.compare.live_vs_ship.max === 0 && out.compare.sensitivity_unchanged_vs_ship.max > 0, out.compare);
  } finally {
    srv.close();
  }

  // The record: JSON plus a readable table.
  out.log = log;
  writeFileSync(join(OUT, 'live-session.json'), JSON.stringify(out, null, 2));
  const md = [
    '# Live session through the bridge (MCP, the way the AI drives it)',
    '',
    `Bridge port ${PORT}, engine ${ROOT}, started ${new Date(started).toISOString()}.`,
    '',
    '| # | Time (UTC) | +s | Tool | Step | Reply ms | OK | Overlay commands | Reply (short) |',
    '|---|---|---|---|---|---|---|---|---|',
    ...log.map((x) => `| ${x.step} | ${x.at.slice(11, 23)} | ${x.t_s ?? ''} | ${x.tool} | ${x.label} | ${x.reply_ms ?? ''} | ${x.ok === undefined ? '' : x.ok ? 'yes' : 'no'} | ${x.overlay_commands ?? ''} | ${JSON.stringify(x.reply ?? '').replace(/\|/g, '/').slice(0, 160)} |`),
    '',
    '## Checks',
    '',
    ...results.map((x) => `- ${x.ok ? 'PASS' : 'FAIL'} ${x.name}`),
    '',
  ].join('\n');
  writeFileSync(join(OUT, 'live-session.md'), md);
  return { broken: out.broken, bake_diff_stat: out.bake_diff_stat, compare: out.compare };
}

// ---------- talk: "Talk to the AI" end to end with the look guard (Gate 1) ----------

// TEST ONLY: the well-known look-guard test key (tools/evani-lookguard/src/testkey.rs). Its seed is
// public, so anything it signs proves nothing, and the engine marks such approvals test_key: true.
const TEST_SEED = Buffer.from('EVANI-LOOK-GUARD-TEST-KEY-NEVER!', 'ascii');
const edKey = (seed) => createPrivateKey({ key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]), format: 'der', type: 'pkcs8' });
const edPub = (k) => createPublicKey(k).export({ format: 'der', type: 'spki' }).subarray(-32);
const isoSec = (d = new Date()) => d.toISOString().replace(/\.\d+Z$/, 'Z');

/** Plays Evani.App's side of the look guard (docs/live-code-tab.md "Stage 1 tab asks"): pins the
 *  TEST key in a temp APPDATA (never the real Evani.App folder), starts the bridge with the stdin
 *  key line, answers the seq endpoint, listens on /evani-dev for approval requests and, when the
 *  script says so (Chris's click), signs the record and posts the approval. */
async function fakeEvaniApp(root, port, outDir) {
  const { createServer } = await import('node:http');
  const key = edKey(TEST_SEED);
  const pub = edPub(key).toString('hex');
  const keyId = createHash('sha256').update(edPub(key)).digest('hex').slice(0, 16);
  const appdata = mkdtempSync(join(tmpdir(), 'evani-talk-appdata-'));
  const store = join(appdata, 'Evani.App', 'look-approvals');
  mkdirSync(store, { recursive: true });
  writeFileSync(join(store, 'pubkey'), pub + '\n');
  const projectId = 'talk-to-ai-e2e';
  const seqs = new Map();
  const seqLog = [];
  const seqSrv = createServer((req, res) => {
    const u = new URL(req.url, 'http://x');
    const look = u.searchParams.get('look');
    seqLog.push({ at: isoSec(), look, seq: seqs.get(look) ?? null });
    res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ project: u.searchParams.get('project'), look, seq: seqs.get(look) ?? null }));
  });
  await new Promise((r) => seqSrv.listen(0, '127.0.0.1', r));
  const secret = randomBytes(32).toString('hex');
  mkdirSync(outDir, { recursive: true });
  const bridgeLog = [];
  const exe = join(root, 'target', 'debug', 'evani-live-bridge.exe');
  const bridge = spawn(exe, ['serve', '--root', root, '--port', String(port), '--game', 'sandbox', '--no-hot'], { cwd: root, env: { ...process.env, APPDATA: appdata }, stdio: ['pipe', 'ignore', 'pipe'], windowsHide: true });
  bridge.stderr.on('data', (d) => bridgeLog.push(String(d)));
  // Evani.App's first stdin line (LIVE_API 15.6). The secret stays in this process's memory.
  bridge.stdin.write(JSON.stringify({ approve_secret: secret, approvals_pubkey: pub, project_id: projectId, approvals_endpoint: `http://127.0.0.1:${seqSrv.address().port}/` }) + '\n');

  const events = [];
  let dev = null;
  const connectDev = async () => {
    dev = new WebSocket(`ws://127.0.0.1:${port}/evani-dev`);
    dev.addEventListener('message', (ev) => {
      try {
        const m = JSON.parse(ev.data);
        if (m.type === 'event' && String(m.event).startsWith('look_guard.')) events.push({ at: isoSec(), event: m.event, data: m.data });
      } catch {
        // not JSON
      }
    });
    await new Promise((res, rej) => {
      dev.addEventListener('open', res, { once: true });
      dev.addEventListener('error', () => rej(new Error('/evani-dev did not open')), { once: true });
    });
  };
  const approvalText = (d) => `evani-look-approve/1\napprovalId=${d.approvalId}\nkind=${d.kind}\nlook=${d.look}\nhash=${d.hash}\nexpiry=${d.expiry}\nsession=${d.session}\n`;
  const macOf = (d) => createHmac('sha256', Buffer.from(secret, 'hex')).update(`${d.approvalId}|${d.hash}|${d.expiry}`).digest('hex');
  return {
    pub,
    keyId,
    projectId,
    events,
    seqLog,
    connectDev,
    exited: () => bridge.exitCode !== null,
    logTail: () => bridgeLog.join('').split('\n').slice(-15).join('\n'),
    async waitEvent(pred, ms = 30000) {
      for (const t0 = Date.now(); Date.now() - t0 < ms; await sleep(50)) {
        const e = events.find(pred);
        if (e) return e;
      }
      return null;
    },
    /** Chris approves request `d` (an approval_requested event's data) on Evani.App's card. For
     *  limits, Evani.App first writes the signed record and bumps its seq. `badMac` / `otherKey`:
     *  forged approvals that must be refused. */
    async approve(d, { badMac = false, otherKey = false } = {}) {
      const forged = badMac || otherKey;
      let record = null;
      if (d.kind === 'limits' && !forged) {
        const seq = (seqs.get(d.look) ?? 0) + 1;
        const rec = { projectId, projectPath: root, look: d.look, seq, approvedAt: isoSec(), hash: d.hash };
        const canon = `evani-look-approval/1\nprojectId=${rec.projectId}\nprojectPath=${rec.projectPath}\nlook=${rec.look}\nseq=${seq}\napprovedAt=${rec.approvedAt}\nhash=${rec.hash}\n`;
        record = { format: 'evani-look-approval/1', ...rec, sig: edSign(null, Buffer.from(canon, 'utf8'), key).toString('hex') };
        mkdirSync(join(store, projectId), { recursive: true });
        writeFileSync(join(store, projectId, `${d.look}.json`), JSON.stringify(record, null, 2));
        seqs.set(d.look, seq);
      }
      const signer = otherKey ? edKey(randomBytes(32)) : key;
      const body = { approvalId: d.approvalId, kind: d.kind, look: d.look, hash: d.hash, expiry: d.expiry, session: d.session, sig: edSign(null, Buffer.from(approvalText(d), 'utf8'), signer).toString('hex') };
      const r = await fetch(`http://127.0.0.1:${d.approvePort}/look-guard/v1/approve`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Evani-Approve ${badMac ? '0'.repeat(64) : macOf(d)}` }, body: JSON.stringify(body), signal: AbortSignal.timeout(30000) });
      return { status: r.status, body: await r.json().catch(() => null), record };
    },
    async stop() {
      try {
        dev?.close();
      } catch {
        // already closed
      }
      seqSrv.close();
      try {
        bridge.stdin.end();
      } catch {
        // gone
      }
      if (bridge.exitCode === null) spawnSync('taskkill', ['/PID', String(bridge.pid), '/T', '/F'], { stdio: 'ignore' });
      await sleep(500);
      writeFileSync(join(outDir, 'talk-bridge.log'), bridgeLog.join(''));
      try {
        rmSync(appdata, { recursive: true, force: true });
      } catch {
        // a file may still be held for a moment
      }
    },
  };
}

// Extended Reinhard on luminance (white point 8): a gentle shoulder that keeps a little more
// highlight than the look's plain Reinhard, mild enough to stay inside the look's limits.
const REINHARD_EXT = '    // extended Reinhard on luminance (white point 8, keeps hue)\n    let l = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));\n    let mapped = l * (1.0 + l / 64.0) / (1.0 + l);\n    let scale = select(0.0, mapped / l, l > 0.0001);\n    return clamp(color * scale, vec3<f32>(0.0), vec3<f32>(1.0));';

const near = (x, v) => typeof x === 'number' && Math.abs(x - v) < 1e-5;
const TUNED_BLOOM = 0.33;

async function modeTalk() {
  if (await status()) throw new Error(`something already answers on port ${PORT}; pick a free --port`);
  const LOOK = 'evani-look-basic';
  const lookDir = `looks/${LOOK}`;
  const git = (...a) => spawnSync('git', ['-C', ROOT, ...a], { encoding: 'utf8' }).stdout.trim();
  const tracked = (p) => spawnSync('git', ['-C', ROOT, 'ls-files', '--error-unmatch', p], { stdio: 'ignore' }).status === 0;
  const touched = new Set();
  const restoreLook = () => {
    for (const p of touched) {
      if (tracked(p)) spawnSync('git', ['-C', ROOT, 'checkout', '--', p], { stdio: 'ignore' });
      else rmSync(join(ROOT, p), { force: true });
    }
    touched.clear();
  };
  const app = await fakeEvaniApp(ROOT, PORT, OUT);
  const started = Date.now();
  const steps = [];
  const out = {};
  let page = null;
  let mcp = null;
  const trim = (v) => JSON.parse(JSON.stringify(v ?? null, (k, x) => (typeof x === 'string' && x.length > 400 ? `${x.slice(0, 400)}... (${x.length} chars)` : x)));
  const call = async (label, tool, a) => {
    const at = new Date();
    const t = performance.now();
    const r = await mcp.call(tool, a);
    const ms = +(performance.now() - t).toFixed(1);
    let code = null;
    let data = null;
    if (r.isError) {
      const [head, ...rest] = (r.text[0] ?? '').split('\n');
      code = head.split(':')[0];
      try {
        data = JSON.parse(rest.join('\n'));
      } catch {
        // no data
      }
    }
    const n = String(steps.length + 1).padStart(2, '0');
    const images = r.images.map((img, i) => savePng(`talk-${n}-${label}${r.images.length > 1 ? `-${i + 1}` : ''}.png`, img.data));
    const row = { step: steps.length + 1, label, at: at.toISOString(), t_s: +((at - started) / 1000).toFixed(2), tool, args: trim(a), reply_ms: ms, ok: !r.isError, code, reply: trim(r.json ?? r.text[0]), images };
    steps.push(row);
    log(`${row.at} ${tool} ${label}: ${row.ok ? 'ok' : `error ${code}`} in ${ms} ms`);
    return { r, row, code, data, json: r.json };
  };
  const note = (t) => AI_NOTE(t);
  const lookStatus = () => git('status', '--porcelain', '--untracked-files=all', '--', lookDir);
  let flowErr = null;
  try {
    // 0. The bridge builds the dev page (Trunk), then goes idle.
    for (const t0 = Date.now(); (await status())?.phase !== 'idle'; await sleep(3000)) {
      if (app.exited()) throw new Error('the bridge stopped:\n' + app.logTail());
      if (Date.now() - t0 > 40 * 60000) throw new Error('the bridge did not finish building in 40 min:\n' + app.logTail());
    }
    await app.connectDev();
    const lookFiles0 = lookStatus();
    page = await openLivePage();
    mcp = mcpClient();
    await mcp.init();
    const tools = (await mcp.tools()).map((t) => t.name);
    check('talk: the AI\'s MCP tools include the look guard, and none can approve', ['look_status', 'look_check', 'look_baseline_propose', 'compare_ab', 'snapshot_save', 'bake_export'].every((t) => tools.includes(t)) && !tools.some((t) => /approve/i.test(t)), tools.filter((t) => /look|snapshot|compare|bake/.test(t)).join(', '));

    // 1. Status: game connected, look unguarded, approvals on with the TEST key.
    const st = await call('status', 'live_status', {});
    const lg = (await status())?.look_guard;
    out.look_guard_status = lg;
    check('talk: game connected; approvals switched on by the stdin key line', st.json?.engine_connected && lg?.approvals === 'enabled' && lg?.key_id === app.keyId, lg);
    const ls0 = await call('look-status-before', 'look_status', {});
    check('talk: the sandbox look has no approved limits yet (unguarded)', ls0.json?.guard === 'unguarded' && ls0.json?.look === LOOK, { guard: ls0.json?.guard, approvals: ls0.json?.approvals?.approvals });

    // 2. Snapshot A.
    await call('snapshot-original', 'snapshot_save', { name: 'original', note: note('the look as it was') });

    // 3. With no approved limits every real bake is refused.
    const b0 = await call('bake-unguarded', 'bake_export', { title: 'Talk to the AI: before any limits' });
    check('talk: a bake before any approved limits is refused (look_unapproved), nothing written', b0.code === 'look_unapproved' && lookStatus() === lookFiles0, b0.row.reply);

    // 4. The AI proposes a baseline; "Chris" approves it on Evani.App's card.
    const moments = { tier: 'medium', width: 320, moments: [{ name: 'start', capture: { sim_time: 2, camera: 'path' } }, { name: 'mid', capture: { sim_time: 6, camera: 'path' } }] };
    const ev0 = app.events.length;
    const prop = await call('baseline-propose', 'look_baseline_propose', { moments, note: note('baseline of the sandbox look as it ships today, so later tuning is checked against it') });
    const req0 = await app.waitEvent((e) => app.events.indexOf(e) >= ev0 && e.event === 'look_guard.approval_requested' && e.data?.kind === 'limits');
    check('talk: baseline proposal is only pending and reaches Evani.App as an approval request', prop.json?.status === 'pending' && !!req0 && req0.data.approvalId === prop.json?.approvalId, { reply: prop.json?.status, event: req0?.data?.approvalId, files: req0?.data?.files?.length });
    if (!req0) throw new Error('no approval request for the baseline reached Evani.App; cannot go on');
    const ap0 = await app.approve(req0.data);
    check('talk: Chris approves the baseline; guard set written and verified', ap0.status === 200 && ap0.body?.verify?.ok === true, { status: ap0.status, written: ap0.body?.written, verify: ap0.body?.verify });
    mkdirSync(OUT, { recursive: true });
    for (const w of ap0.body?.written ?? []) if (w.endsWith('.png')) writeFileSync(join(OUT, `talk-baseline-${w.split('/').pop()}`), readFileSync(join(ROOT, w)));
    const ls1 = await call('look-status-approved', 'look_status', {});
    check('talk: look_status now verified (test key marked, rollback checked with Evani.App)', ls1.json?.guard === 'verified' && ls1.json?.verified?.test_key === true && ls1.json?.verified?.rollback?.checked === true, ls1.json?.verified);

    // 5. Bloom and the tonemap slot.
    const p0 = (await call('bloom-before', 'param_get', { names: ['fx.bloom.intensity', 'fx.bloom.threshold'] })).json;
    const bl = await call('bloom', 'param_set', { values: { 'fx.bloom.intensity': TUNED_BLOOM }, note: note('Bloom a little stronger so the glow reads at a distance'), capture_width: 480 });
    check(`talk: Bloom changed (0.3 -> ${TUNED_BLOOM})`, bl.row.ok && (bl.json?.applied ?? []).some((x) => x.name === 'fx.bloom.intensity' && near(x.new, TUNED_BLOOM)), bl.json?.applied);
    const g0 = (await call('tonemap-before', 'slot_get', { name: 'tonemap' })).json;
    const tm = await call('tonemap', 'slot_set', { name: 'tonemap', source: REINHARD_EXT, note: note('extended Reinhard tonemap: keeps a little more highlight detail'), capture_width: 480 });
    const g1 = (await call('tonemap-after', 'slot_get', { name: 'tonemap' })).json;
    check('talk: tonemap slot rewritten (new hash)', tm.row.ok && g1?.source?.trim() === REINHARD_EXT.trim() && g1?.hash !== g0?.hash, { before: g0?.hash, after: g1?.hash, compile: tm.json?.compile });

    // 6. Inside the limits?
    const c1 = await call('look-check-tuned', 'look_check', {});
    check('talk: look_check passes for the tuned look', c1.json?.passed === true, { passed: c1.json?.passed, sentences: c1.json?.sentences });

    // 7. Snapshot B, A/B compare.
    await call('snapshot-tuned', 'snapshot_save', { name: 'tuned', note: note(`Bloom ${TUNED_BLOOM} + extended Reinhard`) });
    const ab1 = await call('compare-original-vs-tuned', 'compare_ab', { a: 'original', b: 'tuned', seconds: 2, capture_width: 480 });
    const p1 = (await call('bloom-after-compare', 'param_get', { names: ['fx.bloom.intensity'] })).json;
    check('talk: A/B compare of the two snapshots returns both pictures and a difference; current state kept', ab1.row.ok && ab1.row.images.length >= 2 && near(p1?.values?.['fx.bloom.intensity'], TUNED_BLOOM), { images: ab1.row.images.length, diff: trim(ab1.json?.diff ?? ab1.json?.pixel_diff ?? null) });

    // 8. A deliberately bad change, snapshot, compare.
    await call('too-hot', 'param_set', { values: { 'fx.bloom.intensity': 3, 'fx.bloom.threshold': 0.3 }, note: note('deliberately far too much bloom, to show the look guard flags it'), capture_width: 480 });
    await call('snapshot-too-hot', 'snapshot_save', { name: 'too-hot', note: note('the deliberately bad state') });
    const ab2 = await call('compare-tuned-vs-too-hot', 'compare_ab', { a: 'tuned', b: 'too-hot', seconds: 2, capture_width: 480 });
    check('talk: A/B compare tuned vs too-hot', ab2.row.ok && ab2.row.images.length >= 2, { images: ab2.row.images.length });
    const c2 = await call('look-check-too-hot', 'look_check', {});
    check('talk: look_check flags the too-hot look', c2.json?.passed === false && (c2.json?.sentences ?? []).length > 0, c2.json?.sentences);

    // 9. The flagged bake is refused without Chris's approval; nothing written.
    const title = 'Talk to the AI: too much bloom (flagged on purpose)';
    const ev1 = app.events.length;
    const files0 = lookStatus();
    const bf = await call('bake-flagged', 'bake_export', { title });
    const req1 = await app.waitEvent((e) => app.events.indexOf(e) >= ev1 && e.event === 'look_guard.approval_requested' && e.data?.kind === 'bake');
    check('talk: flagged bake refused (look_flagged), no files written, approval request sent to Evani.App', bf.code === 'look_flagged' && lookStatus() === files0 && !!req1 && req1.data.approvalId === bf.data?.approvalId, { code: bf.code, sentences: bf.data?.sentences, approvalId: bf.data?.approvalId });

    // 10. The AI can't approve it: no tool, the command is forbidden, forged approvals bounce.
    const ai = await cmd('look.approve', { approvalId: bf.data?.approvalId }, note('trying to approve my own bake'));
    check('talk: look.approve over the AI\'s command path is forbidden', ai.ok === false && ai.error?.code === 'forbidden', ai.error);
    if (!req1) throw new Error('no approval request for the flagged bake reached Evani.App; cannot go on');
    const fm = await app.approve(req1.data, { badMac: true });
    check('talk: an approval with a wrong MAC is refused (401)', fm.status === 401 && fm.body?.error?.code === 'bad_auth', { status: fm.status, body: fm.body });
    const fk = await app.approve(req1.data, { otherKey: true });
    check('talk: an approval signed by another key is refused (401)', fk.status === 401 && fk.body?.error?.code === 'bad_signature', { status: fk.status, body: fk.body });
    out.forged = { ai_command: ai.error, wrong_mac: { status: fm.status, body: fm.body }, other_key: { status: fk.status, body: fk.body } };

    // 11. Chris approves; the same bake goes through once.
    const ap1 = await app.approve(req1.data);
    check('talk: Chris approves the flagged bake', ap1.status === 200 && ap1.body?.ok === true, ap1.body);
    const ba = await call('bake-approved', 'bake_export', { title });
    for (const w of ba.json?.written ?? []) touched.add(w.path);
    for (const p of [ba.json?.look_guard?.manifest, ba.json?.changes_md]) if (p) touched.add(p);
    writeFileSync(join(OUT, 'talk-approved-bake.diff'), spawnSync('git', ['-C', ROOT, 'diff', '--', lookDir], { encoding: 'utf8' }).stdout);
    check('talk: after approval the same bake goes through (check "approved")', ba.row.ok && ba.json?.look_guard?.check === 'approved' && ba.json?.look_guard?.approval_id === req1.data.approvalId, ba.json?.look_guard);
    const again = await call('bake-approved-again', 'bake_export', { title });
    check('talk: the approval is one-time (the same bake again is flagged again)', again.code === 'look_flagged', again.code);
    restoreLook();

    // 12. Back to the tuned snapshot; the passing bake.
    await call('restore-tuned', 'snapshot_restore', { snapshot: 'tuned', note: note('back to the tuned look') });
    const p2 = (await call('bloom-restored', 'param_get', { names: ['fx.bloom.intensity', 'fx.bloom.threshold'] })).json;
    const files1 = lookStatus();
    const bp = await call('bake-passing', 'bake_export', { title: `Talk to the AI: Bloom ${TUNED_BLOOM}, extended Reinhard tonemap` });
    for (const w of bp.json?.written ?? []) touched.add(w.path);
    for (const p of [bp.json?.look_guard?.manifest, bp.json?.changes_md]) if (p) touched.add(p);
    out.bake_diff_stat = git('diff', '--stat', '--', lookDir);
    writeFileSync(join(OUT, 'talk-passing-bake.diff'), spawnSync('git', ['-C', ROOT, 'diff', '--', lookDir], { encoding: 'utf8' }).stdout);
    check('talk: the tuned bake passes the look check and is written', bp.row.ok && bp.json?.look_guard?.check === 'passed' && lookStatus() !== files1 && near(p2?.values?.['fx.bloom.intensity'], TUNED_BLOOM), { look_guard: bp.json?.look_guard, written: bp.json?.written?.map((w) => w.path) });
    const ls2 = await call('look-status-after', 'look_status', {});
    out.last_bake = ls2.json?.last_bake;
    out.audit = ls2.json?.audit;
    check('talk: the look guard audit log chain is intact', ls2.json?.audit?.chain === 'ok', ls2.json?.audit);
    out.params = { before: p0, tuned: p2 };
  } catch (e) {
    flowErr = e;
    log('the flow stopped:', e.stack ?? e);
  } finally {
    mcp?.close();
    await page?.close();
  }

  // 13. Rebuild without live from the baked sources; compare pixels with the live state.
  try {
    if (flowErr) throw flowErr;
    const N = Number(args.frames ?? 360);
    if (!args['no-rebuild']) {
      const t = Date.now();
      const b = spawnSync('cargo', ['xtask', 'size-report', 'sandbox'], { cwd: ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 64 << 20 });
      steps.push({ step: steps.length + 1, label: 'rebuild-ship', at: new Date().toISOString(), tool: 'cargo xtask size-report sandbox (ship build, live off)', reply: { exit: b.status, seconds: Math.round((Date.now() - t) / 1000), tail: (b.stdout + b.stderr).trim().split('\n').slice(-6) } });
      if (b.status !== 0) throw new Error(`ship rebuild failed: ${b.stdout}\n${b.stderr}`);
    }
    for (const t0 = Date.now(); (await status())?.phase !== 'idle'; await sleep(2000)) if (Date.now() - t0 > 600000) throw new Error('bridge not idle');
    const srv = await serveDir(join(ROOT, 'target', 'evani-sizes', 'sandbox-ship'));
    try {
      const liveShot = await shootFrames({ url: `http://127.0.0.1:${PORT}/`, N, values: { 'fx.bloom.intensity': TUNED_BLOOM }, slots: { tonemap: REINHARD_EXT }, png: join(OUT, `talk-live-f${N}.png`) });
      const shipShot = await shootFrames({ url: `http://127.0.0.1:${srv.port}/`, N, png: join(OUT, `talk-ship-f${N}.png`) });
      const untuned = await shootFrames({ url: `http://127.0.0.1:${PORT}/`, N, png: join(OUT, `talk-live-untuned-f${N}.png`) });
      out.compare = { frame: N, live: liveShot.file, ship: shipShot.file, live_api_in_ship: shipShot.live_api_in_page, replay: liveShot.applied, live_vs_ship: diffPng(liveShot.file, shipShot.file), sensitivity_untuned_vs_ship: diffPng(untuned.file, shipShot.file) };
      steps.push({ step: steps.length + 1, label: 'pixel-compare', at: new Date().toISOString(), tool: `stepped clock, frame ${N}`, reply: { live_vs_ship: out.compare.live_vs_ship, sensitivity: out.compare.sensitivity_untuned_vs_ship } });
      check('talk: the rebuild without evani-live is pixel-identical to the baked live state', out.compare.live_api_in_ship === false && out.compare.live_vs_ship.max === 0 && out.compare.sensitivity_untuned_vs_ship.max > 0, out.compare);
    } finally {
      srv.close();
    }
  } finally {
    // 14. Put the sandbox's look back as it was (unless --keep-bake); stop the bridge.
    const auditFile = join(ROOT, '.evani', 'look-guard', 'audit.jsonl');
    if (existsSync(auditFile)) writeFileSync(join(OUT, 'talk-audit.jsonl'), readFileSync(auditFile));
    out.events = app.events.map((e) => ({ at: e.at, event: e.event, kind: e.data?.kind, approvalId: e.data?.approvalId, origin: e.data?.origin, note: e.data?.note }));
    out.seq_queries = app.seqLog.length;
    await app.stop();
    if (!args['keep-bake']) {
      restoreLook();
      rmSync(join(ROOT, lookDir, 'guard'), { recursive: true, force: true });
      out.restored = lookStatus() === '' ? 'look files back as before' : `left: ${lookStatus()}`;
    } else out.restored = 'kept (--keep-bake)';
    out.steps = steps;
    out.checks = results;
    writeFileSync(join(OUT, 'talk-session.json'), JSON.stringify(out, null, 2));
    const md = [
      '# Talk to the AI, end to end (raw log written by accept.mjs talk)',
      '',
      `Bridge port ${PORT}, engine ${ROOT}, started ${new Date(started).toISOString()}. Evani.App's side played with the public TEST key.`,
      '',
      '| # | Time (UTC) | +s | Tool | Step | Reply ms | OK | Reply (short) |',
      '|---|---|---|---|---|---|---|---|',
      ...steps.map((x) => `| ${x.step} | ${x.at.slice(11, 23)} | ${x.t_s ?? ''} | ${x.tool} | ${x.label} | ${x.reply_ms ?? ''} | ${x.ok === undefined ? '' : x.ok ? 'yes' : `no (${x.code})`} | ${JSON.stringify(x.reply ?? '').replace(/\|/g, '/').slice(0, 160)} |`),
      '',
      '## Checks',
      '',
      ...results.map((x) => `- ${x.ok ? 'PASS' : 'FAIL'} ${x.name}`),
      '',
    ].join('\n');
    writeFileSync(join(OUT, 'talk-session.md'), md);
  }
  return { compare: out.compare, restored: out.restored, bake_diff_stat: out.bake_diff_stat };
}

const modes = { talk: modeTalk, session: modeSession, quality: modeQuality, shot: modeShot, probe: modeProbe, ai: modeAi, safety: modeSafety, tune: modeTune, frames: modeFrames, diff: async () => diffPng(pos[0], pos[1]) };
if (!modes[MODE]) {
  console.error('usage: node tools/live-check/accept.mjs talk|ai|safety|tune|frames|diff|quality|session|shot|probe ... (see the header)');
  process.exit(2);
}
modes[MODE]()
  .then((r) => {
    const failed = results.filter((x) => !x.ok).length;
    process.stdout.write(JSON.stringify({ mode: MODE, passed: results.length - failed, failed, results, out: r }, null, 2) + '\n');
    process.exit(failed ? 1 : 0);
  })
  .catch((e) => {
    console.error('[accept] could not run:', e.stack ?? e);
    process.stdout.write(JSON.stringify({ mode: MODE, error: String(e.message ?? e), results }, null, 2) + '\n');
    process.exit(2);
  });
