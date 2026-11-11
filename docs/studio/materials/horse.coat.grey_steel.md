# Horse coat, steel grey (`horse.coat.grey_steel`): material pass 1

Queue row 3 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Bright Toon (the
studio's default style), Voxel and Cinematic Realism, at Ultra (desktop) and
High (the flagship-phone tier), headless and muted with
`tools/studio/material-shots.mjs` (before: the live preview; after: a private
build of this branch on port 8124).

## What a steel grey looks like

A dark, even, slightly blue iron grey: a fine mix of black and white hairs
all over the body, so close up you see single silver hairs in a dark coat
and from a few metres the horse reads as one smooth dark blue-grey. No
dapples, no patches, legs as dark as the body or darker. Short sleek summer
coat with a cool satin shine. Clearly darker and plainer than a dapple grey.

## Before: the flaws (strict)

1. **Rain dashes, not hairs.** Each white hair of the roan variant was a
   whole pattern cell long (about 4 cm on the horse): long stark white
   streaks over the body, like rain on a window or scratches.
2. **Near black.** The coat (`36363a` / `56565e`) read as a black horse with
   white streaks, not a steel grey, and had no blue in it.
3. **Pale legs.** `points` 0.25 mixes the thin parts toward the pattern
   colour, which is the white hair colour: the legs went lighter than the
   body, the opposite of a real grey.
4. **Blotches.** `clump` 0.15 made patchy darker lock blotches and dark
   tiles (very visible on the sphere).
5. Pattern contrast 0.95 with mix 0.12: few hairs, each pure white, so
   close it looked streaked and far it looked speckled, never an even grey.

Not this material (noted for their own rows / styles): the sphere's uv seam
band; Bright Toon's hard bands; Voxel turns any fine hair mix into a block
speckle (one colour per voxel), which is the style.

## After (3 rounds)

Side by side: `.evani/shots/mat03/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png`, and the other roans: `roans-ba.png` (headless
captures, kept a day; progress shots in the Viewer's Progress, task
`materials`).

- **Fur recipe, roan variant only** (`rc_fur.wgsl`, variant 6): the hair grid
  is 3 across and 2 along per pattern cell (was 2 and 1), so a hair is half
  a cell long and a third as wide: fine single hairs. The cover per cell is
  unchanged, so the far average (`mix * 0.45`) still matches; the smallest
  feature for the anti-aliasing fade is 0.17 cells (was 0.25), so hairs
  thinner than 1.5 px fade into that average on every tier. Same lookups,
  no new branch, binding or texture: inside SHADER_BUDGET. A test now pins
  the grid and the fade size.
- **The row**: colours `2e3138` / `565a64` (dark blue-grey root and tip, was
  `36363a` / `56565e`) and `d2d6dc` (cool silver hairs, was `ecece8`);
  pattern scale 50 (40), pattern contrast 0.6 (0.95), mix 0.35 (0.12),
  clump 0 (0.15), points 0 (0.25), sheen 0.5 (0.3), sheen shift 0.3 (0.4).
- **Round 1** (settings only, old hair grid): right colour, no pale legs,
  no blotches, but still rain dashes; a slider sweep of contrast 0.25-0.4
  and mix 0.3-0.5 could not fix it, which is why the recipe changed.
  **Round 2**: finer hairs; read as real grizzled hair. **Round 3**: base a
  step darker, because far away it read a little light for a steel grey.
- **Result**: close, single fine silver hairs through a dark blue-grey coat;
  mid, a grizzled iron grey; far, an even dark blue-grey; legs as dark as
  the body; phone High matches Ultra; nothing like the light dapple grey.
- **Art director, what's left**: Voxel shows a light/dark block speckle
  (style). Real steel greys often have a darker head and legs; that needs
  the body-region mask the recipe does not have (uv-space only).
- **Shared recipe**: only the roan branch and its fade size changed. The
  blue roan, red roan, bay roan and rabicano use it and now get the same
  finer hairs (shot: blue and red roan look finer, no worse); their own
  colour and settings flaws stay for their rows. The bay, dapple grey,
  palomino and tan dog do not use the roan variant (tan dog: solid); the
  bay, dapple grey and palomino were re-shot live against this branch and
  match (`others.png`).
