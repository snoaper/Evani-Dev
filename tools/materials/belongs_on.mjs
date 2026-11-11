// The belongs_on fill (docs/studio/WORK_PACKAGES.md WP2 step 4): applies the
// reviewed rules in tools/materials/belongs_on.json to
// catalog/materials/materials.json.
//
//   node tools/materials/belongs_on.mjs           report; exit 1 if the file differs from the rules
//   node tools/materials/belongs_on.mjs --write   write the rules' values into materials.json
//
// Only `belongs_on` changes, in place on each row's one line (after
// `subcategory`), so the diff reads row by row. A rule naming a class that
// isn't in catalog/assets/assets.json is refused (`waits <class>` is the
// way to park one). Our own code, Node only, no packages.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const MATERIALS = join(root, "catalog", "materials", "materials.json");
const ASSETS = join(root, "catalog", "assets", "assets.json");
const RULES = join(root, "tools", "materials", "belongs_on.json");

/** A row's decision: { kind: "class" | "generic" | "waits", value, why, by }. */
export function decide(rules, row) {
  let best = null;
  for (const key of Object.keys(rules.ids)) {
    const hit = key.endsWith(".") ? row.id.startsWith(key) : row.id === key;
    if (hit && (!best || key.length > best.length)) best = key;
  }
  const raw = best ? rules.ids[best] : rules.subcategories[`${row.category}/${row.subcategory}`];
  const by = best ? `id ${best}` : `subcategory ${row.category}/${row.subcategory}`;
  if (raw === undefined) return { kind: "none", by };
  if (raw.startsWith("generic:")) return { kind: "generic", why: raw.slice(8).trim(), by };
  if (raw.startsWith("waits ")) {
    const rest = raw.slice(6);
    const at = rest.indexOf(":");
    return { kind: "waits", value: (at < 0 ? rest : rest.slice(0, at)).trim(), why: at < 0 ? "" : rest.slice(at + 1).trim(), by };
  }
  return { kind: "class", value: raw, by };
}

function main() {
  const write = process.argv.includes("--write");
  const rules = JSON.parse(readFileSync(RULES, "utf8"));
  const classes = new Set(JSON.parse(readFileSync(ASSETS, "utf8")).classes.map((c) => c.id));
  for (const [k, v] of [...Object.entries(rules.subcategories), ...Object.entries(rules.ids)]) {
    if (!v.startsWith("generic:") && !v.startsWith("waits ") && !classes.has(v)) {
      console.error(`belongs_on.json: \`${k}\` names \`${v}\`, which is no class in catalog/assets/assets.json (use \`waits ${v}: <why>\` until it is added)`);
      process.exit(2);
    }
  }
  const text = readFileSync(MATERIALS, "utf8");
  const lines = text.split("\n");
  const counts = {};
  const waits = {};
  const undecided = [];
  const changed = [];
  const out = lines.map((line) => {
    const t = line.trim();
    if (!t.startsWith('{"id":')) return line;
    const row = JSON.parse(t.replace(/,$/, ""));
    const d = decide(rules, row);
    const ready = ["ready", "reviewed", "alias"].includes(row.status);
    const tag = d.kind === "class" ? d.value : null;
    const key = d.kind === "class" ? d.value : d.kind === "waits" ? `waits ${d.value}` : d.kind;
    counts[key] = counts[key] || { rows: 0, ready: 0 };
    counts[key].rows += 1;
    counts[key].ready += ready ? 1 : 0;
    if (d.kind === "waits") (waits[d.value] = waits[d.value] || []).push(row.id);
    if (d.kind === "none") undecided.push(row.id);
    const now = row.belongs_on ?? null;
    if (JSON.stringify(now) === JSON.stringify(tag)) return line;
    changed.push(`${row.id}: ${JSON.stringify(now)} -> ${JSON.stringify(tag)} (${d.by})`);
    let next = line.replace(/"belongs_on":("[^"]*"|\[[^\]]*\]),/, "");
    if (tag) {
      const sub = `"subcategory":${JSON.stringify(row.subcategory)},`;
      if (!next.includes(sub)) throw new Error(`${row.id}: no subcategory field to write after`);
      next = next.replace(sub, `${sub}"belongs_on":${JSON.stringify(tag)},`);
    }
    return next;
  });
  console.log("belongs_on decisions (rows, of them ready):");
  for (const [k, v] of Object.entries(counts).sort()) console.log(`  ${k.padEnd(28)} ${String(v.rows).padStart(4)} ${String(v.ready).padStart(4)}`);
  console.log(`classes waiting to be added: ${Object.keys(waits).sort().join(", ") || "none"}`);
  if (undecided.length) {
    console.error(`no rule for ${undecided.length} row(s): ${undecided.slice(0, 10).join(", ")}${undecided.length > 10 ? " ..." : ""} (add their subcategory to tools/materials/belongs_on.json)`);
    process.exit(1);
  }
  if (!changed.length) {
    console.log("materials.json matches the rules.");
    return;
  }
  if (!write) {
    console.error(`${changed.length} row(s) differ from the rules (run with --write):\n  ${changed.slice(0, 20).join("\n  ")}`);
    process.exit(1);
  }
  const result = out.join("\n");
  JSON.parse(result);
  writeFileSync(MATERIALS, result);
  console.log(`wrote ${changed.length} row(s):\n  ${changed.join("\n  ")}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
