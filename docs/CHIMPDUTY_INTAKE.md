# ChimpDuty intake

ChimpDuty (..\ChimpDuty) handed Evani.Dev a set of gap crates
(`handover/evani-dev/gaps/`) for pieces the engine did not have. Chris chose
"Everything now" (2026-10-21): every gap comes into the engine now, even
where STAGES.md would put it later (DECISIONS.md "ChimpDuty intake:
everything now"). Each gap gets one section here, written on its own intake
branch; a later worker updates ChimpDuty's HANDOVER.md from these sections.

## Collision (`evani-physics-collide` gap)

- **Engine name:** `evani-physics` feature `collide`, module
  `evani_physics::collide` (crates/evani-physics/src/collide/). Not a new
  crate: the physics crate already gates its backends (`simple`, `rigid`)
  as features, and the gap itself proposed this.
- **Feature flag:** `evani-physics = { workspace = true, features = ["collide"] }`
  (or, in an app crate, a feature that forwards `evani-physics/collide`). Off
  by default; a game without it downloads none of it (the four reference
  ship builds are byte-identical with and without the branch).
- **Branch and commits:** `intake-collide`, from main 54fe7bb5; the module is
  commit 9f0f755c, the sandbox proof 61e6e364, the layout fix and draft notes
  173685dd, the final notes and DECISIONS entry follow (`git log intake-collide`).
- **Bytes:** off, the four reference ship builds are byte-identical. On (sandbox
  wasm): +2,482 B raw / +985 gzip for the feature alone, +30,649 / +14,538 with
  the proof scene.
- **API:** the same as the gap's, under the new path.
  - `WorldBuilder::new()`, `.triangle(a, b, c, tag)`, `.mesh(positions, indices, tag)`,
    `.aabb(min, max, tag)`, `.oriented_box(centre, half, axes, tag)`, `.build() -> StaticWorld`.
  - `StaticWorld::raycast(origin, dir, max) -> Option<RayHit { t, point, normal, tag, .. }>`,
    `raycast_filtered`, `line_of_sight(a, b)`, `line_of_sight_filtered` (new: skip tags,
    for glass or foliage), `query_aabb`.
  - `StaticWorld::cast_capsule`, `cast_sphere` (thrown and fast objects; no tunnelling),
    `contacts`, `overlaps`, `depenetrate`; `Capsule::upright`, `Capsule::sphere`.
  - `StaticWorld::move_character(feet, height, motion, &CharacterConfig, stick) -> MoveResult`
    (slide, step-up, slopes, ceilings, ground snap).
  - `collide::trig::{sin, cos, atan2, radians}`: deterministic trig (same bits native and wasm).
  - `CollideParams` (a world resource when `PhysicsModule` is in the app): the live
    tunables `physics.collide.radius`, `.height`, `.step_height`, `.max_slope`, `.skin`,
    `.snap_distance`, `.max_slides`, `.depenetrate_passes`, `.speed`;
    `CollideParams::config(&registry)` gives the mover config from their current values.
- **What ChimpDuty changes to switch over:**
  1. Cargo: drop the workspace member `handover/evani-dev/gaps/evani-physics-collide`
     and its `[workspace.dependencies]` line; depend on `evani-physics` with
     `features = ["collide"]` in apps/chimp-game, crates/chimp-combat and crates/chimp-levels
     (the engine's `evani-core` comes with it, for the parameters).
  2. Imports: `evani_physics_collide::X` becomes `evani_physics::collide::X` (same names:
     `StaticWorld`, `WorldBuilder`, `CharacterConfig`, `Capsule`, `V3`, ...).
  3. Sizes: the engine has no chimp preset. ChimpDuty already passes its own sizes
     (`CharacterConfig::new(PLAYER_RADIUS, PLAYER_STEP, PLAYER_MAX_SLOPE_DEG)` in
     player.rs, chimp-combat's tests and chimp-levels' graybox test), so this keeps
     working. `CharacterConfig::default()` is now a generic adult (0.3 m radius,
     0.35 m step, 45 degrees), not the chimp.
  4. `CharacterConfig` has a new field `depenetrate_passes` (default 4, the gap's
     fixed count). Code that builds it as a struct literal must add it; code that
     uses `CharacterConfig::new` needs nothing.
  5. Optional, to tune live: add `evani_physics::PhysicsModule` to the app and read
     `CollideParams::config(&registry)` each fixed step instead of a constant config
     (then `param_set physics.collide.step_height` and the others change the mover
     on the next step). The defaults are the generic ones, so set the chimp's values
     as baked parameters (`physics.collide.radius` and the rest) if the app uses them.
  6. Optional, for determinism across devices: the player's `yaw.sin()` and
     `cos()` (player.rs `forward_xz`, `look_dir`) can use `collide::trig::sin` and
     `cos`, which give the same bits on every platform.
- **Behaviour:** the same results as the gap within float rounding. The maths
  now avoids the platform library: `f32::min`, `max`, `clamp` and `hypot` are plain
  comparisons and `sqrt`, so a few results can differ from the gap in the last bit;
  ChimpDuty's graybox test (walk the map) should be re-run once after switching.
- **Proof in the engine:** the sandbox feature `collide-proof` (a capsule walking
  stairs, a platform, a ramp and a wall corner), with headless captures in
  .evani-shot-intake/collide/ of the main checkout.

## Combat (`evani-combat` gap)

- **Engine name:** the new opt-in crate `evani-combat` (crates/evani-combat/),
  built on `evani-physics` feature `collide`. A crate of its own, not a
  feature of an existing one: it is gameplay (guns, health, bots) that only
  shooters want, and a kit that does not depend on it downloads none of it.
- **Feature flag:** a kit depends on it with `evani-combat.workspace = true`
  (it turns on `evani-physics/collide` itself); an app crate puts it behind
  its own feature, as the sandbox's `combat-proof` does
  (`combat-proof = ["dep:evani-combat", "dep:evani-physics", "evani-physics/collide"]`).
  Off by default; the five reference ship builds are byte-identical with and
  without the branch.
- **Branch and commits:** `intake-combat`, from 085416a3 (the collide intake);
  7ac31e4e (the crate), 5bbef5ea (the sandbox proof), 460e65c6 (proof layout), then the byte check and these notes (`git log intake-combat`).
- **Bytes:** `EVANI_GIT_COMMIT=4b61971`, `cargo xtask build <id> --ship --no-diff`: match-3, sandbox, base-building-rts, kart-racer and tactical-fps have the same sha1, size and gzip size as `sha-join.txt`. When on (sandbox, ship feature list, release trunk builds): base 648,583 B (286,596 gzip); plus `evani-physics/collide` alone 651,065 B (287,581), +2,482 B (+985); plus the whole combat-proof scene (crate, collide and the proof) 710,715 B (316,007), +62,132 B (+29,411) over base.
- **API** (the gap's, with the changes listed below):
  - `hitscan`: `Body::new(feet, height, eye)`, `trace(world, origin, dir, range, bodies, skip) -> Option<Hit>`
    (`Hit::World { .. }` with the surface tag, `Hit::Body { index, zone, .. }`; nearest first,
    a wall in front of a body stops the bullet), `ray_body`, `nearest_on_axis` (new).
  - `gun`: `Gun::new(WeaponStats)`, `.tick(dt)` (recoil recovery, reload, heat), `.fire(trigger, yaw,
    pitch, Stance, run_speed, &mut Pcg32) -> Option<Shot>`, `.spread`, `.start_reload`, `cone_dir`;
    `damage_at(&WeaponStats, t)` (new: the range falloff `damage * range_mod ^ (t / RANGE_UNIT)`,
    `RANGE_UNIT` = 9.525 m).
  - `health`: `Health::new(hp, armour, helmet)`, `.take(dmg, pen, Zone) -> Damage`, `Zone::mult`.
  - `grenade`: `Grenade::thrown(from, yaw, pitch, thrower_vel, &GrenadeStats, owner)`,
    `.step(world, stats, dt, &mut events) -> bool` (`GrenadeEvent::Bounce`, `Explode`).
  - `rocket`: `Rocket::launched(from, dir, &RocketStats, owner)`, `.step(world, bodies, stats, dt) -> Option<Impact>`.
  - `blast`: `blast_damage(world, centre, damage, radius, &Body)` (linear falloff, line of sight
    to the chest or head), `blast_sees`, `nearest_on_body`.
  - `path`: `shortest_path(&impl path::Graph, from, to)` (Dijkstra), `smooth(points, clear)` (string pull).
  - `bot`: `Brain::new(BotTune, seed)`, `.think(&BotIn, &dyn Router) -> BotOut`, `BotState`, `Foe`,
    `Sound`, `Orders`; `Router` is the game's route finder.
  - `tune`: `Tuning::default()` (the gap's `Tuning::cs()` numbers), `WeaponStats`, `GrenadeStats`,
    `RocketStats`, `BotTune`, `SoundTune`.
  - `params`: `CombatModule` (`evani.module.combat`) registers 114 live `combat.*` parameters
    (`combat.<rifle|pistol|launcher>.<field>`, `combat.grenade.*`, `combat.rocket.*`,
    `combat.bot.*`, `combat.sound.*`, `combat.health`, `.armour`, `.helmet`, `.run_speed`,
    `.respawn`), owner `combat`, baked into the kit's parameter file; `CombatParams::apply(&registry,
    &mut tuning)` refreshes a table every fixed step without allocating, `CombatParams::tuning`
    builds a fresh one.
  - Helpers: `look_dir`, `yaw_pitch_of`, `wrap`, `dist`, `dist_xz`, `dir_to`.
- **What ChimpDuty changes to switch over:**
  1. Cargo: drop the workspace member `handover/evani-dev/gaps/evani-combat` (Cargo.toml line 24)
     and its `[workspace.dependencies]` line (line 67); point `evani-combat` at the engine
     crate (`crates/evani-combat`, same pinned engine revision as the other engine crates) in
     crates/chimp-combat, crates/chimp-levels and apps/chimp-game. `serde_json` is no longer pulled
     in by it.
  2. Random numbers: `evani_combat::Rng` is gone; use the engine's `evani_core::rng::Pcg32`
     (`Pcg32::seeded(0xC4_1D_D0_77)` where chimp-combat had `Rng::new(..)`). `Gun::fire` and
     `cone_dir` take `&mut Pcg32`. `Brain::new(tune, seed)` is unchanged.
  3. Names: `Tuning.he` is now `Tuning.grenade` (chimp-combat lib.rs `t.he.carry`, `self.tune.he`,
     the tests); `Tuning::cs()` is `Tuning::default()` with the same numbers; the default weapon
     and grenade names are `launcher` and `grenade` (were `rocket` and `he`). Nothing in the
     engine says CS or chimp.
  4. Tuning file: the JSON loader `Tuning::from_json` is gone (the engine keeps tuning in live
     parameters). Either add `CombatModule` to the app and bake ChimpDuty's numbers as `combat.*`
     parameters (then read `CombatParams::apply` each fixed step, and `param_set combat.rifle.damage`
     and the rest change the fight live), or build `Tuning::default()` and override the fields in
     code. The test that compares the JSON file with `cs()` goes.
  5. New, optional: `damage_at` gives the range falloff the gap computed inline; `nearest_on_axis`
     and `RANGE_UNIT` are public.
- **Behaviour:** the same results as the gap within float rounding. The maths avoids the
  platform library (`collide::trig` sine, cosine and atan2, `dmath::powf` for the falloff,
  plain-comparison min, max and clamp), so a few results can differ from the gap in the last bit;
  ChimpDuty's fight tests should be re-run once after switching.
- **Proof in the engine:** the sandbox feature `combat-proof` (a bot routes round a wall to a
  target, fights it with the rifle, tracers and hit marks, health and armour bars), with headless
  captures in .evani-shot-intake/combat/ of the main checkout. Do not turn it on together with
  `collide-proof` (both stand at the same spot).
- **Adopting it in tactical-fps (a note only; the kit is not changed by this intake):**
  - weapons.rs `fire` (ray against the arena, `ray_body`, spread and recoil) becomes `Gun::fire`
    plus `trace`, and the damage line becomes `damage_at` plus `Health::take`.
  - weapons.rs `Projectile` and `step_projectiles` become `Grenade::step` and `Rocket::step`;
    `explode` becomes `blast_damage` per body (the kit keeps its own hole cutting and collapse,
    which the engine does not have).
  - bot.rs `scan` and `think` become `Brain::think` with the kit's own `Router` over its arena
    graph (`plan` and `wander` become `Orders`).
  - tune.rs and data/tuning.json become `combat.*` baked parameters read with `CombatParams::apply`.
  - The arena's voxels become a `StaticWorld` built from the arena boxes (`WorldBuilder::aabb`
    per solid box, the material as the tag), rebuilt when a wall is cut.

## Render limits (branch `intake-render-limits`)

ChimpDuty's handover (`..\ChimpDuty\handover\evani-dev\HANDOVER.md`) lists five places where the engine held the game back. Chris chose "Everything now" (DECISIONS.md, "ChimpDuty intake: render limits"), so all five are in the engine, each behind its own opt-in feature. A game that turns none of them on builds byte for byte what it built before (checked on the eight ship builds, see "Bytes" below).

Proof: the sandbox features `render-limits-before` (the scenes on the engine as it was) and `render-limits-proof` (the same scenes with the new features), `apps/sandbox/src/proof_limits.rs`, scenes picked with `?rl=near|fog|haze|pcss|warm`. Neither feature is in the sandbox's `evani.toml` or `default`. The page title carries the numbers. Pictures and run files: `C:\dev\evani-dev\.evani-shot-intake\render-limits\` (`before-*.png`, `after-*.png`, one `.json` per run).

| Limit | Feature | Crate | Setting or API |
|---|---|---|---|
| 1a. Camera near plane | `camera-range` | evani-render (evani-fx `camera-range` for decals, outline, fog) | `render.camera.near`, `render.camera.far` |
| 1b. First-person models | `view-model-models` | evani-render | `Models::draw_view_model(_with)` |
| 2. Looks switching effects on | `look-effects` | evani-fx | `LookContext::request_effect(_from)` |
| 3. Colour clamp, fog colour | `hdr-color`, `fog-color` | evani-render | `render.clear_color` up to 64, `render.fog.own_color`, `render.fog.color` |
| 4. Shader pre-warm | `prewarm` | evani-render | `prewarm::Prewarm` resource (optional) |
| 5. PCSS speckle | `pcss-stable` (implies `pcss`) | evani-render | none (same `pcss_params` slot) |

### 1a. Camera near and far planes

**Was:** every world projection used the fixed `shaders::NEAR` 0.1 m and `FAR` 1000 m, so a first-person eye against a wall cut the wall away.

**Now (`camera-range`):** two live params, read once a frame at the start of the 3D path into a per-thread cell, so `shaders::view_proj` keeps its signature and every user of the camera gets the same planes that frame: the forward, model and particle frame uniforms, frustum culling and the GTAO depth linearisation; with evani-fx's `camera-range` (which turns the render one on) also decals' and the outline's depth reconstruction and the fog's far fade.

- `render.camera.near`: metres, default 0.1, range 0.005 to 10.
- `render.camera.far`: metres, default 1000, range 10 to 100000.
- The far plane is kept at least 10x the near plane; non-finite values fall back to the default. Depth is reversed in a 32-bit float buffer, so a near plane 10x closer costs almost nothing in precision far away.
- `camera_range::{from_params, resolve, sync}` for tools that project the camera themselves.

**ChimpDuty switches over:** turn on `evani-fx/camera-range` (it brings `evani-render/camera-range`) and set `render.camera.near` to 0.01 to 0.03 in the kit's params JSON (and `far` to the map's size if it is over 1 km). Drop any near-plane workaround.

### 1b. First-person models

**Was:** the `view-model` layer (its own field of view, near plane and depth buffer) took built-in and custom meshes only, so a loaded gun or arms model had to be drawn in the world and folded in front of the camera.

**Now (`view-model-models`, implies `view-model` and `models`):** `models.draw_view_model(ModelInstance)` and `models.draw_view_model_with(instance, paints)` take an instance in camera space (x right, y up, -z forward, metres from the eye; rotation and scale camera-relative). The renderer turns them into world space through the frame's camera (the first split-screen view when there are views) and draws them in the `view_model` pass after the view-model meshes, with the one-sample model pipeline, the world's sun, shadow map and point lights. They draw their finest level (level 0 plus shared parts), cast no shadow, are not culled and are cleared every frame. Node hiding (`hide`) and recolours work as in the world. Skinned parts are skipped (pose the arms in the model, or draw one static model per pose).

**ChimpDuty switches over:** turn on `evani-render/view-model-models`; each frame call `models.draw_view_model(ModelInstance::new(id, [x, y, -z]))` for the gun and the arms instead of the world draw and fold trick.

### 2. Looks asking for an effect

**Was:** a look could only veto an effect (`post_effect(name, false)`); the tier's `effects_on` list decided what ran, so fog or grade could not be switched on from a look.

**Now (evani-fx `look-effects`):** in `LookPlugin::build`, `look.request_effect("fog")` (or `request_effect_from("fog", min_points)`). The rule, in this order (`evani_core::look_fx`):

1. The request only counts for an effect compiled into the game, and only on tiers with at least the request's points: default 9, which is VR, Medium, High and Ultra; Low (7) and Lite (5) veto it. `request_effect_from(name, 0)` asks on every tier.
2. A look that also vetoes the effect wins: the veto is applied after the request.
3. The effect then goes through the tier's budget, the VR comfort rule and adaptive quality like any other: wanted on, never forced.

The requests live in a `LookEffectRequests` world resource made by the first request, not in `LookData` or a tier, so a game whose look never calls it carries none of it.

**ChimpDuty switches over:** turn on `evani-fx/look-effects` (and the effects it wants, for example `evani-fx/fog`), and in its look's `build` call `look.request_effect("fog")` (and `"grade"` if wanted). Remove any per-tier `effects_on` edits made only to get them on.

### 3. HDR background and haze, and a fog colour of its own

**Was:** colour params clamp to 0..1, so `render.clear_color` could not match an HDR horizon, and the fog always faded to the background colour.

**Now:**

- `hdr-color`: `render.clear_color` takes linear values up to 64 (`color_ext::HDR_MAX`). The scene renders into Rgba16Float before exposure and the tonemap, so a background of 3.0 is a bright horizon the tonemap rolls off.
- `fog-color`: `render.fog.own_color` (bool, default off: the fog keeps the background colour, as before) and `render.fog.color` (the haze colour while it is on, HDR too with `hdr-color`). The forward and model shaders already took the fog colour as its own uniform; only the colour that goes into it changed. `color_ext::fog_color(reg, background)` gives the frame's value.

**ChimpDuty switches over:** turn on `evani-render/hdr-color` and `evani-render/fog-color`; set `render.clear_color` to the dome's horizon value without the `fit` scale-down, `render.fog.own_color` true and `render.fog.color` to the sky's haze colour. The fog slot can stop leaving the dome alone by hand.

### 4. Shader pre-warm

**Was:** the canvas started drawing as soon as the engine's base pipelines existed. Everything else was still being built while the game ran: the 4x MSAA variants, the model, skin and effect pipelines and, the big one, a kit's material code: `KitMaterial::set_wgsl` on the first frame rebuilds the forward pipelines (1x and 4x) and builds the cut-out shadow pipeline. The browser builds a pipeline in its GPU process in the order commands arrive, so the frame after a late build waits for it.

**Now (`prewarm`):** after the base pipelines exist the frame keeps being skipped (the game's systems still run, the canvas shows its loading state) while the render module

1. builds the kit material code set so far (before, it was only seen inside the draw path, once the game was on screen),
2. waits until no pipeline build has been in flight for `quiet_frames` (3) frames, after at least `min_frames` (8),
3. asks the GPU queue once for "submitted work done" and waits for the answer, which comes only after the GPU process has finished every pipeline it was asked for (a new build after the ask starts the wait again).

`max_ms` (6000) caps the hold. A game may insert its own `evani_render::prewarm::Prewarm` before the first frame to change the numbers; after the hold `held_ms`, `held_frames` and `timed_out` say how it went.

**Measured (the `warm` scene, frame gaps in the first 4 s after the first draw, four runs each, a busy PC):** before, the longest gap was 250 ms with three gaps over 100 ms (250, 247, 218 ms), then 50, 33, 33. After, the longest was 115 ms (one over 100 ms), then 56, 19, 19. The hold itself lasts 123 to 139 ms (9 frames) and never timed out. The numbers are noisy because the PC was loaded; read them as "cold-cache stalls of about 250 ms are gone in most runs, and the hold costs about 130 ms before the first picture".

**ChimpDuty switches over:** turn on `evani-render/prewarm`. Set the material-proc code (`KitMaterial::set_wgsl`) in the kit's first frames as today; it is built during the hold.

### 5. PCSS speckle under bodies lying on the floor

**Cause (reproduced in the `pcss` scene, a body lying on a floor under a sun 14 degrees up):** two things in `shadows_pcss.wgsl`.

- Self-blocking: the blocker search compared every tap with the centre's depth, so on a floor at a low sun the floor's own texels a few texels towards the light counted as blockers (a slope the constant bias cannot cover); the blocker distance came out near zero and the filter shadowed part of its own taps.
- Noise: the taps were spun by a hash per shadow texel, so the part-shadowed taps changed from texel to texel into a blocky speckle.

**Now (`pcss-stable`, implies `pcss`):** new `shadows_pcss_stable.wgsl` with `shadows_one_stable.wgsl` and `shadows_two_stable.wgsl` (the one- and two-cascade lookups pass the receiver's normal); the existing WGSL is untouched. Each tap is compared with the receiver plane's own depth at that tap (receiver-plane depth from the normal through the light matrix); the tap pattern is a fixed golden-angle spiral; the filter radius is at least 1.5 texels. 16 taps on tiers with a PCF radius (High, Ultra), 8 without. Same `pcss_params` slot and penumbra model.

The lesson of the first version: it used one texel of slope as the margin, and at a 14 degree sun that is over 30 cm of depth, so real shadows (the limb and head of the lying body) were lost with the speckle. The shipped version blends the four texels of each tap by hand, each against the plane at its own centre, with a margin of 0.2 texel and a cap of 16 (test `the_margin_stays_small_at_a_low_sun`).

**Measured (the `pcss` scene, zoomed on the floor under the body):** before, a jagged sawtooth band across the floor under the body; after, a smooth penumbra with the limb and head shadows kept (`pcss-zoom-shot.png` after, `before-pcss-shot.png` before; the first version is kept as `pcss-zoom-v1-shot.png` and `after-pcss-v1-shot.png`).

**ChimpDuty switches over:** where it turns on `evani-render/pcss`, turn on `evani-render/pcss-stable` instead (or as well). No look change.

### Proof scenes and numbers

Each scene is run twice in headless Chrome (WebGPU) on the same PC: once on the `render-limits-before` build and once on `render-limits-proof`. Pictures and run files are in `C:\dev\evani-dev\.evani-shot-intake\render-limits\`.

| Scene | Before | After |
|---|---|---|
| `near` (eye against a wall, held model) | `render.camera.near` absent (0.1 m): the wall is cut away | near 0.01 m: the wall stays, the held model is drawn on top (`after-near-shot.png`, `after-near-vmworld-shot.png`) |
| `fog` at Medium | fog off | fog on (the look asks for it) |
| `fog` at Low | fog off | fog off (the Low tier vetoes a request by default) |
| `haze` | background clamped to 1.0, white; fog always the background colour | background 3.2 / 2.5 / 1.7 (a bright warm horizon), fog its own colour 2.2 / 2.0 / 1.8 |
| `pcss` (body on a floor, sun 14 degrees up) | jagged sawtooth band | smooth penumbra, limb and head shadows kept |
| `warm` | longest frame gap 250 ms in the first 4 s | longest 115 ms; hold 123 to 139 ms |

Exceptions: none in any run. Frame gaps outside the `warm` scene were between 20 and 50 ms on most runs, with some stalls of 100 ms and more on this loaded PC in both builds (a first-run shader build, not the new code).

### Bytes

**Games that turn nothing on: 0 bytes.** The eight ship builds (`2d-platformer`, `base-building-rts`, `kart-racer`, `match-3`, `model-view`, `sandbox`, `tactical-fps`, `top-down-action-adventure`, built with `EVANI_GIT_COMMIT=4b61971`) have the same SHA-1 for their `.js` and `_bg.wasm` files as the same eight built on the join-3 baseline (`89abbe44`). Only the file name differs, because it carries a hash of the build folder's path (and for tactical-fps one leading zero of that hash).

| Game | wasm bytes | wasm gzip |
|---|---|---|
| 2d-platformer | 666,273 | 285,988 |
| base-building-rts | 724,405 | 309,097 |
| kart-racer | 1,028,634 | 437,153 |
| match-3 | 682,984 | 300,891 |
| model-view | 566,311 | 251,116 |
| sandbox | 648,143 | 286,497 |
| tactical-fps | 747,879 | 325,302 |
| top-down-action-adventure | 608,472 | 260,134 |

**Games that turn everything on:** the sandbox with `render-limits-proof` (all five features) against the same scenes on `render-limits-before` is +12,099 bytes raw and +4,922 bytes gzip in the wasm. Each feature costs only what it is used for; a game that wants one pays for that one.

## Materials, sky, daylight look and ACES (branch `intake-materials`, HANDOVER sections 10 and 12)

Chris chose "Everything now" (2026-10-21): the procedural materials, the sky
dome, the daylight look and the tone-map check all come in, the sky leaves
the Parking lot, and `evani-material-proc` is the start of the engine's
shared materials library. Commits on `intake-materials` (from main
54fe7bb5): ecbd48df (crate, sky, daylight preset, ACES test), b9aa92cf
(lint clean, sandbox `material-proof`), b9c63c11 (catalog-fit colour, param
guards, optional-crate list), db2c9683 (daylight preset without a Cargo
feature, so tactical-fps's bytes stay the same; notes drafted), then the
bytes, DECISIONS and docs commit.

### 1. Procedural surface materials and baked AO

- **Engine name.** Crate `crates/evani-material-proc` (workspace member and
  workspace dependency). Listed in `tools/evani-guide` `OPTIONAL_CRATES`, so
  `cargo xtask check` refuses a game that carries it as a plain dependency.
- **Feature.** The crate itself is the opt-in: a game lists it as an
  optional dependency behind one of its own features. Only then do
  evani-render's `forward` and `kit-material` turn on; a game that doesn't
  list it downloads none of it (byte check below).
- **API.**
  ```rust
  // Everything in one call: bake, register the material.* params, add the
  // world resource and its draw system.
  evani_material_proc::install(ctx, scene)?;

  // Or by hand (e.g. to start the sky from other values):
  let mut world = ProcWorld::new(scene)?;          // bakes and packs
  world.register_params(&mut registry)?;           // material.* (and sky.*)
  ctx.resource(world);
  ctx.system(evani_material_proc::system());
  ```
  `Scene { boxes: Vec<ProcBox>, regions: Vec<Region>, default_region }`,
  surface ids in `surface::*` (WALL, FLOOR, PLASTER, SANDSTONE, CONCRETE,
  CEILING, WOOD_CRATE, METAL_CRATE, STONE_STEP, PLATFORM, RAMP, PAVING,
  BRICK_PAVING, TRIM_STONE, TRIM_WOOD, SIGN_A, SIGN_B, ARROW, GROUND). The
  look provides white `world` and `decal` materials (`WORLD_MATERIAL`,
  `DECAL_MATERIAL`).
- **Live params** (owner `material-proc`, 61 of them):
  `material.<surface>.color` for every surface, `.wear`, `.grime` and
  `.streaks` for the surfaces that weather, `material.ao.strength` and
  `material.ao.warmth`. The draw system re-packs only the tune records, and
  only when the registry revision changes. `ProcWorld::set_tuning` does the
  same without params.
- **Layout.** `kit_data` HEADER is 8 (record 7: tune base, count, AO
  strength, AO warmth; then two tune records per surface id).
- **Tests.** Param ranges and defaults, AO bake determinism (pinned hash
  `0x8252_1005_e2c1_6434`), layout `find` against the CPU replica, the WGSL
  validated with naga on its own and spliced into the forward and shadow
  passes.

### 2. Sky dome with clouds

- **Engine name.** `evani_material_proc::sky` (`SkyParams`, `SkyHandles`,
  `SKY_MATERIAL`), WGSL in `crates/evani-material-proc/shaders/sky.wgsl`.
- **Feature.** `evani-material-proc/sky`. Without it the crate and its
  shader carry no sky code (a test checks the composed WGSL).
- **API.** `ProcWorld.sky: SkyParams` (start values); the dome is drawn by
  `system()` only when the look has an unlit `sky` material. Live params
  (14): `sky.turbidity`, `sky.rayleigh`, `sky.mie_coefficient`,
  `sky.mie_directional_g`, `sky.cloud_scale`, `sky.cloud_speed`,
  `sky.cloud_coverage`, `sky.cloud_density`, `sky.cloud_elevation`,
  `sky.brightness`, `sky.sun_disc`, `sky.follow_light` (default on: the sun
  is `render.light.direction`, the shadow light), and `sky.sun_elevation` /
  `sky.sun_azimuth` (used when `follow_light` is off; default 35 and 56
  degrees). `SkyParams::radiance` and `ring` are the same clear sky on the
  CPU.
- **Licences.** Ported from three.js `examples/jsm/objects/Sky.js` (MIT,
  notice in the WGSL and THIRD_PARTY_NOTICES.md); the model is Preetham,
  Shirley and Smits, SIGGRAPH 1999 (credited in both places).
- **Tests.** Sky colour at known sun angles (zenith pinned at 30 degrees),
  the records, the follow-light switch.

### 3. Daylight look preset

- **Engine name.** `evani_look_photoreal::daylight::DaylightLook` (look name
  `photoreal`) and `PhotorealLook::daylight()`.
- **Feature.** None on purpose: a game opts in by adding `DaylightLook`
  instead of `PhotorealLook`, and a game that never names it links none of
  it. Declaring any Cargo feature on the look changes its `-C metadata`
  (cargo 1.98) and so the bytes of every game that links photoreal
  (tactical-fps), even with the feature off. The look never depends on
  the materials crate: its sky-matched colours are constants, and
  `crates/evani-material-proc/tests/daylight_match.rs` checks them against
  the sky.
- **What it sets.** `world`, `decal` and unlit `sky` materials first (low
  slot numbers), then the photoreal look; a golden low sun (15 degrees up,
  56 degrees azimuth, colour `[1, 0.78, 0.52]`, intensity 2.9), exposure
  0.62, a warm-neutral ambient, the clear colour at the dome's horizon, a
  haze fog slot that leaves the dome alone past 450 m, bloom off. The sky
  tints (`render.sky.*`) and the faint matte reflection
  (`render.ibl.specular` 0.015, roughness 0.95) are set only when the game
  builds evani-render `ibl`; `fx.shadows.distance` 45 only when it lists
  shadows.
- **Catalog fit.** Grading is photoreal's own filmic stage (white 8, toe
  0.02) and grade (contrast 1.1, saturation 0.88, split 0.3, warm
  highlights, cool shadows): the catalog describes photoreal as
  desaturated, so the colour is pulled back a little from full. The catalog
  has no sky or daylight entries; nothing in it is contradicted, but a
  `daylight` variant of photoreal is not listed there (gap, see below).

### 4. ACES tone map

- **Found already in the engine.** evani-core's default `tonemap` slot
  body is the Narkowicz 2015 ACES fit (CC0). No new curve was added; a test
  (`aces_tests` at the end of `crates/evani-core/src/slots.rs`) reads the
  five constants from the WGSL and checks known values (0.18 to 0.2669, 1.0
  to 0.8038, monotonic, clamps at 1). CC0 credit in THIRD_PARTY_NOTICES.md.
- ChimpDuty's own tonemap body (split tone, ACES, then an S-curve) was not
  brought in: photoreal's filmic and grade stages cover it.

### Proof

Sandbox feature `material-proof` (never in `evani.toml` or default): a
walled yard with every surface, under the daylight preset and the sky.
`trunk build --release --features material-proof`. Headless, muted
captures (high, low, phone, and before/after a live
`param_set material.plaster.color`) are in
`C:\dev\evani-dev\.evani-shot-intake\materials\`. The live read-back
in the high run shows the preset applied (`fx.shadows.distance` 45,
`render.exposure` 0.62, the sun 15 degrees up, `sky.follow_light` on), the
plaster colour changing live, no exceptions and no "not in this build"
lines (the sandbox doesn't build evani-render `ibl`, so the sky tints are
left out).

### Cost

**Games that don't use it pay nothing.** Ship builds with
`EVANI_GIT_COMMIT` pinned, SHA-1 of every wasm and js file against the
same build before the intake: match-3, sandbox, base-building-rts,
kart-racer and tactical-fps (which links photoreal) are byte for byte the
same.

**What a game that uses it pays.** Sandbox `trunk build --release` (a dev
build, live tools included; the JS glue is the same 131,062 bytes in all
three), wasm:

| Build | wasm raw | wasm gzip -9 | Added (gzip) |
|---|---|---|---|
| sandbox, default features | 1,292,472 | 552,968 | |
| `material-proof` without `sky` (materials crate, kit-material hook, photoreal and its daylight preset, the proof yard) | 1,372,162 | 582,416 | +29,448 |
| `material-proof` (with `sky`) | 1,389,008 | 589,553 | +7,137 for the sky, +36,585 in all |

The sky (dome, clouds, its params and shader) is about 7 KB gzip; the rest
(about 29 KB) is the materials, the look and the yard together, not the
materials alone.

### Engine gaps found

- **No per-material roughness in the forward mesh pass.** The kit-material
  hook returns a colour only; roughness comes from the look slot (`world`
  0.85, `decal` 0.9). So the materials have no `roughness` param.
- **The catalog has no sky or daylight entries** (catalog/looks.json,
  catalog/effects.json). The preset fits photoreal's entry; a catalog line
  for the sky and for photoreal's `daylight` variant is Chris's call.
- From ChimpDuty's notes, still open: a look can turn fx effects off but
  not on; colour params clamp to 0..1, so `render.clear_color` cannot match
  the dome's HDR horizon (the preset scales it); a real evani-render sky
  pass (full-screen, before the opaques) would let a game drop the dome
  mesh.

### What ChimpDuty changes

1. Drop its own copy of `evani-material-proc` and depend on the engine's,
   optional, behind a game feature, with `features = ["sky"]`.
2. `surface::SPAWN_PAD` is now `surface::BRICK_PAVING` (same id, 12).
3. Its `look` feature and `evani_material_proc::DaylightLook` are gone: use
   `evani-look-photoreal` (no feature needed) and
   `app.add_look(&evani_look_photoreal::daylight::DaylightLook)`, or keep a
   look of its own that has `world`, `decal` and `sky` materials. The grade
   is now photoreal's filmic and grade (saturation 0.88), not ChimpDuty's
   tonemap body; to keep its old curve it sets its own `tonemap` slot.
4. Call `install(ctx, scene)`, or call `register_params` before adding the
   resource, so the `material.*` and `sky.*` params exist (without it the
   world draws at its defaults and nothing is live).
5. Turn on evani-render `ibl` if it wants the dome-matched sky tints and the
   faint reflection; without it the preset leaves them out.
6. If `chimp-map-art` reads `kit_data` itself: HEADER is 8, not 7.
