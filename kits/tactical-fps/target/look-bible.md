# Look bible: Hexagon fortress

Look-neutral: the toon look carries it now, the photoreal look later. Colours are sRGB hex; look crates store linear floats.

## (a) Palette

| Role | Hex | Where |
|---|---|---|
| Concrete (breakable) | `#a9a39a` | Every walking floor tile, every cover block. Warm grey so it reads apart from metal. |
| Concrete broken face | `#6e675e` | Inside holes and craters, rubble. |
| Metal frame (unbreakable) | `#3b4250` | Outer walls, dividers, trim, lift frames, rails. Cool and dark. |
| Blue team | `#2f7bff` | Blue soldiers, Blue base lights, Blue core. |
| Orange team | `#ff7a1a` | Orange soldiers, Orange base lights, Orange core. |
| Core glow | `#fff2a8` | The core zone ring while it is being held. |
| Chasm haze | `#c7d3df` | Fog in the void under the fortress and round it. |
| Sky | `#8fb4d8` | Open sky through the centre. |
| Hazard stripe | `#e8c547` | The ring cars' inner edge stripe and the lift frames. |

## (b) Shape language

- Hexagon everywhere: the fortress is six 60-degree sectors round an open centre (inner radius 14 m, outer 38 m), three levels 8 m apart.
- Concrete is blocky and cut from 1 m cells: low walls, pillars, open boxes, steps. Holes are round bites; the rest stays square-edged.
- Metal is thin and straight: frames, trims, rails and dividers with sharp edges and no bevel; it never breaks.
- Soldiers are upright capsules about 1.8 m tall, team-coloured, readable at 40 m.

## (c) Sky and backdrop

An open sky above the centre, a deep hazy chasm below the bottom level (falling in kills), and the fortress's own outer wall closing every horizontal view, so no world edge shows from any first-person camera. The top level's walls are 6 m high and open to the sky.

## (d) Theme props

| Prop | Where |
|---|---|
| Concrete low wall (1-2 m high, 3-6 m long) | Every level, scattered cover. |
| Concrete pillar (1x1 or 2x2, 3 m) | Every level. |
| Open concrete box (3x3, one side open) | Every level. |
| Concrete steps (2x2 with a 1 m top) | Every level. |
| Floor tiles (about 4 m) | Every walking floor. |
| Metal outer wall | Round each level's outer edge. |
| Metal sector dividers with doorways | Bottom level (four doorways); solid on the ring and the top. |
| Ring car stripe (hazard yellow) | The ring cars' inner edge. |
| Lift column frame | Two per sector: bottom to ring, ring to top. |
| Inner rail | The bottom and top level's inner edge. |
| Team core zone (2.6 m ring of light) | Each team's top-level base sector. |
| Base light strip in team colour | The two base sectors' top walls. |
| Ceiling lamps | Under each concrete level. |

## (e) Lighting

Sun high and from one side, entering through the open centre: warm light on the inner faces, cool fill in the sectors' depths. Team colours glow at the two bases. Haze grows with depth in the chasm.
