// The Evani.commons server's own pictures of a studio-object asset, shot on this PC before a round is scored.
//
// Why (Pebbles, column 71, 2026-11-11): round 3 scored 76/100 on a picture from the asset's dev scene
// (examples/proc-field, feature `pebbles`: a 20 degree lens, a grey sweep, its own key light and contact
// passes). The server drops examples/proc-field/** from every package and shoots the asset alone in the
// render app examples/proc-field/apps/commons-render with its own orbit cameras and sun, so the published
// version showed smaller, darker stones under a sky band and scored 3.0. A round is now shot and scored
// through the server's staging instead (docs/ASSET_STUDIO_GUIDE.md "Server views").
//
//   node tools/studio/server-views.mjs build --root <worktree> --asset <library key> --dist <dist>
//        trunk build of commons-render with only that asset's library feature, as the server builds it.
//        --offscreen also builds <dist>-offscreen (the same plus `live`), the backup for shoot: when a
//        browser screenshot is unusable, asset-shoot.mjs retakes it through the engine's own read-back.
//   node tools/studio/server-views.mjs shoot --dist <dist> --out <folder> [--asset <key>] [--seed 1] [--size 1536x1024]
//        the six views of the server's object plan: view0 (the asset's reference camera, `camera` in
//        catalog/procedural/references.json, as the server holds it; none: yaw 0, pitch 20, the framing
//        distance, sun 135/40), low, behind, close, edge and light
//        (the sun moved), at the server's size, plus stats.json and sheet.png (all six, labelled)
//   node tools/studio/server-views.mjs plan [--asset <key>] [--seed 1] [--camera '{"cam_yaw_deg":0,...}'] [--studio '{...}']
//        prints the shots file only
//
// Per-object studio (2026-11-11): an object's `studio` in references.json (its reference picture's backdrop and floor
// grey and warmth, measured with tools/studio/studio-measure.py) goes on every view as kit.proc_field.studio_backdrop,
// studio_floor and studio_warmth, and a camera's `cam_aim_y` and `cam_lens_deg` as kit.proc_field.cam_aim_y and
// cam_lens_deg, as the server passes them; an asset without them gets the same params as before.
//
// The plan copies Evani.App's drawPlan (platform/server/src/commons.ts, "object" kind) and its xorshift
// draws: view0 and the light family are the server's exactly; the hidden views' angles are drawn per job
// on the server, so a fixed seed here gives views from the same ranges, not the same angles.
// Headless, muted Chrome through asset-shoot.mjs and shot-sheet.mjs (each closes its own browser). Run it
// through run-low.mjs; its children inherit the low priority.
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** The server's camera when it holds none for an item (commons.ts `DEFAULT_CAMERA`). */
export const DEFAULT_CAMERA = Object.freeze({ cam_yaw_deg: 0, cam_pitch_deg: 20, cam_dist_scale: 1, sun_yaw_deg: 135, sun_pitch_deg: 40 });
export const SERVER_BASE = DEFAULT_CAMERA;
/** The bounds a reference camera is held to (commons.ts `CAMERA_RANGES`; evani-guide's reference_cameras.rs).
 *  `cam_aim_y` (the aim above the asset's centre, in bounds radii) and `cam_lens_deg` (the vertical lens) are optional:
 *  left out, the render app's own (0 and 50) stand and no param is sent. */
export const CAMERA_RANGES = Object.freeze({
  cam_yaw_deg: [-360, 360],
  cam_pitch_deg: [-10, 85],
  cam_dist_scale: [0.3, 3],
  sun_yaw_deg: [-360, 360],
  sun_pitch_deg: [5, 89],
  cam_aim_y: [-1, 1],
  cam_lens_deg: [10, 90],
});
/** The camera keys a view only carries when the camera sets them, and their params. */
export const OPTIONAL_CAMERA = Object.freeze({ cam_aim_y: "kit.proc_field.cam_aim_y", cam_lens_deg: "kit.proc_field.cam_lens_deg" });

/** commons.ts `readCameraParams`: any of the params over the default; none, or one out of range: null. */
export function readCameraParams(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const out = { ...DEFAULT_CAMERA };
  let any = false;
  for (const [k, [lo, hi]] of Object.entries(CAMERA_RANGES)) {
    const v = raw[k];
    if (v === undefined) continue;
    if (typeof v !== "number" || !Number.isFinite(v) || v < lo || v > hi) return null;
    out[k] = v;
    any = true;
  }
  return any ? out : null;
}

/** The bounds an object's studio is held to (evani-guide's reference_cameras.rs `STUDIO_RANGES`): 8-bit greys as its
 *  reference shows them, and red minus blue. */
export const STUDIO_RANGES = Object.freeze({ backdrop_grey: [40, 230], floor_grey: [40, 230], warmth: [-40, 40] });
/** The render app's params for a studio's values. */
export const STUDIO_PARAMS = Object.freeze({ backdrop_grey: "kit.proc_field.studio_backdrop", floor_grey: "kit.proc_field.studio_floor", warmth: "kit.proc_field.studio_warmth" });

/** An object's studio: both greys (and the warmth, default 0) in range; anything else: null (the sweep's own). */
export function readStudio(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const out = { warmth: 0 };
  for (const [k, [lo, hi]] of Object.entries(STUDIO_RANGES)) {
    const v = raw[k];
    if (v === undefined && k === "warmth") continue;
    if (typeof v !== "number" || !Number.isFinite(v) || v < lo || v > hi) return null;
    out[k] = v;
  }
  for (const k of Object.keys(raw)) if (!(k in STUDIO_RANGES)) return null;
  return out;
}

/** A studio's view params (none for no studio). */
export function studioParams(studio) {
  const st = readStudio(studio);
  if (!st) return {};
  const r1 = (n) => Math.round(n * 10) / 10;
  return Object.fromEntries(Object.entries(STUDIO_PARAMS).map(([k, p]) => [p, r1(st[k])]));
}

/** The library feature of a key (`rocks-pebbles@photoreal` -> `rocks-pebbles`). */
export const featureOf = (key) => String(key).split("@")[0];

/** An asset's entry in catalog/procedural/references.json, or null. */
function referenceEntry(root, asset) {
  const file = join(resolve(root), "catalog", "procedural", "references.json");
  if (!asset || !existsSync(file)) return null;
  return JSON.parse(readFileSync(file, "utf8"))?.assets?.[featureOf(asset)] ?? null;
}
/** The reference camera catalog/procedural/references.json holds for an asset (as the server reads it), or null. */
export function referenceCamera(root, asset) {
  return readCameraParams(referenceEntry(root, asset)?.camera);
}
/** The studio catalog/procedural/references.json holds for an asset, or null. */
export function referenceStudio(root, asset) {
  return readStudio(referenceEntry(root, asset)?.studio);
}
/** The server's picture size for an item without its own (commons.ts drawPlan's `size`). */
export const SERVER_SIZE = "1536x1024";

/** A JPEG's or PNG's pixel size from its header, or null. */
export function pictureSize(buf) {
  if (buf.length > 24 && buf.readUInt32BE(0) === 0x89504e47) return [buf.readUInt32BE(16), buf.readUInt32BE(20)];
  if (buf[0] !== 0xff || buf[1] !== 0xd8) return null;
  let i = 2;
  while (i + 9 < buf.length) {
    if (buf[i] !== 0xff) return null;
    const m = buf[i + 1];
    if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) return [buf.readUInt16BE(i + 7), buf.readUInt16BE(i + 5)];
    i += 2 + buf.readUInt16BE(i + 2);
  }
  return null;
}

/** The size an object is shot at: 1536 wide in its reference picture's shape (the cube's reference is 16:9:
 *  1536x864), else the server's default. Evani.App's seed carries it as the item's `size`. */
export function referenceShotSize(root, asset) {
  const ref = referenceEntry(root, asset)?.reference;
  const file = ref && join(resolve(root), "apps", "material-view", "assets", ref);
  if (!file || !existsSync(file)) return SERVER_SIZE;
  const wh = pictureSize(readFileSync(file));
  if (!wh || !wh[0] || !wh[1]) return SERVER_SIZE;
  return `1536x${Math.round((1536 * wh[1]) / wh[0] / 2) * 2}`;
}
/** The order and names of the server's object views. */
export const VIEW_NAMES = ["view0", "low", "behind", "close", "edge", "light"];

/** The server's object plan: [{ name, params }], the same params and draw order as drawPlan; a studio's params and a
 *  camera's aim and lens on every view. */
export function objectPlan(seed = 1, camera = null, studio = null) {
  let x = seed >>> 0 || 1;
  const rnd = (lo, hi) => {
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    return lo + ((x % 10_000) / 10_000) * (hi - lo);
  };
  const r1 = (n) => Math.round(n * 10) / 10;
  // The server's base: the item's camera read as commons.ts reads it (missing values the default's), else the default.
  const base = readCameraParams(camera) ?? DEFAULT_CAMERA;
  const fixed = {
    ...Object.fromEntries(Object.entries(OPTIONAL_CAMERA).filter(([k]) => base[k] !== undefined).map(([k, p]) => [p, Math.round(base[k] * 1000) / 1000])),
    ...studioParams(studio),
  };
  const v = (p) => {
    const all = { ...base, ...p };
    return {
      "kit.proc_field.cam_free": 1,
      "kit.proc_field.sun_free": 1,
      "kit.proc_field.cam_yaw_deg": r1(all.cam_yaw_deg),
      "kit.proc_field.cam_pitch_deg": r1(all.cam_pitch_deg),
      "kit.proc_field.cam_dist_scale": Math.round(all.cam_dist_scale * 1000) / 1000,
      "kit.proc_field.sun_yaw_deg": r1(all.sun_yaw_deg),
      "kit.proc_field.sun_pitch_deg": r1(all.sun_pitch_deg),
      ...fixed,
    };
  };
  // The same calls in the same order as the server, so its draws line up.
  const low = (() => { const p = rnd(4, 12); return v({ cam_pitch_deg: p, cam_yaw_deg: base.cam_yaw_deg + rnd(-30, 30) }); })();
  const behind = v({ cam_yaw_deg: base.cam_yaw_deg + rnd(150, 210) });
  const close = (() => { const d = base.cam_dist_scale * rnd(0.55, 0.7); return v({ cam_dist_scale: d, cam_yaw_deg: base.cam_yaw_deg + rnd(-20, 20) }); })();
  const edge = v({ cam_yaw_deg: base.cam_yaw_deg + 90, cam_pitch_deg: Math.min(base.cam_pitch_deg, 8) });
  const light = (() => { const y = base.sun_yaw_deg + rnd(90, 150); return v({ sun_yaw_deg: y, sun_pitch_deg: rnd(20, 50) }); })();
  return [
    { name: "view0", params: v({}) },
    { name: "low", params: low },
    { name: "behind", params: behind },
    { name: "close", params: close },
    { name: "edge", params: edge },
    { name: "light", params: light },
  ];
}

/** asset-shoot.mjs's shots file for a plan (each view with the asset; subject_off 0 as the server sets it). */
export function shotsFile(plan) {
  return plan.map((v) => ({ view: 0, file: `${v.name}.png`, params: { ...v.params, "kit.proc_field.subject_off": 0 } }));
}

const here = dirname(fileURLToPath(import.meta.url));
const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] !== undefined && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
function run(cmd, args, cwd) {
  const r = spawnSync(cmd, args, { cwd, stdio: "inherit", windowsHide: true, shell: process.platform === "win32" && cmd === "trunk" });
  if (r.status !== 0) {
    console.error(`server-views: ${cmd} ${args.slice(0, 2).join(" ")} ... failed (${r.status ?? r.error})`);
    process.exit(1);
  }
}

function main() {
  const cmd = process.argv[2];
  const seed = Number(arg("seed", "1"));
  // --camera wins; else the asset's camera in references.json (--root, default this checkout); else the default.
  const camera = arg("camera") ? JSON.parse(arg("camera")) : referenceCamera(arg("root", join(here, "..", "..")), arg("asset"));
  if (arg("camera") && !readCameraParams(camera)) {
    console.error(`server-views: --camera is not a valid reference camera (${Object.entries(CAMERA_RANGES).map(([k, [lo, hi]]) => `${k} ${lo}..${hi}`).join(", ")})`);
    process.exit(2);
  }
  // --studio wins (`none`: the sweep's own); else the asset's studio in references.json; else none.
  const studio = arg("studio") ? (arg("studio") === "none" ? null : JSON.parse(arg("studio"))) : referenceStudio(arg("root", join(here, "..", "..")), arg("asset"));
  if (arg("studio") && arg("studio") !== "none" && !readStudio(studio)) {
    console.error(`server-views: --studio is not a valid studio (${Object.entries(STUDIO_RANGES).map(([k, [lo, hi]]) => `${k} ${lo}..${hi}`).join(", ")}; both greys)`);
    process.exit(2);
  }
  if (cmd === "plan") {
    console.log(JSON.stringify(shotsFile(objectPlan(seed, camera, studio)), null, 1));
  } else if (cmd === "build") {
    const root = arg("root"), asset = arg("asset"), dist = arg("dist");
    if (!root || !asset || !dist) return usage();
    const app = join(resolve(root), "examples", "proc-field", "apps", "commons-render");
    if (!existsSync(app)) {
      console.error(`server-views: no commons-render app in ${root}`);
      process.exit(2);
    }
    console.log(`server-views: building commons-render with evani-procedural-lib/${asset} (the server's build: no dev scene)`);
    run("trunk", ["build", "--release", "--no-default-features", "--features", `evani-procedural-lib/${asset}`, "--public-url", "./", "--dist", resolve(dist)], app);
    if (process.argv.includes("--offscreen")) {
      console.log(`server-views: building the off-screen backup ${resolve(dist)}-offscreen (the same plus live)`);
      run("trunk", ["build", "--release", "--no-default-features", "--features", `evani-procedural-lib/${asset},live`, "--public-url", "./", "--dist", resolve(dist) + "-offscreen"], app);
    }
  } else if (cmd === "shoot") {
    const dist = arg("dist"), out = arg("out");
    if (!dist || !out) return usage();
    const size = arg("size", referenceShotSize(arg("root", join(here, "..", "..")), arg("asset")));
    mkdirSync(out, { recursive: true });
    const plan = objectPlan(seed, camera, studio);
    const file = join(out, "server-views.json");
    writeFileSync(file, JSON.stringify(shotsFile(plan), null, 1));
    console.log(`server-views: shooting ${plan.length} views at ${size} (seed ${seed}; view0 ${JSON.stringify(readCameraParams(camera) ?? DEFAULT_CAMERA)}; studio ${JSON.stringify(readStudio(studio) ?? "the sweep's own")})`);
    const shoot = spawnSync(process.execPath, [join(here, "asset-shoot.mjs"), "--dist", dist, "--out", out, "--size", size, "--shots-file", file, "--stats", join(out, "stats.json"), "--warm-ms", arg("warm-ms", "12000"), ...(arg("offscreen-dist") ? ["--offscreen-dist", arg("offscreen-dist")] : [])], { stdio: "inherit", windowsHide: true });
    if (shoot.status === 4) {
      console.error(`server-views: no usable picture of some views by either path (browser or engine off-screen); the reasons are above and in ${join(out, "capture-paths.json")}`);
      process.exit(4);
    }
    if (shoot.status !== 0) {
      console.error(`server-views: asset-shoot.mjs failed (${shoot.status ?? shoot.error})`);
      process.exit(1);
    }
    const pics = plan.map((v) => join(out, `${v.name}.png`));
    run(process.execPath, [join(here, "shot-sheet.mjs"), "--out", join(out, "sheet.png"), "--cols", "3", "--width", "1800", "--title", "Server views (Evani.commons staging): view0, low, behind, close, edge, light", ...pics]);
    console.log(`server-views: done: ${join(out, "view0.png")} (the round picture), ${join(out, "sheet.png")}`);
  } else usage();
}
function usage() {
  console.error("usage: node tools/studio/server-views.mjs build --root <worktree> --asset <key> --dist <dist> | shoot --dist <dist> --out <folder> [--asset <key>] [--seed N] [--camera <json>] [--studio <json>|none] | plan [--asset <key>] [--seed N] [--camera <json>] [--studio <json>|none]");
  process.exit(2);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
