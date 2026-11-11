// The grass proof's picture and timing harness (evani-grass; the proof game
// is examples/grass-field). Serves a built dist folder, opens it in headless
// muted Chrome (a fresh full-path profile, locks stubbed, closed by its own
// PID) on a virtual clock and takes fixed views at fixed sim times:
//
//   views    every camera view at one tier, the walker hidden, t = 3 s
//   wet      the meadow and the mud dry, half wet and soaked
//   trample  the walker's path through the meadow at fixed times while it
//            is pressed flat and while it springs back (and kept flat)
//   motion   six stills 0.4 s apart of the meadow and the wheat in the wind
//   tiers    the meadow and the overview on every tier, with the plan
//   bench    (--gpu only) real time, frames uncapped: per tier the grass
//            compute pass and the forward pass with and without grass,
//            medians over the run
//
//   node tools/grass/grass-shots.mjs --dist <built dist> --out <dir>
//        [--do views,wet,trample,motion,tiers,bench] [--gpu] [--size 1600x900]
//        [--tier high] [--timeout-s 300] [--bench-s 12] [--views meadow,wheat]
//
// Default is WebGPU on SwiftShader (the CPU); --gpu uses the graphics card.
// Writes PNGs and report.json into <out>.
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, extname } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const dist = arg("dist", "");
const out = arg("out", ".evani/shots/grass");
if (!dist || !existsSync(dist)) {
  console.error("--dist <built grass-field folder> is needed");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const gpu = has("gpu");
const [W, H] = arg("size", "1600x900").split("x").map(Number);
const tier = arg("tier", "high");
const timeoutMs = Number(arg("timeout-s", "300")) * 1000;
const todo = arg("do", "views").split(",");
const benchS = Number(arg("bench-s", "12"));
const VIEWS = arg("views", "meadow,overview,lawn,mud,wheat,close,low").split(",");
const EXTRA = arg("extra", "");
const log = (...a) => console.error("[grass]", ...a);

const MIME = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".css": "text/css" };
const server = createServer((req, res) => {
  const path = decodeURIComponent(req.url.split("?")[0]);
  const file = join(dist, path === "/" ? "index.html" : path);
  if (!existsSync(file)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;

const flags = gpu ? (todo.includes("bench") ? ["--disable-gpu-vsync", "--disable-frame-rate-limit"] : []) : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader"];
const { cdp, stop, version } = await startChrome({ width: W, height: H, flags, prefix: "evani-grass-" });
log(`${version} ${gpu ? "gpu" : "swiftshader"} ${W}x${H}`);
const target = await cdp.send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: target.targetId, flatten: true });
const s = (m, p = {}, t) => cdp.send(m, p, sessionId, t);
await s("Page.enable");
await s("Runtime.enable");
await s("Emulation.setFocusEmulationEnabled", { enabled: true });
await s("Page.bringToFront");
await s("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
await s("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
// The virtual clock: every frame moves `fast` sixtieths of a second, never
// past the hold time in the address's hash (#h=<seconds>), so a held frame
// is the same moment however slowly the CPU renders it.
await s("Page.addScriptToEvaluateOnNewDocument", {
  source: `(() => {
    try {
      const q = new URLSearchParams(location.search);
      const t = q.get('tier');
      if (t) localStorage.setItem('evani:grass-field:render.tier', t);
      if (q.get('vclock')) {
        let now = 0;
        const fast = Number(q.get('fast')) || 4;
        const raf = window.requestAnimationFrame.bind(window);
        const hold = () => { const m = /h=([0-9.]+)/.exec(location.hash); return m ? Number(m[1]) * 1000 : Infinity; };
        performance.now = () => now;
        window.requestAnimationFrame = (cb) => raf(() => { const h = hold(); if (now < h) now = Math.min(h, now + fast * 1000 / 60); cb(now); });
      }
    } catch (e) {}
    const add = () => {
      const st = document.createElement('style');
      st.textContent = '#evani-overlay, #evani-panel, #evani-convert-zone, #evani-touch { display: none !important; }';
      (document.head || document.documentElement).appendChild(st);
    };
    if (document.documentElement) add();
    else document.addEventListener('DOMContentLoaded', add, { once: true });
  })();`,
});
const errors = [];
cdp.on((m) => {
  if (m.method === "Runtime.exceptionThrown") errors.push(m.params?.exceptionDetails?.exception?.description ?? m.params?.exceptionDetails?.text ?? "exception");
  if (m.method === "Runtime.consoleAPICalled" && (m.params?.type === "error" || m.params?.type === "warning")) errors.push((m.params.args ?? []).map((a) => a.value ?? a.description ?? "").join(" ").slice(0, 600));
});

const evalJs = async (expression) => {
  const r = await s("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  return r?.result?.value;
};
const title = async () => (await evalJs("document.title")) ?? "";
const field = (t, k) => new RegExp(`\\b${k}=(\\S+)`).exec(t)?.[1];
const num = (t, k) => Number(field(t, k));

async function waitTitle(test, label, ms = timeoutMs) {
  const end = Date.now() + ms;
  let t = "";
  while (Date.now() < end) {
    t = await title();
    if (t.startsWith("gf ") && test(t)) return t;
    await sleep(60);
  }
  throw new Error(`${label}: timed out; title "${t}"; errors ${errors.slice(-6).join(" | ")}`);
}

// The grass level the tier in a query should reach (-1: don't check).
const LEVELS = { lite: 0, low: 1, medium: 2, vr: 2, high: 3, ultra: 4 };
let expect = -1;

async function open(query, vclock = true) {
  const q = new URLSearchParams(query);
  expect = q.get("q") ? Number(q.get("q")) : LEVELS[q.get("tier")] ?? -1;
  const url = `http://127.0.0.1:${port}/?${query}${vclock ? "&vclock=1" : ""}${EXTRA ? "&" + EXTRA : ""}`;
  await s("Page.navigate", { url }, 300000);
  await sleep(400);
}

async function holdAt(sec, label) {
  // The sim starts a frame after the raw clock: hold a frame later, so
  // the sim stops at (or just past) `sec`.
  await evalJs(`location.hash = 'h=${(sec + 0.07).toFixed(3)}'`);
  const t = await waitTitle((x) => num(x, "t") >= sec - 0.002 && num(x, "f") > 3 && (expect < 0 || num(x, "lvl") === expect), label);
  await evalJs("new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(r))))");
  return await title();
}

async function shot(name) {
  const r = await s("Page.captureScreenshot", { format: "png", fromSurface: true }, 120000);
  const buf = Buffer.from(r.data, "base64");
  if (name) writeFileSync(join(out, name), buf);
  return buf;
}

const stats = (t) => ({ t: num(t, "t"), tier: field(t, "tier"), level: num(t, "lvl"), tiles: num(t, "tiles"), blades: num(t, "blades"), scale: num(t, "scale"), draws: num(t, "draws"), gpu: num(t, "gpu"), grass: num(t, "grass"), fwd: num(t, "fwd") });
const report = { chrome: version, gpu, size: [W, H], tier, shots: [] };
const record = (name, query, t) => {
  report.shots.push({ name, query, ...stats(t) });
  log(`${name}: ${t}`);
};

try {
  if (todo.includes("views")) {
    for (const v of VIEWS) {
      const q = `view=${v}&tier=${tier}&walker=0`;
      await open(q);
      const t = await holdAt(Number(arg("at", "1.5")), v);
      await shot(`${v}.png`);
      record(v, q, t);
    }
  }
  if (todo.includes("wet")) {
    for (const v of ["meadow", "mud", "close"]) {
      for (const w of [0, 0.5, 1]) {
        const q = `view=${v}&tier=${tier}&walker=0&wet=${w}`;
        await open(q);
        const t = await holdAt(1.5, `${v} wet ${w}`);
        await shot(`wet-${v}-${w}.png`);
        record(`wet-${v}-${w}`, q, t);
      }
    }
  }
  if (todo.includes("trample")) {
    const times = arg("times", "8,14,20,28,40").split(",").map(Number);
    for (const flat of has("no-flat") ? [0] : [0, 1]) {
      const q = `view=trample&tier=${tier}&fast=${arg("fast", "4")}${flat ? "&flat=1" : ""}`;
      await open(q);
      for (const sec of flat ? [times[times.length - 1]] : times) {
        const t = await holdAt(sec, `trample ${sec}`);
        await shot(`trample${flat ? "-flat" : ""}-${String(sec).padStart(2, "0")}.png`);
        record(`trample${flat ? "-flat" : ""}-${sec}`, q, t);
      }
    }
  }
  if (todo.includes("motion")) {
    for (const v of arg("motion-views", "meadow,wheat").split(",")) {
      const q = `view=${v}&tier=${tier}&walker=0`;
      await open(q);
      for (let i = 0; i < Number(arg("motion-n", "6")); i++) {
        const sec = Number((1.5 + i * 0.4).toFixed(1));
        const t = await holdAt(sec, `${v} motion ${sec}`);
        await shot(`motion-${v}-${i}.png`);
        record(`motion-${v}-${i}`, q, t);
      }
    }
  }
  if (todo.includes("tiers")) {
    for (const t of ["lite", "low", "medium", "high", "ultra"]) {
      for (const v of arg("tier-views", "meadow").split(",")) {
        const q = `view=${v}&tier=${t}&walker=0`;
        await open(q);
        const ti = await holdAt(3, `${v} ${t}`);
        await shot(`tier-${t}-${v}.png`);
        record(`tier-${t}-${v}`, q, ti);
      }
    }
  }
  if (todo.includes("bench")) {
    if (!gpu) throw new Error("bench needs --gpu");
    report.bench = [];
    const median = (a) => {
      const b = [...a].sort((x, y) => x - y);
      return b.length ? b[Math.floor(b.length / 2)] : null;
    };
    for (const t of arg("bench-tiers", "low,medium,high,ultra").split(",")) {
      const row = { tier: t };
      for (const on of [1, 0]) {
        await open(`view=meadow&tier=${t}&walker=1&aq=0${on ? "" : "&grass=0"}`, false);
        await waitTitle((x) => num(x, "f") > 30, `bench ${t}`);
        // Settle (pipelines, adaptive quality), then sample each new GPU reading.
        await sleep(3000);
        const g = [], f = [], tot = [];
        let last = -1, blades = 0, level = -1;
        const end = Date.now() + benchS * 1000;
        while (Date.now() < end) {
          const x = await title();
          const smp = num(x, "samples");
          if (smp !== last && num(x, "gpu") > 0) {
            last = smp;
            g.push(num(x, "grass"));
            f.push(num(x, "fwd"));
            tot.push(num(x, "gpu"));
            blades = num(x, "blades") || blades;
            level = num(x, "lvl");
          }
          await sleep(25);
        }
        const k = on ? "on" : "off";
        row[k] = { readings: g.length, grass_ms: median(g.filter((v) => v >= 0)), forward_ms: median(f), gpu_ms: median(tot), blades, level };
        log(`bench ${t} grass ${k}: ${JSON.stringify(row[k])}`);
      }
      if (row.on && row.off) row.grass_total_ms = Number(((row.on.grass_ms ?? 0) + (row.on.forward_ms - row.off.forward_ms)).toFixed(4));
      report.bench.push(row);
    }
  }
} catch (e) {
  report.error = String(e?.message ?? e);
  log("ERROR", report.error);
} finally {
  report.errors = errors.slice(0, 40);
  writeFileSync(join(out, "report.json"), JSON.stringify(report, null, 1));
  await stop();
  server.close();
}
if (report.error) process.exit(1);
