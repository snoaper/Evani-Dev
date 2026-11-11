// Gathers one grass review round: the fixed views, the sheets (wind, rain,
// trampling, tiers) and the measured numbers into one folder with its
// visual-review.json (the format Evani.App's visual review reads).
//
//   node tools/grass/review-pack.mjs --shots <dir> --trample <dir> --bench <dir>
//        --out <dir> --build <commit> [--title "..."]
//
// The sheets must already exist in <shots> / <trample> (tools/grass/sheet.mjs).
import { copyFileSync, mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--") ? process.argv[i + 1] : d;
};
const shots = arg("shots", "");
const trample = arg("trample", "");
const bench = arg("bench", "");
const out = arg("out", "");
const build = arg("build", "");
if (!shots || !out) {
  console.error("--shots and --out are needed");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const rep = JSON.parse(readFileSync(join(shots, "report.json"), "utf8"));
const trep = trample && existsSync(join(trample, "report.json")) ? JSON.parse(readFileSync(join(trample, "report.json"), "utf8")) : { shots: [] };
const brep = bench && existsSync(join(bench, "report.json")) ? JSON.parse(readFileSync(join(bench, "report.json"), "utf8")) : { bench: [] };
const byName = (r, n) => r.shots.find((s) => s.name === n);
const put = (dir, file) => {
  copyFileSync(join(dir, file), join(out, file));
  return file;
};
// Per-picture numbers: the plan and the GPU times the page reported at the
// held frame (graphics card, 1280 x 720, High).
const nums = (s) =>
  s ? { blades_planned: s.blades, tiles: s.tiles, quality_level: s.level, gpu_frame_ms: s.gpu, grass_compute_ms: s.grass, forward_pass_ms: s.fwd } : {};

const views = [];
const single = [
  ["Meadow towards the low sun", "meadow"],
  ["Close in the meadow, eye 0.75 m", "close"],
  ["Overview of all four patches (context only, other kinds still being tuned)", "overview"],
  ["Low in the meadow, eye 0.45 m", "low"],
  ["Uncut wheat at ear height", "ears"],
  ["Along the edge of the wheat", "wheat"],
  ["Short lawn", "lawn"],
  ["Muddy trampled ground", "mud"],
];
for (const [name, v] of single) {
  if (!existsSync(join(shots, `${v}.png`))) continue;
  views.push({ name, picture: put(shots, `${v}.png`), numbers: nums(byName(rep, v)) });
}
const sheet = (dir, file, name, numbers) => {
  if (existsSync(join(dir, file))) views.push({ name, picture: put(dir, file), numbers });
};
for (const k of ["meadow", "wheat", "lawn", "mud"]) {
  const last = byName(rep, `motion-${k}-5`);
  sheet(shots, `sheet-motion-${k}.png`, `${k} in the wind: 6 stills 0.4 s apart`, { ...nums(last), stills: 6, seconds_apart: 0.4 });
}
sheet(shots, "sheet-wet.png", "Rain: wetness 0, 0.5 and 1 (meadow, close, mud)", nums(byName(rep, "wet-meadow-1")));
const t40 = byName(trep, "trample-40");
sheet(trample, "sheet-trample.png", "Trampling and spring-back (8 to 40 s), and kept flat", { ...nums(t40), half_life_s: 8, walker_radius_m: 0.45 });
const tiers = {};
for (const t of ["lite", "low", "medium", "high", "ultra"]) {
  const s = byName(rep, `tier-${t}-meadow`);
  if (s) tiers[`${t}_blades_planned`] = s.blades;
}
sheet(shots, "sheet-tiers.png", "The meadow on each quality tier", tiers);
// The per-tier cost, measured separately with frames uncapped.
const cost = {};
for (const b of brep.bench ?? []) {
  if (b.grass_total_ms != null) cost[`${b.tier}_grass_ms`] = b.grass_total_ms;
  if (b.on) cost[`${b.tier}_blades_planned`] = b.on.blades;
}
const review = {
  title: arg("title", "Procedural cinematic grass and crops (evani-grass): meadow, lawn, mud and wheat with wind, trampling and rain"),
  build,
  shared: {
    renderer: "Evani.Dev WebGPU in headless, muted Chrome on the graphics card (RTX 4060 Ti), 1280 x 720",
    quality: "High tier unless a picture says otherwise: 4x MSAA, grass to 62 m, full density to 16 m",
    scene: "late-afternoon sun 15 degrees up, procedural sky; every blade generated on the GPU each frame, no meshes or textures",
    clock: "virtual clock held on the stated sim time, so every picture is a fixed moment",
    grass_cost_per_tier: ["low", "medium", "high", "ultra"]
      .filter((t) => cost[`${t}_grass_ms`] != null)
      .map((t) => `${t[0].toUpperCase()}${t.slice(1)} ${cost[`${t}_grass_ms`].toFixed(2)} ms (${Math.round((cost[`${t}_blades_planned`] ?? 0) / 1000)}k blades planned)`)
      .join(", ") + " per frame: the grass compute pass plus the extra forward-pass time, 1600 x 900, frames uncapped",
    grass_download: "about 24 KB gzip (20 KB brotli) added to a game that lists grass; 0 bytes for every other game (9 games' ship builds byte for byte the same)",
  },
  views,
  numbers_from:
    "Per picture: the grass plan and the GPU timestamp times the proof game reports at the held frame (grass compute pass, forward pass, whole frame; 1280 x 720). The per-tier grass cost in 'shared' is a separate run at 1600 x 900 with frames uncapped and adaptive quality off: the grass compute pass plus the forward pass with grass less without, medians over 10 s (tools/grass/grass-shots.mjs --do bench).",
};
writeFileSync(join(out, "visual-review.json"), JSON.stringify(review, null, 1));
console.log(`${out}: ${views.length} views`);
