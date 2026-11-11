# evani-kit-scene-sunmoon-meadow

The Sun and moon meadow Scene (docs/SCENES.md): a free-flying camera, no
character, over Asset Studio column 38's best round (round 8, commit
f4266d5c): a golden-hour meadow of real grass, dark fields, far tree lines
and the procedural library's sun and moon discs.

- Keys and mouse: WASD fly, Space/E up, C/Q down, Shift faster, arrows turn;
  hold the mouse to look, or click to ask for the pointer lock.
- Touch: move stick (left), look pad (right), Up and Down, pause.
- `kit.scene_sunmoon_meadow.time` (the hour) and `day_minutes` (a whole day
  in real minutes, 0 holds it): day, dusk and a moonlit night. Round 8's
  golden hour is kept exactly; the day sky dims and clears as the sun
  climbs, and after sunset the dome's sun waits just under the horizon while
  the sky fades to navy (an afterglow, then night).
- `kit.scene_sunmoon_meadow.view`: 0 the column's view (the start), 1 a
  close look at the grass (captures and reviews:
  `tools/studio/asset-shoot.mjs --view-param kit.scene_sunmoon_meadow.view`).
- Kit feature `sky` (the app's `procedural`): the sky dome, the discs and
  the grass from evani-procedural-lib.
