// The spectral sea's GPU harness (evani-material-nature, part
// `spectral-sea`): draws the sea shader exactly as the engine assembles it,
// with the uniform words the engine packs (both dumped by a test), over a
// small stand-in scene (a sky with sun and clouds, a sea bed or a beach,
// rocks, all with depth), tone-maps and saves pictures; checks that no
// trough ever shows the bed ("bottoming out"); times each quality level.
//
//   1. cargo test -p evani-material-nature --features spectral-sea --lib dump_for_the_gpu_harness
//        with EVANI_SEA_DUMP=<dir>
//   2. node tools/nature/sea-shots.mjs --dump <dir> --out <dir>
//        [--gpu] (default: WebGPU on SwiftShader, the CPU; no graphics card)
//        [--shots calm,rough] [--levels 0,1,2,3] [--size 1600x900]
//        [--reps 5] [--timeout-s 600] [--no-worst] [--no-timing] [--no-rim]
//
// Writes <out>/<shot>-<level>.png, <out>/worst-*.png (only when a hole
// shows), <out>/rim-*.png (only when a rim shows) and <out>/report.json;
// exits 1 when a hole or a rim shows or a shader fails.
// The bottoming-out check: opaque water over a magenta bed 2 m down in the
// worst storm the settings allow, many views and times; any magenta pixel
// in the result is a hole. The rim check: every deep-water view with no bed
// behind the water (only sky, as in the studio and most open-sea games),
// drawn with and without the waves' bending; a pixel much brighter only
// when bent is a refraction leak (the sky past a rock's edge seen through
// the water undarkened). Headless, muted, a fresh full-path profile,
// pointer and keyboard locks stubbed, Chrome closed by its own PID.
import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startChrome, STUBS } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const dump = arg("dump", "");
const out = arg("out", ".evani/shots/sea");
if (!dump) {
  console.error("--dump <dir> (from the dump test) is needed");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const scenes = JSON.parse(readFileSync(join(dump, "scenes.json"), "utf8"));
const SEA = readFileSync(join(dump, "sea.wgsl"), "utf8");
// The field sea (src/sea/field.rs): its wave textures, mesh and fill, from
// the dump when it has them, else assembled from this checkout's shader
// files exactly as src/sea/mod.rs does.
const SH = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "crates", "evani-material-nature", "shaders");
const shf = (n) => readFileSync(join(SH, n), "utf8");
const fromDump = (n, make) => (existsSync(join(dump, n)) ? readFileSync(join(dump, n), "utf8") : existsSync(SH) ? make() : "");
const WAVES = fromDump("waves.wgsl", () => shf("sea_waves.wgsl") + "\n" + shf("sea_field_bands.wgsl") + "\n" + shf("sea_field_table.wgsl"));
// (With the dev builds' foam view: --foam-debug turns it on.)
const MESH = fromDump("field.wgsl", () => shf("sea_field.wgsl") + "\n" + shf("sea_common.wgsl") + "\n" + shf("sea_field_bands.wgsl") + "\n" + shf("sea_field_break.wgsl") + "\n" + shf("sea_field_debug.wgsl") + "\n" + shf("sea_table.wgsl"));
const FILL = fromDump("fill.wgsl", () => shf("sea_fill.wgsl") + "\n" + shf("sea_vs.wgsl") + shf("sea_common.wgsl") + "\n" + shf("sea_table.wgsl"));
const SPRAY = fromDump("spray.wgsl", () => shf("sea_spray.wgsl") + "\n" + shf("sea_common.wgsl") + "\n" + shf("sea_field_bands.wgsl") + "\n" + shf("sea_field_break.wgsl") + "\n" + shf("sea_table.wgsl"));
// The mesh per level (rings, segments) from the shader's own table.
const GRID = (() => {
  const m = MESH.match(/array<vec2<u32>, 4>\(([^;]*)\);/);
  return m ? [...m[1].matchAll(/vec2<u32>\((\d+)u, (\d+)u\)/g)].map((x) => 6 * (Number(x[1]) - 1) * Number(x[2])) : [0, 0, 0, 0];
})();
const want = arg("shots", "");
const shots = scenes.shots.filter((s) => !want || want.split(",").includes(s.name));
const levels = arg("levels", "0,1,2,3").split(",").map(Number);
// The old water (the nature pass) of each shot's "before", spliced per preset.
const OLD = {};
for (const s of shots) if (s.old && !OLD[s.old]) OLD[s.old] = readFileSync(join(dump, `old-${s.old}.wgsl`), "utf8");
const before = has("before");
const [W, H] = arg("size", "1600x900").split("x").map(Number);
// The uniform word of a field of the shader's `struct Sea` (WGSL uniform
// layout: vec3 and vec4 start on 4 words, vec2 on 2).
function seaWord(field, comp) {
  const at = SEA.indexOf("struct Sea {");
  const body = SEA.slice(at, SEA.indexOf("}", at));
  const sizes = { f32: [1, 1], "vec2<f32>": [2, 2], "vec3<f32>": [3, 4], "vec4<f32>": [4, 4] };
  let off = 0;
  for (const m of body.matchAll(/^\s*(\w+)\s*:\s*(f32|vec[234]<f32>)\s*,/gm)) {
    const [size, align] = sizes[m[2]];
    off = Math.ceil(off / align) * align;
    if (m[1] === field) return off + comp;
    off += size;
  }
  throw new Error(`struct Sea has no field ${field}`);
}
// Rim check: the deep-water views (their own cameras and rocks), no bed.
const rimShots = has("no-rim") ? [] : shots.filter((s) => s.bed === "deep").map((s) => s.name);
// --foam: each picture also drawn with the spindrift and spray off
// ("<name>.foamonly.png") and with all foam and spray off
// ("<name>.nofoam.png"), so storm-metrics.mjs can measure the foam alone
// (what the foam adds, sun glints and sky reflections the same in both).
const foamWords = { whitecaps: seaWord("foam", 0), shore: seaWord("foam", 3), spray: seaWord("spray", 0), level: seaWord("level", 0) };
// --strips <n>: the sea drawn in n horizontal strips, each its own
// submission (a heavy view, e.g. the Ultra storm from the air at 1600x900,
// in one draw outlasts what the processor's stand-in GPU allows).
// The splash per level (cells across, puffs a cell), from its own table.
const SPRAYV = (() => {
  const m = SPRAY.match(/array<vec2<u32>, 4>\(([^;]*)\);/);
  return m ? [...m[1].matchAll(/vec2<u32>\((\d+)u, (\d+)u\)/g)].map((x) => 6 * Number(x[1]) * Number(x[1]) * Number(x[2])) : [0, 0, 0, 0];
})();
const fieldSize = [Number((MESH.match(/const F_W: f32 = ([\d.]+);/) || [])[1] || 390), Number((MESH.match(/const F_H: f32 = ([\d.]+);/) || [])[1] || 1041)];
const config = { fieldStats: has("field-stats"), field: { size: fieldSize, spray: SPRAYV, sprayWord: seaWord("spray", 0), verts: GRID, levelWord: seaWord("level", 0), beachWord: seaWord("beach", 0) }, strips: Number(arg("strips", "1")), W, H, levels, reps: Number(arg("reps", "5")), worst: has("no-worst") ? [] : scenes.worst, timing: !has("no-timing"), before, rim: rimShots, refractWord: seaWord("light", 2), foam: has("foam"), fw: foamWords, foamDebug: has("foam-debug") };

// The stand-in scene: sky (gradient, sun disc, clouds) where nothing is
// hit, else the bed (flat, a beach, a reef or magenta) and a few rocks,
// lit by the sun; depth as the engine stores it (reversed:
// 1 / view distance = inv0 + inv1 z; nothing past 1000 m).
const SCENE_WGSL = SEA.slice(SEA.indexOf("struct Sea {"), SEA.indexOf("const PI:")) + `
@group(0) @binding(0) var<uniform> u: Sea;
struct Kind { k: vec4<f32> };
@group(0) @binding(1) var<uniform> kind: Kind;
struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> O {
  let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  var o: O; o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0); o.uv = vec2<f32>(p.x, 1.0 - p.y); return o;
}
fn col(p: f32) -> vec3<f32> {
  let q = u32(max(p, 0.0) + 0.5);
  let v = vec3<f32>(f32(q & 255u), f32((q >> 8u) & 255u), f32((q >> 16u) & 255u)) / 255.0;
  return v * v;
}
fn h2(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453); }
fn n2(p: vec2<f32>) -> f32 {
  let i = floor(p); let f = fract(p); let w = f * f * (3.0 - 2.0 * f);
  return mix(mix(h2(i), h2(i + vec2<f32>(1.0, 0.0)), w.x), mix(h2(i + vec2<f32>(0.0, 1.0)), h2(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}
fn fbm(p: vec2<f32>) -> f32 { var s = 0.0; var a = 0.5; var q = p; for (var i = 0; i < 5; i++) { s += a * n2(q); q = q * 2.03 + 1.7; a *= 0.5; } return s; }
fn sky(d: vec3<f32>) -> vec3<f32> {
  let base = mix(col(u.cols.x), col(u.cols.y), sqrt(clamp(d.y, 0.0, 1.0)));
  var c = base;
  if (d.y > 0.0) {
    let p = d.xz / d.y * 2.0;
    // A storm preset (sun glint under 1: the sun behind storm cloud) gets
    // more, greyer cloud; the others are as before.
    let g = clamp(u.light.x, 0.0, 1.0);
    if (g < 1.0) {
      // A storm (sun glint under 1: the sun behind storm cloud): a broken,
      // dark storm deck with slate-grey bases and lighter, warmer gaps
      // where the sun is behind it, streaked along the wind, over a paler
      // haze at the horizon (lighter than the sea, so far crests stand out
      // against it); no sun disc.
      let sd0 = max(dot(d, u.sun), 0.0);
      let q = vec2<f32>(p.x * 0.8 + p.y * 0.35, p.y * 1.3 - p.x * 0.2);
      let n1 = fbm(q * 0.16 + vec2<f32>(3.0, 1.0));
      let n2 = fbm(q * 0.55 + vec2<f32>(7.0, 2.0));
      let n3 = fbm(q * 2.1 + vec2<f32>(1.0, 9.0));
      let dens = smoothstep(0.32, 0.72, n1 * 0.65 + n2 * 0.35);
      let lit = vec3<f32>(0.66, 0.63, 0.58) * (0.8 + 0.7 * pow(sd0, 5.0));
      let base = vec3<f32>(0.20, 0.22, 0.26) * (0.9 + 0.4 * pow(sd0, 3.0));
      var cl = mix(lit, base, dens * 0.92) * (0.88 + 0.24 * n3);
      // Overhead the deck is darkest; low down it fades into the haze.
      cl = cl * (1.0 - 0.25 * smoothstep(0.15, 0.6, d.y));
      let hz = vec3<f32>(0.60, 0.58, 0.54) * (0.88 + 0.45 * pow(sd0, 4.0));
      return mix(hz, cl, smoothstep(0.0, 0.12, d.y));
    }
    let cover = 0.3 * (1.0 - g);
    let cl = smoothstep(0.52 - cover, 0.78 - cover, fbm(p * 0.35 + vec2<f32>(3.0, 1.0)));
    c = mix(c, vec3<f32>(0.95, 0.95, 0.97) * (0.75 + 0.35 * max(u.sun.y, 0.0)) * mix(0.62, 1.0, g), cl * smoothstep(0.0, 0.08, d.y) * 0.85);
  }
  let sd = dot(d, u.sun);
  // No sun disc behind storm cloud (glint under 1).
  let disc = select(0.0, 1.0, u.light.x >= 1.0);
  c += u.sun_col * (smoothstep(0.99996, 0.99999, sd) * 30.0 * disc + (pow(max(sd, 0.0), 500.0) * 0.5 + pow(max(sd, 0.0), 40.0) * 0.25 + pow(max(sd, 0.0), 6.0) * 0.12) * clamp(u.light.x, 0.0, 1.0));
  return c;
}
fn enter(ro: vec3<f32>, rd: vec3<f32>, a: vec3<f32>, b: f32) -> f32 {
  // The ray enters the half-space dot(a, p) <= b.
  let s = dot(a, ro) - b; let v = dot(a, rd);
  if (s <= 0.0) { return 0.0; }
  if (v >= 0.0) { return 1.0e9; }
  return -s / v;
}
struct F { @builtin(frag_depth) d: f32, @location(0) c: vec4<f32> };
@fragment fn fs(i: O) -> F {
  let ndc = vec2<f32>(i.uv.x * 2.0 - 1.0, 1.0 - i.uv.y * 2.0);
  let ray = u.fwd + u.right * ndc.x + u.up * ndc.y;
  let rl = length(ray);
  let rd = ray / rl;
  let ro = u.eye;
  let lvl = u.centre.y;
  let k = i32(kind.k.x + 0.5);
  // The flat bottom, and (beach) the slope rising to the land.
  // Kind 4: no bed at all (only the sky behind the water, as in the studio
  // and most open-sea games), rocks as kind 0. Kind 5: open sea, no bed and
  // no rocks (the reference's views).
  var t = select(enter(ro, rd, vec3<f32>(0.0, 1.0, 0.0), lvl - u.shape.y), 1.0e9, k == 4 || k == 5);
  var n = vec3<f32>(0.0, 1.0, 0.0);
  let land = vec2<f32>(cos(u.beach.z), sin(u.beach.z));
  if (u.beach.x > 0.0 && k != 4 && k != 5) {
    // y <= lvl - (dist - dot(x - c, land)) slope
    let a = vec3<f32>(-land.x * u.beach.x, 1.0, -land.y * u.beach.x);
    let b = lvl - u.beach.y * u.beach.x - dot(u.centre.xz, land) * u.beach.x;
    let tb = enter(ro, rd, a, b);
    if (tb < t) { t = tb; n = normalize(a); }
  }
  // Rocks.
  var rocks = array<vec4<f32>, 3>(vec4<f32>(-30.0, -1.5, -95.0, 7.0), vec4<f32>(26.0, -3.0, -150.0, 10.0), vec4<f32>(8.0, -1.0, -38.0, 2.6));
  if (k == 1) { rocks = array<vec4<f32>, 3>(vec4<f32>(10.0, -1.2, 18.0, 3.2), vec4<f32>(-6.0, -1.5, -30.0, 4.0), vec4<f32>(60.0, 1.5, -6.0, 3.5)); }
  if (k == 2) { rocks = array<vec4<f32>, 3>(vec4<f32>(-4.0, -3.4, -14.0, 1.4), vec4<f32>(5.0, -3.2, -22.0, 1.8), vec4<f32>(-9.0, -3.5, -34.0, 2.2)); }
  var rock = false;
  if (k != 3 && k != 5) {
    for (var j = 0; j < 3; j++) {
      let s = rocks[j];
      let oc = ro - s.xyz;
      let bq = dot(oc, rd); let cq = dot(oc, oc) - s.w * s.w; let hq = bq * bq - cq;
      if (hq > 0.0) {
        let tr = -bq - sqrt(hq);
        if (tr > 0.0 && tr < t) { t = tr; n = normalize(ro + rd * tr - s.xyz); rock = true; }
      }
    }
  }
  var f: F;
  let vd = t / rl;
  if (t > 1.0e8 || vd > 1000.0) {
    f.d = 0.0;
    f.c = vec4<f32>(sky(rd), 1.0);
    return f;
  }
  f.d = (1.0 / vd - u.inv0) / u.inv1;
  let p = ro + rd * t;
  var alb = vec3<f32>(0.10, 0.11, 0.10) * (0.7 + 0.6 * fbm(p.xz * 0.3));
  if (k == 1 || k == 2) {
    // Sand: ripples and grain; reef patches.
    let rip = 0.5 + 0.5 * sin(dot(p.xz, vec2<f32>(0.9, 2.3)) + 2.0 * fbm(p.xz * 0.4));
    alb = vec3<f32>(0.62, 0.52, 0.36) * (0.82 + 0.12 * rip + 0.12 * fbm(p.xz * 3.0));
    if (k == 2) { alb = mix(alb, vec3<f32>(0.20, 0.16, 0.10), smoothstep(0.62, 0.72, fbm(p.xz * 0.18 + 5.0)) * 0.8); }
  }
  if (rock) { alb = vec3<f32>(0.11, 0.10, 0.09) * (0.6 + 0.8 * fbm(p.xz * 1.7 + p.y)); }
  if (k == 3) { alb = vec3<f32>(2.0, 0.0, 2.0); }
  let ndl = max(dot(n, u.sun), 0.0);
  let amb = mix(col(u.cols.x), col(u.cols.y), 0.5) * 0.6;
  f.c = vec4<f32>(select(alb * (amb + u.sun_col * ndl * 0.8), alb, k == 3), 1.0);
  return f;
}`;

const TONE_WGSL = `
@group(0) @binding(0) var src: texture_2d<f32>;
struct O { @builtin(position) pos: vec4<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> O {
  let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  var o: O; o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0); return o;
}
@fragment fn fs(i: O) -> @location(0) vec4<f32> {
  let c = textureLoad(src, vec2<i32>(i.pos.xy), 0).rgb * 0.85;
  // ACES (Narkowicz fit), then sRGB.
  let a = (c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14);
  let x = clamp(a, vec3<f32>(0.0), vec3<f32>(1.0));
  let s = select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
  return vec4<f32>(s, 1.0);
}`;

const page = `<!doctype html><meta charset="utf-8"><body><script>
const CFG = ${JSON.stringify(config)};
const SHOTS = ${JSON.stringify(shots)};
const SEA = ${JSON.stringify(SEA)};
const WAVES = ${JSON.stringify(WAVES)};
const MESH = ${JSON.stringify(MESH)};
const FILL = ${JSON.stringify(FILL)};
const SPRAY = ${JSON.stringify(SPRAY)};
const OLD = ${JSON.stringify(OLD)};
const SCENE_WGSL = ${JSON.stringify(SCENE_WGSL)};
const TONE_WGSL = ${JSON.stringify(TONE_WGSL)};
const BEDS = { deep: 0, sand: 1, reef: 2, magenta: 3, none: 4, open: 5 };
async function run() {
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) return { error: "no WebGPU adapter" };
  const info = adapter.info || {};
  // GPU timestamps per pass when the adapter has them (the whole frame and
  // each pass, as Ultra-Water's own numbers are measured).
  const tsOk = adapter.features.has("timestamp-query");
  const dev = await adapter.requestDevice(tsOk ? { requiredFeatures: ["timestamp-query"] } : undefined);
  let TS = null;
  const tsw = (label) => {
    if (!TS) return undefined;
    const i = TS.n;
    TS.n += 2;
    TS.labels.push(label);
    return { querySet: TS.qs, beginningOfPassWriteIndex: i, endOfPassWriteIndex: i + 1 };
  };
  const errors = [];
  dev.onuncapturederror = (e) => errors.push(String(e.error.message).slice(0, 600));
  const mk = (w, h, format, usage) => dev.createTexture({ size: [w, h], format, usage });
  const RT = GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING;
  let size = [0, 0], tex = {};
  function ensure(w, h) {
    if (size[0] === w && size[1] === h) return;
    size = [w, h];
    tex = {
      depth: mk(w, h, "depth32float", RT),
      scene: mk(w, h, "rgba16float", RT),
      hdr: mk(w, h, "rgba16float", RT),
      ldr: mk(w, h, "rgba8unorm", RT | GPUTextureUsage.COPY_SRC),
      read: dev.createBuffer({ size: Math.ceil(w * 4 / 256) * 256 * h, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ }),
    };
  }
  const ubuf = dev.createBuffer({ size: 256, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const kbuf = dev.createBuffer({ size: 16, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const samp = dev.createSampler({ magFilter: "linear", minFilter: "linear" });
  async function module(code, label) {
    const m = dev.createShaderModule({ code, label });
    const ci = await m.getCompilationInfo();
    for (const msg of ci.messages) if (msg.type === "error") errors.push(label + ": " + msg.lineNum + ":" + msg.linePos + " " + msg.message);
    return m;
  }
  const t0 = performance.now();
  const seaMod = await module(SEA, "sea");
  const seaPipe = await dev.createRenderPipelineAsync({ layout: "auto", vertex: { module: seaMod, entryPoint: "vs_main" }, fragment: { module: seaMod, entryPoint: "fs_main", targets: [{ format: "rgba16float" }] }, primitive: { topology: "triangle-list" } }).catch((e) => { errors.push("sea pipeline: " + e.message); return null; });
  // The field sea's three pipelines (only when the shaders came along).
  let wavesPipe = null, meshPipe = null, fillPipe = null, sprayPipe = null;
  const fieldTex = dev.createTexture({ size: CFG.field.size, format: "rgba16float", usage: RT | GPUTextureUsage.COPY_SRC });
  // --field-stats: the wave textures read back after a frame, per band the
  // rms of tile A's height and of tile B's slopes, and the band numbers row.
  async function fieldStats() {
    const [w, h] = CFG.field.size, bpr = Math.ceil(w * 8 / 256) * 256;
    const buf = dev.createBuffer({ size: bpr * h, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
    const enc = dev.createCommandEncoder();
    enc.copyTextureToBuffer({ texture: fieldTex }, { buffer: buf, bytesPerRow: bpr }, [w, h]);
    dev.queue.submit([enc.finish()]);
    await buf.mapAsync(GPUMapMode.READ);
    const u16 = new Uint16Array(buf.getMappedRange().slice(0));
    buf.unmap();
    const half = (x) => { const e = (x >> 10) & 31, m = x & 1023, sg = x >> 15 ? -1 : 1; return e === 0 ? sg * m * 2 ** -24 : e === 31 ? NaN : sg * (1 + m / 1024) * 2 ** (e - 15); };
    const at = (x, y, c) => half(u16[(y * bpr) / 2 + x * 4 + c]);
    const bands = [];
    for (let b = 0; b < 8; b++) {
      let hh = 0, ss = 0, dd = 0, n = 0;
      for (let y = 1; y <= 128; y++) for (let x = 1; x <= 128; x++) { const yy = b * 130 + y; hh += at(x, yy, 1) ** 2; dd += at(x, yy, 0) ** 2 + at(x, yy, 2) ** 2; ss += at(130 + x, yy, 0) ** 2 + at(130 + x, yy, 1) ** 2; n++; }
      bands.push({ b, h_rms: +Math.sqrt(hh / n).toFixed(3), d_rms: +Math.sqrt(dd / n).toFixed(3), s_rms: +Math.sqrt(ss / n).toFixed(4), info: [0, 1, 2, 3].map((c) => +at(b, 1040, c).toPrecision(4)) });
    }
    return { bands, total: [0, 1, 2, 3].map((c) => +at(8, 1040, c).toPrecision(4)) };
  }
  if (MESH) {
    const wm = await module(WAVES, "waves"), mm = await module(MESH, "field"), fm = await module(FILL, "fill");
    wavesPipe = await dev.createRenderPipelineAsync({ layout: "auto", vertex: { module: wm, entryPoint: "vs_main" }, fragment: { module: wm, entryPoint: "fs_main", targets: [{ format: "rgba16float" }] }, primitive: { topology: "triangle-list" } }).catch((e) => { errors.push("waves pipeline: " + e.message); return null; });
    meshPipe = await dev.createRenderPipelineAsync({ layout: "auto", vertex: { module: mm, entryPoint: "vs_main" }, fragment: { module: mm, entryPoint: "fs_main", targets: [{ format: "rgba16float" }] }, primitive: { topology: "triangle-list" }, depthStencil: { format: "depth32float", depthWriteEnabled: true, depthCompare: "greater" } }).catch((e) => { errors.push("field pipeline: " + e.message); return null; });
    fillPipe = await dev.createRenderPipelineAsync({ layout: "auto", vertex: { module: fm, entryPoint: "vs_main" }, fragment: { module: fm, entryPoint: "fs_main", targets: [{ format: "rgba16float" }] }, primitive: { topology: "triangle-list" } }).catch((e) => { errors.push("fill pipeline: " + e.message); return null; });
    const sm = await module(SPRAY, "spray");
    const add = { operation: "add", srcFactor: "one", dstFactor: "one" };
    sprayPipe = await dev.createRenderPipelineAsync({ layout: "auto", vertex: { module: sm, entryPoint: "vs_main" }, fragment: { module: sm, entryPoint: "fs_main", targets: [{ format: "rgba16float", blend: { color: add, alpha: add } }] }, primitive: { topology: "triangle-list" }, depthStencil: { format: "depth32float", depthWriteEnabled: false, depthCompare: "greater" } }).catch((e) => { errors.push("spray pipeline: " + e.message); return null; });
  }
  const isField = (w) => ((Math.round(w[CFG.field.levelWord]) >>> 2) & (1 << 20)) !== 0;
  const fieldUnder = (w) => ((Math.round(w[CFG.field.levelWord]) >>> 2) & (1 << 21)) !== 0;
  let zbuf = null, zsize = [0, 0];
  // The field sea's draws after the scene: the copy as its stand-in, the
  // fill when under the water or over a beach, this frame's wave textures,
  // then the mesh (its own depth, cleared) over them; reps x the sea.
  function fieldSea(enc, words, reps) {
    if (zsize[0] !== size[0] || zsize[1] !== size[1]) {
      zbuf = mk(size[0], size[1], "depth32float", RT);
      zsize = [size[0], size[1]];
    }
    const lvl = Math.round(words[CFG.field.levelWord]) & 3;
    const p0 = enc.beginRenderPass({ colorAttachments: [{ view: tex.hdr.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }], timestampWrites: tsw("copy") });
    p0.setPipeline(copyPipe); p0.setBindGroup(0, dev.createBindGroup({ layout: copyPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }] })); p0.draw(3); p0.end();
    const sceneBg = (pipe) => dev.createBindGroup({ layout: pipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }, { binding: 1, resource: samp }, { binding: 2, resource: { buffer: ubuf } }, { binding: 3, resource: tex.depth.createView() }] });
    for (let r = 0; r < reps; r++) {
      if (words[CFG.field.beachWord] > 0 || fieldUnder(words)) {
        const pf = enc.beginRenderPass({ colorAttachments: [{ view: tex.hdr.createView(), loadOp: "load", storeOp: "store" }], timestampWrites: tsw("fill") });
        pf.setPipeline(fillPipe); pf.setBindGroup(0, sceneBg(fillPipe)); pf.draw(3); pf.end();
      }
      const pw = enc.beginRenderPass({ colorAttachments: [{ view: fieldTex.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 0] }], timestampWrites: tsw("waves") });
      pw.setPipeline(wavesPipe); pw.setBindGroup(0, dev.createBindGroup({ layout: wavesPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: ubuf } }] })); pw.draw(3); pw.end();
      const pm = enc.beginRenderPass({ colorAttachments: [{ view: tex.hdr.createView(), loadOp: "load", storeOp: "store" }], depthStencilAttachment: { view: zbuf.createView(), depthLoadOp: "clear", depthClearValue: 0, depthStoreOp: "store" }, timestampWrites: tsw("mesh") });
      pm.setPipeline(meshPipe);
      pm.setBindGroup(0, dev.createBindGroup({ layout: meshPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }, { binding: 1, resource: samp }, { binding: 2, resource: { buffer: ubuf } }, { binding: 3, resource: tex.depth.createView() }, { binding: 4, resource: fieldTex.createView() }] }));
      pm.draw(CFG.field.verts[lvl]); pm.end();
      if (words[CFG.field.sprayWord] >= 256 && CFG.field.spray[lvl] > 0) {
        const ps = enc.beginRenderPass({ colorAttachments: [{ view: tex.hdr.createView(), loadOp: "load", storeOp: "store" }], depthStencilAttachment: { view: zbuf.createView(), depthLoadOp: "load", depthStoreOp: "store" }, timestampWrites: tsw("spray") });
        ps.setPipeline(sprayPipe);
        ps.setBindGroup(0, dev.createBindGroup({ layout: sprayPipe.getBindGroupLayout(0), entries: [{ binding: 1, resource: samp }, { binding: 2, resource: { buffer: ubuf } }, { binding: 4, resource: fieldTex.createView() }] }));
        ps.draw(CFG.field.spray[lvl]); ps.end();
      }
    }
  }
  const compileMs = performance.now() - t0;
  const scMod = await module(SCENE_WGSL, "scene");
  const scPipe = dev.createRenderPipeline({ layout: "auto", vertex: { module: scMod, entryPoint: "vs" }, fragment: { module: scMod, entryPoint: "fs", targets: [{ format: "rgba16float" }] }, depthStencil: { format: "depth32float", depthWriteEnabled: true, depthCompare: "always" }, primitive: { topology: "triangle-list" } });
  const COPY_WGSL = "@group(0) @binding(0) var src: texture_2d<f32>; struct O { @builtin(position) pos: vec4<f32> }; @vertex fn vs(@builtin(vertex_index) i: u32) -> O { let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)); var o: O; o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0); return o; } @fragment fn fs(i: O) -> @location(0) vec4<f32> { return vec4<f32>(textureLoad(src, vec2<i32>(i.pos.xy), 0).rgb, 1.0); }";
  const cpMod = await module(COPY_WGSL, "copy");
  const copyPipe = dev.createRenderPipeline({ layout: "auto", vertex: { module: cpMod, entryPoint: "vs" }, fragment: { module: cpMod, entryPoint: "fs", targets: [{ format: "rgba16float" }] }, primitive: { topology: "triangle-list" } });
  const tnMod = await module(TONE_WGSL, "tone");
  const tnPipe = dev.createRenderPipeline({ layout: "auto", vertex: { module: tnMod, entryPoint: "vs" }, fragment: { module: tnMod, entryPoint: "fs", targets: [{ format: "rgba8unorm" }] }, primitive: { topology: "triangle-list" } });
  if (!seaPipe || errors.length || (MESH && (!wavesPipe || !meshPipe || !fillPipe || !sprayPipe))) return { error: "shader", errors, compileMs };
  function pass(enc, view, pipe, bg, depth, strip) {
    const load = strip && strip[1] > 0 ? "load" : "clear";
    const label = pipe === scPipe ? "scene" : pipe === tnPipe ? "tone" : "sea";
    const p = enc.beginRenderPass({ colorAttachments: [{ view, loadOp: load, storeOp: "store", clearValue: [0, 0, 0, 1] }], depthStencilAttachment: depth ? { view: depth, depthLoadOp: "clear", depthClearValue: 0, depthStoreOp: "store" } : undefined, timestampWrites: strip ? undefined : tsw(label) });
    if (strip) p.setScissorRect(0, strip[1], size[0], strip[2]);
    p.setPipeline(pipe); p.setBindGroup(0, bg); p.draw(3); p.end();
  }
  // The picture in strips (--strips): scene, each strip of the sea on its
  // own submission, then the tone map.
  async function stripFrame(words, bed) {
    if (isField(words)) {
      // (The field sea is cheap at any height: one submission.)
      dev.queue.submit([frame(words, bed).finish()]);
      await dev.queue.onSubmittedWorkDone();
      return;
    }
    dev.queue.writeBuffer(ubuf, 0, new Float32Array(words));
    dev.queue.writeBuffer(kbuf, 0, new Float32Array([BEDS[bed] ?? 0, 0, 0, 0]));
    let enc = dev.createCommandEncoder();
    pass(enc, tex.scene.createView(), scPipe, dev.createBindGroup({ layout: scPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: ubuf } }, { binding: 1, resource: { buffer: kbuf } }] }), tex.depth.createView());
    dev.queue.submit([enc.finish()]);
    const sbg = dev.createBindGroup({ layout: seaPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }, { binding: 1, resource: samp }, { binding: 2, resource: { buffer: ubuf } }, { binding: 3, resource: tex.depth.createView() }] });
    const n = CFG.strips, h = Math.ceil(size[1] / n);
    for (let i = 0; i < n; i++) {
      const y0 = i * h, hh = Math.min(h, size[1] - y0);
      if (hh <= 0) break;
      enc = dev.createCommandEncoder();
      pass(enc, tex.hdr.createView(), seaPipe, sbg, null, [i, y0, hh]);
      dev.queue.submit([enc.finish()]);
      await dev.queue.onSubmittedWorkDone();
    }
    enc = dev.createCommandEncoder();
    pass(enc, tex.ldr.createView(), tnPipe, dev.createBindGroup({ layout: tnPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.hdr.createView() }] }), null);
    dev.queue.submit([enc.finish()]);
    await dev.queue.onSubmittedWorkDone();
  }
  const oldPipes = {};
  if (CFG.before) for (const [k, code] of Object.entries(OLD)) {
    const m = await module(code, "old-" + k);
    oldPipes[k] = dev.createRenderPipeline({ layout: "auto", vertex: { module: m, entryPoint: "vs_main" }, fragment: { module: m, entryPoint: "fs_main", targets: [{ format: "rgba16float" }] }, primitive: { topology: "triangle-list" } });
  }
  const obuf = dev.createBuffer({ size: 256, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  // The scene from the sea's words, then the old water (its own words) on it.
  function frameOld(words, oldWords, oldKey, bed) {
    dev.queue.writeBuffer(ubuf, 0, new Float32Array(words));
    dev.queue.writeBuffer(obuf, 0, new Float32Array(oldWords));
    dev.queue.writeBuffer(kbuf, 0, new Float32Array([BEDS[bed] ?? 0, 0, 0, 0]));
    const enc = dev.createCommandEncoder();
    pass(enc, tex.scene.createView(), scPipe, dev.createBindGroup({ layout: scPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: ubuf } }, { binding: 1, resource: { buffer: kbuf } }] }), tex.depth.createView());
    const op = oldPipes[oldKey];
    // The old shader discards pixels it doesn't cover: start from the scene.
    const p0 = enc.beginRenderPass({ colorAttachments: [{ view: tex.hdr.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }] });
    p0.setPipeline(copyPipe); p0.setBindGroup(0, dev.createBindGroup({ layout: copyPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }] })); p0.draw(3); p0.end();
    const p1 = enc.beginRenderPass({ colorAttachments: [{ view: tex.hdr.createView(), loadOp: "load", storeOp: "store" }] });
    p1.setPipeline(op);
    p1.setBindGroup(0, dev.createBindGroup({ layout: op.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }, { binding: 1, resource: samp }, { binding: 2, resource: { buffer: obuf } }, { binding: 3, resource: tex.depth.createView() }] }));
    p1.draw(3); p1.end();
    pass(enc, tex.ldr.createView(), tnPipe, dev.createBindGroup({ layout: tnPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.hdr.createView() }] }), null);
    return enc;
  }
  function frame(words, bed, reps = 1) {
    dev.queue.writeBuffer(ubuf, 0, new Float32Array(words));
    dev.queue.writeBuffer(kbuf, 0, new Float32Array([BEDS[bed] ?? 0, 0, 0, 0]));
    const enc = dev.createCommandEncoder();
    pass(enc, tex.scene.createView(), scPipe, dev.createBindGroup({ layout: scPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: ubuf } }, { binding: 1, resource: { buffer: kbuf } }] }), tex.depth.createView());
    if (isField(words)) {
      fieldSea(enc, words, reps);
    } else {
      const sbg = dev.createBindGroup({ layout: seaPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.scene.createView() }, { binding: 1, resource: samp }, { binding: 2, resource: { buffer: ubuf } }, { binding: 3, resource: tex.depth.createView() }] });
      for (let r = 0; r < reps; r++) pass(enc, tex.hdr.createView(), seaPipe, sbg, null);
    }
    pass(enc, tex.ldr.createView(), tnPipe, dev.createBindGroup({ layout: tnPipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: tex.hdr.createView() }] }), null);
    return enc;
  }
  async function readback() {
    const [w, h] = size, bpr = Math.ceil(w * 4 / 256) * 256;
    const enc = dev.createCommandEncoder();
    enc.copyTextureToBuffer({ texture: tex.ldr }, { buffer: tex.read, bytesPerRow: bpr }, [w, h]);
    dev.queue.submit([enc.finish()]);
    await tex.read.mapAsync(GPUMapMode.READ);
    const src = new Uint8Array(tex.read.getMappedRange());
    const px = new Uint8ClampedArray(w * h * 4);
    for (let y = 0; y < h; y++) px.set(src.subarray(y * bpr, y * bpr + w * 4), y * w * 4);
    tex.read.unmap();
    return px;
  }
  // A picture goes to the tool as soon as it is drawn (a long run keeps none in
  // the page, whose memory they would fill).
  async function putImg(name, px) {
    const [w, h] = size;
    const c = new OffscreenCanvas(w, h), g = c.getContext("2d");
    g.putImageData(new ImageData(px, w, h), 0, 0);
    const b = await c.convertToBlob({ type: "image/png" });
    await fetch("/img?name=" + encodeURIComponent(name), { method: "POST", body: b });
  }
  const timing = {}, worst = [], rim = [];
  for (const shot of SHOTS) {
    if (CFG.before && shot.old) {
      ensure(CFG.W, CFG.H);
      dev.queue.submit([frameOld(shot.levels[3], shot.old_words, shot.old, shot.bed).finish()]);
      await dev.queue.onSubmittedWorkDone();
      await putImg(shot.name + "-before", await readback());
    }
    for (const level of CFG.levels) {
      ensure(CFG.W, CFG.H);
      const words = shot.levels[level].slice();
      // --foam-debug: the field sea's foam view (the spray word, bit 16: src/sea/field.rs).
      if (CFG.foamDebug && isField(words)) words[CFG.field.sprayWord] = (Math.round(words[CFG.field.sprayWord]) & 65535) + 65536;
      // First frame of this shot: from submit to done (no warm-up).
      const f0 = performance.now();
      if (CFG.strips > 1) {
        await stripFrame(words, shot.bed);
      } else {
        dev.queue.submit([frame(words, shot.bed).finish()]);
        await dev.queue.onSubmittedWorkDone();
      }
      const first = performance.now() - f0;
      await putImg(shot.name + "-" + level, await readback());
      if (CFG.fieldStats && isField(words)) (window.__fstats = window.__fstats || {})[shot.name + "-" + level] = await fieldStats();
      if (CFG.foam) {
        const variant = async (mut) => {
          const w = words.slice();
          mut(w);
          if (CFG.strips > 1) {
            await stripFrame(w, shot.bed);
          } else {
            dev.queue.submit([frame(w, shot.bed).finish()]);
            await dev.queue.onSubmittedWorkDone();
          }
          return await readback();
        };
        // The storm settings ride on the level word: keep the mid waves,
        // the streaks if asked, never the spindrift.
        const storm = (w, streaks) => {
          const v = Math.round(w[CFG.fw.level]);
          const code = v >> 2;
          w[CFG.fw.level] = (v & 3) + 4 * ((code & 127) | ((streaks ? (code >> 7) & 63 : 0) << 7) | (code & (3 << 20)));
        };
        // (The field sea keeps its breaking crests in the spray word's high bits.)
        const nospray = (w) => { w[CFG.fw.spray] = isField(w) ? Math.floor(w[CFG.fw.spray] / 256) * 256 : 0; };
        await putImg(shot.name + "-" + level + ".foamonly", await variant((w) => { nospray(w); storm(w, true); }));
        await putImg(shot.name + "-" + level + ".nofoam", await variant((w) => { w[CFG.fw.spray] = 0; w[CFG.fw.whitecaps] = 0; w[CFG.fw.shore] = 0; storm(w, false); }));
      }
      if (CFG.timing) {
        // The sea draw alone: (REPS + 1 draws) - (1 draw), per draw.
        const ms = [];
        for (let r = 0; r < CFG.reps; r++) {
          const a = performance.now();
          dev.queue.submit([frame(words, shot.bed, 1).finish()]);
          await dev.queue.onSubmittedWorkDone();
          const one = performance.now() - a;
          const b = performance.now();
          dev.queue.submit([frame(words, shot.bed, 3).finish()]);
          await dev.queue.onSubmittedWorkDone();
          const three = performance.now() - b;
          ms.push((three - one) / 2);
        }
        ms.sort((x, y) => x - y);
        timing[shot.name + "-" + level] = { sea_ms: +ms[Math.floor(ms.length / 2)].toFixed(3), first_frame_ms: +first.toFixed(1) };
        // GPU time per pass and for the whole frame (scene, sea, tone map),
        // from timestamps: median of the reps.
        if (tsOk) {
          const runs = [];
          for (let r = 0; r < CFG.reps; r++) {
            const qs = dev.createQuerySet({ type: "timestamp", count: 32 });
            const rb = dev.createBuffer({ size: 256, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC });
            const mb = dev.createBuffer({ size: 256, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
            TS = { qs, n: 0, labels: [] };
            const enc = frame(words, shot.bed, 1);
            const t = TS;
            TS = null;
            enc.resolveQuerySet(qs, 0, t.n, rb, 0);
            enc.copyBufferToBuffer(rb, 0, mb, 0, t.n * 8);
            dev.queue.submit([enc.finish()]);
            await mb.mapAsync(GPUMapMode.READ);
            const v = new BigInt64Array(mb.getMappedRange().slice(0));
            mb.unmap();
            const one = { frame: Number(v[t.n - 1] - v[0]) / 1e6 };
            t.labels.forEach((l, i) => { one[l] = (one[l] || 0) + Number(v[2 * i + 1] - v[2 * i]) / 1e6; });
            one.sea_all = ["copy", "fill", "waves", "mesh", "spray", "sea"].reduce((a, k) => a + (one[k] || 0), 0);
            runs.push(one);
            qs.destroy(); rb.destroy(); mb.destroy();
          }
          const med = (k) => { const xs = runs.map((x) => x[k] || 0).sort((a, b) => a - b); return +xs[Math.floor(xs.length / 2)].toFixed(3); };
          const keys = [...new Set(runs.flatMap((x) => Object.keys(x)))];
          timing[shot.name + "-" + level].gpu = Object.fromEntries(keys.map((k) => [k, med(k)]));
        }
      }
    }
  }
  // Bottoming out: magenta bed under opaque water; count magenta pixels.
  for (const w of CFG.worst) {
    ensure(800, 450);
    dev.queue.submit([frame(w.words, w.bed).finish()]);
    await dev.queue.onSubmittedWorkDone();
    const px = await readback();
    let holes = 0;
    for (let i = 0; i < px.length; i += 4) if (px[i] > 140 && px[i + 2] > 140 && px[i + 1] < 90) holes++;
    worst.push({ name: w.name, holes });
    if (holes > 0) await putImg(w.name, px);
  }
  // Refraction rims: each deep-water view with no bed behind the water
  // (only sky), drawn with the waves' bending and without it. Bending may
  // move what is seen through the water, never brighten it a lot: the
  // water's darkening is worked out for the unbent pixel's path, so a bent
  // sample past a rock's edge (the sky) would show through as a bright rim.
  for (const name of CFG.rim) {
    const shot = SHOTS.find((s) => s.name === name);
    if (!shot) continue;
    for (const level of CFG.levels.filter((l) => l >= 1)) {
      ensure(800, 450);
      const words = shot.levels[level];
      dev.queue.submit([frame(words, "none").finish()]);
      await dev.queue.onSubmittedWorkDone();
      const bent = await readback();
      const flat = words.slice();
      flat[CFG.refractWord] = 0;
      dev.queue.submit([frame(flat, "none").finish()]);
      await dev.queue.onSubmittedWorkDone();
      const still = await readback();
      const luma = (p, i) => 0.2126 * p[i] + 0.7152 * p[i + 1] + 0.0722 * p[i + 2];
      let leaks = 0;
      const box = [1e9, 1e9, -1, -1];
      for (let i = 0; i < bent.length; i += 4) {
        if (luma(bent, i) - luma(still, i) > 40) {
          leaks++;
          const x = (i / 4) % size[0], y = Math.floor(i / 4 / size[0]);
          box[0] = Math.min(box[0], x); box[1] = Math.min(box[1], y); box[2] = Math.max(box[2], x); box[3] = Math.max(box[3], y);
        }
      }
      rim.push(leaks > 0 ? { name: name + "-" + level, leaks, box } : { name: name + "-" + level, leaks });
      if (leaks > 0) {
        await putImg("rim-" + name + "-" + level, bent);
        await putImg("rim-" + name + "-" + level + "-unbent", still);
      }
    }
  }
  return { adapter: { vendor: info.vendor, architecture: info.architecture, description: info.description }, compileMs, timing, worst, rim, errors, fieldStats: window.__fstats };
}
run().then((r) => { window.__sea = r; }, (e) => { window.__sea = { error: String(e && e.stack || e) }; });
</script>`;

const server = createServer((req, res) => {
  if (req.method === "POST" && req.url.startsWith("/img?name=")) {
    const name = decodeURIComponent(req.url.slice("/img?name=".length));
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      writeFileSync(join(out, name + ".png"), Buffer.concat(chunks));
      res.writeHead(204);
      res.end();
    });
    return;
  }
  res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
  res.end(page);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/`;
// The CPU renderer takes seconds a frame at Ultra: no GPU-process watchdog.
const flags = has("gpu") ? ["--enable-dawn-features=allow_unsafe_apis", "--disable-dawn-features=timestamp_quantization"] : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader", "--disable-gpu-watchdog"];
const { cdp, stop } = await startChrome({ width: 900, height: 600, flags, prefix: "evani-seashots-" });
const limit = Number(arg("timeout-s", "600")) * 1000;
let code = 1;
const finish = async (c) => {
  await stop().catch(() => {});
  server.close();
  process.exit(c);
};
setTimeout(() => {
  console.error(`sea-shots: the ${limit / 1000} s limit was reached`);
  finish(3);
}, limit).unref();
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, limit);
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
  await send("Page.navigate", { url });
  const r = await send("Runtime.evaluate", {
    expression: "new Promise((ok) => { const f = () => window.__sea ? ok(window.__sea) : setTimeout(f, 250); f(); })",
    awaitPromise: true,
    returnByValue: true,
  });
  const rep = r.result.value || { error: JSON.stringify(r.exceptionDetails || r).slice(0, 800) };
  const holes = (rep.worst || []).reduce((a, w) => a + w.holes, 0);
  const rimLeaks = (rep.rim || []).reduce((a, w) => a + w.leaks, 0);
  rep.holes = holes;
  rep.rim_leaks = rimLeaks;
  rep.ok = !rep.error && (rep.errors || []).length === 0 && holes === 0 && rimLeaks === 0;
  writeFileSync(join(out, "report.json"), JSON.stringify(rep, null, 1));
  console.log(JSON.stringify({ ok: rep.ok, holes, rim_leaks: rimLeaks, compileMs: rep.compileMs, errors: rep.errors, error: rep.error, adapter: rep.adapter, timing: rep.timing, worst: rep.worst, rim: rep.rim }, null, 1));
  code = rep.ok ? 0 : 1;
} catch (e) {
  console.error(String(e && e.stack || e));
  code = 1;
}
await finish(code);
