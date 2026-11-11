// The realism parity proof (procedural realism round 8; docs/PROCEDURAL.md
// "Applying a realism material in a game"): headless, muted shots of the
// proof game's parity scene (examples/proc-field built with its `wall`
// feature: the library's cast-concrete cube in the engine's realism studio).
//
//   node tools/procedural/parity.mjs --url http://127.0.0.1:8192/ --out <dir>
//        [--size 1600x900] [--wait-ms 9000]
//
// Writes <dir>/game-parity.png (kit.proc_field.view 0: the library's cube
// under the library's camera, to hold against the library's own shot with
// tools/procedural/parity_diff.py), <dir>/game-wall.png (view 1: the same
// concrete as a wall), <dir>/game-owner-ref.png (view 2: the scene of the
// owner's reference picture 4) and <dir>/diag-*.png (view 2 in the concrete's
// diagnostic views: base colour, normal, roughness, porosity, edge wear,
// height). The view is set through the dev build's
// window.__engine.paramSet; never the real mouse or keyboard. Chrome runs
// --headless=new and --mute-audio on a fresh profile, closed by its own PID.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const url = arg("url", "http://127.0.0.1:8192/");
const out = arg("out", ".evani/shots/parity");
const [W, H] = arg("size", "1600x900").split("x").map(Number);
const waitMs = Number(arg("wait-ms", "9000"));
mkdirSync(out, { recursive: true });
const { cdp, stop } = await startChrome({ width: W, height: H, flags: [], prefix: "evani-parity-" });
const done = setTimeout(() => {
  console.error("parity: 240 s limit reached; stopping");
  stop().finally(() => process.exit(3));
}, 240000);
done.unref();
let code = 0;
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, 120000);
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + ";try{localStorage.clear();localStorage.setItem('evani:render.tier','high')}catch(e){}" });
  const ev = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(expr.slice(0, 80) + ": " + JSON.stringify(r.exceptionDetails).slice(0, 300));
    return r.result.value;
  };
  const frames = (n) => ev(`new Promise(r => { let k = 0; const f = () => { if (++k >= ${n}) r(k); else requestAnimationFrame(f); }; requestAnimationFrame(f); })`);
  const shot = async (file) => {
    const { data } = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(out, file), Buffer.from(data, "base64"));
    console.error("shot", file);
  };
  await send("Page.navigate", { url });
  // Until the engine's hooks are up, then the pipelines and the material.
  for (let i = 0; i < 120 && !(await ev(`!!(window.__engine && window.__engine.paramSet)`)); i++) await sleep(500);
  await sleep(waitMs);
  await frames(30);
  // Round 9: view 2 is the owner's reference picture's scene (camera, light,
  // floor, backdrop), shot as it is and then in the concrete's diagnostic
  // views (procedural.geometry.cube_cuboid.view 1-6).
  const DIAG = ["base", "normal", "roughness", "porosity", "edge-wear", "height"];
  const shots = [[0, "game-parity.png"], [1, "game-wall.png"], [2, "game-owner-ref.png"], ...DIAG.map((d, i) => [2, `diag-${d}.png`, i + 1])];
  for (const [view, file, diag] of shots) {
    await ev(`window.__engine && window.__engine.paramSet ? window.__engine.paramSet('procedural.geometry.cube_cuboid.view', ${diag || 0}) : false`);
    const ok = await ev(`window.__engine && window.__engine.paramSet ? window.__engine.paramSet('kit.proc_field.view', ${view}) : false`);
    if (!ok) console.error("no paramSet hook: shot as it is");
    await sleep(waitMs / 2);
    // The engine's debug overlay (a text box, top left) is not the scene.
    await ev(`(() => { const o = document.getElementById('evani-overlay'); if (o) o.style.visibility = 'hidden'; return true; })()`);
    await frames(30);
    await shot(file);
  }
} catch (e) {
  console.error(String((e && e.stack) || e));
  code = 1;
} finally {
  await stop();
  process.exit(code);
}
