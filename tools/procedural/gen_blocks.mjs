// Building blocks for Asset Studio (docs/ASSET_STUDIO_GUIDE.md "Building blocks"): what each landscape and each block is made of.
// Reads catalog/procedural/blocks.json (the graph), catalog/procedural/library.json (procedural assets) and
// catalog/materials/materials.json (catalogue materials), checks the graph (every id exists, every link says why it helps and HOW: its
// kind, layer / scatter / base, with its params; no duplicate or self links, no cycles) and writes:
//   tools/evani-commons-tab/blockdata.js   the tab's copy (window.ASBLOCKS): each block resolved to its tab column (type, category, name)
//   docs/ASSET_STUDIO_GUIDE.md             the per-landscape table between <!-- blocks:table --> and <!-- /blocks:table -->
//   crates/evani-procedural-lib/src/blocks_table.rs  the engine's copy (evani_procedural::compose assembles an asset from it)
// node tools/procedural/gen_blocks.mjs          write both
// node tools/procedural/gen_blocks.mjs --check  fail (exit 1) when any is out of date or the graph is wrong
// node tools/procedural/gen_blocks.mjs --migrate  upgrade an older list (links without kinds) to typed links, backing it up first
// The checks are checkBlocks() below; tools/procedural/gen_blocks.test.mjs tests them (node --test).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const WHY_MAX = 200;
const KINDS = { material: "material:", planned: "planned:" };

/* ---------- typed links (blocks.json version 2; DECISIONS.md "Typed building-block links") ----------
   Every link says HOW the child feeds its parent: { block, why, kind, params }.
   - layer:   a material layer blended into the parent's surface by a mask: params { mask: [term...], blend, falloff, condition? }.
              A term is { by: slope | height | curvature | noise, min?, max?, scale? (noise only, patches per metre) }: slope in degrees
              (0..90), height a fraction of the parent's height span (0..1), curvature -1..1 (below 0 concave: cracks, hollows), noise
              0..1. blend is the layer's weight (0..1], falloff the soft edge as a fraction of each term's range (0..1). condition names
              the engine's shader layer that draws it (evani-material-recipes' condition layers), when it has one.
   - scatter: instanced copies: params { density (per m2), scale [min, max], lod (m), impostor (m, 0 = none, else at least lod),
              slope? [min, max] degrees, height? [min, max] 0..1, avoid_water? }.
   - base:    geometry or base shape: params { join: merge (joined into the parent's own meshes) | part (its own draw: sky, sea,
              a separate piece) | surface (the parent wears its surface: a material, or the stone a rock is proven on) }. */
export const EDGE_KINDS = ["layer", "scatter", "base"];
export const MASKS = { slope: { lo: 0, hi: 90, unit: "°" }, height: { lo: 0, hi: 1, unit: "" }, curvature: { lo: -1, hi: 1, unit: "" }, noise: { lo: 0, hi: 1, unit: "" } };
export const CONDITIONS = ["wet", "mud", "dust", "snow", "wear", "oxide", "growth", "blood"];
export const JOINS = ["merge", "part", "surface"];
const PARAMS = { layer: { req: ["mask", "blend", "falloff"], opt: ["condition"] }, scatter: { req: ["density", "scale", "lod", "impostor"], opt: ["slope", "height", "avoid_water"] }, base: { req: ["join"], opt: [] } };
const num = x => typeof x === "number" && Number.isFinite(x);
const pair = (x, lo, hi, strict) => Array.isArray(x) && x.length === 2 && num(x[0]) && num(x[1]) && x[0] >= lo && x[1] <= hi && (strict ? x[0] < x[1] : x[0] <= x[1]);

/* The problems with one link's kind and params (strings; empty when fine). childKind: "asset" | "material" | "planned". */
export function checkEdge(x, at, childKind) {
  const out = [];
  if (x.kind === undefined) return [`${at}: no kind (layer, scatter or base); an older list is upgraded by node tools/procedural/gen_blocks.mjs --migrate`];
  if (!EDGE_KINDS.includes(x.kind)) return [`${at}: unknown kind "${x.kind}" (a link is a layer, a scatter or a base)`];
  const P = PARAMS[x.kind], p = x.params;
  if (!p || typeof p !== "object" || Array.isArray(p)) return [`${at}: a ${x.kind} link needs params (${P.req.join(", ")})`];
  const miss = P.req.filter(k => p[k] === undefined); if (miss.length) out.push(`${at}: the ${x.kind} link is missing ${miss.join(", ")}`);
  const extra = Object.keys(p).filter(k => !P.req.includes(k) && !P.opt.includes(k)); if (extra.length) out.push(`${at}: unknown ${x.kind} param(s) ${extra.join(", ")}`);
  if (x.kind === "layer") {
    if (p.mask !== undefined) {
      if (!Array.isArray(p.mask) || !p.mask.length) out.push(`${at}: mask must list one or more terms ({ by, min?, max? })`);
      else p.mask.forEach((t, i) => {
        const m = t && MASKS[t.by], ta = `${at}: mask[${i}]`;
        if (!m) { out.push(`${ta}: by must be ${Object.keys(MASKS).join(", ")}`); return; }
        const bad = Object.keys(t).filter(k => !["by", "min", "max", "scale"].includes(k)); if (bad.length) out.push(`${ta}: unknown field(s) ${bad.join(", ")}`);
        if (t.min === undefined && t.max === undefined) out.push(`${ta}: a ${t.by} term needs a min or a max threshold`);
        for (const k of ["min", "max"]) if (t[k] !== undefined && (!num(t[k]) || t[k] < m.lo || t[k] > m.hi)) out.push(`${ta}: ${k} must be a number from ${m.lo} to ${m.hi}`);
        if (num(t.min) && num(t.max) && t.min >= t.max) out.push(`${ta}: min must be below max`);
        if (t.scale !== undefined && (t.by !== "noise" || !num(t.scale) || t.scale <= 0)) out.push(`${ta}: scale (patches per metre, above 0) is for noise terms only`);
      });
    }
    if (p.blend !== undefined && (!num(p.blend) || p.blend <= 0 || p.blend > 1)) out.push(`${at}: blend must be above 0 and at most 1`);
    if (p.falloff !== undefined && (!num(p.falloff) || p.falloff < 0 || p.falloff > 1)) out.push(`${at}: falloff must be from 0 to 1`);
    if (p.condition !== undefined && !CONDITIONS.includes(p.condition)) out.push(`${at}: condition must be one of the engine's shader layers (${CONDITIONS.join(", ")})`);
  } else if (x.kind === "scatter") {
    if (childKind === "material") out.push(`${at}: a material can't be scattered (make it a layer, or a base surface)`);
    if (p.density !== undefined && (!num(p.density) || p.density <= 0 || p.density > 1000)) out.push(`${at}: density (copies per m2) must be above 0 and at most 1000`);
    if (p.scale !== undefined && !pair(p.scale, 0.001, 100, false)) out.push(`${at}: scale must be [min, max] with 0 < min <= max <= 100`);
    if (p.lod !== undefined && (!num(p.lod) || p.lod <= 0)) out.push(`${at}: lod (metres to the lower detail) must be above 0`);
    if (p.impostor !== undefined && (!num(p.impostor) || p.impostor < 0 || (p.impostor > 0 && num(p.lod) && p.impostor < p.lod))) out.push(`${at}: impostor (metres) must be 0 (none) or at least lod`);
    if (p.slope !== undefined && !pair(p.slope, 0, 90, true)) out.push(`${at}: slope must be [min, max] degrees from 0 to 90`);
    if (p.height !== undefined && !pair(p.height, 0, 1, true)) out.push(`${at}: height must be [min, max] from 0 to 1`);
    if (p.avoid_water !== undefined && typeof p.avoid_water !== "boolean") out.push(`${at}: avoid_water must be true or false`);
  } else if (x.kind === "base") {
    if (p.join !== undefined && !JOINS.includes(p.join)) out.push(`${at}: join must be ${JOINS.join(", ")}`);
    if (childKind === "material" && (p.join === "merge" || p.join === "part")) out.push(`${at}: a material has no geometry: use join surface, or make it a layer`);
  }
  return out;
}

/* Defaults for an untyped link (Chris confirmed, 2026-11-08): snow on high, flat ground; moss in low, shady cracks (low and concave:
   the curvature stands in for low light); small objects scattered sparsely. */
export const SNOW = { mask: [{ by: "height", min: 0.6 }, { by: "slope", max: 35 }], blend: 0.9, falloff: 0.15, condition: "snow" };
export const MOSS = { mask: [{ by: "height", max: 0.4 }, { by: "curvature", max: -0.1 }], blend: 0.8, falloff: 0.2, condition: "growth" };
const LAYER_PLANNED = {
  "stony-scar": { mask: [{ by: "slope", min: 30 }], blend: 1, falloff: 0.1 },
  "alpine-meadow": { mask: [{ by: "height", min: 0.15, max: 0.6 }, { by: "slope", max: 30 }], blend: 1, falloff: 0.15 },
  "meadow": { mask: [{ by: "slope", max: 20 }, { by: "height", max: 0.5 }], blend: 1, falloff: 0.15 },
  "dry-wash": { mask: [{ by: "height", max: 0.1 }, { by: "curvature", max: -0.2 }], blend: 1, falloff: 0.1 },
  "dune-grass": { mask: [{ by: "height", min: 0.3 }, { by: "noise", min: 0.4, scale: 0.2 }], blend: 1, falloff: 0.2 },
  "cave-floor": { mask: [{ by: "slope", max: 25 }], blend: 1, falloff: 0.1 },
};
const sc = (density, scale, lod, impostor, more) => Object.assign({ density, scale, lod, impostor }, more || {});
const SCATTER = {
  "rocks-pebbles": sc(0.4, [0.6, 1.4], 25, 0, { avoid_water: true }),
  "rocks-small-stones": sc(0.4, [0.6, 1.4], 25, 0, { avoid_water: true }),
  "rocks-medium-rocks": sc(0.02, [0.7, 1.4], 80, 250, { slope: [0, 45] }),
  "rocks-rounded-river-rocks": sc(0.1, [0.6, 1.3], 60, 0, { avoid_water: false }),
  "rocks-jagged-rock-formations": sc(0.01, [0.7, 1.3], 120, 400),
  "trees-oak-trees": sc(0.002, [0.8, 1.2], 120, 600, { slope: [0, 30], avoid_water: true }),
  "trees-pine-trees": sc(0.05, [0.8, 1.25], 120, 600, { slope: [0, 35], avoid_water: true }),
  "trees-dense-forest-clusters": sc(0.002, [0.8, 1.2], 200, 1200, { slope: [0, 35], height: [0, 0.6], avoid_water: true }),
  "trees-tree-canopies": sc(0.005, [0.8, 1.2], 150, 800, { avoid_water: true }),
  "trees-hedgerows": sc(0.002, [0.9, 1.1], 120, 500, { slope: [0, 20], avoid_water: true }),
  "vegetation-short-grass": sc(4, [0.8, 1.2], 40, 0, { slope: [0, 40], avoid_water: true }),
  "vegetation-long-grass": sc(3, [0.8, 1.3], 40, 0, { slope: [0, 45], avoid_water: true }),
  "vegetation-wildflowers": sc(0.3, [0.8, 1.2], 30, 0, { slope: [0, 30], avoid_water: true }),
  "vegetation-shrubs": sc(0.05, [0.7, 1.3], 60, 250, { slope: [0, 40], avoid_water: true }),
  "vegetation-reeds": sc(1, [0.8, 1.2], 40, 0, { height: [0, 0.2], avoid_water: false }),
  "agriculture-crop-rows": sc(0.05, [0.9, 1.1], 80, 300, { slope: [0, 15] }),
};
const PART = /^(sky-|effects-|architecture-)|^water-(oceans|streams|wave-foam|rivers|lakes)$|^rocks-scree-slopes$/;
const MERGE = /^terrain-|^rocks-rock-walls$|^water-ocean-waves$/;
const copy = o => JSON.parse(JSON.stringify(o));
/* The default { kind, params } of a link from parent to child (an id); childKind as in checkEdge. */
export function defaultEdge(parent, child, childKind) {
  if (childKind === "material") {
    if (/snow/.test(child)) return { kind: "layer", params: copy(SNOW) };
    if (/moss/.test(child)) return { kind: "layer", params: copy(MOSS) };
    return { kind: "base", params: { join: "surface" } };
  }
  if (childKind === "planned") { const p = LAYER_PLANNED[child.slice(KINDS.planned.length)]; return p ? { kind: "layer", params: copy(p) } : { kind: "base", params: { join: "merge" } }; }
  if (child === "vegetation-moss-patches") return { kind: "layer", params: copy(MOSS) };
  if (/^rocks-/.test(parent) && child === "rocks-pebbles") return { kind: "base", params: { join: "surface" } }; // rock chain by size: the stone proven small
  if (PART.test(child)) return { kind: "base", params: { join: "part" } };
  if (MERGE.test(child)) return { kind: "base", params: { join: "merge" } };
  return { kind: "scatter", params: copy(SCATTER[child] || sc(0.1, [0.8, 1.2], 60, 0)) };
}
const childKindOf = id => (typeof id === "string" && id.startsWith(KINDS.material) ? "material" : typeof id === "string" && id.startsWith(KINDS.planned) ? "planned" : "asset");
/* An older list (version 1, links without kinds) upgraded: every untyped link gets its default. Typed links are kept as they are.
   Returns { data (a copy, version 2), migrated (links filled in) }. */
export function migrateBlocks(data) {
  const d = copy(data || {}); let migrated = 0;
  for (const [id, list] of Object.entries(d.needs || {})) if (Array.isArray(list)) list.forEach(x => { if (x && typeof x.block === "string" && x.kind === undefined && x.params === undefined) { Object.assign(x, defaultEdge(id, x.block, childKindOf(x.block))); migrated++; } });
  if (migrated || !d.version || d.version < 2) d.version = 2;
  return { data: d, migrated };
}
/* blocks.json as written: one landscape, planned block or link per line. */
const compact = v => (Array.isArray(v) ? "[" + v.map(compact).join(", ") + "]" : v && typeof v === "object" ? "{ " + Object.entries(v).map(([k, x]) => JSON.stringify(k) + ": " + compact(x)).join(", ") + " }" : JSON.stringify(v));
export function formatBlocks(d) {
  const keys = Object.keys(d), out = ["{"];
  keys.forEach((k, i) => {
    const v = d[k], end = i < keys.length - 1 ? "," : "";
    if (["landscapes", "planned"].includes(k) && v && typeof v === "object") { const e = Object.entries(v); out.push(` ${JSON.stringify(k)}: {`, ...e.map(([id, x], j) => `  ${JSON.stringify(id)}: ${compact(x)}${j < e.length - 1 ? "," : ""}`), ` }${end}`); }
    else if (k === "needs" && v && typeof v === "object") { const e = Object.entries(v); out.push(` "needs": {`, ...e.map(([id, l], j) => `  ${JSON.stringify(id)}: [\n` + l.map((x, m) => `   ${compact(x)}${m < l.length - 1 ? "," : ""}`).join("\n") + `\n  ]${j < e.length - 1 ? "," : ""}`), ` }${end}`); }
    else out.push(` ${JSON.stringify(k)}: ${JSON.stringify(v)}${end}`);
  });
  out.push("}");
  return out.join("\n") + "\n";
}
const fmtN = n => String(+n.toFixed(3));
/* How a child is used, in a few words for the tree's link: "blend: slope > 35°", "scatter: 0.4/m²", "base". */
export function edgeLabel(x) {
  if (!x || !x.params) return "";
  const p = x.params;
  if (x.kind === "layer") return "blend: " + (p.mask || []).map(t => { const u = (MASKS[t.by] || {}).unit || ""; const nm = t.by === "curvature" ? "curve" : t.by; return t.min !== undefined && t.max !== undefined ? `${nm} ${fmtN(t.min)}–${fmtN(t.max)}${u}` : t.min !== undefined ? `${nm} > ${fmtN(t.min)}${u}` : `${nm} < ${fmtN(t.max)}${u}`; }).join(", ");
  if (x.kind === "scatter") return "scatter: " + fmtN(p.density) + "/m²";
  if (x.kind === "base") return p.join === "surface" ? "base: surface" : p.join === "part" ? "base: own part" : "base";
  return "";
}
/* The same graph as Rust data for the engine (evani-procedural's compose module reads it): crates/evani-procedural-lib/src/blocks_table.rs. */
export function rustTable(needs) {
  const f = n => { const s = String(+n); return /[.eE]/.test(s) ? s : s + ".0"; };
  const opt = v => (v === undefined ? "None" : `Some(${f(v)})`);
  const MASK = { slope: "Slope", height: "Height", curvature: "Curvature", noise: "Noise" };
  const kind = x => {
    const p = x.params;
    if (x.kind === "layer") return `Kind::Layer(Layer { mask: &[${p.mask.map(t => `Term { by: Mask::${MASK[t.by]}, min: ${opt(t.min)}, max: ${opt(t.max)}, scale: ${f(t.scale ?? 1)} }`).join(", ")}], blend: ${f(p.blend)}, falloff: ${f(p.falloff)}, condition: ${p.condition ? `Some(${JSON.stringify(p.condition)})` : "None"} })`;
    if (x.kind === "scatter") return `Kind::Scatter(Scatter { density: ${f(p.density)}, scale: [${f(p.scale[0])}, ${f(p.scale[1])}], lod: ${f(p.lod)}, impostor: ${f(p.impostor)}, slope: [${f((p.slope || [0, 90])[0])}, ${f((p.slope || [0, 90])[1])}], height: [${f((p.height || [0, 1])[0])}, ${f((p.height || [0, 1])[1])}], avoid_water: ${!!p.avoid_water} })`;
    return `Kind::Base(Join::${p.join[0].toUpperCase() + p.join.slice(1)})`;
  };
  const ids = Object.keys(needs).sort();
  return "//! The typed building-block graph (catalog/procedural/blocks.json) as engine data: written by\n" +
    "//! tools/procedural/gen_blocks.mjs, never by hand (`--check` fails when it is stale). Read through\n" +
    "//! evani_procedural::compose (docs/PROCEDURAL.md \"Composition from typed links\").\n\n" +
    "use evani_procedural::compose::{Edge, Graph, Join, Kind, Layer, Mask, Node, Scatter, Term};\n\n" +
    "/// Every block with links, sorted by id.\npub static GRAPH: Graph = Graph {\n    nodes: &[\n" +
    ids.map(id => `        Node {\n            id: ${JSON.stringify(id)},\n            edges: &[\n` + needs[id].map(x => `                Edge { block: ${JSON.stringify(x.block)}, kind: ${kind(x)} },\n`).join("") + "            ],\n        },\n").join("") +
    "    ],\n};\n";
}

/* What an id names: { kind: "asset" | "material" | "planned", name, t?, c?, n? } or null. t/c/n are the tab column's type, category and
   item name (Procedure / category id / asset name; Material / material category / material name). */
export function resolver(lib, mats, planned) {
  const assets = new Map(), materials = new Map();
  for (const c of (lib && lib.categories) || []) for (const a of c.assets || []) assets.set(c.id + "-" + a.id, { kind: "asset", name: a.name, t: "Procedure", c: c.id, n: a.name, cat: c.name });
  for (const r of (mats && mats.rows) || []) if (r && r.id) materials.set(r.id, { kind: "material", name: r.name, t: "Material", c: r.category, sub: r.subcategory, n: r.name });
  return id => {
    if (typeof id !== "string" || !id) return null;
    if (id.startsWith(KINDS.material)) return materials.get(id.slice(KINDS.material.length)) || null;
    if (id.startsWith(KINDS.planned)) { const p = planned && planned[id.slice(KINDS.planned.length)]; return p && typeof p.name === "string" ? { kind: "planned", name: p.name, note: p.note || "" } : null; }
    return assets.get(id) || null;
  };
}

/* The cycles in a needs map, each as the id path that closes on itself ([a, b, a]); [] for a DAG. */
export function findCycles(needs) {
  const state = new Map(), stack = [], cycles = [], seen = new Set();
  const kids = id => (Array.isArray(needs[id]) ? needs[id] : []).map(x => x && x.block).filter(x => typeof x === "string");
  const visit = id => {
    state.set(id, 1); stack.push(id);
    for (const k of kids(id)) {
      if (state.get(k) === 1) { const loop = stack.slice(stack.indexOf(k)).concat([k]); const sig = loop.slice(0, -1).slice().sort().join("|"); if (!seen.has(sig)) { seen.add(sig); cycles.push(loop); } }
      else if (!state.has(k)) visit(k);
    }
    stack.pop(); state.set(id, 2);
  };
  for (const id of Object.keys(needs || {})) if (!state.has(id)) visit(id);
  return cycles;
}

/* Every problem with the graph (strings, empty when it is fine) and, when fine, the resolved graph. */
export function checkBlocks(data, lib, mats) {
  const problems = [];
  if (!data || typeof data !== "object") return { problems: ["blocks.json: not an object"] };
  const planned = data.planned || {}, needs = data.needs || {}, lands = data.landscapes || {};
  const res = resolver(lib, mats, planned);
  const label = id => { const r = res(id); return r ? r.name + " (" + id + ")" : id; };
  for (const [pid, p] of Object.entries(planned)) {
    if (!/^[a-z0-9]+(-[a-z0-9]+)*$/.test(pid)) problems.push(`planned.${pid}: ids are lower-case words joined by -`);
    if (!p || typeof p.name !== "string" || !p.name.trim()) problems.push(`planned.${pid}: needs a name`);
  }
  for (const [id, list] of Object.entries(needs)) {
    if (!res(id)) problems.push(`needs.${id}: no such block (a library asset key, material:<id> or planned:<id>)`);
    if (!Array.isArray(list) || !list.length) { problems.push(`needs.${id}: must list one or more blocks (leave a block with none out)`); continue; }
    const dup = new Set();
    list.forEach((x, i) => {
      const at = `needs.${id}[${i}]`;
      if (!x || typeof x.block !== "string") { problems.push(`${at}: needs a block id`); return; }
      if (!res(x.block)) problems.push(`${at}: ${x.block} is not a library asset, catalogue material or planned block`);
      if (x.block === id) problems.push(`${at}: ${label(id)} can't need itself`);
      if (dup.has(x.block)) problems.push(`${at}: ${label(x.block)} is listed twice`);
      dup.add(x.block);
      const why = typeof x.why === "string" ? x.why.trim() : "";
      if (!why) problems.push(`${at}: the link ${label(x.block)} -> ${label(id)} needs a one-line why (how it helps its parent)`);
      else if (why.length > WHY_MAX || /\n/.test(why)) problems.push(`${at}: the why must be one line of ${WHY_MAX} characters at most`);
      const extra = Object.keys(x).filter(k => !["block", "why", "kind", "params"].includes(k)); if (extra.length) problems.push(`${at}: unknown field(s) ${extra.join(", ")}`);
      const r = res(x.block); problems.push(...checkEdge(x, `${at} (${label(x.block)} -> ${label(id)})`, r ? r.kind : childKindOf(x.block)));
    });
  }
  for (const [key, L] of Object.entries(lands)) {
    const r = res(key);
    if (!r || r.kind !== "asset") problems.push(`landscapes.${key}: not a procedural asset in library.json`);
    if (!Array.isArray(needs[key]) || !needs[key].length) problems.push(`landscapes.${key}: has no blocks in needs`);
    if (!L || !["picture", "framing", "name"].includes(L.evidence)) problems.push(`landscapes.${key}: evidence must be picture, framing or name`);
    if (!L || typeof L.source !== "string" || !L.source.trim()) problems.push(`landscapes.${key}: needs a source (what the tree was worked out from)`);
  }
  const used = new Set(); Object.values(needs).forEach(l => (Array.isArray(l) ? l : []).forEach(x => x && used.add(x.block)));
  for (const pid of Object.keys(planned)) if (!used.has("planned:" + pid)) problems.push(`planned.${pid}: no parent uses it`);
  for (const cyc of findCycles(needs)) problems.push("cycle: " + cyc.map(label).join(" -> ") + " (a block can't be made of itself; remove one of these links)");
  if (problems.length) return { problems };
  // The resolved graph: every id that appears, with its tab column.
  const nodes = {}, add = id => { if (!nodes[id]) nodes[id] = res(id); };
  Object.keys(lands).forEach(add); Object.entries(needs).forEach(([id, l]) => { add(id); l.forEach(x => add(x.block)); });
  return { problems, nodes, needs, landscapes: lands, planned };
}

/* Per landscape: its blocks (every block under it), leaves (blocks with no blocks of their own), merges (blocks under it with two or
   more blocks), depth (links on the longest path down to a leaf) and the blocks it shares with other landscapes. */
export function landscapeStats(g) {
  const kids = id => (g.needs[id] || []).map(x => x.block);
  const under = id => { const out = new Set(), go = x => kids(x).forEach(k => { if (!out.has(k)) { out.add(k); go(k); } }); go(id); return out; };
  const depthMemo = new Map(), depth = id => { if (depthMemo.has(id)) return depthMemo.get(id); const d = kids(id).length ? 1 + Math.max(...kids(id).map(depth)) : 0; depthMemo.set(id, d); return d; };
  const keys = Object.keys(g.landscapes), U = Object.fromEntries(keys.map(k => [k, under(k)]));
  return keys.map(k => {
    const all = [...U[k]];
    const shared = all.filter(b => keys.some(o => o !== k && U[o].has(b)));
    return { key: k, name: g.nodes[k].name, evidence: g.landscapes[k].evidence, blocks: all.length, leaves: all.filter(b => !kids(b).length).length, merges: all.filter(b => kids(b).length >= 2).length, depth: depth(k), shared };
  });
}

export function tabCopy(g, data) {
  const nodes = {};
  for (const [id, n] of Object.entries(g.nodes)) nodes[id] = n.kind === "planned" ? { kind: "planned", name: n.name, note: n.note } : { kind: n.kind, name: n.name, t: n.t, c: n.c, n: n.n };
  const needs = {};
  for (const [id, l] of Object.entries(g.needs)) needs[id] = l.map(x => ({ block: x.block, why: x.why, kind: x.kind, params: x.params, use: edgeLabel(x) }));
  return { version: data.version, built_from: "catalog/procedural/blocks.json + library.json + catalog/materials/materials.json", draft: data.draft || "", rule: data.rule || "", landscapes: g.landscapes, nodes, needs };
}
/* Per landscape: its direct links by kind. */
const kindCount = (g, k) => { const c = { layer: 0, scatter: 0, base: 0 }; (g.needs[k] || []).forEach(x => { c[x.kind]++; }); return c; };

function main() {
  const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
  const rd = p => fs.readFileSync(path.join(ROOT, p), "utf8");
  const CHECK = process.argv.includes("--check");
  const BJ = "catalog/procedural/blocks.json";
  let data = JSON.parse(rd(BJ));
  const lib = JSON.parse(rd("catalog/procedural/library.json")), mats = JSON.parse(rd("catalog/materials/materials.json"));
  if (process.argv.includes("--migrate")) { // an older (untyped) list: back it up, fill in each link's default kind and params
    const m = migrateBlocks(data);
    if (m.migrated) {
      const bak = path.join(ROOT, ".evani", "tmp", "block-edges", "backup"); fs.mkdirSync(bak, { recursive: true });
      fs.copyFileSync(path.join(ROOT, BJ), path.join(bak, "blocks." + Date.now() + ".json"));
      fs.writeFileSync(path.join(ROOT, BJ), formatBlocks(m.data));
      console.log(`building blocks: typed ${m.migrated} link(s) in ${BJ} (backup in .evani/tmp/block-edges/backup/)`);
    } else console.log("building blocks: every link already has a kind");
    data = m.data;
  }
  const g = checkBlocks(data, lib, mats);
  if (g.problems.length) { console.error("building blocks: " + g.problems.length + " problem(s)\n  " + g.problems.join("\n  ")); process.exit(1); }
  const js = "/* Asset Studio building blocks: written by tools/procedural/gen_blocks.mjs from catalog/procedural/blocks.json (with library.json and\n" +
    "   catalog/materials/materials.json). Do not edit by hand: change the catalogue and run the script. */\n" +
    "window.ASBLOCKS = " + JSON.stringify(tabCopy(g, data)) + ";\n";
  const st = landscapeStats(g), nm = id => g.nodes[id].name;
  const table = ["<!-- blocks:table -->",
    `Written by \`tools/procedural/gen_blocks.mjs\` from \`catalog/procedural/blocks.json\`: ${st.length} landscapes, ${Object.keys(g.nodes).length} blocks in all (${Object.keys(g.planned).length} planned, not built yet), ${Object.values(g.needs).reduce((a, l) => a + l.length, 0)} links, each with its why and its kind (${["layer", "scatter", "base"].map(k => Object.values(g.needs).reduce((a, l) => a + l.filter(x => x.kind === k).length, 0) + " " + k).join(", ")}). **${data.draft || "Draft"}**`,
    "", "| Landscape | Evidence | Its links: layer / scatter / base | Leaf blocks | Merge blocks | Depth | Shared with other landscapes |", "|---|---|---|---|---|---|---|",
    ...st.map(s => { const c = kindCount(g, s.key); return `| ${s.name} | ${s.evidence} | ${c.layer} / ${c.scatter} / ${c.base} | ${s.leaves} | ${s.merges} | ${s.depth} | ${s.shared.length ? s.shared.map(nm).join(", ") : "none"} |`; }),
    "<!-- /blocks:table -->"].join("\n");
  const DOC = "docs/ASSET_STUDIO_GUIDE.md", JSF = "tools/evani-commons-tab/blockdata.js", RS = "crates/evani-procedural-lib/src/blocks_table.rs";
  const rs = rustTable(g.needs), curRs = fs.existsSync(path.join(ROOT, RS)) ? rd(RS).replace(/\r\n/g, "\n") : "";
  let doc = rd(DOC); const crlf = doc.includes("\r\n"); if (crlf) doc = doc.replace(/\r\n/g, "\n");
  const m = /<!-- blocks:table -->[\s\S]*?<!-- \/blocks:table -->/;
  if (!m.test(doc)) { console.error(`building blocks: ${DOC} has no <!-- blocks:table --> block`); process.exit(1); }
  const doc2 = doc.replace(m, table);
  const curJs = fs.existsSync(path.join(ROOT, JSF)) ? rd(JSF).replace(/\r\n/g, "\n") : "";
  const sum = `${st.length} landscapes, ${Object.keys(g.nodes).length} blocks, no cycles`;
  if (CHECK) {
    const stale = [curJs !== js && JSF, doc2 !== doc && DOC, curRs !== rs && RS].filter(Boolean);
    if (stale.length) { console.error("building blocks: out of date: " + stale.join(", ") + " (run node tools/procedural/gen_blocks.mjs)"); process.exit(1); }
    console.log("building blocks: clean (" + sum + ")");
  } else {
    fs.writeFileSync(path.join(ROOT, JSF), js);
    if (curRs !== rs) fs.writeFileSync(path.join(ROOT, RS), rs);
    if (doc2 !== doc) fs.writeFileSync(path.join(ROOT, DOC), crlf ? doc2.replace(/\n/g, "\r\n") : doc2);
    console.log(`building blocks: wrote ${JSF}, ${RS} and the table in ${DOC} (${sum})`);
    if (process.argv.includes("--report")) for (const s of st) console.log(`${s.name}\t${s.evidence}\tleaves ${s.leaves}\tmerges ${s.merges}\tdepth ${s.depth}\tshared ${s.shared.length}: ${s.shared.map(nm).join(", ")}`);
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
