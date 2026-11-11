// Writes docs/studio/MATERIAL_QUEUE.md: every ready material the studio
// offers, in the order a user meets them on the opening asset (the materials
// bar's rule in apps/material-view/src/studio/page_view.rs `chip_materials`:
// families native to the asset first, then those that go on it, then the
// rest; bigger families first; then by family id; inside a family the
// catalogue order, the asset's default material first).
// Keeps the status and note of every row already in the queue.
// Usage: node tools/studio/material-queue.mjs [--write]
import fs from "node:fs";
const root = new URL("../../", import.meta.url);
const read = (p) => JSON.parse(fs.readFileSync(new URL(p, root), "utf8"));
const assets = read("catalog/assets/assets.json");
const mats = read("catalog/materials/materials.json").rows;
const asset = assets.assets.find((a) => a.id === assets.default_asset);
const cls = (id) => assets.classes.find((k) => k.id === id);
const matches = (p, c) => p === c || (p.endsWith(".*") && c.startsWith(p.slice(0, -1)));
const bo = (m) => (m.belongs_on === undefined ? [] : [].concat(m.belongs_on));
function rank(m) {
  const b = bo(m);
  if (b.some((p) => matches(p, asset.class))) return 0;
  if (b.length === 0 || cls(asset.class)?.swatch) return 1;
  return assets.classes.some((k) => k.exclusive && b.some((p) => matches(p, k.id))) ? 2 : 1;
}
const ready = mats.filter((m) => ["ready", "reviewed"].includes(m.status));
const fams = [];
for (const m of ready) {
  let f = fams.find((f) => f.sub === m.subcategory);
  if (!f) fams.push((f = { sub: m.subcategory, rank: 2, ids: [] }));
  f.rank = Math.min(f.rank, rank(m));
  f.ids.push(m);
}
fams.sort((a, b) => a.rank - b.rank || b.ids.length - a.ids.length || (a.sub < b.sub ? -1 : a.sub > b.sub ? 1 : 0));
for (const f of fams) {
  const i = f.ids.findIndex((m) => m.id === asset.default_material);
  if (i > 0) f.ids.unshift(...f.ids.splice(i, 1));
}
const path = new URL("docs/studio/MATERIAL_QUEUE.md", root);
const old = new Map();
if (fs.existsSync(path))
  for (const l of fs.readFileSync(path, "utf8").split("\n")) {
    const c = l.split("|").map((s) => s.trim());
    if (c.length > 5 && /^\d+$/.test(c[1])) old.set(c[2].replace(/`/g, ""), { status: c[5], note: c[6] });
  }
let n = 0;
let body = "";
for (const f of fams) {
  body += `\n### ${f.sub} (${f.ids.length})\n\n| # | Material | Name | Recipe | Status | Note |\n|---|---|---|---|---|---|\n`;
  for (const m of f.ids) {
    n++;
    const o = old.get(m.id);
    const note = o?.note || (m.look_note || "").replace(/\|/g, "/");
    body += `| ${n} | \`${m.id}\` | ${m.name.replace(/\|/g, "/")} | ${m.recipe || "-"} | ${o?.status || "todo"} | ${note} |\n`;
  }
}
const head = `# Material queue: one material at a time

Chris, 2026-10-31: "take 1 material at a time improve it then move on". Each
material worker takes the **top \`todo\` row**, improves exactly that material
(before and after pictures, checks, merge into \`studio\`, preview refresh),
marks it \`done\` with a one-line note, and stops.

Order: the order a user meets the materials in the studio, on the opening
asset (${asset.name}, default material \`${asset.default_material}\`): the
materials bar's families (native to the asset first, then the ones that go
on it, then the rest; bigger families first), each family in catalogue order.
${n} ready materials in total; planned rows join when they are built. Regenerate
with \`node tools/studio/material-queue.mjs --write\` (keeps every status and note).
`;
const out = head + body;
if (process.argv.includes("--write")) fs.writeFileSync(path, out);
console.log(`${n} materials in ${fams.length} families; first: ${fams[0].ids[0].id}, then ${fams[0].ids[1]?.id}`);
