// The ocean upgrade's default-unchanged proof (2026-11-01): the old and the
// new nature shaders, spliced with their default material tables, drawn on
// the same inputs (camera, sim time, sun, a sea and a pool over a sloping
// bed) in every mode the pass uses (0 Lite/Low multiply, 1 Medium multiply,
// 2 Medium add, 3 High/Ultra refract, 4 Lite/Low add), read back as 32-bit
// floats and compared value for value. Also draws the new shader with the
// upgrade switched on (a sanity picture) and times old/new/on draws.
//
//   1. cargo test -p evani-material-nature dump_default_shaders_for_the_gpu_proof
//        with EVANI_NATURE_DUMP=<dir> and EVANI_NATURE_OLD=<dir with the old
//        nature.wgsl, nature_refract.wgsl, nature_common.wgsl>
//   2. node tools/nature/ocean-default-proof.mjs --dump <dir> --out <dir>
//        [--gpu] (default: WebGPU on SwiftShader, the CPU; no graphics card)
//        [--timeout-s 180] [--size 640x400] [--reps 6]
//
// Writes <out>/report.json, old-*.png, new-*.png, on-*.png (tone-mapped
// mode 2 and mode 3 pictures) and exits 1 if any value differs.
// Headless, muted, a fresh full-path profile, pointer/keyboard locks stubbed,
// Chrome closed by its own PID (tools/bench-runner/chrome.mjs).
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startChrome, STUBS } from "../bench-runner/chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const dump = arg("dump", "");
const out = arg("out", ".evani/shots/st-ocean/default-proof");
const [W, H] = arg("size", "640x400").split("x").map(Number);
const reps = Number(arg("reps", "6"));
if (!dump) {
  console.error("--dump <dir> (from the dump test) is needed");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const src = (f) => readFileSync(join(dump, f), "utf8");
// The upgrade switched on: rows 18-20 of the new table's slot body.
const ON_ROWS = [
  "vec4<f32>(35.00000, 0.70000, 0.80000, 0.00000),",
  "vec4<f32>(0.10000, 0.55000, 0.42000, 2.00000),",
  "vec4<f32>(1.20000, 0.40000, 0.00000, 0.00000),",
];
const switchOn = (s) => {
  const lines = s.split("\n");
  const idx = [];
  lines.forEach((l, i) => l.trim().startsWith("vec4<f32>(") && l.trim().endsWith("),") && idx.push(i));
  // The table is the first run of 21 rows in the slot function.
  const start = idx.findIndex((_, k) => idx[k + 20] === idx[k] + 20);
  if (start < 0) throw new Error("no 21-row table in the new shader");
  for (let r = 0; r < 3; r++) lines[idx[start + 18 + r]] = "        " + ON_ROWS[r];
  // --rows "4=0.6,1.8,0.4,0.5;20=0.8,0.5,0,0": more rows for the "on" draw (tuning).
  for (const part of arg("rows", "").split(";").filter(Boolean)) {
    const [row, vals] = part.split("=");
    const v = vals.split(",").map((x) => Number(x).toFixed(5));
    lines[idx[start + Number(row)]] = `        vec4<f32>(${v.join(", ")}),`;
  }
  return lines.join("\n");
};
const shaders = {
  old: { nature: src("old-nature.wgsl"), refract: src("old-refract.wgsl") },
  new: { nature: src("new-nature.wgsl"), refract: src("new-refract.wgsl") },
};
shaders.on = { nature: switchOn(shaders.new.nature), refract: switchOn(shaders.new.refract) };

const page = `<!doctype html><meta charset="utf-8"><body><script>
const W = ${W}, H = ${H}, REPS = ${reps};
const SH = ${JSON.stringify(shaders)};
// Depth of the scene: a bed sloping from 0.3 m to 7 m under the sea, a
// raised pool rim, sky above the horizon. Reversed depth: 1/dist = inv0 + inv1 z.
const DEPTH_WGSL = \`
struct U { eye: vec4<f32>, fwd: vec4<f32>, right: vec4<f32>, up: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> O {
  let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  var o: O; o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0); o.uv = vec2<f32>(p.x, 1.0 - p.y); return o;
}
struct F { @builtin(frag_depth) d: f32, @location(0) c: vec4<f32> };
@fragment fn fs(i: O) -> F {
  let ndc = vec2<f32>(i.uv.x * 2.0 - 1.0, 1.0 - i.uv.y * 2.0);
  let ray = u.fwd.xyz + u.right.xyz * ndc.x + u.up.xyz * ndc.y;
  var f: F; f.d = 0.0;
  // A sloping bed: y = -0.3 - 0.12 * (distance from the shore line z = 6).
  var t = 1.0e9;
  for (var k = 0; k < 48; k = k + 1) {
    let tt = f32(k) * 0.08 * (1.0 + f32(k) * 0.25);
    let p = u.eye.xyz + ray * tt;
    let bed = -0.3 - 0.12 * max(6.0 - p.z, 0.0) + 0.15 * sin(p.x * 0.7) * cos(p.z * 0.5);
    if (p.y < bed) { t = tt; break; }
  }
  if (t < 1.0e8) {
    f.d = (1.0 / t - u.eye.w) / u.fwd.w;
  }
  let g = fract(i.uv * 9.0);
  f.c = vec4<f32>(0.3 + 0.4 * step(0.5, g.x), 0.25 + 0.3 * step(0.5, g.y), 0.2 + 0.5 * i.uv.y, 1.0);
  return f;
}\`;
const norm = (v) => { const l = Math.hypot(...v); return v.map((x) => x / l); };
const cross = (a, b) => [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
function header(view, time, mode) {
  const fwd = norm(view.target.map((x, i) => x - view.eye[i]));
  const right = norm(cross(fwd, [0, 1, 0]));
  const up = cross(right, fwd);
  const tan = Math.tan((60 * Math.PI) / 360), asp = W / H;
  const inv0 = 0.0, inv1 = 10.0;
  const sun = norm(view.sun);
  const a = new Float32Array(64);
  a.set([...view.eye, inv0, ...fwd, inv1, ...right.map((x) => x * tan * asp), time, ...up.map((x) => x * tan), mode, ...sun, 2, 3.0, 2.8, 2.5, 4210752], 0);
  // A sea rectangle (shape 1, turned 0.3 rad) and a pool ellipse (shape 0).
  a.set([0, 0, -20, 1, 40, 1, 40, 0.3], 24);
  a.set([3, 0.15, 2, 0, 2.5, 1, 1.6, 0], 32);
  return a;
}
// The top of each picture is black: above the horizon the test scene has no
// sky (depth 0, nothing drawn there), and the nature pass draws only where a
// surface is; the water shader is not involved.
const VIEWS = [
  { name: "sea", eye: [0, 3, 8], target: [0, 0, -10], sun: [0.25, 0.35, -0.9] },
  { name: "low", eye: [1, 0.9, 6], target: [-2, 0.2, -30], sun: [-0.6, 0.12, -0.8] },
  // The pool ellipse close up: still inland water (a lake).
  { name: "lake", eye: [3.5, 1.8, 6.5], target: [3, 0.1, 1.6], sun: [0.4, 0.5, -0.75] },
];
const TIMES = [0, 7.3, 123.4];
const MODES = [0, 1, 2, 3, 4];
async function run() {
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) return { error: "no WebGPU adapter" };
  const info = adapter.info || {};
  const dev = await adapter.requestDevice();
  const errors = [];
  dev.onuncapturederror = (e) => errors.push(String(e.error.message).slice(0, 400));
  const depth = dev.createTexture({ size: [W, H], format: "depth32float", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING });
  const scene = dev.createTexture({ size: [W, H], format: "rgba16float", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.TEXTURE_BINDING });
  const target = dev.createTexture({ size: [W, H], format: "rgba32float", usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
  const read = dev.createBuffer({ size: W * H * 16, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
  const ubuf = dev.createBuffer({ size: 256, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const dbuf = dev.createBuffer({ size: 64, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const samp = dev.createSampler({ magFilter: "linear", minFilter: "linear" });
  const dmod = dev.createShaderModule({ code: DEPTH_WGSL });
  const dpipe = dev.createRenderPipeline({ layout: "auto", vertex: { module: dmod, entryPoint: "vs" }, fragment: { module: dmod, entryPoint: "fs", targets: [{ format: "rgba16float" }] }, depthStencil: { format: "depth32float", depthWriteEnabled: true, depthCompare: "always" }, primitive: { topology: "triangle-list" } });
  const pipes = {};
  for (const k of Object.keys(SH)) {
    pipes[k] = {};
    for (const s of ["nature", "refract"]) {
      const m = dev.createShaderModule({ code: SH[k][s] });
      const ci = await m.getCompilationInfo();
      for (const msg of ci.messages) if (msg.type === "error") errors.push(k + "/" + s + ": " + msg.message);
      pipes[k][s] = dev.createRenderPipeline({ layout: "auto", vertex: { module: m, entryPoint: "vs_main" }, fragment: { module: m, entryPoint: "fs_main", targets: [{ format: "rgba32float" }] }, primitive: { topology: "triangle-list" } });
    }
  }
  function fillDepth(view) {
    const h = header(view, 0, 0);
    const d = new Float32Array(16);
    d.set([...h.slice(0, 3), h[3], ...h.slice(4, 7), h[7], ...h.slice(8, 11), 0, ...h.slice(12, 15), 0]);
    dev.queue.writeBuffer(dbuf, 0, d);
    const enc = dev.createCommandEncoder();
    const p = enc.beginRenderPass({ colorAttachments: [{ view: scene.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }], depthStencilAttachment: { view: depth.createView(), depthLoadOp: "clear", depthClearValue: 0, depthStoreOp: "store" } });
    p.setPipeline(dpipe);
    p.setBindGroup(0, dev.createBindGroup({ layout: dpipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: dbuf } }] }));
    p.draw(3);
    p.end();
    dev.queue.submit([enc.finish()]);
  }
  function draw(kind, view, time, mode, n = 1) {
    dev.queue.writeBuffer(ubuf, 0, header(view, time, mode));
    const refract = mode === 3;
    const pipe = pipes[kind][refract ? "refract" : "nature"];
    const entries = refract
      ? [{ binding: 0, resource: scene.createView() }, { binding: 1, resource: samp }, { binding: 2, resource: { buffer: ubuf } }, { binding: 3, resource: depth.createView() }]
      : [{ binding: 0, resource: { buffer: ubuf } }, { binding: 1, resource: depth.createView() }];
    const bg = dev.createBindGroup({ layout: pipe.getBindGroupLayout(0), entries });
    const enc = dev.createCommandEncoder();
    for (let r = 0; r < n; r++) {
      const p = enc.beginRenderPass({ colorAttachments: [{ view: target.createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 0] }] });
      p.setPipeline(pipe);
      p.setBindGroup(0, bg);
      p.draw(3);
      p.end();
    }
    return enc;
  }
  async function pixels(kind, view, time, mode) {
    const enc = draw(kind, view, time, mode);
    enc.copyTextureToBuffer({ texture: target }, { buffer: read, bytesPerRow: W * 16 }, [W, H]);
    dev.queue.submit([enc.finish()]);
    await read.mapAsync(GPUMapMode.READ);
    const f = new Float32Array(read.getMappedRange().slice(0));
    read.unmap();
    return f;
  }
  async function png(f) {
    const c = new OffscreenCanvas(W, H), g = c.getContext("2d"), img = g.createImageData(W, H);
    for (let i = 0; i < W * H; i++) for (let k = 0; k < 3; k++) {
      const x = Math.max(f[i * 4 + k], 0); const t = x / (1 + x);
      img.data[i * 4 + k] = Math.round(255 * Math.pow(t, 1 / 2.2));
    }
    for (let i = 0; i < W * H; i++) img.data[i * 4 + 3] = 255;
    g.putImageData(img, 0, 0);
    const b = await c.convertToBlob({ type: "image/png" });
    const bytes = new Uint8Array(await b.arrayBuffer());
    let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  }
  const results = [], images = {};
  let differing = 0, total = 0, maxAbs = 0, onDiffers = 0;
  for (const view of VIEWS) {
    fillDepth(view);
    for (const time of TIMES) for (const mode of MODES) {
      const a = await pixels("old", view, time, mode);
      const b = await pixels("new", view, time, mode);
      let diff = 0, mx = 0, covered = 0;
      for (let i = 0; i < a.length; i++) {
        if (a[i] !== 0 && a[i] !== 1) covered++;
        if (a[i] !== b[i] && !(Number.isNaN(a[i]) && Number.isNaN(b[i]))) { diff++; mx = Math.max(mx, Math.abs(a[i] - b[i])); }
      }
      differing += diff; total += a.length; maxAbs = Math.max(maxAbs, mx);
      const c = await pixels("on", view, time, mode);
      let od = 0; for (let i = 0; i < a.length; i++) if (a[i] !== c[i]) od++;
      onDiffers += od;
      results.push({ view: view.name, time, mode, values: a.length, covered, differing: diff, max_abs: mx, on_differs: od });
      if (time === 7.3 && (mode === 2 || mode === 3)) {
        images["old-" + view.name + "-m" + mode] = await png(a);
        images["new-" + view.name + "-m" + mode] = await png(b);
        images["on-" + view.name + "-m" + mode] = await png(c);
      }
    }
  }
  // Timing: REPS full-screen draws per kind, mode 2 and 3, sea view.
  fillDepth(VIEWS[0]);
  const timing = {};
  for (const mode of [2, 3]) for (const kind of ["old", "new", "on"]) {
    const ms = [];
    for (let r = 0; r < REPS; r++) {
      const t0 = performance.now();
      dev.queue.submit([draw(kind, VIEWS[0], 7.3 + r, mode, 1).finish()]);
      await dev.queue.onSubmittedWorkDone();
      ms.push(performance.now() - t0);
    }
    ms.sort((x, y) => x - y);
    timing[kind + "-m" + mode] = { median_ms: ms[Math.floor(ms.length / 2)], all_ms: ms.map((x) => +x.toFixed(2)) };
  }
  return { adapter: { vendor: info.vendor, architecture: info.architecture, description: info.description, device: info.device }, size: [W, H], differing, total, maxAbs, onDiffers, results, timing, errors, images };
}
run().then((r) => { window.__proof = r; }, (e) => { window.__proof = { error: String(e && e.stack || e) }; });
</script>`;

const server = createServer((req, res) => {
  res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
  res.end(page);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/`;
const flags = process.argv.includes("--gpu") ? [] : ["--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader"];
const { cdp, stop } = await startChrome({ width: 800, height: 600, flags, prefix: "evani-oceanproof-" });
const limit = Number(arg("timeout-s", "180")) * 1000;
let code = 1;
const finish = async (c) => {
  await stop().catch(() => {});
  server.close();
  process.exit(c);
};
setTimeout(() => {
  console.error(`ocean-default-proof: the ${limit / 1000} s limit was reached`);
  finish(3);
}, limit).unref();
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const send = (m, p) => cdp.send(m, p, s, 170000);
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.addScriptToEvaluateOnNewDocument", { source: STUBS });
  await send("Page.navigate", { url });
  const r = await send("Runtime.evaluate", {
    expression: "new Promise((ok) => { const f = () => window.__proof ? ok(window.__proof) : setTimeout(f, 200); f(); })",
    awaitPromise: true,
    returnByValue: true,
  });
  const proof = r.result.value || { error: JSON.stringify(r.exceptionDetails || r).slice(0, 500) };
  for (const [k, b64] of Object.entries(proof.images || {})) writeFileSync(join(out, k + ".png"), Buffer.from(b64, "base64"));
  delete proof.images;
  proof.identical = !proof.error && proof.errors?.length === 0 && proof.differing === 0;
  writeFileSync(join(out, "report.json"), JSON.stringify(proof, null, 1));
  console.log(JSON.stringify({ identical: proof.identical, differing: proof.differing, total: proof.total, maxAbs: proof.maxAbs, onDiffers: proof.onDiffers, errors: proof.errors, error: proof.error, timing: proof.timing, adapter: proof.adapter }, null, 1));
  code = proof.identical ? 0 : 1;
} catch (e) {
  console.error(String(e && e.stack || e));
  code = 1;
}
await finish(code);
