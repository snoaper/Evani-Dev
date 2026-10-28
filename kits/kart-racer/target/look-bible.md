# Look bible: toon candy mountain

Hex values are sRGB. Entries marked "look" are converted from `looks/evani-look-toon` (linear floats); the rest are proposed and extend its 16 material roles (the kit tints the white `kart_body` material per instance).

## (a) Palette

| Role | Hex | Note |
|---|---|---|
| Sky top | #4f9bf5 | deep candy blue; look clear colour is #95cefd |
| Sky horizon | #bfe9ff | also the fog and haze colour |
| Sky warm band (sun side) | #ffe9c4 | faint, one side only |
| Cloud lit / shade | #ffffff / #cfe0fa | shade is the cool band |
| Road | #6f6c7e | look |
| Road variation (patches, worn line) | #7b7889 / #646172 | about 8% value shift, low contrast |
| Road centre dash | #f7f7fa | look sugar |
| Kerb red / white | #f1506c / #f7f7fa | look cherry / sugar |
| Grass light / dark | #7cd161 / #5fbb55 | light = look apple; wide stripes plus noise patches |
| Grass lip near road | #9be07a | lighter edge |
| Gumdrop hill pink | #f9b3ce | look bubblegum |
| Gumdrop hill mint | #a0edce | look mint |
| Gumdrop hill lilac | #ceb8f6 | look lilac |
| Gumdrop hill lemon | #fff0a0 | new |
| Lollipop stick | #f7f7fa | |
| Lollipop swirl pairs | #f1506c/#f7f7fa, #ffda50/#ffffff, #89e5ff/#f7f7fa, #a23cff/#f9b3ce | pick by hash |
| Candy cane | #f7f7fa with #f1506c stripes | |
| Wrapper colours (signs) | #ffda50, #89e5ff, #f1506c, #a0edce; twist ends #f7f7fa | |
| Tree foliage / trunk | #55bf86 / #ad7c55 | look (plain trees) |
| Ink outline | #2b1240 | deep purple, never pure black; HUD ink is the same |
| Wheel | #3f3f4b | look |
| Item box | solid candy blocks: #2fc46a, #9b4dff, #3557d6, #f1506c, #ffd23f, #18c7c0 (one at a time, fading slowly); white "?" and four rivets per face | lit body; the "?" and rivets unlit white at half the old ribbon bloom |
| Spark tiers 1 / 2 / 3 | #49b6ff / #ff9a2e / #ff5fa8 | emissive |
| Boost flame | #ff9a2e | emissive |
| Player kart pink | #ff4fa3 | hue 331; the only pink kart |

### Rival kart colours

These match `kits/evani-kit-kart-racer/data/karts.json` (spec order; the linear values there are the source of truth). Hue distance from the player pink is at least 54 degrees, hues spread round the wheel, mixed light and dark values.

| Spec | Kart | sRGB | Linear in karts.json | Hue | Role in a race |
|---|---|---|---|---|---|
| 0 | Pip | #ff4fa3 | [1.000, 0.078, 0.366] | 331 | the player (P1); the only pink kart |
| 1 | Mo | #2a7bff | [0.023, 0.198, 1.000] | 217 | rival; P2 in 2-player mode |
| 2 | Taffy | #ffd400 | [1.000, 0.658, 0.000] | 50 | rival |
| 3 | Bramble | #00b86b | [0.000, 0.479, 0.147] | 155 | rival |
| 4 | Zuzu | #a23cff | [0.361, 0.045, 1.000] | 271 | rival |
| 5 | Nib | #00c8e0 | [0.000, 0.578, 0.745] | 186 | rival |
| 6 | Clover | #8bd900 | [0.258, 0.694, 0.000] | 82 | rival |
| 7 | Rumble | #ff7a1a | [1.000, 0.195, 0.010] | 25 | rival |

A race has 8 karts by default: the player (Pip) plus the 7 rivals, specs 1 to 7. In 2-player mode P1 is Pip and P2 is Mo; the AI field is specs 2 to 7. Larger fields (up to 12 karts) reuse the roster in order. Nearest rivals to the player's hue: Zuzu (60 degrees) and Rumble (54). Light karts: Taffy, Clover; dark: Bramble, Mo. The driver wears the kart colour lightened 25% (helmet) with a white visor.

## (b) Shape language

- Rounded, bevelled, chunky, slightly oversized: wheels 1.2x real, heads 1.3x, props 1.5x. No hard 90 degree edges; bevel radius about 6% of the part.
- Karts: bevelled body, big rear wheels, spoiler, exhaust, visible steering wheel. Drivers: bevelled torso, sphere head with a helmet shell, visor and a pom or crest, so the head reads as a volume and never as a flat disc.
- Toon shading: 3 hard bands (lit, mid, shadow), edges at N.L 0.02 and 0.45. Every model near the camera shows at least 2 bands.
- Ink outlines about 1.6 px at 1080p (`fx.outline.thickness` 1.6, strength 0.9), colour #2b1240.
- Rim light: a thin bright edge (+20% value, cool white) on the side away from the sun, on karts, drivers and near props.

## (c) Sky and backdrop

- Gradient sky #4f9bf5 to #bfe9ff at the horizon, drawn as a dome so no camera angle shows a flat colour.
- Clouds: clusters of 3 to 5 flattened ellipsoids (scale 1 : 0.35 : 0.7), flat bottoms, 2 bands (lit, shade); 24 to 40 on a ring at 300 to 500 m, drifting slowly; about 80 triangles each.
- Backdrop: ring of gumdrop hills (pink, mint, lilac, lemon) at 150 to 250 m, two overlapping rows, 5 to 25 m tall, closed through 360 degrees. Light haze in #bfe9ff blends the ring into the sky. The ground extends past the ring and is fully hazed at its edge.
- Pass rule: no chase, orbit, podium or capture camera shows the end of the ground, a gap in the ring, or a flat void.

## (d) Theme props

All procedural, shared meshes, instanced. "Dropped on" is the tier that removes it.

| # | Prop | Recipe | Tris | Placement | Dropped on |
|---|---|---|---|---|---|
| 1 | Lollipop tree | cylinder stick (6 sides) + two stacked flattened discs (12 sides), 2 swirl colours, top disc turned 30 degrees | 110 | verges, infield | Low keeps 1 in 3 |
| 2 | Candy-cane barrier | cylinder + half-torus hook, white with red band rings, 4 per segment | 90 | outside kerbs on corners | Low: straights only |
| 3 | Gumdrop hill | squashed hemisphere (10x6) plus 6 small sugar spheres | 140 | backdrop ring, infield | Low: backdrop only |
| 4 | Wrapped-sweet sign | rounded box + two cones as twist ends, chevron panel | 70 | corner entries | none |
| 5 | Gingerbread stand | bevelled box, striped sloped roof, stepped rows | 150 | start straight, last corner | Low drops crowd |
| 6 | Crowd sweets | cone + sphere gumdrops, 3 colours, 2 rows of 8 | 16 each | on stands | Low |
| 7 | Candy-floss bush | 3 overlapping spheres, pink or lilac | 60 | grass between trees | Low |
| 8 | Jelly-bean rock | ellipsoid 8x5, pastel, tilted | 50 | verges, outside corners | Low |
| 9 | Sprinkle mushroom | cylinder stem + hemisphere cap with 5 dots | 70 | infield | Low |
| 10 | Cupcake building | cylinder + cone frosting + cherry sphere | 80 | backdrop, infield | Low: far only |
| 11 | Peppermint post | cylinder with two colour rings + sphere top | 40 | corner apex kerb | none |
| 12 | Start gate | two candy-cane pylons + arch beam + 3 emissive light spheres | 220 | start line | none |
| 13 | Lollipop flag line | thin stick + flat disc flags on a string | 50 | start straight | Low |
| 14 | Chevron kerb | alternating red/white bevelled slabs | 30 per m | corner outsides | none |
| 15 | Grass tufts | cone triples in 2 greens | 12 | dark grass patches | Low |

Budget: scenery under 70 000 triangles on Low, about 110 000 on Medium and up. Every corner has a sign, a barrier run and at least 3 props within 25 m of the apex. Grass is never one flat colour. Road shows subtle variation (patches, wear along the racing line).

## (e) Lighting

- Sun from the side and slightly ahead (direction about [0.62, 0.85, 0.28]), warm key #ffebc7, so the bands split karts into lit, mid and shadow sides.
- Ambient cool blue-violet, [0.24, 0.26, 0.34] linear: shadows are cool, never black.
- Rim: cool-white edge on the shadow side, strongest on karts.
- Shadows: map from Medium up, blob shadows on Low. Fog is the horizon colour, haze from 60 m, capped at 0.45.
- Shadow tint: soft purple #a98bf0 at strength 0.65 (look parameters `render.shadow_tint` and `render.shadow_tint_strength`, live-tunable in the Look Lab). Shadows from the map and the blob and disc stand-ins keep their brightness but lean purple; never near-black. Strength 0 gives the old neutral shadows.
- Post: saturation 1.15, gentle filmic tonemap; bloom only on sparks, flames, item boxes and gate lights.
