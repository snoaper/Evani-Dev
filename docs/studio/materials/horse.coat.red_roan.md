# Horse coat, red roan (`horse.coat.red_roan`): material pass 1

Queue row 5 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Bright Toon (WK01,
the studio's default style), Voxel (WK08) and Cinematic Realism (WK24), at
Ultra (desktop) and High (the flagship-phone tier), headless and muted with
`tools/studio/material-shots.mjs` (before: the live preview; after: a private
build of this branch on port 8125, as 8124 was taken).

## What a red roan looks like

A chestnut coat with white hairs mixed evenly through the body, so the body
reads as a soft strawberry or pinkish chestnut from a few metres and close up
you see single pale hairs in red hair. The head, the lower legs, the mane and
the tail stay solid chestnut, a step darker than the body: that dark frame
round a lighter body is what tells a roan from a plain chestnut. Short sleek
coat with a warm satin shine. No patches or spots.

## Before: the flaws (strict)

1. **Barcode, not hair.** Pattern contrast 0.75 with stark white hairs
   (`f2ece6`) at pattern scale 40: dense white dashes in rows over the whole
   body, worst on the sphere and in Realism (white rain on a salmon coat).
2. **Roan legs, mane and tail.** `points` 0 and no way to say otherwise: the
   legs, mane and tail were as roaned as the body, so the horse had no dark
   frame and read as a speckled brick-red horse. With `points` above 0 the
   points would have gone white (they pull toward the last colour, the white
   hair), the opposite of a roan.
3. **Blotches.** `clump` 0.15 made darker lock patches and tiles (sphere,
   Voxel).
4. **Too orange and light in Realism**: `74281a` / `9a4426` came out a flat
   salmon, not chestnut.
5. Voxel: white blocks in clusters (blotches times voxels).

Not this material (noted for their own rows / styles): the sphere's uv seam
band; the pale flat patches on the horse's shoulder and neck edge, where the
uv is stretched and the hairs fade to their average (shared by every roan);
the pale halo round the silhouette in Toon and Realism (the style's sky rim);
the black muzzle and hooves (their own regions).

## After (3 rounds)

Side by side: `.evani/shots/mat05/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png` (headless captures, kept a day; progress shots in the
Viewer's Progress, task `materials`).

- **New row hint `points_to`** (catalogue + studio, no shader change): the
  colour a row's points pull toward, in place of its last colour, and its
  points regions' pattern colour becomes it outright, so a roan's white hairs
  turn dark chestnut on the legs, mane and tail (solid points, no white
  hairs). Like `points_on`, a row with it keeps the recipe's thin-part points
  to its points regions, so the body's thin parts don't streak white. A
  studio hint: not in the 32-byte block (a test proves the block is identical
  with or without it), so no game download changes. `evani-material-recipes`
  refuses anything but an `rrggbb` colour. The bay roan (white legs from
  `points` 0.85 toward white today) and the blue roan will need it on their
  rows.
- **The row**: colours `6a2616` / `8a3a22` / `e4d0ca` (was `74281a` /
  `9a4426` / `f2ece6`), pattern scale 50 (40), pattern contrast 0.42 (0.75),
  mix 0.5 (0.45), clump 0 (0.15), points 0.75 (0), points_to `5a2014`, sheen
  0.6 (0.5), sheen shift 0.25 (0.4).
- **Round 1**: `points_to`, finer softer hairs, no clump: solid dark legs,
  mane and tail, no blotches, but Realism still a light orange with visible
  white dashes. **Round 2**: base a step deeper, hairs pinker and softer
  (contrast 0.42), a little more of them (mix 0.55): the strawberry body.
  **Round 3**: mix 0.5, because above 0.5 the hair share clamps and the
  close coat drew darker than its far average (pale patches where it fades).
- **Result**: close, fine pale hairs through a red coat; mid and far, an even
  strawberry chestnut body framed by solid dark chestnut legs, mane and tail;
  phone High matches Ultra; inside SHADER_BUDGET (no WGSL touched).
- **Art director, what's left**: the head is still roaned, because the studio
  horse has no head region (the head is part of `coat`; uv-space only). When
  the asset gains one it can join the points regions. Voxel shows a soft
  light/dark speckle (one colour per voxel: the style).
- **Shared code**: no shader change. `points_row` and `region_row` change only
  for a row with `points_to` (only this one today); a test pins the bay's
  points colours and that `region_row` returns the bay unchanged, and the
  palomino test still holds, so the bay, dapple grey, steel grey and palomino
  draw exactly as before.
