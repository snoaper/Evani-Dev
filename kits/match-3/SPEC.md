# Kit 003: Match-3 (`match-3`)

Roadmap row 003 · physics `none` · first look `cartoon2d` · camera `board-ui`.
Catalog entry: `catalog/genres.json` → `match-3`. Reference games describe mechanics only; every name, piece, character, level and logo here is original.

Crates: kit `kits/evani-kit-match-3` (`KitPlugin::name()` = `match-3`, owner `kit.match-3`), look `looks/evani-look-cartoon2d`, demo `apps/match-3` (**Puff's Sweet Grid**, mascot Puff, a round cloud-candy creature).
Param names use `kit.match_3.*` (param segments allow only `a-z`, `0-9` and `_`; the owner stamp keeps the hyphenated kit id).

What this kit proves (roadmap note): a game with **no physics module at all**, a **board and UI** game (DOM HUD plus a sprite-drawn board), **touch and mouse input** on the same code path (swipe or tap-tap), and **juice** (squash and stretch, pops, sparkles, floating score, combo callouts, shake).

## 1. Gameplay systems

Every number below is a registered param (default shown) unless marked "data". Data lives in JSON shipped with `include_str!` and registered with `data_file`. The game runs on the fixed 60 Hz tick; all randomness is a seeded `Pcg32` per level, so a level plays the same for the same moves.

### 1.1 Swap validation

- The board is a grid of cells (data: width, height and a mask of playable cells per level). A cell holds at most one piece; pieces have one of up to 6 colours (`colors` per level, data) and a kind (normal or one of the specials in 1.3).
- Pointer (mouse, pen, touch, one code path through `evani_core::input`): press on a piece and drag more than `swipe_threshold` (0.35) of a cell towards a neighbour to swap with it; or tap a piece (it lifts and bobs) and tap a neighbour. Tapping the selected piece again or a far piece moves the selection.
- Keyboard and gamepad: arrows / d-pad move a cursor; Space, Enter or pad A picks the piece; then an arrow swaps in that direction (Esc / B drops the pick). Rebindable in settings.
- A swap is valid when, after it, either piece is part of a line of three or more of one colour, or either piece is a Prism, or both are specials (special combos, 1.3). An invalid swap slides `invalid_slide` (0.45) of the way, then bounces back with a little head-shake; it costs no move.
- Swap animation: `swap_s` (0.16 s) with an ease-in-out; the moved pieces squash in the direction of travel (juice).

### 1.2 Cascade resolution

- After a valid swap, every horizontal and vertical run of 3+ same-coloured pieces is found. Runs of one colour that share a cell are one match group.
- Cleared pieces pop (scale up to `fx.juice.pop_scale`, then shrink and spin) over `clear_s` (0.24 s); specials in the cleared set fire (1.3) and can fire more specials (chain reactions); each fired cell gets a short delay by its distance from the source (`chain_delay_s` 0.035 s per cell) so blasts sweep across the board.
- Gravity: pieces above an empty cell fall down their column (passing through holes in the mask) under `fall_gravity` (55 cells/s²) up to `fall_speed` (16 cells/s); new pieces drop in from above the board with the seeded RNG. Landing squashes the piece (`land_squash_s` 0.16 s, amount from the look's `squash`).
- When everything has landed the board is checked again; every further round is a cascade, and the cascade count raises the score and shows a callout (Nice!, Great!, Superb!, Fantastic!, Unstoppable!).
- Score per match group: formula `kit.match_3.match_score(pieces, cascade, special)` = `pieces * 60 * (1 + cascade * 0.5) + special * 120` (`cascade` 0 for the first round; `special` 1 when the group made a special). Pieces cleared by a special score `blast_score` (40) each, times the same cascade factor. Floating score text rises from each group's centre (juice).
- If no valid move exists once the board settles, the pieces (specials included) are reshuffled with the level RNG until there is no match and at least one move; the pieces fly to their new cells over `shuffle_s` (0.6 s).

### 1.3 Special pieces

Made from a match group, at the swapped cell if it is in the group, else at the group's middle cell:

| Match | Makes | When fired |
|---|---|---|
| 4 in a line | **Streak** (horizontal or vertical, along the line) | clears its whole row (horizontal) or column (vertical) |
| L, T or + shape (lines of 3+ crossing) | **Pop Bomb** | clears the 3×3 around it (`bomb_radius` 1) |
| 5+ in a line | **Prism** (colourless) | clears every piece of one colour: the colour it was swapped with, else the most common colour |

Combos (swapping two specials, always valid):

- Streak + Streak: clears the row and the column of the target cell (a cross).
- Streak + Pop Bomb: clears three rows and three columns.
- Pop Bomb + Pop Bomb: clears the 5×5 around the target (`bomb_radius` + 1).
- Prism + normal piece: clears every piece of that colour.
- Prism + Streak or Pop Bomb: every piece of the other's colour becomes that special, then they all fire.
- Prism + Prism: clears the whole board.

A special cleared by any match or blast fires too (chains). Firing pushes screen-shake trauma: `trauma_streak` (0.25), `trauma_bomb` (0.4), `trauma_prism` (0.6), plus `trauma_cascade` (0.08) per cascade round.

### 1.4 Level goals and move limits

Levels are data (`data/levels.json`): name, board mask, colours, moves, seed, goals and three star thresholds. Goals:

- `collect`: clear N pieces of a colour (any way: matches, blasts).
- `frosting`: clear every frosting tile. Frosting (1 or 2 layers, in the mask) sits under a piece; each time the piece on it is cleared, one layer melts.
- `score`: reach a score (the first star threshold is always implied).

Each valid swap uses one move (`moves` per level, data). Win: every goal met when the board settles, even on the last move; the remaining moves become **Sugar Rush** (each left-over move turns a random piece into a Streak that fires, `rush_step_s` 0.25 s apart, scoring as blasts), then the result screen with 1–3 stars. Lose: no moves left and a goal unmet once the board settles.

The demo ships three short levels: **Sprinkle Meadow** (8×8, 5 colours, collect goals), **Frosting Falls** (8×8 with holes, frosting goal) and **Prism Peak** (9×9, 6 colours, collect and frosting). Level 1 alone uses every system.

### 1.5 Board generation with no starting matches

- Fill each playable cell from the level RNG, re-rolling any colour that would make a line of three with the two cells to its left or above (scan order: row by row, left to right), so the starting board has no match.
- Then the board must have at least one valid move; if not (rare), refill with the next RNG draws (at most `gen_attempts` 50 tries, then a fixed fallback pattern that is known to have moves).
- A hint shows the best move after `hint_delay_s` (5 s) idle: the two pieces wobble towards each other. The best move is the one with the highest immediate value (specials made, then pieces cleared); ties go to the first in scan order, so it is deterministic.

### 1.6 Flow, pause and the attract board

Title (an attract board plays itself behind the menu, AI moves every `attract_move_s` 1.6 s) → Level intro (goal card, `intro_s` 1.4 s) → Play → Sugar Rush (on a win) → Result (win with stars, or lose) → next level, retry or title. Pause (Esc, P, pad Start or the pause button) freezes the sim; settings over the game also pause it.

## 2. Camera

`board-ui` (the catalog camera for match-3, which `cartoon2d` supports). No 3D camera moves: the board is drawn as screen-space sprites in CSS pixels, laid out from the canvas size every frame (`layout.rs`, pure and tested): landscape puts the HUD panel and Puff left of the board, portrait puts the HUD above it and Puff in the HUD corner. Cell size is the largest that fits, between `min_cell_px` (28) and `max_cell_px` (96). Screen shake moves the board origin in pixels (`fx.shake.amount` × `shake_px_per_unit` 40) and rolls nothing (a rolling grid hurts reading). The bench path is a fixed camera (the sprites ignore it).

## 3. Physics

`none`, exactly as the catalog says: evani.toml `physics = "none"`, so `evani-physics` is not in the build (checked by the review card's marker scan). Falling pieces use the kit's own one-axis gravity tween, not a physics engine.

## 4. Look

`cartoon2d` (row says `cartoon2d`; new look crate `looks/evani-look-cartoon2d`, catalog `cost_points` 1, style class 2D). Catalog render rules: vector-style sprites with 3 to 4 px dark outlines, two-tone cel shading baked into sprites, squash and stretch in the animator. How that is done with the engine's sprite atlas (white shapes tinted per sprite):

- Each piece is layered sprites: a soft contact shadow (offset down-right, away from the light), an ink outline (the shape enlarged by `look.cartoon2d.outline_px`, 3.5 px), the shade tone (the colour × `look.cartoon2d.shade`, 0.62) and the lit tone (slightly smaller, offset up-left, so a crescent of shade stays on the lower right), plus a small highlight up-left. The light comes from the top left everywhere (pieces, board frame, Puff), so the light direction reads.
- Pieces differ by shape as well as colour (colour-blind safe): cherry circle (red), toffee square (orange), lemon star (yellow), mint diamond (green), berry triangle (blue), grape ring (purple).
- The look supplies palettes by role (`pieces` 6 colours, `ui` ink, board light, board dark, frame, background top, background glow) and style params read by name `look.<look>.<param>` (`outline_px`, `shade`, `highlight`, `squash`, `shadow`); a look without them gets the kit's flat defaults (no outline, no shade split), which is how the kit runs with the `basic` look.
- Its own `tonemap` body: identity below 0.8 with a soft shoulder above, so flat cel colours stay exactly as painted and only HDR effect sprites (sparkles, beams) roll off and bloom. Post params: `render.clear_color` (warm candy sky), `render.saturation` 1.05, `fx.bloom.threshold` 1.2 (only HDR effect sprites glow).
- The look's `checker` generator draws a soft two-tone board tile (cell 0 of the atlas), used for the board backing.

## 5. Effects

Start from none; each kept only if a capture is clearly better (kit process, "Choosing effects"). Match-3 is a slow, readable puzzle: juice and sparkle, no motion blur, chroma, grain or depth of field.

| Effect | Cost | Why |
|---|---|---|
| `juice` (new, params only) | 0 | Reads every move: squash on swaps and landings, pop-and-spin clears, floating score, combo callouts, star bar bounce. Off = pieces just vanish and slide. |
| `particles` (new, params only; the shared GPU particle system draws it) | 2 | Reads matches and blasts: sparkles in the piece colour, beam streaks for Streaks, a burst ring for bombs, confetti on a win. The kit emits into the shared queue; the engine caps them at the tier's `max_particles`, lowered by `fx.particles.max` (600). |
| `shake` (existing) | 0 | Reads big moments: blasts and cascades push trauma; the kit moves the board origin. Comfort-off in VR. |
| `bloom` (existing) | 3 | Sells the specials and blasts: Prism shimmer, beams and sparkles are HDR (above the look's 1.2 threshold) and glow; flat pieces never bloom. |

Not used: `outline` (a depth-edge effect for 3D; the cartoon2d outlines are baked into the sprites as the catalog asks), vignette (no reading value on a bright board), speedlines, chroma, grain, motion blur, depth of field (clarity), shadows and ambient occlusion (3D only; contact shadows are sprites).

The catalog says particles are "compute-shader simulation with instanced drawing". Since 2026-10-16 that is true for match-3: the kit keeps no particle pool and draws no particle; it emits sparkles and confetti into the engine's `ParticleQueue` (`src/sparks.rs`) and the shared GPU particle system simulates and draws them. Beams and rings stay sprites, as they are shapes of the special, not particles.

Start-up per tier (`plan_budget`: look 1 point plus effects; `effects_on` in `assets/tiers/*.json` gains `particles` and `juice`):

| Tier | Points | On at start | Used |
|---|---|---|---|
| Low | 7 | juice, particles, shake (Low's preset starts without bloom) | 3 |
| Medium | 11 | juice, particles, shake, bloom | 6 |
| High | 16 | juice, particles, shake, bloom | 6 |
| Ultra | 24 | juice, particles, shake, bloom | 6 |
| VR | 9 | juice, particles, bloom (shake off by VR comfort) | 6 |

Particle cap: the engine's, per tier (`max_particles` in the tier file), lowered by `fx.particles.max` (600 by default); the oldest particle is recycled at the cap.

## 6. Core additions this kit needs (small, additive)

1. `evani-core` input: the primary pointer's last position and kind stay readable after the pointer is gone (`Input::primary_pointer()`), so a quick tap whose down and up land in the same frame (touch, synthetic events) still has a position.

Recorded in DECISIONS.md when built. Bigger needs go to CORE_REQUESTS.md: a shared GPU particle system, and a sprite-sheet / custom-art path with larger atlas cells (the built-in shapes are 64 px, so pieces over about 96 px soften).

## 7. Demo (`apps/match-3`)

Title screen (Play, Levels, Settings) over the attract board; level intro card; HUD (level name, moves left, score with a three-star bar, goal chips with progress, pause button) as light DOM laid out from the kit's layout; combo callouts and floating scores as pooled DOM text; pause menu (resume, restart, settings, quit to title); settings (quality tier, effect toggles for what is compiled in, volume, key rebinding, stats); result screen (win with stars and score, or lose; next level, retry, title). Sounds are synthesised at start (no downloads): select, swap, invalid, pops pitched up per cascade, streak, bomb, prism, land, shuffle, star, win, lose.

## 8. Reference image prompt

```
In-game screenshot from an original Match-3 video game, board-ui framing: the whole board centred and flat-on, HUD panel beside it.
A glossy grid of candy-like jewels with a colour bomb exploding into cascades and sparkles, a cute mascot beside the board. HUD: moves left, goal icons, star bar.
Art style: Cartoon 2D. Bold outlines, flat cel colours, squash-and-stretch animation.
Crisp, polished 2D at native resolution: clean edges, readable silhouettes, rich colour and lighting where the style allows.
16:9 widescreen game capture, 2560×1440. Everything is original: characters, vehicles, creatures, logos and UI. No existing game franchises, characters, brands or real people, and no watermark. Any on-screen text is short and legible.
```
