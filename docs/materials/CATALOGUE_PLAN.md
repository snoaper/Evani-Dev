# Evani.Materials: catalogue plan

Status: plan only (2026-10-23). Nothing here is built. The full list is `docs/materials/catalogue.csv`, one row per material.

Total materials: **1,054** (994 core in 9 categories, plus 60 in two optional categories).

The goal is a material library at the scale of a big open-world game. A large western open world was the yardstick for *how many*, never for *what*: every name here is generic and every material is original. The list is built for any Evani game, not only westerns, so two optional categories (vehicles, fantasy / sci-fi) are sized at the end.

## 1. How the library is built

Three layers, from small to large:

1. **29 base recipes** (section 3). A recipe is one shading model with a handful of numbers: metal, wood, fur, skin, cloth, stone and so on. Each of the 24 art styles learns each recipe once (how a toon, watercolour or voxel style draws "metal"), and every material on that recipe follows.
2. **About 1,000 base materials** (the CSV). A material is a recipe plus its numbers: gold, silver and copper are all `metal` with different colour and roughness; oak and pine are both `wood` with different grain colour and scale.
3. **8 condition layers** (section 4): wet, mud, dust, snow, wear, oxide, growth, blood. They go over any material, with an amount, so "muddy oak" or "wet wool" are never separate entries.

So the art work grows with *recipes x styles* (29 x 24), not with *materials x styles* (1,000 x 24).

### What exists in the engine today

Read from the material families on the `material-viewer-styles`, `layered-materials`, `char-materials`, `nature-materials` and `playground-materials` branches (crates `evani-material-layers`, `evani-material-character`, `evani-material-nature`, `evani-material-proc` with its `relief` feature) and `kits/evani-kit-material-view/data/library.json`:

| Family (crate) | Materials in the viewer today | Recipes it gives |
|---|---|---|
| Layered finishes (`evani-material-layers`) | clear-coat paint, soap film, brushed metal, velvet, woven cloth | `paint`, `metal` (anisotropy), `cloth` (sheen, weave) |
| Character (`evani-material-character`) | soft skin, wet eyes, hair | `skin`, `eye`, `hair` |
| Nature (`evani-material-nature`) | pool water, murky pond, window glass, bottle glass, sunlit leaves and grass, lamp glow, thin back-light (wax, paper, lampshade) | `water`, `glass`, `foliage`, `thin`, `glow` |
| Playground surfaces (`evani-material-proc`, `relief`) | cobbles, brick wall, glazed tiles, plank deck, rain and puddles, moss | `masonry`, `ceramic`, `wood`; the wet and growth layers |
| Procedural world (`evani-material-proc`) | plaster, sandstone, concrete, wood crate, painted metal, grime and edge wear, baked AO | `plaster`, `stone` (partly), `paint`; the wear and dust layers |
| Art styles (`styles.json`) | 4-6 draft starter materials per style (red plastic, gold coin, ...) | style presets, not base materials |

29 rows in the CSV are marked `exists_today` (`yes:<family>`). The style starter materials are not counted: they are per-style presets that will be re-pointed at catalogue materials.

## 2. Categories

Columns of the CSV: `id, name, category, subcategory, recipe, search_words, look_note, batch, exists_today, conditions`. `conditions` lists the condition layers that make sense for that material (the viewer only offers those).

| Category | Count | Subcategories (count) | Change from the starting split, and why |
|---|---|---|---|
| `animals` | 283 | horse coats 40, horse parts 10, livestock 30, dogs and cats 22, wild mammals 54, hides 9, animal parts 16, birds 41, reptiles and amphibians 21, fish and sea 25, insects 15 | +33. Horses alone need about 40 coats to read as a herd, and birds, fish and reptiles each need their own recipe tests |
| `people` | 94 | skin tones 30, skin traits 19, hair 20, beards and body 8, eyes 11, teeth and nails 6 | +4. 30 tones (10 depths x 3 undertones) plus traits |
| `clothing` | 104 | plain weaves 16, twills 9, wool and felt 7, knits 6, sheen fabrics 16, synthetics 8, leather 13, fur trims 9, fittings 10, beads and misc 10 | -21. Colour is a number, not a new material: "red wool" and "blue wool" are one fabric with a tint, so only distinct weaves are listed |
| `weapons_tools` | 62 | metals 28, wood handles 6, grips 6, ammunition 4, blades and tools 11, armour 7 | -3. The metals here are shared by every category |
| `buildings` | 102 | brick 10, stonework 12, plaster and concrete 14, wood 17, roofing 8, metalwork 10, tiles 8, interior 12, glass 11 | -28. Condition layers (wear, growth, dust) cover what would otherwise be "old" and "ruined" copies |
| `land` | 90 | soil 16, sand 6, gravel 7, rock 27, grass ground 13, snow and ice ground 9, special ground 12 | -20. Wet, snow and mud layers replace wet and frozen versions |
| `plants` | 98 | leaves 15, needles 6, bark 14, timber 16, flowers 14, crops 10, desert plants 7, fungi and mosses 9, plant parts 7 | -12. Cut timber species live here, once, for every category |
| `household` | 120 | pottery 12, glassware 10, kitchen metal 10, furniture finishes 12, upholstery 6, paper and books 11, wax and soap 5, toys and plastic 10, food 28, misc props 12, jewellery 4 | -30. Furniture reuses timber and cloth instead of listing them again |
| `elements` | 41 | water 17, ice 6, fire 11, glow 7 | +1 |
| **Core total** | **994** | | |
| `vehicles` (optional) | 30 | paint 10, metal trim 8, rubber and glass 12 | section 11 |
| `fantasy_scifi` (optional) | 30 | crystals 12, magic 8, sci-fi 10 | section 11 |

A material sits in exactly one category, where people would first look for it. Others reach it through search words (copper is in weapons and tools, but "pipe" and "pot" find it).

## 3. Base recipes (29)

"Numbers" are what change between materials of the same recipe. "Extras" are the parts a person can switch on and off (section 5). "Light version" is what the recipe falls back to on low tiers and phones. "Today": the crate that already has it, or *new*.

| Recipe | Covers | Numbers | Extras (switchable) | Light version | Today |
|---|---|---|---|---|---|
| `metal` | steel, iron, brass, copper, gold, silver, chrome, lead | colour, roughness, anisotropy, grain scale, edge polish | brushed grain, engraving relief | plain metal-rough, no grain | partly: `evani-material-layers` brushed; painted metal in `evani-material-proc` |
| `paint` | painted surfaces, lacquer, clear-coat car paint, varnish, enamel, thin films | base colour, coat strength, coat roughness, flake amount, film strength | clear coat, metal flake, thin-film colour shift | one gloss lobe, flakes off | yes: `evani-material-layers` clear coat and iridescence |
| `plastic` | plastic, rubber, vinyl, early plastics, foam | colour, roughness, translucency, specks | soft translucency | plain lit colour | new (small; the plain model material covers most of it) |
| `wood` | cut timber, planks, furniture, handles | grain colour (2), grain scale, ring warp, knot amount, roughness | grain relief, plank gaps (parallax) | flat two-tone grain | partly: `evani-material-proc` wood crate, `relief` planks |
| `bark` | tree bark, logs with bark, roots | colour (2), furrow depth, furrow scale, plate size | parallax depth | flat furrow stripes | new |
| `stone` | natural rock, marble, cliffs, ore | colour (2), vein amount, grain scale, layering, roughness | parallax depth, mica glints, edge translucency (marble) | flat noise, no glints | partly: sandstone in `evani-material-proc` |
| `masonry` | brick, cobble, setts, ashlar, block walls | unit colour, mortar colour, unit size, joint width, unevenness, pattern | parallax depth, per-unit colour variation | flat pattern, no depth | yes: `evani-material-proc` `relief` cobble and brick |
| `plaster` | plaster, render, stucco, concrete, limewash, painted walls | colour, roughness, trowel marks, crack amount | cracks, trowel relief | flat colour with noise | yes: `evani-material-proc` plaster and concrete |
| `ground` | soil, sand, gravel, mud flats, roads | colour (2), stone size, stone amount, ripple, roughness | parallax pebbles, sand sparkle | flat noise | new |
| `cloth` | woven, knit and pile fabrics | colour (2), weave pattern, weave scale, sheen, fuzz | sheen, weave relief, fuzz rim | flat colour with a sheen rim | yes: `evani-material-layers` sheen and weave |
| `leather` | tanned leather, suede, rawhide, tooled | colour, roughness, grain scale, crease amount, nap | crease relief, suede nap | flat with soft sheen | new |
| `fur` | animal coats, fur trims | base colour, tip colour, pattern (stripes, spots, patches), length, density, sheen | fur shells or strands, pattern | flat painted coat with a sheen rim | new (pattern maths can follow `evani-material-character` hair) |
| `feather` | bird plumage, quills | colour (2), barring, iridescence, feather size | iridescence, feather edge relief | flat barred colour | new |
| `scales` | snakes, lizards, fish, crocodilians | colour (2), scale size, scale shape, pattern, iridescence | scale relief, fish shimmer | flat pattern | new |
| `hide` | bare animal skin: pig, elephant, frog, octopus | colour, wrinkle depth, wetness, thin scatter | wrinkle relief, surface scatter | lit colour with wrinkles painted | new (a cut-down `skin`) |
| `skin` | human skin, tones and traits | tone (depth), undertone, pore scale, wrinkle amount, freckles, redness | subsurface glow, pores, wrinkles | wrap lighting, no scatter | yes: `evani-material-character` skin |
| `hair` | human hair, beards, manes | colour, root darkening, shift, highlight strength (2) | second highlight, strand edges | one highlight | yes: `evani-material-character` hair |
| `eye` | human and animal eyes | iris colour (2), pupil size, iris depth, wetness | wet cornea lobe, iris depth | painted iris with a catch-light | yes: `evani-material-character` eye |
| `keratin` | horn, bone, antler, teeth, nails, shell, beetle shells | colour (2), translucency, growth rings, gloss | edge translucency, nacre shimmer | flat glossy colour | new |
| `foliage` | leaves, needles, grass, crops, straw | colour (2), back-light colour, variation, thickness | back-light glow, wind sway | flat colour, no back-light | yes: `evani-material-nature` foliage and grass |
| `thin` | paper, wax, petals, fruit skin, wings, membranes, lampshades | colour, thickness, light-through colour, pattern | light through | flat colour | yes: `evani-material-nature` thin back-light |
| `organic` | food, mushrooms, flesh, slime | colour (2), wetness, scatter, crumb | surface scatter | flat colour | new |
| `ceramic` | pottery, glaze, tiles, enamel, roof tiles | body colour, glaze colour, glaze gloss, crackle, specks | crackle, glaze depth | one gloss lobe | partly: `relief` glazed tiles |
| `glass` | windows, bottles, crystal glass, stained glass | tint, thickness, roughness (frost), waviness | refraction, coloured shadow | flat tinted see-through | yes: `evani-material-nature` glass |
| `gem` | cut stones, crystals, amber | colour, dispersion, inclusion amount, cut | sparkle (dispersion), inner glow | glass with a sparkle mark | new |
| `water` | pools, lakes, rivers, oceans, liquids | colour, depth fade, wave size, foam, murk | waves, refraction, foam, sun glint | flat tinted surface with a fresnel edge | yes: `evani-material-nature` water |
| `ice` | ice, icicles, frost, snow surfaces | colour, clarity, crack amount, sparkle, scatter | inner cracks, sparkle, scatter | white with a blue rim | new |
| `fire` | flames, embers, lava, smoke, steam | colour ramp, flicker speed, intensity, softness | animated flicker, light cast | static glowing card | new (the `particles` effect can carry it) |
| `glow` | lamps, neon, screens, magic light | colour, intensity, pulse | bloom halo, pulse | plain emissive colour | yes: `evani-material-nature` lamp, model emissive |

Count: **12 exist today** (`paint`, `masonry`, `plaster`, `cloth`, `skin`, `hair`, `eye`, `foliage`, `thin`, `glass`, `water`, `glow`), **4 exist in part** (`metal`, `wood`, `stone`, `ceramic`) and **13 are new** (`plastic`, `bark`, `ground`, `leather`, `fur`, `feather`, `scales`, `hide`, `keratin`, `organic`, `gem`, `ice`, `fire`; `plastic` and `hide` are small cut-downs of existing ones).

## 4. Condition layers (8)

Each layer is a mask and a handful of numbers laid over the base material, with an amount from 0 to 1. Any material can take any layer it lists in its `conditions` column.

| Layer | What it does | Numbers | Today |
|---|---|---|---|
| `wet` | darkens and adds gloss; puddles on flat ground | amount, puddle amount, puddle scale | yes: `relief` wetness and puddles |
| `mud` | caked mud from the ground up, drier at the top | amount, height, colour | new |
| `dust` | dust, soot, ash and grime in hollows and on top faces | amount, colour, cavity bias | partly: `evani-material-proc` grime and streaks |
| `snow` | snow and frost on upward faces | amount, angle, frost | new |
| `wear` | edge wear, scratches, chipped paint, fraying | amount, edge width, scratch amount | partly: `evani-material-proc` edge wear |
| `oxide` | rust on iron, green on copper, tarnish on silver | amount, colour picked by the metal | new |
| `growth` | moss, lichen and algae on damp, shaded faces | amount, colour, scale | yes: `relief` moss |
| `blood` | blood splats, drying to dark brown | amount, age | new |

Why 8: these are the states a large outdoor world needs on almost everything. Burnt, frozen and weathered looks come from combining them (dust + wear, snow + wet) or are their own materials where the surface itself changes (charred wood, burnt grass).

## 5. Settings you can turn on and off

Three levels, all named as live parameters, so the viewer's sliders, the Look Lab, an AI agent and the game's own options menu all flip the same names through the same control API. Engine-wide material settings follow the existing pattern `material.<family>.<name>` (as `material.layers.quality` and `material.character.quality` do today); viewer-only switches use `kit.material_view.<name>`.

### 5.1 Per-material switches in the viewer

- **Condition layers.** One switch and amount slider per layer the material lists: `kit.material_view.cond.<layer>.on` and `kit.material_view.cond.<layer>.amount` (layer = wet, mud, dust, snow, wear, oxide, growth, blood). In a game the same layer is `material.cond.<layer>.amount`, set per object or per area (rain sets wet everywhere).
- **Recipe extras.** The "Extras" column of section 3 is the list per recipe. Each is a switch `kit.material_view.extra.<recipe>.<extra>` (for example `extra.fur.strands`, `extra.paint.clearcoat`, `extra.skin.subsurface`, `extra.masonry.parallax`, `extra.stone.parallax`, `extra.water.refraction`), so a person can see what each extra adds and costs.
- The frame-time readout already in the viewer's Quality section shows the cost of each switch.

### 5.2 Player quality settings (a graphics menu)

Groups, each a live parameter with levels `off / low / medium / high / ultra`. Each tier file in `assets/tiers/*.json` gets a default for every group (planned field `material_quality`, not added here); the player or the game can override any of them.

| Group (parameter) | Lite | Low | Medium | High | Ultra |
|---|---|---|---|---|---|
| Texture and detail `material.quality.detail` | flat colour, no relief | flat, painted grain | relief on near objects | parallax on near objects | parallax everywhere in view |
| Fur and hair `material.quality.fur` | painted coat | painted coat + sheen rim | short shells near | shells | shells + strands near the camera |
| Water `material.quality.water` | flat tinted, fresnel | + waves | + foam, sun glint | + refraction | + refraction everywhere |
| Glass and refraction `material.quality.glass` | flat tinted | flat tinted | + coloured shadow | + bent view in bottles | as High (matches nature's High / Ultra today) |
| Skin `material.quality.skin` | wrap light | wrap light | + pores | + subsurface glow | + wrinkle detail |
| Foliage `material.quality.foliage` | flat | + per-cell colour | + back-light glow | + wind | + wind on grass |
| Condition layers `material.quality.conditions` | 1 layer per object (strongest) | 2 | 4 | all 8 | all 8 + puddle reflections |
| Reflections `material.quality.reflections` | sky colour only | sky colour | sky cube (`ibl_cube`) | sky cube + metal anisotropy | + screen-space reflection (`ssr`) where the look has it |

Rule: every recipe has a light version (section 3). Turning a group down swaps to the light version of that recipe on the GPU; it never removes the material or draws it wrong. The look's own limits (look guard) still apply.

### 5.3 Build-time on/off

The engine's strict module rule applies to materials too. A game lists the recipes and materials it uses in `evani.toml` (for example `[materials] recipes = ["wood", "metal", "fur"]` plus a material list or category filter; the exact keys are for the build step to decide). Only those recipes' shader code and those materials' numbers reach the download; a recipe nobody lists costs nothing, and a missing recipe is a check error, the same way a `.glb` without `models` is today. The viewer itself lists everything.

## 6. Skin

- **Tone range, never names of peoples.** 10 depth steps from very fair (01) to deepest (10), each in a warm (golden-peach), neutral and cool (rosy-pink) undertone: 30 materials, ids `skin.tone.<01-10>.<warm|neutral|cool>`. Tone and undertone are two numbers, so a game can also slide between steps.
- **Traits are their own entries on the same recipe** (19): freckles, sun-weathered, elder, child, rosy cheeks, sickly pale, stubble shadow, scar, burn scar, bruise, tattoo ink, pale patches, calluses, plus lips and makeup. In practice a trait is a second set of numbers mixed onto any tone; listing them lets the viewer and search find them.
- Hair (20 colours and textures), beards (8), eyes (11) and teeth and nails (6) complete the person.

## 7. Batch plan

> Update (2026-10-24): Chris approved batch 1 and "also do the 2 optional", so batch 11 (vehicles 30 and fantasy / sci-fi 30) is built alongside batch 1, not later. See `BATCH1_BUILD_PLAN.md`.

**Batch size: about 100.** Reason: each batch is judged in all 24 styles, so 100 materials is about 2,400 style looks, a reviewable sitting with the viewer's compare tool; smaller batches would re-judge the same recipes too often, larger ones would hide problems. Batches are grouped by what a game world needs together (a town, a herd), so each one finishes a usable set.

| Batch | Size | What | Why this order |
|---|---|---|---|
| 1 | 106 | **Sampler**: 9 to 18 from every core category, at least one of every recipe (29) | every style is judged across the whole library early; every new recipe gets built once |
| 2 | 98 | Land and plants: soil, sand, gravel, rock, grass, snow ground, leaves, needles, bark | the ground and trees fill most of a frame |
| 3 | 90 | Buildings and town | a town is the second thing every open world needs |
| 4 | 120 | People and core clothing: all skin tones and traits, hair, eyes, main fabrics, leather | characters need skin and clothes together |
| 5 | 95 | Horses, livestock, dogs and cats | the `fur` recipe across its widest range |
| 6 | 74 | Wild mammals, hides, horn and bone | builds on batch 5's fur |
| 7 | 96 | Birds, reptiles, fish and insects | `feather` and `scales` at scale |
| 8 | 129 | Household props, food, timber, crops | interiors, once rooms exist from batch 3 |
| 9 | 106 | Weapons and tools, the rest of the clothing | metals and finishes, reusing batch 4 fabrics |
| 10 | 80 | Water, ice, fire, glow, flowers, desert plants, special ground | the remaining nature and effects |
| 11 | 60 | Optional: vehicles, fantasy / sci-fi | kept as the last batch (decided while Chris was away, 2026-10-23; easy to drop) |

**Size per material.** A material is a recipe id and its numbers: 1 byte recipe, about 10 to 16 numbers stored as 8- or 16-bit values (colours 3 bytes each), a condition mask byte, and default condition amounts. **About 24 bytes, at most 32.** Names, search words and notes are dev data for the viewer and catalogue, never shipped. For 1,054 materials: **about 25 KB raw, roughly 8 to 12 KB gzipped** for the whole library; a game that lists 60 materials carries about 1.5 KB. The real download cost is the recipes' shader code (a few KB each, only for listed recipes), so the recipe count matters far more than the material count.

## 8. Search and browse (design only)

- **Top: the 24 styles** stay as they are, a row of chips. The chosen style draws everything in the panel and scene.
- **Left: category tree.** The 9 core categories (plus the 2 optional ones when the viewer lists them), each opening to its subcategories with counts ("Animals 283 > Horse coats 40"). Clicking a subcategory shows its materials as small swatches drawn in the current style.
- **Search box above the tree.** Matches the name, id and search words, typo-tolerant, ranked: exact name, then name start, then search word, then look note. "penny" finds copper, "gator" finds alligator. Results keep their category path so people learn the tree.
- **Filters under the search box:** recipe (29 chips, grouped as hard / soft / living / liquid and light), "has condition layers" plus a chip per layer (shows only materials that list it), "exists today", and batch. Filters combine with search.
- **Swatch actions:** open on the test shape, compare A / B (already in the viewer), switch condition layers (section 5.1), copy the id for `evani.toml`.
- **Cost:** the tree and search index are built at dev time from the CSV and live only in the viewer's dev data, never in a shipped game.

## 9. Coverage check against public libraries

The tree was compared with the category structure of the large public CC0 texture libraries (rock, ground, wood, metal, fabric, leather, brick, tiles, plaster, concrete, roofing, plants). Every one of their top-level groups has a home here; they have almost nothing for animals, people or food, which is where most of this list goes. Only the structure was compared; no names or content were taken.

## 10. Notes for later

- `catalog/looks.json` has 51 looks and the viewer has 24 styles; the styles' starter materials (`styles.json`) should be re-pointed at catalogue ids once batch 1 exists.
- The 8 condition layers and the `material.quality.*` groups are new engine work; the tier files do not have a material field yet.
- Fire and smoke are drawn as cards or particles (the `particles` effect), not as surfaces; they are in the catalogue so styles can be judged on them.

## 11. Optional categories

- **Vehicles (30, batch 11):** car paints (solid, metallic, pearl, colour-shift, matte, primer, carriage lacquer, locomotive enamel), chrome, wheels, exhaust heat tint, tyres, dashboard and seat vinyl, windscreen and lamp lenses, wagon wood and canvas. Mostly `paint`, `metal` and `plastic`, so cheap once batch 1 exists.
- **Fantasy / sci-fi (30, batch 11):** cut gems and crystals, glowing runes, portal, ghost glow, force field, potion, slime, spaceship hull, alien skin and chitin, hologram, energy core, circuit board, solar panel, liquid metal. Uses `gem`, `glow`, `metal` and existing recipes; no new recipe.

## 12. Count tables

These tables are read by the check script (`.evani-tmp-matcat/check.mjs`); keep them equal to the CSV.

### Counts per category

| Category | Count |
|---|---|
| animals | 283 |
| people | 94 |
| clothing | 104 |
| weapons_tools | 62 |
| buildings | 102 |
| land | 90 |
| plants | 98 |
| household | 120 |
| elements | 41 |
| vehicles | 30 |
| fantasy_scifi | 30 |

### Counts per batch

| Batch | Count |
|---|---|
| 1 | 106 |
| 2 | 98 |
| 3 | 90 |
| 4 | 120 |
| 5 | 95 |
| 6 | 74 |
| 7 | 96 |
| 8 | 129 |
| 9 | 106 |
| 10 | 80 |
| 11 | 60 |

### Counts per recipe

| Recipe | Count |
|---|---|
| metal | 76 |
| paint | 29 |
| plastic | 21 |
| wood | 54 |
| bark | 16 |
| stone | 34 |
| masonry | 21 |
| plaster | 12 |
| ground | 44 |
| cloth | 74 |
| leather | 18 |
| fur | 153 |
| feather | 42 |
| scales | 28 |
| hide | 29 |
| skin | 50 |
| hair | 34 |
| eye | 13 |
| keratin | 40 |
| foliage | 59 |
| thin | 50 |
| organic | 24 |
| ceramic | 25 |
| glass | 31 |
| gem | 13 |
| water | 23 |
| ice | 14 |
| fire | 12 |
| glow | 15 |

`magic.slime` moved from thin (a membrane: flat colour) to organic's slime
variant in material-view round 4, so it is wet, glowing and see-through
(organic's `clear` extra); thin 51 to 50, organic 23 to 24.

Material-view round 5 moved `food.wine` from water to glass (see-through ruby, the way whiskey and honey are drawn): water 24 to 23, glass 30 to 31.
