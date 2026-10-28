# evani-kit-top-down-action-adventure

A top-down action-adventure kit (kit 004: greybox S5-K4a, art S5-K4b). The
demo is **Mossgate Keep**: six single-screen rooms, a hero with a sword and a
wand, slimes, bats, husks, spitters, a locked door, a floor switch with a
gate, a boss (the Warden) and a Dawn Shard to win. It ships on the `pixelhd`
look with the `lights2d` effect.

- Spec: `kits/top-down-action-adventure/SPEC.md`. Target pack and genre
  playbook: `kits/top-down-action-adventure/target/` (`playbook.md`).
- Data: `data/level.json` (rooms as 16x11 tile rows, spawns, decoration
  props, links; about 5 KB). Registered as the data file
  `top_down_action_adventure.level`.
- Params: every number is `kit.top_down_action_adventure.<name>` (owner
  `kit.top-down-action-adventure`): `f32` tunables in `Tune`, integer ones in
  `Ints` (see `src/lib.rs`). HUD positions, shadow darkness and light sizes
  are params too.
- Actions: `move_x`, `move_y` (WASD, arrows, pad stick), `attack` (Space, J, Z),
  `item` (K, X), `pause` (Esc, P). Touch: a move stick, A and B buttons and a
  pause button, all driving those same actions (`src/touch.rs`).
- Modules: `level` (data), `game` (rules on plain data, fixed 60 Hz, seeded
  `Pcg32`, no trig, no clock), `ecs` (entities mirror the game's lists),
  `art` (the pixel art, drawn in code), `draw` (sprites, HUD strip, lights),
  `font` (3x5 pixel font), `touch`.
- Art: every frame is original and drawn in code in palette roles
  (`src/art.rs`), coloured from the active look's ramps (`ramp.<name>`, with
  the look bible's colours as fallback) and added to the sprite art as one
  pixel-art sheet of `tda.*` frames, with normal maps (`.n`) for the 2D
  lights and white hit flashes (`.w`). A look that ships a frame of the same
  name replaces it; the kit never names a look.
- Lights (feature `lights2d`): flickering warm torches, the cool Dawn Shard,
  wand bolts and a latched switch, the Warden's core and a soft hero light.
  The HUD, bolts, sparks and text are unlit. Off, nothing light-related is
  compiled in.
- Free roam: `KitPlugin::free_roam` is true. Under `PlayMode::FreeRoam` the
  hero starts in the gate hall, there is no Dawn Shard and a hero at 0 health
  respawns at the room entry instead of ending the run.
- Light by design: no image files (the sheet is built at start), no new
  shaders, no audio files, no baked tables.

Not in the kit yet (see SPEC): block puzzles, dialogue, sound.
