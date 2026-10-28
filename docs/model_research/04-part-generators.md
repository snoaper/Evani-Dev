# 04 Part generators

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 4)
One interface for our own procedural generators (rounded boxes, shells, lathe profiles, sweeps and tubes, lofts, panels, frames, planks, bricks, noise rocks, branching trees, blended stylized bodies), which also accepts optional external generator services (OmniPart fed by our part boxes; TRELLIS.2 for organic hero assets), constrained to fit the part graph.

## Known approaches
- **Parametric primitives in a DCC tool** (3D-GPT and LL3M drive Blender's primitives, modifiers and geometry nodes). Rich, but the output is a mesh whose structure the agent cannot query, and each generator has its own conventions.
- **Learned part generators.** OmniPart (arXiv 2507.06165) plans part boxes autoregressively, then synthesises all parts in one rectified-flow pass; FullPart (arXiv 2510.26140) generates each part in its own full-resolution grid and aligns them with a centre-point encoding. Both show that giving each part its own resolution matters for small parts.
- **Graph nodes with retries.** ShapeCraft (arXiv 2510.17603) runs several attempts per part node and keeps the best.

## Our approach
Every generator implements one interface (described here in words; the Rust trait is written in M1 from this note):
- **Parameter schema**: named, typed, unit-bearing parameters with hard limits and the style-relative default ranges.
- **Field builder**: emits the part's signed distance field as nodes of our SDF graph (note 05), in the part's local frame, never a mesh. Large relief layers are added on top later (note 12).
- **Anchors**: named points and frames on the part (top centre, hub, mount points along a sweep), each given analytically from parameters, which the constraint solver uses (note 06).
- **Bounds**: a guaranteed bounding box and a Lipschitz bound for the field, used by the mesher, the search's early culling and the GPU tiling.
- **Natural parameterisation**: how the surface maps to 2D (lathe: angle and profile length; sweep: arc length and perimeter; rounded box: six faces with rounded seams; planks and bricks: per piece), used for UV charts (note 16) and to place relief in real units (note 12).
- **Collision hint**: the simplest proxy that covers the part (box, capsule, cylinder, sphere, or a few-point convex hull), computed from parameters (note 15).
- **Thin-feature flag**: when a part is thinner than the finest tier's cell size, the generator may emit an explicit mesh alongside the field (brief: honest limits).

Generators in M1: rounded box, shell (offset of a field, hollowed), lathe (profile curve revolved), sweep and tube (profile along a curve, with per-length scale and twist), loft (between profiles), panel (thin rounded slab with optional inset), frame (rectangular tubes along a path), plank and brick runs (array along a line or wall with per-piece jitter from the seed), noise rock (blended ellipsoids plus our own value-noise displacement, deterministic), branching tree (recursive sweeps with our own branching rules and leaf blobs), blended body (smooth union of ellipsoids and capsules with a skeleton of named anchors).

**External services behind the same interface (optional, off by default).** An external generator receives the part's box, anchors and a text or image prompt, returns a mesh, and we convert it into a field (signed distance by our own mesh-to-field step) clipped to the part box, so the rest of the pipeline (constraints, relief, meshing, LODs, collision) is unchanged. It never sees the rest of the model, so it cannot break the graph. Licences: see plan-baselines.md; OmniPart contains code under the NVIDIA Source Code License and TRELLIS.2 depends on NVIDIA libraries with non-commercial terms, so both stay optional plug-ins until Chris decides (plan-decisions.md, D8).

## Why it should be better
- Fields, not meshes, from every generator: parts blend, cut and relief-layer without boolean failures (note 05), and the mesher sees exact distances and gradients.
- Anchors and bounds are analytic, so placing and checking parts is exact maths, not mesh measurement.
- Per-part resolution comes for free: each part's natural size and importance drive its mesh resolution (FullPart's insight, achieved without training).

## Baseline and measure
- Per generator: compile time, field evaluations per second on CPU and GPU, Lipschitz bound tightness (how far sphere-tracing steps fall short of the true distance), meshes watertight and manifold (test A).
- Against Blender primitives in baseline F: tokens and calls needed to express the same part.

## Risks
- **Lofts and sweeps with tight bends** give fields that are not true distances (the bound is loose); sphere tracing slows and blends shift. Mitigation: per-generator Lipschitz bounds and a correction factor; measured in M1.
- **Mesh-to-field conversion of external meshes** may fail on open, non-manifold outputs (TRELLIS.2 says it handles open surfaces). Mitigation: generalised winding number style inside test, with an unsigned fallback; external parts are flagged in scorecards.

## Sources (ideas only)
OmniPart arXiv 2507.06165; FullPart arXiv 2510.26140; ShapeCraft arXiv 2510.17603; 3D-GPT arXiv 2310.12945; LL3M arXiv 2508.08228; TRELLIS.2 README (hardware and output description only).
