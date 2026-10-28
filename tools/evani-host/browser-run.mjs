// Browser proof of the general web mode: evani-host (server.mjs) runs the
// referee; two player pages and a spectator page, each its own tab, are
// given the server address in their URL (`?evani-server=ws://...`) and talk
// JSON over WebSocket, every message under 4 KB. A fourth page gets an
// address nobody listens on and must stay the single-player game. Headless,
// muted Chrome on a throwaway profile; synthetic keys only.
//
//   node browser-run.mjs <referee.wasm> <game folder> <shots folder> [seconds] [port]

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const [refPath, gameDir, shotsDir, secsArg, portArg] = process.argv.slice(2);
const SECS = Number(secsArg || 40);
const PORT = Number(portArg || 8111);
mkdirSync(shotsDir, { recursive: true });
const server = spawn(process.execPath, [join(here, "server.mjs"), "--referee", resolve(refPath), "--port", String(PORT), "--static", resolve(gameDir), "--laps", "1", "--seed", "77"], { stdio: ["ignore", "pipe", "inherit"], windowsHide: true });
await new Promise((r) => server.stdout.once("data", r));
let chrome = null;
const report = { ok: false };
try {
  chrome = await startChrome({ width: 960, height: 540, prefix: "evani-host-run-" });
  const { cdp } = chrome;
  const base = `http://127.0.0.1:${PORT}/index.html`;
  const ws = `ws://127.0.0.1:${PORT}/`;
  const pages = [
    { name: "player1", url: `${base}?evani-server=${encodeURIComponent(ws)}` },
    { name: "player2", url: `${base}?evani-server=${encodeURIComponent(ws)}` },
    { name: "spectator", url: `${base}?evani-server=${encodeURIComponent(ws)}&evani-role=spectator` },
    { name: "no-server", url: `${base}?evani-server=${encodeURIComponent("ws://127.0.0.1:1/")}` },
  ];
  for (const p of pages) {
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    p.s = (m, a) => cdp.send(m, a, sessionId);
    await p.s("Page.enable");
    await p.s("Runtime.enable");
    await p.s("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
    // Background tabs would be throttled: every page renders as if focused.
    await p.s("Emulation.setFocusEmulationEnabled", { enabled: true });
    await p.s("Page.navigate", { url: p.url });
    p.eval = async (e) => (await p.s("Runtime.evaluate", { expression: e, returnByValue: true })).result.value;
  }
  await sleep(6000);
  const key = (type, code) => `document.dispatchEvent(new KeyboardEvent('${type}', { code: '${code}', key: '${code}', bubbles: true }))`;
  for (const p of pages.slice(0, 2)) await p.eval(key("keydown", "KeyW"));
  const t0 = Date.now();
  while ((Date.now() - t0) / 1000 < SECS) {
    const t = (Date.now() - t0) / 1000;
    for (const [k, p] of pages.slice(0, 2).entries()) {
      const phase = Math.floor(t / 2 + k) % 4;
      await p.eval(key(phase === 1 ? "keydown" : "keyup", "KeyA"));
      await p.eval(key(phase === 3 ? "keydown" : "keyup", "KeyD"));
    }
    await sleep(400);
  }
  report.pages = {};
  for (const p of pages) {
    report.pages[p.name] = {
      online: await p.eval("document.documentElement.getAttribute('data-online')"),
      title: await p.eval("(() => { const t = document.getElementById('title'); return t ? getComputedStyle(t).display !== 'none' : null; })()"),
    };
    const png = await p.s("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(shotsDir, `web-${p.name}.png`), Buffer.from(png.data, "base64"));
  }
  report.server = await (await fetch(`http://127.0.0.1:${PORT}/stats`)).json();
  report.ok = true;
} catch (e) {
  report.error = String(e && e.stack ? e.stack : e);
} finally {
  if (chrome) await chrome.stop();
  server.kill();
}
writeFileSync(join(shotsDir, "web-report.json"), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
process.exit(report.ok ? 0 : 1);
