// node --test tools/studio/visual-review.test.mjs (also run by `cargo test -p evani-xtask commons`).
// The picture notes Asset Studio helpers hand to Evani.App's visual review: written by the tool, every field
// computed, and refused here for exactly what Evani.App refuses (its checker: claude-code-desktop
// src/main/qqVisualPacketCore.ts; when that checkout is on this PC, the last test runs both on the same files).
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { buildNotes, checkNotes, encodePng, main, pngFacts, plainValue, usable, valueProblem } from "./visual-review.mjs";

// A picture: "noise" (a real-looking frame), "black", "flat" (one mid grey) or "white".
function png(w, h, kind = "noise", seed = 1) {
  const px = Buffer.alloc(w * h * 4);
  let s = seed >>> 0;
  for (let i = 0; i < w * h; i++) {
    s = (s * 1664525 + 1013904223) >>> 0;
    const v = kind === "black" ? 0 : kind === "flat" ? 128 : kind === "white" ? 255 : 40 + (s >>> 24) % 180;
    px[i * 4] = v; px[i * 4 + 1] = kind === "noise" ? (v * 7) & 255 : v; px[i * 4 + 2] = kind === "noise" ? (v * 13) & 255 : v; px[i * 4 + 3] = 255;
  }
  return encodePng(w, h, px);
}
const box = () => mkdtempSync(join(tmpdir(), "visual-review-"));

test("Evani.App's value rule: numbers and text only; yes/no, null and empty text are refused with a plain reason", () => {
  for (const ok of [0, 1, 6.5, "yes", "none", "C:/a.png"]) assert.equal(valueProblem(ok), null, String(ok));
  assert.match(valueProblem(false), /true\/false/);
  assert.match(valueProblem(true), /true\/false/);
  assert.match(valueProblem(null), /empty/);
  assert.match(valueProblem(""), /empty text/);
  assert.match(valueProblem(Number.NaN), /finite/);
  assert.equal(plainValue(true), "yes");
  assert.equal(plainValue(false), "no");
  assert.equal(plainValue(null), "none");
  assert.equal(plainValue(""), "none");
});

test("by default no earlier-round fields (Evani.App works them out itself since ef6ebfd0), and a spec's own are dropped", () => {
  const d = box();
  writeFileSync(join(d, "fog.png"), png(64, 40));
  const { json, check } = buildNotes({ views: [{ name: "Fog", picture: "fog.png", numbers: { bytes_same_as_earlier_round: false, earlier_round_file: null, w: 1 } }] }, { specDir: d, outFile: join(d, "visual-review.json") });
  assert.equal(check.ok, true, check.problems.join("; "));
  const n = json.views[0].parts[0].numbers;
  for (const k of ["earlier_round_file", "earlier_round_sha256", "bytes_same_as_earlier_round"]) assert.equal(n[k], undefined, k);
  assert.equal(n.w, 1);
  assert.match(n.sha256, /^[0-9a-f]{64}$/);
});

test("--legacy-fields: a view with no earlier round gets complete notes: earlier file none, not the same bytes, sizes and hash", () => {
  const d = box();
  writeFileSync(join(d, "fog.png"), png(64, 40));
  const { json, check } = buildNotes({ title: "t", views: [{ name: "Fog", picture: "fog.png", numbers: { lit: true, empty: null } }] }, { specDir: d, outFile: join(d, "visual-review.json"), legacyFields: true });
  assert.equal(check.ok, true, check.problems.join("; "));
  const n = json.views[0].parts[0].numbers;
  assert.equal(n.earlier_round_file, "none");
  assert.equal(n.earlier_round_sha256, "none");
  assert.equal(n.bytes_same_as_earlier_round, "no");
  assert.equal(n.width_px, 64);
  assert.equal(n.height_px, 40);
  assert.match(n.sha256, /^[0-9a-f]{64}$/);
  assert.equal(n.lit, "yes");
  assert.equal(n.empty, "none");
  assert.equal(n.capture_path, "not recorded");
  for (const [k, v] of Object.entries(n)) assert.equal(valueProblem(v), null, k);
});

test("--legacy-fields: a stored reference picture gets valid notes too (capture_path stored, no earlier round)", () => {
  const d = box();
  writeFileSync(join(d, "ref.png"), png(80, 45, "noise", 3));
  writeFileSync(join(d, "ours.png"), png(80, 45, "noise", 4));
  const spec = { views: [{ name: "Fog against its reference", parts: [{ label: "Stored reference", role: "reference", picture: "ref.png" }, { label: "Ours", picture: "ours.png" }] }] };
  const { json, check } = buildNotes(spec, { specDir: d, outFile: join(d, "visual-review.json"), legacyFields: true });
  assert.equal(check.ok, true, check.problems.join("; "));
  assert.equal(json.views[0].parts[0].role, "reference");
  assert.equal(json.views[0].parts[0].numbers.capture_path, "stored");
  assert.equal(json.views[0].parts[0].numbers.earlier_round_file, "none");
});

test("--legacy-fields: the earlier round is matched by view and label: same bytes says yes, changed bytes says no", () => {
  const old = box(), d = box();
  writeFileSync(join(old, "a.png"), png(32, 32, "noise", 9));
  writeFileSync(join(old, "b.png"), png(32, 32, "noise", 10));
  writeFileSync(join(old, "visual-review.json"), JSON.stringify({ views: [{ name: "A", picture: "a.png" }, { name: "B", picture: "b.png" }] }));
  writeFileSync(join(d, "a.png"), png(32, 32, "noise", 9)); // unchanged
  writeFileSync(join(d, "b2.png"), png(32, 32, "noise", 11)); // changed, new file name, same view
  const { json, check } = buildNotes({ views: [{ name: "A", picture: "a.png" }, { name: "B", picture: "b2.png" }] }, { specDir: d, outFile: join(d, "visual-review.json"), earlier: join(old, "visual-review.json"), legacyFields: true });
  assert.equal(check.ok, true, check.problems.join("; "));
  const [a, b] = json.views.map((v) => v.parts[0].numbers);
  assert.equal(a.bytes_same_as_earlier_round, "yes");
  assert.equal(a.earlier_round_file, join(old, "a.png"));
  assert.equal(a.earlier_round_sha256, a.sha256);
  assert.equal(b.bytes_same_as_earlier_round, "no");
  assert.equal(b.earlier_round_file, join(old, "b.png"));
  assert.notEqual(b.earlier_round_sha256, b.sha256);
});

test("--legacy-fields: an earlier round given as a picture folder is matched by file name", () => {
  const old = box(), d = box();
  writeFileSync(join(old, "a.png"), png(32, 32, "noise", 5));
  writeFileSync(join(d, "a.png"), png(32, 32, "noise", 6));
  const { json } = buildNotes({ views: [{ name: "A", picture: "a.png" }] }, { specDir: d, outFile: join(d, "visual-review.json"), earlier: old, legacyFields: true });
  assert.equal(json.views[0].parts[0].numbers.earlier_round_file, join(old, "a.png"));
  assert.equal(json.views[0].parts[0].numbers.bytes_same_as_earlier_round, "no");
});

test("capture_path comes from the capture tool's capture-paths.json (browser or offscreen)", () => {
  const d = box();
  writeFileSync(join(d, "a.png"), png(32, 32, "noise", 1));
  writeFileSync(join(d, "b.png"), png(32, 32, "noise", 2));
  writeFileSync(join(d, "capture-paths.json"), JSON.stringify({ "a.png": { path: "browser" }, "b.png": { path: "offscreen", primary_problem: "black" } }));
  const { json } = buildNotes({ views: [{ name: "A", picture: "a.png" }, { name: "B", picture: "b.png" }] }, { specDir: d, outFile: join(d, "visual-review.json") });
  assert.deepEqual(json.views.map((v) => v.parts[0].numbers.capture_path), ["browser", "offscreen"]);
});

test("the checker finds what Evani.App refuses: hand-written yes/no and empty values, no views, blank and missing pictures", () => {
  const d = box();
  writeFileSync(join(d, "a.png"), png(32, 32));
  writeFileSync(join(d, "blank.png"), png(32, 32, "flat"));
  const hand = { views: [{ name: "Fog", parts: [{ label: "Ref", role: "reference", picture: "a.png", numbers: { earlier_round_file: null, bytes_same_as_earlier_round: false } }] }], numbers_from: "x" };
  const r = checkNotes(hand, join(d, "visual-review.json"));
  assert.equal(r.ok, false);
  assert.ok(r.problems.some((p) => p.includes('"earlier_round_file"') && p.includes("empty")), r.problems.join("; "));
  assert.ok(r.problems.some((p) => p.includes('"bytes_same_as_earlier_round"') && p.includes("true/false")), r.problems.join("; "));
  assert.match(checkNotes({ views: [] }, join(d, "v.json")).problems[0], /no pictures never counts/);
  assert.match(checkNotes({}, join(d, "v.json")).problems[0], /no pictures never counts/);
  const blank = checkNotes({ views: [{ name: "B", picture: "blank.png" }], no_numbers_because: "x" }, join(d, "v.json"));
  assert.ok(blank.problems.some((p) => /blank/.test(p)), blank.problems.join("; "));
  const missing = checkNotes({ views: [{ name: "M", picture: "nope.png" }], no_numbers_because: "x" }, join(d, "v.json"));
  assert.ok(missing.problems.some((p) => /wasn't found/.test(p)), missing.problems.join("; "));
  assert.ok(missing.problems.some((p) => /no pictures at all/.test(p)), missing.problems.join("; "));
});

test("a picture neither path could make stops the notes with its name (nothing is written)", () => {
  const d = box();
  writeFileSync(join(d, "spec.json"), JSON.stringify({ views: [{ name: "Fog", picture: "fog.png" }] }));
  assert.equal(main(["write", "--spec", join(d, "spec.json")]), 1);
  assert.equal(existsSync(join(d, "visual-review.json")), false);
});

test("write then check: the CLI writes notes only when they pass, beside the pictures", () => {
  const d = box();
  writeFileSync(join(d, "a.png"), png(48, 27));
  writeFileSync(join(d, "spec.json"), JSON.stringify({ views: [{ name: "A", picture: "a.png" }] }));
  assert.equal(main(["write", "--spec", join(d, "spec.json")]), 0);
  const json = JSON.parse(readFileSync(join(d, "visual-review.json"), "utf8"));
  assert.equal(json.views[0].parts[0].picture, "a.png");
  assert.ok(json.numbers_from);
  assert.equal(main(["check", join(d, "visual-review.json")]), 0);
});

test("usable frames: the right size and real content; black, one flat colour, white and the wrong size are not", () => {
  assert.equal(usable(png(64, 36), { width: 64, height: 36 }).ok, true);
  assert.match(usable(png(64, 36), { width: 128, height: 72 }).why, /wrong size/);
  assert.match(usable(png(64, 36, "black")).why, /blank|black/);
  assert.match(usable(png(64, 36, "flat")).why, /blank|uniform/);
  assert.match(usable(png(64, 36, "white")).why, /blank|uniform/);
  assert.match(usable(Buffer.from("not a png at all, not a png at all, not a png")).why, /readable/);
  // A dark but real frame (night scene) still counts.
  const px = Buffer.alloc(64 * 36 * 4);
  for (let i = 0; i < 64 * 36; i++) { const v = 6 + (i * 37) % 20; px[i * 4] = v; px[i * 4 + 1] = v; px[i * 4 + 2] = v; px[i * 4 + 3] = 255; }
  assert.equal(usable(encodePng(64, 36, px)).ok, true);
  assert.deepEqual(pngFacts(png(10, 7)), { width: 10, height: 7, blank: false });
});

// Both checkers on the same packets, when Evani.App's checkout is on this PC (read-only use; skipped elsewhere).
const APP = process.env.EVANI_APP_DIR || join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "claude-code-desktop"); // Evani.App checkout, if any; the test skips without it
test("the same verdict as Evani.App's own checker", { skip: !existsSync(join(APP, "src/main/qqVisualPacketCore.ts")) && "Evani.App's checkout isn't here" }, async () => {
  const { register } = await import("node:module");
  register(pathToFileURL(join(APP, "tests/ts-resolve.mjs")));
  const { collectVisualPacket } = await import(pathToFileURL(join(APP, "src/main/qqVisualPacketCore.ts")).href);
  const d = box();
  writeFileSync(join(d, "a.png"), png(64, 40, "noise", 21));
  writeFileSync(join(d, "ref.png"), png(64, 40, "noise", 22));
  const cases = {
    toolWrittenLegacy: buildNotes({ views: [{ name: "Fog", picture: "a.png" }] }, { specDir: d, outFile: join(d, "x.json"), legacyFields: true }).json,
    toolWritten: buildNotes({ views: [{ name: "Fog", parts: [{ label: "Ref", role: "reference", picture: "ref.png" }, { label: "Ours", picture: "a.png" }] }] }, { specDir: d, outFile: join(d, "x.json") }).json,
    boolean: { views: [{ name: "Fog", picture: "a.png", numbers: { bytes_same_as_earlier_round: false } }], numbers_from: "x" },
    nullFile: { views: [{ name: "Fog", picture: "a.png", numbers: { earlier_round_file: null } }], numbers_from: "x" },
    emptyText: { views: [{ name: "Fog", picture: "a.png", numbers: { earlier_round_file: "" } }], numbers_from: "x" },
    noNumbersFrom: { views: [{ name: "Fog", picture: "a.png", numbers: { w: 1 } }] },
    noViews: { views: [] },
  };
  for (const [name, json] of Object.entries(cases)) {
    const sub = join(d, name);
    mkdirSync(sub);
    writeFileSync(join(sub, "a.png"), readFileSync(join(d, "a.png")));
    writeFileSync(join(sub, "ref.png"), readFileSync(join(d, "ref.png")));
    writeFileSync(join(sub, "visual-review.json"), JSON.stringify(json));
    const app = collectVisualPacket({ sources: ["visual-review.json"], root: sub, changed: [], outDir: join(sub, "round") });
    const ours = checkNotes(json, join(sub, "visual-review.json"));
    assert.equal(ours.ok, app.ok, `${name}: ours ${ours.ok} (${ours.problems.join("; ")}), Evani.App ${app.ok} (${app.why ?? ""})`);
  }
});
