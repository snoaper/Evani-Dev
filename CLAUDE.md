<!-- BEGIN:evani-pointer engine=0.1.0 sha=5c4614a91a8193b9 (managed by `cargo xtask init-ai`; edits inside are replaced) -->
@AGENTS.md

The Evani.Dev rules for AI agents are in AGENTS.md (imported above); they ship with the engine. Project notes go below this block.
<!-- END:evani-pointer -->

# Evani.Dev: instructions for the coding agent

Evani.Dev is a light, low-bandwidth, high-capability browser game engine in Rust on WebGPU. Every game keeps only the modules it needs. The foundation (milestone 1) is described in `docs/FOUNDATION_PROMPT.md` and `docs/SPEC-M1.md`, and its live API in `docs/LIVE_API.md`. Read those, plus README.md and DECISIONS.md once they exist (planned, M1 in progress), before changing anything.

## Stages come first

- Read `STAGES.md` before any work.
- Only work inside the current stage. The next stage may be researched and planned (documents, or throwaway spikes on a scratch branch), never built into main.
- A stage opens only after the previous stage's gate is in the `STAGES.md` gate log with the maintainer's sign-off.
- Requests that belong to a later stage are filed in that stage, or in the Parking lot, and not built.
- A regression reopens the earlier gate it breaks; fix it before continuing.

## Commands I will give you

- **`gate status`**: report the current stage from `STAGES.md` and each of its gate checks as passed, failed or not yet run, each with its evidence (command output, file, screenshot or test name). Never tick a gate box without evidence.
- **`next kit`**: only if the current stage in `STAGES.md` allows kit work (otherwise say which stage does, and stop). Take the first row with status `todo` in `ROADMAP.md` and build it by following `docs/KIT_PROCESS.md` from step 1 to step 9. Then stop.
- **`kit <id>`**: build that specific kit the same way, whatever its position in the roadmap.
- **`fix <id>: <what>`**: change an existing kit after my review, re-run its Done-when checks, and keep its status at `review`.
- **`core requests`**: work through `CORE_REQUESTS.md`, keeping every existing kit building and passing, then unblock the affected rows.

## Always

- The engine rules (data lives in `catalog/` and is never contradicted, all content is original, build-time on/off is sacred, `cargo xtask check` clean before hand-over) are in the engine block in AGENTS.md, imported above. They moved there so every Evani.Dev user gets them; they still apply here in full. If the catalog is wrong, say so in the hand-over.
- One kit per session. Stop after the hand-over; never start the next kit on your own.
- Commit after each step of the kit process.
