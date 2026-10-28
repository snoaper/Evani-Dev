# 20 Integration: evani-convert, geocheck and the library

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 12, where it lives, test J)
Every model passes evani-convert (quality gate, compression, size report) and geocheck (zero untagged issues); approved models go to the Evani Library with provenance (intent and seed attached); for imported meshes, part splitting in evani-convert is designed by us, informed by PartField-style research; design for a later recipe-only delivery compiled on the player's GPU. Evani.Model never modifies evani-core or evani-render; engine needs go to CORE_REQUESTS.md. Test J: the kart family drops into the kart racer through evani-convert, the kit uses its anchors, bench passes on every tier, per-tier download sizes reported.

## Current state in the workspace (read 2026-10-15)
- **evani-convert does not exist yet.** It is Stage 3 work (STAGES.md: "Evani.dev model converter"). Evani.Model's track sits between Stage 2 and Stage 3, so M1 to M4 cannot use it. Flag: M5 depends on Stage 3's converter (decision D11).
- **geocheck exists** (docs/GEOCHECK.md, tools/evani-geocheck). Its mesh validation (`meshcheck::validate`: degenerate and duplicate triangles, open and non-manifold edges, flipped normals, self-intersections) is a library, but it cannot be linked into Evani.Model: tools/evani-geocheck depends on evani-core, evani-render and serde_json unconditionally (even with default features off), and `meshcheck` takes the renderer's own mesh type, so using it would pull the engine core into the modeler against the brief's dependency rule. Evani.Model therefore has its own small checker (note 13, decision D17); geocheck's checker is used only from the dev-only benchmark tool, as a second opinion on the same meshes. Scene checks (overlaps, floating, gaps, z-fighting, clipping, collision mismatch, camera) need a game scene and run once a model is placed in the kart racer.
- **The kart racer draws karts from primitives** in code (`draw_kart` in kits/evani-kit-kart-racer/src/draw.rs: bevelled cubes, cylinders and spheres), with colours per driver from data/karts.json. It has no model loading yet (Stage 3: glTF import).
- **docs/LOOP_API.md** (named in the brief's internal references) is on the `evani-loop` branch, not on m1-foundation; read from there.

## Known approaches
- **Part segmentation of arbitrary meshes.** PartField (arXiv 2504.11451) learns a continuous 3D feature field by distilling 2D and 3D part proposals with contrastive learning, then clusters it into a hierarchy of parts; one feed-forward pass, reported up to 20% more accurate and often orders of magnitude faster than earlier class-agnostic methods. Its code is under the NVIDIA Source Code License (non-commercial); we use the idea only.
- **Asset pipelines** usually treat generation and import separately; provenance is rarely kept.

## Our approach
- **Until evani-convert exists (M1 to M4).** Each compiled model is checked by: our own reader round-trip, the Khronos glTF-Validator (run separately), our own `meshcheck` on every tier's mesh (with geocheck's checker as a cross-check in the benchmark tool, which may link engine crates because nothing in evani-model does), and our must tests. Sizes per tier are reported raw and with a stand-in compression measure (gzip of the .glb, using the flate2 already in the workspace only if approved, otherwise reported later).
- **With evani-convert (M5).** Every model passes its quality gate and size report; Evani.Model's metadata (part tree, anchors, tiers) tells evani-convert what it may drop or merge, so it does not have to guess.
- **geocheck in the game.** The kart racer gets an adapter path for imported models (a kit change, Stage 3 or 4 work), so scene checks run with Evani.Model karts in place; zero untagged issues required.
- **Kit uses anchors.** The kart kit reads wheel hubs, seat, exhausts and the collision capsule from the model's metadata instead of constants (a kit change done when glTF import exists).
- **Library with provenance.** `model.save_to_library` writes the model, its intent, seed, level and schedule version, recipe, style profile version, prior hash, scorecards and contact sheet into the local library folder and updates the class prior (note 09). Uploading to the Evani library on evani.app is never done by the tool: it only prepares the item, and Evani.App's publish card (Chris's click) uploads it. Only model assets are published (Evani.App's rule), never the tool.
- **Part splitting for imported meshes (designed by us, M5).** For meshes that come from outside (Blender, plug-in generators), evani-convert gets a splitter of our own, informed by PartField's idea of a feature field clustered into parts, but without a learned network to start: we convert the mesh to a field (note 04), compute per-point descriptors from the field (thickness, curvature, distance to the medial axis estimated from the field) and split where thickness and curvature change sharply (concave creases), producing a part graph Evani.Model can then edit and relief. A learned field is a later option behind our own interface.
- **Recipe-only delivery (later).** A recipe is intent hash, seed, parameter vector and style version: a few hundred bytes. Because the CPU path is plain Rust and the GPU path is WGSL, a player's browser could compile a recipe to the tier's mesh at load time. Designed for now (deterministic, small); not built until Chris asks. The recipe alone is small, but compiling it in the browser also needs the compiler (the CPU path as WebAssembly, or the WGSL path), the class templates and the style data in the game's download; that one-off cost is weighed against the meshes it saves when recipe delivery is designed.
- **Engine needs.** Anything the engine must add (glTF loading, LOD switching by tier, reading anchors) is filed in CORE_REQUESTS.md, never patched into evani-core or evani-render.

## Why it should be better
- Models arrive with the information the converter, the checker and the kit need, so integration is reading data, not guessing it.
- Provenance makes every library model reproducible and editable by the next agent.

## Baseline and measure
- Test J: kart family loads through evani-convert, the kit uses its anchors, bench passes on every tier, per-tier download sizes reported.
- geocheck: zero untagged issues with the Evani.Model karts in the race.

## Risks
- **Stage order**: M5 waits for Stage 3's converter and glTF import. Mitigation: decision D11 (proceed with M1 to M4; build M5 when Stage 3 lands).
- **Kit changes** for anchors belong to the kart kit's stage; they are filed, not done inside this track.

## Sources (ideas only)
PartField arXiv 2504.11451; STAGES.md; docs/GEOCHECK.md; kits/evani-kit-kart-racer (draw.rs, data/karts.json, hull.rs); CORE_REQUESTS.md.
