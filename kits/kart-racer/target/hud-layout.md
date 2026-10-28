# HUD layout

The HUD is light DOM sized by `--u`, which is 1 vmin clamped to 5 to 11 px (one player) or 0.62 vmin clamped to 4 to 7 px (split screen). Sizes below are in px at 1920x1080 (u = 10.8, clamped to 11) and on a phone at 844x390 (u = 5). Anchors are offsets from the player's own view rectangle. Text colour is white with a 2 px #2b1240 outline plus a 4 px drop (`text-shadow`); panels have a 3 px #2b1240 border. Minimum font size: 12 px on a phone, 16 px at 1080p; nothing under that.

| Element | Anchor | Offset (1080p / phone) | Size (1080p / phone) | Colours |
|---|---|---|---|---|
| Position "1st/8" | top-left | 24, 15 / 11, 7 | number 108 px / 50 px font, "/8" at 40% | #ffd23f number, white "/8" |
| Lap "Lap 2/3" | top-left under position | below position, 4 gap | 50 px / 23 px font | white; yellow #ffd23f on the final lap |
| Race timer | top-right | 24, 15 / 11, 7 | 35 px / 16 px font, tabular digits | white |
| Minimap | top-right under timer | 9 / 4 gap | 150 x 150 px / 90 x 90 px | track #6f6c7e on a 60% #2b1240 panel; player #ff4fa3 with white ring; rivals in their kart colours |
| Item slot | top-centre (single player) | y = 15 / 7 | 143 x 143 px / 65 x 65 px, icon 88 / 40 px | white 90% panel, lilac #b58cff inner ring, ink border; icon in candy colours with ink outline |
| Item roulette | in the slot | n/a | icon cycles 8 times per second | slot turns #ffd23f while rolling |
| Item name | in the slot, below icon | n/a | 19 px / 12 px font | #2b1240 on white |
| Drift tier indicator | bottom-centre above kart | 18% from bottom / 14% | 44 px / 20 px font | tier 1 #49b6ff, tier 2 #ff9a2e, tier 3 #ff5fa8, boost #ffd23f |
| Speedometer | bottom-left | 24, 17 / 11, 8 | number 82 px / 38 px font, "km/h" 26 px / 12 px | white |
| WRONG WAY | top-centre, in the lap-banner band under the item slot (19 u from the top; moved up from 38% after review because the 38% line sat on the player marker, which is at about 37%); in a split-screen half under 600 px tall it sits beside the item slot instead | n/a | 76 px / 35 px font (7 u; 12 px at least) | #ff4d6d |
| Countdown 3-2-1-GO | centre | 38% from top | 97 px / 45 px font; pops from 130% to 100% on each step | white 3-2-1, #4fe0b6 on GO |
| Final-lap banner | top-centre, as near 12% from the top as the item slot allows: its top edge sits just under the slot (15 u; the slot reaches down to about 14.6%), so its centre is at about 19% at 1080p (fix round 3: 12% would cover the item slot) | n/a | 76 px / 35 px font, shown 2 s | #ffd23f |
| Good-start text | centre, 55% from top; on a phone (under 480 px tall) in the banner band under the item slot (15 u), because 55% sits on the kart there (fix round 3) | n/a | 54 px / 25 px font | #ffd23f |
| Player marker | above own kart, tracked in world space; split-screen and the finish orbit only (hidden in the one-player chase view and on the podium) | 2.2 m over the kart | pink arrow 36 px / 20 px with white outline, name label 22 px / 12 px | #ff4fa3, outline #2b1240; clamps to screen edge if off screen |
| Rival tags (optional) | above rival karts within 40 m | n/a | 16 px / 12 px | their kart colour with ink outline |
| Results / podium | centre panel | n/a | table rows 40 px / 22 px, buttons 64 / 44 px tall minimum | rows in kart colours, player row #ff4fa3 highlight |

Notes:
- All elements are drawn in the countdown phase too, except item slot hidden if empty at 55% opacity, not removed (it stays visible as a dashed circle), and the timer reads 0:00.0.
- Safe area: keep 2.2 u from every edge (24 px at 1080p, 11 px on a phone) and clear of notches (use `env(safe-area-inset-*)`).
- Contrast: every glyph over bright sky or grass has the 2 px ink outline; no text is placed directly on the road centre.
- Touch (phone): steer left/right zones on the lower thirds, drift button bottom-right 64 px, item button above it 56 px; none overlap the speedometer or minimap.
- The kart stays low and central: no HUD element sits in the middle 30% of the width below 60% of the height.

## Two-player split-screen

- Canvas is split top and bottom; each half is a full HUD set at `--u` 0.62 vmin (clamped 4 to 7 px): at 1080p about 6.7 px, so position font about 67 px, lap 31 px, timer 21 px; on a phone 4 px, so 40 / 18 / 13 px (minimum font 12 px still applies).
- Elements per half: position, lap, timer, item slot, speedometer, drift tier, minimap (smaller: 100 px / 64 px), WRONG WAY, countdown number, marker (split-screen only). The final-lap banner and countdown are shown in each half.
- P1 marker pink #ff4fa3, P2 marker #a23cff.
- A 3 px #2b1240 divider between halves. Results and podium show once, full screen.
