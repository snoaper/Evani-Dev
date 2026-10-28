# HUD layout

A light DOM HUD in the page (`#tactical-fps-hud`). Sizes at 1920x1080 / on a phone in portrait (390x844). Minimum font 16 px at 1080p, 12 px on a phone. White text with a dark outline; team colours from the look bible.

| Element | Anchor | Size (1080p / phone) | Shows |
|---|---|---|---|
| Crosshair | centre | 24 px / 18 px | A thin cross; widens with movement spread and recoil. |
| Health | bottom-left | 48 px / 24 px font | 0 to 100; red under 30; a shield icon while the spawn shield is on. |
| Weapon slots | bottom-right | four 64 px / 40 px slots | Pistol, Sniper, Rockets, Grenades; the selected one lit; a cooldown sweep. |
| Team score | top-centre | 44 px / 22 px font | "Blue 12 : 9 Orange" in team colours, first to 25. |
| Match timer | top-centre under the score | 28 px / 16 px font | Time left of the 15 minutes. |
| Ring status | top-left | 24 px / 14 px font | "Ring turns in 0:42" or "Ring turning". |
| Core hold bar | top-centre under the timer | 360x14 px / 200x10 px | Either team's hold of the other's core, in the holder's colour; hidden at 0. |
| Kill feed | top-right | 20 px / 13 px font, 5 lines | "Ada [rockets] Bram", killer and victim in team colours. |
| Respawn notice | centre | 40 px / 22 px font | "Respawn in 2" while dead. |
| Touch controls | phones and tablets | engine touch layer | Move stick bottom-left, look stick bottom-right, Fire, Jump, Weapon, pause top-right clear of the score. |

## During the warm-up

A centre banner "Warm-up 0:05" counts down; score, timer and ring status show their start values; firing is held off.

## During the results

A centre panel replaces the crosshair: the winner line and both scoreboards (name, kills, deaths; bots marked); the touch controls hide except pause.

## Title

"Hexagon" and "Press Fire or Jump to start" (tap on a phone) over a slow view of the fortress.
