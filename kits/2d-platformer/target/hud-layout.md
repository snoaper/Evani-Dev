# HUD layout

Drawn by the kit in pixel glyphs on the art pixel grid (one HUD pixel = one art pixel), across the top, 8 art px from the sides and 6 from the top. Four groups, left to right: lives (head icon, `x3`), coins (coin icon, `x00`), score (`000000`, centred), time (clock icon, `200`; flashes red under 30 s). HUD glyphs 6 × 8 on a 7 px advance, white over gold with an ink shadow.

Measured by the kit test `hud_is_on_the_art_grid_and_fits_a_portrait_phone` (art px: x0, y0, x1, y1):

| Screen | View (art px) | Scale | Lives | Coins | Score | Time |
|---|---|---|---|---|---|---|
| 1920 × 1080 desktop | 480 × 270 | 4 | 8, 6, 38, 17 | 46, 6, 83, 20 | 219, 7, 263, 17 | 438, 6, 474, 18 |
| 3440 × 1440 desktop (view framed at 640 columns) | 640 × 288 | 5 | 8, 6, 38, 17 | 46, 6, 83, 20 | 299, 7, 343, 17 | 598, 6, 634, 18 |
| 360 × 780 CSS phone, portrait, dpr 3 (view framed at 288 rows) | 270 × 288 | 4 | 8, 6, 38, 17 | 46, 6, 83, 20 | 114, 7, 158, 17 | 228, 6, 264, 18 |
| 390 × 844 CSS phone, portrait, dpr 3 (view framed at 288 rows) | 292 × 288 | 4 | 8, 6, 38, 17 | 46, 6, 83, 20 | 125, 7, 169, 17 | 251, 6, 287, 18 |

Lives and coins sit 8 art px apart; the score is at least 31 art px from its neighbours on the narrowest phone; the HUD needs about 190 art px and the narrowest view is 256. On a 1080p screen one HUD pixel is 4 device pixels (the 6 × 8 glyphs are 24 × 32 px; the whole-pixel scale rule in DECISIONS.md, 2026-10-15, shows 224 to 288 rows, so 1080p is 480 × 270 at 4 and 1440p 5); on the phones 4 device pixels (about 8 × 11 CSS px). The top 20 art px (7 % of the 270 rows at 1080p) belong to the HUD: floating text may pass under it but no gameplay object is placed there by the camera framing (the ground line sits at 72 %, row 194 of 270 at 1080p, with 76 rows of ground below it).

Held upright, a phone or tablet shows the view framed: the 256-column floor sets the scale (4 on the phones), so the view is cut to 288 rows and centred with black bars above and below (DECISIONS.md, 2026-10-16). The HUD stays at the top of the view, as above; the pause button sits in the bar above it, and the movement buttons, Run and Jump in the bar below, clear of play (tests `the_pause_button_sits_under_the_hud_on_every_screen`, `upright_the_controls_sit_in_the_bar_below_the_view`). On a landscape screen the controls lie over the ground rows, as before.

Menus (title, pause, settings, results) are light DOM drawn by the app over the canvas, not part of the captures.
