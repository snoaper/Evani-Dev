# Signature moments

Each is a scripted capture moment (`capture.frame { moment }`, the kit's `CaptureMoments`, listed in `apps/2d-platformer/evani.toml` `[review] moments`). Each is a state of the title screen's attract recording (the bot's deterministic run), shown exactly at its tick, so a capture is the same frame every time. Captured at 1920 × 1080 (480 × 270 art pixels at 4×); the card takes each on its listed tier, and the showcase takes them all on both Low and Ultra.

The kit has no power-up item; the reward moment is the extra life (one is set one coin short of 50 just before a coin in an arc).

Framing for every moment: the hero well inside the frame; on the ground, the feet at 60–80 % of the height and terrain below the feet at most about 35 % of the screen.

## start

Card tier: Low. Must show: Fennel standing at the signpost at the level's left edge, the ground line in the lower third, the first coins and the hill ahead, full HUD (lives 3, coins 00, score 000000, time 200). Must not show: a screen of dirt; the hero at an edge.

## stomp

Card tier: Ultra. Must show: Fennel at the top of the bounce above a flattened beetle, the puff and the stomp points (merged with any coins just taken, e.g. "130") rising clear of the hero. Must not show: the score text on the hero.

## power_up

Card tier: Low. Must show: the 50th coin taken in mid-air in a coin line: "1UP" and the score total rising clear of the hero and of each other, the rest of the arc ahead, HUD lives 4, and the extra life landing on Fennel: a burst of short gold rays flying out and a dithered gold glow behind Fennel (it fades by its dither within 0.6 s). Must not show: text on text; coins inside decor.

## checkpoint

Card tier: Ultra. Must show: the flag just raised beside Fennel, its sparkle.

## spikes

Card tier: Low. Must show: Fennel close above the spike pit beside the stone pillar (cap, shaft, plinth), the spikes plainly visible. Must not show: spikes hidden by decor or off screen.

## pit

Card tier: Ultra. Must show: Fennel in mid-leap over the first 10-tile pit, both lips in frame, the gap reading as an empty drop (a dark blue void with the cliff faces either side), not as dark ground or bushes. Must not show: parallax bushes or green fill showing inside the gap; the gap in the same colour as the ground.

## goal

Card tier: Ultra. Must show: Fennel in the castle gate with the torches lit, lit by a soft gold glow in the dark gateway, the level-clear state: the LEVEL CLEAR! banner in the sky between the HUD and the castle's towers. Must not show: the banner over the towers or the battlements.
