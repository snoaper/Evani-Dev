# STAGES.md change: move PF (fur shells) from Stage 8 into Stage 3

Status: ready to apply (written 2026-10-25). Not applied here: this branch does not carry STAGES.md. The fur-shells builder (package B5-F in `BATCH5_BUILD_PLAN.md`) applies it on its own branch, in the same commit as the first fur-shells work.

## Why

Chris's decision, 2026-10-25, overruling the earlier "keep the painted coat" call that Evani.App made while he was away: **"the decision should have been to get fur not keep painted"**. The horse and animal material batches (5 and 6) get real 3D fur shells. His own instruction is the approval for moving `fur-shells` (PF) from Stage 8 into the current stage, Stage 3. No gate box is ticked by this.

Source text below is the exact wording of `STAGES.md` on `m1-foundation` (`git -C C:\dev\evani-dev show m1-foundation:STAGES.md`), taken 2026-10-25. If the file has moved on, match by the quoted text, not by line number.

## Edit 1: Stage 8 "Includes" (remove the line, leave a pointer)

Find, in `## Stage 8: Unlimited detail` (line 174 on `m1-foundation`):

```
- Render path filed here (CORE_REQUESTS.md, 2026-10-17, from ChimpDuty, sent with Chris's OK): `fur-shells`, instanced fur shells and fins with the baked clump texture as the fallback on low tiers. Proposed, not approved; Chris may move it earlier.
```

Replace with:

```
- Moved to Stage 3 (Chris, 2026-10-25): `fur-shells`. See Stage 3.
```

## Edit 2: Stage 3 "Includes" (add a bullet)

In `## Stage 3: Real content`, add this bullet after the multiplayer bullet that starts "Multiplayer engine build moved into now (Chris, 2026-10-24:" (line 109 on `m1-foundation`), as the last Includes item before `## Stage 4`:

```
- Fur shells (`fur-shells`, PF) moved here from Stage 8 (Chris, 2026-10-25: "the decision should have been to get fur not keep painted"; this overrules the earlier call to keep the painted coat for the horse and animal material batches, which Evani.App made while he was away). Instanced fur shells and fins, an opt-in module: a game that does not list it ships byte for byte as before; the baked clump texture is the fallback on low tiers so every device keeps the same look; the painted coat patterns stay as the strand root colour under the shells; fur length and density per body region use the per-vertex `_EVANI_REGION` chart. Built as package B5-F of the material catalogue's batch 5, before the horse, farm-animal, pet and wild-animal fur rows are finished: docs/materials/BATCH5_BUILD_PLAN.md, docs/materials/PARALLEL_ORDER.md, docs/materials/STAGE_MOVE_PF.md. His request overrides rule 1 for this item only; it stays listed as a Stage 8 look-ahead nowhere else. No gate box is ticked by this.
```

## Edit 3: Stage 3 "Includes" history line (optional, keep the record true)

Line 90 on `m1-foundation` (the 2026-10-17 ChimpDuty entry) says "`fur-shells` for Stage 8". Leave it as written: it is history. Do not edit it.

## Edit 4: CORE_REQUESTS.md

The `fur-shells` row there reads "Stage 8" / proposed. Change its stage to "Stage 3 (Chris, 2026-10-25)" and its status from "proposed" to "approved by Chris: build as B5-F". Match by the row name `fur-shells`.

## Edit 5: the Next step reviews and the gate

- Rule 9 ("No game polish until Stage 8") is unaffected: this is engine work, not game polish.
- Stage 3's Gate list does not change. Add no gate line; if a check is wanted later it needs Chris's sign-off.
- Stage 8's Gate list does not change.

## Check after applying

- `grep -n "fur-shells" STAGES.md` shows the new Stage 3 bullet and the Stage 8 pointer, and the unchanged line-90 history.
- `cargo xtask check` stays clean (a docs-only change).
