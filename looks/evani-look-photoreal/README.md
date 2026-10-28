# evani-look-photoreal

"Gritty photoreal" (`catalog/looks.json` id `photoreal`): desaturated documentary realism, grime and wear. 3D, 5 points, full look from the High tier. Hexagon's look (docs/HEXAGON_PLAN.md).

## What it sets

- Palette `photoreal` and 13 metal-roughness materials: `concrete`, `concrete_dark`, `steel`, `steel_worn`, `paint_blue`, `paint_orange`, `cloth`, `rubber`, `dirt`, `grime`, `scorch`, and the unlit HDR `lamp` and `muzzle_flash` that bloom catches.
- `tonemap` slot: a filmic stage (toe, then a white-point shoulder, per channel) followed by the look's **grade slot** (contrast around mid grey, desaturation, cool-shadow / warm-highlight split tone, a slight black lift). The grade sits between the lines `// @photoreal-grade begin` and `// @photoreal-grade end` in the slot body.
- `fog` slot: exponential height haze from 25 m, capped at 70 %.
- Wants `shadows` and `bloom`; sets an overcast sky, cool ambient, a soft neutral sun and a bloom threshold of 3 so only lamps, flashes and blasts glow.

## Params (owner `look.photoreal`)

| Param | Default | What it does |
|---|---|---|
| `look.photoreal.tonemap.white` | 11.0 | Scene brightness that maps to display white |
| `look.photoreal.tonemap.toe` | 0.03 | How far blacks are pressed down (mid grey stays) |
| `look.photoreal.grade.source` | `look` | `look` (this grade), `custom` (a game's body), `none` |
| `look.photoreal.grade.contrast` | 1.08 | Contrast around mid grey |
| `look.photoreal.grade.saturation` | 0.76 | Colour saturation |
| `look.photoreal.grade.lift` | 0.012 | Matte black lift |
| `look.photoreal.grade.split` | 0.22 | Split-tone strength |
| `look.photoreal.grade.shadow_tint` | [0.82, 0.95, 1.0] | Shadow tint |
| `look.photoreal.grade.highlight_tint` | [1.0, 0.93, 0.80] | Highlight tint |

Slot bodies only see their arguments, so these values are written into the tonemap body. A frame system re-composes the body and asks the renderer to rebuild the tonemap pipeline when one changes. It never overwrites a body it did not compose (a hand edit through `slot.set`, or a baked `slots/tonemap.wgsl`).

## The grade hook (for S5-FX1)

The catalog's `grade` effect (a 32³ LUT with a crossfade) is built later in `evani-fx` as its own pass. This look does not depend on it. When it lands:
- with `look.photoreal.grade.source = none` the LUT grades alone;
- with `look` the LUT stacks on top of the look's grade;
- a game can also pass its own grade stage with `PhotorealLook::default().with_grade_body(BODY)` (statements on `var c: vec3<f32>`).

## Expected from other pieces (not in this crate)

- Cascaded sun shadows (`shadow-cascades-2`, later 4), image-based lighting (prefiltered cubemap and BRDF lookup), clustered forward point lights for lamps and muzzle flashes, CPU frustum culling.
- Sampling metallic-roughness, occlusion and tiling detail maps in the forward pass for this look; grime through the `decals` effect; `volumetric` atmosphere on Ultra (Stage 8).
- Auto exposure (`exposure` slot: today it receives a fixed 0.18 luminance placeholder), and a vignette: the tonemap slot only gets a colour, no screen position, so the plan's "light vignette in the tonemap slot" needs the `vignette` effect instead.
- What "reduced" means below `full_look_from_tier` is not defined in the engine or catalog yet.
