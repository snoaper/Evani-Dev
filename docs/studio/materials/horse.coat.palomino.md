# Horse coat, palomino (`horse.coat.palomino`): material pass 1

Queue row 4 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Bright Toon (the
studio's default style), Voxel and Cinematic Realism, at Ultra (desktop) and
High (the flagship-phone tier), headless and muted with
`tools/studio/material-shots.mjs` (before: the live preview; after: a private
build of this branch on port 8124).

## What a palomino looks like

A golden coat, from light gold to deep gold (a "new penny" gold at its best),
with a white or flaxen mane and tail. The legs are the same gold as the body,
sometimes a little lighter, never cream stockings unless the horse has white
markings. The coat is short, flat and sleek like a bay's, with a satin
shine. From a distance it reads as one warm gold horse with a pale mane.

## Before: the flaws (strict)

1. **Brush strokes, not hair.** Pattern scale 8 stretched the strand noise to
   cells of about 12 cm on the horse: dark streaks like dry-brush paint or wood
   grain all over the body, worst on the sphere. In Voxel they turn into dark
   lines of cubes; in Realism into a shaggy, streaky long-haired look.
2. **Cream legs.** `points` 0.5 pulled the legs (a `tone: points` region) half
   way to the near-white pattern colour, so the horse wore pale cream stockings
   to the hoof. A palomino's legs are gold.
3. **Mane only half pale.** The same 0.5 left the mane and tail a pale gold,
   not the white or flaxen that defines the colour. The row had no way to make
   the mane white without also making the legs white (one `points` number for
   legs, mane and tail).
4. **Lemon, not gold.** `b08030` / `e0b868` read as a saturated mustard in
   Toon and a lemon yellow in Realism.
5. **Grey-white fuzz rim** from `sheen_shift` 0.6 and a weak sheen (0.6), and
   `clump` 0.05 blotches: the bay's own early flaws.
6. White thin-part streaks: the recipe's curvature points pull thin convex
   parts (backs of the legs, shoulder edges) toward the white pattern colour.

Not this material (noted for their own rows / styles): the sphere's uv seam
band; Voxel's dark step lines (the style's own step shading, same on the bay);
the pale halo round the silhouette in Toon and Realism (the style's sky rim);
the black muzzle and hooves (their own regions).

## After (3 rounds)

Side by side: `.evani/shots/mat04/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png` (headless captures, kept a day; progress shots in the
Viewer's Progress, task `materials`).

- **New row hint `points_on`** (catalogue + studio, no shader change): a row
  may list which of the asset's `tone: points` regions wear its points colour
  (`["mane","tail"]` here). Left out, every points region does, so every other
  row is unchanged. When it is set, the other regions (legs, coat) also draw
  the recipe's thin-part points at 0, so they stay gold instead of streaking
  white. A studio hint like `shape`: not in the 32-byte block (a test proves
  the block is identical with or without it), so no game download changes.
  `evani-material-recipes` validates it (a list of names); the studio's
  catalogue check (`problems`) errors on a name that isn't a `tone: points`
  region of the row's asset, with a fix line, so a typo can't recur on any
  later row (the flaxen chestnut and silver dapple will need it).
- **The row**: colours `a27438` / `c08f52` / `e8dcc0` (was `b08030` / `e0b868`
  / `efe2c4`), pattern scale 40 (8), pattern contrast 0 (0.6, unused by
  `solid`), clump 0 (0.05), sheen 0.8 (0.6), sheen shift 0.15 (0.6), points
  0.9 (0.5), `points_on` mane and tail.
- **Round 1**: `points_on`, bay-like sleek settings, a warmer gold: no brush
  strokes, white mane, gold legs, but white streaks down the backs of the legs
  and on the shoulder (the thin-part points) and still a little lemon in
  Realism. **Round 2**: thin-part points off outside `points_on`, gold a step
  deeper. **Round 3**: root and tip closer together (same average), because
  the Toon bands picked out darker flecks on the light coat.
- **Result**: an even deep gold body and legs with a satin highlight, a
  cream-white mane and tail; reads as a palomino from far away in every style;
  phone High matches Ultra; inside SHADER_BUDGET (no WGSL touched).
- **Art director, what's left**: Realism's close sphere is a touch velvety
  (the shared short-coat strand layer; same on the bay, not worth a shared
  shader change for this row). The studio horse's points regions are leg,
  mane and tail only, so ear rims and a darker muzzle skin (common on real
  palominos) can't be shown yet. In a game the recipe's curvature points
  still read `points` 0.9 on the coat itself; a game wearing this row on its
  own body mesh should set its own points.
- **Shared code**: no shader change, so the bay, dapple grey, steel grey and
  tan dog draw exactly as before (they have no `points_on`; a test pins that
  `region_row` returns the bay unchanged on every region).
