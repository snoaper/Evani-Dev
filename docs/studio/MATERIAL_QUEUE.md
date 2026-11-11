# Material queue: one material at a time

Chris, 2026-10-31: "take 1 material at a time improve it then move on". Each
material worker takes the **top `todo` row**, improves exactly that material
(before and after pictures, checks, merge into `studio`, preview refresh),
marks it `done` with a one-line note, and stops.

Order: the order a user meets the materials in the studio, on the opening
asset (Horse, default material `horse.coat.bay`): the
materials bar's families (native to the asset first, then the ones that go
on it, then the rest; bigger families first), each family in catalogue order.
694 ready materials in total; planned rows join when they are built. Regenerate
with `node tools/studio/material-queue.mjs --write` (keeps every status and note).

### horse_coats (8)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 1 | `horse.coat.bay` | Horse coat, bay | fur | done | 2026-10-31: sleek satin mahogany coat, no wood grain or black dapple rings; fur recipe gained a short-coat rule (docs/studio/materials/horse.coat.bay.md) |
| 2 | `horse.coat.grey_dapple` | Horse coat, dapple grey | fur | done | 2026-11-01: soft pale filled dapples in a darker grey net, darker legs, no doughnut rings or scribbles (docs/studio/materials/horse.coat.grey_dapple.md) |
| 3 | `horse.coat.grey_steel` | Horse coat, steel grey | fur | done | 2026-11-01: dark even blue-grey with fine single silver hairs, dark legs, no rain-dash streaks or blotches; roan hairs are finer for every roan (docs/studio/materials/horse.coat.grey_steel.md) |
| 4 | `horse.coat.palomino` | Horse coat, palomino | fur | done | 2026-11-01: even deep gold body and legs with a satin shine, cream-white mane and tail, no brush strokes or cream stockings; rows can now say which points regions wear the points colour (`points_on`) (docs/studio/materials/horse.coat.palomino.md) |
| 5 | `horse.coat.red_roan` | Horse coat, red roan | fur | done | 2026-11-01: strawberry chestnut body of fine soft pale hairs, solid dark chestnut legs, mane and tail, no barcode dashes or blotches; rows can now name the colour their points pull toward (`points_to`) (docs/studio/materials/horse.coat.red_roan.md) |
| 6 | `horse.coat.blue_roan` | Horse coat, blue roan | fur | done | 2026-11-01: even slate blue-grey body of fine soft pale hairs, solid black legs, mane and tail, no powder-blue toy coat, white dashes or blotches (docs/studio/materials/horse.coat.blue_roan.md) |
| 7 | `horse.coat.bay_roan` | Horse coat, bay roan | fur | done | 2026-11-01: frosted rosy red-brown body of fine soft pale hairs, solid black legs, mane and tail (were white), no barcode dashes or blotches (docs/studio/materials/horse.coat.bay_roan.md) |
| 8 | `horse.coat.rabicano` | Horse coat, rabicano | fur | todo | Solid coat with white ticking on flanks and tail root |

### skin_tones (30)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 9 | `skin.tone.01.warm` | Skin, tone 01 (very fair), warm | skin | todo | Step 01 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 10 | `skin.tone.01.neutral` | Skin, tone 01 (very fair), neutral | skin | todo | Step 01 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 11 | `skin.tone.01.cool` | Skin, tone 01 (very fair), cool | skin | todo | Step 01 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 12 | `skin.tone.02.warm` | Skin, tone 02 (fair), warm | skin | todo | Step 02 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 13 | `skin.tone.02.neutral` | Skin, tone 02 (fair), neutral | skin | todo | Step 02 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 14 | `skin.tone.02.cool` | Skin, tone 02 (fair), cool | skin | todo | Step 02 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 15 | `skin.tone.03.warm` | Skin, tone 03 (light), warm | skin | todo | Step 03 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 16 | `skin.tone.03.neutral` | Skin, tone 03 (light), neutral | skin | todo | Step 03 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 17 | `skin.tone.03.cool` | Skin, tone 03 (light), cool | skin | todo | Step 03 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 18 | `skin.tone.04.warm` | Skin, tone 04 (light-medium), warm | skin | todo | Step 04 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 19 | `skin.tone.04.neutral` | Skin, tone 04 (light-medium), neutral | skin | todo | Step 04 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 20 | `skin.tone.04.cool` | Skin, tone 04 (light-medium), cool | skin | todo | Step 04 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 21 | `skin.tone.05.warm` | Skin, tone 05 (medium), warm | skin | todo | Step 05 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 22 | `skin.tone.05.neutral` | Skin, tone 05 (medium), neutral | skin | todo | Step 05 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 23 | `skin.tone.05.cool` | Skin, tone 05 (medium), cool | skin | todo | Step 05 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 24 | `skin.tone.06.warm` | Skin, tone 06 (medium-tan), warm | skin | todo | Step 06 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 25 | `skin.tone.06.neutral` | Skin, tone 06 (medium-tan), neutral | skin | todo | Step 06 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 26 | `skin.tone.06.cool` | Skin, tone 06 (medium-tan), cool | skin | todo | Step 06 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 27 | `skin.tone.07.warm` | Skin, tone 07 (medium-deep), warm | skin | todo | Step 07 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 28 | `skin.tone.07.neutral` | Skin, tone 07 (medium-deep), neutral | skin | todo | Step 07 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 29 | `skin.tone.07.cool` | Skin, tone 07 (medium-deep), cool | skin | todo | Step 07 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 30 | `skin.tone.08.warm` | Skin, tone 08 (deep), warm | skin | todo | Step 08 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 31 | `skin.tone.08.neutral` | Skin, tone 08 (deep), neutral | skin | todo | Step 08 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 32 | `skin.tone.08.cool` | Skin, tone 08 (deep), cool | skin | todo | Step 08 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 33 | `skin.tone.09.warm` | Skin, tone 09 (very deep), warm | skin | todo | Step 09 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 34 | `skin.tone.09.neutral` | Skin, tone 09 (very deep), neutral | skin | todo | Step 09 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 35 | `skin.tone.09.cool` | Skin, tone 09 (very deep), cool | skin | todo | Step 09 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |
| 36 | `skin.tone.10.warm` | Skin, tone 10 (deepest), warm | skin | todo | Step 10 of 10 from light to deep, golden-peach undertone; soft scatter under the surface |
| 37 | `skin.tone.10.neutral` | Skin, tone 10 (deepest), neutral | skin | todo | Step 10 of 10 from light to deep, balanced undertone, neither pink nor gold; soft scatter under the surface |
| 38 | `skin.tone.10.cool` | Skin, tone 10 (deepest), cool | skin | todo | Step 10 of 10 from light to deep, rosy-pink undertone; soft scatter under the surface |

### metals (28)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 39 | `metal.steel.blued` | Steel, blued | metal | todo | Deep blue-black with soft highlights |
| 40 | `metal.steel.case_hardened` | Steel, case-hardened | metal | todo | Mottled blue, straw and purple swirls |
| 41 | `metal.steel.brushed` | Steel, brushed | metal | todo | Fine straight grain stretching the highlight |
| 42 | `metal.steel.polished` | Steel, polished | metal | todo | Bright mirror steel |
| 43 | `metal.steel.forged` | Steel, black forged | metal | todo | Dark hammer-marked with grey scale |
| 44 | `metal.steel.damascus` | Steel, pattern-welded | metal | todo | Flowing light and dark wavy lines |
| 45 | `metal.steel.parkerized` | Steel, matte grey phosphate | metal | todo | Flat grey-green fine-grain finish |
| 46 | `metal.iron.cast` | Iron, cast | metal | todo | Grainy dark grey, sandy surface |
| 47 | `metal.iron.wrought` | Iron, wrought | metal | todo | Dark iron with fibrous streaks |
| 48 | `metal.brass.polished` | Brass, polished | metal | todo | Warm yellow mirror shine |
| 49 | `metal.brass.aged` | Brass, aged | metal | todo | Brown-gold, dull and darker in recesses |
| 50 | `metal.bronze` | Bronze | metal | todo | Deep red-gold, softer highlight |
| 51 | `metal.copper.polished` | Copper, polished | metal | todo | Bright orange-pink mirror shine |
| 52 | `metal.copper.hammered` | Copper, hammered | metal | todo | Dimpled copper catching many highlights |
| 53 | `metal.silver.polished` | Silver, polished | metal | todo | Bright cool white mirror |
| 54 | `metal.silver.engraved` | Silver, engraved | metal | todo | Silver with fine cut scroll patterns |
| 55 | `metal.gold.polished` | Gold, polished | metal | todo | Rich yellow mirror, warm reflections |
| 56 | `metal.gold.rose` | Gold, rose | metal | todo | Pinkish gold |
| 57 | `metal.nickel` | Nickel plate | metal | todo | Warm grey-white bright plating |
| 58 | `metal.chrome` | Chrome | metal | todo | Perfect cold mirror |
| 59 | `metal.pewter` | Pewter | metal | todo | Soft dull blue-grey |
| 60 | `metal.tin` | Tin plate | metal | todo | Light grey thin bright metal |
| 61 | `metal.lead` | Lead | metal | todo | Dull blue-grey, very soft sheen |
| 62 | `metal.zinc.galvanised` | Galvanised steel | metal | todo | Grey with crystal spangle pattern |
| 63 | `metal.aluminium` | Aluminium | metal | todo | Light grey, slightly milky shine |
| 64 | `metal.titanium` | Titanium | metal | todo | Dark grey with faint rainbow heat tint |
| 65 | `metal.gunmetal` | Gunmetal alloy | metal | todo | Dark grey-bronze |
| 66 | `metal.heat_tinted` | Steel, heat-tinted | metal | todo | Straw to blue bands from heat |

### rock (27)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 67 | `rock.granite.grey` | Granite, grey | stone | todo | Speckled grey with mica glints |
| 68 | `rock.granite.pink` | Granite, pink | stone | todo | Pink with black flecks |
| 69 | `rock.basalt` | Basalt | stone | todo | Dark grey fine rock, columnar cracks |
| 70 | `rock.sandstone.red` | Sandstone, red | stone | todo | Layered rusty red with pale bands |
| 71 | `rock.sandstone.buff` | Sandstone, buff | stone | todo | Pale cream layered rock |
| 72 | `rock.sandstone.striped` | Sandstone, banded | stone | todo | Swirling red, orange and cream bands |
| 73 | `rock.limestone` | Limestone | stone | todo | Pale grey with pits and fossils |
| 74 | `rock.chalk` | Chalk | stone | todo | Soft bright white, crumbly |
| 75 | `rock.slate` | Slate | stone | todo | Dark grey thin flaking layers |
| 76 | `rock.shale` | Shale | stone | todo | Dark brittle layers |
| 77 | `rock.marble.white` | Marble, white | stone | todo | White with soft grey veins, translucent edge |
| 78 | `rock.marble.green` | Marble, green | stone | todo | Dark green with white veins |
| 79 | `rock.obsidian` | Obsidian | gem | todo | Glossy black, sharp curved fractures |
| 80 | `rock.pumice` | Pumice | stone | todo | Pale grey full of holes |
| 81 | `rock.lava.cooled` | Cooled lava | stone | todo | Black ropy wrinkled rock |
| 82 | `rock.quartzite` | Quartzite | stone | todo | Sugary white and pink |
| 83 | `rock.flint` | Flint | stone | todo | Dark glassy with a white crust |
| 84 | `rock.conglomerate` | Conglomerate | stone | todo | Pebbles cemented in sandy rock |
| 85 | `rock.travertine` | Travertine | stone | todo | Cream with small holes and bands |
| 86 | `rock.mossy_boulder` | Boulder, lichen-spotted | stone | todo | Grey with pale green and orange lichen spots |
| 87 | `rock.cave_wall` | Cave wall | stone | todo | Damp dark rock, smooth flowstone |
| 88 | `rock.stalactite` | Flowstone | stone | todo | Wet cream-orange drapery stone |
| 89 | `rock.ore.iron` | Iron ore | stone | todo | Rusty red-black heavy rock |
| 90 | `rock.ore.copper` | Copper ore | stone | todo | Grey rock with green and blue streaks |
| 91 | `rock.ore.gold_vein` | Gold-bearing quartz | stone | todo | White quartz with gold flecks |
| 92 | `rock.ore.coal` | Coal | stone | todo | Shiny black layered lumps |
| 93 | `rock.salt` | Rock salt | gem | todo | Translucent pink-white crystals |

### food (23)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 94 | `food.bread.crust` | Bread crust | organic | todo | Golden-brown crust with flour dust |
| 95 | `food.bread.crumb` | Bread crumb | organic | todo | Airy pale soft inside |
| 96 | `food.meat.raw` | Meat, raw | organic | todo | Deep red muscle grain with small flecks of creamy fat |
| 97 | `food.meat.cooked` | Meat, roasted | organic | todo | Browned crisp outside |
| 98 | `food.meat.jerky` | Jerky | organic | todo | Dark red-brown leathery strips with a fibrous grain |
| 99 | `food.fish.cooked` | Fish, cooked | organic | todo | White flaky flesh, the flakes parted by soft shadows |
| 100 | `food.cheese.hard` | Cheese, hard | organic | todo | Pale yellow block with a few small holes |
| 101 | `food.cheese.holes` | Cheese with holes | organic | todo | Pale yellow with round holes |
| 102 | `food.egg` | Egg | keratin | todo | Matte brown shell, glossy yolk inside |
| 103 | `food.apple` | Apple | thin | todo | Waxy red skin with a speckled shine |
| 104 | `food.orange` | Orange | thin | todo | Dimpled bright orange peel |
| 105 | `food.grapes` | Grapes | thin | todo | Dusty purple translucent skins |
| 106 | `food.banana` | Banana | thin | todo | Yellow skin with brown flecks |
| 107 | `food.potato` | Potato | organic | todo | Dusty brown skin with eyes |
| 108 | `food.carrot` | Carrot | organic | todo | Bright orange with fine rings |
| 109 | `food.corn_cob` | Corn cob | organic | todo | Shiny yellow kernels in rows |
| 110 | `food.beans` | Beans in sauce | organic | todo | Glossy brown beans in sauce |
| 111 | `food.pie` | Fruit pie | organic | todo | Golden lattice crust over a glossy red fruit filling |
| 112 | `food.chocolate` | Chocolate | organic | todo | Glossy dark brown with a snap edge |
| 113 | `food.honey` | Honey | glass | todo | Thick clear golden liquid |
| 114 | `food.coffee` | Coffee | water | todo | Dark brown liquid with crema |
| 115 | `food.whiskey` | Whiskey | glass | todo | Clear amber liquid |
| 116 | `food.wine` | Red wine | glass | todo | Deep ruby liquid |

### birds (18)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 117 | `crow.feather` | Crow feathers | feather | todo | Matte black with a faint purple sheen |
| 118 | `pigeon.feather` | Pigeon feathers | feather | todo | Grey with green-purple neck shimmer |
| 119 | `dove.feather` | Dove feathers, pale | feather | todo | Soft tan-grey with small black spots |
| 120 | `sparrow.feather` | Sparrow feathers | feather | todo | Streaky brown and grey |
| 121 | `bluejay.feather` | Jay feathers, blue | feather | todo | Bright blue with black bars and white tips |
| 122 | `eagle.feather.brown` | Eagle feathers, brown | feather | todo | Dark brown with golden neck feathers |
| 123 | `owl.feather.barred` | Owl feathers, barred | feather | todo | Soft brown and cream bars, velvety |
| 124 | `turkey.feather` | Turkey feathers, bronze | feather | todo | Bronze with copper-green shimmer and barred tail |
| 125 | `goose.feather` | Goose feathers, grey | feather | todo | Grey-brown with pale edges |
| 126 | `swan.feather` | Swan feathers | feather | todo | Pure white, dense and soft |
| 127 | `egret.feather` | Egret feathers | feather | todo | White with wispy long plumes |
| 128 | `peacock.feather` | Peacock feathers | feather | todo | Bronze-green with blue eye spots |
| 129 | `pheasant.feather` | Pheasant feathers | feather | todo | Copper with black scale marks, long tail |
| 130 | `quail.feather` | Quail feathers | feather | todo | Speckled grey-brown with white scallops |
| 131 | `hummingbird.feather` | Hummingbird feathers | feather | todo | Metallic green with a ruby throat |
| 132 | `ostrich.feather` | Ostrich feathers | feather | todo | Loose black plumes with white tips |
| 133 | `woodpecker.feather` | Woodpecker feathers | feather | todo | Black and white bars, red crown |
| 134 | `quill.feather` | Writing quill | feather | todo | Single large white flight feather, clear shaft |

### reptiles_amphibians (17)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 135 | `snake.rattle` | Snake scales, diamond-back | scales | todo | Tan with dark-edged diamonds, keeled scales |
| 136 | `snake.garter` | Snake scales, striped | scales | todo | Dark green with three yellow stripes |
| 137 | `snake.black` | Snake scales, glossy black | scales | todo | Smooth glossy black, white chin |
| 138 | `snake.banded` | Snake scales, banded red | scales | todo | Red, black and yellow rings |
| 139 | `snake.python` | Snake scales, blotched | scales | todo | Large tan and brown blotches, satin sheen |
| 140 | `snake.green` | Snake scales, green | scales | todo | Bright smooth leaf green |
| 141 | `alligator.back` | Alligator back | scales | todo | Dark bumpy armoured scutes |
| 142 | `alligator.belly` | Alligator belly | scales | todo | Cream rectangular scales in rows |
| 143 | `crocodile.skin` | Crocodile skin | scales | todo | Olive-grey with dark bands |
| 144 | `iguana.skin` | Iguana skin | scales | todo | Green with darker bands, fine bead scales |
| 145 | `chameleon.skin` | Chameleon skin | scales | todo | Bright green with turquoise and yellow bands |
| 146 | `gecko.skin` | Gecko skin | scales | todo | Yellow with dark spots, velvety tiny scales |
| 147 | `beaded_lizard.skin` | Beaded lizard skin | scales | todo | Black and orange bead-like scales |
| 148 | `horned_lizard.skin` | Horned lizard skin | scales | todo | Sandy with small spikes and dark spots |
| 149 | `tortoise.skin` | Tortoise skin | scales | todo | Grey-brown heavy scales |
| 150 | `frog.green` | Frog skin, green | hide | todo | Wet green skin with dark blotches |
| 151 | `toad.warty` | Toad skin, warty | hide | todo | Dry brown bumpy skin |

### wood_build (17)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 152 | `wood.plank.deck` | Plank deck | wood | todo | Long planks with gaps and nail heads |
| 153 | `wood.plank.weathered` | Weathered grey boards | wood | todo | Sun-greyed boards with raised grain |
| 154 | `wood.plank.painted` | Painted boards, flaking | wood | todo | Paint peeling to bare wood |
| 155 | `wood.clapboard` | Clapboard siding | wood | todo | Overlapping horizontal boards |
| 156 | `wood.board_batten` | Board and batten | wood | todo | Vertical boards with narrow strips |
| 157 | `wood.log.peeled` | Log wall, peeled | wood | todo | Round golden logs with chinking |
| 158 | `wood.log.bark` | Log wall, bark on | bark | todo | Logs still in bark |
| 159 | `wood.shingle` | Wood shingles | wood | todo | Overlapping split cedar shakes |
| 160 | `wood.floor.oak` | Oak floorboards | wood | todo | Warm honey boards, varnished |
| 161 | `wood.floor.pine` | Pine floorboards | wood | todo | Pale yellow with dark knots |
| 162 | `wood.parquet` | Parquet | wood | todo | Small blocks in a herringbone |
| 163 | `wood.panelling` | Wood panelling | wood | todo | Dark polished raised panels |
| 164 | `wood.beam.oak` | Oak beam | wood | todo | Adze-marked dark oak |
| 165 | `wood.railway_tie` | Railway sleeper | wood | todo | Dark creosoted heavy timber |
| 166 | `wood.crate` | Wooden crate | wood | todo | Rough pine slats |
| 167 | `wood.lattice` | Lattice | wood | todo | Thin crossed slats |
| 168 | `wood.bamboo` | Bamboo | wood | todo | Jointed glossy green-yellow poles |

### plain_weaves (16)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 169 | `cloth.cotton.white` | Cotton, white | cloth | todo | Soft matte plain weave, slightly warm white |
| 170 | `cloth.cotton.unbleached` | Cotton, unbleached | cloth | todo | Creamy off-white with flecks |
| 171 | `cloth.cotton.chambray` | Chambray, blue | cloth | todo | Blue warp and white weft, soft mottle |
| 172 | `cloth.linen.natural` | Linen, natural | cloth | todo | Uneven slubby weave, crisp sheen |
| 173 | `cloth.linen.white` | Linen, bleached | cloth | todo | Bright white with visible slubs |
| 174 | `cloth.muslin` | Muslin, sheer | cloth | todo | Open loose weave, light passes through |
| 175 | `cloth.calico_print` | Printed cotton, small flowers | cloth | todo | Tiny repeated flowers on cotton |
| 176 | `cloth.gingham` | Gingham check | cloth | todo | Small even two-colour checks |
| 177 | `cloth.plaid` | Plaid flannel | cloth | todo | Brushed soft weave with bold crossing stripes |
| 178 | `cloth.tartan` | Wool tartan | cloth | todo | Dense twill with many-coloured crossing bands |
| 179 | `cloth.pinstripe` | Pinstripe suiting | cloth | todo | Dark wool with fine pale stripes |
| 180 | `cloth.ticking` | Ticking stripe | cloth | todo | Stiff cotton with narrow blue stripes |
| 181 | `cloth.burlap` | Burlap | cloth | todo | Coarse open brown weave, hairy fibres |
| 182 | `cloth.canvas.natural` | Canvas, natural | cloth | todo | Heavy tight weave, stiff and matte |
| 183 | `cloth.canvas.waxed` | Canvas, waxed | cloth | todo | Darker canvas with a waxy sheen and creases |
| 184 | `cloth.canvas.army` | Canvas, olive drab | cloth | todo | Olive-green heavy canvas |

### soil (16)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 185 | `ground.soil.loam` | Soil, dark loam | ground | todo | Crumbly dark brown with small clods |
| 186 | `ground.soil.clay_red` | Soil, red clay | ground | todo | Dense orange-red, cracks when dry |
| 187 | `ground.soil.clay_grey` | Soil, grey clay | ground | todo | Smooth sticky blue-grey |
| 188 | `ground.soil.ploughed` | Ploughed field | ground | todo | Rows of turned dark earth |
| 189 | `ground.soil.packed` | Packed dirt | ground | todo | Hard flattened light-brown earth |
| 190 | `ground.soil.wagon_ruts` | Dirt road with ruts | ground | todo | Packed earth with two deep wheel ruts |
| 191 | `ground.soil.trail` | Footpath | ground | todo | Narrow worn earth line through grass |
| 192 | `ground.soil.forest_floor` | Forest floor | ground | todo | Dark soil under needles, twigs and leaves |
| 193 | `ground.soil.peat` | Peat | ground | todo | Fibrous very dark brown |
| 194 | `ground.soil.ash_volcanic` | Volcanic soil | ground | todo | Fine black gritty soil |
| 195 | `ground.soil.cracked_dry` | Cracked dry earth | ground | todo | Pale crust split into polygons |
| 196 | `ground.soil.alkali` | Alkali flat | ground | todo | White crusty crystals over grey |
| 197 | `ground.mud.swamp` | Swamp mud | ground | todo | Dark glossy mud with standing water |
| 198 | `ground.mud.riverbank` | Riverbank mud | ground | todo | Smooth grey-brown silt with ripples |
| 199 | `ground.mud.tidal` | Tidal mudflat | ground | todo | Shiny grey mud with worm casts |
| 200 | `ground.corral_dirt` | Corral dirt | ground | todo | Churned earth with hoof prints and straw |

### timber (16)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 201 | `timber.oak` | Oak, cut | wood | todo | Honey-brown with ray flecks |
| 202 | `timber.pine` | Pine, cut | wood | todo | Pale yellow with brown knots |
| 203 | `timber.cedar` | Cedar, cut | wood | todo | Pink-red with soft grain |
| 204 | `timber.birch` | Birch, cut | wood | todo | Very pale fine grain |
| 205 | `timber.cherry` | Cherry, cut | wood | todo | Warm red-brown, smooth |
| 206 | `timber.mahogany` | Mahogany | wood | todo | Deep red-brown, interlocked grain |
| 207 | `timber.teak` | Teak | wood | todo | Golden brown, oily sheen |
| 208 | `timber.walnut` | Walnut, cut | wood | todo | Chocolate with darker streaks |
| 209 | `timber.redwood` | Redwood, cut | wood | todo | Light red, straight grain |
| 210 | `timber.ash` | Ash, cut | wood | todo | Pale with strong straight grain |
| 211 | `timber.elm` | Elm, cut | wood | todo | Brown with wild wavy grain |
| 212 | `timber.olive` | Olive wood | wood | todo | Cream with dark swirling lines |
| 213 | `timber.driftwood` | Driftwood | wood | todo | Smooth silver-grey worn wood |
| 214 | `timber.charred` | Charred wood | wood | todo | Black cracked squares of char |
| 215 | `timber.end_grain` | End grain | wood | todo | Concentric growth rings |
| 216 | `timber.plywood` | Plywood | wood | todo | Thin face veneer, layered edge |

### leaves (15)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 217 | `leaf.oak` | Leaves, oak | foliage | todo | Lobed mid-green leaves, glow when backlit |
| 218 | `leaf.maple.green` | Leaves, maple green | foliage | todo | Five-pointed bright green leaves |
| 219 | `leaf.maple.autumn` | Leaves, maple autumn | foliage | todo | Red and orange, strong backlit glow |
| 220 | `leaf.birch` | Leaves, birch | foliage | todo | Small round leaves, yellow in autumn |
| 221 | `leaf.cottonwood` | Leaves, cottonwood | foliage | todo | Glossy triangular leaves, pale underside |
| 222 | `leaf.willow` | Leaves, willow | foliage | todo | Long narrow silvery-green leaves |
| 223 | `leaf.elm` | Leaves, elm | foliage | todo | Rough oval serrated leaves |
| 224 | `leaf.magnolia` | Leaves, magnolia | foliage | todo | Thick glossy dark leaves, rusty underside |
| 225 | `leaf.palm` | Palm fronds | foliage | todo | Long feathery green fronds |
| 226 | `leaf.banana` | Banana leaves | foliage | todo | Huge torn bright green leaves |
| 227 | `leaf.fern` | Fern fronds | foliage | todo | Fine lacy fronds |
| 228 | `leaf.ivy` | Ivy | foliage | todo | Glossy dark heart-shaped leaves |
| 229 | `leaf.vine.grape` | Grape vine leaves | foliage | todo | Broad lobed leaves with curling tendrils |
| 230 | `leaf.jungle` | Jungle leaves, broad | foliage | todo | Big split glossy leaves |
| 231 | `leaf.dead_brown` | Leaves, dead brown | foliage | todo | Curled dry brown, no glow |

### bark (14)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 232 | `bark.oak` | Bark, oak | bark | todo | Deep grey furrows and ridges |
| 233 | `bark.pine` | Bark, pine | bark | todo | Orange-brown flaking plates |
| 234 | `bark.birch` | Bark, birch | bark | todo | White with black marks, peeling strips |
| 235 | `bark.aspen` | Bark, aspen | bark | todo | Smooth pale green-white with eye marks |
| 236 | `bark.beech` | Bark, beech | bark | todo | Smooth silver-grey |
| 237 | `bark.cottonwood` | Bark, cottonwood | bark | todo | Thick corky grey ridges |
| 238 | `bark.redwood` | Bark, redwood | bark | todo | Thick soft red-brown fibres |
| 239 | `bark.cherry` | Bark, cherry | bark | todo | Glossy red-brown with horizontal bands |
| 240 | `bark.palm` | Palm trunk | bark | todo | Grey rings and fibrous scars |
| 241 | `bark.mangrove` | Mangrove roots | bark | todo | Grey-brown arching wet roots |
| 242 | `bark.cypress` | Bark, swamp cypress | bark | todo | Stringy reddish grey bark |
| 243 | `bark.dead_snag` | Dead tree, bare wood | bark | todo | Weathered silver-grey dead wood, split along the grain, no bark |
| 244 | `bark.burnt` | Bark, charred | bark | todo | Black cracked charcoal with glowing seams possible |
| 245 | `bark.vine_woody` | Woody vine | bark | todo | Twisted brown cords |

### plaster_concrete (14)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 246 | `plaster.white` | Plaster, white | plaster | todo | Smooth matte white with faint trowel marks |
| 247 | `plaster.peach` | Plaster, warm painted | plaster | todo | Warm tinted plaster, soft |
| 248 | `plaster.lime_wash` | Limewash | plaster | todo | Chalky uneven white, patchy |
| 249 | `plaster.stucco` | Stucco, textured | plaster | todo | Swirled coarse texture |
| 250 | `plaster.cracked` | Plaster, cracked old | plaster | todo | Crazed cracks, chunks missing showing lath |
| 251 | `plaster.pebbledash` | Pebbledash | plaster | todo | Tiny pebbles stuck in render |
| 252 | `plaster.mud_daub` | Wattle and daub | plaster | todo | Brown mud with straw over woven sticks |
| 253 | `concrete.smooth` | Concrete, smooth | plaster | todo | Light grey with small air holes |
| 254 | `concrete.board` | Concrete, board-formed | plaster | todo | Wood-grain imprint in grey concrete |
| 255 | `concrete.block` | Concrete block | masonry | todo | Grey blocks with grainy face |
| 256 | `concrete.aggregate` | Concrete, exposed aggregate | plaster | todo | Pebbles showing through grey cement |
| 257 | `concrete.polished` | Concrete, polished | plaster | todo | Smooth sheen with soft mottling |
| 258 | `asphalt.road` | Asphalt | ground | todo | Dark grey with small stones |
| 259 | `asphalt.cracked` | Asphalt, cracked | ground | todo | Grey asphalt with tar-filled cracks |

### grass_ground (13)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 260 | `ground.grass.lawn` | Grass, short lawn | foliage | todo | Even short green blades |
| 261 | `ground.grass.meadow` | Grass, meadow | foliage | todo | Tall mixed grasses with seed heads |
| 262 | `ground.grass.prairie` | Grass, dry prairie | foliage | todo | Knee-high gold-brown grass |
| 263 | `ground.grass.bunch` | Bunchgrass | foliage | todo | Separate tufts with bare soil between |
| 264 | `ground.grass.marsh` | Marsh grass | foliage | todo | Tall green blades in water |
| 265 | `ground.grass.alpine` | Alpine turf | foliage | todo | Short tight green with tiny flowers |
| 266 | `ground.grass.burnt` | Grass, burnt | foliage | todo | Black stubble with grey ash |
| 267 | `ground.grass.frosted` | Grass, frosted | foliage | todo | White frost crystals on blade tips |
| 268 | `ground.grass.dead` | Grass, dead winter | foliage | todo | Flattened pale tan blades |
| 269 | `ground.clover` | Clover patch | foliage | todo | Round three-part leaves, low mat |
| 270 | `ground.moss.carpet` | Moss carpet | foliage | todo | Thick soft bright green cushion |
| 271 | `ground.leaf_litter` | Leaf litter, autumn | foliage | todo | Red, orange and brown fallen leaves |
| 272 | `ground.pine_needles` | Pine needles | foliage | todo | Rust-brown needle carpet |

### leather (13)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 273 | `leather.saddle_tan` | Leather, saddle tan | leather | todo | Smooth warm tan with soft sheen |
| 274 | `leather.dark_brown` | Leather, dark brown | leather | todo | Deep brown, polished highlights |
| 275 | `leather.black` | Leather, black | leather | todo | Black with sharp polish highlights |
| 276 | `leather.oxblood` | Leather, oxblood | leather | todo | Deep red-brown polish |
| 277 | `leather.rough_out` | Leather, rough-out | leather | todo | Fuzzy flesh side, matte |
| 278 | `leather.suede` | Suede | leather | todo | Velvety short nap, colour shifts when brushed |
| 279 | `leather.buckskin` | Buckskin | leather | todo | Soft pale smoked hide, creamy matte |
| 280 | `leather.rawhide` | Rawhide | leather | todo | Stiff translucent yellowish hide |
| 281 | `leather.tooled` | Leather, tooled | leather | todo | Carved floral patterns pressed into tan leather |
| 282 | `leather.patent` | Patent leather | leather | todo | Mirror-glossy black |
| 283 | `leather.embossed_scale` | Leather, embossed scale | leather | todo | Pressed scale pattern, glossy |
| 284 | `leather.snake` | Snakeskin leather | scales | todo | Fine scale leather with patterned bands |
| 285 | `leather.cracked_old` | Leather, old cracked | leather | todo | Dry dark leather with fine crazed cracks (look changed, bug fix: the cracks were drawn far too big) |

### sheen_fabrics (13)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 286 | `cloth.velvet.red` | Velvet, red | cloth | todo | Deep pile with bright sheen at edges |
| 287 | `cloth.velvet.black` | Velvet, black | cloth | todo | Near-black pile, soft rim light |
| 288 | `cloth.velvet.green` | Velvet, green | cloth | todo | Bottle-green pile |
| 289 | `cloth.satin.white` | Satin, white | cloth | todo | Smooth glossy face with long highlights |
| 290 | `cloth.satin.black` | Satin, black | cloth | todo | Black with silvery sharp highlights |
| 291 | `cloth.silk.plain` | Silk, plain | cloth | todo | Fine smooth weave, soft liquid sheen |
| 292 | `cloth.silk.brocade` | Brocade | cloth | todo | Raised woven pattern in gold thread |
| 293 | `cloth.lace.white` | Lace, white | thin | todo | Open floral thread pattern, see-through |
| 294 | `cloth.lace.black` | Lace, black | thin | todo | Fine black thread pattern |
| 295 | `cloth.tulle` | Tulle netting | thin | todo | Fine hexagon net |
| 296 | `cloth.fleece` | Fleece | cloth | todo | Soft fuzzy synthetic pile |
| 297 | `cloth.terry` | Terry towelling | cloth | todo | Dense small loops, soft and matte |
| 298 | `cloth.embroidery` | Embroidery thread | cloth | todo | Raised satin-stitch pattern over cloth |

### furniture_finish (12)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 299 | `finish.varnish.dark` | Varnished dark wood | wood | todo | Deep brown wood under glossy varnish |
| 300 | `finish.varnish.golden` | Varnished golden wood | wood | todo | Warm amber gloss |
| 301 | `finish.lacquer.black` | Black lacquer | paint | todo | Deep mirror-black |
| 302 | `finish.lacquer.red` | Red lacquer | paint | todo | Deep glossy red |
| 303 | `finish.painted.cream` | Painted furniture, cream | paint | todo | Soft cream paint, brush marks |
| 304 | `finish.painted.green` | Painted furniture, sage | paint | todo | Dull sage-green paint |
| 305 | `finish.milk_paint` | Milk paint, blue | paint | todo | Chalky blue, uneven |
| 306 | `finish.gilt` | Gilt | metal | todo | Thin gold leaf with red base showing |
| 307 | `finish.clearcoat` | Clear-coat paint | paint | todo | Coloured base under a deep clear layer |
| 308 | `finish.soap_film` | Soap film | paint | todo | Swirling rainbow film |
| 309 | `finish.wax_polish` | Waxed wood | wood | todo | Soft low gloss over wood |
| 310 | `finish.marquetry` | Marquetry | wood | todo | Pieces of light and dark veneer |

### interior (12)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 311 | `wall.paper.damask` | Wallpaper, damask | thin | todo | Repeating tone-on-tone flourish |
| 312 | `wall.paper.stripe` | Wallpaper, striped | thin | todo | Vertical stripes |
| 313 | `wall.paper.floral` | Wallpaper, floral | thin | todo | Faded flower print |
| 314 | `wall.paper.peeling` | Wallpaper, peeling | thin | todo | Curling sheets showing plaster |
| 315 | `wall.tin_ceiling` | Pressed tin ceiling | metal | todo | White-painted embossed squares |
| 316 | `wall.paint.matte` | Painted wall, matte | plaster | todo | Flat even painted plaster |
| 317 | `wall.paint.gloss` | Painted wall, gloss | paint | todo | Shiny hard paint with brush marks |
| 318 | `wall.carpet.wool` | Carpet, wool | cloth | todo | Dense short pile |
| 319 | `wall.carpet.patterned` | Carpet, patterned | cloth | todo | Pile with a bold repeating pattern |
| 320 | `wall.rug.woven` | Woven rug | cloth | todo | Flat-woven geometric wool rug |
| 321 | `wall.linoleum` | Linoleum | plastic | todo | Smooth mottled sheet floor |
| 322 | `wall.cork` | Cork board | wood | todo | Tan granular cork |

### pottery (12)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 323 | `pot.terracotta` | Terracotta pot | ceramic | todo | Unglazed orange clay, chalky |
| 324 | `pot.stoneware.brown` | Stoneware, brown glaze | ceramic | todo | Glossy brown glaze with speckles |
| 325 | `pot.stoneware.grey` | Stoneware, grey salt-glazed | ceramic | todo | Orange-peel grey glaze with blue marks |
| 326 | `pot.porcelain.white` | Porcelain, white | ceramic | todo | Bright white glossy, thin and slightly translucent |
| 327 | `pot.porcelain.blue_white` | Porcelain, blue and white | ceramic | todo | Cobalt painted scenes under clear glaze |
| 328 | `pot.enamel.white` | Enamelware, white | ceramic | todo | White enamel with dark rim, chips show black |
| 329 | `pot.enamel.speckled` | Enamelware, speckled blue | ceramic | todo | Blue with white speckles |
| 330 | `pot.earthenware` | Earthenware, glazed | ceramic | todo | Thick creamy glaze, crazed |
| 331 | `pot.crackle` | Crackle glaze | ceramic | todo | Fine dark crack network in glaze |
| 332 | `pot.celadon` | Celadon glaze | ceramic | todo | Soft pale green-grey glass glaze |
| 333 | `pot.lustre` | Lustreware | ceramic | todo | Metallic sheen on glaze |
| 334 | `pot.bisque` | Bisque, unglazed white | ceramic | todo | Matte chalky white |

### stonework (12)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 335 | `stonework.cobble` | Cobblestones | masonry | todo | Rounded stones with deep gaps |
| 336 | `stonework.setts` | Granite setts | masonry | todo | Square grey stones in rows |
| 337 | `stonework.ashlar` | Ashlar, cut stone | masonry | todo | Smooth large cut blocks, thin joints |
| 338 | `stonework.rubble` | Rubble wall | masonry | todo | Irregular stones in thick mortar |
| 339 | `stonework.drystone` | Dry-stone wall | masonry | todo | Flat stones stacked without mortar |
| 340 | `stonework.flagstone` | Flagstone paving | masonry | todo | Large irregular flat slabs |
| 341 | `stonework.river_rock` | River-rock wall | masonry | todo | Rounded grey stones in mortar |
| 342 | `stonework.sandstone` | Sandstone blocks | masonry | todo | Warm layered blocks, soft edges |
| 343 | `stonework.limestone` | Limestone blocks | masonry | todo | Pale cream blocks with fossil specks |
| 344 | `stonework.marble_floor` | Marble floor, chequered | stone | todo | Black and white polished squares |
| 345 | `stonework.terrazzo` | Terrazzo | stone | todo | Polished stone chips in cement |
| 346 | `stonework.mosaic` | Mosaic | ceramic | todo | Small coloured tiles in patterns |

### fish_sea (11)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 347 | `trout.skin` | Trout skin | scales | todo | Silver with pink band and black speckles |
| 348 | `salmon.skin` | Salmon skin, silver | scales | todo | Bright silver, blue-green back |
| 349 | `carp.skin` | Carp skin, gold | scales | todo | Large gold scales with dark edges |
| 350 | `koi.skin` | Koi skin, patched | scales | todo | White with orange and black patches |
| 351 | `pike.skin` | Pike skin | scales | todo | Green with pale bean-shaped spots |
| 352 | `sturgeon.skin` | Sturgeon skin | scales | todo | Grey with bony plates along the sides |
| 353 | `mackerel.skin` | Mackerel skin | scales | todo | Green-blue with wavy black lines |
| 354 | `sardine.skin` | Sardine skin | scales | todo | Mirror-bright silver |
| 355 | `squid.skin` | Squid skin | thin | todo | Translucent cream with red-brown dots |
| 356 | `shrimp.shell` | Shrimp shell | thin | todo | Translucent grey-pink segments |
| 357 | `clam.shell` | Clam shell | keratin | todo | Ridged cream shell with growth rings |

### glass_build (11)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 358 | `glass.window.clear` | Window glass | glass | todo | Clear with faint sky reflection |
| 359 | `glass.window.wavy` | Old window glass | glass | todo | Slightly wavy, distorted view |
| 360 | `glass.window.frosted` | Frosted glass | glass | todo | Milky blurred see-through |
| 361 | `glass.stained.red` | Stained glass, red | glass | todo | Deep ruby glowing when backlit |
| 362 | `glass.stained.blue` | Stained glass, blue | glass | todo | Rich cobalt blue |
| 363 | `glass.stained.amber` | Stained glass, amber | glass | todo | Warm honey yellow |
| 364 | `glass.leaded` | Leaded window | glass | todo | Small panes in dark lead lines |
| 365 | `glass.wired` | Wired safety glass | glass | todo | Clear glass with a wire grid inside |
| 366 | `glass.glass_block` | Glass block | glass | todo | Thick rippled glass bricks |
| 367 | `glass.mirror` | Mirror | metal | todo | Silvered perfect reflection |
| 368 | `glass.mirror_old` | Old mirror | metal | todo | Mirror with dark speckled patches |

### misc_props (11)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 369 | `misc.tobacco` | Tobacco, cut | organic | todo | Shredded brown leaf |
| 370 | `misc.cigar_ash` | Cigar ash | ground | todo | Grey flaky ash |
| 371 | `misc.coal_lump` | Coal lumps | stone | todo | Black shiny lumps |
| 372 | `misc.charcoal` | Charcoal | wood | todo | Matte black with wood grain |
| 373 | `misc.ivory_comb` | Comb, horn | keratin | todo | Translucent brown horn |
| 374 | `misc.feather_duster` | Feather duster | feather | todo | Fluffy grey feathers on a stick |
| 375 | `misc.wicker` | Wicker | wood | todo | Woven pale cane strips |
| 376 | `misc.rattan` | Rattan | wood | todo | Glossy woven cane |
| 377 | `misc.cork` | Cork | wood | todo | Tan spongy granular |
| 378 | `misc.sponge` | Sea sponge | organic | todo | Tan porous soft blob |
| 379 | `misc.leather_strap` | Leather strap | leather | todo | Smooth brown leather strip |

### beads_misc (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 380 | `fit.bead.glass` | Glass beads | glass | todo | Tiny glossy coloured beads |
| 381 | `fit.bead.wood` | Wooden beads | wood | todo | Small rounded polished wood |
| 382 | `fit.quillwork` | Quillwork | keratin | todo | Flattened dyed quills in bands |
| 383 | `fit.ribbon` | Ribbon, satin | cloth | todo | Narrow glossy satin strip |
| 384 | `fit.straw_hat` | Straw, woven hat | cloth | todo | Woven pale straw |
| 385 | `fit.cork_sole` | Cork sole | wood | todo | Tan cork grain |
| 386 | `fit.rubber_sole` | Rubber sole | plastic | todo | Matte black rubber with tread |
| 387 | `fit.celluloid` | Celluloid | plastic | todo | Ivory-like smooth plastic |
| 388 | `fit.clog_wood` | Clog wood | wood | todo | Painted pale smooth wood |
| 389 | `fit.badge_enamel` | Enamel badge | ceramic | todo | Coloured glossy enamel in a metal edge |

### brick (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 390 | `brick.red.common` | Brick, red common | masonry | todo | Uneven red bricks in grey mortar |
| 391 | `brick.red.clean` | Brick, red pressed | masonry | todo | Sharp even red bricks, thin joints |
| 392 | `brick.yellow` | Brick, yellow | masonry | todo | Sandy yellow bricks |
| 393 | `brick.brown` | Brick, brown | masonry | todo | Chocolate-brown bricks, dark mortar |
| 394 | `brick.white_painted` | Brick, painted white | masonry | todo | White paint over brick, bumps showing |
| 395 | `brick.glazed` | Brick, glazed | masonry | todo | Glossy cream bricks |
| 396 | `brick.adobe` | Adobe bricks | masonry | todo | Large straw-flecked earth bricks |
| 397 | `brick.firebrick` | Firebrick | masonry | todo | Pale buff bricks, sooty |
| 398 | `brick.herringbone` | Brick, herringbone path | masonry | todo | Bricks laid in zigzag rows |
| 399 | `brick.basket` | Brick, basket weave | masonry | todo | Pairs of bricks alternating direction |

### crystals (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 400 | `gem.ruby` | Ruby | gem | todo | Deep red faceted stone with bright sparkle |
| 401 | `gem.sapphire` | Sapphire | gem | todo | Rich blue facets |
| 402 | `gem.emerald` | Emerald | gem | todo | Grassy green with soft inclusions |
| 403 | `gem.diamond` | Diamond | gem | todo | Clear with rainbow fire |
| 404 | `gem.amethyst` | Amethyst cluster | gem | todo | Purple pointed crystals in a geode |
| 405 | `gem.quartz_clear` | Quartz crystal | gem | todo | Clear six-sided points |
| 406 | `gem.amber` | Amber | gem | todo | Honey-coloured with tiny bubbles |
| 407 | `gem.jade` | Jade | gem | todo | Milky green, waxy polish |
| 408 | `gem.turquoise` | Turquoise | stone | todo | Sky blue with dark veins |
| 409 | `gem.pearl` | Pearl | keratin | todo | Soft white orient sheen |

### fittings (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 410 | `fit.button.brass` | Button, brass | metal | todo | Polished brass dome |
| 411 | `fit.button.horn` | Button, horn | keratin | todo | Mottled brown polished horn |
| 412 | `fit.button.shell` | Button, shell | keratin | todo | Pearly white shimmer |
| 413 | `fit.button.bone` | Button, bone | keratin | todo | Off-white polished bone |
| 414 | `fit.buckle.silver` | Buckle, silver | metal | todo | Bright engraved silver |
| 415 | `fit.buckle.iron` | Buckle, iron | metal | todo | Dark plain iron |
| 416 | `fit.zip` | Zip, metal | metal | todo | Small interlocking brass teeth |
| 417 | `fit.rivet.copper` | Rivet, copper | metal | todo | Small domed copper |
| 418 | `fit.spur` | Spur, steel | metal | todo | Brushed steel with a spiked wheel |
| 419 | `fit.concho` | Concho, silver | metal | todo | Stamped silver disc |

### kitchen_metal (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 420 | `kitchen.cast_iron_pan` | Cast-iron pan, seasoned | metal | todo | Black satin with a smooth cooking surface |
| 421 | `kitchen.copper_pot` | Copper pot, tinned | metal | todo | Polished copper outside, silver tin inside |
| 422 | `kitchen.tin_cup` | Tin cup | metal | todo | Dull dented grey tin |
| 423 | `kitchen.stainless` | Stainless steel | metal | todo | Satin steel with smudges |
| 424 | `kitchen.silverware` | Silverware, plated | metal | todo | Bright plating worn at the edges |
| 425 | `kitchen.cast_stove` | Cast-iron stove | metal | todo | Black iron with nickel trim |
| 426 | `kitchen.enamel_stove` | Enamelled stove | ceramic | todo | Cream enamel with chrome handles |
| 427 | `kitchen.coffee_grinder` | Brass and iron grinder | paint | todo | Painted iron with a brass hopper |
| 428 | `kitchen.lantern` | Tin lantern | metal | todo | Punched tin with light leaking out |
| 429 | `kitchen.oil_can` | Painted oil can | paint | todo | Red paint over tin, dented |

### livestock (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 430 | `cattle.coat.black` | Cattle hide, black | fur | todo | Short dense black hair, slight sheen |
| 431 | `cattle.coat.speckled` | Cattle hide, speckled longhorn | fur | todo | Mottled red, white and brown speckles |
| 432 | `sheep.wool.white` | Sheep wool, white | fur | todo | Thick crimped cream-white curls |
| 433 | `sheep.wool.black` | Sheep wool, black | fur | todo | Dark brown-black crimped curls |
| 434 | `sheep.wool.brown` | Sheep wool, brown | fur | todo | Warm brown crimped fleece |
| 435 | `sheep.wool.grey` | Sheep wool, grey | fur | todo | Mixed grey and white curls |
| 436 | `sheep.skin.shorn` | Sheep, freshly shorn | fur | todo | Very short pale fleece showing pink skin |
| 437 | `pig.skin.pink` | Pig skin, pink | hide | todo | Pink skin with sparse pale bristles |
| 438 | `pig.skin.black` | Pig skin, black | hide | todo | Dark grey skin, black bristles |
| 439 | `pig.skin.spotted` | Pig skin, spotted | hide | todo | Pink skin with black blotches |

### metalwork (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 440 | `build.iron.railing` | Painted iron railing | paint | todo | Black paint over iron |
| 441 | `build.iron.rivet_plate` | Riveted iron plate | metal | todo | Iron plates with rows of rivets |
| 442 | `build.steel.girder` | Steel girder | paint | todo | Painted steel with rivets |
| 443 | `build.metal.painted_crate` | Painted metal | paint | todo | Painted sheet with chipped edges |
| 444 | `build.copper.roof` | Copper roof | metal | todo | Sheet copper with seams |
| 445 | `build.lead.flashing` | Lead flashing | metal | todo | Soft dull grey sheet with creases |
| 446 | `build.grate` | Steel grating | metal | todo | Open grid of steel bars |
| 447 | `build.diamond_plate` | Diamond plate | metal | todo | Raised diamond pattern on steel |
| 448 | `build.chain_link` | Chain-link fence | metal | todo | Woven wire diamonds |
| 449 | `build.mesh` | Wire mesh | metal | todo | Fine hexagonal wire |

### toys_plastic (10)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 450 | `plastic.glossy.red` | Plastic, glossy | plastic | todo | Bright smooth glossy plastic |
| 451 | `plastic.matte.grey` | Plastic, matte | plastic | todo | Dull fine-texture plastic |
| 452 | `plastic.clear` | Plastic, clear | glass | todo | Clear with soft edges |
| 453 | `plastic.rubber.black` | Rubber, black | plastic | todo | Matte black, soft highlight |
| 454 | `plastic.rubber.red` | Rubber, red | plastic | todo | Soft red matte rubber |
| 455 | `plastic.rubber_duck` | Toy rubber, yellow | plastic | todo | Glossy yellow vinyl |
| 456 | `plastic.bakelite` | Early phenolic plastic | plastic | todo | Glossy dark brown mottled plastic |
| 457 | `plastic.toy_brick` | Toy brick plastic | plastic | todo | Very glossy saturated colour |
| 458 | `plastic.foam` | Foam | plastic | todo | Soft open-cell pale surface |
| 459 | `plastic.marble_toy` | Glass marble | glass | todo | Clear sphere with a coloured twist inside |

### crops (9)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 460 | `crop.wheat.ripe` | Wheat, ripe | foliage | todo | Golden stalks with heavy heads |
| 461 | `crop.wheat.green` | Wheat, green | foliage | todo | Soft green shoots |
| 462 | `crop.corn` | Maize | foliage | todo | Tall stalks with long leaves |
| 463 | `crop.cotton` | Cotton plant | foliage | todo | White fluffy bolls on dry stems |
| 464 | `crop.tobacco` | Tobacco leaf | foliage | todo | Broad leaves drying to brown |
| 465 | `crop.hay` | Hay | foliage | todo | Dry pale green-gold strands |
| 466 | `crop.straw` | Straw | foliage | todo | Hollow golden stalks |
| 467 | `crop.sugarcane` | Sugar cane | foliage | todo | Tall jointed green-purple stems |
| 468 | `crop.rice` | Rice paddy | foliage | todo | Bright green shoots in water |

### glassware (9)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 469 | `glass.bottle.green` | Bottle glass, green | glass | todo | Thick green glass, darker at edges |
| 470 | `glass.bottle.brown` | Bottle glass, brown | glass | todo | Dark amber glass |
| 471 | `glass.bottle.clear` | Bottle glass, clear | glass | todo | Clear with faint green edge |
| 472 | `glass.bottle.cobalt` | Bottle glass, cobalt | glass | todo | Deep cobalt blue |
| 473 | `glass.milk` | Milk glass | glass | todo | Opaque soft white glass |
| 474 | `glass.lamp_chimney` | Lamp chimney | glass | todo | Thin clear glass, sooty at the top |
| 475 | `glass.jar.mason` | Preserving jar | glass | todo | Slightly blue-green moulded glass |
| 476 | `glass.frosted_bulb` | Frosted bulb | glass | todo | Milky glass glowing from inside |
| 477 | `glass.vaseline` | Glowing uranium glass | glass | todo | Yellow-green glass, glows under black light |

### twills (9)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 478 | `cloth.denim.indigo` | Denim, indigo | cloth | todo | Diagonal twill, blue face, white back |
| 479 | `cloth.denim.faded` | Denim, faded | cloth | todo | Pale washed blue with whitened seams |
| 480 | `cloth.denim.black` | Denim, black | cloth | todo | Charcoal twill, faded at folds |
| 481 | `cloth.gabardine` | Gabardine | cloth | todo | Tight smooth twill, slight sheen |
| 482 | `cloth.drill.khaki` | Khaki drill | cloth | todo | Sand-coloured cotton twill |
| 483 | `cloth.serge.navy` | Serge, navy | cloth | todo | Navy wool twill with a fine rib |
| 484 | `cloth.tweed.brown` | Tweed, brown | cloth | todo | Rough flecked brown wool |
| 485 | `cloth.tweed.herringbone` | Tweed, herringbone | cloth | todo | Zigzag grey wool twill |
| 486 | `cloth.houndstooth` | Houndstooth | cloth | todo | Broken black and white jagged check |

### fire (8)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 487 | `fire.flame.wood` | Flame, wood fire | fire | todo | Orange-yellow tongues, blue base |
| 488 | `fire.flame.candle` | Flame, candle | fire | todo | Small calm teardrop flame |
| 489 | `fire.flame.gas` | Flame, gas burner | fire | todo | Steady blue cone |
| 490 | `fire.flame.torch` | Flame, oil torch | fire | todo | Big smoky orange flame |
| 491 | `fire.embers` | Embers | fire | todo | Black lumps with shifting orange cracks |
| 492 | `fire.lava` | Lava, molten | fire | todo | Bright orange flow with black crust |
| 493 | `fire.smoke` | Smoke | fire | todo | Soft grey drifting cloud, lit from below |
| 494 | `fire.steam` | Steam | fire | todo | White wisps fading quickly |

### paper_books (8)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 495 | `paper.newsprint` | Newsprint | thin | todo | Grey-cream thin paper with printed columns |
| 496 | `paper.parchment` | Parchment | thin | todo | Yellowed translucent sheet |
| 497 | `paper.kraft` | Brown paper | thin | todo | Brown fibrous paper |
| 498 | `paper.cardboard` | Cardboard | thin | todo | Brown board with ridged edges |
| 499 | `paper.book_cloth` | Book cloth cover | cloth | todo | Woven cloth over board, gold title |
| 500 | `paper.book_leather` | Leather book cover | leather | todo | Worn leather with gilt edges |
| 501 | `paper.banknote` | Paper money | thin | todo | Engraved green and black on soft paper |
| 502 | `paper.wanted_poster` | Printed poster | thin | todo | Bold black type on cheap paper |

### roofing (8)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 503 | `roof.tile.terracotta` | Roof tiles, terracotta | ceramic | todo | Curved orange clay tiles |
| 504 | `roof.tile.slate` | Slate roof | masonry | todo | Thin overlapping blue-grey slates |
| 505 | `roof.tile.flat_clay` | Roof tiles, flat clay | ceramic | todo | Small flat red-brown tiles |
| 506 | `roof.tin_corrugated` | Corrugated tin | metal | todo | Wavy galvanised sheet |
| 507 | `roof.tin_standing` | Standing-seam metal roof | metal | todo | Painted sheets with raised seams |
| 508 | `roof.thatch` | Thatch | foliage | todo | Thick bundled golden straw |
| 509 | `roof.tar_paper` | Tar paper | plastic | todo | Black gritty sheet with seams |
| 510 | `roof.sod` | Sod roof | foliage | todo | Grass growing over earth |

### snow_ice_ground (8)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 511 | `ground.snow.fresh` | Snow, fresh powder | ice | todo | Soft bright white with blue shadows and sparkle |
| 512 | `ground.snow.packed` | Snow, packed | ice | todo | Compressed slightly grey with tracks |
| 513 | `ground.snow.crust` | Snow, wind crust | ice | todo | Shiny ridged crust |
| 514 | `ground.snow.slush` | Slush | ice | todo | Grey wet translucent heaps |
| 515 | `ground.snow.dirty` | Snow, roadside grey | ice | todo | Grey-brown speckled old snow |
| 516 | `ground.ice.lake` | Lake ice | ice | todo | Dark clear ice with white cracks and bubbles |
| 517 | `ground.ice.glacier` | Glacier ice | ice | todo | Deep blue compressed ice |
| 518 | `ground.permafrost` | Frozen ground | ground | todo | Hard grey earth with frost crystals |

### tiles (8)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 519 | `tile.glazed.white` | Glazed tiles, white | ceramic | todo | Glossy squares in grey grout |
| 520 | `tile.glazed.green` | Glazed tiles, green | ceramic | todo | Deep green crackled glaze |
| 521 | `tile.subway` | Tiles, brick-pattern | ceramic | todo | Bevelled white rectangles |
| 522 | `tile.encaustic` | Patterned floor tiles | ceramic | todo | Matte tiles with coloured patterns |
| 523 | `tile.quarry` | Quarry tiles | ceramic | todo | Matte red-brown squares |
| 524 | `tile.hex` | Hexagon tiles | ceramic | todo | Small white hexagons |
| 525 | `tile.blue_painted` | Painted tiles, blue on white | ceramic | todo | Hand-painted blue motifs on white glaze |
| 526 | `tile.mexican` | Painted tiles, bright | ceramic | todo | Bright yellow and blue patterns |

### blades_tools (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 527 | `tool.blade.edge` | Sharpened edge | metal | todo | Bright bevel line on a dull blade |
| 528 | `tool.saw_blade` | Saw blade | metal | todo | Thin bright steel with etched teeth |
| 529 | `tool.file` | File, steel | metal | todo | Dark steel with fine diagonal teeth |
| 530 | `tool.anvil` | Anvil face | metal | todo | Polished face over dark rough iron |
| 531 | `tool.wire_barbed` | Barbed wire | metal | todo | Galvanised grey twisted wire |
| 532 | `tool.whetstone` | Whetstone | stone | todo | Fine grey stone, oily centre |
| 533 | `tool.grindstone` | Grindstone | stone | todo | Coarse buff sandstone |

### eyes (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 534 | `eye.brown_dark` | Eyes, dark brown | eye | todo | Deep brown iris, wet cornea shine |
| 535 | `eye.brown` | Eyes, brown | eye | todo | Warm mid-brown iris |
| 536 | `eye.hazel` | Eyes, hazel | eye | todo | Green outer ring, gold-brown centre |
| 537 | `eye.amber` | Eyes, amber | eye | todo | Even golden-amber iris |
| 538 | `eye.grey` | Eyes, grey | eye | todo | Pale grey iris, dark limbal ring |
| 539 | `eye.blue` | Eyes, blue | eye | todo | Clear mid-blue fibrous iris |
| 540 | `eye.blue_pale` | Eyes, pale blue | eye | todo | Very pale blue, almost grey |

### gravel (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 541 | `ground.gravel.road` | Gravel road | ground | todo | Grey angular gravel |
| 542 | `ground.gravel.pea` | Pea gravel | ground | todo | Small rounded mixed pebbles |
| 543 | `ground.gravel.shingle` | Shingle beach | ground | todo | Rounded grey and white pebbles |
| 544 | `ground.gravel.scree` | Scree | ground | todo | Sharp broken rock fragments on a slope |
| 545 | `ground.gravel.railway` | Railway ballast | ground | todo | Dark angular stone chips |
| 546 | `ground.gravel.cinders` | Cinders | ground | todo | Black porous lumps |
| 547 | `ground.gravel.creek` | Creek bed | ground | todo | Round wet stones in clear shallows |

### paint (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 548 | `car.paint.solid_red` | Car paint, solid red | paint | todo | Deep glossy red under clear coat |
| 549 | `car.paint.metallic_silver` | Car paint, metallic silver | paint | todo | Fine metal flakes sparkle under clear coat |
| 550 | `car.paint.metallic_blue` | Car paint, metallic blue | paint | todo | Deep blue with sparkling flakes |
| 551 | `car.paint.pearl_white` | Car paint, pearl white | paint | todo | White with a soft colour shift |
| 552 | `car.paint.colour_shift` | Car paint, colour-shift | paint | todo | Shifts purple to green with angle |
| 553 | `car.paint.matte_black` | Car paint, matte black | paint | todo | Flat black, no clear coat |
| 554 | `car.paint.primer` | Primer | paint | todo | Flat grey, slightly rough |

### rubber_glass (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 555 | `car.tyre.tread` | Tyre tread | plastic | todo | Matte black rubber with deep tread |
| 556 | `car.dashboard` | Dashboard plastic | plastic | todo | Grained soft-touch dark plastic |
| 557 | `car.carbon` | Carbon fibre panel | paint | todo | Woven black check under clear resin |
| 558 | `car.windscreen` | Windscreen glass | glass | todo | Clear glass with a green edge tint |
| 559 | `car.wood.wagon` | Wagon wood, painted | wood | todo | Red or green painted boards, worn |
| 560 | `car.canvas.cover` | Wagon canvas cover | cloth | todo | Stretched off-white canvas over hoops |
| 561 | `car.leather.seat` | Saddle-leather seat | leather | todo | Tufted brown leather |

### wild_mammals (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 562 | `bear.coat.brown` | Bear fur, brown | fur | todo | Long coarse dark brown fur |
| 563 | `wolf.coat.grey` | Wolf fur, grey | fur | todo | Grey agouti with tan legs and pale belly |
| 564 | `fox.coat.red` | Fox fur, red | fur | todo | Bright orange-red, white chest, black socks |
| 565 | `tiger.coat` | Tiger fur | fur | todo | Orange with black vertical stripes, white belly |
| 566 | `leopard.coat` | Leopard fur, rosettes | fur | todo | Gold with black rosette spots |
| 567 | `cheetah.coat` | Cheetah fur, spotted | fur | todo | Pale gold with solid round spots |
| 568 | `giraffe.coat` | Giraffe coat | fur | todo | Brown patches split by cream lines |

### wool_felt (7)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 569 | `cloth.wool.homespun` | Wool, homespun | cloth | todo | Uneven heavy wool, fuzzy surface |
| 570 | `cloth.wool.fine_black` | Wool, fine black | cloth | todo | Dense smooth black, soft sheen |
| 571 | `cloth.wool.red` | Wool, red | cloth | todo | Rich red dense wool |
| 572 | `cloth.wool.blanket` | Wool blanket, striped | cloth | todo | Thick fuzzy wool with bold stripes |
| 573 | `cloth.wool.boiled` | Boiled wool | cloth | todo | Dense felted surface, no visible weave |
| 574 | `cloth.felt.hat` | Felt, hat | cloth | todo | Smooth dense brushed felt, slight sheen |
| 575 | `cloth.felt.craft` | Felt, craft | cloth | todo | Fuzzy flat felt with stray fibres |

### dogs_cats (6)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 576 | `dog.coat.tan` | Dog coat, tan short | fur | todo | Short smooth tan coat |
| 577 | `dog.coat.merle` | Dog coat, merle | fur | todo | Grey-blue marbled with black patches |
| 578 | `dog.coat.curly_brown` | Dog coat, curly brown | fur | todo | Tight brown curls |
| 579 | `cat.coat.tabby_brown` | Cat coat, brown tabby | fur | todo | Brown with dark stripes and an M on the brow |
| 580 | `cat.coat.calico` | Cat coat, calico | fur | todo | White with orange and black patches |
| 581 | `cat.coat.tortoiseshell` | Cat coat, tortoiseshell | fur | todo | Mottled black and orange |

### grips_parts (6)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 582 | `grip.ivory` | Grip, ivory | keratin | todo | Cream with fine grain |
| 583 | `grip.pearl` | Grip, mother-of-pearl | keratin | todo | Rainbow pearly shimmer |
| 584 | `grip.stag` | Grip, stag horn | keratin | todo | Knobbly brown and cream antler |
| 585 | `grip.bakelite` | Grip, early plastic | plastic | todo | Glossy dark brown moulded plastic |
| 586 | `grip.leather_wrap` | Grip, leather wrap | leather | todo | Spiral-wrapped dark leather |
| 587 | `grip.cord_wrap` | Grip, cord wrap | cloth | todo | Wound cord in a diamond pattern |

### insects (6)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 588 | `beetle.black` | Beetle shell, black | keratin | todo | Glossy black hard shell |
| 589 | `beetle.green` | Beetle shell, iridescent green | keratin | todo | Metallic green-gold shell |
| 590 | `ladybird.shell` | Ladybird shell | keratin | todo | Red with black spots, high gloss |
| 591 | `butterfly.orange` | Butterfly wing, orange | thin | todo | Orange with black veins and white spots |
| 592 | `butterfly.blue` | Butterfly wing, blue | thin | todo | Shimmering electric blue |
| 593 | `snail.shell` | Snail shell | keratin | todo | Brown spiral with cream bands |

### needles (6)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 594 | `needle.pine` | Needles, pine | foliage | todo | Long clustered dark green needles |
| 595 | `needle.spruce` | Needles, spruce | foliage | todo | Short dense blue-green needles |
| 596 | `needle.fir` | Needles, fir | foliage | todo | Flat soft dark green needles |
| 597 | `needle.cedar` | Foliage, cedar | foliage | todo | Flat scaly sprays |
| 598 | `needle.juniper` | Juniper with berries | foliage | todo | Prickly grey-green with blue berries |
| 599 | `needle.dead` | Needles, dead rust | foliage | todo | Orange-brown dry needles |

### sand (6)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 600 | `ground.sand.beach` | Sand, beach | ground | todo | Fine pale gold sand with ripples |
| 601 | `ground.sand.desert` | Sand, desert dune | ground | todo | Wind-rippled orange sand |
| 602 | `ground.sand.white` | Sand, white | ground | todo | Very pale fine sand |
| 603 | `ground.sand.black` | Sand, black | ground | todo | Glittering dark sand |
| 604 | `ground.sand.wet` | Sand, tide-packed | ground | todo | Dark firm sand with a sheen |
| 605 | `ground.sand.river` | River sand | ground | todo | Coarse grey-brown sand with pebbles |

### wood_handles (6)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 606 | `wood.walnut.stock` | Walnut, oiled stock | wood | todo | Dark brown with swirling grain and oil sheen |
| 607 | `wood.maple.curly` | Maple, curly | wood | todo | Golden wood with rippling bars of light |
| 608 | `wood.ash.handle` | Ash, tool handle | wood | todo | Pale straight grain, smooth from use |
| 609 | `wood.hickory.handle` | Hickory, handle | wood | todo | Light-brown tough straight grain |
| 610 | `wood.ebony` | Ebony | wood | todo | Near-black fine grain, polished |
| 611 | `wood.rosewood` | Rosewood | wood | todo | Red-brown with dark streaks |

### armour_misc (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 612 | `armour.plate` | Armour plate | metal | todo | Polished steel with hammer ripples |
| 613 | `armour.scale` | Scale armour | metal | todo | Overlapping small metal plates |
| 614 | `armour.leather_hard` | Hardened leather | leather | todo | Stiff glossy dark leather |
| 615 | `armour.kevlar` | Woven aramid | cloth | todo | Tight yellow woven fibre |
| 616 | `armour.carbon` | Carbon fibre | paint | todo | Woven black check under clear resin |

### glow (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 617 | `glow.lamp.warm` | Lamp glow, warm | glow | todo | Warm orange light from inside the shade |
| 618 | `glow.bulb.white` | Light bulb, white | glow | todo | Bright white-yellow filament glow |
| 619 | `glow.neon.red` | Neon tube, red | glow | todo | Saturated red tube with a soft halo |
| 620 | `glow.neon.blue` | Neon tube, blue | glow | todo | Icy blue tube with halo |
| 621 | `glow.screen` | Screen glow | glow | todo | Even cool light from a flat panel |

### horse_parts (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 622 | `horse.hoof.dark` | Horse hoof, dark | keratin | todo | Dark grey-black horn with fine vertical lines |
| 623 | `horse.hoof.pale` | Horse hoof, pale | keratin | todo | Cream horn, slightly translucent at edges |
| 624 | `horse.hoof.striped` | Horse hoof, striped | keratin | todo | Dark and pale vertical bands |
| 625 | `horse.muzzle.dark` | Horse muzzle skin, dark | hide | todo | Velvety dark grey skin with sparse whiskers |
| 626 | `horse.muzzle.pink` | Horse muzzle skin, pink | hide | todo | Soft pink skin with grey freckles |

### ice (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 627 | `ice.cube` | Ice, clear block | ice | todo | Clear with cloudy centre and cracks |
| 628 | `ice.icicle` | Icicles | ice | todo | Clear rippled spikes |
| 629 | `ice.frost` | Frost | ice | todo | Feathery white crystals on surfaces |
| 630 | `ice.sea_ice` | Sea ice | ice | todo | White slabs with blue edges |
| 631 | `ice.snowball` | Snow, packed ball | ice | todo | Dense white with grainy surface |

### magic (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 632 | `magic.rune` | Rune glow | glow | todo | Carved lines glowing soft blue |
| 633 | `magic.portal` | Portal swirl | glow | todo | Swirling purple light disc |
| 634 | `magic.cursed` | Dark cursed aura | fire | todo | Black smoke with violet edges |
| 635 | `magic.holy` | Holy light | glow | todo | Warm white-gold radiance |
| 636 | `magic.slime` | Slime | organic | todo | Glossy translucent green jelly |

### synthetics (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 637 | `cloth.nylon` | Nylon | cloth | todo | Thin smooth shiny fabric |
| 638 | `cloth.vinyl` | Vinyl, glossy | plastic | todo | Very glossy plastic sheet with creases |
| 639 | `cloth.rubberised` | Rubberised cloth | plastic | todo | Matte rubber coating over cloth |
| 640 | `cloth.sequins` | Sequins | metal | todo | Tiny overlapping mirror discs |
| 641 | `cloth.hi_vis` | High-visibility | cloth | todo | Fluorescent yellow with reflective strips |

### teeth_nails (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 642 | `teeth.healthy` | Teeth, healthy | keratin | todo | Off-white enamel, slight translucency at the edge |
| 643 | `teeth.stained` | Teeth, stained | keratin | todo | Yellow-brown stains near the gums |
| 644 | `teeth.gold` | Tooth, gold | metal | todo | Bright polished gold cap |
| 645 | `nails.natural` | Nails, natural | keratin | todo | Pink-tinted keratin with pale tips |
| 646 | `nails.painted` | Nails, painted | paint | todo | Glossy red lacquer |

### wax_soap (5)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 647 | `wax.candle.white` | Candle wax, white | thin | todo | Creamy wax, warm glow near the flame |
| 648 | `wax.candle.beeswax` | Beeswax | thin | todo | Warm yellow wax with honeycomb texture |
| 649 | `wax.candle.red` | Candle wax, red | thin | todo | Deep red wax, glossy drips |
| 650 | `wax.soap` | Soap bar | thin | todo | Pale smooth slightly translucent bar |
| 651 | `wax.lampshade` | Fabric lampshade | thin | todo | Pleated cloth glowing from the bulb |

### ammo_powder (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 652 | `ammo.cartridge.brass` | Cartridge brass | metal | todo | Bright yellow brass with a copper tip |
| 653 | `ammo.shotgun_shell` | Shotgun shell | plastic | todo | Red paper tube with a brass base |
| 654 | `ammo.powder` | Black powder | ground | todo | Dull grainy black |
| 655 | `ammo.lead_ball` | Lead ball | metal | todo | Dull grey sphere |

### animal_parts (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 656 | `antler.bone` | Antler, bone | keratin | todo | Ridged pale brown bone, polished tips |
| 657 | `horn.cattle` | Horn, cattle | keratin | todo | Cream fading to dark tips, polished |
| 658 | `bone.fresh` | Bone, fresh | keratin | todo | Off-white bone with pinkish tint |
| 659 | `turtle.shell` | Turtle shell, patterned | keratin | todo | Plates with radiating brown and amber bands |

### hair (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 660 | `hair.black` | Hair, black | hair | todo | Blue-black strands with a strong highlight |
| 661 | `hair.blond` | Hair, blond | hair | todo | Golden yellow, bright highlight |
| 662 | `hair.ginger` | Hair, ginger | hair | todo | Bright orange-red |
| 663 | `hair.grey` | Hair, grey | hair | todo | Mixed grey and white strands |

### hides (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 664 | `elephant.hide` | Elephant hide | hide | todo | Deeply wrinkled dry grey skin |
| 665 | `rhino.hide` | Rhinoceros hide | hide | todo | Thick grey plates and folds |
| 666 | `walrus.hide` | Walrus hide | hide | todo | Lumpy cinnamon-pink skin with sparse bristles |
| 667 | `armadillo.shell` | Armadillo shell | keratin | todo | Leathery banded plates, tan grey |

### knits (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 668 | `cloth.knit.jersey` | Jersey knit | cloth | todo | Fine stretchy knit, small loops |
| 669 | `cloth.knit.chunky` | Chunky knit | cloth | todo | Big rope-like cable loops |
| 670 | `cloth.knit.ribbed` | Ribbed knit | cloth | todo | Vertical ridges of knit loops |
| 671 | `cloth.knit.socks` | Wool socks | cloth | todo | Thick soft knit, darned heels |

### metal_trim (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 672 | `car.alloy.wheel` | Alloy wheel | metal | todo | Bright machined aluminium with brake dust |
| 673 | `car.steel.wheel` | Steel wheel, painted | paint | todo | Black-painted pressed steel |
| 674 | `car.engine.block` | Engine block | metal | todo | Grimy grey cast metal |
| 675 | `car.iron.wheel_rim` | Iron wagon tyre | metal | todo | Dark iron band, worn bright |

### upholstery (4)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 676 | `uph.leather_button` | Buttoned leather seat | leather | todo | Deep-buttoned oxblood leather |
| 677 | `uph.horsehair` | Horsehair cloth | cloth | todo | Stiff glossy black weave |
| 678 | `uph.chintz` | Chintz | cloth | todo | Glazed floral cotton, light sheen |
| 679 | `uph.cushion_velvet` | Velvet cushion | cloth | todo | Soft plush pile |

### jewellery (3)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 680 | `jewel.garnet` | Garnet, cut | gem | todo | Dark wine-red facets with sharp sparkle |
| 681 | `jewel.jet` | Jet bead | stone | todo | Polished deep black, warm to the eye |
| 682 | `jewel.coral_bead` | Coral bead | stone | todo | Polished orange-red with fine pores |

### scifi (3)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 683 | `sf.chitin` | Alien chitin | keratin | todo | Dark iridescent armour plates |
| 684 | `sf.holo` | Hologram | glow | todo | Flickering blue see-through lines |
| 685 | `sf.energy_core` | Energy core | glow | todo | Bright cyan glow behind glass |

### water (3)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 686 | `water.pool.clear` | Water, clear pool | water | todo | Clear blue-green with sky reflection and caustics |
| 687 | `water.pond.murky` | Water, murky pond | water | todo | Brown-green, sun fades fast with depth |
| 688 | `water.river.flowing` | Water, flowing river | water | todo | Moving ripples with white streaks |

### fungi_mosses (2)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 689 | `fungus.cap_red` | Mushroom, red spotted | organic | todo | Red cap with white spots |
| 690 | `fungus.cap_brown` | Mushroom, brown | organic | todo | Smooth brown cap, pale gills |

### skin_traits (2)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 691 | `skin.trait.freckled` | Skin trait, freckles | skin | todo | Scattered small warm-brown dots over any tone |
| 692 | `skin.trait.elder` | Skin trait, elder | skin | todo | Thin skin, fine wrinkles, age spots, less scatter |

### flowers (1)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 693 | `flower.rose.red` | Rose petals, red | thin | todo | Velvety deep red petals |

### fur_trims (1)

| # | Material | Name | Recipe | Status | Note |
|---|---|---|---|---|---|
| 694 | `trim.fur.sheepskin` | Sheepskin | fur | todo | Cream curly wool on a suede back |
