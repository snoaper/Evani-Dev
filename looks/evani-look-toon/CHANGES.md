# Changes

Tuned live and written by bake.export, newest first.

## 2026-10-14 — Shadows tint to soft purple (hand edit)

- **render.shadow_tint** new colour param, set to #a98bf0 (soft purple), and **render.shadow_tint_strength** new 0 to 1 param, set to 0.65: shadow-map and blob shadows keep their brightness but take the tint's hue (a green meadow in shadow turns lavender) instead of going near-black. Both are live-tunable in the Look Lab. Strength 0 (the default for other looks) leaves the output unchanged.

## 2026-10-14 — Sugarpeak Rally second review (hand edit)

- **rim** slot: new `RIM_BODY` gives drivers and karts a soft rim light.
- **fog** slot: now fades the far distance so no camera sees the end of the world.

## 2026-10-14 06:46 UTC — Sugarpeak Rally tuning pass (tx 1–5)

- **fx.outline.thickness** 1.0 → 1.6 — Ink lines a little thicker and darker (1.6 px, 0.9) so mountain, kart and gantry silhouettes read as cel-shaded; the outline pass cost did not change (about 0.25 ms). Re-applied: the first try was auto-reverted while A/B runs were measuring *(http)*
- **fx.outline.strength** 0.85 → 0.9 — Ink lines a little thicker and darker (1.6 px, 0.9) so mountain, kart and gantry silhouettes read as cel-shaded; the outline pass cost did not change (about 0.25 ms). Re-applied: the first try was auto-reverted while A/B runs were measuring *(http)*
- **kit.kart_racer.item_respawn_s** 3.0 → 1.5 — Item boxes come back after 1.5 s so a player behind the AI pack still finds boxes to take (at 3 s the AI ahead left every box empty) *(http)*
- **fog** slot rewritten — Lighter haze (starts at 60 m, thinner, capped at 0.45) so the far track and grass keep their candy colours instead of going milky *(http)*
- Reverted automatically: 2 changes (frame_time).

