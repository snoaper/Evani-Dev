# Characters by code: how Evani.Model builds detailed game characters

Plan, 2026-10-16. Our own design. Chris asked on 2026-10-16: "ok but we need to be able to build these models with code", and on the prior-art card chose **"Build fresh"**: design and build our own approach. The prior-art report in `.vibedeck/research/2026-10-16-building-detailed-game-characters-by-code-evani-mo.md` was background only. Nothing here is based on its findings: no external body model (no Anny, MakeHuman, MPFB or SMPL), no LL3M, GarmentCode or Infinigen code or assets, and no code copied from anywhere. The targets are Chris's own Meshy models (docs/stage3/source-models.md); they are reference targets for measuring only, and nothing of their mesh, textures or UVs ever enters Evani.Model's output.

## 1. Where this extends the brief (said plainly)

The brief (docs/EVANI_MODEL_BRIEF.md) says Evani.Model makes "shape and surface relief ... No colour, materials, skinning or animation", lists "materials and colour, skinning, animation, native detailed organic hero characters" under "Not expected yet", and expects detailed organic hero characters to come from optional plug-in generators. Chris's request of 2026-10-16 changes that for characters. This plan therefore adds, beyond the brief:

1. **Colour and materials** (region colours in MC1; procedural base colour, normal and roughness maps in MC3).
2. **Skinning**: a skeleton, skin weights and rig anchors in the glTF (MC1 skeleton and weights; deformation tests in MC2).
3. **Native detailed organic characters** built by our own generators, not by plug-in services.
4. **Close-up faces** (Chris, 2026-10-16, confirmed; see section 4): real face features as geometry in the high and ultra game tiers (MC5), and a separate `closeup` cinematic tier with eye, jaw and eyelid joints and expression-ready faces (MC6).

Animation itself stays out: Evani.Model delivers a rig-ready model in a T-pose; Stage 3 (S3-16) animates it. Everything else in the brief holds: dev-only, no engine crate in Evani.Model and no Evani.Model crate in the engine, the contract through glTF plus `extras.evani`, determinism, our own code, CPU path first. These additions need Chris's approval as part of the milestones below; none is ticked by an agent.

## 2. The character template: layered parts around a skeleton we define

A character is one class template (`character`, data file `crates/evani-model/data/classes/character.json`) built in three layers, each a set of ordinary Evani.Model parts (generators, anchors, relationships) with three additions: a **skeleton**, a **layer** per part and a **region** per part.

**Skeleton first.** The template declares named joints in a T-pose, positioned by formulas over the class measures (height, head size, shoulder width, arm length, leg length, ...), exactly as parts are. Joint names are fixed for the class so a game can animate any character it makes:

`root, hips, spine, chest, neck, head, clavicle_l, upperarm_l, forearm_l, hand_l, thigh_l, shin_l, foot_l, toe_l` and the `_r` mirror images. `_l` is the character's left, which is +x (the model faces +z). Fingers come later (MC2), as a hand variant.

**Body regions** (layer `body`) are parts placed along the bones: pelvis, torso, neck, head, upper and lower arms, hands, thighs, shins, feet. Each declares the bone it belongs to (`"bone": "forearm_{side}"` for a mirrored pair). Body parts smooth-blend with each other with the body's blend width (a few centimetres, soft joints).

**Garments** (layer `garment`) are **shells over the body**: a new `layer` generator takes the fields of named body parts, offsets them outward by a thickness (the shell is the body field minus the thickness) and clips the result by bands written in the template's own words: hem height, sleeve end, collar line, an open front. A coat is "torso, pelvis and upper arms, 2 cm thick, from the collar to the hem at 45% of the leg, sleeves to the wrist, open at the front below the belt". Garments blend with the body with a small width (sharp seams and hems), so a hem or a cuff reads as an edge, not a smear. Because the garment is defined over the body, a longer arm or a broader chest moves the coat with it; nothing is placed twice.

The offset is exact over a single body part but only approximate near smooth blends (armpits, crotch, elbows), where the blended field is no longer a true distance: the shell can be a little thicker or thinner there. Each compile samples the garment's thickness at its named check points (shoulders, armpits, elbows) and reports the spread; anything outside ±30% of the target thickness is flagged.

**Loose garments drape, they do not wrap.** A plain offset of the legs would wrap each thigh separately below the pelvis, like trousers. A garment can therefore declare a **drape** below a height: there the shell is built from one rounded loft (a flared cone or hull) around all the covered parts instead of each part's offset, so a coat skirt hangs as one panel around both legs with a gap to them, a slight flare and an optional back split. Sleeves can flare the same way below the elbow. The Marshal's coat skirt is the first user, and the compare tool reports the front-view IoU in the hem band so the drape is measured, not assumed.

**Accessories** (layer `accessory`) are ordinary generators attached at body anchors: the hat (a lathe crown with a dent and a brim, attached at `head.top`, with its own brim width and tilt), the belt (a band over the pelvis), the buckle (a rounded box at `belt.front`), the boots (a shell over shin and foot with a sole and a heel), the bandana (a shell over the neck band plus a triangle at the chest), a holster, a badge. Accessories join with an almost hard blend.

**Layered blends.** A solid's field is built per layer: `body` parts smooth-unioned with the body width; that result smooth-unioned with the garments at the garment width; then the accessories at theirs. Templates without layers keep M1's single blend, byte for byte.

**Proportions** are measures, as in every class: total height, head height (in "heads"), shoulder width, chest depth, arm length, leg length, hand and foot size, plus garment and accessory measures (brim width, crown height, coat length, boot height, belt height, hair length, beard length). Adjectives map onto them through the style profile, and `fit_box` fits the whole figure. Ranges come from our own proportion rules (a realistic adult about 7.5 heads tall; a heroic one about 8; a chibi 2 to 3), not from any scanned body.

## 3. Small repeated detail, and how it survives each tier

Fringes, stitches, hair strands and beard tufts are **pattern parts**: one spec, many copies, laid out along a curve of the part graph (a sleeve's underside, a coat's hem, a yoke line across the back, a scalp patch), with seeded jitter in length, angle and spacing, the same on every machine.

- **Fringe row**: thin flat strips hanging from an edge, count and length as measures (`fringe.count`, `fringe.length`), jittered.
- **Hair**: a scalp cap plus tufts (rounded cones) seeded over the back and sides of the head and flowing downwards; long hair is tufts that reach the shoulders. **Beard**: a jaw mass plus a moustache and chin tufts.
- **Stitches, seams, leather grain**: relief, not geometry (M4's pattern library: `stitching`, `grain`).

Each tier keeps what it can show, by the rule the compiler already uses (a tier's smallest feature is 1.6 cells; anything thinner is dropped or thickened). For pattern parts we add one rule, **envelope substitution**: when a tier cannot keep the strands, it does not drop the row; it replaces the row by its envelope (one panel with the row's outline), and moves the strand pattern into the normal map (M4) on tiers that have one. So:

| Detail | closeup (MC6) | ultra | high | medium | low |
|---|---|---|---|---|---|
| Jacket fringe | strands as geometry, full count | strands as geometry | strands, thickened | envelope panel + strand normal map | envelope panel, flat |
| Hair and beard | dense tufts plus strand cards | tufts as geometry | merged tufts | mass + tuft normal map | mass only |
| Brows and lashes | brow and lash cards | brow masses (MC5) | brow masses (MC5) | colour only | colour only |
| Stitches, grain | stitches as geometry on visible seams, grain in the 4096 body atlas's normal map | normal map | normal map | normal map (lower density) | shader micro-detail or none |

The silhouette is protected on every tier: the envelope keeps the row's outline, so the low tier's silhouette IoU against the ultra tier stays high (measured, section 8).

## 4. A face that reads at game distance

At the kart racer's distances a face is a few dozen pixels high. It reads from **masses and planes, not features**: brow ridge, eye sockets in shadow, a nose wedge, cheekbones, the jaw (here under the beard), the ears and the hat brim's shadow. The face is built from a small set of face parts on the head (brow, nose, cheeks, jaw, ears) whose sizes are measures, plus two region colours (skin, and a darker eye region) so eyes read even when their geometry is a few cells. Readability is checked by maths per tier: the face height in pixels at the class's view distance and each face part's smallest dimension against the tier's cell; parts that cannot show become colour only.

**Close-up faces: two further paths (Chris, 2026-10-16).** Asked whether faces should also hold up close, Chris answered: "we need to do both", and when asked which he meant, confirmed: "Do both: game face and a detailed close-up face tier". So the plan has **both** close-up quality in the game tiers **and** a separate cinematic tier, on top of the kart-distance MC1 slice, which carries on unchanged. Both are planned milestones (MC5 and MC6); neither is built in the MC1 run.

- **(i) Close-up quality in the high and ultra game tiers (MC5 "close-up game tiers").** For first-person and third-person shooters, where a face can fill a fifth of the screen at about 1 m: real eyes as geometry (an eyeball with iris and pupil regions, upper and lower eyelids), lips, nostrils, ears with a rim and a bowl, and brows as raised masses. These are face parts like the masses above, sized by measures (eye width, lip width, nose length, ear height), and they live on the high and ultra tiers only; medium and low keep the masses and planes. Maps at 2048 (high) and 4096 (ultra), and a **face triangle share**: the head gets a fixed share of the tier's budget (about 15% on high and ultra; which parts count is defined in section 7), so face detail never starves the coat or the hat, and the reverse.
- **(ii) A separate `closeup` cinematic tier (MC6 "cinematic tier").** A fifth tier, built only when an intent asks for it, for cut-scenes and portraits: separate eyeball meshes (own parts, not blended into the head) parented to eye joints `eye_l` and `eye_r` so they can look around; eyelids with a blink-ready joint per lid (or a blink shape); lips and a mouth interior (a mouth bag, upper and lower teeth, a tongue) behind them; eyelash and brow cards (thin strips, like the fringe pattern parts); a `jaw` joint; and facial joints or morph targets ready for expressions (smile, frown, brow up, jaw open, blink). Scalp hair and beard become dense tufts plus strand cards, and the fringe keeps every strand (section 3). Budget about 120,000 to 200,000 triangles and two 4096 atlases: a **body atlas** (garments, accessories, hair and the rest of the body) and a **face atlas** shared by the face skin, eyes and mouth, so the face gets a higher texel density than the body; each atlas's texel density is stated and checked within 5% on its own.

  Geometry alone would still look like a plastic doll up close, so the closeup tier also gets **close-up materials** (built on MC3's procedural looks, in the face atlas): **skin** with a subsurface tint (a warm red-orange tint map for light passing through ears, nose and lips), a pore-scale normal map and a roughness map (oilier on the nose and forehead); **eyes** with separate iris, sclera and pupil regions, and a clear cornea shell over the iris with a wet, very low roughness layer; **mouth** with teeth, gums and tongue as their own regions, each with its own colour and a wet, low roughness. These are written as glTF material data the engine can read (base colour, normal, roughness and metallic, plus the subsurface tint as an extra map named in `extras.evani`); how the engine shades them is a Stage 3 rendering matter, not Evani.Model's. The game tiers stay as they are; the closeup tier is never in a game download unless the game names it.

## 5. Rig-ready output

- **Skeleton in the glTF**: joint nodes in a hierarchy with their rest positions (translation only in the T-pose), a `skins` entry with inverse bind matrices, and the joint names of section 2.
- **Skin weights from the part graph**: every triangle knows its part (the field's labels); every part knows its bone. A vertex starts from its part's bone; near a joint it blends with the neighbouring bones of the graph (parent and children only, never an unrelated bone), by distance to the bone segments, at most four influences, weights summing to one. Garments and accessories inherit the bone of the body part under them (a sleeve follows the forearm, a hat follows the head, the belt follows the hips). This is better than distance-only weighting from a finished mesh because the graph forbids the classic mistakes (an arm vertex weighted to the chest, a beard tuft to the hand).
- **Skinned tiers store float positions.** M1 quantises positions and undoes it with the node's scale, but glTF ignores the transform of a skinned mesh's node, so skinned tiers write 32-bit positions (normals and UVs stay quantised).
- **Anchors** follow their joints (child nodes of the joint, rest positions in `extras.evani`): `hand_l`, `hand_r` (grip), `seat` (pelvis contact for the kart seat), `head_look` (between the eyes, looking forward), `head_top` (hats), `foot_l`, `foot_r`.
- **Face joints (MC5 and MC6)**: `jaw` (child of `head`, pivot under the ears), `eye_l` and `eye_r` (children of `head`, at the eyeball centres), and one lid joint per eyelid (`eyelid_upper_l`, `eyelid_lower_l` and the `_r` pair) for blinking. Face parts take these as their bones the same way the body does (lower teeth, tongue, chin and lower lip: `jaw`; eyeballs: `eye_{side}`), and the beard, moustache and chin tufts follow `jaw` (blending into `head` above the mouth corners), so the beard opens with the mouth instead of the mouth opening behind a still beard; scalp hair and the hat stay on `head`, and the same parent-and-child rule keeps a lip from being weighted to an eye. On MC5's game tiers only `jaw`, `eye_l` and `eye_r` are added; MC6 adds the lids and the expression set.
- **Checks**: the Khronos validator clean; every vertex has 1 to 4 influences summing to 1; the largest influence is the part's own bone outside the joint zones; a test pose (elbow bent 90 degrees, knee 60 degrees) moves only the expected vertices and keeps the mesh free of new self-intersections beyond a small, reported count (MC2).

## 6. Materials by code

Regions come from parts (`"region": "leather"`), colours from the intent (`colours: { leather: "#7a5636", ... }`) or the template's defaults.

- **MC1**: one flat base colour per region, written as vertex colours (`COLOR_0`) on every tier, so the compiled model is coloured in any glTF viewer and in the sheets.
- **MC3**: procedural material looks per region, evaluated in 3D at each texel of the tier's atlas (the same per-texel projection onto the field that already bakes normal maps): leather (low-frequency mottling, darker creases from field curvature, stitch lines along garment edges), cloth (weave normal, fold shading), skin (warm variation, darker brows and eye sockets), hair (strand direction normal), metal (metallic 1, low roughness, edge highlights). Output per tier: base colour, normal, and a packed roughness and metallic map, sized by the tier's texel density. All deterministic, all ours.

## 7. Per-tier budgets

For a 1.9 m character. Low and medium, and high and ultra without MC5, are sized for the kart racer's driver distances; the MC5 values and the closeup row are for shooter and close-up viewing distances (a face at about 1 m).

| Tier | Triangles | Maps (MC3) | Keeps |
|---|---|---|---|
| low | 3,000 | 256 | body, garments, hat, boots, hair and beard masses, fringe envelopes |
| medium | 8,000 | 512 | plus face planes, buckle, bandana knot |
| high | 20,000 | 1024, or 2048 with MC5 | plus fringe strands (thickened), hair tufts; with MC5 also eyes, eyelids, lips, nostrils, ears and brows |
| ultra | 60,000 | 2048, or 4096 with MC5 | everything as geometry; with MC5 also the face features above |
| closeup (MC6) | 120,000 to 200,000 | two 4096 atlases: body, and face (skin, eyes, mouth) | separate eyes, eyelids, mouth interior with teeth, gums and tongue, lash and brow cards, face joints or morph targets; hair and beard as dense tufts and strand cards, full fringe; close-up skin, eye and mouth materials |

With MC5 high and ultra keep their triangle budgets but give the head a fixed share of about 15% and gain real eye, lid, lip, nostril and ear geometry; their maps rise to 2048 (high) and 4096 (ultra). The **head share** counts the head body part, its face parts (eyes, lids, lips, nose, ears, brows) and the beard and moustache; the hat and the scalp hair are counted separately, each with its own share reported. The Meshy Marshal has 19,545 triangles and 2048 to 4096 px maps; our high tier is its peer. The skeleton is the same on every tier (MC2 may drop toe joints on low).

## 8. How quality is measured against the Meshy target

Status: built and merged into evani-model (merge 9236045); on the Frontier Marshal it reproduces mean IoU 0.7897.

The tool `evani-model compare <intent> --target <master.glb>`:

1. **Load both.** Our model: the compiled tier (ultra by default). The target: any glTF binary with float positions (the Meshy master), all mesh nodes with their node transforms. Nothing of the target is kept beyond the run.
2. **Align, never rescale.** Both are put on the ground (lowest point at y = 0) and centred in x and z on their boxes. The height ratio is reported, not hidden.
3. **Render the same views** with our own CPU rasteriser (preview.rs, fixed point, four samples per pixel, identical bytes on every machine): front, side, back and three-quarter, orthographic, one frame shared by both models.
4. **Score silhouettes**: IoU per view and the mean.
5. **Score region proportions** from the silhouettes with the same rules on both (rows of the front and side masks, measured in metres): total height, arm span, hat brim width (widest row in the top band), head width (narrowest row between brim and shoulders), shoulder width, coat hem height (where the width steps in below the waist), boot top height (where the leg width steps out above the feet), and the body depth in the side view. Each with ours, target and the difference.
6. **Write a side-by-side sheet**: per view the target, ours and an overlay (both, target only, ours only in distinct colours), with the numbers on the sheet.
7. **Colour regions (from MC3)**: outlines alone cannot tell a right-looking model from one with the right outline and the wrong colours or missing inner detail (fringe, belt, buckle, bandana). From MC3 the tool also renders both models in colour (the target with its own base-colour texture, read only for the measurement; ours with its regions) and reports, per view and per region mask of ours, the mean colour difference (CIE76 ΔE on the averaged colours) and the share of the region's pixels whose nearest target colour cluster matches.

**Sensitivity bound.** The tool must separate right from wrong: identical inputs score exactly 1.0 on every view, and a model made 10% taller, or 10% wider, must lose at least 0.05 of mean IoU against the original. (A sideways shift is no test: step 2 centres both models, which undoes it.) Both are tests (MC1).

**How it fits M2.** The compare score (mean IoU plus weighted proportion errors) is exactly what M2's search would optimise for a character: the template's measures are the search space, M2's sampler and our evolution strategy move them, and the compare tool is the scorer. MC4 wires them together; MC1 only measures.

## 9. Milestones (proposed; each ends with a report and Chris's approval)

| # | What | Done when (tests) |
|---|---|---|
| **MC1** character template (Signed off by Chris 2026-10-16) | `character` class with skeleton, layers, regions, garments as shells, the coat skirt as a drape, fringe pattern parts, hat, belt and buckle, boots, bandana, hair and beard masses; the Marshal intent; region colours as vertex colours; skeleton, skin weights and anchors in the glTF; the compare tool and its sheet | the Marshal compiles on all four tiers, deterministic (identical bytes at 1 and 8 threads), musts pass, meshes clean, Khronos validator clean; anchors and joints present; weights valid (1 to 4 influences summing to 1); coat thickness spread reported; front-view hem-band IoU reported and the sheet shows a gap between the coat skirt and the legs; compare: identical inputs score exactly 1.0, a model 10% taller or 10% wider loses at least 0.05 mean IoU; numbers and sheets in report-mc1.md |
| **MC2** garments and detail | envelope substitution per tier, face planes, hair and beard tufts, fringe rows along curves, deformation test poses, finger variant | every tier keeps the fringe outline (low-tier silhouette IoU against ultra at least 0.9 per view); test poses deform only the expected regions; face readability per tier reported |
| **MC3** materials | procedural leather, cloth, skin, hair and metal baked to base colour, normal and roughness/metallic per tier; colour regions in the compare tool | maps seam-free (M1's seam test), deterministic, texel density per tier within 5%; sheets in colour; compare: mean colour difference per region at most ΔE 15 against the Meshy target on the front view, and every named region (hat, hair, skin, leather, bandana, belt, buckle, trousers, boots) found |
| **MC4** search fit | the compare score as M2's objective over the template's measures | the Marshal reaches a mean silhouette IoU of at least 0.85 against the Meshy master (0.9 is the brief's bar for karts), with every proportion within 5%; then the strongman and the cowgirl from new intents |
| **MC5** close-up game tiers | real eyes (eyeball, iris and pupil regions), eyelids, lips, nostrils, ears and brows as face parts on high and ultra; the head's triangle share; `jaw`, `eye_l`, `eye_r` joints; maps 2048 (high) and 4096 (ultra) | eyes, eyelids, lips, nostrils, ears and brows present as named parts and regions on high and ultra and absent (masses only) on medium and low; face feature readability at 1 m: every face part's smallest dimension at least 3 px in a 1080p view with a 60 degree field of view; head share (head, face parts, beard and moustache; hat and scalp hair reported separately, section 7) within 15% ± 3% of the tier's triangles; a jaw-open test pose (20 degrees) moves only face, beard and moustache vertices below the nose, and no scalp-hair, hat or body vertex moves more than 1 mm; validator clean and deterministic on every tier; the compare tool's face crop view (head only, front and three-quarter) scored against the Meshy head and reported |
| **MC6** cinematic tier | a fifth tier `closeup`, built only when the intent asks; separate eyeball meshes on `eye_l` and `eye_r`; eyelids with blink joints or a blink shape; lips, mouth interior, teeth and tongue; lash and brow cards; hair and beard as dense tufts and strand cards; `jaw` and face joints or morph targets for smile, frown, brow up, jaw open and blink; close-up materials for skin (subsurface tint, pores, roughness), eyes (iris, sclera, pupil, wet cornea) and mouth (teeth, gums, tongue) | the Marshal's closeup tier compiles within 120,000 to 200,000 triangles with a 4096 body atlas and a 4096 face atlas, each atlas's texel density within 5% of its stated value, validator clean, identical bytes at 1 and 8 threads; eyes, eyelids, lips, teeth, gums and tongue present as named parts and regions; all five expressions exist as named joint sets or morph targets, and each moves only its own face area: jaw open (lower face, beard and moustache below the nose), blink (the lids), smile (mouth corners and cheeks), frown (mouth corners and brows), brow up (brows and forehead), with no hat, scalp-hair or body vertex moving more than 1 mm in any of them; at full blink the gap between the upper and lower lid edges is at most 0.2 mm along the whole lid, and no lid vertex goes inside the eyeball; at ±30 degrees yaw and ±20 degrees pitch of each eye joint, no eyeball vertex comes out through the eyelid or head surface by more than 0.5 mm; skin, eye (iris, sclera, cornea) and mouth (teeth, gums, tongue) regions each have base colour, normal and roughness in the face atlas, and skin has its subsurface tint map; a close-up front render sheet of the face is in the report; the compare tool's face crop view scored against the Meshy head; no closeup data in any game-tier file |

The other Meshy models follow MC4: the strongman (same template, body measures only), the cowgirl (same template, different garments), the chibi figure (chibi proportions), the statue (static, drapery garments), the pumpkin fiend (a head variant) and the adventurer (a creature variant, last).

## 10. What only Evani.Model knows, and why that should beat a generic mesh pipeline

The part graph names every region, bone and garment, so weights, colours, envelopes and region measures come from meaning, not from guessing on a finished mesh; the field makes garment shells follow the body (exact over single parts, checked near blends) and lets every tier re-extract its own mesh; and every number lives in a measure, so the search (M2) and language edits (M3: "longer coat", "wider brim") work on characters exactly as they work on karts.
