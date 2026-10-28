# Changes

The kit's own changes, newest first. Live tuning of the look is in `looks/evani-look-basic/CHANGES.md`.

## 2026-10-15: sandbox polish after critic round 2

- Upright phones and narrow windows: the camera rises and looks down more steeply so the sky stays the 14 to 17 % band, and pulls back just far enough that the whole grid fits across, filling the frame down to just above the HUD panel (`frame_for_aspect`, `narrow_frame`, solved once per canvas shape). The band is measured to where the ground visibly ends (its edge or the far plane), which from that height sits a few degrees under the true horizon, so the pitch tilts down by that dip too (it read 18.5 % on screen before). The terraces moved out to 36 to 54 m so the frame's bottom edge on a phone meets the ground inside their ring: no terrace is cut by the bottom edge or sits under the panel.
- Sprites keep their 1080p density on smaller canvases (`sprite_count`: 110 at 1920 x 1080, at least 24), so phones and small windows are not crowded.
- Bloom halos keep the same size relative to the frame at every window size (an engine change in evani-fx, so every game gets it).

## 2026-10-15: sandbox polish after critic round 1

- Spinners are bevelled cubes and ringed orbs (a ball with a stone ring) instead of raw cubes and spheres, and each rests on the ground on its lowest point while it turns (new `place` module; `place::reach` works out the turned mesh's lowest vertex exactly). The glowing ones are scattered by a hash (about one in seven), not in stripes.
- Real shadows from Medium up (the sandbox lists the `shadows` effect); on Low each spinner stands on a dark shadow disc.
- The scene drawing moved from `apps/sandbox/src/scene.rs` into the kit (`draw` module), so native tools draw exactly what the game does. New middle distance: stone and slate terraces 30 to 50 m out, columns with stone caps 64 to 110 m out (some crowned with a glowing orb), hills 320 to 700 m out under a light haze. 110 sprites in palette colours (was 300).
- Orbit: radius 26 m to 42 m, height 12 m to 11 m, 42° field of view, so the whole grid and a band of sky stay in frame all the way round; the field of view widens on portrait screens, and on a portrait phone the camera also pulls back up to a third (staying inside the columns) so the whole grid fits across. The glow material reads warm orange on screen, not lemon yellow.
- Spinner poses are a pure function of sim time (a shared `SpinClock` the spin system re-anchors when the speed changes; `spinner_angles` works out the poses at the render time), so a capture at a given sim time draws the same frame every run, and motion is smooth between ticks.
- New `kit.example.static_world` param: the geometry check's still world (spinners held and hidden).
- New `review_dump` test (each spinner's measured gap for the review card) and an `evani-geocheck sandbox` adapter.
- Look `basic`: new `stone` and `slate` materials, a warmer orange glow, a distance-haze fog slot.

## 2026-10-14: review paperwork for the test kit

- Added SPEC.md and a target pack in `kits/example/`, this README and a ROADMAP row (000, test kits) so `cargo xtask review sandbox` can produce a complete card (Stage 1 gate check 5).
- `apps/sandbox`: five signature moments on the orbit for `capture.frame { moment }`, a small HUD panel, and a ground plane that reaches the horizon.
