# Look bible: Evani.Materials (look `stylizedpbr`, film preset)

A bright, warm studio for judging materials: the subjects are clean, rounded
test pieces lit like an animated film (warm key from the upper right, cool
fill, a strong rim on the shadow side), standing on a sand-coloured stage
floor that runs to the horizon under a clear blue sky. The palette is
saturated but the stage stays mid-toned, so the materials, not the set,
carry the colour.

## Palette

sRGB hex. The scenes and lights store linear floats
(`apps/material-view/src/scenes/mod.rs`, `kits/evani-kit-material-view/data/lights.json`).

| Hex | Role |
|---|---|
| #bfa58a | Stage floor, warm sand (every scene but the garden) |
| #6aa84f | Garden lawn (pool and lamp corner) |
| #86addd | Sky at the zenith (the procedural sky) |
| #ceddea | Sky at the horizon, where the floor fades into the fog |
| #d9d2c5 | Plinths and stands (head plinth, disc stand) |
| #e0b25a | UI accent: selected chips, the open scene, sliders |
| #0e1016 | UI panel ink at 82 % (HUD, bars, drawer, panel) |
| #f2efe9 | UI text |
| #ffd27a | The sun dial and the warm key light |
| #ffc56b | Lamp glow (lamp corner at night) |

## Shape language

- **Test subjects first.** Every material sits on a clean, readable subject
  sized for it: spheres and seven other test shapes for the art styles, a
  rounded shader ball, a pouf for cloth, a bust on a stone plinth for skin,
  eyes and hair, a walled pool for water, a lamp with a thin shade for
  back-light.
- **Rounded and chunky.** Bevelled edges (bevel cubes for walls and stands),
  no razor-thin geometry, nothing spiky: the look's chunky proportions.
- **Grounded.** Everything rests on the floor or on a stand with a contact
  shadow; nothing floats, nothing is cut by the floor.
- **One subject per moment.** The camera frames the subject at its own
  scale; the set never competes with it.

## Lighting

Studio: a warm key at 42 degrees elevation from the upper right (azimuth
38), a cool ambient fill, the look's rim at 0.55 on the side away from the
sun, soft shadows and the engine's ambient occlusion (Medium and up) in the
creases and under each subject. Overcast, Sunset and Night lamp are the
other presets; the lamp moment shows the lamp's own glow at night. Bloom
only on real emitters (lamp glow, flames, glow materials). A light vignette
(0.18) keeps the corners calm without a haze in the middle.

## Backdrop

No walls and no world edge: the stage floor is 4 km across, so from every
scripted camera it meets the horizon under the fog, and the procedural sky
dome (no downloaded picture) fills everything above it, blue at the zenith
and pale at the horizon. The garden scenes swap the sand for lawn.

## Props

| Prop | Where it goes |
|---|---|
| Test spheres (a row of up to five) | Art styles scene, side by side across the open part of the view |
| Monkey head, teapot, cube, long box, prism, cylinder, ring | Art styles scene, picked with the shape chips |
| Selection disc (a thin glowing ring) | On the floor under the selected art-style material |
| Shader ball | Shader ball scene, centre |
| Brushed steel disc on its low stand | Shader ball scene, when the brushed finish is open |
| Velvet pouf | Cushion scene, centre |
| Head bust on a stone plinth | Head scene, centre, facing the camera |
| Plank deck and bench, brick pillar, tiled plinth, stone ball | Ground scene, round the floor patch |
| Walled pool with pebbles | Pool scene, front left |
| Glass bottle on a wooden table, a window pane on a frame | Pool scene, right |
| Leaf spray, hedge and grass patch | Pool scene, behind the pool |
| Lamp on a pole with a thin shade | Lamp corner, beside the pool |
