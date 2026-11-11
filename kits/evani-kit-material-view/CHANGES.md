# Changes: evani-kit-material-view

## Critic round 3 fixes, viewer and scene share (2026-10-26)

- Head: a new bust (`tools/models/char-bust.mjs`). The skin is the sculpted distance field's own surface (surface nets, simplified onto the true surface with full detail round the eyes, nostrils and mouth), so the nose has real nostrils (no white slivers) and the mouth a symmetric line with tucked corners. Head and shoulders in a plain crew-neck top on a turned stone socle (no ruff); the hair is a sculpted volume with two close lock layers flowing from a crown whorl, tapering only over the last few millimetres to a soft hairline; eyebrows. The scene stands it on a moulded square pedestal and frames bust and socle whole above the bar.
- Scenes can carry their own light params (`light` in library.json, put on after the preset's): the head's sky has no clouds, so no wisp drifts behind it.
- Pool: the lawn reads #6aa84f once lit; a slate tiled bed and a deeper water tint, so the bed shows through clear water; tufts of long grass instead of one square mat; finer leaf clusters on the bushes (no cobble cells).
- Lamp: the globe and the nature pass's lamp shade only under the night light (no glow by day); the paper lantern is lit at night; a wider, gentler pool of light (reach 5 m, strength 1), so the falloff is soft and the pole stays under the bloom threshold.
- Framing: `orbit.fit_width` keeps a wide subject inside the open part's width (the pool's basin and table, the yard's bench and pillar on phones held upright).
- Art styles: look-backed materials keep their style's relief; grass, fabric and leather each draw their own (upright blades, a woven over-under, pebbled grain with creases) on a finer sphere.
- Captures: the burned-in review label lifts above any button it would cover (it hid "Convert a model").

## Critic round 2 fixes (2026-10-25)

- Grounding on every tier: each scene draws contact (blob) shadows under its subjects; the renderer uses them only where there is no shadow map (Lite, Low).
- Night lamp: the sky dome's sun goes under the horizon (a dark night sky); the lamp's globe draws in the glow material (bloom where the tier has it) and the nature pass gives it a warm pool of light (`material.nature.lamp.reach` and `.pool`, the same on every tier, fading out by daylight).
- Shader ball: a real shader-ball shape (a ball with a recessed band on a foot) for the clear coat and the soap film; the viewer turns on the engine's sky reflection (`ibl`) so coats and metals reflect a sky with a horizon.
- Head: a new sculpted bust (`tools/models/char-bust.mjs`: jaw, cheekbones, nose with wings and nostrils, lips, ears, softer hairline); one-sample tiers cut hair coverage cleanly instead of a per-pixel dither (no dot spray, no streaks on the collar).
- Same material on every tier: the cloth weave keeps its relief on the cheap path; High and Ultra water use the flat path's reflection weight and body colour, with a gentler bent view.
- HUD: each scene's camera fits its subject (`orbit.fit`) into the part of the view the page leaves open; phones (narrow or short) start with the drawer and panel closed; capture moments ask for them (`Viewer::panels`): open on desktop for the moments, closed on phones.
- Props: a table on legs, clustered bushes, a bottle with a neck and cork, a spray of three leaves, a paper lantern and a bowl of plums (the thin back-light kinds' jobs) instead of the blank signs and the head on a pole; the yard bench stands on legs.
- Look effects: no vignette (its bright middle read as a haze disc); a stronger, wider rim (0.85, width 0.55). The styles scene's selection is a thin glowing ring; Painterly's metal is a real metal and its stone, leather, fabric and grass carry relief.
- Proof moments (`moments::PROOFS`): 20 catalogue rows (lace, damask, newsprint, hay, wheat, lawn, meadow, oak and maple leaves, rose, crystal, pine, slime, denim, wallpaper, tooled leather, hide, horn, a painted coat, frosted glass), each on its own shape, for the review's Low and Ultra showcase.
- After the first round 3 review run: the night lamp's pool of light is stronger and fuller near the lamp, the night preset lowers bloom's threshold so the lit shade blooms (the day presets put the look's own bloom back), and the lamp scene's camera frames the lamp and its pool; the bust's nose wings sit back (no fold beside the nose), its nostrils are smaller and a mouth line is carved; each proof is captured on Low and on Ultra.
- Bloom was off on every tier: the photoreal daylight preset (layered for the playground surfaces) vetoes bloom, and as the later look its veto won. The viewer puts the film look's bloom wish back last, so the night lamp's shade and every glowing material now bloom where the tier has bloom (Medium and up). New `LookData::vetoed_effects()` names any effect a later look switches off; the viewer's test asserts it is empty. Rim at the look's full strength (1.0, width 0.7).
- Head: a wide, shallow mouth crease (the narrow slit folded in the medium tier). Known, not fixed here: the converter's simplifier still leaves 3 sub-millimetre flipped slivers at the ear crease of the medium tier (and a few at the hair crown), hidden under the skin's and hair's mesh tags; a converter flip guard is a follow-up.
- Geometry: each side leaf grows from a small knuckle on its twig; the shader ball's foot is one smooth run (a seam there was cracked open by the converter's simplifier) and ships at full detail (the `ultra` tier); geocheck's floating test also tests where a tilted part reaches lowest.

## Critic round 1 fixes (2026-10-25)

- Art styles: the reference-game field is now `mood`, a few original words per style; style 10 is "Cinematic Pixel" (short "Pixel HD"). A test keeps existing game, studio and brand names out of the viewer data, the page and the catalogue's shown text.
- Capture moments (`moments.rs`): one per scene (styles, ball, cushion, head, ground, pool, lamp) plus `spin` for the run video; each opens the scene's default material with a fixed light, the turntable stopped and the home camera.
- `open_material` and `apply_light` take `&World`, so a capture moment can call them.
- Light presets set the look's own rim (0.55, width 0.4) and a lighter vignette (0.18, radius 0.95).
