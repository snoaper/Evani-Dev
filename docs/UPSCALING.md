# Upscaling in the browser: DLSS and the alternatives

Status: research only, 2026-10-16. Nothing is built. Source: the researcher's Prior art report, `.vibedeck/research/2026-10-16-dlss-and-other-upscalers-for-a-browser-webgpu-engi.md`. Links below are taken from it. Anything the report could not confirm is marked "unverified".

## Short answers

- **Does NVIDIA DLSS work in a browser today? No.** Official DLSS needs a native app (D3D12 or Vulkan), NVIDIA's driver and NVIDIA's own software. A web page can reach none of them. ([NVIDIA DLSS 5 announcement](https://www.nvidia.com/en-us/geforce/news/dlss5-breakthrough-in-visual-fidelity-for-games/))
- **What is DLSS 5?** NVIDIA announced it on 16 March 2026 for fall 2026 ([NVIDIA](https://www.nvidia.com/en-us/geforce/news/dlss5-breakthrough-in-visual-fidelity-for-games/), [Guru3D](https://www.guru3d.com/story/nvidia-unveils-dlss-5-with-neural-rendering-for-more-realistic-games/)). It is not an upscaler. It is a "neural rendering" model that re-lights and re-textures a finished frame from colour and motion vectors, and it ships through NVIDIA Streamline. The NVIDIA page names no GPU list and says nothing about the web.
- **Is DLSS 5 limited to the RTX 50 series?** Press coverage says so ([Guru3D](https://www.guru3d.com/story/nvidia-unveils-dlss-5-with-neural-rendering-for-more-realistic-games/)); the NVIDIA page does not. **Unverified.**
- **Can something similar be detected and turned on automatically?** Yes, for the open upscalers below, which are plain shaders. Detection and fallback is sketched in section 5.

## 1. The "DLSS 5 in a browser" story

- A developer called MAAN published OpenDLSS-NR in September 2026. It reimplements the DLSS 5 network in Vulkan and WebGPU. The repository says it contains no NVIDIA weights and users must supply their own ([repo](https://github.com/maanHimself/OpenDLSS-NR), [XenoSpectrum](https://xenospectrum.com/en/opendlss-nr-vulkan-webgpu-reimplementation/)).
- Press reports say the weights came from a leaked NVIDIA library ([HotHardware](https://hothardware.com/news/nvidia-dlss-5-mod-runs-in-a-browser)). **Unverified**, but the legal status of the weights is a problem either way.
- Press reports give about 72 ms at 512x512 and about 465 ms for a 1904x929 frame in the browser path, with weights of about 141 to 147 MB. These are press figures, **unverified** by us.
- The press articles describe a live demo, and a YouTube demo is mentioned. The researcher could not open any video, so the YouTube side is **unverified**.
- Verdict: it is not official DLSS, and it cannot ship in a game (weights of unclear legal status, about 145 MB, under one frame per second).

## 2. Browsers have no hook for vendor upscalers

- NVIDIA RTX Video Super Resolution in Chrome and Edge upscales video playback only ([NVIDIA](https://blogs.nvidia.com/blog/rtx-video-super-resolution)). The researcher found nothing that applies it to a WebGPU canvas.
- WebGPU subgroups shipped in Chrome 134 ([Chrome](https://developer.chrome.com/blog/new-in-webgpu-134)), which helps hand-written compute upscalers.
- The matrix-multiply (subgroup matrix) extension is only experimental and was reported at 0% support ([web3dsurvey](https://web3dsurvey.com/webgpu/features/chromium-experimental-subgroup-matrix)). Neural upscalers cannot count on tensor-style hardware paths in a browser.

## 3. Comparison

Cells marked "general knowledge" are not from the report: image quality and relative GPU cost reflect how these techniques are commonly described, were not measured by us, and need a bench on our own scenes before anyone relies on them. Every number from the report names its source. Download costs are our estimates, not measurements.

| Option | Type | Image quality | GPU cost | Inputs needed | Browser support today | Download cost (estimate) | Licence: copy code or only ideas |
|---|---|---|---|---|---|---|---|
| FSR 1 ([AMD](https://gpuopen.com/fsr2/)) | Spatial, EASU + RCAS | Fair; sharpens a lower-resolution frame but adds no new detail (general knowledge) | Lowest: one or two cheap passes (general knowledge) | Colour only, no motion data | WGSL ports of EASU/RCAS exist inside pmndrs/upscaler | About 3 to 6 KB gzip | MIT: code can be copied with AMD's notice kept |
| FSR 2/3 temporal ([AMD](https://gpuopen.com/fsr2/); WGSL port [pmndrs/upscaler](https://github.com/pmndrs/upscaler)) | Temporal, compute | Good; recovers detail across frames, can ghost without a good reactive mask (general knowledge) | Moderate: several compute passes. pmndrs docs claim 6 to 76% GPU overhead saved against FSR 3.1.5 (README, no ms figures) | Colour, depth, velocity, jitter, reactive mask, exposure | pmndrs/upscaler: WGSL, three.js-based, about 24 stars, last release date not seen | About 12 to 25 KB gzip | MIT (AMD source and pmndrs): WGSL can be adapted with notices kept; do not take the three.js glue |
| Snapdragon GSR ([Qualcomm](https://github.com/SnapdragonGameStudios/snapdragon-gsr)) | SGSR 1 spatial, SGSR 2 temporal | Aimed at Adreno phones; not compared by the researcher | SGSR 1 is one pass; SGSR 2 is tuned to Adreno's tiled design | SGSR 1: one colour texture. SGSR 2: temporal inputs | No browser port found; shader language not confirmed | Similar to FSR 1 for SGSR 1 | BSD-3-Clause: code can be copied with the notice kept |
| Arm ASR ([Arm](https://github.com/arm/accuracy-super-resolution)) | Temporal, based on FSR 2.2.2 | Close to FSR 2 with mobile cuts (general knowledge) | Arm's figures for Immortalis-G720: up to 53% higher frame rate and 20% lower power ([Neowin](https://www.neowin.net/news/arm-joins-the-super-resolution-bandwagon-with-accuracy-super-resolution/)) | As FSR 2 | Targets Vulkan, Unreal and custom engines; no web port found | Not applicable (read for ideas) | MIT: copyable, but its value is the list of what to cut for phones |
| XeSS ([Intel](https://github.com/intel/xess)) | ML | Good (general knowledge) | XeSS 2 runs on any GPU with Shader Model 6.4 via DP4a, faster on Arc ([Wccftech](https://wccftech.com/intel-has-finally-released-xess-2-sdk-for-faster-developer-integration/amp/)) | Colour, depth, velocity | None; the SDK is precompiled | Not possible | Intel's own licence, binary only: ideas only |
| FSR 4 ([AMD](https://gpuopen.com/manuals/fidelityfx_sdk2/techniques/super-resolution-ml/)) | ML | Best of the AMD line (general knowledge) | About 0.35 ms at 1080p on an RX 9070 XT (AMD manual); needs an AMD 9000-series GPU | As FSR 2/3 | None; HLSL/DX12 | Not possible | Source was pulled after an accidental release ([Guru3D](https://www.guru3d.com/story/amd-accidentally-opensources-fsr-4-code-on-github)); press says it carried an MIT label, **unverified**. Do not copy: legal status unclear. Ideas only |
| Anime4K WebGPU ([Anime4K](https://github.com/bloc97/Anime4K), [npm README](https://cdn.jsdelivr.net/npm/anime4k-webgpu@1.0.0/README.md)) | Small convolution networks, single frame | Tuned for anime and video; a poor fit for game frames (researcher's view) | Real time on video; much faster than Real-ESRGAN, which is not real time. No ms figures found for a game frame | Colour only | Yes, `anime4k-webgpu` on npm | Not measured | **MIT, confirmed:** the LICENSE file in the repository reads "MIT License, Copyright (c) 2019 bloc97" (read 2026-10-16). The researcher had not re-read it |
| OpenDLSS-NR ([repo](https://github.com/maanHimself/OpenDLSS-NR)) | Neural re-render, not an upscaler | Not comparable; it re-lights the frame | Press figures: 72 ms at 512x512, 465 ms at 1904x929 (**unverified**) | Colour and motion vectors, plus weights the user supplies | Vulkan and WebGPU, demo only | About 141 to 147 MB of weights (press) | Code licence not recorded by the researcher; the weights are the problem (leak claim **unverified**). Do not use |

Licences re-read on 2026-10-16: pmndrs/upscaler is MIT ("Copyright (c) 2026 Dennis Smolek"); Snapdragon GSR carries an SPDX line of BSD-3-Clause; Anime4K is MIT. AMD's MIT status for FSR 1 to 3 is as the researcher reported from [GPUOpen](https://gpuopen.com/fsr2/); re-read each LICENSE file at copy time. AMD says FSR 4 now supersedes FSR 3, and the FSR 4 SDK falls back to FSR 3.1.5 ([GPUOpen](https://gpuopen.com/manuals/fidelityfx_sdk2/techniques/super-resolution-ml/)).

## 4. Recommendation

1. **Spatial rung first** (FSR 1 style: EASU upscale plus RCAS sharpen). One pass, no motion data, works on phones. Reference: the spatial path in pmndrs/upscaler and AMD's MIT source.
2. **Temporal rung on top** (FSR 2/3 style). pmndrs/upscaler is the WGSL reference (pass list, parity notes); AMD's MIT source gives the details (jitter sequence, history reprojection, disocclusion handling, reactive mask).
3. **Do not pursue DLSS 5 or OpenDLSS-NR.** No legitimate route into a browser, a leaked-weights claim (unverified), about 145 MB, under one frame per second. Do not copy FSR 4 or XeSS. Read Snapdragon GSR and Arm ASR later, only for phone tuning.
4. **Licence notes if code is adapted:**
   - AMD FidelityFX (FSR 1, 2, 3): MIT. Keep AMD's copyright and permission notice in the adapted shader files and in a third-party notices file shipped with the game.
   - pmndrs/upscaler: MIT, copyright Dennis Smolek. Keep its notice for any WGSL taken from it. It carries AMD's notice for the EASU/RCAS shaders; keep that too.
   - Snapdragon GSR (BSD-3-Clause) and Arm ASR (MIT): only if code is taken; keep their notices.
   - Anime4K: MIT, notice needed if anything is copied. Not recommended.
   - New dependencies, if any, are pinned and recorded in DECISIONS.md as AGENTS.md requires. This sketch needs none: the passes are WGSL files in the module.
   - Evani.Dev itself is MIT (docs/LICENSING.md); third-party code inside it keeps its own notice.

## 5. Detect-and-fallback sketch (optional module)

Not built. Fitted to what the engine has today (read only):

- `crates/evani-render/src/control.rs`: render size is CSS size x min(devicePixelRatio, tier DPR cap) x tier render scale x adaptive-quality scale (`render_size`); the same file runs the first-run tier warm-up (`WARMUP_TOTAL_MS`, `WARMUP_SETTLE_FRAMES`).
- `crates/evani-render/src/shaders.rs`: `jitter_view_proj` exists, but its only caller (`gpu/frame.rs`) is the geometry flicker check behind the `capture` feature.
- `gpu/mod.rs` and `gpu/timing.rs`: timestamp queries are requested when the adapter has `TIMESTAMP_QUERY`; without them timing falls back to an estimate.
- `crates/evani-fx`: effects are optional Cargo features, nothing on by default (`bloom = []`, `shadows = ["evani-render/shadows"]`).
- `catalog/tiers.json`: Ultra lists "optional temporal upscale"; no other tier mentions upscaling. Low renders at 0.75x, Medium and High at 1.0x.
- `docs/LIVE_API.md`: parameters are dotted (`fx.bloom.intensity`, `aq.*`); `tier.set` is a mutating command that needs a note.

### Build-time switch

A new `upscale` feature (on `evani-fx`, or a small crate behind it), listed in a game's `evani.toml` like `bloom`. A game that does not list it compiles none of the WGSL or Rust and downloads zero extra bytes. It depends on the renderer pieces below, as `particles` depends on `evani-render/particles`. The ship guard and `cargo xtask sizes` get a marker for it.

### Detection and warm-up

1. At adapter setup: check compute support, f16 and subgroup availability, and whether timestamp queries exist (already read in `gpu/mod.rs`).
2. Short warm-up measurement, extending the existing first-run tier warm-up: run the candidate rung for a few settle frames, read its cost (timestamps if present, otherwise the frame-time estimate) and keep it only if it fits the tier's frame budget (`aq.target_ms`).
3. Any failure (no compute, pipeline error, over budget) drops one rung, and the reason shows in the status line, as "paused to hold frame rate" does for effects.

### Fallback ladder

Temporal, then spatial, then plain bilinear render scale (what the engine does today). The adaptive-quality controller steps down this ladder and back up with its existing hysteresis, before it lowers render scale below its floor. Where this sits relative to cutting effects needs Chris's decision when it is built.

### Parameters (live-tunable, owner `fx.upscale`)

| Name | Meaning |
|---|---|
| `fx.upscale.mode` | off, spatial, temporal (the controller may lower it, never raise it above what warm-up allowed) |
| `fx.upscale.ratio` | Output size divided by render size, for example 1.5 to 2.0 |
| `fx.upscale.sharpness` | Sharpen strength after upscaling (RCAS-style) |
| `fx.upscale.reactive` | Strength of the reactive mask that limits ghosting on particles and transparency |

All four register like `fx.bloom.*`, so Look Lab and an AI agent can set them through `param.set`, `sweep.run` and `compare.ab`, and `bake_export` can write them into source.

### What the renderer must add for the temporal rung

- A velocity buffer (per-pixel motion, including moving objects and camera).
- A per-frame jitter sequence (for example Halton) applied to the projection. `jitter_view_proj` is a starting point, but it is capture-only today.
- History textures (previous output) with reprojection and rejection.
- A depth texture that later passes can sample.
- A reactive mask written by transparent and particle draws.
- MSAA resolved first. Ultra uses 4x MSAA today, so the resolve must come before the upscaler; MSAA and temporal upscaling together are costly, so Ultra may have to trade one for the other. Needs measuring.
- A render size lower than the output size, with the final scale-to-screen moved into the upscale pass.

The spatial rung needs none of these: only a lower-resolution colour target and one pass that reads it.

### Proposed tier defaults (a proposal, not approved)

| Tier | Default mode | Note |
|---|---|---|
| Low | spatial, ratio about 1.33 | Phones; one cheap pass. Today Low is plain 0.75x |
| Medium | spatial | Same pass; the ratio may stay near 1.0 where there is headroom |
| High | spatial; temporal opt-in on desktops after warm-up | Opt-in only |
| Ultra | temporal, falling back to spatial | Matches the filed `temporal-upscale` row |
| VR | off | Stereo and foveation need their own study |

### Estimated download cost per rung

Estimates only, nothing measured. Compare `cargo xtask sizes` before and after when it is built.

| Rung | Extra gzip (estimate) |
|---|---|
| Plain bilinear render scale | 0 (already in the engine) |
| Spatial (EASU + RCAS) | about 3 to 6 KB |
| Temporal (adds history, resolve, reactive, auto-exposure) | about 12 to 25 KB on top of spatial |
| Renderer additions (velocity, history, jitter) | small Rust code; velocity adds shader variants to the forward pass only for games that list `upscale` |

## 6. Where this belongs in the stages

The current stage is 2 (First kits) in STAGES.md, so nothing here is built now.

- **Temporal rung:** matches the existing `temporal-upscale` row in CORE_REQUESTS.md ("optional temporal upscale for the `3d` render path at Ultra", filed for Stage 8). It stays there. This document is its design input.
- **Spatial rung for lower tiers:** new. It would change what `catalog/tiers.json` says (today only Ultra lists an upscale), so it needs Chris's approval. It is filed in the STAGES.md Parking lot and as a CORE_REQUESTS.md row (`spatial-upscale`) marked "proposed, not approved". Neither entry lets anyone build it.
- **Detection, warm-up and the ladder** serve both rungs and follow whichever is approved first.
- **Open questions for Chris:** build the spatial rung before Stage 8 or with it? Should Low and Medium move from plain render scale to spatial upscale by default? Where on the ladder does cutting effects sit?
