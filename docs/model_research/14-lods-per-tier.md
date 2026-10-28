# 14 Per-tier LODs from the source

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 11 and test D)
LODs re-extracted from the source per tier, dropping parts, details and relief layers below each tier's on-screen threshold while protecting the silhouette, instead of simplifying a finished mesh. Target: at least 30% fewer triangles than a generic simplifier baseline at equal silhouette error; bytes per tier reported.

## Known approaches
- **Edge-collapse simplification with quadric error** (Garland and Heckbert 1997) and its many variants: collapse edges in order of the error they add, measured as squared distance to the original faces' planes; attribute-aware versions also protect normals and UVs. This is what generic tools such as meshoptimizer's simplifier do (meshoptimizer is a baseline only, run separately).
- **Progressive meshes** (Hoppe 1996): a stream of collapses that can be replayed backwards.
- **Impostors and billboards** for distant objects.
- All of these start from a finished mesh: they do not know which triangles belong to a removable part, where relief came from, or which views the game uses.

## Our approach
- **Re-extract per tier.** For each tier (Low, Medium, High, Ultra, plus VR if the game uses it, from catalog/tiers.json) the compiler re-runs meshing (note 13) on the same field with that tier's settings, rather than simplifying the Ultra mesh.
- **Tier settings from data.** For each tier: the triangle budget for this model (a share of the tier's scene triangle limit, set per class: a kart in a field of 8 karts gets a fraction of Low's 100k), the pixel size on the model at the class's typical view distance (from the tier's render scale), and which relief deliveries apply (note 12).
- **Drop by meaning.** Parts whose projected size at the typical distance is below about 2 pixels are dropped at that tier (the kart's steering column on Low), or merged into their parent's field without their own detail. Roles from the template guide the order (detail before trim before body; wheels never dropped). Relief layers below the tier's geometry threshold go to normal maps or shader detail, or are dropped if unreadable.
- **Protect the silhouette.** The octree refinement priority (note 13) weights cells that lie on the outline for the class's typical views (computed by projecting cell boxes and testing against the field's silhouette in those views). Interior and back-facing detail gets cells last.
- **Measure, then trim.** After extraction, the silhouette error per typical view against the Ultra field is measured; if it is above the tier's tolerance, budget moves from interior cells to outline cells and the tier is re-extracted (at most two passes, deterministic).
- **Consistent seams across tiers.** UV charts are cut along the same part boundaries at every tier (note 16), so normal maps of different tiers line up and the game can switch LODs without texture changes per part.

## Why it should be better
- Removing a part or a relief layer by meaning saves many more triangles than collapsing edges evenly, at no silhouette cost.
- Re-extraction cannot carry over artefacts from a finer mesh, and every tier is watertight and manifold by construction.
- The typical views make silhouette protection target what players actually see.

## Baseline and measure
- Baseline: meshoptimizer's simplifier (and its gltfpack command line) run separately on our Ultra mesh to the same triangle counts; also to the same silhouette error. Test D: at least 30% fewer triangles at equal silhouette error.
- Silhouette error: 1 − IoU per typical view at the tier's resolution against the Ultra field; Chamfer distance to the field.
- Bytes per tier after quantisation (note 17) and after evani-convert's compression once it exists.

## Risks
- **Popping between tiers** when a part disappears. Mitigation: only parts already below 2 pixels are dropped; the game's LOD switch distance follows the same pixel thresholds.
- **Typical views missing a real camera** (a photo mode). Mitigation: classes list several views; geocheck's camera tests still run in the game.

## Sources (ideas only)
Garland and Heckbert, "Surface simplification using quadric error metrics", 1997; Hoppe, "Progressive meshes", 1996; meshoptimizer project README (as a baseline tool description only); catalog/tiers.json.
