// Writes char-head.glb, the character-materials fixture: our own small
// close-up head whose materials carry the `evani_kind` tags
// (docs/MODEL_FORMAT.md, "Character materials"). Node built-ins only, no
// textures; the output is the same bytes on every run.
//
//   node tests/fixtures/models/char-head.mjs tests/fixtures/models/char-head.glb
//
// Metres, the head's centre at the origin, facing +z, +y up:
// * head: an ellipsoid (0.09 x 0.115 x 0.1) with a nose, a brow and two
//   eye sockets pressed into it; its grid bunches up round the eyes.
//   Equirect UVs: u = longitude / 2 pi (seam at the back), v = polar
//   angle / 2 pi (0 at the crown, 0.5 under the chin), so a UV unit is
//   about the same length both ways for the pores. Material `skin`
//   (evani_kind skin, pore_scale 500).
// * neck: a short cylinder under it, and two lid shells round the eyes
//   with an almond opening, all `skin`.
// * eyes: two spheres (r 0.012) sitting in the sockets behind the lids,
//   looking along +z.
//   Azimuthal UVs, the layout the eye shader reads: uv (0.5, 0.5) is the
//   pupil axis and the distance from it is the angle from the axis / 2 pi
//   (no seam: only the hidden back pole is degenerate). Material `eye`.
// * hair: two caps over the crown (0.004 and 0.014 m off the skin, the
//   outer one flaring a little towards its tips), 32 locks round each: u =
//   longitude x 32 / 2 pi (the outer layer offset half a lock), v from the
//   crown (0, root) to the hairline (1, tip): low on the forehead, down to
//   the nape at the back. Double-sided, material `hair`.
// * collar: a ruff of fur round the neck, two flared rings of 48 locks, v
//   from the neck (root) outwards (tip). Double-sided, material `fur`.
// Normals are the analytic surface normals (finite differences of each
// part's position function), so seams carry no crease.

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

// ---- the head's shape -------------------------------------------------------
const RX = 0.09, RY = 0.115, RZ = 0.1;
export const EYE_R = 0.012;
export const EYE_X = 0.033, EYE_Y = 0.022;
/** Unit direction for polar angle `t` (from +y) and longitude `p` (from +z
 * towards +x). */
const dirOf = (t, p) => [Math.sin(t) * Math.sin(p), Math.cos(t), Math.sin(t) * Math.cos(p)];
/** The plain ellipsoid point along `d`. */
const ellipsoid = (d) => {
  const k = 1 / Math.hypot(d[0] / RX, d[1] / RY, d[2] / RZ);
  return scale(d, k);
};
/** How far the face features push the surface out along `d` at the
 * ellipsoid point `e` (front only). */
const features = (e, d) => {
  const [x, y, z] = e;
  if (z <= 0) return 0;
  const front = Math.min(1, z / 0.04);
  const ax = Math.abs(x);
  const nose = 0.02 * gauss(x, 0.011) * gauss(y + 0.012, 0.024) * (y < -0.036 ? gauss(y + 0.036, 0.008) : 1);
  const brow = 0.005 * gauss(y - 0.042, 0.009) * gauss(x, 0.055);
  const cheek = 0.004 * gauss(ax - 0.04, 0.02) * gauss(y + 0.01, 0.015);
  return (nose + brow + cheek) * front + socket(e, d);
};
const smooth = (a, b, x) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};
/** The plain ellipsoid's z at (x, y) (front half). */
const surfaceZ = (x, y) => RZ * Math.sqrt(1 - (x / RX) ** 2 - (y / RY) ** 2);
/** The z of the flat front the eyes sit in (the plain surface at an eye's
 * centre). */
const FRONT_Z = surfaceZ(EYE_X, EYE_Y);
/** The eye socket, as a radial offset at ellipsoid point `e` along `d`:
 * round each eye the face is flattened onto the plane z = FRONT_Z (so both
 * corners of an eye sit at the same depth; the round head would bury the
 * inner one and bare the outer one) and pressed into a bowl, 11 mm deep at
 * the eye's centre, narrower up and down than across. Inside the eye
 * opening the bowl is well behind the eyeball; past it, the skin rises over
 * the lid shells (`lids` below) so they tuck under it, about 4 mm out from
 * the opening. Blends back to the plain head from 13 to 26 mm out. */
const socket = (e, d) => {
  const [x, y, z] = e;
  if (z <= 0) return 0;
  const dx = Math.abs(x) - EYE_X;
  const dy = y - EYE_Y;
  const re = Math.hypot(dx, dy / (dy > 0 ? 0.6 : 0.45));
  const w = 1 - smooth(0.013, 0.026, re);
  if (w <= 0) return 0;
  const target = FRONT_Z - 0.011 * gauss(re, 0.014);
  // A soft min(0, .) with a 1.5 mm knee: the bowl's rim, where the target
  // meets the plain head, carries no crease (the lid shells lie on it).
  const v = target - z, k = 0.0015;
  const h = Math.max(k - Math.abs(v), 0) / k;
  return (w * (Math.min(0, v) - (h * h * k) / 4)) / Math.max(d[2], 0.3);
};
const headAt = (t, p, off = 0) => {
  const d = dirOf(t, p);
  const e = ellipsoid(d);
  return add(e, scale(d, features(e, d) + off));
};

// ---- a grid surface ---------------------------------------------------------
/** A (rows + 1) x (cols + 1) grid of `pos(a, b)` for a, b in 0..1, with
 * UVs from `uvOf(a, b)`. Normals are the surface normal, turned to agree
 * with `outward(a, b)` (any vector on the outer side); triangles wind
 * counter-clockwise seen from that side, and ones with no area are
 * dropped (the poles). */
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
/** Several grids as one primitive. */
const merge = (...ms) => {
  const out = { pos: [], nrm: [], uv: [], idx: [] };
  for (const m of ms) {
    const base = out.pos.length / 3;
    out.pos.push(...m.pos);
    out.nrm.push(...m.nrm);
    out.uv.push(...m.uv);
    out.idx.push(...m.idx.map((k) => k + base));
  }
  return out;
};

// Head: a from the crown (0) to under the chin (1), b round from the back.
// Both are warped so the rows and columns bunch up (to about 0.4 of the even
// spacing, under 2 mm apart) round the eyes and spread out at the back and
// under the chin, where nobody looks; the ends stay put, so the seam closes.
// The UVs follow the warped angles, so the pores keep one size.
const warp = (t, at, k) => t - (k / TAU) * (Math.sin(TAU * (t - at)) + Math.sin(TAU * at));
const EYE_A = 0.44;
const wa = (a) => warp(a, EYE_A, 0.6);
const wb = (b) => warp(b, 0.5, 0.6);
const head = grid(
  80,
  112,
  (a, b) => headAt(wa(a) * Math.PI, (wb(b) - 0.5) * TAU),
  (a, b) => [wb(b), (wa(a) * Math.PI) / TAU],
  (a, b) => dirOf(wa(a) * Math.PI, (wb(b) - 0.5) * TAU),
);
// Neck: a down from inside the head, b round.
const NECK_R = 0.04;
const neck = grid(
  6,
  48,
  (a, b) => [NECK_R * Math.sin((b - 0.5) * TAU), -0.07 - 0.1 * a, NECK_R * Math.cos((b - 0.5) * TAU) - 0.01],
  (a, b) => [b, 0.5 + 0.08 * a],
  (a, b) => [Math.sin((b - 0.5) * TAU), 0, Math.cos((b - 0.5) * TAU)],
);

// Eyes: a from the pupil axis (0) to the back pole (1), b round the axis.
// The front of the eyeball stands 0.5 mm proud of the flat front the
// socket presses the face onto; the lids cover all but an almond of it.
export const EYE_Z = FRONT_Z + 0.0005 - EYE_R;
const eye = (sx) => {
  const c = [sx * EYE_X, EYE_Y, EYE_Z];
  const d = (a, b) => [Math.sin(a * Math.PI) * Math.cos(b * TAU), Math.sin(a * Math.PI) * Math.sin(b * TAU), Math.cos(a * Math.PI)];
  return grid(
    28,
    48,
    (a, b) => add(c, scale(d(a, b), EYE_R)),
    (a, b) => [0.5 + (a / 2) * Math.cos(b * TAU), 0.5 + (a / 2) * Math.sin(b * TAU)],
    d,
  );
};
const eyes = merge(eye(1), eye(-1));

// Lids: one skin shell per eye, round the eyeball with an almond hole cut
// exactly on its grid (so the lid's edge is a smooth curve however coarse
// the head is). a runs from the lid's edge (0) out across a band 7 mm wide
// (1), b round the eye. The edge hugs the eyeball (0.3 mm off it) and the
// shell thickens to 1 mm off it within the first quarter, a rounded lid
// margin. From there it bends onto the head's own surface, never below it
// (a smooth max of the two), and lies on it from a = 0.7 out, so lid and
// face meet tangentially with the same normal: no outline where a dome
// would cut the socket's skin. It never comes closer than 0.4 mm to the
// eyeball. The opening: 21 mm across, 4.5 mm up and 4.2 mm down from the
// centre, its outer corner 0.8 mm higher than the inner one. UVs: the
// head's equirect mapping, so the pores run on.
export const LID_W = 0.0105, LID_UP = 0.0045, LID_DOWN = 0.0042, LID_BAND = 0.007;
/** The head's z at (x, y) on the front (bisection along z of the radial
 * surface: outside, a point is farther out than the surface along its own
 * direction). */
const headZ = (x, y) => {
  let lo = FRONT_Z - 0.03, hi = FRONT_Z + 0.02;
  for (let i = 0; i < 34; i++) {
    const z = 0.5 * (lo + hi);
    const q = [x, y, z];
    const d = norm(q);
    const e = ellipsoid(d);
    const out = Math.hypot(...q) > Math.hypot(...e) + features(e, d);
    if (out) hi = z;
    else lo = z;
  }
  return 0.5 * (lo + hi);
};
/** A smooth max with a knee of `k` (0.5 mm), so the joins add no crease. */
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
  // x here runs out towards the temple (cos s = 1 is the outer corner).
  const at = (a, b) => {
    const s = b * TAU;
    const [ex, ey] = edge(b);
    const px = ex + LID_BAND * Math.cos(s) * a;
    const py = ey + LID_BAND * Math.sin(s) * a;
    const r = EYE_R + 0.0003 + 0.0007 * smooth(0, 0.25, a);
    const rho2 = px * px + py * py;
    // The sphere, carried on along its tangent past 0.8 r instead of
    // turning down its steep side.
    const rho = Math.sqrt(rho2), r0 = 0.8 * r, z0 = Math.sqrt(r * r - r0 * r0);
    const zs = rho < r0 ? Math.sqrt(r * r - rho2) : z0 - (r0 / z0) * (rho - r0);
    const hz = headZ(c[0] + sx * px, c[1] + py) - c[2] - 0.00005;
    const t = smooth(0.12, 0.7, a);
    const lid = smax(zs, hz, 0.002);
    // Clear of the eyeball by 0.4 mm over it; past its edge the floor falls
    // away, so it never lifts the lid off the skin round the temple.
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
    // Outward: the grid's own front, (out from the eye) x (round the eye),
    // mirrored for the left eye; no guess from a direction, since the shell
    // faces up, down and sideways where it lies in the socket.
    (a, b) => {
      const h = 1e-4, a0 = Math.min(Math.max(a, h), 1 - h);
      const da = sub(at((a0 + h) ** 1.5, b), at((a0 - h) ** 1.5, b));
      const db = sub(at(a0 ** 1.5, b + h), at(a0 ** 1.5, b - h));
      return scale(cross(da, db), sx);
    },
  );
};

// Hair: a from the crown (root) to the hairline (tip), b round from the
// back. The hairline's polar angle by longitude: forehead, temples, nape.
export const LOCKS = 32;
const hairline = (p) => {
  const c = Math.cos(p); // 1 front, -1 back
  const deg = c > 0 ? 92 - 40 * c * c : 92 + 34 * c * c;
  return (deg * Math.PI) / 180;
};
const hairLayer = (layer) =>
  grid(
    24,
    LOCKS * 4,
    (a, b) => {
      const p = (b - 0.5) * TAU;
      const reach = layer ? 0.96 : 1;
      // The outer layer stands 10 mm over the inner one (hair has volume):
      // the baked sun of Lite and Low starts its rays two cells (about 9 mm
      // on this head) off a surface, so with 4 mm between them the inner
      // layer and the base cap under it read sunlit where the shadow map
      // draws them shaded (20 points apart, model_wgsl
      // baked_sun_draws_no_smear_on_any_model).
      return headAt(a * hairline(p) * reach, p, 0.004 + 0.01 * layer + 0.002 * layer * a * a);
    },
    (a, b) => [b * LOCKS + 0.5 * layer, a],
    (a, b) => dirOf(a * hairline((b - 0.5) * TAU), (b - 0.5) * TAU),
  );
const hair = merge(hairLayer(0), hairLayer(1));
// The base cap under the locks, as game hair has: opaque, 2 mm off the skin,
// stopping a little short of the hairline so the locks' tapered tips make
// the hairline soft. Material `hair_base`, untagged (the plain lit pass), so
// the scalp never shows between the locks.
const hairBase = grid(
  24,
  LOCKS * 4,
  (a, b) => {
    const p = (b - 0.5) * TAU;
    return headAt(a * hairline(p) * 0.93, p, 0.002);
  },
  (a, b) => [b * LOCKS, a],
  (a, b) => dirOf(a * hairline((b - 0.5) * TAU) * 0.93, (b - 0.5) * TAU),
);

// Collar: a ruff round the neck, a from the neck (root) out (tip).
export const COLLAR_LOCKS = 48;
const collarLayer = (layer) =>
  grid(
    6,
    COLLAR_LOCKS * 3,
    (a, b) => {
      const p = (b - 0.5) * TAU;
      const r = NECK_R + 0.004 + 0.05 * a;
      return [r * Math.sin(p), -0.128 - 0.032 * a * a + 0.005 * layer, r * Math.cos(p) - 0.01];
    },
    (a, b) => [b * COLLAR_LOCKS + 0.5 * layer, a],
    () => [0, 1, 0],
  );
const collar = merge(collarLayer(0), collarLayer(1));

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
// Float values are rounded to f32 once, so min/max match the stored data.
const acc = (arr, type, comps, minmax = false) => {
  const isIdx = type === 'SCALAR';
  const f = isIdx ? null : new Float32Array(arr);
  const buf = isIdx ? Buffer.from(new Uint16Array(arr).buffer) : Buffer.from(f.buffer);
  const view = pushView(buf, isIdx ? 34963 : 34962);
  const a = { bufferView: view, componentType: isIdx ? 5123 : 5126, count: arr.length / comps, type };
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
  if (m.pos.length / 3 > 65535) throw new Error('too many vertices for u16 indices');
  return {
    attributes: { POSITION: acc(m.pos, 'VEC3', 3, true), NORMAL: acc(m.nrm, 'VEC3', 3), TEXCOORD_0: acc(m.uv, 'VEC2', 2) },
    indices: acc(m.idx, 'SCALAR', 1),
    material,
  };
};
/** sRGB 0..255 to linear 0..1. */
const lin = (c) => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};
const rgb = (r, g, b) => [lin(r), lin(g), lin(b), 1].map((v) => Math.round(v * 1e4) / 1e4);

const parts = [
  ['head', merge(head, neck, lids(1), lids(-1)), 0],
  ['eyes', eyes, 1],
  ['hair', hair, 2],
  ['collar', collar, 3],
  ['hair_base', hairBase, 4],
];
const gltf = {
  asset: { version: '2.0', generator: 'Evani.Dev char-head.mjs (character materials fixture)' },
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
      pbrMetallicRoughness: { baseColorFactor: rgb(96, 62, 40), metallicFactor: 0, roughnessFactor: 0.5 },
      doubleSided: true,
      extras: { evani_kind: 'hair' },
    },
    {
      name: 'fur',
      pbrMetallicRoughness: { baseColorFactor: rgb(196, 168, 128), metallicFactor: 0, roughnessFactor: 0.6 },
      doubleSided: true,
      extras: { evani_kind: 'fur', evani_hair: { tip_width: 0.35, edge_softness: 0.4 } },
    },
    {
      name: 'hair_base',
      pbrMetallicRoughness: { baseColorFactor: rgb(66, 42, 27), metallicFactor: 0, roughnessFactor: 0.7 },
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
const tris = parts.map(([n, m]) => `${n} ${m.idx.length / 3}`).join(', ');
console.log(`${out}: ${12 + 16 + json.length + binPadded.length} bytes (triangles: ${tris}); eye centre z ${EYE_Z.toFixed(4)}`);
