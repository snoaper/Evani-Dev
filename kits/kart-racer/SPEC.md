# Kit 001: Kart racer (`kart-racer`)

Roadmap row 001 · physics `simple` · first look `toon` · camera `third-person`.
Catalog entry: `catalog/genres.json` → `kart-racer`. Reference games describe mechanics only; every name, character, track and logo here is original.

Crates: kit `kits/evani-kit-kart-racer` (`KitPlugin::name()` = `kart-racer`, owner `kit.kart-racer`), look `looks/evani-look-toon`, demo `apps/kart-racer`.
Param names use `kit.kart_racer.*` (param segments allow only `a-z`, `0-9` and `_`; the owner stamp keeps the hyphenated kit id, and bake finds the crate from `BuildInfo.kit`).

## 1. Gameplay systems

Every number below is a registered param (default shown) unless marked "data". Data lives in JSON shipped with `include_str!` and registered with `data_file`.

### 1.1 Arcade kart physics with drift-charge boosts

Kinematic arcade model in the kit (fixed 60 Hz tick, no rigid bodies; `physics = "simple"` in evani.toml puts the physics module in the build, and the kit reads `physics.gravity` when it exists, else its own `kit.kart_racer.gravity`).

- State per kart: position, heading (yaw), speed along heading, vertical offset and velocity (hops, bumps), drift state, drift charge, boost timer, spin-out timer, shield timer.
- Controls per kart (human from input actions, AI from its driver): steer −1..1, throttle 0..1, brake 0..1, drift held, item pressed.
- Speed: accelerates towards `max_speed` (26 m/s) at `accel` (14 m/s²), brakes at `brake` (30 m/s²) and reverses to `reverse_speed` (8 m/s); `drag` (0.6 /s) when coasting. Off the road surface the cap is multiplied by `offroad_factor` (0.55).
- Steering: yaw rate = steer × `turn_rate` (2.2 rad/s) × curve `kit.kart_racer.steer_by_speed(speed_frac)` (low at a standstill, full at mid speed, slightly less at top speed).
- Drift: pressing drift while steering above `drift_min_speed` (10 m/s) starts a drift in the steer direction with a small hop (`hop_height` 0.35 m). While drifting the yaw rate is `drift_turn` (1.6 rad/s) widened or tightened by steer (`drift_steer` 0.8), the body is drawn at a slip angle (`drift_slip_deg` 22°), and charge grows at `drift_charge_rate` (1.0 /s) × (0.6 + 0.4 × steer tightness).
- Charge tiers: `drift_tier1` (0.8 s) blue sparks, `drift_tier2` (1.6 s) orange, `drift_tier3` (2.6 s) pink. Releasing drift fires a boost of `boost_tier1_s` (0.6), `boost_tier2_s` (1.0) or `boost_tier3_s` (1.5) seconds; below tier 1 nothing.
- Boost: cap × `boost_speed_mult` (1.35) and accel `boost_accel` (40 m/s²).
- Bumps: karts are circles of `kart_radius` (1.1 m); overlapping karts are pushed apart and exchange speed with `bump_restitution` (0.5). The track edge walls (road half width + `verge_width` 7 m) stop the kart's edge (its centre stays `kart_radius` inside, so it never sinks into the rail) and cut speed by `wall_speed_loss` (0.4) scaled by how squarely the kart hits (head-on the full 0.4, grinding along the wall almost nothing). Fix round 1: each kart is really a capsule, the `kart_radius` circle swept 0.45 m each way along its heading (karts are 3.1 m long), resolved in two passes so packs never sink into each other; the speed exchange and bump event happen on the first pass only. A spin-out also gives a small hop (upward speed at least 4.5 m/s).
- Spin-out (hit by an item): `spinout_s` (1.1 s) of no control, speed × 0.3, the body spins twice.

### 1.2 Item boxes with position-weighted odds

- Item box rows (data, `track.json`): 3 rows of 4 boxes across the road. A box is collected on contact, hides for `item_respawn_s` (3 s in code, tuned live to 1.5 s and baked into the kit's `params.json`, so a player behind the AI pack still finds boxes) and gives the kart an item if its slot is empty, after a roulette of `roulette_s` (1.2 s).
- Odds (data, `items.json`): a weight table per position band (`front`, `mid`, `back`). The kart's band value is `position / (racers − 1)` (0 = leading, 1 = last); the weights are linearly blended between the bands, then one item is drawn with the seeded RNG.
- Items (all original):
  - **Zoom Pop**: an instant boost of `zoom_boost_s` (1.2 s). Weights front 20 / mid 30 / back 35.
  - **Goo Blob**: dropped behind; the first kart to touch it spins out; lasts `goo_lifetime_s` (20 s). Weights 45 / 15 / 0.
  - **Boing Bomb**: thrown ahead at `bomb_speed` (34 m/s) on an arc, bounces twice, explodes on touching a kart or after `bomb_fuse_s` (2.5 s); everyone within `bomb_radius` (4 m) spins out; nearby players get screen shake. Weights 5 / 30 / 20.
  - **Seeker Star**: flies along the track to the racer one place ahead at `seeker_speed` (40 m/s) and spins it out. Weights 0 / 10 / 35.
  - **Shield Bubble**: blocks the next hit for `shield_s` (8 s). Weights 30 / 15 / 10.

### 1.3 AI racers on racing-line splines with rubber-banding

- The track centreline is a closed Catmull-Rom spline through control points (data, `track.json`), sampled every ~2 m with tangent, side normal and distance. The racing line is the centreline shifted towards the inside of each corner by curvature (`racing_line_inset` 0.55 of the half width), then smoothed.
- Each AI (data, `karts.json`: name, colour, `skill` 0..1, lane bias, item aggression) steers to a point `ai_lookahead` (11 m) ahead on its line plus its lane bias, with gain `ai_steer_gain` (2.4); it eases off when the angle error is large, drifts through corners whose curvature over the next 25 m passes `ai_drift_curvature` (0.035 /m), and releases on the exit.
- Item use: Zoom Pop on straights, Goo Blob when a kart is within 12 m behind, Boing Bomb / Seeker Star when a target is ahead within 40 m, Shield Bubble soon after pick-up; delays scale with aggression.
- Rubber-banding: AI cap × (1 + formula `kit.kart_racer.rubber_band(gap, dist, strength)`), default `clamp(gap / dist, -1, 1) * strength`, where `gap` is how far (m, along the track, laps included) the AI is behind the best human, `dist` = `rubber_band_distance` (60 m), `strength` = `rubber_band_strength` (0.12). Skill spreads base caps by ±`ai_skill_spread` (6 %).

### 1.4 Laps, checkpoints and wrong-way detection

- `checkpoints` (8) gates evenly spaced along the track; gate 0 is the start/finish line. A lap counts only when the kart crosses the line having passed every gate in order. `laps` (3, i32) per race.
- Race position: sorted by (laps done, next gate, distance along the track); ties broken by kart index (deterministic).
- Wrong way: heading · track tangent < −0.35 while moving above 4 m/s for `wrong_way_s` (1.0 s) shows WRONG WAY until the kart turns round.
- Race flow (`RaceState`): Title (attract race, AI only) → Countdown (3, 2, 1, GO; `countdown_s` 3) → Racing → Finished (results with times). Pause freezes the sim. A human finishing 1st to 3rd wins (podium); 4th or lower loses. In two-player mode each player gets their own result.
- `karts` (i32, 8, range 1–12) sets the field size; AI racers = karts − players (7 in one-player, 6 in two-player).

### 1.5 Split-screen

- One or two local players (title menu). Two players: the canvas is split top and bottom, each half with its own chase camera; HUD and minimap per half.
- Bindings (rebindable in settings, saved): P1 steer `keys:KeyA/KeyD`, throttle `key:KeyW`, brake `key:KeyS`, drift `key:ShiftLeft`, item `key:KeyE`, plus gamepad 1. P2 steer `keys:ArrowLeft/ArrowRight`, throttle `key:ArrowUp`, brake `key:ArrowDown`, drift `key:ShiftRight`, item `key:Enter`, plus gamepad 2. One-player mode also accepts the P2 keys and any gamepad. Pause `key:Escape` / `key:KeyP` / pad Start.
- Needs two small core additions (see §6): extra render views, and gamepad bindings for a specific pad.

### 1.6 ECS layout

Every gameplay object is an entity in evani-core's ECS (the overlay's `entities` counts them); scenery is the draw system's statics.

- Components (registered by name in `KartRacerPlugin::build`): `kart_racer.kart` (`Kart`), `kart_racer.ai` (`AiState`, on every kart: AIs always, humans on the autopilot and after they finish), `kart_racer.controls` (`Controls`, this tick's input, human or AI), `kart_racer.item_box` (`ItemBox`), `kart_racer.goo`, `kart_racer.bomb`, `kart_racer.seeker`, `kart_racer.blast` (items in flight) and `kart_racer.in_flight` (`InFlight`: on or idle, and spawn order).
- Entities: one per kart, respawned on every new race (`race::reset_mut`); one per item box, spawned once and reset in place; a fixed pool per item kind (Goo 16, Bomb 8, Seeker 8, Blast 8), spawned once, where a full pool reuses its oldest item. A one-player race has 8 karts + 12 boxes + 40 pooled items = 60 entities.
- Resource `Race`: race-wide state (phase, countdown, clock, order, finish order, rng, events, requests, counts, trauma, autopilot, cam_snap, track, odds, roster) and the entity lists `grid` (karts in grid order; the index is the kart number in events, order and results), `boxes` and `pools`. Systems walk these lists, never a storage's dense order, so the sim is deterministic.
- Systems (fixed `Update`, in this order): `kart_racer.flow` (0: first-tick grid, pause button, requests, countdown and phase), `kart_racer.control` (10: human input or AI drive into `Controls`, rubber band), `kart_racer.karts` (20: item use, `kart::step`, wrong way, gates), `kart_racer.boxes` (30: pick-ups, roulette, respawn timers), `kart_racer.bumps` (40: kart-against-kart pairs in grid order), `kart_racer.items` (50: goo, bombs, seekers, blasts, oldest first; shields wear off), `kart_racer.standings` (60: places and the end of the race). `kart_racer.draw` (`Frame`) reads a `RaceView` (the race plus copies of its objects in these orders).
- Capture moments get only `&World`, so they stage a race on the existing entities (`race::reset`, `race::tick_shared`); a moment that needs a different field size fails with a clear error.

### 1.7 Second review additions (2026-10-14)

- Props are ECS entities: one `kart_racer.prop` entity per `scenery::Prop` (hills, clouds, far ground, lollipop trees, floss bushes, low-poly trees, tufts, sweet signs, candy canes, grandstands with a crowd), kept in layout order in the `scenery::Scenery` resource. The plugin spawns the full layout; the draw system swaps in the thin layout on small-budget tiers or when the verge changes. Statics are built from the entities (`draw::build_statics_with`).
- Start-gate lights: three `LOW_SPHERE` lights under the banner; one more turns red on each of 3, 2, 1, then all three show green for 1.5 s after GO.
- Gentle grass slopes run from each shoulder edge down to the meadow (no vertical edge, so no ink line across the grass).
- Start boost: throttle held in the last `start_boost_window_s` (0.5 s) before GO gives a `start_boost_s` (0.8 s) boost and the HUD cue "GOOD START!". Both are registered tunables. Local players only; the AI stays deterministic.
- HUD per hud-layout: item slot top-centre, timer alone top-right, countdown steps and GO pop from 130 % to 100 %, "GOOD START!" at 55 % from the top, small print at least 12 px on a phone and 16 px at 1080p. The minimap is top-right under the timer (150 px at 1080p, 90 px on a phone) and the drift tier bottom-centre, 18 % from the bottom (14 % on a phone), as hud-layout says (fix round 1).
- The stats overlay is hidden while a capture is pending or a held capture is active; the player's setting returns afterwards.
- Capture moments: `countdown`, `countdown_go`, `boost`, `overtake`, `final_lap`, `podium` (plus the earlier three), all listed in `evani.toml [review] moments`. The finish and podium moments are cut from one staged race (stream 808, the player's boost topped up on the road), so the player has the same place in both. The finish is 3.4 s after the line: the FINISH! banner, the place in large type, wrapped-sweet confetti, the gate lit on both faces and the camera easing into a slow orbit round the kart. The podium is the 3D scene (three blocks, a candy-cane arch, the top three drivers in their kart colours, a fixed three-quarter low camera) with the results panel beside it, never over it (`finale.rs`).
- New built-in meshes in evani-render: `LOW_SPHERE` (120 triangles), `CONE` (24), `DOME` (96).
- Fix round 1 capture changes: the `drift` moment is staged at tier 1 (blue sparks, "Drift: blue"); the charge is topped up to the start of tier 1 mid-corner. The `item_hit` moment is staged 0.25 s after the bang so the blast and the spinning-out rival show.

## 2. Camera

`third-person` (the only catalog camera the toon look also supports). Chase camera per local player: `cam_distance` (7 m) behind, `cam_height` (3 m), looking `cam_look_ahead` (4 m) ahead; the camera smooths its heading (and the anchor's height) with `cam_smooth` (8 /s), never the kart's position, so the kart stays fixed low in the centre of the frame while the view swings round bends a little late and a drift's slide angle shows; field of view `cam_fov` (62°), kicked out by `fx.speedlines.fov_kick_deg` on a boost. A new race or a capture moment (`Race.cam_snap`) snaps the camera and kick into place. A spinning gem floats over each local player's kart. Title/attract mode and the bench path use a slow orbit over the track (`CameraPath::orbit`). Occlusion (fix round 1): rival karts are capsule-shaped blockers; when one sits between the camera and the player the camera rises smoothly (fast attack, slow release, up to 2.6 m) and a backstop keeps the eye out of every kart volume. No snapping, nothing allocated per frame. Boost widens the field of view by the tunable `boost_fov_kick` (12 degrees).

## 3. Physics

`simple`, exactly as the catalog says: evani.toml `physics = "simple"`. The kart model is the kit's own arcade kinematics (§1.1); the physics module is still a stub and supplies `physics.gravity`.

## 4. Look

`toon` (row says `toon`; new look crate `looks/evani-look-toon`, catalog `cost_points` 2, style 3D). Render rules from the catalog: 2–3 band ramp diffuse (`toon_ramp` slot, 3 hard bands, edges at N.L 0.02 and 0.45 softened over only ±0.025, written as constants in the slot body, tuned through the slot itself; the sun comes from the side and a little ahead so the bands split every model into a lit, a mid and a shadow side), ink outlines (the `outline` effect, which the look asks for), saturated candy palette (16 material roles: road, kerb red/white, grass, mountain pink/mint/lilac, tree, trunk, kart body, wheel, item box (emissive), boost flame (emissive), spark (emissive), goo, bomb; there is no sand on the track, and the driver is the kart body material with a lighter tint), its own `tonemap` body (gentle filmic, keeps saturation), `fog` slot (light distance haze in the sky colour), and post params: `render.saturation` 1.15, sky `render.clear_color`, warm `render.light.*`, `render.ambient`.

The kit asks for materials by role name; when a look lacks a role (e.g. `basic`) it falls back to material 0 and tints the instance so the colour still reads, so the kit runs with any 3D look.

## 5. Effects

Start from none; each kept only if a capture is clearly better (§7 of the kit process).

| Effect | Cost | Why |
|---|---|---|
| `outline` (new) | 2 | Sells the look: the toon look's signature ink lines (depth-edge silhouettes). |
| `speedlines` (new) | 1 | Reads speed and boosts: radial streaks only on a boost or past the normal top speed (karts cruise at the cap, so lines there would never go away). Comfort-off in VR. Split-aware (one or two stacked views). |
| `shake` (new) | 0 | Reads impacts: trauma from bomb blasts, spin-outs and wall hits drives a camera offset and roll (applied by the kit while the effect is on). Comfort-off in VR. No render pass. |
| `bloom` (existing) | 3 | Reads the drift-charge tier and boosts: spark and flame colours glow, and the white "?" on the item boxes glows softly. |
| `shadows` (new, polish pass) | 2 | Grounds everything: one directional shadow map from Medium up (tier `shadow_map` 1024 / 2048 / 4096, PCF on High and Ultra; `kit.kart_racer.shadow_soften`, default 1, raises Medium to the same 3x3 filter so its kart shadows have soft edges instead of stair steps, VR keeps its single tap). Without it (Low, or switched off) the kit's blob shadows ground the karts (two discs each), bombs, seekers and item boxes, so nothing floats on any tier. |

Item-hit blast (fix round 1): flat candy shapes (ring, spikes, dome) in lit toon materials so the ink outline draws them and bloom cannot wash them out, plus a shockwave ring and pooled debris puffs; it still reads as shapes on Low. Boost adds a pooled orange spark trail behind the kart.

Not used: vignette (no reading or look value on a bright toon track), depth of field / motion blur / chroma (clarity in a competitive racer), ambient occlusion (not built; blob and map shadows already ground the scene).

Start-up per tier (`plan_budget`: look 2 points plus effects; `effects_on` in `assets/tiers/*.json` gains `outline`, `speedlines`, `shake`, and `shadows` from Medium up):

| Tier | Points | On at start | Used |
|---|---|---|---|
| Low | 7 | outline, speedlines, shake (bloom's 3 points don't fit; blob shadows) | 5 |
| Medium | 11 | outline, speedlines, shake, bloom, shadows | 10 |
| High | 16 | outline, speedlines, shake, bloom, shadows | 10 |
| Ultra | 24 | outline, speedlines, shake, bloom, shadows | 10 |
| VR | 9 | outline, bloom, shadows (speedlines and shake off by VR comfort) | 9 |

Measured in the live session (`bench.run`, 10 s on the orbit path): Low, Medium, High and Ultra pass at 60 fps with these budgets. Below 120 000 triangles per frame (Low's limit is 100 000) the kit draws one tree in three, leaves out the round props, and only local players' karts puff dust: after the polish pass (bevelled karts and drivers, dressed track) the scenery is under 70 000 triangles on Low, leaving room for the pack. VR only misses its 72 Hz frame target because the test browser runs at 60 Hz; WebXR with WebGPU isn't available in browsers yet, so VR is marked unavailable.

Tuned live and baked (`looks/evani-look-toon/CHANGES.md`): `fx.outline.thickness` 1.6 and `strength` 0.9 (look `params.json`), and a lighter `fog` slot (haze from 60 m, density 0.0025, capped at 0.45; `slots/fog.wgsl`).

## 6. Core additions this kit needs (small, additive)

1. `evani-render`: `RenderWorld.views` (extra camera views with a viewport rectangle); the forward pass draws the instances once per view. Empty = today's single camera.
2. `evani-render`: effects may sample the depth target (`EffectShader.depth`), for `outline`.
3. `evani-core` input: `padN:button:B` / `padN:axis:A[:+|-]` bindings for one specific gamepad (today `pad:` means any pad).

Recorded in DECISIONS.md when built.

## 7. Demo (`apps/kart-racer`)

Title screen (1 player / 2 players / settings) over the attract race; one track ("Sugarpeak Loop"), 3 laps, 8 karts; countdown; HUD per player (position, lap, item slot with roulette, race time, WRONG WAY, drift tier) as light DOM plus a sprite minimap; pause menu (resume, restart, settings, quit to title); settings (tier, effect toggles for what is compiled in, key rebinding, volume); results (podium win or lose). Sounds are synthesised at start (no downloads).

## 8. Reference image prompt

```
In-game screenshot from an original Kart racer video game, third-person chase camera just behind and above the player's kart.
A go-kart drifts around a candy-coloured mountain track throwing blue drift sparks, rivals dodging bouncing item bombs, floating item boxes ahead. HUD: race position, lap counter, item slot, minimap.
Art style: Toon cel-shaded. Hard-edged lighting bands and ink outlines, like an animated film.
Top-quality real-time 3D at the engine's Ultra tier: rich global illumination, soft shadows, clean anti-aliasing, film-quality colour grading.
16:9 widescreen game capture, 2560×1440. Everything is original: characters, vehicles, creatures, logos and UI. No existing game franchises, characters, brands or real people, and no watermark. Any on-screen text is short and legible.
```
