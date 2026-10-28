# HUD layout

The HUD is drawn in the canvas on the art grid (a strip of 24 art pixels above the 16x11-tile room). Positions are in art pixels from the top-left of the view; at 1080p the art grid is 480x270 (4x), so the room (256x176) sits centred under the strip with the dark letterbox either side. On a phone the view is scaled by the largest whole number that fits and the touch controls sit over the letterbox where there is one (below).

## Elements at 1080p (view 256 art px wide, HUD strip 24 art px high)

| Element | Position | Shows |
|---|---|---|
| Hearts | left, x 6, y 6, 8 px apart | one heart per 2 health points: full, half, empty; a flash when a point is lost |
| Key count | x 84, y 6 | a small brass key and the count |
| Gem count | x 116, y 6 | a small gem and the count |
| Item slot | x 154, y 3, 18x18 frame | the bolt wand once found; empty frame before |
| Room map | right, x 196, y 3 | one small square per room by grid position; visited rooms filled, the current room bright, the boss room red-marked once seen |
| Free-roam label | centre of the strip | "FREE ROAM" instead of nothing |

## Banners

Title, win and game-over text are drawn centred over the room in a 3x5 pixel font on a dark panel: "MOSSGATE KEEP / PRESS ATTACK", "YOU WIN", "GAME OVER". They appear only in those states, never during play.

## Phone (portrait, about 390x844 CSS px)

The view keeps the same layout, scaled to the widest whole-number scale (at least 1). The room sits in the upper half; below it the touch controls fill the free area: a four-way pad bottom left, Attack (large) and Item bottom right, pause top right under the HUD strip. In landscape (844x390) the room fills the height and the controls overlay the letterbox at the sides.

## During a countdown or popup

The game has no countdown. Pick-up messages are small text above the pickup that rise and fade in under a second and never cover the hero; the HUD itself never moves.
