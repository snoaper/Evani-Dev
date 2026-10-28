<!-- BEGIN:evani-folder-rules engine=0.1.0 sha=848217d1f219d72c (managed by `cargo xtask init-ai`; edits inside are replaced) -->
# Looks

The engine rules and tools are in the AGENTS.md at the repository root.

- A look is the crate `looks/evani-look-<id>` implementing `LookPlugin` (start from `evani-look-basic`); it registers palette, materials, shader slots, post effects and parameter overrides, owner `look.<id>`. It must match its `catalog/looks.json` entry.
- `guard/` is the look guard set (moments, limits, baseline captures). Changing any of it needs the project owner's approval in Evani.App; never edit it to make a check pass. Propose changes with `look_limits_propose` or `look_baseline_propose` (docs/LIVE_API.md 15).
- `params.json`, the `.wgsl` slot files and `CHANGES.md` are written by `bake_export`; keep them in the commit.
<!-- END:evani-folder-rules -->
