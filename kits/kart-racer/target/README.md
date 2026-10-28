# Target pack: Sugarpeak Rally (kart-racer kit)

The "target" is what a good capture of Sugarpeak Rally on Sugarpeak Loop must look like: a toon cel-shaded candy-mountain kart racer, procedural low-poly models, about 300 KB download.

| File | Holds |
|---|---|
| `reference-prompt.md` | The reference image prompt (verbatim from SPEC.md section 8) and variants for title, podium and countdown. |
| `look-bible.md` | Palette (sRGB hex), shape language, sky and backdrop, 15 theme props with recipes and budgets, lighting. |
| `moments.md` | The signature capture moments, with ids, what must be in frame, camera, HUD state, pass criteria. |
| `hud-layout.md` | Every HUD element with anchor, offset, size at 1920x1080 and 844x390, fonts, colours, split-screen. |

## How a reviewer uses it

1. Take the capture for a moment id from `moments.md`.
2. Compare it with the reference prompt's scene (`reference-prompt.md`): drift sparks, rivals, item boxes, HUD.
3. Check the look against `look-bible.md`: palette hexes, 2-3 visible bands, ink outline, rim light, candy props in the middle distance, sky with clouds, no world edge visible from any camera.
4. Check the HUD against `hud-layout.md`: every element present, legible at both sizes, visible during the countdown.
5. Check each moment's "pass looks like" list. Each failed line is one finding, quoted with the moment id and file section.
6. Check rival colours: eight distinct, none near the player's pink, player kart marked.

Binding art direction (Chris): push the candy theme (lollipop trees, candy-cane barriers, gumdrop hills, wrapped-sweet signs, all procedural and small); stylised clouds; filled middle distance (grass variation, props at corners, stands or crowds, subtle road variation); toon bands and rim light on karts and drivers, drivers' heads not flat discs; distinct rivals; countdown lights on the start gate with a small boost for a good start; no visible world edge; HUD legible at 1080p and on phones, in every capture including the countdown.

Colours here are sRGB hex. The look crate stores linear floats; convert when applying.
