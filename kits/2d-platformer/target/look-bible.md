# Look bible: Fennel's Leap on `pixel16`

**One line:** a bright 16-bit hillside: saturated grass and sky, warm earth that darkens into deep soil, a fox-eared hero in a teal scarf, everything outlined in ink and lit from the top left.

## Pixel rules

- Internal view 270 art pixels high at 1080p (480 × 270, scale 4), 288 at 1440p (512 × 288, scale 5): the smallest whole scale that fits; width follows the window, at least 256 art pixels.
- One pixel size for everything: tiles, sprites, parallax planes, clouds, floating text and HUD are on the same grid at whole-pixel positions. Nearest sampling; no rotation, no sub-pixel scrolling.
- Tiles and characters 16 × 16 (hero frame 16 wide with its outline); HUD glyphs 6 × 8 on a 7 px advance; floating text glyphs 3 × 5 (6 × 8 with outline) on a 4 px advance.
- Selective outline in `ink`; lit keys get top-left light (highlight top and left edges, shade bottom and right). Gradients only as ordered (Bayer) dither between ramp shades, never smooth.
- Palette cycling for water and torch fire (4 frames, 6 fps). No post effects except parallax and screen shake.

## Palette (the 15 ramps in `kits/evani-kit-2d-platformer/data/art.json`; a look's `ramp.<name>` palettes replace them)

| Ramp | 0 (dark) | 1 | 2 | 3 (light) | Used for |
|---|---|---|---|---|---|
| ink | #101018 | #282838 | #484860 | #707088 | Outlines, text shadow, darkest details |
| fur | #782808 | #C05010 | #F08830 | #F8C070 | Fennel's fur and ears |
| teal | #084850 | #108888 | #30C0B0 | #88F0D8 | Fennel's scarf, checkpoint flag |
| green | #184010 | #307818 | #58B028 | #A8E850 | Grass tops, bushes, ledges, reeds, stems |
| leaf | #103028 | #205838 | #388850 | #68B870 | Tree canopies, far foliage |
| brown | #381808 | #683818 | #986030 | #C89058 | Earth near the surface, trunks, planks, signpost |
| stone | #383848 | #606878 | #9098A8 | #C8D0D8 | Stone pillar, rocks, castle, spikes |
| gold | #704000 | #C08000 | #F8C800 | #F8F8A0 | Coins, mini text, HUD glyph lower half, torch flame |
| red | #580818 | #981828 | #D82848 | #F88080 | Flowers, beetle shell, hurry timer |
| blue | #182058 | #2840A0 | #4878E0 | #90C0F8 | Puddle hopper |
| water | #103878 | #2060B0 | #3890D8 | #80D0F8 | Water (cycled) |
| sky | #3858B0 | #5888D8 | #88B8F0 | #C0E0F8 | Sky ramp |
| white | #787890 | #A8A8C0 | #D8D8E8 | #F8F8F8 | Highlights, HUD and text |
| shadow | #101828 | #202840 | #303858 | #404870 | Drop shadows, deep shade |
| soil | #100810 | #1C0E12 | #2A1410 | #482818 | Deep earth below the brown band (darkest dirt) |

Sky bands, top to horizon: #3060C0, #4078D0, #5890E0, #70A8E8, #90C0F0, #B8D8F8.

Parallax layer tints (multiply, lighter and cooler with distance): far #D0E0FF to #A8C8F0 (haze 0.45); mid #E8F0FF to #B0D0F0 (haze 0.22); near #FFFFFF to #C0E0F8 (haze 0).

## Terrain

Grass cap (`green`) over earth (`brown`, getting darker with depth), a step darker every 22 px from 10 px down, then deep `soil` from about 76 px (five tiles) down; bands joined by dither and blended across columns so there are no horizontal stripes. Roots under the grass; pebbles, stone rocks and boulders mid-depth; rare bones and fossils deep down (16 variants: eight different pictures, an ammonite, a trilobite, a fish skeleton, a scallop, a long bone, a skull, a ribcage and a jaw, each lying flat or turned on its side; at most one in every ten columns, and one picture comes back only eight strips on, so no screen the game can show holds it twice). The level keeps five rows of deep soil under its lowest ground, so the camera never has to rest on the map's bottom. Stone (`stone`) only where it is built: the pillar and the castle.

## Theme props (16)

| Prop | Where / role |
|---|---|
| Wooden signpost | Level start, points the way |
| Round bush | Ground decor, start and banks |
| Red flower | Ground decor in grass |
| Round tree (30 × 44, lit canopy on a trunk) | Off the hilltop and on the plateau, never over coins |
| Grass tuft | Ground decor |
| Reed | Pool edges |
| Rock | Ground decor |
| Gold coin (4-frame spin) | Lines at walking height and arcs on real jumps |
| Checkpoint flag (down, up) | Two checkpoints |
| Torch with cycled flame | Castle approach |
| Castle wall and gate | The goal |
| Wooden plank (one-way) | Over the pit, drop-through |
| Grassy ledge (one-way) | Floating steps over the pit |
| Stone pillar (mossy cap, ashlar shaft, plinth) | Beside the spike pit, walkable cap |
| Spikes | The pit beside the pillar |
| Water (cycled surface and deep) | Pools to jump over |

Creatures: **Fennel** (fox-eared hero, `fur` + `teal` scarf), **bramble beetle** (red shell, patrols, stomp to flatten), **puddle hopper** (blue, hops towards the hero).

## Parallax

Three planes behind the banded sky: far hills with the castle silhouette, mid hills with round trees, near bushes; each tinted lighter and cooler with distance (haze), scrolling at 0.15 / 0.4 / 0.7 of the camera and 0.3 vertically; clouds drift in the far plane.

## Shape language

Chunky, rounded and readable at a glance: every character and tile sits in a 16 × 16 cell with a one-pixel ink outline and a lit top-left edge, so silhouettes read at 1080p from four device pixels per art pixel. Round, soft canopies and bushes (circles lit from the top left, never spiky) for the friendly hillside; flat-topped, squared blocks for anything solid you can stand on (grass-capped earth, ashlar stone, planks); sharp triangles are kept for spikes only, so a point always means danger. The hero is the busiest shape on screen (fox ears, a teal scarf, a white chest, a long tail), the enemies are low and rounded (a red domed beetle, a blue frog), so the player's silhouette never merges with theirs. The bottomless pit is a flat dark blue void with no highlights: it is the only large empty shape in the ground line.

## Sky and backdrop

A six-band dithered blue sky, three parallax strips (far blue mountains with the castle on the horizon, mid green hills, near round bushes) that move at 15 %, 40 % and 70 % of the camera, and drifting clouds. The backdrop is lighter and less saturated than the play layer, so solid ground and hazards always read in front of it. Under the near bushes the backdrop fades into a pale mist and then a hazy valley of far treetops in sky-ramp blues, so a drop seen from high ground shows scenery, never ground or a flat band. A bottomless pit is a void in the shadow ramp: slate at the lip, dithered down to black, with a lighter rim along each wall.
