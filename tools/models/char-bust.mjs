// Writes the material viewer's bust (apps/material-view, model id
// `char-head`): our own head and shoulders on a turned stone socle, whose
// materials carry the `evani_kind` tags (docs/MODEL_FORMAT.md, "Character
// materials"). Node built-ins only, no textures; the output is the same
// bytes on every run.
//
//   node tools/models/char-bust.mjs <out.glb>
//   cargo xtask convert <out.glb> --id char-head --tiers low,medium,high --out <dir>
//
// Critic round 3 found the round-2 head still crude: its surface was a
// distance field sampled along rays from the head's centre, which cannot
// draw anything the ray meets twice (under the nose tip, inside a nostril,
// the mouth's corners, the ear's hollow), so the nostrils came out as white
// tooth-like slivers and the mouth as a lopsided crease. Now the skin is the
// field's own surface, meshed with surface nets on a grid that is fine
// (0.75 mm) round the eyes, nose and mouth and coarse (3 mm) where nobody
// looks, every vertex pulled onto the true surface and lit with the field's
// gradient as its normal. The hair is a sculpted volume (thick on top,
// close at the sides, tapering to nothing at a soft hairline) with two thin
// layers of 96 locks lying on it, flowing out from a whorl at the crown; no
// standing-off shell, no flaring tips. The ruff is gone: the bust wears a
// plain crew-neck top over its shoulders and stands on a turned socle.
//
// Metres, the head's centre at the origin, facing +z, +y up:
// * skin (material `skin`, evani_kind skin, pore_scale 500): head, ears and
//   neck as one field; two eye sockets pressed into it; two lid shells with
//   an almond opening round the eyes. Equirect UVs from the head's centre:
//   u = longitude / 2 pi (seam at the back, where the vertices are split),
//   v = polar angle / 2 pi, so a UV unit is about the same length both ways.
// * eyes (r 0.012) behind the lids, looking along +z, azimuthal UVs: uv
//   (0.5, 0.5) is the pupil axis (material `eye`).
// * hair: two lock layers 1 and 2 mm over the hair volume, u = lock (a
//   whole step a lock, the layers half a lock apart), v from the whorl (0,
//   root) to the hairline (1, tip). Double-sided, material `hair`. Under
//   them the volume itself, opaque, material `hair_base` (untagged).
// * top: shoulders and chest in a crew-neck top with a rolled neckband, cut
//   flat under the chest (material `top`, untagged).
// * socle: a turned stone socle under the chest (material `stone`,
//   untagged); its foot is the model's lowest point, SOCLE_FOOT under the
//   head's centre.

import { writeFileSync } from 'node:fs';

const TAU = Math.PI * 2;
const norm = (a) => {
  const l = Math.hypot(...a) || 1;
  return a.map((v) => v / l);
};
const sub = (a, b) => a.map((v, i) => v - b[i]);
const add = (a, b) => a.map((v, i) => v + b[i]);
const scale = (a, k) => a.map((v) => v * k);
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a, b) => a.reduce((s, v, i) => s + v * b[i], 0);
const gauss = (x, s) => Math.exp(-(x / s) * (x / s));
const smooth = (a, b, x) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};
const dirOf = (t, p) => [Math.sin(t) * Math.sin(p), Math.cos(t), Math.sin(t) * Math.cos(p)];

// ---- field primitives (scalar arguments: the field runs millions of times) --
const ell = (x, y, z, cx, cy, cz, rx, ry, rz) => {
  const qx = (x - cx) / rx, qy = (y - cy) / ry, qz = (z - cz) / rz;
  return (Math.sqrt(qx * qx + qy * qy + qz * qz) - 1) * Math.min(rx, ry, rz);
};
const cap = (x, y, z, ax, ay, az, bx, by, bz, ra, rb) => {
  const px = x - ax, py = y - ay, pz = z - az, dx = bx - ax, dy = by - ay, dz = bz - az;
  const h = Math.min(1, Math.max(0, (px * dx + py * dy + pz * dz) / (dx * dx + dy * dy + dz * dz)));
  const qx = px - dx * h, qy = py - dy * h, qz = pz - dz * h;
  return Math.sqrt(qx * qx + qy * qy + qz * qz) - (ra + (rb - ra) * h);
};
const smin = (a, b, k) => {
  const h = Math.max(k - Math.abs(a - b), 0) / k;
  return Math.min(a, b) - (h * h * k) / 4;
};
const smaxk = (a, b, k) => -smin(-a, -b, k);
/** `a` with `b` carved out of it, the edge rounded over `k`. */
const ssub = (a, b, k) => smaxk(a, -b, k);

// ---- the head ---------------------------------------------------------------
export const EYE_R = 0.012;
export const EYE_X = 0.032, EYE_Y = 0.021;
/** The mouth wraps round the teeth: in its own coordinates the face's
 * curve back from the middle (BEND x^2) is taken out. */
const BEND = 22;

/** The head without its eye sockets (negative inside). `ears` false: the
 * hair's own field, which lies over the ears' tops instead of round them. */
const headField = (x, y, z, ears = true) => {
  const ax = Math.abs(x);
  // Cranium, a forehead, the face's mass, the jaw's angles and a chin.
  let d = ell(x, y, z, 0, 0.014, -0.008, 0.076, 0.096, 0.095);
  d = smin(d, ell(x, y, z, 0, 0.028, 0.035, 0.064, 0.05, 0.06), 0.03);
  d = smin(d, ell(x, y, z, 0, -0.03, 0.02, 0.056, 0.072, 0.074), 0.032);
  d = smin(d, ell(ax, y, z, 0.037, -0.058, 0.004, 0.02, 0.022, 0.034), 0.026);
  d = smin(d, ell(x, y, z, 0, -0.084, 0.054, 0.02, 0.016, 0.021), 0.02);
  // The muzzle over the teeth, so cheeks run into the lips without a
  // valley; cheekbones and the brow ridge.
  d = smin(d, ell(x, y, z, 0, -0.041, 0.063, 0.031, 0.026, 0.026), 0.016);
  d = smin(d, ell(ax, y, z, 0.041, -0.006, 0.055, 0.019, 0.013, 0.019), 0.018);
  d = smin(d, ell(x, y, z, 0, 0.037, 0.072, 0.056, 0.012, 0.02), 0.02);
  // The nose: a bridge narrowing down from the brow, a round tip, two
  // wings sitting back on the face; two nostrils under the tip.
  let nose = cap(x, y, z, 0, 0.03, 0.085, 0, -0.008, 0.109, 0.0068, 0.0084);
  nose = smin(nose, ell(x, y, z, 0, -0.016, 0.1085, 0.0102, 0.0092, 0.0098), 0.006);
  nose = smin(nose, ell(ax, y, z, 0.0118, -0.0224, 0.0962, 0.0066, 0.0062, 0.0072), 0.0075);
  d = smin(d, nose, 0.01);
  d = ssub(d, ell(ax, y, z, 0.006, -0.027, 0.0995, 0.0032, 0.002, 0.0048), 0.0024);
  // The mouth, in coordinates that follow the curve of the teeth.
  const mz = z + BEND * x * x;
  // The groove from the nose to the lip (philtrum).
  d = ssub(d, cap(x, y, mz, 0, -0.0305, 0.1005, 0, -0.0392, 0.1042, 0.0016, 0.002), 0.003);
  // Lips: the upper one with a small bead at its middle, the lower one
  // fuller and a little back; a thin line where they meet and the soft
  // groove under the lower lip.
  let lips = ell(x, y, mz, 0, -0.0424, 0.0928, 0.0228, 0.0058, 0.0094);
  lips = smin(lips, ell(x, y, mz, 0, -0.0452, 0.0968, 0.0062, 0.0028, 0.0052), 0.003);
  lips = smin(lips, ell(x, y, mz, 0, -0.0536, 0.0888, 0.0196, 0.0066, 0.0098), 0.002);
  d = smin(d, lips, 0.0045);
  d = ssub(d, ell(x, y, mz, 0, -0.0484, 0.1036, 0.0218, 0.0014, 0.0042), 0.0016);
  d = ssub(d, ell(x, y, mz, 0, -0.0636, 0.0962, 0.0135, 0.0022, 0.0026), 0.007);
  if (ears) {
    // Ears: a flap leaning back with a rolled rim, a hollow and a lobe.
    let ear = ell(ax, y, z, 0.075, 0.004, -0.014, 0.011, 0.029, 0.018);
    ear = smin(ear, ell(ax, y, z, 0.078, -0.024, -0.01, 0.007, 0.009, 0.008), 0.005);
    ear = ssub(ear, ell(ax, y, z, 0.087, 0.0, -0.011, 0.0055, 0.0185, 0.0115), 0.0025);
    ear = ssub(ear, ell(ax, y, z, 0.085, -0.008, -0.004, 0.006, 0.008, 0.007), 0.003);
    d = smin(d, ear, 0.006);
  }
  // The neck, oval, leaning a little forward, into the shoulders.
  const nz = z + 0.016 + 0.06 * (y + 0.06);
  const neck = (Math.hypot(x / 0.058, nz / 0.052) - 1) * 0.052;
  d = smin(d, Math.max(neck, y + 0.03), 0.012);
  // Cut inside the top, where nothing shows.
  return Math.max(d, -(y + 0.235));
};
/** The z of the plain head at (x, y) on its front (bisection). */
const surfaceZ = (x, y) => {
  let lo = 0.0, hi = 0.14;
  for (let i = 0; i < 50; i++) {
    const z = 0.5 * (lo + hi);
    if (headField(x, y, z) > 0) hi = z;
    else lo = z;
  }
  return 0.5 * (lo + hi);
};
/** The z of the flat front the eyes sit in. */
const FRONT_Z = surfaceZ(EYE_X, EYE_Y);
/** The eye socket: round each eye the face is flattened onto z = FRONT_Z
 * (so both corners of an eye sit at one depth) and pressed into a bowl,
 * 11 mm deep at the eye's centre, narrower up and down than across; the
 * head is cut by that surface with a 4 mm knee, fading back to the plain
 * head from 12 to 34 mm out (no rim round a flat plate). */
const sockets = (x, y, z, d) => {
  if (z <= 0) return d;
  const dx = Math.abs(x) - EYE_X, dy = y - EYE_Y;
  const re = Math.hypot(dx, dy / (dy > 0 ? 0.6 : 0.45));
  const w = 1 - smooth(0.012, 0.034, re);
  if (w <= 0) return d;
  const target = FRONT_Z - 0.011 * gauss(re, 0.014);
  return d + w * (smaxk(d, z - target, 0.004) - d);
};
const skinField = (x, y, z) => sockets(x, y, z, headField(x, y, z));

// ---- the top and the socle ----------------------------------------------------
export const CHEST_CUT = -0.33;
const rbox = (x, y, z, cx, cy, cz, hx, hy, hz, r) => {
  const qx = Math.abs(x - cx) - hx + r, qy = Math.abs(y - cy) - hy + r, qz = Math.abs(z - cz) - hz + r;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0), Math.max(qz, 0)) + Math.min(Math.max(qx, qy, qz), 0) - r;
};
const topField = (x, y, z) => {
  const ax = Math.abs(x);
  // A flat-fronted chest, shoulders sloping down from the neck, the arms
  // cut off flat at the sides, as busts are.
  let d = rbox(x, y, z, 0, -0.262, -0.012, 0.15, 0.065, 0.06, 0.035);
  d = smin(d, cap(ax, y, z, 0.0, -0.166, -0.022, 0.172, -0.21, -0.02, 0.046, 0.052), 0.045);
  d = smin(d, ell(x, y, z, 0, -0.177, -0.026, 0.074, 0.035, 0.058), 0.03);
  d = smin(d, ell(x, y, z, 0, -0.19, 0.012, 0.085, 0.035, 0.05), 0.03);
  d = smaxk(d, ax - 0.192, 0.012);
  // The rolled neckband, lower at the front.
  const ny = y + 0.163 - 0.01 * smooth(-0.02, 0.06, z);
  const nr = Math.hypot(x / 1.08, z + 0.018) - 0.063;
  d = smin(d, Math.hypot(nr, ny) - 0.0075, 0.006);
  return smaxk(d, CHEST_CUT - y, 0.006);
};
/** The socle's profile, top down: [radius, y]. */
const SOCLE = [
  [0.0, CHEST_CUT + 0.002], [0.064, CHEST_CUT + 0.002], [0.067, CHEST_CUT - 0.002], [0.067, CHEST_CUT - 0.011], [0.064, CHEST_CUT - 0.014],
  [0.054, CHEST_CUT - 0.018], [0.046, CHEST_CUT - 0.026], [0.043, CHEST_CUT - 0.036], [0.042, CHEST_CUT - 0.062], [0.044, CHEST_CUT - 0.066],
  [0.049, CHEST_CUT - 0.069], [0.051, CHEST_CUT - 0.073], [0.049, CHEST_CUT - 0.077], [0.047, CHEST_CUT - 0.079], [0.052, CHEST_CUT - 0.085],
  [0.064, CHEST_CUT - 0.095], [0.078, CHEST_CUT - 0.104], [0.086, CHEST_CUT - 0.109], [0.088, CHEST_CUT - 0.112], [0.088, CHEST_CUT - 0.132],
  [0.085, CHEST_CUT - 0.135], [0.0, CHEST_CUT - 0.135],
];
export const SOCLE_FOOT = CHEST_CUT - 0.135;

// ---- surface nets -------------------------------------------------------------
/** Grid coordinates from lo to hi: `fine` metres apart inside [a, b], `coarse`
 * outside, blending over `blend` metres. */
const axis = (lo, hi, a, b, fine, coarse, blend) => {
  const out = [lo];
  let x = lo;
  while (x < hi) {
    const out1 = Math.max(a - x, x - b, 0);
    x += fine + (coarse - fine) * smooth(0, blend, out1);
    out.push(x);
  }
  return out;
};
/** The zero surface of `f` on the grid X x Y x Z: one vertex a crossed
 * cell, pulled onto the surface along the field's gradient; quads across
 * every crossed grid edge, split on their shorter diagonal, facing out. */
const surfaceNets = (f, X, Y, Z) => {
  const nx = X.length, ny = Y.length, nz = Z.length;
  const F = new Float32Array(nx * ny * nz);
  const id = (i, j, k) => (k * ny + j) * nx + i;
  for (let k = 0; k < nz; k++) for (let j = 0; j < ny; j++) for (let i = 0; i < nx; i++) F[id(i, j, k)] = f(X[i], Y[j], Z[k]);
  const cid = (i, j, k) => (k * (ny - 1) + j) * (nx - 1) + i;
  const cellV = new Int32Array((nx - 1) * (ny - 1) * (nz - 1)).fill(-1);
  const pos = [];
  const E = [[0, 1], [2, 3], [4, 5], [6, 7], [0, 2], [1, 3], [4, 6], [5, 7], [0, 4], [1, 5], [2, 6], [3, 7]];
  const grad = (p) => {
    const h = 1e-4;
    return [f(p[0] + h, p[1], p[2]) - f(p[0] - h, p[1], p[2]), f(p[0], p[1] + h, p[2]) - f(p[0], p[1] - h, p[2]), f(p[0], p[1], p[2] + h) - f(p[0], p[1], p[2] - h)].map((v) => v / (2 * h));
  };
  for (let k = 0; k < nz - 1; k++) for (let j = 0; j < ny - 1; j++) for (let i = 0; i < nx - 1; i++) {
    const c = [];
    let neg = 0;
    for (let q = 0; q < 8; q++) {
      const v = F[id(i + (q & 1), j + ((q >> 1) & 1), k + ((q >> 2) & 1))];
      c.push(v);
      if (v < 0) neg++;
    }
    if (neg === 0 || neg === 8) continue;
    const P = (q) => [X[i + (q & 1)], Y[j + ((q >> 1) & 1)], Z[k + ((q >> 2) & 1)]];
    let s = [0, 0, 0], n = 0;
    for (const [a, b] of E) {
      if (c[a] < 0 === c[b] < 0) continue;
      const t = c[a] / (c[a] - c[b]);
      s = add(s, add(P(a), scale(sub(P(b), P(a)), t)));
      n++;
    }
    let p = scale(s, 1 / n);
    const lo = P(0), hi = P(7);
    for (let it = 0; it < 4; it++) {
      const g = grad(p);
      const gg = dot(g, g);
      if (gg < 1e-12) break;
      p = sub(p, scale(g, f(...p) / gg));
      // Stay near the cell (half a cell out at most).
      p = p.map((v, ax) => Math.min(Math.max(v, lo[ax] - 0.5 * (hi[ax] - lo[ax])), hi[ax] + 0.5 * (hi[ax] - lo[ax])));
    }
    cellV[cid(i, j, k)] = pos.length;
    pos.push(p);
  }
  const idx = [];
  const quad = (a, b, c, d, flip) => {
    if ([a, b, c, d].some((v) => v < 0)) return;
    if (flip) [b, d] = [d, b];
    const l1 = Math.hypot(...sub(pos[a], pos[c])), l2 = Math.hypot(...sub(pos[b], pos[d]));
    if (l1 <= l2) idx.push(a, b, c, a, c, d);
    else idx.push(a, b, d, b, c, d);
  };
  for (let k = 1; k < nz - 1; k++) for (let j = 1; j < ny - 1; j++) for (let i = 0; i < nx - 1; i++) {
    const a = F[id(i, j, k)], b = F[id(i + 1, j, k)];
    if (a < 0 === b < 0) continue;
    quad(cellV[cid(i, j - 1, k - 1)], cellV[cid(i, j, k - 1)], cellV[cid(i, j, k)], cellV[cid(i, j - 1, k)], !(a < 0));
  }
  for (let k = 1; k < nz - 1; k++) for (let j = 0; j < ny - 1; j++) for (let i = 1; i < nx - 1; i++) {
    const a = F[id(i, j, k)], b = F[id(i, j + 1, k)];
    if (a < 0 === b < 0) continue;
    quad(cellV[cid(i - 1, j, k - 1)], cellV[cid(i - 1, j, k)], cellV[cid(i, j, k)], cellV[cid(i, j, k - 1)], !(a < 0));
  }
  for (let k = 0; k < nz - 1; k++) for (let j = 1; j < ny - 1; j++) for (let i = 1; i < nx - 1; i++) {
    const a = F[id(i, j, k)], b = F[id(i, j, k + 1)];
    if (a < 0 === b < 0) continue;
    quad(cellV[cid(i - 1, j - 1, k)], cellV[cid(i, j - 1, k)], cellV[cid(i, j, k)], cellV[cid(i - 1, j, k)], !(a < 0));
  }
  const nrm = pos.map((p) => norm(grad(p)));
  return { pos, nrm, idx };
};
/** Simplify `m` (pos: [p], idx) to about `target` vertices by quadric edge
 * collapses (Garland and Heckbert 1997). A collapsed pair moves to its
 * midpoint pulled back onto the field `f`'s surface, so the mesh never
 * drifts off the true shape; `weight(p)` scales the cost (the face's
 * features keep their detail), and a collapse that would turn a triangle
 * over or make a sliver is refused. Normals come from the field's
 * gradient afterwards. */
const simplify = (m, f, target, weight, keep = () => false) => {
  if (process.env.BUST_RAW) target = Infinity;
  const P = m.pos.map((p) => p.slice());
  const nv = P.length;
  const T = [];
  for (let t = 0; t < m.idx.length; t += 3) T.push([m.idx[t], m.idx[t + 1], m.idx[t + 2]]);
  const alive = new Uint8Array(T.length).fill(1);
  const vt = Array.from({ length: nv }, () => []);
  T.forEach((t, i) => t.forEach((v) => vt[v].push(i)));
  const Q = Array.from({ length: nv }, () => new Float64Array(10));
  const plane = (t) => {
    const [a, b, c] = t.map((v) => P[v]);
    const n = cross(sub(b, a), sub(c, a));
    const l = Math.hypot(...n);
    if (l < 1e-18) return null;
    return [...n.map((x) => x / l), -dot(n, a) / l, l / 2];
  };
  const addQ = (q, [a, b, c, d], w) => {
    q[0] += w * a * a; q[1] += w * a * b; q[2] += w * a * c; q[3] += w * a * d;
    q[4] += w * b * b; q[5] += w * b * c; q[6] += w * b * d;
    q[7] += w * c * c; q[8] += w * c * d; q[9] += w * d * d;
  };
  for (const t of T) {
    const pl = plane(t);
    if (!pl) continue;
    for (const v of t) addQ(Q[v], pl, pl[4] * weight(P[v]));
  }
  const err = (q, p) => {
    const [x, y, z] = p;
    return q[0] * x * x + 2 * q[1] * x * y + 2 * q[2] * x * z + 2 * q[3] * x + q[4] * y * y + 2 * q[5] * y * z + 2 * q[6] * y + q[7] * z * z + 2 * q[8] * z + q[9];
  };
  const onSurface = (p) => {
    for (let it = 0; it < 4; it++) {
      const h = 1e-4;
      const g = [f(p[0] + h, p[1], p[2]) - f(p[0] - h, p[1], p[2]), f(p[0], p[1] + h, p[2]) - f(p[0], p[1] - h, p[2]), f(p[0], p[1], p[2] + h) - f(p[0], p[1], p[2] - h)].map((v) => v / (2 * h));
      const gg = dot(g, g);
      if (gg < 1e-12) break;
      p = sub(p, scale(g, f(...p) / gg));
    }
    return p;
  };
  const gradN = (p) => {
    const h = 1e-4;
    return norm([f(p[0] + h, p[1], p[2]) - f(p[0] - h, p[1], p[2]), f(p[0], p[1] + h, p[2]) - f(p[0], p[1] - h, p[2]), f(p[0], p[1], p[2] + h) - f(p[0], p[1], p[2] - h)]);
  };
  const G = P.map(gradN);
  const ver = new Int32Array(nv);
  const gone = new Uint8Array(nv);
  // A binary heap of [cost, a, b, verA, verB, x, y, z].
  const heap = [];
  const push = (e) => {
    heap.push(e);
    let i = heap.length - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (heap[p][0] <= heap[i][0]) break;
      [heap[p], heap[i]] = [heap[i], heap[p]];
      i = p;
    }
  };
  const pop = () => {
    const top = heap[0], last = heap.pop();
    if (heap.length) {
      heap[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1, r = l + 1;
        let s = i;
        if (l < heap.length && heap[l][0] < heap[s][0]) s = l;
        if (r < heap.length && heap[r][0] < heap[s][0]) s = r;
        if (s === i) break;
        [heap[s], heap[i]] = [heap[i], heap[s]];
        i = s;
      }
    }
    return top;
  };
  const consider = (a, b) => {
    const q = Q[a].map((v, i) => v + Q[b][i]);
    const p = onSurface(scale(add(P[a], P[b]), 0.5));
    const len = Math.hypot(...sub(P[a], P[b]));
    push([err(q, p) + 1e-9 * len * len, a, b, ver[a], ver[b], p]);
  };
  const fill = () => {
  const seen = new Set();
  for (let ti = 0; ti < T.length; ti++) if (alive[ti]) for (let k = 0; k < 3; k++) {
    const t = T[ti];
    const a = Math.min(t[k], t[(k + 1) % 3]), b = Math.max(t[k], t[(k + 1) % 3]);
    const key = a * nv + b;
    if (seen.has(key)) continue;
    seen.add(key);
    consider(a, b);
  }
  };
  let live = nv;
  for (let pass = 0; pass < 8 && live > target; pass++) {
  const before = live;
  heap.length = 0;
  fill();
  while (live > target && heap.length) {
    const [, a, b, va, vb, p] = pop();
    if (gone[a] || gone[b] || ver[a] !== va || ver[b] !== vb) continue;
    if (keep(P[a]) || keep(P[b])) continue;
    const gp = gradN(p);
    // Refuse a collapse that turns a triangle over or squeezes it thin.
    let ok = true;
    const touched = new Set([...vt[a], ...vt[b]].filter((t) => alive[t]));
    const neighbours = (v) => new Set(vt[v].filter((t) => alive[t]).flatMap((t) => T[t]).filter((x) => x !== v));
    const na = neighbours(a), nb = neighbours(b);
    let shared = 0;
    for (const x of na) if (nb.has(x)) shared++;
    if (shared !== 2) ok = false; // keep the surface a manifold
    for (const t of touched) {
      if (!ok) break;
      const tri = T[t];
      if (tri.includes(a) && tri.includes(b)) continue;
      const before = tri.map((v) => P[v]);
      const after = tri.map((v) => (v === a || v === b ? p : P[v]));
      const n0 = cross(sub(before[1], before[0]), sub(before[2], before[0]));
      const n1 = cross(sub(after[1], after[0]), sub(after[2], after[0]));
      const l0 = Math.hypot(...n0), l1 = Math.hypot(...n1);
      if (l1 < 1e-16 || dot(n0, n1) < 0.5 * l0 * l1) ok = false;
      // Every triangle stays within 35 degrees of the true surface.
      const gs = norm(tri.map((v) => (v === a || v === b ? gp : G[v])).reduce(add));
      if (l1 > 0 && dot(n1, gs) < Math.cos((35 * Math.PI) / 180) * l1) ok = false;
      const e = [sub(after[1], after[0]), sub(after[2], after[1]), sub(after[0], after[2])].map((x) => Math.hypot(...x));
      const longest = Math.max(...e);
      const e0 = [sub(before[1], before[0]), sub(before[2], before[1]), sub(before[0], before[2])].map((x) => Math.hypot(...x));
      const q0 = l0 / Math.max(...e0) ** 2, q1 = l1 / (longest * longest);
      if (q1 < 0.12 && q1 < q0) ok = false;
    }
    if (!ok) continue;
    // a takes b's place.
    P[a] = p;
    G[a] = gp;
    for (let i = 0; i < 10; i++) Q[a][i] += Q[b][i];
    gone[b] = 1;
    live--;
    for (const t of vt[b]) {
      if (!alive[t]) continue;
      const tri = T[t];
      if (tri.includes(a)) alive[t] = 0;
      else {
        tri[tri.indexOf(b)] = a;
        vt[a].push(t);
      }
    }
    vt[a] = vt[a].filter((t) => alive[t]);
    ver[a]++;
    for (const x of new Set(vt[a].flatMap((t) => T[t]))) if (x !== a) ver[x]++;
    for (const x of new Set(vt[a].flatMap((t) => T[t]))) if (x !== a) consider(a, x);
  }
  if (live > before - 50) break;
  }
  if (process.env.BUST_DEBUG) console.error(`simplify: ${nv} -> ${live} (target ${target})`);
  const remap = new Int32Array(nv).fill(-1);
  const pos = [];
  for (let v = 0; v < nv; v++) if (!gone[v] && vt[v].some((t) => alive[t])) {
    remap[v] = pos.length;
    pos.push(P[v]);
  }
  const idx = [];
  T.forEach((t, i) => {
    if (alive[i]) idx.push(...t.map((v) => remap[v]));
  });
  const h = 1e-4;
  const nrm = pos.map((p) => norm([f(p[0] + h, p[1], p[2]) - f(p[0] - h, p[1], p[2]), f(p[0], p[1] + h, p[2]) - f(p[0], p[1] - h, p[2]), f(p[0], p[1], p[2] + h) - f(p[0], p[1], p[2] - h)]));
  return { pos, nrm, idx };
};
/** A surface-nets mesh with equirect UVs from the head's centre; vertices
 * on triangles that cross the seam at the back are split. */
const withEquirect = (m) => {
  const out = { pos: [], nrm: [], uv: [], idx: [] };
  const uvOf = (p) => [Math.atan2(p[0], p[2]) / TAU + 0.5, Math.acos(Math.max(-1, Math.min(1, p[1] / (Math.hypot(...p) || 1)))) / TAU];
  const uvs = m.pos.map(uvOf);
  for (let v = 0; v < m.pos.length; v++) {
    out.pos.push(...m.pos[v]);
    out.nrm.push(...m.nrm[v]);
    out.uv.push(...uvs[v]);
  }
  const dup = new Map();
  for (let t = 0; t < m.idx.length; t += 3) {
    const tri = m.idx.slice(t, t + 3);
    const us = tri.map((v) => uvs[v][0]);
    const wraps = Math.max(...us) - Math.min(...us) > 0.5;
    for (const v of tri) {
      if (wraps && uvs[v][0] < 0.5) {
        if (!dup.has(v)) {
          dup.set(v, out.pos.length / 3);
          out.pos.push(...m.pos[v]);
          out.nrm.push(...m.nrm[v]);
          out.uv.push(uvs[v][0] + 1, uvs[v][1]);
        }
        out.idx.push(dup.get(v));
      } else out.idx.push(v);
    }
  }
  return out;
};
/** A plain mesh (no UV needs) in the flat-array form. */
const flat = (m, uvOf) => {
  const out = { pos: [], nrm: [], uv: [], idx: m.idx.slice() };
  m.pos.forEach((p, v) => {
    out.pos.push(...p);
    out.nrm.push(...m.nrm[v]);
    out.uv.push(...uvOf(p));
  });
  return out;
};

// ---- a grid surface ---------------------------------------------------------
/** A (rows + 1) x (cols + 1) grid of `pos(a, b)` for a, b in 0..1, with
 * UVs from `uvOf(a, b)`. Normals are the surface normal, turned to agree
 * with `outward(a, b)`; triangles wind counter-clockwise seen from that
 * side, and ones with no area are dropped. */
const grid = (rows, cols, pos, uvOf, outward) => {
  const m = { pos: [], nrm: [], uv: [], idx: [] };
  const h = 1e-4;
  const nAt = (a, b) => {
    const a0 = Math.min(Math.max(a, 2e-3), 1 - 2e-3);
    const da = sub(pos(a0 + h, b), pos(a0 - h, b));
    const db = sub(pos(a0, b + h), pos(a0, b - h));
    let n = norm(cross(da, db));
    if (dot(n, outward(a, b)) < 0) n = scale(n, -1);
    return n;
  };
  for (let i = 0; i <= rows; i++) {
    for (let j = 0; j <= cols; j++) {
      const a = i / rows, b = j / cols;
      m.pos.push(...pos(a, b));
      m.nrm.push(...nAt(a, b));
      m.uv.push(...uvOf(a, b));
    }
  }
  const P = (k) => m.pos.slice(k * 3, k * 3 + 3);
  const N = (k) => m.nrm.slice(k * 3, k * 3 + 3);
  const tri = (a, b, c) => {
    const n = cross(sub(P(b), P(a)), sub(P(c), P(a)));
    if (Math.hypot(...n) < 1e-14) return;
    if (dot(n, add(add(N(a), N(b)), N(c))) >= 0) m.idx.push(a, b, c);
    else m.idx.push(a, c, b);
  };
  for (let i = 0; i < rows; i++) {
    for (let j = 0; j < cols; j++) {
      const k = i * (cols + 1) + j;
      tri(k, k + cols + 1, k + cols + 2);
      tri(k, k + cols + 2, k + 1);
    }
  }
  return m;
};
/** Several meshes as one primitive. */
const merge = (...ms) => {
  const out = { pos: [], nrm: [], uv: [], idx: [] };
  for (const m of ms) {
    const base = out.pos.length / 3;
    for (const v of m.pos) out.pos.push(v);
    for (const v of m.nrm) out.nrm.push(v);
    for (const v of m.uv) out.uv.push(v);
    for (const k of m.idx) out.idx.push(k + base);
  }
  return out;
};

// ---- skin ---------------------------------------------------------------------
// Fine round the eyes, nose and mouth, coarse elsewhere, then simplified
// to SKIN_VERTS, keeping the face's features (the eyes' sockets, where the
// lids lie on the skin, most of all).
const SKIN_VERTS = 20000;
const faceWeight = (p) => {
  const eyes = gauss(Math.hypot(Math.abs(p[0]) - EYE_X, (p[1] - EYE_Y) * 1.4), 0.02);
  const mid = gauss(Math.hypot(p[0] * 1.3, p[1] + 0.03), 0.045) * smooth(0.04, 0.08, p[2]);
  return 1 + 40 * eyes * smooth(0.03, 0.06, p[2]) + 8 * mid;
};
const skinMesh = withEquirect(
  simplify(
    surfaceNets(
      skinField,
      axis(-0.1, 0.1, -0.045, 0.045, 0.0008, 0.0026, 0.012),
      axis(-0.245, 0.125, -0.07, 0.04, 0.0008, 0.0026, 0.012),
      axis(-0.115, 0.13, 0.055, 0.13, 0.0008, 0.0026, 0.012),
    ),
    skinField,
    SKIN_VERTS,
    faceWeight,
    // The lids lie on the skin round each eye: it keeps its full detail
    // there, so no simplified facet pokes through them.
    // So do the nostrils and the mouth's line and corners.
    (p) =>
      (p[2] > 0.04 && Math.abs(p[0]) > 0.006 && Math.abs(p[0]) < EYE_X + 0.02 && p[1] - EYE_Y > -0.02 && p[1] - EYE_Y < 0.0145) ||
      (p[2] > 0.08 && Math.abs(p[0]) < 0.017 && p[1] > -0.034 && p[1] < -0.014) ||
      (p[2] > 0.065 && Math.abs(p[0]) < 0.03 && p[1] > -0.056 && p[1] < -0.041),
  ),
);

// Eyes: welded cube-spheres, UVs from direction.
// The front of the eyeball stands 0.5 mm proud of the flat front the
// socket presses the face onto; the lids cover all but an almond of it.
export const EYE_Z = FRONT_Z + 0.0005 - EYE_R;
// A cube's six faces pushed out onto the sphere, welded: no poles, so no fan
// of slivers for the converter's simplifier to turn over (geocheck, at the
// pupil and then at a corner pole); the UVs come from each point's
// direction, the same azimuthal layout as before.
const EYE_CELLS = 14;
const eye = (sx) => {
  const c = [sx * EYE_X, EYE_Y, EYE_Z];
  const uvOf = (v) => {
    const ang = Math.acos(Math.max(-1, Math.min(1, v[2])));
    const ph = Math.atan2(v[1], v[0]);
    return [0.5 + (ang / TAU) * Math.cos(ph), 0.5 + (ang / TAU) * Math.sin(ph)];
  };
  const m = { pos: [], nrm: [], uv: [], idx: [] };
  const ids = new Map();
  const vert = (v) => {
    const d = norm(v);
    const key = d.map((x) => x.toFixed(7)).join(',');
    if (!ids.has(key)) {
      ids.set(key, m.pos.length / 3);
      m.pos.push(...add(c, scale(d, EYE_R)));
      m.nrm.push(...d);
      m.uv.push(...uvOf(d));
    }
    return ids.get(key);
  };
  const faces = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
  for (const [i, j, k] of faces) {
    for (const s of [-1, 1]) {
      const at = (u, v) => {
        const p = [0, 0, 0];
        p[i] = s;
        p[j] = -1 + (2 * u) / EYE_CELLS;
        p[k] = -1 + (2 * v) / EYE_CELLS;
        return vert(p);
      };
      if ((i + 1) % 3 !== j) continue; // each axis once, both signs
      for (let u = 0; u < EYE_CELLS; u++) {
        for (let v = 0; v < EYE_CELLS; v++) {
          const q = [at(u, v), at(u + 1, v), at(u + 1, v + 1), at(u, v + 1)];
          for (const [a, b, d] of [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]) {
            const P = (x) => m.pos.slice(x * 3, x * 3 + 3);
            const n = cross(sub(P(b), P(a)), sub(P(d), P(a)));
            if (dot(n, sub(P(a), c)) >= 0) m.idx.push(a, b, d);
            else m.idx.push(a, d, b);
          }
        }
      }
    }
  }
  return m;
};
const eyes = merge(eye(1), eye(-1));

// Lids: one skin shell per eye, round the eyeball with an almond hole cut
// exactly on its grid. a runs from the lid's edge (0) out across a band 7 mm
// wide (1), b round the eye. The edge hugs the eyeball (0.3 mm off it), the
// shell thickens to 1 mm off it within the first quarter (a rounded lid
// margin), then bends onto the skin's own surface, never below it, and lies
// on it from a = 0.7 out, so lid and face meet with the same normal. The
// opening: 21 mm across, 3.6 mm up and 3.3 mm down from the centre (the
// lids cover the iris's top and bottom edges: no staring white ring), its
// outer corner 0.8 mm higher. UVs: the skin's equirect mapping.
export const LID_W = 0.0105, LID_UP = 0.0036, LID_DOWN = 0.0033, LID_BAND = 0.007;
/** The skin's z at (x, y) round the eyes (bisection along z). */
const headZ = (x, y) => {
  let lo = FRONT_Z - 0.03, hi = FRONT_Z + 0.02;
  for (let i = 0; i < 30; i++) {
    const z = 0.5 * (lo + hi);
    if (skinField(x, y, z) > 0) hi = z;
    else lo = z;
  }
  return 0.5 * (lo + hi);
};
const smax = (a, b, k = 0.0005) => {
  const h = Math.min(1, Math.max(0, 0.5 + (0.5 * (a - b)) / k));
  return b + (a - b) * h + k * h * (1 - h);
};
const lids = (sx) => {
  const c = [sx * EYE_X, EYE_Y, EYE_Z];
  const edge = (b) => {
    const s = b * TAU;
    const sn = Math.sin(s);
    const h = sn > 0 ? LID_UP : LID_DOWN;
    return [LID_W * Math.cos(s), h * Math.sign(sn) * Math.abs(sn) ** 1.3 + 0.0008 * Math.cos(s)];
  };
  const at = (a, b) => {
    const s = b * TAU;
    const [ex, ey] = edge(b);
    const px = ex + LID_BAND * Math.cos(s) * a;
    const py = ey + LID_BAND * Math.sin(s) * a;
    const r = EYE_R + 0.0003 + 0.0007 * smooth(0, 0.25, a);
    const rho2 = px * px + py * py;
    const rho = Math.sqrt(rho2), r0 = 0.8 * r, z0 = Math.sqrt(r * r - r0 * r0);
    const zs = rho < r0 ? Math.sqrt(r * r - rho2) : z0 - (r0 / z0) * (rho - r0);
    const hz = headZ(c[0] + sx * px, c[1] + py) - c[2] - 0.00005;
    const t = smooth(0.12, 0.7, a);
    const lid = smax(zs, hz, 0.002);
    const floor = 0.0004 + (rho < EYE_R ? Math.sqrt(EYE_R * EYE_R - rho2) : -10 * (rho - EYE_R));
    const pz = smax(lid + (hz - lid) * t, floor, 0.002);
    return add(c, [sx * px, py, pz]);
  };
  return grid(
    12,
    72,
    (a, b) => at(a ** 1.5, b),
    (a, b) => {
      const p = at(a ** 1.5, b);
      return [Math.atan2(p[0], p[2]) / TAU + 0.5, Math.acos(p[1] / Math.hypot(...p)) / TAU];
    },
    (a, b) => {
      const h = 1e-4, a0 = Math.min(Math.max(a, h), 1 - h);
      const da = sub(at((a0 + h) ** 1.5, b), at((a0 - h) ** 1.5, b));
      const db = sub(at(a0 ** 1.5, b + h), at(a0 ** 1.5, b - h));
      return scale(cross(da, db), sx);
    },
  );
};

// ---- hair ---------------------------------------------------------------------
/** The hair's field surface along unit direction `d` from the head's
 * centre (no ears: the hair passes over their tops). */
const hairR = (d) => {
  let hi = 0.14, r = hi;
  while (r > 0.02 && headField(d[0] * r, d[1] * r, d[2] * r, false) > 0) {
    hi = r;
    r -= 0.002;
  }
  let lo = r;
  for (let i = 0; i < 30; i++) {
    const mid = 0.5 * (lo + hi);
    if (headField(d[0] * mid, d[1] * mid, d[2] * mid, false) > 0) hi = mid;
    else lo = mid;
  }
  return 0.5 * (lo + hi);
};
/** The hairline's polar angle (from +y) by longitude `p` (0 at the front):
 * the forehead at 47 degrees, a little higher at the temples, a short
 * sideburn in front of each ear, over the ears' tops, down to the nape. */
const hairline = (p) => {
  const s = (1 - Math.cos(p)) / 2;
  const temple = 4 * gauss(s - 0.28, 0.07);
  const burn = 14 * gauss(s - 0.4, 0.035);
  const deg = 47 + 19 * smooth(0, 0.5, s) + 42 * smooth(0.55, 1, s) - temple + burn;
  return (deg * Math.PI) / 180;
};
/** The whorl at the crown, a little behind the top; locks flow out from it
 * forward over the top, down the sides and the back. */
const WHORL = norm([0.012, 0.93, -0.36]);
const E1 = norm(cross([1, 0, 0], WHORL)).map((v) => -v); // towards the front
const E2 = cross(WHORL, E1);
const fromWhorl = (alpha, beta) => {
  const c = Math.cos(alpha), s = Math.sin(alpha);
  return norm(add(scale(WHORL, c), scale(add(scale(E1, Math.cos(beta)), scale(E2, Math.sin(beta))), s)));
};
const polarLon = (d) => [Math.acos(Math.max(-1, Math.min(1, d[1]))), Math.atan2(d[0], d[2])];
/** How far from the whorl (radians) the hairline lies along `beta`. */
const reachOf = (beta) => {
  const past = (a) => {
    const [t, p] = polarLon(fromWhorl(a, beta));
    return t > hairline(p);
  };
  let a = 0.02;
  while (!past(a) && a < 3) a += 0.02;
  let lo = a - 0.02, hi = a;
  for (let i = 0; i < 30; i++) {
    const mid = 0.5 * (lo + hi);
    if (past(mid)) hi = mid;
    else lo = mid;
  }
  return 0.5 * (lo + hi);
};
export const LOCKS = 96;
const HAIR_COLS = LOCKS * 2, HAIR_ROWS = 30;
const REACH = Array.from({ length: HAIR_COLS + 1 }, (_, j) => reachOf((j / HAIR_COLS) * TAU));
const RADIUS = new Map();
/** The hair volume's thickness at share `s` of the way from the whorl to the
 * hairline, by direction: fullest over the top and the front, close at
 * the sides, thinning to nothing at the hairline. */
const thickness = (s, d) => {
  const top = smooth(-0.2, 0.9, d[1]);
  const front = smooth(-0.3, 0.8, d[2]);
  const full = 0.0055 + 0.0055 * top + 0.003 * front * top;
  return full * (1 - smooth(0.62, 1.0, s) * 0.9) * (1 - smooth(0.9, 1.0, s));
};
/** A lock's own length (a share of the reach): each lock ends a little
 * before or after its neighbours, so the hairline is not one hard curve. */
const lockEnd = (lock) => 1 + 0.03 * (Math.sin(lock * 12.9898) * 43758.5453 % 1);
const hairAt = (a, j, lift, own = 1) => {
  const beta = (j / HAIR_COLS) * TAU;
  const s = a * own;
  const d = fromWhorl(s * REACH[j], beta);
  const key = `${s.toFixed(6)}:${j}`;
  let r = RADIUS.get(key);
  if (r === undefined) {
    r = hairR(d);
    RADIUS.set(key, r);
  }
  return scale(d, r + thickness(Math.min(s, 1), d) + lift);
};
/** A grid over the hair: rows from the whorl, columns round it. */
const hairGrid = (pos, uvOf) => {
  const m = { pos: [], nrm: [], uv: [], idx: [] };
  const P = [];
  for (let i = 0; i <= HAIR_ROWS; i++) for (let j = 0; j <= HAIR_COLS; j++) P.push(pos(i / HAIR_ROWS, j));
  const at = (i, j) => P[Math.min(Math.max(i, 0), HAIR_ROWS) * (HAIR_COLS + 1) + (((j % HAIR_COLS) + HAIR_COLS) % HAIR_COLS)];
  for (let i = 0; i <= HAIR_ROWS; i++) {
    for (let j = 0; j <= HAIR_COLS; j++) {
      const p = at(i, j);
      const da = i === 0 ? sub(at(1, j), p) : i === HAIR_ROWS ? sub(p, at(i - 1, j)) : sub(at(i + 1, j), at(i - 1, j));
      const db = sub(at(i, j + 1), at(i, j - 1));
      let n = norm(cross(db, da));
      if (i === 0 || Math.hypot(...cross(db, da)) < 1e-12) n = norm(p);
      if (dot(n, p) < 0) n = scale(n, -1);
      m.pos.push(...p);
      m.nrm.push(...n);
      m.uv.push(...uvOf(i / HAIR_ROWS, j / HAIR_COLS));
    }
  }
  for (let i = 0; i < HAIR_ROWS; i++) {
    for (let j = 0; j < HAIR_COLS; j++) {
      const k = i * (HAIR_COLS + 1) + j;
      for (const [a, b, c] of [[k, k + HAIR_COLS + 1, k + HAIR_COLS + 2], [k, k + HAIR_COLS + 2, k + 1]]) {
        const pa = m.pos.slice(a * 3, a * 3 + 3), pb = m.pos.slice(b * 3, b * 3 + 3), pc = m.pos.slice(c * 3, c * 3 + 3);
        const n = cross(sub(pb, pa), sub(pc, pa));
        if (Math.hypot(...n) < 1e-14) continue;
        if (dot(n, pa) >= 0) m.idx.push(a, b, c);
        else m.idx.push(a, c, b);
      }
    }
  }
  return m;
};
// The volume: opaque, stopping just short of the hairline under the locks.
const hairBase = hairGrid((a, j) => hairAt(a * 0.935, j, 0.0003), (a, b) => [b * LOCKS, a]);
// Two lock layers lying on it (1 and 2 mm up), the second half a lock round.
/** The engine narrows a lock along v (root 0, tip 1); v runs slowly over
 * most of the lock and fast over its last 6 %, so the locks stay full until
 * just before the hairline and taper to points there. */
const lockV = (a) => (a < 0.94 ? (0.3 * a) / 0.94 : 0.3 + (0.7 * (a - 0.94)) / 0.06);
const hairLayer = (layer) =>
  hairGrid(
    (a, j) => {
      const lock = Math.floor(((j / HAIR_COLS) * LOCKS + 0.5 * layer) % LOCKS);
      return hairAt(a, j, 0.001 + 0.001 * layer, lockEnd(lock + 97 * layer));
    },
    (a, b) => [b * LOCKS + 0.5 * layer, lockV(a)],
  );
/** The skin's z at (x, y) on the front (bisection from outside). */
const frontZ = (x, y) => {
  let lo = 0.0, hi = 0.14;
  for (let i = 0; i < 40; i++) {
    const z = 0.5 * (lo + hi);
    if (skinField(x, y, z) > 0) hi = z;
    else lo = z;
  }
  return 0.5 * (lo + hi);
};
/** Eyebrows: one soft lock each, lying 0.7 mm over the brow, from a full
 * inner end (v 0, the root) arching out to a thin tail (v 1); the hair
 * material narrows them towards the tail. */
const brow = (sx) => {
  const at = (a, t) => {
    const x = 0.011 + 0.041 * t;
    const mid = EYE_Y + 0.0125 + 0.0045 * Math.sin(Math.PI * Math.min(1, t * 1.15)) - 0.003 * t;
    const h = 0.0078 * (0.55 + 0.45 * smooth(0, 0.22, t)) * (1 - 0.45 * t);
    const y = mid + (a - 0.5) * h;
    const p = [x, y, frontZ(x, y)];
    const h2 = 1e-4;
    const n = norm([skinField(x + h2, y, p[2]) - skinField(x - h2, y, p[2]), skinField(x, y + h2, p[2]) - skinField(x, y - h2, p[2]), skinField(x, y, p[2] + h2) - skinField(x, y, p[2] - h2)]);
    const q = add(p, scale(n, 0.0007));
    return [sx * q[0], q[1], q[2]];
  };
  return grid(6, 24, (a, b) => at(a, b), (a, b) => [a, b], (a, b) => [0, 0, 1]);
};
const hair = merge(hairLayer(0), hairLayer(1), brow(1), brow(-1));

// ---- the top and the socle ----------------------------------------------------
const topMesh = flat(
  simplify(
    surfaceNets(topField, axis(-0.205, 0.205, -0.07, 0.07, 0.0028, 0.0045, 0.02), axis(-0.345, -0.085, -0.2, -0.14, 0.0025, 0.004, 0.02), axis(-0.13, 0.13, 0.0, 0.1, 0.0028, 0.0045, 0.02)),
    topField,
    4500,
    () => 1,
  ),
  (p) => [p[0] * 4, p[1] * 4],
);
const lathe = (profile, segs) => {
  // Each profile corner gets its own ring of vertices on both sides, so
  // the mouldings keep their edges; smooth runs share one normal.
  const m = { pos: [], nrm: [], uv: [], idx: [] };
  const rings = [];
  for (let i = 0; i < profile.length - 1; i++) {
    const [r0, y0] = profile[i], [r1, y1] = profile[i + 1];
    const t = norm([r1 - r0, y1 - y0]);
    const n2 = [-t[1], t[0]]; // outward in (r, y)
    rings.push([[r0, y0, n2], [r1, y1, n2]]);
  }
  // Average neighbouring normals where the turn is gentle (< 35 degrees).
  for (let i = 0; i < rings.length - 1; i++) {
    const a = rings[i][1][2], b = rings[i + 1][0][2];
    if (a[0] * b[0] + a[1] * b[1] > Math.cos((35 * Math.PI) / 180)) {
      const n = norm([a[0] + b[0], a[1] + b[1]]);
      rings[i][1][2] = n;
      rings[i + 1][0][2] = n;
    }
  }
  for (const seg of rings) {
    const base = m.pos.length / 3;
    for (const [r, y, n] of seg) {
      for (let j = 0; j <= segs; j++) {
        const th = (j / segs) * TAU;
        m.pos.push(r * Math.sin(th), y, r * Math.cos(th));
        m.nrm.push(n[0] * Math.sin(th), n[1], n[0] * Math.cos(th));
        m.uv.push(j / segs, y * 4);
      }
    }
    for (let j = 0; j < segs; j++) {
      const a = base + j, b = base + j + 1, c = base + segs + 1 + j, d = base + segs + 2 + j;
      const pa = m.pos.slice(a * 3, a * 3 + 3), pb = m.pos.slice(b * 3, b * 3 + 3), pc = m.pos.slice(c * 3, c * 3 + 3), pd = m.pos.slice(d * 3, d * 3 + 3);
      const out = m.nrm.slice(a * 3, a * 3 + 3);
      for (const [x, y, z, q] of [[a, c, d, [pa, pc, pd]], [a, d, b, [pa, pd, pb]]]) {
        const n = cross(sub(q[1], q[0]), sub(q[2], q[0]));
        if (Math.hypot(...n) < 1e-14) continue;
        if (dot(n, out) >= 0) m.idx.push(x, y, z);
        else m.idx.push(x, z, y);
      }
    }
  }
  return m;
};
const socle = lathe(SOCLE, 72);

// ---- glTF ---------------------------------------------------------------------
const bin = [];
let binLen = 0;
const views = [];
const accessors = [];
const pushView = (buf, target) => {
  const pad = (4 - (binLen % 4)) % 4;
  if (pad) {
    bin.push(Buffer.alloc(pad));
    binLen += pad;
  }
  views.push({ buffer: 0, byteOffset: binLen, byteLength: buf.length, target });
  bin.push(buf);
  binLen += buf.length;
  return views.length - 1;
};
const acc = (arr, type, comps, minmax = false) => {
  const isIdx = type === 'SCALAR';
  const f = isIdx ? null : new Float32Array(arr);
  const big = isIdx && arr.some((v) => v > 65535);
  const buf = isIdx ? Buffer.from((big ? new Uint32Array(arr) : new Uint16Array(arr)).buffer) : Buffer.from(f.buffer);
  const view = pushView(buf, isIdx ? 34963 : 34962);
  const a = { bufferView: view, componentType: isIdx ? (big ? 5125 : 5123) : 5126, count: arr.length / comps, type };
  if (minmax) {
    const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < f.length; i += 3) for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k], f[i + k]); max[k] = Math.max(max[k], f[i + k]); }
    a.min = min;
    a.max = max;
  }
  accessors.push(a);
  return accessors.length - 1;
};
const prim = (m, material) => {
  if (m.pos.length / 3 > 65535 && !process.env.BUST_RAW) throw new Error(`too many vertices for u16 indices (${m.pos.length / 3})`);
  return {
    attributes: { POSITION: acc(m.pos, 'VEC3', 3, true), NORMAL: acc(m.nrm, 'VEC3', 3), TEXCOORD_0: acc(m.uv, 'VEC2', 2) },
    indices: acc(m.idx, 'SCALAR', 1),
    material,
  };
};
const lin = (c) => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};
const rgb = (r, g, b) => [lin(r), lin(g), lin(b), 1].map((v) => Math.round(v * 1e4) / 1e4);

const parts = [
  ['head', merge(skinMesh, lids(1), lids(-1)), 0],
  ['eyes', eyes, 1],
  ['hair', hair, 2],
  ['hair_base', hairBase, 3],
  ['top', topMesh, 4],
  ['socle', socle, 5],
];
const gltf = {
  asset: { version: '2.0', generator: 'Evani.Dev char-bust.mjs (material viewer bust)' },
  scene: 0,
  scenes: [{ nodes: [0] }],
  nodes: [{ name: 'char_head', children: parts.map((_, i) => i + 1) }, ...parts.map(([name], i) => ({ name, mesh: i }))],
  meshes: parts.map(([name, m, mat]) => ({ name, primitives: [prim(m, mat)] })),
  materials: [
    {
      name: 'skin',
      pbrMetallicRoughness: { baseColorFactor: rgb(224, 172, 140), metallicFactor: 0, roughnessFactor: 0.55 },
      extras: { evani_kind: 'skin', evani_skin: { pore_scale: 500 } },
    },
    {
      name: 'eye',
      pbrMetallicRoughness: { baseColorFactor: rgb(235, 228, 220), metallicFactor: 0, roughnessFactor: 0.05 },
      extras: { evani_kind: 'eye', evani_eye: { iris_color: [0.12, 0.25, 0.33] } },
    },
    {
      name: 'hair',
      pbrMetallicRoughness: { baseColorFactor: rgb(66, 40, 24), metallicFactor: 0, roughnessFactor: 0.5 },
      doubleSided: true,
      extras: { evani_kind: 'hair', evani_hair: { tip_width: 0.25, edge_softness: 0.3, highlight_color: [0.5, 0.38, 0.26] } },
    },
    {
      name: 'hair_base',
      pbrMetallicRoughness: { baseColorFactor: rgb(52, 32, 20), metallicFactor: 0, roughnessFactor: 0.65 },
    },
    {
      name: 'top',
      pbrMetallicRoughness: { baseColorFactor: rgb(74, 92, 112), metallicFactor: 0, roughnessFactor: 0.85 },
    },
    {
      name: 'stone',
      pbrMetallicRoughness: { baseColorFactor: rgb(212, 204, 190), metallicFactor: 0, roughnessFactor: 0.45 },
    },
  ],
  bufferViews: views,
  accessors,
  buffers: [{ byteLength: 0 }],
};
const binBuf = Buffer.concat(bin);
const binPadded = Buffer.concat([binBuf, Buffer.alloc((4 - (binBuf.length % 4)) % 4)]);
gltf.buffers[0].byteLength = binPadded.length;
let json = Buffer.from(JSON.stringify(gltf), 'utf8');
json = Buffer.concat([json, Buffer.alloc((4 - (json.length % 4)) % 4, 0x20)]);
const header = Buffer.alloc(12);
header.writeUInt32LE(0x46546c67, 0);
header.writeUInt32LE(2, 4);
header.writeUInt32LE(12 + 8 + json.length + 8 + binPadded.length, 8);
const jh = Buffer.alloc(8);
jh.writeUInt32LE(json.length, 0);
jh.writeUInt32LE(0x4e4f534a, 4);
const bh = Buffer.alloc(8);
bh.writeUInt32LE(binPadded.length, 0);
bh.writeUInt32LE(0x004e4942, 4);
const out = process.argv[2] ?? 'char-head.glb';
writeFileSync(out, Buffer.concat([header, jh, json, bh, binPadded]));
const tris = parts.map(([n, m]) => `${n} ${m.idx.length / 3}/${m.pos.length / 3}v`).join(', ');
console.log(`${out}: ${12 + 16 + json.length + binPadded.length} bytes (triangles/vertices: ${tris}); eye centre z ${EYE_Z.toFixed(4)}; socle foot ${SOCLE_FOOT.toFixed(3)}`);
