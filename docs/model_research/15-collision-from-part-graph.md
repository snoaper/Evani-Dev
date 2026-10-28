# 15 Collision from the part graph

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 11)
Collision built from the part graph (one simple proxy per part or part group), not by decomposing a finished mesh.

## Known approaches
- **Approximate convex decomposition** of a finished mesh: V-HACD (voxel-based hierarchical splitting) and CoACD (Wei et al., SIGGRAPH 2022: a collision-aware concavity measure and a tree search over cutting planes, which keeps parts that matter for contact). They work on any mesh but are slow on large meshes, can produce many hulls, and know nothing about which pieces the game cares about.
- **Hand-made proxies** (boxes, capsules, spheres) placed by artists: cheap and exactly what physics engines like, but manual.
- **The current kart racer** already uses a hand-made capsule footprint per kart (`kits/evani-kit-kart-racer/src/hull.rs`: a 2D capsule 1.1 m radius with a 0.45 m half-segment, matching the kart's 1.55 m half length).

## Our approach
- **Proxies from parameters.** Every generator gives the simplest proxy that covers its part (note 04): rounded box → oriented box; lathe and tyre → cylinder; capsule-like sweeps → capsule chain; blended bodies → a few capsules along the skeleton; rocks and trees → a convex hull of a few extreme points of the field (found by support queries in fixed directions) plus a trunk capsule. Computed analytically, so exact and instant.
- **Group by the graph.** Small parts (by volume share or role `detail`) merge into their parent's proxy; mirrored parts share one proxy shape; parts that move (a door, a chest lid, steering wheels) keep separate proxies named by their anchors so the game can animate them.
- **Fit check.** For each proxy, measure how well it matches its part's field: volume of proxy outside the part and of part outside the proxy, sampled on the octree. If the error is above the class tolerance, the proxy is upgraded (box → hull of extreme points → two proxies split at the part's natural parameter midpoint).
- **Game-shaped proxies.** The class template can ask for the game's own collision shape: for karts, a capsule footprint matching the kit's hull (half length and radius taken from the model, exported as metadata) plus wheel cylinders, so the kit can read its numbers from the model instead of constants.
- **Export.** Proxies are exported as glTF nodes with no mesh (or simple meshes for hulls) under a `collision` node, plus their shape parameters in Evani metadata (note 17).

## Why it should be better
- Proxies come from what the parts are, so a kart is a capsule plus four wheels in microseconds rather than dozens of hulls after seconds of decomposition.
- Moving parts get their own named proxies automatically.
- The game's collision rule can be matched exactly (the kart proxy's numbers can replace the kit's hull constants; geocheck check 7 today compares a kit's collision limits with the drawn track walls, so a check of a model's proxies against its drawn shape would be new).

## Baseline and measure
- CoACD run separately on the Ultra mesh (default settings and a setting tuned to a similar hull count): hull count, total vertices, time, and volume error (symmetric difference with the model, sampled), and contact error on a set of probe points.
- Ours must have fewer proxies and lower or equal volume error on hard-surface classes; organic classes report both honestly.

## Risks
- **Concave parts** (a shield's inner curve, a bench's gap) need more than one proxy; the fit check catches them.
- **External generator parts** have no analytic proxy; they use the extreme-point hull and are flagged.

## Sources (ideas only)
Wei, Liu, Ling and Su, "Approximate Convex Decomposition for 3D Meshes with Collision-Aware Concavity and Tree Search" (CoACD), SIGGRAPH 2022, arXiv 2205.02961; V-HACD project description; kits/evani-kit-kart-racer/src/hull.rs; docs/GEOCHECK.md check 7.
