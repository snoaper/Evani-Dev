# Look bible: engine sandbox (look `basic`)

The sandbox is a test scene. This bible describes what it shows today, so the critic and Chris judge the review captures against the real thing, not an ambition.

## Palette

Material base colours from `looks/evani-look-basic` (`PALETTE`, `STONE`, `SLATE`, linear RGB, written here in sRGB hex); lighting, the haze, the Reinhard tonemap and bloom change them on screen.

| Hex | Role |
|---|---|
| `#76ad95` | Ground plane and the far hills (`ground` material; the hills tinted towards blue with distance) |
| `#95c4f3` | Spinner cubes and orbs (`accent` material, tinted per spinner from pale blue to cyan) |
| `#ff961e` | Glowing pieces, about one in seven spinners and the orbs on some columns (`glow` material, unlit, base [1.0, 0.15, 0.02] + emissive [5.0, 0.72, 0.02], over the bloom threshold): warm orange on screen, not yellow |
| `#e9e4da` | Pale warm stone (`stone`): the orbs' rings, terrace tops, column bases and caps |
| `#56606e` | Blue-grey slate (`slate`): terraces and column shafts |
| `#a0c4ed` | Sky (`render.clear_color`) and the colour the distance haze fades to |
| `#3f4550` | Palette dark: shadowed faces, shadow discs and the checker generator's dark square |
| `#f9f6f1` | Palette light: HUD text and the checker generator's light square |
| `#0d0f14` | Page background behind the canvas and the HUD panel tint |

## Shape language

- Spinners: bevelled cubes (0.9 m, edges chamfered by 12 %) and ringed orbs (a 0.72 m ball with a thin 1.3 m stone ring round its waist), alternating like a chessboard in one 20 × 20 grid, 1.6 m apart.
- Every spinner turns about a tilted axis and rests on its lowest point on the ground while it turns (0.012 m clear), so its faces catch the light at different angles and nothing floats.
- The middle distance is built from the same bevelled block: long low terraces, square columns with a wider base and cap. No raw boxes or untextured placeholders.
- Screen-space sprites are small flat atlas cells (8 to 30 px) in palette colours, drifting left to right with a slight wave.

## Sky and backdrop

A clear light-blue sky and a green ground plane at y = 0, 2 km square, that runs to the horizon from every orbit camera. A band of sky shows along the top of every frame (14 to 17 % of its height). Between the grid and the horizon:

- 36 to 54 m out: a loose ring of low stone and slate terraces, some with a smaller slab on top.
- 64 to 110 m out: standing slate columns, 4 to 14 m tall, with stone bases and caps; one in three is crowned with a glowing orb that blooms far off.
- 320 to 700 m out: low domed hills in a blue-leaning green.

A light distance haze (from 60 m out, at most 55 %, thinner higher up) fades the columns and hills towards the sky colour, so the depth reads. No clouds and no skybox.

## Lighting and shadows

One directional light from above and to the side, plus a cool ambient (`render.ambient` 0.08, 0.09, 0.12). The light direction reads from the brighter top faces of the cubes and from the shadows, which all fall the same way.

- Medium and up: the shadow map is on, and every spinner, terrace and column casts a real shadow on the ground.
- Low: no shadow map; every spinner stands on a dark, soft-edged shadow disc (a flat decal under its lowest point) instead.

## Props

1. Bevelled cube spinner (accent): the grid's even cells.
2. Ringed orb spinner (accent ball, stone ring): the grid's odd cells.
3. Glowing cube (glow): a scattered one in seven of the cubes.
4. Glowing orb (glow ball and ring): a scattered one in seven of the orbs.
5. Shadow disc (Low only): under every spinner.
6. Ground plane: under everything, at y = 0.
7. Terrace (stone or slate, sometimes two stacked): the inner ring of the middle distance.
8. Column (stone base, slate shaft, stone cap): the outer ring.
9. Column orb (glow): on one column in three.
10. Hill (ground, tinted): the far distance.
11. Circle, ring, star, diamond, plus, glow and checker sprites: drifting across the whole screen.

## Effects

- Bloom on the glowing pieces only (threshold 1.5 after the M1 tuning), from Medium up; on Low the glow reads as flat warm orange.
- Shadows (shadow map) from Medium up; shadow discs on Low.
- Vignette darkening the four corners.
