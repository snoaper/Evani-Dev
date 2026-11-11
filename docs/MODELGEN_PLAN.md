# Procedural modelling in Evani.Model: assessment and plan

Status: placed in the Evani.Model track by Chris (2026-10-25). **MG1 (construct core) is built** on branch `modelgen-core` (see "MG1 as built" at the end), awaiting Chris's review; MG2 to MG10 are not built. Plan written on branch `modelgen-plan` from m1-foundation b56f3fae.

Request (from the Broadside3D project, passing on Chris's wish): "We need to be able to make models with Evani.model." Broadside's models are made today by about 4,300 lines of Node JS generators in `..\Broadside3D\art\` (lib/geo.mjs, gltf.mjs, trim.mjs, pack.mjs, ao.mjs; models/ship.mjs, world.mjs, people.mjs, props.mjs, fort.mjs, dock.mjs, sky.mjs). They want all of it made with Evani.model, in Evani.dev's Rust, instead. Their generators were read as reference only (ideas, never code to copy, never run).

## 0. In plain words

- **Evani.Model exists and works**, but it is a different kind of modeller from what Broadside needs. It is a "describe it and let the machine search" modeller: shapes are soft volumes (signed distance fields) that are melted together and then turned into triangles. That is good for karts, props, rocks and characters. It is poor at what Broadside does: hundreds of separate, crisp pieces (planks, stones, boards, ropes) laid along curved surfaces.
- **The gap is a second engine room inside Evani.Model:** a "construct" back end that builds exact pieces (strips, boards, blocks, ropes, cloth) along curves and surfaces, with seeded variation, and writes them out through the same files, tiers, checks and download path as today.
- **It runs when a game is built, not in the player's browser** (first). The player downloads finished model files as today. The same code is written so it could later run inside the game to save download size, which may cut Broadside's model download by most of its 1.8 MB; that is a later, measured decision (section 3.3).
- **Stage:** it is not Stage 3 work. It fits the Evani.Model track (dev-only tools, never touching the engine core), as an extension of its brief, the way the code-built characters were. The Parking lot already holds this idea ("Recipe modeler (evani-shape)"), with the trigger "if kits keep rebuilding their own placeholder generators": Broadside's 4,300 lines are exactly that. Chris decides the placement (section 4).
- **Ten packages**, one worker session each: a core, then one per family (hull and planks, rigging and sails, rocks, fort, pier and hut, props, fracture and damage, crew, live tuning).

## 1. What Evani.Model is today

### 1.1 Where it lives (all on m1-foundation b56f3fae)

| What | Where | State |
|---|---|---|
| The modeller library | `crates/evani-model/` (about 12,700 lines of Rust in `src/`: intent, build, generator, sdf, solve, mesh, compile, surface, gltf, meshcheck, must, collide, rig, pose, preview, compare, png, sha256, dmath, rng, expr, vec) | M1 compiler signed off by Chris 2026-10-16; MC1 (code-built Frontier Marshal) signed off 2026-10-16; M2 (search) open |
| Its data | `crates/evani-model/data/`: 25 class templates (`classes/*.json`: kart, crate, barrel, rock, tree, house, tower, fence, character, ...), one style (`styles/toon_chunky.json`), `vocab.json`, bodies, looks | Data-driven templates with formulas (`expr.rs`) |
| Command line and MCP server | `tools/evani-model/` (`make`, `compile`, `check`, `sheet`, `compare`, `describe-system`, `mcp`) | Seven of eleven planned `model_*` tools built |
| Build-time add-on tool | `tools/evani-model-fur-locks/` (grows explicit fur-lock geometry into a compiled `.glb`) | Built; a precedent for explicit geometry made outside the field path |
| Design docs | `docs/EVANI_MODEL.md`, `EVANI_MODEL_BRIEF.md` (binding brief), `MODEL_API.md` (.evmodel schema and tools), `EVANI_MODEL_TAB.md`, `docs/model_research/` (21 notes, plans, reports) | |
| The Evani.Model tab | Engine side: `apps/model-view` and the bridge's model mode (T1a, merge b66b7e5). Evani.App side: **not built** (milestone A1). Evani.App today has only a general model viewer (`src/renderer/src/viewer/ModelView.tsx`, three.js), not an Evani.Model tab | Partly |
| An `Evani.Model` folder in the projects folder | none; Evani.Model lives inside the Evani.Dev repo | |

### 1.2 How it works

Intent text (`.evmodel`) picks a class template and a style; the template's parts each come from one of 16 **generators** (`generator.rs`: rounded_box, panel, shell, cylinder, tube/sweep, ellipsoid, wheel, lathe, loft, frame, planks, bricks, noise_rock, branching_tree, blended_body; `external` off). Each generator returns a **field** (never a mesh), anchors, a collision proxy and its thinnest dimension. Relationships are solved (Levenberg-Marquardt), fields are unioned, and each render tier is **re-extracted from the fields** (`compile.rs`, `mesh.rs`: sparse dual-contouring grid, cell size from the tier's triangle budget; thin parts kept at least 1.6 cells, parts too small for a tier dropped). Output is one `.glb` per tier (`gltf.rs`: KHR_mesh_quantization, part tree, `anchor:` nodes, sockets, collision proxies, `extras.evani` format 1), with normal maps from the exact field gradient, MikkTSpace tangents, axis-projected UV charts, region vertex colours and, for characters, a skeleton and skin weights. Everything is deterministic (`dmath` own transcendental functions, `rng::Pcg32`, fixed orders, SHA-256 of the output).

### 1.3 The engine side it feeds

- `crates/evani-gltf` (reader), the `models` module in evani-render (handles, tiers, streaming, `stream.json`, level groups), variants (hide or recolour named nodes, `anchor_frame`: docs/CONTENT.md), docs/MODEL_FORMAT.md (the contract).
- `cargo xtask convert` (tools/evani-convert: gate renders, meshopt, KTX2; Evani.Model files are compressed and gated but never re-simplified), `cargo xtask model-import`, `cargo xtask manifest`, the Blender add-on (`tools/blender-addon/`).
- `evani-render::spline` (runtime Catmull-Rom path and profile sweep for roads and walls, feature `meshes`).
- `crates/evani-destruct` (runtime destruction after Hexagon: hole spheres in concrete, 1 m cover cells, support and tipping, debris pools). It does **not** use pre-broken meshes.
- Materials: `evani-material-proc` (procedural box surfaces, relief), `evani-material-layers`, `-nature`, `-character`; glTF material tags `extras.evani.relief`, `extras.evani_kind`. The ~1,000-material catalogue on 29 recipes, with a 32-byte `extras.evani_recipe` block per glTF material written by `cargo xtask convert --materials`, is on branch `matcat-engine` (d61fa139), **not merged**.

## 2. Can it do Broadside's level today?

Broadside's models, for scale (the files the game ships, `evani-game/app/assets/models/`): ship-carrack 390 KB, ship-galleon 331 KB, ship-caravel 239 KB, fort 194 KB, dock 130 KB, rocks 125 KB, hut 89 KB, crew 60 KB, sky 51 KB, captain 39 KB, mast-square 31 KB, chest, crate, mast-lateen, debris, palms, tree, bush 5 to 19 KB each; about 1.78 MB without the island, 2.36 MB with it (585 KB), plus one palette PNG and one plank trim-sheet JPEG.

| Capability | Today | Evidence |
|---|---|---|
| Lofted hull lines | **No** (only rough stand-ins) | `loft` is a rectangle-to-rectangle frustum (`generator.rs` 291-332); `tube`/`sweep` is a round tube along points with two radii; `lathe` is a revolve; `blended_body` smooth-unions capsules. Nothing defines a surface from stations and waterlines or from a half-breadth function such as Broadside's `hullFns` (half breadth `hz(t, y)` from length station `t` and height `y`, with sheer, tumblehome, rake and counter). |
| Laying planks along curves | **No** | `planks` lays `count` straight, flat boards side by side along x with a little seeded length and tilt jitter (`generator.rs` 359-384); `bricks` lays a flat running-bond wall. No strakes between two curves on a surface, no staggered butts, no lapped (proud) strakes, no per-plank UV row. Even if built as fields, a plank seam narrower than a tier's cell (the hull's cell is centimetres) would melt shut in the dual-contouring mesher. The relief pattern `planks(straight|staggered)` is specified in MODEL_API 1.5 but relief (M4) is not built. |
| Instanced parts | **Partly** | Loose solids (wheels) are meshed once and reused by several nodes, mirrored where paired (`compile.rs` 9-10, 155-161); part specs take `count` and `spread` along a line and `mirror: x` (`build.rs` 933-942). `array_along` is reserved as a word (`build.rs` 599) but places nothing along a curve. No scatter on a surface. The engine reader refuses EXT_mesh_gpu_instancing (convert `merge.rs` 87-93), so instancing means node reuse. |
| Trim sheets, or our procedural material recipes | **Partly** | Evani.Model writes axis-projected charts per part, exact normal maps and region vertex colours (`surface.rs`, `gltf.rs` 139). It has no trim-sheet UV layout (Broadside's `trim.mjs`: 16 rows of grained planks, each plank picks a row) and does not tag materials with catalogue recipes; the recipe block exists only on the unmerged `matcat-engine` branch. |
| LODs per tier | **Yes** | Every tier is re-extracted from the fields at its own budget, small parts dropped with a reason (`compile.rs` 1-12; MODEL_FORMAT `dropped`, `cell_m`). Within-tier distance LODs come from the converter for imported models only (MODEL_FORMAT `lods`). |
| Compact output | **Partly** | The format is compact (16-bit positions, 8/16-bit normals, quantised UVs; meshopt from the converter). The geometry is not: a field mesher spends triangles evenly on flat faces and still rounds crisp edges, so a model of hundreds of hard-edged pieces costs far more triangles than Broadside's exact quads at the same look. |
| Break-apart pieces for destruction | **No** (generation) / **Partly** (runtime) | Evani.Model has no fracture, damage stages or break planes. At runtime the engine can hide and show named nodes (CONTENT.md), which is how Broadside's damage stages (`base`, `dmg0..2`, `bulw_1..6x`, `chunk_1..4`, `frac_b/_s`) are drawn; evani-destruct works on cube cells and hole spheres, not model pieces. |

Also not there: ropes, rigging and other thin explicit geometry (EVANI_MODEL.md "Limits": "very thin features use explicit meshes alongside the fields", not built); cloth (sails with billow, seams, reef bands, tears); faceted, flat-shaded rock with strata and cleave planes (`noise_rock` is smooth blobs plus noise); stone-by-stone masonry along arcs; per-vertex baked AO. Already there and reusable: characters (skinned, region colours) for the crew, with poses limited to finger curl (`pose.rs`).

**Conclusion.** Evani.Model's field back end is the right tool for organic and chunky forms and for search; Broadside's art is constructive: exact pieces placed by rules on curves and surfaces. That needs a second back end, not more field generators.

## 3. Design: Evani.Model Construct

### 3.1 Shape of it

A new dev-only crate **`crates/evani-construct`** (the "construct" back end of Evani.Model; name picked here, the Parking lot's `evani-shape` would do as well):

- **Std only, no dependencies, WebAssembly-clean.** It holds the geometry kernel and the recipe interpreter. To share one deterministic maths base, `vec`, `dmath`, `rng` (Pcg32) and `expr` move down from evani-model into it and evani-model re-exports them; the move is proven harmless by evani-model's 30 benchmark fingerprints and its determinism tests staying identical.
- **evani-model depends on it**, not the other way: evani-model supplies the `.glb` writer (extended for explicit meshes, per-vertex colour and a second UV set), `meshcheck`, `preview` sheets, `compare`, the MCP tools and the workspace. The brief's rule holds: neither crate depends on, or changes, evani-core or evani-render.
- **Mixed models.** A class template may take a part from either back end: a field part (`kind: lathe`) or a construct part (`construct: hull_planks`). Construct pieces are not melted into fields; they are written as their own meshes beside the field solids, which also fills Evani.Model's "explicit meshes for thin features" gap (ropes, wires).

### 3.2 Determinism and seeds

- Same recipe, same parameters, same seed, same tool version: the same bytes on every machine (the Evani.Model promise). `cargo xtask model <id> --check` builds twice and compares hashes.
- **Seeds per piece, not per run.** Every piece has a stable path id (`hull/strake_3/plank_7`); its Pcg32 stream is `Pcg32::new(model_seed, hash(path))`. Changing the number of planks in one strake therefore never reshuffles the wood of every other plank, the stones of the fort or the facets of a rock. (Broadside draws from one shared stream, so any change ripples through the whole model.)
- Noise (`noise3`, `fbm3`) is our own, on `dmath`, so a value is the same native and in wasm.

### 3.3 When it runs: build time first, runtime as a later option

| | Build time (`cargo xtask model`) | Runtime (in the game) |
|---|---|---|
| Player download | the finished `.glb` per tier, as today (Broadside about 1.8 MB) | the construct code (estimate 25 to 50 KB gz wasm) plus recipes (estimate 10 KB gz for all of Broadside) |
| Load on a phone | nothing extra | generation time on the CPU (estimate tens to a few hundred ms per ship, unmeasured), memory for the build |
| Checks | geocheck, the converter's quality gate, meshcheck, look guard and review sheets all see the exact file | the same, if and only if the runtime output is bit-identical to the build-time file (determinism makes that testable) |
| Flexibility | parameters change at build time and in the live loop (3.8) | per-instance variation in the game (every ship of a class slightly different) for free |

**Recommendation: build time now; keep runtime possible.** Build time is needed anyway for checks, the gate and review, and it adds nothing to any game. Because the crate is std-only, deterministic and wasm-clean, a later opt-in runtime module (`modelgen = true` in `[modules]`, optional dependency behind a same-named feature, zero bytes to games without it) can run the same recipes in the game, with a test that its output equals the build-time file byte for byte. Weighed by Chris's lightness rule this could be the bigger win (most of Broadside's model bytes), so it is worth a measured spike later; it belongs to **Stage 7 (Lean delivery)**, which is about per-device bytes. Not proposed for now.

### 3.4 The authoring API

Two ways in, one model:

**Recipes as data** (most models): JSON in the kit, `kits/evani-kit-<id>/models/<model>.construct.json`, with formulas through `expr` (the same formula language as the class templates). Parameters are declared with ranges, units and whether they are live.

```json
{
  "construct": 1,
  "id": "ship-caravel",
  "seed": 7,
  "params": {
    "length":  { "value": 27.9, "min": 20, "max": 50, "unit": "m", "live": true },
    "beam":    { "value": 8.55, "min": 6,  "max": 16, "unit": "m", "live": true },
    "sheer_bow": { "value": 1.8, "min": 0, "max": 4, "unit": "m", "live": true }
  },
  "surfaces": {
    "hull": { "kind": "lines", "stations": 21, "waterlines": 9,
              "half_breadth": "beam/2 * fullness(t) * section(t, y)",
              "keel": "draft * keel_rise(t)", "sheer": "freeboard + sheer_bow * max(t,0)^2 / 0.25" }
  },
  "lines": { "boot_top": "0.5", "wale_low": "deck(t) - 2.9" },
  "parts": [
    { "name": "hull_planks", "node": "hull", "op": "strakes", "on": "hull",
      "from": "keel", "to": "boot_top", "count": 7,
      "butts": { "length": 11.7, "stagger": [0, 0.5, 0.25, 0.75, 0.125, 0.625] },
      "piece": { "kind": "plank", "thickness": 0.06, "gap": 0.004, "proud": { "every": 2, "by": 0.02 } },
      "material": "wood.oak_weathered", "vary": { "tone": 0.08, "grain_seed": true },
      "detail": { "low": "strip", "medium": "planks", "high": "planks+proud", "ultra": "planks+proud+treenails" } },
    { "name": "wale", "node": "hull", "op": "timber", "along": "line:wale_low", "profile": "half_round(0.5, 0.55)",
      "joints": 4, "material": "wood.oak_dark" },
    { "name": "guns", "node": "guns", "op": "along", "curve": "line:gun_deck", "count": "guns", "piece": "ref:cannon",
      "anchor": "gun_{i}" }
  ],
  "stages": { "intact": {}, "damaged": { "knock_out": { "parts": ["bulwark_*"], "share": 0.25 } } },
  "anchors": { "mast_main": { "at": "surface:deck(0.02, 0)", "up": [0, 1, 0] } }
}
```

**Recipes in Rust** (the escape hatch for anything the node set can't say): a kit may add its own node kinds by implementing `construct::Node` in an example binary, `kits/evani-kit-<id>/examples/construct.rs` (the same pattern as the 2D kits' `examples/geocheck_dump.rs`); `cargo xtask model` runs it when present, else interprets the JSON with the built-in nodes only.

```rust
use evani_construct::prelude::*;

/// A kit-defined node: a figurehead built from built-in pieces.
struct Figurehead;
impl Node for Figurehead {
    fn build(&self, p: &Params, cx: &mut Ctx) -> Result<Pieces, ConstructError> {
        let head = cx.lathe("head", &profile![(0.0, 0.0), (0.4, 0.2), (0.3, 0.6)], 12)?;
        let mane = cx.along(&Curve::arc(p.v3("centre")?, 0.5, 0.0, 3.1), 9, |i, f| cx.spike(f, 0.25 * cx.vary(i, 0.2)))?;
        Ok(Pieces::of([head, mane]).material("metal.gilt"))
    }
}
```

### 3.5 Primitives

| Group | Primitives |
|---|---|
| Curves | polyline, centripetal Catmull-Rom, cubic Bézier, arc, helix, catenary (rope sag between two points), curve from formula; arc-length resampling; frames by parallel transport with an optional up direction; offset along a surface normal |
| Surfaces (u, v to point, normal, tangent) | **lines** surface (stations by waterlines, from a half-breadth formula or a table of offsets, smoothed by a bicubic patch: hull lines); loft through section curves along a spine with scale and twist per station; sweep (profile along a curve); lathe; ruled surface between two curves; formula patch |
| Regions on a surface | band between two lines `y = a(t)` and `y = b(t)` over `t0..t1`; split a band into strakes or courses; mirror |
| Layout | `strakes` (n strips, each cut into pieces at staggered butts), `courses` (masonry bonds along a line or arc: running, Flemish, random ashlar), `along` (count or spacing along a curve, with seeded jitter and per-index anchors), `grid`, `scatter` (Poisson-disc on a surface or region, with a mask formula and a minimum spacing), `rings` (ring-by-ring columns) |
| Pieces | `plank` and `board` (a strip given thickness, bevel, proud offset, end faces, seam gap), `timber` (a profile along a curve in jointed lengths), `stone_block` (chamfered, set proud of a mortar bed), `stave` (barrels), `spar` (tapered round), `rope` and `ribbon` (thin explicit geometry, kept at least one pixel wide per tier), `cloth` (a grid panel with billow, seams and reef band formulas, optional tears), `facet_mass` (icosphere or ring column, swollen by noise, cleaved by planes into flat facets, strata steps, flat-shaded), `tile` (roof tiles in courses), any field part from Evani.Model's generators |
| Variation | per-piece tone, length, tilt and offset jitter on the piece's own seed stream; noise and fbm for surfaces; wear masks from formulas (up-facing, near water line, edges) written to vertex colour |
| Assembly | transforms, mirror (winding flipped), references to other recipes (`ref:cannon`) written once and reused by node, groups to named glTF nodes, sockets and anchors (`anchor:` nodes with rotation), stages (each piece tagged with the stages it appears in), split by a plane |

**Boolean-free.** Nothing is ever cut by a solid boolean. Openings (gun ports, embrasures, doors) are made by layout: strakes and courses skip or end at an opening's outline and a frame piece is placed round it; a "hole" is a dark inner piece behind a frame. The only cut is a **plane split** (3.7), which is robust on our own pieces.

### 3.6 Outputs

- **Meshes**: one `.glb` per tier through evani-model's writer (format 1, KHR_mesh_quantization), node names from the recipe (`hull_b`, `bulw_3x`, `chunk_2`), so games keep hiding and showing nodes as now.
- **Instances**: identical pieces (cannons, barrels, bollards, treenails) written once and referenced by many nodes; above a per-tier count (a parameter), small instances are merged into one mesh per material to save draw calls.
- **Anchors and sockets**: `anchor:` nodes with rotations (MODEL_FORMAT "Sockets") for mast steps, gun ports, crew stations, the break plane.
- **Gameplay data**: models are render-only (AGENTS.md), so `cargo xtask model` also writes the recipe's anchors, stations and break planes as kit data (`kits/evani-kit-<id>/data/models/<model>.json`) from the same recipe in the same run; the two can't drift (Broadside's `ships.json` and the generated tables in `damage.rs` are this idea).
- **Fracture pieces** (3.7).
- **Report**: `<model>.evmodel.json` with the recipe hash, seed, tool version, triangles, draw calls and bytes per tier, checks.

### 3.7 Fracture and damage

1. **Stages.** Each piece carries the stages it is drawn in (`base`, `dmg0`, `dmg1`, `dmg2`); pieces every stage keeps go into `base` once (Broadside's fort, dock and hut do this by hand). Stage rules are seeded selectors: knock out a share of the pieces in a region, swap a piece for its broken twin, add scorch and splinters along the edge of what was removed.
2. **Broken twins.** A plank or board snapped at a seeded point gets a jagged end cap and splinters; a stone loses a chunk as a smaller faceted block; a sail gets ragged holes. Each kind of piece knows its own broken form, so damage is generated, not modelled per ship.
3. **Plane split.** A model splits at a plane (the hull's break plane): whole pieces go to the side their centre lies on; only pieces that cross the plane are cut, and each cut gets its cap (a polygon on the plane, filled by ear clipping) and optional splinter ends. Output: two named halves (`hull_b`, `hull_s`) and the break ends (`frac_b`, `frac_s`).
4. **Chunks.** A `facet_mass` breaks into a core and n chunks by seeded cutting planes (convex pieces cut with caps; the scar on the core uses the fresh-stone colour), giving Broadside's `core` and `chunk_1..4`.
5. **Support graph.** `extras.evani.fracture` lists each chunk or piece group with mass, centre and which pieces it rests on. evani-destruct does not read this today; letting it collapse pre-broken model pieces is a request for CORE_REQUESTS.md in destruct's own stage (Stage 5), not part of this plan.

### 3.8 Tiers, LODs and the download path

- **Per tier, by recipe, never simplified** (Evani.Model's own rule): each part names what it becomes per tier (`detail` in 3.4): at Low a strake band is one strip and the seams are left to the material; at Medium it is planks; at High proud steps are added; at Ultra treenails and nails. Ropes keep a minimum on-screen width. Each tier is checked against its triangle budget (`tri_budget`) and the tier limits in `assets/tiers/*.json`.
- Then the existing path, unchanged: `cargo xtask model <id>` writes `assets/models/<model>/<model>.<tier>.glb` and runs `cargo xtask convert` on them in Evani.Model mode (compressed and gated, never re-simplified; construct files carry Evani.Model's `extras.evani.tool`), then `cargo xtask manifest <id>` (stream.json, boot group). `cargo xtask check` flags a construct output whose recipe hash is older than its recipe, the same way it flags a stale stream.json.
- Within-tier distance LODs: a part may also give a cheaper form for far away; the writer emits `lods` levels (MODEL_FORMAT) for construct files, since they can be made exactly rather than by simplification.

### 3.9 Materials: recipes, not textures

- Every piece names a catalogue material (`wood.oak_weathered`, `stone.limestone_warm`, `cloth.canvas`, `metal.iron_cast`, `rope.hemp`). The writer emits one glTF material per catalogue material with its `extras.evani_recipe` block once `matcat-engine` is merged; until then, the material's name, base colour factors and the existing tags (`evani_kind`, `extras.evani.relief`).
- **Replacing the trim sheet.** Each piece gets UVs in real units along its grain (u along the plank, v across) and a per-piece variation value. The look varies grain per plank from that value, so no trim-sheet image is needed. Today the engine's recipe stage cannot read a per-vertex variation value; that is filed as a request for whichever stage owns the recipe shader (CORE_REQUESTS.md), and until it exists the variation is baked into the vertex colour as a tone (what Broadside does with its palette ramps).
- **AO** is baked per vertex at build time (hemisphere rays against the model's own pieces, seeded and deterministic) into the vertex colour's alpha, so Lite and Low get contact shading without GTAO.

### 3.10 Live tuning

- Recipe parameters marked `live` are registered as `kit.<id>.model.<model>.<name>` params in the dev build only (they never ship: the shipped file is the result).
- `cargo xtask model <id> --watch` rebuilds a model when its recipe or a live param changes and hot-swaps it into the running game through the model bridge and drop-to-convert's hot-load path (S3-14), or into the Evani.Model tab's page. Target: a ship hull rebuilds in under a second natively, so `sweep_run` over a hull parameter is practical.
- `bake_export` of such a param writes the value back into the recipe JSON (dry run first), as with any kit param.

### 3.11 Geometry checks

Construct knows every piece, so it checks more than a game-level scan can (each check reusable by every game, per the "errors become tools" rule):

1. **Per piece**: meshcheck (degenerate and duplicate triangles, flipped normals, open edges where the piece should be closed).
2. **Between pieces**: overlap volume of pieces that should only touch; coplanar faces of different pieces (z-fighting: an emblem laid on a sail, a deck board over a stringer) unless one is lifted by the tier's minimum offset; intended gaps (seams) declared by the layout so they are not reported as cracks.
3. **Support**: every piece touches its parent or its layout's neighbours (nothing floats), per stage, so a knocked-out board never leaves a floating nail.
4. **Fracture**: every cut piece capped and closed; halves of a split together equal the whole.
5. The written files are then registered in the game's geocheck as custom meshes (GEOCHECK.md "Custom meshes"), as any model is. `cargo xtask model --check` runs 1 to 4; `cargo xtask check` runs them on stale outputs only.

## 4. Stage placement

- The current stage is **3 (Real content)**. Its gate is about converting a detailed Blender kart and an AI-generated character, Low downloading only its LODs, drop-to-convert, and games compiling without these features. Construct does not help pass it and is not in its "Includes", so it is **not Stage 3 work** under rule 1.
- It does fit the **Evani.Model track** (placed by Chris between Stages 2 and 3, still running): dev-only, never changes evani-core or evani-render, integrates through glTF, the converter and geocheck. It extends the brief (which says Evani.Model makes "shape and surface relief" by search) the way the code-built characters (MC1 to MC4) did, and that extension needs Chris's approval.
- The **Parking lot** already has it: "Recipe modeler (evani-shape). Revisit after Stage 5 if kits keep rebuilding their own placeholder generators." Broadside rebuilding 4,300 lines of generators is that trigger, two stages early.
- The runtime option (3.3) belongs to **Stage 7**. Stage 8's "prompt to playable asset" and "look transfer" can use construct later; nothing here waits for them.
- **Minimal version, if Chris keeps rule 1 strict:** no build into main. Allowed now: this plan and a throwaway spike on a scratch branch (rule 1), for example MG1's lines surface and MG2's strakes on the caravel hull, to measure triangles, bytes and build time against Broadside's 239 KB caravel.
- **If Chris places it in the Evani.Model track now:** MG1 and MG2 are the smallest useful slice (a planked caravel hull through the full download path, zero bytes in any game), and each later package adds a family.
- Rule 9 (no game polish until Stage 8): the packages below are engine tooling. Each proves itself on one Broadside model family as a fixture; moving all of Broadside's art onto construct is game work on Broadside's side.

## 5. Packages

One worker session each. "Proof" is the fixture each package must reach against Broadside's current file at the same tier (bytes after the converter, triangles, a side-by-side review sheet from `preview`). Every package: `cargo xtask check` clean and `--full` before hand-over; byte proof that all shipped games are unchanged (construct is dev-only, so this must always be +0 bytes); determinism test (two builds, same SHA-256; 1 and 8 threads equal); runs at below-normal priority with `CARGO_TARGET_DIR` on E:.

| # | Package | What | Depends on | Size, worker | Budget |
|---|---|---|---|---|---|
| MG1 | Construct core | crate `evani-construct`; `vec`/`dmath`/`rng`/`expr` moved down (evani-model's 30 fingerprints unchanged); curves, frames, surfaces (lines, loft, sweep, lathe, ruled, formula), regions and bands, piece ids and seed streams, mesh with named groups and stages; evani-model writer for explicit meshes (COLOR_0, TEXCOORD_1); JSON recipe loader with params; `cargo xtask model <id> [--model] [--tier] [--check] [--watch later]`; stale-output check; checks 1 and 4 | none | L, opus-high | crate under 4,000 lines; a 100k-triangle model writes in under 1 s natively |
| MG2 | Hull and planks | lines surface for hulls (formula and offset table), `strakes`, staggered butts, `plank`, `board` (decks), `timber` (wales, rails), keel, stem, ribs and frames, treenails as instances, proud lapping, mirror; per-tier detail; checks 2 and 3 | MG1 | L, opus-high | caravel hull at High within 239 KB and Broadside's triangle count, review sheet side by side |
| MG3 | Rigging and sails | masts, yards, spars, tops; ropes and catenaries, shrouds with ratlines, stays, ribbons; minimum on-screen width per tier; `cloth` sails (billow, seams, reef band, bolt ropes), lateen and square, torn variants, flags and pennants; emblem laid on cloth without z-fighting | MG1 | M, opus-medium | mast-square within 31 KB, mast-lateen within 16 KB |
| MG4 | Rocks and stone masses | `facet_mass` (noise swell, cleave planes, strata steps, flat shading), ring-by-ring columns (ribs, ledges, peaks, wet foot), boulders, `scatter` of tufts on up-facing ledges; core and chunks (needs MG8's plane cut, or a simple convex cut here that MG8 generalises) | MG1 | M, opus-high | rocks within 125 KB |
| MG5 | Masonry: the fort | `courses` along lines and arcs (ashlar, batter, staggered joints), `stone_block` on a mortar bed, buttresses, string course, embrasures and crenellations by layout, round tower, roof `tile` courses, gate; stage tags with a shared `base` | MG1 (MG8 for damage stages) | M, opus-medium | fort within 194 KB |
| MG6 | Timber structures: pier and hut | piling bents, X bracing, caps, stringers, deck boards (MG2's `board`), rope rail (MG3), steps; stilt hut with thatch courses, porch, yard props | MG2, MG3 | M, sonnet-high | dock within 130 KB, hut within 89 KB |
| MG7 | Props kit | boxes clad in boards, barrels from staves, chest, ammo crate, cannon on carriage, capstan, wheel, lantern, coil, rowboat; each a reusable recipe written once and referenced (`ref:`) | MG2 | M, sonnet-high | props within today's chest, crate and debris sizes (19, 19, 15 KB) |
| MG8 | Fracture and damage | stages and seeded selectors, broken twins per piece kind (snapped plank, chipped stone, torn cloth), plane split with caps and splinter ends, convex chunks, `extras.evani.fracture` support graph; files the evani-destruct request (Stage 5) | MG2, MG4, MG5 | L, opus-high | the caravel's `bulw_1..6x`, `wreck`, `frac` nodes and the fort's `dmg0..2`, each within Broadside's bytes |
| MG9 | Crew and figures | through Evani.Model's character class (not construct): baked static poses from joint angles (the rig exists; `pose.rs` has only finger curl), clothing regions, tincture node; six working poses as one model | Evani.Model MC1 (done) | M, opus-medium | crew within 60 KB |
| MG10 | Live loop and materials | `--watch`, live `kit.<id>.model.*` params, hot swap into the game and the Evani.Model tab, `bake_export` back to the recipe; catalogue material names to `evani_recipe` blocks (needs `matcat-engine` merged); per-vertex AO bake | MG2; `matcat-engine` merge for the recipe blocks | M, opus-medium | a hull rebuild and hot swap under 1 s |

Order: MG1, then MG2; MG3, MG4, MG5 and MG9 can then run in parallel; MG6 and MG7 after MG2 and MG3; MG8 after MG2, MG4 and MG5; MG10 any time after MG2. Island ground, palms and vegetation (Broadside's `world.mjs` island parts) map onto MG4's masses and scatter plus Evani.Model's existing `tree` and `bush` classes; the sky ring is a look or effect matter, not a model, and stays with the game.

Out of scope for this plan: the runtime module (Stage 7), evani-destruct reading fracture graphs (Stage 5), the per-vertex variation input for recipe shaders (CORE_REQUESTS.md), the Evani.App side of the Evani.Model tab (A1).

### 5.1 Broadside's generators: reference input, not a contribution

- **The engine's primitives are written fresh in Rust, with Broadside's generators as reference input.** Chris's rule for model tools is "all must be tools specifically developed for our Evani.dev and Evani.model", built from scratch with outside projects as ideas only. Their JS is also a different shape from what the engine needs: one-off functions over a single shared random stream, a palette atlas and a trim sheet, versus a general recipe graph with per-piece seeds, catalogue materials and checks. A port would carry those choices in.
- **What Broadside should contribute is recipes, on our API, in its own kit.** Once MG1 and a family package land, Broadside rewrites that family's models as `*.construct.json` recipes (and small `examples/construct.rs` nodes where needed) in its kit, under the game's MIT licence. Its numbers (ship specs, castles, masts, palette, poses) carry straight over as recipe data. Each MG package's proof fixture is the first such recipe, so Broadside has a worked example per family.
- Broadside's game lives outside the Evani.Dev workspace (`..\Broadside3D\evani-game`), so `cargo xtask model` must accept a game folder outside the workspace. Branch `bs/xtask-app-dir` exists for that (at b56f3fae, no commits yet); MG1 should use whatever it lands, or add `--app-dir` itself.

## 6. Decision for Chris

Where does this work go?

1. **Into the Evani.Model track now, as an extension of its brief** (recommended): dev-only, touches no engine core, adds zero bytes to any game, and Broadside needs it now; the Parking lot's own trigger is met.
2. **Stay in the Parking lot until after Stage 5**, as it says today; Broadside keeps its JS generators until then. Only this plan and a throwaway spike are allowed meanwhile.
3. **Stage 8**, alongside "prompt to playable asset" and look transfer.

## 7. MG1 as built (2026-10-25)

- **Crate `crates/evani-construct`** (std plus serde_json; no new dependency): `vec`, `dmath`, `rng`, `expr` moved down from evani-model and re-exported there unchanged (proof: all 320 files evani-model makes for its 31 benchmark intents, every tier, glb and normal maps, are byte-identical built before and after the move). `formula` (recipe formulas compiled once), `curve` (polyline, centripetal Catmull-Rom, Bezier, arc, helix, catenary, formula curves; arc-length paths; frames with an up direction or by parallel transport), `surface` (lines, loft, ruled, sweep, lathe, formula; bands, strips, boards), `mesh` (closed exact solids: slabs and ring solids, hard edges), `seed` (per-piece streams), `model` (pieces, groups, prototypes and instances, anchors, stages, plane split), `check` (check 1 per piece: closed by welded position, outward, no degenerate or duplicate triangle, unit normals; check 4: the halves of a split equal the whole), `recipe` (the JSON recipe of 3.4: params, `let`, per-tier numbers, per-tier `detail` overrides and `tiers`, curves, surfaces, prototypes, operations `boards` (staggered butts, gap, proud), `sweep` (profiles, joints, taper), `lathe`, `block`, `along` (instances with anchors), `mirror`, anchors and sockets, stage rules (`knock_out` a seeded share), `split`).
- **Writer** `evani_model::construct_out` (format 1, KHR_mesh_quantization): group nodes by recipe node and stage set, one primitive per catalogue material, instances reuse one prototype mesh, `anchor:` nodes and `sockets`, COLOR_0 (piece tone), TEXCOORD_0 (grain in metres, normalised per primitive), TEXCOORD_1 (piece variation), glTF materials named after the catalogue material. The engine draws `baseColorFactor` only today, so per-piece tone shows in glTF viewers and review pictures but not yet in the engine (the per-vertex input is the CORE_REQUESTS item of 3.9). Report `<model>.evmodel.json`; tier picture `<model>.tiers.png`.
- **Commands:** `evani-model construct <recipe> [--tiers] [--out] [--set k=v] [--check] [--sheet]`; `cargo xtask model <app> [--model] [--tiers] [--check] [--no-convert]` (recipes in `kits/evani-kit-<app>/models/`, converted into `apps/<app>/assets/models/<model>/`, report beside the recipe, `stream.json` rewritten) and `cargo xtask model --recipe <file> [--out] [--convert]` (any folder, so a game outside the workspace can take the files). `cargo xtask check` errors on a recipe whose report is missing or carries another recipe hash (evani-guide `construct.rs`).
- **Demo:** `crates/evani-construct/data/demo/sea-chest.construct.json` (planked chest, arched lid of boards, iron straps, studs as instances, rope handles on catenaries, lock, a damaged stage). Tiers 628 / 928 / 1,880 / 2,416 triangles; after the converter 16.6 / 21.6 / 32.5 / 35.7 KB (5.6 / 7.8 / 12.0 / 13.7 KB gzip); Khronos validator 0 errors on every tier; the engine's reader opens every file. `cargo xtask geocheck --model` on the converted files: 0 issues on every tier (19 / 33 / 47 / 47 pieces; overlaps between every pair of pieces, model on the ground and every node attached, mesh validation per piece, z-fighting from 36 orbit cameras); a copy with a block sunk through the floor is caught (overlap 0.004 m3). Pictures of each tier as converted: `.evani/shots/modelgen-core/sea-chest-<tier>.png` and `sea-chest-tiers.png`.
- **Geocheck for one model** (`cargo xtask geocheck --model <folder|file>`, docs/GEOCHECK.md "One model"): added so a model outside any game can pass geocheck; each primitive is split into pieces by edges shared by exactly two triangles, so boards meeting in a butt joint stay two pieces. Construct's own checks 2 (overlaps, z-fighting between pieces) and 3 (support) stay MG2's; until then this is the overlap and attachment test for construct models.
- **Checks (2026-10-25):** `cargo xtask check` clean (8 warnings, none from MG1). `cargo xtask check --full` ran once with two errors that were the PC, not the code: evani-convert's `gate` test binary could not start (0xc0000142, STATUS_DLL_INIT_FAILED) and evani-net gave no failure text. Rerun on their own, evani-net passes (51 tests) and every evani-convert test binary passes (lib 38, anim_crosscheck 2, clips 10, convert 8, gate 8, look 11; two look tests timed out on a busy PC with two threads and pass with one). Byte proof: the 8 games' 16 wasm/js files are byte-identical to b56f3fae (EVANI_GIT_COMMIT=65e8497).
- **Tier gain (2026-10-25): see 7.1.**
- **Not in MG1** (by the plan): checks 2 and 3 (MG2), `--watch` and live params (MG10), the Rust `Node` escape hatch (first family that needs it), cutting pieces that cross a split plane (MG8), `--app-dir` (the recipe mode's `--out` covers games outside the workspace for now).

### 7.1 Tier gain: every costlier tier must show (2026-10-25)

Chris saw the sea chest's High (1,880 triangles) and Ultra (2,416) look the same. Ultra's 536 extra triangles were all detail too fine to see: studs at 14 sides instead of 10 (+192) and the rope handles at 10 sides by 18 lengths instead of 8 by 12 (+344). Measured at Ultra's close-up view, Ultra changed 243 of 735,825 model pixels (0.03%).

**The check (errors become tools).** Each tier has a design view: the picture size the model fills when seen as close as that tier is meant for, framed as the tier pictures are (box diagonal plus 10% across): Low 128 px, Medium 256, High 512, Ultra and VR 1024 (`construct_out::design_px`; on a 1440-pixel-tall screen with a 50 degree view, the chest is then about 17, 8.4, 4.2 and 2.1 m away). Each adjacent pair is drawn in one shared frame at the higher tier's design view, from the front three-quarter and the opposite back corner; a pixel changes when a colour channel moves by more than 24 of 255. If the higher tier costs more (triangles, or over 1 KB) and changes under **2%** of the model's pixels (`TIER_GAIN_MIN`), `cargo xtask model` prints a `WARN tier gain` naming the tier, its extra triangles and KB, the measured share, the parts that hold the extra triangles, and `fix: make the extra detail visible at <distance> m, or drop the <tier> tier so it falls back to <lower>`. The report (`<model>.evmodel.json`) carries `tier_gain` per pair. Threshold from the chest: the wasted Ultra changed 0.03%; its real detail steps change 8.1 to 8.3%; 2% is 60 times above the one and 4 times below the other. Tests: `tier_gain_warns_on_detail_that_does_not_show_and_passes_real_detail` (rounder studs only: warns, names `studs`; boards split plus a plate: passes; identical tiers: never warned). `geocheck --model` adds the same measure per pair as notes on the converted files, and `views/closeup.png`.

**Ultra rebuilt for close range** (same silhouette and look; Low, Medium and High geometry unchanged):
- bevelled plank edges on every board (`piece.bevel` 4 mm; each chamfer its own flat strip, so it catches the light; the back gains no triangles);
- laid rope on the handles (`rope` profile, 3 strands, `twist` 1800 degrees per metre, 6 by 24, replacing 8 by 12 round);
- iron corner caps at the eight body corners (a rectangle swept round the corner, mitred);
- 16 nail heads where the boards meet at the corners (`along` with `bake`: merged into the body, no draw call each);
- a hasp hanging from the lid over the lock, and two back hinges (leaf, pin, lid strap);
- the wasted detail removed: studs back to 10 sides at Ultra.

Also found and fixed on the way: every tier carried each prototype mesh whether placed or not (a tier-only prototype cost the lower tiers about 4.4 KB raw), and each lower tier listed every higher-tier part twice with reasons (2.4 KB converted): unplaced prototypes are no longer written and `dropped` is a list of names (the reasons stay in the report).

| tier | before: triangles, KB converted | after | tier gain (changed share at design view) |
|---|---|---|---|
| Low | 628, 16.2 | 628, 15.8 | |
| Medium | 928, 21.1 | 928, 21.1 | 8.1% (256 px) |
| High | 1,880, 31.7 | 1,880, 31.9 | 8.3% (512 px) |
| Ultra | 2,416, 34.9 | 3,004, 44.9 | 0.03% before, 8.3% after (1024 px) |

Draw calls unchanged (5 / 7 / 16 / 16). `geocheck --model` on the converted files: 0 issues on every tier. Nothing in a game changes: construct runs at build time and no kit has a recipe yet.
