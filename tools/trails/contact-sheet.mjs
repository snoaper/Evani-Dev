// A before-and-after contact sheet of the ribbons proof, as one JPEG small
// enough for a phone: each row is one view, today's dense particles on the left
// and the `trails` effect on the right. Headless, muted, closed by its own PID.
//
//   node tools/trails/contact-sheet.mjs --dir <folder of trail-shots pictures>
//        --out <file.jpg> [--views kart-chase,kart-high] [--width 900] [--quality 82]
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const dir = arg("dir", "");
const out = arg("out", "");
if (!dir || !out) {
  console.error("--dir and --out are needed");
  process.exit(2);
}
const views = arg("views", "kart-chase,kart-high,kart-side,kart-front,sword-front,sword-side,sword-above,sword-close").split(",");
const W = Number(arg("width", "900"));
const data = (f) => `data:image/png;base64,${readFileSync(join(dir, f)).toString("base64")}`;
const rows = views
  .map(
    (v) => `<div class="r"><figure><img src="${data(`${v}-dense.png`)}"><figcaption>Today: dense particles</figcaption></figure><figure><img src="${data(`${v}-strip.png`)}"><figcaption>New: ribbons and trails</figcaption></figure></div>`,
  )
  .join("");
const html = `<!doctype html><meta charset="utf-8"><style>
body { margin: 0; background: #14161c; color: #fff; font: 22px sans-serif; width: ${2 * W + 12}px; }
.r { display: flex; gap: 4px; margin: 4px; }
figure { margin: 0; position: relative; width: ${W}px; }
img { width: ${W}px; display: block; }
figcaption { position: absolute; left: 10px; top: 8px; background: #0009; padding: 4px 10px; border-radius: 6px; }
</style>${rows}`;
const { cdp, stop } = await startChrome({ width: 2 * W + 12, height: 1000, flags: ["--disable-gpu"], prefix: "evani-sheet-" });
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const s = (m, p = {}, t) => cdp.send(m, p, sessionId, t);
  await s("Page.enable");
  const { frameId } = await s("Page.getFrameTree").then((r) => ({ frameId: r.frameTree.frame.id }));
  await s("Page.setDocumentContent", { frameId, html });
  await s("Runtime.evaluate", { expression: "Promise.all([...document.images].map((i) => i.decode()))", awaitPromise: true }, 120000);
  const m = await s("Page.getLayoutMetrics");
  const c = m.cssContentSize;
  const r = await s("Page.captureScreenshot", { format: "jpeg", quality: Number(arg("quality", "82")), captureBeyondViewport: true, clip: { x: 0, y: 0, width: c.width, height: c.height, scale: 1 } }, 120000);
  const buf = Buffer.from(r.data, "base64");
  writeFileSync(out, buf);
  console.log(`${out}: ${(buf.length / 1048576).toFixed(2)} MB, ${c.width} x ${c.height}`);
} finally {
  await stop();
}
