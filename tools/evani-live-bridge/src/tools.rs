//! The MCP tool table (docs/LIVE_API.md 11): one tool per command, with the
//! descriptions the AI sees and JSON Schemas for their inputs.

use serde_json::{Map, Value, json};

/// How a tool's arguments become a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Reads: arguments are the command's params.
    Read,
    /// Changes: `note` (required or optional), `telemetry_ms` and
    /// `capture_width` are lifted out of the arguments into the envelope.
    Change { note_required: bool },
    /// `telemetry_sample`: subscribe, wait, unsubscribe.
    Sample,
    /// Runs in the MCP server itself, no game needed (`evani_check`).
    Local,
    /// `live_sessions`: the session files in this folder (LIVE_API.md 2.6).
    Sessions,
}

/// The optional `session` argument every tool that talks to a game takes.
pub const SESSION_PROP: &str = "session";

pub struct ToolSpec {
    pub name: &'static str,
    pub cmd: &'static str,
    pub kind: Kind,
    pub desc: &'static str,
    /// JSON object of the tool-specific input properties.
    props: &'static str,
    required: &'static [&'static str],
}

const CHANGE: Kind = Kind::Change { note_required: true };
const UNDO: Kind = Kind::Change { note_required: false };

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "live_status",
        cmd: "session.status",
        kind: Kind::Read,
        desc: "Is the game connected; active tier; history head; whether a change is being watched for auto-revert. Call first.",
        props: "{}",
        required: &[],
    },
    ToolSpec {
        name: "live_sessions",
        cmd: "local.sessions",
        kind: Kind::Sessions,
        desc: "The live sessions running in this project folder (several games can be live at once, each on its own port): port, game, kind, started, whether it answers, and which one tools use by default (the most recent, or EVANI_LIVE_SESSION). Pass a port or game name as `session` on any other tool to drive that one.",
        props: "{}",
        required: &[],
    },
    ToolSpec {
        name: "schema_get",
        cmd: "schema.get",
        kind: Kind::Read,
        desc: "List every tunable parameter, shader slot, curve, effect, tier and the adaptive-quality policy, with types, ranges, units and descriptions.",
        props: r#"{"include":{"type":"array","items":{"type":"string","enum":["params","slots","curves","effects","tiers","policy"]},"description":"Sections to return (default all)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "param_get",
        cmd: "param.get",
        kind: Kind::Read,
        desc: "Read current parameter values (by names or prefix).",
        props: r#"{"names":{"type":"array","items":{"type":"string"},"description":"Exact parameter names, e.g. fx.bloom.threshold."},"prefix":{"type":"string","description":"All parameters starting with this, e.g. fx.bloom. Omit both for all."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "param_set",
        cmd: "param.set",
        kind: CHANGE,
        desc: "Set one or more parameters atomically on the next frame; out-of-range values are clamped. Needs a note saying why. Returns old/new values and the next second of frame timing. Put every value of a change in one call (several names in values, or batch); to choose a value, use sweep_run first instead of one param_set per try.",
        props: r#"{"values":{"type":"object","description":"Parameter name to new value, e.g. {\"fx.bloom.intensity\": 0.9}.","additionalProperties":true}}"#,
        required: &["values"],
    },
    ToolSpec {
        name: "slot_get",
        cmd: "slot.get",
        kind: Kind::Read,
        desc: "Read a shader slot's WGSL body (or an expression slot's formula) and its fixed signature.",
        props: r#"{"name":{"type":"string","description":"Slot name, e.g. tonemap."},"with_prelude":{"type":"boolean","description":"Also return the fixed signature, uniforms and helpers around the body."}}"#,
        required: &["name"],
    },
    ToolSpec {
        name: "slot_set",
        cmd: "slot.set",
        kind: CHANGE,
        desc: "Replace a shader slot body (WGSL, body only) or an expression slot. Compiles first; on error returns line/column in your body and keeps the old shader running. Needs a note.",
        props: r#"{"name":{"type":"string","description":"Slot name, e.g. tonemap."},"source":{"type":"string","description":"The body only (between the braces), or one expression for expression slots."}}"#,
        required: &["name", "source"],
    },
    ToolSpec {
        name: "curve_get",
        cmd: "curve.get",
        kind: Kind::Read,
        desc: "Read a piecewise-linear curve's control points.",
        props: r#"{"name":{"type":"string","description":"Curve name, e.g. lod.distance."}}"#,
        required: &["name"],
    },
    ToolSpec {
        name: "curve_set",
        cmd: "curve.set",
        kind: CHANGE,
        desc: "Replace a curve's control points (x increasing). Needs a note.",
        props: r#"{"curves":{"type":"object","description":"Curve name to its points [[x, y], ...], x strictly increasing.","additionalProperties":{"type":"array","items":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}}}}"#,
        required: &["curves"],
    },
    ToolSpec {
        name: "policy_get",
        cmd: "policy.get",
        kind: Kind::Read,
        desc: "Read the adaptive-quality policy (frame target, window, hysteresis, degrade order, render-scale step and floor, effect weights).",
        props: "{}",
        required: &[],
    },
    ToolSpec {
        name: "policy_set",
        cmd: "policy.set",
        kind: CHANGE,
        desc: "Patch the adaptive-quality policy. Needs a note.",
        props: r#"{"set":{"type":"object","description":"Fields to change, e.g. {\"hysteresis_pct\": 15, \"weights\": {\"vignette\": 0.1}}.","additionalProperties":true}}"#,
        required: &["set"],
    },
    ToolSpec {
        name: "effect_toggle",
        cmd: "effect.toggle",
        kind: CHANGE,
        desc: "Turn an effect on or off now (scope session) or in the active tier's preset (scope tier). Reports what the budget switched. Needs a note.",
        props: r#"{"effect":{"type":"string","description":"Effect name, e.g. bloom."},"on":{"type":"boolean"},"scope":{"type":"string","enum":["session","tier"],"description":"Default session."},"override_budget":{"type":"boolean","description":"Turn it on even if it doesn't fit the tier's budget."}}"#,
        required: &["effect", "on"],
    },
    ToolSpec {
        name: "tier_set",
        cmd: "tier.set",
        kind: CHANGE,
        desc: "Switch quality tier live (lite, low, medium, high, ultra, vr). Needs a note.",
        props: r#"{"tier":{"type":"string","description":"lite, low, medium, high, ultra or vr."}}"#,
        required: &["tier"],
    },
    ToolSpec {
        name: "telemetry_sample",
        cmd: "telemetry.subscribe",
        kind: Kind::Sample,
        desc: "Measure for N seconds (default 2) and return averaged frame time, per-pass GPU ms, draw calls, triangles/sprites and budget points.",
        props: r#"{"seconds":{"type":"number","minimum":0.5,"maximum":30,"description":"How long to measure (default 2)."},"rate_hz":{"type":"number","minimum":0.5,"maximum":60,"description":"Samples per second (default 4)."},"fields":{"type":"array","items":{"type":"string"},"description":"Only these fields (default all)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "capture_frame",
        cmd: "capture.frame",
        kind: Kind::Read,
        desc: "Take a downscaled screenshot with stats, optionally at a fixed simulation time on the deterministic camera path or from a camera you place. For geometry checks: sky paints the sky (magenta by default) so holes show, jitter_px shifts the view a sub-pixel (render twice to find flicker), geometry shows the colour-coded geometry debug view in the shot.",
        props: r#"{"width":{"type":"integer","minimum":64,"maximum":4096,"description":"Default 480; height keeps the aspect ratio; never wider than the game's canvas."},"sim_time":{"type":"number","description":"Seconds: pause, set the fixed simulation time, capture, resume."},"camera":{"oneOf":[{"type":"string","enum":["current","path"]},{"type":"object","properties":{"eye":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"target":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"fov_y_deg":{"type":"number","minimum":5,"maximum":150}},"required":["eye","target"]}],"description":"\"current\" (default), \"path\" (the bench path at sim_time), or {eye, target, fov_y_deg} in metres (holds the clock at sim_time or now)."},"moment":{"type":"string","description":"A scripted moment the game registered (live_status lists them): set it up, hold the clock and capture through the game camera. Not with sim_time or camera."},"sky":{"oneOf":[{"type":"boolean"},{"type":"array","items":{"type":"number","minimum":0,"maximum":1},"minItems":3,"maxItems":3}],"description":"true paints the sky magenta for this shot, or give a linear RGB colour."},"jitter_px":{"type":"array","items":{"type":"number","minimum":-4,"maximum":4},"minItems":2,"maxItems":2,"description":"Shift the view by [x, y] pixels (sub-pixel allowed)."},"geometry":{"type":"boolean","description":"Show the geometry debug view (debug_geometry) in this shot."},"hide_overlay":{"type":"boolean"}}"#,
        required: &[],
    },
    ToolSpec {
        name: "focus_list",
        cmd: "focus.list",
        kind: Kind::Read,
        desc: "List the game's assets that can be shown on their own (id, label, kind, the parameters that tune each) and what is focused now.",
        props: "{}",
        required: &[],
    },
    ToolSpec {
        name: "focus_set",
        cmd: "focus.set",
        kind: Kind::Read,
        desc: "Show one asset on its own on a slow turntable camera (the game is paused, its animation keeps playing, its tunables come first in schema_get), or move the turntable. Not a change: no note. Check it with capture_frame; focus_clear goes back to the full game.",
        props: r#"{"asset":{"type":"string","description":"Asset id from focus_list, e.g. item_box. Omit to adjust the current focus."},"yaw_deg":{"type":"number","description":"Orbit angle from the asset's front."},"pitch_deg":{"type":"number","minimum":-89,"maximum":89,"description":"Camera elevation (default 18)."},"distance":{"type":["number","null"],"minimum":0.05,"maximum":10000,"description":"Metres from the asset; null fits it to the view (default)."},"spin_deg_s":{"type":"number","minimum":-360,"maximum":360,"description":"Turntable speed (default 12; 0 = still)."},"fov_deg":{"type":"number","minimum":5,"maximum":150,"description":"Vertical field of view (default 35)."},"paused":{"type":"boolean","description":"Freeze the game while focused (default true)."},"backdrop":{"type":"boolean","description":"Keep the static scenery behind the asset (default true); false shows it over the sky."},"instance":{"type":["integer","null"],"minimum":0,"description":"Which one when the asset exists several times (e.g. item box 3); null = the kit's choice."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "focus_clear",
        cmd: "focus.clear",
        kind: Kind::Read,
        desc: "Leave asset focus: back to the full game, which carries on where it paused.",
        props: "{}",
        required: &[],
    },
    ToolSpec {
        name: "debug_geometry",
        cmd: "debug.geometry",
        kind: Kind::Read,
        desc: "Geometry debug view: checks what the game draws for overlapping objects, floating objects and z-fighting faces (worked out from the renderer's depth maths as the live camera sees them: each z-fight issue's data gives the separation, the depth epsilon there, the view distance and the patch outline) and lists every issue (location, objects, size, suggested fix), plus a readable summary. With on:true the game paints overlaps red, floating objects yellow, z-fighting cyan, everything else grey, and the sky magenta so gaps and cracks show; check it with capture_frame. focus:n points the camera at issue n. Turn it off with on:false when done.",
        props: r#"{"on":{"type":"boolean","description":"Show the colour-coded view (true) or switch it off (false). Omit to just list issues."},"checks":{"type":"array","items":{"type":"string","enum":["overlap","floating","zfight"]},"description":"Which checks to run (default all three)."},"sky":{"type":"boolean","description":"Paint the sky magenta while the view is on (default true)."},"focus":{"type":["integer","null"],"minimum":0,"description":"Point the camera at this issue index; null gives the game camera back."},"every_s":{"type":"number","minimum":0.05,"maximum":30,"description":"Re-check at most this often while on (default 0.5 s)."},"tags":{"type":"array","items":{"type":"object","properties":{"check":{"type":"string"},"object":{"type":"string"},"at":{"type":"array","items":{"type":"number"}},"radius":{"type":"number"},"reason":{"type":"string"}},"required":["reason"]},"description":"Intentional cases to skip, each with a reason (as in geocheck.toml)."},"thresholds":{"type":"object","description":"Tolerance overrides, e.g. {\"sink\":0.2,\"float_gap\":0.05}."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "bench_run",
        cmd: "bench.run",
        kind: Kind::Read,
        desc: "Run the fixed camera path for N seconds at a tier; returns timing stats, pass/fail against the tier's limits and captures.",
        props: r#"{"seconds":{"type":"number","minimum":1,"maximum":120},"tier":{"type":"string"},"warmup_s":{"type":"number"},"capture_at":{"type":"array","items":{"type":"number"}},"capture_width":{"type":"integer","minimum":64,"maximum":1024}}"#,
        required: &[],
    },
    ToolSpec {
        name: "compare_ab",
        cmd: "compare.ab",
        kind: Kind::Read,
        desc: "Compare two states on the same camera path and sim time: A (default the current state) against B (a snapshot, or changes to try such as {\"values\":{\"fx.bloom.enabled\":false}}). Returns both screenshots, a per-pixel difference, frame and per-pass GPU timing for each and the delta. B is reverted afterwards unless keep is true (then it needs a note). Runs off-screen: the visible game holds its last frame until the run ends, then shows the result (B with keep, else the unchanged state).",
        props: r#"{"a":{"oneOf":[{"type":"string"},{"type":"object"}],"description":"Default \"current\". A snapshot id or name, \"current\", or changes: {\"values\":{...}} or {\"cmds\":[...]} (as in batch)."},"b":{"oneOf":[{"type":"string"},{"type":"object"}],"description":"A snapshot id or name, \"current\", or changes applied to the current state: {\"values\":{\"fx.bloom.enabled\":false}} or {\"cmds\":[...]} (as in batch)."},"seconds":{"type":"number","minimum":1,"maximum":120,"description":"Measured seconds per run (default 4)."},"warmup_s":{"type":"number","minimum":0,"maximum":60,"description":"Unmeasured seconds before each run (default 0.5)."},"tier":{"type":"string","description":"Run both sides at this tier (default the active one)."},"order":{"type":"string","enum":["ABBA","AB"],"description":"Default ABBA (cancels drift)."},"capture_at":{"type":"array","items":{"type":"number"},"maxItems":8,"description":"Path times to screenshot both sides (default the middle)."},"capture_width":{"type":"integer","minimum":64,"maximum":1024,"description":"Screenshot width (default 320)."},"keep":{"type":"boolean","description":"End on B instead of reverting it (one undoable step; needs a note)."},"note":{"type":"string","minLength":1,"maxLength":500,"description":"Why; required with keep."}}"#,
        required: &["b"],
    },
    ToolSpec {
        name: "sweep_run",
        cmd: "sweep.run",
        kind: Kind::Read,
        desc: "Try a list of values for one parameter (or a grid of a few) on the fixed camera path and get one compact table back: per row the values, median work/CPU/GPU ms and per-pass GPU ms, and the look-check measures (mean_luma, log_avg, luma_std, luma_range, saturation, colorfulness, clipped, crushed, bloom coverage). Row 0 is the current state (base_row). Nothing is kept: every value is restored and no history is written; apply the chosen values afterwards with one param_set or batch. The rows run off-screen: the visible game holds its last frame, so only the values you apply show. Thumbnails, if asked, are written to files and returned as paths.",
        props: r#"{"param":{"type":"string","description":"The parameter to sweep, e.g. fx.bloom.intensity (with values)."},"values":{"type":"array","maxItems":32,"description":"Values to try for param, e.g. [0.2,0.4,0.8]."},"grid":{"type":"object","additionalProperties":{"type":"array"},"description":"Instead of param/values: several parameters, every combination (at most 32 rows), e.g. {\"fx.bloom.intensity\":[0.3,0.6],\"fx.bloom.threshold\":[1,1.5]}."},"path":{"type":"string","enum":["bench"],"description":"Camera path (default the game's bench path)."},"frames_per_value":{"type":"integer","minimum":10,"maximum":3600,"description":"Measured frames per row (default 120)."},"warmup_s":{"type":"number","minimum":0,"maximum":10,"description":"Unmeasured seconds after each change (default 0.25)."},"tier":{"type":"string","description":"Run at this tier (default the active one)."},"look_at":{"type":"array","items":{"type":"number"},"maxItems":8,"description":"Path times (s) of the frames the look measures average over (default 3 spread over the run; [] for costs only)."},"look_width":{"type":"integer","minimum":64,"maximum":1024,"description":"Width of the measured frames (default 320)."},"bloom":{"type":"boolean","description":"Measure bloom coverage against bloom-off frames (default true when bloom is in the build)."},"baseline":{"type":"boolean","description":"Add the current state as row 0 (default true)."},"thumbs":{"oneOf":[{"type":"boolean"},{"type":"object","properties":{"w":{"type":"integer","minimum":32,"maximum":480},"h":{"type":"integer","minimum":32,"maximum":480},"inline":{"type":"boolean","description":"Return base64 instead of file paths."},"dir":{"type":"string","description":"Folder under the project root for the files (default .evani/live/sweeps/<run>)."}}}],"description":"Also keep one small picture per row (true = 160 wide)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "snapshot_save",
        cmd: "snapshot.save",
        kind: Kind::Read,
        desc: "Save the whole live state under a name to compare or return to.",
        props: r#"{"name":{"type":"string"},"note":{"type":"string","maxLength":500,"description":"Optional: why."}}"#,
        required: &["name"],
    },
    ToolSpec {
        name: "snapshot_list",
        cmd: "snapshot.list",
        kind: Kind::Read,
        desc: "List saved snapshots.",
        props: "{}",
        required: &[],
    },
    ToolSpec {
        name: "snapshot_restore",
        cmd: "snapshot.restore",
        kind: CHANGE,
        desc: "Return to a saved snapshot (undoable). Needs a note.",
        props: r#"{"snapshot":{"type":"string","description":"Snapshot id, e.g. snap-4."}}"#,
        required: &["snapshot"],
    },
    ToolSpec {
        name: "history_undo",
        cmd: "history.undo",
        kind: UNDO,
        desc: "Undo the last change(s).",
        props: r#"{"steps":{"type":"integer","minimum":1,"maximum":50,"description":"Default 1."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "history_redo",
        cmd: "history.redo",
        kind: UNDO,
        desc: "Redo undone change(s).",
        props: r#"{"steps":{"type":"integer","minimum":1,"maximum":50,"description":"Default 1."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "history_list",
        cmd: "history.list",
        kind: Kind::Read,
        desc: "List recent changes with their notes, source and whether they were undone or auto-reverted.",
        props: r#"{"limit":{"type":"integer","minimum":1,"description":"Default 20."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "batch",
        cmd: "batch",
        kind: CHANGE,
        desc: "Apply several changes as one atomic, undoable step with one note. The way to apply a set chosen with sweep_run: one batch of param.set (and slot/curve/tier) commands instead of one call per change.",
        props: r#"{"cmds":{"type":"array","description":"Mutating commands, e.g. [{\"cmd\":\"param.set\",\"params\":{\"values\":{\"fx.bloom.intensity\":1.1}}}].","items":{"type":"object","properties":{"cmd":{"type":"string"},"params":{"type":"object"}},"required":["cmd"]}}}"#,
        required: &["cmds"],
    },
    ToolSpec {
        name: "edit_begin",
        cmd: "edit.begin",
        kind: Kind::Read,
        desc: "Start an edit transaction for a request that asks for several live changes at once (\"turn the lights down, make the sky blue, reverse the spin\"). Split the request into independent atomic edits, list them here, then run one sub-agent per edit in parallel (no cap: 3, 6 or 20), each calling edit_stage for its own edit, and call edit_commit when all are done. mode stream (default unless live.edit.mode says otherwise) shows each edit as soon as it is staged; together shows them all on one frame at commit. Returns the transaction id and the plan.",
        props: r#"{"request":{"type":"string","maxLength":500,"description":"The user's request, in their words."},"edits":{"type":"array","maxItems":64,"description":"The atomic edits you split it into: strings (\"lights down\") or {id, what}; ids default to e1, e2, ...","items":{"oneOf":[{"type":"string"},{"type":"object","properties":{"id":{"type":"string"},"what":{"type":"string"}}}]}},"mode":{"type":"string","enum":["stream","together"],"description":"Default: the live.edit.mode parameter (stream unless changed)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "edit_stage",
        cmd: "edit.stage",
        kind: CHANGE,
        desc: "Stage one atomic edit of an edit transaction: one mutating command (param.set, slot.set, curve.set, effect.toggle, tier.set, policy.set, snapshot.restore or batch) with its own note. Call it from each parallel sub-agent; edits never wait on each other. Answers state applied (stream), staged (together), failed (this edit only, with the reason; out-of-range values fail with out_of_range unless clamp is true) or conflict (another edit changes the same thing: neither stays applied; ask the user which wins, then edit_resolve).",
        props: r#"{"txn":{"type":"string","description":"Transaction id from edit_begin, e.g. edit-1."},"edit":{"type":"string","description":"The edit's id from the plan, e.g. e2 (default a new id)."},"cmd":{"type":"string","enum":["param.set","slot.set","curve.set","policy.set","effect.toggle","tier.set","snapshot.restore","batch"],"description":"The command this edit runs."},"params":{"type":"object","description":"That command's params, e.g. {\"values\":{\"render.light.intensity\":0.6}}."},"clamp":{"type":"boolean","description":"Clamp out-of-range values instead of failing (default false)."}}"#,
        required: &["txn", "cmd", "params"],
    },
    ToolSpec {
        name: "edit_commit",
        cmd: "edit.commit",
        kind: UNDO,
        desc: "Finish an edit transaction once every sub-agent has staged its edit. together: applies every staged edit on one frame (each its own undoable history entry; the view holds while shaders compile). stream: closes it. Returns every edit's result: applied, failed (with why) or conflict (ask the user, then edit_resolve).",
        props: r#"{"txn":{"type":"string","description":"Transaction id, e.g. edit-1."}}"#,
        required: &["txn"],
    },
    ToolSpec {
        name: "edit_resolve",
        cmd: "edit.resolve",
        kind: UNDO,
        desc: "Settle a conflict after asking the user: keep names the edit that wins (it is applied, the others in its conflict are dropped), or drop drops that edit's whole conflict. Never pick a winner without the user.",
        props: r#"{"txn":{"type":"string","description":"Transaction id, e.g. edit-1."},"keep":{"type":"string","description":"The winning edit's id."},"drop":{"type":"string","description":"An edit whose whole conflict is dropped."}}"#,
        required: &["txn"],
    },
    ToolSpec {
        name: "edit_status",
        cmd: "edit.status",
        kind: Kind::Read,
        desc: "One edit transaction with every edit's state (or, without txn, the recent transactions and the default mode).",
        props: r#"{"txn":{"type":"string","description":"Transaction id; omit for the list."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "edit_cancel",
        cmd: "edit.cancel",
        kind: Kind::Read,
        desc: "Cancel an edit transaction: staged and conflicting edits are dropped; edits already applied stay (undo them with history_undo).",
        props: r#"{"txn":{"type":"string","description":"Transaction id, e.g. edit-1."}}"#,
        required: &["txn"],
    },
    ToolSpec {
        name: "bake_export",
        cmd: "bake.export",
        kind: Kind::Read,
        desc: "Write the tuned session into the game's source files (tier presets, look parameters, slot .wgsl files, curves) plus a CHANGES.md built from the notes. Use dry_run first to see the plan. A real bake first runs the look check (look_check): it is refused with look_unapproved if the look has no limits Chris approved, or look_flagged if the captures are outside the look's limits; then Chris must approve that bake in Evani.App and you run the same bake again within 5 minutes. You can't approve it yourself.",
        props: r#"{"dry_run":{"type":"boolean"},"include":{"type":"array","items":{"type":"string","enum":["tiers","params","slots","curves","policy"]}},"title":{"type":"string"}}"#,
        required: &[],
    },
    ToolSpec {
        name: "look_status",
        cmd: "look.status",
        kind: Kind::Read,
        desc: "The look guard of a look: whether its limits are approved by Chris (verified, unguarded or failed, with why), its capture moments, pending proposals, the last bake's manifest and whether approvals can be answered in this session.",
        props: r#"{"look":{"type":"string","description":"Look crate, e.g. evani-look-basic (default the game's look)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "look_check",
        cmd: "look.check",
        kind: Kind::Read,
        desc: "Capture the look's moments (with and without bloom) and compare them with its approved limits: clipped highlights, crushed shadows, contrast, saturation, brightness, bloom coverage. Returns passed, and one plain sentence per stat outside its limits. Changes nothing.",
        props: r#"{"look":{"type":"string","description":"Look crate (default the game's look)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "look_limits_propose",
        cmd: "look.limits.propose",
        kind: Kind::Read,
        desc: "Propose new look-check limits (tolerances and/or band widening). Only a proposal: nothing changes until Chris approves it in Evani.App, and it shows him an old-vs-new row per stat, marked looser or tighter. Needs a note saying why.",
        props: r#"{"look":{"type":"string","description":"Look crate (default the game's look)."},"tolerances":{"type":"object","description":"Stat to {mode: abs|rel|up, value, min?}, e.g. {\"mean_luma\":{\"mode\":\"abs\",\"value\":0.05}}."},"band_widen":{"type":"object","description":"{fraction?, floors?: {stat: number}}."},"note":{"type":"string","minLength":1,"maxLength":500,"description":"Why (Chris sees it)."}}"#,
        required: &["note"],
    },
    ToolSpec {
        name: "look_baseline_propose",
        cmd: "look.baseline.propose",
        kind: Kind::Read,
        desc: "Capture new baseline images of the look's moments and propose them (with limits generated from them) as the look's baseline. Only a proposal: nothing changes until Chris approves it in Evani.App. Give moments the first time (tier, width, and named moments with sim_time, camera or moment). Needs a note saying why.",
        props: r#"{"look":{"type":"string","description":"Look crate (default the game's look)."},"moments":{"type":"object","description":"Like moments.json: {\"tier\":\"medium\",\"width\":320,\"moments\":[{\"name\":\"start\",\"capture\":{\"sim_time\":2,\"camera\":\"path\"}}]} (default the approved moments)."},"note":{"type":"string","minLength":1,"maxLength":500,"description":"Why (Chris sees it)."}}"#,
        required: &["note"],
    },
    ToolSpec {
        name: "model_convert",
        cmd: "model.convert",
        kind: CHANGE,
        desc: "Convert a model file with evani-convert and its quality gate (one job at a time, below-normal priority): a .glb, a .gltf (converted where it is), a zipped .gltf or a .blend. Answers at once with {job, hash, id}; a convert_done event follows (minutes for a big model), then model_report has the sizes and scores. Busy while another conversion runs. Writes only under .evani/convert/<hash>/ (the note goes into its job record); older conversions there are removed (the newest 8 are kept, none past 7 days).",
        props: r#"{"path":{"type":"string","description":"The model file, absolute or relative to the engine root."},"id":{"type":"string","description":"Model id: lowercase letters, digits, - and _ (default from the file name)."}}"#,
        required: &["path"],
    },
    ToolSpec {
        name: "model_load",
        cmd: "model.load",
        kind: Kind::Read,
        desc: "Show a converted model on the turntable without reloading the game (the game is paused, the frame keeps running): the converted tier on its own, or with compare on, the original on the left and the converted tier on the right of a split. Answers once the model is drawn. Check it with capture_frame; focus_clear goes back to the game.",
        props: r#"{"hash":{"type":"string","description":"The conversion (default the latest)."},"tier":{"type":"string","enum":["low","medium","high","ultra"],"description":"Which converted tier (default the page's active tier, else the first)."},"compare":{"type":"boolean","description":"Split view with the original (default true)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "model_compare",
        cmd: "model.compare",
        kind: Kind::Read,
        desc: "Move the before/after line of the loaded model (the original is on the left of the line, the converted tier on the right), turn the split on or off, and/or switch its converted tier. Not a change: no note. Check it with capture_frame.",
        props: r#"{"at":{"type":"number","minimum":0,"maximum":1,"description":"Where the line is across the frame: 0 = all converted, 1 = all original."},"split":{"type":"boolean","description":"true: the split view; false: the converted model whole."},"tier":{"type":"string","enum":["low","medium","high","ultra"]}}"#,
        required: &[],
    },
    ToolSpec {
        name: "model_report",
        cmd: "model.report",
        kind: Kind::Read,
        desc: "A finished conversion's record and report: per tier its file, bytes (raw and gzip), draw calls, triangles and gate scores (SSIMULACRA2, worst pixel, worst view, sheet); the package total; notes; the files and their URLs. Default the latest conversion. Needs no game.",
        props: r#"{"hash":{"type":"string","description":"The conversion (default the latest)."}}"#,
        required: &[],
    },
    ToolSpec {
        name: "evani_check",
        cmd: "local.check",
        kind: Kind::Local,
        desc: "The Evani check gate (same as `cargo xtask check`): every game's module list against its crate, catalog fit (look, effects, physics), geocheck report, review card FAIL lines and look guard, each issue with a fix and a doc. No running game needed. Repeat until it says clean before you hand over; full also runs the tests and the ship build (slow).",
        props: r#"{"app":{"type":"string","description":"Only this game (apps/<id>); default all."},"full":{"type":"boolean","description":"Also cargo test and the ship build (minutes)."}}"#,
        required: &[],
    },
];

pub fn find(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

impl ToolSpec {
    /// The tool's input JSON Schema.
    pub fn schema(&self) -> Map<String, Value> {
        let mut props: Map<String, Value> = serde_json::from_str(self.props).unwrap_or_default();
        let mut required: Vec<&str> = self.required.to_vec();
        if let Kind::Change { note_required } = self.kind {
            props.insert(
                "note".into(),
                json!({ "type": "string", "minLength": 1, "maxLength": 500,
                        "description": "Why you are making this change (kept in history and CHANGES.md)." }),
            );
            props.insert(
                "telemetry_ms".into(),
                json!({ "type": "integer", "minimum": 0, "maximum": 5000,
                        "description": "How long to measure after the change (default 1000; 0 = return at once)." }),
            );
            props.insert(
                "capture_width".into(),
                json!({ "type": "integer", "minimum": 64, "maximum": 1024,
                        "description": "Also return a screenshot this wide after the change." }),
            );
            if note_required {
                required.push("note");
            }
        }
        if !matches!(self.kind, Kind::Local | Kind::Sessions) {
            props.insert(
                SESSION_PROP.into(),
                json!({ "type": "string",
                        "description": "Which live session (see live_sessions): a port such as \"8098\", or a game name. Default: the most recent session." }),
            );
        }
        let mut s = Map::new();
        s.insert("type".into(), json!("object"));
        s.insert("properties".into(), Value::Object(props));
        if !required.is_empty() {
            s.insert("required".into(), json!(required));
        }
        s
    }

    /// The command envelope for these tool arguments (11: params plus `note`
    /// and the flattened `opts`).
    pub fn envelope(&self, id: &str, args: &Map<String, Value>) -> Value {
        let mut params = args.clone();
        params.remove(SESSION_PROP);
        let note = params.remove("note");
        let mut opts = Map::new();
        if matches!(self.kind, Kind::Change { .. }) {
            if let Some(t) = params.remove("telemetry_ms") {
                opts.insert("telemetry_ms".into(), t);
            }
            if let Some(w) = params.remove("capture_width") {
                opts.insert("capture".into(), json!({ "width": w }));
            }
        }
        let mut env = json!({ "type": "cmd", "v": 1, "id": id, "cmd": self.cmd, "params": params });
        if let Some(n) = note {
            env["note"] = n;
        }
        if !opts.is_empty() {
            env["opts"] = Value::Object(opts);
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command in LIVE_API.md section 11's tool mapping has its tool.
    #[test]
    fn mapping_is_complete() {
        let mapping = [
            ("live_status", "session.status"),
            ("live_sessions", "local.sessions"),
            ("schema_get", "schema.get"),
            ("param_get", "param.get"),
            ("param_set", "param.set"),
            ("slot_get", "slot.get"),
            ("slot_set", "slot.set"),
            ("curve_get", "curve.get"),
            ("curve_set", "curve.set"),
            ("policy_get", "policy.get"),
            ("policy_set", "policy.set"),
            ("effect_toggle", "effect.toggle"),
            ("tier_set", "tier.set"),
            ("telemetry_sample", "telemetry.subscribe"),
            ("capture_frame", "capture.frame"),
            ("focus_list", "focus.list"),
            ("focus_set", "focus.set"),
            ("focus_clear", "focus.clear"),
            ("debug_geometry", "debug.geometry"),
            ("bench_run", "bench.run"),
            ("compare_ab", "compare.ab"),
            ("sweep_run", "sweep.run"),
            ("snapshot_save", "snapshot.save"),
            ("snapshot_list", "snapshot.list"),
            ("snapshot_restore", "snapshot.restore"),
            ("history_undo", "history.undo"),
            ("history_redo", "history.redo"),
            ("history_list", "history.list"),
            ("batch", "batch"),
            ("edit_begin", "edit.begin"),
            ("edit_stage", "edit.stage"),
            ("edit_commit", "edit.commit"),
            ("edit_resolve", "edit.resolve"),
            ("edit_status", "edit.status"),
            ("edit_cancel", "edit.cancel"),
            ("bake_export", "bake.export"),
            ("look_status", "look.status"),
            ("look_check", "look.check"),
            ("look_limits_propose", "look.limits.propose"),
            ("look_baseline_propose", "look.baseline.propose"),
            ("model_convert", "model.convert"),
            ("model_load", "model.load"),
            ("model_compare", "model.compare"),
            ("model_report", "model.report"),
            ("evani_check", "local.check"),
        ];
        assert_eq!(TOOLS.len(), mapping.len());
        // The AI never gets an approve tool (15.7).
        assert!(TOOLS.iter().all(|t| !t.name.contains("approve") && !t.cmd.contains("approve")));
        for (tool, cmd) in mapping {
            assert_eq!(find(tool).map(|t| t.cmd), Some(cmd), "{tool}");
        }
        for t in TOOLS {
            let s = t.schema();
            assert_eq!(s["type"], "object", "{}", t.name);
            assert!(s["properties"].is_object(), "{} has bad props JSON", t.name);
            let mutating = crate::proto::MUTATING.contains(&t.cmd);
            assert_eq!(matches!(t.kind, Kind::Change { .. }), mutating, "{}", t.name);
        }
    }

    #[test]
    fn arguments_become_an_envelope() {
        let t = find("param_set").unwrap();
        let args = json!({"values":{"fx.bloom.threshold":1.5},"note":"why","telemetry_ms":0,"capture_width":320});
        let env = t.envelope("m-1", args.as_object().unwrap());
        assert_eq!(
            env,
            json!({"type":"cmd","v":1,"id":"m-1","cmd":"param.set","params":{"values":{"fx.bloom.threshold":1.5}},
                   "note":"why","opts":{"telemetry_ms":0,"capture":{"width":320}}})
        );
        assert_eq!(t.schema()["required"], json!(["values", "note"]));
        // bench_run keeps capture_width as its own param.
        let b = find("bench_run").unwrap();
        let env = b.envelope("m-2", json!({"seconds":5,"capture_width":320}).as_object().unwrap());
        assert_eq!(env["params"]["capture_width"], 320);
        assert!(env.get("opts").is_none());
        // undo: note optional.
        assert_eq!(find("history_undo").unwrap().schema().get("required"), None);
        // compare_ab: sides as objects, keep stays a param, the note goes on the envelope.
        let c = find("compare_ab").unwrap();
        assert_eq!(c.schema()["required"], json!(["b"]));
        assert!(c.schema()["properties"]["b"]["oneOf"].is_array());
        let args = json!({"b":{"values":{"fx.bloom.enabled":false}},"keep":true,"note":"bloom off is cheaper","capture_width":480});
        let env = c.envelope("m-3", args.as_object().unwrap());
        assert_eq!(env["note"], "bloom off is cheaper");
        assert_eq!(env["params"], json!({"b":{"values":{"fx.bloom.enabled":false}},"keep":true,"capture_width":480}));
        assert!(env.get("opts").is_none());
        // `session` picks the bridge; it never reaches the command.
        let env = t.envelope("m-4", json!({"values":{"a":1},"note":"n","session":8098}).as_object().unwrap());
        assert_eq!(env["params"], json!({"values":{"a":1}}));
        assert!(t.schema()["properties"]["session"].is_object());
        assert!(find("evani_check").unwrap().schema()["properties"].get("session").is_none());
        assert!(find("live_sessions").unwrap().schema()["properties"].get("session").is_none());
    }
}
