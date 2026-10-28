# Playbook: 2D platformer

What a 2D platformer must get right, as a checklist the critic scores (rubric R8) and the builder copies into the kit's target pack (`kits/<id>/target/playbook.md`). Each item says why it matters. Mechanics only: all content is original.

Automated help: items 24 to 26 of docs/KIT_PROCESS.md read the geocheck-2d report for camera framing, bot level checks and the UI text rule.

## Camera

- [ ] **Platform snapping.** The camera's height follows the ground the player last stood on, not every jump. Why: a camera that bobs with each jump makes the whole screen bounce and hides where the player will land.
- [ ] **Lower-third ground anchor.** When standing, the ground under the player sits in the lower third of the screen. Why: the player needs to see what is above and ahead (platforms, enemies, collectibles), not a band of dirt.
- [ ] **Horizontal look-ahead.** The camera leads in the direction of travel and eases over when the player turns. Why: at running speed the player must see hazards before they arrive; a centred camera shows them too late.
- [ ] **Vertical dead zone.** Small vertical moves inside a zone around the player do not move the camera; leaving the zone (a long fall, a tall climb) does, smoothly. Why: it stops jitter on small steps while still following big height changes.
- [ ] **Terrain at most about 35 % of the screen.** The solid ground below the player fills no more than about a third of the picture. Why: more than that wastes the screen on nothing playable and makes the level feel cramped.

## Game feel

- [ ] **Coyote time.** A jump still works for a few frames (about 0.1 s) after running off a ledge. Why: players press jump at the moment they see the edge; without the grace the game feels like it ate the input.
- [ ] **Jump buffer.** A jump pressed a few frames (about 0.1 s) before landing happens on landing. Why: the same reason from the other side: early presses must not be lost.
- [ ] **Variable jump height.** Releasing the button early cuts the jump short; holding it gives the full height. Why: it gives the player fine control over short hops and long jumps with one button.
- [ ] **Responsive ground control.** Fast acceleration and turn-around on the ground, a little less in the air. Why: sluggish starts make precise platforming frustrating; a little air drift keeps jumps committed.

## Level readability

- [ ] **Solid looks solid.** Anything the player can stand on reads clearly as solid (outline, top edge, contrast); background decor is darker or less saturated. Why: the player must never jump onto scenery or miss a platform because it looked like decor.
- [ ] **Hazards read as hazards.** Spikes, pits and enemies have a distinct shape and colour and are never hidden behind decor. Why: deaths must feel fair; a hidden hazard feels like a trick.
- [ ] **One idea at a time.** Each new mechanic or hazard is first shown in a safe spot before it is combined with others. Why: the level teaches without text.

## Collectible placement

- [ ] **Clear air.** Coins and pickups sit in open air with space around them, not inside or touching terrain. Why: half-buried pickups look like bugs and are hard to read.
- [ ] **Arcs follow real jumps.** Coin trails trace the arc the player's actual jump makes at the tuned speed and gravity. Why: the trail shows the player the jump; a wrong arc teaches a jump that fails.
- [ ] **Every collectible reachable.** No pickup needs a jump the physics cannot make. Why: an unreachable coin is a broken promise (checked by the bot level checks).

## Juice

- [ ] **Popups rise and fade in under a second.** Score and pickup popups float up and are gone within about 1 s. Why: lingering text clutters the play area.
- [ ] **Popups merge into combos.** Quick pickups add up in one popup ("+50 x3") instead of stacking many. Why: a pile of numbers hides the action and reads as noise.
- [ ] **Never cover the player.** Popups appear above or beside the player and move away from them. Why: the player's character is where the player is looking; covering it causes deaths.
- [ ] **Landing and hit feedback.** Small dust on landing, a squash, a short flash or stop on a hit. Why: it tells the player the input landed without reading anything.

## Terrain

- [ ] **Variation with depth.** Ground has a top edge, a body and shading or layers that get darker with depth, and the surface varies (grass, rock, slopes, ledges). Why: flat repeating tiles look unfinished and hide where the walkable top is.
- [ ] **Parallax backdrop.** At least two background layers move slower than the play layer. Why: it gives depth and speed at almost no cost.

## Pixel integrity

- [ ] **One pixel scale.** Every sprite, tile, font and effect uses the same pixel size. Why: mixed pixel sizes are the clearest sign of a careless pixel game.
- [ ] **Integer positions.** Sprites and the camera are drawn at whole internal pixels. Why: half-pixel positions shimmer and blur edges.
- [ ] **Integer-scaled captures.** The game renders at its internal resolution and scales up by a whole number (320x180 x 6 = 1920x1080), and so do the review captures. Why: non-integer scaling makes uneven pixels that look like a bug (checked by the showcase captures).
