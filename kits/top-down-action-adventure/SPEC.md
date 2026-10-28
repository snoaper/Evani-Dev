# SPEC: top-down action-adventure (kit 004)

Demo: **Mossgate Keep** (`apps/top-down-action-adventure`). Kit crate `kits/evani-kit-top-down-action-adventure`, id `top-down-action-adventure` (params `kit.top_down_action_adventure.*`, owner `kit.top-down-action-adventure`). Target pack: [target/](target/) (reference prompt, look bible, moments, HUD layout, playbook). Catalog entry: `catalog/genres.json`, `top-down-action-adventure`.

Greybox pass (KIT_PROCESS step 4, S5-K4a) and art pass (step 5, S5-K4b): the art pass replaced the rectangles in `draw.rs` with pixel art (`art.rs`) and 2D lights; the rules in `game.rs` stayed.

## Catalog constraints

- Camera: `top-down`, fixed on one room (16 x 11 tiles of 16 px = 256 x 176 art px) with a 24 px HUD strip above; no scrolling inside a room.
- Physics: `simple` per the catalog. The kit moves bodies kinematically on a tile grid and does not read `physics.*`; the build lists `physics = "simple"` in `evani.toml` as the catalog says (the module is a stub today).
- Look: `pixelhd`, with the `lights2d` effect. The kit never names a look: its frames are drawn in palette roles and coloured from the active look's `ramp.<name>` palettes (falling back to the look bible's colours), and the light colours come from the look's `light` palette.
- Compatible looks: all 2D looks in the catalog list; the kit draws sprites only (evani-render's `forward` stays off).

## Systems from the catalog

| Catalog system | Mechanics | Data / tunables |
|---|---|---|
| Room and screen transitions | Rooms sit on a grid. A doorway in a wall joins two rooms; walking out through an open doorway puts the hero at the opposite side of the neighbour, with an entry grace. Only the current room is simulated. | `data/level.json` rooms, links; `enter_grace_s` |
| Sword plus item hotbar | Slot 1: sword, always (`attack`). Slot 2: the bolt wand once found (`item`); fires a bolt that damages enemies, flies over pits and presses switches. | `sword_*`, `bolt_*` |
| Dungeon keys, switches and block puzzles | Small keys open locked links (one key per door, consumed). A floor switch (stood on, or hit by a bolt) latches its room's gate open. Pits block walkers, not bolts, so the vault's switch needs the wand. Pushable blocks are not in the greybox. | `level.json` tiles `S`, `G`, `P`; link `kind` |
| Heart containers | `max_hp` in half hearts; a container adds `container_hp` and refills. Hearts heal `heal_hp`. | ints |
| Overworld map | The HUD shows a small map of rooms seen so far (current room bright). | `level.json` grid positions |

Also: gems (count only). Pots, bones, banners and the other props are decoration only (`level.json` room `props`); dialogue is later.

## The loop

Title (press attack) -> Playing -> Won (touch the Dawn Shard after the Warden falls) or Lost (health 0). Pause with `pause`. Won/Lost wait 1 s, then attack returns to Title. Free roam (`PlayMode::FreeRoam`): starts in Playing, no crystal, no lost state: at 0 health the hero is put back at the room's entry with full health; nothing is reported on `PlayOutcomes` as a game over.

### Hero

Eight-way movement at `walk_speed` (diagonals normalised), four-way facing (last dominant direction). Collision box `hero_w` x `hero_h`. Attack: a swing of `sword_time_s` with a hitbox `sword_reach` x `sword_width` in front, hitting each enemy once per swing for `sword_damage`; `sword_cooldown_s` before the next. Hit enemies are knocked back and flash; a hit gives a `hit_stop_s` freeze. Item: a bolt (`bolt_speed`, `bolt_life_s`, `bolt_damage`) every `bolt_cooldown_s`. Taking a hit: `hurt_invuln_s` of blinking invulnerability, knockback, screen shake. Entering a room: `enter_grace_s` with no contact damage.

### Enemies (simple AI, all in `data` stats from params)

| Kind | Behaviour |
|---|---|
| Slime | Wanders: picks a cardinal direction or rests every `slime_turn_s` (seeded jitter). Contact damage. |
| Bat | Flies over pits; drifts randomly, homes in with a wobble when the hero is within `bat_sight`. Weak (1 hit). |
| Husk | Patrols; when the hero is in line within `husk_sight`: winds up `husk_windup_s`, charges `husk_charge_s` at `husk_charge_speed`, rests `husk_rest_s`. |
| Spitter | Stationary; every `spitter_period_s` winds up `spitter_windup_s` then fires a seed at the hero. |
| Warden (boss) | Chases, winds up, then dashes; below half health also fires a fan of `warden_fan_count` seeds. Dying drops hearts and, in Play mode, reveals the Dawn Shard. |

Dead enemies stay dead for the run. Drops: `drop_chance` of a heart (70 %) or a gem, from the seeded Pcg32.

### Rooms (Mossgate Keep, 6 rooms)

Gate Hall (start, heart, slimes) -> Cellar (hub: slimes, bats; N Armoury, S Vault, E locked to the Antechamber) ; Armoury (husk; bolt wand) ; Vault (two spitters; a switch on a pit island opens the gate over the alcove with the first key and a heart container) ; Antechamber (bats, husk; second key; E locked to the Warden) ; Warden's Hall (boss; Dawn Shard). The wand is needed for the first key (through the switch), the first key opens the Cellar's east door, the second key the boss door.

### Win and lose

Win: touch the Dawn Shard. Lose: health 0 (Play mode). Both are reported: `PlayOutcomes::game_over()` on lose; there is no timer and no lives.

## Data and tunables

- `data/level.json` (data file `top_down_action_adventure.level`): rooms (grid position, 11 rows of 16 tile characters `# . , P S G`), enemy and pickup spawns by tile, links (`open` or `locked`), the start. Validated when the kit builds (bad ids, wrong row sizes, links that do not match the grid).
- Every tuning value is a parameter `kit.top_down_action_adventure.<name>` (f32 or i32 with range, unit and description), registered through two tables in `lib.rs`; `params.json` holds baked values. Layout numbers are params too: `hud_height_px`, and the HUD positions in art px from `target/hud-layout.md` (`hud_hearts_x` 6, `hud_hearts_y` 6, `hud_heart_step` 8, `hud_keys_x` 84, `hud_gems_x` 116, `hud_icons_y` 6, `hud_slot_x` 154, `hud_slot_y` 3, `hud_map_x` 196, `hud_map_y` 3, `hud_label_y` 16).
- Art and light params: `shadow_alpha` (0.35, contact shadows and the shade strips under walls), `torch_light_radius` (72 art px) and `torch_light_intensity` (1.0), `hero_light_radius` (48 art px) and `hero_light_intensity` (0.35), `ambient_tint` (0.35: how far the unlit shade leans to the look's ambient light colour), `banner_band_alpha` (1.0: the dark band behind the title, pause and end-screen lettering, so floor cracks never show between the letters). The lights' overall levels are the look's `fx.lights2d.*` params.
- The simulation is a fixed 60 Hz step; randomness only from a `Pcg32` seeded from the app's seeded resource at game start; no wall clock.

## Input

Actions: `move_x`, `move_y` (axes: A/D, W/S, arrows, pad stick), `attack` (Space, J, Z, pad A), `item` (K, X, pad B), `pause` (Esc, P, pad Start). Touch: a movement stick drives `move_x`/`move_y`, buttons drive `attack` and `item`, and the pause button; nothing in the sim knows about touch. The layer stays on in every phase because the title, win and lose screens wait for `attack`.

## Effects

`effects = ["lights2d"]`: normal-mapped 2D lights on the High and Ultra tiers (the catalog's 2D render path), behind the kit's `lights2d` feature. Each frame the kit pushes flickering warm torch lights, a pulsing cool light on the Dawn Shard, cool lights on wand bolts and a latched switch, the Warden's core (stronger as it weakens) and a soft hero light; the renderer keeps the strongest the tier allows. Layers from FX up (sparks, bolts, the sword arc, text) and the HUD are unlit. Kit-drawn, on every tier: contact shadows under actors and pickups, shade strips under walls (catalog: top-down favours shadows/AO), hit sparks and flashes, dust puffs, screen shake and torch flicker.

## Art

All frames are original, drawn in code on the art grid (`src/art.rs`) in eleven palette roles and packed into one pixel-art sheet (`tda.*` frames; scene frames get normal maps `.n`, actors white hit flashes `.w`). Walls autotile by their open neighbours (16 frames), floors use a checker with seeded detail, doors split into halves (N/S and W/E), the hero has four facings with two walk frames, each enemy two frames (the Warden a bright one at half health and while winding up). The letterbox around the view is tiled with a dark backdrop frame.

## Playtest bot

Not written in the greybox pass (Chris, 2026-10-20: tests are deferred to the combined pass). Plan: win = the scripted route Gate Hall -> Cellar -> Armoury (wand) -> Cellar -> Vault (shoot the switch, take key) -> Cellar -> Antechamber (key) -> Warden's Hall, attacking enemies on contact; lose = stand still until health reaches 0.
