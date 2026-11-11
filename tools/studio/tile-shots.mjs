// Headless, muted shots of the studio's asset tiles (the Variations strip,
// the Presets panel and the current combination's picture), before and after
// a change, and a check that each tile on screen is the engine's own picture
// of the asset in that material and style once drawing is ready.
//
//   node tools/studio/tile-shots.mjs --out <dir> [--tag before|after]
//        [--url http://localhost:8123/] [--widths 700,1280,2560] [--height 1000]
//        [--assets animal.horse,building.wall,animal.dog] [--styles default,WK08]
//        [--check] [--perf]
//
// Writes <dir>/<tag>__page-<w>.png, <dir>/<tag>__vars-<asset>-<style>.png,
// <dir>/<tag>__presets-<asset>-<style>.png and <dir>/<tag>__tiles.json. With
// --check it exits 1 when a tile on screen still shows the recoloured
// thumbnail after the page had time to draw it. --perf adds frame times on
// the High tier while every tile on screen is redrawn (a style change).
//
// Drives the page only through its own controls (element clicks, never the
// real mouse or keyboard); pointer lock, fullscreen and keyboard lock are
// stubbed. Chrome runs --headless=new and --mute-audio on a fresh full-path
// profile made under TEMP (set TEMP to choose where), closed by its own PID.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const url = arg("url", "http://localhost:8123/");
const out = arg("out", ".evani/shots/st-tiles");
const tag = arg("tag", "after");
const widths = arg("widths", "700,1280,2560").split(",").map(Number);
const H = Number(arg("height", "1000"));
const assets = arg("assets", "animal.horse,building.wall,animal.dog").split(",");
const styles = arg("styles", "default,WK08").split(",");
mkdirSync(out, { recursive: true });

const { cdp, stop } = await startChrome({ width: Math.max(...widths), height: H, prefix: "evani-sttiles-" });
const report = { url, tag, pages: [], closeups: [], logs: [] };
let failed = [];
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, 120000);
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + ";try{localStorage.clear();indexedDB.deleteDatabase('evani-studio')}catch(e){}" });
  cdp.on((m) => {
    if (m.sessionId === s && m.method === "Runtime.consoleAPICalled" && /error|warn/.test(m.params.type))
      report.logs.push(m.params.args.map((a) => a.value ?? a.description).join(" "));
  });
  const ev = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(expr.slice(0, 80) + ": " + JSON.stringify(r.exceptionDetails).slice(0, 300));
    return r.result.value;
  };
  const size = async (w, scale = 1) => {
    await send("Emulation.setDeviceMetricsOverride", { width: w, height: H, deviceScaleFactor: scale, mobile: false });
    await sleep(600);
  };
  const shoot = async (name, clip) => {
    const p = { format: "png" };
    if (clip) p.clip = { ...clip, scale: 1 };
    const r = await send("Page.captureScreenshot", p);
    writeFileSync(join(out, `${tag}__${name}.png`), Buffer.from(r.data, "base64"));
  };
  const rect = (sel) =>
    ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const r = e.getBoundingClientRect(); return r.width > 0 ? {x: r.x, y: r.y, width: r.width, height: r.height} : null; })()`);
  const pad = (r, p) => r && { x: Math.max(0, r.x - p), y: Math.max(0, r.y - p), width: r.width + 2 * p, height: r.height + 2 * p };
  const click = (act, v) =>
    ev(`(() => { const b = [...document.querySelectorAll('[data-act="${act}"]')].find(e => ${v === undefined ? "true" : `e.dataset.v === ${JSON.stringify(v)}`}); if (!b) return false; b.click(); return true; })()`);
  // A pick through the page's own delegated click handler (the asset's
  // thumbnail may sit in a closed category).
  const act = (a, v) =>
    ev(`(() => { const root = document.querySelector('#studio-page'); if (!root) return false; const b = document.createElement('button'); b.dataset.act = ${JSON.stringify(a)}; b.dataset.v = ${JSON.stringify(v)}; b.style.display = 'none'; root.appendChild(b); b.click(); b.remove(); return true; })()`);
  // Every tile picture on the page: drawn by the engine (its canvas painted)
  // or still the recoloured thumbnail.
  const TILES = "#st-vars .st-var:not(.st-add) .pic, .st-presets .st-preset .pic, .st-cur > .pic";
  const measure = () =>
    ev(`(() => [...document.querySelectorAll(${JSON.stringify(TILES)})].map(p => {
      const r = p.getBoundingClientRect(), c = p.querySelector('canvas[data-tile]');
      const row = p.closest('.st-vars-row'), pr = row && row.getBoundingClientRect();
      const visible = r.width > 0 && r.right > 0 && r.left < innerWidth && r.bottom > 0 && r.top < innerHeight && (!pr || (r.right > pr.left && r.left < pr.right));
      const out = { where: p.closest('.st-var') ? 'variation' : p.closest('.st-preset') ? 'preset' : 'current', material: p.closest('[data-v]')?.dataset.v || null, visible, canvas: !!c, painted: !!(c && c.dataset.k), not_ready: !!(c && c.dataset.nb), w: c ? c.width : 0 };
      if (!out.painted || !c.width) return out;
      const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data, n = c.width;
      let det = 0, cnt = 0;
      for (let y = 1; y < n - 1; y++) for (let x = 1; x < n - 1; x++) {
        const i = (y * n + x) * 4, l = (k) => 0.3 * d[k] + 0.59 * d[k + 1] + 0.11 * d[k + 2];
        det += Math.abs(l(i) - l(i + 4)) + Math.abs(l(i) - l(i + n * 4)); cnt++;
      }
      out.detail = +(det / Math.max(cnt, 1) / 2).toFixed(3);
      return out;
    }))()`);
  const settle = async (ms = 45000) => {
    const t = Date.now();
    while (Date.now() - t < ms) {
      const m = await measure();
      if (!m.some((x) => x.visible && x.canvas && !x.painted && !x.not_ready)) return true;
      await sleep(500);
    }
    return false;
  };

  await size(1280);
  await send("Page.navigate", { url });
  for (let i = 0; i < 240 && !(await ev(`!!document.querySelector('[data-act="mat"]')`)); i++) await sleep(500);
  await sleep(4000);
  // Two saved presets so the Presets panel shows saved tiles too.
  for (let i = 0; i < 2; i++) {
    const v = await ev(`[...document.querySelectorAll('#st-vars .st-var[data-act="var"]:not(.foreign):not(.on)')][${i}]?.dataset.v`);
    if (v) {
      await click("var", v);
      await sleep(900);
      await click("save");
      await sleep(900);
    }
  }
  for (const w of widths) {
    await size(w);
    const ok = await settle();
    await sleep(1500);
    await shoot(`page-${w}`);
    report.pages.push({ width: w, settled: ok, tiles: await measure() });
  }
  if (has("perf")) {
    const pick = (a, v) => ev(`(() => { const e = document.querySelector('[data-act="${a}"]'); if (!e) return false; e.value = ${JSON.stringify(v)}; e.dispatchEvent(new Event('change', {bubbles:true})); return true; })()`);
    await size(1280);
    await pick("quality", "high");
    await sleep(4000);
    await settle();
    const frames = (ms) => ev(`new Promise(res => { const t = []; let last = performance.now(); const end = last + ${ms}; const f = (now) => { t.push(now - last); last = now; if (now < end) requestAnimationFrame(f); else { t.sort((a, b) => a - b); const q = (p) => +t[Math.min(t.length - 1, Math.floor(p * t.length))].toFixed(1); res({ n: t.length, p50: q(0.5), p95: q(0.95), max: +t[t.length - 1].toFixed(1) }); } }; requestAnimationFrame(f); })`);
    const quiet = await frames(4000);
    const defStyle = await ev(`document.querySelector('[data-act="style"][aria-selected="true"]')?.dataset.v`);
    await click("style", "WK08");
    const busy = await frames(8000);
    const painted = (await measure()).filter((t) => t.painted).length;
    report.perf = { tier: await ev(`document.querySelector('[data-act="quality"]')?.value`), quiet, while_making_tiles: busy, tiles_painted_after: painted };
    console.log("perf", JSON.stringify(report.perf));
    await click("style", defStyle);
    await pick("quality", "auto");
    await sleep(3000);
  }
  await size(1280, 2);
  const defStyle = await ev(`document.querySelector('[data-act="style"][aria-selected="true"]')?.dataset.v`);
  for (const a of assets) {
    await act("asset", a);
    await sleep(1500);
    for (const st of styles) {
      await click("style", st === "default" ? defStyle : st);
      await sleep(1500);
      const ok = await settle(60000);
      await sleep(1200);
      const rv = await rect("#st-vars");
      if (rv) await shoot(`vars-${a}-${st}`, pad(rv, 4));
      const rp = await ev(`(() => { const e = document.querySelector('.st-presets')?.closest('.st-sec'); if (!e) return null; const r = e.getBoundingClientRect(); return {x: r.x, y: r.y, width: r.width, height: r.height}; })()`);
      if (rp) await shoot(`presets-${a}-${st}`, pad(rp, 4));
      report.closeups.push({ asset: a, style: st, settled: ok, tiles: await measure() });
    }
  }
  await click("style", defStyle);
  if (has("check")) {
    const all = [...report.pages.flatMap((p) => p.tiles), ...report.closeups.flatMap((c) => c.tiles)];
    const missing = all.filter((t) => t.visible && !t.painted && !t.not_ready);
    failed = missing.map((t) => ({ reason: "still the recoloured thumbnail", ...t }));
    report.check = { tiles: all.length, painted: all.filter((t) => t.painted).length, not_ready: all.filter((t) => t.not_ready).length, failed };
    if (!all.some((t) => t.painted)) failed.push({ reason: "no engine-drawn tile on the page" });
  }
} finally {
  writeFileSync(join(out, `${tag}__tiles.json`), JSON.stringify(report, null, 1));
  await stop();
}
if (failed.length) {
  console.log("THUMBNAIL TILES:", JSON.stringify(failed.slice(0, 20)));
  process.exit(1);
}
console.log("ok", tag, out);
