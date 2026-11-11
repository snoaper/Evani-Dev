// Headless, muted shots and frame times of the studio page's Procedural
// section (apps/material-view/src/studio/proc_*.rs).
//
//   node tools/studio/proc-shots.mjs --url http://localhost:8131/ --out <dir>
//        [--sizes 700x900,1280x800,2560x1440] [--shot-size 1600x1000]
//        [--tier high] [--presets 2] [--wait 2500] [--time-ms 4000]
//        [--before]   (only the materials page at --shot-size: the "before" shot)
//        [--materials-sizes] (the materials page at every --sizes too)
//        [--timeout-s 180] (hard limit on the whole run) [--wait-cap-ms 45000] (then shoot anyway)
//        [--skip-materials] (no materials-page shot first)
//        [--no-items] (only the layouts, folds and switch back)
//        [--items glass,sky] (only these items) [--shots-only] (no layouts, folds or switch back)
//        [--only-presets storm,beach] (of those presets, only these: finish a run cut short)
//        [--cpu]      (WebGPU on SwiftShader: no graphics card; frame times are then CPU numbers)
//        [--off-shot] (each library item also switched off: <item>__off.png)
//        [--bare]     (the item shots with the page's bars and panels hidden: the scene alone,
//                      for phone pictures)
//        [--variants <file.json>] (each item shot once per variant: a JSON list of
//                      {"tag": "a", "set": {"render.exposure": 0.6, ...}}, set through the dev
//                      build's window.__engine.paramSet; files <item>__<preset>__<tag>.png)
//
// Writes <dir>/<item>__<preset>.png (each item, its first --presets presets),
// <dir>/layout-<w>.png (the procedural layout per size, with an overlap and
// clipping check), <dir>/materials-<w>.png and <dir>/report.json (the
// checks and the GPU frame times per item on --tier, from the page root's
// data-gpu-ms). Drives the page only through its own buttons (CDP-evaluated
// clicks, never the real mouse or keyboard); pointer lock, fullscreen and
// keyboard lock are stubbed. Chrome: --headless=new, --mute-audio, a fresh
// profile under TEMP (a full path), closed by its own PID.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const url = arg("url", "http://localhost:8131/");
const out = arg("out", ".evani/shots/st-proc");
const sizes = arg("sizes", "700x900,1280x800,2560x1440").split(",").map((s) => s.split("x").map(Number));
const [W, H] = arg("shot-size", "1600x1000").split("x").map(Number);
const tier = arg("tier", "high");
const nPresets = Number(arg("presets", "2"));
const wait = Number(arg("wait", "2500"));
const timeMs = Number(arg("time-ms", "4000"));
const variants = arg("variants") ? JSON.parse(readFileSync(arg("variants"), "utf8")) : [{ tag: "", set: {} }];
mkdirSync(out, { recursive: true });

const flags = has("cpu") ? ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader"] : [];
const { cdp, stop } = await startChrome({ width: W, height: H, flags, prefix: "evani-procshots-" });
// CPU rendering takes every core it can: keep this run's own Chrome (found
// by its profile name) on below-normal priority and 6 cores, so the PC stays under
// its budget. Only our processes are touched; nothing is stopped.
const limitCpu = () => {
  if (!has("cpu") || process.platform !== "win32") return;
  const ps = "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | Where-Object { $_.CommandLine -like '*evani-procshots-*' } | ForEach-Object { try { $p = Get-Process -Id $_.ProcessId -ErrorAction Stop; $p.PriorityClass = 'BelowNormal'; $p.ProcessorAffinity = [IntPtr]0x3F } catch {} }";
  spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", ps], { stdio: "ignore" });
};
limitCpu();
// A hard limit on the whole run: whatever is done is written and Chrome closed.
const overall = Number(arg("timeout-s", "180")) * 1000;
setTimeout(() => {
  console.error(`proc-shots: the ${overall / 1000} s limit was reached; stopping`);
  report.error = (report.error || "") + " overall time limit reached";
  try { writeFileSync(join(out, "report.json"), JSON.stringify(report, null, 1)); } catch (e) {}
  stop().finally(() => process.exit(3));
}, overall).unref();
const report = { url, tier, cpu: has("cpu"), shots: [], layouts: [], timings: [], console: [] };
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  // CPU rendering (--cpu) can take minutes for one big frame.
  const send = (m, p) => cdp.send(m, p, s, has("cpu") ? 900000 : 120000);
  await send("Page.enable");
  await send("Runtime.enable");
  // On the CPU renderer a very wide window is drawn at half density (the
  // same layout in CSS pixels, a quarter of the pixels to draw).
  const size = (w, h) => send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: has("cpu") && w > 2000 ? 0.5 : 1, mobile: false });
  await size(W, H);
  const store = `try{localStorage.clear();localStorage.setItem('evani:material-view:render.tier',${JSON.stringify(tier)})}catch(e){}`;
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + ";" + store });
  cdp.on((m) => {
    if (m.sessionId === s && m.method === "Runtime.consoleAPICalled" && /error|warn/.test(m.params.type))
      report.console.push(m.params.args.map((a) => a.value ?? a.description).join(" ").slice(0, 400));
  });
  const ev = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(expr.slice(0, 80) + ": " + JSON.stringify(r.exceptionDetails).slice(0, 300));
    return r.result.value;
  };
  // evaluate with a cap (a frame count can take long on a slow GPU)
  const evCap = (expr) => Promise.race([ev(expr), sleep(waitCap).then(() => null)]);
  const shot = async (file) => {
    const { data } = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(out, file), Buffer.from(data, "base64"));
    console.error("shot", file);
  };
  const click = (act, v) =>
    ev(`(() => { const b = [...document.querySelectorAll('[data-act="${act}"]')].find(e => ${v === undefined ? "true" : `e.dataset.v === ${JSON.stringify(v)}`}); if (!b) return false; b.click(); return true; })()`);
  // Waits that follow the page, not the clock (a CPU-drawn frame can take
  // many seconds): until an expression holds, then n more drawn frames.
  const waitCap = Number(arg("wait-cap-ms", "45000"));
  const until = async (expr, what) => {
    const t0 = Date.now();
    while (Date.now() - t0 < waitCap) {
      if (await ev(expr)) return true;
      await sleep(500);
    }
    report.late = (report.late || []).concat(what);
    console.error("not seen in time, carrying on:", what);
    return false;
  };
  const frames = (n) => evCap(`new Promise(r => { let k = 0; const f = () => { if (++k >= ${n}) r(k); else requestAnimationFrame(f); }; requestAnimationFrame(f); })`);
  const sizeClass = (w) => (w < 900 ? "narrow" : w < 1220 ? "mid" : "wide");
  // Settled: n more finished GPU frames (the engine's GPU readings on the
  // page root; requestAnimationFrame runs ahead of a slow GPU), else frames.
  const gpuN = () => ev(`Number(document.getElementById('studio-page')?.dataset.gpuN || -1)`);
  const settle = async (n) => {
    const start = await gpuN();
    if (start < 0) return frames(n * 20);
    await until(`Number(document.getElementById('studio-page').dataset.gpuN || 0) >= ${start + n}`, n + " finished frames");
    return frames(2);
  };
  await send("Page.navigate", { url });
  setTimeout(limitCpu, 8000); // the GPU process starts with the page
  for (let i = 0; i < 240 && !(await ev(`!!document.querySelector('[data-act="mat"]')`)); i++) await sleep(500);
  await sleep(4000);
  // The materials page as it is (before and after: the same page, now with the switch).
  for (const [w, h] of has("skip-materials") ? [] : has("before") || !has("materials-sizes") ? [[W, H]] : [[W, H], ...sizes]) {
    await size(w, h);
    await until(`document.getElementById('studio-page').classList.contains('${sizeClass(w)}')`, "the size");
    await settle(3);
    await shot(`materials-${w}.png`);
  }
  if (has("before")) throw "done";
  // The procedural section.
  if (!(await click("mode", "proc"))) throw new Error("no Procedural switch on the page");
  await until(`document.getElementById('studio-page').classList.contains('proc') && document.querySelectorAll('[data-act="proc-item"]').length > 0`, "procedural mode");
  await settle(3);
  const only = arg("items", "").split(",").filter(Boolean);
  const noItems = has("no-items");
  const items = (await ev(`[...document.querySelectorAll('[data-act="proc-item"]')].map(e => e.dataset.v)`)).filter((i) => !noItems && (!only.length || only.includes(i)));
  for (const item of items) {
    await click("proc-item", item);
    await until(`document.querySelector('[data-act="proc-item"][data-v="${item}"]')?.getAttribute('aria-pressed') === 'true' && !!document.querySelector('#st-ppanel [data-act="proc-preset"]')`, item);
    await settle(3);
    const presets = await ev(`[...document.querySelectorAll('[data-act="proc-preset"]')].map(e => e.dataset.v)`);
    const onlyP = arg("only-presets", "").split(",").filter(Boolean);
    for (const p of presets.slice(0, nPresets).filter((x) => !onlyP.length || onlyP.includes(x))) {
      await click("proc-preset", p);
      await until(`document.querySelector('[data-act="proc-preset"][data-v="${p}"]')?.getAttribute('aria-pressed') === 'true'`, p);
      await settle(4);
      // Never a "simple preview": wait until the full materials are ready
      // (the engine marks <html data-evani-preparing> while it compiles).
      await until(`!document.documentElement.dataset.evaniPreparing || document.documentElement.dataset.evaniPreparing === '0'`, item + " materials ready");
      await settle(2);
      await sleep(wait);
      for (const v of variants) {
        if (Object.keys(v.set || {}).length) {
          const ok = await ev(`(() => { const e = window.__engine; if (!e || !e.paramSet) return false; for (const [k, x] of Object.entries(${JSON.stringify(v.set)})) e.paramSet(k, x); return true; })()`);
          if (!ok) console.error("no window.__engine.paramSet in this build: variant", v.tag, "shot as is");
          await settle(4);
          await until(`!document.documentElement.dataset.evaniPreparing || document.documentElement.dataset.evaniPreparing === '0'`, item + " " + v.tag + " ready");
          await settle(3);
        }
        if (has("bare")) {
          await ev(`(() => { for (const id of ['st-mode', 'st-plist', 'st-ppanel']) { const e = document.getElementById(id); if (e) e.style.visibility = 'hidden'; } return true; })()`);
          // The view refits to the whole canvas once the panels are hidden.
          await frames(4);
        }
        const file = `${item}__${p}${v.tag ? "__" + v.tag : ""}.png`;
        await shot(file);
        if (has("bare")) await ev(`(() => { for (const id of ['st-mode', 'st-plist', 'st-ppanel']) { const e = document.getElementById(id); if (e) e.style.visibility = ''; } return true; })()`);
        report.shots.push({ item, preset: p, variant: v.tag || undefined, file, shadow_half_m: await ev(`document.getElementById('studio-page')?.dataset.shadowHalf || null`) });
      }
    }
    // A library asset switched off (its On/Off switch): removed from the view.
    if (has("off-shot") && (await ev(`!!document.querySelector('#st-ppanel [data-act="proc-on"]')`))) {
      await ev(`(() => { document.querySelector('#st-ppanel [data-act="proc-on"]').click(); return true; })()`);
      await until(`document.querySelector('#st-ppanel [data-act="proc-on"]')?.getAttribute('aria-checked') === 'false'`, item + " off");
      await settle(4);
      await sleep(wait);
      await shot(`${item}__off.png`);
      report.shots.push({ item, preset: "off", file: `${item}__off.png` });
      await ev(`(() => { document.querySelector('#st-ppanel [data-act="proc-on"]').click(); return true; })()`);
      await until(`document.querySelector('#st-ppanel [data-act="proc-on"]')?.getAttribute('aria-checked') === 'true'`, item + " on");
      await settle(3);
    }
    // Frame times on the tier: GPU ms readings for timeMs.
    const samples = await ev(`new Promise(res => { const r = document.getElementById('studio-page'); const got = []; let n = r.dataset.gpuN; const t0 = performance.now(); let frames = 0;
      const tick = () => { frames++; if (r.dataset.gpuN !== n) { n = r.dataset.gpuN; const x = parseFloat(r.dataset.gpuMs); if (isFinite(x)) got.push(x); }
        if (performance.now() - t0 < ${timeMs}) requestAnimationFrame(tick); else res({ got, frames, ms: performance.now() - t0, tier: r.dataset.tier, note: isFinite(parseFloat(r.dataset.gpuMs)) ? '' : r.dataset.gpuMs }); };
      requestAnimationFrame(tick); })`);
    const g = [...samples.got].sort((a, b) => a - b);
    const med = g.length ? g[Math.floor(g.length / 2)] : null;
    const p95 = g.length ? g[Math.min(g.length - 1, Math.floor(g.length * 0.95))] : null;
    report.timings.push({ item, tier: samples.tier, gpu_median_ms: med, gpu_p95_ms: p95, readings: g.length, fps: +(samples.frames / (samples.ms / 1000)).toFixed(1), note: samples.note });
    console.error("timing", item, JSON.stringify(report.timings.at(-1)));
  }
  // The layout at each size, with the overlap and clipping check.
  if (has("shots-only")) throw "done";
  if (items.length) await click("proc-item", items[0]);
  for (const [w, h] of sizes) {
    await size(w, h);
    await until(`document.getElementById('studio-page').classList.contains('${sizeClass(w)}')`, "the size " + w);
    await settle(3);
    const check = await ev(`(() => {
      const ids = ['st-mode','st-plist','st-ppanel','st-view'];
      const box = id => { const e = document.getElementById(id); if (!e || e.hidden) return null; const r = e.getBoundingClientRect(); return { id, l: r.left, t: r.top, r: r.right, b: r.bottom, w: r.width, h: r.height }; };
      const b = ids.map(box).filter(Boolean);
      const over = [];
      for (let i = 0; i < b.length; i++) for (let j = i + 1; j < b.length; j++) {
        const x = Math.min(b[i].r, b[j].r) - Math.max(b[i].l, b[j].l), y = Math.min(b[i].b, b[j].b) - Math.max(b[i].t, b[j].t);
        if (x > 1 && y > 1) over.push(b[i].id + '/' + b[j].id);
      }
      const out = b.filter(x => x.l < -1 || x.t < -1 || x.r > innerWidth + 1 || x.b > innerHeight + 1).map(x => x.id);
      const tiny = b.filter(x => x.w < 60 || x.h < 40).map(x => x.id);
      // Text cut without an ellipsis: an element wider inside than its box and not scrolling.
      const cut = [...document.querySelectorAll('#st-ppanel .lab span, #st-ppanel button, .pc-tx b, .st-modeb')].filter(e => e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).textOverflow !== 'ellipsis').map(e => e.textContent.trim().slice(0, 30));
      const hiddenMat = ['st-left','st-styles','st-mats','st-insp','st-side','st-vars'].filter(id => { const e = document.getElementById(id); return e && e.getBoundingClientRect().width > 0; });
      // Text drawn over other text in the panel (a badge style pulling a
      // label out of its line, say): visible text leaves whose boxes cross.
      const leaves = [...document.querySelectorAll('#st-ppanel b, #st-ppanel span, #st-ppanel em, #st-ppanel h3, #st-ppanel button, #st-ppanel p, #st-ppanel summary')]
        .filter(e => e.offsetParent && e.textContent.trim() && ![...e.children].some(c => c.textContent.trim()))
        .map(e => ({ t: e.textContent.trim().slice(0, 24), r: e.getBoundingClientRect() })).filter(x => x.r.width > 0 && x.r.height > 0);
      const textOver = [];
      for (let i = 0; i < leaves.length; i++) for (let j = i + 1; j < leaves.length; j++) {
        const a = leaves[i].r, c = leaves[j].r;
        const x = Math.min(a.right, c.right) - Math.max(a.left, c.left), y = Math.min(a.bottom, c.bottom) - Math.max(a.top, c.top);
        if (x > 2 && y > 2) textOver.push(leaves[i].t + ' / ' + leaves[j].t);
      }
      return { boxes: b, overlaps: over, offscreen: out, tiny, cut, text_overlaps: textOver.slice(0, 20), materials_parts_showing: hiddenMat, cls: document.getElementById('studio-page').className };
    })()`);
    await shot(`layout-${w}.png`);
    report.layouts.push({ size: `${w}x${h}`, file: `layout-${w}.png`, ...check });
    console.error("layout", w, JSON.stringify({ overlaps: check.overlaps, offscreen: check.offscreen, tiny: check.tiny, cut: check.cut, text_overlaps: check.text_overlaps, materials_parts_showing: check.materials_parts_showing }));
  }
  // The Advanced and Use in a game folds open, at the shot size.
  await size(W, H);
  await until(`document.getElementById('studio-page').classList.contains('${sizeClass(W)}')`, "the shot size");
  await click("proc-adv");
  await until(`!!document.querySelector('details.pc-fold[open]')`, "the Advanced fold");
  await click("proc-game");
  await until(`document.querySelectorAll('details.pc-fold[open]').length === 2`, "the game fold");
  await settle(2);
  await ev(`document.getElementById('st-ppanel').scrollTop = 1e6`);
  await sleep(300);
  await shot(`folds-open.png`);
  // Back to the materials: the switch goes both ways.
  await click("mode", "mat");
  await until(`!document.getElementById('studio-page').classList.contains('proc')`, "the materials mode");
  await settle(3);
  report.back = await ev(`({ cls: document.getElementById('studio-page').className, left: document.getElementById('st-left').getBoundingClientRect().width > 0, plist: !document.getElementById('st-plist') || document.getElementById('st-plist').hidden })`);
  await shot(`materials-back.png`);
} catch (e) {
  if (e !== "done") {
    report.error = String(e && e.stack || e);
    console.error(report.error);
  }
} finally {
  writeFileSync(join(out, "report.json"), JSON.stringify(report, null, 1));
  await stop();
}
