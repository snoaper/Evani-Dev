// sweep.run end to end, and what tuning costs the old way and the new way (docs/LIVE_TUNING.md).
//
// Needs a running bridge (`evani-live-bridge serve --root <engine> --port <P> --game kart-racer`);
// it never starts or stops one. Opens one headless Chrome at a time (pointer lock, fullscreen
// and keyboard lock stubbed; the real mouse and keyboard are never used) and closes it.
//
//   node tools/live-check/sweep.mjs [--port 8143] [--root <engine>] [--out <dir>]
//
// The tuning task, done twice through the bridge's MCP server (as an AI would):
//   "find a bloom intensity and threshold that keep clipped highlights within the look's limit
//    (the look guard's tolerance: at most 1 point above the current look) at the lowest cost".
//   Old way: per try one param_set, one capture_frame and one telemetry_sample; then apply.
//   New way: one sweep_run over the same grid, then one batch to apply.
// Every MCP request and response is counted in bytes (tokens ~ bytes / 4); images are also
// counted the way Claude bills them (width x height / 750).
//
// Then the overlay's GPU name is read with and without --enable-webgpu-developer-features.
// Writes table.json, table.md, tuning-cost.json, overlay.json and the thumbnails to --out.
// Exit 0 when every check passed, 1 when one failed, 2 when it could not run.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[++i] : true;
}
const ROOT = resolve(args.root ?? fileURLToPath(new URL('../..', import.meta.url)));
const PORT = Number(args.port ?? 8143);
const OUT = resolve(args.out ?? join(ROOT, '.vibedeck', 'screenshots', 'sweep'));
const log = (...m) => console.error('[sweep]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok: !!ok, detail });
  log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ` - ${typeof detail === 'string' ? detail : JSON.stringify(detail)}` : ''}`);
  return !!ok;
};

// ---------- Chrome over CDP (as accept.mjs; no npm packages) ----------

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
  // Show the kart's overlay (its saved setting), so the GPU name can be read.
  try { localStorage.setItem('evani:kart-racer:kart.overlay', '1'); } catch (e) {}
})();`;

async function openChrome({ devFeatures }) {
  const exe = findChrome();
  if (!exe) throw new Error('no Chrome or Edge found (pass --chrome)');
  const profile = mkdtempSync(join(tmpdir(), 'evani-sweep-'));
  const flags = ['--headless=new', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check', '--enable-unsafe-webgpu', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--window-size=1280,720'];
  if (devFeatures) flags.push('--enable-webgpu-developer-features');
  const chrome = spawn(exe, [...flags, 'about:blank'], { stdio: 'ignore', windowsHide: true });
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
  const evaluate = async (expression, ms = 120000) => {
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

async function openLivePage(devFeatures) {
  for (const t0 = Date.now(); (await status())?.phase !== 'idle'; await sleep(2000)) {
    if (Date.now() - t0 > 20 * 60000) throw new Error(`no idle bridge on ${PORT}`);
  }
  const page = await openChrome({ devFeatures });
  await page.s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` });
  for (const t0 = Date.now(); !(await status())?.engine_connected; await sleep(300)) {
    if (Date.now() - t0 > 180000) throw new Error('the page did not connect in 180 s\n' + page.consoleLines.join('\n'));
  }
  await sleep(3000);
  log(`live page connected (developer features ${devFeatures ? 'on' : 'off'})`);
  return page;
}

const overlayText = (page) => page.evaluate("(document.getElementById('evani-overlay') || {}).textContent || ''");

// ---------- The bridge's MCP server on stdio, counting every byte ----------

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
      const r = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'sweep-check', version: '1' } });
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
      return r.result;
    },
    async tools() {
      return (await rpc('tools/list', {})).result.tools;
    },
    /** One tool call; `meter` gets what an AI would read and write for it. */
    async call(name, a, meter) {
      const t0 = performance.now();
      const r = await rpc('tools/call', { name, arguments: a });
      const res = r.result ?? { isError: true, content: [{ type: 'text', text: JSON.stringify(r.error) }] };
      const text = res.content.filter((c) => c.type === 'text').map((c) => c.text);
      const images = res.content.filter((c) => c.type === 'image');
      if (meter) {
        meter.commands++;
        meter.by_tool[name] = (meter.by_tool[name] ?? 0) + 1;
        meter.request_bytes += Buffer.byteLength(JSON.stringify({ name, arguments: a }));
        meter.response_text_bytes += text.reduce((n, t) => n + Buffer.byteLength(t), 0);
        for (const im of images) {
          meter.image_base64_bytes += im.data.length;
          const { w, h } = pngSize(Buffer.from(im.data, 'base64'));
          meter.image_tokens_claude += Math.ceil((w * h) / 750);
        }
        meter.seconds += (performance.now() - t0) / 1000;
      }
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

const newMeter = () => ({ commands: 0, by_tool: {}, request_bytes: 0, response_text_bytes: 0, image_base64_bytes: 0, image_tokens_claude: 0, seconds: 0 });
function totals(m) {
  const bytes = m.request_bytes + m.response_text_bytes + m.image_base64_bytes;
  const textBytes = m.request_bytes + m.response_text_bytes;
  return {
    ...m,
    seconds: +m.seconds.toFixed(1),
    bytes,
    tokens_bytes_over_4: Math.round(bytes / 4),
    tokens_text_plus_images_as_claude: Math.round(textBytes / 4) + m.image_tokens_claude,
  };
}

// ---------- PNG (8-bit RGB/RGBA, non-interlaced) and the look measures (look-guard stats v1) ----------

function pngSize(b) {
  return { w: b.readUInt32BE(16), h: b.readUInt32BE(20) };
}

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

/** clipped and mean luma of a capture (the old way's AI judges by eye; the script needs numbers). */
function lookOf(b64) {
  const { w, h, rgb } = decodePng(Buffer.from(b64, 'base64'));
  let clipped = 0, luma = 0;
  for (let i = 0; i < w * h; i++) {
    const r = rgb[i * 3], g = rgb[i * 3 + 1], b = rgb[i * 3 + 2];
    if (Math.max(r, g, b) >= 250) clipped++;
    luma += 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }
  return { clipped: clipped / (w * h), mean_luma: luma / (w * h) / 255 };
}

// ---------- The tuning task ----------

const INTENSITY = [0.3, 0.6, 1.0, 1.5];
const THRESHOLD = [0.8, 1.2, 1.6];
const LOOK_TOL = 0.01; // look guard stats v1: clipped may rise at most 1 point (Mode::Up, tol 0.01)
const SHOT_T = 1.0; // path time (s) for the old way's screenshots: the sweep's middle look frame at 120 frames
const NOTE = (t) => `[sweep check] ${t}`;

async function oldWay(mcp, meter) {
  const tries = [];
  for (const intensity of INTENSITY) {
    for (const threshold of THRESHOLD) {
      const set = await mcp.call('param_set', { values: { 'fx.bloom.intensity': intensity, 'fx.bloom.threshold': threshold }, note: NOTE(`try bloom ${intensity}/${threshold}`) }, meter);
      if (set.isError) throw new Error('param_set: ' + set.text.join(' '));
      const cap = await mcp.call('capture_frame', { width: 320, sim_time: SHOT_T, camera: 'path' }, meter);
      if (cap.isError || !cap.images.length) throw new Error('capture_frame: ' + cap.text.join(' '));
      const tel = await mcp.call('telemetry_sample', { seconds: 2 }, meter);
      if (tel.isError) throw new Error('telemetry_sample: ' + tel.text.join(' '));
      const avg = tel.json?.average ?? {};
      tries.push({ intensity, threshold, ...lookOf(cap.images[0].data), work_ms: avg.work_ms?.avg ?? avg.frame_ms?.avg ?? null, gpu_ms: avg.gpu_ms?.avg_total ?? null });
    }
  }
  return tries;
}

function choose(rows, base) {
  const limit = base.clipped + LOOK_TOL;
  const cost = (r) => (Number.isFinite(r.gpu_ms) ? r.gpu_ms : r.work_ms);
  const ok = rows.filter((r) => r.clipped <= limit + 1e-9 && Number.isFinite(cost(r)));
  ok.sort((a, b) => cost(a) - cost(b) || Math.abs(a.mean_luma - base.mean_luma) - Math.abs(b.mean_luma - base.mean_luma));
  return { limit, pick: ok[0] ?? null, within: ok.length };
}

function tableMd(t) {
  const fmt = (v) => (v === null ? '–' : typeof v === 'number' ? String(v) : JSON.stringify(v));
  const head = `| ${t.columns.join(' | ')} |\n|${t.columns.map(() => '---').join('|')}|\n`;
  return head + t.rows.map((r) => `| ${r.map(fmt).join(' | ')} |`).join('\n') + '\n';
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  const report = {};
  let page = null;
  let mcp = null;
  try {
    // ---- 1. The tuning session, old and new, with developer features OFF (as players run Chrome) ----
    page = await openLivePage(false);
    report.overlay_default = await overlayText(page);
    mcp = mcpClient();
    await mcp.init();
    const tools = (await mcp.tools()).map((t) => t.name);
    check('the MCP server lists sweep_run', tools.includes('sweep_run'), `${tools.length} tools`);
    const before = (await mcp.call('param_get', { prefix: 'fx.bloom.' })).json;
    await mcp.call('snapshot_save', { name: 'sweep-check-start', note: 'start of the sweep check' });

    // The baseline the limit is measured from (not counted: both ways need it, and the sweep's row 0 gives it).
    const base0 = await mcp.call('capture_frame', { width: 320, sim_time: SHOT_T, camera: 'path' });
    const baseLook = lookOf(base0.images[0].data);

    log('old way: param_set + capture_frame + telemetry_sample per try ...');
    const oldMeter = newMeter();
    const tries = await oldWay(mcp, oldMeter);
    const oldPick = choose(tries, baseLook);
    if (oldPick.pick) {
      const last = tries[tries.length - 1];
      if (last.intensity !== oldPick.pick.intensity || last.threshold !== oldPick.pick.threshold) {
        await mcp.call('param_set', { values: { 'fx.bloom.intensity': oldPick.pick.intensity, 'fx.bloom.threshold': oldPick.pick.threshold }, note: NOTE('keep the cheapest bloom within the clipped limit') }, oldMeter);
      }
    }
    report.old = { tries, choice: oldPick, cost: totals(oldMeter) };
    await mcp.call('snapshot_restore', { snapshot: 'sweep-check-start', note: 'back to the start for the new way' });

    log('new way: one sweep_run + one batch ...');
    const newMeterV = newMeter();
    const thumbsDir = relative(ROOT, join(OUT, 'thumbs')).replace(/\\/g, '/');
    const sw = await mcp.call('sweep_run', { grid: { 'fx.bloom.intensity': INTENSITY, 'fx.bloom.threshold': THRESHOLD }, thumbs: { w: 160, dir: thumbsDir } }, newMeterV);
    if (sw.isError || !sw.json) throw new Error('sweep_run: ' + sw.text.join(' '));
    const t = sw.json;
    writeFileSync(join(OUT, 'table.json'), sw.text[0] + '\n');
    writeFileSync(join(OUT, 'table.md'), tableMd(t));
    const col = (name) => t.columns.indexOf(name);
    const rows = t.rows.map((r) => ({ intensity: r[col('fx.bloom.intensity')], threshold: r[col('fx.bloom.threshold')], clipped: r[col('clipped')], mean_luma: r[col('mean_luma')], work_ms: r[col('work_ms')], gpu_ms: r[col('gpu_ms')] }));
    const base = rows[t.base_row ?? 0];
    const newPick = choose(rows.filter((_, i) => i !== t.base_row), base);
    if (newPick.pick) {
      const b = await mcp.call('batch', { cmds: [{ cmd: 'param.set', params: { values: { 'fx.bloom.intensity': newPick.pick.intensity, 'fx.bloom.threshold': newPick.pick.threshold } } }], note: NOTE('cheapest bloom within the clipped limit, from one sweep') }, newMeterV);
      check('the chosen set applies in one batch', !b.isError, b.text[0]?.slice(0, 200));
    }
    report.new = { run: t.run, choice: newPick, cost: totals(newMeterV) };

    // Checks on the sweep itself.
    check('the sweep returns one row per grid combination plus the baseline', t.rows.length === INTENSITY.length * THRESHOLD.length + 1, `${t.rows.length} rows`);
    check('every row has every column', t.rows.every((r) => r.length === t.columns.length), t.columns.join(','));
    check('the table has per-pass costs and the look measures', ['work_ms', 'gpu_ms', 'clipped', 'bloom', 'colorfulness'].every((c) => t.columns.includes(c)) && t.columns.some((c) => c.startsWith('gpu.')), t.columns.join(','));
    check('thumbnails are files, not inline images', sw.images.length === 0 && (t.thumbs ?? []).every((x) => x.path && existsSync(x.path) && !x.png_base64), `${(t.thumbs ?? []).length} files`);
    check('the sweep restored the start state', t.restored === true);
    // The sweep never changes anything: compare against the start (the batch above did change it).
    await mcp.call('snapshot_restore', { snapshot: 'sweep-check-start', note: 'back to the start' });
    const after = (await mcp.call('param_get', { prefix: 'fx.bloom.' })).json;
    check('bloom values are back to the start', JSON.stringify(after) === JSON.stringify(before));
    const bad = await mcp.call('sweep_run', { param: 'fx.bloom.nope', values: [1] });
    check('a bad parameter fails with unknown_target and the row', bad.isError && /unknown_target/.test(bad.text[0]) && /"row":0/.test(bad.text[0]), bad.text[0]?.slice(0, 160));

    report.overlay_default_after = await overlayText(page);
    mcp.close();
    mcp = null;
    await page.close();
    page = null;

    // ---- 2. The overlay's GPU name with developer features ON ----
    page = await openLivePage(true);
    report.overlay_dev_features = await overlayText(page);
    await page.close();
    page = null;

    const gpuLine = (s) => (s.match(/webgpu ok ([^\n·]*)/)?.[1] ?? '').trim();
    report.gpu_name = { default_flags: gpuLine(report.overlay_default_after || report.overlay_default), developer_features: gpuLine(report.overlay_dev_features) };
    check('the overlay names the GPU with default Chrome flags', report.gpu_name.default_flags && report.gpu_name.default_flags !== '(adapter)', report.gpu_name.default_flags);
    check('the overlay names the GPU with developer features', report.gpu_name.developer_features && report.gpu_name.developer_features !== '(adapter)', report.gpu_name.developer_features);

    const o = report.old.cost, n = report.new.cost;
    report.summary = {
      task: `bloom intensity ${JSON.stringify(INTENSITY)} x threshold ${JSON.stringify(THRESHOLD)}; clipped <= baseline + ${LOOK_TOL}; lowest cost`,
      old: { commands: o.commands, bytes: o.bytes, tokens_bytes_over_4: o.tokens_bytes_over_4, tokens_text_plus_images_as_claude: o.tokens_text_plus_images_as_claude, seconds: o.seconds, pick: report.old.choice.pick },
      new: { commands: n.commands, bytes: n.bytes, tokens_bytes_over_4: n.tokens_bytes_over_4, tokens_text_plus_images_as_claude: n.tokens_text_plus_images_as_claude, seconds: n.seconds, pick: report.new.choice.pick },
    };
    check('the new way uses fewer commands and tokens', n.commands < o.commands && n.tokens_bytes_over_4 < o.tokens_bytes_over_4, report.summary);
  } finally {
    if (mcp) mcp.close();
    if (page) await page.close();
    writeFileSync(join(OUT, 'tuning-cost.json'), JSON.stringify({ ...report, checks: results }, null, 2) + '\n');
  }
  const failed = results.filter((r) => !r.ok).length;
  log(`${results.length - failed}/${results.length} checks passed; written to ${OUT}`);
  process.exit(failed ? 1 : 0);
}

// `--probe [--dev]`: open the live page for 20 s and print what it shows (overlay, console, status).
async function probe() {
  const page = await openChrome({ devFeatures: !!args.dev });
  try {
    await page.s('Page.navigate', { url: `http://127.0.0.1:${PORT}/?live=1` });
    await sleep(20000);
    console.log(JSON.stringify({ status: await status(), overlay: await overlayText(page), console: page.consoleLines }, null, 2));
  } finally {
    await page.close();
  }
}

(args.probe ? probe : main)().catch((e) => {
  log('could not run:', e.stack ?? e);
  process.exit(2);
});
