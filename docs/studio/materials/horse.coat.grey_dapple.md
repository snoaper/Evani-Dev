# Horse coat, dapple grey (`horse.coat.grey_dapple`): material pass 1

Queue row 2 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Bright Toon (the
studio's default style), Voxel and Cinematic Realism, at Ultra (desktop) and
High (the flagship-phone tier), headless and muted with
`tools/studio/material-shots.mjs` (before: the live preview; after: a private
build of this branch on port 8124).

## What a dapple grey looks like

A mid to light cool grey horse whose body is covered in pale, round dapples
about a hand across, packed close together, each with a soft edge that fades
into a darker grey net between them. The dapples are filled, not hollow
rings; their sizes and outlines vary. The legs, and often the head, are a
darker grey than the body, with fewer or no dapples. The coat is short and
sleek like any summer horse coat, with a soft satin shine.

## Before: the flaws (strict)

1. **Hollow rings, not dapples.** The `dapple` variant drew an annulus round
   every cell point in the light pattern colour: rows of doughnut rings,
   like chain mail or the Olympic rings, all the same width and evenly spaced.
   No real coat looks like that.
2. **Dark body, pale rings.** The coat itself was the dark grey (`5a5c60` to
   `8a8c90`), so the horse read as a dark grey with a ring print, not a light
   dapple grey.
3. **Pale legs.** Points mix toward the pattern colour, which was the light
   ring grey, so the legs went *lighter*: the opposite of a real dapple grey.
4. **Dark scribbles.** `clump` 0.05 left dark lock streaks over neck and
   shoulder at mid range.
5. **Rings too big and too stark** (scale 14, contrast 0.55 on a ring that is
   already 0.8): at a distance the horse looked spotted, not dappled.
6. Voxel: the rings became broken lines of light cubes over a dark body.

Not this material (noted for their own rows / styles): the sphere's uv seam
band; Bright Toon's hard colour bands (the style posterises every soft
gradient); the dark crease lines on the Toon horse's shoulder (style outline).

## After (2 rounds)

Side by side: `.evani/shots/mat02/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png` (headless captures, kept a day; progress shots in the
Viewer's Progress, task `materials`).

- **Fur recipe, dapple variant only** (`rc_fur.wgsl`, variant 5; no other
  catalogue row uses it): the mask is now the *net between* round dapples:
  `smoothstep(rd, rd + 0.26, F1 + wobble)`, dapple radius 0.24-0.38 of a
  pattern cell per cell, the edge wobbled by one slow value noise so no
  dapple is a true circle. Far fade average 0.47 (the net's measured cover,
  was 0.25 for the old ring). One noise lookup more than before, no new
  branch, binding or texture: inside SHADER_BUDGET.
- **The row**: colours `a4a6a8` / `bfc1c3` (pale dapples, was `5a5c60` /
  `8a8c90`) and `5c5e62` (the darker net and the points, was `c4c6c8`);
  pattern scale 18 (14), pattern contrast 0.65 (0.55), clump 0 (0.05),
  points 0.7 (0.3: darker grey legs), sheen 0.6 (0.5), sheen shift 0.3 (0.5).
- **Round 1 to 2**: round 1 (contrast 0.8, 0.18 edge) read as cow or leopard
  spots in Toon; round 2 softened the edge to 0.26 and the net to 0.65.
- **Result**: a light grey horse covered in soft pale dapples in a gentle
  darker net, darker grey legs, no rings and no scribbles; Realism reads as
  a real dapple grey; Voxel is an even pale grey with grey patches; phone
  High matches Ultra; far shots read as a light dapple grey.
- **Art director, what's left**: dapples cover the legs and neck as evenly as
  the barrel (a real horse dapples most on barrel and quarters); placing
  them by body part would need a body-region mask the recipe does not have
  (uv-space only), so it is left for the studio's region work. Toon still
  shows hard-edged dapples because of the style's colour bands.
- **Shared recipe**: only the dapple variant changed and only this row uses
  it, so the bay, palomino and tan dog are untouched (the short-coat rule
  from pass 1 is kept as it was).
