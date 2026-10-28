# Target pack: Hexagon (tactical-fps kit)

The "target" is what a good capture of Hexagon must look like: a first-person team shooter in a hexagonal concrete-and-metal fortress over a chasm, three levels, a middle ring that turns one sector at a time, concrete cover that takes holes, Blue against Orange, four a side with bots. The demo is Chris Pearson's game Hexagon (MIT, @Snoaper), rebuilt on Evani.Dev (docs/HEXAGON_PLAN.md).

The first look is `toon` (the only workspace look in the genre's `compatible_looks`); the planned `photoreal` look replaces it later (ROADMAP row 006). The pack describes the fortress itself, so it holds for both looks.

| File | Holds |
|---|---|
| `reference-prompt.md` | The reference image prompt and variants (title, core hold, results). |
| `look-bible.md` | Palette (sRGB hex), shape language, sky and backdrop, theme props, lighting. |
| `moments.md` | The signature capture moments: ids, what must be in frame, pass criteria. |
| `hud-layout.md` | Every HUD element with anchor and size at 1080p and on a phone, and its state during the warm-up and results. |
| `playbook.md` | The tactical-fps genre items, how this kit meets each, and Hexagon's two deliberate differences. |

## How a reviewer uses it

1. Take the capture for a moment id from `moments.md`.
2. Compare it with the reference prompt's scene.
3. Check the look against `look-bible.md`: concrete reads as breakable, metal as frame, team colours unmistakable, no world edge from any camera.
4. Check the HUD against `hud-layout.md`.
5. Check the playbook items; each failed line is one finding.
