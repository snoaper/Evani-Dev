# Changes

Tuned live and written by bake.export, newest first.

## 2026-10-14 08:44 UTC — Live session (tx 1–2)

- **fx.bloom.threshold** 1.2 → 1.5 — Gentler glow for the cartoon board: only HDR beams, sparkles and the Prism core glow; blasts no longer turn into a white blob that hides the pieces (compare.ab: 13.8% of pixels change, GPU -0.14 ms) *(http)*
- **fx.bloom.knee** 0.5 → 0.25 — Gentler glow for the cartoon board: only HDR beams, sparkles and the Prism core glow; blasts no longer turn into a white blob that hides the pieces (compare.ab: 13.8% of pixels change, GPU -0.14 ms) *(http)*
- **fx.bloom.intensity** 0.6 → 0.35 — Gentler glow for the cartoon board: only HDR beams, sparkles and the Prism core glow; blasts no longer turn into a white blob that hides the pieces (compare.ab: 13.8% of pixels change, GPU -0.14 ms) *(http)*
- **look.cartoon2d.outline_px** 3.5 → 4.0 — Ink outlines at the top of the catalog's 3-4 px: at 1080p the soft shape edges ate most of the 3.5 px rim *(http)*

## 2026-10-14 — first version

- Palettes `pieces` and `ui`, style params (`outline_px` 3.5, `shade` 0.62, `highlight` 0.85, `squash` 1.0, `shadow` 0.3), flat tonemap with a soft shoulder, bevelled board tile generator; wants bloom, particles, juice and shake, vetoes the 3D outline effect (kit 003 step 4).
