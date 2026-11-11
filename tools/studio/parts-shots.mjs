// Headless, muted shots of the studio page driven through its own controls,
// for checking per-part materials (docs/studio/CONTRACTS.md, "Parts").
//
//   node tools/studio/parts-shots.mjs --url http://localhost:8123/ --out <dir>
//        [--plan <plan.json>] [--profile-root <dir>] [--wait 3000] [--cpu] [--cdp-timeout 180000]
//
// A plan is a list of shots: {"name", "size": "1280x800", "full": true|false,
// "steps": [["asset", "building.roof"], ["style", "WK01"], ["part", "gable"],
// ["mat", "brick.red.common"], ["part", ""], ["tap", 0.5, 0.45], ["wait", 2000]]}.
// "asset" and "mat" find their control through the page's own search boxes,
// "part" clicks the inspector's parts list ("" is All parts), "click" clicks [data-act=kind][data-v=v], "svp" clicks a view button
// (`mode-material`, `mode-render`), "tap" taps the
// 3D view at a share of the canvas (CDP touch-free mouse events into the
// page, never the real mouse). Writes <dir>/<name>.png (the whole page with
// "full", else the view) and <dir>/shots.json (each shot's record and the
// page's warnings). Chrome: --headless=new, --mute-audio, a fresh profile
// under --profile-root (a full path), pointer lock, fullscreen and keyboard
// lock stubbed; closed by its own PID. --cpu asks for WebGPU on SwiftShader
// (no graphics card).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const url = arg("url", "http://localhost:8123/");
const out = resolve(arg("out", ".evani/shots/st-parts"));
const wait = Number(arg("wait", "3000"));
const plan = JSON.parse(readFileSync(arg("plan", ""), "utf8"));
const root = arg("profile-root", "");
if (root) {
  mkdirSync(resolve(root), { recursive: true });
  process.env.TEMP = process.env.TMP = resolve(root);
}
const flags = process.argv.includes("--cpu") ? ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader"] : [];
mkdirSync(out, { recursive: true });

const { cdp, stop } = await startChrome({ width: 2560, height: 1440, prefix: "evani-stparts-", flags });
const report = [];
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, Number(arg("cdp-timeout", "180000")));
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + ";try{localStorage.clear();indexedDB.deleteDatabase&&0}catch(e){}" });
  const logs = [];
  cdp.on((m) => {
    if (m.sessionId === s && m.method === "Runtime.consoleAPICalled" && /error|warn/.test(m.params.type))
      logs.push(m.params.args.map((a) => a.value ?? a.description).join(" ").slice(0, 300));
  });
  const ev = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(expr.slice(0, 80) + ": " + JSON.stringify(r.exceptionDetails).slice(0, 300));
    return r.result.value;
  };
  const q = (kind, v) => `document.querySelector('[data-act="${kind}"][data-v="${v}"]')`;
  const until = async (expr, ms = 20000) => {
    for (let t = 0; t < ms; t += 250) {
      if (await ev(expr)) return true;
      await sleep(250);
    }
    return false;
  };
  const type = async (kind, text) =>
    ev(`(() => { const i = document.querySelector('input[data-act="${kind}"]'); if (!i) return false; i.value = ${JSON.stringify(text)}; i.dispatchEvent(new Event('input', {bubbles: true})); return true; })()`);
  const click = async (kind, v) => ev(`(() => { const e = ${q(kind, v)}; if (!e) return false; e.click(); return true; })()`);

  for (const shot of plan) {
    const [W, H] = (shot.size || "1280x800").split("x").map(Number);
    await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    if (shot.reload !== false) {
      await send("Page.navigate", { url });
      await until(`!!document.querySelector('[data-act="mat"]')`, 120000);
      await sleep(1500);
    }
    const done = [];
    for (const [what, a, b] of shot.steps || []) {
      let ok = false;
      if (what === "asset") {
        ok = await click("asset", a);
        if (!ok) {
          await type("asset-q", a.split(".").pop());
          ok = (await until(`!!${q("asset", a)}`, 5000)) && (await click("asset", a));
          await type("asset-q", "");
        }
      } else if (what === "mat") {
        ok = await click("mat", a);
        if (!ok) {
          const name = await ev(`(() => { for (const e of document.querySelectorAll('[data-act="mat"]')) if (e.dataset.v === ${JSON.stringify(a)}) return e.title; return ${JSON.stringify(a.split(".").slice(1).join(" "))}; })()`);
          await type("mat-q", b || name);
          ok = (await until(`!!${q("mat", a)}`, 8000)) && (await click("mat", a));
          await type("mat-q", "");
        }
      } else if (what === "style" || what === "part" || what === "light") {
        ok = await click(what, a);
      } else if (what === "click") {
        ok = await click(a, b);
      } else if (what === "svp") {
        ok = await ev(`(() => { const e = document.querySelector('[data-svp="${a}"]'); if (!e) return false; e.click(); return true; })()`);
      } else if (what === "tap") {
        const r = await ev(`(() => { const c = document.getElementById('evani-canvas').getBoundingClientRect(); return [c.left, c.top, c.width, c.height]; })()`);
        const [x, y] = [r[0] + r[2] * a, r[1] + r[3] * b];
        // Event times 50 ms apart: a slow (CPU-drawn) page would otherwise
        // see a long press, which the view treats as the start of a drag.
        const t0 = Date.now() / 1000;
        await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1, pointerType: "mouse", timestamp: t0 });
        await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1, pointerType: "mouse", timestamp: t0 + 0.05 });
        ok = true;
      } else if (what === "wait") {
        await sleep(a);
        ok = true;
      }
      done.push([what, a, ok]);
      await sleep(400);
    }
    await sleep(shot.wait ?? wait);
    // Materials compile slowly on a CPU adapter: shoot once the page's
    // "Preparing materials" note has gone (at most `settle` ms) and the
    // view has had a moment to draw.
    const settled = await until(`!document.body.innerText.includes('Preparing materials')`, shot.settle ?? 120000);
    await sleep(1500);
    const file = join(out, shot.name + ".png");
    let clip;
    if (!shot.full) {
      const r = await ev(`(() => { const v = document.getElementById('st-view') || document.getElementById('evani-canvas'); const c = v.getBoundingClientRect(); return [c.left, c.top, c.width, c.height]; })()`);
      clip = { x: r[0], y: r[1], width: r[2], height: r[3], scale: 1 };
    }
    const shotImg = await send("Page.captureScreenshot", clip ? { format: "png", clip } : { format: "png" });
    writeFileSync(file, Buffer.from(shotImg.data, "base64"));
    const state = await ev(`(() => { try { return localStorage.getItem('evani.studio.state'); } catch (e) { return null; } })()`);
    const parts = await ev(`[...document.querySelectorAll('.st-part')].map(e => e.title)`);
    report.push({ name: shot.name, file, size: [W, H], steps: done, settled, parts, state: state && JSON.parse(state).selection, part: state && JSON.parse(state).part, warnings: logs.splice(0) });
    console.log(shot.name, done.map((d) => d.join(":")).join(" "), file);
  }
} finally {
  writeFileSync(join(out, "shots.json"), JSON.stringify(report, null, 1));
  await stop();
}
