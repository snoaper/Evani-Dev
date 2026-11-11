// A contact sheet of grass proof pictures: a grid of captioned stills as
// one PNG or JPEG (the motion and trample sequences for the visual review,
// and phone-sized JPEGs). Headless, muted, closed by its own PID.
//
//   node tools/grass/sheet.mjs --out <file.png|file.jpg> --cols 3 --width 640
//        <picture>=<caption> [<picture>=<caption> ...] [--quality 84] [--title "..."]
import { readFileSync, writeFileSync } from "node:fs";
import { startChrome } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const out = arg("out", "");
const cols = Number(arg("cols", "3"));
const W = Number(arg("width", "640"));
const title = arg("title", "");
const flags = new Set(["--out", "--cols", "--width", "--quality", "--title"]);
const items = [];
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (flags.has(a)) {
    i++;
    continue;
  }
  const eq = a.indexOf("=");
  items.push(eq > 0 ? [a.slice(0, eq), a.slice(eq + 1)] : [a, ""]);
}
if (!out || !items.length) {
  console.error("--out and at least one picture are needed");
  process.exit(2);
}
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const cells = items
  .map(([f, cap]) => `<figure><img src="data:image/png;base64,${readFileSync(f).toString("base64")}">${cap ? `<figcaption>${esc(cap)}</figcaption>` : ""}</figure>`)
  .join("");
const width = cols * W + (cols + 1) * 4;
const html = `<!doctype html><meta charset="utf-8"><style>
body { margin: 0; background: #14161c; color: #fff; font: 20px sans-serif; width: ${width}px; }
h1 { font-size: 22px; margin: 8px 10px 4px; font-weight: 600; }
.g { display: grid; grid-template-columns: repeat(${cols}, ${W}px); gap: 4px; padding: 4px; }
figure { margin: 0; position: relative; }
img { width: ${W}px; display: block; }
figcaption { position: absolute; left: 8px; top: 6px; background: #000a; padding: 3px 9px; border-radius: 6px; }
</style>${title ? `<h1>${esc(title)}</h1>` : ""}<div class="g">${cells}</div>`;
const { cdp, stop } = await startChrome({ width, height: 1000, flags: ["--disable-gpu"], prefix: "evani-grass-sheet-" });
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const s = (m, p = {}, t) => cdp.send(m, p, sessionId, t);
  await s("Page.enable");
  const frameId = (await s("Page.getFrameTree")).frameTree.frame.id;
  await s("Page.setDocumentContent", { frameId, html });
  await s("Runtime.evaluate", { expression: "Promise.all([...document.images].map((i) => i.decode()))", awaitPromise: true }, 120000);
  const lm = (await s("Page.getLayoutMetrics")).cssContentSize;
  const bodyH = (await s("Runtime.evaluate", { expression: "Math.ceil(document.body.getBoundingClientRect().height)", returnByValue: true })).result.value;
  const c = { width: lm.width, height: Math.min(lm.height, bodyH || lm.height) };
  const jpeg = /\.jpe?g$/i.test(out);
  const r = await s("Page.captureScreenshot", { format: jpeg ? "jpeg" : "png", ...(jpeg ? { quality: Number(arg("quality", "84")) } : {}), captureBeyondViewport: true, clip: { x: 0, y: 0, width: c.width, height: c.height, scale: 1 } }, 120000);
  const buf = Buffer.from(r.data, "base64");
  writeFileSync(out, buf);
  console.log(`${out}: ${(buf.length / 1048576).toFixed(2)} MB, ${Math.round(c.width)} x ${Math.round(c.height)}`);
} finally {
  await stop();
}
