# evani-look-photoreal changes

## Unreleased (2026-10-21, test-pass fixes)

- `render.ambient` 0.10 to 0.22 (cool overcast fill): walls in shadow read as grey concrete instead of near-black.
- `muzzle_flash` emission 14 to 5 (still over the bloom threshold of 3): the flash glows orange instead of whiting out the first-person view.

## 0.1.0 (2026-10-20, S5-L3)

- New look crate for the catalog's `photoreal` look: palette, 13 PBR materials, filmic tonemap with the look's grade slot, height haze, shadows and bloom wanted, overcast light.
- Params `look.photoreal.tonemap.*` and `look.photoreal.grade.*`, re-composed into the tonemap body live.
- Grade hook for the later `grade` LUT effect (S5-FX1): `look.photoreal.grade.source`.
