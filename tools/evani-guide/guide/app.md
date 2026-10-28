# Game `{{app}}`

The engine rules and tools are in the AGENTS.md at the repository root; paths below are from there.

- `apps/{{app}}/evani.toml` is this game's module list. Change an effect or module there and in this crate's `[features]` together, with the same names; `default` mirrors the list so `trunk serve` matches the real build.
- Gameplay belongs in the game's kit crate (`kit = ...` in `evani.toml`), not in `src/`; `src/` only assembles the `App`.
- Run it: `cargo xtask live --game {{app}}`. Check it: `cargo xtask check --app {{app}}` (add `--full` before hand-over). Ship it: `cargo xtask build {{app}} --ship`.
- `[playground]` marks a game for the Evani.app library. A playground must support free roam (`free_roam = true`, the kit's declaration and its free-roam test); `check` and `build --ship` refuse it otherwise (docs/PLAYGROUNDS.md). The whole ship folder must also stay under 10 MB uncompressed; `build --ship` prints the total and fails over it.
- `[review] try` says what a person should try; `[review] moments` names the signature moments the kit can stage for captures (docs/KIT_PROCESS.md step 6).
