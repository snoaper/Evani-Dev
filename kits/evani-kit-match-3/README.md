# evani-kit-match-3

Kit 003 of Evani.Dev: a match-3 puzzle. The demo game is **Puff's Sweet Grid** (`apps/match-3`): three short levels, a mascot (Puff), the cartoon2d look (`looks/evani-look-cartoon2d`). The full design is in `kits/match-3/SPEC.md`.

It proves a game with **no physics module**, a **board and UI** game, **touch and mouse** on one code path, and **juice**.

## What it does

- Swap validation: drag a piece onto a neighbour, or tap two neighbours (mouse, pen or touch); arrows and Space (or a gamepad's d-pad and A) work too. A swap with no match slides and bounces back and costs nothing.
- Cascades: matches pop, pieces fall through the board (and past holes), new ones drop in, and every new line that forms on its own is a cascade with a higher score and a callout.
- Special pieces: four in a line makes a **Streak** (clears its row or column), an L, T or + makes a **Pop Bomb** (clears the 3×3 around it), five in a line makes a **Prism** (clears one colour). Swapping two specials together makes a bigger blast; a Prism swapped with a special turns that colour into the special.
- Level goals and move limits: collect a number of pieces of a colour, melt all the frosting, or reach a score, within the moves. Win with 1–3 stars; the left-over moves fire as **Sugar Rush**. Run out and you lose.
- Boards start with no matches and always have a move; a hint wobbles the best move after 5 s; a board with no moves reshuffles.
- Everything runs on the fixed 60 Hz tick with seeded randomness, so a level plays the same for the same moves.
- Synthesised sounds (no downloads), pitched up per cascade.

## Plug it in

```rust
App::new("my-puzzle")
    .with_kit(evani_kit_match_3::Match3Plugin)
    .with_look(evani_look_cartoon2d::Cartoon2dLook) // any look works
```

The kit draws everything as screen-space atlas sprites. It takes its colours from the look's `pieces` (six colours) and `ui` (ink, board light, board dark, frame, background glow, background dots, mascot, frosting) palettes and its style from the look's params `look.<look>.outline_px`, `shade`, `highlight`, `squash` and `shadow`. A look without them (such as `basic`) gets flat pieces with no outline. The app's UI reads `hud::Match3Hud` and sends `game::Request`s; the kit never touches the DOM.

## Entities and free roam

Pieces, clearing pieces, blasts and the mascot are ECS entities (`ecs.rs`: `Piece`, `Cell`, `Motion`, `Popping`, `Blast`, `Puff`). The `Grid` resource is the rules view rebuilt from them each tick; `Game` holds session and score only. Score floats are a fixed pool; sparkles and confetti are emitted into the engine's shared GPU particle system (`sparks.rs`), so the kit keeps no particle pool. Bots and tests move pieces with `Request::Swap(a, b)`. Under `PlayMode::FreeRoam` there are no goals, move limit, Rush, win or lose.

## Tuning

Every gameplay and feel number is a live parameter under `kit.match_3.*` (`schema.get` lists them): swipe distance, swap, pop, fall and squash timings, chain delay, hint delay, shake trauma per special, cell size limits. Also `kit.match_3.bomb_radius` and the score formula `kit.match_3.match_score(pieces, cascade, special)`. Levels are data in `data/levels.json` (board shape with holes and frosting, colours, moves, seed, goals, star scores).

Values tuned in a live session and written by `bake.export` live in `params.json` here and in the look crate and load in every build, with or without live.

## Controls

| | Mouse / touch | Keys | Gamepad |
|---|---|---|---|
| Swap | drag a piece onto a neighbour, or tap two neighbours | arrows move the cursor, Space or Enter picks, then an arrow swaps | d-pad, A |
| Drop the picked piece | tap it again | Backspace | B |
| Pause | the II button | Esc or P | Start |

Every key can be changed in Settings.

## What to try

- Press Play, then drag a piece onto a neighbour (or tap two neighbours) to make a line of three; on a phone, swipe.
- Line up four for a Streak, an L or T for a Pop Bomb, five for a Prism; then swap two specials together.
- Watch a cascade: pieces fall, new lines pop on their own and the callouts climb (Nice!, Great!, Superb!).
- Collect the goal pieces before the moves run out: win and the left-over moves fire as Sugar Rush; run out and you lose.
- Pause with Esc or the pause button, open Settings, switch Sparkles or Bouncy animation off and on, and change a key.

## Effects

The game lists `juice`, `particles`, `shake` and `bloom` in its `evani.toml`; it has no physics module. The tier budget keeps juice, particles and shake on every tier (bloom from Medium up, as the tier presets say); VR drops shake for comfort.
