// Reference classes for Asset Studio and Evani.commons (docs/ASSET_STUDIO_GUIDE.md "Reference classes").
// Reads catalog/procedural/library.json (`ref_class` on each category, and on an asset where it differs) and
// catalog/procedural/ref_classes.json (what each class means: framing, test scenes, behaviour specs), checks they agree,
// and writes:
//   tools/evani-commons-tab/classes.js   the tab's copy (window.ASCLASSES)
//   docs/ASSET_STUDIO_GUIDE.md           the table between <!-- ref-classes:table --> and <!-- /ref-classes:table -->, and the
//                                        viewpoint audit (every asset, its view(s), a reason) between <!-- ref-views:audit --> markers
// node tools/procedural/gen_ref_classes.mjs          write both
// node tools/procedural/gen_ref_classes.mjs --check  fail (exit 1) when either is out of date or the data disagree
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const rd = p => fs.readFileSync(path.join(ROOT, p), "utf8");
const LIB = JSON.parse(rd("catalog/procedural/library.json"));
const RC = JSON.parse(rd("catalog/procedural/ref_classes.json"));
const CHECK = process.argv.includes("--check");
const problems = [];
const CLASSES = Object.keys(RC.classes);

const catClass = {}, over = {}, byClass = { object: [], scene: [], behaviour: [] }, names = {};
// Attribute staging (ref_classes.json "staging"; the owner 2026-11-11): set by hand per asset with `staging` in library.json, never guessed.
const STG = RC.staging || {}, STAGINGS = Object.keys(STG).filter(k => k !== "about"), stagingOf = {};
for (const s of STAGINGS) {
  const T = STG[s] || {};
  for (const f of ["label", "reference", "template", "lead_photoreal", "lead_other", "marker", "author"]) if (typeof T[f] !== "string" || !T[f].trim()) problems.push(`staging.${s}.${f}: needs plain text`);
  if (T.template && T.marker && !T.template.includes(T.marker)) problems.push(`staging.${s}: its template must contain its marker ${JSON.stringify(T.marker)} (how a stored picture is known to be a staged one)`);
  if (!Array.isArray(T.review) || !T.review.length) problems.push(`staging.${s}.review: needs the reviewer's lines`);
}
for (const c of LIB.categories) {
  if (!CLASSES.includes(c.ref_class)) problems.push(`category ${c.id}: ref_class ${JSON.stringify(c.ref_class)} is not one of ${CLASSES.join(", ")}`);
  catClass[c.id] = c.ref_class;
  for (const a of c.assets) {
    const key = c.id + "-" + a.id;
    names[key] = a.name;
    let cls = c.ref_class;
    if (a.ref_class != null) {
      if (!CLASSES.includes(a.ref_class)) problems.push(`asset ${key}: ref_class ${JSON.stringify(a.ref_class)} is not a class`);
      else if (a.ref_class === c.ref_class) problems.push(`asset ${key}: ref_class repeats its category's default (leave it out)`);
      else { over[key] = a.ref_class; cls = a.ref_class; }
    }
    (byClass[cls] || (byClass[cls] = [])).push(key);
    const f = RC.assets[key] || {};
    if (cls === "behaviour") {
      if (!f.test_scene || !RC.test_scenes[f.test_scene]) problems.push(`asset ${key}: class behaviour needs a test_scene in ref_classes.json`);
      if (!Array.isArray(f.spec) || !f.spec.length) problems.push(`asset ${key}: class behaviour needs a measurable spec in ref_classes.json`);
      for (const s of f.spec || []) if (!s.id || !s.what || !Array.isArray(s.range) || s.range.length !== 2 || !(s.range[0] <= s.range[1])) problems.push(`asset ${key}: spec line ${JSON.stringify(s.id)} needs id, what and a [min, max] range`);
    }
    if (cls === "scene" && !RC.categories[c.id]) problems.push(`asset ${key}: class scene needs framing for category ${c.id} in ref_classes.json`);
    if (a.staging != null) {
      if (!STAGINGS.includes(a.staging)) problems.push(`asset ${key}: staging ${JSON.stringify(a.staging)} is not one of ref_classes.json staging (${STAGINGS.join(", ") || "none"})`);
      else if (cls === "behaviour") problems.push(`asset ${key}: staging is for assets you can photograph (its class is behaviour)`);
      else {
        const st = f.staged || {};
        for (const g of ["show", "time", "focus", "subject", "context"]) if (typeof st[g] !== "string" || !st[g].trim()) problems.push(`ref_classes.json assets.${key}.staged.${g}: a staged asset needs it`);
        // v1.5.45: its own comparison line when the staging's reviewer lines ask for one ({compare}); its own capture line and camera aim are optional
        if ((STG[a.staging].review || []).some(x => String(x).includes("{compare}")) && (typeof st.compare !== "string" || !st.compare.trim())) problems.push(`ref_classes.json assets.${key}.staged.compare: staging ${a.staging}'s reviewer lines take each asset's own comparison line`);
        for (const g of ["author"]) if (st[g] != null && (typeof st[g] !== "string" || !st[g].trim())) problems.push(`ref_classes.json assets.${key}.staged.${g}: needs plain text`);
        if (st.camera && st.camera.aim != null && (typeof st.camera.aim !== "string" || !st.camera.aim.trim())) problems.push(`ref_classes.json assets.${key}.staged.camera.aim: needs plain text (how the camera points, e.g. "tilted up about 12 degrees"; "level" when left out)`);
        if (!Number.isInteger(st.scene_subject) || st.scene_subject < 0) problems.push(`ref_classes.json assets.${key}.staged.scene_subject: the subject number of the asset in the staging's engine scene (${STG[a.staging].engine_scene}) is needed`);
        else if (Object.keys(stagingOf).some(o => (RC.assets[o].staged || {}).scene_subject === st.scene_subject)) problems.push(`ref_classes.json assets.${key}.staged.scene_subject ${st.scene_subject}: another staged asset has it`);
        stagingOf[key] = a.staging;
      }
    }
  }
}
for (const key of Object.keys(RC.assets)) if (RC.assets[key].staged && !stagingOf[key]) problems.push(`ref_classes.json assets.${key}.staged: the asset has no staging in library.json (set it there by hand)`);
for (const key of Object.keys(RC.assets)) if (!names[key]) problems.push(`ref_classes.json assets.${key}: no such asset in library.json`);
for (const key of Object.keys(RC.assets)) {
  const f = RC.assets[key], cls = over[key] || catClass[key.split("-")[0]];
  if (f.spec && cls !== "behaviour") problems.push(`ref_classes.json assets.${key}: has a spec but its class is ${cls}`);
}
const total = LIB.categories.reduce((n, c) => n + c.assets.length, 0);
// Viewpoints: every class names its one default view; an asset's own `views` (two or more) need a layout with that many panels.
const VP = RC.viewpoints || {}, LY = RC.layouts || {}, VA = RC.view_audit || {};
const clsOf = key => over[key] || catClass[key.split("-")[0]];
for (const c of CLASSES) if (!VP[RC.classes[c].default_view]) problems.push(`class ${c}: default_view ${JSON.stringify(RC.classes[c].default_view)} is not a viewpoint`);
const viewsOf = key => { const f = RC.assets[key] || {}; return Array.isArray(f.views) && f.views.length ? f.views : [RC.classes[clsOf(key)].default_view]; };
for (const key of Object.keys(RC.assets)) {
  const f = RC.assets[key]; if (f.views == null) continue;
  if (!Array.isArray(f.views) || f.views.length < 2) problems.push(`ref_classes.json assets.${key}: views needs two or more viewpoints (one view is the class's default: leave it out)`);
  for (const v of f.views || []) if (!VP[v] || !VP[v].panel) problems.push(`ref_classes.json assets.${key}: view ${JSON.stringify(v)} is not a viewpoint with a panel wording`);
  if (clsOf(key) !== "scene") problems.push(`ref_classes.json assets.${key}: views are for scene assets (its class is ${clsOf(key)})`);
  const L = LY[f.layout]; if (!L || !L.template || L.panels !== (f.views || []).length) problems.push(`ref_classes.json assets.${key}: layout ${JSON.stringify(f.layout)} needs a template with ${(f.views || []).length} panels`);
  if (L && L.template && !L.template.includes(L.marker || "\u0000")) problems.push(`layout ${f.layout}: its template must contain its marker ${JSON.stringify(L.marker)} (how a stored picture is known to be a ${L.label} one)`);
  for (const v of Object.keys(f.view_frame || {})) if (!(f.views || []).includes(v)) problems.push(`ref_classes.json assets.${key}: view_frame.${v} is not one of its views`);
  if (!(VA.confirmed || {})[key]) problems.push(`ref_classes.json assets.${key}: has views but no reason in view_audit.confirmed`);
}
for (const key of Object.keys(VA.confirmed || {})) if (!(RC.assets[key] || {}).views) problems.push(`view_audit.confirmed.${key}: no views in assets.${key}`);
for (const [key, s] of Object.entries(VA.suggested || {})) {
  if (!names[key]) problems.push(`view_audit.suggested.${key}: no such asset`);
  else if ((RC.assets[key] || {}).views) problems.push(`view_audit.suggested.${key}: already confirmed (has views): remove the suggestion`);
  else if (clsOf(key) !== "scene") problems.push(`view_audit.suggested.${key}: views are for scene assets`);
  for (const v of (s && s.views) || []) if (!VP[v]) problems.push(`view_audit.suggested.${key}: ${v} is not a viewpoint`);
  if (!s || !s.reason) problems.push(`view_audit.suggested.${key}: needs a reason`);
}
for (const key of Object.keys(VA.asset_reason || {})) if (!names[key]) problems.push(`view_audit.asset_reason.${key}: no such asset`);
// Subjects (what each column is judged on; ref_classes.json "subjects"): every class has a default subject, context and region; every
// scene category has its own; every region named exists; asset entries name real assets.
const SJ = RC.subjects || {}, SR = SJ.regions || {};
const sjCheck = (where, e) => {
  if (!e || typeof e !== "object") { problems.push(`subjects.${where}: needs an object`); return; }
  for (const f of ["subject", "context"]) if (e[f] != null && (typeof e[f] !== "string" || !e[f].trim())) problems.push(`subjects.${where}.${f}: needs plain text`);
  if (e.region != null && !SR[e.region]) problems.push(`subjects.${where}.region ${JSON.stringify(e.region)} is not one of subjects.regions`);
};
if (!SR.whole) problems.push("subjects.regions: needs whole");
for (const [r, v] of Object.entries(SR)) if (!v || !v.label || (v.rows != null && !["above", "below"].includes(v.rows))) problems.push(`subjects.regions.${r}: needs a label (and rows above or below, or none)`);
for (const c of CLASSES.concat(["material"])) { const e = (SJ.classes || {})[c]; sjCheck("classes." + c, e); if (!e || !e.subject || !e.context || !e.region) problems.push(`subjects.classes.${c}: needs subject, context and region`); }
for (const [id, e] of Object.entries(SJ.categories || {})) { sjCheck("categories." + id, e); if (!LIB.categories.some(c => c.id === id)) problems.push(`subjects.categories.${id}: no such category in library.json`); }
for (const id of Object.keys(RC.categories)) if (!(SJ.categories || {})[id]) problems.push(`subjects.categories.${id}: a scene category needs its own subject and context`);
for (const [key, e] of Object.entries(SJ.assets || {})) { sjCheck("assets." + key, e); if (!names[key]) problems.push(`subjects.assets.${key}: no such asset in library.json`); }
const multi = Object.keys(RC.assets).filter(k => viewsOf(k).length > 1);

const out = {
  version: RC.version,
  built_from: "catalog/procedural/library.json + catalog/procedural/ref_classes.json",
  classes: RC.classes, categories: RC.categories, assets: RC.assets, test_scenes: RC.test_scenes, flag: RC.flag,
  viewpoints: VP, layouts: LY, subjects: { regions: SR, classes: SJ.classes || {}, categories: SJ.categories || {}, assets: SJ.assets || {} },
  cat_class: catClass, over,
  staging: STG, staging_of: stagingOf,
};
const js = "/* Asset Studio reference classes: written by tools/procedural/gen_ref_classes.mjs from catalog/procedural/library.json (ref_class)\n" +
  "   and catalog/procedural/ref_classes.json. Do not edit by hand: change the catalogue and run the script. */\n" +
  "window.ASCLASSES = " + JSON.stringify(out) + ";\n";

const catName = id => (LIB.categories.find(c => c.id === id) || {}).name || id;
const nm = k => names[k] + " (" + catName(k.split("-")[0]).split(/[ ,]/)[0].toLowerCase() + ")";
const rows = LIB.categories.map(c => {
  const ex = c.assets.filter(a => a.ref_class != null).map(a => a.name + ": " + a.ref_class);
  return `| ${c.name} | ${c.assets.length} | ${c.ref_class} | ${ex.length ? ex.join("; ") : "none"} |`;
});
const table = [
  "<!-- ref-classes:table -->",
  `Written by \`tools/procedural/gen_ref_classes.mjs\` from the catalogue; ${total} assets: ${byClass.object.length} object (a), ${byClass.scene.length} scene (b), ${byClass.behaviour.length} behaviour (c). Materials are always object.`,
  "",
  "| Category | Assets | Class (default) | Exceptions |",
  "|---|---|---|---|",
  ...rows,
  "",
  `**Scene (b), ${byClass.scene.length}:** ` + byClass.scene.map(nm).join(", ") + ".",
  "",
  `**Behaviour (c), ${byClass.behaviour.length}:** ` + byClass.behaviour.map(k => nm(k) + ", test scene `" + RC.assets[k].test_scene + "`, " + RC.assets[k].spec.length + " checks").join("; ") + ".",
  "",
  `**Volumetric air effects (FX) on neutral staging, ${Object.keys(stagingOf).length}** (set by hand with \`staging\` in library.json; they keep their class): ` + (Object.keys(stagingOf).length ? Object.keys(stagingOf).map(k => nm(k) + ", `" + stagingOf[k] + "`").join("; ") : "none") + ".",
  "<!-- /ref-classes:table -->",
].join("\n");

// The viewpoint audit: every asset with its view(s) and a one-line reason, Terrain and landforms first. Within a category, assets
// that share views and reason share a row.
const vName = v => (VP[v] && VP[v].label) || v;
const vText = vs => vs.length > 1 ? vs.map(vName).join(" + ") + " (" + (LY[(RC.assets[vs.key] || {}).layout] || LY["two-panel"] || { label: "one picture" }).label + ", one picture)" : vName(vs[0]);
const reasonOf = key => (VA.confirmed || {})[key] || (VA.asset_reason || {})[key] || (clsOf(key) === "scene" && (VA.category_reason || {})[key.split("-")[0]]) || (VA.class_reason || {})[clsOf(key)] || "";
const order = LIB.categories.slice().sort((a, b) => (a.id === "terrain" ? -1 : b.id === "terrain" ? 1 : 0));
const nSug = Object.keys(VA.suggested || {}).length;
const audit = ["<!-- ref-views:audit -->",
  `Written by \`tools/procedural/gen_ref_classes.mjs\` from \`catalog/procedural/ref_classes.json\` (\`assets.<key>.views\` in force, \`view_audit\` the reasons and suggestions); ${total} assets: ${total - multi.length} with one view, ${multi.length} with two views in one picture (${multi.map(k => names[k]).join(", ")}), ${nSug ? `${nSug} suggested for two views, **waiting for Chris to confirm** (they keep one view until he does)` : "none waiting for Chris to confirm"}.`,
  ""];
for (const c of order) {
  audit.push(`**${c.name}** (${c.assets.length})`, "", "| Asset | View(s) in force | Reason |", "|---|---|---|");
  const rows = [];
  for (const a of c.assets) {
    const key = c.id + "-" + a.id, vs = Object.assign(viewsOf(key).slice(), { key }), sug = (VA.suggested || {})[key];
    const view = vText(vs) + (sug ? `; suggested: ${sug.views.map(vName).join(" + ")} (Chris to confirm)` : "") + ((VA.confirmed || {})[key] ? " (confirmed)" : "");
    const why = sug ? sug.reason : reasonOf(key);
    const row = rows.find(r => r.view === view && r.why === why && !sug && !r.sug);
    if (row) row.names.push(a.name); else rows.push({ names: [a.name], view, why, sug: !!sug });
  }
  for (const r of rows) audit.push(`| ${r.names.join(", ")} | ${r.view} | ${r.why} |`);
  audit.push("");
}
audit.push("<!-- /ref-views:audit -->");

const DOC = "docs/ASSET_STUDIO_GUIDE.md", JSF = "tools/evani-commons-tab/classes.js";
let doc = rd(DOC);
const crlf = doc.includes("\r\n"); if (crlf) doc = doc.replace(/\r\n/g, "\n");
const m = /<!-- ref-classes:table -->[\s\S]*?<!-- \/ref-classes:table -->/;
if (!m.test(doc)) problems.push(`${DOC}: no <!-- ref-classes:table --> block`);
const mv = /<!-- ref-views:audit -->[\s\S]*?<!-- \/ref-views:audit -->/;
if (!mv.test(doc)) problems.push(`${DOC}: no <!-- ref-views:audit --> block`);
const doc2 = doc.replace(m, table).replace(mv, audit.join("\n"));
if (problems.length) { console.error("ref classes: " + problems.length + " problem(s)\n  " + problems.join("\n  ")); process.exit(1); }
const curJs = fs.existsSync(path.join(ROOT, JSF)) ? rd(JSF).replace(/\r\n/g, "\n") : "";
if (CHECK) {
  const stale = [curJs !== js && JSF, doc2 !== doc && DOC].filter(Boolean);
  if (stale.length) { console.error("ref classes: out of date: " + stale.join(", ") + " (run node tools/procedural/gen_ref_classes.mjs)"); process.exit(1); }
  console.log(`ref classes: clean (${total} assets: ${byClass.object.length} object, ${byClass.scene.length} scene, ${byClass.behaviour.length} behaviour)`);
} else {
  fs.writeFileSync(path.join(ROOT, JSF), js);
  if (doc2 !== doc) fs.writeFileSync(path.join(ROOT, DOC), crlf ? doc2.replace(/\n/g, "\r\n") : doc2);
  console.log(`ref classes: wrote ${JSF} and the table in ${DOC} (${total} assets: ${byClass.object.length} object, ${byClass.scene.length} scene, ${byClass.behaviour.length} behaviour)`);
}
