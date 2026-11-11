// Joins shots into one labelled sheet with a headless, muted Chrome (no npm
// packages: the browser lays out an HTML grid and screenshots it).
//
//   node tools/studio/shot-sheet.mjs --out sheet.png --cols 3 [--title "..."] [--width 1800] a.png b.png ...
//   node tools/studio/shot-sheet.mjs --out ba.png --pair <beforeDir> <afterDir> [--match "ultra__"] [--title "..."]
//   node tools/studio/shot-sheet.mjs --bare --cols 2 --width 1536 --out round3.png aerial.png inside.png
//
// --bare: the pictures only, side by side with a 4 px dark line between them: no title, captions or margins
// (an Asset Studio round of a multi-view asset: its panels in the reference's layout, docs/ASSET_STUDIO_GUIDE.md "Viewpoints").
//
// --pair puts each shot of <beforeDir> next to the same-named shot of
// <afterDir> (Before | After), one pair per row, two pairs per row with
// --cols 4. --crop x,y,w,h (fractions 0..1) crops every picture alike.
import { readFileSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import { join, basename, resolve } from "node:path";
import { startChrome, sleep } from "../bench-runner/chrome.mjs";

const argv = process.argv.slice(2);
const opt = (k, d) => {
  const i = argv.indexOf("--" + k);
  if (i < 0) return d;
  const v = argv[i + 1];
  argv.splice(i, 2);
  return v;
};
const bi = argv.indexOf("--bare"), bare = bi >= 0;
if (bare) argv.splice(bi, 1);
const out = opt("out", "sheet.png");
const title = opt("title", "");
const width = Number(opt("width", "1800"));
const match = opt("match", "");
const crop = (opt("crop", "") || "").split(",").filter(Boolean).map(Number);
let cols = Number(opt("cols", "0"));
const pi = argv.indexOf("--pair");
const cells = [];
if (pi >= 0) {
  const [a, b] = [argv[pi + 1], argv[pi + 2]];
  for (const f of readdirSync(a).filter((f) => f.endsWith(".png") && f.includes(match)).sort()) {
    if (!existsSync(join(b, f))) continue;
    const label = f.replace(/\.png$/, "").replace(/__/g, " · ");
    cells.push({ file: join(a, f), label: "BEFORE · " + label });
    cells.push({ file: join(b, f), label: "AFTER · " + label });
  }
  cols ||= 2;
} else {
  for (const f of argv) cells.push({ file: f, label: basename(f).replace(/\.png$/, "").replace(/__/g, " · ") });
  cols ||= 3;
}
if (!cells.length) throw new Error("no pictures");
const img = (f) => "file:///" + resolve(f).split("\\").join("/");
const gap = bare ? 4 : 8, pad = bare ? 0 : 8;
const cw = Math.floor((width - 2 * pad - (cols - 1) * gap) / cols);
const cropCss = crop.length === 4
  ? (f) => `object-fit:none;object-position:${-crop[0] * 100}% ${-crop[1] * 100}%;` // simple fallback
  : () => "";
const html = `<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0;background:#111;color:#eee;font:14px system-ui;width:${width}px">
${title && !bare ? `<div style="padding:10px 8px;font-size:20px;font-weight:600">${title}</div>` : ""}
<div style="display:grid;grid-template-columns:repeat(${cols},${cw}px);gap:${gap}px;padding:${pad}px">
${cells
  .map(
    (c) => `<figure style="margin:0"><div style="width:${cw}px;overflow:hidden;${crop.length === 4 ? `aspect-ratio:${crop[2]}/${crop[3]};position:relative` : ""}"><img src="${img(c.file)}" style="${crop.length === 4 ? `position:absolute;width:${100 / crop[2]}%;left:${(-crop[0] / crop[2]) * 100}%;top:${(-crop[1] / crop[3]) * 100}%` : "width:100%"};display:block"></div>${bare ? "" : `<figcaption style="padding:3px 2px;color:${c.label.startsWith("AFTER") ? "#9f9" : c.label.startsWith("BEFORE") ? "#f99" : "#ccc"}">${c.label}</figcaption>`}</figure>`,
  )
  .join("\n")}
</div></body></html>`;
void cropCss;
const tmp = join(process.env.TEMP || ".", `evani-sheet-${process.pid}.html`);
writeFileSync(tmp, html);
const { cdp, stop } = await startChrome({ width, height: 800, prefix: "evani-sheet-" });
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, 120000);
  await send("Page.enable");
  await send("Page.navigate", { url: "file:///" + tmp.replace(/\\/g, "/") });
  await sleep(1500);
  const r = await send("Runtime.evaluate", { expression: "document.body.scrollHeight", returnByValue: true });
  const h = r.result.value;
  await send("Emulation.setDeviceMetricsOverride", { width, height: h, deviceScaleFactor: 1, mobile: false });
  await sleep(500);
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width, height: h, scale: 1 } });
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log("sheet:", out, `${width}x${h}`, cells.length, "pictures");
} finally {
  await stop();
}
