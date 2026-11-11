// Builds p10b-source.glb: eight uv spheres side by side, one named material each
// (no textures, no catalogue data). `cargo xtask convert ... --materials map.json`
// then fills the materials from the catalogue.
import fs from 'node:fs';

const names = ['oak', 'steel', 'lacquer', 'granite', 'horse', 'saddle', 'ruby', 'velvet'];
const ids = {
  oak: 'timber.oak',
  steel: 'metal.steel.brushed',
  lacquer: 'finish.lacquer.red',
  granite: 'rock.granite.grey',
  horse: 'horse.coat.bay',
  saddle: 'leather.saddle_tan',
  ruby: 'gem.ruby',
  velvet: 'cloth.velvet.red',
};
const rings = 20, segs = 32;
const pos = [], nor = [], uv = [], tan = [], idx = [];
for (let r = 0; r <= rings; r++) {
  const th = Math.PI * r / rings;
  for (let s = 0; s <= segs; s++) {
    const ph = 2 * Math.PI * s / segs;
    const n = [Math.sin(th) * Math.cos(ph), Math.cos(th), Math.sin(th) * Math.sin(ph)];
    pos.push(n[0] * 0.5, n[1] * 0.5, n[2] * 0.5);
    nor.push(...n);
    uv.push(s / segs, r / rings);
    // tangent along increasing phi (the u direction)
    tan.push(-Math.sin(ph), 0, Math.cos(ph), 1);
  }
}
for (let r = 0; r < rings; r++) {
  for (let s = 0; s < segs; s++) {
    const a = r * (segs + 1) + s, b = a + segs + 1;
    idx.push(a, a + 1, b, a + 1, b + 1, b);
  }
}
const f32 = (a) => Buffer.from(new Float32Array(a).buffer);
const u16 = (a) => Buffer.from(new Uint16Array(a).buffer);
const parts = [f32(pos), f32(nor), f32(uv), f32(tan), u16(idx)];
const pad = (b) => (b.length % 4 ? Buffer.concat([b, Buffer.alloc(4 - (b.length % 4))]) : b);
let off = 0;
const views = parts.map((p, i) => {
  const v = { buffer: 0, byteOffset: off, byteLength: p.length, target: i === 4 ? 34963 : 34962 };
  off += pad(p).length;
  return v;
});
const bin = Buffer.concat(parts.map(pad));
const n = pos.length / 3;
const accessors = [
  { bufferView: 0, componentType: 5126, count: n, type: 'VEC3', min: [-0.5, -0.5, -0.5], max: [0.5, 0.5, 0.5] },
  { bufferView: 1, componentType: 5126, count: n, type: 'VEC3' },
  { bufferView: 2, componentType: 5126, count: n, type: 'VEC2' },
  { bufferView: 3, componentType: 5126, count: n, type: 'VEC4' },
  { bufferView: 4, componentType: 5123, count: idx.length, type: 'SCALAR' },
];
const meshes = names.map((nm, k) => ({ name: nm, primitives: [{ attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2, TANGENT: 3 }, indices: 4, material: k }] }));
const nodes = names.map((nm, k) => ({ name: nm, mesh: k, translation: [(k - 3.5) * 1.24, 0.5, 0] }));
const doc = {
  asset: { version: '2.0', generator: 'p10b proof source' },
  extras: { evani: { format: 1, class: 'p10b', min_distance_m: 20 } },
  scene: 0,
  scenes: [{ nodes: nodes.map((_, i) => i) }],
  nodes, meshes,
  materials: names.map((nm) => ({ name: nm, pbrMetallicRoughness: { baseColorFactor: [0.8, 0.8, 0.8, 1], metallicFactor: 0, roughnessFactor: 0.6 } })),
  accessors, bufferViews: views, buffers: [{ byteLength: bin.length }],
};
let js = Buffer.from(JSON.stringify(doc));
js = js.length % 4 ? Buffer.concat([js, Buffer.alloc(4 - (js.length % 4), 0x20)]) : js;
const total = 12 + 8 + js.length + 8 + bin.length;
const h = Buffer.alloc(12); h.write('glTF', 0); h.writeUInt32LE(2, 4); h.writeUInt32LE(total, 8);
const jh = Buffer.alloc(8); jh.writeUInt32LE(js.length, 0); jh.write('JSON', 4);
const bh = Buffer.alloc(8); bh.writeUInt32LE(bin.length, 0); bh.write('BIN\0', 4);
const out = process.argv[2];
fs.writeFileSync(out + '/p10b-source.glb', Buffer.concat([h, jh, js, bh, bin]));
fs.writeFileSync(out + '/p10b-map.json', JSON.stringify(ids, null, 2));
console.log('wrote', out);
