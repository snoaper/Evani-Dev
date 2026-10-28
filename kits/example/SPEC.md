# Test kit 000: engine sandbox (`example`)

Roadmap row 000 (test kits, not a genre) · physics `simple` · look `basic` · camera: scripted orbit.
Not in `catalog/genres.json`: this kit exists to test the engine and the review process (Stage 1 gate check 5), not to ship a genre. There is no genre playbook and no win or lose state.

Crates: kit `kits/evani-kit-example` (`KitPlugin::name()` = `example`, owner `kit.example`), look `looks/evani-look-basic`, demo `apps/sandbox`. Target pack: [target/](target/).

## 1. Systems

Every number is a registered param (default shown) unless marked "data".

- **Spinners.** The scene `example.main` spawns `kit.example.count` spinners (64 in code; the sandbox bakes 400) with a seeded angle and a rate of 0.5 to 1.5. Each turns at `kit.example.spin_speed` (1.5 rad/s) × the curve `kit.example.spin_falloff(rate)` (identity by default): the fixed-step system `example.spin` sets the rate of a shared `SpinClock`, and each pose is worked out from it at the render time (`spinner_angles`), so a capture at a given sim time always draws the same frame.
- **Boost.** The input action `boost` (Space or gamepad A) scales the speed by the formula `kit.example.boost_scale(held, boost)` = `mix(1, boost, held)`, with `kit.example.boost` (3×).
- **Data.** `example.level` (data, shipped in the wasm): `{"name":"example","ring_radius":4.0}`.
- **Layout (`place`).** The spinners stand in a square grid 1.6 m apart, bevelled cubes (0.9 m) and ringed orbs (0.72 m ball, 1.3 m stone ring) alternating like a chessboard, about one in seven (scattered by a hash, not in lines) in the look's `glow` material so bloom has something to catch. Each rests on its lowest point 0.012 m above the ground while it turns: `place::reach` works out the depth of the turned mesh's lowest vertex exactly, every frame.
- **Scene (`draw`, drawn by apps/sandbox).** The grid on a 2 km ground plane at y = 0; a middle distance of low stone and slate terraces (36 to 54 m out), standing columns with stone caps (64 to 110 m out, one in three crowned with a glowing orb) and low hills (320 to 700 m out) under the look's distance haze; 110 drifting screen-space sprites in palette colours (circle, ring, star, diamond, plus, glow, checker). The drawing lives in the kit so native tools (the geocheck, the review dump) draw exactly what the game does.
- **Shadows.** With the shadow map on (Medium and up) everything casts real shadows; without it (Low) each spinner stands on a dark shadow disc.

## 2. Camera

A slow orbit (`CameraPath::orbit`, radius 42 m, height 11 m, 60 s a turn, a 0.5 m bob, 42° vertical field of view), a pure function of sim time. From there the whole grid stays in frame with a margin all the way round, and a band of sky (14 to 17 % of the frame) shows above the horizon. On a narrower screen than 16:9 the field of view widens (up to 80°) so the grid still fits across a portrait phone. The bench and review cameras fly the same path. The signature moments are fixed points on it (`scene::moments` in apps/sandbox).

## 3. Physics

`simple`, listed in evani.toml so the module's build-time on/off is exercised. The kit itself uses no physics.

## 4. Look and effects

Look `basic`: lit flat colours, pale stone and blue-grey slate for the scenery, a Reinhard tonemap, a light distance haze and a light-blue sky (see [target/look-bible.md](target/look-bible.md)).

- **bloom**: sells the look's one signature, the glowing pieces (threshold 1.5, tuned live and baked).
- **vignette**: frames the grid and darkens the corners.
- **shadows**: grounds the spinners and the scenery (shadow map from Medium up).

Start-up state per tier comes from the budget calculator (`plan_budget`) and `assets/tiers/<tier>.json`; the sandbox does not override it.

## 5. Playtest bot

None. The sandbox has no win or lose state, so the review's playtest item reads "not run". The kit's unit tests cover the spin system, the boost formula, the baked files, the layout (every spinner touches the ground in every pose) and the framing; the `review_dump` test writes each spinner's measured gap for the review card, and `evani-geocheck sandbox` checks the whole scene.

## 6. Reference image

```
In-game screenshot from an original engine test scene, a high three-quarter orbit view.
A 20 by 20 grid of small spinning bevelled cubes and ringed orbs in soft blue, each resting on the ground with a soft shadow, about one in seven glowing warm orange with a soft bloom halo, on a flat green ground that runs to the horizon. Low pale-stone terraces and tall slate columns with stone caps stand around it in the middle distance, some columns crowned with a glowing orb, and low hills fade into a light haze, with a band of light-blue sky along the top edge. Small coloured sprites (circles, rings, stars) drift across the screen. A gentle dark vignette in the corners.
Art style: basic. Lit flat colours, simple shapes, no textures.
Top-quality real-time 3D at the engine's Ultra tier: rich global illumination, soft shadows, clean anti-aliasing, film-quality colour grading.
16:9 widescreen game capture, 2560×1440. Everything is original: characters, vehicles, creatures, logos and UI. No existing game franchises, characters, brands or real people, and no watermark. Any on-screen text is short and legible.
```
