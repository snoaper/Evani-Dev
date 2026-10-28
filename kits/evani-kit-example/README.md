# evani-kit-example

The smallest real kit, and the engine's test kit (ROADMAP row 000, "Test kits"). It proves the `KitPlugin` interface end to end, gives `bake.export` a kit crate to write into, and is what `apps/sandbox` runs. It is not a genre: there is no win or lose state.

- Spec: [kits/example/SPEC.md](../example/SPEC.md). Target pack: [kits/example/target/](../example/target/).
- Params (owner `kit.example`): `kit.example.spin_speed`, `kit.example.count`, `kit.example.boost`, and `kit.example.static_world` (the geometry check's still world: spinners held and hidden).
- Formulas: `kit.example.boost_scale(held, boost)` and the curve `kit.example.spin_falloff(rate)`.
- Input action: `boost` (Space or gamepad A).
- Component `Spinner`, system `example.spin` (fixed `Update` stage), scene `example.main`.
- Data file `example.level`.
- Modules `place` (the grid layout and the exact resting height of each turned spinner) and `draw` (the sandbox's scene drawing: spinners, middle distance, sprites and the orbit camera). They live in the kit so the geocheck's sandbox adapter and the `review_dump` test draw exactly what the game does.
- Tuned values: `bake.export` writes `params.json`, `curves/*.json` and `slots/*.expr` here; the look's tuning history is in `looks/evani-look-basic/CHANGES.md`.

## What to try

- Watch the camera orbit the grid of spinning bevelled cubes and ringed orbs. Each should rest on the ground with a shadow under it, and the glowing ones should bloom.
- Switch the tier to Low: the shadows become dark discs under the spinners and the glow loses its halo.
- Hold Space (or pad A). The spinners should turn faster while it is held.
- Untick bloom, then vignette, in the debug panel. The glow and then the dark corners should go.
