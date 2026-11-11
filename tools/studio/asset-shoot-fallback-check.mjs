// End-to-end check of asset-shoot.mjs's two capture paths on a real scene build (the graphics card, headless,
// muted, one capture at a time: asset-shoot takes the GPU capture lock itself). Not part of the quick tests:
// it needs a trunk build of a dev scene with `live` (the engine's own read-back), e.g.
//   trunk build --release --no-default-features --features airstage,live --dist <live-dist>   (examples/proc-field/apps/proc-field)
//
//   node tools/studio/asset-shoot-fallback-check.mjs --dist <live-dist> --out <folder>
//        [--plain-dist <the same scene without live>] [--size 1672x941] [--view-param kit.proc_field.fogstage_view]
//        [--params '{"kit.proc_field.fogstage_subject":0,"kit.proc_field.fogstage_asset":1}']
//
// Cases: 1 the browser picture is usable (path "browser"); 2 the browser picture forced unusable, the engine
// retakes it on the same page (path "offscreen"); 3 (with --plain-dist) a build without live, the backup taken
// from --offscreen-dist on a second page; 4 both forced to fail: exit 4, path "failed" and a reason naming both.
// Each picture made is checked with visual-review.mjs (usable at the size) and the notes are written and
// checked. Writes report.json; exit 0 when every case behaved, 1 otherwise.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { usable, buildNotes } from "./visual-review.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const arg = (k, d) => { const i = process.argv.indexOf("--" + k); return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d; };
const dist = arg("dist"), outRoot = arg("out") && resolve(arg("out"));
if (!dist || !outRoot) { console.error("usage: --dist <live dist> --out <folder> [--plain-dist <dist>]"); process.exit(2); }
const size = arg("size", "1672x941");
const [W, H] = size.split("x").map(Number);
const common = ["--size", size, "--view-param", arg("view-param", "kit.proc_field.fogstage_view"), "--params", arg("params", '{"kit.proc_field.fogstage_subject":0,"kit.proc_field.fogstage_asset":1}'), "--shots", "0:fog.png", "--timeout-s", "300"];

const shoot = (name, extra) => {
  const out = join(outRoot, name);
  mkdirSync(out, { recursive: true });
  console.log(`\n== case ${name}`);
  const r = spawnSync(process.execPath, [join(here, "asset-shoot.mjs"), "--out", out, ...common, ...extra], { encoding: "utf8", windowsHide: true });
  process.stdout.write(r.stderr || "");
  const paths = existsSync(join(out, "capture-paths.json")) ? JSON.parse(readFileSync(join(out, "capture-paths.json"), "utf8")) : {};
  const pic = join(out, "fog.png");
  const u = existsSync(pic) ? usable(readFileSync(pic), { width: W, height: H }) : null;
  let notes = null;
  if (u?.ok) {
    const b = buildNotes({ title: `asset-shoot ${name}`, views: [{ name: "Fog staging", picture: pic }] }, { specDir: out, outFile: join(out, "visual-review.json") });
    notes = { ok: b.check.ok, problems: b.check.problems, capture_path: b.json.views[0].parts[0].numbers.capture_path };
    if (b.check.ok) writeFileSync(join(out, "visual-review.json"), JSON.stringify(b.json, null, 1) + "\n");
  }
  return { status: r.status, paths: paths["fog.png"], usable: u, notes, stderr_tail: (r.stderr || "").trim().split("\n").slice(-2) };
};

const cases = {};
const expect = {};
cases.primary = shoot("1-primary", ["--dist", dist]);
expect.primary = cases.primary.status === 0 && cases.primary.paths?.path === "browser" && cases.primary.usable?.ok && cases.primary.notes?.ok && cases.primary.notes.capture_path === "browser";
cases.fallback_same_page = shoot("2-fallback-same-page", ["--dist", dist, "--force-primary-fail"]);
expect.fallback_same_page = cases.fallback_same_page.status === 0 && cases.fallback_same_page.paths?.path === "offscreen" && cases.fallback_same_page.usable?.ok && cases.fallback_same_page.notes?.ok && cases.fallback_same_page.notes.capture_path === "offscreen";
if (arg("plain-dist")) {
  cases.fallback_backup_build = shoot("3-fallback-backup-build", ["--dist", arg("plain-dist"), "--offscreen-dist", resolve(dist), "--force-primary-fail"]);
  expect.fallback_backup_build = cases.fallback_backup_build.status === 0 && cases.fallback_backup_build.paths?.path === "offscreen" && cases.fallback_backup_build.usable?.ok && cases.fallback_backup_build.notes?.ok;
}
cases.both_fail = shoot("4-both-fail", ["--dist", dist, "--force-primary-fail", "--force-offscreen-fail"]);
expect.both_fail = cases.both_fail.status === 4 && cases.both_fail.paths?.path === "failed" && /browser: .+; offscreen: .+/.test(cases.both_fail.paths?.why || "");

const ok = Object.values(expect).every(Boolean);
writeFileSync(join(outRoot, "report.json"), JSON.stringify({ ok, expect, cases }, null, 1) + "\n");
console.log(`\nasset-shoot fallback check: ${ok ? "PASS" : "FAIL"} ${JSON.stringify(expect)}`);
process.exit(ok ? 0 : 1);
