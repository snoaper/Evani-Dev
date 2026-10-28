# AI guidance that ships with the engine

Evani.Dev tells any AI coding agent (Claude Code, Codex, Cursor, Gemini, Copilot, anything that reads
AGENTS.md) which tools exist, in which order to use them, and the rules it can't work out itself.
It ships with the engine and is versioned with it. The source is `tools/evani-guide/guide/`; the
code is the `evani-guide` crate, which `cargo xtask` and the live MCP server both call.

## What gets written

`cargo xtask init-ai` (and `cargo xtask new-game`) writes:

| File | What it is |
|---|---|
| `AGENTS.md` | The engine block: the check gate, how to start a game, the catalog and module rules, the tools in order, what to do when something is off, the doc index. About 4.5 KB. |
| `CLAUDE.md`, `GEMINI.md` | A pointer that imports AGENTS.md (`@AGENTS.md`). No copied rules. |
| `.cursor/rules/evani.mdc` | An always-applied Cursor rule pointing at AGENTS.md. |
| `apps/<id>/AGENTS.md`, `kits/AGENTS.md`, `looks/AGENTS.md` (+ a `CLAUDE.md` pointer next to each) | Short local rules for that folder. |
| `.mcp.json` | The `evani-live` MCP server, as a prebuilt binary (below). Other servers in the file are kept. |
| `.claude/settings.json` | The Claude Code Stop hook (below). Other settings and hooks are kept. |
| `.github/copilot-instructions.md` | Only with `--with copilot`. |

Each engine-written part sits between markers:
`<!-- BEGIN:<id> engine=<version> sha=<body hash> (managed …) -->` … `<!-- END:<id> -->`.
Everything outside the markers is the project's own and is never touched. Writing twice changes
nothing. Project notes go below the block.

## Commands

```
cargo xtask init-ai [--with copilot] [--no-mcp-build]   write everything above; install the MCP server
cargo xtask doctor [--fix]                              missing, stale (older engine) or hand-edited blocks,
                                                        and whether the MCP server binary is current; --fix repairs
cargo xtask check [--app <id>].. [--full | --quick] [--strict] [--json]
cargo xtask new-game <id> [--look <look>]               apps/<id> + kits/evani-kit-<id> from the working example,
                                                        registered in the workspace, with its guidance
cargo xtask ai-eval --plan | --score <dir>              the eval harness (below)
```

The same commands run without xtask as `cargo run -p evani-guide -- <command>`; a future standalone
`evani` CLI calls `evani_guide::cli::run`.

## The check gate

`cargo xtask check` (MCP tool `evani_check`) checks every game in `apps/` and prints each issue as
`ERROR|WARN|note <game> · <area>: <what>`, then a `fix:` line and a `docs:` line. Exit 1 on any
ERROR (and on WARN with `--strict`).

- ERROR: `evani.toml` doesn't match the game crate's features; an optional engine crate that isn't
  `optional`; a look, effect or physics the catalog doesn't allow (effect not in
  `catalog/effects.json` or not an `evani-fx` feature, effect not for the look's style, look outside
  the genre's `compatible_looks`, physics other than the genre's); untagged geocheck issues; FAIL
  lines on the review card; a look guard failure; a playground (`[playground]` in `evani.toml`)
  without free roam: no `free_roam = true`, no `fn free_roam` in its kit, or no `#[test]` calling
  `evani_core::play::free_roam_test` (area `playground`, docs/PLAYGROUNDS.md; `build --ship`
  refuses it too); a kit that draws 3D (or a `3d`-style look) while no crate turns on evani-render's
  `forward` feature (area `render path`, README "Render").
- WARN: a playground whose last ship build is over the 10 MB package cap; no geocheck report; a 2D
  game that turns on `forward` but draws no 3D; `default` features that don't mirror `evani.toml`; a look guard that
  can't be approved on this machine; out-of-date guidance or MCP server.
- Freshness: a geocheck report or review card only counts when it is newer than everything the game
  is built from (its app, kit and look, `crates/`, `assets/`, `Cargo.lock`). An older one is a
  "stale, re-run" warning instead, so old evidence neither blocks nor passes a game.
- `--full` also runs the game's and kit's tests and `cargo xtask build <id> --ship`. Its cargo
  children run at below-normal priority on Windows with capped test threads (what it verifies is
  unchanged).
- `--quick` is the fast loop while you work (it cannot be combined with `--full` or `--json`). It
  prints the same static checks, then lists the workspace crates changed since the last commit
  (staged, unstaged and untracked files against `HEAD`, mapped to crates with `cargo metadata`)
  and every crate that depends on them, transitively, each with its reason (`touched` or
  `dependent (via <crate>)`). A root `Cargo.toml`, `Cargo.lock`, `clippy.toml` or `.cargo/` change
  selects every crate; files outside any crate (docs, catalog, assets) select none. It then runs
  `cargo fmt -p <touched> -- --check`, `cargo clippy -p <chosen> --all-targets -- -D warnings` and
  `cargo test -p <chosen>` in the normal target folder (cargo's incremental cache is reused), each
  step with its wall time. Any clippy warning or error fails the run (`-D warnings`; the
  workspace is clean, and it also denies `unwrap_used`, `expect_used` and unsafe code): fix the
  lint, or add a narrowly scoped `#[allow(...)]` with a one-line reason. With nothing changed it says so and runs only the static checks. It
  never writes formatting. The repository is not rustfmt-formatted, so fmt can only be advisory:
  touched files that differ are WARN lines and do not fail the run. Load caps: `CARGO_BUILD_JOBS`
  if set, else about half the cores, for build jobs and test threads (`RUST_TEST_THREADS`
  overrides the latter), and below-normal priority on Windows. Its last line says it is not a
  substitute for `--full`: run `--full` once before review or hand-over.

## The Claude Code Stop hook

`.claude/settings.json` runs `cargo run --quiet --package evani-guide --target-dir target/guide --
check --hook` when Claude is about to stop (exec form, 300 s timeout). It reads the session
transcript, finds the files the session's Edit/Write calls touched and maps them to games (`crates/`,
`catalog/`, `assets/` and `Cargo.*` count for every game). No game touched: it does nothing. Errors:
it exits 2 with the fixes, which Claude Code hands back to the model to keep working. After three
blocks in one session it lets the session stop, so it can't trap an agent. It builds in its own
target folder and doesn't go through xtask, so it stays quick. Other agents run `cargo xtask check`
themselves, as AGENTS.md tells them.

## The live MCP server

`.mcp.json` starts `.evani/bin/evani-live-bridge mcp`, a prebuilt copy that `init-ai` and
`doctor --fix` build into `target/mcp` and copy there, with a stamp of its source hash.

Why: `.mcp.json` used to run `cargo run -p evani-live-bridge`. The bridge's sources change almost
every day, so a session start usually rebuilt it (40 to 150 s at two build jobs) while Claude Code
waits about 30 s for an MCP server to start: the `evani-live` CONNECT_TIMEOUT. The prebuilt binary
answers at once. `check` and `doctor` warn when it is older than its sources.

What the server tells an agent (its instructions and tool descriptions, plus line 2 of the tools
list in AGENTS.md): choose values with one `sweep_run` or `compare_ab` and apply the result in one
`param_set` or `batch`. Those runs try their values off-screen: the game's view holds its last
frame (with a small "measuring…" tag) and only what the agent applies shows, so the person watching
never sees the tries flick past (docs/LIVE_API.md 6.30).

A request that asks for several live changes at once ("turn the lights down, turn the blue map,
reverse the rotation") is the AI's to split, and speed is the point: it never makes the changes
one after another in one agent. It splits the request into independent atomic edits, opens an edit
transaction with `edit_begin` (passing that list), starts one sub-agent per edit in parallel with
no fixed cap (3 edits, 3 agents; 20 edits, 20 agents), and each agent calls `edit_stage` for its
own edit with its own note; then `edit_commit`. Each edit answers `applied`, `failed` (with the
reason; the others carry on) or `conflict`. On a conflict (two edits change the same thing, such as
"darker" and "brighter" on one light) neither stays applied and the AI asks the user which one
wins, then calls `edit_resolve`; it never chooses itself. The default mode is `stream` (each edit
shows as soon as its agent stages it); `together` shows them all on one frame at commit. Either can
be chosen per transaction (`mode`), and the default can be changed with the `live.edit.mode`
parameter (docs/LIVE_API.md 6.31).

What a checkout needs: commit `.mcp.json`, run `cargo xtask doctor --fix` once per checkout (and after
pulling bridge changes when it warns), and approve `evani-live` once in Claude Code (`/mcp`) or
Evani.App. The command path is relative because Claude Code starts it from the project folder and
doesn't expand `${CLAUDE_PROJECT_DIR}` in `.mcp.json` commands.

## Eval harness (stub)

Does the guidance make a one-shot "make me a game" request come out right? `guide/evals.toml` holds
short prompts written the way a person asks (a kart racer, a 2D platformer, an effects trap that
asks for modules the engine doesn't have, tuning an existing game), each with what a good result
looks like.

- `cargo xtask ai-eval --plan [--agent "<cmd>"]` prints, per prompt, the commands to make a fresh
  worktree, run `init-ai` there, run the agent with the prompt, score it and clean up.
- `cargo xtask ai-eval --score <dir> [--full]` runs the check on a finished worktree and reports
  JSON: the games the agent added, errors, warnings, pass or fail.

It never starts an agent itself: agent runs cost money and time, so a person starts them. Score
several runs per prompt, compare with and without the guidance (an `init-ai`-less worktree), and
keep the block only as long as it earns its place: the prior-art finding is that long or generic
context files make agents worse.
