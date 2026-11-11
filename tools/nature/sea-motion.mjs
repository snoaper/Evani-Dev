// Motion check for the sea: does it move smoothly or shimmer? From consecutive
// frames of the comparison rig (tools/nature/compare.mjs with a views file whose
// moments are 1/30 s apart), the same measure for both engines, pixels only.
//
//   node tools/nature/sea-motion.mjs --views <views.json> --work <rig work dir>
//        [--levels 2,3] [--out <report.json>] [--video <dir>]
//
// Per view and engine, in three bands of the water under the horizon (far: the
// first 60 rows, middle: the next 180, near: the rest), on linear luminance:
//  * motion: mean |L(t+1) - L(t)|, how much the picture changes per frame;
//  * flicker: mean |L(t+1) - 2 L(t) + L(t-1)|, how much that change itself jumps
//    from frame to frame;
//  * shimmer = flicker / motion. Smooth motion keeps it low; a pixel that flips
//    at random between frames (detail too fine for the pixels) tends to sqrt(3),
//    about 1.73. So motion says how alive the water is and shimmer how stable.
// --video also encodes each sequence to <dir>/<view>-<engine>.mp4 (ffmpeg).
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { readPng } from "./png-small.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const cfg = JSON.parse(readFileSync(arg("views"), "utf8"));
const work = arg("work");
const levels = arg("levels", "2,3").split(",").map(Number);
const nf = cfg.moments.length;
const srgb = (v) => {
  const c = v / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};
const LUT = Float64Array.from({ length: 256 }, (_, i) => srgb(i));

function luma(file) {
  const p = readPng(readFileSync(file));
  const out = new Float32Array(p.width * p.height);
  for (let i = 0; i < out.length; i++) out[i] = 0.2126 * LUT[p.data[i * 4]] + 0.7152 * LUT[p.data[i * 4 + 1]] + 0.0722 * LUT[p.data[i * 4 + 2]];
  return { w: p.width, h: p.height, l: out };
}

function measure(frames, r0) {
  const { w, h } = frames[0];
  const bands = { far: [r0 + 2, r0 + 60], middle: [r0 + 60, r0 + 240], near: [r0 + 240, h] };
  const res = {};
  for (const [name, [a0, b0]] of Object.entries(bands)) {
    const a = Math.max(a0, 0), b = Math.min(b0, h);
    if (b - a < 4) continue;
    let d1 = 0, d2 = 0, n1 = 0, n2 = 0;
    for (let t = 1; t < frames.length; t++) {
      const f1 = frames[t].l, f0 = frames[t - 1].l, fm = t >= 2 ? frames[t - 2].l : null;
      for (let y = a; y < b; y++) {
        for (let x = 0; x < w; x += 2) {
          const i = y * w + x;
          d1 += Math.abs(f1[i] - f0[i]);
          n1++;
          if (fm) {
            d2 += Math.abs(f1[i] - 2 * f0[i] + fm[i]);
            n2++;
          }
        }
      }
    }
    const motion = d1 / n1, flicker = d2 / Math.max(n2, 1);
    res[name] = { motion: +(1000 * motion).toFixed(2), flicker: +(1000 * flicker).toFixed(2), shimmer: +(flicker / Math.max(motion, 1e-6)).toFixed(3) };
  }
  return res;
}

const out = { method: "tools/nature/sea-motion.mjs: motion and flicker in 1/1000 of linear luminance; shimmer = flicker / motion (white flicker tends to 1.73)", views: {} };
const video = arg("video");
if (video) mkdirSync(video, { recursive: true });
for (const v of cfg.views) {
  const H = cfg.size[1];
  const pitch = (v.camera.pitch_deg * Math.PI) / 180, half = ((v.camera.fov_deg / 2) * Math.PI) / 180;
  const r0 = Math.round(H / 2 + (Math.tan(pitch) / Math.tan(half)) * (H / 2));
  const engines = [["ultra-water", (k) => join(work, "shots-uw", `${v.id}-m${k}.png`)]];
  for (const L of levels) engines.push([L === 3 ? "ours-ultra" : L === 2 ? "ours-high" : `ours-${L}`, (k) => join(work, "shots-ours", `${v.id}-m${k}-${L}.png`)]);
  out.views[v.id] = {};
  for (const [name, f] of engines) {
    const fr = [];
    for (let k = 0; k < nf && existsSync(f(k)); k++) fr.push(luma(f(k)));
    if (fr.length < 3) continue;
    const m = measure(fr, r0);
    out.views[v.id][name] = m;
    console.log(v.id.padEnd(12), name.padEnd(12), Object.entries(m).map(([b, x]) => `${b}: motion ${x.motion} shimmer ${x.shimmer}`).join(" | "));
    if (video) {
      const list = join(video, `${v.id}-${name}.txt`);
      writeFileSync(list, Array.from({ length: fr.length }, (_, k) => `file '${f(k).replace(/\\/g, "/")}'\nduration ${1 / 30}\n`).join(""));
      spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", list, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", join(video, `${v.id}-${name}.mp4`)], { stdio: "ignore", windowsHide: true });
    }
  }
}
if (arg("out")) writeFileSync(arg("out"), JSON.stringify(out, null, 1));
