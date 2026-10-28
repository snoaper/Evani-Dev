# HUD layout

Greybox: the HUD is drawn inside the canvas (bitmap text and sprite boxes on sprite layers 100 to 120). Positions are tunables (`kit.base_building_rts.hud_*`).

| Element | 1920x1080 | Phone (844x390 landscape, 390x844 portrait) | Shows |
|---|---|---|---|
| Crystal counter | Top left, 16 px in, 20 px text | Top left, 10 px in, 20 px text | Banked crystal and the unit count / cap |
| Mode line | Top centre | Top centre, below the pause button row | "Skirmish" or "Free roam", the AI's state in debug |
| Selection panel | Bottom left, 360 x 96 | Bottom centre, full width minus the touch buttons | Selected count by type, the selected building's queue and progress |
| Build / train row | Bottom left above the panel: key hints (Q worker, E trooper, R rumbler, B foundry, T bastion) | Touch buttons bottom right (Build, Train, Attack, Stop) | What the selection can do |
| Minimap | Bottom right, 200 x 200 | Top right under pause, 120 x 120 | Explored map, team dots, camera box |
| Banner | Centre, 40 px text | Centre, 40 px text | "Victory", "Defeat", "Paused" |
| Command marker | In world | In world | Move (white), attack (red), gather (amber) at the target |

During a popup (pause, victory, defeat) the counter and minimap stay visible; the banner sits over the world, never over the counter.
