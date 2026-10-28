# 03 Class templates

M0 clean-room note, 2026-10-15. Own words; no third-party code opened or copied.

## What it must do (brief, design instruction 3)
Default parts, proportions, relationships and gameplay anchors for: kart, stylized character body (no face detail), building, tree, rock, fence, sign, crate, barrel, lamp post, bench, door, chest, sword, shield, lollipop, gumdrop, candy cane. Approved models can extend their class.

## Known approaches
- **Category-free generation** (TRELLIS.2; FullPart arXiv 2510.26140 with its 40K-object, 320K-part dataset; OmniPart arXiv 2507.06165) learns part structure from data. Powerful for organic shapes, but the parts carry no names or gameplay meaning, and the layouts are boxes predicted by a network.
- **Procedural asset libraries driven by language** (3D-GPT, arXiv 2310.12945) rely on hand-written generators per category with many raw parameters exposed to the language model.
- **Graph decomposition per request** (ShapeCraft, arXiv 2510.17603) builds the part graph from scratch each time from the prompt.

## Our approach
A class template is a data file (`crates/evani-model/data/classes/<class>.json`) with:
- **Parts**: name, generator (note 04), default parameters as ranges relative to the class's reference size, whether optional, and a role (`body`, `wheel`, `trim`, `detail`, `grip`...). Roles drive LOD dropping (note 14), collision grouping (note 15) and UV seam placement (note 16).
- **Relationships** between named anchors (note 06): `rests_on`, `flush_with`, `attached_at`, `centred_on`, `aligned`, `mirrored`, `array_along`.
- **Gameplay anchors** exported to the game: kart `driver_seat`, `wheel_hub.{fl,fr,rl,rr}`, `exhaust.{l,r}`, `steer_pivot`, `item_slot`; sword `grip`, `guard`, `tip`; door `hinge`, `handle`; chest `lid_hinge`; lamp post `light`; bench `seat_line`; character `head`, `hand.{l,r}`, `foot.{l,r}`, `hip` (no face detail).
- **Default must tests** (always on, cheap): stands on the ground plane, one connected solid, symmetric where the class is (kart, bench, chest), wheels touching the ground, tri budgets per tier.
- **Typical views**: the cameras the game sees it from (kart: chase camera behind and above, side on the podium). These weight silhouette scores and LOD protection.
- **Measures**: the named, checkable quantities adjectives act on (ride height, wheelbase, track width, tyre radius ratio, body bulk, nose bluntness...).

**Extension by approval.** When Chris approves a model, its parameter vector joins the class's prior for its style (note 09), and a new named part or arrangement it introduced (for example a truck cab on a kart chassis) can be saved as a template variant (`kart/truck_cab`) that later intents can name with `like`.

**The 18 classes split into five template families** so the work scales: vehicles (kart), bodies (character), structures (building, fence, door, sign, lamp post, bench, crate, barrel, chest), nature and candy (tree, rock, lollipop, gumdrop, candy cane), and weapons (sword, shield). Families share part kinds and measures.

## Why it should be better
- Knowledge fills the gaps (design principle 6): the agent never has to remember that a kart needs four wheel hubs and a seat; it only says what is different.
- Named parts and anchors are what the game needs (kit anchors, collision per part), which learned generators do not provide.
- Templates make relationships and must tests checkable from the first call, so the search starts inside the valid space instead of discovering it.

## Baseline and measure
- Ablation: intents with the template disabled (the agent must list every part); measure tokens, calls and must-test pass rate.
- Coverage: each of the 18 classes compiles from a two-line intent (`make` and `style`) and passes its default must tests.

## Risks
- **Template rigidity**: everything looks like the template. Mitigation: wide default ranges, the `like` field, variants, and `add` / `remove` of parts in intents.
- **Authoring effort** for 18 templates. Mitigation: five families share most parts; templates are data and a lighter model can write them from this note (brief: lighter models for templates).

## Sources (ideas only)
FullPart arXiv 2510.26140; OmniPart arXiv 2507.06165; 3D-GPT arXiv 2310.12945; ShapeCraft arXiv 2510.17603; the current kart model in kits/evani-kit-kart-racer (draw_kart: tub, nose fairing, side pods, bumper, engine block, exhausts, wing on struts, seat, steering column and wheel, four wheels with rims and hub bars).
