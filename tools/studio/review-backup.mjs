// Evani.App's backup capture hook for this project (named in .vibedeck/visual-capture.json; Evani.App QQ-17,
// claude-code-desktop docs/qqloop.md "Worked out, never asked, and the backup"). When a helper's review pictures
// are missing or refused, Evani.App runs this (no shell, hidden, low priority, stopped after timeoutSec) before it
// asks anyone. It shoots the same views again, into Evani.App's own folder only:
//
//   node tools/studio/review-backup.mjs --out {out} --root {root} [--budget-s 280]
//   env: EVANI_CAPTURE_OUT, EVANI_CAPTURE_ROOT, EVANI_CAPTURE_FILES (JSON list: what the helper listed and changed),
//        EVANI_CAPTURE_HIDDEN=1, EVANI_CAPTURE_MUTED=1 (always true here: every browser is headless and muted)
//
// 1. From the listed files (pictures, visual-review*.json and the pictures they name, folders) find the folders with
//    a capture-recipe.json: asset-shoot.mjs writes one beside every set of pictures it shoots (the dist, size, tier,
//    view param, params and shots), and server-views.mjs shoot runs asset-shoot, so its pictures have one too.
// 2. Shoot each recipe's views (only the listed pictures when some are listed) with asset-shoot.mjs into {out}/shots-N:
//    the browser's screenshot first, the engine's own off-screen read-back (capture.frame) when that is unusable.
// 3. Write {out}/visual-review.json with tools/studio/visual-review.mjs (view names from the helper's own notes when
//    they name the picture), checked by Evani.App's rules; every picture is inside {out}.
// Exit 0 with at least one picture; 3 when no listed picture has a recipe (it wasn't shot by asset-shoot.mjs: say
// so); 1 when nothing could be shot (each reason on stderr). Never writes outside {out}.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, extname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { buildNotes } from "./visual-review.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const NOTES = /^visual-review[^\\/]*\.json$/i;

/** Folders holding a capture-recipe.json for the listed files, with the shots wanted from each. */
export function recipesFor(files, root) {
  const want = new Map(); // folder -> Set of picture file names (empty set: every shot)
  const names = new Map(); // picture file name (lower) -> { name, label } from the helper's notes
  const note = (dir, pic) => {
    if (!want.has(dir)) want.set(dir, new Set());
    if (pic) want.get(dir).add(pic.toLowerCase());
  };
  for (const raw of files) {
    if (typeof raw !== "string") continue;
    const f = isAbsolute(raw) ? raw : resolve(root, raw);
    if (!existsSync(f)) continue;
    const st = statSync(f);
    if (st.isDirectory()) { note(f, null); continue; }
    if (/\.png$/i.test(f)) { note(dirname(f), basename(f)); continue; }
    if (NOTES.test(basename(f))) {
      let json;
      try { json = JSON.parse(readFileSync(f, "utf8").replace(/^﻿/, "")); } catch { continue; }
      for (const v of json?.views ?? []) {
        const parts = Array.isArray(v.parts) && v.parts.length ? v.parts : [{ label: v.name, picture: v.picture }];
        for (const p of parts) {
          if (typeof p?.picture !== "string" || !p.picture.trim() || p.role === "reference") continue; // a stored reference is never re-shot
          const pic = isAbsolute(p.picture) ? p.picture : resolve(dirname(f), p.picture);
          note(dirname(pic), basename(pic));
          names.set(basename(pic).toLowerCase(), { name: String(v.name || basename(pic)), label: String(p.label || basename(pic)) });
        }
      }
    }
  }
  const recipes = [];
  for (const [dir, pics] of want) {
    const file = join(dir, "capture-recipe.json");
    if (!existsSync(file)) continue;
    let recipe;
    try { recipe = JSON.parse(readFileSync(file, "utf8")); } catch { continue; }
    const shots = (recipe.shots || []).filter((s) => !pics.size || pics.has(String(s.file).toLowerCase()));
    if (shots.length) recipes.push({ dir, recipe, shots });
  }
  return { recipes, names };
}

/** asset-shoot.mjs's arguments for a recipe, shooting into `dest`. */
export function shootArgs(recipe, dest, shotsFile, timeoutS) {
  const a = ["--dist", recipe.dist, "--out", dest, "--size", recipe.size, "--tier", recipe.tier || "high", "--shots-file", shotsFile, "--timeout-s", String(Math.max(30, Math.floor(timeoutS)))];
  if (recipe.view_param) a.push("--view-param", recipe.view_param);
  if (recipe.params && Object.keys(recipe.params).length) a.push("--params", JSON.stringify(recipe.params));
  if (recipe.offscreen_dist && existsSync(recipe.offscreen_dist)) a.push("--offscreen-dist", recipe.offscreen_dist);
  if (recipe.settle_ms !== undefined) a.push("--settle-ms", String(recipe.settle_ms));
  if (recipe.frames !== undefined) a.push("--frames", String(recipe.frames));
  if (recipe.warm_ms) a.push("--warm-ms", String(recipe.warm_ms));
  return a;
}

export function main(argv, env = process.env) {
  const arg = (k, d) => { const i = argv.indexOf("--" + k); return i >= 0 && argv[i + 1] !== undefined && !argv[i + 1].startsWith("--") ? argv[i + 1] : d; };
  const outArg = arg("out", env.EVANI_CAPTURE_OUT || "");
  if (!outArg) { console.error("review-backup: no output folder (--out {out} or EVANI_CAPTURE_OUT)"); return 2; }
  const out = resolve(outArg);
  const root = resolve(arg("root", env.EVANI_CAPTURE_ROOT || process.cwd()));
  mkdirSync(out, { recursive: true });
  let files;
  try { files = JSON.parse(arg("files", env.EVANI_CAPTURE_FILES || "[]")); } catch { console.error("review-backup: EVANI_CAPTURE_FILES isn't a JSON list"); return 2; }
  if (!Array.isArray(files)) { console.error("review-backup: EVANI_CAPTURE_FILES isn't a JSON list"); return 2; }
  // Every browser this starts is headless and muted (tools/bench-runner/chrome.mjs: --headless=new, --mute-audio,
  // windowsHide), so EVANI_CAPTURE_HIDDEN and EVANI_CAPTURE_MUTED always hold: nothing here shows a window or plays sound.
  console.error(`review-backup: hidden and muted (EVANI_CAPTURE_HIDDEN=${env.EVANI_CAPTURE_HIDDEN ?? "-"}, EVANI_CAPTURE_MUTED=${env.EVANI_CAPTURE_MUTED ?? "-"}); ${files.length} listed files`);
  const { recipes, names } = recipesFor(files, root);
  if (!recipes.length) {
    console.error("review-backup: none of the listed pictures has a capture-recipe.json beside it, so they weren't shot by tools/studio/asset-shoot.mjs and can't be shot again here");
    return 3;
  }
  const shooter = env.EVANI_BACKUP_SHOOTER || join(here, "asset-shoot.mjs"); // tests swap in a stand-in
  const budget = Number(arg("budget-s", "280")) * 1000, start = Date.now();
  const made = [], reasons = [];
  recipes.forEach(({ dir, recipe, shots }, i) => {
    const left = (budget - (Date.now() - start)) / 1000;
    if (left < 30) { reasons.push(`${dir}: no time left`); return; }
    const dest = join(out, `shots-${i + 1}`);
    mkdirSync(dest, { recursive: true });
    const shotsFile = join(dest, "shots.json");
    writeFileSync(shotsFile, JSON.stringify(shots, null, 1));
    console.error(`review-backup: shooting ${shots.length} view(s) of ${recipe.dist} again (recipe in ${dir})`);
    const r = spawnSync(process.execPath, [shooter, ...shootArgs(recipe, dest, shotsFile, left - 10)], { encoding: "utf8", windowsHide: true, timeout: Math.max(30, left) * 1000 });
    if (r.stderr) process.stderr.write(r.stderr);
    let paths = {};
    try { paths = JSON.parse(readFileSync(join(dest, "capture-paths.json"), "utf8")); } catch { /* none written */ }
    for (const s of shots) {
      const p = paths[s.file];
      const pic = join(dest, s.file);
      if (p && (p.path === "browser" || p.path === "offscreen") && existsSync(pic)) made.push({ pic, file: s.file });
      else reasons.push(`${s.file}: ${p?.why || (r.status === null ? "the shoot timed out" : `the shoot stopped (exit ${r.status})`)}`);
    }
  });
  if (!made.length) {
    console.error(`review-backup: no usable picture by either path. ${reasons.join(" | ")}`);
    return 1;
  }
  const views = made.map(({ pic, file }) => {
    const n = names.get(file.toLowerCase());
    const stem = basename(file, extname(file));
    return { name: n ? n.name : stem, parts: [{ label: n ? n.label : stem, picture: pic }] };
  });
  const seen = new Map(); // two recipes with one view name: keep them apart
  for (const v of views) { const k = v.name.toLowerCase(); const c = (seen.get(k) || 0) + 1; seen.set(k, c); if (c > 1) v.name = `${v.name} (${c})`; }
  const notesFile = join(out, "visual-review.json");
  const built = buildNotes({
    title: "Backup pictures shot again by the project's capture hook",
    views,
    numbers_from: "tools/studio/review-backup.mjs: asset-shoot.mjs from each picture's capture-recipe.json (browser screenshot, else the engine's own off-screen read-back), file facts by visual-review.mjs",
    ...(reasons.length ? { shared: { not_shot: reasons.join(" | ").slice(0, 380) } } : {}),
  }, { specDir: out, outFile: notesFile });
  if (!built.check.ok) {
    console.error(`review-backup: the pictures were shot but their notes fail Evani.App's rules: ${built.check.problems.join("; ")}`);
    return 1;
  }
  writeFileSync(notesFile, JSON.stringify(built.json, null, 1) + "\n");
  console.error(`review-backup: ${made.length} picture(s) and visual-review.json in ${out}${reasons.length ? `; not shot: ${reasons.join(" | ")}` : ""}`);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
