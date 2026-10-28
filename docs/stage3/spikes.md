# Stage 3 spikes (item S3-0): notes

Date: 2026-10-15. Branch `stage3-spikes` (scratch); only these notes were merged into m1-foundation (docs only, 44d2bc5); the scratch files and downloads were not. Throwaway checks; no engine code kept.
Machine: this PC (Windows 11, RTX 4060 Ti, Chrome 155, Edge 155, Node 24.15, Rust 1.98).
Downloaded tools live in `..\evani-dev-spikes\tools\_dl\` (not committed, not in the system). Test files are in `_scratch\` (not committed).

## Tools used (exact versions)

| Tool | Version | Download | Licence | sha256 of the download |
|---|---|---|---|---|
| gltfpack | 1.3 | https://github.com/zeux/meshoptimizer/releases/download/v1.3/gltfpack-windows.zip | MIT | f6e9c09d66af23da3da71b86f0652d2413e81c734e0aecd1cd3d0e6e6e8645c0 |
| KTX-Software (`ktx`) | 4.4.2 | https://github.com/KhronosGroup/KTX-Software/releases/download/v4.4.2/KTX-Software-4.4.2-Windows-x64.exe (installer; `/S /D=<dir>` installs silently into a chosen folder, no system change needed) | Apache-2.0 | 1f323b0fec19794f5e6c0425a61d4b1da396872a10be862d105f4f4b2d2957fe |
| msdf-atlas-gen | 1.4 | https://github.com/Chlumsky/msdf-atlas-gen/releases/download/v1.4/msdf-atlas-gen-1.4-win64.zip | MIT | 55ab2bef57b9c7bf305b610989fa8eeda812d85bc93e632b8c39ce05b6e64e9d |
| glTF-Transform CLI | 4.5.1 (npm `@gltf-transform/cli@4.5.1`, installed with `npm install --save-exact`) | npm registry | MIT | n/a (npm lockfile) |
| sharp (only to turn the JPEG into a PNG for `ktx create`) | 0.35.5 (npm, comes with the CLI's tree) | npm registry | Apache-2.0 | n/a |
| playwright-core (driver for headless Chrome/Edge) | 1.60.0 (npm) | npm registry | Apache-2.0 | n/a |

Test assets:
- Model: Khronos glTF-Sample-Assets `DamagedHelmet` (glTF-Binary, 3.77 MB, 46,356 triangles, five 2048x2048 JPEG textures). Licence: CC-BY 4.0 (c) 2018 ctxwing for the rebuild (an earlier version by theblueturtle_ is CC-BY-NC 4.0, so do not ship this model in a game; fine for a throwaway test). Source: https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/DamagedHelmet
- Font: Lato Regular (google/fonts `ofl/lato`), SIL Open Font License 1.1 (c) 2010-2014 tyPoland Lukasz Dziedzic.

## (a) gltfpack 1.3 on Windows

Master file `master.glb` = the helmet with one added node name `helmet_root` and `extras` `{"evani":{"role":"hero","lod":0}}` on that node and on `asset`.

```
gltfpack -i master.glb -o t_plain.glb -si 0.5 -cc -kn -ke
```
- Size 3,773,972 to 3,298,780 bytes (textures stay JPEG).
- `-si 0.5`: triangles 46,356 to 23,178 (exactly half). Works.
- `-cc`: output requires `EXT_meshopt_compression` and `KHR_mesh_quantization`. Works.
- `-ke`: node extras `{"evani":{...}}` and asset extras survive intact. Works.
- `-kn`: the named node `helmet_root` survives with its name and extras, BUT the mesh is moved onto a new unnamed node and the mesh name is dropped (nodes after: `[{mesh:0}, {name:"helmet_root", extras}]`). So names of nodes are kept; mesh names are not, and a named node that carried a mesh may be split. Consequence for S3-4 ("same node names across tiers"): rely on node names and keep our identifiers in node `extras`, never on mesh names or on "the named node holds the mesh". Add a converter test that re-reads the output and checks names/extras per tier.

```
gltfpack -i master.glb -o t_tc.glb -tc -tj 2 -kn -ke     (14.9 s)
```
- `-tc` has a BUILT-IN encoder (BasisU, ETC1S by default; `-tu` switches to UASTC; `-tq` quality 1-10; `-tj` threads; per-class options for color/normal/attrib). No KTX-Software needed to run it.
- Result: five `image/ktx2` textures, requires `KHR_texture_basisu`, file 2,508,612 bytes (ETC1S). With `-tu -tq 5`: 11,938,508 bytes, 49 s (UASTC, huge).
- BUT the output is Basis-supercompressed (ETC1S/UASTC). Using it in the browser needs a Basis transcoder (WASM) to turn it into ASTC or BC7. Chris's chosen approach is "no transcoder in the game; ready-made ASTC, BC7 and RGBA sets". gltfpack cannot write those. So gltfpack's `-tc` is not the route for the per-device sets; run gltfpack WITHOUT `-tc`/`-tw` for geometry, and make the texture sets separately (b).
- Verified: `-kn -ke` also kept the extras in `t_tc.glb`; `glTF-Transform inspect` (4.5.1) opens gltfpack output fine (generator "gltfpack 1.3").

## (b) ASTC 4x4, BC7 and RGBA KTX2 on Windows

KTX-Software 4.4.2 installed silently into the spikes folder: `ktx_installer.exe /S /D=...\ktx-sw` (no system change). Source texture: the helmet's base colour, 2048x2048 JPEG decoded to a 1024x1024 PNG (`ktx create` reads PNG/EXR/PNM, not JPEG, so the converter needs a JPEG-to-PNG step; sharp did it here).

```
ktx create --format R8G8B8A8_SRGB --generate-mipmap base1k.png out_rgba.ktx2           0.6 s   5,592,888 bytes  (11 levels, no supercompression)
ktx create --format ASTC_4x4_SRGB_BLOCK --astc-quality medium --generate-mipmap base1k.png out_astc.ktx2    0.8 s   1,398,608 bytes
ktx create --format R8G8B8A8_SRGB --encode uastc --uastc-quality 2 --zstd 18 --generate-mipmap base1k.png out_uastc.ktx2    1,163,045 bytes (intermediate)
ktx transcode --target bc7 out_uastc.ktx2 out_bc7.ktx2                                  1,398,576 bytes  vkFormat BC7_SRGB_BLOCK
ktx create --format BC7_SRGB_BLOCK ... base1k.png x.ktx2
  -> "ktx create fatal: Unsupported format for non-raw create: VK_FORMAT_BC7_SRGB_BLOCK"
```
- ASTC 4x4 and RGBA: direct, fast. Confirmed by `ktx info` (`vkFormat: VK_FORMAT_ASTC_4x4_SRGB_BLOCK`, `VK_FORMAT_R8G8B8A8_SRGB`, 11 mip levels).
- BC7: `ktx create` cannot encode BC7 directly in 4.4.2. The route is: encode UASTC, then `ktx transcode --target bc7` (UASTC to BC7 is a near-lossless transcode). Output is a plain BC7 KTX2 with no supercompression (`ktx transcode` result `vkFormat BC7_SRGB_BLOCK`). The quality gate (S3-6) must score the BC7 set like any other. Alternatives if direct BC7 is wanted later: `texconv` (DirectXTex, MIT) or `bc7enc_rdo` (MIT); not tried.
- Sizes for one 1024x1024 texture with mips: RGBA 5.59 MB, ASTC 4x4 1.40 MB, BC7 1.40 MB (both 4 times smaller than RGBA; add `--zstd` for the download if wanted, but ASTC/BC7 then need decompression in JS or wasm; the browser already gzips on the wire).
- Normal maps and other linear data need `--format R8G8B8A8_UNORM` / `ASTC_4x4_UNORM_BLOCK` (not tried; same tool).

## (c) Compressed-texture features WebGPU reports

Test page `_scratch/web/index.html` (not committed) reads `adapter.features`, run in headless Chrome and Edge on this PC through Playwright (`--enable-unsafe-webgpu`, muted, pointer-lock / fullscreen / keyboard.lock stubbed), and in real Chrome on the phone over `adb reverse tcp:8099 tcp:8099` plus an intent to open `http://localhost:8099/`, with the page posting its result back to a small local server on port 8099.

| Device | Browser | astc | bc | etc2 | GPU reported |
|---|---|---|---|---|---|
| This PC (RTX 4060 Ti) | Chrome 155 headless | no | YES | no | nvidia / lovelace |
| This PC | Edge 155 headless | no | YES | no | same |
| Samsung SM-G991B (phone) | Chrome 155, Android | YES (also astc-sliced-3d) | no | YES | arm / valhall (Mali) |
| Lenovo TB305FU (tablet) | not measured | - | - | - | - |

- A device with `texture-compression-bc` can actually be created (`requestDevice({requiredFeatures:['texture-compression-bc']})` succeeded); same for astc on the phone.
- Desktop: BC only. Phone: ASTC and ETC2, no BC. So the two sets (ASTC for phones, BC7 for desktop) cover both, with RGBA for the rest. Also noteworthy: the PC lists `texture-compression-bc-sliced-3d`; the phone `texture-compression-astc-sliced-3d`.
- Tablet: could NOT be tested. The tablet is held by a child-lock app (`family.nesty.child`, "Nesty") that keeps its own screen in front and refuses to launch Chrome (`am start ... Error: Activity not started, unknown error code 101`, also via `monkey`); the screen was also asleep. I did not try to bypass the lock. Only Chris can do this: unlock or pause the child lock on the tablet (or open `http://<PC address>:8099/` in the tablet's Chrome by hand while a spike server is running, or tell me to use another route). Expectation, not evidence: a Lenovo TB305FU (Mediatek/Unisoc Arm Mali or similar) is likely ASTC + ETC2 and no BC.
- My page also tried to create a second device on the same adapter, which Dawn refuses ("adapter is consumed"); that was a test-page slip, the feature lists are unaffected.

## (d) Does a dropped file reach the page?

Headless Chrome and Edge, plain local page (`http://127.0.0.1:8099/`), synthetic `DragEvent` with a `DataTransfer` holding `File("model.glb", 1234 bytes)` dispatched on a div:
```
dragenter  files [{name:"model.glb", size:1234}]
dragover   files [{name:"model.glb", size:1234}]
drop       files [{name:"model.glb", size:1234}]   -> file.arrayBuffer() returned 1234 bytes
```
So on a plain page in Chrome and Edge, `dragenter/dragover/drop` carry the file and its bytes are readable with `arrayBuffer()` (the page must call `preventDefault()` on `dragover` and `drop`, otherwise the browser navigates to the file).

RESULT (Chris, 2026-10-15): he dragged a plain text file (`.vibedeck/drop-test/DROP-TEST.txt`) onto the running game in the Evani.dev tab and "nothing happened": the file was not opened and nothing visible changed. The cause is unknown: the drop may never have reached the view, or it reached it and the file navigation was blocked. Also unknown: whether a page drop handler would receive the drop (the game page has none). So S3-14 first tests a drop handler in a dev build, keeps the "Convert a model" button fallback, and asks Evani.App to forward drops if the handler gets nothing. (The original open question was whether a file dropped onto the game in that view reaches the page.)

## (e) SSIMULACRA2 Rust crate

Throwaway crate `_scratch/ss` with `ssimulacra2 = "=0.5.1"` and `yuvxyb = "=0.4.1"` (0.4.0 does not satisfy ssimulacra2's `^0.4.1`, so pin 0.4.1).

- Licence: crates.io `ssimulacra2` 0.5.1 (2024-12-29, rust-av/ssimulacra2): BSD-2-Clause. `v_frame` (BSD-2-Clause). Everything else MIT or Apache-2.0 (yuvxyb MIT, av-data MIT; rayon, num-*, crossbeam MIT OR Apache-2.0; unicode-ident `(MIT OR Apache-2.0) AND Unicode-3.0`). No GPL/LGPL/MPL. MSRV 1.65.
- Tree (`cargo tree`, trimmed): ssimulacra2 -> num-traits, rayon (-> rayon-core, crossbeam-*), thiserror 2 (-> proc-macro2, quote, syn), yuvxyb 0.4.1 (-> av-data, log, num-traits, v_frame, yuvxyb-math). 35 distinct crates counting build-time ones; `cargo tree` has no native or C dependencies. Pure Rust.
- Build time, from scratch, release profile, `CARGO_BUILD_JOBS=2`, low priority via run-low.mjs: **31 s** (`Finished release ... in 30.31s`). Own target dir; `Cargo.lock` of Evani.Dev untouched.
- Worked: `compute_frame_ssimulacra2(Rgb, Rgb)` returned 100.0 for identical 128x128 images and 60.06 for a +3% brightness / 10% contrast change, so the API (`Rgb::new(data, w, h, TransferCharacteristic::SRGB, ColorPrimaries::BT709)`) behaves.
- It uses rayon (threads); under wasm it is not a concern because it is converter-only (native).

## (f) msdf-atlas-gen on Windows

v1.4 `win64` zip is one `msdf-atlas-gen.exe` (no DLLs needed). Ran from the spikes folder:
```
msdf-atlas-gen -font Lato-Regular.ttf -charset cs.txt -type mtsdf -format png -imageout atlas.png -json atlas.json -size 32 -pxrange 4 -dimensions 256 256
  Atlas image file saved. / Glyph layout and metadata written into JSON file. / Loaded geometry of 95 out of 95 glyphs.   (0.4 s)
  atlas.png 73,459 bytes, atlas.json 20,668 bytes (JSON with atlas, metrics, per-glyph advance, planeBounds, atlasBounds)
```
`cs.txt` held `[0x20,0x7E]` (printable ASCII, 95 glyphs). Works with a static font. A variable font (IBM Plex Sans in google/fonts is `[wdth,wght]` only) was not tried. For production: use `-type msdf` (3 channels) instead of `mtsdf` when no shadow/outline channel is needed, and compress the PNG; the 20 KB JSON can be turned into a compact table by the converter. Font licence: SIL OFL 1.1, allowed by decision 9 (ship the licence file with the font).

## (g) `navigator.storage.persist()` prompt

Measured here (headless Chrome 155 and Edge 155 via Playwright, `http://127.0.0.1:8099/`): `permissions.query({name:'persistent-storage'})` state `prompt`; `persist()` resolved to `false` after 0 ms and no dialog event fired. Chrome and the phone's Chrome 155 also resolved `false` in 1 ms. That immediate `false` means no prompt was shown in these runs. Caveat: headless profiles have no site-importance history, and I could not run a headed browser.

Cited, not measured: web.dev "Persistent storage" says the browser may prompt, and for Chromium: "does not show any prompts to the user" and instead grants or denies by heuristics (site importance: bookmarked, installed as an app, notifications allowed, high engagement). Edge is Chromium, so same. MDN `StorageManager.persist()` and Mozilla's Firefox documentation say Firefox does show a permission prompt (doorhanger) when `persist()` is called. Firefox is NOT installed on this PC, so Firefox is documentation only. Safari/WebKit not asked.

Meaning for S3-4: `persist()` is safe to call only on Chromium-family browsers (no prompt, may return false). On Firefox it can prompt, so either never call it there (feature-check `navigator.userAgent`/`'mozInnerScreenX' in window`) or do not call it at all; Cache Storage works without it, and the shim already re-fetches on eviction.

## What this means for decisions 1 and 2

Decision 1 (install gltfpack, glTF-Transform, msdf-atlas-gen): all three run on this PC from a plain folder, pinned and checksummable: gltfpack 1.3 (single exe), glTF-Transform 4.5.1 (npm, MIT), msdf-atlas-gen 1.4 (single exe). Recommendation stands: yes. Note for `doctor --fix`: msdf-atlas-gen's zip unpacks to one exe (no DLLs), so `doctor --fix` extracts just that file; gltfpack's `-tc` is not used; gltfpack `-kn` drops mesh names.

Decision 2 (KTX-Software): **YES, needed.** gltfpack's `-tc` only makes Basis-supercompressed KTX2 (ETC1S/UASTC), which needs a transcoder in the game, which Chris ruled out. Raw ASTC 4x4 and RGBA KTX2 come directly from `ktx create`; BC7 comes from `ktx create --encode uastc` then `ktx transcode --target bc7`. No other tool tried does all three. It installs silently into a chosen folder (`/S /D=`), 16 MB unpacked, Apache-2.0, version 4.4.2. A JPEG-to-PNG step (sharp, already part of the glTF-Transform tree) is also needed before `ktx create`. The converter can also write ASTC/BC7 itself later (not needed now).

## Blender for command line (pinned 2026-10-16)

- Version: **Blender 5.2.2 LTS** (5.2 is the current LTS line), the official portable Windows x64 zip `blender-5.2.2-windows-x64.zip` from https://download.blender.org/release/Blender5.2/ (404,453,484 bytes). SHA-256 `3849d17a682cba006075aaa3f3597ecb5c9c30ec31035b2e092c53e40679b535`, matching the official `blender-5.2.2.sha256` file. Licence: GPL-3.0-or-later (a tool only; no Blender code in the engine).
- Unpacked to `.evani/tools/blender-5.2.2-windows-x64/blender.exe` in the main checkout (`.evani/` is gitignored). No installer, no admin. `blender -b --version` prints `Blender 5.2.2 LTS (hash d13f752e3b9c built 2026-09-15 01:37:04)`.
- Run headless as `blender -b --factory-startup -t 2 --python <script> -- <args>`. Its glTF exporter reports itself as "Khronos glTF Blender I/O v5.2.40".
- Later items (S3-5's `.blend` input, S3-15, S3-16) use this same pin. `cargo xtask doctor --fix` does not install it yet; it should install this pin (version, URL and checksum above) later.
- Validator used for the masters: `gltf-validator` 2.0.0-dev.3.10 (npm, pinned with `--save-exact` in `.evani/tools/gltf-validator/`); script `tools/blender/validate_glb.mjs`. The import, measure, export and preview script is `tools/blender/meshy_prepare.py`. See `docs/stage3/source-models.md`.

## Things only Chris can do
- Tablet (TB305FU): unlock or pause its child-lock app so Chrome can be opened, or confirm the tablet's compressed-texture features some other way.
- Drop test inside the Evani.dev tab: done 2026-10-15, nothing happened (see (d)).
- Firefox is not installed; if Firefox persistence matters, a headed run there would settle it (low priority: avoid calling persist() on Firefox).
