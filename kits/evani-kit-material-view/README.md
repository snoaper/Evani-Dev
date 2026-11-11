# evani-kit-material-view

The material viewer's kit: a game for viewing and tuning materials, and the
in-engine home of Evani.Materials (one room per world).

* `data/library.json`: scenes (shader ball, cushion, head, ground, pool, lamp
  corner), the four material families with their materials (presets over the
  registered params) and the rules that sort params into Base, Finish, Detail
  and Quality.
* `data/lights.json`: the four fixed light presets (Studio, Overcast, Sunset,
  Night lamp).
* `data/world_rooms.json`: the 24 world rooms and their material slots by
  Meridian Vale zone.
* `src/viewer.rs`: the viewer state and the commands the page and the keys
  send; lights are written only when the viewer changes them.
* `src/orbit.rs`: orbit camera and turntable (fixed ticks).

Params: `kit.material_view.{orbit_speed,zoom_speed,turntable_speed,key_orbit_speed,split}`.
Keys: arrows or WASD orbit, Q/E or -/= zoom, R reset, T turntable, C compare,
N next scene, L next light, Esc or P pause. Touch: Reset, Spin, pause.
The scenes are drawn by the app (`apps/material-view/src/scenes`), one module
per family behind its `material-<family>` feature.
