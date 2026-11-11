// How quickly the spectral sea shows in the studio, and how quickly its
// settings take effect (Chris: "every live setting shows its effect within
// a frame or two", "the sea shows quickly at start-up").
//
//   node tools/nature/sea-latency.mjs --site <built material-view folder> --out <dir>
//        [--gpu] (default SwiftShader, the CPU) [--size 1280x800] [--timeout-s 420]
//
// Serves the built page, opens it in a headless, muted Chrome (fresh
// full-path profile, locks stubbed, closed by its own PID), switches to
// Procedural > Open sea through the page's own buttons (CDP-evaluated
// clicks), and measures:
//   * from navigation (and from the click) to the first picture of the sea,
//     and to the full sea (the page's `data-evani-preparing` back to 0);
//   * per setting group, from moving its slider to the first changed
//     picture (ms, and the page's frames meanwhile), from the browser's own
//     small screenshots of the view.
// Writes <out>/latency.json and pictures of the first and the full sea.
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync, existsSync, statSync } from "node:fs";
import { join, extname, resolve } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";
import { readPng } from "./png-small.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const site = resolve(arg("site", "."));
const out = arg("out", ".evani/shots/sea-latency");
const [W, H] = arg("size", "1280x800").split("x").map(Number);
if (!site || !existsSync(join(site, "index.html"))) {
  console.error("--site <folder with index.html> is needed");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".png": "image/png", ".glb": "model/gltf-binary", ".css": "text/css" };
const server = createServer((req, res) => {
  let p = decodeURIComponent((req.url || "/").split("?")[0]);
  if (p.endsWith("/")) p += "index.html";
  const f = join(site, p);
  if (!f.startsWith(site) || !existsSync(f) || statSync(f).isDirectory()) {
    res.writeHead(404);
    return res.end();
  }
  res.writeHead(200, { "content-type": types[extname(f)] || "application/octet-stream", "cache-control": "no-store" });
  res.end(readFileSync(f));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/`;
const flags = process.argv.includes("--gpu") ? [] : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader"];
const { cdp, stop } = await startChrome({ width: W, height: H, flags, prefix: "evani-sealatency-" });
const limit = Number(arg("timeout-s", "420")) * 1000;
const report = { url, gpu: process.argv.includes("--gpu") };
const finish = async (c) => {
  writeFileSync(join(out, "latency.json"), JSON.stringify(report, null, 1));
  await stop().catch(() => {});
  server.close();
  process.exit(c);
};
setTimeout(() => {
  report.error = (report.error || "") + " time limit";
  finish(3);
}, limit).unref();

// In-page helper: a frame counter (requestAnimationFrame), so a wait can be
// told in frames as well as milliseconds.
const HELPERS = `
window.__rafN = 0;
(function tick() { window.__rafN++; requestAnimationFrame(tick); })();
`;
let S = null;
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  S = s;
  const send = (m, p, ms = 120000) => cdp.send(m, p, s, ms);
  const ev = async (expr, ms) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }, ms);
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails).slice(0, 400));
    return r.result.value;
  };
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + HELPERS });
  const tNav = Date.now();
  await send("Page.navigate", { url });
  const until = async (expr, what, ms = 240000) => {
    const t = Date.now();
    while (Date.now() - t < ms) {
      if (await ev(expr).catch(() => false)) return Date.now() - tNav;
      await sleep(100);
    }
    throw new Error("timed out waiting for " + what);
  };
  // The live view as a small grid of colours, from the browser's own
  // screenshot of the view (what the user sees, composited).
  let clip = null;
  const grab = async () => {
    if (!clip) {
      const r = await ev(`(() => { const e = document.getElementById('st-view') || document.getElementById('evani-canvas'); const b = e.getBoundingClientRect(); return { x: b.left, y: b.top, width: b.width, height: b.height }; })()`);
      clip = { x: r.x + r.width * 0.1, y: r.y + r.height * 0.2, width: r.width * 0.8, height: r.height * 0.7, scale: 64 / (r.width * 0.8) };
    }
    const shot = await send("Page.captureScreenshot", { format: "png", clip });
    return readPng(Buffer.from(shot.data, "base64")).data;
  };
  const diff = (a, b) => {
    let d = 0;
    const n = Math.min(a.length, b.length);
    for (let i = 0; i < n; i += 4) d += Math.abs(a[i] - b[i]) + Math.abs(a[i + 1] - b[i + 1]) + Math.abs(a[i + 2] - b[i + 2]);
    return d / (n / 4) / 3;
  };
  const changeAfter = async (before, ms, thr = 1.2) => {
    const t0 = Date.now();
    const f0 = await ev(`window.__rafN`);
    while (Date.now() - t0 < ms) {
      const g = await grab();
      const d = diff(g, before);
      if (d > thr) return { ms: Date.now() - t0, frames: (await ev(`window.__rafN`)) - f0, diff: +d.toFixed(2) };
    }
    return { ms: -1, frames: -1 };
  };
  report.page_ready_ms = await until(`!!document.querySelector('[data-act="mode"][data-v="proc"]')`, "the page");
  await ev(`document.querySelector('[data-act="mode"][data-v="proc"]').click(), true`);
  await until(`!!document.querySelector('[data-act="proc-item"][data-v="sea"]')`, "the procedural cards");
  // The first item (the old Ocean) settles, then the Open sea is opened.
  await ev(`new Promise(r => { let k = 0; const f = () => (++k >= 30 ? r(k) : requestAnimationFrame(f)); requestAnimationFrame(f); })`, 240000);
  const before = await grab();
  const prep0 = await ev(`document.documentElement.getAttribute('data-evani-preparing')`);
  const tClick = Date.now();
  await ev(`document.querySelector('[data-act="proc-item"][data-v="sea"]').click(), true`);
  report.sea_clicked_ms = tClick - tNav;
  report.preparing_before = prep0;
  // The sea's scene shows (the quick stand-in first if the full sea is
  // still building).
  const first = await changeAfter(before, 240000, 3.0);
  report.first_sea_after_click = first;
  report.first_sea_ms = first.ms < 0 ? -1 : tClick - tNav + first.ms;
  const s1 = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(out, "first-sea.png"), Buffer.from(s1.data, "base64"));
  // The full sea: the page's count of pipelines being built goes back to
  // 0 (it rises when the sea's pipelines start).
  let seen = 0;
  const tFull = Date.now();
  while (Date.now() - tFull < 240000) {
    const p = Number(await ev(`document.documentElement.getAttribute('data-evani-preparing') || 0`));
    seen = Math.max(seen, p);
    if (p === 0 && (seen > 0 || Date.now() - tFull > 3000)) break;
    await sleep(100);
  }
  report.full_sea_ms = Date.now() - tNav;
  report.full_sea_after_click_ms = Date.now() - tClick;
  report.preparing_peak_after_click = seen;
  await ev(`new Promise(r => { let k = 0; const f = () => (++k >= 20 ? r(k) : requestAnimationFrame(f)); requestAnimationFrame(f); })`, 240000);
  const s2 = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(out, "full-sea.png"), Buffer.from(s2.data, "base64"));
  // Each group: move one slider, wait for the first changed picture.
  const groups = [
    ["spectrum (wind speed)", "material.nature.water.wind_speed", 20],
    ["heights (wave height)", "material.nature.water.wave_height", 2],
    ["choppiness", "material.nature.water.choppiness", 0.95],
    ["foam (whitecaps)", "material.nature.water.whitecap_amount", 2],
    ["colour (scatter colour)", "material.nature.water.scatter_color", "#3a7a2a"],
    ["light (glint)", "material.nature.water.glint", 6],
    ["beach (slope)", "material.nature.water.beach_slope", 0.08],
    ["quality switch (Low)", "quality", 0],
  ];
  report.settings = [];
  for (const [name, param, value] of groups) {
    await ev(`new Promise(r => { let k = 0; const f = () => (++k >= 8 ? r(k) : requestAnimationFrame(f)); requestAnimationFrame(f); })`, 240000);
    const b = await grab();
    const ok = await ev(`(async () => {
      if (${JSON.stringify(param)} === 'quality') {
        const q = document.querySelector('[data-act="proc-quality"][data-v="${value}"]');
        if (!q) return false;
        q.click();
        return true;
      }
      if (!document.querySelector('input[data-v="${param}"]')) {
        const adv = document.querySelector('[data-act="proc-adv"]');
        if (adv) adv.click();
        await new Promise(r => setTimeout(r, 300));
      }
      const j = document.querySelector('input[data-v="${param}"]');
      if (!j) return false;
      j.value = ${JSON.stringify(String(value))};
      j.dispatchEvent(new Event('input', { bubbles: true }));
      return true;
    })()`);
    if (!ok) {
      report.settings.push({ name, param, missing: true });
      continue;
    }
    report.settings.push({ name, param, ...(await changeAfter(b, 60000)) });
  }
  console.log(JSON.stringify(report, null, 1));
  await finish(0);
} catch (e) {
  report.error = String(e && e.stack || e);
  try {
    const s3 = await cdp.send("Page.captureScreenshot", { format: "png" }, S, 20000);
    writeFileSync(join(out, "error.png"), Buffer.from(s3.data, "base64"));
  } catch (_) {}
  console.error(report.error);
  await finish(1);
}
