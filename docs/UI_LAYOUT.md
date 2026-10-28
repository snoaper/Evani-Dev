# Box layout for menus and HUD panels (`evani-ui`)

`evani-ui` lays out in-canvas menus and HUD panels: a tree of row and column boxes built each frame, sized from the screen, then read back as a list of rectangles and text runs that the game draws. It has no dependency, no allocation and no randomness: the same tree and the same measure give the same boxes. Games get it through `ui = true` in `evani.toml` (the app's `ui` feature turns on `dep:evani-ui` with the sharp MSDF text only: the bitmap pixel font is the separate render feature `text`). Plan: docs/STAGE3_PLAN.md S3-13b. Working example: `apps/sandbox/src/hudpanel.rs` (build it with `trunk build --features ui` in `apps/sandbox`, then `cargo xtask fonts sandbox --dist <dist>`).

## Build, lay out, draw

```rust
use evani_ui::{Align, Cmd, Size, Style, Text, Ui};

let mut ui = Ui::new();                       // per frame, on the stack (about 24 KB)
ui.open(Style { size: [Size::Fixed(300.0), Size::Fit], pad: [12.0; 4], gap: 8.0,
                fill: Some(PANEL), ..Style::COLUMN });
ui.text(Text::new("Mission 3", 22.0, INK));   // a leaf: a run of text
ui.open(Style { size: [Size::Grow(1.0), Size::Fixed(8.0)], fill: Some(TRACK), ..Style::ROW });
ui.close();
ui.close();
ui.layout([x, y], [w, h], |t, max_width| text::measure_in(&rw, t.s, &style_of(t, max_width)));
for cmd in ui.commands() { /* Cmd::Rect { rect, color, depth } | Cmd::Text { text, pos, max_width, depth } */ }
```

- `Style::ROW` and `Style::COLUMN` are the starting points (fit size, no padding, no fill); override fields with struct update syntax.
- A box's size per axis is `Size::Fit` (its contents), `Size::Grow(weight)` (its contents plus a weighted share of the free room along the parent's main axis; across it, the parent's whole inside) or `Size::Fixed(px)`.
- When the contents are wider than the room, fit and grow boxes shrink toward their smallest size (text: its longest word; a scroll box: nothing), then overflow. Fixed boxes never shrink.
- `pad` is left, top, right, bottom. `gap` is between children on the main axis. `align` is `[x, y]`: along the main axis the children block moves as a whole, across it each child moves.
- `scroll: [None, Some(offset)]` makes a box scroll on y (or x) and clips its children to its inside. The offset is clamped to the range; `ui.max_scroll(id)` is the range, for a scroll bar.
- Text: `Text { width: Size::Grow(1.0), align: Align::Center, .. }` centres each line in the run's box. A fit run wraps as soon as its parent is narrower than the line.
- `layout` closes any box you left open. A tree holds `CAP` (128) boxes; the rest are dropped and counted in `ui.dropped()` (never a panic).
- Root boxes are laid out at `origin` in the `avail` area; one root with `Grow` sizes fills the screen, and its `align` puts the panel in a corner or the middle.

## Measure, paint, fonts

The measure callback is `|text, max_width| -> [width, height]`, with `max_width` 0 meaning one line. With the engine's MSDF font it is `text::measure_in(&rw, t.s, &style)` with `style.max_width = max_width`: layout and drawing then wrap identically, and a run is drawn with the `max_width` its `Cmd::Text` carries. Measure only once the font has arrived (`rw.fonts.ready(font)`): before that the measure returns an empty line. The text run's `font` field is yours to map to your font ids.

Paint in command order (a box before the boxes inside it). Give a box at `depth` the sprite layer `BASE + 2 * depth` and its text `BASE + 2 * depth + 1`, so inner boxes cover outer ones.

- Rectangles are already on whole pixels and cut to the scroll boxes above them.
- A text run that is not wholly inside the scroll boxes above it is left out (glyph quads cannot be cut). Keep scroll-list rows one run each and scroll by whole rows.
- Rectangle fills: draw each `Cmd::Rect` as one sprite with `evani_render::msdf::cell(0, Mode::Solid, 0)`, size = the rectangle, colour = its fill (see `fill` in `hudpanel.rs`). `Mode::Solid` is a plain hard-edged rectangle in the `ui` sprite shader, needs no font, and takes a translucent colour. Do not stretch an atlas square: its edge fades over a fixed number of texels and blurs at panel size.
- HUD after post: see the next section. Paint the panel on `HUD_LAYER + n` and its colours land on screen as written.

## HUD after post (the `ui` pass)

A game built with `ui` has one more render pass, `ui`, after the tone map and every post effect on `ldr` (order 900: after the vignette at 200, before `present` at 1000). It draws the HUD sprites with the MSDF sprite variant straight into the display-referred target, so a HUD colour lands on screen exactly as authored: linear 1.0 is 255, no bloom halo, no vignette, no exposure, no tone curve, on every tier and every screen.

What goes where, by sprite layer (`evani_render::msdf::HUD_LAYER`, 16384):

- **Screen HUD, layer `HUD_LAYER` and above:** panels, menus, buttons, scores, timers, health bars, anything fixed to the screen that reads as interface. It draws after post. Use `HUD_LAYER + 2 * depth` for boxes and `+ 1` for their text, as above.
- **The scene, below `HUD_LAYER`:** 2D world art, and text that belongs to the world (a name over a kart, a floating score where a match popped, a sign in the level). It keeps the scene's look: it blooms, is vignetted and tone-mapped like the art around it.
- Screen particles always draw in the scene, even if their layer is on the HUD layers.
- Games without `ui` have no `ui` pass; every sprite draws in the scene as before.

Colours are linear (sprites tint in linear light) and blend in linear light into the sRGB target, as in the scene. Pick HUD colours as numbers, not by looking at the scene.

The check: `node tools/hud-check/run.mjs` (`hud_white_stays_white`) captures the sandbox `ui` build on Lite, Low, Medium, High and Ultra and on a phone view and fails if the panel's white ink reads under 254 (under 200 on Lite and Low: their render scale shrinks the canvas, which the browser stretches, so thin strokes of every sprite are softer there; the colour is still exact), its fill differs from the authored colour, or a bloom halo spills over it. `cargo test -p evani-render --features msdf` runs `hud_draws_after_the_tone_map` (the pass order) and `hud_layers_split_after_the_scene`.

## Checks

The HUD-after-post checks are in the section above. `cargo test -p evani-ui` runs `layout_matches_golden_boxes`, `layout_does_not_allocate` (a counting allocator), `same_tree_same_boxes` and the rule tests (grow weights, wrapping, scrolling, alignment, a full tree, bad numbers).
