# Evani.Model benchmark set (test B)

Fixed and versioned benchmark for Evani.Model (docs/EVANI_MODEL_BRIEF.md, test B). Data only: intents, a manifest and this plan. Reference files (silhouettes, meshes) are made in M1 by the reference tool described below (M0 delivered the intents, the manifest and the plan; the pictures moved to M1, decision D16); nothing here is fetched from the web and no copyrighted images are used.

## Contents
| Path | What |
|---|---|
| `props/*.evmodel` | 10 hard-surface props: crate, barrel, sign, lamp post, fence, bench, door, chest, sword, shield |
| `vehicles/*.evmodel` | 10 vehicles and structures: the 8 Sugarpeak Rally karts (`kart_01_pip` to `kart_08_rumble`), a small house, a tower |
| `organic/*.evmodel` | 10 stylized organic-ish shapes: tree, rock, mushroom, cloud, lollipop, gumdrop, candy cane, cactus, bush, chibi body |
| `manifest.json` | Conventions (axes, views, formats, units), the kart base reference (source, pose, draw order), and per item: class, seed, size, view frame and the reference description |
| `ref/<id>/` (from M1) | `front.png`, `side.png` (and `top.png`, `chase.png` for karts and the cloud), `mesh.glb` and `outer.bin` where a reference mesh exists, `ref.json` (provenance: tool version, kit commit, overrides) |

Every intent has a fixed seed (101 to 310), `explore: auto` and `return: 3`, and uses the `toon_chunky` profile. `auto` gives the same model on machines of the same tier; for test H (hashes across machines) the runner overrides `explore` with fixed levels so every machine runs the same schedules (docs/MODEL_API.md 1.8).

Test D (priors) runs in two fixed orders: the manifest's order (the 8 karts first, then props, structures, organic shapes) and its reverse. Nobody approves during a run, so in benchmark runs only a model that meets the benchmark bar counts as approved for that run, in a run-local prior file that never touches the real library priors (docs/model_research/09-learned-priors.md, decision D18). The priors-on and priors-off arms use identical `like` lines, so the difference is the priors alone. Definitions of every test D measure: docs/model_research/plan-baselines.md section 4.

Coverage: every must kind in docs/MODEL_API.md 1.6 appears in at least one intent (`clear` on the chest, `min_size` on the bench, `max_size` on the shield), and every pattern in 1.5 at least once (`weave` on the shield straps); dents and scratches also appear through `wear`. The 180-edit set for test C (note 10) is test data for M3 and is added to this folder before M3 starts.

The intents use the vocabulary proposed in docs/MODEL_API.md; class templates for `house`, `tower`, `mushroom`, `cloud`, `cactus` and `bush` are benchmark additions to the brief's 18 classes (noted in plan-m1.md).

## Kart references: what exists today
- The current karts are drawn in code, not loaded from files: `draw_kart` in `kits/evani-kit-kart-racer/src/draw.rs` places 34 instances of the engine's built-in meshes (bevelled cubes, cylinders, spheres) per kart: body parts, a driver (7 instances) and four wheels of three pieces each.
- **All 8 karts have the same shape.** The roster (`data/karts.json`: Pip, Mo, Taffy, Bramble, Zuzu, Nib, Clover, Rumble) changes only body and suit colours, and Evani.Model makes no colour. So the existing references for the 8 karts are 8 copies of one shape. Decision D4 (docs/model_research/plan-decisions.md): option A keeps all 8 references identical; option B (recommended) gives the 7 rivals shape variants defined as overrides on the base reference (listed in `manifest.json`), so the benchmark tests 8 different targets. Chris chose option B (2026-10-15), so the rival intents stay as written, adjectives and all; under A they would have had to be reduced to the base intent as well, or they would fight the shared reference.
- Measured size of the base kart without driver (tools/model-bench-ref, M1): 3.106 m long (to the exhaust tips), 2.21 m wide over the hub bars, 1.264 m tall to the wing; 2.14 m with the driver's helmet (M0 estimated 3.09 x 2.21 x 1.27).

## Plan for the kart reference extraction tool (built in M1)
A small native Rust tool, `tools/model-bench-ref` (a workspace member, development only), with no new dependencies. Reading our own kit's code and calling its public functions is allowed: it is our code.
1. **Capture.** Build the kart with `Kart::new(0, None, [0.0, 0.0, 0.0], 0.0)` (spec 0, no player, at the origin, yaw 0); a new kart has every motion field at rest (steer, drift, boost, hop, squash, slip and wheel roll zero; assert this). Call the kit's public `draw::draw_kart(&kart, ([1.0, 1.0, 1.0], None), [0.0, 0.0, 0.0], 0.0, 1.0, &mats, &mut rw)`: colours do not affect geometry; the view yaw is 0; any max speed above zero works because speed is 0; `mats` is a neutral table of 16 `Mat { index: 0, tint: [1.0; 3] }` (materials do not affect geometry either); `rw` is `RenderWorld::default()`. At rest the rig's squash factor is exactly 1 on every axis and roll and pitch are 0, so instance scales and rotations are the built ones. Read `RenderWorld::meshes` (mesh id, position, rotation, scale per instance). Assert there are exactly 34 instances in the order listed in the manifest; if the kit changes, the tool fails loudly and the manifest is updated rather than guessing.
2. **Split.** Instances 15 to 21 are the driver; the rest are the kart. Both are kept: the kart for the shape references, the seated driver as `ref/kart_driver/` (named `bench/kart_driver` by the karts' `seats` must).
3. **Overrides (option B).** Apply each rival's listed overrides to the captured instances (resize, move, rotate, swap the mesh id, add or remove instances). The overrides are data in the manifest, so the references are reproducible.
4. **Mesh.** Expand each instance with `evani_render::mesh::builtin` (CPU geometry of the built-in meshes) and its transform, and write `mesh.glb` with our own minimal glTF writer (the same one Evani.Model will use, or a simpler float-only version until it exists).
5. **Outer surface.** Because the instances overlap, compute the union by voxels: mark a 1 cm voxel inside if it is inside any instance (all built-in meshes are convex, so "inside" is a half-space test against each instance's faces), then take boundary voxels and project them to the nearest instance surface; store these points in `outer.bin` for Chamfer distance and F1.
6. **Silhouettes.** Rasterise the union into orthographic masks for front, side and top inside the item's fixed frame (`frame_m` metres square, 512 px), and a perspective mask from the kit's default chase camera pose, read from the kit's registered camera params at the commit used. Write 8-bit greyscale PNG with our own encoder (stored, uncompressed deflate blocks: valid PNG, no compression library).
7. **Provenance.** Write `ref.json`: tool version, kit commit hash, instance count, overrides applied, frame and camera used, and a SHA-256 of every output file.
8. **Tests.** The base kart's measured size matches the manifest within 1 cm; masks are symmetric for symmetric karts; running twice gives identical bytes.

Size: about 400 to 600 lines; a lighter model can build it from this plan (plan-m1.md task T2).

## Plan for the other 22 references
No images are fetched. Each item's front and side silhouettes are described in words in `manifest.json` (`front`, `side`, and `top` where needed) together with a list of simple masses with sizes (`masses`). In M1 the same tool builds each reference as a primitive assembly from those masses (boxes, rounded boxes, cylinders, lathes, tubes along paths, spheres and ellipsoids, with the stated blends), then makes masks and, where useful, an outer-surface point set exactly as for the karts. The primitive assemblies are written as data (`ref/<id>/assembly.json`) by a lighter model from the descriptions and checked by eye on one contact sheet by Chris before the M2 runs (drawn by Evani.Model's own preview renderer, docs/model_research/21-sheets-and-previews.md, M1 task T15). If Chris prefers images from an image generator for some organic items (decision D12), they would be generated, thresholded to masks and stored with their prompt and generator licence.

## How the benchmark is scored (summary; details in docs/model_research/plan-baselines.md)
Per intent and tier: must tests (pass or fail), silhouette IoU per view (target 0.9 or better), Chamfer distance and F1 where a reference mesh exists, calls, tokens, wall time, GPU time, candidates explored, bytes per tier.
