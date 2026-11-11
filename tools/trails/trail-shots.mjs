// The ribbons and trails proof's harness (plan item R5; sandbox feature
// `trails-proof`). Serves a built sandbox, opens it in headless muted Chrome
// (a fresh full-path profile, locks stubbed, closed by its own PID) and:
//
//   shots     every view (0-7) as strip, dense and off at one tier, the sim
//             clock held on the same tick, so each pair is the same moment
//             and camera; plus the blind pairs a critic judges.
//   gaps      for each picture, how many separate pieces the marks fall into
//             (off picture subtracted): a strip with a gap shows as two.
//   temporal  a still camera and steady colours, stepping tick by tick: the
//             worst change between frames among pixels the marks cover in both,
//             away from the strips' moving ends (new ground the head reaches
//             and ground the tail leaves are not a flicker). The plan's pass
//             line: under 0.1 at the 99.9th percentile. Raw whole-frame numbers
//             beside it.
//   tiers     view 0 and 4 on every tier, and the tier's whole cap of emitters.
//   screen    a comet in screen space, between the sprite layers.
//   bench     (--gpu only) the tier's whole cap of emitters, the draws toggling;
//             the compute pass's median, and the forward pass with the draws
//             less without them, median over neighbouring windows.
//
//   node tools/trails/trail-shots.mjs --dist <built dist> --out <dir>
//        [--do shots,gaps,temporal,tiers,screen,bench] [--gpu] [--size 1600x900]
//        [--tier high] [--timeout-s 120] [--bench-frames 6000] [--steps 5]
//
// Default is WebGPU on SwiftShader (the CPU); --gpu uses the graphics card.
// Writes PNGs and report.json into <out>; exits 1 when a pass line fails.
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, extname } from "node:path";
import { inflateSync } from "node:zlib";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const dist = arg("dist", "");
const out = arg("out", ".evani/shots/r5-ribbons");
if (!dist || !existsSync(dist)) {
  console.error("--dist <built sandbox folder> is needed");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const gpu = has("gpu");
const [W, H] = arg("size", "1600x900").split("x").map(Number);
const tier = arg("tier", "high");
const timeoutMs = Number(arg("timeout-s", "120")) * 1000;
const todo = arg("do", "shots,gaps,temporal,tiers,screen").split(",");
const benchFrames = Number(arg("bench-frames", "6000"));
const KART_HOLD = 175;
const SWORD_HOLD = 333;
const holdOf = (view) => (view < 4 ? KART_HOLD : SWORD_HOLD);
const STEPS = Number(arg("steps", "5"));
// `--extra flame=0`: more address flags for every page.
const EXTRA = arg("extra", "");
// `--views 0,2`: only those views (shots, gaps, temporal).
const ONLY = arg("views", "0,1,2,3,4,5,6,7").split(",").map(Number);
const VIEW_NAMES = ["kart-chase", "kart-high", "kart-side", "kart-front", "sword-front", "sword-side", "sword-above", "sword-close"];

// ---- a small PNG reader (what CDP screenshots are: 8-bit RGB or RGBA) --------
function decodePng(buf) {
  let p = 8;
  let w = 0, h = 0, ch = 4;
  const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p);
    const type = buf.toString("latin1", p + 4, p + 8);
    const d = buf.subarray(p + 8, p + 8 + len);
    if (type === "IHDR") {
      w = d.readUInt32BE(0);
      h = d.readUInt32BE(4);
      ch = d[9] === 6 ? 4 : d[9] === 2 ? 3 : 0;
      if (d[8] !== 8 || !ch || d[12]) throw new Error("unexpected PNG format");
    } else if (type === "IDAT") idat.push(d);
    p += 12 + len;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * ch;
  const rgb = new Uint8Array(w * h * 3);
  let prev = new Uint8Array(stride);
  let at = 0;
  for (let y = 0; y < h; y++) {
    const f = raw[at++];
    const line = new Uint8Array(stride);
    for (let x = 0; x < stride; x++) {
      const a = x >= ch ? line[x - ch] : 0;
      const b = prev[x];
      const c = x >= ch ? prev[x - ch] : 0;
      let v = raw[at + x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const pp = a + b - c;
        const pa = Math.abs(pp - a), pb = Math.abs(pp - b), pc = Math.abs(pp - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      line[x] = v & 255;
    }
    at += stride;
    for (let x = 0; x < w; x++) {
      for (let k = 0; k < 3; k++) rgb[(y * w + x) * 3 + k] = line[x * ch + k];
    }
    prev = line;
  }
  return { w, h, rgb };
}

const pixelDiff = (a, b, i) => Math.max(Math.abs(a[i] - b[i]), Math.abs(a[i + 1] - b[i + 1]), Math.abs(a[i + 2] - b[i + 2]));

// The marks' pieces: connected regions of what differs from the off picture
// (actor hidden in both). Very small specks are not counted as pieces.
function pieces(img, off, threshold = 10, minPx = 12) {
  const { w, h, rgb } = img;
  const on = new Uint8Array(w * h);
  let covered = 0;
  for (let i = 0; i < w * h; i++) {
    if (pixelDiff(rgb, off.rgb, i * 3) > threshold) {
      on[i] = 1;
      covered++;
    }
  }
  const seen = new Uint8Array(w * h);
  const stack = [];
  let count = 0, specks = 0, largest = 0;
  for (let i = 0; i < w * h; i++) {
    if (!on[i] || seen[i]) continue;
    let n = 0;
    stack.push(i);
    seen[i] = 1;
    while (stack.length) {
      const q = stack.pop();
      n++;
      const x = q % w, y = (q / w) | 0;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const nx = x + dx, ny = y + dy;
          if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
          const r = ny * w + nx;
          if (on[r] && !seen[r]) {
            seen[r] = 1;
            stack.push(r);
          }
        }
      }
    }
    if (n >= minPx) count++;
    else specks++;
    largest = Math.max(largest, n);
  }
  return { pieces: count, specks, largest, covered };
}

// The strips' ends as the page reports them (CSS pixels at this size).
const endsOf = (title) => (field(title, "ends") ?? "").split(";").filter(Boolean).map((p) => p.split(",").map(Number));
const onScreen = ([x, y], w, h) => Number.isFinite(x) && Number.isFinite(y) && x > -200 && y > -200 && x < w + 200 && y < h + 200;

// The frame-to-frame score: 0 to 1 per pixel, the largest channel change.
// `covered` looks only at pixels the marks cover in both frames, leaving out
// the ground a strip's head reaches or its tail leaves between the two (a
// disc round each end, as wide as the end moved). What is left is the strip's
// body, which should simply stay still.
function temporalScore(a, b, off, endsA, endsB, threshold = 10) {
  const { w, h } = a;
  const covered = [];
  const whole = [];
  let moved = 0;
  // Ends off screen (or behind the camera) do not count; the disc is capped so
  // it can never cover the whole frame.
  endsA.forEach((p, i) => {
    const q = endsB[i];
    if (q && onScreen(p, w, h) && onScreen(q, w, h)) moved = Math.max(moved, Math.hypot(p[0] - q[0], p[1] - q[1]));
  });
  const r = Math.min(36 + 2 * moved, 120);
  const keep = new Uint8Array(w * h).fill(1);
  for (const [cx, cy] of [...endsA, ...endsB]) {
    if (!Number.isFinite(cx) || !Number.isFinite(cy)) continue;
    const x0 = Math.max(0, Math.floor(cx - r)), x1 = Math.min(w - 1, Math.ceil(cx + r));
    const y0 = Math.max(0, Math.floor(cy - r)), y1 = Math.min(h - 1, Math.ceil(cy + r));
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        if ((x - cx) ** 2 + (y - cy) ** 2 <= r * r) keep[y * w + x] = 0;
      }
    }
  }
  for (let i = 0; i < w * h; i++) {
    const d = pixelDiff(a.rgb, b.rgb, i * 3);
    whole.push(d);
    if (keep[i] && pixelDiff(a.rgb, off.rgb, i * 3) > threshold && pixelDiff(b.rgb, off.rgb, i * 3) > threshold) covered.push(d);
  }
  const at = (v, q) => {
    if (!v.length) return 0;
    const s = Uint16Array.from(v).sort();
    return s[Math.min(s.length - 1, Math.floor(s.length * q))] / 255;
  };
  return {
    covered_px: covered.length,
    covered_p999: Number(at(covered, 0.999).toFixed(3)),
    covered_max: Number(at(covered, 1).toFixed(3)),
    whole_p999: Number(at(whole, 0.999).toFixed(3)),
    mask_radius_px: Math.round(r),
  };
}

// ---- the page ------------------------------------------------------------------
const MIME = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".glb": "model/gltf-binary", ".css": "text/css" };
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

// Frames uncapped for the cost: at 60 frames a second the card idles at low
// clocks, which inflates every time it reports.
const flags = gpu ? (todo.includes("bench") ? ["--disable-gpu-vsync", "--disable-frame-rate-limit"] : []) : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader"];
const { cdp, stop, version } = await startChrome({ width: W, height: H, flags, prefix: "evani-trails-" });
console.error(`[trails] ${version} ${gpu ? "gpu" : "swiftshader"}`);
const target = await cdp.send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: target.targetId, flatten: true });
const s = (m, p = {}, t) => cdp.send(m, p, sessionId, t);
await s("Page.enable");
await s("Runtime.enable");
await s("Emulation.setFocusEmulationEnabled", { enabled: true });
await s("Page.bringToFront");
await s("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
await s("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
// The debug overlay and panels never show; the tier the address asks for is
// the saved one, so the start-up measuring never picks another.
await s("Page.addScriptToEvaluateOnNewDocument", {
  source: `(() => {
    try {
      const q = new URLSearchParams(location.search);
      const t = q.get('tier');
      if (t) localStorage.setItem('evani:sandbox:render.tier', t);
      // A steady clock: every animation frame is exactly one fixed tick,
      // however slowly the picture is drawn, so a hold lands on its tick and a
      // step moves one tick (the CPU renderer draws a few frames a second).
      if (q.get('vclock')) {
        let now = 0;
        const raf = window.requestAnimationFrame.bind(window);
        performance.now = () => now;
        // Four ticks a frame on the way to the hold: the same ticks run, fewer
        // pictures are drawn; the last dozen go one by one.
        const fast = (Number(q.get('hold')) || 0) - 12;
        window.requestAnimationFrame = (cb) => raf(() => { now += ((now * 60) / 1000 < fast ? 4 : 1) * (1000 / 60); cb(now); });
      }
    } catch (e) {}
    const add = () => {
      const st = document.createElement('style');
      st.textContent = '#evani-overlay, #evani-panel, #sandbox-hud, #evani-convert-zone { display: none !important; }';
      (document.head || document.documentElement).appendChild(st);
    };
    if (document.documentElement) add();
    else document.addEventListener('DOMContentLoaded', add, { once: true });
  })();`,
});
const errors = [];
cdp.on((m) => {
  if (m.method === "Runtime.exceptionThrown") errors.push(m.params?.exceptionDetails?.exception?.description ?? m.params?.exceptionDetails?.text ?? "exception");
  if (m.method === "Runtime.consoleAPICalled" && m.params?.type === "error") errors.push((m.params.args ?? []).map((a) => a.value ?? a.description ?? "").join(" ").slice(0, 600));
});

const evalJs = async (expression) => {
  const r = await s("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  return r?.result?.value;
};
const title = () => evalJs("document.title");
const field = (t, k) => new RegExp(`\\b${k}=(\\S+)`).exec(t)?.[1];

async function waitTitle(test, label, ms = timeoutMs) {
  const end = Date.now() + ms;
  let t = "";
  while (Date.now() < end) {
    t = (await title()) ?? "";
    if (t.startsWith("tr ") && test(t)) return t;
    await sleep(50);
  }
  const engine = await evalJs("JSON.stringify({ engine: window.__engine, url: location.href, body: document.body?.innerText?.slice(0, 200) })");
  throw new Error(`${label}: timed out; title "${t}"; page ${engine}; errors ${errors.join("; ")}`);
}

async function open(query) {
  const url = `http://127.0.0.1:${port}/?${query}${query.includes("bench=") ? "" : "&vclock=1"}${EXTRA ? "&" + EXTRA : ""}`;
  await s("Page.navigate", { url });
  await sleep(300);
}

async function shot(name) {
  // Two presented frames after the hold, so the picture is the held one.
  await evalJs("new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(r))))");
  const r = await s("Page.captureScreenshot", { format: "png", fromSurface: true }, 60000);
  const buf = Buffer.from(r.data, "base64");
  if (name) writeFileSync(join(out, name), buf);
  return buf;
}

async function heldShot(query, name, hold) {
  await open(`${query}&hold=${hold}`);
  const t = await waitTitle((t) => field(t, "held") === "1", query);
  const buf = await shot(name);
  return { buf, tick: Number(field(t, "tick")), live: Number(field(t, "live")), title: t };
}

const report = { chrome: version, gpu, size: [W, H], tier };
let failed = false;

try {
  // ---- shots and gaps --------------------------------------------------------
  if (todo.includes("shots") || todo.includes("gaps")) {
    report.views = [];
    for (let view = 0; view < 8; view++) {
      if (!ONLY.includes(view)) continue;
      const name = VIEW_NAMES[view];
      const hold = holdOf(view);
      const row = { view, name, hold };
      const a = await heldShot(`tr=strip&view=${view}&tier=${tier}`, `${name}-strip.png`, hold);
      const b = await heldShot(`tr=dense&view=${view}&tier=${tier}`, `${name}-dense.png`, hold);
      const o = await heldShot(`tr=off&view=${view}&tier=${tier}`, `${name}-off.png`, hold);
      row.tick = [a.tick, b.tick, o.tick];
      // The same moment, or the pair is no comparison.
      if (a.tick !== b.tick || a.tick !== o.tick) {
        failed = true;
        row.error = "held on different ticks";
      }
      if (todo.includes("gaps")) {
        // No actor, no bloom, steady colours: what differs from the off
        // picture is the marks and nothing else.
        const q = `view=${view}&tier=${tier}&actor=0&steady=1`;
        const off = decodePng((await heldShot(`tr=off&${q}`, null, hold)).buf);
        row.strip = pieces(decodePng((await heldShot(`tr=strip&${q}`, null, hold)).buf), off);
        row.dense = pieces(decodePng((await heldShot(`tr=dense&${q}`, null, hold)).buf), off);
      }
      report.views.push(row);
      // A mark that vanished is one piece too; its coverage tells.
      if (row.strip && row.dense && row.strip.covered < 0.25 * row.dense.covered) {
        failed = true;
        row.error = "the strip covers under a quarter of what the dense particles cover";
      }
      console.error(`[trails] ${name}: strip ${row.strip?.pieces ?? "-"} pieces over ${row.strip?.covered ?? "-"} px, dense ${row.dense?.pieces ?? "-"} pieces (${row.dense?.specks ?? "-"} specks) over ${row.dense?.covered ?? "-"} px`);
    }
    // Blind pairs: same picture order shuffled by a fixed seed; the key stays beside them.
    const key = {};
    let seed = 20261102;
    mkdirSync(join(out, "blind"), { recursive: true });
    for (let view = 0; view < 8; view++) {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      if (!existsSync(join(out, `${VIEW_NAMES[view]}-strip.png`)) || !existsSync(join(out, `${VIEW_NAMES[view]}-dense.png`))) continue;
      const stripFirst = (seed >> 16) % 2 === 0;
      const name = VIEW_NAMES[view];
      const left = readFileSync(join(out, `${name}-${stripFirst ? "strip" : "dense"}.png`));
      const right = readFileSync(join(out, `${name}-${stripFirst ? "dense" : "strip"}.png`));
      writeFileSync(join(out, "blind", `view${view + 1}-A.png`), left);
      writeFileSync(join(out, "blind", `view${view + 1}-B.png`), right);
      key[`view${view + 1}`] = { A: stripFirst ? "strip" : "dense", B: stripFirst ? "dense" : "strip", name };
    }
    writeFileSync(join(out, "blind-key.json"), JSON.stringify(key, null, 1));
  }

  // ---- temporal ---------------------------------------------------------------
  if (todo.includes("temporal")) {
    report.temporal = [];
    for (let view = 0; view < 8; view++) {
      if (!ONLY.includes(view)) continue;
      const name = VIEW_NAMES[view];
      const hold = holdOf(view);
      const row = { view, name };
      const q = `view=${view}&tier=${tier}&still=${hold}&actor=0&steady=1`;
      const offShot = await heldShot(`tr=off&${q}`, null, hold);
      const off = decodePng(offShot.buf);
      for (const marks of ["strip", "dense"]) {
        await open(`tr=${marks}&${q}&hold=${hold - STEPS}`);
        let t0 = await waitTitle((t) => field(t, "held") === "1", `${name} ${marks}`);
        let prev = decodePng(await shot(null));
        t0 = await title();
        let worst = null;
        const steps = [];
        for (let i = 1; i <= STEPS; i++) {
          await evalJs(`location.hash = 'h=${hold - STEPS + i}'`);
          await waitTitle((t) => field(t, "held") === "1" && Number(field(t, "tick")) >= hold - STEPS + i, `${name} ${marks} step`);
          const img = decodePng(await shot(null));
          const t1 = await title();
          const sc = temporalScore(prev, img, off, endsOf(t0), endsOf(t1));
          sc.tick = Number(field(t1, "tick"));
          steps.push(sc);
          if (!worst || sc.covered_p999 > worst.covered_p999) worst = sc;
          prev = img;
          t0 = t1;
        }
        row[marks] = { worst, steps };
      }
      report.temporal.push(row);
      if (row.strip.worst.covered_p999 >= 0.1) failed = true;
      // No covered pixels is no evidence.
      if (row.strip.steps.some((st) => st.covered_px === 0)) {
        failed = true;
        row.error = "no covered pixels to score";
      }
      console.error(`[trails] ${name}: temporal strip ${row.strip.worst.covered_p999} (raw ${row.strip.worst.whole_p999}), dense ${row.dense.worst.covered_p999} (raw ${row.dense.worst.whole_p999})`);
    }
  }

  // ---- every tier -------------------------------------------------------------
  if (todo.includes("tiers")) {
    report.tiers = [];
    for (const t of ["ultra", "high", "medium", "low"]) {
      for (const view of [0, 4]) {
        const a = await heldShot(`tr=strip&view=${view}&tier=${t}`, `tier-${t}-${VIEW_NAMES[view]}.png`, holdOf(view));
        const u = { tier: t, view, live: a.live, title: a.title };
        report.tiers.push(u);
        if (a.live < 2) {
          failed = true;
          u.error = "no strips live";
        }
      }
      const st = await heldShot(`tr=strip&set=stress&tier=${t}`, `tier-${t}-stress.png`, 90);
      const cap = Number(field(st.title, "cap"));
      const row = { tier: t, set: "stress", cap, live: st.live, draws: Number(field(st.title, "draws")), tris: Number(field(st.title, "tris")) };
      report.tiers.push(row);
      if (st.live !== cap) {
        failed = true;
        row.error = "not at the tier's cap";
      }
      console.error(`[trails] tier ${t}: stress ${st.live} of ${cap}`);
    }
  }

  // ---- screen space -----------------------------------------------------------
  if (todo.includes("screen")) {
    const a = await heldShot(`tr=strip&set=screen&tier=${tier}`, "screen-strip.png", 120);
    report.screen = { live: a.live, draws: Number(field(a.title, "draws")) };
    if (a.live < 1) failed = true;
    console.error(`[trails] screen: ${a.live} live`);
  }

  // ---- GPU cost ---------------------------------------------------------------
  if (todo.includes("bench")) {
    report.bench = [];
    for (const t of ["ultra", "high", "medium", "low"]) {
      await open(`tr=strip&set=stress&tier=${t}&bench=1`);
      const end = await waitTitle((x) => Number(field(x, "frames")) >= benchFrames, `bench ${t}`, Math.max(timeoutMs, benchFrames * 200));
      const row = {
        tier: t,
        frames: Number(field(end, "frames")),
        windows: Number(field(end, "n")),
        sim_ms: Number(field(end, "sim")),
        draw_ms: Number(field(end, "draw")),
        forward_without_ms: Number(field(end, "fwd_off")),
        cap: Number(field(end, "cap")),
        tris: Number(field(end, "tris")),
      };
      row.cost_ms = Number((row.sim_ms + row.draw_ms).toFixed(4));
      report.bench.push(row);
      console.error(`[trails] bench ${t}: sim ${row.sim_ms} + draw ${row.draw_ms} = ${row.cost_ms} ms (cap ${row.cap}, ${row.windows} windows)`);
    }
  }
} catch (e) {
  failed = true;
  report.error = String(e?.stack ?? e);
  console.error(report.error);
}

report.errors = errors;
writeFileSync(join(out, "report.json"), JSON.stringify(report, null, 1));
await stop();
server.close();
process.exit(failed ? 1 : 0);
