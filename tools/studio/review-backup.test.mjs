// node --test tools/studio/review-backup.test.mjs (also run by `cargo test -p evani-xtask commons`).
// Evani.App's backup capture hook (.vibedeck/visual-capture.json -> tools/studio/review-backup.mjs), run the way
// Evani.App runs it (claude-code-desktop src/main/qqBackupCaptureCore.ts: no shell, cwd the work's folder, {out} and
// {root} filled, EVANI_CAPTURE_* in the env). A stand-in shooter replaces asset-shoot.mjs (no graphics card here);
// the real one is proven by tools/studio/asset-shoot-fallback-check.mjs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, isAbsolute } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { checkNotes, encodePng } from "./visual-review.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const box = () => mkdtempSync(join(tmpdir(), "review-backup-"));
const noise = (w, h, seed) => {
  const px = Buffer.alloc(w * h * 4);
  let s = seed >>> 0;
  for (let i = 0; i < w * h; i++) { s = (s * 1664525 + 1013904223) >>> 0; const v = 40 + (s >>> 24) % 180; px[i * 4] = v; px[i * 4 + 1] = (v * 7) & 255; px[i * 4 + 2] = (v * 13) & 255; px[i * 4 + 3] = 255; }
  return encodePng(w, h, px);
};

// The stand-in for asset-shoot.mjs: honours --out and --shots-file, writes a picture per shot and capture-paths.json.
const STUB = `
import { readFileSync, writeFileSync } from "node:fs"; import { join } from "node:path";
import { encodePng } from ${JSON.stringify(pathToFileURL(join(ROOT, "tools/studio/visual-review.mjs")).href)};
const a = (k) => process.argv[process.argv.indexOf("--" + k) + 1];
const out = a("out"), shots = JSON.parse(readFileSync(a("shots-file"), "utf8")); const [w, h] = a("size").split("x").map(Number);
writeFileSync(join(out, "args.json"), JSON.stringify(process.argv.slice(2)));
const paths = {};
for (const s of shots) {
  if (process.env.STUB_FAIL) { paths[s.file] = { path: "failed", why: "browser: black (mean brightness 0 of 255); offscreen: capture.frame: the GPU device was lost" }; continue; }
  const px = Buffer.alloc(w * h * 4); for (let i = 0; i < w * h; i++) { const v = 30 + (i * 7919) % 200; px[i*4] = v; px[i*4+1] = (v*3)&255; px[i*4+2] = (v*5)&255; px[i*4+3] = 255; }
  writeFileSync(join(out, s.file), encodePng(w, h, px)); paths[s.file] = { path: "offscreen", primary_problem: "black" };
}
writeFileSync(join(out, "capture-paths.json"), JSON.stringify(paths));
process.exit(process.env.STUB_FAIL ? 4 : 0);
`;

// A helper's earlier shoot: pictures, its notes and the recipe asset-shoot.mjs leaves beside them.
function helperShots(d, { recipe = true } = {}) {
  const shots = join(d, "work", ".evani", "shots", "fog");
  mkdirSync(shots, { recursive: true });
  writeFileSync(join(shots, "fog.png"), noise(48, 27, 1));
  writeFileSync(join(shots, "ref.png"), noise(48, 27, 2));
  writeFileSync(join(shots, "visual-review.json"), JSON.stringify({ views: [{ name: "Fog at the reference view", parts: [{ label: "Stored reference", role: "reference", picture: "ref.png" }, { label: "Ours", picture: "fog.png", numbers: { bytes_same_as_earlier_round: false } }] }], numbers_from: "x" }));
  if (recipe) writeFileSync(join(shots, "capture-recipe.json"), JSON.stringify({ tool: "tools/studio/asset-shoot.mjs", dist: join(d, "dist"), offscreen_dist: "", size: "48x27", tier: "high", view_param: "kit.proc_field.fogstage_view", params: { "kit.proc_field.fogstage_asset": 1 }, settle_ms: 3000, frames: 40, warm_ms: 0, shots: [{ view: 0, file: "fog.png" }, { view: 1, file: "close.png" }] }));
  return shots;
}

// Run the declared command as Evani.App does.
function runHook(d, files, extraEnv = {}) {
  const decl = JSON.parse(readFileSync(join(ROOT, ".vibedeck", "visual-capture.json"), "utf8"));
  const out = join(d, "round", "backup-command");
  mkdirSync(out, { recursive: true });
  const work = join(d, "work");
  const fill = (a) => a.split("{out}").join(out).split("{root}").join(work);
  writeFileSync(join(d, "stub.mjs"), STUB);
  // The declared args name the script relative to the project; run from the project root (Evani.App's cwd is the work's folder,
  // which for this project is a checkout holding tools/studio).
  const r = spawnSync(decl.command, decl.args.map(fill), {
    cwd: ROOT, shell: false, windowsHide: true, encoding: "utf8", timeout: decl.timeoutSec * 1000,
    env: { ...process.env, EVANI_CAPTURE_OUT: out, EVANI_CAPTURE_ROOT: work, EVANI_CAPTURE_FILES: JSON.stringify(files), EVANI_CAPTURE_HIDDEN: "1", EVANI_CAPTURE_MUTED: "1", EVANI_BACKUP_SHOOTER: join(d, "stub.mjs"), ...extraEnv },
  });
  return { r, out };
}
const inside = (root, f) => { const rel = relative(root, f); return !!rel && !rel.startsWith("..") && !isAbsolute(rel); };

test("the declaration is what Evani.App accepts: node with no shell script, {out} in the args, at most 10 minutes", () => {
  const decl = JSON.parse(readFileSync(join(ROOT, ".vibedeck", "visual-capture.json"), "utf8"));
  assert.equal(decl.command, "node");
  assert.ok(decl.args.includes("{out}"));
  assert.ok(existsSync(join(ROOT, decl.args[0])), decl.args[0]);
  assert.ok(decl.timeoutSec > 0 && decl.timeoutSec <= 600);
  assert.match(decl.about, /off-screen/);
});

test("the hook re-shoots the helper's listed pictures into {out} and leaves notes Evani.App accepts", () => {
  const d = box();
  const shots = helperShots(d);
  const { r, out } = runHook(d, [join(shots, "visual-review.json"), "src/changed.rs"]);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stderr, /hidden and muted \(EVANI_CAPTURE_HIDDEN=1, EVANI_CAPTURE_MUTED=1\)/);
  const notes = JSON.parse(readFileSync(join(out, "visual-review.json"), "utf8"));
  const check = checkNotes(notes, join(out, "visual-review.json"));
  assert.equal(check.ok, true, check.problems.join("; "));
  assert.equal(notes.views.length, 1); // only the listed picture; the stored reference is never re-shot
  assert.equal(notes.views[0].name, "Fog at the reference view"); // the helper's own view name
  const part = notes.views[0].parts[0];
  assert.equal(part.label, "Ours");
  assert.equal(part.numbers.capture_path, "offscreen");
  assert.equal(part.numbers.bytes_same_as_earlier_round, undefined); // Evani.App works that out itself
  assert.ok(inside(out, resolve(out, part.picture)), part.picture);
  // The same camera and staging as the helper's shoot.
  const args = JSON.parse(readFileSync(join(out, "shots-1", "args.json"), "utf8"));
  const at = (k) => args[args.indexOf("--" + k) + 1];
  assert.equal(at("size"), "48x27");
  assert.equal(at("view-param"), "kit.proc_field.fogstage_view");
  assert.deepEqual(JSON.parse(at("params")), { "kit.proc_field.fogstage_asset": 1 });
  assert.equal(at("dist"), join(d, "dist"));
});

test("a listed shots folder re-shoots every view in its recipe", () => {
  const d = box();
  const shots = helperShots(d);
  const { r, out } = runHook(d, [shots]);
  assert.equal(r.status, 0, r.stderr);
  const notes = JSON.parse(readFileSync(join(out, "visual-review.json"), "utf8"));
  assert.deepEqual(notes.views.map((v) => v.name).sort(), ["close", "fog"]);
});

test("pictures not shot by asset-shoot (no recipe): exit 3 with the plain reason, nothing in {out}", () => {
  const d = box();
  const shots = helperShots(d, { recipe: false });
  const { r, out } = runHook(d, [join(shots, "fog.png")]);
  assert.equal(r.status, 3);
  assert.match(r.stderr, /capture-recipe\.json/);
  assert.equal(existsSync(join(out, "visual-review.json")), false);
});

test("both paths failing: a non-zero exit and a reason naming the browser and the engine path", () => {
  const d = box();
  const shots = helperShots(d);
  const { r, out } = runHook(d, [join(shots, "fog.png")], { STUB_FAIL: "1" });
  assert.equal(r.status, 1);
  assert.match(r.stderr, /no usable picture by either path\. fog\.png: browser: black .*; offscreen: capture\.frame/);
  assert.equal(existsSync(join(out, "visual-review.json")), false); // a review with no pictures never counts
});

// Evani.App's own reader and checker on the same files, when its checkout is on this PC (read-only; skipped elsewhere).
const APP = process.env.EVANI_APP_DIR || join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "claude-code-desktop"); // Evani.App checkout, if any; the test skips without it
test("Evani.App reads the declaration and accepts the hook's packet", { skip: !existsSync(join(APP, "src/main/qqBackupCaptureCore.ts")) && "Evani.App's checkout isn't here" }, async () => {
  const { register } = await import("node:module");
  register(pathToFileURL(join(APP, "tests/ts-resolve.mjs")));
  const { readCaptureDeclaration } = await import(pathToFileURL(join(APP, "src/main/qqBackupCaptureCore.ts")).href);
  const { collectVisualPacket } = await import(pathToFileURL(join(APP, "src/main/qqVisualPacketCore.ts")).href);
  const got = readCaptureDeclaration([ROOT]);
  assert.ok(got.found, got.why);
  assert.equal(got.found.command, "node");
  assert.equal(got.found.timeoutMs, 300_000);
  const d = box();
  const shots = helperShots(d);
  const { r, out } = runHook(d, [join(shots, "visual-review.json")]);
  assert.equal(r.status, 0, r.stderr);
  const app = collectVisualPacket({ sources: [join(out, "visual-review.json")], root: out, changed: [], outDir: join(d, "app-round") });
  assert.equal(app.ok, true, app.why);
});
