# Evani.Materials: running order for batches 4 to 6

Plans: `BATCH4_BUILD_PLAN.md` (people, clothing), `BATCH5_BUILD_PLAN.md` (horses, livestock, pets), `BATCH6_BUILD_PLAN.md` (wild mammals, hides, horn and bone). Written 2026-10-25.

**Limit:** at most **3 to 4 Rust builds at once** across all three batches (the PC has 16 cores and is shared). Each build runs with `CARGO_BUILD_JOBS=2` through `node "<Evani.App tools folder>/run-low.mjs"`, after `resource_check`, and waits if the PC is over 80%. **A 16-file byte proof (8 ship builds) runs alone.** Research sessions and data-only edits don't count as builds. Each worktree's `target/` is several GB: check free disk before making a fourth.

## 1. Wave 1: can start now, side by side, each from the `matcat-engine` tip

| Package | Builds? | Needs | Worker |
|---|---|---|---|
| **B5-P1** fur variants: roan, merle, mottle (tortoiseshell, calico), crimp (fleece, curls), reticulated (giraffe), number `mix`; serves batches 5 and 6 | yes | `matcat-engine` only; owns `rc_fur.wgsl` | opus-high |
| **B5-P2** hide `bristles` and `plates`; keratin `hoof`, `scutes`, `banded`; serves batches 5 and 6 | yes | `matcat-engine` only; owns `rc_hide.wgsl`, `rc_keratin.wgsl` | sonnet-high |
| **B4-P1** eye extension in the character family: flecks, cloud, glass eye, pupil tint, bar and slit pupils; record packing | yes | **the tier-switch fix committed first**: at the time of writing `matcat-engine` has uncommitted edits to `character.wgsl` and DECISIONS from that work, and this package edits the same file | opus-high |
| **B4-P2** leather `embossed` variant and `crazed` extra | yes | `matcat-engine` only; owns `rc_leather.wgsl` | sonnet-high |
| **B4-R1** research: skin and hair through the recipe stage with the character family (nine questions, BATCH4 3.4) | no | nothing | opus-high |
| **B5-R1** research: the body-region mask (ten questions, BATCH5 3.3, plus BATCH6 3.3's four) | no (one probe at most) | nothing | opus-high |

That is **four builds and two research sessions**. If the PC is busy, start B5-P1, B5-P2 and B4-P2 first and B4-P1 when the tier-switch fix is in and a slot frees.

**Wave 1b, as build slots free** (data and captures, one build each, still from `matcat-engine`): **B4-N1** (56 rows), **B5-N1** (50 rows), **B6-N1** (40 rows). They touch only their rows in `materials.json`, so they merge cleanly with everything above. That is 146 rows drawn before any new engine piece lands.

None of wave 1 touches a file that batch 3 changed (leather, fur, hide, keratin, the character family, catalogue rows of batches 4 to 6), so it merges into the joined tree with add-beside conflicts only (`COMPILED`, `ALL_RECIPES`, `EXTRA_NAMES`, `recipes.json`; the extras numbering is regenerated at integration).

## 2. Waits for batch 3 to join `matcat-engine`

`b3-patterns` is complete (90 of 90 rows, size trim done) but not joined; `matcat-engine` is not in its history past 378f9b02.

- **B4-P0** and **B5-P0**: make `matcat-b4` and `matcat-animals` from the joined tree, merge the wave-1 branches, and **rebuild the byte-proof baseline** (`sha-base-b456.txt`): `matcat-engine` now carries the sun-bake fix, so kart-racer and sandbox wasm differ from the old `sha-base.txt` by exactly +590 B raw (+356 and +313 gzip). Record that delta once; every later proof is 16/16 against the new base.
- **B4-P3a then B4-P3b** cloth: the `sett` (checks, tartan, houndstooth, stripes), `slubs`, `felt`, `open` and `herringbone` additions to batch 3's cloth recipe stage, then cloth's sheen into the recipe stage (drops the 7.2 KB layers family for cloth games; 16-file proof).
- **B4-P2b** leather `carved` (batch 3's layout motif), optional.
- **B6-N3** (rhino, walrus, armadillo, turtle numbers) as soon as B5-P0 has merged B5-P2.

## 3. Waits for research

- **After B4-R1:** **B4-P4** character + recipe composition (evani-render, 16-file proof), then **B4-P5** skin and **B4-P6** hair recipe stages in parallel; then B4-P7 integration, B4-N2a/N2b numbers and B4-P8 viewer in parallel, B4-P9 final.
- **After B5-R1 and B5-P1:** **B5-F** fur shells (Chris moved PF into Stage 3 on 2026-10-25; opt-in; its fur-shells research report comes first; per-region length and density use the chosen per-vertex `_EVANI_REGION` chart, and the painted coat from B5-P1 is the strand root colour) runs beside P3 and P4, its region-driven part after P3 merges. **B5-P3** region mask (engine, converter, check; 16-file proof) and **B5-P4** code-built quadruped in parallel; then B5-P5 integration; then B5-N2a, B5-N2b, B5-P6 (GPU) and **B6-P1** body shapes (after B5-P4) in parallel; then B6-N2 region coats; finals B5-P7, then B6-P2.

**Now waiting for B5-F (added 2026-10-25):** B5-N2a (horses), B5-N2b (livestock, dogs and cats), B5-P6 (GPU: the herd with shells), B5-P7 (final check and review card), B6-N2 (wild region coats), the furred rows of B6-N1 and B6-N3 (finished with shells), and B6-P2 (batch 6 final). **Not waiting:** B5-R1, B5-P1, B5-P2, B5-P3, B5-P4, B5-N1, everything in batch 4, and the hide, horn, scale and eye rows. B5-F's own place: after B5-R1 (the chart) and B5-P1 (the root colour), beside P3 and P4, and merged by B5-P5 if ready, otherwise directly after P5 and before N2a and N2b.

## 4. Order of byte proofs (each alone)

B4-P0 baseline → B4-P3b → B4-P4 → B5-P3 → B4-P7 → B5-P5 → finals B4-P9, B5-P7, B6-P2. Recipe-only, family-only and data packages prove with `git diff --stat`, the `Cargo.lock` diff and a `cargo tree` check instead.

## 5. Row totals

| | Rows | Drawn at start | Set now (wave 1b) | After new parts | Catalogue drawn after the batch |
|---|---|---|---|---|---|
| before batch 4 | | | | | 402 of 1,059 (batches 1 to 3, counted on `b3-patterns`) |
| batch 4 | 120 | 3 | 56 | 61 | 519 |
| batch 5 | 95 | 0 | 50 | 45 | 614 |
| batch 6 | 74 | 7 | 40 | 27 | 681 (64%) |
