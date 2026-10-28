# Live code swapping: what the Evani.dev tab should show

For Evani.App (the Evani.dev tab). Engine side: `docs/LIVE_API.md` section 10.4; the tab's
channel: section 3.4. Nothing here needs a change to the protocol (`evani-live/1`); every
message below is already sent today, in shapes the tab already parses.

## What happens

While a game runs in the tab, saving a Rust file anywhere in the engine folder (the game, a kit,
a look, an engine crate) compiles a small patch and swaps it into the running game in a few
seconds (measured 3 to 15 s). No reload: the race, the camera and every live value stay as they
were. It is on by default: `live.mjs` starts the bridge, and the bridge builds with the pinned
`dx` when it is installed, or with Trunk as before when it isn't (then nothing below happens).

## Messages on `/evani-dev` (already sent)

| Message | Meaning |
|---|---|
| `{"type":"live_status","state":"applied","kind":"code","message":"Code updated: 397 functions, 3.1 s, no reload"}` | A saved change is now running in the game. |
| `{"type":"live_status","state":"error","kind":"code","message":"Code change didn't build: error[E0609]: no field `lapz` on type `&PlayerHud` (apps/kart-racer/src/ui.rs:199)"}` | A saved change doesn't compile. The game keeps running the code it had. |
| `{"type":"live_swap_failed","build":0,"kind":"code","message":"needs a fresh start","reason":"…"}` | The change was too big to swap (the page reloads by itself after a full rebuild), or a patch couldn't be loaded, or the game stopped in changed code and reloaded with the last code that worked. `reason` says which, in plain words. |

"N functions" is the size of the swap (every function in the recompiled code), not the number
the person edited; show it as is or leave it out, but don't call it "functions you changed".

## What the tab should show

1. **Swapped.** On `applied` with `kind:"code"`: a short note in the toolbar, e.g.
   "Code updated, no reload (3.1 s)", fading after a few seconds like other live notes. Never a
   reload prompt and never the hidden build check: the page didn't change.
2. **Didn't compile.** On `error` with `kind:"code"`: keep one line in the toolbar until the next
   `applied`/`kind:"code"` clears it, e.g. "Your last code change doesn't compile yet. The game
   is still running the code before it." with the message under a closed "Details". Offer the
   same "Send to AI to fix" the tab offers for a failed build, passing the message (it carries
   rustc's error code and the file and line). Don't mark the game as broken and don't offer
   Reload: nothing stopped.
3. **Needs a fresh start.** On `live_swap_failed` with `kind:"code"`: as for page edits today
   ("needs a fresh start" with the reason). For a too-big change the page is already reloading
   itself; the tab need not do anything else.
4. **This session's history.** A small list (newest first, the last 20) of code swaps: time, the
   one-line message, and its state (swapped, didn't compile, needed a fresh start). Either keep
   it from the messages above, or read `GET /live/status` → `code.history` (the same entries,
   kept by the bridge, so a tab opened later still sees them):

   ```json
   "code": { "mode": "hot", "builder": "dx", "state": "ready", "seq": 5,
             "history": [ { "state": "patched", "at": 1792007453492, "seq": 1, "functions": 346,
                            "ms": 14508, "message": "Code updated: 346 functions, 14.5 s, no reload" },
                          { "state": "error", "at": …, "message": "Code change didn't build: …" } ] }
   ```

   `state` is one of `patched`, `error`, `failed`, `crashed`, `reload`.
5. **Which mode.** Somewhere quiet (settings, or a tooltip on the status dot): "Code changes
   swap in live" when `/live/status` `code.mode` is `hot`; "Code changes rebuild the game
   (live swapping isn't installed)" when it is `trunk`, with `code.reason`; nothing when the
   field is missing (an older engine).

## Optional, later

- A "Swapping code…" spinner between the save and the swap. The bridge sends `code.patching` on
  `/ctl` today; if the tab wants it on `/evani-dev` too, the bridge can add
  `live_status` `state:"patching"` `kind:"code"` (additive). Say if you want it.
- A switch to turn live code swapping off for a project: launch `live.mjs` with
  `EVANI_LIVE_HOT=0` in the environment (the bridge then builds with Trunk).

## Stage 1 tab asks (2026-10-15)

Checked against the real live server with a hidden test copy of Evani.App (evidence:
`.vibedeck/screenshots/gate1/tab-connect.md`). Both Gate 1 checks it covers already pass with the
tab code in Evani.App 0.2.3: the tab connects by itself with no Try again (also after the live
server restarts), and a Live API mismatch shows "Evani.App and the engine don't match" with
"Update Evani.App." or "Update the engine.". Nothing in the protocol needs to change.

What Evani.App should still change:

1. **Needed for gate evidence: a read-only way for agents to see the Evani.dev tab.** Evani.App
   refuses its own window to agents' desktop tools, pictures and reading included, so no agent can
   show the installed tab connected or showing the update message. Either allow the read-only
   desktop calls (`read`, `look`) on Evani.App's own window while still refusing every action, or
   add an agent tool, e.g. `evani_dev_tab_status`, that returns the tab's state (the window's
   `DevView`: state, error, fix, errorKind, liveApi, engine) and, on request, a picture of the
   tab's panel with the game view drawn into it (what `evaniDev:screenshot` already captures, plus
   the panel around it).
2. **Nice to have: a button for "Update the engine."** When the engine is older, the fix line has no
   action. Offer the same "Send to AI to fix" the tab offers for a failed build, sent to the engine's
   project, e.g. "The Evani.dev tab speaks Live API v1 but this engine speaks v0. Please update the
   engine's live server to evani-live/1."
3. **Needed for gate evidence: a read-only record of shown kit review cards.** (2026-10-15) Evani.App
   judged the refreshed sandbox card (its `kit-reviews.json` holds that card's exact SHA-1 with
   `:j2`, and its own `kitReviewGate` returns show, nothing failing), but whether the heads-up card
   was actually drawn can't be shown: the window is off limits to the desktop tools, and the
   `history` of shown or sent-back cards in `src/main/kitReview.ts` is kept in memory only (test
   copies only). Either append each entry (time, app, card hash, shown or sent back, failing items)
   to a log file under Evani.App's logs folder, which agents may read, or cover the kit review card
   in the read-only status tool from item 1, with a picture of the card on request.

## Stage 1 tab asks: look guard approvals

For Evani.App. Engine side: `docs/LIVE_API.md` section 15. The engine refuses every real bake on a
look until Chris has approved that look's limits, and refuses a bake the look check flags until
Chris approves that bake; only Evani.App can approve. Proven end to end with a harness playing
Evani.App (`node tools/live-check/accept.mjs talk`, evidence in
`.vibedeck/screenshots/gate1/talk-to-ai.md`); Evani.App has none of this yet, so from the tab today
every real bake answers `look_unapproved` ("Restart the engine from Evani.App to approve").

1. **A signing key, pinned.** Make one Ed25519 key pair per install; keep the private key in the
   key vault. Write the public key as 64 hex chars to
   `%APPDATA%\Evani.App\look-approvals\pubkey` (15.6). Never use the TEST key (15.10).
2. **The key line on start.** When the tab starts `live.mjs`, write one line to its stdin (it
   passes stdin through to the bridge): `{"approve_secret":"<32 random bytes, hex, new each
   start>","approvals_pubkey":"<the pinned key>","project_id":"<this project's id>"}` (15.6).
   Keep stdin open or close it after the line; either works. Keep the secret in memory only.
3. **The approval card.** Listen on `/evani-dev` for `look_guard.approval_requested` (15.7) and
   show a card in plain words: for `kind:"limits"` "The AI wants to set this look's limits" with
   the `note` and the old-vs-new `rows` (flag any marked `looser`); for `kind:"bake"` "The look
   check flagged this bake" with the `rows` sentences. Buttons Approve and Not now; say when it
   expires (`expiry`, 5 minutes). Close the card on `look_guard.approved` or when it expires.
4. **On Approve.** For `limits`: write the signed record `<project id>\<look>.json` (15.5) with
   the next `seq`. Then post the signed approval with the HMAC header to
   `127.0.0.1:<approvePort>/look-guard/v1/approve` (15.7), from the app itself (no `Origin`
   header, so not from a web view). Show the answer: "Approved" or the refusal in plain words.
5. **Seq endpoint (optional, recommended).** While Evani.App runs, write
   `look-approvals\endpoint.json` `{"port":N}` and answer
   `GET /look-approvals/v1/seq?project=&look=` with the latest seq, so an older record put back
   by hand is caught (15.6).
6. **Status.** Show `/live/status` `look_guard.approvals` quietly (e.g. in the live status
   tooltip): "Look approvals on" when `enabled`, else its `reason`.

The "Talk to the AI" box itself needs nothing new: the AI already gets `look_status`,
`look_check`, `look_baseline_propose` and `look_limits_propose`, and a refused bake tells it in
plain words to wait for Chris.
