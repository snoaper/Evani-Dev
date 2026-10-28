# 09 Learned priors

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 7 and test D)
Each approved model shifts the sampling distribution for its class and style, so fewer candidates are needed over time; test D reports candidates needed to reach the quality target falling as priors grow, across the benchmark run order.

## Known approaches
- **Learned generators** (OmniPart, FullPart, TRELLIS.2) carry priors in network weights trained on large datasets; they cannot be updated by one approval and are not inspectable.
- **Bayesian optimisation and estimation-of-distribution methods** fit a model of where good parameters are and sample from it; CMA-ES itself is a self-adapting normal distribution (Hansen arXiv 1604.00772).
- **Retrieval of exemplars** (LL3M's retrieval of documentation examples) reuses past successes as context rather than as a distribution.

## Our approach
- **Per (class, style) prior file**, data only, in the library folder: the normalised parameter vectors of approved models (and, with lower weight, models Chris saw but did not reject), with their intents' adjectives.
- **Conditional on adjectives.** A prior entry counts only for intents whose adjectives are compatible (an approved low, wide kart says nothing about a tall, narrow one). Compatibility is by the measures the adjectives constrain.
- **Shape of the prior.** A small mixture of normals in the normalised box (one per approved cluster), with a floor of uniform mass: a fixed share of candidates (default 30%) always comes from the plain quasi-random sequence so the search never collapses onto past answers.
- **Sampling stays deterministic and prefix-stable.** Quasi-random points are warped through the mixture's inverse distribution per dimension (warping each dimension on its own means each mixture component has a diagonal covariance: no correlations between measures within a component; correlated measures are covered by having several components) (with the mixture component chosen by a deterministic stratified rule), so the same prior file, intent and seed give the same candidates. The prior file's content hash is part of the recipe and the scorecard.
- **Cold start.** Before any approvals, the class template's default ranges are the prior. The 8 karts are run first in the benchmark so later karts and the kart-like classes benefit (test D records this order).
- **Benchmark runs (decision D18).** Nobody approves models during an automated run, so test D could not show priors growing. In benchmark runs only, a model whose top candidate meets the benchmark bar (every must passes, gate IoU 0.9 or better in every reference view, mesh valid; plan-baselines.md section 4) counts as approved *for that run*: it is added, with the weight of an approval, to a run-local prior file under the run's own folder (`.evani/bench/<run>/priors/`). A benchmark run never reads or writes the real priors in the library folder; it starts from empty priors (or from a prior snapshot named by its hash in the run's settings), so runs are reproducible and Chris's approvals stay his alone. The priors ablation keeps every intent's `like` identical in both arms, and the test runs in two fixed orders (plan-baselines.md section 4).
- **Forgetting.** Entries carry the style profile version; when a style is recalibrated, older entries are down-weighted rather than deleted.

## Why it should be better
- One approval improves the next search at once, with no training run and no GPU time.
- The prior is a small readable file: Chris or an agent can see why the search favours a region and can remove a bad entry.
- Keeping a uniform floor protects variety, which learned generators tend to lose.

## Baseline and measure
- Ablation G "priors off": candidates to target (the smallest level that meets the bar, plan-baselines.md section 4) and time for each benchmark intent, in both run orders, with and without priors, `like` identical in both arms.
- Diversity: average parameter distance between the top 3 returned, with and without priors (must not collapse).

## Risks
- **Too few approvals** per class to matter early. Mitigation: adjective-conditioned sharing across related classes in a family (note 03).
- **Bias towards early taste.** Mitigation: uniform floor, version-aware down-weighting, and the scorecard always says how many candidates came from the prior.

## Sources (ideas only)
Hansen arXiv 1604.00772; LL3M arXiv 2508.08228; OmniPart arXiv 2507.06165; FullPart arXiv 2510.26140.
