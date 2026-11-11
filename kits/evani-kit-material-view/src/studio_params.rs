//! The studio's registered params (WP1 step 1): every control each style
//! lists, as `kit.material_view.studio.<style>.<control>` over the control's
//! range and step, defaulting to the style's own value; and the variations
//! strip's size.
//!
//! Two ways, one value: a param moved (the panel or evani-live `param_set`)
//! sets that style's control in the record's state; the state's controls are
//! written back to the params every frame they differ. A control back on the
//! style's own value is dropped from the state, so a record's cache keys
//! never depend on whether a slider was touched.

use std::collections::BTreeMap;

use evani_core::ecs::World;
use evani_core::params::{F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};
use evani_studio::styles::{control, control_param};

use super::{Catalogs, Studio, StudioState};

/// `kit.material_view.studio.variations`: tiles in the variations strip.
pub const VARIATIONS: &str = "kit.material_view.studio.variations";
/// `kit.material_view.studio.variations_marked`: of those, at most this many
/// marked "doesn't go on it" (so the strip shows what can't, without filling
/// up with it).
pub const VARIATIONS_MARKED: &str = "kit.material_view.studio.variations_marked";

/// One registered control.
#[derive(Debug, Clone)]
pub struct ControlParam {
    pub style: String,
    pub control: String,
    pub handle: F32Param,
    pub default: f32,
}

/// The studio's param handles (a world resource).
#[derive(Debug, Clone)]
pub struct StudioParams {
    pub controls: Vec<ControlParam>,
    pub variations: I32Param,
    pub variations_marked: I32Param,
    /// The registry revision after the studio's last frame: a control param
    /// changed after it was moved by someone else.
    seen: u64,
}

impl StudioParams {
    /// The variations strip's size and its marked share, from the params.
    pub fn strip(&self, reg: &ParamRegistry) -> (usize, usize) {
        let n = |h: I32Param| usize::try_from(reg.i32(h)).unwrap_or(0);
        (n(self.variations), n(self.variations_marked))
    }
}

/// Register every (style, control) pair and the strip's params.
pub fn register(ctx: &mut PluginContext, c: &Catalogs) -> Result<StudioParams, PluginError> {
    let mut controls = Vec::new();
    for s in &c.styles.styles {
        for id in &s.controls {
            let Some(def) = control(id) else { continue };
            let default = s.control_default(id).unwrap_or(def.min).clamp(def.min, def.max);
            let name = control_param(&s.id, id);
            let handle = ctx.param_f32(ParamDef::f32(&name, default).range(def.min, def.max).step(def.step).describe(&format!("{}: {} ({})", s.name, def.label, def.what)))?;
            controls.push(ControlParam { style: s.id.clone(), control: id.clone(), handle, default });
        }
    }
    let variations = ctx.param_i32(ParamDef::i32(VARIATIONS, 12).range(4.0, 32.0).describe("Tiles in the studio's variations strip: the same asset and style in other materials."))?;
    let variations_marked = ctx.param_i32(
        ParamDef::i32(VARIATIONS_MARKED, 3).range(0.0, 8.0).describe("Of the variations tiles, at most this many show a material that doesn't go on the asset (greyed, with the reason)."),
    )?;
    Ok(StudioParams { controls, variations, variations_marked, seen: 0 })
}

fn same(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-6
}

/// Step 1: a control param moved by someone else sets its style's control.
pub fn read(world: &mut World, c: &Catalogs) {
    let Some(p) = world.try_res::<StudioParams>().map(|p| p.clone()) else { return };
    let moved: Vec<(String, String, f32)> = {
        let reg = world.res::<ParamRegistry>();
        if p.seen == 0 || reg.revision() == p.seen {
            return;
        }
        p.controls.iter().filter(|cp| reg.changed_since(cp.handle.id(), p.seen)).map(|cp| (cp.style.clone(), cp.control.clone(), reg.f32(cp.handle))).collect()
    };
    if moved.is_empty() {
        return;
    }
    let mut s = world.res_mut::<Studio>();
    for (style, id, v) in moved {
        if set_style_control(&mut s.state, c, &style, &id, v) {
            s.state.revision += 1;
        }
    }
}

/// Set `style`'s control `id` (any style, not only the current one), clamped;
/// the style's own value removes it. True when the state changed.
pub fn set_style_control(state: &mut StudioState, c: &Catalogs, style: &str, id: &str, value: f32) -> bool {
    let (Some(st), Some(def)) = (c.styles.style(style), control(id)) else { return false };
    if !value.is_finite() || !st.controls.iter().any(|x| x == id) {
        return false;
    }
    let v = value.clamp(def.min, def.max);
    let own = st.control_default(id);
    let before = state.controls.get(style).and_then(|m| m.get(id)).copied();
    if own.is_some_and(|d| same(d, v)) {
        if let Some(m) = state.controls.get_mut(style) {
            m.remove(id);
            if m.is_empty() {
                state.controls.remove(style);
            }
        }
    } else {
        state.controls.entry(style.to_string()).or_default().insert(id.to_string(), v);
    }
    before != state.controls.get(style).and_then(|m| m.get(id)).copied()
}

/// After the commands: a control set back on its style's own value (by a
/// `Control` command) is dropped, so the keys stay canonical.
pub fn prune(state: &mut StudioState, c: &Catalogs) {
    let mut drop: Vec<(String, String)> = Vec::new();
    for (style, m) in &state.controls {
        let Some(st) = c.styles.style(style) else { continue };
        for (id, v) in m {
            if st.control_default(id).is_some_and(|d| same(d, *v)) {
                drop.push((style.clone(), id.clone()));
            }
        }
    }
    for (style, id) in drop {
        if let Some(m) = state.controls.get_mut(&style) {
            m.remove(&id);
        }
    }
    state.controls.retain(|_, m| !m.is_empty());
}

/// Step 3: write the state's controls to their params (the style's own
/// value where the state holds none).
pub fn write(world: &mut World, _c: &Catalogs) {
    let Some(p) = world.try_res::<StudioParams>().map(|p| p.clone()) else { return };
    let want: BTreeMap<String, BTreeMap<String, f32>> = world.res::<Studio>().state.controls.clone();
    let mut reg = world.res_mut::<ParamRegistry>();
    for cp in &p.controls {
        let v = want.get(&cp.style).and_then(|m| m.get(&cp.control)).copied().unwrap_or(cp.default);
        if !same(reg.f32(cp.handle), v) {
            let _ = reg.set_f32(cp.handle, v);
        }
    }
}

/// The end of the frame: what the studio wrote is not someone else's move.
pub fn settle(world: &mut World) {
    let rev = world.res::<ParamRegistry>().revision();
    if let Some(mut p) = world.try_res_mut::<StudioParams>() {
        p.seen = rev.max(1);
    }
}
