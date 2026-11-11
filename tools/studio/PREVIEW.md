# Asset & Style Studio: live preview

A page that always shows the latest `studio` branch with the whole library in it, so the
owner can watch the studio grow without waiting for the end.

- **Link:** http://localhost:8123/ (one fixed port)
- **Refresh after every merge into `studio`** (run it from any studio-family worktree):

  ```
  node tools/studio/preview-refresh.mjs          # --force rebuilds the same commit
  ```

## What the refresh does

1. Reads the `studio` branch head and checks it out in a **private build worktree in a scratch folder**
   (`<root>\src`). It never builds in the shared studio worktree.
2. Builds the material-view app: `cargo xtask build material-view --no-diff` with
   `CARGO_BUILD_JOBS=1`, `CARGO_TARGET_DIR`, `TEMP` and `TMP` in the preview root, through
   `run-low.mjs` (below-normal priority). The first build is long (the dependencies compile
   once); later ones only rebuild what changed. Log: `<root>\build.log`.
3. **Only if the build worked**, copies the page to `sites\<sha>-<time>`, adds the library
   badge and the "what's new" bell, and points `current.txt` at it. If the build fails the
   last good preview stays up and the script prints why (exit code 1,
   `<root>\status.json`).
4. Makes sure the static server (`preview-server.mjs`) is running on port 8123, starting it
   detached if not; its PID is in `<root>\server.pid`.

Only one refresh runs at a time (a second one says so and exits with code 2). If `studio`
has not moved since the last good build it says "already built" and exits quickly.

## What the page shows

The material-view app (the viewer the studio grows inside), plus two small additions in the
top-left corner that are **not** part of the engine or the studio:

- **Studio preview badge** (click it): the whole library of this build. Assets and
  Materials tabs, All / Built / Planned filters, search. Built items are marked "built",
  anything not built yet is greyed and marked "planned · not built yet" (an asset being
  made says "being built"). Nothing is ever drawn as a stand-in shape: this is only a list.
- **Bell with a count:** `whats-new.json`, newest first, up to 200 entries, written by the
  refresh. An entry is something that became *built* (a material or asset with status
  `ready`/`reviewed`, a style whose status is no longer `draft`, a light preset) or, when a
  merge adds no catalogue item but changes code, a `feature` entry with the merge title.
  Planned items do not count until they are built. The count is how many entries landed
  since the bell was last opened (remembered in the browser's localStorage). Clicking an
  entry opens the library list at that item, and for a material also opens it in the
  viewer's own library list when that list is on screen. The page checks for news about once
  a minute and shows a "new version, click to reload" hint when the build changed.

For the studio page (WP6): clicking an entry fires `window` event `studio-preview-select`
with `detail = {kind, id}`; listen to it to select that asset, style or material.

## Stop the server

```
node -e "process.kill(Number(require('fs').readFileSync('<root>/server.pid','utf8')))"
```

or `Stop-Process -Id (Get-Content <root>\server.pid)` in PowerShell. Stop it
by that PID only, never by program name. The next refresh starts it again. `GET /__health`
answers `{ok, pid, port, site}`.

## Files (all under the preview root, `<temp>/evani-studio-preview` unless `STUDIO_PREVIEW_ROOT` is set)

| File | What |
|---|---|
| `src\` | the private build worktree (detached at the studio head) |
| `target\`, `tmp\` | the cargo target folder and temp folder |
| `sites\<sha>-<time>\` | the last three good builds; `current.txt` names the served one |
| `whats-new.json`, `last-good.json` | the cumulative news list and the last good commit |
| `status.json`, `build.log`, `server.log`, `server.pid`, `refresh.lock` | state |

Settings (environment): `STUDIO_PREVIEW_ROOT`, `STUDIO_PREVIEW_PORT`, `EVANI_RUN_LOW`.
