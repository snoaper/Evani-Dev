// Headless, muted shots of the studio's material balls (the inspector ball,
// the combination card's ball and the materials bar's chips), before and
// after a change, and a pixel check that each ball shows a real material.
//
//   node tools/studio/ball-shots.mjs --out <dir> [--tag before|after]
//        [--url http://localhost:8123/] [--widths 700,1280,2560] [--height 1000]
//        [--materials brick.red.common,wood.walnut.stock,horse.coat.bay,metal.steel.brushed]
//        [--styles default,WK24] [--check] [--perf]
//
// Writes <dir>/<tag>__page-<w>.png (the whole page at each width),
// <dir>/<tag>__chips-<search>.png (the chips row, close up),
// <dir>/<tag>__inspector-<style>-<material>.png (the inspector ball, close
// up), <dir>/<tag>__card-<style>-<material>.png and <dir>/<tag>__balls.json
// (each ball's pixel measures). With --check it exits 1 when a ball on
// screen is not the engine's picture, or a textured material's ball (brick,
// wood, a coat) is a plain tint: too little fine detail inside it. --perf
// adds frame times on the High tier while every ball on screen is redrawn.
//
// Drives the page only through its own controls (element clicks and input
// events, never the real mouse or keyboard); pointer lock, fullscreen and
// keyboard lock are stubbed. Chrome runs --headless=new and --mute-audio on a
// fresh full-path profile (made under TEMP, so set TEMP to choose where),
// closed by its own PID.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const url = arg("url", "http://localhost:8123/");
const out = arg("out", ".evani/shots/st-balls");
const tag = arg("tag", "after");
const widths = arg("widths", "700,1280,2560").split(",").map(Number);
const H = Number(arg("height", "1000"));
const materials = arg("materials", "brick.red.common,wood.walnut.stock,horse.coat.bay,metal.steel.brushed").split(",");
const searches = arg("searches", "brick,wood,fur,metal").split(",");
// The inspector close-ups in the opening style and in a realistic one (WK24, Cinematic Realism:
// the engine's own light, no toon bands), where metal shows its reflections.
const styles = arg("styles", "default,WK24").split(",");
const textured = arg("textured", "brick.red.common,wood.walnut.stock,horse.coat.bay").split(",");
mkdirSync(out, { recursive: true });

const { cdp, stop } = await startChrome({ width: Math.max(...widths), height: H, prefix: "evani-stballs-" });
const report = { url, tag, pages: [], chips: [], inspector: [], card: [], logs: [] };
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
  const search = (q) =>
    ev(`(() => { const e = document.querySelector('[data-act="mat-q"]'); if (!e) return false; e.value = ${JSON.stringify(q)}; e.dispatchEvent(new Event('input', {bubbles:true})); return true; })()`);
  // Every engine-drawn ball on the page (a canvas the renderer painted):
  // its fine detail (mean difference to its right and lower neighbours,
  // inside the ball's circle), its colour spread and how many it measured.
  const measure = (sel) =>
    ev(`(() => [...document.querySelectorAll(${JSON.stringify(sel)})].map(c => {
      const r = c.getBoundingClientRect();
      const out = { material: c.dataset.ball || c.closest('[data-v]')?.dataset.v, painted: c.dataset.k || null, not_ready: !!c.dataset.nb, w: c.width, visible: r.width > 0 && r.right > 0 && r.left < innerWidth && (() => { const row = c.closest(".st-chips"); if (!row) return true; const p = row.getBoundingClientRect(); return r.right > p.left && r.left < p.right; })() };
      if (!c.dataset.k || !c.width) return out;
      const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data, n = c.width, h = n / 2, rr = (n / 2 - 3) ** 2;
      let det = 0, cnt = 0, sum = [0,0,0], sq = 0;
      for (let y = 1; y < n - 1; y++) for (let x = 1; x < n - 1; x++) {
        if ((x - h) ** 2 + (y - h) ** 2 > rr) continue;
        const i = (y * n + x) * 4, l = (k) => 0.3 * d[k] + 0.59 * d[k + 1] + 0.11 * d[k + 2];
        const v = l(i), a = l(i + 4), b = l(i + n * 4);
        det += Math.abs(v - a) + Math.abs(v - b); cnt++; sum[0] += d[i]; sum[1] += d[i+1]; sum[2] += d[i+2]; sq += v * v;
      }
      out.detail = +(det / Math.max(cnt, 1) / 2).toFixed(3);
      out.mean = sum.map(v => Math.round(v / Math.max(cnt, 1)));
      out.pixels = cnt;
      return out;
    }))()`);

  await size(1280);
  await send("Page.navigate", { url });
  for (let i = 0; i < 240 && !(await ev(`!!document.querySelector('[data-act="mat"]')`)); i++) await sleep(500);
  await sleep(4000);
  // A ball is drawn when the page has painted it (after) or at once (before).
  const settle = async (sel, ms = 30000) => {
    const t = Date.now();
    while (Date.now() - t < ms) {
      const left = await ev(`[...document.querySelectorAll(${JSON.stringify(sel)})].filter(c => { const r = c.getBoundingClientRect(); const row = c.closest(".st-chips"), p = row && row.getBoundingClientRect(); return r.width > 0 && r.right > 0 && r.left < innerWidth && (!p || (r.right > p.left && r.left < p.right)) && !c.dataset.k && !c.dataset.nb; }).length`);
      if (!left) return true;
      await sleep(500);
    }
    return false;
  };
  const BALLS = "canvas[data-ball]";

  for (const w of widths) {
    await size(w);
    await settle(BALLS);
    await sleep(1500);
    await shoot(`page-${w}`);
    report.pages.push({ width: w, balls: await measure(BALLS) });
  }
  // The flagship-phone tier: on High, a style change makes every ball on
  // screen again; frame times while they are made (requestAnimationFrame
  // gaps over 6 s) against a quiet stretch before.
  if (has("perf")) {
    const pick = (act, v) => ev(`(() => { const e = document.querySelector('[data-act="${act}"]'); if (!e) return false; e.value = ${JSON.stringify(v)}; e.dispatchEvent(new Event('change', {bubbles:true})); return true; })()`);
    await pick("quality", "high");
    await sleep(4000);
    const frames = (ms) => ev(`new Promise(res => { const t = []; let last = performance.now(); const end = last + ${ms}; const f = (now) => { t.push(now - last); last = now; if (now < end) requestAnimationFrame(f); else { t.sort((a, b) => a - b); const q = (p) => +t[Math.min(t.length - 1, Math.floor(p * t.length))].toFixed(1); res({ n: t.length, p50: q(0.5), p95: q(0.95), max: +t[t.length - 1].toFixed(1) }); } }; requestAnimationFrame(f); })`);
    const quiet = await frames(4000);
    const defStyle = await ev(`document.querySelector('[data-act="style"][aria-selected="true"]')?.dataset.v`);
    const before = await ev(`document.querySelectorAll('canvas[data-ball][data-k]').length`);
    await click("style", "WK08");
    const busy = await frames(6000);
    const after = await ev(`[...document.querySelectorAll('canvas[data-ball]')].filter(c => c.dataset.k).length`);
    report.perf = { tier: await ev(`document.querySelector('[data-act="quality"]')?.value`), quiet, while_making_balls: busy, balls_painted_before: before, balls_painted_after: after };
    console.log("perf", JSON.stringify(report.perf));
    await click("style", defStyle);
    await pick("quality", "auto");
    await sleep(3000);
  }  await size(1280, 2);
  for (const q of searches) {
    await search(q);
    await sleep(800);
    await settle(`#st-mats ${BALLS}`);
    await sleep(500);
    const r = await rect(".st-chips");
    if (r) await shoot(`chips-${q}`, pad(r, 4));
    report.chips.push({ search: q, balls: await measure(`#st-mats ${BALLS}`) });
  }
  await click("mat-all");
  const defStyle = await ev(`document.querySelector('[data-act="style"][aria-selected="true"]')?.dataset.v`);
  for (const st of styles) {
    await click("style", st === "default" ? defStyle : st);
    await sleep(1500);
    for (const m of materials) {
      await search(m.split(".").slice(0, 2).join(" "));
      await sleep(800);
      if (!(await click("mat", m))) {
        await search(m);
        await sleep(800);
        if (!(await click("mat", m))) console.log("material chip not found:", m);
      }
      await sleep(800);
      await settle(`#st-insp ${BALLS}`);
      await settle(`.svp-card ${BALLS}`);
      await sleep(1200);
      const ri = await rect("#st-insp .st-ball");
      if (ri) await shoot(`inspector-${st}-${m}`, pad(ri, 10));
      const rc = await rect(".svp-card");
      if (rc) await shoot(`card-${st}-${m}`, pad(rc, 6));
      report.inspector.push({ style: st, material: m, balls: await measure(`#st-insp ${BALLS}`) });
      report.card.push({ style: st, material: m, balls: await measure(`.svp-card ${BALLS}`) });
    }
  }
  await click("style", defStyle);  await click("mat-all");

  // The check: every ball on screen is the engine's picture (or marked as a
  // row not ready to draw), and the textured materials show their pattern:
  // fine detail well above a smooth shaded ball's (about 1), in every style.
  if (has("check")) {
    const all = [...report.pages.flatMap((p) => p.balls), ...report.chips.flatMap((c) => c.balls), ...report.inspector.flatMap((c) => c.balls), ...report.card.flatMap((c) => c.balls)];
    const min = Number(arg("min-detail", "2"));
    const missing = all.filter((b) => b.visible && !b.painted && !b.not_ready);
    const plain = report.inspector.filter((i) => textured.includes(i.material)).flatMap((i) => i.balls.map((b) => ({ ...b, style: i.style }))).filter((b) => !b.painted || b.detail < min);
    failed = [...missing.map((b) => ({ reason: "not drawn by the engine", ...b })), ...plain.map((b) => ({ reason: "a plain tint, no pattern", ...b }))];
    report.check = { painted: all.filter((b) => b.painted).length, not_ready: all.filter((b) => b.not_ready).length, min_detail: min, failed };
    if (!all.some((b) => b.painted)) failed.push({ reason: "no engine-drawn ball on the page" });
  }} finally {
  writeFileSync(join(out, `${tag}__balls.json`), JSON.stringify(report, null, 1));
  await stop();
}
if (failed.length) {
  console.log("PLAIN BALLS:", JSON.stringify(failed.slice(0, 20)));
  process.exit(1);
}
console.log("ok", tag, out);
