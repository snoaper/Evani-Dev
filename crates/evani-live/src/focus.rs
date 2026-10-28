//! Asset focus commands (`focus.list`, `focus.set`, `focus.clear`,
//! docs/LIVE_API.md 6.28) and the page's `?focus=<id>` flag. The generic
//! state lives in `evani_core::focus` (core feature `focus`, turned on only
//! by this crate); kits list their assets and draw the focused one.
//!
//! Focus is a view, not a change: no note, no history entry, never baked.

use evani_core::ecs::World;
use evani_core::focus::{FocusAssets, FocusState};
use evani_core::params::ParamRegistry;
use serde_json::{Value, json};

use crate::LiveError;

/// Params of the focused asset, in registry order (`schema.get` lists them
/// first). Empty when nothing is focused.
pub(crate) fn focused_params(world: &World) -> Option<(String, Vec<String>)> {
    let st = world.try_res::<FocusState>()?;
    let id = st.active.clone()?;
    let assets = world.try_res::<FocusAssets>()?;
    let a = assets.get(&id)?;
    let reg = world.res::<ParamRegistry>();
    let names = reg
        .schema()
        .into_iter()
        .map(|p| p.name)
        .filter(|n| a.tunes(n))
        .collect();
    Some((id, names))
}

fn round3(x: f32) -> f64 {
    (f64::from(x) * 1000.0).round() / 1000.0
}

fn asset_json(world: &World, a: &evani_core::focus::FocusAsset) -> Value {
    let reg = world.res::<ParamRegistry>();
    let params: Vec<String> = reg
        .schema()
        .into_iter()
        .map(|p| p.name)
        .filter(|n| a.tunes(n))
        .collect();
    json!({
        "id": a.id, "label": a.label, "kind": a.kind, "owner": a.owner, "path": a.path,
        "radius": round3(a.radius), "params": params,
    })
}

/// The asset view's name for the focused id (`session.status` `focused`).
pub(crate) fn focused_id(world: &World) -> Value {
    world
        .try_res::<FocusState>()
        .and_then(|s| s.active.clone())
        .map_or(Value::Null, Value::String)
}

// ---- The asset view Evani.App's Evani.dev tab asks for ----
//
// `asset.list`, `asset.focus` and `asset.clear` are the same focus mode under
// the names and shapes the tab uses (its docs/evani-dev-asset-view.md): a
// plain asset record, and a `view` of `turntable` (the camera turns on its
// own) or `orbit` (a still camera that a drag in the page turns).

fn asset_record(a: &evani_core::focus::FocusAsset) -> Value {
    json!({ "id": a.id, "name": a.label, "kind": a.kind, "path": a.path, "focusable": true })
}

/// `asset.list { query?, kind?, limit? }`.
pub(crate) fn asset_list(world: &World, params: &Value) -> Result<Value, LiveError> {
    let query = params.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
    let kind = params.get("kind").and_then(Value::as_str).unwrap_or("");
    let limit = match params.get("limit") {
        None | Some(Value::Null) => 200,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=1000).contains(n))
            .ok_or_else(|| LiveError::new("invalid_value", "limit must be a whole number from 1 to 1000"))?
            as usize,
    };
    let all: Vec<Value> = world.try_res::<FocusAssets>().map_or_else(Vec::new, |l| {
        l.iter()
            .filter(|a| kind.is_empty() || a.kind == kind)
            .filter(|a| query.is_empty() || a.id.to_lowercase().contains(&query) || a.label.to_lowercase().contains(&query))
            .map(asset_record)
            .collect()
    });
    let total = all.len();
    let assets: Vec<Value> = all.into_iter().take(limit).collect();
    Ok(json!({ "assets": assets, "total": total, "focused": focused_id(world) }))
}

/// `asset.focus { id, view? }`: show one asset alone (the default view is the
/// turntable).
pub(crate) fn asset_focus(world: &World, params: &Value) -> Result<Value, LiveError> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("invalid_value", "id must be an asset id from asset.list").with(json!({ "field": "id" })))?;
    let orbit = match params.get("view").and_then(Value::as_str) {
        None | Some("turntable") => false,
        Some("orbit") => true,
        Some(_) => {
            return Err(LiveError::new("invalid_value", "view must be turntable or orbit").with(json!({ "field": "view" })));
        }
    };
    // Same asset again: change only the view (the camera stays where it is).
    let again = world.try_res::<FocusState>().is_some_and(|s| s.active.as_deref() == Some(id));
    let mut p = json!({ "asset": id, "spin_deg_s": if orbit { 0.0 } else { evani_core::focus::DEFAULT_SPIN_DEG_S } });
    if !again {
        p["pitch_deg"] = json!(evani_core::focus::DEFAULT_PITCH_DEG);
    }
    set(world, &p)?;
    let a = world.res::<FocusAssets>().get(id).map(asset_record).unwrap_or(Value::Null);
    let mut out = a;
    out["view"] = json!(if orbit { "orbit" } else { "turntable" });
    Ok(out)
}

/// `asset.clear`: back to the game (fine when nothing is focused).
pub(crate) fn asset_clear(world: &World) -> Value {
    clear(world);
    json!({ "focused": null })
}

/// The focus state as JSON (`null` fields when nothing is focused).
pub(crate) fn state_json(world: &World) -> Value {
    let Some(st) = world.try_res::<FocusState>() else {
        return json!({ "active": null });
    };
    let Some(id) = st.active.clone() else {
        return json!({ "active": null });
    };
    let frame = st.last.map(|f| {
        json!({
            "center": f.center.map(round3), "radius": round3(f.radius),
            "front_yaw_deg": round3(f.front_yaw_deg),
            "distance": round3(st.distance_for(f.radius)),
        })
    });
    let params = focused_params(world).map(|(_, p)| p).unwrap_or_default();
    json!({
        "active": id,
        "yaw_deg": round3(st.yaw_deg),
        "pitch_deg": round3(st.pitch_deg),
        "distance": st.distance.map(round3),
        "spin_deg_s": round3(st.spin_deg_s),
        "fov_deg": round3(st.fov_deg),
        "paused": st.paused,
        "backdrop": st.backdrop,
        "instance": st.instance,
        "drawn": st.frames_drawn > 0,
        "frame": frame,
        "params": params,
    })
}

/// `focus.list`.
pub(crate) fn list(world: &World) -> Value {
    let assets: Vec<Value> = world.try_res::<FocusAssets>().map_or_else(Vec::new, |l| {
        l.iter().map(|a| asset_json(world, a)).collect()
    });
    json!({ "assets": assets, "focus": state_json(world) })
}

fn ids(world: &World) -> Vec<String> {
    world
        .try_res::<FocusAssets>()
        .map_or_else(Vec::new, |l| l.iter().map(|a| a.id.clone()).collect())
}

fn num(params: &Value, k: &str, lo: f32, hi: f32) -> Result<Option<f32>, LiveError> {
    let Some(v) = params.get(k) else {
        return Ok(None);
    };
    let x = v
        .as_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| LiveError::new("invalid_value", format!("{k} must be a finite number")))?
        as f32;
    if !(lo..=hi).contains(&x) {
        return Err(LiveError::new("invalid_value", format!("{k} must be in {lo}..{hi}, got {x}"))
            .with(json!({ "field": k, "min": lo, "max": hi })));
    }
    Ok(Some(x))
}

fn flag(params: &Value, k: &str) -> Result<Option<bool>, LiveError> {
    params.get(k).map_or(Ok(None), |v| {
        v.as_bool()
            .map(Some)
            .ok_or_else(|| LiveError::new("invalid_value", format!("{k} must be true or false")))
    })
}

/// `focus.set`: focus an asset and/or move the turntable. Everything is
/// validated before anything changes.
pub(crate) fn set(world: &World, params: &Value) -> Result<Value, LiveError> {
    if !world.has_resource::<FocusState>() {
        return Err(LiveError::new("internal", "focus state not installed"));
    }
    let asset = match params.get("asset") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(LiveError::new("invalid_value", "asset must be a string")),
    };
    if let Some(id) = &asset
        && world.try_res::<FocusAssets>().is_none_or(|l| l.get(id).is_none())
    {
        let available = ids(world);
        let did_you_mean: Vec<&String> = available
            .iter()
            .filter(|a| a.contains(id.as_str()) || id.contains(a.as_str()))
            .collect();
        return Err(LiveError::new("unknown_target", format!("no focus asset {id}"))
            .with(json!({ "available": available, "did_you_mean": did_you_mean })));
    }
    if asset.is_none() && world.res::<FocusState>().active.is_none() {
        return Err(LiveError::new("not_focused", "nothing is focused: give asset")
            .with(json!({ "available": ids(world) })));
    }
    let yaw = params
        .get("yaw_deg")
        .map(|v| {
            v.as_f64()
                .filter(|x| x.is_finite())
                .map(|x| x as f32)
                .ok_or_else(|| LiveError::new("invalid_value", "yaw_deg must be a finite number"))
        })
        .transpose()?;
    let pitch = num(params, "pitch_deg", -89.0, 89.0)?;
    let distance = match params.get("distance") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(_) => Some(num(params, "distance", 0.05, 10_000.0)?),
    };
    let spin = num(params, "spin_deg_s", -360.0, 360.0)?;
    let fov = num(params, "fov_deg", 5.0, 150.0)?;
    let paused = flag(params, "paused")?;
    let backdrop = flag(params, "backdrop")?;
    let instance = match params.get("instance") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(Some(v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| {
            LiveError::new("invalid_value", "instance must be a whole number >= 0 or null")
        })?)),
    };
    {
        let mut st = world.res_mut::<FocusState>();
        if let Some(id) = &asset
            && st.active.as_deref() != Some(id.as_str())
        {
            st.focus(id);
        }
        if let Some(y) = yaw {
            st.yaw_deg = y.rem_euclid(360.0);
        }
        if let Some(p) = pitch {
            st.pitch_deg = p;
        }
        if let Some(d) = distance {
            st.distance = d;
        }
        if let Some(s) = spin {
            st.spin_deg_s = s;
        }
        if let Some(f) = fov {
            st.fov_deg = f;
        }
        if let Some(p) = paused {
            st.paused = p;
        }
        if let Some(b) = backdrop {
            st.backdrop = b;
        }
        if let Some(i) = instance {
            st.instance = i;
            st.last = None;
        }
    }
    Ok(state_json(world))
}

/// `focus.clear`: back to the full game (fine when nothing is focused).
pub(crate) fn clear(world: &World) -> Value {
    let was = world.try_res_mut::<FocusState>().and_then(|mut st| {
        let was = st.active.take();
        st.clear();
        was
    });
    json!({ "active": null, "was": was })
}

/// The page query's focus request: `focus=<id>` (or the older
/// `view=<id>`), with optional `instance=<n>` (or `box=<n>`).
pub fn query_focus(query: &str) -> Option<(String, Option<u32>)> {
    let mut id = None;
    let mut inst = None;
    for kv in query.trim_start_matches('?').split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "focus" | "view" if !v.is_empty() => id = Some(v.to_string()),
            "instance" | "box" => inst = v.parse().ok(),
            _ => {}
        }
    }
    id.map(|i| (i, inst))
}

/// Apply the page's `?focus=` at start (after every kit registered its
/// assets). An unknown id is reported in the overlay's error log.
pub fn start_from_query(world: &mut World, query: &str) {
    let Some((id, inst)) = query_focus(query) else {
        return;
    };
    let known = world.try_res::<FocusAssets>().is_some_and(|l| l.get(&id).is_some());
    if !known {
        let available = ids(world).join(", ");
        if let Some(mut log) = world.try_res_mut::<evani_core::overlay::ErrorLog>() {
            log.error(format!("live: ?focus={id}: no such asset (available: {available})"));
        }
        return;
    }
    if let Some(mut st) = world.try_res_mut::<FocusState>() {
        st.focus(&id);
        st.instance = inst;
    }
}

/// Wasm: mark the page while focused (`<html data-evani-focus="id">`) and
/// hide the game's DOM UI (`#ui` and anything with `data-evani-ui`), so
/// only the 3D view shows. Runs every frame; touches the DOM only on change.
///
/// In the asset view's orbit mode (a focus with `spin_deg_s` 0) a drag on the
/// page turns the camera round the asset and the wheel zooms; pointer lock is
/// never used. Events only add up here; the frame applies them.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub(crate) struct PageMark {
    shown: Option<String>,
    styled: bool,
    orbit: Option<Orbit>,
}

/// Drag and wheel input waiting for the next frame.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct Pending {
    /// Orbit mode is on: the listeners take drags and the wheel.
    on: bool,
    down: bool,
    dyaw: f32,
    dpitch: f32,
    zoom: f32,
}

#[cfg(target_arch = "wasm32")]
struct Orbit {
    pending: std::rc::Rc<std::cell::RefCell<Pending>>,
    _listeners: Vec<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
}

#[cfg(target_arch = "wasm32")]
impl Orbit {
    fn new(win: &web_sys::Window) -> Self {
        use wasm_bindgen::JsCast;
        let pending = std::rc::Rc::new(std::cell::RefCell::new(Pending::default()));
        let mut listeners = Vec::new();
        for name in ["pointerdown", "pointermove", "pointerup", "pointercancel", "wheel", "blur"] {
            let p = pending.clone();
            let cb = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
                let mut p = p.borrow_mut();
                if !p.on {
                    p.down = false;
                    return;
                }
                match ev.type_().as_str() {
                    "pointerdown" => p.down = true,
                    "pointerup" | "pointercancel" | "blur" => p.down = false,
                    "pointermove" => {
                        if p.down
                            && let Some(m) = ev.dyn_ref::<web_sys::MouseEvent>()
                        {
                            p.dyaw -= m.movement_x() as f32 * 0.4;
                            p.dpitch += m.movement_y() as f32 * 0.3;
                        }
                    }
                    "wheel" => {
                        if let Some(w) = ev.dyn_ref::<web_sys::WheelEvent>() {
                            p.zoom += w.delta_y() as f32;
                        }
                    }
                    _ => {}
                }
            });
            let _ = win.add_event_listener_with_callback(name, cb.as_ref().unchecked_ref());
            listeners.push(cb);
        }
        Self { pending, _listeners: listeners }
    }
}

#[cfg(target_arch = "wasm32")]
impl PageMark {
    pub(crate) fn frame(&mut self, world: &World) {
        self.orbit_input(world);
        let now = world.try_res::<FocusState>().and_then(|s| s.active.clone());
        if now == self.shown {
            return;
        }
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Some(html) = doc.document_element() else {
            return;
        };
        if !self.styled
            && let Ok(style) = doc.create_element("style")
        {
            style.set_text_content(Some(
                "html[data-evani-focus] #ui, html[data-evani-focus] [data-evani-ui] { display: none !important; }",
            ));
            let _ = html.append_child(&style);
            self.styled = true;
        }
        match &now {
            Some(id) => {
                let _ = html.set_attribute("data-evani-focus", id);
            }
            None => {
                let _ = html.remove_attribute("data-evani-focus");
            }
        }
        self.shown = now;
    }

    /// Turn drags and the wheel into the focus camera (orbit mode only).
    fn orbit_input(&mut self, world: &World) {
        let Some(mut st) = world.try_res_mut::<FocusState>() else {
            return;
        };
        let orbiting = st.active.is_some() && st.spin_deg_s == 0.0;
        if self.orbit.is_none() {
            if !orbiting {
                return;
            }
            let Some(win) = web_sys::window() else {
                return;
            };
            self.orbit = Some(Orbit::new(&win));
        }
        let Some(orbit) = &self.orbit else {
            return;
        };
        let mut p = orbit.pending.borrow_mut();
        p.on = orbiting;
        if !orbiting {
            *p = Pending::default();
            return;
        }
        if p.dyaw != 0.0 || p.dpitch != 0.0 {
            st.yaw_deg = (st.yaw_deg + p.dyaw).rem_euclid(360.0);
            st.pitch_deg = (st.pitch_deg + p.dpitch).clamp(-89.0, 89.0);
        }
        if p.zoom != 0.0 {
            let radius = st.last.map_or(1.0, |f| f.radius);
            let now = st.distance_for(radius);
            st.distance = Some((now * (p.zoom * 0.001).exp()).clamp(0.05, 10_000.0));
        }
        p.dyaw = 0.0;
        p.dpitch = 0.0;
        p.zoom = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_flags() {
        assert_eq!(query_focus("?live=1"), None);
        assert_eq!(query_focus("?live=1&focus=item_box"), Some(("item_box".into(), None)));
        assert_eq!(query_focus("?view=item_box&box=3"), Some(("item_box".into(), Some(3))));
        assert_eq!(query_focus("focus=kart&instance=1"), Some(("kart".into(), Some(1))));
        assert_eq!(query_focus("?focus="), None);
    }
}
