# Playbook items: Fennel's Leap

The items of `docs/playbooks/2d-platformer.md`, each with how this kit meets it (and the test or check that shows it). [x] met, [~] partly met. The critic scores these under R8.

## Camera

- [x] **Platform snapping.** The camera's height follows the anchor, the feet height of the ground the hero last stood on (`Cam::anchor`), not each jump; test `camera_holds_still_through_a_jump`.
- [x] **Lower-third ground anchor.** The ground line sits at about 72 % of the screen height when standing (`cam_ground_frac`), everywhere including the valley: the level keeps five rows of deep soil under its lowest ground, so the view never rests on the map's bottom; test `camera_framing_keeps_the_ground_low_and_the_hero_in_frame` (every settled frame of the bot's run in 60–80 %, none resting on the bottom) and geocheck-2d "Camera framing" (0 issues). Measured in the 1920x1080 captures: feet at about 72 % in shot-start, shot-checkpoint and shot-spikes (y about 770-775 of 1080) and about 75 % in the valley (video-frame-4).
- [x] **Horizontal look-ahead.** The camera leads in the direction of travel with an eased signed offset (`Cam::look`) that eases over when the hero turns.
- [x] **Vertical dead zone.** The anchor moves only when the hero stands on different ground, so small hops and slope steps do not move the camera; long falls and tall climbs do (same tests).
- [x] **Terrain at most about 35 % of the screen.** Checked by geocheck-2d (`camera`, item 24) on the bot's whole run: 0 issues.

## Game feel

- [x] **Coyote time.** About 0.09 s after leaving a ledge; test `coyote_time_allows_a_late_jump`.
- [x] **Jump buffer.** A jump pressed just before landing fires on landing; test `jump_buffering_fires_on_landing`.
- [x] **Variable jump height.** A tapped jump is under half the height of a held one; test `variable_jump_height`.
- [x] **Responsive ground control.** Fast ground acceleration and turn-around, less in the air; tunables `kit.2d_platformer.*` (registered, live-tunable).

## Level readability

- [x] **Solid looks solid.** Grass-capped earth, ashlar stone and planks are outlined with a lit top edge; the backdrop is lighter and less saturated (look bible, "Sky and backdrop").
- [x] **Hazards read as hazards.** Spikes are the only sharp shape in the level, the beetle is red and the hopper blue against the fox's orange (review dump: colour distances), and the bottomless pit is a flat dark void (moment `pit`, before and after captures in `.vibedeck/screenshots/gate2/`). Geocheck-2d "Bot level checks" finds no hazard hidden behind decor.
- [~] **One idea at a time.** Partly: the first thing in the level is the 10-tile pit with a wooden plank and a grassy ledge over it as the safe way across, spikes and water pools come later beside their own props (pillar, reeds), but the pacing of beetles, hoppers, pools and the moving platforms is judged by playing, not measured by a check.

## Collectible placement

- [x] **Clear air.** Coins keep 2 px of clear air from terrain; test `a_centred_coin_keeps_two_pixels_of_air` and geocheck-2d "Overlaps and clear air" (0 issues).
- [x] **Arcs follow real jumps.** Test `every_coin_is_reachable_arcs_are_one_jump_and_coins_keep_clear`.
- [x] **Every collectible reachable.** Same test and geocheck-2d "Bot level checks" (0 issues).

## Juice

- [x] **Popups rise and fade in under a second.** Floating text lives 0.8 s (`POP_TEXT_LIFE`); test `floating_text_stays_clear_merges_and_fades`; geocheck-2d "Text popups" (0 issues).
- [x] **Popups merge into combos.** Score pops merge within 0.5 s (`POP_MERGE`); test `score_pops_merge_then_expire`.
- [x] **Never cover the player.** Layout pushes text clear of the hero; test `layout_pushes_text_clear`.
- [x] **Landing and hit feedback.** Landing dust and a baked landing squash frame, a screen shake on a hurt (the `shake` effect), a blinking hero while invulnerable.

## Terrain

- [x] **Variation with depth.** Earth darkens in steps (brown then soil) with dithered bands, roots, pebbles, rocks, rare bones; slopes and ledges vary the surface.
- [x] **Parallax backdrop.** Three strips at 15 %, 40 % and 70 % of the camera plus clouds (effect `parallax`, switchable in Settings).

## Pixel integrity

- [x] **One pixel scale.** Everything on the art grid; tests `every_sprite_sits_on_the_art_pixel_grid` and geocheck-2d "Pixel integrity" (0 issues).
- [x] **Integer positions.** The camera and sprites snap to whole art pixels (`Screen::snap`).
- [x] **Integer-scaled captures.** 480 × 270 art pixels at 4 device pixels per art pixel at 1920 × 1080 (512 × 288 at 5 on 2560 × 1440); test `whole_pixel_scale` and the showcase captures (every `shot-*.png` is 1920 × 1080 with 4 × 4 device pixels per art pixel).
