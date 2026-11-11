// Storm reference shots of the Ultra-Water demo (the look the spectral
// sea's storm presets are compared with): a headless, muted Chrome on a
// fresh full-path profile, pointer and keyboard locks stubbed, closed by its
// own PID. It opens every lil-gui folder, dumps every control (name, value,
// range), sets the given values through the page's own controls (as a user
// typing into them would), picks each camera shot, hides the panels and saves
// a screenshot per shot. Ideas only: nothing of the page is copied, only
// pictures of it are taken for the side-by-side proof.
//
//   node tools/nature/storm-ref.mjs --out <dir> [--size 1600x900] [--gpu]
//        [--set-file values.json] [--cameras "Open ocean,Aerial"]
//        [--name ref] [--wait-ms 25000] [--settle-ms 6000] [--dump]
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const url = arg("url", "https://gelatin.games/ultrawater");
const out = arg("out", ".evani/shots/storm-ref");
const [W, H] = arg("size", "1600x900").split("x").map(Number);
const name = arg("name", "ref");
const sets = JSON.parse(arg("set-file", "") ? readFileSync(arg("set-file"), "utf8") : arg("set", "{}"));
const cams = arg("cameras", "").split(",").map((s) => s.trim()).filter(Boolean);
mkdirSync(out, { recursive: true });
const flags = process.argv.includes("--gpu") ? [] : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader", "--use-angle=swiftshader"];
const { cdp, stop } = await startChrome({ width: W, height: H, flags, prefix: "evani-stormref-" });
const limit = Number(arg("timeout-s", "300")) * 1000;
const report = { url, set: sets, cameras: cams, shots: [], console: [] };
const finish = async (c) => {
  writeFileSync(join(out, name + "-report.json"), JSON.stringify(report, null, 1));
  await stop().catch(() => {});
  process.exit(c);
};
setTimeout(() => {
  report.error = "time limit";
  finish(3);
}, limit).unref();

// Page code: every control of the lil-gui panel, folders opened first.
const DUMP = `(() => {
  document.querySelectorAll('.lil-gui.lil-closed > .lil-title, .lil-gui.closed > .title').forEach(t => t.click());
  const rows = [];
  document.querySelectorAll('.lil-gui .lil-controller, .lil-gui .controller').forEach(c => {
    const n = (c.querySelector('.lil-name, .name')?.textContent || '').trim();
    const inp = c.querySelector('input');
    const sel = c.querySelector('select');
    const folder = (c.closest('.lil-gui')?.querySelector(':scope > .lil-title, :scope > .title')?.textContent || '').trim();
    rows.push({ folder, name: n, kind: [...c.classList].filter(x => x !== 'controller' && x !== 'lil-controller').join(' '),
      value: sel ? sel.value : inp ? (inp.type === 'checkbox' ? inp.checked : inp.value) : null,
      options: sel ? [...sel.options].map(o => o.textContent) : undefined });
  });
  return JSON.stringify(rows);
})()`;

// Page code: set one control by its label (number, checkbox or option).
const SET = (k, v) => `(() => {
  const c = [...document.querySelectorAll('.lil-gui .lil-controller, .lil-gui .controller')].find(c => (c.querySelector('.lil-name, .name')?.textContent || '').trim() === ${JSON.stringify(k)});
  if (!c) return 'missing ' + ${JSON.stringify(k)};
  const sel = c.querySelector('select');
  if (sel) {
    const o = [...sel.options].find(o => o.textContent.trim() === ${JSON.stringify(String(v))});
    if (!o) return 'no option ' + ${JSON.stringify(String(v))};
    sel.value = o.value; sel.dispatchEvent(new Event('change', { bubbles: true }));
    return 'ok';
  }
  const inp = c.querySelector('input');
  if (inp.type === 'checkbox') { if (inp.checked !== ${JSON.stringify(v)}) inp.click(); return 'ok'; }
  inp.focus(); inp.value = ${JSON.stringify(String(v))};
  inp.dispatchEvent(new Event('input', { bubbles: true }));
  inp.dispatchEvent(new Event('change', { bubbles: true }));
  inp.blur();
  return 'ok ' + inp.value;
})()`;

const HIDE = `(() => {
  document.querySelectorAll('.lil-gui.lil-root, .lil-gui.root').forEach(e => e.style.display = 'none');
  // The stats card (top left): every positioned panel that holds no canvas,
  // and the outermost positioned box round the "GPU timers" text.
  document.querySelectorAll('body *').forEach(e => { const p = getComputedStyle(e).position; if (e.tagName !== 'CANVAS' && (p === 'fixed' || p === 'absolute') && !e.querySelector('canvas') && /FPS|GPU timers|benchmark/i.test(e.textContent || '')) e.style.display = 'none'; });
  // Whatever still covers the card's spot: its outermost box holding no canvas.
  for (const [x, y] of [[100, 60], [100, 250]]) {
    let e = document.elementFromPoint(x, y), last = null;
    while (e && e !== document.body && e !== document.documentElement && e.tagName !== 'CANVAS' && !e.querySelector('canvas')) { last = e; e = e.parentElement; }
    if (last) last.style.visibility = 'hidden';
  }
  return 'hidden';
})()`;
// Page code: the three.js camera's pose if the page exposes one (globals
// scanned two levels deep for an object with a fov and a position).
const CAMINFO = `(() => {
  const f = document.createElement('iframe'); document.body.appendChild(f);
  const std = new Set(Object.getOwnPropertyNames(f.contentWindow)); f.remove();
  const own = Object.getOwnPropertyNames(window).filter(k => !std.has(k));
  const isCam = (o) => o && typeof o === 'object' && typeof o.fov === 'number' && o.position && typeof o.position.y === 'number';
  const pose = (c, path) => { const d = c.getWorldDirection ? c.getWorldDirection(new c.position.constructor()) : null; return { path, fov: c.fov, aspect: c.aspect, near: c.near, far: c.far, pos: [c.position.x, c.position.y, c.position.z], dir: d ? [d.x, d.y, d.z] : null }; };
  for (const k of own) { let v; try { v = window[k]; } catch (e) { continue; }
    if (isCam(v)) return JSON.stringify({ globals: own.slice(0, 60), cam: pose(v, k) });
    if (v && typeof v === 'object') for (const j of Object.keys(v).slice(0, 200)) { let w; try { w = v[j]; } catch (e) { continue; } if (isCam(w)) return JSON.stringify({ globals: own.slice(0, 60), cam: pose(w, k + '.' + j) }); }
  }
  return JSON.stringify({ globals: own.slice(0, 60), cam: null });
})()`;

const SHOW = `(() => { document.querySelectorAll('.lil-gui.lil-root, .lil-gui.root').forEach(e => e.style.display = ''); document.querySelectorAll('body > *').forEach(e => { if (e.style.display === 'none') e.style.display = ''; }); return 'shown'; })()`;

try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p, ms = 60000) => cdp.send(m, p, s, ms);
  const ev = async (x) => {
    const r = await send("Runtime.evaluate", { expression: x, awaitPromise: true, returnByValue: true }, 60000);
    return r.result?.value ?? r.exceptionDetails?.exception?.description ?? r.exceptionDetails?.text;
  };
  cdp.handlers.push((msg) => {
    if (msg.method === "Runtime.consoleAPICalled" && report.console.length < 20) report.console.push(msg.params.args.map((a) => a.value ?? a.description).join(" ").slice(0, 200));
  });
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
  const t0 = Date.now();
  await send("Page.navigate", { url });
  await sleep(Number(arg("wait-ms", "25000")));
  // The demo may sit in a frame: report the tree (open the frame's own
  // address with --url if so).
  const tree = await send("Page.getFrameTree");
  const frames = [];
  const walk = (n) => {
    frames.push(n.frame.url);
    (n.childFrames || []).forEach(walk);
  };
  walk(tree.frameTree);
  report.frames = frames;
  report.title = await ev("document.title + ' | ' + document.querySelectorAll('*').length + ' elements | ' + [...new Set([...document.querySelectorAll('div,ul,li,span')].map(e => typeof e.className === 'string' ? e.className.split(' ')[0] : '').filter(Boolean))].slice(0, 40).join(',')");
  report.before = JSON.parse((await ev(DUMP)) || "[]");
  report.applied = {};
  for (const [k, v] of Object.entries(sets)) report.applied[k] = await ev(SET(k, v));
  await sleep(1500);
  report.after = JSON.parse((await ev(DUMP)) || "[]");
  if (process.argv.includes("--dump")) await finish(0);
  const settle = Number(arg("settle-ms", "6000"));
  const list = cams.length ? cams : [null];
  for (const cam of list) {
    if (cam) {
      await ev(SHOW);
      report.applied["camera:" + cam] = await ev(SET("camera", cam));
      // A camera preset may reset values: set them again.
      for (const [k, v] of Object.entries(sets)) await ev(SET(k, v));
    }
    await ev(HIDE);
    await sleep(settle);
    const cinfo = await ev(CAMINFO);
    try {
      report["cam:" + (cam || "view")] = JSON.parse(cinfo);
    } catch {
      report["cam:" + (cam || "view")] = cinfo;
    }
    // A few moments of each view (the sea moves; one frame can be lucky).
    const moments = Number(arg("moments", "1"));
    for (let m = 0; m < moments; m++) {
      if (m > 0) await sleep(Number(arg("every-ms", "3000")));
      const shot = await send("Page.captureScreenshot", { format: "png" }, 120000);
      const f = `${name}-${(cam || "view").toLowerCase().replace(/[^a-z0-9]+/g, "-")}${moments > 1 ? "-" + m : ""}.png`;
      writeFileSync(join(out, f), Buffer.from(shot.data, "base64"));
      report.shots.push({ file: f, camera: cam, at_ms: Date.now() - t0 });
    }
  }
  await finish(0);
} catch (e) {
  report.error = String((e && e.stack) || e);
  await finish(1);
}
