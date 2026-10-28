<!-- BEGIN:evani-folder-rules engine=0.1.0 sha=636d51f41cb683fd (managed by `cargo xtask init-ai`; edits inside are replaced) -->
# Game `top-down-action-adventure`

The engine rules and tools are in the AGENTS.md at the repository root; paths below are from there.

- `apps/top-down-action-adventure/evani.toml` is this game's module list. Change an effect or module there and in this crate's `[features]` together, with the same names; `default` mirrors the list so `trunk serve` matches the real build.
- Gameplay belongs in the game's kit crate (`kit = ...` in `evani.toml`), not in `src/`; `src/` only assembles the `App`.
- Run it: `cargo xtask live --game top-down-action-adventure`. Check it: `cargo xtask check --app top-down-action-adventure` (add `--full` before hand-over). Ship it: `cargo xtask build top-down-action-adventure --ship`.
- `[playground]` marks a game for the Evani.app library. A playground must support free roam (`free_roam = true`, the kit's declaration and its free-roam test); `check` and `build --ship` refuse it otherwise (docs/PLAYGROUNDS.md). The whole ship folder must also stay under 10 MB uncompressed; `build --ship` prints the total and fails over it.
- `[review] try` says what a person should try; `[review] moments` names the signature moments the kit can stage for captures (docs/KIT_PROCESS.md step 6).
<!-- END:evani-folder-rules -->
