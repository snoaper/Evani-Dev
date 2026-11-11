// Ultra-Water's real wave heights at the rig's settings, read from its own water query
// (reading only; ideas and numbers, none of its code). node tools/nature/uw-probe.mjs
import { spawn, spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createServer } from "node:net";
import { startChrome, STUBS } from "../bench-runner/chrome.mjs";

const uwDir = process.env.EVANI_UW_DIR || join(tmpdir(), "ultra-water");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const freePort = () => new Promise((ok) => { const s = createServer(); s.listen(0, "127.0.0.1", () => { const p = s.address().port; s.close(() => ok(p)); }); });
const CLOCK = `${STUBS}
(() => { const q = []; let now = 0; window.requestAnimationFrame = (cb) => { q.push(cb); return q.length; }; window.cancelAnimationFrame = () => {};
  window.__step = (n, dtMs) => { if (!now) now = performance.now(); for (let i = 0; i < n; i++) { now += dtMs; const cbs = q.splice(0); for (const cb of cbs) cb(now); } return q.length; }; })();`;
const port = await freePort();
const vite = spawn(process.execPath, [join(uwDir, "node_modules", "vite", "bin", "vite.js"), "--port", String(port), "--strictPort", "--host", "127.0.0.1"], { cwd: uwDir, windowsHide: true, stdio: "ignore" });
const url = `http://127.0.0.1:${port}/`;
for (let i = 0; i < 100; i++) { try { if ((await fetch(url)).ok) break; } catch {} await sleep(200); }
const chrome = await startChrome({ width: 800, height: 450, flags: ["--ignore-gpu-blocklist", "--disable-gpu-watchdog"], prefix: "evani-uwprobe-" });
const { cdp } = chrome;
const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
const send = (m, p, ms = 120000) => cdp.send(m, p, s, ms);
const ev = async (x, ms = 200000) => { const r = await send("Runtime.evaluate", { expression: x, awaitPromise: true, returnByValue: true }, ms); if (r.exceptionDetails) throw new Error((r.exceptionDetails.exception?.description || r.exceptionDetails.text).slice(0, 600)); return r.result.value; };
await send("Page.enable"); await send("Runtime.enable");
await send("Page.addScriptToEvaluateOnNewDocument", { source: CLOCK });
await send("Page.navigate", { url });
for (let i = 0; i < 300; i++) { await sleep(200); if (await ev("!!(window.app && window.__step)").catch(() => false)) break; }
const seas = {
  open: { windSpeed: 9, fetchKm: 120, swellScale: 0.7, waveScale: 1, windDirection: 20, swellDirection: -15, choppiness: 0.45 },
  rough: { windSpeed: 15, fetchKm: 250, swellScale: 0.9, waveScale: 1, windDirection: 20, swellDirection: -15, choppiness: 0.95 },
  storm: { windSpeed: 25, fetchKm: 700, swellScale: 1.2, waveScale: 1.4, windDirection: 20, swellDirection: -15, choppiness: 1 },
};
const out = {};
for (const [name, waves] of Object.entries(seas)) {
  const res = await ev(`(async () => {
    const a = window.app; window.__step(2, 33);
    a.floaters.clear(); Object.assign(a.ocean.params, ${JSON.stringify(waves)}); a.ocean.dirty = true;
    const Q = a.query.constructor, q = new Q(a.renderer, a.surface, 512);
    const all = [];
    const ang = ${(25 * Math.PI) / 180};
    let tag = 1;
    for (let line = 0; line < 40; line++) {
      window.__step(210, 33);   // 7 s on
      // a transect 512 x 2 m along the wind from far out at sea
      const xz = new Float32Array(1024);
      const x0 = 4000 + (line % 8) * 900, z0 = 4000 - (line % 5) * 700;
      for (let i = 0; i < 512; i++) { xz[i * 2] = x0 + Math.cos(ang) * i * 2; xz[i * 2 + 1] = z0 + Math.sin(ang) * i * 2; }
      q.submit(xz, 512, tag, 0);
      for (let w = 0; w < 200 && q.resultTag !== tag; w++) { await new Promise((r) => setTimeout(r, 25)); window.__step(1, 0.01); }
      const h = []; for (let i = 0; i < 512; i++) h.push(q.result[i * 4]);
      all.push(h); tag++;
    }
    return all;
  })()`);
  const hs = res.flat();
  const m = hs.reduce((a, b) => a + b, 0) / hs.length;
  const v = hs.reduce((a, b) => a + (b - m) * (b - m), 0) / hs.length;
  const sl = []; for (const l of res) for (let i = 1; i < l.length; i++) sl.push((l[i] - l[i - 1]) / 2);
  const sv = sl.reduce((a, b) => a + b * b, 0) / sl.length;
  // mean crest-to-crest length along the transects: count of local maxima over 1 m-ish smoothing
  const sorted = [...hs].sort((a, b) => a - b);
  out[name] = { Hs: +(4 * Math.sqrt(v)).toFixed(2), mean: +m.toFixed(2), min: +sorted[0].toFixed(1), p99: +sorted[Math.floor(hs.length * 0.99)].toFixed(1), max: +sorted[hs.length - 1].toFixed(1), rms_slope_2m: +Math.sqrt(sv).toFixed(3), samples: hs.length };
  console.log(name, JSON.stringify(out[name]));
  writeFileSync(join(tmpdir(), `uw-probe-${name}.json`), JSON.stringify(res));
}
await chrome.stop().catch(() => {});
if (vite.exitCode === null) spawnSync("taskkill", ["/PID", String(vite.pid), "/T", "/F"], { stdio: "ignore" });
process.exit(0);
