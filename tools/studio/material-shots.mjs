// Headless, muted shots of one material in the studio page, for the
// one-material-at-a-time loop (docs/studio/MATERIAL_QUEUE.md).
//
//   node tools/studio/material-shots.mjs --material horse.coat.bay --out <dir>
//        [--url http://localhost:8123/] [--assets animal.horse,shape.sphere]
//        [--styles default,WK13,WK01] [--tiers ultra,high] [--light daylight]
//        [--dists close,mid,far] [--size 1600x1000] [--wait 2500] [--probe]
//
// Writes <dir>/<tier>__<asset>__<style>__<dist>.png (the 3D view only) and
// <dir>/shots.json. Drives the page only through its own buttons (CDP clicks
// and wheel events into the page, never the real mouse or keyboard); pointer
// lock, fullscreen and keyboard lock are stubbed. Uses the bench runner's
// Chrome helper: --headless=new, --mute-audio, a fresh full-path profile in
// TEMP, closed by its own PID.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const url = arg("url", "http://localhost:8123/");
const material = arg("material", "horse.coat.bay");
const out = arg("out", ".evani/shots/material");
const assets = arg("assets", "").split(",").filter(Boolean);
const styles = arg("styles", "default,WK13,WK01").split(",");
const tiers = arg("tiers", "ultra,high").split(",");
const light = arg("light", "daylight");
const dists = arg("dists", "close,mid,far").split(",");
const [W, H] = arg("size", "1600x1000").split("x").map(Number);
const wait = Number(arg("wait", "2500"));
// --edits sheen=0.8,clump=0: try numbers through the inspector's sliders
// (remembered per record, so they are set again after every style change).
const edits = arg("edits", "").split(",").filter(Boolean).map((e) => e.split("="));
mkdirSync(out, { recursive: true });

const { cdp, stop } = await startChrome({ width: W, height: H, prefix: "evani-matshots-" });
const shots = [];
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, 120000);
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS + ";try{localStorage.clear()}catch(e){}" });
  const logs = [];
  cdp.on((m) => {
    if (m.sessionId === s && m.method === "Runtime.consoleAPICalled" && /error|warn/.test(m.params.type))
      logs.push(m.params.args.map((a) => a.value ?? a.description).join(" "));
  });
  const ev = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(expr.slice(0, 80) + ": " + JSON.stringify(r.exceptionDetails).slice(0, 300));
    return r.result.value;
  };
  await send("Page.navigate", { url });
  for (let i = 0; i < 240 && !(await ev(`!!document.querySelector('[data-act="mat"]')`)); i++) await sleep(500);
  await sleep(3000);
  const click = (act, v) =>
    ev(`(() => { const b = [...document.querySelectorAll('[data-act="${act}"]')].find(e => ${v === undefined ? "true" : `e.dataset.v === ${JSON.stringify(v)}`}); if (!b) return false; b.click(); return true; })()`);
  const pick = (act, v) =>
    ev(`(() => { const e = document.querySelector('[data-act="${act}"]'); if (!e) return false; e.value = ${JSON.stringify(v)}; e.dispatchEvent(new Event('change', {bubbles:true})); return true; })()`);
  // The 3D view: the studio's view area (else the largest canvas).
  const view = () =>
    ev(`(() => { const c = (document.getElementById('st-view') || [...document.querySelectorAll('canvas')].sort((a,b) => b.width*b.height - a.width*a.height)[0]).getBoundingClientRect(); return {x: Math.round(c.x), y: Math.round(c.y), w: Math.round(c.width), h: Math.round(c.height)}; })()`);
  // Wait until the page has no loading or preparing notice up (at most 40 s).
  const settle = async () => {
    for (let i = 0; i < 80; i++) {
      if (!(await ev(`/Loading the|Preparing materials|Building the|being made/i.test(document.body.innerText)`))) return true;
      await sleep(500);
    }
    return false;
  };
  const state = () =>
    ev(`(() => ({ style: document.querySelector('[data-act="style"][aria-selected="true"]')?.dataset.v, mat: document.querySelector('.st-chip.on')?.dataset.v, light: document.querySelector('.st-light.on')?.dataset.v, quality: document.querySelector('[data-act="quality"]')?.value, qualities: [...(document.querySelector('[data-act="quality"]')?.options||[])].map(o=>o.value), cur: document.querySelector('.st-cur-tx')?.innerText, notice: document.querySelector('.st-notice')?.innerText }))()`);
  if (has("probe")) {
    console.log(JSON.stringify(await state(), null, 1), await view());
    const r = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(out, "probe.png"), Buffer.from(r.data, "base64"));
    console.log(logs.slice(0, 20).join("\n"));
  } else {
    const first = await state();
    const defStyle = first.style;
    await click("light", light);
    // Search for the material so its chip is on the bar, then pick it.
    const findMat = async () => {
      if (await click("mat", material)) return true;
      await ev(`(() => { const e = document.querySelector('[data-act="mat-q"]'); e.value = ${JSON.stringify(material)}; e.dispatchEvent(new Event('input', {bubbles:true})); })()`);
      await sleep(800);
      const ok = await click("mat", material);
      await sleep(300);
      await click("mat-all");
      return ok;
    };
    // A real (synthetic CDP) click at an element's centre: the camera rail
    // listens to pointer events, not to element.click().
    const press = async (sel) => {
      const b = await ev(`(() => { const r = document.querySelector('${sel}')?.getBoundingClientRect(); return r ? {x: r.x + r.width / 2, y: r.y + r.height / 2} : null; })()`);
      if (!b) return false;
      for (const type of ["mouseMoved", "mousePressed", "mouseReleased"])
        await send("Input.dispatchMouseEvent", { type, x: b.x, y: b.y, button: "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
      return true;
    };
    const zoom = async (dist) => {
      await press('[data-svp="reset"]');
      await sleep(500);
      const r = await view();
      const x = r.x + r.w / 2;
      const y = r.y + r.h / 2;
      const n = dist === "close" ? -Number(arg("close", "5")) : dist === "far" ? Number(arg("far", "6")) : 0;
      for (let i = 0; i < Math.abs(n); i++) {
        await send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY: Math.sign(n) * 100 });
        await sleep(60);
      }
    };
    for (const tier of tiers) {
      await pick("quality", tier);
      await sleep(1500);
      for (const asset of assets.length ? assets : [null]) {
        if (asset && !(await click("asset", asset))) {
          await click("tab", "all");
          await sleep(500);
          if (!(await click("asset", asset))) console.log("asset not found:", asset);
        }
        await sleep(800);
        if (!(await findMat())) console.log("material chip not found:", material, "on", asset);
        for (const style of styles) {
          const sid = style === "default" ? defStyle : style;
          await click("style", sid);
          await sleep(wait);
          for (const [k, v] of edits) {
            const ok = await ev(`(() => { const e = [...document.querySelectorAll('input[data-act="switch"]')].find(i => i.dataset.v === ${JSON.stringify(k)} || i.dataset.v.endsWith('.' + ${JSON.stringify(k)}) || i.dataset.v.endsWith('_' + ${JSON.stringify(k)})); if (!e) return [...document.querySelectorAll('input[data-act="switch"]')].map(i => i.dataset.v).join(' '); e.value = ${JSON.stringify(v)}; e.dispatchEvent(new Event('input', {bubbles:true})); e.dispatchEvent(new Event('change', {bubbles:true})); return true; })()`);
            if (ok !== true) console.log("no slider", k, "among", ok);
            await sleep(150);
          }
          if (edits.length) await sleep(wait);
          for (const dist of dists) {
            await zoom(dist);
            await sleep(wait);
            if (!(await settle())) console.log("still loading:", material, asset, sid, dist);
            const r = await view();
            const st = await state();
            const name = `${tier}__${(asset || "default").replace(/\./g, "-")}__${sid}__${dist}.png`;
            const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: r.x, y: r.y, width: r.w, height: r.h, scale: 1 } });
            writeFileSync(join(out, name), Buffer.from(shot.data, "base64"));
            shots.push({ name, tier, asset, style: st.style, dist, material: st.mat, light: st.light, quality: st.quality, cur: st.cur });
            console.log(name, st.mat, st.light, st.quality);
          }
        }
      }
    }
    writeFileSync(join(out, "shots.json"), JSON.stringify({ url, material, light, default_style: defStyle, shots, logs: logs.slice(0, 50) }, null, 1));
  }
} finally {
  await stop();
}
