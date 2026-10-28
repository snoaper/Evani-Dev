# 01 Intent document and schema (.evmodel)

M0 clean-room note, 2026-10-15. Written in our own words from the brief, the papers listed below and our own design work. No third-party code was opened or copied.

## What it must do (brief, design instruction 1)
A readable text file that states what to make, not how: `make, like, style, parts, relief, must, match, explore, return, seed`. Its schema is published in docs/MODEL_API.md. It is the single input to `model.make` and the thing saved with every approved model as provenance.

## Known approaches
- **Code as the model description.** LL3M (arXiv 2508.08228) and 3D-GPT (arXiv 2310.12945) have the language model write Blender Python, or fill the parameters of Blender procedural generators. The code is readable, but every coordinate is chosen by the language model, errors show up only when the script runs, and LL3M needs a six-agent loop (plan, retrieve, code, critique, verify, user) to converge. LL3M reports that about 59% of user edits landed with one instruction; the rest needed three or four follow-ups.
- **A CAD script.** CADSmith (arXiv 2603.26512) writes CadQuery and corrects it in two loops (execution errors, then geometric validation). Exact, but the agent still places every number.
- **A graph of procedural tasks.** ShapeCraft (arXiv 2510.17603) parses the request into a graph of parts ("graph-based procedural shape") and refines each node over several attempts.
- **A language designed for the agent.** aDSL (arXiv 2608.17975) co-designs the modeling language with the agent: relational operators (align to centre or anchor, place along an axis with a gap, stack, distribute, grid, radial) instead of absolute coordinates, with the same relations later checked by the critic. Their ablation: removing the spatial helpers raised the rounds needed from 4.25 to 4.67; each round takes about 190 s.

## Our approach
- **Declarative intent, not a program.** The file has no statements, loops or coordinates by default. Each field is a fact or a wish: the class (`make`), what it resembles (`like`), its style, adjectives per part, relief layers, hard tests (`must`), references (`match`), search effort (`explore`), how many results (`return`) and the seed.
- **Our own small text format.** Indentation-based like the brief's example, but with three things a generic format lacks: quantities with units (`4mm`, `30 deg`, `[1.3, 0.8, 0.7] m`), bare adjectives inside inline maps (`{ stance: low, wide }`) and call forms for patterns and tests (`tread(chevron, depth 4mm, pitch 2cm)`). We parse it ourselves (no YAML dependency). Every file has an exact canonical JSON form, which is what the MCP tools accept too, so an agent can send either.
- **A closed, versioned vocabulary.** Every word the file may use (class names, part names per class, adjectives per style, pattern names, must kinds, units) comes from data files the parser knows. An unknown word is an error with the three nearest known words and their meanings, so a wrong word costs one short answer instead of a failed build. This is how we take the gain LL3M gets from retrieval (26% fewer errors) and push it further: most errors become impossible to write.
- **Defaults from the class template.** An intent with only `make: kart` and `style: toon_chunky` is valid: the template fills every part, proportion, relationship and anchor. The agent writes only what differs.
- **Numbers only where exact sizes matter.** Adjectives are the norm; a number with a unit pins one measure (`fit_box`, `length: 0.6 m`) and turns that measure into a constraint rather than a search dimension.
- **Seed and determinism.** A missing seed is derived from the canonical form's content hash, so the same text always gives the same result. The resolved search effort is written back into the scorecard and recipe (see note 18).
- **Edits are intent diffs.** `model.edit` changes the intent (an adjective, a number, a removed part), so the version history is a list of small, readable diffs. The intent plus the chosen candidate's parameter vector is the recipe.

## Why it should be better
- The agent writes concepts (what it is good at); the solver, search and compiler place every number (what it is bad at). A typical kart intent is about 20 lines and 150 tokens; the same kart as a Blender script is hundreds of lines of coordinates.
- Because the vocabulary is closed and checked before anything runs, an invalid intent fails in milliseconds with a fix line, not after a build.
- Relationships and must tests are part of the intent, so the checker verifies exactly what the author asked for (the aDSL idea of a shared plan/check language, but enforced by maths, not by a critic reading code).

## Baseline and measure
- Baseline F (note: docs/model_research/plan-baselines.md): the same agent writing Blender Python or raw SDF for 10 intents.
- Measures: tokens written per finished model, parse errors per session, share of intents that validate first time, calls to a finished model (target: median 2, 90% within 3).

## Risks
- **Too narrow a vocabulary** blocks real requests. Mitigation: `parts.<name>.add` with an explicit generator, and an escape hatch for a number on any parameter; words that agents reach for but we lack are logged and become candidates for the vocabulary (aDSL's co-design idea, done from logs).
- **Format drift** between the text and JSON forms. Mitigation: one schema, round-trip tests (text → JSON → text is stable).
- **Ambiguous adjectives** across styles ("chunky" means different ranges in different looks). Mitigation: adjectives are always resolved through the style profile (note 02), never globally.

## Sources (ideas only)
LL3M arXiv 2508.08228; 3D-GPT arXiv 2310.12945; CADSmith arXiv 2603.26512; ShapeCraft arXiv 2510.17603; aDSL arXiv 2608.17975.
