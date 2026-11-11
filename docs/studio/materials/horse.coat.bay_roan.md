# Horse coat, bay roan (`horse.coat.bay_roan`): material pass 1

Queue row 7 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Bright Toon (WK01,
the studio's default style), Voxel (WK08) and Cinematic Realism (WK24), at
Ultra (desktop) and High (the flagship-phone tier), headless and muted with
`tools/studio/material-shots.mjs` (before: the live preview; after: a private
build of this branch on port 8125). First coat done on the studio with model
parts (Coat, Mane, Tail, Hooves, Muzzle, Eyes).

## What a bay roan looks like

A bay coat (red-brown body, black points) with white hairs mixed evenly
through the body, so the body reads as a frosted, rosy or mauve red-brown from
a few metres, and close up you see single pale hairs in red hair. The lower
legs, the mane and the tail stay solid black, as on a bay; the head stays
darker and less roaned. Short sleek coat with a soft satin shine. No patches
or spots.

## Before: the flaws (strict)

1. **White legs, mane and tail: the opposite of a bay roan.** `points` 0.85
   pulled the points toward the row's last colour, the white hair (`ece2d6`),
   so the horse had white stockings to the elbow and a white mane and tail.
   The one thing a bay roan must show, the black points, was missing.
2. **Barcode, not hair.** Pattern contrast 0.95 with near-white hairs at
   pattern scale 40: rows of stark white dashes over the body and the sphere
   (white rain in Realism).
3. **Too few hairs to read as roan.** `mix` 0.12: from mid and far the body
   was a plain dark chestnut with a few white flecks.
4. **Blotches.** `clump` 0.15: white blotches and tiles in Voxel.

Not this material (shared by every roan, noted in the red and blue roan
passes): the pale flat patches where the horse's uv is stretched (shoulder,
neck edge); the pale sky-rim halo in Toon and Realism; the sphere's uv seam
band; the head, which stays roaned because the horse has no head part.

## Parts: how the points land

The studio now builds the horse from parts. The Coat part covers the body,
legs and ears; Mane and Tail are their own parts that follow the coat in its
points tone, and the legs are points-tone regions of the Coat. So one coat
row still sets all of them: `points_to` `16120f` (near black) makes every
points region (legs, mane, tail) solid black with no white hairs, and
`region_row` drops the thin-part points on the body, so the body is not
darkened. A user who gives the mane or tail its own hair breaks that follow,
as the parts contract says. No part or asset data changed.

## After (2 rounds)

Side by side: `.evani/shots/mat07/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png` (headless captures, kept a day; progress shot in the
Viewer's Progress, task `materials`).

- **The row**: colours `46200f` / `683020` / `d0c4bc` (was `3c1c10` /
  `6a3420` / `ece2d6`), pattern scale 50 (40), pattern contrast 0.46 (0.95),
  mix 0.56 (0.12), clump 0 (0.15), points 0.8 (0.85), `points_to` `16120f`
  (new on this row; the red roan pass's hint), sheen 0.4 (0.5), sheen shift
  0.3 (0.4).
- **Round 1**: `points_to` near black, softer, greyer hairs (contrast 0.42),
  scale 50, mix 0.45, no clump: solid black legs, mane and tail at once, no
  barcode or blotches; but the body read as a plain bay, too little frost at
  mid and far, and Realism far went salmon.
- **Round 2**: a deeper red-brown base and more hairs (mix 0.56, contrast
  0.46): a frosted rosy red-brown body framed by solid black legs, mane and
  tail in Toon, Voxel and Realism; phone High matches Ultra.
- **Art director, what's left**: close up on the horse the pale hairs still
  line up along the flow where the uv is stretched (every roan); Realism far
  reads a soft mauve-brown, which is in range for a bay roan; Voxel shows a
  light/dark speckle (one colour per voxel: the style); the head is roaned (no
  head part yet). Stopped after round 2: a third round would only trade one
  of these for another.
- **Shared code**: none touched. Catalogue row only; no WGSL or Rust change,
  so the six earlier coats draw exactly as before. Inside SHADER_BUDGET; the
  32-byte block is the same size.
