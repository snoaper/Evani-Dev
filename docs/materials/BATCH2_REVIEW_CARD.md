# Batch 2 review card (for Chris)

Batch 2 is 98 land and plant rows: 97 drawn, plus the black-ice alias. Everything below was made on branch `b2-main`.

## What to look at

1. **The contact sheets** (all 98 rows, default look, one picture each):
   - `.evani/shots/b2-p10/sheets/sheet-high.png` (High)
   - `.evani/shots/b2-p10/sheets/sheet-low.png` (Low)
   - `.evani/shots/b2-p10/sheets/sheet-styles.png` (10 rows in 5 looks)
2. **The five rows fixed at the last step** (before and after, same camera): `.evani/shots/b2-p10/before-*.png` and `after-*.png`.
   - Scree, railway ballast, cinders and gravel road are now angular stone (new ground variant `crushed`).
   - The dead snag is now long grey grain running along the trunk (it was rings running round the trunk, which looked like stacked plates).
   - Beech is a darker, warmer grey with faint bands.
3. **In the viewer:** open the Library, Land, then Gravel, and pick Scree; open Plants, then Bark, and pick Dead tree.

## What to judge

- Does each row look like the thing it is named after, from the distance a game camera would show it?
- Rows that matter most: scree, ballast, cinders (angular?), dead snag (weathered wood?), beech (smooth grey?), moss, the three turfs.
- Foliage rows show the leaf on a flat card; the real shape comes from the game's own leaf model.

## What this card does not show (known limits)

- **Looks other than the default only show colour, not detail,** until Stage 8 moves the look treatments into the engine. The styles sheet is for judging colour and value only.
- **Terrain and herd cost on a full screen is not measured.** The real-GPU numbers are for small scenes (see `DECISIONS.md`, B2-P9 and B2-P10).
- **Foliage is 313 B over its added-size line** (1,513 B against 1.2 KB in a full build). No look-neutral trim exists; every feature costs 5 to 60 B and dropping all of them is still over. Chris to accept or to cut a feature (see `DECISIONS.md`).
- **Ground is about 20 B over its 2.0 KB line** (2,018 B standalone) after adding the `crushed` variant.
- The viewer's quick crop shows the viewer's own toolbar at the top of some thumbnails.

## Checks that passed

- `cargo xtask check`: `evani check: clean`.
- The 8 shipped games, built with `EVANI_GIT_COMMIT=65e8497` and rustc 1.99.1: all 16 files identical to the batch-1 baseline.
- Tests and `clippy -D warnings` (native and wasm) for the recipes crate, the viewer and the sandbox proof.
