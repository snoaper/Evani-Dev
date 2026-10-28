# Stage 4 per-tier size targets: proposal for Chris

Status: **approved as provisional (Chris, 2026-10-20)**; recorded in STAGES.md Stage 4. Was: proposed, not approved. Written 2026-10-20 for the Stage 4 gate line "Its published size meets the per-tier targets" (STAGES.md). Nothing here changes `catalog/tiers.json`, `assets/tiers/*.json` or STAGES.md; once Chris approves, those are separate edits. No build, test or bench was run for this document: every number is copied from a record in the repo, and a number nobody recorded says "not measured".

## 1. What the catalog says today

The catalog sets only a texture-memory ceiling per tier. It has no download targets (DECISIONS.md "Sizes labelled by basis": "none are invented ... Stage 7 plans them"). The other size rules in force:

- **10 MB playground cap**: the raw, uncompressed ship folder, all files, all texture families together (docs/PLAYGROUNDS.md; `build --ship` and `publish` fail over it).
- **1.5 MB size line**: raw wasm + js + html + css of the build (`cargo xtask size-report`, `LIMIT_BYTES` 1536 KiB).
- **Boot group warning**: over 1 MB raw (a parameter, STAGE3_PLAN S3-11).

| Tier | Aimed at (catalog) | `max_texture_memory` (assets/tiers) |
|---|---|---|
| Lite | budget tablets, low-end phones | 64 MB |
| Low | mid-range phones, old integrated GPUs | 96 MB |
| Medium | recent phones, integrated laptop GPUs | 192 MB |
| High | desktops/laptops with discrete GPUs | 512 MB |
| Ultra | high-end desktop GPUs | 1536 MB (streamed) |
| VR | headsets (`auto_select` false) | 256 MB |

## 2. How the numbers are built

Two facts shape the targets:

1. **The code is the same on every tier.** Modules are chosen at build time, not per tier, so wasm + js does not shrink on Lite. Tiers differ only in the streamed files (per-tier `.glb` and textures, S3-4) and in the boot group. So a first-load target is a code ceiling (by kind of game) plus a boot-asset ceiling (by tier).
2. **Players download one texture family** (ASTC on phones, BC7 on desktops, or the PNG/JPEG fallback) but the package holds all of them, so the 10 MB cap binds the package, not the player.

**Headroom rule** (one rule, applied everywhere): target = today's measured size, plus the measured cost of modules the game is planned to add, plus 15 to 20 percent, rounded up to a round number. For texture memory: 75 percent of the catalog ceiling, leaving a quarter for render targets, shadow maps and MSAA that share the same GPU memory. Sizes are gzip (level 9) of what the browser fetches (index.html and the files it loads); the loader, README, licences and preview are not downloads.

## 3. Proposed targets

### 3a. First load (before the title is playable) = code ceiling + boot-asset ceiling

Code ceiling, wasm + js gzip, the same on every tier:

| Kind of game | Ceiling | Why |
|---|---|---|
| 2D (no `forward`, no `models`) | **350 KB** | Today 298.3 and 295.9 KB (Gate 2 table); +17 percent headroom |
| 3D with `models` + `anim` (kart racer after the art pass) | **500 KB** | Kart today 376.6 KB without the Stage 3 modules, plus the measured module costs: `models` +32,330 B, streaming +8,628 B, `anim` +9,300 B, relief +2,395 B = +52.7 KB gz, so about 429 KB; +16 percent headroom. `ui` would add +17,066 B and needs its own decision if the kart uses it |

Boot-asset ceiling, gzip, what must arrive before the title plays (the `[stream] boot` group):

| Tier | Boot assets | First-load total, 3D flagship (code 500 KB + boot) | First-load total, 2D game (code 350 KB + boot) |
|---|---|---|---|
| Lite | 100 KB | **600 KB** | 450 KB |
| Low | 200 KB | **700 KB** | 550 KB |
| Medium | 300 KB | **800 KB** | 650 KB |
| High | 400 KB | **900 KB** | 750 KB |
| Ultra | 500 KB | **1,000 KB** | 850 KB |
| VR | 300 KB | **800 KB** | 650 KB |

Reasoning: Lite and Low are phones on weaker links, so they get the smallest boot (the title and a placeholder; the hero model streams after, as S3-4 does). The 1 MB boot-group warning stays as the hard stop; these ceilings sit well under it. The Hexagon plan estimates about 1.6 MB over the wire for a Low first load (docs/HEXAGON_PLAN.md 4.3, an estimate); these targets are stricter because the kart racer has no large procedural-arena code.

### 3b. Total download per player (one texture family, everything the tier ever fetches), gzip

| Tier | Proposed total | Basis |
|---|---|---|
| Lite | **2.5 MB** | Hexagon plan's Low total is 3.75 MB on disk (estimate); Lite uses a lower texture set and the lowest model LODs |
| Low | **3.5 MB** | Hexagon plan Low 3.75 MB (estimate), trimmed one step |
| Medium | **5 MB** | Hexagon plan Medium 4.85 MB (estimate) |
| High | **6.5 MB** | Hexagon plan High 6.5 MB (estimate) |
| Ultra | **7 MB**, only if the package fits; otherwise the same files as High | The Hexagon plan gives Ultra the same files as High and says more needs Chris (its question 4) |
| VR | **5 MB** | Hexagon plan: VR as Medium, 4.85 MB (estimate) |

Package rule that goes with it: the raw package (code + shared models + every texture family for the largest tier + fallback) must stay **under 10 MB**, which `publish` already enforces. If both ASTC and BC7 families are shipped, a per-player texture total T costs 2T in the package, so the per-tier totals above are only reachable when 1.2 MB of code, the models and 2T fit in 10 MB. The cut order from the Hexagon plan applies if not: halve High textures first, then drop the middle model LOD, then recorded audio.

Texture supercompression stays postponed (Chris, 2026-10-20; DECISIONS.md, 2026-10-20 entry). Measured there on the sandbox ASTC base map: Zstd saves 6 to 10 percent and generic Brotli about 12.7 percent, so the "gzip" figures for texture files are close to raw.

### 3c. Texture memory (GPU), where `max_texture_memory` applies

| Tier | Catalog ceiling | Proposed game target (75 percent) |
|---|---|---|
| Lite | 64 MB | **48 MB** |
| Low | 96 MB | **72 MB** |
| Medium | 192 MB | **144 MB** |
| High | 512 MB | **384 MB** |
| Ultra | 1536 MB (streamed) | **1152 MB** (streamed; unreachable inside a 10 MB package, so in practice bounded by the download target) |
| VR | 256 MB | **192 MB** |

The 2D games do not set `max_texture_memory` (it exists only with the `texture-budget` feature, which a game's `models` turns on, STAGE3_PLAN S3-1), so for them this target does not apply. For reference, match-3's art atlas is about 1.4 MB of GPU memory at most (DECISIONS.md 2026-10-16, "Cost").

## 4. Current sizes per game

All download figures are gzip; "not measured" means no record exists in the repo.

| Game | wasm + js gzip | xtask ship gzip total | wasm raw | Folder total raw | Other recorded | Sources |
|---|---|---|---|---|---|---|
| kart-racer | 376.6 KB (wasm 360.6, js 16.0) | 393.5 KB | 840.2 KB | 1.21 MB (publish folder); 0.93 MB (ship folder) | No models, no boot assets, no `max_texture_memory` use | `.vibedeck/publish-check/size-table.md` (2026-10-16, Gate 2); DECISIONS.md 2026-10-16 "Result"; docs/PUBLISHING.md line 37 |
| 2d-platformer | 298.3 KB (282.9 + 15.4) | 303.4 KB | 654.8 KB | 0.79 MB; 0.76 MB (ship) | Same | same |
| match-3 | 295.9 KB (279.8 + 16.1) | 316.5 KB | 631.5 KB | 1.16 MB; 0.76 MB (ship); preview.png 413 KB is not a download | Atlas about 1.4 MB GPU at most | same; DECISIONS.md 2026-10-16 "Cost" |
| sandbox | js not measured. wasm 292,965 B gz with `models` + relief on, toon look (engine-only relief figure) | 245.2 KB on 2026-10-16, before the `models`/`anim`/relief work (+32.3, +8.6, +2.4 KB wasm since) | not measured | Not a playground; not measured | Streamed manifest: 5,525,943 B raw for all families, every file in group `later`, boot group empty. Derived from that manifest, one family (BC7 or ASTC, shared model and maps): about 2.95 MB for the Ultra path. Low path not measured as a total (Low fetches only `frontier-marshal.low.glb` 308,736 B, its low JPEG 77,945 B and low normal map) | DECISIONS.md 2026-10-16 "Result", "Size" entries (S3-3, S3-4, S3-10); `apps/sandbox/assets/models/stream.json` |
| model-view | not measured | not measured | not measured | not measured | Dev only, never published (`publish` refuses it) | DECISIONS.md "The Evani.Model tab's page" |

Not recorded for any game: Brotli sizes (the `size-report` tool prints them, but none are stored), per-game boot-group bytes (all are zero or empty today), and GPU texture memory in use. The Stage 3 notes record that the four games' ship wasm and js were SHA-1 identical after each Stage 3 item, so the Gate 2 code numbers above still describe the kart racer, 2d-platformer and match-3.

Older sandbox figures for context: 329.2 KB gz full and 221.2 KB no-live at the stage 6 check (`.vibedeck/research/sizes.log`, `sizes-baseline-stage6.txt`, 2026-10-14). These are history, not current.

## 5. Which games fail each proposed target today

| Target | kart-racer | 2d-platformer | match-3 | sandbox | model-view |
|---|---|---|---|---|---|
| First load, 2D code ceiling 350 KB | n/a (3D) | pass (298.3) | pass (295.9) | n/a | n/a |
| First load, 3D code ceiling 500 KB | pass (376.6; about 429 projected with models and anim) | n/a | n/a | pass on wasm alone (293); js not measured, adding about 16 KB still passes | not measured |
| Boot assets (every tier) | pass (none) | pass (none) | pass (none) | pass (empty boot group) | not measured |
| Total per player (every tier) | pass (about 0.38 MB, code only) | pass (about 0.30 MB) | pass (about 0.30 MB) | pass on the Ultra path (about 3.3 MB incl. code); Low path not measured | not measured |
| Package under 10 MB raw | pass (0.93 to 1.21 MB) | pass | pass | not a playground | never published |
| Texture memory | not measured | does not apply | does not apply | not measured | not measured |

**No game fails any proposed target today.** That is expected: all three published games carry no model assets yet. The targets bite at the Stage 4 art pass, when the kart racer gains models. Two risks to watch when it does: the converter's own real run produced a 165 MB package for the Frontier Marshal (STAGE3_PLAN line 157), so a hero asset needs its texture count and sizes held down to fit; and the kart racer's total will be set by textures, not code.

## 6. Decisions for Chris

1. Approve the code ceilings (350 KB for 2D, 500 KB for 3D with `models` and `anim`)? I recommend yes: they sit 16 to 17 percent above today's measured size plus planned module costs.
2. Approve the boot and total ladders (sections 3a and 3b)? I recommend yes for 3a. For 3b the per-tier totals lean on the Hexagon plan's estimates, which nobody has measured; I recommend approving them as provisional and fixing them after the kart racer's first netlog on Low and Ultra.
3. Approve texture-memory targets at 75 percent of the catalog ceiling (3c)? I recommend yes: it needs no measurement to enforce and leaves room for render targets.
4. Whether Ultra may download more than High while the package is capped at 10 MB. I recommend no for now (Ultra = High's files, as in the Hexagon plan).

If approved, the follow-ups are separate steps: add the numbers to `catalog/tiers.json` and `assets/tiers/*.json`, teach `publish` and `size-report` to check them per tier, and record them in STAGES.md Stage 4.
