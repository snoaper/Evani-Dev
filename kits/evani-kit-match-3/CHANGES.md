# Changes

Kit changes, newest first. Values tuned live and baked are logged in `looks/evani-look-cartoon2d/CHANGES.md` (bake.export writes there).

## 2026-10-16 — callouts clear of Puff, confetti over the board (critic round 3)

- Combo callouts (landscape) have a slot of their own between the HUD panel's shadow and the top of Puff's reach (`Layout::callout_slot`, `hud::callout_slot`: the biggest callout box, the big style fitted to the panel for the shortest word, plus 3 px of air each side). Puff stands below it, so his sprout star can no longer be drawn over "Great!", "Shuffle!" or "Superb!" (critic rounds 1 to 3, seen at 844x390 and 1280x720). The cost is paid by Puff, not the panel: he gives up size first only so the panel keeps its full height (never under 18 % of the screen height; 720p: 173 to 149 px), then so a short screen keeps the panel's 15 px unit (`MIN_MASCOT_VH` 0.12 to 0.05; a sideways phone gets a Puff of about 43 px, 844x390, was 65). Landscape callout fonts have lower floors (16 and 18 px, was 22 and 26) so a short screen has room for the slot; portrait is unchanged.
- Test `combo_callouts_never_overlap_puff_the_panel_or_the_board` (hud.rs): every callout word, at every moment of its pop, at 19 desktop, phone and tablet sizes, landscape and portrait, never overlaps Puff's reach box, the panel and its shadow, or the board, and stays inside the panel's width.
- Win confetti is shed from the board's upper rows and floats down over the tiles (a fifth of the sparkles' gravity, so most of it is still up when the win card shows, about 1.3 s in); like the sparkles its life ends where its path reaches the board's edge (it was thrown up from the middle and fell beside and below the board, off it). Test `confetti_falls_over_the_board_and_never_past_it`. Sparkle clipping now ends on the last step still inside the edge, not the one that crossed it.
- Test `blast_sparkles_start_on_the_popped_piece_in_every_layout`: every blast effect starts at its piece's screen position (or along its streak's row or column), inside the board, in five layouts including portrait. The stray sparkles at a portrait phone's edge in round 3's pictures were not emitted wrong: the capture tool resized to portrait without staging the moment again, so the particles emitted for the sideways layout stayed where they were. `tools/live-check/capture.mjs` now stages the moment again for the portrait shot, as it does for the phone shot.
- Gameplay unchanged (determinism hashes identical). Particles of a game that is resized or turned while an effect plays still stay where they were emitted (they last under a second).

## 2026-10-16 — piece bodies drawn from the look's smooth sprites, by name (S3-13c first half)

- The kit asks the sprite art for `piece.cherry`, `piece.toffee`, `piece.lemon`, `piece.mint`, `piece.berry`, `piece.grape` and `piece.prism` (`SpriteArt::cell`); the `cartoon2d` look supplies them as 128 px smooth frames with mips, so pieces keep clean edges at 128 px cells and shrink without shimmer. A look without those frames (look-basic, or any look that does not supply them) gets the built-in shapes exactly as before (test `named_frame_falls_back_to_a_builtin_shape`). The kit does not depend on the cartoon2d crate.
- Only the piece body layers (shadow, ink, shade, lit) use the smooth art. The specials' add-ons (such as the donut hole), the tray, Puff and the particles keep the built-in shapes.
- There are no heart or wrapped-sweet pieces in this kit (cherry circle, toffee rounded square, lemon star, mint diamond, berry triangle, grape circle; the prism is a circle), so silhouettes and colours are unchanged. Swapping the cherry for a heart would be a look change (the kit asks by colour name); it changes the look bible, so it is left for Chris.
- The ink outline follows the art's edge (`SpriteArt::edge`, `SpriteEdge`): the built-in shapes keep their fixed +1 px allowance; crisp frames grow by their own antialiased edge, so the outline weighs the same as before (about 4 px of ink mass against 5.1 px with the fixed allowance; the look asks for 3.5 px). Test `the_ink_allowance_follows_the_sprite_edge`.
- Gameplay unchanged: the determinism hashes and the review-dump hashes are identical.
- Ship size of the demo: 316.5 KB to 325.4 KB gzip (324,144 to 333,263 bytes, +8.9 KB). This is over the 2 to 4 KB plan estimate because match-3 is the first game to turn `sprite-art` on: the engine's packer, mip chain and name lookup are about +7.3 KB gzip (DECISIONS 2026-10-15), and the look's frame generator and the kit's named-frame code are the remaining 1.6 KB; the frames themselves are drawn at startup, not downloaded. 2d-platformer, kart-racer and sandbox ship builds have the same wasm and js SHA-1.
- Review: the pictures change slightly (smoother edges), so the visual regression check (item 21) will flag them once a baseline exists; none is accepted yet, Chris accepts. Captures: .vibedeck/screenshots/m3-art/.

## 2026-10-16 — HUD panel as tall as its measured content

- Landscape panel: `HUD_UNITS` 13.6 and `HUD_FIXED_PX` 50 (was 15 and 32, never measured). The content measured in the page (level 3, banner showing) needs 454 px at `--u` 30, 387 at 25.2, 364 at 23.6, 299 at 18.9, 250 at 15 and 229 at 13.3, a line of 13.45 u + 50 px; the model is that line plus about 4 px. The desktop panel is 458 px at 1920x1080 (was 482). Short screens still shrink Puff first, then the unit; 844x390 checked: panel 254 px for a 250 px content, Puff 65 px.
- Portrait strip: it was 23 % of the height whatever it held, so the goal chips in its last row ran up to 29 px past its border on common phone and tablet sizes (390x844: 14 px). It is now `PORTRAIT_HUD_UNITS` 6.2 u + `PORTRAIT_HUD_FIXED_PX` 50 px at the unit it already had; the board keeps its size on phones. The gap between the strip and the board is at least 1.5 u + 6 px so the "Few moves left!" banner no longer touches the top piece on small phones (320x568); there the board is about 5 % smaller.
- `Layout` carries `unit` (the page's `--u`) and `hud_need()`; test `panel_is_at_least_as_tall_as_its_measured_content` pins panel height >= measured content height for 13 landscape and 6 portrait sizes. Layout only: the determinism hashes and the other tests are unchanged.

## 2026-10-16 — sparkles and confetti on the shared GPU particle system

- The kit's own `Sparkles` pool and its sprite drawing are removed. `sparks.rs` emits sparkles (pop, blast, prism) and the win confetti into the engine's `ParticleQueue`; the shared GPU system simulates and draws them. Same colours, shapes, timing and spread; blast and pop sparkles still stay inside the board (their life ends where their path reaches its edge), confetti still falls past it.
- The kit keeps a seeded `FxStream` for the particles' randomness; it never feeds back into the game. The determinism hashes are unchanged in value (the sparkle block left the fold; `EXPECTED` explains).
- The particle cap is the engine's (tier `max_particles`, lowered by `fx.particles.max`). Particles do not follow screen shake and keep animating while paused.
- Review captures replay a moment and use `ParticleQueue::emit_ago` so the frame shows particles at their real age.
- Ship size of the demo: 337.4 KB gzip, 345,482 to 345,525 bytes (+43 bytes).

## 2026-10-15 — pieces in the ECS, free roam

- Pieces (`Piece`, `Cell`, `Motion`), clearing pieces (`Popping`), special-piece blasts (`Blast`) and the mascot (`Puff`) are now entities. `Grid` is the rules view, gathered from the entities at the start of every tick and synced back after each board change. `Game` keeps only session and score.
- Score floats live in a fixed pool (`Floats`; sparkles moved to the shared particle system on 2026-10-16; floats hold their text inline, no per-frame allocation).
- New `Request::Swap(a, b)` for bots, the app and tests.
- Free roam: `free_roam()` is true; under `PlayMode::FreeRoam` there are no goals, move cost, Rush, win or lose, and the board keeps refilling. Test `free_roam_holds`; the app declares `[playground] free_roam = true`. A lost Play-mode game reports `game_over`.
- Behaviour is unchanged: the determinism test keeps its original hashes.
- Ship size of the demo: 319.7 KB gzip, up from 308.4 KB (+3.7%), from the ECS storage code for six component types.

## 2026-10-14 — first version, ready for review

- Board rules: swap validation, match groups (lines, L, T, +), Streak, Pop Bomb and Prism with every two-special combo, chain reactions with distance delays, gravity through holes, refills, frosting, boards generated with no starting match and at least one move, hints and reshuffles (kit 003 step 3).
- Game flow: title with a self-playing attract board, level intro, play, Sugar Rush, win (1–3 stars) or lose; three data levels.
- Juice: squash and stretch on swaps, falls and landings, idle breathing, pop-and-spin clears with shockwave rings, sparkles, beams and rings for specials, floating score and cascade callouts, Puff the mascot reacting to the game, board shake.
- Input: one pointer path for mouse, pen and touch (drag or tap-tap), keyboard cursor and gamepad; works for a tap whose down and up land in the same frame (core `Input::primary_pointer`).
- Demo game Puff's Sweet Grid in `apps/match-3`: title, levels, HUD laid out for desktop and phone, pause, settings (tier, effects, key rebinding, volume), results; synthesised sounds (step 6).
