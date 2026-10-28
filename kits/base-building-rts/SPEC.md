# SPEC: base-building-rts ("Cinder Ridge")

Kit 005, crate `kits/evani-kit-base-building-rts`, game `apps/base-building-rts`. The target pack is in [`target/`](target/README.md): look bible, moments, HUD layout and playbook are not repeated here.

A one-versus-one skirmish on a 128 x 128 m highland split by a river with two fords. The player (south, team 0, #1fb5a8) and the rival AI (north, team 1, #e0662b) each start with a command hub, four workers and 400 amber crystal. Destroy the rival's command hub to win; lose yours and you lose.

## Simulation model (lockstep-safe)

- Fixed 60 Hz `Stage::Update`; `Time::fixed_dt` only, never wall-clock time. Randomness only from the world's seeded `Pcg32` (AI placement jitter).
- **Orders are the only input.** Pointer and keys become `Command`s (move, attack, gather, build, train, rally, stop) in a queue; the AI pushes into the same queue. The sim applies the queue at the start of a tick, sorted by (team, sequence), then steps. A future net layer only has to exchange command lists per tick.
- Units, buildings and crystal nodes live in plain `Vec`s inside the `Match` resource, in id order (ids are a global increasing `u32`, removal keeps order), so every loop runs in the same order on every machine. No hash-map iteration.
- Selection, camera, markers and the HUD are local UI state (`Ui` resource); they never feed the sim except through commands.

## Systems (from `catalog/genres.json`)

### Resource harvesting
- Crystal nodes sit on single blocked cells; each holds `amount` (map JSON). Workers ordered to gather walk next to a node, mine for `gather_s`, pick up `gather_amount` (or what is left), walk to the nearest finished own hub, and bank it; then go back. A depleted node sends the worker to the nearest node within 24 m, else it idles.
- Tunables: `kit.base_building_rts.gather_s`, `gather_amount`, `start_crystal`.

### Build queues and tech tree
- Buildings (`data/units.json`): command hub (HQ, trains workers), foundry (trains troopers and rumblers), bastion (defence turret). Each has hp, footprint (cells), cost, build time, sight, and the bastion a weapon.
- Tech tree (`requires` in JSON): foundry needs a hub; bastion needs a foundry; the rumbler needs a finished bastion.
- Building: a build order spends the cost at once and places a site; assigned workers walk to it and raise it (`build_s` divided by the number of workers, up to 3 count). Placement must be on passable, free, explored cells.
- Training: each producer has a queue of up to 5 (`queue_max`); the cost is spent when queued, the unit appears at the building edge after `train_s` and walks to the rally point. Supply cap `pop_cap`.
- Refusals ("Not enough crystal", "Needs a bastion", "Supply cap reached", "Can't build there") are notices shown in the HUD for 2 s.

### Drag-select and control groups
- Click or tap a unit to select it; drag a box (more than `drag_px`) to select every own unit inside. Click a building to select it. Click empty ground (mouse) to clear.
- Ctrl + 1/2/3 stores the selection as a group, 1/2/3 recalls it; G selects the whole army (every own trooper and rumbler; the touch "Army" button).
- Right click (mouse) or tap with a selection (touch) is the smart command: an enemy -> attack, a crystal node -> gather (workers; others move), own building under construction -> help build (workers), ground -> move; with only buildings selected it sets their rally point.
- F (touch "Attack") arms attack: the next click or tap on an enemy attacks it, on ground attack-moves (move, but fight what comes into sight). X (touch "Stop") stops. B (touch "Build") arms foundry placement, a second press bastion, a third cancels; the next click or tap places it with the selected workers (or the nearest idle worker).

### Flow-field pathfinding
- The map is a 64 x 64 grid of 2 m cells. Rock, water and buildings block; fords are passable.
- A flow field is a Dijkstra integration field (10 straight, 14 diagonal, no corner cutting, ties broken by cell index) from the goal cell to every reachable cell. A blocked goal (a building, a node) seeds every blocked cell of its footprint, so units stop at its edge.
- Every unit moving to the same goal cell shares one field: cost is one grid pass per destination, not per unit. Fields are cached (up to 24) and dropped when a building is placed or destroyed.
- Each tick a unit steers toward the lowest-cost neighbour of its cell (straight at the goal once in the goal cell), plus separation from nearby units (`separation`), and slides along blocked cells. A group arrives when within `arrive_spread * sqrt(group size)` of the goal, so groups spread instead of stacking.

### Fog of war
- Per team and per cell: unexplored, explored, visible. Every `fog_ticks` ticks, visible cells fall back to explored and every own unit and building marks the cells within its sight radius visible.
- The player sees: unexplored cells as dark fog (#1b2230), explored ones dimmed with their terrain and last-known enemy buildings, visible ones in full with enemy units. The minimap shows the same.

### Skirmish AI
- Thinks every `ai_think_s`: keeps idle workers gathering, trains workers up to `ai_workers`, builds a foundry then a bastion near its hub, queues troopers (every third unit a rumbler once a bastion stands).
- Attack waves: after `ai_first_wave_s`, when its idle army reaches the wave size (`ai_wave_size`, plus `ai_wave_growth` per wave), it masses then sends the whole wave attack-moving at the player's hub across a ford.
- Defence: player units within `ai_defend_radius` of its hub pull the AI army back to fight them.
- The AI reads the full map (no fog), as simple skirmish AIs usually do.

## Combat
- Units attack in range (`range` + radii) every `cooldown` seconds for `damage`. Idle and attack-moving combat units pick the nearest visible enemy within sight; workers only fight when ordered. Chasing stops beyond `chase_leash` of where it began for auto-acquired targets.
- Bastions shoot the nearest enemy unit in range. A building at 0 hp is removed and its cells freed.

## Win, lose and free roam
- Play: the rival hub destroyed -> "Victory"; the player's -> "Defeat"; `PlayOutcomes::game_over()` is reported once. Enter or a tap starts a new match.
- Free roam (`PlayMode::FreeRoam`, docs/PLAYGROUNDS.md): the same map with no win or lose, no game over and no AI attack waves (the AI still builds and defends its base). `KitPlugin::free_roam` is true and the `free_roam_holds` test drives it.
- Title screen ("Cinder Ridge", click or press Enter); P or Esc pauses ("Paused" banner, touch pause button).

## Camera
Catalog cameras: isometric, top-down. The kit uses a high three-quarter top-down view (pitch `cam_pitch_deg` 58, north up), panned by WASD or the arrow keys or the touch stick (`cam_pan_speed`), zoomed by the wheel, + and - (and the touch +/- buttons; the touch layer has no pinch, so pinch zoom is buttons), between `cam_zoom_min` and `cam_zoom_max`. H centres on the hub. Narrow (portrait) screens pull the camera back so the same width of map shows.

## Physics
The catalog says `simple`: `evani.toml` lists `physics = "simple"`. RTS movement is the kit's own grid steering; the physics module is only built in, as the catalog asks.

## Look and effects
- Look: `toon` (in `compatible_looks`, exists in the workspace). ROADMAP names `stylizedpbr`, which arrives at the join (S5-L2); the kit never depends on a look.
- Effects: `shadows` only, because contact shadows tell where units stand on the ground (readability). No bloom, vignette, depth of field or blur: strategy clarity first (playbook). Low tier has no shadow map; the kit pushes blob shadows under units instead. The art pass (K5b) chooses further effects.

## Data (JSON, `kit.data_file` + `include_str!`)
- `data/map.json`: grid size, cell size, terrain rectangles (rock, water, ford), crystal nodes, start positions, start workers.
- `data/units.json`: unit and building types with every stat, costs, `trains` and `requires`.
- Every stat in `units.json` is also a live parameter `kit.base_building_rts.<type>_<stat>` (for example `kit.base_building_rts.trooper_damage`), defaulting to the JSON value. Global tunables are `kit.base_building_rts.<name>` (see `src/lib.rs`, `tunables!`).

## Staying light (Chris, 2026-10-20)
- The map is a handful of terrain rectangles and node points in JSON (under 2 KB), expanded into the grid at start; no grid or lookup tables are baked into code.
- JSON is read with the `serde_json` evani-core already ships, so the kit adds no library to the download. Drawing uses only built-in meshes (no models, textures or fonts).
- Commands are small: a kind, a team, a target (a cell, a point or an id) and the list of unit ids. A lockstep tick with no orders is empty.
- Flow fields and fog are computed at run time (64 x 64 cells, a few kilobytes each), never shipped.

## Touch controls
evani-core `touch` is on. Layout (each names an action the keys or mouse drive): pan stick bottom left (`pan_x`/`pan_y`), buttons bottom right Build (`build`), Train (`train`), Heavy (`train_heavy`), Attack (`attack`), Stop (`stop`), Army (`select_army`), + (`zoom_in`), - (`zoom_out`), and pause top right (`pause`). Tap-select, drag-box and tap commands come from the primary pointer on the canvas.

## HUD
In-canvas greybox HUD on sprite layers 100 to 120: crystal and supply counter, mode line, selection panel with queue progress, key hints (touch hides them), minimap with camera box, banner, notices. Layout from `target/hud-layout.md`.

## Playtest bot (deferred)
Testing is deferred to one combined pass (Chris, 2026-10-20), so the K5a greybox ships no `playtest_win` / `playtest_lose` or `review_dump` tests. The bot planned for that pass: win = select army, attack-move the rival hub with a large army given a high start crystal; lose = do nothing with `ai_first_wave_s` at 10 s and the AI wave attacking the undefended hub.
