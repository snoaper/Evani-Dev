# Look bible: Mossgate Keep

Written for the `pixelhd` look (480x270 internal, pixel-perfect sprites, optional normal-mapped 2D lights, particles at native resolution). The greybox draws the roles below as flat colours so the palette can be swapped by any look.

## Palette

| Hex | Role |
|---|---|
| `#2b2a35` | Wall top and the dark between stones (outline) |
| `#4a4a5e` | Wall face |
| `#6c6d82` | Wall highlight edge, lit side |
| `#8a7f6a` | Floor stone, base |
| `#a39a82` | Floor stone, light tile in the checker |
| `#3e7d4b` | Moss, gloop slime body |
| `#c9453a` | Hero cap, hearts, damage flash |
| `#2f9aa0` | Hero tunic, shield rim |
| `#e5b93c` | Brass: sword, keys, gems, locked door plate |
| `#6e7bd8` | Warden core, bolt wand and bolts, Dawn Shard glow |
| `#f1ece0` | Highlights, hit sparks, text |

Lighting direction: from the top left. Walls are lit on their top and left edges and shaded on the bottom and right. Torches add warm pools of light (HD look: normal-mapped).

## Shape language

Chunky and readable from above. The hero is a 12-wide, 14-tall figure with a big cap and a round shield on the left arm so its facing reads from the silhouette. Slimes are low half-domes with two eye dots; bats are wide, thin and spiky; the husk is square-shouldered with a plume; the spitter is a squat statue head. Walls are blocky tiles with a one-pixel dark outline and a lighter top edge. Doors are inset arches; the locked door has a brass plate with a keyhole. Pickups bob slightly and have a one-pixel sparkle. Proportions: a tile is 16x16 art pixels, a room is 16x11 tiles, the hero is slightly under one tile.

## Backdrop and sky

There is no sky: the camera looks straight down into the room. The space outside the room (the letterbox around the 16x11 room on wide screens) is the dark `#2b2a35` with a faint moss-and-stone pattern; the HUD strip above the room is the same dark with a one-pixel lighter bottom edge.

## Theme props

| Prop | Where it goes |
|---|---|
| Wall torch (animated flame, warm light pool) | Wall faces beside doors and in the boss room |
| Cracked floor tile | Scattered on the floor, never on a door tile |
| Moss patch | Wall bases, floor corners |
| Clay pot (breakable, may drop a heart or gem) | Room corners |
| Bone pile | Dead-end corners of the cellar and vault |
| Rubble pile | Beside walls in the gate hall (blocks nothing) |
| Banner (torn cloth) | Wall faces in the armoury and boss room |
| Weapon rack | Along the armoury wall |
| Cobweb | Room corners in the cellar and vault |
| Pillar base | Inside the antechamber, as decoration only |
| Floor switch (round stone plate) | Vault, in front of the gate |
| Stone gate (bars) | Vault, over the alcove with the wand |
| Locked door (brass plate with keyhole) | Cellar east wall, antechamber east wall |
| Pedestal | Under the key in the antechamber and the Dawn Shard in the boss room |
| Fallen helmet | Gate hall, a hint of what lies ahead |
| Water drip puddle | Cellar floor |

## Light and effects

Torch flicker, hit sparks, a short hit-stop on sword hits, a small screen shake on taking damage, dust puffs on walking, a glow on the Dawn Shard. The catalog's top-down camera favours shadows for depth: a soft contact shadow under every actor.
