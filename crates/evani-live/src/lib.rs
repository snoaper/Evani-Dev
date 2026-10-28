//! Live control API (`evani.module.live`): dev builds only. A shipped game
//! is built without this crate, so it carries no live code at all.
//!
//! Requests in the docs/LIVE_API.md envelope
//! (`{"type":"cmd","v":1,"id","cmd","params","note"}`) go in with
//! [`submit`], are processed in arrival order by [`tick`] at a frame
//! boundary, and their result envelopes come out of [`take_replies`].
//! Every mutating command is one versioned transaction in the
//! [`session::LiveSession`] history (undo/redo, snapshots, batch); see
//! [`change::Change`] and [`commands::plan`].
//!
//! In the browser, [`LiveModule`] installs `window.evani.live`
//! (`send(msg)`, `call(cmd, params, opts)`, `on(event, fn)`, `protocol`);
//! the bridge socket feeds the same queue.
//!
//! The tunables (params, slots, formulas, tiers, effect status) all live in
//! evani-core; this crate only reads and drives them.

pub mod bake;
pub mod capture;
pub mod change;
pub mod commands;
#[cfg(feature = "models")]
pub mod convert;
pub mod edits;
pub mod focus;
pub mod geometry;
pub mod img;
pub mod look;
pub mod page_patch;
pub mod revert;
pub mod session;
pub mod telemetry;

use std::collections::BTreeMap;

use evani_core::assets::content_hash;
use evani_core::ecs::World;
use evani_core::effects::{BudgetStatus, EffectsStatus};
use evani_core::formulas::{FormulaBody, FormulaJson, Formulas};
use evani_core::params::{BakeTarget, F32Param, ParamDef, ParamError, ParamRegistry};
use evani_core::plugin::{BuildInfo, EngineModule, PluginContext, PluginError};
use evani_core::slots::ShaderSlots;
use evani_core::tiers::Tiers;
use serde_json::{Map, Value, json};

pub use change::{Change, FullState};
pub use session::LiveSession;

/// Protocol name and major version (`schema.get` → `protocol`).
pub const PROTOCOL: &str = "evani-live/1";

/// Commands that change state and therefore need a `note`.
pub const MUTATING: &[&str] = &[
    "param.set",
    "slot.set",
    "curve.set",
    "policy.set",
    "effect.toggle",
    "tier.set",
    "snapshot.restore",
    "batch",
];

/// A failed command.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveError {
    pub code: &'static str,
    pub message: String,
    pub data: Option<Value>,
}

impl LiveError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub(crate) fn with(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// An error for a command the app answers itself ([`AppCommands`]):
    /// a code from docs/LIVE_API.md 5.5 or the app's own documented list.
    pub fn app(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }
}

/// One command the app answers itself (see [`AppCommands`]).
#[derive(Debug, Clone, PartialEq)]
pub struct AppCall {
    /// The reply token ([`submit`]).
    pub token: u64,
    /// The request's `id`.
    pub id: Value,
    pub cmd: String,
    pub params: Value,
}

/// Commands an app (not a game: a dev page such as `apps/model-view`)
/// answers itself: every command starting with `prefix` goes to `handle`
/// instead of `unknown_command`. `handle` returns the answer now, or
/// `None` when it answers later with [`app_reply`]. Optional and additive:
/// without this resource nothing changes.
#[derive(Debug, Clone, Copy)]
pub struct AppCommands {
    pub prefix: &'static str,
    pub handle: fn(&World, &AppCall) -> Option<Result<Value, LiveError>>,
}

/// Answer an [`AppCall`] whose handler returned `None`.
/// Nothing happens without a live session (no one to answer).
pub fn app_reply(world: &World, call: &AppCall, res: Result<Value, LiveError>) {
    if world.has_resource::<LiveSession>() {
        commands::reply(world, call.token, &call.id, res);
    }
}

/// Send an event (`{"type":"event","v":1,"event":…,"data":…}`) to the
/// connected clients, as the engine's own events go.
pub fn emit_event(world: &World, event: &str, data: Value) {
    if let Some(mut s) = world.try_res_mut::<LiveSession>() {
        let ev = json!({ "type": "event", "v": 1, "event": event, "data": data });
        s.events.push(ev.to_string());
    }
}

pub(crate) fn param_error(reg: &ParamRegistry, e: ParamError) -> LiveError {
    match &e {
        ParamError::Unknown(name) => {
            let close = did_you_mean(reg, name);
            LiveError::new("unknown_target", format!("No parameter named {name}"))
                .with(json!({ "did_you_mean": close }))
        }
        _ => LiveError::new("invalid_value", e.to_string()),
    }
}

/// Up to 3 registered names sharing the longest dotted prefix with `name`.
pub(crate) fn did_you_mean(reg: &ParamRegistry, name: &str) -> Vec<String> {
    let parts: Vec<&str> = name.split('.').collect();
    for keep in (1..parts.len()).rev() {
        let prefix = format!("{}.", parts[..keep].join("."));
        let hits: Vec<String> = reg
            .defs()
            .iter()
            .filter(|d| d.name.starts_with(&prefix))
            .take(3)
            .map(|d| d.name.clone())
            .collect();
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}
/// Queue one request (JSON text in the docs/LIVE_API.md envelope). `token`
/// identifies the reply in [`take_replies`]; the caller picks it (the page
/// keeps a token → responder map). Processed at the next [`tick`].
pub fn submit(world: &World, token: u64, text: &str) {
    match world.try_res_mut::<LiveSession>() {
        Some(mut s) => s.inbox.push_back((token, text.to_string())),
        None => {
            // No live module: answer at once so nothing hangs.
            let reply = json!({ "type": "result", "v": 1, "id": null, "ok": false,
                "error": { "code": "internal", "message": "live session not installed" } });
            let _ = world
                .try_res_mut::<LiveSession>()
                .map(|mut s| s.reply(token, reply.to_string()));
        }
    }
}

/// Process queued requests in arrival order (called by [`LiveModule`] at a
/// frame boundary). A mutating command waiting for a WGSL slot compile
/// pauses the inbox until it finishes (or times out).
pub fn tick(world: &World) {
    if !world.has_resource::<LiveSession>() {
        return;
    }
    {
        let mut s = world.res_mut::<LiveSession>();
        if s.start_state.is_none() {
            drop(s);
            let st = FullState::capture(world);
            s = world.res_mut::<LiveSession>();
            s.start_state = Some(st);
        }
    }
    telemetry::sample(world);
    revert::frame(world);
    telemetry::pump_subs(world);
    commands::run_tier_amend(world);
    capture::frame(world);
    edits::settle_hold(world);
    capture::sync_hold(world);
    loop {
        // A capture run (like a parked slot compile or a `together` edit
        // commit waiting for its compiles) pauses the inbox.
        if !commands::poll_parked(world)
            || !edits::poll_commit(world)
            || capture::capture_active(world)
        {
            return;
        }
        let next = world.res_mut::<LiveSession>().inbox.pop_front();
        let Some((token, text)) = next else { return };
        commands::process(world, token, &text);
        // A tier change settles over the next frame; wait for it.
        if world.res::<LiveSession>().tier_amend.is_some() {
            return;
        }
    }
}

/// Finished replies (result envelopes) with their tokens, oldest first.
pub fn take_replies(world: &World) -> Vec<(u64, String)> {
    world
        .try_res_mut::<LiveSession>()
        .map_or_else(Vec::new, |mut s| core::mem::take(&mut s.replies))
}

/// Outgoing event envelopes (`auto_revert`, `telemetry`...), oldest first.
pub fn take_events(world: &World) -> Vec<String> {
    world
        .try_res_mut::<LiveSession>()
        .map_or_else(Vec::new, |mut s| core::mem::take(&mut s.events))
}

/// Synchronous helper (tests, tools): submit, tick once, and return this
/// request's reply, or `None` if it is still waiting (a parked slot compile;
/// keep ticking frames and collect it with [`take_replies`]).
pub fn handle_sync(world: &World, text: &str) -> Option<String> {
    const TOKEN: u64 = u64::MAX;
    // Synchronous: mutating results come back at once (`telemetry: null`)
    // unless the request sets `opts.telemetry_ms` itself.
    let text = match serde_json::from_str::<Value>(text) {
        Ok(mut v) if v.is_object() => {
            if v.get("opts").is_none_or(|o| !o.is_object()) {
                v["opts"] = json!({});
            }
            if v["opts"].get("telemetry_ms").is_none() {
                v["opts"]["telemetry_ms"] = json!(0);
            }
            v.to_string()
        }
        _ => text.to_string(),
    };
    submit(world, TOKEN, &text);
    tick(world);
    let mut s = world.try_res_mut::<LiveSession>()?;
    let i = s.replies.iter().position(|(t, _)| *t == TOKEN)?;
    Some(s.replies.remove(i).1)
}

/// `session.status` (6.26), as far as the page knows. The page answering
/// means it is connected. `watch` is the armed auto-revert guard, or null.
pub(crate) fn session_status(world: &World) -> Value {
    let s = schema(world);
    let head = world.try_res::<LiveSession>().map(|l| l.head_tx());
    let now_ms = world
        .try_res::<evani_core::time::Time>()
        .map_or(0.0, |t| t.now_ms);
    let busy = world
        .try_res::<LiveSession>()
        .and_then(|l| l.run.as_ref().map(|r| r.busy_json(now_ms)))
        .unwrap_or(Value::Null);
    let moments: Vec<Value> = world
        .try_res::<evani_core::capture::CaptureMoments>()
        .map(|m| {
            m.list()
                .iter()
                .map(|x| json!({ "name": x.name, "label": x.label }))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "engine_connected": true,
        "moments": moments,
        "entities": world.entity_count(),
        "game": s["game"],
        "tier": s["tiers"]["active"],
        "head": head,
        "busy": busy,
        "focus": focus::state_json(world),
        "focused": focus::focused_id(world),
        "schema_version": s["schema_version"],
        "versions": versions_json(world),
        "watch": world.try_res::<LiveSession>().and_then(|l| l.watch).map(|w| json!({
            "tx": w.tx, "auto_revert": "armed", "target_ms": telemetry::round2(w.target_ms),
            "limit_ms": telemetry::round2(w.limit_ms), "sustain_s": w.sustain_s,
            "window_s": w.window_s, "over": w.over_since.is_some() })),
    })
}

/// `session.status.versions` (6.26): the engine version and commit, the
/// protocol, every Evani.Dev part compiled into this game (from
/// [`BuildInfo::parts`], so only what is compiled in) and the overlay's
/// versions line.
pub(crate) fn versions_json(world: &World) -> Value {
    use evani_core::version::{ENGINE_COMMIT, ENGINE_VERSION, write_versions};
    let info = world.try_res::<BuildInfo>();
    let parts = info.as_ref().map_or(&[][..], |i| &i.parts[..]);
    let mut line = String::new();
    write_versions(&mut line, parts);
    let list: Vec<Value> = parts
        .iter()
        .map(|p| {
            let mut o = Map::new();
            o.insert("kind".into(), json!(p.kind.as_str()));
            o.insert("name".into(), json!(p.name));
            o.insert("version".into(), json!(p.version));
            if !p.features.is_empty() {
                o.insert("features".into(), json!(p.features));
            }
            if let Some(proto) = p.protocol {
                o.insert("protocol".into(), json!(proto));
            }
            Value::Object(o)
        })
        .collect();
    json!({
        "engine": ENGINE_VERSION,
        "commit": ENGINE_COMMIT,
        "protocol": PROTOCOL,
        "parts": list,
        "line": line,
    })
}

/// `fn tonemap(color: vec3<f32>) -> vec3<f32>`.
pub(crate) fn wgsl_signature(d: &evani_core::slots::SlotDef) -> String {
    let args: Vec<String> = d
        .signature
        .params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect();
    format!(
        "fn {}({}) -> {}",
        d.signature.name,
        args.join(", "),
        d.signature.ret
    )
}

/// `(dt, speed, distance) -> f32`.
pub(crate) fn expr_signature(d: Option<&evani_core::formulas::FormulaDef>) -> String {
    format!(
        "({}) -> f32",
        d.map(|d| d.vars.join(", ")).unwrap_or_default()
    )
}

/// `schema.get` with the optional `include` filter (6.1).
pub(crate) fn schema_filtered(world: &World, params: &Value) -> Value {
    let mut s = schema(world);
    if let Some(inc) = params.get("include").and_then(Value::as_array)
        && let Some(obj) = s.as_object_mut()
    {
        let keep: Vec<&str> = inc.iter().filter_map(Value::as_str).collect();
        for k in ["params", "slots", "curves", "effects", "tiers", "policy"] {
            if !keep.contains(&k) {
                obj.remove(k);
            }
        }
    }
    s
}

/// `schema.get`: params, slots, formulas, effects, tiers and the build.
pub fn schema(world: &World) -> Value {
    let reg = world.res::<ParamRegistry>();
    let params = reg.schema();
    let mut sig = String::new();
    for p in &params {
        sig.push_str(&p.name);
        sig.push(':');
        sig.push_str(p.ty.as_str());
        sig.push(';');
    }
    let schema_version = format!("{:08x}", content_hash(sig.as_bytes()) as u32);
    // While an asset is focused, its tunables come first (registry order
    // otherwise; the schema version ignores the order).
    let focus = focus::focused_params(world);
    let mut params = params;
    if let Some((_, names)) = &focus {
        params.sort_by_key(|p| !names.contains(&p.name));
    }

    let mut slots: Vec<Value> = world.try_res::<ShaderSlots>().map_or_else(Vec::new, |s| {
        s.iter()
            .map(|(_, d)| {
                json!({
                    "name": d.signature.name,
                    "lang": "wgsl",
                    "signature": wgsl_signature(d),
                    "owner": d.owner,
                    "desc": d.signature.description,
                    "hash": change::body_hash(&d.body),
                    "default_hash": change::body_hash(&d.default_body),
                    "is_default": d.body == d.default_body,
                })
            })
            .collect()
    });

    // CPU formulas: expressions are slots (`lang: "expr"`), curves are curves.
    let mut curves: Vec<Value> = Vec::new();
    if let Some(f) = world.try_res::<Formulas>() {
        for id in f.ids() {
            let (Some(d), Some(body)) = (f.def(id), f.to_json_body(id)) else {
                continue;
            };
            let default = match f.default_body(id) {
                Some(FormulaBody::Expr(e)) => FormulaJson::Expr(e.source().to_string()),
                Some(FormulaBody::Curve(c)) => FormulaJson::Curve(c.clone()),
                None => continue,
            };
            let range = |r: Option<[f32; 2]>| {
                r.map_or(
                    json!({ "min": null, "max": null }),
                    |[a, b]| json!({ "min": a, "max": b }),
                )
            };
            if change::is_curve(&f, id) {
                let mut x = range(d.x_range);
                x["name"] = json!(d.vars.first());
                curves.push(json!({
                    "name": d.name, "x": x, "y": range(d.y_range),
                    "points": change::curve_points_json(&body),
                    "default_points": change::curve_points_json(&default),
                    "max_points": evani_core::curve::MAX_POINTS,
                    "owner": d.owner, "desc": d.description,
                    "is_default": body == default,
                }));
            } else {
                let src = change::formula_text(&body);
                let def_src = change::formula_text(&default);
                slots.push(json!({
                    "name": d.name, "lang": "expr", "signature": expr_signature(Some(d)),
                    "vars": d.vars, "owner": d.owner, "desc": d.description,
                    "hash": change::body_hash(&src), "default_hash": change::body_hash(&def_src),
                    "is_default": src == def_src,
                }));
            }
        }
    }

    let effects: Vec<Value> = world.try_res::<EffectsStatus>().map_or_else(Vec::new, |s| {
        s.effects
            .iter()
            .map(|e| {
                json!({
                    "name": e.info.name,
                    "on": e.state.is_on(),
                    "state": e.state.as_str(),
                    "cost_points": e.info.cost,
                    "value_weight": e.weight.map_or(e.info.value, |h| reg.f32(h)),
                    "styles": e.info.styles.names().collect::<Vec<_>>(),
                    "vr_comfort_off": e.info.vr_comfort_off,
                    "passes": e.passes,
                })
            })
            .collect()
    });

    let tiers = world.res::<Tiers>();
    let points: BTreeMap<&str, u32> = tiers
        .presets()
        .iter()
        .map(|p| (p.id.as_str(), p.points))
        .collect();
    let budget = *world.res::<BudgetStatus>();

    let build = world.try_res::<BuildInfo>().map_or(Value::Null, |b| {
        json!({
            "modules": b.modules.iter().map(|(_, label)| *label).collect::<Vec<_>>(),
            "effects": effects.iter().filter_map(|e| e["name"].as_str()).collect::<Vec<_>>(),
            "look": b.look,
            "kit": b.kit,
        })
    });
    let game = world.try_res::<BuildInfo>().map(|b| b.game.clone());
    let policy_params: Vec<&str> = params
        .iter()
        .filter(|p| p.name.starts_with("aq."))
        .map(|p| p.name.as_str())
        .collect();
    let weights: Vec<&str> = params
        .iter()
        .filter(|p| p.name.starts_with("fx.") && p.name.ends_with(".value_weight"))
        .map(|p| p.name.as_str())
        .collect();

    json!({
        "protocol": PROTOCOL,
        "schema_version": schema_version,
        "game": game,
        "build": build,
        "params": params,
        "slots": slots,
        "curves": curves,
        "effects": effects,
        "tiers": {
            "active": tiers.active().id,
            "available": tiers.presets().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            "points": points,
            "points_used": budget.used,
        },
        "policy": { "params": policy_params, "weights": weights },
        "focus": focus.map(|(asset, params)| json!({ "asset": asset, "params": params })),
    })
}

pub(crate) fn param_get(world: &World, params: &Value) -> Result<Value, LiveError> {
    let reg = world.res::<ParamRegistry>();
    let mut values = Map::new();
    let value_of = |id| {
        let def = reg.def(id)?;
        Some(def.value_to_json(&reg.value(id)?))
    };
    if let Some(names) = params.get("names") {
        let names = names
            .as_array()
            .ok_or_else(|| LiveError::new("bad_request", "names must be an array"))?;
        for n in names {
            let n = n
                .as_str()
                .ok_or_else(|| LiveError::new("bad_request", "names must be strings"))?;
            let id = reg
                .id(n)
                .ok_or_else(|| param_error(&reg, ParamError::Unknown(n.to_string())))?;
            values.insert(n.to_string(), value_of(id).unwrap_or(Value::Null));
        }
    } else {
        let prefix = params.get("prefix").and_then(Value::as_str).unwrap_or("");
        for d in reg.defs() {
            if d.name.starts_with(prefix)
                && let Some(id) = reg.id(&d.name)
            {
                values.insert(d.name.clone(), value_of(id).unwrap_or(Value::Null));
            }
        }
    }
    Ok(json!({ "values": values }))
}

// ---- the module ------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct LiveParams {
    pub revert_over_pct: F32Param,
    pub revert_sustain_s: F32Param,
    pub revert_window_s: F32Param,
    pub telemetry_ms: F32Param,
}

/// The live module (`evani.module.live`). Add it only in dev builds (the
/// sandbox's `live` feature); xtask's sizes report proves a build without it
/// has no live code.
#[derive(Default)]
pub struct LiveModule {
    params: Option<LiveParams>,
    #[cfg(target_arch = "wasm32")]
    page: page::Page,
    /// The socket to the live bridge, when the bridge served this page.
    #[cfg(target_arch = "wasm32")]
    link: Option<std::rc::Rc<ws::Link>>,
    /// Marks the page while an asset is focused (hides the DOM UI).
    #[cfg(target_arch = "wasm32")]
    focus_mark: focus::PageMark,
    /// The "measuring…" tag while a trial run holds the canvas.
    #[cfg(target_arch = "wasm32")]
    trial_mark: trial_mark::TrialMark,
}

impl LiveModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// The auto-revert param handles (registered in `build`).
    pub fn params(&self) -> Option<LiveParams> {
        self.params
    }
}

impl EngineModule for LiveModule {
    fn id(&self) -> &'static str {
        "evani.module.live"
    }

    fn label(&self) -> &'static str {
        "live"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn protocol(&self) -> Option<&'static str> {
        Some(PROTOCOL)
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        ctx.resource(LiveSession::default());
        // This is a development build: the overlay's download line says so
        // and shows what players download (the bridge passes the last
        // `cargo xtask size-report` gzip total in at build time).
        ctx.resource(evani_core::overlay::DevBuild {
            published_gz: option_env!("EVANI_PUBLISHED_GZ").and_then(|s| s.trim().parse().ok()),
        });
        // Only live queues captures. The bench path is the game's
        // `BenchPath` resource, or `capture::default_path()` without one.
        ctx.resource(evani_core::capture::CaptureQueue::default());
        // Trial runs hold the canvas on its last frame (6.30).
        ctx.resource(evani_core::capture::HoldCanvas::default());
        // Drawing slows while Evani.App parks the tab (host.rs, 3.5).
        ctx.resource(evani_core::capture::RenderPace::default());
        // The geometry debug view (`debug.geometry`): last in the frame
        // stage, after every kit has drawn and before the renderer draws.
        ctx.resource(geometry::GeometryView::default());
        // Asset focus (`focus.*`); kits fill `FocusAssets` in `add_kit`.
        ctx.resource(evani_core::focus::FocusState::default());
        ctx.system(
            evani_core::ecs::SystemDesc::new("live.geometry", evani_core::ecs::Stage::Frame, geometry::frame)
                .order(i32::MAX),
        );
        // Drop-to-convert's split view (`model.load`, LIVE_API.md 16): after
        // every kit has drawn, before the geometry view reads the draws.
        #[cfg(feature = "models")]
        {
            ctx.resource(convert::CompareView::default());
            ctx.system(
                evani_core::ecs::SystemDesc::new("live.model", evani_core::ecs::Stage::Frame, convert::frame)
                    .order(i32::MAX - 1),
            );
        }
        self.params = Some(LiveParams {
            revert_over_pct: ctx.param_f32(
                ParamDef::f32("live.revert.over_pct", 25.0)
                    .range(5.0, 200.0)
                    .unit("%")
                    .describe(
                        "Auto-revert a change if frame time stays this far over the tier target.",
                    )
                    .owner("live")
                    .bake(BakeTarget::None),
            )?,
            revert_sustain_s: ctx.param_f32(
                ParamDef::f32("live.revert.sustain_s", 3.0)
                    .range(0.5, 30.0)
                    .unit("s")
                    .describe("How long frame time must stay over before the change reverts.")
                    .owner("live")
                    .bake(BakeTarget::None),
            )?,
            revert_window_s: ctx.param_f32(
                ParamDef::f32("live.revert.window_s", 10.0)
                    .range(1.0, 120.0)
                    .unit("s")
                    .describe("How long after a change the auto-revert guard watches frame time.")
                    .owner("live")
                    .bake(BakeTarget::None),
            )?,
            telemetry_ms: ctx.param_f32(
                ParamDef::f32("live.telemetry_ms", 1000.0)
                    .range(0.0, 10_000.0)
                    .unit("ms")
                    .describe("Telemetry gathered after a change before its result is sent.")
                    .owner("live")
                    .bake(BakeTarget::None),
            )?,
        });
        // How an edit transaction applies its edits when `edit.begin` names
        // no mode (6.31). Stream is the default; together can be chosen per
        // transaction or made the default here.
        ctx.param_enum(
            ParamDef::enumeration(edits::MODE_PARAM, &["stream", "together"], 0)
                .describe(
                    "Edit transactions without a mode: stream (each edit shows as soon as it is staged) or together (all show at once on commit).",
                )
                .owner("live")
                .bake(BakeTarget::None),
        )?;
        #[cfg(target_arch = "wasm32")]
        {
            page::install(&self.page).map_err(PluginError)?;
            self.link = ws::connect(self.page.queue.clone(), self.page.listeners.clone());
            // Drop-to-convert's page controls (LIVE_API.md 16), on pages the
            // bridge served (they post to it).
            #[cfg(feature = "models")]
            if self.link.is_some() {
                convert_ui::install();
            }
            // Page state, heartbeat and render pacing (LIVE_API.md 3.5).
            host::install(self.link.clone());
            // Live code swapping (LIVE_API.md 10.4).
            #[cfg(feature = "hotpatch")]
            hotpatch::install(self.link.clone());
        }
        Ok(())
    }

    /// Capture the session's start state (after kits, looks and baked
    /// files are applied) for snapshots and bake.
    fn start(&mut self, world: &mut World) {
        // Hot mode: baked files changed since the build (10.4).
        #[cfg(target_arch = "wasm32")]
        baked_from_page(world);
        let st = FullState::capture(world);
        if let Some(s) = world.res_get_mut::<LiveSession>() {
            s.start_state = Some(st);
        }
        // `?focus=<id>`: open the page on one asset (every kit has
        // registered its assets by now).
        #[cfg(target_arch = "wasm32")]
        if let Some(q) = web_sys::window().and_then(|w| w.location().search().ok()) {
            focus::start_from_query(world, &q);
        }
    }

    fn frame(&mut self, world: &mut World) {
        #[cfg(target_arch = "wasm32")]
        {
            let measuring = world.try_res::<LiveSession>().is_some_and(|s| s.run.is_some());
            host::frame(world, measuring);
            if let Some(link) = &self.link {
                link.frame(world);
            }
            self.page.accept(world);
        }
        tick(world);
        #[cfg(target_arch = "wasm32")]
        self.focus_mark.frame(world);
        #[cfg(target_arch = "wasm32")]
        self.trial_mark.frame(world);
        #[cfg(target_arch = "wasm32")]
        {
            let events = self.page.respond(world);
            if let Some(link) = &self.link {
                for e in &events {
                    link.send_text(e);
                }
            }
        }
    }

    fn status(&self, world: &World, out: &mut String) {
        use core::fmt::Write;
        let handled = world.try_res::<LiveSession>().map_or(0, |s| s.handled);
        let _ = write!(out, "window.evani.live ready · {handled} commands");
        #[cfg(target_arch = "wasm32")]
        if let Some(link) = &self.link {
            let _ = write!(
                out,
                " · bridge {}",
                if link.connected() {
                    "connected"
                } else {
                    "offline"
                }
            );
        }
    }
}

/// Hot mode (docs/LIVE_API.md 10.4): apply the baked files the live server
/// says changed since the build (`{owner: {path: text}}`, owners
/// `kit.<name>` and `look.<name>`), for this game's kit and look only, the
/// same way the build's own baked files were applied at start. Returns how
/// many files were applied. Dev only: the texts are leaked (once per page).
pub fn apply_changed_baked(world: &mut World, files: &Value) -> usize {
    let (kit, look) = {
        let info = world.res::<BuildInfo>();
        (info.kit.clone(), info.look.clone())
    };
    let ours = |owner: &str| {
        let is = |prefix: &str, krate: &Option<String>, dir: &str| {
            owner
                .strip_prefix(prefix)
                .is_some_and(|n| krate.as_deref() == Some(format!("{dir}{n}").as_str()))
        };
        is("kit.", &kit, "evani-kit-") || is("look.", &look, "evani-look-")
    };
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let mut list: Vec<(String, evani_core::plugin::BakedFiles)> = Vec::new();
    let mut n = 0;
    for (owner, m) in files.as_object().into_iter().flatten() {
        if !ours(owner) {
            continue;
        }
        let entries: Vec<(&'static str, &'static str)> = m
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(rel, t)| t.as_str().map(|t| (leak(rel), leak(t))))
            .collect();
        n += entries.len();
        list.push((owner.clone(), Box::leak(entries.into_boxed_slice())));
    }
    if n > 0 {
        world.res_mut::<evani_core::plugin::PendingBaked>().files = list;
        evani_core::app::apply_pending_baked(world);
    }
    n
}

/// Read `<script id="evani-live-baked">` (the live server puts it in the
/// page in hot mode) and apply it.
#[cfg(target_arch = "wasm32")]
fn baked_from_page(world: &mut World) {
    let Some(text) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("evani-live-baked"))
        .and_then(|e| e.text_content())
    else {
        return;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(files) => {
            let n = apply_changed_baked(world, &files);
            if n > 0 {
                world
                    .res_mut::<evani_core::overlay::ErrorLog>()
                    .note(format!("live: {n} baked file(s) newer than the build applied"));
            }
        }
        Err(e) => world
            .res_mut::<evani_core::overlay::ErrorLog>()
            .error(format!("live: the page's baked files are not JSON: {e}")),
    }
}

#[cfg(target_arch = "wasm32")]
mod host;

#[cfg(target_arch = "wasm32")]
mod ws;

#[cfg(target_arch = "wasm32")]
mod trial_mark;

#[cfg(all(target_arch = "wasm32", feature = "models"))]
mod convert_ui;

#[cfg(all(target_arch = "wasm32", feature = "hotpatch"))]
mod hotpatch;

#[cfg(target_arch = "wasm32")]
mod page {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use evani_core::ecs::World;
    use js_sys::{Function, Object, Promise, Reflect};
    use wasm_bindgen::JsValue;
    use wasm_bindgen::closure::Closure;

    /// Who gets a reply.
    pub enum Responder {
        /// `send(msg)` and the bridge socket: called with the whole result
        /// envelope.
        Envelope(Function),
        /// `call(cmd, params, opts)`: resolved with `result`, or rejected with
        /// an `Error` carrying `.code`, `.message`, `.data`.
        Call { resolve: Function, reject: Function },
    }

    pub type Queue = Rc<RefCell<Vec<(String, Responder)>>>;
    pub type Listeners = Rc<RefCell<Vec<(u64, String, Function)>>>;

    /// Call the `on(event)` listeners with `data` (the bridge's own events,
    /// such as `convert_done`, reach the page this way, LIVE_API.md 16).
    pub fn dispatch(listeners: &Listeners, event: &str, data: &JsValue) {
        let ls: Vec<Function> = listeners.borrow().iter().filter(|(_, e, _)| e == event).map(|(_, _, f)| f.clone()).collect();
        for f in ls {
            let _ = f.call1(&JsValue::NULL, data);
        }
    }

    /// Page side of the session: requests from JS and the socket wait in
    /// `queue` until the frame boundary, then get a token.
    #[derive(Default)]
    pub struct Page {
        pub queue: Queue,
        pub listeners: Listeners,
        waiting: BTreeMap<u64, Responder>,
        next_token: u64,
    }

    fn set(o: &JsValue, k: &str, v: &JsValue) -> Result<(), String> {
        Reflect::set(o, &JsValue::from_str(k), v)
            .map(|_| ())
            .map_err(|_| format!("live: cannot set {k}"))
    }

    fn to_json(v: &JsValue) -> serde_json::Value {
        if v.is_undefined() || v.is_null() {
            return serde_json::Value::Null;
        }
        js_sys::JSON::stringify(v)
            .ok()
            .and_then(|s| serde_json::from_str(&String::from(s)).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    fn get(o: &JsValue, k: &str) -> JsValue {
        Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
    }

    /// `window.evani.live = { protocol, send, call, on }`.
    pub fn install(page: &Page) -> Result<(), String> {
        let win = web_sys::window().ok_or("live: no window")?;
        let evani = Reflect::get(&win, &JsValue::from_str("evani"))
            .ok()
            .filter(JsValue::is_object)
            .unwrap_or_else(|| Object::new().into());
        let live = Object::new();

        let queue = page.queue.clone();
        let send = Closure::<dyn FnMut(JsValue) -> Promise>::new(move |msg: JsValue| {
            let text = msg.as_string().unwrap_or_else(|| {
                js_sys::JSON::stringify(&msg)
                    .map(String::from)
                    .unwrap_or_default()
            });
            let q = queue.clone();
            Promise::new(&mut |resolve, _reject| {
                q.borrow_mut()
                    .push((text.clone(), Responder::Envelope(resolve)));
            })
        });

        let queue = page.queue.clone();
        let counter = Rc::new(Cell::new(0u64));
        let call = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Promise>::new(
            move |cmd: JsValue, params: JsValue, opts: JsValue| {
                counter.set(counter.get() + 1);
                let opts = to_json(&opts);
                let mut params = to_json(&params);
                if params.is_null() {
                    params = serde_json::json!({});
                }
                let mut env = serde_json::json!({
                    "type": "cmd", "v": 1, "id": format!("page-{}", counter.get()),
                    "cmd": cmd.as_string().unwrap_or_default(), "params": params,
                    "source": "page",
                });
                if let Some(note) = opts.get("note") {
                    env["note"] = note.clone();
                }
                let mut o = serde_json::Map::new();
                for k in ["telemetry_ms", "capture"] {
                    if let Some(v) = opts.get(k) {
                        o.insert(k.to_string(), v.clone());
                    }
                }
                env["opts"] = serde_json::Value::Object(o);
                let text = env.to_string();
                let q = queue.clone();
                Promise::new(&mut |resolve, reject| {
                    q.borrow_mut()
                        .push((text.clone(), Responder::Call { resolve, reject }));
                })
            },
        );

        let listeners = page.listeners.clone();
        let next_listener = Rc::new(Cell::new(0u64));
        let on = Closure::<dyn FnMut(JsValue, Function) -> JsValue>::new(
            move |event: JsValue, f: Function| {
                next_listener.set(next_listener.get() + 1);
                let id = next_listener.get();
                listeners
                    .borrow_mut()
                    .push((id, event.as_string().unwrap_or_default(), f));
                let ls = listeners.clone();
                Closure::once_into_js(move || {
                    ls.borrow_mut().retain(|(i, _, _)| *i != id);
                })
            },
        );

        set(&live, "send", send.as_ref())?;
        set(&live, "call", call.as_ref())?;
        set(&live, "on", on.as_ref())?;
        set(&live, "protocol", &JsValue::from_str(super::PROTOCOL))?;
        set(&evani, "live", &live)?;
        set(&win, "evani", &evani)?;
        // Live as long as the page.
        send.forget();
        call.forget();
        on.forget();
        Ok(())
    }

    impl Page {
        /// Move queued requests into the session inbox (frame boundary).
        pub fn accept(&mut self, world: &World) {
            let items = core::mem::take(&mut *self.queue.borrow_mut());
            for (text, responder) in items {
                self.next_token += 1;
                self.waiting.insert(self.next_token, responder);
                super::submit(world, self.next_token, &text);
            }
        }

        /// Answer finished requests and dispatch events to `on` listeners.
        /// Returns the event envelopes (the socket forwards them).
        pub fn respond(&mut self, world: &World) -> Vec<String> {
            for (token, reply) in super::take_replies(world) {
                let Some(r) = self.waiting.remove(&token) else {
                    continue;
                };
                let v = js_sys::JSON::parse(&reply).unwrap_or_else(|_| JsValue::from_str(&reply));
                match r {
                    Responder::Envelope(f) => {
                        let _ = f.call1(&JsValue::NULL, &v);
                    }
                    Responder::Call { resolve, reject } => {
                        if get(&v, "ok").as_bool() == Some(true) {
                            let _ = resolve.call1(&JsValue::NULL, &get(&v, "result"));
                        } else {
                            let e = get(&v, "error");
                            let msg = get(&e, "message").as_string().unwrap_or_default();
                            let err = js_sys::Error::new(&msg);
                            let _ =
                                Reflect::set(&err, &JsValue::from_str("code"), &get(&e, "code"));
                            let _ =
                                Reflect::set(&err, &JsValue::from_str("data"), &get(&e, "data"));
                            let _ = reject.call1(&JsValue::NULL, &err);
                        }
                    }
                }
            }
            let events = super::take_events(world);
            if !events.is_empty() {
                let ls: Vec<(String, Function)> = self
                    .listeners
                    .borrow()
                    .iter()
                    .map(|(_, e, f)| (e.clone(), f.clone()))
                    .collect();
                for text in &events {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
                        continue;
                    };
                    let name = v["event"].as_str().unwrap_or("");
                    let data = js_sys::JSON::parse(&v["data"].to_string()).unwrap_or(JsValue::NULL);
                    for (e, f) in &ls {
                        if e == name {
                            let _ = f.call1(&JsValue::NULL, &data);
                        }
                    }
                }
            }
            events
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    fn app() -> App {
        let mut a = App::new("sandbox").with_module(LiveModule::new());
        a.context("look.basic")
            .param_f32(ParamDef::f32("look.basic.exposure", 1.0).range(0.0, 4.0))
            .unwrap();
        a.run_headless(1, 16.0);
        a
    }

    fn call(a: &App, req: Value) -> Value {
        serde_json::from_str(&handle_sync(&a.world, &req.to_string()).unwrap()).unwrap()
    }

    #[test]
    fn schema_get_lists_params_tiers_build() {
        let a = app();
        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"c-1","cmd":"schema.get","params":{}}),
        );
        assert_eq!(r["ok"], true);
        assert_eq!(r["id"], "c-1");
        let res = &r["result"];
        assert_eq!(res["protocol"], PROTOCOL);
        assert_eq!(res["game"], "sandbox");
        assert_eq!(res["build"]["modules"], json!(["live"]));
        assert_eq!(res["schema_version"].as_str().unwrap().len(), 8);
        let names: Vec<&str> = res["params"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"look.basic.exposure"));
        assert!(names.contains(&"live.revert.over_pct"));
        assert!(names.contains(&"core.audio.master"));
        assert!(res["params"][0]["desc"].is_string());
        assert_eq!(res["tiers"]["points"]["medium"], 11);
        assert!(
            res["slots"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["name"] == "tonemap")
        );
    }

    #[test]
    fn param_get_and_atomic_set() {
        let a = app();
        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"g","cmd":"param.get","params":{"prefix":"look."}}),
        );
        assert_eq!(r["result"]["values"], json!({"look.basic.exposure": 1.0}));

        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"s","cmd":"param.set",
                   "params":{"values":{"look.basic.exposure": 9.0}},"note":"brighter"}),
        );
        assert_eq!(r["ok"], true, "{r}");
        let ap = &r["result"]["applied"][0];
        assert_eq!(ap["new"], 4.0);
        assert_eq!(ap["clamped"], true);
        assert_eq!(r["result"]["tx"], 1);

        // One bad name: nothing applies.
        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"s2","cmd":"param.set",
                   "params":{"values":{"look.basic.exposure": 2.0, "look.basic.exposur": 1.0}},"note":"x"}),
        );
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["code"], "unknown_target");
        assert_eq!(r["error"]["data"]["did_you_mean"][0], "look.basic.exposure");
        let reg = a.world.res::<ParamRegistry>();
        assert_eq!(reg.f32(reg.f32_handle("look.basic.exposure").unwrap()), 4.0);
    }

    #[test]
    fn session_status_reports_game_and_tier() {
        let a = app();
        let r: Value = serde_json::from_str(
            &handle_sync(
                &a.world,
                r#"{"type":"cmd","v":1,"id":"s","cmd":"session.status"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(r["result"]["engine_connected"], true);
        assert_eq!(r["result"]["game"], "sandbox");
        assert!(r["result"]["tier"].is_string());
        assert!(r["result"]["entities"].is_u64());
        assert_eq!(r["result"]["schema_version"].as_str().unwrap().len(), 8);
        // Only live is compiled into this test app (plus core).
        let v = &r["result"]["versions"];
        assert_eq!(v["engine"], evani_core::version::ENGINE_VERSION);
        assert_eq!(v["commit"], evani_core::version::ENGINE_COMMIT);
        assert_eq!(v["protocol"], PROTOCOL);
        let names: Vec<&str> = v["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["core", "live"]);
    }

    /// Hot mode (10.4): baked files newer than the build arrive as data
    /// and are applied for this game's kit and look only.
    #[test]
    fn changed_baked_files_apply_for_this_games_kit_and_look() {
        let mut a = App::new("sandbox")
            .with_kit(evani_kit_example::ExampleKitPlugin)
            .with_look(evani_look_basic::BasicLook)
            .with_module(evani_render::RenderModule::new())
            .with_module(evani_fx::FxModule::compiled())
            .with_module(LiveModule::new());
        a.run_headless(1, 16.0);
        let get = |a: &App| {
            let reg = a.world.res::<ParamRegistry>();
            reg.id("fx.bloom.intensity").and_then(|id| reg.value(id))
        };
        assert_eq!(get(&a), Some(evani_core::params::ParamValue::F32(0.3)), "the build's own baked value");
        let files = json!({
            "look.basic": { "params.json": "{\"fx.bloom.intensity\": 0.55}" },
            "look.toon": { "params.json": "{\"fx.bloom.intensity\": 0.9}" },
            "kit.kart-racer": { "params.json": "{}" }
        });
        assert_eq!(apply_changed_baked(&mut a.world, &files), 1);
        assert_eq!(get(&a), Some(evani_core::params::ParamValue::F32(0.55)));
        assert_eq!(apply_changed_baked(&mut a.world, &json!({})), 0);
    }

    /// Chris: "version numbers of things being used from Evani.dev":
    /// every part compiled in, with its crate version, and nothing else.
    #[test]
    fn session_status_lists_every_part_with_its_version() {
        let mut a = App::new("sandbox")
            .with_kit(evani_kit_example::ExampleKitPlugin)
            .with_look(evani_look_basic::BasicLook)
            .with_module(evani_render::RenderModule::new())
            .with_module(evani_fx::FxModule::compiled())
            .with_module(LiveModule::new());
        a.run_headless(1, 16.0);
        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"s","cmd":"session.status","params":{}}),
        );
        let v = &r["result"]["versions"];
        let ver = env!("CARGO_PKG_VERSION");
        // Whatever this test build unified into evani-fx (at least bloom and
        // vignette, the dev-dependency's features).
        let fx = evani_fx::FxModule::compiled().version_features();
        assert!(fx.starts_with(&["bloom", "vignette"]), "{fx:?}");
        assert_eq!(
            v["parts"],
            json!([
                { "kind": "core", "name": "core", "version": ver },
                { "kind": "module", "name": "render", "version": ver },
                { "kind": "module", "name": "fx", "version": ver, "features": fx },
                { "kind": "module", "name": "live", "version": ver, "protocol": PROTOCOL },
                { "kind": "kit", "name": "example", "version": ver },
                { "kind": "look", "name": "basic", "version": ver },
            ])
        );
        let c = evani_core::version::ENGINE_COMMIT;
        assert_eq!(
            v["line"],
            format!(
                "evani {ver} @{c} · core {ver} · render {ver} · fx {ver} [{}] · \
                 live {ver} (evani-live/1) · kit example {ver} · look basic {ver}",
                fx.join(" ")
            )
        );
        // The overlay's first line is the same text.
        a.run_headless(20, 16.0);
        let text = a.world.res::<evani_core::overlay::Overlay>().text().to_string();
        assert_eq!(text.lines().next(), v["line"].as_str(), "{text}");
    }

    #[test]
    fn envelope_errors() {
        let a = app();
        let code = |req: &str| -> String {
            let r: Value = serde_json::from_str(&handle_sync(&a.world, req).unwrap()).unwrap();
            assert_eq!(r["ok"], false);
            assert_eq!(r["type"], "result");
            r["error"]["code"].as_str().unwrap().to_string()
        };
        assert_eq!(code("{nope"), "bad_request");
        assert_eq!(
            code(r#"{"type":"cmd","v":2,"id":"a","cmd":"schema.get"}"#),
            "version_mismatch"
        );
        assert_eq!(
            code(r#"{"type":"cmd","v":1,"id":"a","cmd":"fly"}"#),
            "unknown_command"
        );
        assert_eq!(
            code(r#"{"type":"cmd","v":1,"id":"a","cmd":"compare.ab"}"#),
            "bad_request"
        );
        assert_eq!(
            code(r#"{"type":"cmd","v":1,"id":"a","cmd":"param.set","params":{"values":{}}}"#),
            "note_required"
        );
        assert_eq!(
            code(
                r#"{"type":"cmd","v":1,"id":"a","cmd":"param.set","params":{"values":{"look.basic.exposure":"x"}},"note":"n"}"#
            ),
            "invalid_value"
        );
        assert_eq!(
            code(
                r#"{"type":"cmd","v":1,"id":"a","cmd":"effect.toggle","params":{"effect":"bloom","on":true},"note":"n"}"#
            ),
            "not_compiled_in"
        );
    }

    #[test]
    fn tier_set_switches() {
        let mut a = app();
        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"t","cmd":"tier.set","params":{"tier":"low"},"note":"weak laptop"}),
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["result"]["tier"]["points_limit"], 7);
        a.run_headless(1, 16.0);
        assert_eq!(a.world.res::<BudgetStatus>().total, 7);
        let r = call(
            &a,
            json!({"type":"cmd","v":1,"id":"t","cmd":"tier.set","params":{"tier":"potato"},"note":"x"}),
        );
        assert_eq!(r["error"]["code"], "unknown_target");
    }
}

#[cfg(test)]
mod revert_tests;
#[cfg(test)]
mod tx_tests;
#[cfg(test)]
mod edit_tests;

#[cfg(test)]
mod bake_tests;
#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod geometry_tests;
