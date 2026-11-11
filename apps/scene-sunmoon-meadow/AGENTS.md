<!-- BEGIN:evani-folder-rules engine=0.1.0 sha=9abde6089c8371c5 (managed by `cargo xtask init-ai`; edits inside are replaced) -->
# Game `scene-sunmoon-meadow`

The engine rules and tools are in the AGENTS.md at the repository root; paths below are from there.

- `apps/scene-sunmoon-meadow/evani.toml` is this game's module list. Change an effect or module there and in this crate's `[features]` together, with the same names; `default` mirrors the list so `trunk serve` matches the real build.
- Gameplay belongs in the game's kit crate (`kit = ...` in `evani.toml`), not in `src/`; `src/` only assembles the `App`.
- Run it: `cargo xtask live --game scene-sunmoon-meadow`. Check it: `cargo xtask check --app scene-sunmoon-meadow` (add `--full` before hand-over). Ship it: `cargo xtask build scene-sunmoon-meadow --ship`.
- `[playground]` marks a game for the Evani.app library. A playground must support free roam (`free_roam = true`, the kit's declaration and its free-roam test); `check` and `build --ship` refuse it otherwise (docs/PLAYGROUNDS.md). A game's whole ship folder must also stay under 10 MB uncompressed; `build --ship` prints the total and fails over it. The 10 MB download cap applies only to games (and playgrounds, which are game examples); the library, Scenes, viewers and tools have no size cap.
- `[review] try` says what a person should try; `[review] moments` names the signature moments the kit can stage for captures (docs/KIT_PROCESS.md step 6).
<!-- END:evani-folder-rules -->
