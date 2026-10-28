# Evani.Dev kit pack

Everything a coding agent needs to build the genre kits one by one after the foundation is finished.

## What's inside

- `CLAUDE.md`: standing instructions for the agent, including the commands `next kit`, `kit <id>`, `fix <id>: …` and `core requests`. Claude Code reads this automatically from the repo root.
- `ROADMAP.md`: all 127 kits in build order with status. Phase A is six very different kits that prove the engine. Phase B is everything else, most played first. Phase C is the massive online kits.
- `docs/KIT_PROCESS.md`: the nine steps and the "done when" checks every kit follows, plus the rules for choosing effects and the reference image prompt template.
- `docs/FOUNDATION_PROMPT.md`: the milestone 1 prompt, kept as the reference for what the engine promises.
- `CORE_REQUESTS.md`: where kits log engine changes they need but aren't allowed to make themselves.
- `catalog/`: the Atlas as data: `genres.json` (systems, cameras, physics, rank, image scene, compatible looks), `looks.json`, `effects.json`, `tiers.json`.

## How to use it

1. Finish milestone 1 with `docs/FOUNDATION_PROMPT.md` until all its checks pass.
2. Copy this pack into the repo root. If the repo already has a CLAUDE.md, append this one to it.
3. Start a Claude Code session with Opus and type `next kit`.
4. When it stops at `review`, play the demo. Then either set the row to `done`, or type `fix <id>: <what to change>`.
5. Start a fresh session and type `next kit` again.

Change the order or a kit's first look by editing `ROADMAP.md`.
