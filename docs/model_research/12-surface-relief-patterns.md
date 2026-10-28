# 12 Surface relief and the pattern library

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 10)
Named relief layers on regions chosen by meaning (edges, cavities, up-facing, contact areas, along curves, inside parts), from our own pattern library (tread, knurling, rivets, panel lines, bricks, planks, scales, weave, hammered metal, cracks, dents, scratches, stitching, wood grain, stone, bark), in real units, with no UV work by the AI. One height definition, three deliveries: large detail as geometry in the LODs (WebGPU has no tessellation), medium as normal maps (plus height for parallax on High and Ultra), fine as shader micro-detail at zero download cost. Readability checked by maths per tier. Wear follows plausibility rules.

## Known approaches
- **Bump and normal mapping** (Blinn 1978 and later): perturb shading normals from a height or normal texture; cheap, but needs UVs and a bake, and shows no silhouette change.
- **Displacement** moves geometry; on GPUs with tessellation it is done at draw time. WebGPU has no tessellation stage (brief), so displacement must be baked into the mesh.
- **Parallax and parallax occlusion mapping** fake depth from a height map at a per-pixel cost; suited to higher tiers.
- **Procedural patterns** (Perlin noise 1985; Worley cellular noise 1996; tiling patterns for bricks and planks) evaluated in 3D or on a surface parameterisation; triplanar projection avoids UVs at the cost of blending seams.
- **Mask-driven wear** in texturing tools uses curvature and ambient-occlusion masks to put chips on convex edges and dirt in cavities.
- **Learned generators** (TRELLIS.2) output detailed geometry and PBR textures together, with no separation into layers or tiers.

## Our approach
- **One height function per layer.** A layer is `region selector × pattern × amplitude`, all in metres, defined on the part's natural parameterisation (note 04: a tyre's tread runs around the lathe's angle and across its profile; planks follow the plank's length) or in part-local 3D for volumetric patterns (stone, hammered metal). The AI writes `tyres.outer: tread(chevron, depth 4mm, pitch 2cm)` and never touches UVs.
- **Region selectors from the field and the graph.** `edges` (high curvature of the field, plus part boundaries and blend seams known from the graph), `cavities` (an ambient-occlusion estimate from a few field samples along the normal), `up` (normal against the up axis), `contact` (near another part's anchor or the ground), `along(curve)` (a sweep's centre curve or a named edge), `inside` (part-local volume), and named part regions from generators (`outer`, `rim`, `top`). Selectors are soft masks with widths in metres, so layers fade, not cut.
- **Pattern library, ours.** Each pattern is a height function with named, unit-bearing parameters and a declared spectrum (its main wavelengths): tread (chevron, block, rib), knurling (diamond, straight), rivets (rows along curves), panel lines (grooves along part seams or chosen curves), bricks, planks, scales, weave, hammered metal, cracks (cellular noise ridges), dents (sparse smooth pits), scratches (thin directional grooves), stitching (dashes along a seam), wood grain, stone, bark. All noise is our own deterministic value and cellular noise seeded from the layer's hash.
- **Three deliveries, split by wavelength and tier.** For each tier, from the class's typical view distance and the tier's render resolution we compute the size of one screen pixel on the model. A pattern's spectrum is split at two thresholds:
  - wavelengths larger than about 6 pixels at that tier's LOD distance and amplitudes that change the silhouette → added to the field before meshing that tier (geometry);
  - between about 1.5 and 6 pixels → baked into the tier's normal map (and height map on High and Ultra for parallax), sampled exactly per texel (note 16);
  - below that → exported as a micro-detail recipe (pattern id, parameters, mask) for the look's shader, costing no download bytes.
  The split is per tier, so Low gets tread as normal map only while Ultra gets the tread blocks as geometry.
- **Readability by maths.** A layer is readable at a tier if its feature is at least 2 pixels across at the tier's typical distance and its shading change crosses a visible threshold under the look's lighting. For the toon look (2 to 3 diffuse bands, catalog render rules) small normal changes inside one band are invisible, so the check estimates whether the relief's normal variation pushes lighting across a band edge for the look's key light, or whether its depth or normal edge is strong enough to draw an outline. Unreadable layers are reported (and dropped from that tier's delivery) instead of costing bytes.
- **Wear plausibility rules.** Chips on convex edges weighted by exposure (outward-facing, near the ground or contact areas); scratches aligned with the part's likely motion or handling direction (from anchors such as grips); dents on large exposed faces, never in cavities; nothing inside cavities (dirt there would be colour, which is outside the brief); density in the intent is a cap, not a uniform spray.

## What Evani.Model does, and what the game must add (not assumed)
The three deliveries are only half Evani.Model's work. The engine today draws no normal maps, has no tangent or texture input for models, no parallax and no micro-detail hook in any look (checked in crates/evani-render/src and looks/*/src, 2026-10-15), and it cannot load glTF at all until Stage 3. This track may not change evani-core, evani-render or a look, so the split is:
- **Evani.Model (this track, M4):** the height functions, region masks and per-tier wavelength split; geometry relief meshed into each tier's LOD; per-texel normal and height bakes with MikkTSpace-convention tangents in each tier's `.glb` (note 16, 17); the micro-detail recipe (pattern id, parameters, mask) in `extras.evani`; the readability check by maths; and pictures of every tier as a player would see it, drawn by our own renderer (note 21), including normal maps and the band-limited micro-detail.
- **The game side (filed in CORE_REQUESTS.md on 2026-10-15 as "relief display for imported models", status requested, for the Evani.Model track):** glTF import that keeps tangents, normal and height textures and the `extras.evani` data (Stage 3 "Real models"); normal-mapped shading in the 3D forward path, with the toon look's ramp taking the mapped normal; height parallax on High and Ultra; and a micro-detail slot in the toon look that evaluates a recipe from the model's metadata, faded out by screen-space derivatives below about 1.5 pixels so it does not shimmer.
- **Consequence for M4 (decision D19):** M4 is judged on Evani.Model's own tier previews and its maths checks ("tread, bolts and panel lines correct on every tier" in the preview at each tier's resolution). The in-game proof of the same karts waits for Stage 3 and the engine request above, together with M5's integration (D11). Geometry relief needs nothing new from the game beyond glTF import.

## Why it should be better
- One definition serves geometry, normal map and shader detail, so the three never disagree, and each tier gets only what it can show.
- Real units on the part's own parameterisation remove UV work from the AI completely.
- Readability is checked per tier against the actual look, so we never ship tread that the toon bands erase.

## Baseline and measure
- Readability against known cases (test A): a tread that must read on High and not on Low, a panel line that must survive the outline pass.
- Bytes per tier with and without the three-way split (test D); visual difference against an all-geometry Ultra reference rendered at each tier's resolution.
- M4 target: karts have tread, bolts and panel lines correct on every tier, judged in our tier previews (note 21) until the game can show normal maps and micro-detail; then again in the game.

## Risks
- **Geometry relief on curved parts** distorts patterns (tread pitch on a tyre's sidewall). Mitigation: patterns are defined on the natural parameterisation with metric correction.
- **Toon readability estimate** depends on the look's band thresholds and outline settings; if the look changes, thresholds must be re-read (from the look crate's registered params, not copied).

## Sources (ideas only)
J. Blinn, "Simulation of wrinkled surfaces", 1978; K. Perlin, "An image synthesizer", 1985; S. Worley, "A cellular texture basis function", 1996; parallax occlusion mapping literature; catalog/looks.json (toon render rules); TRELLIS.2 README (output description).
