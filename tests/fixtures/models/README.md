# Model fixtures (S3-2)

Small files the model reader (`crates/evani-gltf`, its `tests/reader.rs`) and later Stage 3 work test against. The engine and Evani.Model share them as their contract with docs/MODEL_FORMAT.md; neither depends on the other's code. Every file below was re-made from these commands on 2026-10-16 and came out byte-identical. Total 239 KB.

## The corpus (our own source model, packed by gltfpack)

`corpus.src.glb` is written by our own example, all floats, no extensions: a UV sphere with normals, tangents, UVs and a material using every core metallic-roughness texture (one 2x2 PNG), a skinned tube with two joints, a line strip, two `anchor:` nodes, a `collision` node with one `col:` proxy, one animation and a root `extras.evani` (format 1, part tree, LOD thresholds, texture sets).

```
cargo run -p evani-gltf --example corpus_source -- tests/fixtures/models/corpus.src.glb
```

The packed variants come from **gltfpack 1.3** (meshoptimizer's command-line packer, MIT), the pinned version in docs/stage3/spikes.md. It is not installed by the repo; the copy used was the Windows release zip (sha256 `f6e9c09d…45c0`, as in the spikes notes), `gltfpack.exe` sha256 `a082a0db2332908133100332caeeddaa304afcf5e8094adc755ebdea8c720984`. `-kn -ke` keep node names and extras; `-cc` compresses with meshopt (all three modes and all three filters appear); `-ce ext` picks the `EXT_meshopt_compression` name (the other choice is `khr`); `-vpn` stores positions as normalised integers; `-cf` writes a compressed file with an uncompressed fallback buffer (it cannot be combined with `-cc`).

| File | Command (input `corpus.src.glb`) | What it tests |
|---|---|---|
| `corpus.q.glb` | `gltfpack -i corpus.src.glb -o corpus.q.glb -kn -ke` | KHR_mesh_quantization, integer positions |
| `corpus.qc.glb` | `... -o corpus.qc.glb -kn -ke -cc -ce ext` | the same, meshopt-compressed; fallback buffer with no data |
| `corpus.qn.glb` | `... -o corpus.qn.glb -kn -ke -vpn` | normalised positions |
| `corpus.qnc.glb` | `... -o corpus.qnc.glb -kn -ke -vpn -cc -ce ext` | normalised positions, compressed |
| `corpus.cf.gltf`, `corpus.cf.bin`, `corpus.cf.fallback.bin` | `... -o corpus.cf.gltf -kn -ke -cf -ce ext` | `.gltf` with external buffers; the fallback file is never fetched |
| `kart.low.q.glb` | `gltfpack -i kart.low.glb -o kart.low.q.glb -kn -ke` | an Evani.Model file after gltfpack |
| `kart.low.qc.glb` | `gltfpack -i kart.low.glb -o kart.low.qc.glb -kn -ke -cc -ce ext` | the same, compressed |

What gltfpack 1.3 changes, seen in these files (docs/MODEL_FORMAT.md, "After gltfpack"): the root `extras` is dropped even with `-ke`; node names and node `extras` survive; each mesh moves to a new unnamed child of its named node, which carries the dequantisation scale and offset; mesh names are dropped; identical textures merge (the corpus's five become one); UVs are quantised with a `KHR_texture_transform` scale; animations are resampled to 30 keys a second; vertices are ordered differently with and without `-cc`.

## Evani.Model output (its M1 writer)

Built with this repo's `evani-model` command (`cargo build --release -p evani-model`, crate last changed in d4bc10d):

```
evani-model compile tests/model_bench/vehicles/kart_01_pip.evmodel --tiers low,ultra --out <dir>
evani-model compile tests/model_bench/props/crate.evmodel --tiers low --out <dir>
evani-model compile tests/model_bench/props/barrel.evmodel --tiers low --out <dir>
```

Kept: `kart.low.glb`, `kart.evmodel.json` (the kart's report, which lists both tiers), `crate.low.glb`, `barrel.low.glb`. The ultra file and the normal-map PNGs are left out to keep the fixtures small.

## The animated rig (S3-7)

`rig.src.glb` (about 9 KB) is our own skinned, animated model for the clip baker, written by code: `evani_convert::fixture::rig_glb`, saved with `cargo run -p evani-convert --example rig_source -- tests/fixtures/models/rig.src.glb`. Six joints (hips, spine, neck, head, arm_l, hand_l) drive a skinned tube, with three clips: `idle` (2 s: a LINEAR hips bob and spine sway, a constant neck tilt off the rest pose, STEP head nods, and a hand scale equal to the rest pose), `wave` (1 s: a CUBICSPLINE arm swing and a 60 Hz hand wave) and `pose` (one key per channel). The test `rig_fixture_is_the_writer_output` checks the file is still the writer's bytes.

`rig.clips.evc1` (682 bytes) is the rig's three clips baked into one `EVC1` clip block (docs/MODEL_FORMAT.md "Clips") at a 0.5 mm bound, the bytes the animation runtime's tests (`crates/evani-anim`, S3-8) decode. evani-convert's `anim_crosscheck` test checks it is what the baker writes today and that the runtime decoder and the reference reader sample it to the same bits; rewrite it with `EVANI_REWRITE_FIXTURES=1 cargo test -p evani-convert --test anim_crosscheck`.

`rig.static.glb` (about 8 KB) is the rig's tube as a plain static mesh: the same nodes, buffer and material with no skin, no JOINTS_0 or WEIGHTS_0 and no clips (`evani_convert::fixture::rig_static_glb`, saved with `cargo run -p evani-convert --example rig_source -- --static tests/fixtures/models/rig.static.glb`). GPU skinning (S3-9) is checked against it: the skinned rig drawn in bind pose must match it (`bind_pose_matches_the_static_mesh` natively, a same-camera capture in the browser). The test `rig_static_fixture_is_the_writer_output` checks the file is still the writer's bytes.

## Surface relief (S3-10)

`relief-tile.glb` (12384 bytes) is our own small model for surface relief (docs/MODEL_FORMAT.md, "Surface relief"), written by `node tests/fixtures/models/relief-tile.mjs tests/fixtures/models/relief-tile.glb` (Node built-ins only; byte-identical on re-run, 2026-10-17). A floor slab, material `plate`: bevelled panels with a rivet each, a height map (texture 2, depth 0.03 UV) and the normal map made from the same height, plus a `knurl` micro pattern (period 0.025, depth 0.6) masked by the base alpha (the panel tops). A wall in three columns (one primitive each), all with the same normal map and no height map, each with one micro pattern everywhere: material `panel` `ribs` (period 0.02, depth 0.5, angle 90), `dots` (period 0.03, depth 0.7) and `grain` (period 0.04, depth 0.6), so all four of the toon look's patterns show in one view. Tangents are written so the bitangent runs along +v. The sandbox's `relief` proof draws it; `evani_guide::modelcheck` holds it clean.

`char-head.glb` (1125896 bytes) is our own close-up head for the character materials (docs/MODEL_FORMAT.md, "Character materials"), written by `node tests/fixtures/models/char-head.mjs tests/fixtures/models/char-head.glb` (Node built-ins only, no textures; byte-identical on re-run, 2026-10-22). Metres, facing +z: an ellipsoid head with a nose, a brow and two eye sockets, a short neck and two lid shells with an almond opening that roll onto the face and lie on it, sharing its normal, so there is no outline where lid meets skin (material `skin`, `evani_kind` skin, `evani_skin.pore_scale` 500, equirect UVs); two eyeballs (r 0.012) in the sockets behind the lids (material `eye`, azimuthal UVs: uv 0.5, 0.5 on the pupil axis, distance from it = angle / 2 pi; `evani_eye.iris_color` set); a two-layer hair cap of 32 locks, 4 and 14 mm off the skin so the baked sun of Lite and Low (rays start about 9 mm off a surface on this head) shades the inner layer and the base cap as the shadow map does (material `hair`, double-sided, u = lock number, v crown 0 to hairline 1) over an opaque base cap (material `hair_base`, untagged); and a two-layer fur ruff of 48 locks round the neck (material `fur`, `evani_hair.tip_width` 0.35, `edge_softness` 0.4). The sandbox's `char-proof` build draws it (compiled in).

## A look-matched file (S3-12)

`look-match.low.glb` is the converter's own output: a small made-up car (a sphere body of three primitives whose materials differ only in what the toon look never samples, a glowing trim, named nodes `body` and `trim`, an `anchor:seat`) converted with `--look toon` for the Low tier. The three paints became one material, the metal-rough and occlusion maps are gone, and gltfpack joined the body's primitives. evani-render's `variant_works_on_a_look_matched_file` hides and paints its nodes and stands a part at its anchor. The file is written by the test `look_match_fixture_is_the_converter_output` (the source model is built in code there); the test fails when the converter's output changes, and `EVANI_REWRITE_FIXTURES=1 cargo test -p evani-convert --test look` rewrites it. Its texture URIs point at files that are not kept (like `kart.low.glb`'s normal maps); the reader does not fetch them.

## Checksums (sha256)

```
890462e5e8bae519fba9b22b087e7996a58b86713912ef680e8fb88cd56f6487  barrel.low.glb
4b22c11e0734d672fae0a51592937e25171de21fbac15a5f2d3893ccc843368f  char-head.glb
8a59be5c54127db6d2fc359e1359123f5e3150ddb12c73545d1603cf00113ed8  corpus.cf.bin
f34190704ae9dbf6fb4dce32a1cc5571070cd09b66321f93686e0108de15c869  corpus.cf.fallback.bin
2f4ed04d70a3311679ca0c8ab436a2ef7fc65fe104a4ffac8509a0a258113ab8  corpus.cf.gltf
0f0fa224c7f034925c3b97c2560ee4b958e80e2d5f56e98c0e4063882587b7f2  corpus.q.glb
1c1bdd4f9624f95a3571d0da2583ad9ebaa5c6009d2cb30f63741b7287001006  corpus.qc.glb
c1f0938b942aeedd75a441a2a598e085154f6556f51fab1c3da6209dd9b6fc35  corpus.qn.glb
aa82cd5f4a7b825c6269117d9a335e12fd76a0d234ce62f91b328b75abead212  corpus.qnc.glb
3e3912dcd613aed39f1c91e77a16f747f2672e9a395ddce15756382a17856c06  corpus.src.glb
df01403e84125c23322b6fef72a981e12a0fa86034150eef37238f6c5719f466  crate.low.glb
c4f287d4f08ac7f6fb035017cbd2f79e01a5dd25c650478ba3c91937aeb830fd  kart.evmodel.json
8a58c99a63a4ae7039c459e84e49fc8601dcaf9bcabdeaa30e3bc3c0ea8af8ba  kart.low.glb
30858546d8f36b95fa807dcfa9d893092ccfff76c14b29a87ffdc49c8a9799d0  kart.low.q.glb
c4edf61b3534b67db09542af78f63449418b1f2734f45ca271f6c31cdccc4fe7  kart.low.qc.glb
2dd3609964b3652e5c0e250206bd4f37064e14b8e0a25d967c8e237990b0288f  look-match.low.glb
5e2e6627522cb9399ba5c974687bf5b02ca635a67b1c78571a17b775431853dc  relief-tile.glb
b6a79ce06397d81ff148fe5669c01389031e2f6c1e847dd3b530a20d9732a6dd  rig.clips.evc1
f2c84623f6d549334082b9d8a584a81407774085634b02af98839ad419e89ba2  rig.src.glb
a170174d32265f0624d7748994d0edc114184ecfff7477464e80032471ea2404  rig.static.glb
```
