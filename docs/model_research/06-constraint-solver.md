# 06 Constraint solver

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 6)
Named anchors on every part; relationships `rests_on`, `flush_with`, `attached_at`, `centred_on`, `aligned`, `mirrored`, `array_along`, solved analytically where possible, otherwise by our own nonlinear least squares. Must tests become constraints where possible, verified checks otherwise.

## Known approaches
- **Relational layout in a modeling language.** aDSL (arXiv 2608.17975) gives agents operators to align centres or anchors, place along an axis with a gap, stack, distribute, and lay out on grids or circles; the critic then checks those relations after running the program. Their ablation shows these helpers cut rounds.
- **Bounding-box layouts.** OmniPart (arXiv 2507.06165) and FullPart (arXiv 2510.26140) place parts as boxes predicted by a network; contact between parts is learned, not guaranteed.
- **Exact validation after the fact.** CADSmith (arXiv 2603.26512) measures bounding box, volume, validity, face and edge counts and centre of mass with a CAD kernel, plus a vision judge. Its own failure analysis: a quadcopter frame passed every check with F1 0.963 and IoU 0.985 yet had small gaps between the arms and the hub that three fixed views missed.
- **Geometric constraint solving in CAD** decomposes the constraint graph into small rigid clusters solved in closed form, with a numeric solver (Newton or Levenberg-Marquardt) for the rest.

## Our approach
- **Anchors are frames.** Every anchor is a point with an orientation and, where it makes sense, a contact patch (top face of a box, the tread line of a tyre, the seat pan). Generators give them analytically from parameters (note 04).
- **Constraint graph by kind.**
  - Analytic, solved in order: `centred_on` (copy a coordinate), `aligned` (copy an axis), `mirrored` (a domain mirror, so the twin is the same part, not a copy that can drift), `attached_at` (anchor to anchor, rigid transform), `array_along` (count and spacing along a line or a sweep's curve, by arc length).
  - Surface-aware: `rests_on` and `flush_with` are solved against the actual field, not the box: the part is moved along the stated axis until its contact patch touches the other part's surface, found by a few exact field queries (a short sphere trace from each contact sample along the axis, then the minimum). This is the gap CADSmith's boxes and views could not see, closed by construction.
  - Numeric: anything left (cycles, inequality constraints such as clearances, "fit inside" boxes) goes to our own damped Gauss-Newton (Levenberg-Marquardt) over the free part parameters, with hard bounds by projection and a fixed iteration order and count so it is deterministic.
- **Solve order.** The relationship graph is split into strongly connected groups; acyclic groups are solved analytically in dependency order, cyclic groups numerically, so most models never touch the numeric solver.
- **Must tests to constraints.** Each must kind declares whether it can be a constraint (`fit_box` scales or clamps parameters; `seats: <body>` places the body at the seat anchor and becomes a clearance inequality) or only a check (`wheels_clear_chassis_at_steer: 30 deg` sweeps the wheel's field through the steering arc and tests the minimum distance to the chassis). Checks are hard pass or fail and decided by field maths: intersection of two fields is tested by evaluating the larger of the two distances over the region and the shared anchors; contact is "the union is one connected solid at the joint", tested at the anchor with a tolerance smaller than the finest tier's cell.
- **Connectivity as a default must.** Every model must be one solid unless a part is marked loose; tested by a flood fill over the octree cells and by the contact tests at every `attached_at` and `rests_on`.
- **Blend-aware.** Smooth unions add material at joins, so contact targets allow for the blend width declared on the edge.

## Why it should be better
- Contact and clearance are exact (field queries, not boxes or pictures), so CADSmith's "tiny gaps at joints" failure cannot pass.
- Most relations are closed-form, so a solve is microseconds and the search can re-solve every candidate.
- Mirrors and arrays are domain operations, so symmetry is exact, not approximately maintained.

## Baseline and measure
- Unit tests with passing and failing cases for every relationship and must kind (test A); seeded failure cases include a 1 mm gap at a joint, a wheel touching the chassis at 29 degrees and a seat 2 cm too narrow.
- Solve time per candidate (target: under 0.1 ms analytic, under 2 ms with numeric groups) and numeric solver convergence rate.
- Baseline F models: how many have gaps or overlaps at joints that our checks catch.

## Risks
- **Over-constrained intents** (a fixed box plus a fixed part size that cannot fit). Mitigation: the solver reports the smallest conflicting set of constraints in words, with the numbers.
- **Non-smooth constraints** (min distance over a sweep) slow the numeric solver. Mitigation: treat them as checks first; only promote to constraints when the template says it helps.

## Sources (ideas only)
aDSL arXiv 2608.17975; OmniPart arXiv 2507.06165; FullPart arXiv 2510.26140; CADSmith arXiv 2603.26512 (failure analysis, figure 6); general CAD constraint-solving literature (graph decomposition plus Levenberg-Marquardt, Moré 1978 for the LM method).
