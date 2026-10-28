# Changes

Kit changes, newest first. Values tuned live and baked are logged in `looks/evani-look-pixel16/CHANGES.md` (bake.export writes there).

## 2026-10-16 - extra-life rays read as rays

- The eight extra-life rays are longer (6, 5, 4 art px by step), two pixels wide, with a deep orange body, a dark orange edge and a white tip, so on the pale sky they read as rays and not as a halo; the dotted gold glow behind is unchanged. Draw only: the simulation is untouched (attract-recording state identical before and after).

## 2026-10-16 - polish: upright phones, banner, extra-life light

- Held upright, a phone or tablet frames the view at 288 rows (`view_height_max_px`), centred with black bars above and below: Fennel and the ground sit in a 292 x 288 picture at 4x (390 x 844 phone) instead of a strip under a 633-row sky; the pause button is in the top bar, the movement buttons, Run and Jump in the bottom bar, clear of play. Landscape screens are unchanged. DECISIONS.md 2026-10-16.
- The LEVEL CLEAR! banner sits at 10 % of the view height (never above row 24, under the HUD's 20 rows), in the sky between the HUD and the castle, instead of 28 % over the towers.
- An extra life lands with light on Fennel: eight short gold rays fly out (three frames, 0.27 s) and a dithered gold glow behind Fennel fades by its pattern (three steps, 0.6 s). The same glow, held, lights Fennel in the dark gateway at the level clear. New art `glow` and `burst` (original, in the look's gold and white ramps); drawn from the existing "1UP" pop and the phase, so the simulation, the recordings and their hashes are unchanged.
- Tests: `whole_pixel_scale` (portrait phones and an upright tablet framed at 288 rows, bars centred on each axis), the pause-button test now measures from the page's top and accepts the bar above a framed view, and `upright_the_controls_sit_in_the_bar_below_the_view`.

## 2026-10-16 - touch controls

- On-screen controls for phones and tablets (README "Touch controls"): left and right (the two sides of `move`), Run, a large Jump (held like Space, so jump height follows the thumb) and a pause button under the HUD at the top right. They only set actions the keys already drive; the simulation, the recorded runs and the determinism hashes are unchanged.
- The layer is up only while a run is being played (`touch::controls_on`): off on the title, the pause and settings menus and the results, which are tapped on the page. Shown only on a coarse pointer; a key hides it.
- `Cargo.toml`: `evani-core` with the `touch` feature (the layer is in this game's download only). New module `touch`.
- Tests: every control's action has key bindings; touch right and jump run frame for frame like D and Space (held and tapped jump); Run and left; the pause button pauses and the layer follows the screens (title, pause, resume); free roam has the layer; the pause button starts under the HUD on seven phone and tablet sizes, in play and free roam.

## 2026-10-15 - free roam (playground)

- `KitPlugin::free_roam` is true. Under `PlayMode::FreeRoam` (`?mode=free` on the web): starts straight in the level; no lives, timer, game over or goal (the gate is just a place); a pit, water, spikes or the map's edge respawn the hero free on the last safe ground or checkpoint (blinking, with a sparkle); enemies cannot hurt, stomps still work; the HUD shows a small FREE ROAM label instead of lives and timer. Play mode is unchanged.
- Play mode reports every life lost, expired timer and game over (also the gate) on `PlayOutcomes` (`Game::take_fails`, handed over by `platformer.sim`).
- Mini font gains F, R, E, O, A, M for the label.
- Tests: `free_roam_holds` (two minutes of scripted input through `free_roam_test`), `play_mode_reports_outcomes`, the `game` free-roam tests (pit, enemy, timer and gate, checkpoint, title) and `free_roam_hud_shows_a_label_instead_of_lives_and_timer`.

## 2026-10-15 - geocheck clean (0 untagged)

- Coins: a sprite centred on a point puts an odd height's spare half pixel above (`draw::centred_top`), so a 13 px coin at its tile's centre keeps 2 px of clear air over the ground below (it was drawn half a pixel low, rounding to 1 px of air). Test: `a_centred_coin_keeps_two_pixels_of_air`.
- Drop shadows: one baked ellipse frame per width (4 to 24 px, 3 or 4 px tall) drawn 1:1, instead of one 16 x 6 sprite scaled to size (non-square pixels).
- Landing squash: each hero pose has a baked squash frame (2 px wider, 2 px shorter, outlined) drawn 1:1, instead of scaling the pose when drawn. Test: `shadows_and_the_landing_squash_are_drawn_one_to_one`.
- Geocheck scene dump `examples/geocheck_dump.rs` (dev only): `cargo xtask geocheck 2d-platformer` runs it and the 2D checker. Earth beside the stone pillar is reported as the transition tile it is drawn as (the bonded blocks and seam).

## 2026-10-15 - fix 002: slopes and big-screen scaling

- Slopes: the earth tile under a slope carries the slope's line, measures its dirt depth from it and draws the rest of the grass lip, so every slope is one clean diagonal with dirt below it (no square dirt notches); slope dirt uses the earth's shading. Test: `slopes_draw_along_the_collision_floor_without_notches`.
- Scale: the smallest whole scale that shows at most `view_height_max_px` (288, new) rows, or `floor(h / view_height_px)` when that shows fewer than 224; 1080p 5x -> 4x, 1440p 6x -> 5x, a 3440 x 1440 screen at 125 % 6x -> 5x. Each axis maps by its own CSS-to-device ratio.
- Wide screens: views wider than `view_width_max_px` (640, new) are centred with black bars; `wake_distance` 320 -> 336 so enemies wake before they come into view.
- The kit turns on `evani-render`'s new `pixel-art` feature (canvas backing from the exact device-pixel box).
- Tests: the scale table for every review size, per-axis mapping at dpr 1.25, the grid test at dpr 1.25, the HUD test on an ultrawide.

## 2026-10-14 - fixes after Chris's review

- Camera: anchored to the ground the hero last stood on, ground line at 72 % of the view (lower third), vertical dead zone (72 px), follows down at once, eased look-ahead in the direction of movement; sky above the level allowed. New params `cam_ground_frac`, `cam_smooth_y`; `cam_deadzone_y` 32 -> 72, `cam_smooth` 6 -> 4.
- Terrain: depth darkening (brown, then a new `soil` ramp, also in pixel16), 16 hashed details, roots, stone bonded beside the pillar; the stone column is a pillar with cap, shaft and plinth.
- Score pops: merge within 0.5 s, live 0.8 s with an eased rise and fade, laid out in screen space clear of the hero and of each other.
- Coins: trees moved off the hilltop and plateau coins; the pit, pool and step arcs follow simulated jumps.
- Pixel grid: HUD at the art scale; the renderer keeps pixel art at full device pixels on every tier.
- Capture moments (`moments.rs`) and `[review] moments` in the app; target pack in `kits/2d-platformer/target/`.
- Kit tests for framing, floating text, coins (reachability, one-jump arcs, clear air), pixel grid, HUD on phones and the moments.

## 2026-10-14 - first version, ready for review

- Kinematic hero controller (coyote time, jump buffering, variable jump height, stomp bounce), slopes and one-way platforms, moving platforms, beetles and hoppers, coins, checkpoints, timer, lives, castle goal, tile-map level loader, side camera, HUD, lookahead bot for the attract demo, synthesised sounds (kit 002 steps 3 and 6).
- Demo game Fennel's Leap in `apps/2d-platformer`: title, play, pause, settings (tier, effects, key rebinding, volume) and results.
- New look pixel16 (crisp whole-pixel scaling, parallax, shake).
- Live tuning pass (step 7): `shadow_alpha` 0.45 -> 0.62, `stomp_trauma` 0.25 -> 0.35, `fx.shake.amount` 0.35 -> 0.6, baked into `params.json`. Shadows now sit on the ground; the stomp key frame is taken at the bounce's height.
- Known cosmetic risk: the 10-tile pit near the start shows the near-bush layer's dark green fill, so it reads like dark ground rather than a gap.
