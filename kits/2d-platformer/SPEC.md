# Kit 002: 2D platformer (`2d-platformer`)

Roadmap row 002 · physics `simple` · first look `pixel16` · camera `side`.
Catalog entry: `catalog/genres.json` → `2d-platformer`. Reference games describe mechanics only; every name, character, level and logo here is original.

Crates: kit `kits/evani-kit-2d-platformer` (`KitPlugin::name()` = `2d-platformer`, owner `kit.2d-platformer`), look `looks/evani-look-pixel16` (new), demo `apps/2d-platformer` (game **Fennel's Leap**, level **Bramble Hills**).
Param names use `kit.platformer.*`: a param segment must start with a letter (`2d_...` is rejected), the owner stamp keeps the hyphenated kit id, and bake finds the crate from `BuildInfo.kit` (as kit 001 does). Data files are named `platformer.*`.

The point of this kit: prove the 2D path (sprites only, no meshes) and a tiny download. Every choice below prefers fewer bytes: no new shaders, art as indexed pixels in JSON, sounds synthesised at start, a small DOM UI.

## 1. Gameplay systems

Units: 1 tile = 16 art pixels ("px" below means art pixels, not screen pixels). Every number is a registered param (default shown) unless marked "data". Data is JSON shipped with `include_str!` and registered with `data_file`.

### 1.1 Kinematic character controller (coyote time, jump buffering, variable jump height)

Fixed 60 Hz tick, kinematic (no rigid bodies). `physics = "simple"` puts the physics module in the build; the kit reads `physics.gravity` when it exists (scaled by `gravity_scale`), else its own `gravity`.

- Hero body: an axis-aligned box `hero_w` × `hero_h` (10 × 14 px), drawn as a 16 × 16 sprite anchored at the feet.
- Run: accelerate towards `walk_speed` (90 px/s), or `run_speed` (150 px/s) while run is held, at `ground_accel` (700 px/s²) on the ground and `air_accel` (450 px/s²) in the air; with no input, `ground_friction` (900 px/s²) / `air_friction` (120 px/s²) slow it; turning against the motion uses `turn_accel` (1400 px/s²).
- Gravity `gravity` (980 px/s²) × `gravity_scale` (1.0), `fall_gravity_mult` (1.6, heavier on the way down), capped at `max_fall` (330 px/s).
- Jump: initial speed `jump_speed` (330 px/s; a running jump adds `run_jump_bonus` 25 px/s). **Variable height**: releasing jump while rising multiplies vertical speed by `jump_cut` (0.45). **Coyote time**: a jump is still allowed `coyote_s` (0.09 s) after walking off a ledge. **Jump buffering**: a jump pressed up to `buffer_s` (0.12 s) before landing fires on landing.
- Stomp bounce `stomp_bounce` (260 px/s; holding jump gives `jump_speed`).
- Hurt: touching an enemy's side, spikes or water costs a life; `invuln_s` (1.5 s) of blinking after a respawn.

### 1.2 Tile collision with slopes and one-way platforms

- The level is a grid of 16 px tiles. Tile kinds (data, `level.json` legend): empty, solid, one-way (top surface only), slope (a floor line from a left height to a right height, 0..16 px), water (hazard, palette-cycled), spikes (hazard), decoration (no collision).
- Slopes: 45° up/down (`/` 0→16, `\` 16→0) and gentle 22.5° pairs (0→8, 8→16 and 16→8, 8→0). The hero's foot point (bottom centre) follows the floor line; walking up or down a slope keeps the hero glued to it (snap down up to `slope_snap` 6 px when grounded). Slope sides never block sideways movement.
- Moves are resolved per axis: x first against solid tiles (box sweep), then y against solid tiles, one-way tops and slopes.
- One-way platforms: land only when falling and the feet were at or above the platform top on the previous tick; down + jump drops through (`drop_through_s` 0.2 s of ignoring one-ways).

### 1.3 Moving platforms

- Data (`level.json` `platforms`): width in tiles, a path of two or more points (tiles), speed (px/s), mode `pingpong` or `loop`, optional wait at the ends (s). Default speed `platform_speed` (40 px/s) when the data gives none.
- Platforms are one-way solid on top. A hero standing on one is carried by its movement each tick (before the hero's own move), and inherits its velocity when jumping off.
- The level has a horizontal platform over a wide water gap and a vertical one up to a high ledge.

### 1.4 Patrolling and jumping enemies

- **Bramble beetle** (patrol): walks at `beetle_speed` (28 px/s), turns at walls and at ledges (it never walks off), gravity like the hero.
- **Puddle hopper** (jumper): sits, then every `hopper_period_s` (1.6 s) jumps `hopper_jump_speed` (260 px/s) up, sideways at `hopper_hop_speed` (50 px/s) towards the hero when the hero is within `hopper_sight` (96 px), else in its facing direction; it turns at walls.
- Stomp: the hero lands on an enemy while falling with its feet within `stomp_window` (6 px) of the enemy top → the enemy is squashed (removed after 0.5 s), the hero bounces, `score_stomp` (100) points, shake trauma 0.25. Any other overlap hurts the hero.
- Enemies sleep until within `wake_distance` (320 px) of the camera, so the start is deterministic.

### 1.5 Collectibles and checkpoints

- Coins (data positions, plus coin arcs): `score_coin` (10) points each; every `coins_per_life` (50) coins gives a life.
- Checkpoint flags (data): touching one raises it, saves the respawn point and the time left; a lost life respawns there (enemies and coins are not reset).
- Goal: the castle gate at the end. Touching it wins: the time left × `score_time` (5) is added to the score.
- Lives `lives` (3, i32). A lost life with none left is game over (lose). The level timer `time_limit_s` (data, 200 s; overridable by the param `time_limit_s`, 0 = use the data) running out costs a life and resets the timer.

### 1.6 Tile-map level loader

- `data/level.json`: `name`, `time_limit_s`, `tile` (16), `legend` (char → tile kind and art), `rows` (strings of equal length, top row first), `entities` (start, coins, beetles, hoppers, checkpoints, goal, decorations by tile coordinate), `platforms`, `background` (parallax layer names and art).
- The loader validates (rectangular rows, known characters, a start and a goal, platform paths of two or more points) and returns an error string with the row and column; a bad file is a build error in the kit's tests, never a runtime panic.
- The level: **Bramble Hills**, about 210 × 15 tiles: flat start with coins and a beetle, a 45° hill, plank one-way platforms over a gap with a coin arc, a water pit, a gentle 22.5° valley, hoppers by a pond, a checkpoint, the horizontal moving platform over wide water, a vertical lift to a high ledge with spikes below, a last beetle pair, and the hilltop castle gate.

## 2. Camera

`side` (the only catalog camera of this genre; pixel16 supports it). A 2D camera in art pixels, not a 3D camera, framed for reading the ground ahead (Chris's review of the first build: no more screens that are mostly dirt):

- **Ground line in the lower third.** The camera keeps an *anchor*: the feet height of the ground the hero last stood on. Its ground line eases to the anchor (`cam_smooth_y` 5 /s) and is drawn at `cam_ground_frac` (0.72) of the view height from the top, so about a quarter of the screen is ground and the rest is sky and what is ahead. The view may rise above the level's top (sky); only the level's bottom clamps it.
- **Vertical dead zone.** A jump does not move the camera: it follows up only once the hero rises more than `cam_deadzone_y` (72 px) above the anchor, and follows down at once when the hero drops below it (never more than 24 px behind).
- **Look-ahead.** X follows with a dead zone (`cam_deadzone_x` 24 px) and looks `cam_lookahead` (32 px) ahead in the direction of movement (facing while nearly still), eased at `cam_smooth` (4 /s).
- Visible height: between `view_height_px` (224, the look's 320 × 224 rule; 14 tiles) and `view_height_max_px` (288, 18 tiles) art pixels, whichever whole-number scale lands there (below). Visible width follows the window, from 256 up to `view_width_max_px` (640) art pixels; a wider window (an ultrawide monitor) frames the view, centred, with black bars at the sides. Enemies wake at `wake_distance` (336 px) from the camera, more than half the widest view, so none is seen asleep. Respawn and reset put the camera at rest on the hero (`Cam::at`).
- Checked by a kit test: over a whole bot run at 1920 × 1080, the hero stays well inside the frame, and once the hero has stood still for a second, terrain below the feet covers at most 35 % of the screen (worst 28 %) with the feet at 60–80 % of the view height.

Screen mapping: the renderer's target size gives device pixels per art pixel. When the look asks for crisp pixels (`render.pixel_art`, §6), that scale is a whole number: the smallest that shows at most `view_height_max_px` rows, `ceil(h / 288)`, unless that shows fewer than `view_height_px` rows, then `floor(h / 224)`; at least 1, and a portrait phone keeps at least 256 art pixels across. So 720p: 3 (240 rows), 1080p: 4 (270 rows), 1440p and a 3440 × 1440 ultrawide: 5 (288 rows; the ultrawide framed at 640 across), a 3440 × 1440 screen at 125 % Windows scaling: 5 (278 rows), an 844 × 390 phone at dpr 3: 5 (234 rows). Each axis maps by its own CSS-to-device ratio, and the renderer sizes a pixel-art canvas from the browser's exact device-pixel box (`evani-render` feature `pixel-art`, on for this kit), so at a fractional devicePixelRatio no row or column of art pixels is doubled. The camera, every sprite, the parallax planes, the floating text and the HUD are snapped to whole art pixels, so every art pixel is the same number of screen pixels. The renderer renders pixel art at full device pixels on every tier (no DPR cap, render scale or adaptive-quality scale), so Low keeps the grid whole too. Otherwise the scale is fractional and positions are not snapped. Shake (§5) moves the camera by whole art pixels.

Title screen: the attract demo. The kit's lookahead bot plays the level once at start and its input is recorded with snapshots; playback is a pure function of `Time::render_seconds()` (0 s: the first jump's apex, 20 s: a stomp, 40 s: the castle gate), so `capture.frame` at held sim times always shows the same frame. Capture moments (§7) are states of the same recording. The app still inserts a still `BenchPath` for the 3D camera, which this kit doesn't use.

## 3. Physics

`simple`, exactly as the catalog says: evani.toml `physics = "simple"`. The controller and enemies are the kit's own kinematics (§1.1–1.4); the physics module is still a stub and supplies `physics.gravity` (m/s², converted at 16 px per metre × `gravity_scale` 6.25 so 9.81 m/s² gives about 980 px/s²).

## 4. Look

`pixel16` (row says `pixel16`; new look crate `looks/evani-look-pixel16`, catalog `cost_points` 1, style 2D). Render rules from the catalog, and how each is met:

- **320 × 224 internal**: the kit shows 224 to 288 art pixels of height (`view_height_px`, `view_height_max_px`) at a whole-number screen scale (§2). Exactly 224 at a whole scale only fits screens whose height divides by 224, so the view shows a little more rather than cropping below 224 or scaling unevenly (a deliberate stretch of the catalog's rule, noted in the hand-over). Width follows the window up to 640 art pixels, then the view is framed.
- **Up to 256 colours with per-layer palettes**: the look registers named palettes: 15 colour ramps `ramp.<name>` (4 shades each, dark to light; ink, fur, teal, green, leaf, brown, stone, gold, red, blue, water, sky, white, shadow, soil) and per-layer tints `layer.far`, `layer.mid`, `layer.near` (multiplied into the parallax planes, cooler and lighter with distance) and `sky` (6 bands, top to horizon). Colours are on the 15-bit grid (5 bits per channel) of 16-bit hardware. The kit's art is indexed (each pixel names a ramp and a shade), so a different look recolours everything.
- **16 × 16 tiles**: the kit's tiles and sprites are 16 × 16 (HUD glyphs 8 × 8).
- **Terrain with depth**: earth darkens with depth below its column's surface (brown, then the darker `soil` ramp, bands joined by ordered dither and blended across columns), with roots under the grass and 16 hashed details (pebbles, stone rocks, boulders, rare bones and fossils) so no two screens repeat. The stone column is a deliberate pillar (a mossy cap slab you can stand on, an ashlar shaft, a plinth where it meets the ground), with stone bonded into the earth beside it.
- **Several parallax planes**: three planes (far hills with the castle, mid hills, near bushes) plus the banded sky, through the `parallax` effect (§5).
- **Palette cycling for water and fire**: the kit builds four frames of each water and torch-fire tile by rotating that ramp one step per frame, and plays them at `cycle_fps` (6).

The look also sets `render.pixel_art` true (nearest sampling and whole-pixel scaling, §6), a `tonemap` slot body that passes colours through unchanged (exact palette colours, no filmic curve), `render.exposure` 1.0, `render.saturation` 1.0 and the sky colour as `render.clear_color`. It asks for `parallax` on.

The kit asks the look for its palettes by name and uses its own default ramps (in `art.json`) for any the look lacks, so it runs with any look; with `basic` it draws the same art smooth-scaled in the kit's default colours.

## 5. Effects

Start from none; each kept only if a capture is clearly better (kit process §7).

| Effect | Cost | Why |
|---|---|---|
| `parallax` (new) | 1 | Sells the look: pixel16's "several parallax planes". No render pass: params only (`fx.parallax.far`, `mid`, `near` scroll factors and `vertical`), and the kit draws the planes while the effect is on (a flat sky without it). |
| `shake` (existing) | 0 | Reads impacts: stomps, hurts and the goal give trauma; the kit offsets its 2D camera by whole art pixels (amount 0.35 m = 5.6 px at full trauma). Comfort-off in VR. |

Not used: bloom, vignette, outline, speedlines, crt, grain, chroma (none helps the player read a platformer or belongs to the 16-bit look, and each would add bytes); `dither` (the art is already on the palette).

Start-up per tier (`plan_budget`: look 1 point plus effects; `effects_on` in `assets/tiers/*.json` gains `parallax`):

| Tier | Points | On at start | Used |
|---|---|---|---|
| Low | 7 | parallax, shake | 2 |
| Medium | 11 | parallax, shake | 2 |
| High | 16 | parallax, shake | 2 |
| Ultra | 24 | parallax, shake | 2 |
| VR | 9 | parallax (shake off by VR comfort) | 2 |

Sprite counts: a 1440p Ultra frame shows at most about 41 × 19 tiles at scale 5 (the framed ultrawide view is the widest), plus parallax planes, entities and the HUD: under 2,000 sprites, far below Low's 5,000.

## 6. Core additions this kit needs (small, additive)

1. `evani-render`: **kit sprite sheet.** A resource `SpriteSheet { width, height, rgba8, frames: Vec<[u32; 4]>, revision }` (frames are pixel rectangles). When present, the renderer puts it in the atlas texture below the built-in 4 × 4 cells and sprite `cell` 16 + i draws frame i. Built-in cells 0..15 are unchanged. Re-uploaded when `revision` changes.
2. `evani-render`: **`render.pixel_art`** (bool param, default false). When true the atlas is sampled with nearest filtering and sheet frames use exact edges; looks turn it on with `post_param`. Kits read it to choose whole-number scaling.
3. `evani-render`: **`RenderWorld.target_px`**, the render target size in device pixels (set by the renderer next to `viewport_css`), so a 2D kit can pick a whole number of device pixels per art pixel.
4. Mirrored sprites need nothing new: a negative `Sprite.size[0]` flips the quad (the sprite pipeline doesn't cull).

Recorded in DECISIONS.md when built.

## 7. Demo (`apps/2d-platformer`)

**Fennel's Leap**: title screen (Play / Settings) over the attract pan; one level, Bramble Hills; HUD across the top drawn by the kit in pixel glyphs (lives, coins, score, timer); pause menu (resume, restart, settings, quit to title); settings (tier, effect toggles for what is compiled in, key rebinding, volume); results: "Level clear!" with the score tally (win) or "Game over" (lose). Menus are light DOM. Sounds (jump, coin, stomp, hurt, checkpoint, life, win, lose) are synthesised at start.

Controls (rebindable, saved): move `keys:KeyA/KeyD` and arrows, jump `Space` / `KeyW` / `ArrowUp` / `KeyK`, run `ShiftLeft` / `KeyJ`, down `KeyS` / `ArrowDown` (with jump: drop through a plank), pause `Escape` / `KeyP`; gamepad stick or d-pad, A jump, X run, Start pause.

HUD: drawn by the kit on the art pixel grid (one HUD pixel is one art pixel): lives and coins on the left, the score in the middle, the timer on the right, 8 px from the edges. It needs about 190 art pixels across and fits the narrowest view (256; a portrait phone shows 270).

Floating text: score pops and "1UP" rise 12 px with an ease-out and fade over their last 45 % within 0.8 s; points won within 0.5 s of a score pop merge into its running total (a coin line or a stomp chain shows one combo). A pure screen-space layout keeps every pop on whole art pixels and 1 px clear of the hero and of other text, pushing it up when needed.

Capture moments (live builds only, `[review] moments` in evani.toml; core `CaptureMoments`): `start` (level start at the signpost), `stomp` (the bounce off a stomped beetle, its points rising), `power_up` (an extra life: the coin count is set to 49 just before a coin taken in mid-air, so the 50th raises "1UP"; the kit has no power-up item), `checkpoint` (a flag just raised), `spikes` (the hero on the lift beside the spike pit) and `goal` (in the castle gate). Each is a state of the attract recording, shown exactly at its tick.

## 8. Reference image prompt

```
In-game screenshot from an original 2D platformer video game, side-on 2D camera following the hero.
A small fox-eared hero in a teal scarf leaps between floating grassy platforms over a gap, a trail of gold coins arcing overhead, a shelled beetle patrolling below and a hilltop castle on the horizon. HUD: lives, coin count and timer across the top.
Art style: 16-bit pixel. Richer palettes, layered parallax, detailed sprite animation.
Crisp, polished 2D at native resolution: clean edges, readable silhouettes, rich colour and lighting where the style allows.
16:9 widescreen game capture, 2560×1440. Everything is original: characters, vehicles, creatures, logos and UI. No existing game franchises, characters, brands or real people, and no watermark. Any on-screen text is short and legible.
```
