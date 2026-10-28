# Evani.Model: the brief (Chris, 2026-10-15)

Chris's brief, kept word for word as he gave it (two dated notes in square brackets, added 2026-10-16, point to the character plan; they are not his words). Workers build from this file and from the clean-room notes in docs/model_research/. Placement: its own track, placed between Stage 2 and Stage 3 in STAGES.md ("Place Evani.Model in between", Chris).

DELIVERABLE: Evani.Model is working software, a standalone 3D modeling tool with its own geometry engine, comparable to Blender in what it produces but operated by AI through MCP commands instead of by people through a UI. It is not a framework, a guide, a plugin for another modeler, or a set of scripts driving one. Documents support the tool; they are never the deliverable. Every milestone after M0 ends with running code that produces real models.

BUILD EVANI.MODEL — AN ORIGINAL, AI-NATIVE 3D MODELER

PURPOSE
Evani.Model is a 3D modeler used only by AI agents, never by people. It produces game-ready geometry: shape and surface relief (bumps, grooves, tread, dents, grain). No colour, materials, skinning or animation. People only see results.
AI is strong at concepts, structure, analogies and tests, and weak at placing thousands of exact numbers or judging fine detail by eye. So the AI states intent, and the machine does all geometry, exploration and verification.
Our two measures of success: QUALITY (exact, clean, beautiful geometry) and USAGE EFFICIENCY (fewest calls, tokens, seconds, GPU time and bytes per finished model). Target: a finished, tested model in 2 or 3 tool calls, each returning in seconds, on any creator's machine.

ORIGINALITY (non-negotiable)
1. Evani.Model is a fresh design. Research, papers and other projects are inspiration only. No code is copied, ported, translated or adapted from any other project, and no third-party source files or model weights live in our repository.
2. Clean-room process: a research agent reads papers and, where needed, other projects, and writes design notes in its own words, with no code, into docs/model_research/. Builder agents implement only from this brief and those notes, and never open third-party source code for the functionality they are building.
3. Improve, don't replicate: for each component, the notes record the known approaches, then our approach, why it should be better (using what only Evani.Model knows: the part graph, the SDF source, the style profile and the game's tiers), and the measured result against a baseline.
4. Standards are followed, implemented ourselves: glTF 2.0, the MikkTSpace tangent convention, WGSL.
5. Dependencies: only the foundation crates the engine already uses (wgpu, glam, bytemuck, serde). Anything else needs Chris's approval with a one-line reason. External tools (meshoptimizer, xatlas, CoACD and similar) may be run separately as benchmark baselines only, never linked or copied. External generators (TRELLIS.2, OmniPart) may be called only as optional plug-in services behind our own interface.
6. Before each milestone sign-off, run a licence and code-similarity scan against the projects referenced below; any match is investigated and rewritten.

WHERE IT LIVES
In the Evani.Dev workspace as crates/evani-model (library) and tools/evani-model (native CLI plus MCP server). Development-only: only compiled models (and later, optionally, recipes) reach players. It never modifies evani-core or evani-render; engine needs go to CORE_REQUESTS.md. It runs as its own track alongside the stages and integrates through glTF, evani-convert and geocheck.

HARDWARE ADAPTIVITY (a tool for every creator, not one machine)
1. Detect at startup: GPU adapter, vendor, memory and wgpu limits, CPU cores and memory, platform (Windows, macOS, Linux via Vulkan, Metal, DX12). Expose it through model.describe_system.
2. Compute tiers, automatic and overridable: Pro (24 GB or more), Standard (mainstream discrete GPU or Apple Silicon), Lite (integrated or older GPU), CPU-only, and Cloud (the same API run remotely through Evani.app).
3. Fixed time, adaptive effort: every call has a time budget (default 3 s for make, 1 s for edit). Candidate count, scoring resolution and refinement scale to fit it. Scorecards report candidates explored and the tier used.
4. Memory safety: tile and stream evaluation and meshing to fit the adapter's limits. Degrade gracefully and say so; never crash on small GPUs.
5. Portable by default: every core component is our own, on wgpu compute and CPU paths that run everywhere. Nothing required depends on one vendor.
6. Same result everywhere: the GPU explores only. The top candidates are re-scored and the final model compiled on a deterministic CPU path (fixed operation order, no fast-math), so the same intent and seed give the same model hash on every machine.
7. Agent-agnostic MCP tools, usable by any creator's AI.
8. Later option: a WebAssembly and WebGPU build for in-browser search and previews, with heavy steps in the cloud.

DESIGN PRINCIPLES
1. Concepts, not coordinates; numbers only where exact sizes matter.
2. Search, don't sculpt: many candidates, scored automatically, best few returned.
3. One round trip per decision.
4. Words and scores first; pictures only on request, as one combined image.
5. Edits as adjectives, analogies and tests.
6. Knowledge fills the gaps through class templates.
7. Exactness from maths, never from vision; vision and embeddings only guide.
8. Deterministic, as defined above.
9. Efficiency is a feature: every step is designed to cut calls, tokens, time, GPU work and bytes.

DESIGN INSTRUCTIONS

1. Intent document (.evmodel): readable text with a published schema in docs/MODEL_API.md. Fields: make, like, style, parts (adjectives, optional numbers, relationships), relief, must, match, explore, return, seed. Example:

    make: kart
    like: library/sugarpeak_kart
    style: toon_chunky
    parts:
      chassis: { stance: low, wide }
      rear_tyres: { size: oversized }
      engine_cover: { shape: bubble, exhausts: 2 }
      seat: { type: bucket }
      nose: { shape: blunt, rounded }
    relief:
      tyres.outer: tread(chevron, depth 4mm, pitch 2cm)
      chassis.edges: wear(chips, depth 1mm, density 0.3)
    must:
      fit_box: [1.3, 0.8, 0.7] m
      wheels_clear_chassis_at_steer: 30 deg
      seats: chibi_driver_v2
    match: { side: ref/kart_side.png, front: ref/kart_front.png }
    explore: auto
    return: 3

2. Style profiles: per look, adjectives mapped to parameter ranges, "more/less" to step sizes, plus bevel and proportion rules. A calibration tool renders sweeps on a contact sheet; a critic proposes the mapping; Chris approves it once per style. Calibrate toon_chunky first. Use catalog/looks.json for style descriptions.

3. Class templates: default parts, proportions, relationships and gameplay anchors for: kart, stylized character body (no face detail), building, tree, rock, fence, sign, crate, barrel, lamp post, bench, door, chest, sword, shield, lollipop, gumdrop, candy cane. Approved models can extend their class.

4. Part generators (one interface): our own procedural generators for rounded boxes, shells, lathe profiles, sweeps and tubes, lofts, panels, frames, planks, bricks, noise rocks, branching trees, blended stylized bodies. The interface also accepts optional external generator services (OmniPart for organic parts fed by our part boxes; TRELLIS.2 for organic hero assets), constrained to fit the part graph.

5. Shape back end: our own SDF system with smooth blends, booleans that never fail, domain operations (mirror, repeat, twist, bend, taper), parametric sweeps, lathes, lofts and subdivision cages; WGSL compute on the GPU plus the deterministic CPU path.

6. Constraint solver: named anchors on every part; relationships (rests_on, flush_with, attached_at, centred_on, aligned, mirrored, array_along) solved analytically where possible, otherwise by our own nonlinear least squares. "Must" tests become constraints where possible, verified checks otherwise.

7. Search and scoring, designed for efficiency:
   - Quasi-random sampling within the style's ranges.
   - Multi-fidelity scoring: a cheap low-resolution pass prunes most candidates; only survivors get higher-resolution scoring (successive halving).
   - Early stop when the top candidates converge.
   - Learned priors: each approved model shifts the sampling distribution for its class and style, so fewer candidates are needed over time.
   - Refinement of the best with our own evolution-strategy optimizer; evaluate a differentiable-rendering fit as an alternative and keep whichever is faster at equal quality.
   - Scores: silhouette IoU per view, proportions against the style, every "must" test (hard pass or fail), and an optional image-text score from a plug-in embedding model.
   - Top K returned with compact scorecards.

8. Language edits, retrieval and verification: adjective deltas through the style profile; analogies resolved against the Evani Library; a retrieval index over our docs, verbs, patterns and approved recipes, queried before writing intent; after every edit, verification that the requested change happened and nothing else moved beyond tolerance; versioned, undoable edits; parts cached by content hash.

9. Critic: a different, stronger model than the builder; refiners get exact numeric discrepancies; after 2 failed rounds, rethink the construction. Spatial correctness is always decided by maths tests.

10. Surface relief: named layers on regions chosen by meaning (edges, cavities, up-facing, contact areas, along curves, inside parts), from our own pattern library (tread, knurling, rivets, panel lines, bricks, planks, scales, weave, hammered metal, cracks, dents, scratches, stitching, wood grain, stone, bark), in real units, no UV work by the AI. One height definition, three deliveries: large detail as geometry in the LODs (WebGPU has no tessellation); medium as normal maps (plus height for parallax on High and Ultra); fine as shader micro-detail at zero download cost. Readability checked by maths per tier. Wear follows plausibility rules.

11. Compiler, our own, built to beat generic mesh tools by using what only we know:
   - Meshing: our own feature-preserving extraction from the SDF, with resolution adapted to curvature, relief importance and on-screen size per tier.
   - LODs: re-extracted from the source per tier, dropping parts, details and relief layers below each tier's on-screen threshold while protecting the silhouette, instead of simplifying a finished mesh.
   - Collision: built from the part graph (one simple proxy per part or part group), not by decomposing a finished mesh.
   - Normals and relief: sampled exactly from SDF gradients and relief height functions per texel, with no ray-casting artefacts; tangents follow the MikkTSpace convention, implemented ourselves.
   - UVs: our own unwrapping, with seams placed at part boundaries and hidden edges known from the graph; packed per tier.
   - Gameplay anchors exported (driver seat, wheel hubs, exhausts, grips).
   - Output: glTF 2.0 with quantization, plus Evani metadata (part tree, anchors, height map, micro-detail recipe, size per tier). Compression is handled by evani-convert.

12. Integration: every model passes evani-convert (quality gate, compression, size report) and geocheck (zero untagged issues); approved models go to the Evani Library with provenance (intent and seed attached); for imported meshes, part splitting in evani-convert is designed by us, informed by PartField-style research; design for a later recipe-only delivery compiled on the player's GPU.

MCP TOOLS (compact JSON, never raw vertex data)
model.describe_system, model.make, model.pick, model.edit, model.fit, model.check, model.describe, model.sheet (blueprint, raking light, variants, curvature, thickness), model.compile, model.calibrate_style, model.save_to_library.

RESEARCH AND REFERENCES (for ideas only: read, summarise in our own words, then design something better)
Internal: STAGES.md, docs/KIT_PROCESS.md, docs/LIVE_API.md, docs/LOOP_API.md, evani-convert and geocheck docs, catalog/looks.json, catalog/genres/, the current Sugarpeak Rally kart models.
Papers:
- CADSmith, arXiv 2603.26512: exact geometric validation plus a stronger-model judge; tiny gaps slipped through.
- LL3M, arXiv 2508.08228: retrieval over docs cut errors by 26%; verification of edits.
- ShapeCraft, arXiv 2510.17603: graph-based procedural shapes; several attempts per part.
- OmniPart, arXiv 2507.06165: part-aware generation from bounding-box layouts.
- PartField, arXiv 2504.11451: fast part segmentation of any shape.
- DiffCSG, arXiv 2409.01421: differentiable CSG for fitting.
- SigLIP 2, arXiv 2502.14786: image-text encoder.
- aDSL, arXiv 2608.17975: designing the modeling language with the agent.
- FullPart, arXiv 2510.26140: full-resolution part generation.
- 3D-GPT, arXiv 2310.12945: procedural modeling with language models.
Techniques to study, then improve on: SDF distance functions and smooth blending (Inigo Quilez's articles); feature-preserving isosurface extraction (Dual Contouring of Hermite Data, Ju et al. 2002, and successors); evolution strategies (The CMA Evolution Strategy: A Tutorial, Hansen, arXiv 1604.00772).
Standards to comply with: Khronos glTF 2.0 and its quantization extension; MikkTSpace.
Benchmark baselines, run separately and never linked: meshoptimizer, xatlas, CoACD, TRELLIS.2.
If a source contradicts this brief, flag it instead of silently changing the design.

WHAT TO EXPECT AT THE END
1. crates/evani-model and tools/evani-model with the MCP tools.
2. docs/EVANI_MODEL.md (design, limits), docs/MODEL_API.md (schema, verbs, tools, examples), docs/model_research/ (clean-room notes and a known/ours/why/result table per component), and a section in docs/BENCHMARKS.md.
3. A calibrated toon_chunky profile, the class templates, our procedural generators and the relief pattern library.
4. The Sugarpeak kart family (player kart and 7 rivals) rebuilt in Evani.Model and running in the kart racer.
5. A benchmark report covering every test below, and 3 recorded example sessions with calls, time and tokens.
Not expected yet: materials and colour, skinning, animation, native detailed organic hero characters.
[Note added 2026-10-16, not part of Chris's original words: Chris asked for characters built by code ("ok but we need to be able to build these models with code") and chose "Build fresh". Colour, materials, skinning and native detailed characters are now planned in docs/model_research/plan-characters.md (milestones MC1 to MC4), pending his approval of each.]

HOW WE TEST THAT WE HIT THE GOAL
A. Correctness, automated on every change: determinism (identical hash on CPU and GPU paths and across machines); unit tests with passing and failing cases for every "must" kind; meshes watertight, manifold, free of self-intersections and within triangle budgets; valid normal maps without seams; relief readability against known cases; edit locality.
B. Benchmark set, fixed and versioned in tests/model_bench/: 30 intents with references. 10 hard-surface props (crate, barrel, sign, lamp post, fence, bench, door, chest, sword, shield); 10 vehicles and structures (the 8 Sugarpeak karts, a small house, a tower); 10 stylized organic-ish shapes (tree, rock, mushroom, cloud, lollipop, gumdrop, candy cane, cactus, bush, chibi body). References: the current kart models' meshes and silhouettes; for the rest, front and side silhouettes from generated reference images or simple reference meshes.
C. Quality targets, the same on every tier: at least 90% of intents pass all "must" tests within 3 calls (median 2); silhouette IoU 0.9 or better; Chamfer distance and F1 reported where reference meshes exist; at least 95% of adjective edits move the target measure correctly with nothing else moving beyond tolerance; every model passes evani-convert and geocheck.
D. Efficiency targets: tokens, calls, wall time and GPU time per finished model reported for every intent; at least 5x fewer tokens and calls than the baseline in F at equal or better quality; candidates needed to reach the quality target falling as learned priors grow (reported across the benchmark run order); LODs with at least 30% fewer triangles than a generic simplifier baseline at equal silhouette error; bytes per tier reported.
E. Speed per tier: model.make within its time budget on every tier, exploring at least Pro 512, Standard 256, Lite 64, CPU-only 32 candidates; model.edit under 1 s on Pro and Standard, under 2 s on Lite and CPU-only; model.compile for a kart-sized model under 10 s Standard, 30 s Lite, 60 s CPU-only.
F. Baseline: the same agent model without Evani.Model, writing raw SDF or Blender Python for 10 benchmark intents; compare quality, calls, time and tokens.
G. Ablations: rerun the benchmark with each turned off in turn: search, multi-fidelity scoring, learned priors, word calibration, retrieval, edit verification, stronger critic. Report the effect on quality and efficiency.
H. Hardware matrix: run A to E on a laptop with integrated graphics, an Apple Silicon Mac, the dev machine's RTX 4060 Ti, an AMD discrete GPU, CPU-only, and the cloud tier (cloud machines where we lack hardware). Report per tier, confirm identical model hashes everywhere, and no tier may fail a "must" test that passes on another.
I. Originality: clean-room notes and the known/ours/why/result table complete for every component; licence and code-similarity scan clean at each milestone.
J. Game proof: the kart family drops into the kart racer through evani-convert, the kit uses its anchors, bench passes on every tier, and per-tier download sizes are reported.
K. Human sign-off: Chris compares unlabelled contact sheets of the Evani.Model karts against the current karts; done only when Evani.Model's are preferred or judged equal.

MILESTONES (each ends with a commit, a report against the relevant tests, and Chris's approval)
M0 Research: clean-room notes and the known/ours/why table for every component, plus the benchmark set and baselines.
M1 Compiler: intent document, shape back end, constraint solver, procedural generators, compiler. Done when a kart written as intent compiles to valid glTF with LODs, anchors and collision, passing its tests and section A.
M2 Search: sampling, multi-fidelity GPU scoring, refinement, scorecards, hardware tiers. Done when the 8 karts reach IoU 0.9 or better within the speed targets on Standard and Lite.
M3 Language: toon_chunky calibration, edits, analogies, class templates, retrieval, edit verification, learned priors. Done when the edit targets pass and "like sugarpeak_kart but a truck cab" works in one call.
M4 Relief: layers, pattern library, three-way delivery, readability, exact sampling. Done when the karts have tread, bolts and panel lines correct on every tier.
M5 Integration and proof: MCP tools, evani-convert and geocheck, library, full benchmark, efficiency, baseline, ablations, hardware matrix, originality checks, game proof, docs. Done when sections A to K all pass.
Do M0 to M2 first; they carry the most risk. If M2 misses its targets, stop and report before continuing.

WORKING RULES
- Iterate fast with Evani Loop practices: GPU evaluation, content-hash caching, compact structured outputs, sweeps instead of single commands.
- Model routing: Opus on high effort for architecture, solver and search design; lighter models for patterns, templates, docs and test data.
- Original designs only, in both code and the models produced.
- Record significant choices and reasons in DECISIONS.md.
- Honest limits in the docs: very thin features and very large scenes use explicit meshes alongside SDFs; detailed organic hero characters come from optional plug-in generators through evani-convert.
  [Note added 2026-10-16, not part of Chris's original words: superseded for characters by Chris's "Build fresh" choice; see docs/model_research/plan-characters.md, pending his approval of MC1 to MC4.]
