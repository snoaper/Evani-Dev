# Kit 006: Tactical FPS (`tactical-fps`)

Roadmap row 006 · physics `simple` · first look `toon` (the photoreal look is built separately and switched in later) · camera `first-person`.
Catalog entry: `catalog/genres.json` → `tactical-fps`. Rules and bots are ported from Hexagon (..\Hexagon, MIT, by Chris Pearson @Snoaper): `src/rules.ts`, `src/runtime.ts`, `src/bot.ts`, `game/tuning.json`. Everything is original content.

Crates: kit `kits/evani-kit-tactical-fps` (`KitPlugin::name()` = `tactical-fps`), look `looks/evani-look-toon`, demo `apps/tactical-fps`.
Param names use `kit.tactical_fps.*`. Target pack: [target/README.md](target/README.md), [reference prompt](target/reference-prompt.md), [look bible](target/look-bible.md), [moments](target/moments.md), [HUD layout](target/hud-layout.md), [playbook](target/playbook.md).

Approved differences from the genre (Chris, 2026-10-15; ROADMAP row 006, HEXAGON_PLAN sections 2 and 6): no round economy or buy menu (a fixed four-weapon loadout), and Hexagon's core hold instead of plant and defuse. Both are `[n/a]` in the playbook.

## 1. Gameplay systems

Every number below is a registered param `kit.tactical_fps.<name>` (default shown, from Hexagon's `tuning.json`, times in seconds) unless marked "data". Defaults live in `data/tuning.json`; the arena layout lives in `data/arena.json`. Both ship with `include_str!` and are registered with `data_file`.

### 1.1 Match flow

- `Title` (attract: slow orbit over the fortress; press Fire or Jump) → `Pre` (`pre_s` 6: warm-up at the bases, no damage) → `Play` → `Results` (`results_s` 9) → `Title`. A new arena seed (seeded `Pcg32`) each match.
- Two teams, Blue and Orange, `team_size` (4, i32) each. The player is on Blue; bots fill both teams.
- Win: the core hold (`hold_s` 8 s in the enemy core zone with no defender inside; the hold decays at half speed while nobody holds it), `kills_to_win` (25, i32) team kills, or when `round_s` (900) runs out the team with more kills (else a draw).
- Death: `respawn_s` (3) later at the team base, with `spawn_shield_s` (2) of no damage. Falling below `fall_death_y` (-30) kills. As in Hexagon, a fall or a self-kill (one's own blast) scores a kill for the other team; a kill by a foe scores for the killer's team. Outside `Play` (the warm-up) a fall just respawns, without scoring.

### 1.2 Movement (`body.rs`)

Kinematic cylinder soldiers on the fixed 60 Hz step (Hexagon's sub-step is 50 ms / 3, which is the same step).
- `walk_speed` 6.5 m/s, `accel_ground` 70, `accel_air` 14, `gravity` 20, `jump_speed` 7.
- Lifts: standing in a lift shaft carries the soldier up at `lift_up` 6 (jump held) or down at `lift_down` 4.
- Collision against floors (with holes), concrete cover cells, metal walls, the ring dividers and doors.

### 1.3 Weapons (`weapons.rs`)

Four weapons, cycled with `weapon`: pistol, sniper, rockets, grenades.
- Damage `pistol_damage` 22, `sniper_damage` 95, `rocket_damage` 110, `grenade_damage` 100. Cooldowns `pistol_cooldown_s` 0.28, `sniper_cooldown_s` 1.3, `rocket_cooldown_s` 1.0, `grenade_cooldown_s` 0.8. Ranges `pistol_range` 140, `sniper_range` 320.
- Pistol and sniper are hitscan rays (`trace`): the nearest soldier cylinder, concrete cell (not where holed) or metal within range. A concrete hit adds a hole.
- Recoil and movement spread (added, HEXAGON_PLAN section 2): each shot adds `pistol_recoil` / `sniper_recoil` (radians) that decays at `recoil_decay` per second; the spread cone is `spread_base` plus `spread_move` × speed fraction plus `spread_air` when airborne, sniper spread scaled by `sniper_spread_mult`. The cone sample is drawn from the seeded `Pcg32`.
- Rockets fly straight at `rocket_speed` 32; grenades are thrown at `grenade_speed` 17 plus `grenade_up` 3.5, bounce with `grenade_bounce` 0.45 and blow after `grenade_fuse_s` 2.2. Projectiles live at most `proj_life_s` 5.
- Blasts: `blast_radius` 4.5 with linear falloff and knockback; no team damage; the shooter takes `self_damage` 0.5 of their own blast. A blast adds a large hole to concrete in reach.

### 1.4 Destructible concrete (`holes.rs`, `collapse.rs`)

- Holes: quantised position and radius (`pistol_hole` 0.12, `sniper_hole` 0.3, `rocket_hole` 1.5, `grenade_hole` 1.25, depth `hole_depth` 0.04). A hole inside an existing one adds nothing; when a spatial bucket holds `hole_cap` (24, i32) holes, a new one merges into the nearest. Above `hole_max` (1800, i32) the oldest holes are dropped (replaces Hexagon's `makeRoom`).
- Collapse: each cover cell's solid share is measured against its holes (`cellMat`). A cell below `cell_min_solid` 0.35 is weak: it no longer counts as support and joins the group above it (it is not removed on its own). Each cover group then checks its support (`unsupported` and the `topple` support test: the convex hull of the solid footprint against the centre of mass, `support_min` 0.3, `tip_margin` 0.05); an unsupported group's cells become gone at once, as in Hexagon's final state.
- Left out for later pieces: the tipping fall animation, landing craters, floor islands that drop away, lamps, dents and debris (all visual or later-stage).

### 1.5 The ring (`ring.rs`)

The middle level's ring of cars turns one sector every `ring_interval_s` (180), taking `ring_rotate_s` (5), with a `ring_warn_s` (10) warning. Soldiers standing in a car are carried with it; the static doorways line up with a different car after each turn.

### 1.6 Bots (`bot.rs`)

Ported from Hexagon's `bot.ts`. A bot thinks every 3rd step and holds that input for 3 steps (Hexagon's tick).
- Roles: attackers route level by level (lifts, doorways) towards the enemy core; defenders hold their own base and core.
- Sight `bot_sight` 95 m with a line-of-sight trace; a reaction delay `bot_react_s` 0.45 before firing; aim error `bot_aim_error` 0.05 that settles while the target stays in view.
- Weapon by range: sniper far, pistol mid, rockets or grenades close or at groups behind cover.
- They sidestep craters and blast their way out when stuck.

### 1.7 Free roam

Under `PlayMode::FreeRoam` (read once at build): no bots, no score, no timer, no win or results; a fall respawns at the base without scoring; the ring still turns; shooting concrete still makes holes. `KitPlugin::free_roam` returns true.

## 2. Camera

First person at the player's eye (1.6 m), yaw and pitch from `look_x/look_y` (sticks, arrow keys; `look_speed` 2.6 rad/s) and `aim_x/aim_y` (mouse deltas, `mouse_sens` 0.0025 rad/px), `fov_deg` 75. On the title a slow orbit over the fortress. A `MomentCam` resource lets the capture moments place the camera.

## 3. Input and touch

Actions: `move_x` (`keys:KeyA/KeyD`, pad axis 0), `move_y` (`keys:KeyW/KeyS`, pad axis 1), `look_x` / `look_y` (arrow keys, pad axes 2 and 3), `aim_x` / `aim_y` (mouse deltas, kept apart so pixel deltas never swamp the keys), `fire` (mouse 0, `KeyF`, pad 7), `jump` (Space, pad 0), `weapon` (`KeyQ`, pad 3), `pause` (Escape, `KeyP`, pad 9).
Touch (evani-core `touch`): a dynamic move stick on the left half, a dynamic look stick on the right half, hold Fire, tap Jump, tap Weapon, pause top right. Every touch control names an action the keys already drive.

## 4. Physics, look and effects

- Physics `simple` (catalog `tactical-fps`): the kit's own kinematic movement; the physics module is in the build for gravity and later pieces.
- Look `toon` (compatible with tactical-fps in the catalog) until the photoreal look lands. Greybox: concrete light grey bevel cubes, metal dark blue-grey, Blue team `#3a7bff`, Orange team `#ff8a2a`, core zones in team colour (look bible).
- Effects: `bloom` (the core zones and muzzle flashes glow), `vignette` (keeps focus on the crosshair), `shadows` (soldiers and cover read as standing on the floors). All three fit the catalog's look style.

## 5. HUD

A `MatchHud` text resource the kit fills each frame (phase banner, timer, score, health, weapon, ring status, core hold); the demo app writes it into `#tactical-fps-hud`. Layout in `target/hud-layout.md`.

## 6. Playtest bot plan (for the later test pass)

- Win: the player is driven by the bots' own attacker logic (the same `bot.rs` brain on the Blue player's slot); it routes to the Orange core and holds it, or the team reaches `kills_to_win`. Pass: `Results` with Blue the winner.
- Lose: the player stands still at the base; Orange's attackers hold the Blue core or time runs out (`round_s` lowered for the test). Pass: `Results` with Orange the winner or a draw.
- Free roam: `evani_core::play::free_roam_test` with a WASD and Space script; no results, no timer.

## 7. Left for later pieces

View model and photoreal art (K6c), networking and lag compensation (K6b), Hexagon's lobby (colour pick and ready up; here a single local player), fall animation, craters, floor drops, lamps, dents, debris and the cat creatures in Hexagon's tuning (not part of its rules).
