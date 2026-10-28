# 08 Refinement: evolution strategy vs differentiable fit

M0 clean-room note, 2026-10-15. Own words from Hansen's tutorial (arXiv 1604.00772, read as maths) and DiffCSG (arXiv 2409.01421). No third-party code opened or copied.

## What it must do (brief, design instruction 7)
Refine the best candidates with our own evolution-strategy optimiser; evaluate a differentiable-rendering fit as an alternative and keep whichever is faster at equal quality.

## Known approaches
- **CMA-ES** (Hansen, "The CMA Evolution Strategy: A Tutorial", arXiv 1604.00772). In our words: sample a population from a multivariate normal around a mean; rank the samples; move the mean to a weighted average of the best; adapt the step size by comparing the length of an accumulated search path with its expected length under random selection; adapt the covariance from both that path (rank-one update) and the spread of the selected steps (rank-mu update). It is invariant to rotations and scalings of the problem, needs no gradients, handles noisy and non-smooth objectives, and has well-studied default settings for population size and learning rates as functions of the dimension. Restarts with growing populations help on multi-modal problems.
- **Differentiable rendering of CSG** (DiffCSG arXiv 2409.01421): rasterise boolean combinations directly and add special antialiasing at primitive intersections so gradients exist exactly where shapes meet, which is where silhouette errors live. Other inverse-rendering work makes silhouettes differentiable by softening them.

## Our approach
**Primary: our own CMA-ES**, written from the tutorial's equations.
- Starts from each of the top 2 or 3 candidates after the last search rung, in the normalised parameter box, with an initial step size set from the style profile's step (one perceptual step).
- Objective: the same score as the search at the top rung, must tests as hard penalties with a feasibility-first ranking (any feasible beats any infeasible).
- Bounded parameters handled by mapping the box through a smooth periodic transform, so samples never leave the style's ranges.
- Deterministic: its random normals come from the engine's seeded `Pcg32` scheme reimplemented in evani-model (no rand crate), one stream per start keyed by the seed and the start's candidate index, and our own normal sampler; the mean, step-size and covariance updates run on the CPU in a fixed order.
- Ranking without GPU rounding: the update uses the ranks of the population, so ranks are decided by the same rule as the search (note 07): when the GPU scores a generation it returns intervals, and every candidate whose place among the selected best (plus the one just below them) is not certain from the intervals is re-scored exactly on the CPU; ties break on the sample index. At the low levels the whole population is simply scored on the CPU.
- Budget: counted in generations, never in time. The level's schedule (MODEL_API.md 1.8) fixes the number of starts, the population and the generations, with a longer generation count when the search stopped early (note 07). Nothing reads the clock (apart from the call's safety stop, which returns no model; MODEL_API.md 1.8), so refinement gives the same result on every machine for the same level.

**Alternative to evaluate in M2: an analytic silhouette-gradient fit.** Our fields give a shortcut that rasterised meshes do not. For an orthographic silhouette, the outline moves when a parameter changes at a speed equal to the field's derivative with respect to that parameter divided by the field's gradient length, at the outline point. The derivative of IoU with respect to a parameter is then an integral over the outline of that speed, signed by whether the reference is inside or outside there. Our CPU evaluator already computes exact gradients (note 05) and can compute parameter derivatives the same way (forward mode, one extra lane per parameter), so this gradient costs one pass over outline pixels, with no soft rasteriser and no special antialiasing. DiffCSG's insight (gradients live at boundaries and intersections) holds automatically, because the outline of a min/max field passes through intersections.
- Use: Gauss-Newton or Adam-style steps on IoU and proportion terms from the best candidate, with the must tests as projections.
- Deterministic as well: it runs on the CPU path with a fixed step count per level, so it obeys the same rule as CMA-ES.
- Decision rule (brief): run both on the 8 karts and 10 more benchmark intents; keep the one that reaches the same final score in less time per tier; keep the other as a fallback if it wins on a class. The method used is part of the level's schedule (and its version), so switching method changes the schedule version, not the meaning of a recorded result.

## Why it should be better
- CMA-ES needs no gradients and handles hard must tests and non-smooth scores (IoU), so it works for every generator, including subdivision cages and external parts.
- The boundary-speed gradient is exact and cheap because we own the field, which mesh-based differentiable renderers cannot do without a soft approximation.
- Both start from the search's best candidates, not from scratch, so refinement is short.

## Baseline and measure
- Field evaluations and time to reach IoU 0.9 (and the final score) from the same start, per tier; final score at a fixed number of field evaluations (the comparison the schedule needs, since budgets are counted in work, not time).
- Baseline: plain random local search with the same budget; and no refinement at all.

## Risks
- **CMA-ES in 40 to 60 dimensions** needs hundreds of evaluations to adapt its covariance; at CPU-only's level 32 schedule it may barely move. Mitigation: refine only the parameters the scorecard says matter (largest silhouette error per part), separable variant at low budgets.
- **Gradient fit gets stuck** at silhouette topology changes (a gap opening). Mitigation: it is only kept if it wins on the benchmark; CMA-ES stays the default.

## Sources (ideas only)
N. Hansen, "The CMA Evolution Strategy: A Tutorial", arXiv 1604.00772; DiffCSG arXiv 2409.01421.
