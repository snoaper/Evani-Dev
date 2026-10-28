# HUD layout

The sandbox has one HUD element, `#sandbox-hud` (a DOM panel in `apps/sandbox/index.html`, declared in `[review] hud`). The debug overlay (top left) and the debug panel (top right) are developer tools, hidden in every review capture.

| Element | 1080p (1920 × 1080) | Phone (portrait, about 390 × 844) | Shows |
|---|---|---|---|
| `#sandbox-hud` | Bottom left, 12 px from the edges, about 300 × 50 px | Bottom left, 12 px from the edges; same size, so it stays clear of the grid in the middle | "Evani.Dev sandbox" and the one control: "Hold Space (or pad A) to spin faster" |

On a portrait phone the camera widens its field of view (up to 90°), rises and looks down more steeply so the sky stays the 14 to 17 % band along the top, and pulls back just far enough that the whole grid fits across the screen. The grid then fills the frame from under the far terraces and columns down to just above the panel; the frame's bottom edge meets the ground inside the terrace ring, so no terrace is cut by the bottom edge or sits under the panel. Sprites keep the same density as at 1080p (fewer on a small screen). The review captures the page at 390 × 844 portrait (`page-*-portrait.png`) as well as landscape.

There is no countdown, popup, score or results screen in the sandbox, so nothing changes during one: the panel stays as it is.

## Playbook

No genre playbook applies: `example` is a test kit, not a genre in `catalog/genres.json`, and `docs/playbooks/` has no entry for it. The items that would apply to any kit are covered elsewhere: grounded objects (every spinner rests on the ground on its lowest point while it turns, with a real shadow from Medium up and a shadow disc on Low; `evani-geocheck sandbox` checks it), readable colours (blue spinners, orange glow, green ground, blue sky) and a HUD clear of the action.
