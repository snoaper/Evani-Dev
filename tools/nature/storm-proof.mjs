// The storm parity proof: copies the reference and our pictures into the
// proof page's folder and writes (or replaces) its top section: matched
// views side by side with each picture's measurements (storm-metrics.mjs).
//
//   node tools/nature/storm-proof.mjs --page <index.html> --ref <dir> --ours <dir>
//        --metrics <metrics.json> --round <n> [--stats <stats.txt>]
//        [--summary <summary.json>] (averages over moments) [--commit <hash>]
//        [--prev <dir>] (the previous round's pictures, shown beside ours)
//        [--mom <dir>] (the moments' pictures, for the foam figures)
//   Each round gets its own section at the top (markers storm-v2-r<n>);
//   earlier rounds' sections stay below it.
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";

const arg = (k, d) => {
  const i = process.argv.indexOf("--" + k);
  return i > 0 ? process.argv[i + 1] : d;
};
const page = arg("page");
const ref = arg("ref");
const ours = arg("ours");
const round = arg("round", "1");
const metrics = JSON.parse(readFileSync(arg("metrics"), "utf8"));
const stats = arg("stats") && existsSync(arg("stats")) ? readFileSync(arg("stats"), "utf8") : "";
const summary = arg("summary") && existsSync(arg("summary")) ? JSON.parse(readFileSync(arg("summary"), "utf8")) : null;
const commit = arg("commit", "");
const prev = arg("prev", "");
const mom = arg("mom", ours);
const dir = join(dirname(page), "storm-v2", "r" + round);
mkdirSync(dir, { recursive: true });
const m = Object.fromEntries(metrics.map((x) => [x.name, x]));
const copy = (src, name) => {
  if (!existsSync(src)) return null;
  copyFileSync(src, join(dir, name));
  return `storm-v2/r${round}/${name}`;
};
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;");
const nums = (k, crest = true) => {
  const x = m[k];
  if (!x) return "";
  const c = crest ? `crests over the horizon on <b>${x.crest_px.over4_pct}%</b> of the width (95th pct ${x.crest_px.p95} px) · ` : "";
  const f = x.foam ? `foam alone ${x.foam.foam_pct}% (spray ${x.foam.spray_pct}%) · ` : "";
  const t = x.turquoise_pct !== undefined ? ` · turquoise ${x.turquoise_pct}% (bright ${x.turquoise_bright_pct}%)` : "";
  const r = x.ribbing ? ` · ribbing ${x.ribbing.peak_over_median} (the strongest near ripple period over the median; a corrugated sheet stands out)` : "";
  return `${c}${f}white water incl. glints ${x.white_water_pct}% · dark troughs ${x.dark_trough_pct}% · light-dark change (steepness) mean ${x.steepness.mean_grad}, 90th pct ${x.steepness.p90_grad} · sea brightness ${x.sea_lum.mean}±${x.sea_lum.sd}${t}${r}`;
};
const fig = (src, name, tag, cap, key, crest = true) => {
  const p = copy(src, name);
  if (!p) return "";
  return `<figure><span class="tag">${esc(tag)}</span><a href="${p}"><img src="${p}" loading="lazy" alt="${esc(cap)}"></a><figcaption><b>${esc(cap)}</b><br>${nums(key, crest)}</figcaption></figure>`;
};
const views = [
  {
    title: "Open ocean: eye 9 m, 50° view, 1.7° down (the reference's own camera)",
    figs: [
      [join(ref, "uw-open-ocean-1.png"), "ref-open.png", "Reference: Ultra-Water", "Ultra-Water, 24 m/s, 700 km fetch, its storm-like panel values", "ref-open-1"],
      [join(ours, "uw-storm-open-3.png"), "ours-open-ultra.png", "Ours: Storm sea, Ultra", "Storm sea, Ultra", "ours-open-3"],
      [join(ours, "uw-storm-open-2.png"), "ours-open-high.png", "Ours: Storm sea, High (flagship phones)", "Storm sea, High (this moment the eye sits by a big near wave)", "ours-open-2"],
      ...(prev ? [[join(prev, "uw-storm-open-3.png"), "prev-open-ultra.png", "Round " + (Number(round) - 1) + ": Storm sea, Ultra", "The last round, same camera and moment", ""]] : []),
      [join(ours, "old-storm-uw-3.png"), "before-open-ultra.png", "Before: the old Storm sea, Ultra", "Old Storm sea, same camera", "before-open-3"],
    ],
  },
  {
    title: "Golden hour: eye 5 m, 50° view, 1.0° down, into the sun",
    figs: [
      [join(ref, "uw-golden-hour-1.png"), "ref-golden.png", "Reference: Ultra-Water", "Ultra-Water, same settings", "ref-golden-1"],
      [join(ours, "uw-storm-golden-3.png"), "ours-golden-ultra.png", "Ours: Storm sea, Ultra", "Storm sea, Ultra", "ours-golden-3"],
      [join(ours, "uw-storm-golden-2.png"), "ours-golden-high.png", "Ours: Storm sea, High", "Storm sea, High", "ours-golden-2"],
      ...(prev ? [[join(prev, "uw-storm-golden-3.png"), "prev-golden-ultra.png", "Round " + (Number(round) - 1) + ": Storm sea, Ultra", "The last round, same camera and moment", ""]] : []),
    ],
  },
  {
    title: "Foam alone (white) and spray (orange), measured without glints: the same pictures drawn again without foam",
    crest: false,
    figs: [
      [join(mom, "uw-storm-open-m2-3.foam.png"), "foam-open-ultra.png", "Storm sea, Ultra, open view", "Foam alone, open view (Ultra, moment 2)", "ours-open-L3-m2"],
      [join(mom, "uw-storm-golden-m4-3.foam.png"), "foam-golden-ultra.png", "Storm sea, Ultra, golden hour", "Foam alone, golden hour (Ultra, moment 4)", "ours-golden-L3-m4"],
      [join(mom, "uw-storm-open-m6-2.foam.png"), "foam-open-high.png", "Storm sea, High, open view", "Foam alone, open view (High, moment 6)", "ours-open-L2-m6"],
    ],
  },
  {
    title: "Aerial: 420 m up, 18° down (1600 x 900)",
    crest: false,
    figs: [
      [join(ref, "uw-aerial-0.png"), "ref-aerial.png", "Reference: Ultra-Water", "Ultra-Water, same settings (its island in view)", "ref-aerial-0"],
      [join(ours, "uw-storm-aerial-3.png"), "ours-aerial-ultra.png", "Ours: Storm sea, Ultra", "Storm sea, Ultra", "ours-aerial-3"],
      [join(ours, "uw-storm-aerial-2.png"), "ours-aerial-high.png", "Ours: Storm sea, High", "Storm sea, High", "ours-aerial-2"],
    ],
  },
  {
    title: "The other presets and levels",
    crest: false,
    figs: [
      [join(ours, "uw-rough-open-3.png"), "rough-ultra.png", "Rough sea, Ultra", "Rough sea (15 m/s), the 9 m camera", "rough-open-3"],
      [join(ours, "uw-rough-open-2.png"), "rough-high.png", "Rough sea, High", "Rough sea, High", "rough-open-2"],
      [join(ours, "beach-3.png"), "beach-ultra.png", "Beach breakers, Ultra", "Beach breakers (1.8 m breakers, torn-lace whitewater)", ""],
      [join(ours, "beach-side-2.png"), "beach-side-high.png", "Beach breakers, High", "Beach breakers from the side", "beach-side-2"],
      [join(ours, "storm-1m5-3.png"), "storm-low-ultra.png", "Storm sea, eye 1.5 m, Ultra", "Low camera in the troughs", ""],
      [join(ours, "uw-storm-open-1.png"), "ours-open-medium.png", "Storm sea, Medium", "Storm sea, Medium", "ours-open-1"],
      [join(ours, "uw-storm-open-0.png"), "ours-open-low.png", "Storm sea, Low", "Storm sea, Low (flat level, the waves' light only)", "ours-open-0"],
    ],
  },
];
let html = `<!-- storm-v2-r${round} start -->
<section>
  <h2>Storm level: like-for-like against Ultra-Water (2026-11-02, round ${esc(round)})</h2>
  <p>The matched side-by-side rig (Ultra-Water run locally from its own code, same 7 cameras, sun and sea states, 8 moments): <a href="../ocean-compare/index.html">ocean-compare/index.html</a>.</p>
  <p class="lead">Chris: "the waves do not reach the velocity we had in the pictures I started with; get this to a proper storm level swell".
  Left: the Ultra-Water demo captured headless and muted at its storm-like settings (wind 24 m/s, fetch 700 km, choppiness 1.14) from its own
  cameras; right: our Storm sea from the same cameras, field of view and sun. Every number under a picture is measured from the picture alone,
  the same way for both (tools/nature/storm-metrics.mjs): how much of the horizon line waves stand above, white water, dark troughs and how
  sharply light and dark change across the sea (a busy, steep sea changes sharply). Foam is also measured on its own for ours (the picture drawn again without foam: what the foam adds, so sun glints never count). Our sky is the harness's stand-in (from round 5 a darker, broken storm deck over a paler haze, no sun disc), not the reference's. Turquoise share and a ribbing score are measured too. Our pictures are drawn on the processor (no graphics card)${commit ? ", from commit " + esc(commit) + " (branch storm-swell)" : ""}.</p>
`;
if (summary) {
  html += `  <h3>Averaged over moments (the reference: 3 moments; ours: 8, High and Ultra)</h3>
  <div class="note" style="max-width:none"><table style="border-collapse:collapse;font-size:13px"><tr><th align=left>view</th><th>moments</th><th>crests over the horizon (% of width)</th><th>95th pct crest (px)</th><th>foam alone % (ours; glints off)</th><th>spray %</th><th>white water incl. glints %</th><th>sea brightness</th><th>its spread (sd)</th><th>turquoise % (bright)</th><th>ribbing</th></tr>`;
  for (const [k, v] of Object.entries(summary)) html += `<tr><td>${esc(k)}</td><td align=center>${v.moments}</td><td>${esc(v.crest_share_pct)}</td><td>${esc(v.crest_p95_px)}</td><td>${esc(v.foam_pct ?? "-")}</td><td>${esc(v.spray_pct ?? "-")}</td><td>${esc(v.white_pct)}</td><td>${esc(v.sea_lum)}</td><td>${esc(v.sea_lum_sd ?? "-")}</td><td>${esc(v.turquoise_pct ?? "-")} (${esc(v.turquoise_bright_pct ?? "-")})</td><td>${esc(v.ribbing ?? "-")}</td></tr>`;
  html += `</table></div>
`;
}
for (const v of views) {
  html += `  <h3>${esc(v.title)}</h3>\n  <div class="grid">\n`;
  for (const [src, name, tag, cap, key] of v.figs) html += "    " + fig(src, name, tag, cap, key, v.crest !== false) + "\n";
  html += "  </div>\n";
}
if (stats) html += `  <div class="note"><b>This round: what changed, speed and the proofs:</b><pre style="white-space:pre-wrap;margin:6px 0 0">${esc(stats)}</pre></div>\n`;
html += `</section>\n<!-- storm-v2-r${round} end -->\n`;
let doc = readFileSync(page, "utf8");
const start = `<!-- storm-v2-r${round} start -->`, end = `<!-- storm-v2-r${round} end -->`;
if (doc.includes(start)) doc = doc.slice(0, doc.indexOf(start)) + html + doc.slice(doc.indexOf(end) + end.length + 1);
else if (doc.includes("</header>\n")) doc = doc.replace("</header>\n", () => "</header>\n\n" + html);
else doc = doc.replace("</header>", () => "</header>\n" + html);
writeFileSync(page, doc);
console.log(`wrote ${basename(page)} round ${round}: ${dir}`);
