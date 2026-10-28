<!-- BEGIN:evani-folder-rules engine=0.1.0 sha=46902ad9337244dc (managed by `cargo xtask init-ai`; edits inside are replaced) -->
# Kits

The engine rules and tools are in the AGENTS.md at the repository root.

- A kit is the crate `kits/evani-kit-<id>` implementing `KitPlugin`; `kits/<id>/` holds its target pack (`target/`) and `SPEC.md`. Make the target pack before code (docs/KIT_PROCESS.md step 2).
- `name()` returns `<id>`; parameters are `kit.<id>.<name>` with `_` for `-`; levels, units and tuning are JSON data files (`data_file` with `include_str!`); `baked_params` loads what `bake_export` writes.
- Free roam (every playground has it): read the `PlayMode` resource; under `PlayMode::FreeRoam` it is the same world with no goals, timer, lives, fail state or game over. Report game over, timer expiry and lost lives on `PlayOutcomes` in every mode, return true from `fn free_roam`, and keep a test that calls `evani_core::play::free_roam_test` (docs/PLAYGROUNDS.md). A playground has no world-size rule, only the 10 MB package cap.
- Never depend on a look crate. Physics only as the genre's catalog entry says.
- The review reads conventions, not kit code: `playtest_win*` and `playtest_lose*` tests, a `review_dump` test, `capture.frame {moment}` for every moment in `target/moments.md`, and geocheck tags with a reason (docs/KIT_PROCESS.md "Kit cooperation", docs/GEOCHECK.md).
<!-- END:evani-folder-rules -->
