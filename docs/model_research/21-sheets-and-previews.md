# 21 Sheets and previews: how Evani.Model draws its own pictures

M0 clean-room note, 2026-10-15 (added in the M0 follow-up after the independent review, finding M3). Own words; no third-party code opened or copied.

## What it must do (brief, design principle 4, design instructions 2 and 10, tests A and K)
Pictures are never the default ("words and scores first; pictures only on request, as one combined image"), but several things need them: `model.sheet` (blueprint, raking light, variants, curvature, thickness); style calibration "renders sweeps on a contact sheet" that Chris approves once per style (note 02); the critic may see one sheet (note 11); test K is Chris comparing unlabelled contact sheets of the Evani.Model karts with the current ones; the 22 non-kart benchmark references are checked by Chris on one contact sheet (tests/model_bench/README.md); and M4's relief must be "correct on every tier" before the game can show it (note 12). Constraints: headless (inside an MCP call or a test run, no window), within the dependency rule (wgpu, glam, bytemuck, serde; no image or font crates, no Blender, no engine crates), and deterministic, because an approval records the hash of the sheet it was judged on (note 02).

## Known approaches
- **A headless offline renderer** (for example Blender in background mode, with its rasteriser or path tracer): good pictures, but a large separate install, a GPL program run as a separate process, seconds of start-up per call, and output that changes with its version and settings.
- **The engine's own capture** (evani-live `capture_frame`): shows the real look and lighting, but needs the game running in the Evani.dev tab and a model the game can load; glTF import is Stage 3.
- **Sphere tracing** of distance fields (Hart 1996): march each pixel's ray by the field's distance bound until it reaches the surface; draws the field with no mesh, at a cost per pixel of many field evaluations.
- **Edge-function rasterisation** (Pineda 1988): decide pixel coverage of a triangle from three linear edge tests and keep the nearest by a depth buffer; cheap on the CPU for tens of thousands of triangles, and exact when the edge tests use integer (fixed-point) coordinates.
- **Cel shading and outlines**: quantise the light term into a few bands; draw outlines where depth or normal changes sharply between neighbouring pixels (the "G-buffer" idea of Saito and Takahashi 1990; cartoon rendering, Decaudin 1996).
- **Diagnostic maps**: curvature and wall thickness shown as false colour on the surface.

## Our approach
- **One small renderer of our own, CPU first, no window** (module `preview`). It draws two kinds of input with the same code:
  - a *candidate* (a field, not compiled yet): first a quick preview mesh from our own mesher at the sheet's pixel size (note 13 with its cheapest settings), which on the CPU is far cheaper than sphere tracing every pixel;
  - a *compiled tier*: that tier's actual mesh, normal map, height map and micro-detail recipe, which is exactly what a player's machine would get.
- **Coverage and depth by rasterising triangles** with edge functions in fixed-point screen coordinates (1/256 of a pixel), four samples per pixel at fixed positions, and a fixed tie rule for equal depths. Coverage is then exact and identical on every machine.
- **Normals per pixel, so detail finer than the mesh still shows.** For a candidate, each visible sample is moved onto the field's surface (one Newton step along the exact gradient, as in note 16's bakes) and shaded with the field's exact gradient, so relief and bevels finer than the preview mesh appear correctly. For a compiled tier, the shading normal is the mesh normal plus the tier's normal map, plus the micro-detail recipe evaluated per pixel and faded out below about 1.5 pixels of footprint (the band-limiting the game's shader will also need, note 12). Each tier is drawn at that tier's resolution and typical view distance, so a sheet row shows what each tier really keeps.
- **The look's own shading rules, read, not copied by hand.** The toon look's ramp has three bands (thresholds 0.02 and 0.45 on the light term, softened by 0.025, levels 0.25, 0.6 and 1.0: `TOON_RAMP_BODY` in looks/evani-look-toon) and outline settings in its params.json (`fx.outline.strength`, `fx.outline.thickness`). params.json is plain data and is read at run time; the band constants live in the look's shader text, so evani-model keeps a small preview-shading data file with those numbers and their source, and a test in the dev-only benchmark tool (which may link the look crate) fails when the look's ramp no longer matches. evani-model itself never depends on a look crate. Models have no colour, so sheets are grey; marks (failed musts, thin walls, curvature) use one small fixed false-colour ramp, written down in the data file.
- **Outlines** from depth and normal jumps between neighbouring pixels, with the look's thickness and strength.
- **The sheet kinds** (each one combined image, a caption line, and a scale bar in metres):
  - `blueprint`: orthographic front, side and top plus the class's own views (the kart's chase view) in a grid; regions where a must failed or a clearance is close are outlined from the checker's numbers.
  - `raking`: light about 10 degrees above the surface from two sides, to show relief; for a compiled model, one row per tier at that tier's pixel size.
  - `variants`: the top K candidates side by side with the same camera, labelled by id.
  - `curvature`: mean curvature from the field's second derivatives (forward mode), false colour.
  - `thickness`: wall thickness from field queries along the inward normal, false colour, anything under `min_thickness` outlined.
  - Internal sheets built from the same parts: the calibration contact sheet (one adjective swept over two or three classes, labelled only with numbers, note 02); the reference check sheet for the 22 benchmark assemblies; and the test K comparison sheet (current karts drawn from their reference meshes and Evani.Model karts, same renderer, same cameras, in an order shuffled by a fixed seed, with no labels; the key is kept apart).
- **Text without a font library**: our own tiny bitmap font (digits, capital letters and a few symbols, 5 by 7 pixels) stored as data in the crate.
- **PNG writer, ours**: 8-bit grey or RGB with stored (uncompressed) deflate blocks and our own CRC and Adler checksums, valid PNG with no compression library (decided by the dependency rule, D3). Sheets for agents default to 1024 pixels wide (an image's token cost depends on its pixel size, not its file size); sign-off sheets for Chris to 2048.
- **Deterministic bytes.** The renderer follows the CPU path rules of note 18 (fixed-point coverage, fixed sample positions, our own maths, tiles merged in order), so the same model and sheet kind give the same PNG bytes on every machine and thread count. Approvals record the sheet's hash. An optional GPU path (a wgpu compute rasteriser) may be added later for fast calibration sweeps if the CPU is too slow; its pictures would be marked "preview" and never used for an approval.
- **Checking the preview against the game, once it can show models.** When Stage 3 lets the kart racer load a model, the same model is captured in the game (`capture_frame` at the same camera and tier) and compared with our tier preview (silhouette IoU and agreement of the band regions). A disagreement means either our preview or the game's import is wrong, and it is fixed before another sign-off relies on the preview.

## Why it should be better
- No external program, engine or window: a sheet is part of the same MCP call, in about a second, on any creator's machine, including CPU-only ones.
- Identical bytes everywhere, so an approval is pinned to exactly the picture Chris saw, and test K's sheets can be regenerated and verified.
- It shows each tier as the player will get it, including the medium and fine relief the game cannot draw yet (note 12), so M4 can be judged before the engine support lands.
- One renderer serves calibration, the critic, references, test K and relief checks, so all of them see the same thing.

## Baseline and measure
- Time per sheet kind (target: a four-view 1024 by 768 kart blueprint under 1 s on 8 CPU threads; a calibration session of about 40 sheets under a minute), and file size.
- Determinism: identical sheet hashes across thread counts, x86-64 and wasm32 in M1, and across the hardware matrix (test H).
- Sanity check in M1: silhouettes and band regions against a render of the same mesh in headless Blender (run separately, as for the other baselines) with matched camera and a three-band ramp.
- After Stage 3: agreement with the in-game capture, as above.

## Risks
- **The preview drifts from the game's look** (the look's ramp or outline changes). Mitigation: the drift test on the band constants, run-time reading of params.json, and the in-game comparison once Stage 3 exists.
- **Faceting from the preview mesh.** Mitigation: per-pixel field normals; the preview mesh only decides coverage and depth.
- **CPU time for large sheets or many tiers.** Mitigation: tiled parallel rendering, a resolution cap per kind, and the optional GPU path for non-approval previews.

## Sources (ideas only)
J. C. Hart, "Sphere tracing", 1996; J. Pineda, "A parallel algorithm for polygon rasterization", 1988; T. Saito and T. Takahashi, "Comprehensible rendering of 3-D shapes", 1990; P. Decaudin, "Cartoon-looking rendering of 3D scenes", 1996; the PNG specification (W3C, ISO/IEC 15948) and RFC 1950 and 1951 (zlib and deflate formats, stored blocks); looks/evani-look-toon (its ramp and params.json, our own code); docs/LIVE_API.md (`capture_frame`).
