// Reference shots of a web page (the Ultra-Water demo the spectral sea is
// compared with): a headless, muted Chrome on a fresh full-path profile
// (SwiftShader, no graphics card unless --gpu), pointer and keyboard locks
// stubbed, closed by its own PID. Waits for the page to settle, then saves
// screenshots; `--eval` runs page code first (e.g. pick a camera shot).
//
//   node tools/nature/ref-shots.mjs --url https://gelatin.games/ultrawater --out <dir>
//        [--size 1600x900] [--wait-ms 20000] [--shots 3] [--every-ms 4000]
//        [--eval "<js>"] [--name ref] [--timeout-s 240] [--gpu]
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const url = arg("url", "https://gelatin.games/ultrawater");
const out = arg("out", ".evani/shots/ref");
const [W, H] = arg("size", "1600x900").split("x").map(Number);
const name = arg("name", "ref");
mkdirSync(out, { recursive: true });
const flags = process.argv.includes("--gpu") ? [] : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader", "--use-angle=swiftshader"];
const { cdp, stop } = await startChrome({ width: W, height: H, flags, prefix: "evani-refshots-" });
const limit = Number(arg("timeout-s", "240")) * 1000;
const report = { url, shots: [], console: [] };
const finish = async (c) => {
  writeFileSync(join(out, name + "-report.json"), JSON.stringify(report, null, 1));
  await stop().catch(() => {});
  process.exit(c);
};
setTimeout(() => {
  report.error = "time limit";
  finish(3);
}, limit).unref();
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p, ms = 60000) => cdp.send(m, p, s, ms);
  cdp.handlers.push((msg) => {
    if (msg.method === "Runtime.consoleAPICalled" && report.console.length < 40) report.console.push(msg.params.args.map((a) => a.value ?? a.description).join(" ").slice(0, 300));
  });
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
  const t0 = Date.now();
  await send("Page.navigate", { url });
  await sleep(Number(arg("wait-ms", "20000")));
  const ev = arg("eval", "");
  if (ev) {
    const r = await send("Runtime.evaluate", { expression: ev, awaitPromise: true, returnByValue: true }, limit);
    report.eval = r.result?.value ?? r.exceptionDetails?.text;
    await sleep(3000);
  }
  const n = Number(arg("shots", "3"));
  for (let i = 0; i < n; i++) {
    const shot = await send("Page.captureScreenshot", { format: "png" }, 120000);
    const f = `${name}-${i}.png`;
    writeFileSync(join(out, f), Buffer.from(shot.data, "base64"));
    report.shots.push({ file: f, at_ms: Date.now() - t0 });
    await sleep(Number(arg("every-ms", "4000")));
  }
  const info = await send("Runtime.evaluate", { expression: "JSON.stringify({title: document.title, buttons: [...document.querySelectorAll('button,select,option')].slice(0,60).map(b => (b.textContent||'').trim().slice(0,40))})", returnByValue: true });
  report.page = info.result?.value;
  await finish(0);
} catch (e) {
  report.error = String(e && e.stack || e);
  await finish(1);
}
