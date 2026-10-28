# Look bible: Puff's Sweet Grid (look `cartoon2d`)

Bold ink outlines, flat two-tone cel colours baked into layered sprites, squash and stretch in the animator (catalog `cartoon2d`). Everything is drawn from the engine's built-in white atlas shapes, tinted per sprite: no downloaded art.

## Palette

sRGB hex (the look crate stores linear floats, `looks/evani-look-cartoon2d/src/lib.rs`).

### Pieces (`pieces` palette; colour and shape both differ)

| Hex | Role |
|---|---|
| #ef3f61 | Cherry, red circle |
| #ffad38 | Toffee, orange square |
| #ffe745 | Lemon, yellow star (also Puff's sprout star) |
| #61e5a2 | Mint, green diamond |
| #50adff | Berry, blue triangle |
| #c469f3 | Grape, purple ring (a circle with a hole) |

### Board and world (`ui` palette)

| Hex | Role |
|---|---|
| #3c2750 | Ink: outlines, contact shadows (at 30 %), tray shadow |
| #fcf3fa | Board light checker tile |
| #efdff3 | Board dark checker tile |
| #ffe5bc | Tray frame (the peach frosting rim around the board) |
| #ffc5d4 | Background glow (the big soft pink light top-left) |
| #ffffff | Background candy dots (at 13 %), highlights |
| #bfecff | Puff's body (pale cloud blue) |
| #fffcfe | Frosting tiles (one layer; two layers mix towards pink) |
| #aa73bf | Sky, the clear colour behind everything |

### Page UI (`apps/match-3/index.html` CSS variables)

| Hex | Role |
|---|---|
| #1c0f2b | UI ink: text outlines, panel border, button borders |
| #ffd23f | Sun: moves number, star bar marks when lit, titles, callouts |
| #ff5fa8 | Pink: big callouts, intro card heading |
| #4fe0b6 | Mint: primary buttons |
| #b58cff | Lilac: secondary buttons, pause button |
| #fff4e2 | Cream: HUD panel, cards |

## Shape language

- **Silhouettes first.** Six piece shapes (circle, square, star, diamond, triangle, ring) so every piece reads by outline alone, colour-blind safe and at 28 px cells. Pieces fill 84 % of their cell, so the checker shows between them.
- **Ink.** Every piece, the tray, Puff and his parts carry a dark ink outline of `look.cartoon2d.outline_px` (3.5 px), scaled per shape so it looks the same width on each.
- **Two-tone cel.** A shade tone (the colour × `look.cartoon2d.shade` 0.62) under a slightly smaller lit tone shifted up-left, so a crescent of shade stays on the lower right; a small white oval highlight up-left. No gradients inside a piece.
- **Soft, chunky proportions.** Rounded corners on the tray and the DOM panel, fat buttons with a hard drop shadow, round-bodied Puff with stubby limbs. Nothing thin or spiky apart from the star and the Streak arrows.
- **Squash and stretch.** Swaps squash along the move, falling pieces stretch up to 16 %, landings squash, pops scale up then shrink and spin, Puff hops and squashes on landing (amount `look.cartoon2d.squash`).
- **Specials are add-ons, not new pieces.** A special keeps its colour and shape and gains a clear overlay: Streak = a white band with arrows at both ends along its line; Pop Bomb = a thin white halo ring with an ink rim behind the piece and just outside its outline (no filled centre, so the whole piece shape stays visible) plus a glowing fuse spark on top; Prism = a dark sphere with six orbiting colour beads and a hot star core (colourless, so it has no piece colour).

## Lighting

One light from the top left everywhere: the lit tone and highlight sit up-left on pieces and Puff, contact shadows fall down-right (piece shadow offset 6 % right, 8 % down of the piece size), the tray's shadow falls down-right. Effect sprites (sparkles, beams, blast rings, the Prism core, the Bomb fuse) are HDR above the look's 1.2 bloom threshold, so only they glow; flat pieces never bloom. The look's tonemap keeps flat colours exactly as painted.

## Backdrop

No 3D world and no horizon: the backdrop is a flat warm candy sky (#aa73bf clear colour), a large soft pink glow top-left (the light source), a second fainter glow bottom-right mixing pink and cloud blue, and a slow diagonal field of faint white candy dots drifting across the screen. It must never compete with the board: low contrast, no shapes the size of pieces.

## Props

| Prop | Where it goes | Recipe |
|---|---|---|
| Board tray | Under the board, following the level mask's shape (holes too) | Peach frame (#ffe5bc) 1.32 cells per cell, ink rim, soft ink shadow down-right |
| Checker tiles | Every playable cell | Look's `checker` generator tile, light and dark lilac alternating |
| Frosting tiles | Cells with frosting (levels 2 and 3) | Cream square over the tile with four tiny coloured sprinkles; two layers tinted pink |
| Puff the mascot | Desktop: under the HUD panel, left of the board; phone: centred under the board | Round cloud-blue body, ink outline, shade and lit tones, highlight, two eyes looking at the board (blink every 3.7 s), pink cheeks, stubby arms and feet, mood mouth |
| Sprout star | On Puff's head, bobbing and swaying | Lemon star with ink outline |
| Puff's ground shadow | Under Puff's feet, on his ground line | Ink oval at 25 %, shrinks while he hops |
| Streak stripes | On a Streak piece | White band along its line with pulsing white arrows at both ends |
| Pop Bomb halo | Behind a Pop Bomb piece, just outside its outline | Thin white-tinted halo ring with an ink rim, gently pulsing, drawn under the piece so its shape stays whole; HDR fuse spark on the halo at the top right, drawn on top |
| Prism orb | A Prism piece | Dark violet sphere, six orbiting colour beads, glowing white star core, soft violet glow under it |
| Streak beams | Across the board when a Streak fires | Hot beam in the piece colour with a white core, kept inside the board |
| Blast rings | Where a Pop Bomb or Prism fires; a small ring where any piece pops | Crisp flat rings built from short straight segments, kept inside the board. Bomb: two warm-white rings with ink rims, the second a step behind, only a little over the bloom threshold, and a short warm flash on the bomb's own cell. Prism: six staggered rainbow rings in the piece colours with thin ink rims, and a brief white flash on the Prism's own cell only. Pops: a small ink-free ring in the piece colour |
| Sparkles | From every popped piece and blast | CPU sprite particles in the piece colour (stars, circles, diamonds), capped by `fx.particles.max` |
| Confetti | Over the board on a win | Squares in the six piece colours falling and spinning |
| Selection and cursor ring | On the picked piece / keyboard cursor | Picked piece lifts and bobs; the cursor is a warm glowing ring |
| Candy dots | The backdrop | Faint white circles drifting diagonally |
| HUD panel and goal chips | Beside (desktop) or above (phone) the board | Cream rounded panel with an ink border and hard drop shadow; goal chips show the piece's own shape as an svg icon |
| Star bar | In the score box | Rounded bar filling yellow, three star marks that light at the thresholds; a lit mark is bigger, sun yellow with a thick ink outline and a warm highlight, so it stands out on a full bar |
