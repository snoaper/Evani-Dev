# Content rules for detailed models (Stage 3, S3-11)

For a game that turns on `models` in `evani.toml`. A game without `models` has none of this code.

## The four rules

1. **Hero assets come from detailed models.** Anything the player looks at closely (a kart, a soldier, a landmark) is a real model with named parts, not a heap of primitives. Primitives are the placeholder while it streams.
2. **Kits or recipes assemble parts into variants.** One detailed base plus named parts gives many looks: hide a node, recolour a node, attach another model at an anchor. The variant is a recipe in the kit's JSON, so live tools can change it and no new model file is needed.
3. **Looks provide shader micro-detail.** Surface grain, relief and wear come from the look (S3-10) and its shaders, not from extra triangles or extra textures in the model.
4. **Detailed assets stream after the title, by tier.** Only the placeholder-sized boot group blocks the title screen. Every other model arrives afterwards, as `<id>.<tier>.glb` for each tier it needs (`<id>.glb` is the shared look for all tiers). Phones fetch the light files; the look stays the same on every device.

`cargo xtask check` warns when a game breaks rule 4 (section 5).

## Hide and recolour named nodes

Names are the node names in the file (Evani.Model writes them from the part names). A model keeps at most 32 distinct node names, sorted; a node's bit is its rank in that sorted list, so the same node has the same bit in every tier file of the model.

```rust
let body = models.node_bit(id, "body");          // Option<u32>, once
let rear = models.node_bit(id, "wheel_rl");
models.draw_with(
    ModelInstance::new(id, pos).hide(rear.unwrap_or(0)),
    &[(body.unwrap_or(0), [0.9, 0.2, 0.1, 1.0])],   // (node bits, colour)
);
```

- `ModelInstance::hide(bits)`: parts under any of those nodes are not drawn at all (not counted in draws or triangles).
- `draw_with(instance, paints)`: each paint multiplies the base colour of the parts under its nodes. It never replaces the colour, so a white (untinted) material takes the paint whole and a textured one is tinted.
- `node_bit` returns `None` until the model is read, and for a name the model lacks. Look names up once (when `resolve` first gives an id, again after a tier change), keep the bits, and use them every frame. A frame does no string work and, once the lists have grown, no allocation.
- The paint list is kept for the frame only; call `draw_with` each frame like `draw`.
- All of this is render data. The simulation never reads it.

## Anchors

A node called `anchor:<name>` in the file is an attachment point.

```rust
let slot = models.anchor(id, "item_slot");                 // Option<u16>, once
let frame = models.anchor_frame(id, slot?, &at);           // Option<Frame>, per frame
```

`anchor_frame` gives the world `Frame` (position, rotation quaternion, scale) of the anchor when the model stands at `at`, so a part drawn with `ModelInstance::at(part, &frame)` follows the base through any turn and scale. Anchors are for drawing.

**Sockets** are anchors with a rotation (a weapon grip in a hand): the model writes `extras.evani.sockets.<name> = {p, q}` and `cargo xtask convert` turns it into a turned `anchor:<name>` node (docs/MODEL_FORMAT.md "Sockets"). Nothing changes in the game code: `anchor_frame` already returns the base's turn times the socket's, so a model built round its grip (origin at the grip) drawn with `ModelInstance::at(weapon, &frame)` sits in the hand, and a point `l` of it lands at `at · (q·l + p)`. The frame is the rest pose: on a skinned model the socket does not follow the animated joint yet. Where a seat or a hit point is for gameplay comes from kit data or the `boot` group, never from a streamed model.

## Variant recipe

In the kit's JSON:

```json
{
  "base": "kart",
  "hide": ["wheel_rl", "wheel_rr"],
  "paint": { "body": [0.9, 0.2, 0.1] },
  "parts": [
    { "model": "crate",  "anchor": "item_slot", "colour": [1.0, 0.8, 0.4] },
    { "model": "barrel", "anchor": "exhaust.l" }
  ]
}
```

- `base` and each part's `model` are model ids; `hide` and `paint` name nodes of the base; `anchor` names an anchor of the base. Colours are linear RGB or RGBA and multiply the base colour (default white).
- `let mut v = Variant::from_json(&mut models, &json)?;` makes the handles. Each frame, `if !v.draw(&mut models, Frame::new(pos)) { draw the placeholder }`.
- `draw` is all or nothing: it returns `false` until the base and every part that can load are ready, so a variant never shows half built. A part whose file failed to load is skipped.
- Names are resolved once per base file. Any the base lacks are listed in `v.unresolved` (`node wheel_x`, `anchor foo`); log them once as an error. Those entries are skipped.

## Level groups (S7-7, feature `level-groups`)

A game with several levels downloads each level when it is reached, not all at once. Two steps:

- **Name the levels** in `evani.toml`: `[stream.levels]`, one line per level, listing model ids (or exact file names): `forest = ["tree", "rock"]`. A name takes the file with that name and every file whose name is the id before its first dot (`tree.glb`, `tree.low.glb`, `tree.low.base.bc7.ktx2`; not `treehouse.glb`). `cargo xtask manifest <id>` writes those files into `stream.json` as group `level:forest` (a file in `[stream] boot` stays in `boot`). A file in two levels, or a name that matches no file, is an error with a `fix:` line; a level group counts toward the package size like any other file.
- **Turn the feature on** in the game's Cargo.toml (`evani-render` feature `level-groups`; it implies `models`). Games that do not are byte for byte what they were, and `stream.json` without level groups behaves as before.

At run time (render only; the simulation never reads it):

- A level group is **left out of the background plan**. Nothing of a level downloads until the game asks, either by `Models::prefetch("level:forest")` (downloads the settled tier's model files and the one texture set this device samples, at low priority, into Cache Storage; asking twice is free) or simply by making the handles when the level starts (a prefetch just makes that a cache hit).
- `Models::release("level:forest")` frees the group's models: GPU textures, buffers and bind groups go away at the next frame, and their texture bytes come back off the tier's `max_texture_memory` budget, so the next level's textures fit. It returns the texture bytes it will free; `Models::texture_bytes()` reads what the loaded models hold. The handles draw the kit's placeholder (`resolve` is `None`) and stay idle: a released level is never fetched again by itself.
- `Models::prefetch` on a released group lets it back in: its handles load again, from Cache Storage (large files) or the HTTP cache, with no new network use. Each release leaves one small dead row per model in the model table, so release a level a handful of times a session, not every frame.
- Call `prefetch` for the next level while the player is still in the current one; call `release` for the old one once the new one is drawn. Gameplay data (anchors, sizes, collision) still comes from kit data, never from a streamed level group.

## Checks

Both are `WARN`, never a failure, with the numbers in the message.

- **Boot group size.** The files in `[stream] boot` should stay under `[stream] boot_max_kb` (default 1024). Over it, the title screen waits too long on a slow link: move the heavy models out of the boot group and keep their gameplay data in the kit.
- **Hero model without tier files.** A model in `[content] hero = ["id", ...]`, or any shared `<id>.glb` bigger than `[content] hero_kb` (default 256), with no `<id>.<tier>.glb` file is warned: phones would download the full file. Run `cargo xtask convert` on it for tier files. `.master.glb` sources are not counted.

```toml
[content]
hero = ["kart"]
hero_kb = 256

[stream]
boot_max_kb = 1024
```
