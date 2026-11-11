// Tests for the building-block checks (tools/procedural/gen_blocks.mjs). Run: node --test tools/procedural/gen_blocks.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkBlocks, findCycles, landscapeStats, migrateBlocks, defaultEdge, edgeLabel, rustTable, formatBlocks, SNOW, MOSS } from "./gen_blocks.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const rd = p => JSON.parse(fs.readFileSync(path.join(ROOT, p), "utf8"));
const LIB = { categories: [
  { id: "terrain", name: "Terrain", assets: [{ id: "mountains", name: "Mountains" }, { id: "mountain-ranges", name: "Mountain ranges" }] },
  { id: "rocks", name: "Rocks", assets: [{ id: "pebbles", name: "Pebbles" }, { id: "medium-rocks", name: "Medium rocks" }] },
  { id: "trees", name: "Trees", assets: [{ id: "oak-trees", name: "Oak trees" }] },
] };
const MATS = { rows: [{ id: "bark.oak", name: "Bark, oak", category: "plants", subcategory: "bark" }] };
const graph = needs => ({ landscapes: { "terrain-mountains": { evidence: "picture", source: "test" } }, planned: {}, needs });
const SC = { kind: "scatter", params: { density: 0.4, scale: [0.6, 1.4], lod: 25, impostor: 0 } };
const SURF = { kind: "base", params: { join: "surface" } };
const ok = {
  "terrain-mountains": [{ block: "rocks-medium-rocks", why: "boulders at the foot", ...SC }, { block: "trees-oak-trees", why: "trees on the foothills", ...SC }],
  "rocks-medium-rocks": [{ block: "rocks-pebbles", why: "rock chain by size", ...SURF }],
  "trees-oak-trees": [{ block: "material:bark.oak", why: "the trunk's bark", ...SURF }],
};
// The problems when Medium rocks' one link (to Pebbles) is `edge`.
const one = edge => checkBlocks(graph({ ...ok, "rocks-medium-rocks": [{ block: "rocks-pebbles", why: "w", ...edge }] }), LIB, MATS).problems;

test("a sound graph passes and resolves each block to its tab column", () => {
  const g = checkBlocks(graph(ok), LIB, MATS);
  assert.deepEqual(g.problems, []);
  assert.deepEqual(g.nodes["material:bark.oak"], { kind: "material", name: "Bark, oak", t: "Material", c: "plants", sub: "bark", n: "Bark, oak" });
  assert.equal(g.nodes["rocks-pebbles"].t, "Procedure");
  const [m] = landscapeStats(g);
  assert.deepEqual([m.blocks, m.leaves, m.merges, m.depth], [4, 2, 0, 2]);
});

test("a cycle is refused with its path, in names", () => {
  const needs = { ...ok, "rocks-pebbles": [{ block: "terrain-mountains", why: "loops back", kind: "base", params: { join: "merge" } }] };
  const g = checkBlocks(graph(needs), LIB, MATS);
  const cyc = g.problems.filter(p => p.startsWith("cycle: "));
  assert.equal(cyc.length, 1, g.problems.join("\n"));
  assert.match(cyc[0], /Mountains \(terrain-mountains\) -> Medium rocks \(rocks-medium-rocks\) -> Pebbles \(rocks-pebbles\) -> Mountains \(terrain-mountains\)/);
  assert.equal(g.nodes, undefined, "no graph is handed on when it has a cycle");
  assert.deepEqual(findCycles({ a: [{ block: "b" }], b: [{ block: "a" }] }), [["a", "b", "a"]]);
  assert.deepEqual(findCycles(ok), []);
});

test("a block that needs itself is refused", () => {
  const g = checkBlocks(graph({ ...ok, "rocks-pebbles": [{ block: "rocks-pebbles", why: "itself", ...SURF }] }), LIB, MATS);
  assert.ok(g.problems.some(p => /can't need itself/.test(p)), g.problems.join("\n"));
});

test("every link needs its one-line why", () => {
  const g = checkBlocks(graph({ ...ok, "rocks-medium-rocks": [{ block: "rocks-pebbles", why: " ", ...SURF }] }), LIB, MATS);
  assert.ok(g.problems.some(p => /needs a one-line why/.test(p)), g.problems.join("\n"));
  const g2 = checkBlocks(graph({ ...ok, "rocks-medium-rocks": [{ block: "rocks-pebbles", why: "x".repeat(201), ...SURF }] }), LIB, MATS);
  assert.ok(g2.problems.some(p => /one line of 200 characters/.test(p)));
});

test("unknown ids, duplicates and unused planned blocks are refused", () => {
  const data = graph({ ...ok, "terrain-mountains": [...ok["terrain-mountains"], { block: "rocks-nope", why: "x", ...SC }, { block: "material:nope", why: "x", ...SURF }, { block: "rocks-medium-rocks", why: "again", ...SC }] });
  data.planned = { riverbank: { name: "Riverbank" } };
  const p = checkBlocks(data, LIB, MATS).problems.join("\n");
  assert.match(p, /rocks-nope is not a library asset/);
  assert.match(p, /material:nope is not a library asset/);
  assert.match(p, /Medium rocks \(rocks-medium-rocks\) is listed twice/);
  assert.match(p, /planned.riverbank: no parent uses it/);
});

test("the catalogue's own graph is sound (no cycles, every id, why and kind)", () => {
  const g = checkBlocks(rd("catalog/procedural/blocks.json"), rd("catalog/procedural/library.json"), rd("catalog/materials/materials.json"));
  assert.deepEqual(g.problems, []);
  assert.ok(Object.keys(g.landscapes).length >= 4);
  for (const k of ["terrain-mountains", "water-rivers", "terrain-rolling-hills", "water-shorelines"]) assert.ok(g.needs[k], k + " is seeded");
});

// ---------- typed links (version 2) ----------
test("every link needs a known kind", () => {
  assert.ok(one({}).some(p => /no kind \(layer, scatter or base\).*--migrate/.test(p)), one({}).join("\n"));
  const p = one({ kind: "sprinkle", params: {} });
  assert.ok(p.some(x => /unknown kind "sprinkle" \(a link is a layer, a scatter or a base\)/.test(x)), p.join("\n"));
});

test("missing and unknown params are refused, each named", () => {
  assert.ok(one({ kind: "layer" }).some(p => /a layer link needs params \(mask, blend, falloff\)/.test(p)));
  assert.ok(one({ kind: "layer", params: { mask: [{ by: "slope", min: 35 }] } }).some(p => /the layer link is missing blend, falloff/.test(p)));
  assert.ok(one({ kind: "scatter", params: { density: 0.4 } }).some(p => /the scatter link is missing scale, lod, impostor/.test(p)));
  assert.ok(one({ kind: "base", params: {} }).some(p => /the base link is missing join/.test(p)));
  assert.ok(one({ kind: "base", params: { join: "surface", size: 3 } }).some(p => /unknown base param\(s\) size/.test(p)));
});

test("bad param values are refused with what is allowed", () => {
  const L = m => one({ kind: "layer", params: { mask: m, blend: 0.5, falloff: 0.1 } }).join("\n");
  assert.equal(L([{ by: "slope", min: 35 }]), "");
  assert.match(L([]), /mask must list one or more terms/);
  assert.match(L([{ by: "light", max: 0.2 }]), /by must be slope, height, curvature, noise/);
  assert.match(L([{ by: "slope" }]), /a slope term needs a min or a max threshold/);
  assert.match(L([{ by: "slope", min: 120 }]), /min must be a number from 0 to 90/);
  assert.match(L([{ by: "height", min: 0.8, max: 0.2 }]), /min must be below max/);
  assert.match(L([{ by: "slope", max: 30, scale: 2 }]), /scale .* is for noise terms only/);
  assert.match(one({ kind: "layer", params: { mask: [{ by: "slope", max: 30 }], blend: 0, falloff: 2, condition: "lava" } }).join("\n"), /blend must be above 0[\s\S]*falloff must be from 0 to 1[\s\S]*condition must be one of/);
  const S = p => one({ kind: "scatter", params: { density: 0.4, scale: [0.6, 1.4], lod: 25, impostor: 0, ...p } }).join("\n");
  assert.equal(S({}), "");
  assert.match(S({ density: 0 }), /density .* must be above 0/);
  assert.match(S({ scale: [2, 1] }), /scale must be \[min, max\]/);
  assert.match(S({ impostor: 10 }), /impostor .* must be 0 \(none\) or at least lod/);
  assert.match(S({ slope: [40, 10] }), /slope must be \[min, max\] degrees/);
  assert.match(S({ avoid_water: "yes" }), /avoid_water must be true or false/);
  assert.match(one({ kind: "base", params: { join: "glue" } }).join("\n"), /join must be merge, part, surface/);
  const mat = checkBlocks(graph({ ...ok, "trees-oak-trees": [{ block: "material:bark.oak", why: "w", ...SC }] }), LIB, MATS).problems.join("\n");
  assert.match(mat, /a material can't be scattered/);
  const mat2 = checkBlocks(graph({ ...ok, "trees-oak-trees": [{ block: "material:bark.oak", why: "w", kind: "base", params: { join: "merge" } }] }), LIB, MATS).problems.join("\n");
  assert.match(mat2, /a material has no geometry: use join surface/);
});

test("a cycle of typed links is still refused", () => {
  const g = checkBlocks(graph({ ...ok, "rocks-pebbles": [{ block: "rocks-medium-rocks", why: "loop", ...SURF }] }), LIB, MATS);
  assert.ok(g.problems.some(p => /^cycle: .*Medium rocks.*Pebbles.*Medium rocks/.test(p)), g.problems.join("\n"));
});

test("an older list without kinds still loads: default plus migration", () => {
  const old = { version: 1, landscapes: { "terrain-mountains": { evidence: "picture", source: "t" } }, planned: {}, needs: {
    "terrain-mountains": [{ block: "rocks-medium-rocks", why: "boulders" }, { block: "trees-oak-trees", why: "trees" }],
    "rocks-medium-rocks": [{ block: "rocks-pebbles", why: "rock chain" }],
    "trees-oak-trees": [{ block: "material:bark.oak", why: "bark" }],
  } };
  assert.ok(checkBlocks(old, LIB, MATS).problems.length > 0, "an untyped list is not accepted as it is");
  const m = migrateBlocks(old);
  assert.equal(m.migrated, 4);
  assert.equal(m.data.version, 2);
  assert.equal(old.needs["rocks-medium-rocks"][0].kind, undefined, "the original is left alone");
  const g = checkBlocks(m.data, LIB, MATS);
  assert.deepEqual(g.problems, []);
  assert.equal(g.needs["terrain-mountains"][0].kind, "scatter");
  assert.equal(g.needs["rocks-medium-rocks"][0].params.join, "surface", "rock chain by size: the stone is proven on pebbles");
  assert.equal(migrateBlocks(m.data).migrated, 0, "typed links are kept");
  assert.deepEqual(JSON.parse(formatBlocks(m.data)), m.data, "the writer round-trips");
});

test("the defaults Chris confirmed: snow high and flat, moss low and concave, small objects sparse", () => {
  assert.deepEqual(defaultEdge("terrain-mountains", "material:ground.snow.fresh", "material"), { kind: "layer", params: SNOW });
  assert.ok(SNOW.mask.some(t => t.by === "height" && t.min >= 0.5) && SNOW.mask.some(t => t.by === "slope" && t.max <= 40) && SNOW.condition === "snow");
  assert.deepEqual(defaultEdge("planned:alpine-meadow", "vegetation-moss-patches", "asset"), { kind: "layer", params: MOSS });
  assert.ok(MOSS.mask.some(t => t.by === "height" && t.max <= 0.5) && MOSS.mask.some(t => t.by === "curvature" && t.max < 0) && MOSS.condition === "growth");
  const peb = defaultEdge("water-shorelines", "rocks-pebbles", "asset");
  assert.equal(peb.kind, "scatter");
  assert.ok(peb.params.density <= 0.5, "sparse");
  assert.deepEqual(defaultEdge("terrain-mountains", "sky-dynamic-sky-dome", "asset"), { kind: "base", params: { join: "part" } });
});

test("each link's label says how the child is used", () => {
  assert.equal(edgeLabel({ kind: "layer", params: { mask: [{ by: "slope", min: 35 }], blend: 1, falloff: 0 } }), "blend: slope > 35°");
  assert.equal(edgeLabel({ kind: "layer", params: SNOW }), "blend: height > 0.6, slope < 35°");
  assert.equal(edgeLabel({ kind: "scatter", params: { density: 0.4 } }), "scatter: 0.4/m²");
  assert.equal(edgeLabel({ kind: "base", params: { join: "merge" } }), "base");
  assert.equal(edgeLabel({ kind: "base", params: { join: "surface" } }), "base: surface");
});

test("the stored catalogue is typed and the engine table carries every link with its kind", () => {
  const stored = rd("catalog/procedural/blocks.json");
  assert.equal(stored.version, 2);
  for (const l of Object.values(stored.needs)) for (const x of l) assert.ok(["layer", "scatter", "base"].includes(x.kind) && x.params, JSON.stringify(x));
  const g = checkBlocks(stored, rd("catalog/procedural/library.json"), rd("catalog/materials/materials.json"));
  const rs = rustTable(g.needs), links = Object.values(g.needs).reduce((a, l) => a + l.length, 0);
  assert.equal((rs.match(/Edge \{ block:/g) || []).length, links);
  assert.equal((rs.match(/Node \{/g) || []).length, Object.keys(g.needs).length);
  assert.match(rs, /block: "material:ground\.snow\.fresh", kind: Kind::Layer\(Layer \{ mask: &\[Term \{ by: Mask::Height, min: Some\(0\.6\)/);
});
