# Signature moments

Each moment is a `capture.frame {moment}` id (kit `src/moments.rs`, registered by the app under the `live` build). The setup plays the real game from a fresh seeded level with the best bot (or the weakest, for the loss) and stops on the first tick that shows the moment, so a moment is the same frame every time. The showcase takes each one as a full-page shot (canvas plus DOM HUD) at 1920x1080 on Low and Ultra.

In every moment: the board is the hero (96 px cells at 1080p, centred with the compact panel beside it), every piece has its ink outline, shade crescent on the lower right and contact shadow down-right, one piece per cell, Puff stands on his ground line under the panel with his star clear of it, the candy-dot backdrop, and no debug overlay.

## title

- In frame: the title screen (tilted "Puff's Sweet Grid" logo, Play, Levels and Settings buttons, the controls line on a big desktop) in the panel column above Puff (on a phone, in the strip above the board), beside the attract board, which plays itself in full view; a line popping; Puff beside the board.
- HUD: the in-level HUD panel is hidden on the title; the title screen is the UI.
- Pass: logo and buttons legible and outlined; board full (no empty cells apart from the popping line) and no part of the menu over it.

## match

- In frame: level 1 (Sprinkle Meadow), the first move: a line of three pieces mid-pop (scaled up, flashing white, a ring in the piece colour), sparkles, and its floating score text over the cleared cells.
- HUD: Level name, moves 1 less than the start, score with the star bar starting to fill, goal chips with progress.
- Pass: the pop and its score read at a glance; the score text is small and fades before pieces fall into those cells; no other popup.

## cascade

- In frame: a cascade round 2 or later: pieces falling into the gaps (stretched in the direction of fall), new pieces dropping in from the top edge, and the combo callout ("Great!", "Superb!") off the board, under the HUD panel above Puff (on a phone, in the strip between the HUD and the board).
- HUD: score climbing; Puff excited (hopping, arms up).
- Pass: no callout or score text over the falling pieces; pieces entering from above fade in just inside the board's top edge, never above it or over the HUD; the callout is outlined and legible.

## bomb

- In frame: a Pop Bomb going off: its 3x3 clearing, a short warm flash on the bomb's own cell, two crisp warm-white rings with ink rims expanding (the second a step behind), kept inside the board, sparkles in the piece colour, the board shaken a few pixels.
- HUD: as in play.
- Pass: the blast reads as flat, bright candy shapes (bloom on Ultra), the board stays readable, and the ring is clearly an effect (an expanding ring) rather than a second piece in a cell.

## prism

- In frame: a Prism firing: six staggered crisp rainbow rings in the piece colours with thin ink rims, kept inside the board, and a brief white flash on the Prism's own cell only, every piece of the target colour popping across the board.
- HUD: as in play; Puff cheering.
- Pass: the glow stays on the effect sprites (flat pieces never bloom); the rings are visible and no flash hides the board; which colour is being cleared is clear from the popping pieces; score texts keep their ink outline (never white on white).

## sugar_rush

- In frame: level 1 won with moves left: the "Sugar Rush!" banner, a left-over move fired as a Streak with its beam across the board, sparkles.
- HUD: moves counting down, score jumping, goal chips ticked (green), the banner in the panel (on a phone, under the HUD strip).
- Pass: the beam stays inside the board; the banner is legible and does not cover the board.

## win

- In frame: the result screen over the cleared board: "Level complete!", one to three big stars, the score, Next level, Play again and Title screen buttons; confetti settling; Puff cheering.
- HUD: the panel bright beside the dimmed board and the card, all goal chips ticked, the star bar's marks lit (sun yellow, bigger, thick ink outline) up to the stars earned.
- Pass: the stars on the card match the score's star bar, whose lit marks stand out on the full bar; the card sits over the board and clears the panel; buttons large and easy to hit; text legible.

## lose

- In frame: level 1 lost by the weakest bot: the result screen "Out of moves", what was missing, Play again and Title screen buttons (no Next level); Puff sad.
- HUD: panel bright beside the dimmed board, moves 0, unmet goal chips.
- Pass: the loss is clear without being harsh; the board under the dim is settled and full (no empty cells).
