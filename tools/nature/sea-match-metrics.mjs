// One shared measure for both seas (ours and Ultra-Water) from the same
// captures, so "how many crests stand over the horizon" and "how much of the
// sea is whitecap" mean the same thing for both. Pixels only, no engine data.
//
//   node tools/nature/sea-match-metrics.mjs --views <views.json>
//        --wavy <rig work dir> --flat <rig work dir of the flat run>
//        [--levels 2,3] [--moments 8] [--out <report.json>] [--only storm,rough]
//
// The rig (tools/nature/compare.mjs, branch ocean-compare) writes the
// captures: <work>/shots-uw/<view>-m<k>.png and
// <work>/shots-ours/<view>-m<k>-<level>.png. The flat run is the same rig
// with both seas at zero wave height (same cameras, sun, sky and moments).
//
// Method (the round-6 research report, section M):
//  * Values are linear: each PNG's sRGB is undone first (thresholds then mean
//    the same light in both, whatever the tone curve).
//  * Horizon row r0, per engine and view, from its FLAT capture only: the
//    row where the median (over the columns) linear luminance changes most
//    steeply from one row to the next, searched within 8 px of the
//    camera's geometric horizon (cloud edges higher up can be steeper).
//    Never from the wavy picture. The camera doesn't move, so neither does the
//    horizon: the median of that row over the view's moments is used for all
//    of them (one moment's cloud edge read as its horizon put it 40 px out
//    and showed every column as a crest). A view whose horizon is out of the
//    frame (the aerial) has no crest measure.
//  * Exposure: Ultra-Water adapts its exposure to each picture (then tone
//    maps), so its sky is a little brighter or darker over a flat sea than
//    over waves. Per channel, a straight line taking the flat capture's sky
//    to the wavy capture's is fitted (least squares) in a band well above
//    any crest (from 5 px down to 200 px over the horizon); the wavy pixel is
//    compared with the flat one taken through that line.
//  * Water against sky, per column: above r0 the flat capture shows only
//    sky, and the same sky as the wavy capture (same moment, same clock), so
//    a wavy pixel is water (or spray) where it differs from the flat one at
//    the same pixel (after the exposure match below) the way water differs
//    from sky: darker by over max(0.04, 10 %), more saturated by over 0.12,
//    or brighter by over max(0.08, 25 %) (foam, spray), for 3 rows running;
//    the topmost such row is the sea's top there.
//  * A moment counts only when the wavy and flat captures were taken at the
//    same sim time (the rig moves a moment on when the camera sits inside a
//    wave; the flat capture is never moved, and Ultra-Water's clouds move
//    with time): times from the rig's <view>.json records.
//  * A moment with the camera under the water's surface doesn't count, for
//    either engine (the rig moves Ultra-Water's moment on, above; ours is
//    read from the dumped frame: the field sea's eye-under bit in the level
//    word, <work>/dump-ours/scenes.json). Nor does a broken frame, for either
//    engine: one where a quarter or more of the columns are one flat tone all
//    down the picture's bottom fifth (a camera just under Ultra-Water's
//    surface that the rig's check missed draws a band of sea hanging in a
//    plain void; a mesh that doesn't reach the eye does too).
//  * Crest share: the share of columns whose top stands over r0 by more than
//    0.5 % of the image height. Crest height: the 50th and 95th percentiles
//    of (r0 - top) over all columns, as a share of the image height.
//  * Whitecap share: pixels of the water region (from each column's top down)
//    that are bright and white: linear max channel over 0.8, saturation
//    under 0.25, outside a disc of 8 % of the image height round the sun's
//    mirror point (its glint), as a share of the water pixels outside it.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { readPng } from "./png-small.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined ? process.argv[i + 1] : d;
};
const cfg = JSON.parse(readFileSync(arg("views"), "utf8"));
const wavy = arg("wavy"), flat = arg("flat");
const levels = arg("levels", "2,3").split(",").map(Number);
const nm = Number(arg("moments", String(cfg.moments.length)));
const only = arg("only", "");
const [W, H] = cfg.size;
const rad = (d) => (d * Math.PI) / 180;
const lin = (v) => {
  const c = v / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
};
const LUT = Float32Array.from({ length: 256 }, (_, i) => lin(i));
function load(file) {
  const { width, height, data } = readPng(readFileSync(file));
  if (width !== W || height !== H) throw new Error(`${file}: ${width}x${height}, the views are ${W}x${H}`);
  const px = new Float32Array(W * H * 3);
  for (let i = 0; i < W * H; i++) for (let c = 0; c < 3; c++) px[i * 3 + c] = LUT[data[i * 4 + c]];
  return px;
}
const lum = (p, i) => 0.2126 * p[i * 3] + 0.7152 * p[i * 3 + 1] + 0.0722 * p[i * 3 + 2];
// The horizon row of a flat capture.
function horizonRow(f, hy) {
  const med = new Float32Array(H);
  const col = new Float32Array(W);
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) col[x] = lum(f, y * W + x);
    med[y] = Float32Array.from(col).sort()[W >> 1];
  }
  let best = 1, bd = -1;
  for (let y = Math.max(2, Math.round(hy) - 8); y < Math.min(H - 2, Math.round(hy) + 8); y++) {
    const d = Math.abs(med[y + 1] - med[y]);
    if (d > bd) {
      bd = d;
      best = y + 1;
    }
  }
  return best;
}
// The sun's mirror point on screen (or null when it is out of view).
function sunMirror(v) {
  const p = rad(v.camera.pitch_deg), y = rad(v.camera.yaw_deg), half = Math.tan(rad(v.camera.fov_deg) / 2);
  const f = [Math.cos(y) * Math.cos(p), Math.sin(p), Math.sin(y) * Math.cos(p)];
  const r = [-Math.sin(y), 0, Math.cos(y)];
  const u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
  const se = rad(v.sun.elevation_deg), sa = rad(v.sun.azimuth_deg);
  const d = [Math.cos(se) * Math.cos(sa), -Math.sin(se), Math.cos(se) * Math.sin(sa)];
  const z = d[0] * f[0] + d[1] * f[1] + d[2] * f[2];
  if (z <= 0) return null;
  const nx = (d[0] * r[0] + d[1] * r[1] + d[2] * r[2]) / z / (half * (W / H));
  const ny = (d[0] * u[0] + d[1] * u[1] + d[2] * u[2]) / z / half;
  return [((nx + 1) / 2) * W, ((1 - ny) / 2) * H];
}
const pct = (a, q) => {
  const s = Float64Array.from(a).sort();
  return s.length ? s[Math.min(s.length - 1, Math.floor(q * s.length))] : 0;
};
// The camera's geometric horizon row (a flat sea seen from the eye).
const horizonY = (v) => ((1 - -Math.tan(rad(v.camera.pitch_deg)) / Math.tan(rad(v.camera.fov_deg) / 2)) / 2) * H;
function measure(wf, ff, v, r0, hasHorizon) {
  const w = load(wf), f = load(ff);
  // Columns that hold one flat tone all down the bottom fifth (a void; even a
  // smooth wall of water close to the camera shades downwards).
  let flatCols = 0;
  for (let x = 0; x < W; x++) {
    let lo = 1e9, hi = -1e9;
    for (let y = Math.floor(H * 0.8); y < H; y++) {
      const l = lum(w, y * W + x);
      lo = Math.min(lo, l);
      hi = Math.max(hi, l);
    }
    if (hi - lo < 0.01) flatCols++;
  }
  const voidShare = flatCols / W;
  // Exposure match from the sky band: per channel, f' = a f + b.
  if (hasHorizon && r0 > 205) {
    for (let c = 0; c < 3; c++) {
      let n = 0, sx = 0, sy = 0, sxx = 0, sxy = 0;
      for (let y = 5; y < r0 - 200; y += 2) for (let x = 0; x < W; x += 3) {
        const i = (y * W + x) * 3 + c;
        n++;
        sx += f[i];
        sy += w[i];
        sxx += f[i] * f[i];
        sxy += f[i] * w[i];
      }
      const d = n * sxx - sx * sx;
      const a = d > 1e-9 ? (n * sxy - sx * sy) / d : 1, b = (sy - a * sx) / Math.max(n, 1);
      for (let i = c; i < f.length; i += 3) f[i] = a * f[i] + b;
    }
  }
  const sat = (p, i) => {
    const mx = Math.max(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]), mn = Math.min(p[i * 3], p[i * 3 + 1], p[i * 3 + 2]);
    return mx > 0 ? (mx - mn) / mx : 0;
  };
  const differs = (i) => {
    const lf = lum(f, i), lw = lum(w, i);
    return lf - lw > Math.max(0.04, 0.1 * lf) || sat(w, i) - sat(f, i) > 0.12 || lw - lf > Math.max(0.08, 0.25 * lf);
  };
  const tops = new Int32Array(W);
  const heights = [];
  let over = 0;
  for (let x = 0; x < W; x++) {
    let top = r0;
    for (let y = 0; hasHorizon && y < r0; y++) {
      if (differs(y * W + x) && differs((y + 1) * W + x) && differs((y + 2) * W + x)) {
        top = y;
        break;
      }
    }
    tops[x] = top;
    const h = Math.max(0, r0 - top) / H;
    heights.push(h);
    if (h > 0.005) over++;
  }
  const sun = sunMirror(v);
  const disc = 0.08 * H;
  let water = 0, white = 0;
  for (let x = 0; x < W; x++) {
    for (let y = tops[x]; y < H; y++) {
      if (sun && Math.hypot(x - sun[0], y - sun[1]) < disc) continue;
      const i = y * W + x;
      water++;
      const mx = Math.max(w[i * 3], w[i * 3 + 1], w[i * 3 + 2]), mn = Math.min(w[i * 3], w[i * 3 + 1], w[i * 3 + 2]);
      if (mx > 0.8 && (mx - mn) / mx < 0.25) white++;
    }
  }
  return { r0, voidShare, crest_share_pct: hasHorizon ? (100 * over) / W : null, crest_p50: hasHorizon ? pct(heights, 0.5) : null, crest_p95: hasHorizon ? pct(heights, 0.95) : null, whitecap_pct: (100 * white) / Math.max(1, water) };
}
// From this share of flat columns in the bottom fifth on, a frame is broken.
const BROKEN = Number(arg("broken", "0.25"));
const mean = (a, k) => (a[0][k] === null ? NaN : a.reduce((s, x) => s + x[k], 0) / a.length);
const out = { method: "tools/nature/sea-match-metrics.mjs (flat-capture horizon, linear values, sun-glint disc excluded)", views: {} };
for (const v of cfg.views.filter((v) => !only || only.split(",").includes(v.id))) {
  if (v.shore) continue;
  const res = {};
  const engines = [["Ultra-Water", (k) => join(wavy, "shots-uw", `${v.id}-m${k}.png`), (k) => join(flat, "shots-uw", `${v.id}-m${k}.png`)]];
  for (const L of levels) engines.push([L === 3 ? "Ours (Ultra)" : L === 2 ? "Ours (High)" : `Ours (level ${L})`, (k) => join(wavy, "shots-ours", `${v.id}-m${k}-${L}.png`), (k) => join(flat, "shots-ours", `${v.id}-m${k}-${L}.png`)]);
  // The rig's capture times (Ultra-Water), per moment, wavy and flat.
  const times = (dir) => {
    const f = join(dir, "shots-uw", `${v.id}.json`);
    return existsSync(f) ? JSON.parse(readFileSync(f, "utf8")).rec.moments.map((m) => m.time) : [];
  };
  const tw = times(wavy), tf = times(flat);
  // Ours: moments whose frame has the camera under the water (bit 21 of the
  // storm code, src/sea/field.rs UNDER_BIT), per level.
  const dumpF = join(wavy, "dump-ours", "scenes.json");
  const shots = existsSync(dumpF) ? JSON.parse(readFileSync(dumpF, "utf8")).shots : [];
  const under = (k, L) => {
    const sh = shots.find((s) => s.name === `${v.id}-m${k}`);
    return !!sh && ((Math.round(sh.levels[L][27]) >>> 2) & (1 << 21)) !== 0;
  };
  for (const [name, wf, ff] of engines) {
    const hy = horizonY(v);
    const hasHorizon = hy > 60 && hy < H - 20;
    const rows = [];
    for (let k = 0; k < nm && hasHorizon; k++) if (existsSync(ff(k))) rows.push(horizonRow(load(ff(k)), hy));
    const r0 = hasHorizon && rows.length ? pct(rows, 0.5) : 0;
    const per = [];
    let broken = 0;
    for (let k = 0; k < nm; k++) {
      if (name === "Ultra-Water" && tw[k] !== undefined && tf[k] !== undefined && Math.abs(tw[k] - tf[k]) > 1e-6) continue;
      const L = name === "Ours (Ultra)" ? 3 : name === "Ours (High)" ? 2 : -1;
      if (L >= 0 && under(k, L)) continue;
      if (existsSync(wf(k)) && existsSync(ff(k))) {
        const m = measure(wf(k), ff(k), v, r0, hasHorizon);
        m.k = k;
        if (m.voidShare >= BROKEN) broken++;
        else per.push(m);
      }
    }
    if (!per.length) continue;
    // (The crest heights' mean over moments is carried by the odd moment with a
    // crest right in front of the eye filling half the frame; the median over
    // moments is reported beside it.)
    const med = (k) => { const xs = per.map((m) => m[k] ?? 0).sort((x, y) => x - y); const n = xs.length; return n % 2 ? xs[(n - 1) / 2] : 0.5 * (xs[n / 2 - 1] + xs[n / 2]); };
    res[name] = { moments: per.length, r0: per[0].r0, crest_share_pct: +mean(per, "crest_share_pct").toFixed(1), crest_p50_pct_h: +(100 * mean(per, "crest_p50")).toFixed(2), crest_p95_pct_h: +(100 * mean(per, "crest_p95")).toFixed(2), crest_p50_med_pct_h: +(100 * med("crest_p50")).toFixed(2), crest_p95_med_pct_h: +(100 * med("crest_p95")).toFixed(2), whitecap_pct: +mean(per, "whitecap_pct").toFixed(2), broken, per };
  }
  out.views[v.id] = res;
  for (const [name, r] of Object.entries(res)) console.log(`${v.id.padEnd(11)} ${name.padEnd(13)} r0 ${String(r.r0).padStart(4)} | crests ${String(r.crest_share_pct).padStart(5)} % | height p50 ${r.crest_p50_pct_h} % p95 ${r.crest_p95_pct_h} % of H (medians over moments ${r.crest_p50_med_pct_h} / ${r.crest_p95_med_pct_h}) | whitecap ${r.whitecap_pct} % (${r.moments} moments${r.broken ? `, ${r.broken} broken frames left out` : ""})`);
}
if (arg("out")) writeFileSync(arg("out"), JSON.stringify(out, null, 1));
