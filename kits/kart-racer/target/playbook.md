# Playbook items: Sugarpeak Rally

The items of `docs/playbooks/kart-racer.md`, each with how this kit meets it, or why it is only partly met or not met yet. [x] met, [~] partly met, [ ] not met yet. The critic scores these under R8.

## Start

- [x] **Countdown lights 3-2-1-GO.** The race opens with 3, 2, 1 one second apart, then GO; karts are held on the grid until GO (`race.rs` countdown phase, the big centre text in the HUD, moment `countdown`).
- [~] **Start boost.** Pressing the throttle inside the last `start_boost_window_s` (0.5 s) before GO and holding it through GO launches with a start boost and "GOOD START!" (`kart.rs`). Revving too early gives no boost, but no stall or spin either: a missed start is a normal start, which keeps it friendly for young players.

## Reading the race

- [x] **Distinct rival colours.** Eight karts in eight hues (look bible, rival table; `data/karts.json`); the review dump checks every pair is at least 20 apart.
- [x] **None near the player's colour.** Pip is the only pink kart; the nearest rival hues are 54 and 60 degrees away; the dump checks a distance of at least 40.
- [x] **The player is marked.** A pink arrow with a white cap over the player's kart in split-screen (one per player) and in the finish orbit, none in the one-player chase view (you know which kart is yours), plus the player's larger dot with a white ring on the minimap (`draw.rs`).
- [x] **HUD always visible.** Position, lap, item slot and speed are on screen from the countdown to the finish, in each split-screen half and on a phone (`hud-layout.md`; review check "captures hide the overlay and HUD" only hides it for the clean shots).

## World

- [x] **No visible world edge.** Gumdrop hills in two closed rows round the track, ground running past them into haze, a sky dome; the title orbit, chase, podium and capture cameras all stay inside (review check "world edge").
- [x] **Middle distance filled.** Lollipop trees, gumdrop hills, candy-floss bushes, jelly-bean rocks, stands and signs between the road and the backdrop (look bible, props).
- [x] **Track edges read at speed.** Red and white kerbs on every corner, candy-cane barriers outside corners, wrapped-sweet chevron signs before corner entries, and a lighter grass lip by the road.
- [x] **Grounded karts with contact shadows.** Karts and props sit on the ground (review dump: gap at most 0.05 m) and cast a contact shadow on every tier: the shadow map on Medium and up, blob shadows or dark ground discs on Low. Item boxes float on purpose (a pickup reads from far away when it hovers and spins); they are tagged as intentional in the dump and still have a ground disc on Low and the shadow map above.

## Driving feel

- [x] **Drift and boost.** Holding drift through a corner builds three spark tiers (blue, orange, pink) that release as a boost of rising length (moments `drift` and `boost`).
- [x] **Items with readable threat.** Each of the five items has its own icon in the item slot and its own look on the track. A seeker aimed at the player within 90 m, or a bomb that will reach its blast ring within 2.5 s (or is already in it), shows a red blinking "SEEKER INCOMING!" or "BOMB INCOMING!" under the item slot, blinking faster as it nears (`threat.rs`, `hud-layout` slot area; captured as the `threat` moment in the review list: shot-threat-low.png, shot-threat-ultra.png, page-threat-phone.png).
- [x] **Catch-up without cheating.** Item odds are weighted by position (`data/items.json`): the back of the pack gets the stronger items. AI speed caps change by at most 12% with the gap (`rubber_band`), never by teleporting.

## Performance

- [x] **Low never costs more than higher tiers.** Bench medians: Low is the cheapest tier (review check "tier cost order").

## Sound

- [x] **Engine pitch follows speed.** One looping engine per local player: playback rate 0.75 at idle up to 3.0 at top speed (more with boost), eased; in 2P both engines play at 0.7 times gain and player 2 is pitched 1.12 times higher; the nearest rival within 30 m adds one quiet engine (`voices.rs`). Probed live: rate 0.75 idle, 1.68 at 35 km/h, 2.48 at 71 km/h, 3.0 at 94 km/h.
- [x] **Distinct countdown beeps.** 3, 2, 1 share one beep (660 Hz, 0.16 s); GO is an octave higher and longer (1320 Hz, 0.42 s) (`sfx.rs`).
- [~] **Drift and boost cues.** A chirp each time the drift charge reaches a new tier, a hop sound and a rising burst when the boost fires; no continuous tyre squeal while drifting.
- [x] **Surface sounds.** Three seamless synthesised loops, road (soft hum), kerb (low rumble) and grass (bright hiss), with gain rising with speed and silent when stopped or airborne; the surface comes from the kart lateral position against the road and kerb width (`voices.rs`, `sfx.rs`). Probed live: road gain 0.16 on the road, grass 0.40 with road 0 on the grass.
- [x] **Warning sound for threats.** A rising double blip (far, near, now) that gets faster and higher as the item closes in, played with the HUD warning (`threat.rs`, `sfx.rs`).
- [~] **Mix puts the player first.** Effects and engines play on the effects bus; engines are 0.7 times gain in 2P and rivals add only one quiet engine within 30 m; only the local players' own events (and blasts within 60 m) make sound. There is no music, so no ducking or final-lap tempo.
- [~] **Small and controllable.** All sounds are synthesised at start (no audio download); the four loops add about 6 KB gzipped to the wasm. A master volume slider in settings is saved and mutes the engines too; voices stop on pause, at the results screen and on a hidden tab (the context suspends). There is no separate music and effects volume because there is no music.
