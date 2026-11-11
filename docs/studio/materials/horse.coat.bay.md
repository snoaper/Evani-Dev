# Horse coat, bay (`horse.coat.bay`): material pass 1

Queue row 1 (docs/studio/MATERIAL_QUEUE.md). Shot in the studio on its own asset
(Horse) and on the sphere, close, mid and far, Daylight, in Toon (the studio's
default style), Voxel and Cinematic Realism, at Ultra (desktop) and High (the
flagship-phone tier), headless and muted with `tools/studio/material-shots.mjs`.

## What a bay looks like

A reddish-brown to deep mahogany body with black points (lower legs, mane,
tail, ear rims). The coat is short (about 1 cm), lies flat and is sleek: on a
groomed horse it reads as one even colour with a soft satin shine that runs
along the hair, the light picking out the muscles of shoulder, barrel and
quarters. Any dapples on a bay are faint and *lighter* than the coat, never
dark rings. Individual hairs are invisible past arm's length.

## Before: the flaws (strict)

1. **Wood grain, not hair.** The strand noise swings the colour all the way
   between the root (`5a2a1a`) and tip (`9a5230`) colours, stretched along the
   flow, so the barrel and neck read as streaky burl veneer or a shaggy
   long-haired coat. A short horse coat has almost no visible strand contrast.
2. **Black dapple rings.** The `dapple` variant draws rings in the pattern
   colour, which for a bay is the points black (`1a1410`): dark contour rings
   over the body. In Voxel each ring becomes a line of near-black cubes, so the
   voxel horse looks scribbled with contour lines; on the voxel sphere they are
   scattered dark cubes.
3. **Lock blotches.** `clump` 0.05 plus the lock cells leave a blotchy
   mottling at mid range (Realism, mid shots).
4. **Matte, not sleek.** Roughness comes out at 0.71 (0.85 - 0.25 x sheen), so
   the coat has no satin highlight; the only shine is a grazing rim. The coat
   looks like felt or suede.
5. **Grey-white fuzz rim.** `sheen_shift` 0.4 mixes the rim toward grey
   (`0.5 + tip/2`), giving the silhouette a pale fuzzy halo: the "fuzz" look,
   not hair.
6. **Too orange in Realism.** With the tip colour carrying the streaks the
   average coat lands near a chestnut orange under Daylight; a bay should be a
   deeper red-brown.
7. Holds at a distance (far shots read as a brown horse with dark legs), and
   High matches Ultra (same look on the flagship phone).

Not this material (noted for their own rows): the sphere's uv seam shows a
smooth vertical band on every uv-space material; the mane's top fades pale in
Toon and Realism (the mane is its own region and follows the coat).

## After (2 rounds)

Round 1 (shader + row), round 2 (colours a step deeper and redder). Side by
side: `.evani/shots/mat01/ba-ultra-horse.png`, `ba-high-horse.png`,
`ba-ultra-sphere.png` (headless captures, kept a day; progress shots in the
Viewer's Progress, task `materials`).

- **Fur recipe, sleek short coats** (`rc_fur.wgsl`, every fur row with length
  under 0.35): `sleek = 1 - smoothstep(0.05, 0.35, length)`; strand contrast
  x (1 - 0.6 sleek), fibre/lock relief -0.15 sleek, roughness
  0.85 - 0.25 sheen (1 + 0.9 sleek). Long coats (wolf, bear, sheep) are
  unchanged. Three lines of WGSL, no new branch, binding or texture, inside
  SHADER_BUDGET.
- **The row**: `solid` (was `dapple`), colours `521f10` / `7c3820` / `1a1410`
  (was `5a2a1a` / `9a5230`), sheen 0.8 (0.55), sheen shift 0.1 (0.4),
  pattern scale 40 (26), pattern contrast 0 (0.3), clump 0 (0.05). Points 0.85
  kept: it is what turns the legs, mane and tail black in the studio.
- **Result**: a deep mahogany coat with a satin highlight that follows the
  muscles of shoulder, barrel and quarters; no wood grain, no dark rings, no
  grey fuzz rim from the material; the voxel horse is an even bay with gentle
  per-cube tone; High (flagship phone) matches Ultra; reads at a distance.
- **Art director, what's left**: in Realism the lit side still leans a little
  warm (within the range of a real blood bay). The dark step lines on the
  voxel horse are the Voxel style's own step shading (identical with the
  coat's points at 0), and the pale halo round the silhouette in Toon and
  Realism is the style's sky rim: both belong to the styles, not this coat.
- **Shared recipe re-shot**: `dog.coat.tan` and `horse.coat.palomino`
  (`share-dog.png`, `share-palomino.png`): both lose streaking and gain a
  cleaner sheen; no regression. Their remaining Toon blotches are their own
  rows' business.
