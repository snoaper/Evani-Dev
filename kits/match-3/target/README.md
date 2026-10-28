# Target pack: Puff's Sweet Grid (match-3 kit)

The "target" is what a good capture of Puff's Sweet Grid must look like: a bright cartoon 2D candy match-3, a flat-on board of outlined, cel-shaded sweets with Puff the cloud-candy mascot beside it, a light DOM HUD, all drawn from the engine's built-in atlas shapes (no downloaded art).

| File | Holds |
|---|---|
| `reference-prompt.md` | The reference image prompt (verbatim from SPEC.md section 8) and variants for the title screen and the win card. |
| `look-bible.md` | Palette (sRGB hex) by role, shape language, backdrop, 14 theme props with where they go, lighting. |
| `moments.md` | The eight signature capture moments (`capture.frame {moment}` ids), what must be in frame and what makes each pass. |
| `hud-layout.md` | Every HUD element with its place at 1920x1080 and on a 390x844 phone, and what it shows during the intro card, callouts, Sugar Rush and the result card. |
| `playbook.md` | Every item of `docs/playbooks/match-3.md`, with how this kit meets it. |

## How a reviewer uses it

1. Take the showcase shot for a moment id from `moments.md` (`shot-<moment>-<tier>.png`).
2. Compare it with the reference prompt's scene: glossy candy board, a special going off with sparkles, Puff beside the board, HUD with moves, goals and star bar.
3. Check the look against `look-bible.md`: palette hexes, ink outlines on every piece, a crescent of shade on the lower right of each piece (light from the top left), contact shadows down-right, the checker board tray, the candy-dot backdrop.
4. Check the HUD against `hud-layout.md`: every element present and labelled, legible at both sizes, nothing over the board or the falling pieces.
5. Check each moment's "pass" list; each failed line is one finding, quoted with the moment id.
6. Check the pieces: six colours and six shapes, one piece per cell, no empty cell once the board has settled.

Binding art direction (Chris): the board is the hero on desktop (big board, compact panel, no big gap); nothing overlaps the panel or the board frame (Puff and his star included); every value has its label on the phone too; praise and score popups brief and away from the falling pieces.

Colours here are sRGB hex. The look crate stores linear floats; convert when applying.
