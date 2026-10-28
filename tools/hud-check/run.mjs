// hud_white_stays_white: the HUD-after-post check (S3-13b fix, docs/UI_LAYOUT.md
// "HUD after post"). It captures a `ui` build of the sandbox on every tier and
// view through the bench runner (headless Chrome, muted, pointer and keyboard
// locks stubbed), reads the screenshots and checks that the HUD panel lands on
// screen exactly as authored:
//   * white ink: the brightest green in each probe box (the panel's text in
//     ink 1.0) reads at least 254 (under the tone map it read light grey);
//   * the panel fill: every pixel of a strip inside the panel's padding is the
//     authored colour (linear PANEL in hudpanel.rs, as sRGB) within 2, so no
//     tone curve, exposure, vignette or bloom halo touches it.
// It also writes a labelled contact sheet (one panel per tier and view).
//
// Ink threshold per tier: a tier whose `render_scale` (assets/tiers/<tier>.json)
// is under 1 (Lite 0.65, Low 0.75) shrinks the canvas backing store itself and
// the browser stretches it to the page, which softens thin strokes for every
// sprite. That is a render-scale property, not post-processing, so on those
// tiers the brightest ink must reach 200 instead of 254 (under the tone map
// they read 145-182). The panel fill is exact on every tier.
//
//   node tools/hud-check/run.mjs --url http://127.0.0.1:8301/ --out <dir> --label after
//        [--tiers lite,low,medium,high,ultra] [--views 1280x720@1,844x390@3]
//        [--at 8000] [--expect fail]
//
// `--expect fail` inverts the exit code (for a build from before the fix,
// to show the check catches the defect). stdout: one line per capture and a
// summary; exit 0 when every capture passes (or, with `--expect fail`, when
// at least one fails). No npm packages: the PNG reader below uses node:zlib.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { inflateSync } from 'node:zlib';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i];
}
const here = dirname(fileURLToPath(import.meta.url));
const runner = join(here, '..', 'bench-runner', 'run.mjs');
const game = args.game ?? 'sandbox';
const label = args.label ?? 'after';
const tiers = (args.tiers ?? 'lite,low,medium,high,ultra').split(',');
const views = (args.views ?? '1280x720@1,844x390@3').split(',').map((v) => {
  const m = /^(\d+)x(\d+)@(\d+(?:\.\d+)?)$/.exec(v);
  if (!m) throw new Error('bad view ' + v + ' (want WxH@DPR)');
  return { name: v, w: Number(m[1]), h: Number(m[2]), dpr: Number(m[3]) };
});
const at = Number(args.at ?? 8000);
function inkMin(tier) {
  const f = join(here, '..', '..', 'assets', 'tiers', `${tier}.json`);
  if (!existsSync(f)) return 254;
  const scale = JSON.parse(readFileSync(f, 'utf8')).render_scale;
  return typeof scale === 'number' && scale < 1 ? 200 : 254;
}
if (!args.url || !args.out) {
  console.error('usage: --url <served ui build> --out <dir> [--label after]');
  process.exit(2);
}
const out = resolve(args.out);
mkdirSync(out, { recursive: true });

// ---- what the sandbox HUD panel authors (apps/sandbox/src/hudpanel.rs) ----
const PANEL = [0.02, 0.025, 0.045];
const MARGIN = 12;
const PAD = 14;
const srgb = (l) => Math.round(255 * (l <= 0.0031308 ? 12.92 * l : 1.055 * Math.pow(l, 1 / 2.4) - 0.055));
const PANEL_SRGB = PANEL.map(srgb);

/** Probe boxes in CSS px for a `w` x `h` view (the panel's layout rules). */
function probes(w, h) {
  const pw = Math.min(Math.max(w - 2 * MARGIN, 200), 340);
  if (h < 480) {
    // Short screen: top left; the title "Mission 3" (22 px, ink) sits first.
    const x0 = MARGIN;
    const y0 = MARGIN;
    return {
      fill: [x0 + 4, y0 + 3, x0 + pw - 4, y0 + PAD - 4],
      white: [['title', [x0 + PAD, y0 + PAD, x0 + PAD + 110, y0 + PAD + 26]]],
    };
  }
  // Tall: bottom right; the two buttons (36 px, ink labels) end the panel.
  const x1 = w - MARGIN;
  const y1 = h - MARGIN;
  const x0 = x1 - pw;
  const by0 = y1 - PAD - 36;
  const mid = (x0 + x1) / 2;
  return {
    fill: [x0 + 4, y1 - PAD + 4, x1 - 4, y1 - 3],
    white: [
      ['Resume', [x0 + PAD, by0, mid - 5, y1 - PAD]],
      ['Quit', [mid + 5, by0, x1 - PAD, y1 - PAD]],
    ],
  };
}

// ---- a small PNG reader (8-bit RGB/RGBA, non-interlaced: Chrome's screenshots) ----
function readPng(file) {
  const b = readFileSync(file);
  let p = 8;
  let w = 0, h = 0, ct = 0;
  const idat = [];
  while (p < b.length) {
    const len = b.readUInt32BE(p);
    const type = b.toString('ascii', p + 4, p + 8);
    const data = b.subarray(p + 8, p + 8 + len);
    if (type === 'IHDR') {
      w = data.readUInt32BE(0);
      h = data.readUInt32BE(4);
      if (data[8] !== 8 || data[12] !== 0) throw new Error(file + ': only 8-bit non-interlaced PNG');
      ct = data[9];
    } else if (type === 'IDAT') idat.push(data);
    else if (type === 'IEND') break;
    p += 12 + len;
  }
  const bpp = ct === 6 ? 4 : ct === 2 ? 3 : 0;
  if (!bpp) throw new Error(file + ': colour type ' + ct);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const px = Buffer.alloc(w * h * 4);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    const line = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? line[i - bpp] : 0;
      const up = prev[i];
      const c = i >= bpp ? prev[i - bpp] : 0;
      let v = line[i];
      if (f === 1) v += a;
      else if (f === 2) v += up;
      else if (f === 3) v += (a + up) >> 1;
      else if (f === 4) {
        const pp = a + up - c;
        const pa = Math.abs(pp - a), pb = Math.abs(pp - up), pc = Math.abs(pp - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? up : c;
      }
      line[i] = v & 255;
    }
    for (let x = 0; x < w; x++) {
      for (let k = 0; k < 3; k++) px[(y * w + x) * 4 + k] = line[x * bpp + k];
      px[(y * w + x) * 4 + 3] = bpp === 4 ? line[x * bpp + 3] : 255;
    }
    prev = line;
  }
  return { w, h, px };
}

/** Pixels of a CSS-px box (scaled by dpr), as [r,g,b] arrays. */
function box(img, [x0, y0, x1, y1], dpr) {
  const out = [];
  const X0 = Math.max(0, Math.ceil(x0 * dpr)), X1 = Math.min(img.w, Math.floor(x1 * dpr));
  const Y0 = Math.max(0, Math.ceil(y0 * dpr)), Y1 = Math.min(img.h, Math.floor(y1 * dpr));
  for (let y = Y0; y < Y1; y++) {
    for (let x = X0; x < X1; x++) {
      const i = (y * img.w + x) * 4;
      out.push([img.px[i], img.px[i + 1], img.px[i + 2]]);
    }
  }
  return out;
}

function judge(file, view, inkMin) {
  const img = readPng(file);
  const pr = probes(view.w, view.h);
  const fill = box(img, pr.fill, view.dpr);
  let worst = 0;
  let worstPx = null;
  for (const p of fill) {
    const d = Math.max(...p.map((v, k) => Math.abs(v - PANEL_SRGB[k])));
    if (d > worst) {
      worst = d;
      worstPx = p;
    }
  }
  const whites = pr.white.map(([name, b]) => {
    const px = box(img, b, view.dpr);
    const best = px.reduce((m, p) => (p[1] > m[1] ? p : m), [0, 0, 0]);
    return { name, brightest: best, ink_min: inkMin, ok: best[1] >= inkMin && best[0] >= inkMin };
  });
  const fillOk = fill.length > 0 && worst <= 2;
  return {
    ok: fillOk && whites.every((w) => w.ok),
    fill: { expected: PANEL_SRGB, worst_diff: worst, worst_px: worstPx, samples: fill.length, ok: fillOk },
    white: whites,
  };
}

// ---- capture ----
const jobs = [];
for (const view of views) {
  for (const tier of tiers) {
    const name = `${label}-${tier}-${view.name.replace('@', 'at')}`;
    // The DOM overlays (debug text, the sandbox hint) are hidden: only the canvas is judged.
    jobs.push({ tier, label: name, css: '*{visibility:hidden!important} canvas{visibility:visible!important}', width: view.w, height: view.h, dpr: view.dpr, storage: { [`evani:${game}:render.tier`]: tier }, view });
  }
}
const run = spawnSync(
  process.execPath,
  [runner, '--url', args.url, '--page-ms', String(at + 1000), '--shots', out, '--shots-at', String(at), '--jobs', JSON.stringify(jobs.map(({ view, ...j }) => j))],
  { encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'inherit'] },
);
if (run.status !== 0) {
  console.error('hud-check: the bench runner failed (' + run.status + ')');
  process.exit(2);
}
const report = JSON.parse(run.stdout || '{}');

const results = [];
for (const [i, job] of jobs.entries()) {
  const file = join(out, `${job.label}-${at}ms.png`);
  const r = report.jobs?.[i] ?? {};
  let res;
  if (!existsSync(file)) res = { ok: false, error: r.error ?? 'no screenshot' };
  else res = judge(file, job.view, inkMin(job.tier));
  res.console = r.console ?? [];
  if (r.page?.error) res.engine_error = r.page.error;
  results.push({ tier: job.tier, view: job.view.name, file, ...res });
  const w = (res.white ?? []).map((x) => `${x.name} ${x.brightest.join(',')}`).join('; ');
  console.log(
    `hud_white_stays_white ${job.tier} ${job.view.name}: ${res.ok ? 'PASS' : 'FAIL'}` +
      (res.fill ? ` (fill worst diff ${res.fill.worst_diff} from ${PANEL_SRGB.join(',')}; brightest ink ${w})` : ` (${res.error})`),
  );
}
writeFileSync(join(out, `${label}-results.json`), JSON.stringify({ adapter: report.adapter, chrome: report.version, at_ms: at, results }, null, 2));

// ---- the labelled contact sheet: columns are tiers, rows are views ----
const cols = tiers.length;
const cellW = 520;
const cells = results
  .map((r) => {
    const verdict = r.ok ? 'PASS' : 'FAIL';
    const fill = r.fill ? `fill ${r.fill.worst_px ? r.fill.worst_px.join(',') : `exact on ${r.fill.samples} px`} (want ${PANEL_SRGB.join(',')})` : r.error;
    const ink = (r.white ?? []).map((x) => `${x.name} ${x.brightest.join(',')}`).join(' / ') + (r.white?.length ? ` (need ${r.white[0].ink_min})` : '');
    return `<figure><img src="${pathToFileURL(r.file).href}"><figcaption><b>${r.tier[0].toUpperCase() + r.tier.slice(1)}</b> ${r.view} <i class="${verdict}">${verdict}</i><br>${ink}<br>${fill}</figcaption></figure>`;
  })
  .join('\n');
const html = `<!doctype html><meta charset="utf-8"><style>
body{margin:0;padding:12px;background:#222;color:#eee;font:13px/1.35 system-ui,sans-serif}
h1{font-size:18px;margin:0 0 10px}
.grid{display:grid;grid-template-columns:repeat(${cols},${cellW}px);gap:10px}
figure{margin:0;background:#111;padding:6px}
img{width:100%;display:block}
figcaption{padding-top:5px}
b{font-size:16px}.PASS{color:#6f6}.FAIL{color:#f66}
</style><h1>${game} ui build, ${label}: HUD panel on every tier (hud_white_stays_white)</h1><div class="grid">${cells}</div>`;
const sheetHtml = join(out, `${label}-sheet.html`);
writeFileSync(sheetHtml, html);
const rows = views.length;
const sheetH = 60 + rows * Math.round(cellW * 0.62 + 70);
const sheet = spawnSync(
  process.execPath,
  [runner, '--url', pathToFileURL(sheetHtml).href, '--page-ms', '2500', '--shots', out, '--shots-at', '2000', '--jobs', JSON.stringify([{ tier: 'sheet', label: `${label}-sheet`, width: 24 + cols * (cellW + 10), height: sheetH }])],
  { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] },
);
if (sheet.status === 0) console.log('sheet: ' + join(out, `${label}-sheet-2000ms.png`));

const failed = results.filter((r) => !r.ok).length;
console.log(`hud_white_stays_white: ${results.length - failed} of ${results.length} captures pass`);
const want = args.expect === 'fail';
process.exit(want ? (failed > 0 ? 0 : 1) : failed > 0 ? 1 : 0);
