// Contact sheets of the batch-2 rows from the material viewer, in one headless muted Chrome session.
//   node tools/materials/sheet.mjs <dist> <outdir> --idsfile <ids.json> [--port 8113] [--only ids,comma] [--skip-styles] [--close] [--title T]
// Rows the viewer cannot draw (held, status planned) are NOT shot; the sheet shows a "held: <reason>" tile for them
// (tools/materials/sheet_cells.mjs; tested by sheet_cells.test.mjs).
// Writes <outdir>/{high,low}/<id>.png, <outdir>/styles/<style>-<id>.png and the sheets
// <outdir>/sheet-high.png, sheet-low.png, sheet-styles.png.
import { createServer } from 'node:http';
import { readFileSync, writeFileSync, existsSync, mkdtempSync, rmSync, mkdirSync } from 'node:fs';
import { join, extname } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { cellHtml, isDrawn, sheetCells } from './sheet_cells.mjs';

const argv = process.argv.slice(2);
const [dist, out] = argv;
const arg = (k, d) => (argv.includes(k) ? argv[argv.indexOf(k) + 1] : d);
const PORT = Number(arg('--port', 8113));
const ROOT = fileURLToPath(new URL('../../', import.meta.url));
const mat = JSON.parse(readFileSync(ROOT + 'catalog/materials/materials.json', 'utf8'));
const planned = JSON.parse(readFileSync(ROOT + 'kits/evani-kit-material-view/data/planned.json', 'utf8'));
const MINE = JSON.parse(readFileSync(arg('--idsfile', 'ids.json'), 'utf8')); let rows = mat.rows.filter((r) => MINE.includes(r.id));
const shootRows = () => rows.filter(isDrawn);
if (argv.includes('--only')) { const o = new Set(arg('--only', '').split(',')); rows = rows.filter((r) => o.has(r.id)); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const TIERS = (arg('--tiers', 'low,ultra')).split(',');
for (const d of [...TIERS, 'styles']) mkdirSync(join(out, d), { recursive: true });

// --auto-ground: ground rows on their own shape (the proof moments' way) instead of a tile.
const shapeOf = (r) => (!argv.includes('--auto-ground') && (r.recipe === 'ground' || r.id.startsWith('ground.')) ? 'tile' : 'auto');
const CLIP = { tile: [450, 300, 340, 120], auto: [350, 150, 580, 420] };
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.png': 'image/png', '.json': 'application/json' };
const server = createServer((req, res) => {
  let p = decodeURIComponent(req.url.split('?')[0]);
  let f;
  if (p.startsWith('/_s/')) f = join(out, p.slice(4)); else { if (p.endsWith('/')) p += 'index.html'; f = join(dist, p); }
  if (!existsSync(f)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': types[extname(f)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
  res.end(readFileSync(f));
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));
const chromePath = [join(process.env.ProgramFiles ?? '', 'Google/Chrome/Application/chrome.exe'), join(process.env.LOCALAPPDATA ?? '', 'Google/Chrome/Application/chrome.exe')].find(existsSync);
const profile = mkdtempSync(join(tmpdir(), 'rcr1-'));
const chrome = spawn(chromePath, ['--headless=new', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--mute-audio',
  '--enable-unsafe-webgpu', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--window-size=1280,720',
  // Frames must not wait for the display: with the screen locked or asleep a WebGPU page gets none and every step times out.
  '--disable-gpu-vsync', '--disable-frame-rate-limit', 'about:blank'], { stdio: 'ignore', windowsHide: true });
let ws = null;
for (let i = 0; i < 200 && !ws; i++) {
  const f = join(profile, 'DevToolsActivePort');
  if (existsSync(f)) { const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/); if (p && path) ws = `ws://127.0.0.1:${p}${path}`; }
  if (!ws) await sleep(100);
}
const sock = new WebSocket(ws);
await new Promise((r) => sock.addEventListener('open', r, { once: true }));
let id = 0; const pend = new Map(); const logs = [];
sock.addEventListener('message', (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pend.has(m.id)) { const p = pend.get(m.id); pend.delete(m.id); m.error ? p.rej(new Error(m.error.message)) : p.res(m.result); }
  else if (m.method === 'Runtime.exceptionThrown') logs.push('EXC ' + m.params.exceptionDetails?.exception?.description);
});
let sid;
const send = (method, params = {}) => new Promise((res, rej) => { const i = ++id; const t = setTimeout(() => { pend.delete(i); rej(new Error('timeout: ' + method)); }, Number(arg('--cdp-ms', 180000))); pend.set(i, { res: (v) => { clearTimeout(t); res(v); }, rej: (e) => { clearTimeout(t); rej(e); } }); sock.send(JSON.stringify({ id: i, method, params, ...(sid ? { sessionId: sid } : {}) })); });
const ev = async (expression) => { const r = await send('Runtime.evaluate', { awaitPromise: true, returnByValue: true, expression }); return r.result?.value ?? r.exceptionDetails?.exception?.description; };
const setp = (values) => ev(`window.evani.live.call('param.set',{values:${JSON.stringify(values)}},{note:'sheet',telemetry_ms:0}).then(r=>JSON.stringify((r.applied||[]).map(a=>a.name+'='+a.new)))`);
// --size WxH: the page at another size (a phone: 390x844); each shot is then the whole page, not a clip.
const SIZE = argv.includes('--size') ? arg('--size', '1280x720').split('x').map(Number) : null;
const shot = async (file, [x, y, w, h], scale) => {
  const s = await send('Page.captureScreenshot', { format: 'png', fromSurface: true, ...(SIZE ? {} : { clip: { x, y, width: w, height: h, scale } }) });
  writeFileSync(file, Buffer.from(s.data, 'base64'));
};
const GROUPS = ['detail', 'fur', 'water', 'glass', 'skin', 'foliage', 'conditions', 'reflections'];
const TEN = ['leaf.maple.green', 'needle.spruce', 'ground.grass.alpine', 'ground.gravel.scree', 'rock.granite.pink', 'bark.beech', 'bark.dead_snag', 'ground.gravel.cinders', 'ground.ice.sheet_road', 'ground.moss.carpet'];
const STY = [[23, 'realism'], [1, 'toon'], [5, 'painterly'], [6, 'lowpoly'], [8, 'voxel']];
try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  ({ sessionId: sid } = await send('Target.attachToTarget', { targetId, flatten: true }));
  await send('Page.enable'); await send('Runtime.enable');
  await send('Page.addScriptToEvaluateOnNewDocument', { source: `(() => { const n = () => Promise.resolve();
    try { Element.prototype.requestPointerLock = n; } catch (e) {} try { Element.prototype.requestFullscreen = n; } catch (e) {}
    try { if (navigator.keyboard) navigator.keyboard.lock = n; } catch (e) {} })();` });
  const [VW, VH] = SIZE ?? [1280, 720];
  await send('Emulation.setDeviceMetricsOverride', { width: VW, height: VH, deviceScaleFactor: 1, mobile: false, screenWidth: VW, screenHeight: VH });
  const COMPOSE = argv.includes('--compose-only');
  if (!COMPOSE) { await send('Page.navigate', { url: `http://127.0.0.1:${PORT}/` });
  await sleep(15000); }
  await ev(`(()=>{const b=document.querySelector('#mv-st-23'); if(b) b.click(); return 1})()`);
  await sleep(1500);
  // --still: the camera's slow orbit stops too, so two runs frame the same pixels (pixel diffs).
  console.log('orbit', await setp({ 'kit.material_view.turntable_speed': 0, ...(argv.includes('--still') ? { 'kit.material_view.orbit_speed': 0 } : {}) }));
  const TH = 1.0;
  for (const q of (argv.includes('--styles-only') || COMPOSE ? [] : TIERS)) {
    const qv = {}; for (const g of GROUPS) qv['material.quality.' + g] = q;
    console.log('quality', q, await setp(qv));
    await sleep(800);
    for (const r of shootRows()) {
      const sh = shapeOf(r);
      await setp({ 'kit.material_view.cat.open': r.id, 'kit.material_view.cat.shape': sh });
      // --settle-ms: how long the view settles after a row opens (pixel diffs want it still).
      await sleep(Number(arg('--settle-ms', sh === 'tile' ? 1100 : 900)));
      await shot(join(out, q, r.id + '.png'), CLIP[sh], sh === 'tile' ? 1.0 : TH);
      if (argv.includes('--close')) {
        // Synthetic keys in the hidden page only: Q held to zoom in, then R (the material's own camera).
        const key = async (code, k, vk, ms) => { await send('Input.dispatchKeyEvent', { type: 'keyDown', code, key: k, windowsVirtualKeyCode: vk }); await sleep(ms); await send('Input.dispatchKeyEvent', { type: 'keyUp', code, key: k, windowsVirtualKeyCode: vk }); };
        await key('KeyQ', 'q', 81, Number(arg('--zoom-ms', 900))); await sleep(900);
        await shot(join(out, q, r.id + '.close.png'), CLIP.auto, 1.0);
        await key('KeyR', 'r', 82, 80); await sleep(400);
      }
    }
    console.log('done', q, shootRows().length, 'shot,', rows.length - shootRows().length, 'held');
  }
  if (!argv.includes('--skip-styles')) {
    await setp(Object.fromEntries(GROUPS.map((g) => ['material.quality.' + g, 'high'])));
    for (const [si, sn] of STY) {
      await ev(`(()=>{const b=document.querySelector('#mv-st-${si}'); if(b) b.click(); return 1})()`);
      await sleep(2500);
      for (const rid of TEN) {
        const r = rows.find((x) => x.id === rid); if (!r) continue;
        const sh = shapeOf(r);
        await setp({ 'kit.material_view.cat.open': rid, 'kit.material_view.cat.shape': sh });
        await sleep(1200);
        await shot(join(out, 'styles', `${sn}-${rid}.png`), CLIP[sh], sh === 'tile' ? 1.0 : TH);
      }
      console.log('done style', sn);
    }
  }
  const html = (title, cells, cols) => `<!doctype html><meta charset=utf-8><body style="margin:0;background:#1d2229;color:#dfe6ee;font:12px system-ui"><h3 style="margin:8px">${title}</h3><div style="display:grid;grid-template-columns:repeat(${cols},1fr);gap:6px;padding:6px">${cells.join('')}</div>`;
  for (const q of (argv.includes('--styles-only') ? [] : TIERS)) {
    writeFileSync(join(out, `sheet-${q}.html`), html(`${arg('--title','W3')}, ${rows.length} rows, ${q}`, sheetCells(rows, q, planned, argv.includes('--close')).map(cellHtml), 8));
  }
  const sheets = argv.includes('--styles-only') ? [] : TIERS.map((t) => 'sheet-' + t);
  if (!argv.includes('--skip-styles')) {
    writeFileSync(join(out, 'sheet-styles.html'), html('Batch 2, 10 rows in 5 looks (realism, toon, painterly, low-poly, voxel); styles show colour, not detail, until Stage 8',
      TEN.flatMap((rid) => STY.map(([, sn]) => cellHtml({ src: `/_s/styles/${sn}-${rid}.png`, caption: `${rid} - ${sn}` }))), 5));
    sheets.push('sheet-styles');
  }
  for (const name of sheets) {
    await send('Emulation.setDeviceMetricsOverride', { width: 2600, height: 1000, deviceScaleFactor: 1, mobile: false, screenWidth: 2600, screenHeight: 1000 });
    await send('Page.navigate', { url: `http://127.0.0.1:${PORT}/_s/${name}.html` });
    await sleep(3000);
    const h = await ev('document.documentElement.scrollHeight');
    await send('Emulation.setDeviceMetricsOverride', { width: 2600, height: Math.min(h, 16000), deviceScaleFactor: 1, mobile: false, screenWidth: 2600, screenHeight: Math.min(h, 16000) });
    await sleep(1500);
    const s = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    writeFileSync(join(out, name + '.png'), Buffer.from(s.data, 'base64'));
    console.log('sheet', name, h);
  }
} catch (e) { console.log('ERR', e.message); }
console.log(logs.join('\n'));
try { sock.close(); } catch (e) {}
chrome.kill();
server.close();
await sleep(500);
try { rmSync(profile, { recursive: true, force: true }); } catch (e) {}
console.log('SHEET-DONE');
