// Headless, muted pictures of one proof-game scene (examples/proc-field, a dev
// scene feature such as cube, pebbles or cone) for an Asset Studio round.
// The one capture script behind docs/ASSET_STUDIO_GUIDE.md.
//
//   node tools/studio/asset-shoot.mjs --dist <trunk dist folder> --out <folder>
//        [--size 1536x1024] [--tier high]
//        [--view-param kit.proc_field.cube_view]
//        [--shots "0:round.png,1:close.png"]   (view value : file name, comma separated)
//        [--params '{"kit.proc_field.cube_wear":0.5}']  (set before the first shot)
//        [--shots-file shots.json]  ([{"view":0,"file":"a.png","params":{"k":v}}], overrides --shots)
//        [--settle-ms 3000] [--frames 40] [--timeout-s 240]
//        [--warm-ms 0]  (an extra wait before the first shot: a material recipe can still be compiling after load)
//        [--busy-timeout-s 180]  (before every shot, wait while window.__engine.pipelinesBusy() says shaders or
//                                pipelines are still building: a look param that changes a shader body, such as the
//                                grey studio sweep's haze, only shows once its rebuild is in, tens of seconds on a big
//                                shader; a build without the hook is shot after the settle as before)
//        [--stats <file.json>]  (the commons-render app only: sets kit.proc_field.stats and writes the asset's
//                               mesh stats {triangles, vertices, assets, bbox_min, bbox_max} from its console line)
//        [--offscreen-dist <dist>]  (the backup: a build of the same app and features plus `live`; default
//                                    <dist>-offscreen when that folder exists; not needed when <dist> has `live`)
//        [--force-primary-fail] [--force-offscreen-fail]  (tests only: treat that path's picture as unusable)
//        [--measure <file.json>]  (the commons-render app only, with kit.proc_field.test_scene set in --params:
//                                 sets kit.proc_field.measure and writes the test scene's measures
//                                 {"<spec id>": number|null} from its `evani-field-measure` console line)
//
// Serves <dist> on a free 127.0.0.1 port, opens it in headless Chrome (a fresh
// profile under TEMP, --mute-audio, closed by its own PID; the graphics card,
// never a software renderer), sets params through the dev build's
// window.__engine.paramSet (a build with the dev hooks: every scene feature
// has them), hides the engine overlay and saves a PNG per shot at the
// window's own size (original quality). Only synthetic CDP input: never the
// real mouse or keyboard.
//
// Every picture is checked (tools/studio/visual-review.mjs `usable`): the right size, not blank, black or one
// flat colour. An unusable browser screenshot (or one that throws) is retaken by the engine itself: its own
// off-screen read-back of the frame it drew (`capture.frame` through window.evani.live, the render `capture`
// feature; docs/LIVE_API.md 6.14), with the same camera, params and size: on the same page when the build has
// `live`, else on a page of --offscreen-dist. capture-paths.json in --out records which path made each
// picture ({"<file>": {"path": "browser" | "offscreen", "primary_problem"?}}); visual-review.mjs copies it into
// the notes as capture_path. Both paths failing: {"path": "failed", "why"}, the reason on stderr
// ("asset-shoot: no usable picture for <file>: browser: ...; offscreen: ...") and exit 4.
import { createServer } from "node:http";
import { readFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, extname, resolve } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";
import { usable } from "./visual-review.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const dist = arg("dist");
const out = arg("out");
if (!dist || !out) {
  console.error("usage: node tools/studio/asset-shoot.mjs --dist <dist> --out <folder> [--shots \"0:round.png\"] [--view-param <name>] [--params <json>] [--size WxH]");
  process.exit(2);
}
const [W, H] = arg("size", "1536x1024").split("x").map(Number);
const tier = arg("tier", "high");
const viewParam = arg("view-param", "");
const extra = JSON.parse(arg("params", "{}"));
const settle = Number(arg("settle-ms", "3000"));
const nFrames = Number(arg("frames", "40"));
const statsFile = arg("stats", "");
const warm = Number(arg("warm-ms", "0"));
const measureFile = arg("measure", "");
const busyTimeout = Number(arg("busy-timeout-s", "180")) * 1000;
// The commons-render app writes one console line `evani-field-stats {json}` while kit.proc_field.stats is 1.
const STATS_HOOK = ";(()=>{const o=console.log;console.log=function(){try{const s=String(arguments[0]);if(s.startsWith('evani-field-stats '))window.__fieldStats=s.slice(18)}catch(e){}return o.apply(this,arguments)}})()";
// ... and `evani-field-measure {json}` once while kit.proc_field.measure is 1 (a test scene's own probes).
const MEASURE_HOOK = ";(()=>{const o=console.log;console.log=function(){try{const s=String(arguments[0]);if(s.startsWith('evani-field-measure '))window.__fieldMeasure=s.slice(20)}catch(e){}return o.apply(this,arguments)}})()";
const shots = arg("shots-file")
  ? JSON.parse(readFileSync(arg("shots-file"), "utf8"))
  : arg("shots", "0:round.png").split(",").map((s) => { const [view, file] = s.split(":"); return { view: Number(view), file }; });
const root = resolve(dist);
mkdirSync(out, { recursive: true });
const offDist = arg("offscreen-dist", existsSync(root + "-offscreen") ? root + "-offscreen" : "");
const forcePrimaryFail = process.argv.includes("--force-primary-fail");
const forceOffscreenFail = process.argv.includes("--force-offscreen-fail");
const pathsFile = join(out, "capture-paths.json");
const paths = (() => { try { return JSON.parse(readFileSync(pathsFile, "utf8")); } catch { return {}; } })();
let failed = 0;
// How these pictures were shot, so Evani.App's backup hook (tools/studio/review-backup.mjs, named in
// .vibedeck/visual-capture.json) can shoot the same views again into its own folder.
{
  const recipeFile = join(out, "capture-recipe.json");
  const old = (() => { try { return JSON.parse(readFileSync(recipeFile, "utf8")); } catch { return null; } })();
  const same = old && old.dist === root && old.size === `${W}x${H}` && JSON.stringify(old.params) === JSON.stringify(extra) && old.view_param === viewParam && old.tier === tier;
  const keep = same ? (old.shots || []).filter((s) => !shots.some((n) => n.file === s.file)) : [];
  writeFileSync(recipeFile, JSON.stringify({ tool: "tools/studio/asset-shoot.mjs", dist: root, offscreen_dist: offDist ? resolve(offDist) : "", size: `${W}x${H}`, tier, view_param: viewParam, params: extra, settle_ms: settle, frames: nFrames, warm_ms: warm, shots: [...keep, ...shots] }, null, 1) + "\n");
}

const MIME = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css", ".png": "image/png", ".jpg": "image/jpeg", ".json": "application/json" };
const srv = createServer((q, r) => {
  let p = decodeURIComponent(q.url.split("?")[0]);
  // /offscreen/... serves the backup build (--offscreen-dist).
  const base = offDist && p.startsWith("/offscreen/") ? resolve(offDist) : root;
  if (base !== root) p = p.slice("/offscreen".length);
  if (p.endsWith("/")) p += "index.html";
  const f = join(base, p);
  if (!f.startsWith(base) || !existsSync(f)) { r.writeHead(404); r.end(); return; }
  r.writeHead(200, { "content-type": MIME[extname(f)] || "application/octet-stream" });
  r.end(readFileSync(f));
});
await new Promise((ok) => srv.listen(0, "127.0.0.1", ok));
const url = `http://127.0.0.1:${srv.address().port}/`;
const { cdp, stop } = await startChrome({ width: W, height: H, flags: [], prefix: "evani-assetshoot-" });
const limit = setTimeout(() => { console.error("asset-shoot: time limit reached; stopping"); stop().finally(() => process.exit(3)); }, Number(arg("timeout-s", "240")) * 1000);
let code = 0;
try {
  // One page of a build: the scene loaded and --params set.
  const openPage = async (pageUrl, hooks) => {
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    const send = (m, p) => cdp.send(m, p, s, 120000);
    await send("Page.enable");
    await send("Runtime.enable");
    await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + ";try{localStorage.clear();localStorage.setItem('evani:render.tier'," + JSON.stringify(tier) + ")}catch(e){}" + hooks });
    const ev = async (expr) => {
      const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(expr.slice(0, 80) + ": " + JSON.stringify(r.exceptionDetails).slice(0, 300));
      return r.result.value;
    };
    const setParam = async (k, v) => {
      const ok = await ev(`window.__engine.paramSet(${JSON.stringify(k)}, ${JSON.stringify(v)})`);
      if (!ok) console.error("param not set:", k);
    };
    const frames = (n) => ev(`new Promise(r => { let k = 0; const f = () => { if (++k >= ${n}) r(k); else requestAnimationFrame(f); }; requestAnimationFrame(f); })`);
    await send("Page.navigate", { url: pageUrl });
    let ready = false;
    for (let i = 0; i < 120 && !ready; i++) { ready = await ev(`!!(window.__engine && window.__engine.paramSet)`); if (!ready) await sleep(500); }
    if (!ready) throw new Error("the page never exposed window.__engine.paramSet (is this a dev scene build?)");
    await sleep(6000);
    for (const [k, v] of Object.entries(extra)) await setParam(k, v);
    if (warm > 0) await sleep(warm);
    const live = await ev(`!!(window.evani && window.evani.live && window.evani.live.call)`);
    return { send, ev, setParam, frames, live };
  };
  // The same view staged on a page: the view param, the shot's params, settle, overlay hidden, frames drawn.
  const stage = async (pg, view, params) => {
    if (viewParam) await pg.setParam(viewParam, Number(view));
    for (const [k, v] of Object.entries(params)) await pg.setParam(k, v);
    await sleep(settle);
    // Shaders still building (a slot rebuild the params asked for): wait until the engine has been idle for 20 frames.
    const t0 = Date.now();
    let idle = 0;
    while (Date.now() - t0 < busyTimeout) {
      const busy = await pg.ev(`(window.__engine && typeof window.__engine.pipelinesBusy === 'function') ? window.__engine.pipelinesBusy() : null`);
      if (busy === null) break;
      idle = busy ? 0 : idle + 1;
      if (idle >= 20) break;
      await pg.frames(1);
    }
    if (idle < 20 && Date.now() - t0 >= busyTimeout) console.error(`asset-shoot: shaders still building after ${busyTimeout / 1000} s; shooting anyway`);
    else if (Date.now() - t0 > 2000) console.error(`asset-shoot: waited ${Math.round((Date.now() - t0) / 1000)} s for shaders`);
    await pg.ev(`(() => { const o = document.getElementById('evani-overlay'); if (o) o.style.visibility = 'hidden'; return true; })()`);
    await pg.frames(nFrames);
  };
  // The engine's own off-screen read-back of the frame it draws (capture.frame), at the full canvas size.
  const engineShot = async (pg) => {
    const r = await pg.ev(`(async () => { try { const x = await window.evani.live.call('capture.frame', { width: ${W}, hide_overlay: true }); return { png: x.png_base64 }; } catch (e) { return { error: String((e && e.message) || e) }; } })()`);
    if (r.error) throw new Error("capture.frame: " + r.error);
    return Buffer.from(r.png, "base64");
  };
  const page = await openPage(url, (statsFile ? STATS_HOOK : "") + (measureFile ? MEASURE_HOOK : ""));
  const { ev, setParam } = page;
  let offPage = null; // the --offscreen-dist page, opened the first time it is needed
  if (statsFile) {
    await setParam("kit.proc_field.stats", 1);
    let line = null;
    for (let i = 0; i < 60 && !line; i++) { line = await ev(`window.__fieldStats || null`); if (!line) await sleep(500); }
    if (!line) throw new Error("no evani-field-stats line in 30 s (--stats needs the commons-render app)");
    writeFileSync(statsFile, JSON.stringify(JSON.parse(line), null, 1) + "\n");
    console.error("stats", statsFile);
  }
  if (measureFile) {
    await setParam("kit.proc_field.measure", 1);
    let line = null;
    for (let i = 0; i < 120 && !line; i++) { line = await ev(`window.__fieldMeasure || null`); if (!line) await sleep(500); }
    if (!line) throw new Error("no evani-field-measure line in 60 s (--measure needs the commons-render app and a kit.proc_field.test_scene)");
    writeFileSync(measureFile, JSON.stringify(JSON.parse(line), null, 1) + "\n");
    console.error("measure", measureFile);
  }
  const want = { width: W, height: H };
  for (const { view, file, params = {} } of shots) {
    // 1. The browser's screenshot of the window.
    let primary;
    try {
      await stage(page, view, params);
      const { data } = await page.send("Page.captureScreenshot", { format: "png" });
      const buf = Buffer.from(data, "base64");
      const u = forcePrimaryFail ? { ok: false, why: "forced to fail (--force-primary-fail)" } : usable(buf, want);
      primary = u.ok ? { buf } : { why: u.why };
    } catch (e) {
      primary = { why: String((e && e.message) || e).slice(0, 300) };
    }
    if (primary.buf) {
      writeFileSync(join(out, file), primary.buf);
      paths[file] = { path: "browser" };
      console.error("shot", join(out, file));
      continue;
    }
    // 2. The engine draws the same view and reads its own frame back.
    console.error(`asset-shoot: the browser picture for ${file} is unusable (${primary.why}); taking it off-screen through the engine`);
    let backup;
    try {
      let pg = page;
      if (!page.live) {
        if (!offDist) throw new Error("this build has no `live` module, and no --offscreen-dist (the same app and features plus `live`, or <dist>-offscreen) was given");
        if (!offPage) {
          offPage = await openPage(url + "offscreen/", "");
          if (!offPage.live) throw new Error(`--offscreen-dist ${offDist} has no \`live\` module (window.evani.live)`);
        }
        pg = offPage;
        await stage(pg, view, params);
      }
      const buf = await engineShot(pg);
      const u = forceOffscreenFail ? { ok: false, why: "forced to fail (--force-offscreen-fail)" } : usable(buf, want);
      backup = u.ok ? { buf } : { why: u.why };
    } catch (e) {
      backup = { why: String((e && e.message) || e).slice(0, 300) };
    }
    if (backup.buf) {
      writeFileSync(join(out, file), backup.buf);
      paths[file] = { path: "offscreen", primary_problem: primary.why };
      console.error("shot (offscreen)", join(out, file));
    } else {
      failed++;
      const why = `browser: ${primary.why}; offscreen: ${backup.why}`;
      paths[file] = { path: "failed", why };
      console.error(`asset-shoot: no usable picture for ${file}: ${why}`);
    }
  }
  if (failed) code = 4;
} catch (e) {
  console.error(String((e && e.stack) || e));
  code = 1;
} finally {
  clearTimeout(limit);
  if (Object.keys(paths).length) { try { writeFileSync(pathsFile, JSON.stringify(paths, null, 1) + "\n"); } catch (e) { console.error("capture-paths.json: " + e.message); } }
  await stop();
  srv.close();
  process.exit(code);
}
