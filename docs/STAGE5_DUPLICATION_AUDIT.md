# Stage 5 duplication audit (S5-H1)

Read-only audit for Stage 5's "Phase A hardening: move systems built twice into shared crates" and its gate line "No system is duplicated across kits". Nothing was built or changed; this file is the only output.

- Tree audited: branch `m1-foundation` at 8c0e3ab (crates/, kits/, looks/, tools/, apps/). Line numbers are from that commit; a `~` means the end of the range was read off a neighbouring symbol and may be a few lines out.
- Method: greps for helper signatures and magic constants across the whole tree, then reading each hit against the engine module that should own it. Every item below lists the copies, the shared target, the effort and whether the merge touches kit gameplay.
- Effort: S = under a day, one reviewer; M = a few days, several files, tests move; L = a week or more, or a design change in the shared module.
- "Kit gameplay change" means the kit's simulation, rules or look would behave or look differently. "Sim code touched" means gameplay code is edited but must stay bit-identical (the determinism hashes and tests prove it). "App only" means only `apps/*` demo shells change.
- "Gate" says whether the item counts against "No system is duplicated across kits" (the same system in two or more of the kits 2d-platformer, kart-racer, match-3, example, model-view).

## Summary, most valuable first

| # | System | Copies | Shared target | Effort | Kit gameplay change | Gate |
|---|---|---|---|---|---|---|
| 1 | Demo-app DOM shell (settings, key rebinding, modal, DOM helpers) | 3 apps | new `evani-core::web::menu` module | L | App only | Yes (apps) |
| 2 | sRGB / hex / CSS colour conversion | 17 | new `evani-core::color` | S | No | Yes |
| 3 | Vec3 / quaternion / matrix helpers | 25+ | `evani-core::math` re-exported by `evani_render::math` | M | Sim code touched (bit-identical) | Yes |
| 4 | Sound synthesiser and sound registry | 3 kits | new `evani-core::synth` | M | No (bytes identical) | Yes |
| 5 | `tunables!` macro | 3 kits (+ `layout!`) | `evani-core::params` macro | S | No | Yes |
| 6 | Integer hash, value noise, LCG noise | 12 | `evani-core::rng` | S | Sim code touched (bit-identical) | Yes |
| 7 | Easing, `approach`, angle wrap | 14 | `evani-core::num` | S | Sim code touched (bit-identical) | Yes |
| 8 | Screen-shake maths | 4 | `evani-core` (fx re-exports) | M | Yes (shake feel changes) | Yes |
| 9 | Bitmap HUD font and digits | 1 kit vs engine text | `evani_render::text` | M | Yes (look change) | Partly |
| 10 | Param and effect readers in draw systems | 3 kits | `ParamRegistry::f32_or / bool_or` | S | No | Yes |
| 11 | Review dump and geocheck dump scaffolding | 4 kits + 2 dumps | `evani-core::review` + `evani-geocheck-2d` helpers | S-M | No | Yes |
| 12 | Kart track spline and sweep | 1 kit vs engine | `evani_render::spline` | L | Yes (track code; geometry must not change) | No |
| 13 | Footprint / placement tests | 2 kits vs engine | `evani_render::place::Footprints` | M | Sim code touched (props) | Yes |
| 14 | PNG / zlib codec, base64, sha256, FNV in tools | 6 + 3 + 6 + 2 | new `tools/evani-tool-util` (or `crates/evani-img`) | M | No | No |
| 15 | f64 vector maths in tools | 4 | `evani-model::vec` or one tools copy | M | No | No |
| 16 | Kit-local particle effects | kart, platformer vs engine particles | `ParticleQueue` | M (platformer) / L (kart) | Yes (look changes) | Yes |
| 17 | Capture-moment "run until" loops | 3 kits | `evani_core::capture` helper | S | No | Yes |
| 18 | Entity "gather / sync" view pattern | 2 kits | leave as is | n/a | n/a | Pattern only |

Suggested order (cheapest and safest first, each one removes copies other items depend on): 2, 6, 7, 10, 5, then 3, 4, 17, 11, then 1, 14, then 13, 8, 9, 16, 12 as separate reviewed changes because they change what a player sees. Items 2, 6, 7 and 3 should be done before 8 and 12 so the shared module exists first.

---

## 1. Demo-app DOM shell (apps)

The three demo apps each carry the same ~700-line menu shell: command enum and its click/change parser, key labels and rebinding rows, effect labels, save/load of volume, overlay and bindings, pause hold, modal state, settings panel HTML, and the browser-side DOM helpers. A diff of the label, binding and save/load blocks shows only game-specific strings and the kit type differ.

Copies:
- `apps/kart-racer/src/ui.rs`: save keys 37-39; `UiCmd` 43-92; `key_label` 368, `binding_label` 385, `BIND_ROWS` 399, `effect_label` 408; `Pending`/`GameUi` 421-660 (`save`/`load`/`request` 461-480, `apply` 500, `key` 569, `load_saved` 606, `hold_pause` 622, `sync_modal` 633, `settings_key` 644, `effects` 658); `EngineModule` impl ~675-725; DOM `Text` 750-785, `by_id`/`part`/`escape`/`set_on` 843-862, `focus_first`/`blur` 988-1003; `settings_html` 1137; `css_hex` 295.
- `apps/match-3/src/ui.rs`: save keys 45-48; `UiCmd` 52-110; `key_label` 221, `binding_label` 232, `BIND_ROWS` 244, `effect_label` 255; `GameUi` 273-583 (`save`/`load`/`request` 300-316, `apply` 318, `key` 393, `load_saved` 416, `hold_pause` 453, `sync_modal` 464, `settings_key` 472, `effects` 482); DOM `Text` 584-631, `by_id`/`escape`/`set_on` 673-689, `focus_first`/`blur` 817-832; `settings_html` 1082; `css_hex` 190.
- `apps/2d-platformer/src/ui.rs`: save keys 38-40; `UiCmd` 48-93; `key_label` 138, `binding_label` 155, `BIND_ROWS` 169, `effect_label` 178; `Pending`/`GameUi` 190-460 (`save`/`load`/`request` 226-243, `apply` 244, `key` 306, `load_saved` 343, `hold_pause` 359, `sync_modal` 365, `settings_key` 374, `effects` 384); DOM `by_id`/`escape`/`set_on` 479-491, `focus_first`/`blur` 582-598; `settings_html` 650.
- Smaller echoes: `apps/model-view/src/web.rs:152` (`escape`), `apps/sandbox/src/panel.rs` and `apps/sandbox/src/hudpanel.rs` (toggle/panel code).

Shared target: no shared home exists today. Add a `menu` module under `crates/evani-core/src/web/` (wasm-only, like `touch.rs` and `audio.rs` beside it): a generic `MenuState` (volume, overlay, bindings, tier, effect toggles, modal), the label functions, a `Text`/`by_id`/`escape`/`set_on`/`focus` DOM kit, and a settings-panel renderer taking the game's `BIND_ROWS`. Each app keeps only its own `UiCmd` variants, `Screen` mapping, HUD view and markup.

Effort: L. Kit gameplay change: none; app only (the kits already expose `Request` and a HUD resource). Risk: `index.html` markup per game must keep its `data-cmd` / `data-set` hooks; the existing app tests (`commands_parse`, `title_to_play_to_pause_and_back`, the rebinding and settings tests) carry over.

## 2. sRGB, hex and CSS colour conversion

No shared colour module exists in `evani-core` or `evani-render`. The same two formulas (sRGB to linear with 0.04045 / 12.92 / 2.4, and the inverse) are written out 17 times.

Copies:
- `kits/evani-kit-2d-platformer/src/art.rs`: `hex` 124-129, `srgb_to_linear` 478-481, `rgb_linear` 483-485, `linear_to_srgb8` 487-490.
- `looks/evani-look-pixel16/src/lib.rs`: `srgb_to_linear` 73-76 (public, but a kit must not depend on a look), inverse inline ~158.
- `looks/evani-look-toon/src/lib.rs:203` (inline).
- `kits/evani-kit-kart-racer/src/draw.rs`: `linear(hex)` 1759-1766; `review.rs:189-197` (`srgb_hex`).
- `kits/evani-kit-match-3/src/review.rs`: `srgb` 39-44, `hex` 45-49.
- `kits/evani-kit-2d-platformer/examples/geocheck_dump.rs:371`.
- `apps/kart-racer/src/ui.rs:295-306` and `apps/match-3/src/ui.rs:190-199` (`css_hex`).
- `crates/evani-live/src/look.rs:102-110`.
- `crates/evani-model/src/gltf.rs:105-110` (`srgb_to_linear_u8`; evani-model keeps its own maths on purpose, see the exceptions list).
- `tools/evani-convert/src/metric.rs:55`, `tools/evani-geocheck-2d/src/render.rs:18-27`, `tools/evani-lookguard/src/stats.rs:167-170`, `tools/xtask/src/kitchecks.rs:557-566` (`hex_to_lab`).

Shared target: new `crates/evani-core/src/color.rs` (`srgb_to_linear(f32)`, `linear_to_srgb(f32)`, `_u8` variants, `hex_to_rgb`, `rgb_hex`, `css_hex`). The f64 tool copies can stay f64 wrappers or call it.
Effort: S. Kit gameplay change: none, provided the formulas are copied exactly (look-guard and review baselines hash colours).

## 3. Vec3 / quaternion / matrix helpers

Each module that needs `sub`, `dot`, `cross`, `normalize` or `rotate-by-quaternion` defines its own private copy; `evani_render::math` exposes only `normalize`, `mul`, `look_at` and two quaternion functions.

Copies (f32 unless noted):
- Engine: `crates/evani-render/src/math.rs` (private `sub` 10, `dot` 14, `cross` 18, public `normalize` 27, `quat_mul` 101); `place.rs` (`sub` 24, `add` 27, `mul` 30, `dot` 33, `cross` 36, `norm` 39, `rotate` 44); `spline.rs` (`sub` 18, `add` 21, `mul` 24, `len` 27, `norm` 30, `cross` 34); `model/sun.rs` (`dot` 76, `cross` 80, `normalize` 84); `model/lod.rs:61` (`rotate`); `crates/evani-anim/src/math.rs` (`add`/`sub`/`scale`/`dot`/`cross`/`length` 56-80, `rotate` 116, `nlerp` 124, `lerp3` 142, `lerp4` 138); `crates/evani-gltf/src/lib.rs` (`mul` 312, `transform_point` 323, duplicating render's `mul` 76 and `transform_point` 87).
- Kart racer: `draw.rs` (`normalize3` 238, `add` 243, `lerp3` 247, `scale3` 1087, `quat_rotate` 1078), `finale.rs` (`norm3` 229, `quat_rotate` 407), `roadmarks.rs` (`sub` 72, `cross` 76, `norm` 80), `trackmesh.rs` (`sub` 150, `cross` 154, `dot` 158, `norm` 162), `itembox.rs:186` (`cross`), `hull.rs:64` (`dot` 2D), `review.rs:61` (`rotate`), `race.rs` (`dist_xz` 366, `dist3` 370), `track.rs:135` (`dist_xz`), `threat.rs:107` (`dist`).
- Platformer: `draw.rs:495` (`lerp2`), `level.rs:147` (`dist`).
- Model-view: `orbit.rs` (`sub` 82, `dot` 86), `decode.rs` (`normalize` 84, `transform_dir` 90).
- Match-3: `draw.rs` colour helpers `mul` 291, `alpha` 295, `mix` 299.
- Tools: `tools/evani-convert/src/clips.rs` (`mul` 48 matrix, `slerp` 250), `fixture.rs:36` (`rotate`), `tools/evani-guide/src/clipblock.rs` (`nlerp` 182, `lerp` 188).
- f64 copies are item 15.

Shared target: one small `evani-core::math` (plain `[f32; N]` functions, no new dependency; `evani-render::math` re-exports it so existing paths keep working). `evani-anim` keeps its `Vec3`/`Quat` type aliases but calls it; `evani-gltf` (a standalone parser) can keep two functions or take the same module.
Effort: M (many files, each edit trivial). Kit gameplay change: none intended, but kart `race.rs`/`track.rs`/`threat.rs` distances feed the sim, so each swap must be bit-identical (same operation order) and prove it with the kart determinism hashes and `playtest` races. Do not "improve" the maths in the same change.

## 4. Sound synthesiser and sound registry

All three kits synthesise their effects with the same hand-written functions: `Wave`/`osc`, `env` (attack/release ramp), `tone`, `sweep`, `noise`, `next_noise` (xorshift32), `wav` (16-bit PCM writer) and a `Sounds` struct that registers each effect with the mixer and plays it on the Effects bus. A diff of the platformer and kart blocks shows 9 differing lines; against match-3 only the attack/release times (0.004 and 0.025 s vs 0.005 and 0.03 s), two extra waveforms (`Sine`, `Saw`) and comments.

Copies:
- `kits/evani-kit-2d-platformer/src/sfx.rs`: `osc` 115, `env` 130, `tone` 137, `sweep` 151, `noise` 168, `next_noise` 181, `wav` 189-211, `Sounds` 213-245.
- `kits/evani-kit-match-3/src/sfx.rs`: `mix` 121, `osc` 138, `env` 155, `tone` 162, `sweep` 176, `noise` 193, `next_noise` 206, `wav` 214-236, `Sounds` 238-272.
- `kits/evani-kit-kart-racer/src/sfx.rs`: `seamless_noise` 231, `osc` 263, `env` 280, `tone` 287, `sweep` 302, `noise` 319, `next_noise` 332, `wav` 340-362, `Sounds` 364-410 (plus loop sounds 137-230 that are kart-only).

Shared target: new `crates/evani-core/src/synth.rs` (the building blocks, with envelope times as arguments or a small `Style` struct; `wav` can sit next to `AudioMixer::add_sound`) and a generic `SoundBank<E>` in `audio.rs`. Each kit keeps only its table of effects.
Effort: M. Kit gameplay change: none; the synthesised bytes must stay identical (add a test per kit that hashes every WAV before and after; the existing "every sound is a short valid WAV" tests are not enough).

## 5. `tunables!` macro

The macro that turns a table of `name = default, [min, max], unit, description` rows into `Handles`, `Tune`, `register` and `read` is copied three times, differing only in the `kit.<id>.` prefix literal. Kart has a second one for HUD layout.

Copies: `kits/evani-kit-2d-platformer/src/lib.rs:59-~110`; `kits/evani-kit-match-3/src/lib.rs:72-~122`; `kits/evani-kit-kart-racer/src/lib.rs:90-~140`; `kits/evani-kit-kart-racer/src/hud_layout.rs:23-~70` (`layout!`).
Shared target: a macro exported from `evani-core` (taking the prefix as its first argument) next to `ParamDef`. Effort: S. Kit gameplay change: none (parameter names, ranges and defaults stay the same, so the live schema is unchanged).

## 6. Integer hash, value noise and LCG noise

Copies of the same Murmur-style finaliser (`0x9E37_79B9`, `0x85EB_CA6B`, `0x7FEB_352D`, `0x846C_A68B`):
- `apps/bench/src/content.rs:135-143` (`hash01`)
- `kits/evani-kit-example/src/place.rs:70-77` (`hash`) and `draw.rs:281-283` (`hash01` on top of it)
- `kits/evani-kit-kart-racer/src/track.rs:140-148` (`hash01`; drives scenery placement)
- `kits/evani-kit-match-3/src/draw.rs:276-290` (`noise1`, same finaliser inside a value-noise)
- `crates/evani-fx/src/shake.rs:101-125` (`noise1`, `grad`)
- Variant with different constants: `kits/evani-kit-2d-platformer/src/art.rs:392-399` (`hash(x, y, seed)`), `crates/evani-live/src/img.rs:247` (`hash3`, a different purpose).
- xorshift32 sound noise: platformer `sfx.rs:181`, match-3 `sfx.rs:206`, kart `sfx.rs:332` (covered by item 4).
- The engine's only RNG is `Pcg32` in `crates/evani-core/src/rng.rs`; it has no stateless hash.

Shared target: add `hash32(i, salt)`, `hash01`, `hash2(x, y, seed)` and `noise1` to `evani-core::rng` (stateless, same constants). Effort: S. Kit gameplay change: sim code touched only where the hash feeds placement (kart scenery); outputs must stay bit-identical (the existing `hash_is_in_unit_range` and kart scenery tests, plus the geocheck baselines, cover it).

## 7. Easing, approach and angle wrap

`evani-core` exposes `num::clamp` and an expression builtin `expr::smoothstep` (`expr.rs:397-403`), but nothing a kit can call for tweens.

Copies:
- Smoothstep `x*x*(3-2x)`: kart `chasecam.rs` (inline 85-90, `smoothstep` 170-173), `finale.rs:44-47` (`smooth01`), `roadmarks.rs:290-293` (`smooth`), `draw.rs:846` (inline); match-3 `draw.rs:315-318` (`ease_in_out`); platformer `draw.rs:479` (inline); `crates/evani-fx/src/shake.rs:107` (inline); f64 copies in `crates/evani-model/src/preview.rs:199` and `rig.rs:108` (see exceptions).
- `approach` (move a value toward a target by a step): kart `kart.rs:225`, platformer `game.rs:938`; kart `chasecam.rs:116` (`ease_lift`) is a damped variant.
- Angle wrap: kart `kart.rs:219` (`wrap_angle`), `playtest.rs:243` (`wrap`), `trackmesh.rs:102` (`wrap`).
- `lerp` for arrays: kart `draw.rs:247`, platformer `draw.rs:495`, match-3 `draw.rs:299`.

Shared target: `evani-core::num` gets `lerp`, `smoothstep` (re-used by `expr.rs`), `smooth01`, `approach`, `wrap_angle`. Effort: S. Kit gameplay change: sim code touched (`kart.rs` and platformer `game.rs` are simulation); results must be bit-identical, so no formula changes in the same commit.

## 8. Screen-shake maths

`crates/evani-fx/src/shake.rs` says `decay_trauma` and `sample` "are the shared maths, so every game shakes the same way", but no kit uses them: kits cannot depend on the optional `evani-fx`, so each wrote its own.

Copies:
- `crates/evani-fx/src/shake.rs`: `ShakeSettings` 30-50, `decay_trauma` 51, `sample` 57-85, `noise1` 101-113 (gradient noise).
- `kits/evani-kit-2d-platformer/src/draw.rs`: `wobble` 235-239 (two sines), use 336-346; trauma decay in `game.rs:475, 495`.
- `kits/evani-kit-kart-racer/src/draw.rs`: `shake_noise` 1668-1671 (three sines), settings read ~1878-1890; trauma add/decay in `race.rs:472, 838`.
- `kits/evani-kit-match-3/src/draw.rs`: `shake_offset` 262-273 and `noise1` 276-290 (value noise).

Shared target: move `ShakeSettings`, `decay_trauma`, `sample` and the noise into `evani-core` (a `shake` module); `evani-fx` re-exports them and keeps its params. Effort: M. Kit gameplay change: yes in the visible sense. Each kit's shake currently uses a different noise, so the shake will feel different; the kit owner must re-run the capture review. Trauma is stored in the sim state (platformer `Game.trauma`, kart `Race.trauma`) but only the draw reads it for the offset, so gameplay stays unchanged.

## 9. Bitmap HUD font and digits

The platformer builds its own pixel font (bold 6 x 7 digits plus letters for "LEVEL CLEAR!", and a 3 x 5 mini set for score pops and "1UP") and its own glyph-to-sprite drawing, while the engine already bakes a 5 x 9 ASCII bitmap font with outline, shadow and sizing (`text.rs`; its header comment says its glyphs are "drawn as text art like the platformer's digits"), and a sharp MSDF path (`msdf.rs`, `evani-ui`).

Copies:
- `kits/evani-kit-2d-platformer/src/font.rs:1-95` (glyph tables, `hud_index`, `mini_index`).
- `kits/evani-kit-2d-platformer/src/art.rs`: `glyphs`/`mini` frames 631, 783-788, `glyph()` outlining 2322-2343.
- `kits/evani-kit-2d-platformer/src/draw.rs`: glyph advances 406-412, score and 1UP text 840-910, "LEVEL CLEAR!" 919-960, glyph overlap note ~1244.
- Not duplicates: kart `finale.rs:480-490` (`digit_segments`, 3D seven-segment numerals on the podium) is geometry, and `itembox.rs` draws its "?" as a swept mesh.
- Not verified: match-3's floating score text (`ecs.rs:335`, `Floats`) is drawn in the kit; STAGES.md files "canvas text" for it under Stage 3 (S3-13c). Check it against `text::draw` before scheduling.

Shared target: `evani_render::text::draw` (+ `TextStyle` outline and shadow). If the platformer's chunkier look is wanted, make the engine font table registerable by a look or kit (a second bitmap `FontId`), not copied into the kit.
Effort: M. Kit gameplay change: yes, a look change (glyph shapes and widths differ), so the kit's review and any Verified status need redoing; the sim is untouched. Gate: counts only if match-3's floating text proves to be a second copy.

## 10. Param and effect readers in the draw systems

Copies: platformer `draw.rs:227-234` (`param_f32`, `param_bool`); kart `draw.rs:1673-1675` (`param_f32`); match-3 `draw.rs:255-262` (`fx_on`, `param_f32`); plus inline `reg.f32_handle(..).map_or(default, ..)` in `crates/evani-fx/src/shake.rs:~61` and a few more places (6 matches of the pattern in the tree).
Shared target: `ParamRegistry::f32_or(name, default)` and `bool_or(name, default)` in `crates/evani-core/src/params.rs`; `EffectsStatus::is_on` already exists for `fx_on`. Effort: S. Kit gameplay change: none.

## 11. Review dump and geocheck dump scaffolding

Each kit writes `$EVANI_REVIEW_DUMP/review.json` (schema in `tools/xtask/src/kitchecks.rs`) with its own `object(...)` builder and colour hex helper; the two 2D kits also each carry a ~450-line `geocheck_dump` example with the same `r()` rounding, rectangle union/intersection and sprite-bounds helpers.

Copies: `kits/evani-kit-kart-racer/src/review.rs` (`object` 198, `review_dump` 208, `footprint` 172, `inside` 185, `triangles` 71, `rotate` 61, `srgb_hex` 189); `kits/evani-kit-match-3/src/review.rs` (`srgb` 39, `hex` 45, `object` 80, `review_dump` 91); `kits/evani-kit-2d-platformer/src/review.rs:74-130`; `kits/evani-kit-example/src/lib.rs:310-355`; `kits/evani-kit-match-3/examples/geocheck_dump.rs` (`r` 64, `rv` 68, `off` 72, `union` 76, `inter_area` 89, `bounds` 96, `layer_bounds` 103) and `kits/evani-kit-2d-platformer/examples/geocheck_dump.rs` (`r` 45, `rect` 49, `sprites_of` 60).
Shared target: a `ReviewObject` builder and `write_dump(dir)` in `evani-core` (new `review.rs`), and the rectangle helpers in `tools/evani-geocheck-2d`'s library. Effort: S-M. Kit gameplay change: none.

## 12. Kart track spline and sweep vs the engine's spline

The kart builds its closed Catmull-Rom centreline and sweeps road, shoulder, kerbs and rails along it itself; the engine already has `spline::Path` (Catmull-Rom, even length sampling, frames) and `spline::sweep`. The kart's own header says why: its sweep "takes a profile per ring; the engine's `spline::sweep` keeps one profile".

Copies: `kits/evani-kit-kart-racer/src/track.rs:121-175` (`catmull` and dense sampling) vs `crates/evani-render/src/spline.rs:38-160` (`Path`, `point`, `frames`); `kits/evani-kit-kart-racer/src/trackmesh.rs:150-280` (`sub`/`cross`/`dot`/`norm`, `sweep`, `rings`, `limits`) vs `spline.rs:194-290` (`sweep`, `Profile`).
Shared target: `evani_render::spline`: add a per-ring profile callback (`sweep_with`) and expose the Catmull-Rom point function; the kart keeps its bank, kerb alternation and fold-avoidance rules and feeds them in.
Effort: L. Kit gameplay change: yes, the track code is rewritten and the driving line (`Track::point_at`, used by karts and AI) must not move: lock it with a hash of the centreline and of every mesh before the change, and re-run geocheck. The engine's own sampling is arc-length based, the kart's is its own `SUB`; they may not agree, which is why this is L.

## 13. Footprint and placement tests

Copies: `kits/evani-kit-example/src/draw.rs:313-322` (`Taken`, a linear scan of circles); `kits/evani-kit-kart-racer/src/scenery.rs:519-540` (`meadow_clear`, a linear scan of props with per-kind foot radii; related placement in `trees` 292, `infield` 398, `cupcake_ring` 445, `roadside` 546); `kits/evani-kit-example/src/place.rs` (hash-based grid placement).
Engine: `crates/evani-render/src/place.rs:224-330` (`Footprints` with a uniform grid, `try_place`, `reserve_path`), plus `ground_height` and `snap_to_ground` 159-222 that kart `scenery.rs`/`draw.rs` ground-disc code partly redoes.
Shared target: `evani_render::place::Footprints` (needs a way to ask "is a circle free" without the gap). Effort: M. Kit gameplay change: sim code touched, since props are sim-visible scenery; the result of every free/not-free test must be identical, so keep the call order and radii and compare the generated prop lists before and after.

## 14. PNG / zlib codec, base64, sha256 and FNV in the tools

No shared support crate exists for the tools, so each re-implements small utilities. `sha2`, `flate2` (workspace pin) and `png` (pinned =0.18.1 in `xtask` and `evani-geocheck-2d`) are already dependencies.

Copies:
- PNG decode/encode, CRC and Adler: `crates/evani-live/src/img.rs` (`adler32` 84, `crc32` 74, `paeth` 104, `encode_png` 148, decoder in tests 401, `base64` 319); `tools/evani-geocheck/src/cli/png.rs` (`crc32` 8, `paeth` 26, with `write_png` in `cli/render.rs:185`); `tools/evani-lookguard/src/png.rs` (`paeth` 23, `base64_decode` 38); `tools/xtask/src/checks.rs` (`decode_png` 273, `paeth` 348) and `publish.rs:637` (`png_size`); `tools/evani-geocheck-2d/src/image.rs` (`decode_png` 7, `encode_png` 31, `write_png` 44, via the `png` crate); `tools/evani-convert/src/gate.rs:621` (`png_rgb`); `crates/evani-gltf/examples/corpus_source.rs:52` (`crc32`).
- sha256 hex wrappers over `sha2`: `tools/evani-lookguard/src/lib.rs:42-52`; `tools/evani-convert/src/report.rs:32`; `tools/evani-guide/src/extools.rs:121` and `fonttools.rs:38` (each with its own `sha256_file`); `tools/xtask/src/publish.rs:90-96`.
- base64 decode: `tools/evani-live-bridge/src/mcp.rs:397`, `tools/evani-lookguard/src/png.rs:38`; encode: `crates/evani-live/src/img.rs:319`.
- FNV: `crates/evani-core/src/assets.rs:18-30` (`content_hash`) and `tools/evani-guide/src/lib.rs:77` (`fnv_hex`, whose test `hash_is_the_engines` shows the intent to match).
- JSON file reading helpers: `tools/evani-guide/src/check.rs:162` and `render_paths.rs:15`; ISO date from milliseconds in `tools/evani-lookguard/src/lib.rs:~55-77`.

Shared target: a new small `tools/evani-tool-util` crate (hex, sha256, base64, FNV, `read_json`, `iso_from_ms`, PNG decode/encode through the already-pinned `png` crate). `evani-live` ships in the dev build; keep its tiny encoder or take the util crate only if the wasm build stays small.
Effort: M. Kit gameplay change: none. Gate: not a kit duplication, but it is "no duplicated systems" in the hardening sense.

## 15. f64 vector maths in the tools

Copies: `crates/evani-model/src/vec.rs` (V3 f64, `lerp` 43, `quat_axis` 70, `quat_ypr` 95; deliberately bit-exact); `tools/evani-geocheck/src/geom.rs` (V3 f64, `lerp` 40, `rotate` 49); `tools/evani-geocheck-2d/src/geom.rs` (2D rect helpers, `inflate` 19); `tools/evani-convert/src/clips.rs:48-250` (M4 `mul`, `slerp`) and `fixture.rs:36`; `tools/evani-guide/src/clipblock.rs:182-190`.
Shared target: reuse `evani-model::vec` (the bit-exact one) from `evani-geocheck`, or one `tools` copy. Effort: M. Kit gameplay change: none. Lower value: each tool is small and self-contained.

## 16. Kit-local particle effects

The shared GPU particle system (`evani_core::particles::ParticleQueue` -> `evani_render::particles` -> `evani-fx` `particles`) is used by match-3 (`sparks.rs`, which says the kit "keeps no particle pool") and the bench. Two kits still do their own:
- Platformer: pops (score, dust, sparkle) are entities/structs: `game.rs:134-165` (`PopKind`), `ecs.rs`, drawn at `draw.rs:826-860`.
- Kart: drift sparks `draw.rs:927-967`, boost sparks `969-1060` (`low_boost_sparks` 1006), item box and bomb sparks ~1309-1323, and the finish confetti `finale.rs:265-400` (`confetti_count`, `Sweet`, `sweet()`, `draw_sweet`, `burst` 531). These are stateless mesh instances computed from the clock, sized by the tier's triangle budget.
Shared target: `ParticleQueue::emit/burst`. Effort: M for the platformer pops, L for the kart (lit HDR meshes and per-tier triangle sizing do not map one-to-one onto GPU particles; check that `Space` covers world-space meshes before scheduling). Kit gameplay change: yes, a look change; none to the sim. Not verified in depth: whether the shared system can match the kart's tuned look.

## 17. Capture-moment "run until" loops

The moment registry itself is shared (`evani_core::capture::CaptureMoments`). The loop that plays the sim until a predicate holds and then holds the clock is repeated: platformer `moments.rs:90-125` (`stage`, `at`, `ticks`); match-3 `moments.rs:78-132` (`setup_of`, `ask`, `play` with `MAX_TICKS` 114); kart `moments.rs:275-424` (`staged_finish`, `stage`, `stage_from`, `stage_with`).
Shared target: a `stage_until(world, max_ticks, pred)` helper in `capture.rs`. Effort: S. Kit gameplay change: none. Low value; do it only with item 11.

## 18. Entity "gather / sync" view pattern (leave alone)

`kits/evani-kit-2d-platformer/src/ecs.rs` (`Objects::gather` / `sync`) and `kits/evani-kit-match-3/src/ecs.rs` (`Grid::gather` / `sync`) use the same design: rules run on plain vectors, entities are a view rebuilt each tick and written back. The data and reconciliation differ (slot index vs piece id), so a generic helper would be larger than either copy. Not recommended for Stage 5; record it in KIT_PROCESS.md as the pattern new kits follow.

---

## Checked and not duplicated

- Touch controls: all kits use `evani_core::touch` (`TouchLayout`, `Button`, `Stick`); the platformer's `touch.rs` only declares a layout.
- Input actions and rebinding: one `Input` in `evani-core`; only the labels around it are copied (item 1).
- Camera: all demo apps and the example/kart kits use `CameraPath::orbit` for the title path. The kart chase and podium cameras, the platformer follow camera and the model-view orbit are different rigs, not copies.
- glTF reading: `evani-gltf` is the one parser for the engine, `evani-convert`, `evani-guide` and the model-view kit.
- Scenes, saves, the audio mixer, ECS, tiers, the live API and params are only in `evani-core`/`evani-live`.
- Look crates: no copied helpers beyond the colour functions in item 2.
- Match-3 sparkles and confetti: already on the shared particle system.

## Deliberate exceptions (do not merge without Chris's decision)

`crates/evani-model` re-implements `Pcg32` (`rng.rs`), sha256 (`sha256.rs`), PNG (`png.rs`), glTF load (`compare.rs:347`), f64 vectors (`vec.rs`) and some colour and smoothstep maths. This follows the Evani.Model rule to build from scratch with no new libraries and bit-exact results on every platform (docs/model_research). Merging it into the engine's f32 modules would break that. The only candidates worth a separate look are the SHA-256 and PNG, if the rule is ever relaxed.

## Not covered

- `tools/xtask` internals and `tools/evani-guide` were checked for the utilities in item 14 only, not for duplicated commands.
- `apps/bench/src/check.rs` and `run.rs` (about 1,800 lines) were checked only for hashes and particles.
- No code was run or compiled. Line numbers and the "identical" claims come from reading and diffing the files; the effort figures are estimates.
