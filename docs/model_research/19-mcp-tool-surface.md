# 19 MCP tool surface

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, MCP tools and design principles 3, 4, 9)
Compact JSON, never raw vertex data: `model.describe_system`, `model.make`, `model.pick`, `model.edit`, `model.fit`, `model.check`, `model.describe`, `model.sheet` (blueprint, raking light, variants, curvature, thickness), `model.compile`, `model.calibrate_style`, `model.save_to_library`. Agent-agnostic. One round trip per decision; words and scores first, pictures only on request as one combined image. Target: a finished, tested model in 2 or 3 calls.

## Known approaches
- **Tool servers that run code in a modeling application** (for example a Blender MCP server that executes Python and returns screenshots): unlimited power, but the agent writes every coordinate, errors arrive late, and screenshots cost many tokens per round.
- **Multi-agent pipelines** (LL3M, CADSmith, aDSL) exchange code and renders between agents for several rounds of minutes each.
- **This workspace's evani-live tools** (docs/LIVE_API.md): compact JSON replies, one `sweep_run` instead of many single changes, notes on every change, capture only when asked. Their MCP names use underscores (`live_status`, `sweep_run`, `debug_geometry`) while the protocol names in the docs use dots (`debug.geometry`).

## Our approach
- **Flag: tool names.** The Claude API accepts only letters, digits, `_` and `-` in tool names, and other clients have similar limits, so a dot in an MCP tool name breaks some agents. Following the evani-live precedent, the MCP tools are registered as `model_describe_system`, `model_make`, `model_pick`, `model_edit`, `model_fit`, `model_check`, `model_describe`, `model_sheet`, `model_compile`, `model_calibrate_style`, `model_save_to_library`; docs and the CLI keep the brief's dotted names (`model.make`). Decision D10 in plan-decisions.md.
- **Two-call happy path.** `model_make` (intent in, top K scorecards out, all must tests already run) then `model_compile` (chosen id in, per-tier files, sizes, hashes and checks out). `model_pick` is folded into compile when the agent passes the id; an edit is a third call only when needed.
- **Compact replies.** Numbers rounded to what matters (mm for sizes, 3 decimals for scores), stable short ids (`k3f2`), parts and measures by name, deltas in steps, only failing or borderline checks listed in full (passing ones counted). Typical make reply for K = 3: under 400 tokens. Errors carry a `fix` line, like `cargo xtask check`.
- **Pictures only on request.** `model_sheet` returns one file path to one combined image (several views or variants on one sheet) and a one-line caption; never inline image data unless the client asks for it.
- **No vertex data, ever.** Geometry stays in the tool; replies carry measures, hashes and paths.
- **Time budgets in arguments.** `budget_s` on make and edit (defaults 3 and 1), `tier` override on every call. The budget only chooses the `auto` level before the call runs (note 18); it never stops a search on the clock, and the reply names the level used so the result can be reproduced. Compile is synchronous up to its tier target; a `wait: false` option returns a job id for very large models, polled by `model_check`.
- **State.** Models, versions and caches live in a project-local work folder (`.evani/model/`), so calls are stateless apart from ids, and two agents can work on different models at once.
- **Transport.** A stdio MCP server inside tools/evani-model, plus the same commands on its command line for scripts and benchmarks. Implementation choice (hand-written over serde_json, or the rmcp crate already pinned for evani-live-bridge) is decision D2.

## Why it should be better
- The agent's work per model is one intent and one compile, a few hundred tokens each way, instead of many rounds of code and screenshots.
- Errors are cheap and specific (vocabulary checks and fix lines), so retries do not cost a round of modeling.
- The same tools serve any MCP-capable agent, so creators are not tied to one AI.

## Baseline and measure
- Test D: tokens, calls, wall time and GPU time per finished model for every benchmark intent, against baseline F (at least 5x fewer tokens and calls at equal or better quality).
- Reply size per tool (tokens), measured on the benchmark run.
- Three recorded example sessions with calls, time and tokens (brief: what to expect, item 5).

## Risks
- **Client timeouts** on long compiles (some clients stop waiting after about 60 s). Mitigation: `wait: false` and the tier targets.
- **Over-compact replies** that force a follow-up `model_describe`. Mitigation: measure follow-up rates in the benchmark and move frequently requested fields into the default reply.

## Sources (ideas only)
docs/LIVE_API.md (evani-live tool conventions); LL3M arXiv 2508.08228; CADSmith arXiv 2603.26512; aDSL arXiv 2608.17975; Model Context Protocol specification (tools); Anthropic API tool-name rules.
