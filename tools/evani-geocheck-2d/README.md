# evani-geocheck-2d

The 2D half of Evani.Dev's geometry integrity checker. A kit's dump harness
writes a JSON scene; this tool runs Chris's seven 2D checks on it and writes a
report with a highlighted crop per issue. Native dev tool only: it never
reaches a game download, and it does not link the engine (plain Rust, serde,
toml, png), so any kit on any engine version can feed it.

```
cargo run -p evani-geocheck-2d -- --scene <dir>/scene.json --out <report dir> [--tags <geocheck.toml>]
```

For a game whose kit has a scene dump (see "How a kit adds a dump"), one
command does both steps: `cargo xtask geocheck <app>`.

Writes `<out>/report.md`, `<out>/report.json` (issues grouped by type with
counts, location, crop path, fix and a `tagged` flag) and `<out>/crops/*.png`.
Exit code 0 when every issue is tagged, 1 when any untagged issue is left,
2 on an error (bad scene, bad tag).

## Scene format (`evani-geocheck-2d/1`)

All world positions are art pixels, `y` down; rects are `[x, y, w, h]`. Every
field is optional unless a check needs it (`src/scene.rs` has the full list).

| Field | What |
|---|---|
| `name`, `source`, `notes` | Labels and free-text notes copied into the report. |
| `tile_size`, `map` | Tile size (art px) and map size in tiles. |
| `pixel_scale` | Device pixels per art pixel the game uses (the one scale). |
| `pixel_art` | `true` (default) for pixel art. A smooth-art game (shapes drawn at any size, like a cartoon look) sets `false`: check 4 does not apply, and the report lists it as not run with the reason. |
| `background` | `#rrggbb` behind crops. |
| `atlas` | `{image, width, height, frames: [[x,y,w,h]], fills: [frame ids]}`: the sprite sheet (PNG, or raw RGBA8 when it ends in `.rgba`) relative to the scene; `fills` are flat-colour frames that may stretch. |
| `layers` | `{world_min, hud_min}`: sprites on `hud_min` and up are screen UI. `layer_names` names layers for messages. |
| `groups` | Autotile groups: `{caps: "tlr", meets: ["other group"]}`: sides that need an edge when open, groups it may touch without a transition. |
| `tiles` | `{x, y, id, layer, shape: none/full/top/slope, slope: [left, right], group, edges: "tlr", transition, hazard, frame}`; `edges` are the sides the autotiler drew as an edge or cap. |
| `objects` | `{name, kind, rect, frame, flip, layer, support: ground/wall/none, foot, hitbox, exclusive}`; `exclusive` names a group whose members must not overlap each other (pieces in a grid, screen panels); kinds: collectible, enemy, prop, decor, hazard, platform, goal, player, other. |
| `platforms` | Moving platforms: `{name, width, path: [[x, y]]}` (top-left along the path). |
| `player` | Movement for the bot models: collision `w`/`h`, `gravity` (rising), `fall_mult`, `max_fall`, `jump_speed`, `run_jump_bonus`, `jump_cut`, `walk_speed`, `run_speed`, `start` (foot point), `sprite` size. |
| `bot` | A bot run: `{dt, completed, end_tick, ticks: [[x, y, grounded]], touched: [object names]}`. |
| `frames` | Drawn frames: `{tick, t, scale, cam, view, player, grounded, sprites}`; `sprites` (optional) are the frame's drawn quads in device px `{f or builtin, x, y, w, h, l, c}` (negative `w` = mirrored, `c` linear tint). |
| `sweep` | Extra drawn frames for check 8 only, same shape as `frames` (with `sprites`): one per whole-art-pixel camera step, so joins that open only at some camera positions are seen. The camera and popup checks never read them. |
| `popups` | Text popups sampled while alive: `{id, text, samples: [{tick, t, rect, alpha, player}]}`. |
| `limits` | Thresholds (defaults below). |
| `geocheck` | Tags (intentional cases), see below. |

## The checks

1. **Overlaps** (`clear_air`, `overlap_terrain`, `overlap_decor`,
   `overlap_object`): collectibles
   need `air_px` (2) of air from terrain and opaque decor pixels; enemies,
   props and the goal may sink at most `overlap_px` (3) into terrain or decor.
   Uses atlas pixels when there is an atlas, else rects. Terrain is tile
   shapes, not tile art. Kits must keep at least 2 px of clear air between
   collectibles and the ground below them (Chris's "clear air" rule).
   Objects sharing an `exclusive` group, of any kind, may overlap each other
   by at most `overlap_px` (`overlap_object`).
2. **Grounding** (`floating_decor`, `floating_prop`, `enemy_not_grounded`,
   `overhangs`): decor, props and
   enemies with `support: ground` must stand on a floor (gap at most
   `float_px`, sinking at most `sink_px`, `support_frac` of the foot on
   something); `support: wall` needs something behind it.
3. **Seams** (`seam`, `inner_edge`, `uncapped`): different groups touching need
   a transition tile or a `meets` entry; same-group neighbours must not draw an
   edge between them; open sides listed in `caps` must show one (a slope side of height 0 needs no cap). Trusts the
   dump's `edges`; does not read tile art (so a slope whose art notches at tile joins
   passes). Map borders are skipped.
4. **Pixels** (`frame_scale`, `stretched`, `mixed_scale`, `hud_scale`,
   `subpixel`): every frame at the one whole-number `pixel_scale`; every
   atlas sprite drawn 1:1 in art pixels (fills excepted), HUD included; world
   sprites on whole art pixels. Stretch and scale findings join per layer:
   a sprite drawn stretched in some frames and at another uniform scale
   (say half size) in others is one issue listing both symptoms (one cause:
   the sprite is scaled when drawn, e.g. a shadow sized by height), typed
   `stretched` (`mixed_scale` when it is never stretched).
   Needs `frames[].sprites`; without them only placed object rects are
   checked for whole pixels. No rotation in the dump. Not run when the scene
   sets `pixel_art: false` (smooth art); the report says so.
5. **Camera** (`player_too_high`, `ground_not_visible`, `terrain_heavy`): on
   grounded frames the feet sit between `zone_top` (2/3, the lower third) and
   `zone_bottom` (0.97) of the view height, and terrain below the feet fills
   at most `max_terrain` (35 %). Runs shorter than `camera_min_s` (0.3 s) are
   ignored. Landing grace: right after landing on a new height (feet more
   than 1 px from the last grounded height) the feet may sit outside the
   zone for up to `land_grace_s` (1 s) if the camera moves towards them over
   that time; a run is reported only when it lasts past the grace or the
   camera does not follow. Why: a camera with a vertical dead zone only
   follows after a landing (it must not bob with every jump), so a brief
   catch-up out of the zone is by design. Sampled frames only.
6. **Bot** (`not_completable`, `unreachable_collectible`, `coin_arc`,
   `hazard_hidden`): the bot run must finish; every collectible must be touched
   by the bot or by the reach model (standable spots every 8 px plus platform
   paths, jumps at 7 speeds with 4 jump-cut timings and walk-offs using the
   scene's physics, seeded from the start and the bot's footholds); groups of
   3+ coins laid out as an arc must fit a real jump (rise at `gravity`, fall
   at `gravity * fall_mult` capped at `max_fall`, one horizontal speed up to
   `run_speed`) within `arc_tol_px` (6), with suggested coin moves; decor in
   front of a hazard may cover at most `hazard_cover` (30 %). Limits: no air
   steering, take-off at full speed, no enemies, stomps, springs or
   drop-throughs; a platform is standable anywhere on its path.
7. **Popups** (`popup_covers_player`, `popup_overlap`, `popup_lingers`):
   popups visible (alpha at least `popup_min_alpha`, 0.1) must not overlap the
   player's drawn rect or each other on the same tick, and must be gone within
   `popup_max_s` (1 s). Uses the dumped text boxes, not glyph pixels.
8. **Joins** (`join_gap`): sprites of one world layer that meet edge to edge
   (a parallax strip repeated across the view, sky bands, a plane and the
   fill under it) must leave no gap. Per frame (`frames` and `sweep`) and per
   layer it marks each art pixel as covered by a sprite rect and as opaque
   from the atlas, then flags runs of at most `join_gap_px` (2) uncovered
   pixels with opaque art of the same layer on both sides, joined into lines
   at least `join_line_px` (4) long. A transparent pixel inside a sprite
   (an outline drawn into the art) is never a gap; only the part of a line
   the player sees counts (gap pixels under opaque art of a nearer layer,
   ground or HUD, are hidden; the crop marks the visible stretch, 2 px out
   so the line shows inside the mark); HUD layers are not checked;
   tints under 0.5 alpha count as see-through. Grouped per layer and sprite
   pair; the message lists the sweep camera positions where it opens. Tag an
   intended gap by object `"<layer name>: ..."` (for example
   `object = "far hills*"`). Needs `frames[].sprites`; not run on smooth art.
   Typical cause: each copy of a strip rounded on its own with
   round-half-away-from-zero, so `-1.5` goes to `-2` but `510.5` to `511`.

Issues of one kind that repeat (the same sprite in many frames, the same popup
text) join into one issue with a count.

Crops: magenta outlines mark the problem; cyan dots draw the real jump arc for
`coin_arc`. Level crops are drawn from the atlas (tiles and objects), frame
crops from the frame's own sprites; a frame crop narrower than 480 px is
scaled up by a whole factor (nearest-neighbour, up to 8x) so a one-pixel join
reads at normal size.

## Tags (intentional cases)

A tag names a check (slug `seams`, number `3`, or one type such as `seam`) and
an `object` (exact name, or a prefix ending in `*`) or a place (`at` in tiles,
`radius` in tiles, default 1). `reason` is required. Tagged issues are listed as
skipped with the reason and never fail the run.

In the level data, copied into the scene's `"geocheck"` array:

```json
"geocheck": [{"check": "coin_arc", "object": "coin 8 .. coin 14", "reason": "a dive arc: the player is meant to drop through it"}]
```

or in `geocheck.toml` next to the scene:

```toml
[[tag]]
check = "seams"
at = [108, 7]
radius = 8
reason = "the stone column is the castle's outer keep, drawn without grass on purpose"
```

## How a kit adds a dump

Write a native example `kits/evani-kit-<kit>/examples/geocheck_dump.rs` (dev
tooling: examples never reach a game download) that takes the output folder
as its one argument and:

1. Builds the kit's level and writes `tiles` (shape, autotile group, the edges
   its autotiler draws, hazards, art frame), `objects` (as the draw system
   places them: drawn rect, frame, layer, support) and `platforms`.
2. Writes `player` from the kit's tuning (gravity as the game applies it).
3. Plays the level with the kit's own bot, recording `bot.ticks` (foot point
   and grounded each tick), `touched` collectibles and whether it finished.
4. Samples frames (every few ticks, on landings, after popups spawn) and runs
   the kit's real draw system on each, writing `cam`, `view`, `scale`, the
   player's rect and, for some frames, the drawn `sprites`.
5. Records each popup's rect and alpha every tick while it lives.
6. Writes the sprite sheet (`atlas.rgba` or a PNG) and its frame rects.

Then `cargo xtask geocheck <app>` runs the dump into
`.vibedeck/geocheck/<app>/dump/` and this tool on it, writing the report in
`.vibedeck/geocheck/<app>/`. The 2D platformer's dump
(`kits/evani-kit-2d-platformer/examples/geocheck_dump.rs`) is a worked
example; it takes placement from the kit's own functions where the kit has
them (coin and popup placement, the screen mapping), so it can't drift from
what the game draws.
