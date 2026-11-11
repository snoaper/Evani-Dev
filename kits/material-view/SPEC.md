# SPEC: material-view (Evani.Materials viewer)

A tool, not a game: a viewer for looking at and tuning materials, the
in-engine home of Evani.Materials, published as a playground. The target pack
is `kits/material-view/target/` (reference prompt, look bible, moments, HUD
layout, playbook); this file does not repeat it.

## Systems

| System | Mechanics | Data and tunables |
|---|---|---|
| Scenes | Seven scenes, one per material family: art styles, shader ball, cushion, head, ground, pool, lamp corner; each family is compiled in only when `evani.toml [modules] materials` lists it | `data/library.json` (scenes, families, presets, cameras) |
| Library | 1,054 catalogue rows: search, filter chips, categories with ready/total counts; the open row drawn on its test shape in the open art style | `catalog/materials/*.json` (dev data, viewer only) |
| Art styles | 24 styles, each with starter materials and a shape treatment; the style detail step keeps a row's pattern inside the style | `data/styles.json`, `kit.material_view.style.*`, `kit.material_view.detail.*` |
| Tuning | A panel generated from the open family's registered params (Base, Finish, Detail, Quality), resets, compare A/B with a split line | the families' params |
| Lights | Four presets (Studio, Overcast, Sunset, Night lamp), a sun dial | `data/lights.json` |
| Camera | Orbit (drag, pinch, wheel, keys, pad), pan, reset, turntable | `kit.material_view.orbit_speed`, `zoom_speed`, `turntable_speed`, `key_orbit_speed` |
| World rooms | Each world's material slots; filled slots open their material | `data/world_rooms.json` |

## Camera

Orbit (catalog camera "third-person" in the look's list, used as an orbit
round the subject); each scene and material carries its home orbit.

## Physics

`none` (nothing moves but the camera).

## Look

`stylizedpbr` with its film preset (the look's own rim, 0.55, set by the
light presets), the engine's ambient occlusion (`gtao`, Medium and up). The
art styles draw their own shape treatments on top.

## Effects

- `bloom`: real emitters only (lamp glow, flames, glow materials); sells the look's lights.
- `vignette`: light (0.18), calms the corners without a haze in the middle.
- `shadows`: grounds every subject (contact shadows are part of reading a material).

## Playtest bot

Not applicable: a viewer has no win or lose. Free roam is the same as play;
the test `free_roam_through_every_scene` drives every scene and light with
keys for 30 s with no error, and `every_capture_moment_stages_its_scene`
stages every moment.
