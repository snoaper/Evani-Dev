# 11 Critic loop

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 9)
The critic is a different, stronger model than the builder; refiners get exact numeric discrepancies; after 2 failed rounds, rethink the construction. Spatial correctness is always decided by maths tests.

## Known approaches
- **Stronger judge.** CADSmith (arXiv 2603.26512) uses Claude Opus as the judge over a Sonnet builder to avoid the builder confirming itself; its judge cross-references three rendered views with kernel measurements and the code. It still passed a model with gaps at joints that the fixed views did not show, and suggests adaptive views and crops of joints as future work.
- **Critic and verifier agents on renders.** LL3M (arXiv 2508.08228) has a critic propose fixes from images and a verifier check them; vision models struggle with spatial artefacts.
- **Plan-execute-critic on a shared language.** aDSL (arXiv 2608.17975) critiques by checking the same relations the plan declared, through visual and code-level inspection; about 4.25 rounds to converge.

## Our approach
- **Roles.** Evani.Model does not run a language model itself: it is agent-agnostic (brief, hardware adaptivity 7). It produces a **critic packet** that any critic model can read, and accepts the critic's verdict as a structured reply. In Evani.App the builder is the worker agent and the critic is a stronger model (by default Opus at extra-high effort when the builder is Opus at high or a lighter model), run through the orchestrator's existing review step.
- **What the critic sees (small).** The intent, the scorecard, the list of failed or borderline checks with exact numbers ("left rear tyre to chassis: 4 mm at 30 degrees steer, limit 10 mm"), the measures furthest from their style targets, and at most one combined sheet image (blueprint views with the problem regions outlined, note 21) when asked. Never raw vertices.
- **What the critic may decide.** Taste and intent fit only: "does this read as a chunky toon kart", "is the nose blunt enough", "which of the three to keep". It cannot overrule a maths test: a must that fails is a fail even if it looks right, and a must that passes is a pass even if the critic is unsure.
- **Refiner input is numeric.** The critic's points are translated into intent diffs (note 10) where they name adjectives, or into the exact discrepancy list where they concern tests; the builder never gets "make it look better" without a number or a word from the vocabulary.
- **Rethink after two failed rounds.** If two rounds of edits fail to fix the same issue, the next reply changes strategy instead of nudging: it proposes a different construction from the class template (another generator for the failing part, a different decomposition, dropping a relation that over-constrains), or asks for a missing reference. The round counter is per issue and lives in the version history.
- **Adaptive inspection by maths.** Where CADSmith suggested adaptive views of joints, we test every joint directly: contact and clearance at each relationship (note 06), thickness and connectivity everywhere (note 13). Pictures are for taste.

## Why it should be better
- The critic's attention goes only where maths cannot judge (taste), and its input is a few hundred tokens, so a stronger model is affordable on every round.
- Spatial errors (CADSmith's gaps) are caught by tests before the critic sees anything.
- The two-round rule stops loops that LL3M and aDSL report (3 to 4 or more rounds).

## Baseline and measure
- Ablation G "stronger critic off": the builder judges itself; measure rounds, calls, tokens and Chris's later rejections.
- Packet size in tokens (target under 600 per round without an image).

## Risks
- **Critic disagreement with Chris's taste.** Mitigation: Chris's approvals feed priors and style calibration; the critic's verdicts are logged and compared with Chris's.
- **Critic cost** if images are always attached. Mitigation: images only when asked (design principle 4).

## Sources (ideas only)
CADSmith arXiv 2603.26512; LL3M arXiv 2508.08228; aDSL arXiv 2608.17975.
