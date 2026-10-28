# Signature moments

Each moment is a capture the game's `capture.frame {moment}` sets up. In every moment: concrete and metal read apart, team colours unmistakable, no world edge, the HUD for that state (`hud-layout.md`).

## base_warmup

- In frame: the player's view at their own top-level base during the 6 s warm-up; teammates standing near; the core zone ring on the floor ahead; the lift hole.
- HUD: "Warm-up 0:05" banner, health 100, pistol selected, score 0 : 0.
- Pass: three or more teammates in frame, the core ring visible, the countdown legible.

## ring_turn

- In frame: from a ring car, the next car's divider passing the static doorway as the ring turns one sector; the bottom level visible through the lift hole.
- HUD: ring status "Ring turning".
- Pass: the turn is visibly mid-way (doorway and car misaligned), the player carried with it (no clipping).

## firefight

- In frame: pistol raised, an enemy soldier at 10 to 30 m behind concrete cover on the bottom level, fresh bullet holes in that cover.
- HUD: health under 100, a hit marker or damage number.
- Pass: the enemy is in team colour and readable; holes sit where shots met the concrete.

## rocket_blast

- In frame: a rocket hitting a low concrete wall, a round crater through it, a soldier knocked back.
- Pass: the crater is a clean round bite through the wall; metal near it intact.

## core_hold

- In frame: the player standing in the enemy core zone on the enemy's top level, the zone lit in the enemy's colour, the hold bar filling.
- HUD: core hold bar over 50 percent.
- Pass: the bar and the zone agree; no defender inside the zone.

## results

- In frame: the dimmed fortress behind the results panel.
- HUD: the winner line ("Blue wins - core held", "Orange wins - 25 kills" or "Draw - time"), both teams' scoreboards.
- Pass: the winner line matches the scores; every soldier listed once, bots marked.
