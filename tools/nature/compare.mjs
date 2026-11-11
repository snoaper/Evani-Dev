// Ocean comparison rig: Ultra-Water (three.js, run locally from its own
// code) and our spectral sea, rendered from the SAME cameras, sun and sea
// states (tools/nature/compare-views.json), measured the same way, and laid
// out side by side on one page. Re-run it for every later round.
//
//   node tools/nature/compare.mjs [--ours <commit|branch|path>] [--views <json>]
//        [--only open-deck,storm] [--moments 8] [--page <dir>] [--work <dir>]
//        [--uw <Ultra-Water folder>] [--skip-ours] [--skip-uw] [--gpu] [--timeout-s 5400]
//        [--fresh-uw] [--no-phone] [--phone <dir>] [--phone-prefix same-settings-]
//        [--checkout <dir>] [--ours-label <text>]
//
//   --checkout   where the rig's own git worktree of "ours" goes (default
//                <work>/ours); the cargo target and TEMP stay in <work>.
//   --ours-label the name shown for ours (the commit is added to it).
//
//   A later round: `--ours <commit>` re-renders only ours; each Ultra-Water
//   view is kept from the last run while its settings are unchanged
//   (--fresh-uw draws it again). `--skip-ours --skip-uw` only re-measures
//   and rewrites the page from the last pictures.
//   --phone    where the phone pictures go (default: <main
//              checkout>/.evani/shots/phone): same-settings-<view>.jpg, labelled,
//              Ultra-Water above ours (Ultra), 1080 px wide, under 600 KB.
//
//   --ours     what "ours" is (default: the storm-swell branch). A commit or
//              branch is checked out (detached) into <work>/ours, a git
//              worktree this rig owns. A folder (another worktree) is only
//              read: its HEAD commit plus its uncommitted diff are applied
//              to <work>/ours, so the folder itself is never touched.
//   --page     the page's folder (default: <main checkout>/.evani/shots/ocean-compare)
//   --work     scratch space (default: <temp>/evani-cmp): the checkout, its
//              cargo target, the dumps and every moment's picture.
//   --gpu      draw both on the graphics card and time frames. Without it
//              (the default: the owner's setting keeps work off the graphics
//              card) both draw on the CPU (SwiftShader: the same shaders,
//              slower), and frame times are left out.
//   --uw       the Ultra-Water clone (default: <temp>/ultra-water, or $EVANI_UW_DIR). Get it
//              once with: git clone https://github.com/Humpalumps/Ultra-Water
//              <that folder> && cd <that folder> && npm install
//
// Steps:
//  1. Ours: appends tools/nature/compare-dump.rs (a test) to the checkout's
//     sea tests, runs it (cargo, low priority, CARGO_TARGET_DIR in <work>)
//     to pack the engine's own uniform words for every view and moment,
//     restores the checkout, then draws them with that checkout's own GPU
//     harness (tools/nature/sea-shots.mjs) at High (2) and Ultra (3).
//  2. Ultra-Water: starts its Vite dev server on a free local port (its own
//     code, untouched), opens it in a headless, muted Chrome (fresh
//     full-path profile, pointer/keyboard locks stubbed) at the views' size,
//     takes over requestAnimationFrame so time only moves when the rig steps
//     it (fixed 1/30 s frames: deterministic, the wave seed is fixed at
//     1337), sets camera, sun, waves, sky and water values on the objects
//     the page exposes (window.app), puts the sim clock 4 s before each
//     moment, runs the wave simulation alone for 116 steps of 1/30 s (its
//     foam builds up and fades as in play), draws 4 whole frames (the auto
//     exposure snaps to the view) and reads the canvas. Frame time (--gpu
//     only): the page's own fence-timed benchmark (60 frames).
//     The dev server and Chrome are stopped by their own PIDs.
//  3. Measures every picture (all moments; the page shows moment 0 and the
//     moments' means): crest share above the horizon and p95 crest px, foam
//     % with the sun's glitter path and isolated sparkles masked out, the
//     light-dark gradient mean and p90, luma, and frame time.
//  4. Writes <page>/index.html (+ img/ and data.json).
//
// Ultra-Water is MIT (c) 2026 Humpalumps; it is a reference only. Nothing
// of it is copied into Evani.Dev: it runs from its own folder, and only
// pictures of it are kept.
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { startChrome, STUBS, sleep } from "../bench-runner/chrome.mjs";
import { readPng } from "./png-small.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const has = (k) => process.argv.includes("--" + k);
const git = (cwd, ...a) => {
  const r = spawnSync("git", ["-C", cwd, ...a], { encoding: "utf8", maxBuffer: 1 << 28 });
  if (r.status !== 0) throw new Error(`git ${a.join(" ")}: ${r.stderr}`);
  return r.stdout;
};
const repo = git(here, "rev-parse", "--show-toplevel").trim();
// The page goes to the main checkout (the first worktree), not this one.
const mainCheckout = git(repo, "worktree", "list", "--porcelain").split("\n")[0].replace(/^worktree /, "").trim();
const viewsFile = resolve(arg("views", join(here, "compare-views.json")));
const work = resolve(arg("work", join(tmpdir(), "evani-cmp")));
const uwDir = resolve(arg("uw", process.env.EVANI_UW_DIR || join(tmpdir(), "ultra-water")));
const pageDir = resolve(arg("page", join(mainCheckout, ".evani", "shots", "ocean-compare")));
const oursArg = arg("ours", "storm-swell");
// Where the rig's own throwaway checkout of "ours" goes (default <work>/ours).
const oursCheckout = resolve(arg("checkout", join(work, "ours")));
const cfg = JSON.parse(readFileSync(viewsFile, "utf8"));
const only = arg("only", "");
const views = cfg.views.filter((v) => !only || only.split(",").includes(v.id));
const nMoments = Math.min(Number(arg("moments", String(cfg.moments.length))), cfg.moments.length);
const moments = cfg.moments.slice(0, nMoments);
const [W, H] = cfg.size;
const LEVELS = { 3: "Ultra", 2: "High" };
mkdirSync(work, { recursive: true });
mkdirSync(join(pageDir, "img"), { recursive: true });
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
const limit = Number(arg("timeout-s", "5400")) * 1000;
const GPU = has("gpu");
// The CPU renderer (SwiftShader) for WebGPU and WebGL; no GPU watchdog (a
// frame takes seconds).
const CPU_FLAGS = ["--use-webgpu-adapter=swiftshader", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-gpu-watchdog"];
const cleanups = [];
const stopAll = async () => {
  for (const c of cleanups.splice(0).reverse()) await c().catch(() => {});
};
setTimeout(async () => {
  console.error(`compare: the ${limit / 1000} s limit was reached`);
  await stopAll();
  process.exit(3);
}, limit).unref();

// A cfg view in the run's own moments (the rig's views file, trimmed).
function runViewsFile() {
  const f = join(work, "views-run.json");
  writeFileSync(f, JSON.stringify({ ...cfg, moments, views }, null, 1));
  return f;
}

// ---------------------------------------------------------------- ours ----
async function renderOurs() {
  const co = oursCheckout;
  let commit, diff = "", label;
  if (existsSync(oursArg) && statSync(oursArg).isDirectory()) {
    commit = git(oursArg, "rev-parse", "HEAD").trim();
    diff = git(oursArg, "diff", "HEAD", "--binary");
    label = `${oursArg} (HEAD ${commit.slice(0, 8)}${diff ? " + uncommitted changes" : ""})`;
  } else {
    commit = git(repo, "rev-parse", oursArg + "^{commit}").trim();
    label = `${oursArg} ${commit.slice(0, 8)}`;
  }
  if (arg("ours-label")) label = `${arg("ours-label")} (${commit.slice(0, 8)}${diff ? " + uncommitted changes" : ""})`;
  if (!existsSync(join(co, ".git"))) {
    log("ours: new checkout", co);
    git(repo, "worktree", "add", "--detach", co, commit);
  } else {
    // The rig's own throwaway checkout: back to the exact commit.
    git(co, "reset", "--hard", "-q");
    git(co, "checkout", "-q", "--detach", commit);
  }
  if (diff) {
    writeFileSync(join(work, "ours.diff"), diff);
    git(co, "apply", "--whitespace=nowarn", join(work, "ours.diff"));
  }
  const testsRs = join(co, "crates", "evani-material-nature", "src", "sea", "tests.rs");
  const src = readFileSync(testsRs, "utf8");
  if (!src.includes("fn dump_compare_views")) writeFileSync(testsRs, src + "\n" + readFileSync(join(here, "compare-dump.rs"), "utf8"));
  const dump = join(work, "dump-ours");
  log("ours: packing the engine's uniform words (cargo test, low priority)");
  const runLow = process.env.EVANI_RUN_LOW || (process.env.APPDATA ? join(process.env.APPDATA, "Evani.App", "tools", "run-low.mjs") : ""); // optional low-priority runner; plain cargo when absent
  const cargo = ["test", "-p", "evani-material-nature", "--features", "spectral-sea", "--lib", "dump_compare_views", "--", "--nocapture"];
  const env = { ...process.env, CARGO_TARGET_DIR: join(work, "target"), TEMP: join(work, "tmp"), TMP: join(work, "tmp"), EVANI_CMP_VIEWS: runViewsFile(), EVANI_SEA_DUMP: dump };
  mkdirSync(env.TEMP, { recursive: true });
  const r = await run(process.execPath, existsSync(runLow) ? [runLow, "cargo", ...cargo] : null, "cargo", cargo, co, env);
  // Leave the checkout as its commit (plus diff) again.
  git(co, "checkout", "--", "crates/evani-material-nature/src/sea/tests.rs");
  if (diff) git(co, "apply", "--whitespace=nowarn", join(work, "ours.diff"));
  if (r !== 0 || !existsSync(join(dump, "scenes.json"))) throw new Error("ours: the dump test failed (exit " + r + ")");
  const shots = join(work, "shots-ours");
  mkdirSync(shots, { recursive: true });
  log(`ours: drawing with the checkout's own harness (High and Ultra, ${GPU ? "graphics card" : "CPU"})`);
  const h = await run(process.execPath, null, process.execPath, [join(co, "tools", "nature", "sea-shots.mjs"), "--dump", dump, "--out", shots, ...(GPU ? ["--gpu", "--reps", "3"] : ["--no-timing"]), "--levels", "2,3", "--no-worst", "--no-rim", "--size", `${W}x${H}`, "--timeout-s", "3600"], co, process.env);
  log("ours: harness exit", h);
  const rec = oursFromDisk(label, commit, !!diff);
  writeFileSync(join(work, "ours-run.json"), JSON.stringify(rec, null, 1));
  return rec;
}
// Our side's result from the files the last run left (dump and pictures).
function oursFromDisk(label, commit, dirty) {
  const dump = join(work, "dump-ours"), shots = join(work, "shots-ours");
  const rep = existsSync(join(shots, "report.json")) ? JSON.parse(readFileSync(join(shots, "report.json"), "utf8")) : {};
  if (rep.error || (rep.errors || []).length) throw new Error("ours: harness error " + JSON.stringify(rep.error || rep.errors).slice(0, 600));
  const scenes = JSON.parse(readFileSync(join(dump, "scenes.json"), "utf8"));
  const times = Object.fromEntries(scenes.shots.map((s) => [s.name, s.time]));
  return { label, commit, dirty, shots, timing: rep.timing || {}, adapter: rep.adapter, applied: scenes.applied || {}, times };
}
// --skip-ours: the last run's record, or (none saved) one from the files.
function lastOurs() {
  const f = join(work, "ours-run.json");
  if (existsSync(f)) return JSON.parse(readFileSync(f, "utf8"));
  if (!existsSync(oursCheckout)) return null;
  const commit = git(oursCheckout, "rev-parse", "HEAD").trim();
  return oursFromDisk(`${commit.slice(0, 8)} (the last rendered checkout)`, commit, false);
}

// Runs a child hidden; `wrapped` (args for `cmd0`) is preferred when given.
function run(cmd0, wrapped, cmd, args, cwd, env) {
  return new Promise((ok) => {
    const p = wrapped ? spawn(cmd0, wrapped, { cwd, env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] }) : spawn(cmd, args, { cwd, env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    let tail = "";
    const keep = (d) => {
      tail = (tail + d.toString()).slice(-4000);
    };
    p.stdout.on("data", keep);
    p.stderr.on("data", keep);
    cleanups.push(async () => {
      if (p.exitCode === null) spawnSync("taskkill", ["/PID", String(p.pid), "/T", "/F"], { stdio: "ignore" });
    });
    p.on("exit", (c) => {
      if (c !== 0) console.error(tail);
      ok(c);
    });
  });
}

// --------------------------------------------------------- Ultra-Water ----
const freePort = () =>
  new Promise((ok) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const p = s.address().port;
      s.close(() => ok(p));
    });
  });

// Page code run before Ultra-Water's own: the stubs, and a requestAnimationFrame
// that only fires when the rig steps it (fixed frame times).
const CLOCK = `${STUBS}
(() => {
  const q = [];
  let now = 0;
  window.requestAnimationFrame = (cb) => { q.push(cb); return q.length; };
  window.cancelAnimationFrame = () => {};
  window.__step = (n, dtMs) => {
    if (!now) now = performance.now();
    for (let i = 0; i < n; i++) { now += dtMs; const cbs = q.splice(0); for (const cb of cbs) cb(now); }
    return q.length;
  };
})();`;

const rad = (d) => (d * Math.PI) / 180;
// Our frame -> Ultra-Water's: a rotation about y by `rot` degrees (headings
// add `rot`) and a shift.
function toUw(p, rot, shift) {
  const c = Math.cos(rad(rot)), s = Math.sin(rad(rot));
  return [p[0] * c - p[2] * s + shift[0], p[1], p[0] * s + p[2] * c + shift[2]];
}
const wrap180 = (a) => ((((a + 180) % 360) + 360) % 360) - 180;

async function renderUw() {
  if (!existsSync(join(uwDir, "node_modules", "vite"))) throw new Error(`Ultra-Water is not installed in ${uwDir} (git clone https://github.com/Humpalumps/Ultra-Water ${uwDir}, then npm install there)`);
  const port = await freePort();
  log("ultra-water: dev server on port", port);
  const vite = spawn(process.execPath, [join(uwDir, "node_modules", "vite", "bin", "vite.js"), "--port", String(port), "--strictPort", "--host", "127.0.0.1"], { cwd: uwDir, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  let viteOut = "";
  vite.stdout.on("data", (d) => (viteOut = (viteOut + d).slice(-2000)));
  vite.stderr.on("data", (d) => (viteOut = (viteOut + d).slice(-2000)));
  cleanups.push(async () => {
    if (vite.exitCode === null) spawnSync("taskkill", ["/PID", String(vite.pid), "/T", "/F"], { stdio: "ignore" });
    log("ultra-water: dev server stopped (PID " + vite.pid + ")");
  });
  const url = `http://127.0.0.1:${port}/?desktop`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(url)).ok) break;
    } catch {
      // not up yet
    }
    await sleep(200);
  }
  // The page, opened lazily (a cached view needs none) and again after a
  // lost browser.
  let ev = null, stopChrome = null, gpu = null;
  async function openPage() {
    if (stopChrome) await stopChrome().catch(() => {});
    const chrome = await startChrome({ width: W, height: H, flags: [...(GPU ? ["--ignore-gpu-blocklist", "--disable-gpu-watchdog"] : CPU_FLAGS), ...(arg("chrome-log") ? ["--enable-logging", "--v=0", `--log-file=${resolve(arg("chrome-log"))}`] : [])], prefix: "evani-cmp-uw-" });
    stopChrome = chrome.stop;
    cleanups.push(chrome.stop);
    const { cdp } = chrome;
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    const send = (m, p, ms = 120000) => cdp.send(m, p, s, ms);
    ev = async (x, ms = 300000) => {
      const r = await send("Runtime.evaluate", { expression: x, awaitPromise: true, returnByValue: true }, ms);
      if (r.exceptionDetails) throw new Error("page: " + (r.exceptionDetails.exception?.description || r.exceptionDetails.text).slice(0, 800));
      return r.result.value;
    };
    await send("Page.enable");
    await send("Runtime.enable");
    await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    await send("Page.addScriptToEvaluateOnNewDocument", { source: CLOCK });
    await send("Page.navigate", { url });
    let ready = false;
    for (let i = 0; i < 300 && !ready; i++) {
      await sleep(200);
      ready = await ev("!!(window.app && window.__step)").catch(() => false);
    }
    if (!ready) throw new Error("ultra-water: the page did not start (" + viteOut.slice(-400) + ")");
    // Its starting values, so each view starts from them.
    gpu = await ev(`(async () => {
      window.__step(3, 1000 / 30);
      const a = window.app, su = a.surface.uniforms;
      window.__defaults = { ocean: { ...a.ocean.params }, sky: { ...a.sky.params }, post: { ...a.post.params }, quality: { ...a.quality },
        optics: su.uOptics.value.toArray(), sss: su.uSSS.value.toArray(), foam: su.uFoam.value.toArray(), glint: su.uGlint.value };
      window.__island = (await import('/src/scene/islandHeight.js')).islandHeight;
      const gl = a.renderer.getContext(), ext = gl.getExtension('WEBGL_debug_renderer_info');
      for (const e of document.body.children) if (e.tagName !== 'CANVAS') e.style.display = 'none';
      return ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : 'unknown';
    })()`);
    log("ultra-water: renderer", gpu);
  }
  const out = join(work, "shots-uw");
  mkdirSync(out, { recursive: true });
  const res = { gpu, url: `http://127.0.0.1:${port}/`, views: {} };
  for (const v of views) {
    // A view already rendered with the same settings (its pictures still
    // there) is kept: Ultra-Water is the fixed reference, so a later round
    // re-renders only ours. --fresh-uw renders it again.
    const cacheKey = JSON.stringify({ v, moments, GPU, W, H });
    const cacheFile = join(out, `${v.id}.json`);
    if (!has("fresh-uw") && existsSync(cacheFile)) {
      const c = JSON.parse(readFileSync(cacheFile, "utf8"));
      if (c.cacheKey === cacheKey && c.rec.moments.every((m) => existsSync(m.file))) {
        res.views[v.id] = c.rec;
        res.gpu = res.gpu || c.gpu;
        log(`ultra-water: ${v.id} kept from an earlier run (same settings)`);
        continue;
      }
    }
    let rec = null;
    for (let attempt = 0; attempt < 2 && !rec; attempt++) {
      try {
        if (!ev) await openPage();
        rec = await renderView(v);
      } catch (e) {
        if (!/connection closed|no answer|Target closed|page did not start/i.test(String(e))) throw e;
        log(`ultra-water: ${v.id}: the browser was lost (${String(e).slice(0, 120)})${attempt === 0 ? "; restarting it" : "; view left out"}`);
        ev = null;
        if (attempt === 1) res.failed = { ...(res.failed || {}), [v.id]: String(e).slice(0, 200) };
      }
    }
    if (!rec) continue;
    writeFileSync(cacheFile, JSON.stringify({ cacheKey, gpu, rec }, null, 1));
    res.views[v.id] = rec;
    res.gpu = gpu;
    log(`ultra-water: ${v.id} done (${moments.length} moments${GPU ? `, ${rec.frame_ms?.toFixed(2)} ms/frame` : ""})`);
  }
  return res;

  async function renderView(v) {
    const u = v.ultra_water || {};
    // Placement: open water far out from the island, looking away from it;
    // a shore view on the island's coast at the given bearing.
    let rot = 0, shift;
    const yaw = v.camera.yaw_deg;
    if (v.shore) {
      const b = u.coast_bearing_deg ?? -6;
      const c = await ev(`(() => { const a = ${rad(b)}; let r = 100; while (r < 800 && window.__island(Math.cos(a) * r, Math.sin(a) * r) > 0) r += 2; return [Math.cos(a) * r, Math.sin(a) * r]; })()`);
      rot = b + 180 - v.shore.land_heading_deg;
      const wl = toUw([v.shore.waterline[0], 0, v.shore.waterline[1]], rot, [0, 0, 0]);
      shift = [c[0] - wl[0], 0, c[1] - wl[2]];
    } else {
      shift = [Math.cos(rad(yaw)) * 4000 - v.camera.eye[0], 0, Math.sin(rad(yaw)) * 4000 - v.camera.eye[2]];
    }
    const eye = toUw(v.camera.eye, rot, shift);
    const p = rad(v.camera.pitch_deg), y = rad(yaw + rot);
    const dir = [Math.cos(y) * Math.cos(p), Math.sin(p), Math.sin(y) * Math.cos(p)];
    const target = eye.map((e, i) => e + dir[i] * 60);
    const sunAz = (((90 - (v.sun.azimuth_deg + rot)) % 360) + 360) % 360;
    const waves = { ...(u.waves || {}) };
    if (waves.windDirection !== undefined) waves.windDirection = wrap180(waves.windDirection + rot);
    if (waves.swellDirection !== undefined) waves.swellDirection = wrap180(waves.swellDirection + rot);
    const set = { eye, target, fov: v.camera.fov_deg, sun: { elevation: v.sun.elevation_deg, azimuth: sunAz }, waves, sky: u.sky || {}, water: u.water || {}, post: u.post || {}, quality: u.quality || {} };
    if (has("debug")) log(`  ${v.id}: setting camera, sun and sea`);
    await ev(`(() => {
      const a = window.app, d = window.__defaults, su = a.surface.uniforms, S = ${JSON.stringify(set)};
      Object.assign(a.ocean.params, d.ocean, S.waves); a.ocean.dirty = true;
      Object.assign(a.sky.params, d.sky, S.sky, S.sun); a.markSun();
      Object.assign(a.post.params, d.post, S.post);
      // Quality (resolution, msaa, reflections, reflectionScale): the page rebuilds its targets on resize.
      const q0 = JSON.stringify(a.quality); Object.assign(a.quality, d.quality, S.quality);
      if (JSON.stringify(a.quality) !== q0) window.dispatchEvent(new Event("resize"));
      su.uOptics.value.fromArray(d.optics); su.uSSS.value.fromArray(d.sss); su.uFoam.value.fromArray(d.foam); su.uGlint.value = d.glint;
      const W = S.water;
      if (W.roughness !== undefined) su.uOptics.value.x = W.roughness;
      if (W.refraction !== undefined) su.uOptics.value.y = W.refraction;
      if (W.reflectionDistort !== undefined) su.uOptics.value.z = W.reflectionDistort;
      if (W.sssPeak !== undefined) su.uSSS.value.x = W.sssPeak;
      if (W.sssView !== undefined) su.uSSS.value.y = W.sssView;
      if (W.foamBias !== undefined) su.uFoam.value.x = W.foamBias;
      if (W.foamScale !== undefined) su.uFoam.value.y = W.foamScale;
      if (W.shoreFoamBand !== undefined) su.uFoam.value.z = W.shoreFoamBand;
      if (W.foamRecovery !== undefined) a.ocean.params.foamRecovery = W.foamRecovery;
      if (W.glitter !== undefined) su.uGlint.value = W.glitter;
      a.floaters.clear();
      a.camera.fov = S.fov; a.camera.updateProjectionMatrix();
      a.camera.position.set(...S.eye); a.controls.target.set(...S.target); a.controls.update();
      return true;
    })()`);
    const rec = { set, rot, shift, moments: [] };
    for (const [k, t0] of moments.entries()) {
      let t = t0, png = null, under = 0;
      for (let tries = 0; tries < 20; tries++) {
        // 4 s before the moment: the simulation alone for 116 steps of
        // 1/30 s, then 4 whole frames (the exposure snaps on the first).
        if (has("debug")) log(`  ${v.id}: moment ${k} at ${t.toFixed(1)} s`);
        png = await ev(`(() => {
          const a = window.app;
          a.ocean.time = ((${t} - 4) % 300 + 300) % 300;
          for (let i = 0; i < 116; i++) a.ocean.update(1 / 30);
          a.post._snap = true;
          // No lens droplets from a camera that was under water while moved.
          a.post.params.droplets = 0;
          window.__step(4, 1000 / 30);
          return { cam: a.camera.position.y, water: a.camWater(), pos: a.camera.position.toArray(), n: (window.__img = a.renderer.domElement.toDataURL('image/png')).length };
        })()`);
        // The camera never inside a wave (as on our side): the next 1.3 s.
        if (png.cam > png.water + 1.0 || v.camera.eye[1] > 20) break;
        under++;
        t += 1.3;
      }
      const f = join(out, `${v.id}-m${k}.png`);
      // The picture comes back in 512 KB pieces (one big reply drops the
      // browser connection on busy, high-detail frames).
      let url = "";
      for (let at = 0; at < png.n; at += 524288) url += await ev(`window.__img.slice(${at}, ${at + 524288})`);
      writeFileSync(f, Buffer.from(url.split(",")[1], "base64"));
      rec.moments.push({ file: f, time: t, nudged: under, cam: png.pos });
    }
    const b = GPU ? await ev(`window.runBenchmark(60).then((r) => r && { ms: r.ms, gpu: r.gpu })`) : null;
    rec.frame_ms = b?.ms ?? null;
    return rec;
  }
}

// ------------------------------------------------------------ measures ----
const pct = (a, q) => {
  if (!a.length) return 0;
  const s = Float64Array.from(a).sort();
  return s[Math.min(s.length - 1, Math.floor(q * s.length))];
};
// One picture, seen from the view's camera: horizon row from the pitch and
// field of view (flat sea, no roll); sea = below it.
function measure(file, v) {
  const { width: w, height: h, data } = readPng(readFileSync(file));
  const L = new Float32Array(w * h);
  const sat = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) {
    const r = data[i * 4] / 255, g = data[i * 4 + 1] / 255, b = data[i * 4 + 2] / 255;
    L[i] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const mx = Math.max(r, g, b), mn = Math.min(r, g, b);
    sat[i] = mx > 0 ? (mx - mn) / mx : 0;
  }
  const p = rad(v.camera.pitch_deg), tv = Math.tan(rad(v.camera.fov_deg) / 2), ta = tv * (w / h);
  const hy = (1 - -Math.tan(p) / tv) / 2 * h; // horizon row (fraction rows from top)
  const horizon = Math.round(hy);
  // Crests: per column, scanning down from 30 px over the horizon, the sky's
  // colour (from the 10 rows above the scan, then following the sky's
  // gradient while rows stay sky-like); the first row unlike it for 4 rows
  // running is where the sea starts.
  const crest = [];
  // 30 px: higher, the reference's clouds and haze near the horizon start
  // to count as crests (checked at 30, 45, 60 px on its calm deck view).
  const CREST_SCAN = Number(arg("crest-scan", "30"));
  // (Not on a shore view: the land stands over the horizon.)
  if (horizon > 30 && horizon < h && !v.shore) {
    const start = Math.max(10, horizon - CREST_SCAN);
    for (let x = 0; x < w; x++) {
      let cr = 0, cg = 0, cb = 0, n = 0;
      for (let y = start - 10; y < start; y++) {
        const i = (y * w + x) * 4;
        cr += data[i]; cg += data[i + 1]; cb += data[i + 2]; n++;
      }
      cr /= n; cg /= n; cb /= n;
      let run = 0, top = horizon;
      for (let y = start; y < Math.min(h, horizon + 6); y++) {
        const i = (y * w + x) * 4;
        const d = Math.hypot(data[i] - cr, data[i + 1] - cg, data[i + 2] - cb) / 255;
        if (d > 0.08) run++;
        else {
          run = 0;
          cr += (data[i] - cr) * 0.2; cg += (data[i + 1] - cg) * 0.2; cb += (data[i + 2] - cb) * 0.2;
        }
        if (run === 4) {
          top = y - 3;
          break;
        }
      }
      crest.push(Math.max(0, horizon - top));
    }
  }
  // The sun's glitter path: pixels whose mirror ray off a flat sea is within
  // 14 deg of the sun; and isolated sparkles (bright pixels in a dark 7x7).
  const se = rad(v.sun.elevation_deg), sa = rad(v.sun.azimuth_deg);
  const sun = [Math.cos(sa) * Math.cos(se), Math.sin(se), Math.sin(sa) * Math.cos(se)];
  const yw = rad(v.camera.yaw_deg);
  const f = [Math.cos(yw) * Math.cos(p), Math.sin(p), Math.sin(yw) * Math.cos(p)];
  const rt = [-Math.sin(yw), 0, Math.cos(yw)];
  const up = [rt[1] * f[2] - rt[2] * f[1], rt[2] * f[0] - rt[0] * f[2], rt[0] * f[1] - rt[1] * f[0]];
  const I = new Float64Array((w + 1) * (h + 1));
  for (let y = 0; y < h; y++) for (let x = 0, row = 0; x < w; x++) {
    row += L[y * w + x];
    I[(y + 1) * (w + 1) + x + 1] = I[y * (w + 1) + x + 1] + row;
  }
  const box = (x, y, r) => {
    const x0 = Math.max(0, x - r), y0 = Math.max(0, y - r), x1 = Math.min(w, x + r + 1), y1 = Math.min(h, y + r + 1);
    return (I[y1 * (w + 1) + x1] - I[y0 * (w + 1) + x1] - I[y1 * (w + 1) + x0] + I[y0 * (w + 1) + x0]) / ((x1 - x0) * (y1 - y0));
  };
  const cosCone = Math.cos(rad(14));
  const seaTop = Math.max(0, Math.min(h, horizon + 3));
  let sea = 0, masked = 0, foam = 0, lumSum = 0, lumSq = 0;
  const grads = [];
  const gradTop = Math.max(1, Math.min(h - 1, horizon + Math.round(0.04 * h)));
  for (let y = seaTop; y < h; y++) {
    const ny = 1 - ((y + 0.5) / h) * 2;
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const nx = ((x + 0.5) / w) * 2 - 1;
      const d = [0, 1, 2].map((k) => f[k] + rt[k] * nx * ta + up[k] * ny * tv);
      const dl = Math.hypot(d[0], d[1], d[2]);
      if (d[1] >= 0) continue; // over the horizon after all
      sea++;
      lumSum += L[i];
      lumSq += L[i] * L[i];
      if (y >= gradTop && y < h - 1 && x > 0 && x < w - 1) grads.push(Math.hypot(L[i + 1] - L[i - 1], L[i + w] - L[i - w]) * 50);
      const m = (d[0] * sun[0] - d[1] * sun[1] + d[2] * sun[2]) / dl;
      const sparkle = L[i] > 0.85 && box(x, y, 3) < 0.5;
      if (m > cosCone || sparkle) {
        masked++;
        continue;
      }
      if (L[i] > 0.72 && sat[i] < 0.18) foam++;
    }
  }
  let all = 0;
  for (let i = 0; i < w * h; i++) all += L[i];
  const seaN = Math.max(1, sea);
  const mean = lumSum / seaN;
  return {
    horizon_row: horizon,
    crest_share_pct: crest.length ? +((100 * crest.filter((c) => c > 4).length) / crest.length).toFixed(1) : null,
    crest_p95_px: crest.length ? pct(crest, 0.95) : null,
    foam_pct: +((100 * foam) / Math.max(1, sea - masked)).toFixed(2),
    glint_masked_pct: +((100 * masked) / seaN).toFixed(1),
    grad_mean: grads.length ? +(grads.reduce((a, b) => a + b, 0) / grads.length).toFixed(2) : null,
    grad_p90: grads.length ? +pct(grads, 0.9).toFixed(2) : null,
    sea_luma: +mean.toFixed(3),
    sea_luma_sd: +Math.sqrt(Math.max(0, lumSq / seaN - mean * mean)).toFixed(3),
    frame_luma: +(all / (w * h)).toFixed(3),
  };
}
const KEYS = ["crest_share_pct", "crest_p95_px", "foam_pct", "glint_masked_pct", "grad_mean", "grad_p90", "sea_luma", "sea_luma_sd", "frame_luma"];
function meanOf(list) {
  const o = {};
  for (const k of KEYS) {
    const xs = list.map((m) => m[k]).filter((x) => x !== null && x !== undefined);
    if (!xs.length) {
      o[k] = null;
      continue;
    }
    const mu = xs.reduce((a, b) => a + b, 0) / xs.length;
    const sd = Math.sqrt(xs.reduce((a, b) => a + (b - mu) * (b - mu), 0) / xs.length);
    o[k] = { mean: +mu.toFixed(3), sd: +sd.toFixed(3) };
  }
  return o;
}

// ---------------------------------------------------------------- page ----
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
// The settings both sides share, in one short line: camera, sun, wind,
// fetch, swell and choppiness (Ultra-Water's wave values; ours takes the
// same wind, fetch and swell, its chop from the preset).
function sharedLine(v0, size) {
  // The views file's own entry (the page's view records leave the
  // Ultra-Water block out).
  const v = cfg.views.find((x) => x.id === v0.id) || v0;
  const c = v.camera, w = v.ultra_water?.waves || {};
  return `Camera ${c.eye.join(",")} m, heading ${c.yaw_deg}°, pitch ${c.pitch_deg}°, FOV ${c.fov_deg}°` +
    ` · Sun ${v.sun.elevation_deg}° up, heading ${v.sun.azimuth_deg}°` +
    ` · Wind ${w.windSpeed ?? "?"} m/s from ${w.windDirection ?? "?"}°, fetch ${w.fetchKm ?? "?"} km, swell ${w.swellScale ?? "?"}` +
    `${w.waveScale && w.waveScale !== 1 ? `, height ×${w.waveScale}` : ""} · Choppiness ${w.choppiness ?? "?"}` +
    (size ? ` · ${size.join("×")}` : "");
}
function cellNums(m, ms, msLabel) {
  if (!m) return "<i>no picture</i>";
  const f = (k, d = 1) => (m[k] ? `${m[k].mean.toFixed(d)}<small>±${m[k].sd.toFixed(d)}</small>` : "–");
  return `<table class="m">
<tr><td>Crests over the horizon</td><td>${f("crest_share_pct")} % of width · p95 ${f("crest_p95_px", 0)} px</td></tr>
<tr><td>Foam (glints masked)</td><td>${f("foam_pct", 2)} % <small>(${m.glint_masked_pct ? m.glint_masked_pct.mean.toFixed(0) : "–"} % of sea masked)</small></td></tr>
<tr><td>Light-dark gradient</td><td>mean ${f("grad_mean", 2)} · p90 ${f("grad_p90", 2)}</td></tr>
<tr><td>Luma</td><td>sea ${f("sea_luma", 3)} (spread ${f("sea_luma_sd", 3)}) · frame ${f("frame_luma", 3)}</td></tr>
<tr><td>${esc(msLabel)}</td><td>${ms === null || ms === undefined ? "<small>not measured (CPU run; --gpu times it)</small>" : ms.toFixed(2) + " ms"}</td></tr>
</table>`;
}
function writePage(data) {
  const rows = data.views.map((v) => {
    const cells = v.cells.map((c) => `<figure><span class="tag">${esc(c.label)}</span>${c.img ? `<a href="${c.img}"><img src="${c.img}" alt="${esc(c.label + " " + v.title)}"></a>` : "<div class='none'>not rendered</div>"}<figcaption>${cellNums(c.mean, c.ms, c.msLabel)}</figcaption></figure>`).join("");
    const cam = v.camera;
    const ours = Object.entries(v.ours_applied || {}).filter(([k]) => /wind|fetch|swell|chop|wave_height|mid_waves|foam|whitecap|beach|breaker/.test(k)).map(([k, x]) => `${k.replace("material.nature.water.", "")} ${Array.isArray(x) ? x.join("/") : x}`).join(", ");
    const uw = v.uw ? Object.entries(v.uw.set.waves).map(([k, x]) => `${k} ${typeof x === "number" ? +x.toFixed(2) : x}`).join(", ") + (Object.keys(v.uw.set.water).length ? "; " + Object.entries(v.uw.set.water).map(([k, x]) => `${k} ${x}`).join(", ") : "") : "";
    return `<section><h2>${esc(v.title)}</h2>
<p class="set"><b>Camera</b> eye ${cam.eye.join(", ")} m · heading ${cam.yaw_deg}° · pitch ${cam.pitch_deg}° · vertical FOV ${cam.fov_deg}° · ${data.size.join("×")}
 · <b>Sun</b> ${v.sun.elevation_deg}° up, heading ${v.sun.azimuth_deg}° · <b>Sea</b> ${esc(v.sea_state)}<br>
<b>Ultra-Water</b> ${esc(uw)}${v.uw ? ` · sun azimuth ${v.uw.set.sun.azimuth.toFixed(1)}° (its own convention)` : ""}<br>
<b>Ours</b> preset <code>${esc(v.preset)}</code>: ${esc(ours)}</p>
${v.uw_failed ? `<p class="set" style="color:#ffb27a"><b>Ultra-Water not drawn:</b> its page crashes the CPU renderer (SwiftShader) when the camera is high up; re-run this view with --gpu (needs the owner to allow the graphics card).</p>` : ""}
<div class="row">${cells}</div>
<p class="shared"><b>Shared settings (both sides):</b> ${esc(sharedLine(v, data.size))}</p></section>`;
  });
  const html = `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ocean comparison: Ultra-Water vs ours</title>
<style>
body{font:14px/1.45 system-ui,sans-serif;margin:0;padding:16px clamp(8px,2vw,32px);background:#111418;color:#dde3ea}
h1{font-size:20px;margin:0 0 4px} h2{font-size:16px;margin:24px 0 4px} p{margin:4px 0;max-width:1400px}
.set{font-size:12.5px;color:#aab5c2} code{color:#ffd27a}
.row{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:12px;max-width:2600px}
figure{margin:0;background:#1a1f26;border-radius:8px;overflow:hidden;position:relative;min-width:0}
figure img{display:block;width:100%;height:auto;aspect-ratio:16/9;object-fit:cover}
.none{aspect-ratio:16/9;display:grid;place-items:center;color:#889}
.tag{position:absolute;left:8px;top:8px;background:#000a;padding:2px 8px;border-radius:4px;font-weight:600;font-size:12.5px}
figcaption{padding:6px 8px}
table.m{border-collapse:collapse;width:100%;font-size:12.5px} table.m td{padding:2px 4px;vertical-align:top} table.m td:first-child{color:#99a6b4;white-space:nowrap;width:40%}
small{color:#8895a3} .note{font-size:12.5px;color:#aab5c2}
.shared{font-size:12.5px;color:#cfe3f5;background:#18212b;border-left:3px solid #ffd27a;padding:4px 8px;margin-top:8px;max-width:2600px;overflow-wrap:anywhere}
</style>
<h1>Ocean comparison: Ultra-Water (three.js) vs our spectral sea</h1>
<p class="note" style="font-size:14px;color:#ffd27a">Ours: <b>${esc(data.ours.label)}</b>, commit <code>${esc((data.ours.commit || "").slice(0, 8))}</code> · rendered ${esc(data.when)} on the ${data.gpu ? "graphics card" : "CPU (SwiftShader)"}, both sides</p>
<p class="note">Rendered ${esc(data.when)} · ours: <b>${esc(data.ours.label)}</b> · Ultra-Water from its own code (MIT, © 2026 Humpalumps) at ${esc(data.uw.dir)} on ${esc(data.uw.gpu || "")} · our harness on ${esc(JSON.stringify(data.ours.adapter || {}))}.
Every view uses the same camera, sun and sea state on both sides (views file <code>tools/nature/compare-views.json</code>). Pictures show moment 0; every number is the mean ± spread over ${data.moments.length} moments (sim seconds ${data.moments.join(", ")}).
Re-run: <code>node tools/nature/compare.mjs --ours &lt;commit|branch|path&gt;</code>.</p>
<p class="note"><b>How to read the numbers.</b> Crests: per column, how far the sea's top edge stands over the camera's horizon line (measured up to 30 px over it, so a p95 of 30 means taller; higher up the reference's clouds would count; not on the shore view, where land stands there); share of columns over 4 px and the 95th percentile. Foam: share of sea pixels that are white (luma over 0.72, saturation under 0.18), leaving out the sun's glitter path (mirror rays within 14° of the sun) and isolated sparkles. Gradient: the luma change per pixel ×50 below the horizon band (busy, steep seas score higher). Frame time: Ultra-Water's whole frame (its own fence-timed benchmark: simulation, island, water, post); ours is the sea draw alone in the GPU harness, so the two are not the same measure.
Known differences of the set-ups: Ultra-Water has its own physical sky, clouds, island and auto exposure; our harness draws a stand-in sky (gradient, sun disc, clouds) and tone-maps with a fixed exposure. Both seas are random with fixed seeds, so the same moment is not the same wave: compare the means.</p>
${rows.join("\n")}
</html>`;
  writeFileSync(join(pageDir, "index.html"), html);
  writeFileSync(join(pageDir, "data.json"), JSON.stringify(data, null, 1));
}

// Phone pictures: per view one labelled JPG, Ultra-Water above ours (Ultra),
// 1080 px wide and under 600 KB, in <main checkout>/.evani/shots/phone
// (same-settings-<view>.jpg). Drawn and encoded by a headless, muted Chrome's 2D
// canvas (no graphics card needed).
async function phoneShots(data) {
  const dir = resolve(arg("phone", join(mainCheckout, ".evani", "shots", "phone")));
  mkdirSync(dir, { recursive: true });
  const chrome = await startChrome({ width: 800, height: 600, flags: CPU_FLAGS, prefix: "evani-cmp-phone-" });
  cleanups.push(chrome.stop);
  const { cdp } = chrome;
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const files = [];
  const num = (m, k, d = 1) => (m && m[k] ? m[k].mean.toFixed(d) : "–");
  for (const v of data.views) {
    const [uwc, ourc] = v.cells;
    const pic = (c) => (c?.img ? "data:image/png;base64," + readFileSync(join(pageDir, c.img)).toString("base64") : null);
    const line = (c) => `crests ${num(c?.mean, "crest_share_pct")}% · foam ${num(c?.mean, "foam_pct", 2)}% · gradient ${num(c?.mean, "grad_mean", 2)} · luma ${num(c?.mean, "sea_luma", 2)}${c?.ms ? ` · ${c.ms.toFixed(2)} ms` : ""}`;
    const job = {
      title: v.title,
      parts: [
        { src: pic(uwc), label: "Ultra-Water (three.js)", nums: line(uwc) },
        { src: pic(ourc), label: `Evani.Dev (ours) · Ultra · ${(data.ours.commit || "").slice(0, 8)}`, nums: line(ourc) },
      ],
      foot: `Same settings: ${sharedLine(v)}`,
    };
    const r = await cdp.send("Runtime.evaluate", {
      expression: `(async (J) => {
        const W = 1080, IH = 608, BAR = 46, NUM = 30, TOP = 54, FOOT = 88;
        const H = TOP + J.parts.length * (BAR + IH + NUM) + FOOT;
        const c = new OffscreenCanvas(W, H), g = c.getContext('2d');
        g.fillStyle = '#111418'; g.fillRect(0, 0, W, H);
        g.fillStyle = '#fff'; g.font = '600 28px system-ui, sans-serif'; g.fillText(J.title, 16, 38);
        let y = TOP;
        for (const p of J.parts) {
          g.fillStyle = '#1f2a36'; g.fillRect(0, y, W, BAR);
          g.fillStyle = '#ffd27a'; g.font = '600 24px system-ui, sans-serif'; g.fillText(p.label, 16, y + 32);
          y += BAR;
          if (p.src) { const b = await createImageBitmap(await (await fetch(p.src)).blob()); g.drawImage(b, 0, y, W, IH); }
          else { g.fillStyle = '#333'; g.fillRect(0, y, W, IH); g.fillStyle = '#bbb'; g.font = '24px system-ui'; g.fillText('not drawn', W / 2 - 50, y + IH / 2); }
          y += IH;
          g.fillStyle = '#c8d2dc'; g.font = '20px system-ui, sans-serif'; g.fillText(p.nums, 16, y + 22);
          y += NUM;
        }
        // The shared settings, wrapped onto as many short lines as they need.
        g.fillStyle = '#8f9cab'; g.font = '18px system-ui, sans-serif';
        let ln = '', ly = y + 26;
        for (const word of J.foot.split(' ')) {
          if (g.measureText(ln + word).width > W - 32 && ln) { g.fillText(ln.trimEnd(), 16, ly); ln = ''; ly += 24; }
          ln += word + ' ';
        }
        g.fillText(ln.trimEnd(), 16, ly);
        let q = 0.86, blob;
        do { blob = await c.convertToBlob({ type: 'image/jpeg', quality: q }); q -= 0.06; } while (blob.size > 590000 && q > 0.3);
        const u = new Uint8Array(await blob.arrayBuffer());
        let s = ''; for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000));
        return btoa(s);
      })(${JSON.stringify(job).replace(/[\u0080-\uffff]/g, (ch) => "\\u" + ch.charCodeAt(0).toString(16).padStart(4, "0"))})`,
      awaitPromise: true,
      returnByValue: true,
    }, s, 120000);
    if (r.exceptionDetails) throw new Error("phone picture: " + (r.exceptionDetails.exception?.description || r.exceptionDetails.text));
    const f = join(dir, `${arg("phone-prefix", "same-settings-")}${v.id}.jpg`);
    writeFileSync(f, Buffer.from(r.result.value, "base64"));
    files.push(f);
  }
  await chrome.stop();
  return files;
}

// ---------------------------------------------------------------- main ----
const t0 = Date.now();
let code = 0;
try {
  const prev = existsSync(join(work, "last-run.json")) ? JSON.parse(readFileSync(join(work, "last-run.json"), "utf8")) : {};
  const ours = has("skip-ours") ? lastOurs() : await renderOurs();
  const uw = has("skip-uw") ? prev.uw : await renderUw();
  await stopAll();
  writeFileSync(join(work, "last-run.json"), JSON.stringify({ ours, uw }, null, 1));
  log("measuring");
  const data = { when: new Date().toISOString().slice(0, 16).replace("T", " "), gpu: GPU, size: cfg.size, moments, ours: { label: ours?.label, commit: ours?.commit, adapter: ours?.adapter }, uw: { dir: uwDir, gpu: uw?.gpu }, views: [] };
  for (const v of views) {
    const cells = [];
    const uwv = uw?.views?.[v.id];
    const uwFiles = uwv ? uwv.moments.map((m) => m.file) : [];
    cells.push({ label: "Ultra-Water", files: uwFiles, ms: uwv?.frame_ms ?? null, msLabel: "Frame time (whole frame)" });
    for (const lv of [3, 2]) {
      const files = moments.map((_, k) => join(ours?.shots || "", `${v.id}-m${k}-${lv}.png`));
      const ms = moments.map((_, k) => ours?.timing?.[`${v.id}-m${k}-${lv}`]?.sea_ms).filter((x) => x !== undefined);
      cells.push({ label: `Ours (${LEVELS[lv]})`, files, ms: ms.length ? ms.reduce((a, b) => a + b, 0) / ms.length : null, msLabel: "Sea draw time" });
    }
    for (const c of cells) {
      const ok = c.files.filter((f) => f && existsSync(f));
      c.per = ok.map((f) => measure(f, v));
      c.mean = c.per.length ? meanOf(c.per) : null;
      if (ok[0]) {
        const name = `${v.id}-${c.label.replace(/[^A-Za-z]+/g, "-").toLowerCase().replace(/-+$/, "")}.png`;
        copyFileSync(ok[0], join(pageDir, "img", name));
        c.img = "img/" + name;
      }
      delete c.files;
    }
    data.views.push({ id: v.id, title: v.title, camera: v.camera, sun: v.sun, sea_state: v.sea_state, preset: v.ours?.preset, ours_applied: ours?.applied?.[v.id], uw: uwv ? { set: uwv.set, rot: uwv.rot, shift: uwv.shift, moments: uwv.moments.map((m) => ({ time: m.time, nudged: m.nudged, cam: m.cam })) } : null, uw_failed: uw?.failed?.[v.id] || null, ours_times: moments.map((_, k) => ours?.times?.[`${v.id}-m${k}`]), cells });
  }
  writePage(data);
  if (!has("no-phone")) for (const f of await phoneShots(data)) log("phone:", f);
  log(`page: ${join(pageDir, "index.html")} (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
} catch (e) {
  console.error(String((e && e.stack) || e));
  code = 1;
}
await stopAll();
process.exit(code);
