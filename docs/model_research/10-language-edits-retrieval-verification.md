# 10 Language edits, retrieval and edit verification

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 8 and test C)
Adjective deltas through the style profile; analogies resolved against the Evani Library; a retrieval index over our docs, verbs, patterns and approved recipes, queried before writing intent; after every edit, verification that the requested change happened and nothing else moved beyond tolerance; versioned, undoable edits; parts cached by content hash. Target: at least 95% of adjective edits move the target measure correctly with nothing else beyond tolerance; "like sugarpeak_kart but a truck cab" works in one call (M3).

## Known approaches
- **Retrieval-augmented code writing.** LL3M (arXiv 2508.08228) built a retrieval base from Blender's API documentation: total errors fell by 26% and use of advanced operations rose about fivefold (5.86 vs 1.20 per model). CADSmith (arXiv 2603.26512) also retrieves API documentation instead of fine-tuning.
- **Edit verification by a vision model.** LL3M's verification agent re-renders and asks a vision-language model whether each critique was addressed. About 59% of user edits succeeded with one instruction; the rest needed 3 or 4 follow-ups; the authors note vision models struggle with spatial artefacts and positioning.
- **Relational edits.** aDSL (arXiv 2608.17975) repairs relation violations by updating the program through the same relational operators the plan used.

## Our approach
- **Edits are intent diffs in words.** `model.edit` accepts short phrases ("wider stance", "rear tyres much more oversized", "nose less blunt", "remove the wing", "exhausts: 4") and turns each into one change of the intent: an adjective step on a named measure (via the style profile's step table), a number, an added or removed part. The parsed diff is echoed back so the agent sees exactly what was understood.
- **Local re-search.** The edited intent is searched again, but only near the current parameters: a small quasi-random cloud within about one step on the dimensions the edit touches, all other dimensions frozen unless a constraint forces them to move. Its size is an edit level from a small fixed ladder with a published schedule, chosen before the re-search from the tier and the 1 s default budget, and its decisions follow the same "decide by interval, settle on the CPU" rule as the search (notes 07, 18), so an edit gives the same new version on every machine at the same level.
- **Verification by measures, not pictures.**
  1. Before and after, compute the measure vector: every named measure of every part, the overall box, per-view silhouettes, per-part content hashes.
  2. Target check: the edited measure moved in the requested direction by at least the minimum step (half a step for "slightly").
  3. Locality check: every other measure stayed within tolerance (default a quarter step), and parts the edit should not touch kept their content hash. Movement forced by constraints (a wider chassis moving the wheels outwards) is allowed but listed with the reason (which relationship).
  4. Must tests re-run.
  If any check fails, the edit is not applied; the reply says which measure misbehaved and by how much, and offers the nearest working alternative.
- **Versioning and undo.** Every applied edit is a new version: parent version, the intent diff, the chosen parameters, the scorecard and the model hash. Undo is "pick the parent". Versions are cheap because parts are cached by content hash: a part whose generator parameters and relief did not change reuses its compiled field, mesh pieces and bakes.
- **Analogies through the library.** `like: library/sugarpeak_kart` loads that item's intent and recipe as the starting point and prior; "but a truck cab" is resolved by the retrieval index to a part arrangement (a template variant such as `kart/truck_cab` or an approved model that has a cab part), merged into the part graph by role, then searched locally. If retrieval finds nothing above a confidence threshold, the reply lists the three nearest options rather than guessing.
- **Retrieval index (our own, small, lexical).** Indexed: MODEL_API.md sections, every class, part, measure, adjective, pattern and must kind (with one-line meanings and examples), approved recipes (their intents and descriptions). Ranking: our own term-weighting over words and their synonyms (a synonym list per style, which calibration grows), with exact-name hits first. Exposed by `model.describe { query }`, which returns at most a few short snippets. An optional embedding plug-in can re-rank, but lexical is the default because the vocabulary is closed and small.

## Why it should be better
- The edit's success is decided by measures and hashes, which are exact: vision models (LL3M's verifier) cannot reliably see whether a wheel moved 2 cm or whether something else moved.
- Locality is checked, not assumed: "nothing else moved beyond tolerance" is a list of numbers.
- Retrieval works over a closed, typed vocabulary, so most errors LL3M's retrieval reduces cannot happen at all, and what remains is answered in one short reply.
- Content-hash caching makes an edit cost only the parts it touches, which is how the 1 s edit budget is met on Lite and CPU-only.

## Baseline and measure
- Edit benchmark: for each of the 30 intents, 6 scripted edits (2 adjective steps, 1 number, 1 remove, 1 analogy, 1 "slightly") = 180 edits; success rate (target ≥ 95%), time per edit per tier (under 1 s Pro and Standard, under 2 s Lite and CPU-only).
- Ablations G: retrieval off (the agent writes intents without `model.describe`), edit verification off (edits applied without checks); measure errors, calls and wrong edits.

## Risks
- **Coupled measures** (wider chassis needs wider track): the locality check must know allowed couplings. Mitigation: couplings come from the constraint graph, so they are explained, not hidden.
- **Analogy coverage** depends on library content. Mitigation: template variants authored for common analogies in M3; misses are logged.

## Sources (ideas only)
LL3M arXiv 2508.08228; CADSmith arXiv 2603.26512; aDSL arXiv 2608.17975.
