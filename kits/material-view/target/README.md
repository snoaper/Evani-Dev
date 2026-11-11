# Target pack: Evani.Materials (material-view kit)

The "target" is what a good capture of the material viewer must look like: a
calm, bright studio showcase where one family of materials at a time sits on
clean test subjects (spheres and shapes, a shader ball, a cushion, a head
bust, a ground patch, a garden pool, a lamp corner) under readable light,
with a compact library HUD round the edges that never covers the subject.
The viewer is a tool, not a game: its "moments" are its seven scenes, and
the critic judges whether each material reads as itself on Low and Ultra.

| File | Holds |
|---|---|
| `reference-prompt.md` | The reference image prompt (KIT_PROCESS template) and a variant for the head close-up. |
| `look-bible.md` | Palette (sRGB hex) by role, shape language of the test subjects, the studio backdrop, 12 theme props with where they go, lighting. |
| `moments.md` | The seven signature moments (`capture.frame {moment}` ids, one per scene), what must be in frame and what makes each pass. |
| `hud-layout.md` | Every HUD element with its place at 1920x1080, on a 1280 px laptop, a half-snapped window, a phone (portrait and landscape) and 2560 px wide. |
| `playbook.md` | There is no genre playbook for a viewer: the brief's checks written as playbook items, with how the viewer meets each. |

## How a reviewer uses it

1. Take the showcase shot for a moment id from `moments.md` (`shot-<moment>-<tier>.png`).
2. Check the subject against the moment's "In frame" and "Pass" lines: the material reads as itself, sits on the floor with a contact shadow, light from the upper right.
3. Check the look against `look-bible.md`: palette hexes, the warm studio floor running to the horizon (no edge of the world), the bright sky, rim light on the subjects' shadow side, soft contact occlusion.
4. Check the HUD against `hud-layout.md`: nothing overlaps, nothing is clipped, long names end in "..." with the full name on hover, the subject stays visible.
5. Low and Ultra must show the same scene and the same subjects: Low may be softer, never a different picture.

All names, subjects and text are original. The art-style tabs describe each
style in our own words; no existing game, studio or brand is named anywhere
the viewer shows.
