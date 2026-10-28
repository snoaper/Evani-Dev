# 02 Style profiles and calibration

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 2)
Per look: adjectives mapped to parameter ranges, "more / less" mapped to step sizes, plus bevel and proportion rules. A calibration tool renders sweeps on a contact sheet, a critic proposes the mapping, Chris approves it once per style. `toon_chunky` first, using catalog/looks.json for style descriptions.

## Flag: `toon_chunky` is not a look in catalog/looks.json
The catalog has `toon` (hard-edged bands, ink outlines, saturated palette), `chibi`, `cozy` and others, but no `toon_chunky`. The kart racer's `evani.toml` uses `look = "toon"`. We do not change the catalog. Proposal (decision D6 in plan-decisions.md): a modeling style profile is a separate object that names its parent look, so `toon_chunky` is "the toon look, with chunky proportions" and lives in Evani.Model's own data (`crates/evani-model/data/styles/toon_chunky.json`), not in catalog/looks.json. A profile may only name a look from the catalog.

## Known approaches
- **Text-conditioned generators** (TRELLIS.2, OmniPart arXiv 2507.06165) take style from the prompt or a reference image; there is no explicit, inspectable mapping from a word to a measure, and "a bit more" is not a defined step.
- **Procedural generators with exposed parameters** (3D-GPT arXiv 2310.12945 driving Infinigen-style generators) let a language model pick parameter values directly; the model guesses what "chunky" means in numbers each time.
- **Embedding scores** (SigLIP 2, arXiv 2502.14786) can rank renders against a phrase, which helps choose, but they are not calibrated per style and they move when the encoder changes.

## Our approach
A profile is data with five parts:
1. **Parent look** (`look: toon`) and a short description taken from the catalog entry, used in sheets and critic packets.
2. **Adjective table.** For each class, part and measure, each adjective maps to a range in the measure's own unit, normalised to the class's size (for example, `chassis.stance: low` → ride height 0.08 to 0.14 of overall length; `rear_tyres.size: oversized` → rear radius 1.25 to 1.5 times the front). Measures are named, physical and checkable (note 10 verifies edits against them).
3. **Steps.** "More X" and "less X" move a measure by a step in a perceptual unit: one step is the smallest change the critic and Chris called "clearly different" during calibration; "much more" is two steps; "slightly" is half a step.
4. **Rules.** Bevel radius as a fraction of the smallest part dimension (chunky: large, no hard edges thinner than a minimum), minimum feature thickness per tier, proportion rules (head-to-body for characters from the `chibi` look's 1:1.5 to 1:2; wheel-to-body ratios for vehicles), and silhouette rules (few large masses, readable from the chase camera).
5. **Provenance.** Who proposed each range, the contact sheet hash it was judged on, and Chris's approval stamp. An unapproved profile works but every scorecard says "uncalibrated".

**Calibration tool (`model.calibrate_style`).** For one adjective at a time: sweep its measure over a wide range on 2 or 3 class examples, render one contact sheet (silhouette plus raking light, labelled only with numbers; drawn by our own renderer, note 21, so its hash is the same on every machine), have the critic (a stronger model than the builder) propose the range for each adjective and the step size, then show Chris one combined sheet with the proposed ranges boxed. Chris approves or nudges once per style. Sweeps run through the same search engine, so a full calibration of toon_chunky (about 40 adjectives) is about 40 sheets and one approval session.

## Why it should be better
- Words become reproducible numbers per style, so "chunkier" means the same on every machine and every day, and edit verification (note 10) can check the measure actually moved.
- Steps are perceptual, so one "more" is visible but never wild, which removes most of the back-and-forth LL3M reports for edits.
- One approval per style, not per model, makes Chris's time the scarce resource we spend least.

## Baseline and measure
- Ablation G "word calibration off": adjectives map to evenly spaced default ranges; compare edit success rate (target ≥ 95% move the right measure in the right direction, nothing else beyond tolerance) and calls per finished model.
- Agreement: on held-out sweeps, how often Chris and the critic call two steps "clearly different" and one half-step "slight".

## Risks
- **Calibration cost** if every class needs its own table. Mitigation: ranges are relative to class size, and most adjectives are shared across classes through generic measures (roundness, taper, stance, bulk).
- **Critic bias** in the proposed ranges. Mitigation: Chris approves on the sheet; ranges are data and can be nudged.
- **Look drift**: if the toon look's shading changes, readable relief thresholds move (note 12). The profile records the look version it was calibrated on.

## Sources (ideas only)
catalog/looks.json (toon, chibi, cozy entries); OmniPart arXiv 2507.06165; 3D-GPT arXiv 2310.12945; SigLIP 2 arXiv 2502.14786.
