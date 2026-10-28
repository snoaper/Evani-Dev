# Signature moments

Each moment is a capture (`Race.cam_snap` positions the camera). Every capture shows the full HUD for its state (`hud-layout.md`), including the countdown. In every moment: toon bands and rim light on karts and drivers, ink outlines, candy props in the middle distance, sky with clouds, hill ring and haze, no world edge.

## countdown

- In frame: start gate with three lights over the track; player's pink kart on the grid (no marker over it in the one-player chase view); at least 4 rival karts in distinct colours; stands with crowd; big centre "3", "2", "1" or "GO!".
- Camera: chase camera on the grid, slightly higher than normal so the gate and grid read.
- HUD: position (e.g. 8th/8), Lap 1/3, empty item slot, speed 0, timer 0:00.0, minimap, countdown number. All visible.
- Lights: 3 lights light one by one on 3, 2, 1 (red, red, red) and turn green on GO. Launching in the window about 0.4 s before GO gives a small boost (flame, short speed kick, "GOOD START!" text).
- Pass: gate lights visibly match the number; numeral large and outlined; HUD fully present; no kart overlaps the gate lights.
- Moment id: `countdown` (also capture `countdown_go` for the green frame).

## drift

- In frame: player's kart sliding through a corner at about 25 degrees, blue (tier 1) sparks from both rear wheels, tyre dust, candy-cane barriers and a wrapped-sweet sign on the corner, rivals ahead.
- Camera: chase camera, heading lagging so the slide angle shows.
- HUD: drift tier indicator on "1" in blue, speed about 90 km/h, item slot, position, lap.
- Pass: sparks bright and bloomed, readable at 1080p, driver's head shows rim light and shaded side, kart marked.
- Moment id: `drift`.

## boost

- In frame: kart with flame from the exhaust, speed lines at the screen edge, FOV kick, boost text, a trail of orange sparks.
- Camera: chase camera pulled back slightly by FOV kick.
- HUD: speedometer above normal top speed, drift tier indicator in "BOOST" yellow.
- Pass: flame and lines clear but the kart stays readable; no flicker of the HUD.
- Moment id: `boost`.

## item_hit

- In frame: a bomb blast or goo splat on a rival (or the player spinning out), shockwave ring, debris puffs, a nearby item box, pink-lit rivals dodging.
- Camera: chase camera with shake trauma applied.
- HUD: item slot just used (empty or next roulette), position updated.
- Pass: explosion shapes read as candy-coloured flat shapes with ink outline, not smoke noise; the camera is not obscured.
- Moment id: `item_hit`.

## overtake

- In frame: player's kart passing a rival at close range side by side, rival's distinct colour clear, slipstream or item box in view.
- Camera: chase camera.
- HUD: position changes (e.g. 4th to 3rd) with a short "+1" pop on the number.
- Pass: both karts clearly separate, outlines visible, position number matches the actual order.
- Moment id: `overtake`.

## final_lap

- In frame: banner "FINAL LAP!" across the upper centre for about 2 s, kart crossing the start line under the gate.
- Camera: chase camera on the start straight, stands and crowd behind.
- HUD: Lap 3/3 in yellow, banner, all other elements present.
- Pass: banner outlined and legible on a bright sky; does not cover the kart.
- Moment id: `final_lap`.

## finish

- In frame: player crossing the line, confetti of wrapped sweets, finish banner "FINISH!" and the final position.
- Camera: chase camera easing into a slow orbit at the line.
- HUD: final place large, lap time, other elements fade out.
- Pass: confetti readable, gate lit, no HUD overlap with text.
- Moment id: `finish`.

## podium

- In frame: three podium blocks (1st in the centre), drivers on kart colours with rounded helmeted heads and rim light, candy-cane arch, gumdrop hills behind, results table.
- Camera: fixed three-quarter front view, slight low angle.
- HUD: results panel with eight rows (place, name, time) in kart colours, "Play again" and "Title screen" buttons, the player's row highlighted.
- Pass: head shapes are volumes with visible bands, all eight names distinct from the player's pink row, buttons at least 44 px tall on the phone.
- Moment id: `podium`.


Also captured (extra registered moments, not part of the eight above; `rivals` is also in the app's review list): `rivals` shows the whole roster lined up on the road ahead of the chase camera, the pink player kart in front and the seven rivals in their own colours, parked and clear of one another so every colour reads; `split` is a two-player race four seconds in, to show both halves, both markers and both minimaps.
