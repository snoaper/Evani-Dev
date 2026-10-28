# Third-party notices

No third-party source file is copied into this repository. Crates we depend on keep their own licences (MIT, Apache-2.0 and similar; `cargo metadata` lists them) and ship under them.

Where our code follows another project's design, the source file says so and names the author:

- `crates/evani-core/src/hotpatch.rs`, `crates/evani-live/src/hotpatch.rs`, `tools/evani-live-bridge/src/code.rs`: follow the calling pattern and message shapes of subsecond and dioxus-devtools, Copyright (c) Dioxus Labs, MIT OR Apache-2.0, https://github.com/DioxusLabs/dioxus. Rewritten without their code or dependencies.
- `crates/evani-gltf/src/meshopt.rs`: a scalar Rust port of the EXT_meshopt_compression decoders. Written from the extension specification, but the decoder's structure (block by block, byte column by byte column, 16-element groups, the edge and vertex FIFOs, the tail checks) follows meshoptimizer's scalar `decodeVertexBuffer`, `decodeIndexBuffer` and `decodeIndexSequence`, Copyright (c) 2016-2026 Arseny Kapoulkine, MIT, https://github.com/zeux/meshoptimizer. Because it is a port, the full MIT notice is kept in that file's header.

- `crates/evani-render/src/gpu/gtao.rs` (feature `gtao`): the ambient-occlusion passes follow Bevy's SSAO (bevy_pbr/src/ssao, MIT OR Apache-2.0, Copyright (c) the Bevy contributors, https://github.com/bevyengine/bevy), itself a port of XeGTAO (https://github.com/GameTechDev/XeGTAO). The WGSL is our own fragment-shader version of XeGTAO's maths, so XeGTAO's MIT notice is kept in that file's header and repeated here:

  Copyright (C) 2016-2021, Intel Corporation

  SPDX-License-Identifier: MIT

  Permission is hereby granted, free of charge, to any person obtaining a
  copy of this software and associated documentation files (the
  "Software"), to deal in the Software without restriction, including
  without limitation the rights to use, copy, modify, merge, publish,
  distribute, sublicense, and/or sell copies of the Software, and to permit
  persons to whom the Software is furnished to do so, subject to the
  following conditions:

  The above copyright notice and this permission notice shall be included
  in all copies or substantial portions of the Software.

  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
  OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
  MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
  NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
  DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
  OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
  USE OR OTHER DEALINGS IN THE SOFTWARE.

- `tools/evani-convert/src/bc7.rs` (converter only, never in a game): a Rust port of the BC7 block decoder in bcdec (`bcdec_bc7`, https://github.com/iOrange/bcdec), which its author offers under MIT or the Unlicense; we take it under MIT:

  Copyright (c) 2022 Sergii Kudlai

  Permission is hereby granted, free of charge, to any person obtaining a copy of
  this software and associated documentation files (the "Software"), to deal in
  the Software without restriction, including without limitation the rights to
  use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
  of the Software, and to permit persons to whom the Software is furnished to do
  so, subject to the following conditions:

  The above copyright notice and this permission notice shall be included in all
  copies or substantial portions of the Software.

  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
  SOFTWARE.

Third-party files shipped as they are:

- `assets/fonts/atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf`: Atkinson Hyperlegible, Copyright 2020 Braille Institute of America, Inc., SIL Open Font License 1.1 (the full text is `OFL.txt` beside it, and every game that bakes it ships it as `fonts/<name>.LICENSE.txt`). Used unmodified as the source of a baked MSDF atlas, for UI text only.
- Tool, not shipped: msdf-atlas-gen 1.4 (Copyright (c) Viktor Chlumsky, MIT, https://github.com/Chlumsky/msdf-atlas-gen) is downloaded by `cargo xtask doctor --fix` into `.evani/bin` and run at build time; none of its code is in a game.

- `tools/blender-addon/evani_export/` is our own Blender add-on, GPL-3.0-or-later (its own LICENSE); it is a separate folder that the engine and games never include or link.

Ported code and published formulas (in `crates/evani-material-proc`, feature `sky`, and the engine's default tone map):

- `crates/evani-material-proc/shaders/sky.wgsl` and `src/sky.rs`: the sky dome is ported from three.js `examples/jsm/objects/Sky.js` (its Preetham sky and its clouds), The MIT License, Copyright (c) 2010-2026 three.js authors (https://github.com/mrdoob/three.js); the full notice is kept in `sky.wgsl`. The sky model itself is A. J. Preetham, P. Shirley and B. Smits, "A Practical Analytic Model for Daylight", SIGGRAPH 1999. Only games that list the module with its `sky` feature ship this code.

  The MIT License. Copyright (c) 2010-2026 three.js authors. Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions: The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

- The default `tonemap` slot body (`crates/evani-core/src/slots.rs`) is the ACES filmic curve fit by Krzysztof Narkowicz (2015, "ACES Filmic Tone Mapping Curve"), published as CC0 (public domain); credited here and in the code, no licence text required.
- `crates/evani-material-nature/shaders/nature_common.wgsl` (the nature and see-through materials): no code is copied; the maths follows these, written fresh in WGSL and credited in the file:
  - the back-light (subsurface) term follows Filament's subsurface shading model (`shading_model_subsurface.fs`), Apache License 2.0, Copyright (C) The Android Open Source Project, https://github.com/google/filament;
  - the thickness attenuation of thin surfaces (ears, leaves, paper, lampshades) follows Babylon.js's thin translucency in its PBR sub-surface block, Apache License 2.0, Copyright (c) The Babylon.js authors, https://github.com/BabylonJS/Babylon.js;
  - the Schlick Fresnel and GGX lobe follow the Khronos glTF-Sample-Viewer, Apache License 2.0, Copyright (c) The Khronos Group Inc., https://github.com/KhronosGroup/glTF-Sample-Viewer;
  - the integer noise hash is the PCG hash of M. Jarzynski and M. Olano, "Hash Functions for GPU Rendering" (JCGT 2020), after M. E. O'Neill's PCG family (Apache License 2.0, https://www.pcg-random.org).

Layered materials (`crates/evani-material-layers`, `crates/evani-render/shaders/model_lit_layers.wgsl`; evani-render feature `material-layers`): published formulas ported into our own WGSL, no source file copied. Only games that turn the feature on ship this code.

- Filament (Copyright Google LLC, Apache License 2.0, https://github.com/google/filament): the Charlie sheen distribution (Estevez and Kulla, "Production Friendly Microfacet Sheen BRDF", 2017), the Neubelt cloth visibility, the clear coat GGX lobe with the Kelemen visibility, the anisotropic GGX distribution and its height-correlated Smith visibility, and the cloth subsurface wrap, as described in Filament's "Physically Based Rendering in Filament" document.
- three.js (Copyright (c) 2010-2026 three.js authors, The MIT License, https://github.com/mrdoob/three.js; the full MIT text is above, under the sky dome): `MeshPhysicalMaterial`'s thin-film iridescence (`evalIridescence`, `evalSensitivity`, after L. Belcour and P. Barla, "A Practical Extension to Microfacet Theory for the Modeling of Varying Iridescence", SIGGRAPH 2017), its fitted sheen albedo (`IBLSheenBRDF`) and its sheen energy term, and the layer order (base, sheen, clear coat).
- glTF-Sample-Viewer (Copyright The Khronos Group Inc., Apache License 2.0, https://github.com/KhronosGroup/glTF-Sample-Viewer): the `KHR_materials_anisotropy` roughness split and the bent normal for reflections, as in the extension's reference implementation.
- Bevy (`bevy_pbr`, Copyright the Bevy contributors, MIT or Apache License 2.0, https://github.com/bevyengine/bevy): the design idea of one shader function per layer, compiled in only when a material uses it; no code taken.

The Apache License 2.0 text is at https://www.apache.org/licenses/LICENSE-2.0. The four proof models in `apps/sandbox/assets/models/layer-*.glb` are our own, made from formulas.

Ideas and defaults followed (in `crates/evani-material-character`, reached only through evani-render's `char-materials` feature):

- `crates/evani-material-character/shaders/character.wgsl` (the eye): the refracting cornea follows the Godot "Eye" shader by tentabrobpy (https://godotshaders.com/shader/eye/, MIT licence): the view ray bends through the cornea (`refract`, index 1.376) onto an iris plane below it, and its defaults (cornea bulge 0.75, index 1.376, wet roughness 0.05) seeded ours. Rewritten from scratch for Evani's lit pass (no object space: the eye's frame comes from its UVs and screen derivatives); none of its code is copied. Credited here and in the WGSL header.
- The same file's skin follows the pre-integrated skin lookup idea of E. Penner and G. Borshukov ("Pre-Integrated Skin Shading", GPU Pro 2, 2011), generated and fitted in our own code (`src/lut.rs`); the hair highlight follows Kajiya and Kay (1989) with Scheuermann's shifted two-lobe form (2004). Published methods, no code copied, no licence text required.

- `crates/evani-render/shaders/kit_surface.wgsl` (evani-render feature `kit-surface`): the GGX distribution, the fast correlated Smith visibility and Schlick's Fresnel follow the published formulas as written in Google Filament's `shaders/src/brdf.fs` (Apache-2.0, Copyright (C) 2016 The Android Open Source Project, https://github.com/google/filament); written fresh in WGSL, no file copied; credited here and in the shader. Only games that turn on `kit-surface` (through evani-material-proc's `relief`) ship it.
- `crates/evani-material-proc/src/relief/pattern.rs` (feature `relief`): ideas only (per-brick hash variation, the weighted joint profile) from the Godot procedural brick shader by arlez80 (MIT, https://godotshaders.com/shader/procedural-brick-shader/); written from scratch, no code copied; credited here and in the code.

Anything copied later keeps its own licence text and copyright notice next to it, and gets a line here.
