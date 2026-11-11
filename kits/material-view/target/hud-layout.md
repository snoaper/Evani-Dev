# HUD layout

The HUD is DOM over the canvas (`apps/material-view/index.html`; the styles
dock and library CSS in `src/panel.rs` and `src/panel_catalogue.rs`, filled
by `src/web.rs`). Every part is sized from the room it really has: the page
measures the styles dock (`--dock-h`) and the bottom stack (`--bottom`: the
bar, the scene row and the dev strip, however the bar wraps) and places the
side panels between them; the art styles scene fits its shapes into the part
of the view no panel covers. Long names end in "..." and keep their whole
text as a tooltip. Strips that scroll sideways (style tabs, chips) fade at
the edge that has more.

## Parts

| Part | What it shows |
|---|---|
| HUD (top left) | "Evani.Materials", the open material's name, and "family · scene · light" (a part that repeats the one before it is left out) |
| Styles dock (top) | The 24 art-style tabs, the open style's header (name, a few original words, status, note, shape look), its material chips and the shape chips (header and chips in the art styles scene only) |
| Library drawer (left) | Library, Families and World rooms tabs; search, filter chips, categories with ready/total counts, the rows |
| Tuning panel (right) | The open material's params by group (Base, Finish, Detail, Quality), resets, the frame time |
| Bar (bottom) | Browse, Tune, the four light presets, the sun dial, Spin, Compare (and Save as A while comparing) |
| Scene row (bottom) | Seven scene tiles: Art styles, Shader ball, Cushion, Head, Ground, Pool, Lamp corner |
| Touch buttons (top right, touch screens) | Spin, Reset, Pause |

## Desktop, 1920x1080 (and 2560 wide)

- The styles dock across the top, 12 px from the edges; the HUD under it on the left, the drawer under the HUD (280 px wide), the panel on the right (330 px) under the dock; both stop above the bar.
- The bar centred at the bottom on one row, the scene row under it; no button scrolls out of the bar.
- At 2560 px the same, with the shapes fitted into the wider open middle.

## Laptop, 1280x720 (also at 125 % and 150 % Windows scaling: 1024x576 and 853x480 CSS px)

- As desktop; the bar wraps onto a second row when the window is too narrow for one, and the side panels shorten to stay above it (`--bottom`, measured).

## Phone landscape (844x390) and short windows (height up to 520 px)

- The styles dock moves to the top edge, left of the touch buttons: the style tabs on one row, a one-line header (name and words, which end in "..."), then the material chips and the shape chips side by side.
- The HUD sits under the dock, smaller; the bar buttons and the scene tiles are smaller; the dev drop strip is hidden; the view keeps the middle of the screen.
- The drawer and the panel start closed; Browse or Tune opens them as sheets.

## Phone portrait (390x844), tablets, and windows under 760 px wide (the 700 px half window)

- The styles dock under the touch buttons, the HUD top left; the bar wraps to two rows above the scene row.
- Going narrow closes the drawer and the panel, so a page that started wide never shows the view covered; Browse opens the library as a full-height sheet with a Done button, Tune opens the panel as a bottom sheet.

## Popups and states

- Compare: the split line with the "A saved" and "B now" labels on the view; "Hold still: copying the saved look" while it copies.
- References: a docked panel on the right (desktop) or a full-screen sheet with Close (narrow).
- The review capture hides the debug overlay in every shot.
