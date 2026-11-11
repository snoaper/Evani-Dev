#!/usr/bin/env node
// Evani.Materials catalogue: CSV <-> catalog/materials/materials.json.
//
//   node tools/materials/import_csv.mjs import [--force]   CSV -> JSON (one time; the JSON is the master after that)
//   node tools/materials/import_csv.mjs export [out.csv]   JSON -> CSV (for spreadsheet review)
//
// Dev tool only. Nothing here ships in a game.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const csvPath = path.join(root, 'docs', 'materials', 'catalogue.csv');
const jsonPath = path.join(root, 'catalog', 'materials', 'materials.json');
const COLUMNS = ['id', 'name', 'category', 'subcategory', 'recipe', 'search_words', 'look_note', 'batch', 'exists_today', 'conditions'];

function parseCsv(s) {
  const rows = [];
  let r = [], f = '', q = false;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) {
      if (c === '"') { if (s[i + 1] === '"') { f += '"'; i++; } else q = false; }
      else f += c;
    } else if (c === '"') q = true;
    else if (c === ',') { r.push(f); f = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && s[i + 1] === '\n') i++;
      r.push(f); f = ''; rows.push(r); r = [];
    } else f += c;
  }
  if (f || r.length) { r.push(f); rows.push(r); }
  return rows;
}

function csvField(v) {
  const s = String(v);
  return /[",\r\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s;
}

// One row per line keeps diffs readable; the file is still plain JSON.
function writeJson(doc) {
  const lines = doc.rows.map((r) => '    ' + JSON.stringify(r));
  return '{\n  "version": 1,\n  "note": ' + JSON.stringify(doc.note) + ',\n  "rows": [\n' + lines.join(',\n') + '\n  ]\n}\n';
}

const mode = process.argv[2];
if (mode === 'import') {
  if (fs.existsSync(jsonPath) && !process.argv.includes('--force')) {
    console.error('materials.json exists and is the master. Use --force to re-import from the CSV (this drops params).');
    process.exit(1);
  }
  const t = fs.readFileSync(csvPath, 'utf8');
  const rows = parseCsv(t);
  if (rows[0].join(',') !== COLUMNS.join(',')) throw new Error('unexpected CSV header');
  const out = rows.slice(1).map((r, i) => {
    if (r.length !== COLUMNS.length) throw new Error(`row ${i + 2}: ${r.length} fields`);
    const batch = Number(r[7]);
    if (!Number.isInteger(batch)) throw new Error(`row ${i + 2}: bad batch ${r[7]}`);
    return {
      id: r[0], name: r[1], category: r[2], subcategory: r[3], recipe: r[4],
      search_words: r[5], look_note: r[6], batch, exists_today: r[8],
      conditions: r[9].split(' ').filter(Boolean),
      params: {}, cond_defaults: {}, status: 'planned',
    };
  });
  const note = 'Master of the material catalogue (imported once from docs/materials/catalogue.csv). Regenerate the CSV with tools/materials/import_csv.mjs export. params and cond_defaults are overrides on the recipe defaults in recipes.json.';
  fs.writeFileSync(jsonPath, writeJson({ note, rows: out }));
  console.log(`imported ${out.length} rows -> ${path.relative(root, jsonPath)}`);
} else if (mode === 'export') {
  const doc = JSON.parse(fs.readFileSync(jsonPath, 'utf8'));
  const lines = [COLUMNS.join(',')];
  for (const r of doc.rows) {
    lines.push([r.id, r.name, r.category, r.subcategory, r.recipe, r.search_words, r.look_note, r.batch, r.exists_today, r.conditions.join(' ')].map(csvField).join(','));
  }
  const dest = process.argv[3] ? path.resolve(process.argv[3]) : csvPath;
  fs.writeFileSync(dest, lines.join('\r\n') + '\r\n');
  console.log(`exported ${doc.rows.length} rows -> ${dest}`);
} else {
  console.error('usage: import_csv.mjs import [--force] | export [out.csv]');
  process.exit(2);
}
