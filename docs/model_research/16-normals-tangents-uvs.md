# 16 Normals, tangents (MikkTSpace convention) and UVs

M0 clean-room note, 2026-10-15. Own words from the MikkTSpace description, the glTF 2.0 specification and the papers below. The MikkTSpace reference source was not opened; we implement the convention from its written description.

## What it must do (brief, design instruction 11 and test A)
Normals and relief sampled exactly from SDF gradients and relief height functions per texel, with no ray-casting artefacts; tangents follow the MikkTSpace convention, implemented ourselves. UVs: our own unwrapping with seams at part boundaries and hidden edges known from the graph; packed per tier. Test A: valid normal maps without seams.

## Known approaches
- **Normal-map baking by ray casting** from a low-poly mesh to a high-poly one along a cage: misses and double hits at concave corners and thin parts give the familiar baking artefacts; results depend on cage distance.
- **MikkTSpace** (M. Mikkelsen, 2008 thesis "Simulation of wrinkled surfaces revisited" and its public description). In our words: a tangent frame is computed per vertex from the triangles around it, using each triangle's texture-coordinate derivatives; contributions are weighted by the corner angle and made perpendicular to the vertex normal; the bitangent is not stored but rebuilt from the normal, the tangent and a stored handedness sign; vertices are only averaged with others that share position, normal, texture coordinate and handedness; and the result does not depend on triangle order or the order of corners. When the baker and the renderer follow the same convention, a baked normal map reproduces the high-detail shading exactly.
- **glTF 2.0** (Khronos): tangents are four-component, with the fourth (+1 or −1) giving handedness; normal textures are tangent-space with +X right, +Y up, +Z out of the surface; when a file has no tangents, renderers are expected to compute MikkTSpace tangents from the given normals and texture coordinates.
- **UV unwrapping.** Least Squares Conformal Maps (Lévy et al. 2002) and Angle-Based Flattening (Sheffer and de Sturler; ABF++ 2005) flatten charts with low angle distortion; xatlas-style tools grow charts from a mesh, flatten and pack them; packing is a bin-packing problem usually solved with skyline or shelf heuristics. Seams are placed by geometry heuristics, not by meaning.

## Our approach
- **Vertex normals** are the exact field gradient at each vertex (note 05), split at declared creases (note 13) so sharp edges stay sharp.
- **Tangents** follow the MikkTSpace convention as described above, implemented from that description: per-corner contributions from texture derivatives, angle weighting, projection perpendicular to the normal, grouping by identical position, normal, UV and handedness, order independence; stored as glTF tangents with the handedness in the fourth component. Verified by comparing against tangents computed by Blender's exporter on the same mesh (run separately as a tool, outputs compared numerically, its code never opened).
- **Exact per-texel normal baking, no rays.** For each texel of a chart, find the point on the mesh triangle it covers, move it onto the true surface of the field plus that tier's geometry relief by a few bounded Newton steps along the gradient (the field gives the distance, so the step is exact and never jumps to another part), then take the gradient of the field plus the normal-map relief layers there. Express that normal in the mesh's MikkTSpace frame at the texel. Height (for parallax on High and Ultra) is the relief height function at the same point. There is no cage, no ray and no miss.
- **Charts from generators.** Each part's natural parameterisation (note 04) is its chart layout: a lathe opens along one seam on the side the class says is hidden (a tyre's inner face), a sweep along its underside, a rounded box into faces whose seams follow bevel midlines on hidden edges. Seams sit on part boundaries (where materials and relief already change) and on edges the typical views never see (undersides, contact areas, cavities). Only parts without a natural parameterisation (blended bodies, noise rocks, external parts) are flattened by our own conformal flattening (the least-squares conformal idea, our own sparse solver) after cutting along their least visible curves.
- **Real-unit texel density per tier.** Charts are scaled to the tier's texel density (from the pixel size on the model at the typical distance, note 14), so all parts share one density; relief patterns are defined in metres and come out at the right scale with no AI work.
- **Packing.** Our own skyline packer, deterministic order (largest chart first, ties by part name), with padding wide enough for the tier's mip levels; texels across a seam are filled by sampling the same 3D function on both sides, so seams match by construction.
- **Seam check (test A).** For every seam edge, decode the normal map on both sides through the MikkTSpace frames and compare the shading normals; the difference must stay under a small angle.

## Why it should be better
- Exact per-texel sampling of the field removes the whole class of ray-casting bake artefacts.
- Seams placed by meaning (part boundaries and hidden edges from the graph and typical views) are invisible in play and cost no extra vertices where materials already split.
- Natural charts have almost no distortion for lathes and sweeps, the commonest parts.

## Baseline and measure
- xatlas run separately on the same meshes: chart count, seam length (total and visible from typical views), area and angle distortion, packing efficiency, time.
- Tangent agreement with Blender's MikkTSpace output (maximum angle difference) on the benchmark meshes.
- Normal-map accuracy: shading difference between Ultra geometry with full relief and the tier's mesh plus its normal map, rendered at the tier's resolution.

## Risks
- **Exact agreement with every MikkTSpace implementation** is hard without the reference code (degenerate triangles, zero-area UV triangles). Mitigation: numeric comparison with Blender's output, documented tolerances, and avoiding degenerate UV triangles in our own charts.
- **Newton projection across thin parts** might land on the wrong side. Mitigation: steps bounded by the cell size and part membership checked by the graph's part labels.

## Sources (ideas only)
M. Mikkelsen, "Simulation of wrinkled surfaces revisited", 2008, and the MikkTSpace description; Khronos glTF 2.0 specification (meshes, tangents, normal textures); Lévy, Petitjean, Ray, Maillot, "Least squares conformal maps", 2002; Sheffer, Lévy, Mogilnitsky, Bogomyakov, "ABF++", 2005; xatlas project README (as a baseline tool description only).
