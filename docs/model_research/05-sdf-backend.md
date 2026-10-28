# 05 SDF back end

M0 clean-room note, 2026-10-15. Own words from Inigo Quilez's articles (read as articles, not code to copy), Hart's sphere tracing paper and our design. No third-party code opened or copied.

## What it must do (brief, design instruction 5)
Our own signed distance field system: smooth blends, booleans that never fail, domain operations (mirror, repeat, twist, bend, taper), parametric sweeps, lathes, lofts and subdivision cages; WGSL compute on the GPU plus a deterministic CPU path.

## Known approaches
- **Distance functions and their operators** (Inigo Quilez, "distance functions", "smooth minimum", "domain repetition", "interior distance" articles). Key facts we rely on, in our words:
  - Many primitives have exact distances (sphere, box, rounded box, capsule, torus, cylinder, cone); others only give a bound (ellipsoid), which is still safe for sphere tracing if it never overestimates.
  - Union, intersection and subtraction as min and max of distances never fail and need no topology, but the result is only a bound inside or near the joins, not an exact distance.
  - Smooth minimum blends two fields over a chosen width; several kernels exist (polynomial of degree 2 or 3, exponential, circular), differing in smoothness, locality and cost; the same kernel also gives a blend factor usable for relief and part labels.
  - Rounding (subtract a radius), shelling (absolute value minus thickness), elongation and revolution or extrusion of a 2D distance are exact or near-exact.
  - Domain repetition is cheap, but a shape that crosses its cell boundary is wrong unless neighbouring cells are also checked; limited repetition clamps the cell index.
  - Twist, bend and taper are not distance-preserving: they stretch space, so the field's Lipschitz constant grows above 1 and tracing steps must shrink by that factor.
- **Sphere tracing** (Hart 1996): march a ray by the field value; needs a Lipschitz bound.
- **Mesh CSG** (as in DCC tools) needs exact mesh booleans, which fail on coplanar or degenerate input; DiffCSG (arXiv 2409.01421) avoids computing the final mesh by rasterising CSG directly, which shows the value of never building the boolean mesh.
- **Fields on the GPU** are commonly written as one hand-made shader per scene.

## Our approach
- **An SDF graph, not shader code.** Parts compile to a small typed graph: primitives (exact where possible), operators (union, intersection, subtraction, smooth variants with a width in metres), modifiers (round, shell, elongate, offset) and domain operations (mirror across a plane, limited repeat along an axis or curve, twist, bend, taper, transform). Every node carries its parameters as slots, a guaranteed bounding box and a Lipschitz bound, computed when the graph is built.
- **Lipschitz bookkeeping.** Each node reports how much it can stretch space (twist by its rate times the radius, bend by its curvature times the extent, taper by its slope). The evaluator scales steps by the product, so every distance the mesher and the scorer use is a safe bound, never an overestimate.
- **Sweeps, lathes and lofts as 2D-in-3D fields.** A lathe revolves an exact 2D profile distance; a sweep measures the closest point on its centre curve (found per segment, with the curve sampled at a fixed parameter step for determinism) and evaluates the profile in that point's frame; a loft interpolates between profiles along the path. All give bounds, with their Lipschitz factors recorded.
- **Subdivision cages.** A low-poly control cage is refined by our own Catmull-Clark style subdivision (the published rules, implemented ourselves) to a fixed level on the CPU, then turned into a field: exact distance to its triangles through our own bounding volume hierarchy, with the sign from a winding-number inside test. On the GPU the cage's field is a sparse brick grid baked once per cage shape.
- **Booleans that never fail.** They are min and max on fields. Thin-wall risk (a subtraction leaving a sliver thinner than a cell) is caught by the thickness check (note 13 and must tests), not by a crash.
- **Two evaluators, one graph.**
  - GPU (exploration only): the graph compiles to a compact instruction list run by one WGSL compute interpreter; all candidates of one search share the instruction list and differ only in a per-candidate parameter block, so 512 candidates are one dispatch with no shader compilation per candidate. When a search evaluates enough points to pay for it, the same graph is also turned into a specialised WGSL function (one pipeline compile per search). We keep whichever is faster per tier (measured in M2).
  - CPU (decisions and final compile): the same instruction list run in f32 with a fixed operation order, no fused multiply-add, and our own implementations of sin, cos, atan2, exp, pow and sqrt-based helpers built only from correctly rounded basic operations (the platform maths library differs between Windows, macOS and Linux, so it is never used on this path). Threads split work into fixed tiles merged in tile order, so the thread count never changes the result.
- **Exact gradients.** The CPU evaluator computes each field value together with its gradient in one pass (forward-mode derivatives through every node), so normals, Hermite data for meshing and relief directions are exact rather than finite differences.
- **Culling by bounds.** Octree cells whose centre distance exceeds the cell's half-diagonal times the Lipschitz bound are empty or full and are skipped, on both paths.

## Why it should be better
- No boolean ever fails and no mesh is built until the end, so the search can try thousands of variations safely (the advantage DiffCSG gets by not meshing, made general).
- Exact Lipschitz bookkeeping means the domain operations the brief asks for (twist, bend, taper) are safe to trace and mesh, which hand-written shaders usually get wrong silently.
- The part graph gives every node a name, so a failed check points at a part, not at a pixel.
- One instruction list for many candidates makes GPU exploration cheap on Lite and integrated GPUs, where pipeline compiles are expensive.

## Baseline and measure
- Evaluation throughput (points per second) per tier on the kart graph; GPU interpreter vs specialised shader.
- Determinism: CPU path hash identical on Windows (x86-64), Linux (x86-64), macOS (arm64) and WebAssembly; differences between CPU and GPU field values measured and bounded.
- Lipschitz tightness: ratio of bound to true distance on sampled points (lower is faster tracing).

## Risks
- **Our own transcendental functions** must be accurate enough (target: within 2 ulp over the ranges used) and identical everywhere; tested against high-precision reference values computed offline.
- **Interpreter speed on the GPU** may be too low for Pro 512 candidates in 3 s at the higher fidelity rungs; the specialised shader is the fallback.
- **Smooth blends move anchors** (a blend adds material at joins). Mitigation: anchors are defined on the unblended parts and the solver accounts for blend width (note 06).

## Sources (ideas only)
Inigo Quilez, articles on distance functions, smooth minimum, domain repetition and interior distances (iquilezles.org); J. C. Hart, "Sphere tracing", The Visual Computer 1996; DiffCSG arXiv 2409.01421; Catmull and Clark 1978 (subdivision rules, as published).
