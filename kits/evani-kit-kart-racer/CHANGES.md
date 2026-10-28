# Changes

Kit changes, newest first. Values tuned live and baked are logged in `looks/evani-look-toon/CHANGES.md` (bake.export writes there).

## 2026-10-21 - Soft shadow edges on Medium

- New param `kit.kart_racer.shadow_soften` (0 to 2, default 1): the least shadow filter radius on tiers with a shadow map. Medium's single hardware tap on its 1024 map drew stair-stepped kart shadows; it now uses the 3x3 filter High and Ultra already had. 0 gives each tier its own radius back; the VR tier keeps its own. Looks only.
- Known: the dust puffs are lit meshes, so they cast small shadows of their own (no engine way yet to stop one mesh casting); the 3x3 filter softens them.

## 2026-10-20 - S4-1b: rival karts from a shared part pack (code half, still at review)

- New `src/rivals.rs` and `data/rivals.json` (needs the `models` module, off in builds without it): each rival is a recipe of named slots (chassis, wheels, seat, spoiler, nose, decals) on the engine's part variant (`Variant`, `draw_with`, `anchor_frame`), colours named from the roster (`kart`, `kart_light`, `kart_deep`, `suit`, `suit_deep`, `white`, `steel`) so one recipe serves any kart colour. All recipes together may use at most 12 different models (`MAX_PACK_MODELS`): rivals recombine and recolour one pack, never one model each.
- Placeholder parts: the existing proof kart, crate and barrel (copied to `apps/kart-racer/assets/models/`). Until a rival's parts have streamed in, when the camera is fading it, for the players, or with `kit.kart_racer.rival_parts` off, the primitive kart is drawn. The driver, steering wheel and column stay primitive.
- New params `kit.kart_racer.rival_parts` (bool, on) and `kit.kart_racer.rival_model_scale` (0.5 to 2). Draw only; no gameplay value reads a model.
- `draw_kart` is now `draw_kart_parts` with groups (`KartParts`) and a shared `kart_pose`; the primitive look is unchanged. Not yet run in a browser or tested (combined pass deferred by Chris).

## 2026-10-15 - Stage 2 gate pass: bench and tuning re-run (still at review)

- No gameplay or look change. Bench re-run on the merged engine: Low, Medium, High, Ultra and VR (mono preset) pass; geocheck 0 untagged; review card 0 FAIL; shipped size 389.7 KB gzipped.
- Live tuning re-run: the five handling values (rubber band 0.12, boost tier 1 0.6 s, turn rate 2.2, boost multiplier 1.35, acceleration 14) were compared in bot-race tables (new ignored test `playtest::tune_table`); each won or tied, so none changed and nothing was baked. `.vibedeck/review/kart-racer/gate2-tuning.md` has the tables.

## 2026-10-15 - free roam (playground, still at review)

- `KitPlugin::free_roam` is true. Under `PlayMode::FreeRoam` (`?mode=free` on the web) the player drives Sugarpeak Loop alone and already racing: no title, no countdown or GO, no rivals (the grid is the player's kart only), no laps, checkpoint gates, race timer, places or finish, and no wrong-way warning; nothing ends the run. Item boxes stay and every item works. Restart, "back to the title" and a new start all mean a fresh start on the track; the pause menu says "Back to the start" and drops "Quit to title". Karts cannot leave the walled road (a wall pushes back, as in Play), so there is no off-track penalty or reset to add.
- HUD: a small FREE ROAM label stands in for the place, the lap and the race time, with a controls line under it for the first 12 s (no title screen shows the keys). Play mode is unchanged.
- `PlayOutcomes`: a race that ends (finish, or the 15-minute limit) reports `game_over`, and the limit also `timer_expired`, in Play mode, so `free_roam_test` catches a regression.
- Tests: `free_roam_holds` (two minutes of scripted accelerate, steer, drift, item, wall and reverse input through `free_roam_test`), `free_roam_starts_on_the_track_alone`, `free_roam_has_no_laps_finish_or_wrong_way`, `play_mode_reports_outcomes` and the app's `free_roam_hud_has_a_label_and_no_race_numbers`.
- Size: ship build 387.7 KB to 389.1 KB gzipped (+1.4 KB, 0.36%).

## 2026-10-14 - item boxes become "?" blocks (Chris's live review, still at review)

- Item boxes are solid candy blocks with bevelled edges, a bold rounded white "?" (this kit's own glyph) and four round rivets on every face, instead of rainbow boxes wrapped in glowing ribbons. New `itembox` module builds the body and the decor as two custom meshes (44 + 516 triangles), rebuilt in place when a shape tunable changes; each box is two instances.
- Colours: a six-colour candy palette (apple green, grape, blueberry, cherry, lemon, mint teal); each box holds one colour and fades to the next every 4 s, and neighbours never match.
- Bloom: the "?" and rivets are unlit white that the bloom pass keeps half as much of as the old ribbons (luminance 0.62 against 1.24 at the toon threshold 2.0).
- Tunables: removed `item_box_hue_speed`, `_hue_spread`, `_glow`, `_ribbon_width`, `_ribbon_overhang` and `_ribbon_color`; added `item_box_color_cycle_s`, `_bevel`, `_glyph_size`, `_glyph_glow` (0.5), `_rivet_size`, `_glyph_color` and `_color_1` to `_6`. `item_box_tilt` default 1.0 to 0.4 so the "?" stays upright.
- Size: ship build wasm +6.0 KB raw, +3.1 KB gzipped (367,477 to 370,635 bytes total gzip).

## 2026-10-14 - fix round 1, job 4: engine, surface and warning sounds, incoming-item warning (still at review)

- Engine: one looping voice per local player whose pitch follows speed (0.75 idle to 3.0 top, boost adds headroom); 2P mixes both at 0.7 times gain with player 2 pitched 1.12 times up; the nearest rival within 30 m adds one quiet engine.
- Surfaces: seamless road, kerb and grass loops chosen from the kart's lateral position; silent when stopped or airborne.
- Threat: a seeker aimed at the player within 90 m or a bomb reaching its blast ring within 2.5 s shows a red blinking warning under the item slot and plays a rising blip cue that speeds up as it nears. New `threat` capture moment (not in the review list).
- Core: looping voices (`voice_start`, `voice_set`, `voice_stop`) in `AudioMixer` and `AudioBackend`, recorded in `CORE_REQUESTS.md` as done. Voices wait for the first gesture, stop on pause, results and hidden tab.
- Size: release wasm +15.4 KB raw, +6.4 KB gzipped (743,099 to 749,479 bytes); no audio files.

## 2026-10-14 - fix round 1, job 2: the finish and the podium (still at review)

- Finish: over the line the HUD shows FINISH! (styled like FINAL LAP!) with the final place in large type, wrapped-sweet confetti (a fixed pool, 90 sweets on Low up to 360 on Ultra, 60 triangles each, nothing allocated per frame), the gate lights lit on both faces flashing gold and green, and a camera that eases from the chase view into a slow orbit round the kart. A finished human kart brakes to a stop past the gate.
- Podium: after 4.5 s the podium scene takes over (three blocks, first taller, a candy-cane arch, the top three drivers in their kart colours, a fixed three-quarter low camera) with the results panel at the side (right 42 %, or the lower half on a tall screen), never over the podium. The final places are latched so the podium and the panel agree. No trees stand on the podium meadow.
- New `finale` module; only engine primitives and the look's materials, so nothing new reaches a download.
- The `finish` and `podium` review moments are one staged race, so the player has the same place in both.

## 2026-10-14 — visual polish pass after Chris's review (still at review)

- Grounding: a shadow map from Medium up (new `shadows` effect) and blob shadows on Low, two discs per kart plus bombs, seekers and item boxes, so nothing floats.
- Look: a harder three-band toon ramp and a lower sun from the side, so every model has a lit, a mid and a shadow side; outlines now ink creases inside the karts and drivers too.
- Placeholder models: karts and drivers are built from bevelled boxes and 16-sided cylinders (tub, nose, side pods, bumper, engine, exhausts, wing, seat, steering wheel; suit, arms, helmet and visor). Wheels roll with distance, the front pair steers, the body leans in a drift, stretches on a hop and squashes on landing. Posts, trunks, the start gantry (with a chequered banner), item boxes and props use the same shapes.
- Track dressing: road shading, painted edge lines and centre dashes, start-grid brackets, grass patches, and candy props near the track (chevrons, flags, fences, lollipops, gumdrops).
- Effects: drift sparks fly back in trails with small round dust puffs; speed lines and an FOV kick only on a boost or past the normal top speed.
- Item boxes are lit candy boxes in a rainbow cycle with two glowing ribbons.
- Camera: it smooths its heading, not its position, so the kart stays low and centred; a new race or a capture moment snaps it into place. A spinning gem floats over each player's kart.
- HUD: the item is an icon in a framed box; place, lap counter, timer and speedometer are bigger and scale with the screen's short side, so they read at 1080p and on a phone.
- Review: scripted capture moments (a drift with sparks on High, an item hit on Low, the finish line on Medium) replace the camera-path views on the review card.

## 2026-10-14 — first version, ready for review

- Arcade kart physics, drift-charge boosts, five original items with position-weighted odds, AI racers with rubber-banding, laps, checkpoints, wrong-way, one or two players on a split screen, synthesised sounds (kit 001 steps 3 and 6).
- Demo game Sugarpeak Rally in `apps/kart-racer`: title, race, pause, settings (tier, effects, key rebinding, volume) and results.
- Live tuning pass (step 7): `item_respawn_s` 3 → 1.5 s, baked into `params.json`, so a player behind the AI pack still finds item boxes.
- Track edges: the road, grass bank, kerbs and rails of each 4 m segment are as long as the edge at their own offset, so bends no longer show dark wedge-shaped gaps.
- Walls: they stop the kart's edge rather than its centre (karts no longer sink into the rail), and the speed lost scales with how squarely the kart hits, so grinding along a curving wall no longer stops the kart dead.
- Low tier: below 120 000 triangles per frame only every other tree is drawn (about 127 000 → 95 000 triangles), so the Low bench passes.
