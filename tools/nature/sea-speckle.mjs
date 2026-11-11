// tools/nature/sea-speckle.mjs <png...>: aliasing speckle on the water (dotted 'tread' rows). In the water band 2-30 % of the
// height below the horizon row (the row of the largest drop in mean brightness): the rms of the 1-pixel detail
// (linear luminance less its 3x3 mean) over the rms of the 4-to-8-pixel detail (5x5 box less 9x9 box mean).
// Aliasing piles energy at the pixel scale; real ripples spread it over scales.
import { readFileSync } from "node:fs";
import { readPng } from "./png-small.mjs";
const lin = (v) => { const c = v / 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
for (const f of process.argv.slice(2)) {
  const { width: W, height: H, data } = readPng(readFileSync(f));
  const Lv = new Float64Array(W * H);
  for (let i = 0; i < W * H; i++) Lv[i] = 0.2126 * lin(data[4 * i]) + 0.7152 * lin(data[4 * i + 1]) + 0.0722 * lin(data[4 * i + 2]);
  const I = new Float64Array((W + 1) * (H + 1));
  for (let y = 0; y < H; y++) { let s = 0; for (let x = 0; x < W; x++) { s += Lv[y * W + x]; I[(y + 1) * (W + 1) + x + 1] = I[y * (W + 1) + x + 1] + s; } }
  const box = (x, y, r) => { const xa = Math.max(x - r, 0), xb = Math.min(x + r + 1, W), ya = Math.max(y - r, 0), yb = Math.min(y + r + 1, H); return (I[yb * (W + 1) + xb] - I[ya * (W + 1) + xb] - I[yb * (W + 1) + xa] + I[ya * (W + 1) + xa]) / ((xb - xa) * (yb - ya)); };
  let best = 0, hy = Math.round(H / 2), prev = null;
  for (let y = Math.round(H * 0.2); y < H * 0.8; y++) { const s = box(W >> 1, y, W >> 1); if (prev !== null && prev - s > best) { best = prev - s; hy = y; } prev = s; }
  const y0 = hy + Math.round(0.02 * H), y1 = Math.min(hy + Math.round(0.3 * H), H - 6);
  let a = 0, b = 0, n = 0;
  for (let y = y0; y < y1; y += 2) for (let x = 6; x < W - 6; x += 2) { const p = Lv[y * W + x] - box(x, y, 1); const q = box(x, y, 2) - box(x, y, 4); a += p * p; b += q * q; n++; }
  console.log(f.split(/[\/]/).slice(-2).join("/").padEnd(36), "rows", y0, y1, "speckle", Math.sqrt(a / Math.max(b, 1e-12)).toFixed(3));
}
