# Evani.Dev kit roadmap

One kit per session, top to bottom. The agent takes the first row with status `todo`, follows `docs/KIT_PROCESS.md`, and sets the status to `review` when done. You set it to `done` after playing the demo. Edit any row to change the order or the first look; `auto` lets the agent pick a fitting look, reusing an existing one where possible.

Statuses: `todo` · `in progress` · `review` · `done` · `blocked` (see CORE_REQUESTS.md)

Human fix rounds: each kit's Notes count the rounds of fixes Chris's own review sent back (`fix <id>: ...`), after the critic passed it. The goal is 0 to 1 per kit; more means the target pack, the automatic checks or the critic missed something, and the lesson goes into docs/KIT_PROCESS.md, the genre playbook or the look README (KIT_PROCESS step 10).

## Phase A: prove the engine (six very different kits)

| # | Kit | ID | Rank | Physics | First look | Status | Notes |
|---|---|---|---|---|---|---|---|
| 001 | Kart racer | `kart-racer` | #19 | simple | `toon` | done | Demo Sugarpeak Rally (`apps/kart-racer`): 322.5 KB gzipped shipped after the second-review pass (298.8 KB after the polish pass). Look `toon` (new); effects outline, speedlines, shake, bloom, shadows. Core additions: split-screen views, depth-reading effects, per-pad bindings, then for the polish pass a shadow map with blob shadows, crease outlines, bevelled-box and cylinder meshes and scripted capture moments (DECISIONS.md). Low draws one tree in three to stay under 100k triangles (87.6k in the item-hit capture). VR unavailable (WebXR with WebGPU not in browsers yet). Human fix rounds: 2. Free roam done (2026-10-15, still `review`): under `PlayMode::FreeRoam` the player drives Sugarpeak Loop alone, already racing, with no countdown, rivals, laps, race timer, places or finish; the HUD shows a FREE ROAM label; `fn free_roam`, the `free_roam_holds` test and `[playground] free_roam = true` are in. Shipped size 387.7 KB to 389.1 KB gzipped (+1.4 KB). Stage 2 gate pass (2026-10-15, still `review`): shipped size 389.7 KB gzipped on the merged engine (engine commit bd988d1); bench on the fixed controller passes Low, Medium, High, Ultra and VR (mono preset), cost order holds (median GPU ms 0.26, 0.62, 1.20, 3.35); geocheck 140 issues, 0 untagged; review card 0 FAIL (visual regression not run until Chris accepts a baseline); critic passed in round 5. Live tuning re-run on a private port (8185): `sweep.run` holds the sim clock so handling cannot show in it (a max_speed sweep leaves every look measure identical), so the five handling decisions (rubber band, boost tier 1, turn rate, boost multiplier, acceleration) were taken from bot-race tables (`playtest::tune_table`, an ignored test); every current value won or tied, so nothing was changed or baked. Evidence: `.vibedeck/review/kart-racer/gate2-tuning.md`. Touch controls done (2026-10-15, still `review`): the evani-core `touch` layer is on for the kart racer (README "Touch controls"). A floating stick on the left half steers, the throttle is held for the player while the controls are up (the Brake button takes it off; keyboard and pad play is unchanged), Drift, Item and Brake sit bottom right and Pause top right, clear of the timer and minimap. The layer shows only on a first touch and only during the countdown and the race (menus are tapped), in free roam too, and the free-roam controls line names the on-screen controls under touch. Checked in headless Chrome with synthetic touches at 390x844, 844x390 and 1340x688 (race and `?mode=free`): no control overlaps the HUD, every button is 48 px or more, and auto-accelerate, steer, drift, item, brake, pause and resume all pass; portrait fits. Unit tests drive a whole race by thumb alone. Shipped size 389.7 KB to 398.7 KB gzipped (+9.0 KB, the touch layer); geocheck 140 tagged, 0 untagged; review card 0 FAIL; bench passes all five tiers (median GPU ms 0.26, 0.61, 1.20, 3.11). Evidence: `.vibedeck/screenshots/kart-touch/`. Flicker fix (2026-10-16, still `review`): the dark flickering patches seen from the far title camera were thin closed slabs (road repair patches, stains, wear and skid strips, start-grid brackets, banner chequers, grass blobs) whose dark underside showed through the top; they are now one-sided tops (a plane, and a blob mesh without its bottom). Z-fighting detector, untagged patches 492 to 279; thin slabs of those kinds 0 left (remaining: kart-body and kerb overlaps, decals behind surfaces, and the 1 cm shadow discs). Evidence: `.vibedeck/screenshots/kart-flicker-fix/`. Last flicker spots cleared (2026-10-16, still `review`): road decals (paint, start chequer, wear and skid marks, repair patches, grid brackets, item box discs) sit 1 mm over the road mesh's real triangles, so none dips under the asphalt (the grid brackets' arms were half buried); lollipop swirl washers, one-sided shadow discs and stand strip, a start banner of solid pieces; geocheck z-fighting treats a closed concave mesh's back faces as hidden. With reversed-Z, z-fighting 279 untagged to 0 (none tagged); geocheck 140 issues, 0 untagged. Evidence: `.vibedeck/screenshots/zfight-clear/`. Verified by Chris 2026-10-16 ("stage 2 looks good"), status `done`. Gate 2 repackage (2026-10-16, m1-foundation 5becff2): shipped size 393.5 KB gzipped (wasm 360.6 KB + js 16.0 KB in the publish package; folder 1.21 MB); geocheck 140 tagged, 0 untagged, full z-fight scan 0; desktop bench 6 of 6 tiers PASS. Evidence: `.vibedeck/screenshots/gate2/`, `.vibedeck/publish-check/size-table.md`. |
| 002 | 2D platformer | `2d-platformer` | #36 | simple | `pixel16` | done | Demo Fennel's Leap (`apps/2d-platformer`): 280.4 KB gzipped shipped (live tooling excluded). Look `pixel16` (new); effects parallax, shake. Bench passes Low to Ultra and VR. Known cosmetic risk: the 10-tile pit near the start shows the near-bush layer's dark green fill, so it reads more like dark ground than a gap. Free roam done 2026-10-15 (branch `kit-002-free-roam`): under `PlayMode::FreeRoam` no lives, timer, game over or level goal, a pit or enemy touch is a free respawn on the last safe ground (or checkpoint), the HUD shows a small FREE ROAM label; `fn free_roam` returns true, `free_roam_holds` runs two minutes of scripted input, and `apps/2d-platformer/evani.toml` has `[playground] free_roam = true`. Play mode is unchanged; shipped size 308.5 -> 309.3 KB gzip (+0.8 KB). Gate 2 work 2026-10-15 (branch `kit-002-gate2`): game objects live in the ECS; the pixel HUD stays in the canvas and the review now checks it there (`[review] hud = ["canvas"]`); beetle shell red #D83838 -> #D82848 for distinct colours; params stay `kit.platformer.*` (owner `kit.2d-platformer`, see DECISIONS). Review card 0 FAIL (18 automatic checks pass, 9 by hand), 317.0 KB to download; critic packet round 1 ready, critic not run yet. Touch controls done 2026-10-16 (branch `plat-touch`): on-screen left, right, run, jump and pause, off on menus, the same actions as the keys (determinism unchanged). Portrait layout still to do. Still `review`. Verified by Chris 2026-10-16 ("stage 2 looks good"), status `done`. Gate 2 repackage (2026-10-16, m1-foundation 5becff2): shipped size 303.5 KB gzipped (wasm 282.9 KB + js 15.4 KB in the publish package; folder 0.79 MB; 78.4 KB, 20.8 percent, below the kart racer in wasm + js gzip); geocheck 0 issues, 0 untagged; desktop bench 6 of 6 tiers PASS. Evidence: `.vibedeck/screenshots/gate2/`, `.vibedeck/publish-check/size-table.md`. |
| 003 | Match-3 | `match-3` | #5 | none | `cartoon2d` | review | Demo Puff's Sweet Grid (`apps/match-3`): 337.4 KB gzipped shipped (review card of 2026-10-16: sparkles and confetti on the shared GPU particle system, +43 bytes against the card before). Look `cartoon2d` (new); effects juice, particles (new: the kit emits sparkles and confetti into the shared GPU particle system), shake, bloom (now allowed for 2D, as the catalog says). No physics module. ECS move done (pieces, pops, blasts, mascot as entities; pool for floats) and free roam built; ship was 319.7 KB gzip before the GPU particle merge. Core addition: `Input::primary_pointer` (DECISIONS.md). Phone and desktop layouts; mouse, touch, keys, pad. Independent critic passed in round 2 (mean 2.3, 0 must-fix; `.vibedeck/review/match-3/critic/critic-round-2.md`). VR unavailable (WebXR with WebGPU not in browsers yet). Verified by Chris 2026-10-16 ("stage 2 looks good"), status `done`. Gate 2 repackage (2026-10-16, m1-foundation 5becff2): shipped size 316.5 KB gzipped (wasm 279.8 KB + js 16.1 KB in the publish package; folder 1.16 MB, of which preview.png is 413 KB; 80.8 KB, 21.4 percent, below the kart racer in wasm + js gzip); geocheck 0 issues, 0 untagged; desktop bench 6 of 6 tiers PASS. Evidence: `.vibedeck/screenshots/gate2/`, `.vibedeck/publish-check/size-table.md`. S3-13c (2026-10-16, `fix match-3`): piece bodies drawn from the look's smooth sprites by name (kits/evani-kit-match-3/CHANGES.md); status is back to `review` until the card and the critic's round 3 (Stage 3 item R10 now applies) are done and Chris signs off again. |
| 004 | Top-down action-adventure | `top-down-action-adventure` | #77 | simple | `pixelhd` | in progress | Tilemaps, rooms, combat, inventory, dialogue |
| 005 | Base-building RTS | `base-building-rts` | #76 | simple | `stylizedpbr` | in progress | Hundreds of units, pathfinding, selection, fog of war |
| 006 | Tactical FPS | `tactical-fps` | #7 | simple | `photoreal` | in progress | First person, hitscan, first use of evani-net with bots. Planned 2026-10-15 (plan only; Chris approved its recommendations the same day): the demo is Chris's game **Hexagon**, rebuilt with a cinematic look (docs/HEXAGON_PLAN.md): hexagonal fortress, rotating middle ring, concrete destruction, 4 weapons, bots 4 v 4, offline first. Built after Gate 3 (Stage 3: glTF, skinning, KTX2, evani-convert, text and UI layer), in Stage 5. First look `photoreal` instead of `realpbr` (Chris, 2026-10-15). Genre differences to record in the playbook: no round economy; the core hold stands in for plant and defuse. |

## Phase B: everything else, most played first

Kits ship offline-first. Multiplayer genres get bots and local play now; online play arrives when evani-net matures.

| # | Kit | ID | Rank | Physics | First look | Status | Notes |
|---|---|---|---|---|---|---|---|
| 007 | Voxel sandbox | `voxel-sandbox` | #2 | simple | auto | todo |  |
| 008 | MOBA | `moba` | #4 | simple | auto | todo |  |
| 009 | Military FPS | `military-fps` | #6 | simple | auto | todo |  |
| 010 | Football (soccer) | `football-soccer` | #8 | simple | auto | todo |  |
| 011 | Block puzzle | `block-puzzle` | #9 | none | auto | todo |  |
| 012 | Open-world crime sandbox | `open-world-crime-sandbox` | #10 | rigid | auto | todo |  |
| 013 | Mobile base-builder strategy | `mobile-base-builder-strategy` | #11 | simple | auto | todo |  |
| 014 | Card and solitaire | `card-and-solitaire` | #12 | none | auto | todo |  |
| 015 | One-tap casual | `one-tap-casual` | #13 | simple | auto | todo |  |
| 016 | Monster collector | `monster-collector` | #14 | simple | auto | todo |  |
| 017 | Word game | `word-game` | #15 | none | auto | todo |  |
| 018 | Open-world RPG | `open-world-rpg` | #16 | simple | auto | todo |  |
| 019 | Digital board game | `digital-board-game` | #17 | none | auto | todo |  |
| 020 | Hero shooter | `hero-shooter` | #18 | simple | auto | todo |  |
| 021 | Life sim | `life-sim` | #20 | simple | auto | todo |  |
| 022 | Basketball | `basketball` | #21 | simple | auto | todo |  |
| 023 | Endless runner | `endless-runner` | #22 | simple | auto | todo |  |
| 024 | Open-world adventure | `open-world-adventure` | #23 | rigid | auto | todo |  |
| 025 | Survival crafting | `survival-crafting` | #24 | simple | auto | todo |  |
| 026 | Farming sim | `farming-sim` | #25 | simple | auto | todo |  |
| 027 | Arcade racer | `arcade-racer` | #26 | simple | auto | todo |  |
| 028 | Merge puzzle | `merge-puzzle` | #27 | none | auto | todo |  |
| 029 | Car soccer | `car-soccer` | #28 | rigid | auto | todo |  |
| 030 | Turn-based JRPG | `turn-based-jrpg` | #29 | simple | auto | todo |  |
| 031 | Table sports | `table-sports` | #31 | rigid | auto | todo |  |
| 032 | Number and grid logic | `number-and-grid-logic` | #32 | none | auto | todo |  |
| 033 | Party minigames | `party-minigames` | #33 | simple | auto | todo |  |
| 034 | Idle and incremental | `idle-and-incremental` | #34 | none | auto | todo |  |
| 035 | Digital card game | `digital-card-game` | #35 | none | auto | todo |  |
| 036 | Loot action RPG | `loot-action-rpg` | #37 | simple | auto | todo |  |
| 037 | Extraction shooter | `extraction-shooter` | #38 | simple | auto | todo |  |
| 038 | Cooking and time management | `cooking-and-time-management` | #39 | simple | auto | todo |  |
| 039 | Soulslike | `soulslike` | #40 | simple | auto | todo |  |
| 040 | Platform fighter | `platform-fighter` | #41 | simple | auto | todo |  |
| 041 | Driving and truck sim | `driving-and-truck-sim` | #42 | simple | auto | todo |  |
| 042 | Tower defense | `tower-defense` | #43 | none | auto | todo |  |
| 043 | Virtual pet | `virtual-pet` | #44 | simple | auto | todo |  |
| 044 | 3D action-adventure | `3d-action-adventure` | #45 | simple | auto | todo |  |
| 045 | Tank combat | `tank-combat` | #46 | simple | auto | todo |  |
| 046 | Bubble shooter | `bubble-shooter` | #47 | simple | auto | todo |  |
| 047 | Auto battler | `auto-battler` | #48 | none | auto | todo |  |
| 048 | 4X strategy | `4x-strategy` | #49 | none | auto | todo |  |
| 049 | Physics puzzle | `physics-puzzle` | #50 | rigid | auto | todo |  |
| 050 | Tap rhythm | `tap-rhythm` | #51 | none | auto | todo |  |
| 051 | Team sports sim | `team-sports-sim` | #52 | simple | auto | todo |  |
| 052 | Survival horror | `survival-horror` | #53 | simple | auto | todo |  |
| 053 | 2D sandbox | `2d-sandbox` | #54 | simple | auto | todo |  |
| 054 | Visual novel | `visual-novel` | #55 | none | auto | todo |  |
| 055 | Action roguelite | `action-roguelite` | #56 | simple | auto | todo |  |
| 056 | Survivors-like | `survivors-like` | #57 | simple | auto | todo |  |
| 057 | Physics driving | `physics-driving` | #58 | rigid | auto | todo |  |
| 058 | Social deduction | `social-deduction` | #59 | simple | auto | todo |  |
| 059 | Deckbuilder roguelike | `deckbuilder-roguelike` | #60 | none | auto | todo |  |
| 060 | City builder | `city-builder` | #61 | none | auto | todo |  |
| 061 | Isometric CRPG | `isometric-crpg` | #62 | simple | auto | todo |  |
| 062 | 2D fighting | `2d-fighting` | #63 | simple | auto | todo |  |
| 063 | Escape room and hidden object | `escape-room-and-hidden-object` | #64 | none | auto | todo |  |
| 064 | Golf and mini golf | `golf-and-mini-golf` | #65 | rigid | auto | todo |  |
| 065 | 3D platformer | `3d-platformer` | #67 | simple | auto | todo |  |
| 066 | Psychological horror | `psychological-horror` | #68 | simple | auto | todo |  |
| 067 | Sim racer | `sim-racer` | #69 | simple | auto | todo |  |
| 068 | Metroidvania | `metroidvania` | #70 | simple | auto | todo |  |
| 069 | Tycoon | `tycoon` | #71 | none | auto | todo |  |
| 070 | Physics sandbox | `physics-sandbox` | #72 | rigid | auto | todo |  |
| 071 | 3D fighting | `3d-fighting` | #73 | simple | auto | todo |  |
| 072 | Note-highway rhythm | `note-highway-rhythm` | #74 | simple | auto | todo |  |
| 073 | Cover shooter | `cover-shooter` | #75 | simple | auto | todo |  |
| 074 | Flight | `flight` | #78 | simple | auto | todo |  |
| 075 | Fishing | `fishing` | #79 | simple | auto | todo |  |
| 076 | Job simulator | `job-simulator` | #80 | simple | auto | todo |  |
| 077 | Character action | `character-action` | #81 | simple | auto | todo |  |
| 078 | Tactics RPG | `tactics-rpg` | #82 | none | auto | todo |  |
| 079 | Space exploration | `space-exploration` | #83 | simple | auto | todo |  |
| 080 | Colony sim | `colony-sim` | #84 | none | auto | todo |  |
| 081 | Factory automation | `factory-automation` | #85 | none | auto | todo |  |
| 082 | Speed platformer | `speed-platformer` | #86 | simple | auto | todo |  |
| 083 | Puzzle platformer | `puzzle-platformer` | #87 | rigid | auto | todo |  |
| 084 | Squad tactics | `squad-tactics` | #88 | none | auto | todo |  |
| 085 | Competitive RTS | `competitive-rts` | #89 | simple | auto | todo |  |
| 086 | Naval combat | `naval-combat` | #90 | simple | auto | todo |  |
| 087 | Brick breaker | `brick-breaker` | #91 | simple | auto | todo |  |
| 088 | Maze chase | `maze-chase` | #92 | simple | auto | todo |  |
| 089 | Cinematic choice adventure | `cinematic-choice-adventure` | #93 | simple | auto | todo |  |
| 090 | Logic puzzle | `logic-puzzle` | #94 | none | auto | todo |  |
| 091 | Twin-stick shooter | `twin-stick-shooter` | #95 | simple | auto | todo |  |
| 092 | Stealth action | `stealth-action` | #96 | simple | auto | todo |  |
| 093 | Skate and extreme sports | `skate-and-extreme-sports` | #97 | simple | auto | todo |  |
| 094 | Grand strategy | `grand-strategy` | #98 | none | auto | todo |  |
| 095 | First-person puzzle | `first-person-puzzle` | #99 | rigid | auto | todo |  |
| 096 | Rally and off-road | `rally-and-off-road` | #100 | simple | auto | todo |  |
| 097 | Tennis and racket sports | `tennis-and-racket-sports` | #101 | simple | auto | todo |  |
| 098 | Space trading sim | `space-trading-sim` | #102 | simple | auto | todo |  |
| 099 | Beat 'em up | `beat-em-up` | #103 | simple | auto | todo |  |
| 100 | Boxing and combat sports | `boxing-and-combat-sports` | #104 | simple | auto | todo |  |
| 101 | Arena FPS | `arena-fps` | #105 | simple | auto | todo |  |
| 102 | Interactive fiction | `interactive-fiction` | #106 | none | auto | todo |  |
| 103 | Mech combat | `mech-combat` | #107 | simple | auto | todo |  |
| 104 | Run and gun | `run-and-gun` | #108 | simple | auto | todo |  |
| 105 | Walking sim | `walking-sim` | #109 | simple | auto | todo |  |
| 106 | Boomer shooter | `boomer-shooter` | #110 | simple | auto | todo |  |
| 107 | Vehicular combat | `vehicular-combat` | #111 | rigid | auto | todo |  |
| 108 | Point-and-click adventure | `point-and-click-adventure` | #112 | none | auto | todo |  |
| 109 | Real-time tactics | `real-time-tactics` | #113 | simple | auto | todo |  |
| 110 | Artillery | `artillery` | #114 | simple | auto | todo |  |
| 111 | Classic arcade shooter | `classic-arcade-shooter` | #115 | simple | auto | todo |  |
| 112 | Shoot 'em up | `shoot-em-up` | #116 | simple | auto | todo |  |
| 113 | Immersive sim | `immersive-sim` | #117 | rigid | auto | todo |  |
| 114 | Space dogfighting | `space-dogfighting` | #118 | simple | auto | todo |  |
| 115 | Top-down racer | `top-down-racer` | #119 | simple | auto | todo |  |
| 116 | Rhythm action | `rhythm-action` | #120 | simple | auto | todo |  |
| 117 | Dungeon crawler | `dungeon-crawler` | #121 | simple | auto | todo |  |
| 118 | Pinball | `pinball` | #122 | rigid | auto | todo |  |
| 119 | Turn-based wargame | `turn-based-wargame` | #123 | none | auto | todo |  |
| 120 | Rail shooter | `rail-shooter` | #124 | simple | auto | todo |  |
| 121 | Classic roguelike | `classic-roguelike` | #125 | none | auto | todo |  |
| 122 | Anti-gravity racer | `anti-gravity-racer` | #126 | simple | auto | todo |  |
| 123 | God game | `god-game` | #127 | simple | auto | todo |  |

## Phase C: massive online kits (need mature evani-net and servers)

| # | Kit | ID | Rank | Physics | First look | Status | Notes |
|---|---|---|---|---|---|---|---|
| 124 | User-generated game platform | `user-generated-game-platform` | #1 | rigid | auto | todo | Start only after evani-net has been proven by several Phase B kits |
| 125 | Battle royale | `battle-royale` | #3 | simple | auto | todo | Start only after evani-net has been proven by several Phase B kits |
| 126 | MMORPG lite | `mmorpg-lite` | #30 | simple | auto | todo | Start only after evani-net has been proven by several Phase B kits |
| 127 | .io multiplayer | `io-multiplayer` | #66 | simple | auto | todo | Start only after evani-net has been proven by several Phase B kits |

## Test kits (not genres)

Kits that exist to test the engine and the review process, not to ship a genre. `next kit` never takes them; the review tool reads their row like any other.

| # | Kit | ID | Rank | Physics | First look | Status | Notes |
|---|---|---|---|---|---|---|---|
| 000 | Test kit: engine sandbox | `example` | - | simple | `basic` | review | Demo `apps/sandbox`: a grid of spinning cubes and spheres with bloom and a vignette, no win or lose state. Used for Stage 1 gate check 5 (a complete review card) and as the smallest real kit. Not in catalog/genres.json, so no genre playbook. |
