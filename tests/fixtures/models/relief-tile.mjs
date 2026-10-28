// Writes relief-tile.glb, the S3-10 surface-relief fixture: our own small
// model with a material `extras.evani.relief` (docs/MODEL_FORMAT.md,
// "Surface relief"). Node built-ins only (zlib for the PNGs); the output is
// the same bytes on every run.
//
//   node tests/fixtures/models/relief-tile.mjs tests/fixtures/models/relief-tile.glb
//
// Two parts, metres, +z towards the viewer:
// * floor: a 2 x 0.1 x 2 slab, top at y 0.1. Material `plate`: 4 x 4 raised
//   panels with bevelled grooves and a rivet in each, a height map
//   (parallax, depth 0.03 UV) and the normal map made from the same height,
//   plus a `knurl` micro pattern on the panel tops only (mask: base alpha).
// * wall: a 2 x 2 x 0.1 plate standing on the slab's back edge, facing +z,
//   in three 2/3 m columns (one primitive each, the map running on across
//   them). Materials `panel` (ribs), `dots` and `grain`: the same normal
//   map, no height map, and that micro pattern everywhere (no mask): the
//   micro-only path, so every pattern of the toon look shows in one view.
// Tangent space: tangent along +u, bitangent b = cross(n, t) * w along +v
// (down the image), and the normal map's green is +v, so the engine's
// shader and this file agree without any flip.

import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const N = 128; // texture size
const PANELS = 4;
const DEPTH = 0.03; // height map depth, UV units

// ---- PNG -------------------------------------------------------------------
const CRC = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (b) => {
  let c = 0xffffffff;
  for (const x of b) c = CRC[(c ^ x) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'latin1');
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
};
/** RGBA8 PNG from `px(x, y) -> [r, g, b, a]` (0..255). */
const png = (px) => {
  const raw = Buffer.alloc(N * (N * 4 + 1));
  for (let y = 0; y < N; y++) {
    raw[y * (N * 4 + 1)] = 0;
    for (let x = 0; x < N; x++) {
      const c = px(x, y);
      for (let k = 0; k < 4; k++) raw[y * (N * 4 + 1) + 1 + x * 4 + k] = Math.max(0, Math.min(255, Math.round(c[k])));
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(N, 0);
  ihdr.writeUInt32BE(N, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
};

// ---- the height field (0 groove floor .. 1 rivet top) ------------------------
const cell = N / PANELS;
const smooth = (e0, e1, x) => {
  const t = Math.max(0, Math.min(1, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};
/** Height at texel centre (x, y), wrapping. */
const height = (x, y) => {
  const xx = (((x % N) + N) % N) + 0.5, yy = (((y % N) + N) % N) + 0.5;
  const fx = xx % cell, fy = yy % cell;
  const edge = Math.min(fx, cell - fx, fy, cell - fy); // texels to the panel's edge
  const panel = 0.8 * smooth(2, 6, edge);
  const r = Math.hypot(fx - cell / 2, fy - cell / 2);
  const rivet = r < 4 ? 0.2 * Math.sqrt(1 - (r / 4) ** 2) : 0;
  return panel + rivet;
};
const heightPng = png((x, y) => {
  const h = height(x, y) * 255;
  return [h, h, h, 255];
});
// Normal from the same height: slope in UV units is DEPTH * dh/du.
const normalPng = png((x, y) => {
  const du = ((height(x + 1, y) - height(x - 1, y)) / 2) * N * DEPTH;
  const dv = ((height(x, y + 1) - height(x, y - 1)) / 2) * N * DEPTH;
  const l = Math.hypot(du, dv, 1);
  return [(-du / l) * 127.5 + 127.5, (-dv / l) * 127.5 + 127.5, (1 / l) * 127.5 + 127.5, 255];
});
// Base colours (sRGB): mint panels with darker grooves, light rivets; the
// alpha is 1 on the panel tops (the knurl's mask) and 0 elsewhere.
const plateBase = png((x, y) => {
  const h = height(x, y);
  const top = h >= 0.79 && h <= 0.81 ? 255 : 0;
  if (h > 0.81) return [214, 236, 228, 0];
  const k = 0.45 + 0.55 * (h / 0.8);
  return [70 * k + 20, 190 * k + 20, 160 * k + 20, top];
});
const panelBase = png((x, y) => {
  const k = 0.5 + 0.5 * (height(x, y) / 0.8);
  return [235 * k + 10, 110 * k + 10, 120 * k + 10, 255];
});

// ---- geometry ----------------------------------------------------------------
const sub = (a, b) => a.map((v, i) => v - b[i]);
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a, b) => a.reduce((s, v, i) => s + v * b[i], 0);
/** A box's faces as quads: [normal, tangent with w, corners (CCW seen from
 * outside)]. u runs from corner 0 to 1 (the tangent), v from corner 3 to 0;
 * w is chosen so b = cross(n, t) * w runs along +v. */
const box = (lo, hi) => {
  const [x0, y0, z0] = lo, [x1, y1, z1] = hi;
  return [
    [[0, 1, 0], [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]]],
    [[0, 0, 1], [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]],
    [[0, 0, -1], [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]]],
    [[1, 0, 0], [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]]],
    [[-1, 0, 0], [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]]],
    [[0, -1, 0], [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]],
  ].map(([n, c]) => {
    const e = sub(c[1], c[0]);
    const t = e.map((v) => v / Math.hypot(...e));
    const w = dot(cross(n, t), sub(c[0], c[3])) > 0 ? 1 : -1;
    return [n, [...t, w], c];
  });
};
// UVs per corner: the big face gets the map from u0 to u1 (all of it by
// default), the thin sides a strip.
const mesh = (faces, bigNormal, u0 = 0, u1 = 1) => {
  const pos = [], nrm = [], tan = [], uv = [], idx = [];
  for (const [n, t, c] of faces) {
    const big = n.every((v, i) => v === bigNormal[i]);
    const s = big ? 1 : 0.05;
    // corners in order: (u0,v1) (u1,v1) (u1,v0) (u0,v0): v grows towards corner 0
    const uvs = [[u0, s], [u1, s], [u1, 0], [u0, 0]];
    const base = pos.length / 3;
    c.forEach((p, i) => {
      pos.push(...p);
      nrm.push(...n);
      tan.push(...t);
      uv.push(...uvs[i]);
    });
    idx.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }
  return { pos, nrm, tan, uv, idx };
};
// Check the tangent frame of every face: b = cross(n, t) * w must point
// along +v (from corner 3 to corner 0) and t along +u (corner 0 to 1).
for (const [n, t, c] of box([0, 0, 0], [1, 1, 1])) {
  const b = cross(n, t.slice(0, 3)).map((v) => v * t[3]);
  if (dot(sub(c[1], c[0]), t.slice(0, 3)) <= 0 || dot(sub(c[0], c[3]), b) <= 0) throw new Error(`tangent frame of face ${n}`);
  if (dot(cross(sub(c[1], c[0]), sub(c[2], c[0])), n) <= 0) throw new Error(`winding of face ${n}`);
}
const floor = mesh(box([-1, 0, -1], [1, 0.1, 1]), [0, 1, 0]);
// Three columns; the faces between two columns are left out (hidden, and
// coplanar pairs would only z-fight).
const COLS = 3;
const walls = [...Array(COLS).keys()].map((i) => {
  const x0 = -1 + (2 * i) / COLS, x1 = -1 + (2 * (i + 1)) / COLS;
  const faces = box([x0, 0.1, -1.1], [x1, 2.1, -1.0]).filter(([n]) => !(n[0] === -1 && i > 0) && !(n[0] === 1 && i < COLS - 1));
  return mesh(faces, [0, 0, 1], i / COLS, (i + 1) / COLS);
});

// ---- glTF --------------------------------------------------------------------
const bin = [];
let binLen = 0;
const views = [];
const accessors = [];
const pushView = (buf, target) => {
  const pad = (4 - (binLen % 4)) % 4;
  if (pad) { bin.push(Buffer.alloc(pad)); binLen += pad; }
  views.push({ buffer: 0, byteOffset: binLen, byteLength: buf.length, ...(target ? { target } : {}) });
  bin.push(buf);
  binLen += buf.length;
  return views.length - 1;
};
const acc = (arr, type, comps, target, extra = {}) => {
  const isIdx = type === 'SCALAR';
  const buf = isIdx ? Buffer.from(new Uint16Array(arr).buffer) : Buffer.from(new Float32Array(arr).buffer);
  const view = pushView(buf, target);
  const a = { bufferView: view, componentType: isIdx ? 5123 : 5126, count: arr.length / comps, type, ...extra };
  if (type === 'VEC3' && extra.minmax) {
    delete a.minmax;
    const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < arr.length; i += 3) for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k], arr[i + k]); max[k] = Math.max(max[k], arr[i + k]); }
    a.min = min; a.max = max;
  }
  accessors.push(a);
  return accessors.length - 1;
};
const prim = (m, material) => ({
  attributes: {
    POSITION: acc(m.pos, 'VEC3', 3, 34962, { minmax: true }),
    NORMAL: acc(m.nrm, 'VEC3', 3, 34962),
    TANGENT: acc(m.tan, 'VEC4', 4, 34962),
    TEXCOORD_0: acc(m.uv, 'VEC2', 2, 34962),
  },
  indices: acc(m.idx, 'SCALAR', 1, 34963),
  material,
});
const meshes = [{ primitives: [prim(floor, 0)] }, { primitives: walls.map((m, i) => prim(m, 1 + i)) }];
const images = [plateBase, normalPng, heightPng, panelBase].map((b) => ({ bufferView: pushView(b), mimeType: 'image/png' }));
const gltf = {
  asset: { version: '2.0', generator: 'Evani.Dev relief-tile.mjs (S3-10 fixture)' },
  extras: { evani: { format: 1, class: 'relief_tile', parts: [{ name: 'floor', role: 'body', solid: 'floor' }, { name: 'wall', role: 'body', solid: 'wall' }] } },
  scene: 0,
  scenes: [{ nodes: [0] }],
  nodes: [
    { name: 'relief_tile', children: [1, 2] },
    { name: 'floor', mesh: 0, extras: { evani: { kind: 'solid', origin: [0, 0, 0], mirrored: false } } },
    { name: 'wall', mesh: 1, extras: { evani: { kind: 'solid', origin: [0, 0, 0], mirrored: false } } },
  ],
  meshes,
  materials: [
    {
      name: 'plate',
      pbrMetallicRoughness: { baseColorTexture: { index: 0 }, metallicFactor: 0, roughnessFactor: 0.8 },
      normalTexture: { index: 1 },
      extras: { evani: { relief: { height: { index: 2, depth: DEPTH }, micro: { pattern: 'knurl', size: 0.025, depth: 0.6, angle: 0, mask: { texture: 'base', channel: 'a' } } } } },
    },
    {
      name: 'panel',
      pbrMetallicRoughness: { baseColorTexture: { index: 3 }, metallicFactor: 0, roughnessFactor: 0.8 },
      normalTexture: { index: 1 },
      extras: { evani: { relief: { micro: { pattern: 'ribs', size: 0.02, depth: 0.5, angle: 90 } } } },
    },
    {
      name: 'dots',
      pbrMetallicRoughness: { baseColorTexture: { index: 3 }, metallicFactor: 0, roughnessFactor: 0.8 },
      normalTexture: { index: 1 },
      extras: { evani: { relief: { micro: { pattern: 'dots', size: 0.03, depth: 0.7, angle: 0 } } } },
    },
    {
      name: 'grain',
      pbrMetallicRoughness: { baseColorTexture: { index: 3 }, metallicFactor: 0, roughnessFactor: 0.8 },
      normalTexture: { index: 1 },
      extras: { evani: { relief: { micro: { pattern: 'grain', size: 0.04, depth: 0.6, angle: 0 } } } },
    },
  ],
  textures: [0, 1, 2, 3].map((i) => ({ sampler: 0, source: i })),
  samplers: [{ magFilter: 9729, minFilter: 9987, wrapS: 10497, wrapT: 10497 }],
  images,
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
const out = process.argv[2] ?? 'relief-tile.glb';
writeFileSync(out, Buffer.concat([header, jh, json, bh, binPadded]));
console.log(`${out}: ${12 + 16 + json.length + binPadded.length} bytes`);
