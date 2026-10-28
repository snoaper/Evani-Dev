# Evani.Model M1: originality and licence scan (test I)

Scan of milestone M1 by a non-builder agent, as plan-m1.md section I and the brief's originality rule 6 require before sign-off (review-m1.md must-fix 3). Run 2026-10-15 on branch `evani-model`. The scanner changed nothing in the work; every tool, corpus and intermediate file lives outside the repository in `..\_scratch\evm-scan\`.

**Verdict: pass with notes.** No third-party licence texts, copyright notices, SPDX lines, vendored files, weights, datasets, third-party meshes or outside-generator output; every dependency is permissive and approved; no code was copied, ported or adapted from the corpus. Every match above the plan's thresholds was looked at by hand: all are boilerplate (module lists, `#[derive]` structs, test modules), JPlag's coarse Rust tokens lining up across unrelated code, or text that a published standard defines (SHA-256 round, CRC-32, Adler-32, glTF enums). No rewrite is needed. The notes are at the end.

## What was scanned

| Snapshot | Commit | Contents |
|---|---|---|
| First pass | `042b351` | `crates/evani-model`, `tools/evani-model`, `tools/model-bench-ref`, `tests/model_bench`, `docs/model_research`, `docs/EVANI_MODEL.md`, `docs/EVANI_MODEL_BRIEF.md`, `docs/MODEL_API.md`: 176 files (25 `.rs`, 3 `.toml`, 38 `.json`, 30 `.evmodel`, 32 `.md`, 39 `.png`, 9 `.glb`) |
| Re-scan (final) | `6856e9d` | The same paths plus `.vibedeck/screenshots/model-m1/`, after the other worker's review-fix commits landed during the scan (must.rs, api.rs, compile.rs, mcp.rs, the new `tests/determinism.rs`, regenerated sheets): 182 files (26 `.rs`, 44 `.png`) |

Steps 1, 3, 4 and 5 were run on both snapshots, with the same results; the figures below are from the final one unless stated. Step 2 was run on the worktree at `6856e9d` (Cargo.lock and every model Cargo.toml are unchanged between the two commits). Commit `6856e9d` is the tip of the M1 work when this record was written: no later M1 commit exists, and `git status` shows no uncommitted model files (the four class JSON files that were uncommitted during the first pass are inside `6856e9d`). The sign-off applies to `6856e9d`; if any M1 commit lands after it, `rescan.sh` must be run on that commit before sign-off and this table updated.

## Tools (portable, outside the repository)

| Tool | Version | Source | SHA-256 of the download |
|---|---|---|---|
| ScanCode Toolkit | 32.5.0 (SPDX list 3.27), in a private venv; its own pinned `requirements.txt` used as constraints (pluggy 1.7 breaks it) | PyPI `scancode-toolkit[full]==32.5.0` | binary wheels from PyPI; the full frozen list (80 packages, `pip freeze --all`) is `work/scancode-venv-freeze.txt`, SHA-256 `74be4b7d40d55487c0fa4086369fc8fa16cfc5ab58ecea6645e2c926857e85c1` |
| Python (for ScanCode and our scripts) | 3.12.15, python-build-standalone 20261013 | github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.12.15+20261013-x86_64-pc-windows-msvc-install_only.tar.gz | `bbc902d4d187ac4914915bf538196a064b49109a33a104965b5164ced5b80a1e` (matches the release's SHA256SUMS) |
| cargo-deny | 0.20.2 (prebuilt) | github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-pc-windows-msvc.tar.gz | `975a22143262fd27476d19ee00c7af67978426e40e1dee94eed6bbade1cf87dc` (matches the published .sha256) |
| JPlag | 6.3.0, `jar-with-dependencies` | github.com/jplag/JPlag/releases/download/v6.3.0/jplag-6.3.0-jar-with-dependencies.jar | `5f2c21e8b88ed77134effcb3a5a3ab13d188f6a3e16d401387f7479e92db9aa2` (no published checksum) |
| Java for JPlag | Eclipse Temurin 25.0.4.1+1 (JPlag 6.3 needs class version 69) | github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.4.1%2B1/OpenJDK25U-jdk_x64_windows_hotspot_25.0.4.1_1.zip | `00c847d804f4a78e9f04f2683faf14fed898535b177b7fc704486cb0284e9283` (matches Adoptium's .sha256.txt) |
| Java 21 (downloaded first, too old for JPlag 6.3, unused) | Temurin 21.0.12.1+1 | github.com/adoptium/temurin21-binaries/.../OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip | `f9d6e191ab098c0d416e7d588a24420a8621cd2f4720dab2459b8b7b2d2d8b4e` (matches) |
| Dolos (second opinion) | 2.9.3 from npm | `@dodona/dolos@2.9.3` | install failed, see note 2 |
| Our scripts | written for this scan | `work/prep_rust.py`, `analyse_jplag.py`, `triage_matches.py`, `distinct.py`, `rescan.sh` in the scratch folder | n/a |

Heavy steps ran at below-normal priority, JPlag capped to 2 processors (`-XX:ActiveProcessorCount=2`, `-Xmx4g`), ScanCode with 2 processes.

## Corpus (read-only, outside the repository)

Cloned with `git clone --depth 1` into `..\_scratch\evm-scan\corpus\`; crates from static.crates.io, each checked against the crates.io index checksum (all 12 match). No builder opened this folder.

**Rust (step 3, 880 `.rs` files, 423k lines):**

| Project | Commit or version | Why |
|---|---|---|
| fu5ha/sdfu | `9c9653e884dae55015c4a33d26e949998ab68161` | SDF library |
| saft (EmbarkStudios) | crate 0.34.1 (`0eaef267…`); the GitHub repo is gone | SDF library, marching cubes |
| mkeeter/fidget | `eb4e9487cac6570110ff0a4d8ae81ad9d8e35110` | implicit surfaces, dual contouring, octree meshing |
| swiftcoder/isosurface | `185a0ebba1d7c08d1d1cbdf30fa77b9012d6f12e` | marching cubes, dual contouring, octrees |
| bonsairobo/fast-surface-nets-rs | `2216ac27ad36068b9342dff02231d26c27b3d3df` | surface nets |
| Gnurfos/transvoxel_rs | `66461ca310039df1292462597d27b8ea33b0cf26` | isosurface extraction |
| pengowen123/cmaes | `2bb2b3f534957bbc5e69a163f51e269af62cd287` | CMA-ES |
| gltf-rs/gltf | `50d65229477fe5f785c2c90df21eb59c93ea2261` | glTF reader and writer |
| gltf-rs/mikktspace | `6275cc4f15cff8be29819fb34ae8be3b9129dae1` | MikkTSpace port |
| bevy_mikktspace | crate 1.0.0 (`bff34eb2…`) | MikkTSpace port |
| rust-cv/levenberg-marquardt | `e67066cf1351e91e054c30fcb2ff49d005db1e50` | Levenberg-Marquardt (solve.rs) |
| zesterer/euc | `e8f7aeec9ff431b373f83de2c773ddf8a2a3ec4d` | software rasteriser (preview.rs) |
| yzsolt/meshopt-rs | `7b64153167830dfe4fb28d6bd699a28d106f172e` | meshoptimizer port |
| libm | crate 0.2.16 (`b6d2cec3…`) | deterministic maths (dmath.rs) |
| sha2 (RustCrypto) | crate 0.11.1 (`47d7069b…`) | SHA-256 (sha256.rs) |
| png, crc32fast, adler2, miniz_oxide | crates 0.18.1, 1.5.2, 2.0.1, 0.9.1 | PNG writer, CRC-32, Adler-32, deflate (png.rs) |
| rand_pcg, sobol_burley | crates 0.10.2, 0.5.0 | PCG and quasi-random sequences (rng.rs) |
| font8x8 | crate 0.3.1 | bitmap font (preview.rs) |
| glam | crate 0.34.1 | vector and quaternion helpers (vec.rs) |

**C, C++ and other (step 4, 3,913 source files):** mmikk/MikkTSpace `3e895b49d05ea07e4c2133156cfa94369e19e409`, zeux/meshoptimizer `f0a684f11ead064e176775c9b796f82a55910e92`, jpcy/xatlas `f700c7790aaa030e794b52ba7791a05c085faf0c`, SarahWeiii/CoACD `131010c4f03fec375ea88c5c6263f120046cb7f1`, elalish/manifold `761661c15bacb6c8459d682c715de84591d51774`, libfive/libfive `8d7e15dcb1aa77deaba771856a3dc10ec0ca46b1`, nothings/stb `2c980bb59875b0d32144a71867fbdebb2f77cd20` (PNG writer, CRC), B-Con/crypto-algorithms `cfbde48414baacf51fc7c74f275190881f037d32` (SHA-256), imneme/pcg-c-basic `bc39cd76ac3d541e618606bcc6e1e5ba5e5e6aa3`, musl `b1efda5b91735e33635376ca11c9b497a1c66e39` (libm, from git.musl-libc.org), Cyan4973/xxHash `680bf463fa1ca0461b9a7c2dab7556e1f54cf4cf`; plus the SDF references the brief names, saved 2026-10-15 22:35 local time: Mercury's hg_sdf.glsl (version 2021-07-28, mercury.sexy/hg_sdf/hg_sdf.glsl, SHA-256 `bbab07b84d1ccc7ea283210098821a7e20cfbe917383ff6dfd505a94c96d88e9`) and Inigo Quilez's article pages iquilezles.org/articles/distfunctions/ (`c40d646a280c5271c152b8b1004f547b6044a30dbcceecf62e0b752ae3abfa5e`), /distfunctions2d/ (`0ed5ffbd8b9ebf33c3732a3c48ed9850d354850652ff55d263aa0061a59a33af`) and /smin/ (`04d738c3d1d03d5ba93f84cc4a3685ca03ec8d48bc2f6247ee97b9c5567ecc31`).

Not in the corpus: TRELLIS.2 and OmniPart (Python and CUDA model code; M1 has no plug-in interface or Python, and D8 keeps them off, so they were checked by search in step 5 instead), and Blender (its tangent code is the MikkTSpace reference, which is in the corpus).

## Step 1: licence scan of our files (ScanCode)

Command: `scancode -clpeu --info -n 2 --json-pp scancode-6856e9d.json <snapshot>`. 182 files, 0 scan errors.

- Copyrights, holders, authors: **0**. E-mail addresses: **0**. URLs: **0**.
- Licence detections: 12 hits in 8 Markdown files, every one a sentence that names the licence of an outside tool we discuss, not a licence text: 04-part-generators.md:25 and 20-integration.md:15 (NVIDIA Source Code License of the plug-in generators), 17-gltf-output.md:13 (glTF-Validator is Apache-2.0), plan-baselines.md:25, 32, 34–36 (baseline tools' licences), plan-decisions.md:48 and 52 (D8, D13), plan-m1.md:59–61 (the scan tools' licences), README.md:74 (flagged source 8), review-m0.md:78. Each line was read.
- No licence detections in any `.rs`, `.toml`, `.json`, `.evmodel`, `.png` or `.glb` file; no vendored files, NOTICE or LICENSE files.
- Package data: the three Cargo.toml files show no declared licence because ScanCode does not resolve `license.workspace = true`; the workspace declares `MIT OR Apache-2.0` (Cargo.toml line 39), which cargo-deny reads correctly below.

**Result: pass.**

## Step 2: dependency licences (cargo-deny)

Command, per crate: `cargo-deny --manifest-path <crate>/Cargo.toml --locked --offline --config work/deny-scan.toml check -s licenses`, with a scratch config allowing permissive licences only (MIT, MIT-0, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, Unlicense, CC0-1.0, BSL-1.0, 0BSD; no copyleft licence, not even weak copyleft). Cargo.lock not modified.

| Crate | Result | Graph |
|---|---|---|
| evani-model | `licenses ok: 0 errors` | 12 crates: evani-model, serde, serde_core, serde_derive, serde_json, itoa, memchr, zmij, proc-macro2, quote, syn, unicode-ident; all MIT and/or Apache-2.0 (memchr Unlicense OR MIT; unicode-ident adds Unicode-3.0) |
| evani-model-tool | `licenses ok: 0 errors` | the same plus evani-model-tool |
| model-bench-ref (dev only) | `licenses ok: 0 errors` | 86 crates through the kart kit, evani-render and the toon look (wgpu, naga and their dependencies): MIT, Apache-2.0, BSD-2-Clause, ISC, Zlib, Unicode-3.0, Unlicense only |

The only warnings are "allowed licence not encountered" for entries in the scratch config. Direct dependencies of the library are serde and serde_json only, as approved in D2; no new crate entered Cargo.lock.

**Result: pass.**

## Step 3: code similarity, same language (JPlag)

Command: `java -XX:ActiveProcessorCount=2 -Xmx4g -jar jplag-6.3.0-jar-with-dependencies.jar -l rust --new jp/ours_n --old jp/corpus_n -n -1 -M RUN --csv-export -r jplag-head`, minimum token match 8 (JPlag's Rust default). One submission per file on both sides (26 of ours, 880 corpus files), so percentages are per file rather than diluted over whole crates. 22,282 comparisons of our files against corpus files.

**Pre-processing (both sides alike).** JPlag 6.3.0's Rust grammar rejects every shift operator (`<<`, `>>`), `let … else`, `union` as a method name, and some block-comment headers; on the raw files it dropped 46 libm files entirely and logged 2,217 parse errors. `prep_rust.py` therefore strips comments (JPlag ignores them anyway; line numbers kept), rewrites spaced shifts to `^`, one-line `let … else {` to `if let … {} else {`, `union(` to `union_x(` and drops backticks, identically for ours and the corpus. Parse errors fell to 121 in total; all 26 of our files tokenise (build.rs and intent.rs still have 37 and 10 recovered errors, mostly long `format!` lines, so a few tokens there are lost; step 4 covers those files by text). 23 corpus files are below the 8-token minimum (one-line `mod` files).

**Rule applied (plan: any pair above 30%, or any matched block longer than about 12 lines).** JPlag's MAX metric is inflated by tiny corpus files (a 9-token `mod` list is "100%" similar), so every pair was checked three ways: AVG > 30%, share of *our* file covered > 30%, and any match spanning more than 12 of our lines. Then `triage_matches.py` compared the real text of every match in the flagged pairs (1,805 file pairs flagged by any of the three checks, containing 1,918 matches) (lexical tokens with names and literals kept, plus shared distinctive names and literals), and the strongest were read by hand.

| Check | Count | What they are |
|---|---|---|
| AVG > 30% | 3 | lib.rs (`pub mod …;` list, 22 tokens) against isosurface, transvoxel and libm module lists |
| Our file covered > 30% | 101 | small files (lib.rs, tools main.rs, mcp.rs, rng.rs, sha256.rs, api.rs, compile.rs, solve.rs) whose coverage is built from many 8–11-token matches of one or two lines each (`for … in … {`, a call, an assignment) |
| Match over 12 of our lines | 438 matches | read below |

The longest matches (up to 484 tokens over 73 lines, e.g. build.rs:702–774 against fidget's `context/mod.rs`, surface.rs:464–514 against the mikktspace port) match several unrelated files at once and share no names or literals: they are runs of ordinary `let` statements and calls that JPlag's coarse Rust tokens cannot tell apart. Read side by side, the code does different things (our part placement against fidget's affine-matrix expansion). Text-level triage: of the 438, 124 have a lexical ratio of 0.4 or more and 4 of 0.7 or more; apart from the SHA-256 case below they are `#[derive(Clone, Debug)]` structs, `#[cfg(test)] mod tests` headers, the cube-edge table in mesh.rs:105 (12 axis/corner tuples) against a libm test-case table, and the lib.rs module list.

**The one real correspondence:** sha256.rs:101–116 (the compression round) against RustCrypto sha2's `soft/compact.rs:22–41`, lexical ratio 0.91, 47 tokens over 15 lines. Both are direct transcriptions of FIPS 180-4 section 6.2.2: the names `a`…`h`, `ch`, `maj`, `t1`, `t2` and the rotation amounts are the standard's. Ours follows the standard's own layout (a full 64-word schedule, T1 summed in the standard's order h + Σ1 + Ch + K + W, constants computed from integer cube and square roots instead of a table); sha2 uses a rolling 16-word schedule, a different summation order and a constants table. Judged independent; a rewrite could not make an implementation of the standard look less like the standard.

**Result: pass.** No pair or block indicates copying.

## Step 4: cross-language ports (distinctive tokens)

`distinct.py` lists from our 26 Rust files every numeric constant with 4+ significant digits (44), every compound identifier (667) and every 6-word comment phrase (7,945), and searches the C/C++ corpus for them (and, as an extra, the Rust corpus, since JPlag never compares comments). Truncated constants count as hits when 6+ leading digits agree.

**Numeric constants: 29 of 44 found in the C/C++ corpus, 25 in the Rust corpus; all reviewed, all defined by a standard or by mathematics:**

- glTF 2.0 enums (5121–5126, 34962, 34963) and limits (32767, 65535, 65536), also in cgltf and gltfpack.
- PNG/zlib: CRC-32 polynomial 0xEDB88320, Adler-32 modulus 65521; the check values 0xCBF43926 (CRC-32 of "123456789") and 0x11E60398 (Adler-32 of "Wikipedia") are textbook test vectors, used only in our tests.
- SHA-256: 0x6a09e667, 0x428a2f98, 0xc67178f2 occur only in sha256.rs's test (lines 178–180), which checks that our computed constants equal the standard's first and last values.
- IEEE 754 and maths: exponent bias 1022/1023, 2^53 = 9007199254740992, 2^32, `PIO2_LO` = 6.123233995736766e-17 (the low part of π/2, also in musl's acos.c and libm; ours is written to 16 digits, theirs to 21), tan(π/8) = √2 − 1 = 0.4142135623730951 and 1/√3, also in Quilez's articles.
- PCG multiplier 6364136223846793005 (Knuth's LCG constant, used by the PCG paper and rand_pcg).
- Everyday numbers (1024, 0xFFFFFFFF, 4294967295, 2025, 32601 and similar).

dmath.rs's sin, cos, atan and exp use Taylor-series coefficients (reciprocal factorials and odd reciprocals, per its header), not fdlibm or musl minimax coefficients: none of musl's polynomial coefficients appear in our code.

**Identifiers: 37 shared with C/C++, 122 with Rust;** all are Rust standard-library methods (`partition_point`, `rotate_right`, `div_ceil`, `trim_end`…), glTF field names (`bufferView`, `byteStride`, `componentType`, `extensionsUsed`, `KHR_mesh_quantization`, `TEXCOORD_0`) or generic words (`rounded_box`, `crc_table`, `normal_map`, `min_feature`, `half_height`). None ties a block of our code to a corpus file.

**Comment phrases: 1 of 7,945 found** ("are bit identical on every platform", vec.rs, also in a manifold test comment): a generic phrase. 0 found in the Rust corpus.

**Dolos (optional second opinion):** not run, see note 2.

**Result: pass.**

## Step 5: weights, datasets and third-party assets; outside generators (D8)

- File types in scope: `.rs`, `.toml`, `.json`, `.evmodel`, `.md`, `.png`, `.glb` only. No `.onnx`, `.pt`, `.pth`, `.ckpt`, `.safetensors`, `.bin`, `.npz`, `.npy`, `.gguf`, `.ply`, `.obj`, `.fbx`, `.stl`, `.blend`, `.usd`, `.gltf`, `.jpg`, `.webp`, `.exr` or `.ktx2` file. Largest file 1.07 MB (each of the blueprint sheets); PNGs total 18.9 MB, `.glb` 0.46 MB.
- All 44 PNGs (benchmark reference views, docs sheets, `.vibedeck/screenshots/model-m1/`) have the signature of our own writer (png.rs): 8-bit greyscale, stored (uncompressed) deflate blocks, only the IHDR, IDAT and IEND chunks, no text chunks. No other encoder makes this combination by default.
- All 9 `.glb` files (`tests/model_bench/ref/*/mesh.glb`) are valid glTF 2.0 with `asset.generator` "Evani.Model 0.1.0 reference", the string gltf.rs:410 writes for the reference meshes that `model-bench-ref` captures from our own kart racer kit.
- Outside AI generators: no code, data or output of TRELLIS.2, OmniPart, PartField or any other generator (name search over every `.rs`, `.json`, `.evmodel` and `.toml`): the only hit is a doc comment in generator.rs:12 saying such services will plug in behind our interface later. No network code. The two external programs the code starts are `nvidia-smi` (api.rs:123, hardware detection) and `git log` (model-bench-ref main.rs:228, records the commit of the reference source). D8 holds.

**Result: pass.**

## Step 6: record

This file is the scan record (tools, corpus with commits, results, actions); the corpus and all outputs stay in `..\_scratch\evm-scan\` (`work/scancode-*.json`, `deny-*.log`, `jplag-*.jplag` and `-summary.txt`, `triage*.txt`, `distinct-*.txt`). The plan puts the record "into the milestone report": report-m1.md belongs to another worker and this scanner does not edit it. Its section 6 still says the scan is pending; its owner should change that line to the verdict (pass with notes) and the scanned commit (`6856e9d`), keeping the pointer to this file.

## Actions

None required. No code is rewritten.

## Notes

1. **JPlag's Rust support is limited.** Version 6.3.0 cannot parse shift operators, `let … else` or some comment headers; without the pre-processing above the comparison against libm (most relevant to dmath.rs) would have silently been empty. Future scans should keep `prep_rust.py` or use a JPlag release with a newer Rust grammar.
2. **Dolos was not run.** `npm install @dodona/dolos@2.9.3` into the scratch folder failed while node-gyp configured its native tree-sitter parsers on Windows (configure error writing `build\bash\binding.sln`). It is optional in the plan; the text-level triage of every flagged JPlag match (step 3) and the token search (step 4) served as the second opinion.
3. **SHA-256 round.** The only close same-language correspondence is the standard's own algorithm (step 3); recorded rather than rewritten, as Chris decided (see "Decision" below).
4. **Standard constants** (glTF, CRC-32, Adler-32, SHA-256, IEEE 754, PCG, π/2 split) appear in our code because the standards define them; none came with surrounding code.
5. **Scope.** The scan covers M1 up to commit `6856e9d`, the tip of the M1 work when this record was written. If any further M1 commit lands before sign-off, run `rescan.sh` (scratch folder; repeats steps 1, 3, 4 and 5 on the current HEAD in about ten minutes) and step 2 on that commit and add it to the snapshot table; the sign-off should name the scanned commit.
6. **Matches over the plan's limit were kept, not rewritten.** The plan says matches above the limits are "investigated and rewritten". Every one was investigated; none is copied, so none was rewritten. The SHA-256 round (step 3) is the closest case and was a question for Chris; his answer is recorded under "Decision" below.

## Decision

**2026-10-16, Chris (project owner):** accepted the reading that the plan's "investigated and rewritten" means *investigated, and rewritten only when copying is found*. The SHA-256 compression round (sha256.rs:101–116) is kept as it is, because the code follows FIPS 180-4 and differs from RustCrypto sha2 in its message schedule, its summing order and how the constants are made (see step 3). The scan's findings above are unchanged by this decision.

Sources, all 2026-10-16:

1. Chris's direct answer in his own words to the card "For Evani.Model's originality record: do you confirm keeping the standard checksum code as it is, rather than rewriting it?" (the "checksum code" is the SHA-256 routine): "Yes, keep it as it is".
2. Chris's answer to the sign-off card "Evani.Model's first milestone is ready for your sign-off. Shall I sign it off and keep the standard fingerprint code as it is?" (the "fingerprint code" is the SHA-256 routine): "Sign it off and keep the code".
3. Earlier, Chris's answer on a QQloop review card for this scan. That card offered a choice, and his yes was taken as accepting its recommended option; answers 1 and 2 have since confirmed it in his own words. He answered yes to the card's suggested change, which read: "Chris either accepts the reading 'investigated, and rewritten only when copying is found' and the record notes his decision, or the round is restructured. Recommendation: accept it, because the code follows FIPS 180-4 and differs from sha2 in schedule, summing order and how the constants are made."
