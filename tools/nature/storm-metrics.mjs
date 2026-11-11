// Storm parity metrics: the same measurements on a reference picture (the
// Ultra-Water demo) and on ours, from the pictures alone, so a pair is
// judged like for like. Node built-ins only.
//
//   node tools/nature/storm-metrics.mjs --out <report.json> \
//        --img <png> --pitch <deg down> --fov <vertical deg> [--mask x0,y0,x1,y1] [--name <label>] \
//        [--img ...]   (one --img group per picture)
//
// Per picture:
//   * crest: how far the waves rise above the horizon line, per column (px):
//     the horizon row comes from the camera (pitch and field of view); the
//     sky's colour per column is sampled well above it, and the first row,
//     scanning down, that differs from it for 4 rows running is where the
//     sea starts. Reports the max, 95th percentile, median and the share of
//     columns standing over 4 px.
//   * white water: the share of sea pixels that are white (luminance over
//     0.72, saturation under 0.18): foam, spray, glints.
//   * steepness: the luminance gradient in the sea below the horizon band
//     (a busy, steep sea has strong light-dark changes): mean, 90th
//     percentile and a histogram; plus the share of dark trough pixels
//     (luminance under 0.12) and the sea's luminance spread.
//   * turquoise: the share of sea pixels that are turquoise (hue 160-200
//     deg, saturation over 0.3): all of them (luminance over 0.1, the dark
//     teal body too) and the bright ones (over 0.22: light through the
//     crests' water).
//   * ribbing: in the bottom 300 rows (the near water), each column's
//     luminance as a signal, its power spectrum over periods of 4 to 100
//     px averaged over the columns; the strongest period's power over the
//     median power. A corrugated sheet (one ripple period running across
//     the near water) stands out as a high peak; a busy criss-cross chop
//     has a flat spectrum (the reference's ratio is the yardstick).
// Writes <png>.metrics.png beside each picture: the horizon (yellow), the
// sea's top edge (red) and the white water (magenta), to check by eye.
import { readFileSync, writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";
import { readPng } from "./png-small.mjs";

const argv = process.argv.slice(2);
const out = argv[argv.indexOf("--out") + 1];
const groups = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--img") groups.push({ img: argv[++i], pitch: 0, fov: 50, mask: [], name: "" });
  else if (argv[i] === "--pitch") groups[groups.length - 1].pitch = Number(argv[++i]);
  else if (argv[i] === "--fov") groups[groups.length - 1].fov = Number(argv[++i]);
  else if (argv[i] === "--mask") groups[groups.length - 1].mask.push(argv[++i].split(",").map(Number));
  else if (argv[i] === "--name") groups[groups.length - 1].name = argv[++i];
  // --foam <foamonly.png>,<nofoam.png> (from sea-shots.mjs --foam): the foam
  // alone, measured as what it adds to the picture.
  else if (argv[i] === "--foam") groups[groups.length - 1].foam = argv[++i];
}

const CRC = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
function crc(buf) {
  let c = -1;
  for (const b of buf) c = CRC[(c ^ b) & 255] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function writePng(path, w, h, rgba) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, rgba.byteOffset + y * w * 4, w * 4).copy(raw, y * (w * 4 + 1) + 1);
  }
  const chunk = (kind, body) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(body.length);
    const kb = Buffer.concat([Buffer.from(kind, "latin1"), body]);
    const c = Buffer.alloc(4);
    c.writeUInt32BE(crc(kb));
    return Buffer.concat([len, kb, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  writeFileSync(path, Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]));
}

const pct = (arr, q) => {
  if (!arr.length) return 0;
  const s = [...arr].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(q * s.length))];
};

function measure(g) {
  const { width: W, height: H, data } = readPng(readFileSync(g.img));
  const at = (x, y) => [data[(y * W + x) * 4] / 255, data[(y * W + x) * 4 + 1] / 255, data[(y * W + x) * 4 + 2] / 255];
  const lum = (c) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  const masked = (x, y) => g.mask.some(([x0, y0, x1, y1]) => x >= x0 && x <= x1 && y >= y0 && y <= y1);
  // The horizon: a direction level with the eye seen by a camera pitched
  // `pitch` deg down.
  const th = Math.tan((g.fov * Math.PI) / 360);
  const yh = Math.round(H / 2 - (Math.tan((g.pitch * Math.PI) / 180) / th) * (H / 2));
  const crest = [];
  const top = new Int32Array(W).fill(yh);
  // The sky's colour per column, as a straight line in the row from two
  // bands well above the horizon (65-95 and 125-155 px up, averaged over 9
  // columns), carried down to the horizon; the sea starts at the first row,
  // scanning down from 60 px up, that differs from it by over 0.09 for 4
  // rows running.
  const band = (x, y0, y1) => {
    let s = [0, 0, 0], n = 0;
    for (let xx = Math.max(0, x - 4); xx <= Math.min(W - 1, x + 4); xx++) {
      for (let y = Math.max(0, y0); y < Math.max(1, y1); y++) {
        const c = at(xx, y);
        s = s.map((v, i) => v + c[i]);
        n++;
      }
    }
    return s.map((v) => v / Math.max(n, 1));
  };
  // The sea's top edge per column is a sharp silhouette against a sky that
  // changes slowly from row to row (haze bands and cloud edges are soft):
  // scanning down from 60 px above the horizon, the first row whose colour
  // jumps by over 0.05 from the row above (3-column average) and stays over
  // 0.06 away from that sky colour for the next 3 rows.
  void band;
  const col3 = (x, y) => {
    let s = [0, 0, 0];
    for (let xx = Math.max(0, x - 1); xx <= Math.min(W - 1, x + 1); xx++) {
      const c = at(xx, y);
      s = s.map((v, i) => v + c[i] / 3);
    }
    return s;
  };
  // A wave against the sky is darker than the sky above it or more
  // saturated (a turquoise lit crest); haze bands and cloud streaks near the
  // horizon are lighter and greyer. The sky's colour: 65-70 px above the
  // horizon (3 columns).
  const sat = (c) => {
    const mx = Math.max(...c), mn = Math.min(...c);
    return mx > 0 ? (mx - mn) / mx : 0;
  };
  for (let x = 0; x < W; x++) {
    if (masked(x, yh - 70) || masked(x, yh)) continue;
    let sky = [0, 0, 0];
    for (let y = yh - 70; y < yh - 64; y++) sky = sky.map((v, i) => v + col3(x, Math.max(0, y))[i] / 6);
    const seaLike = (y) => {
      const c = col3(x, y);
      return lum(sky) - lum(c) > 0.06 || sat(c) - sat(sky) > 0.12;
    };
    let edge = yh + 12;
    for (let y = Math.max(0, yh - 60); y <= Math.min(H - 4, yh + 12); y++) {
      if (!seaLike(y) || ![1, 2, 3].every((k) => seaLike(y + k))) continue;
      // It stands on the sea: sea-like down to the horizon (85 % of rows).
      let n = 0;
      for (let r = y; r < yh; r++) if (seaLike(r)) n++;
      if (y >= yh || n >= 0.85 * (yh - y)) {
        edge = y;
        break;
      }
    }
    top[x] = edge;
    crest.push(Math.max(0, yh - edge));
  }
  // The sea: below its top edge (and below the horizon band for the
  // gradient), outside the masks.
  let sea = 0, white = 0, dark = 0, turq = 0, turqb = 0;
  const grads = [];
  const lums = [];
  const hist = [0, 0, 0, 0, 0, 0, 0];
  const edges = [0.005, 0.01, 0.02, 0.04, 0.08, 0.16];
  const over = new Uint8Array(data);
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      if (y <= Math.max(top[x], 0) || masked(x, y)) continue;
      const c = at(x, y);
      const l = lum(c);
      const mx = Math.max(...c), mn = Math.min(...c);
      const sat = mx > 0 ? (mx - mn) / mx : 0;
      sea++;
      lums.push(l);
      if (l > 0.72 && sat < 0.18) {
        white++;
        const i = (y * W + x) * 4;
        over[i] = 255;
        over[i + 1] = 0;
        over[i + 2] = 255;
      }
      if (l < 0.12) dark++;
      if (sat > 0.3 && l > 0.1 && mx > 0) {
        // Hue (deg) of the pixel.
        const [r, gg, b] = c;
        let hue;
        if (mx === r) hue = 60 * (((gg - b) / (mx - mn)) % 6);
        else if (mx === gg) hue = 60 * ((b - r) / (mx - mn) + 2);
        else hue = 60 * ((r - gg) / (mx - mn) + 4);
        if (hue < 0) hue += 360;
        if (hue >= 160 && hue <= 200) {
          turq++;
          if (l > 0.22) turqb++;
        }
      }
      if (y > yh + 40 && x > 0 && x < W - 1 && y < H - 1) {
        const gx = (lum(at(x + 1, y)) - lum(at(x - 1, y))) * 0.5;
        const gy = (lum(at(x, y + 1)) - lum(at(x, y - 1))) * 0.5;
        const gm = Math.hypot(gx, gy);
        grads.push(gm);
        let b = 0;
        while (b < edges.length && gm >= edges[b]) b++;
        hist[b]++;
      }
    }
  }
  for (let x = 0; x < W; x++) {
    const put = (y, r, gg, b) => {
      if (y < 0 || y >= H) return;
      const i = (y * W + x) * 4;
      over[i] = r;
      over[i + 1] = gg;
      over[i + 2] = b;
    };
    put(yh, 255, 230, 0);
    put(top[x], 255, 30, 30);
    put(top[x] - 1, 255, 30, 30);
  }
  writePng(g.img.replace(/\.png$/i, "") + ".metrics.png", W, H, over);
  // The foam alone: a sea pixel is foam where the foam brightens it by over
  // 0.1 (foam drawn vs no foam, spindrift off in both), spray where the
  // spindrift brightens it by over 0.1 more; sun glints and sky reflections
  // are the same in each pair, so they never count. Writes <png>.foam.png:
  // the picture dimmed, foam white, spray orange.
  let foam = null;
  if (g.foam) {
    const [fo, nf] = g.foam.split(",").map((p) => readPng(readFileSync(p)).data);
    const lumAt = (d, i) => (0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]) / 255;
    const fov = new Uint8Array(data.length);
    let nfoam = 0, nspray = 0, n = 0;
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = (y * W + x) * 4;
        fov[i] = data[i] * 0.35;
        fov[i + 1] = data[i + 1] * 0.35;
        fov[i + 2] = data[i + 2] * 0.35;
        fov[i + 3] = 255;
        if (y <= Math.max(top[x], 0) || masked(x, y)) continue;
        n++;
        if (lumAt(fo, i) - lumAt(nf, i) > 0.1) {
          nfoam++;
          fov[i] = fov[i + 1] = fov[i + 2] = 255;
        } else if (lumAt(data, i) - lumAt(fo, i) > 0.1) {
          nspray++;
          fov[i] = 255;
          fov[i + 1] = 150;
          fov[i + 2] = 40;
        }
      }
    }
    writePng(g.img.replace(/\.png$/i, "") + ".foam.png", W, H, fov);
    foam = { foam_pct: +((100 * nfoam) / Math.max(n, 1)).toFixed(2), spray_pct: +((100 * nspray) / Math.max(n, 1)).toFixed(2) };
  }
  const mean =(a) => (a.length ? a.reduce((s, v) => s + v, 0) / a.length : 0);
  // Ribbing: the near water's dominant vertical period (see the header).
  const ribbing = (() => {
    const N = 300, y0 = H - N;
    if (y0 <= yh) return null;
    const periods = [];
    for (let p = 4; p <= 100; p *= 1.06) periods.push(p);
    const pow = new Float64Array(periods.length);
    let cols = 0;
    for (let x = 0; x < W; x += 4) {
      const s = new Float64Array(N);
      let m = 0;
      for (let y = 0; y < N; y++) m += (s[y] = lum(at(x, y0 + y)));
      m /= N;
      // Detrend (mean and linear slope), Hann window.
      let sl = 0, den = 0;
      for (let y = 0; y < N; y++) { sl += (y - N / 2) * (s[y] - m); den += (y - N / 2) * (y - N / 2); }
      sl /= den;
      for (let y = 0; y < N; y++) s[y] = (s[y] - m - sl * (y - N / 2)) * (0.5 - 0.5 * Math.cos((2 * Math.PI * y) / (N - 1)));
      periods.forEach((p, k) => {
        let re = 0, im = 0;
        const w = (2 * Math.PI) / p;
        for (let y = 0; y < N; y++) { re += s[y] * Math.cos(w * y); im += s[y] * Math.sin(w * y); }
        // Power per log-period band (a flat chop's spectrum falls with
        // frequency; weigh by the period so 1/f noise reads flat).
        pow[k] += (re * re + im * im) / p;
      });
      cols++;
    }
    const arr = Array.from(pow, (v) => v / cols);
    const sorted = [...arr].sort((a, b) => a - b);
    const med = sorted[Math.floor(sorted.length / 2)] || 1e-12;
    let best = 0;
    arr.forEach((v, k) => { if (v > arr[best]) best = k; });
    return { peak_over_median: +(arr[best] / med).toFixed(2), period_px: +periods[best].toFixed(1) };
  })();
  const lm = mean(lums);
  const sd = Math.sqrt(mean(lums.map((v) => (v - lm) * (v - lm))));
  const gsum = grads.length || 1;
  return {
    name: g.name || g.img,
    img: g.img,
    size: [W, H],
    horizon_y: yh,
    crest_px: { max: Math.max(0, ...crest), p95: pct(crest, 0.95), p50: pct(crest, 0.5), over4_pct: +(100 * crest.filter((v) => v > 4).length / Math.max(crest.length, 1)).toFixed(1) },
    // The same against the picture's own far-sea line (the 10th percentile
    // of the sea's top edge over the horizon: where no crest stands, the
    // edge is the far sea itself), so a far sea drawn above or below the
    // camera's horizon row doesn't count as crests or hide them.
    crest_far: (() => {
      const far = pct(crest, 0.1);
      const over = crest.map((v) => v - far);
      return { far_line_px: far, p95: pct(over, 0.95), over4_pct: +(100 * over.filter((v) => v > 4).length / Math.max(over.length, 1)).toFixed(1) };
    })(),
    white_water_pct: +((100 * white) / Math.max(sea, 1)).toFixed(2),
    ...(foam ? { foam } : {}),
    dark_trough_pct: +((100 * dark) / Math.max(sea, 1)).toFixed(2),
    turquoise_pct: +((100 * turq) / Math.max(sea, 1)).toFixed(2),
    turquoise_bright_pct: +((100 * turqb) / Math.max(sea, 1)).toFixed(2),
    ribbing,
    sea_lum: { mean: +lm.toFixed(3), sd: +sd.toFixed(3) },
    steepness: { mean_grad: +mean(grads).toFixed(4), p90_grad: +pct(grads, 0.9).toFixed(4), hist_pct: hist.map((v) => +((100 * v) / gsum).toFixed(1)), hist_edges: edges },
  };
}

const results = groups.map(measure);
writeFileSync(out, JSON.stringify(results, null, 1));
for (const r of results) console.log(`${r.name}: crest max ${r.crest_px.max} p95 ${r.crest_px.p95} px, >4px ${r.crest_px.over4_pct}% | white ${r.white_water_pct}%${r.foam ? ` (foam ${r.foam.foam_pct}%, spray ${r.foam.spray_pct}%)` : ""} dark ${r.dark_trough_pct}% turq ${r.turquoise_pct}% (bright ${r.turquoise_bright_pct}%) rib ${r.ribbing ? r.ribbing.peak_over_median + "@" + r.ribbing.period_px : "-"} | grad mean ${r.steepness.mean_grad} p90 ${r.steepness.p90_grad} | lum ${r.sea_lum.mean}±${r.sea_lum.sd}`);
