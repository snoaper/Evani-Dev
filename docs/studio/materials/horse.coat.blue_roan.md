# Horse coat, blue roan (`horse.coat.blue_roan`): material pass 1

Queue row 6 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Bright Toon (WK01,
the studio's default style), Voxel (WK08) and Cinematic Realism (WK24), at
Ultra (desktop) and High (the flagship-phone tier), headless and muted with
`tools/studio/material-shots.mjs` (before: the live preview; after: a private
build of this branch on port 8125).

## What a blue roan looks like

A black coat with white hairs mixed evenly through the body, so the body reads
as a slate blue-grey from a few metres, and close up you see single pale hairs
in black hair. The head, the lower legs, the mane and the tail stay solid black:
that black frame round a lighter body is what tells a blue roan from a grey.
Short, sleek coat with a soft, cool shine. No patches or spots.

## Before: the flaws (strict)

1. **A pale powder-blue horse, not black and white hair.** Base `4a5262` /
   `6a7488` is a light periwinkle with no black in it; far away (worst in
   Realism) the horse read as a lilac-white toy.
2. **Barcode, not hair.** Pattern contrast 0.95 with near-white hairs
   (`e4eaf2`) at pattern scale 40: white dashes in rows over the whole body
   and the sphere.
3. **Roan legs, mane and tail.** `points` 0: the legs, mane and tail were as
   roaned and pale as the body, so there was no black frame.
4. **Blotches.** `clump` 0.15 made patches of locks; in Voxel, clusters of
   white blocks.

Not this material (shared by every roan, noted in the red roan pass): the pale,
flat patches on the shoulder and neck edge where the uv is stretched; the pale
halo round the silhouette in Toon and Realism (the style's sky rim); the black
muzzle and hooves (their own regions); the sphere's uv seam band.

## After (3 rounds)

Side by side: `.evani/shots/mat06/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png` (headless captures, kept a day; progress shot in the
Viewer's Progress, task `materials`).

- **The row**: colours `262c38` / `3a4256` / `bcc4d2` (was `4a5262` /
  `6a7488` / `e4eaf2`), pattern scale 50 (40), pattern contrast 0.42 (0.95),
  mix 0.42 (0.2), clump 0 (0.15), points 0.8 (0), `points_to` `15161a` (new on
  this row; the red roan pass's hint), sheen 0.35 (0.3), sheen shift 0.3 (0.4).
- **Round 1**: a dark slate base, `points_to` near black, no clump, contrast
  0.6, mix 0.5: solid black legs, mane and tail at once, but the hairs still
  read as rows of white dashes and Realism far washed out pale lilac.
- **Round 2**: softer, greyer hairs (contrast 0.45, `b8c0ce`), less sheen:
  the sphere became a fine-haired dark slate, but nearly black, and the
  horse's far view was still paler than its mid view.
- **Round 3**: a step bluer base and a few fewer hairs (mix 0.42) so far and
  near agree: an even slate blue-grey body of fine pale hairs, framed by solid
  black legs, mane and tail, in Toon, Voxel and Realism; phone High matches
  Ultra.
- **Art director, what's left**: close up on the horse the pale hairs still
  line up a little along the flow, where the horse's uv is stretched (the same
  for every roan; the sphere shows single hairs). The head is still roaned,
  because the studio horse has no head region yet. Voxel shows a light/dark
  speckle (one colour per voxel: the style).
- **Shared code**: none touched. Catalogue row only; no WGSL or Rust change,
  so the bay, dapple grey, steel grey, palomino and red roan draw exactly as
  before. Inside SHADER_BUDGET; the 32-byte block is the same size.
