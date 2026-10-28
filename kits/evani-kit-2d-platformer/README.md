# evani-kit-2d-platformer

Kit 002 of Evani.Dev: a 2D platformer. The demo game is **Fennel's Leap** (`apps/2d-platformer`), one level (Bramble Hills), with the pixel16 look (`looks/evani-look-pixel16`). The full design is in `kits/2d-platformer/SPEC.md`.

## What it does

- Kinematic hero at a fixed 60 Hz: walk and run, variable-height jump, coyote time, jump buffering, stomp bounce.
- Tile collision with 45 and 22.5 degree slopes and one-way plank platforms (down + jump drops through).
- Moving platforms (horizontal and vertical) that carry the hero.
- Bramble beetles (patrol, never walk off a ledge) and puddle hoppers (jump towards you).
- Coins (50 give a life), checkpoint flags, spikes, water, a level timer, lives, and the castle gate goal.
- A tile-map level loader with checked data (bad files fail a test, never a running game).
- A side camera that rests on the ground the hero last stood on, in the lower third of the screen, holds still through jumps (vertical dead zone) and looks ahead in the direction of movement.
- Terrain that darkens with depth into soil, with roots, rocks, bones and fossils; a stone pillar with a cap.
- Score pops that merge into combo totals, rise and fade within 0.8 s and never cover the hero or each other.
- One pixel grid for everything (tiles, sprites, parallax, floating text, HUD), parallax hills, drop shadows, synthesised sounds (no downloads).
- Capture moments for reviews (start, stomp, 1UP, checkpoint, spikes, goal), in live builds only.
- Review checks as kit tests: framing (terrain at most 35 % of the screen), floating text, coin reachability and clearance, the pixel grid and the HUD on phones.

## Plug it in

```rust
App::new("my-platformer")
    .with_kit(evani_kit_2d_platformer::PlatformerPlugin)
    .with_look(evani_look_pixel16::Pixel16Look) // any 2D-capable look works
```

The kit asks the look for its palettes by name and uses its own default ramps for any the look lacks, so it also runs with `basic` (same art, smooth-scaled, in the kit's default colours; covered by a test).

## Tuning

Every gameplay number is a live parameter under `kit.platformer.*` (speeds, jump, coyote and buffer times, enemies, camera, scores); `schema.get` lists them. Values tuned in a live session and written by `bake.export` live in `params.json` here and in the look crate, and load in every build, with or without live. Baked so far: `shadow_alpha` 0.62, `stomp_trauma` 0.35 (kit) and `fx.shake.amount` 0.6 (look).

## Controls

| | Keyboard | Gamepad |
|---|---|---|
| Move | A / D or arrows | stick or d-pad |
| Jump | Space, W, Up or K | A |
| Run | Left Shift or J | X |
| Drop through a plank | S or Down, then jump | down, then A |
| Pause | Esc or P | Start |

Every key can be changed in Settings.

### Touch controls (phones and tablets)

On a touch screen the game shows on-screen controls (only on a coarse pointer, and only while a run is being played; the title, pause, settings and results screens are tapped on the page). They set the same actions the keys do, so a held thumb behaves exactly like a held key, including the variable jump height (hold Jump for higher, let go to cut it).

| Control | Where | Same as |
|---|---|---|
| left / right (hold) | bottom left | A / D (the `move` axis) |
| Jump (large, hold) | bottom right | Space |
| Run (hold) | bottom right, left of Jump | Left Shift |
| pause (II, tap) | top right, under the HUD | Esc |

The pause button sits just under the HUD row (lives, coins, score, time or the FREE ROAM label), never over it; a test measures that on phone and tablet sizes. Dropping through a plank (down + jump) has no button, since no part of the level needs it. Portrait is the narrow, tall view the HUD test already covers, with the same controls in the bottom corners. The layout is `touch::layout` (`src/touch.rs`); the kit depends on `evani-core` with its `touch` feature, so only this game's download has the layer.

## What to try

- Press Play, walk right with D, hold Left Shift to run, and tap or hold Space for a short or high jump.
- Jump on a beetle to stomp it, grab coins and touch a flag to set a checkpoint.
- Ride a moving platform over the water; on a wooden plank, hold S and press Space to drop through.
- Pause with Esc, open Settings, switch Parallax and Screen shake off and on, and change a key.
- Reach the castle gate before the timer runs out for Level clear!, or lose every life for Game over.

## Free roam

Open the game with `?mode=free` (the library's Free roam button) and it is the same level with no lives, timer or goal: it starts straight in the level, a pit or an enemy touch is a free respawn on the last safe ground (or checkpoint), the castle gate is just a place to explore, and a small FREE ROAM label replaces the lives and timer on the HUD. Stomps and coins still work. The default (Play) is unchanged.

## Effects

The game lists `parallax` and `shake` in its `evani.toml`. Both are tiny (about 0.2 KB gzipped each). Shake moves the camera by whole art pixels.

## Known cosmetic risk

The 10-tile pit near the start shows the near-bush layer's dark green fill, so it reads more like dark ground than a gap.
