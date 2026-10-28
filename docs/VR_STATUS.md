# VR status (WebXR with WebGPU)

Checked: 2026-10-14. **Next check: 2027-01-15.**
Full report: `.vibedeck/research/2026-10-14-webxr-with-webgpu-status-checked-2026-10-14.md`.

## Verdict: not yet

Evani.Dev is WebGPU-only, so VR needs the WebXR/WebGPU binding (`XRGPUBinding`). On 2026-10-14:

| Where | Status |
|---|---|
| Meta Quest Browser | WebGPU in WebXR since 146.0 (Apr 2026), plus later layers and foveation, all still marked **Experimental** and behind flags |
| Chrome / Edge stable (desktop, Android) | No `XRGPUBinding` in stable (153). Behind flags since Canary 135; no Intent to Ship found |
| Safari (Vision Pro) | Apple said it will implement it; not confirmed in Safari 27 |
| Firefox, Pico, Android XR | No evidence |
| Spec | Immersive Web Editor's Draft (2026-06-15), still in incubation |
| wgpu (our renderer) | No WebXR hook. PR #9350 closed 2026-04-09 (contribution policy, not the idea); issues #4067 and #8329 open |
| Other engines | Babylon.js 10 has it (experimental, tested on Quest); three.js and PlayCanvas do not |

A public "Enter VR" button would fail for almost every player, so the VR tier stays a preset run mono (as in the bench) and `evani-xr` stays the feature-detect stub described in DECISIONS.md ("XR is a separate, on-demand download").

## What would change the verdict

- Quest Browser release notes drop "Experimental" for WebGPU in WebXR.
- A Chromium Intent to Ship for the WebXR/WebGPU binding, or `XRGPUBinding` in Chrome stable.
- Safari confirms it ships.
- wgpu merges an XR option (`xr_compatible` adapter request plus wrapping XR-supplied textures).

## When it is time

Build `evani-xr` as the on-demand second wasm from DECISIONS.md. Until wgpu has an XR hook, render the per-eye XR textures through the browser's WebGPU directly (web-sys) on the shared `GPUDevice`, or carry a small human-written wgpu patch. Borrow Babylon.js's phase order and pitfalls (per-eye clears, space-warp ordering, feature detection per headset). First target: the kart racer on Quest Browser.
