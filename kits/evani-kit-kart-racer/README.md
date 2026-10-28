# evani-kit-kart-racer

Kit 001 of Evani.Dev: an arcade kart racer. The demo game is **Sugarpeak Rally** (`apps/kart-racer`), one track (Sugarpeak Loop), three laps, eight karts, with the toon look (`looks/evani-look-toon`). The full design is in `kits/kart-racer/SPEC.md`.

## What it does

- Arcade kart physics at a fixed 60 Hz: speed, steering by speed, grass slow-down, hops, bumps between karts, walls.
- Drift-charge boosts: hold drift while steering; sparks go blue, orange, then pink; let go for a short, longer or long boost.
- Item boxes with odds weighted by race position: Zoom Pop, Goo Blob, Boing Bomb, Seeker Star, Shield Bubble.
- AI racers on a racing line with rubber-banding and their own item tactics.
- Laps, checkpoint gates, race positions and WRONG WAY.
- One or two players on one screen (split top and bottom), each with a chase camera, HUD and minimap.
- Synthesised sounds (no downloads).

## Plug it in

```rust
App::new("my-racer")
    .with_kit(evani_kit_kart_racer::KartRacerPlugin)
    .with_look(evani_look_toon::ToonLook) // any 3D look works
```

The kit asks the look for 16 material roles by name (road, kerbs, grass, mountains, tree, trunk, kart body, wheel, item box, boost flame, spark, goo, bomb). A look without a role falls back to its first material, tinted so the colour still reads, so the kit runs with `basic` too. There is no sand material (the track has no sand), and the driver shares the kart body role with a lighter tint.

## Tuning

Every gameplay number is a live parameter under `kit.kart_racer.*` (speeds, drift, items, AI, camera); `schema.get` lists them. A few worth knowing:

- `kit.kart_racer.karts` (8) sets the field size. AI racers = karts − players, so 7 AI in one-player and 6 in two-player.
- `kit.kart_racer.laps` (3) and `kit.kart_racer.checkpoints` (8).
- `kit.kart_racer.item_respawn_s`: 3 s in code, tuned to 1.5 s and baked into `params.json`.
- Formulas `kit.kart_racer.steer_by_speed` (a curve) and `kit.kart_racer.rubber_band` (an expression).

Values tuned in a live session and written by `bake.export` live in `params.json` here and in the look crate, and load in every build, with or without live.

## Controls

| | Player 1 | Player 2 |
|---|---|---|
| Steer | A / D | Left / Right |
| Throttle | W | Up |
| Brake, reverse | S | Down |
| Drift | Left Shift | Right Shift |
| Use item | E | Enter |

Gamepads work too (pad 1 for player 1, pad 2 for player 2). Esc, P or Start pauses. Every key can be changed in Settings.

## What to try

- Start a one-player race, hold W, and drift through the first bend (steer and hold Left Shift) until the sparks turn orange, then let go for a boost.
- Drive through a row of floating item boxes and press E to use what you get; from the back of the pack you get stronger items.
- Pause with Esc, open Settings, switch effects off and on (outlines, speed lines, shake, bloom) and change a key.
- Go back to the title and start a two-player race: player 2 drives with the arrow keys, Right Shift and Enter.
- Finish three laps: first to third gets the podium, fourth or lower loses.

## Effects

The game lists `outline`, `speedlines`, `shake` and `bloom` in its `evani.toml`. The tier budget keeps all four on Medium and up (8 of the points), drops bloom on Low, and on VR keeps outline and bloom (speed lines and shake are off for comfort). On tiers with fewer than 120 000 triangles per frame (Low) the kit draws every other tree.
