# The Evani.Model tab: plan and contract (`evani-model-tab/1`)

Status: plan, 2026-10-16. Nothing here is built yet. Chris decided on 2026-10-16 that models get their own tab in Evani.App, next to the Evani.dev tab ("I want to see a Tab called Evani.models where we can built models", then "Its own Evani.Model tab"). This document is the plan the build follows and the contract between the tab (Evani.App) and the engine page (Evani.Dev). It follows the pattern of docs/LIVE_API.md and reuses its bridge, token, envelope and launch rules wherever they fit; when the first milestone is built, LIVE_API.md gets a short section 17 that points here (additive, the protocol stays `evani-live/1`).

Sources: the prior-art report `.vibedeck/research/2026-10-16-evani-model-tab-3d-model-viewer-ai-editing-referen.md` (options 1 and 2: the Khronos glTF Sample Viewer's control list, drawn by our own engine, and Zoo's "plan, edit, show, keep every version" way of working; ideas only, no code), docs/EVANI_MODEL.md, docs/MODEL_API.md and docs/model_research/ on branch `evani-model` (notes 17, 19 and 21, plan-characters.md), the compare tool on branch `evani-model-compare` (`crates/evani-model/src/compare.rs`), docs/MODEL_FORMAT.md on branch `s3-2-gltf`, and docs/STAGE3_PLAN.md.

Words: "the tab" = the new Evani.Model tab in Evani.App. "The page" = the engine page shown inside it (`apps/model-view`). "The model bridge" = `tools/evani-live-bridge serve` started in model mode for the tab (section 4.2). "The workspace" = Evani.Model's work folder `.evani/model/` in the engine root. "The AI" = the orchestrator of the Evani.Dev project in Evani.App and its workers. MUST / SHOULD / MAY as in LIVE_API.md.

---

## 1. In plain words (for Chris)

- You open the Evani.Model tab and see your model, big, slowly turning. You can drag to turn it, scroll to zoom, and switch between the four detail levels the games use (Low, Medium, High, Ultra), each with its triangle count.
- You type what to change ("make the hat brim a bit wider"). A one-line status shows what the AI is doing. When it is done, the new model swaps in without the view jumping.
- Every change is a line in a history list, in plain words. Undo, or "go back to here" on any line. Nothing is ever thrown away.
- Before/After flips between the last two versions with the same camera; a slider or a see-through "ghost" shows both at once.
- Point at the model and it tells you which part you are pointing at ("hat brim").
- Later: compare with a reference model (for example a Meshy master) with one verdict line ("Matches 94 percent of the outline; hat brim 12 percent too narrow").
- The 3D view is drawn by our own engine, so the model looks as it will in the games. None of this reaches players: it is a workbench, not part of any game.

---

## 2. What Chris sees

### 2.1 The first screen

```
┌──────────────────────────────────────────────────────────────────────┐
│ Model: [ marshal-3a1f ▾ ]          Low 3.0k │ Med 8.0k │ High 20k │ Ultra 60k │
│                                                         [ Before | After ]   │
│                                                                      │
│                       big 3D view (the page)                         │
│                  drag to turn, scroll to zoom, ground grid            │
│                       "hat brim"  (part under the pointer)           │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│ AI: Making the brim wider... building... checking... done (v8)       │
│ [ Say what to change ...                                   ] [Send]  │
├──────────────────────────────────────────────────────────────────────┤
│ History                                                    [Undo]    │
│  8  Made the hat brim wider (8 percent).       you asked: "brim..."  │
│  7  Went back to version 5.                                 [go back to here] │
│  6  Lengthened the coat by 6 cm.                            [go back to here] │
│  ...                                                                 │
│ ▸ Details                                                            │
└──────────────────────────────────────────────────────────────────────┘
```

| Element | Behaviour |
|---|---|
| 3D view | The page (section 4). Auto-turns gently until Chris touches it; drag turns (orbit around the model's centre), scroll or pinch zooms, double-click frames the whole model. A ground grid and a soft shadow under the model. |
| Model picker | Every model in the workspace, newest first. A new model made by a request opens by itself. |
| Tier switch | Four buttons, each with that tier's triangle count. Switching keeps the camera. A tier the version doesn't have is greyed out. |
| Before / After | Flips the view between this version and the one before it (its parent), same camera. Next to it, "Show both": a slider (left before, right after, drag the divider) or a ghost (the earlier version as a see-through outline over the new one). |
| Part name | The part under the pointer, as a small label next to the pointer, drawn by the page. Nothing is shown when the pointer is off the model. |
| AI status | One line: the AI's current step, from the moment Chris presses Send: "Reading your request", then the AI's own steps ("Changing the model", "Building", "Checking"), then "Done (v8)", "Couldn't do it: <one line>" or "The AI has a question (see the chat)". |
| Say what to change | A text box and Send. The words go to the AI (section 7). Disabled while the tab is not connected. |
| History | One line per version, newest first: number, the AI's one-sentence summary of the change, Chris's own words when he asked for it. Undo (go back one step) at the top; "go back to here" on every row. Going back never deletes: it adds a new version that equals the old one (section 8.4), so going back can itself be undone. Hovering a row highlights the parts that version changed. |
| Details (closed by default) | Counts per tier (triangles, vertices, size in KB, draw calls), the checks (musts passed, mesh checks), overlays (section 2.2), compare scores and measurements (section 9), and the intent text the version was built from (read only). |

### 2.2 Details: overlays and numbers

- Overlays, one menu, all off by default: wireframe of the shown tier, flat colour by part, collision shapes, anchors as small labelled markers, bounds with its size in metres, normals. In the first milestone only the counts and the intent text are there; the overlays are milestone T5.
- Numbers come from files Evani.Model already writes (`<model>.evmodel.json`, section 8.2) and from the page's own stats (section 5.4). The tab never computes geometry itself.

### 2.3 Compare with a reference (milestone T3)

- "Compare with..." in Details picks a reference: a Meshy master in `art-source/meshy/<id>/<id>.master.glb` or any other `.glb` on this PC that Chris picks in the tab's file dialog. Any readable `.glb` is allowed because only someone holding the dev token (the tab, after Chris's pick, or the AI) can name one, and the file is only ever read (5.3, 9). The choice is kept per model.
- The view shows ours and the reference side by side, or with the same slider as Before/After, aligned the way the compare tool aligns them (both standing on the ground, centred, never rescaled).
- One verdict line under the view: "Matches 94 percent of the outline; hat brim 12 percent too narrow". The numbers per view and per measure, and the compare tool's sheet picture, are in Details.
- The verdict is worked out again for every new version, so after each change Chris sees whether it moved closer.

### 2.4 What v1 leaves for later

Not in the first milestones (from the research report), each to be planned when it is wanted: normals, UV and wireframe overlays beyond T5's list; skeleton display and clip playback (T6, after Stage 3's animation items); lighting presets; a look switch (the model under another look); a grid of three variants for vague requests; branching history (history stays one list); click a part, then type a change for that part; sliders for the intent's numbers (brim width, height) so small tweaks need no request; a coloured silhouette difference drawn live in the 3D view (T3 shows it in the sheet picture).

---

## 3. Who draws what

```
 Evani.App                                      Evani.Dev (dev build, `live` on)
 ─────────                                      ────────────────────────────────
 Evani.Model tab ──HTTP GET /live/status──────▶ evani-live-bridge serve --models  (127.0.0.1:8097)
   chat box, history, status, buttons           │  HTTP: the page, /live/status, /model/v1/…
   │──WS /ctl (token)── view.* and model.* ───▶ │  WS:   /engine  /ctl
   page view ──HTTP GET / (apps/model-view)───▶ │◀─WS /engine (token)── the page: 3D view, hover, stats
   │                                            │
   │                                            │  watches .evani/model/ (new versions, status)
   └─ typed words ─▶ orchestrator of the        │  runs the compare tool (one at a time, below normal)
      Evani.Dev project                         │
        │ MCP stdio                             ▼
        ▼                                     .evani/model/<model>/v<N>/  (every version, kept)
      evani-model mcp (model_make, model_edit, model_compile, ...) ──writes──▶
```

| Role | Draws or does | Talks |
|---|---|---|
| The tab (Evani.App) | Everything that is not 3D: model picker, tier buttons with counts, Before/After and "Show both", the status line, the text box, the history list with Undo and "go back to here", Details. Starts the model bridge, shows the page, sends the typed words to the AI. | HTTP `/live/status`; WS `/ctl` with the token; the page in a view; chat to the orchestrator. |
| The page (`apps/model-view`) | Everything 3D: the model on the turntable, tiers, the ghost, the split slider, the reference, overlays, highlight, the part label under the pointer. Reports stats, the hovered part and load errors. Draws no buttons of its own (outside a browser opened without the tab, where it shows a minimal fallback panel for tests). | WS `/engine` (as every live page). |
| The model bridge | Serves the page and the workspace files, routes `view.*` commands to the page, answers `model.*` commands itself, watches the workspace, runs the compare tool. Holds no render state. | HTTP and WS on 127.0.0.1:8097. |
| Evani.Model (`tools/evani-model`) | Makes, edits and compiles models when the AI calls its MCP tools; writes each version folder and the status file. Knows nothing of the tab or the bridge: the contract between them is files (section 8). | MCP stdio with the AI; files. |
| The AI | Turns Chris's words into an intent change, compiles a new version with a one-sentence summary. | Chat in; MCP tools out. |

The split keeps the rule of LIVE_API.md section 0: one API with three doors. Everything the tab can do, the AI and test scripts can do through the same commands on `/ctl` (and, for the AI, through MCP tools, section 5.6).

---

## 4. The page and how it is served

### 4.1 `apps/model-view`

- The dev-only app STAGE3_PLAN.md already plans for the quality gate (S3-6) "and the tab"; the tab's milestones build it first and S3-6 extends it, so there is one model viewer, not two.
- Its `evani.toml` lists `models` (and `anim` from T6), the `forward` 3D path, the `live` module and `capture`; it uses the toon look (the kart racer's) by default. No `[playground]` section: it is never a game. `cargo xtask publish model-view` refuses it with a `fix:` line (a check added in T1a), and its ship build would carry no `live`, so it has nothing to show.
- Fixed 60 Hz step like every game: the turntable's angle is a function of the tick count, never of wall-clock time, so a capture at tick N is the same picture on every run.
- Drawing. Until Stage 3's model pipeline (S3-3) is merged, the page draws Evani.Model's files through the existing 3D path with the `meshes` feature: each tier's triangles are decoded by `evani-gltf` (S3-2) and grouped into one mesh per region colour (from `COLOR_0`), each drawn as an instance in that colour; skinned solids are shown in their stored rest pose (the T-pose Evani.Model writes). The page shows a small corner note "Preview shading" until T4 switches it to S3-3's model pipeline, which is the path games use; only from T4 on does "looks exactly as in the game" hold, normal maps included.
- Hover and highlight need no change to `evani-render`: picking is on the CPU (section 10), the highlight is a recoloured instance. If the split slider or the ghost needs a change in `crates/evani-render/src/gpu/frame.rs` or `pipelines.rs`, it follows Stage 3's merge order for those files (STAGE3_PLAN.md section 5) and is shared with S3-14's slider, built once. S3-14 built that split as `evani-render`'s dev-only `model::Split` (behind the `hooks` feature: one frame, `left` scissored left of `at`, `right` the rest) without touching `frame.rs` or `pipelines.rs`; the model view reuses it by turning on `hooks`. `apps/model-view` is deliberately not wired to `evani-live?/models` (drop-to-convert): it keeps its own `view.*` commands and never takes drops (DECISIONS, S3-14).
- Pointer: drag to orbit uses ordinary pointer events. No pointer lock, no keyboard lock, no fullscreen. A drag ends on pointer up, on window blur and when the tab is hidden.

### 4.2 The model bridge

The same bridge binary as the Evani.dev tab's, started a second time with model mode on:

```
evani-live-bridge serve --root <engine root> --port 8097 --game apps/model-view \
    --session .evani/live/model/session.json --models .evani/model
```

- Port **8097** (the Evani.dev tab keeps 8096; the review scripts start at 8180).
- `--session` (LIVE_API 2.4) keeps the tab's own session file apart, so neither bridge overwrites or deletes the other's.
- `--models <folder>` turns on model mode: the `/model/v1/` routes (4.3), the workspace watcher (section 8.3), the `model.*` commands (5.3) and the compare runner (section 9). Without the flag the bridge behaves exactly as today, byte for byte in its replies (test `game_bridge_unchanged_without_models_flag`). The folder MAY point outside the engine root (for example another worktree's `.evani/model/`) while the Evani.Model branch is not merged yet.
- Launch from the tab: a second launcher shim `.vibedeck/dev-harness/model.mjs`, with every rule of LIVE_API 2.1 (Node built-ins only, `--port`, `--quiet`, engine root `../..`, answers `/live/status` with `phase: "building"` while it compiles the bridge, then runs the bridge with the arguments above, exits with its code). Without the tab: `cargo xtask live --game apps/model-view --port 8097 --models`.
- `/live/status` is LIVE_API 3.1's JSON with `"game": "model-view"` and one more field: `"models": { "folder": ".evani/model", "count": 4, "latest": "marshal-3a1f" }`. The tab checks `game` and `models` to know it found the right bridge.
- Token, Host and Origin rules: exactly LIVE_API sections 3 and 4. Page state, paused answers and parked pacing (LIVE_API 3.5) apply to the page unchanged, so a hidden tab costs about 5 frames a second.

### 4.3 HTTP routes (model mode)

| Method, path | Auth | What |
|---|---|---|
| `GET /` | none | The page (token injected, LIVE_API 4.3). |
| `GET /live/status` | none | Status JSON (4.2). |
| `GET /model/v1/models` | none | `[{ "model": "marshal-3a1f", "class": "character", "versions": 8, "latest": 8, "updated": "2026-10-16T19:02:11Z" }]`, newest first. |
| `GET /model/v1/<model>/history` | none | Every `version.json` of that model (8.2), oldest first. |
| `GET /model/v1/requests/<request>` | none | That request's status file (7.3), or `{ "state": "idle" }` when there is none. |
| `GET /model/v1/<model>/v<N>/<file>` | none | One file of a version, only if that version's `version.json` lists it (tier files, the metadata file, the intent, the compare sheet). Anything else, any `..`, any path outside the workspace: 404. |
| `GET /model/v1/<model>/reference.glb` | none | The model's chosen reference file (section 9), streamed from where it lives. Only the one path stored in that model's `reference.json` (set by `model.reference.set`) is ever served; with no reference set, 404 (test `reference_route_serves_only_the_set_file`). |

GET routes need no token: they only read dev files, the Host check (LIVE_API 3) stops other pages reaching them through DNS rebinding, and a request with a foreign `Origin` is refused (403). Everything that writes goes through `/ctl` with the token.

### 4.4 The page's address holds the view

`http://localhost:8097/?live=1#model=marshal-3a1f&v=8&tier=high&cam=35,15,4.2` (azimuth and elevation in degrees, distance in metres; `&before=7&show=slider&split=0.5`, `&ref=1` when comparing). The page reads it at start and keeps it up to date (`history.replaceState`), so a reload comes back to the same view and a test or capture script can open an exact view by address. The tab MAY set it directly; it normally sends commands instead.

---

## 5. Messages

### 5.1 Envelope, connection and rules

- The tab connects to `ws://127.0.0.1:8097/ctl` with `Authorization: Bearer <token>`, the token read from `.evani/live/model/session.json` (the session file, LIVE_API 2.4; presented as in 4.2; the same way LIVE_API section 12, recommended change 4, expects a future tab panel to connect). It reconnects with backoff (1 s doubling to 30 s) and rereads the session file each time, because a bridge restart makes a new token.
- Commands, results and events use LIVE_API 3.3's envelope unchanged: `{"type":"cmd","v":1,"id":"t-17","cmd":"view.load","params":{…}}` → `{"type":"result","v":1,"id":"t-17","ok":true,"result":{…}}`.
- `view.*` and `model.*` are **view commands**: they change what is shown, or add a version by copying files; they never touch the registry (parameters, slots, curves), so they need no `note`, never enter the live history (LIVE_API 6.20), are never baked and never trigger auto-revert.
- `view.*` go to the page (they fail fast with `no_engine` when the page is not connected, LIVE_API 3.3, and answer `paused` while it is paused, 3.5). `model.*` are answered by the bridge itself and work with no page.
- Events reach every `/ctl` client that sent `view.watch` (5.2); the bridge's own events (5.4) are broadcast to all `/ctl` clients.

### 5.2 Tab → page (`view.*`)

| Command | Params | Result |
|---|---|---|
| `view.watch` | `{}` | `{ "watching": true }`. From now on this connection gets the page's events (5.4). |
| `view.load` | `{ "model": "marshal-3a1f", "version": 8, "tier": "high", "keep_camera": true }` | `{ "loaded": { "model", "version", "tier" }, "stats": { … } }` after the swap (section 6). `keep_camera: false` frames the model. |
| `view.tier` | `{ "tier": "low" }` | Same as `view.load` of the shown version at that tier. |
| `view.before_after` | `{ "show": "after" \| "before" \| "slider" \| "ghost", "before": 7, "split": 0.5 }` | `{ "show", "before", "after" }`. `before` defaults to the shown version's parent. Same tier and camera for both. |
| `view.reference` | `{ "on": true, "show": "side_by_side" \| "slider", "split": 0.5 }` | `{ "on", "reference": { "file", "tris" } }`. Needs a reference set with `model.reference.set`. |
| `view.highlight` | `{ "parts": ["hat_brim", "hat_crown"] }` (empty list clears) | `{ "highlighted": [...], "unknown": [...] }`. Tints those parts in the shown tier. |
| `view.camera` | `{ "az": 35, "el": 15, "dist": 4.2 }` or `{ "frame": true }` or `{ "turntable": false }` | `{ "az", "el", "dist", "turntable" }`. |
| `view.overlay` (T5) | `{ "wireframe": true, "parts": false, "collision": false, "anchors": false, "bounds": false, "normals": false }` | The overlay state. |
| `view.state` | `{}` | Everything shown now: model, version, tier, before/after, reference, camera, overlays, highlighted parts, the page's address (4.4). |

The page's existing live commands (`capture.frame`, `session.status`, `tier.set` for the renderer's quality tier) work on it as on any live page. The render tier of the viewer (its own quality) is separate from the model tier being shown; the viewer runs at High on the dev PC unless told otherwise.

### 5.3 Tab → bridge (`model.*`)

| Command | Params | Result |
|---|---|---|
| `model.list` | `{}` | As `GET /model/v1/models`. |
| `model.history` | `{ "model": "marshal-3a1f" }` | Every `version.json`, oldest first. |
| `model.restore` | `{ "model": "marshal-3a1f", "version": 5 }` | `{ "version": 9, "restored_from": 5 }`. Adds a new version equal to version 5 (section 8.4). This is "go back to here". |
| `model.undo` | `{ "model": "marshal-3a1f" }` | `{ "version": 9, "restored_from": 7 }`, or the error `nothing_to_undo` at version 1. One step back by the rule of 8.4, so pressing it again keeps walking back. |
| `model.reference.set` | `{ "model": "marshal-3a1f", "file": "art-source/meshy/marshal/marshal.master.glb" }` (or `null` to clear) | `{ "reference": { "file", "sha256", "tris" } }`, then a compare of the latest version starts (section 9). The file must be a readable `.glb` (checked by its header, not its name); the bridge only ever reads it. |
| `model.compare` | `{ "model": "marshal-3a1f", "version": 8 }` | `{ "queued": true }` or the cached result; the result arrives as a `model_compare` event. |

### 5.4 Events

From the page (to clients that sent `view.watch`):

| Event | `data` | When |
|---|---|---|
| `view_loaded` | `{ "model", "version", "tier", "ms" }` | After each swap. |
| `view_stats` | `{ "tier", "tris", "verts", "draws", "kb", "frame_ms", "gpu_ms" }` | After each load, then at most once a second while it changes. |
| `view_hover` | `{ "part": "hat_brim", "role": "hat", "solid": "head" }` or `{ "part": null }` | When the part under the pointer changes; at most 10 a second. |
| `view_error` | `{ "code", "message", "file" }` | A file that does not load or decode (the evani-gltf error code, MODEL_FORMAT.md "Error codes"); the old model stays on screen. |

From the bridge (to every `/ctl` client):

| Event | `data` | When |
|---|---|---|
| `model_version` | the new `version.json` | A new version folder became complete (8.3). |
| `model_status` | the request's status file (`request`, `model`, `state`, `step`, `line`, `version`) | A file in `_requests/` was created or changed (7.3). |
| `model_compare` | `{ "model", "version", "reference_sha256", "verdict", "iou": { … }, "measures": { … }, "sheet": "<url>" }` or `{ …, "error" }` | A compare finished (section 9). |

### 5.5 Errors

LIVE_API 5.5's codes and shape, plus: `unknown_model`, `unknown_version`, `no_tier` (the version has no such tier), `no_reference`, `bad_reference` (not a readable `.glb`), `nothing_to_undo`, `busy_compare` (only one compare runs at a time; the request is queued and the answer says its place). Every error has a one-line `message` a person can read; the tab shows it in the status line.

### 5.6 The same doors for the AI

The MCP server for the model bridge is `evani-live-bridge mcp --session .evani/live/model/session.json` (one more entry in `.mcp.json`, named `evani-model-view`, added in T1b). Today the `mcp` mode takes only `--root` and always reads `.evani/live/session.json`; T1b adds `--session` to it, as `serve` already has. Its tools mirror the commands (`view_load`, `view_state`, `view_capture` for `capture.frame`, `model_history`, `model_restore`, `model_undo`, `model_compare`), so the AI can look at exactly what Chris sees, at the same camera, when he says "look at the left boot".

---

## 6. Hot swap

When a version is loaded (by `view.load`, or on `model_version` when the tab follows the latest):

1. The page fetches the tier file in the background and decodes it; the old model keeps drawing.
2. On the first frame after decoding finishes, it swaps: same camera, same turntable angle, same overlays and highlight (parts that no longer exist are dropped from the highlight). The previous version is kept in memory for the ghost and the slider, so Before/After right after a change needs no download.
3. It sends `view_loaded` and `view_stats`.

Rules: no page reload, ever (the frame counter keeps rising; test `hot_swap_keeps_camera_and_frame_counter`). A file that fails to load leaves the old model and sends `view_error`. At most three models are held at once (shown, before or ghost, reference); others are freed. The camera is re-framed only when Chris asks (double-click, `frame: true`) or when the new model's bounds no longer fit the view at all (its box is outside the view frustum).

The tab follows the latest version automatically when Chris has not moved to an older one; when he is looking at an older version, it shows "Version 9 is ready" instead of swapping.

---

## 7. From typed words to a new version

### 7.1 What the tab sends to the AI

As the Evani.dev tab does (LIVE_API 2.5): to the orchestrator of the Evani.App project whose folder is the engine root, as chat, in this form:

```
[Evani.Model] make the hat brim a bit wider
Model: marshal-3a1f, version 8, tier high
Request: r-7f3a9c-0042
```

- `Request` is a tab-made id: `r-`, six random hex characters made once each time the tab starts, `-` and a counter. It never repeats across restarts, so an old `version.json` or status file can never match a new request. For a new model the second line is `Model: none (new model)`.
- The tab sets the status line to "Reading your request" at once and keeps the request open until an event names that id. It closes a request only on a `model_version` or `model_status` event that arrives after it sent the request and carries that exact id.

### 7.2 What the AI does (the procedure, in this project's AI guidance from T1b)

1. Read the request and the current version's intent (`intent.evmodel` in the version folder, or `model_describe`).
2. Change the model: `model_edit` with the words or an intent diff (Evani.Model M3), or until then a changed intent through `model_make`, always passing `request: "r-7f3a9c-0042"`.
3. Compile a new version: `model_compile` with `request` and `summary`: one plain sentence, past tense, at most 100 characters, saying what changed for a person ("Made the hat brim wider (8 percent)."). The tool writes the next version folder (section 8).
4. Answer in the chat in one line. If it cannot do it, or needs Chris to choose, it says so with `model_status` (7.3) and in the chat; it never leaves the status at "working".

### 7.3 The status file

One file per request, `.evani/model/_requests/<request>.json`, written by Evani.Model (never by the AI by hand). It is per request, not per model, so a request for a new model has a place for its status before any model folder exists; `model` is `null` until the first `model_make` names the model. (`_requests` can never clash with a model, because model ids never start with `_`.) Tool calls made without a request id (scripts, benches) write no status file.

```json
{ "request": "r-7f3a9c-0042", "model": "marshal-3a1f", "state": "working", "step": "building", "line": "Building the four tiers", "version": null, "updated": "2026-10-16T19:01:58Z" }
```

| `state` | Written when | The tab shows |
|---|---|---|
| `working` | Each model tool starts (`step`: `changing` for make, edit and fit; `building` for compile; `checking` for check) | "Changing the model", "Building", "Checking" |
| `done` | A compile wrote the version (`version` set) | "Done (v9)" |
| `failed` | A tool failed, or the AI said so with `model_status` (`line`: one plain sentence) | "Couldn't do it: <line>" |
| `asking` | The AI needs Chris (`model_status`) | "The AI has a question (see the chat)" |
| `idle` | Nothing open | nothing |

`model_status` is one small Evani.Model MCP tool (`{ "request", "model": null or the id, "state": "failed" \| "asking", "line" }`; `model` may be left out for a new model) added in MT-a. Example: Chris types "make me a new marshal", the AI's first `model_make` fails on a misspelt word and it cannot fix it; the tool has already written `state: "failed"` with its one-line error to `_requests/r-7f3a9c-0043.json`, the bridge sends `model_status`, and the tab shows "Couldn't do it: <line>". The `updated` time is for display only ("12 s ago"); nothing decides on it. If a request has had no new status for 10 minutes, the tab says "No news from the AI for 10 minutes" but keeps the request open.

### 7.4 How the result gets back

The bridge sees the new version folder (8.3) and broadcasts `model_version` with its `version.json`, which carries the `request` id and the `summary`. The tab adds the history line, closes the request and, if it is following the latest, sends `view.load` for the new version. Nothing else is needed: the AI does not talk to the tab, and Evani.Model does not talk to the bridge.

---

## 8. Version storage

### 8.1 Folder layout

```
.evani/model/                       (gitignored, project-local, Evani.Model's workspace)
  _requests/<request>.json          status per request (7.3)
  <model>/                          e.g. marshal-3a1f
    reference.json                  section 9 (only when a reference is set)
    v1/
      version.json                  8.2
      intent.evmodel                the exact intent this version was built from
      <name>.low.glb … <name>.ultra.glb   the tier files, as Evani.Model names them
      <name>.evmodel.json           Evani.Model's dev-side metadata (MODEL_FORMAT.md "Files per model"), plus the triangle ranges of section 10
      compare/<reference sha256>.json and .png   cached compare results (section 9)
    v2/ …
```

This matches MODEL_API.md's `model_compile` output (`.evani/model/<model>/v<N>/…`). Versions are never deleted by the tab, the bridge or the AI; clearing old ones is Chris's call (a later "clean up" command, not in these milestones). Saving a version to the library (`model_save_to_library`) copies it out; publishing stays Evani.App's publish card and Chris's click.

### 8.2 `version.json`

```json
{ "format": 1, "model": "marshal-3a1f", "version": 9, "parent": 8,
  "request": "r-7f3a9c-0042", "said": "make the hat brim a bit wider",
  "summary": "Made the hat brim wider (8 percent).", "by": "ai",
  "restored_from": null, "via": null, "changed_parts": ["hat_brim"],
  "hash": "sha256:4e07…", "intent_hash": "sha256:91ab…",
  "tiers": { "low": { "file": "marshal.low.glb", "tris": 2980, "verts": 1702, "kb": 61 },
             "ultra": { "file": "marshal.ultra.glb", "tris": 59800, "verts": 31022, "kb": 1410 } },
  "meta": "marshal.evmodel.json", "intent": "intent.evmodel",
  "checks": { "must": "9/9", "mesh": "ok", "gltf": "ok" },
  "created": "2026-10-16T19:02:11Z" }
```

- `by`: `ai` (a request), `chris` (a restore from the tab), `tool` (a script or a bench run).
- `changed_parts`: parts whose measures or generator settings differ from the parent's (from the intent diff), for the history row's highlight.
- `hash` is the model hash (SHA-256 of the tier files in tier order), so two versions with the same hash are the same model, byte for byte.
- `created` is display only.

### 8.3 Writing and watching

- Writers (Evani.Model's compile and the bridge's restore) build the version in a temporary folder `v<N>.tmp-<pid>` inside the model folder, write `version.json` last, then rename it to `v<N>` where N is one more than the highest complete version. If the rename fails because `v<N>` exists (another writer won), they retry with N+1 and rewrite `version` in `version.json`; `parent` stays the version the writer started from (the one its change was made on), so the history tells the truth when two writers start from the same version: both from v8 give v9 and v10, each with `parent: 8`. An undo or "go back" that loses the race is worked out again from the new latest version before it retries, so it never undoes someone else's version by accident. A folder without `version.json`, or named `*.tmp-*`, is never read.
- The bridge checks the workspace every 250 ms (as it watches page files, LIVE_API 14.1): a new complete `v<N>` sends one `model_version`; a new or changed file in `_requests/` sends one `model_status`.

### 8.4 Undo and "go back to here"

`model.restore` copies version K's files byte for byte into a new version (N+1) with `parent: N`, `restored_from: K`, `by: "chris"`, `summary: "Went back to version K."` and the same `hash` as K. No compile runs, so it is instant and exact. The history stays one list in creation order; branching history is for later. The next AI edit starts from the latest version, which is now the restored one.

Every restore records how it was made: `"via": "undo"` or `"via": "go_back"` in `version.json` (`null` for versions that are not restores).

**Undo undoes Chris's last action, and walks back one step at a time** (`model.undo`, worked out by the bridge so the rule lives in one place). Take the latest version L:
- L was a "go back to here" (`via: "go_back"`): undo returns to the version Chris left, L's `parent`.
- L was an undo (`via: "undo"`, `restored_from: K`): its content is K's, so one step further back is K's `parent`.
- Otherwise (an AI version): L's `parent`.

Undo restores that version with `via: "undo"`. Example: versions 1 to 8 made by the AI. Undo makes v9 = copy of v7. Undo again: v9 shows v7, whose parent is v6, so v10 = copy of v6. Undo again: v11 = copy of v5. Each press goes one step further back; it never flips between two versions. Another example: from v8, "go back to here" on v3 makes v9 = copy of v3 (`via: "go_back"`); Undo then makes v10 = copy of v8, where Chris was. To redo, Chris presses "go back to here" on the row he left (v8 in the example). Undo at a version whose step back is nothing (the first version) answers `nothing_to_undo` and adds no version.

---

## 9. Reference compare and the verdict line

- `reference.json` (per model): `{ "file": "art-source/meshy/marshal/marshal.master.glb", "sha256": "…", "set": "2026-10-16T19:10:00Z" }`. The reference file is read where it lives and never copied into the workspace or the repository (Meshy masters stay in the gitignored `art-source/`). Any readable `.glb` path is accepted, because only a token holder can set it. When the file's size or modified time differs from what was seen last, the bridge hashes it again and updates `sha256`, so a changed reference gets fresh compares (the cache is keyed by that hash).
- When a reference is set, and for every new version after that, the bridge runs `evani-model compare` (the tool on branch `evani-model-compare`, plan-characters.md section 8) on the version's ultra tier against the reference, one compare at a time, at below-normal priority, with its thread count capped at a quarter of the CPU's threads. The result and its sheet go to `v<N>/compare/<reference sha256>.json` and `.png`; a cached result is reused, so a version is compared once per reference.
- **The verdict line is written by the compare tool** (a `verdict` field added to its JSON in T3), so it is deterministic and the same in the tab, in reports and for the AI:
  - start: "Matches P percent of the outline", P = the mean silhouette IoU times 100, rounded to a whole number;
  - then the measure furthest off, if any is more than 5 percent off (the bar in plan-characters.md, MC4): "; <plain name> Q percent too <word>" (for example "hat brim 12 percent too narrow", "coat hem 6 percent too low"); the plain names and the two words of each measure (narrow / wide, short / tall, low / high, thin / deep) live in the tool's data, beside the measure rules;
  - otherwise: "; every measured proportion within 5 percent";
  - classes without measure rules (karts today) get only the first part.
- The tab shows the verdict under the view while a reference is set, and the history row shows the change against the previous version's verdict ("Matches 94 percent (up from 91)").
- The page draws the reference beside or behind the slider (5.2 `view.reference`) with the compare tool's alignment (lowest point on the ground, centred on its box in x and z, never rescaled). Until T4, a textured reference is drawn plain grey; the compare itself uses outlines only, so the verdict does not depend on how the page draws it.

---

## 10. Part hover

### 10.1 The data we have

Evani.Model knows each triangle's part (`tri_part` in `crates/evani-model/src/mesh.rs`; skin weights and region colours are made from it). Its glTF writer emits one mesh primitive per solid (a merged group of parts), and the part list in `extras.evani.parts` says which solid each part belongs to (MODEL_FORMAT.md, `parts`). So a file today says which solid a triangle is in, but not which part.

### 10.2 Decision: part ranges in the dev-side metadata file

Evani.Model's writer orders each solid's triangles by part (a stable sort on the part's index, keeping the mesher's order within a part), and writes, for each tier and solid, the triangle ranges per part into `<name>.evmodel.json`:

```json
"tri_ranges": { "high": { "head": [["head_skin", 0, 4120], ["hat_brim", 4120, 1380], ["hat_crown", 5500, 2210]] } }
```

(part name, first triangle, count; the ranges of a solid cover each of its triangles exactly once). The page decodes the tier file, casts a ray from the pointer against the triangles on the CPU (a bounding-volume tree built once per load; nearest hit, ties to the lowest triangle index), and looks the hit triangle's index up in its solid's ranges. A file without ranges (an older version, a converted or foreign file) shows the solid's name instead.

Why this one:

| Option | Shipped bytes | Exact per triangle | Notes |
|---|---|---|---|
| A part-id vertex attribute (`_PART`) | 1 to 4 bytes per vertex in every file, unless a strip step rewrites the vertex buffers | No: a vertex on a seam belongs to two parts, and seams are exactly where the pointer goes | Rejected. |
| One primitive per part | More vertices (every seam split) and one draw call per part instead of per solid (a 60-part character against the tier's `max_draw_calls`) | Yes | Rejected: a dev feature would cost players. |
| **Ranges in the dev-side metadata** | **0**: the `.glb` has the same triangles and vertices, only the index order changes | **Yes** | **Chosen.** A few KB per model in a file the game never reads. |

Determinism: the sort is a fixed rule on fixed data, so the files stay byte-identical at 1 and 8 threads. Region colours (`COLOR_0`, "a vertex takes the region of the lowest-index triangle using it") and skin weights are computed before the sort, from the mesher's order, so their bytes do not change (test `colours_and_weights_unchanged_by_grouping`). The `.glb` bytes do change once, when the sort is introduced; that is a normal tool-version change and is recorded in its DECISIONS.md line.

Dev-only: the ranges live only in `<name>.evmodel.json`, which MODEL_FORMAT.md already calls dev side ("the engine doesn't need it to draw"). The converter (S3-5) never copies that file into a package, and gltfpack reorders triangles anyway; T1b adds a publish preflight check that refuses a package containing an `.evmodel.json`. The picking code is in `apps/model-view` only.

---

## 11. Rules (the engine's principles, kept)

1. **Dev-only.** Nothing of this reaches a player: `apps/model-view` is never published (4.1), the model mode is in the native bridge, the view commands live in the page's `live` code that ship builds drop (`cargo xtask build --ship` fails on dev-only markers), the triangle ranges stay in a dev-side file (10.2). Test: the ship builds of all four games (sandbox, kart-racer, 2d-platformer, match-3) have the same wasm and js SHA-1 before and after each milestone.
2. **Build-time on/off.** `apps/model-view` turns on `models`, `forward`, `live` and `capture` for itself only (`anim` from T6). No game's `evani.toml`, features or download changes. `--models` is off unless given.
3. **Determinism.** Fixed 60 Hz step; the turntable follows ticks; same version, tier and camera give the same capture. Version bytes come only from Evani.Model's deterministic compile or a byte copy. The verdict is text made from deterministic numbers. Clock times are shown, never used to decide.
4. **Light.** The page loads one tier at a time (plus at most a before version and a reference); the hidden tab is parked at about 5 frames a second (LIVE_API 3.5); one compare at a time, below-normal priority, capped threads; no JavaScript 3D viewer or third-party library in the page or the tab; the bridge watcher reads only folder names, file sizes and times, and the changed request files on each check.
5. **Evani.Model stays separate.** It never depends on the bridge, the page or engine crates, and the engine never links `evani-model`: they meet in files (sections 7.3, 8, 10) and the compare tool runs as a separate program. This keeps the Evani.Model track's rule (STAGES.md) that it never changes evani-core or evani-render.
6. **Safety.** Same token, Host and Origin rules as LIVE_API section 4; file routes serve only listed files; a reference is only read, never copied or changed, and only a token holder can name one; the bridge never deletes a version. WebGPU only, no fallback. No pointer lock.
7. **Original work.** The control list and way of working are ideas from the Khronos viewer, Zoo, Babylon's inspector and model-viewer (research report); no code from any of them.

---

## 12. Milestones

In order. Each ends with `cargo xtask check` clean, `cargo xtask check --full` before hand-over, a commit, a QQ review, and Chris trying it in the installed Evani.App. None is a gate box; nothing here ticks a gate. Evidence (captures, logs) goes in `.vibedeck/screenshots/model-tab/<milestone>/`, taken in muted headless Chrome.

| # | What | Size | Worker | Where | Depends on |
|---|---|---|---|---|---|
| MT-a | Evani.Model writes what the tab reads | S | sonnet-high | Evani.Dev, `evani-model` branch | Evani.Model M1 (done); after the worker busy on that branch now finishes |
| T1a | First light: the page, the bridge's model mode, turntable, tier switch, hot swap | M | opus-high | Evani.Dev | S3-2 merged into m1-foundation; MT-a |
| T1b | Part hover, the AI's MCP door, publish check, docs | M | opus-high | Evani.Dev | T1a |
| A1 | The tab, first version | M | Evani.App's own worker (opus-medium) | Evani.App | T1a (may start then); its acceptance needs T1b for hover |
| T2 | History, undo, Before/After (slider, ghost) | M | opus-medium | Evani.Dev | T1b (the history highlight uses `view.highlight`) |
| A2 | History list, Undo, go back, Before/After buttons | S | Evani.App's own worker | Evani.App | A1, T2 |
| T3 | Reference compare with its verdict line | M | opus-medium | Evani.Dev (bridge and the compare tool) | T2; `evani-model-compare` merged into `evani-model` |
| A3 | "Compare with...", the verdict line, compare numbers in Details | S | Evani.App's own worker | Evani.App | A2, T3 |
| T4 | Drawn as in the game | S | opus-medium | Evani.Dev | T1a; Stage 3's S3-3 (S3-10 for relief) |
| T5 | Details overlays | M | sonnet-high | Evani.Dev, plus a small Evani.App menu | T1b |
| T6 | Characters in motion: skeleton overlay, clip playback | M | opus-medium | Evani.Dev, plus play controls in Evani.App | Stage 3's S3-7, S3-8 and S3-9 |

**Dependency on Stage 3.** T1a needs S3-2 (our glTF reader) merged; T4 needs S3-3 (the model pipeline) and, for relief, S3-10; T6 needs the animation chain S3-7, S3-8 and S3-9. Until T4 the view uses preview shading (4.1). Characters are shown in their rest pose until T6. Stage 3's own S3-6 (quality gate) and S3-14 (drop-to-convert, with its before/after slider) share the split view. S3-14 came first and put it in `evani-render` (`model::Split`, `hooks` feature), not in `apps/model-view`: drop-to-convert runs in the four model games through `evani-live`'s `models` feature, and model-view is deliberately not wired to it (it has its own `view.*` commands). The model view and S3-6 reuse `Split`.

### MT-a. Evani.Model writes what the tab reads (S, sonnet-high)
- What: `model_compile` writes the next version folder by the rules of 8.3 (temporary folder, exclusive rename, `version.json` last) with `intent.evmodel`, `request`, `summary`, `said`, `by` and `changed_parts`; the status file of 7.3 written as each tool starts and ends; the `model_status` tool; triangles ordered by part and `tri_ranges` in `<name>.evmodel.json` (10.2); model ids starting with `_` refused (the `_requests` folder relies on it); DECISIONS.md line for the sort. The sort changes the bytes of the M1-approved compiler's output once, so the stored hashes of the kart and the Marshal (tests and reports) are updated in the same commit, with old and new hashes listed in the report for Chris.
- Done when: tests `next_version_folder_is_exclusive` (two writers at once get two numbers, no half folder is ever visible), `ranges_cover_every_triangle_once` (every tier of the kart and the Marshal), `colours_and_weights_unchanged_by_grouping` (the `COLOR_0`, `JOINTS_0` and `WEIGHTS_0` accessors' bytes identical before and after the change), `grouped_files_identical_at_1_and_8_threads`, `underscore_model_id_is_refused`, `next_version_folder_is_exclusive` also checks that both racing writers keep `parent` = the version they started from, `status_follows_the_tools` (make, compile, failure); Khronos validator clean on every tier; `cargo xtask check` clean on the branch.

T1 is two parts, built one after the other. The tab itself comes in A1, so both are proven with the page, the bridge and a scripted `/ctl` client standing in for the tab.

### T1a. First light: the page, the bridge, tiers and hot swap (M, opus-high)
- What: `apps/model-view` (4.1: decode with evani-gltf, interim drawing, turntable on ticks, orbit and zoom, double-click to frame, ground grid and shadow, address state 4.4, fallback panel when opened without the tab); publish refuses `model-view`; the bridge's model mode (4.2, 4.3, 8.3 watcher, `model.list`, `model.history`); `view.watch`, `view.load`, `view.tier`, `view.camera`, `view.state`; events `view_loaded`, `view_stats`, `view_error`, `model_version`, `model_status`; hot swap (section 6); the launcher shim `model.mjs`; `cargo xtask live --models`.
- Done when: tests `game_bridge_unchanged_without_models_flag`, `model_routes_serve_only_listed_files` (traversal and unlisted files are 404), `new_version_folder_sends_one_event`, `request_status_sends_one_event` (including a request with no model yet), `hot_swap_keeps_camera_and_frame_counter`, `tier_switch_counts_match_the_metadata`, `turntable_angle_follows_ticks`, `model_view_is_never_published`; captures of the kart and the Marshal on all four tiers at one fixed camera, plus one before and after a hot swap; ship SHA-1 of the four games unchanged.
- **T1a built (merge b66b7e5).** Start the page with `cargo xtask live --models` (port 8097, workspace `.evani/model`; `--port` and `--models <folder>` to change them) or the tab's launcher `.vibedeck/dev-harness/model.mjs --port 8097 --quiet`, then open `http://localhost:8097/?live=1`. Until MT-a lands, put a compiled model into the workspace with `cargo xtask model-import <model> <dir>` (each run adds the next version). Captures: `.vibedeck/screenshots/evani-model-tab/` (kept for a day).

### T1b. Part hover, the AI's doors and the docs (M, opus-high)
- What: CPU picking with the ranges (10.2), the part label, `view.highlight`, the `view_hover` event; `--session` for the bridge's `mcp` mode and the `evani-model-view` MCP entry (5.6); publish preflight refuses `.evmodel.json`; LIVE_API.md section 17 pointing here; the AI guidance lines for `[Evani.Model]` requests (7.2).
- Done when: tests `hover_names_the_part` (synthetic CDP pointer events at fixed pixels over a fixture model hit the expected four parts, and the empty background gives `null`), `hover_without_ranges_names_the_solid`, `highlight_tints_only_those_parts` (capture), `mcp_session_flag_reaches_the_model_bridge`, `package_with_evmodel_json_is_refused`; `cargo xtask init-ai` output carries the `[Evani.Model]` procedure.

### A1. The tab, first version (M, Evani.App)
- The brief in section 14. Done when its acceptance (14.6, first version) passes in a hidden test copy of Evani.App against the real model bridge, then Chris uses it in the installed app.

### T2. History, undo, Before/After (M, opus-medium)
- What: `model.restore` and `model.undo` (8.4); `view.before_after` with `after`, `before`, `slider` (the frame drawn twice with a scissor at the divider, same camera) and `ghost` (the earlier version as a see-through outline in one fixed colour over the new one); highlight of a version's `changed_parts`; the `model_restore` MCP tool.
- Done when: tests `restore_copies_bytes_and_adds_a_version` (same `hash`, new number, `restored_from` set), `repeated_undo_walks_back` (from v8, three undos give the contents of v7, v6 and v5, by `hash`), `undo_after_go_back_returns` (go back to v3 from v8, then undo, gives v8's content), `undo_at_the_first_version_adds_nothing`, `before_after_same_camera` (both captures at the same camera, only the model differs), `slider_split_matches_the_divider` (pixel columns left of the divider equal the before capture, right of it the after capture), `ghost_draws_over_the_new_version`; captures of slider at 0, 50 and 100 percent and of the ghost.

### T3. Reference compare (M, opus-medium)
- What: `model.reference.set`, `model.compare`, the compare runner in the bridge (section 9), the `model_compare` event, `view.reference` (side by side and slider), the `verdict` field in the compare tool with its measure words.
- Done when: tests `verdict_from_numbers` (fixed numbers give the exact sentence, including the "within 5 percent" and kart-only cases), `same_inputs_same_verdict_bytes`, `one_compare_at_a_time`, `compare_is_cached_per_reference`, `changed_reference_is_hashed_again`, `reference_route_serves_only_the_set_file`; a run on the Marshal against its Meshy master shows the verdict and the sheet; the compare's sensitivity tests from plan-characters.md (identical inputs score 1.0; 10 percent taller or wider loses at least 0.05) still pass.

### T4. Drawn as in the game (S, opus-medium)
- What: the page draws through S3-3's model pipeline (quantised vertices, normal maps, the look's ramp and outlines), the "Preview shading" note goes, references are drawn textured.
- Done when: a capture of a fixture model in `apps/model-view` and in a game scene at the same camera, tier and look agree (the method of note 21's "checking the preview against the game": silhouette IoU at least 0.99 and the toon band regions agreeing); the T1a and T1b tests still pass.

### T5. Details overlays (M, sonnet-high)
- What: `view.overlay`: wireframe of the shown tier, flat colour by part, collision shapes from the `collision` node, anchors as labelled markers from `anchor:<name>` nodes, bounds with size in metres, normals; one "Show" menu in Details.
- Done when: a capture per overlay of the kart and the Marshal; `anchors_match_the_file` (every `anchor:` node drawn, none extra); overlays off by default and never in a capture unless asked.

### T6. Characters in motion (M, opus-medium)
- What: skeleton overlay, a clip picker and play, pause and scrub, drawn through S3-9's GPU skinning; joints named on hover.
- Done when: the S3-9 test `bind_pose_draws_like_the_static_mesh` holds in the viewer; a clip scrubbed to tick N gives the same capture on every run; captures of the Marshal's test poses (plan-characters.md MC2).

---

## 13. What is built where

| Built in Evani.Dev (this project) | Built in Evani.App |
|---|---|
| Evani.Model's version folders, status file, `model_status`, triangle ranges (MT-a, on the Evani.Model track) | The tab itself: model picker, tier buttons, Before/After, status line, text box, history, Details (A1 to A3, and small parts of T5 and T6) |
| `apps/model-view`, the page (T1 to T6) | Finding and starting the model bridge through `model.mjs`, showing the page, passing the host state (as the Evani.dev tab does) |
| The bridge's model mode: routes, watcher, `model.*` commands, compare runner (T1 to T3) | The `/ctl` connection with the token, sending `view.*` and `model.*`, reading events |
| The launcher shim `.vibedeck/dev-harness/model.mjs`, `cargo xtask live --models` (T1a), the MCP entry (T1b) | Sending typed words to the orchestrator in the 7.1 form |
| The compare tool's verdict (T3) | The reference file dialog (T3's "Compare with...") |
| LIVE_API.md section 17, the AI guidance for `[Evani.Model]` requests (T1b) | |

---

## 14. For Evani.App

A self-contained brief for Evani.App's own orchestrator. It assumes nothing about Evani.App's code beyond what the Evani.dev tab already does (LIVE_API.md sections 2, 3.5 and 12): it finds a launcher in the engine folder, starts it with Evani.App's Node, waits for `/live/status`, shows the page in a locked-down view on the same origin, passes the host state (`window.__evaniHost` and the `evani-host` event), and sends typed words to the orchestrator of the project whose folder is the engine folder.

### 14.1 What to build

A new tab named **Evani.Model**, next to the Evani.dev tab, for projects that have `<engine folder>/.vibedeck/dev-harness/model.mjs` (the engine folder is the same setting the Evani.dev tab uses). It shows a 3D model page from the engine and, around it, the controls in section 2.1: model picker, four tier buttons with triangle counts, Before/After with "Show both" (slider or ghost), a one-line AI status, a "Say what to change" box with Send, a history list with Undo and "go back to here", and a closed Details section. All text in plain words for a non-programmer.

### 14.2 Starting the page

1. Probe `GET http://localhost:8097/live/status` (2 s timeout). It is the right engine when the JSON has `"game": "model-view"` and a `"models"` object.
2. If nothing answers, run `<Evani.App's Node> <engine folder>/.vibedeck/dev-harness/model.mjs --port 8097 --quiet` with `cwd` = that `dev-harness` folder, `ELECTRON_RUN_AS_NODE=1`, below-normal priority, output to a log of Evani.App's own, exactly as for `live.mjs`. Wait up to 60 s for `/live/status` (the launcher answers within a second, with `phase: "building"` while it compiles), then load `http://localhost:8097/?live=1` in the view. On quit or project switch, end the launcher's whole process tree, as for the Evani.dev tab.
3. Pass the host state to the page exactly as the Evani.dev tab does, so a hidden tab is parked.
4. If a different engine answers on 8097 (`root` differs from the engine folder), say "Another engine is using this address" and do not connect.

### 14.3 The control connection

- Read the token from `<engine folder>/.evani/live/model/session.json` (`token` field). Open `ws://127.0.0.1:8097/ctl` with the header `Authorization: Bearer <token>` (no `Origin`). Reconnect with backoff 1 s doubling to 30 s, rereading the session file each time.
- Send `{"type":"cmd","v":1,"id":"<unique>","cmd":"view.watch","params":{}}` after each connect.
- Every message is one JSON object per text frame. Commands: `{"type":"cmd","v":1,"id","cmd","params"}`; answers: `{"type":"result","v":1,"id","ok","result"|"error"}`; events: `{"type":"event","v":1,"event","data"}`.

### 14.4 What the tab sends and receives

| Chris does | The tab sends |
|---|---|
| Opens the tab | `model.list`, then `model.history` and `view.load` (latest version, tier high) for the newest model |
| Picks a model | `model.history`, `view.load` with `keep_camera: false` |
| Presses a tier button | `view.tier` |
| Before / After / slider / ghost | `view.before_after` with `show` (and `split` while dragging the slider divider, at most 30 times a second) |
| Hovers a history row | `view.highlight` with that version's `changed_parts`; empty list on leave |
| Clicks a history row | `view.load` of that version (the tab then shows "Version N is ready" instead of following the latest) |
| Undo | `model.undo` (the bridge picks the step back, 8.4; pressing again keeps walking back) |
| "Go back to here" | `model.restore` of that version |
| "Compare with..." (A3) | a file dialog for a `.glb`, then `model.reference.set`; "Stop comparing": `model.reference.set` with `file: null` |
| Sends a request | the chat message of 7.1 to the orchestrator; status line "Reading your request" |

| The tab receives | It shows |
|---|---|
| `model_version` | a new history row (`summary`, and `said` when present); closes the matching request; if following the latest, `view.load` of it |
| `model_status` | the status line (table in 7.3), only when its `request` is one the tab sent this session and still has open; for a new model, the `model` it names becomes the shown model once its first version arrives |
| `model_compare` | the verdict line under the view, numbers and the sheet picture in Details |
| `view_stats` | counts in Details; the tier buttons' counts come from `version.json` `tiers` |
| `view_hover` | nothing extra (the page draws the label); MAY show the part name in the status area |
| `view_error` | "Couldn't show this version: <message>" in the status line |
| a `result` with `ok: false` | its `message` in the status line |
| `/live/status` `phase: "building"` / `"error"` | "Starting the model viewer..." / the error with Try again (as the Evani.dev tab) |

### 14.5 Rules

- The tab draws no 3D and loads no 3D library: the page does all of it.
- It never writes into the engine folder, and never deletes a version; Undo and "go back to here" only call `model.undo` and `model.restore`.
- The token is never shown, logged or put in a URL.
- Typed words go only to the orchestrator of the project whose folder is the engine folder, in the 7.1 form.

### 14.6 Acceptance

First version (A1), in a hidden test copy of Evani.App against the real model bridge (the Gate 1 method):
1. Opening a project with `model.mjs` shows the Evani.Model tab; it starts the bridge on its own and shows the newest model turning, with no Try again; also after a restart of Evani.App.
2. The tier buttons show the counts from `version.json`; pressing each one changes the view and the page's `view_stats` count matches.
3. A request typed in the box arrives at the orchestrator in the 7.1 form; the status line follows `model_status` through to "Done (vN)", the new version swaps in without a reload (the page's frame counter keeps rising) and the camera stays where it was.
4. Hovering the model with a synthetic pointer shows the page's part label.
5. Hiding the tab parks the page (`/live/status` `page.state` reports it), showing it again resumes.

History version (A2): a new version adds a history row; Undo and "go back to here" add a "Went back to version K." row and the view shows that version; Before/After, slider and ghost change the view with the same camera.

Compare version (A3): picking a reference shows the verdict line within the time the compare takes (shown as "Comparing..."), and a new version updates it.

Each version: then Chris uses it in the installed Evani.App and says whether it is right.

---

## 15. Open questions

1. Model picker for many models: a plain list is enough for now; a search box when there are more than about 20.
2. Should the AI also see the hovered part with each request ("Pointing at: hat brim")? It is cheap (the tab knows it), but it is close to "click a part, then type", which the report leaves for later. Proposed: leave it out of v1, add it with that feature.
3. Version clean-up: versions are kept forever in v1. When the workspace grows large (a character's four tiers are about 3 MB a version), a "clean up old versions" command that keeps every version Chris named, saved or compared would be asked of Chris first.
