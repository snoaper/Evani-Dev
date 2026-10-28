# evani-look-stylizedpbr

The `stylizedpbr` look from `catalog/looks.json`: "Vibrant colours, chunky
proportions, clean physically based materials", drawn as metal-roughness PBR
with a saturated palette, a strong rim and ambient occlusion (3 points, 3D,
full look from the Low tier).

## Origin and credit

Ported from Chris Pearson's ChimpDuty project, branch `chimp-model`, folder
`handover/evani-dev/gaps/evani-look-stylizedpbr` (built against Evani.Dev
f216906). It was written from scratch for Evani.Dev; XeGTAO, Bevy's SSAO and
PCSS, three.js GTAO, NVIDIA's shells-and-fins paper, a shell-fur breakdown and
Inigo Quilez's biplanar mapping article were read for ideas only, with no
outside code or WGSL copied. Licence: the workspace licence.

## What it sets

- Slot bodies: `toon_ramp` (smooth wrapped diffuse, no bands), `rim` (strong,
  base-tinted, heavier away from the sun), `tonemap` (filmic with a toe), `fog`
  (light haze from 80 m).
- Slots it declares: `specular` (roughness/metal lobe), `surface_params`,
  `ibl_params` and, with feature `pcss`, `pcss_params`.
- Effects it asks for: `shadows`, `bloom`, `ssao` on; `outline` off (the tier
  budget still decides).
- Params `look.stylizedpbr.*` (wrap, spec, rim, rim_width, ao_strength,
  env_spec, pcss_light_deg, pcss_search, fur_shells_max, fur_fins,
  detail_strength, detail_scale); a system rewrites the slot bodies when one
  changes. Baked values (`params.json`, `slots/*.wgsl`) load through
  `tools/bake-index.rs` like the other looks.
- Features, all off by default: `pcss`, `fur`, `biplanar`, `gtao`.
- `tier.rs`: what each tier draws (High and Medium PCSS, AO pass, fur shells;
  Low and Lite blob shadows, baked occlusion map, fur clump map).

## Where the catalog won over the handover

- Palette: the handover's muted military swatches (made for ChimpDuty) became
  vibrant, saturated ones ("vibrant colours", "saturated albedo").
- Rim strength default 0.3 became 0.55 ("strong rim").
- `render.saturation` 1.08 became 1.15.
- The look asks for the catalog's `ssao` effect on every build instead of a
  `gtao` effect only with feature `gtao` ("strong ... ambient occlusion"; the
  catalog has no `gtao` effect id).
- "Image-based light from a small cubemap": not in the handover (it lights from
  a sky/ground hemisphere). Added as a named hook only (`ibl_params` slot,
  `surface.env` generator name), for S5-R5.

## Render side (S5-R5, not applied here)

This crate changes nothing in evani-render. Without the engine half the
renderer still uses the four built-in slot bodies above; the rest waits for
S5-R5. `src/hooks.rs` names every binding, entry point, slot and generator the
engine half must wire up, and `r5/` holds ChimpDuty's reference material:
`look-surface.patch` (not applied), `CORE_REQUESTS-entry.md` and
`catalog-stylizedpbr.json` (a proposal; the catalog stays the source of truth
and its tier `effects_on` rows need Chris's approval).

## Not here yet

- The handover's tests (`src/tests.rs` and the inline test modules on
  `chimp-model`) were left out under the Stage 4 to 6 build rule (testing is one
  combined pass later); port them in that pass.
- No `guard/` set: the look guard for this look is made and approved by the
  owner in Evani.App.
