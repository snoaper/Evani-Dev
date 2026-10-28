//! The sandbox's minimal debug panel (no Look Lab: that is the Evani.dev
//! tab): a tier `<select>` (the tiers selectable now: Lite to Ultra in a
//! normal tab, VR only inside an immersive WebXR session), an Enter VR button
//! shown only when WebXR with WebGPU is available, and one checkbox per
//! compiled-in effect, top
//! right, next to the core overlay.
//!
//! It is an [`EngineModule`] that writes the same things the live API writes
//! (the active tier and the `fx.<name>.enabled` params) directly, so it works
//! in builds without the live module. DOM events only queue commands; they
//! are applied at the next frame boundary. The DOM is refreshed only when the
//! param registry, the tier, the budget, adaptive quality or XR changed.

// Natively the panel has no DOM, so the parts only the browser half (and
// the tests) call look unused there.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::effects::{BudgetStatus, EffectsStatus};
use evani_core::overlay::ErrorLog;
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::EngineModule;
use evani_core::quality::QualityState;
use evani_core::tiers::Tiers;

/// A queued panel action.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelCmd {
    Tier(String),
    Effect(String, bool),
    /// The Enter VR button (shown only when [`Tiers::xr_available`]).
    EnterVr,
}

pub type CmdQueue = Rc<RefCell<Vec<PanelCmd>>>;

/// What the DOM shows; it is redrawn only when this changes: param
/// revision, active tier, budget, adaptive-quality revision (an effect
/// paused or resumed), XR session and XR availability.
type Snap = (u64, usize, BudgetStatus, u64, bool, bool);

/// One effect checkbox: effect name and its `fx.<name>.enabled` param.
#[derive(Debug, Clone)]
pub struct Toggle {
    pub effect: String,
    pub param: ParamId,
}

#[derive(Default)]
pub struct DebugPanel {
    queue: CmdQueue,
    toggles: Vec<Toggle>,
    seen: Option<Snap>,
    #[cfg(target_arch = "wasm32")]
    dom: Option<dom::Dom>,
}

impl DebugPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `fx.<name>.enabled` param, i.e. every effect compiled in.
    fn find_toggles(world: &World) -> Vec<Toggle> {
        let reg = world.res::<ParamRegistry>();
        reg.defs()
            .iter()
            .filter_map(|d| {
                let effect = d.name.strip_prefix("fx.")?.strip_suffix(".enabled")?;
                Some(Toggle {
                    effect: effect.to_string(),
                    param: reg.id(&d.name)?,
                })
            })
            .collect()
    }

    /// Apply queued commands. Errors go to the overlay.
    pub fn apply(world: &World, cmds: Vec<PanelCmd>) {
        for c in cmds {
            match c {
                PanelCmd::Tier(id) => {
                    let r = world.res_mut::<Tiers>().try_set_active(&id);
                    if let Err(e) = r {
                        world
                            .res_mut::<ErrorLog>()
                            .error(format!("panel: tier {id}: {e}"));
                    }
                }
                PanelCmd::EnterVr => {
                    // The immersive session lives in the on-demand VR
                    // module (docs/VR_STATUS.md); M1 only detects support.
                    world
                        .res_mut::<ErrorLog>()
                        .note("Enter VR: the VR module is not in this build yet");
                }
                PanelCmd::Effect(name, on) => {
                    let p = format!("fx.{name}.enabled");
                    let r = world
                        .res_mut::<ParamRegistry>()
                        .set_by_name(&p, ParamValue::Bool(on));
                    if let Err(e) = r {
                        world
                            .res_mut::<ErrorLog>()
                            .error(format!("panel: {p}: {e}"));
                    }
                }
            }
        }
    }

    fn snapshot(world: &World) -> Snap {
        let tiers = world.res::<Tiers>();
        (
            world.res::<ParamRegistry>().revision(),
            tiers.active_index(),
            *world.res::<BudgetStatus>(),
            world.try_res::<QualityState>().map_or(0, |q| q.revision()),
            tiers.xr_session(),
            tiers.xr_available(),
        )
    }

    /// The tier `<select>` options now, as `(id, label)`: Lite to Ultra in
    /// a normal tab, the VR preset only inside an XR session.
    pub fn tier_options(world: &World) -> Vec<(String, String)> {
        world
            .res::<Tiers>()
            .selectable()
            .map(|(_, p)| (p.id.clone(), p.label.clone()))
            .collect()
    }

    /// Checkbox label: `bloom` plus the budget's verdict when fx reports one.
    pub fn label(world: &World, effect: &str, out: &mut String) {
        out.clear();
        out.push_str(effect);
        if let Some(fx) = world.try_res::<EffectsStatus>()
            && let Some(e) = fx.get(effect)
        {
            out.push_str(" (");
            out.push_str(e.state.as_str());
            out.push(')');
        }
    }

    pub fn is_on(world: &World, t: &Toggle) -> bool {
        matches!(
            world.res::<ParamRegistry>().value(t.param),
            Some(ParamValue::Bool(true))
        )
    }
}

impl EngineModule for DebugPanel {
    fn id(&self) -> &'static str {
        "evani.module.debug_panel"
    }

    fn label(&self) -> &'static str {
        "panel"
    }

    fn start(&mut self, world: &mut World) {
        self.toggles = Self::find_toggles(world);
        #[cfg(target_arch = "wasm32")]
        {
            match dom::Dom::create(world, &self.toggles, &self.queue) {
                Ok(d) => self.dom = Some(d),
                Err(e) => world.res_mut::<ErrorLog>().error(format!("panel: {e}")),
            }
        }
    }

    fn frame(&mut self, world: &mut World) {
        let cmds = core::mem::take(&mut *self.queue.borrow_mut());
        if !cmds.is_empty() {
            Self::apply(world, cmds);
        }
        let now = Self::snapshot(world);
        if self.seen == Some(now) {
            return;
        }
        self.seen = Some(now);
        #[cfg(target_arch = "wasm32")]
        if let Some(d) = &mut self.dom {
            d.sync(world, &self.toggles);
        }
    }

    fn status(&self, world: &World, out: &mut String) {
        out.push_str("tier ");
        out.push_str(&world.res::<Tiers>().active().id);
        out.push_str(" · ");
        let n = self.toggles.len();
        let _ = core::fmt::Write::write_fmt(out, format_args!("{n} effect toggles"));
    }
}

#[cfg(target_arch = "wasm32")]
mod dom {
    use evani_core::ecs::World;
    use evani_core::effects::BudgetStatus;
    use evani_core::tiers::Tiers;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{Document, Element, HtmlInputElement, HtmlSelectElement};

    use super::{CmdQueue, DebugPanel, PanelCmd, Toggle};

    pub const PANEL_ID: &str = "evani-panel";

    pub struct Dom {
        doc: Document,
        select: HtmlSelectElement,
        /// The tier ids the `<select>` lists now.
        listed: Vec<String>,
        enter_vr: Element,
        boxes: Vec<(HtmlInputElement, Element)>,
        budget: Element,
        scratch: String,
    }

    /// Fill `select` with the selectable tiers; returns their ids.
    fn fill_tiers(doc: &Document, select: &HtmlSelectElement, world: &World) -> Result<Vec<String>, String> {
        select.set_inner_html("");
        let opts = DebugPanel::tier_options(world);
        for (id, label) in &opts {
            let o = el(doc, "option")?;
            let _ = o.set_attribute("value", id);
            o.set_text_content(Some(label));
            append(select, &o)?;
        }
        select.set_value(&world.res::<Tiers>().active().id);
        Ok(opts.into_iter().map(|(id, _)| id).collect())
    }

    fn el(doc: &Document, tag: &str) -> Result<Element, String> {
        doc.create_element(tag)
            .map_err(|_| format!("cannot create <{tag}>"))
    }

    fn append(parent: &Element, child: &Element) -> Result<(), String> {
        parent
            .append_child(child)
            .map(|_| ())
            .map_err(|_| "cannot append panel element".to_string())
    }

    impl Dom {
        pub fn create(world: &World, toggles: &[Toggle], queue: &CmdQueue) -> Result<Self, String> {
            let doc = web_sys::window()
                .and_then(|w| w.document())
                .ok_or("no document")?;
            let body = doc.body().ok_or("no body")?;
            let root = el(&doc, "div")?;
            root.set_id(PANEL_ID);
            let _ = root.set_attribute(
                "style",
                "position:fixed;right:0;top:0;z-index:1001;padding:6px 8px;\
                 font:12px/1.5 ui-monospace,Consolas,monospace;color:#d8f0e8;\
                 background:rgba(0,0,0,0.6);border-bottom-left-radius:6px;\
                 display:flex;flex-direction:column;gap:2px",
            );

            // Tier select.
            let row = el(&doc, "label")?;
            row.set_text_content(Some("tier "));
            let select: HtmlSelectElement = el(&doc, "select")?
                .dyn_into()
                .map_err(|_| "select is not a <select>")?;
            let listed = fill_tiers(&doc, &select, world)?;
            {
                let (q, s) = (queue.clone(), select.clone());
                let cb = Closure::<dyn FnMut()>::new(move || {
                    q.borrow_mut().push(PanelCmd::Tier(s.value()));
                });
                let _ =
                    select.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
                cb.forget(); // lives as long as the page
            }
            append(&row, &select)?;
            append(&root, &row)?;

            // Enter VR: hidden unless WebXR with WebGPU is available.
            let enter_vr = el(&doc, "button")?;
            enter_vr.set_text_content(Some("Enter VR"));
            let _ = enter_vr.set_attribute("style", "display:none");
            {
                let q = queue.clone();
                let cb = Closure::<dyn FnMut()>::new(move || {
                    q.borrow_mut().push(PanelCmd::EnterVr);
                });
                let _ = enter_vr.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
                cb.forget();
            }
            append(&root, &enter_vr)?;

            // Effect toggles.
            let mut boxes = Vec::with_capacity(toggles.len());
            for t in toggles {
                let row = el(&doc, "label")?;
                let input: HtmlInputElement = el(&doc, "input")?
                    .dyn_into()
                    .map_err(|_| "input is not an <input>")?;
                input.set_type("checkbox");
                {
                    let (q, i, name) = (queue.clone(), input.clone(), t.effect.clone());
                    let cb = Closure::<dyn FnMut()>::new(move || {
                        q.borrow_mut()
                            .push(PanelCmd::Effect(name.clone(), i.checked()));
                    });
                    let _ = input
                        .add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
                    cb.forget();
                }
                let text = el(&doc, "span")?;
                append(&row, &input)?;
                append(&row, &text)?;
                append(&root, &row)?;
                boxes.push((input, text));
            }
            if toggles.is_empty() {
                let none = el(&doc, "span")?;
                none.set_text_content(Some("no effects in this build"));
                append(&root, &none)?;
            }

            let budget = el(&doc, "span")?;
            append(&root, &budget)?;
            body.append_child(&root)
                .map_err(|_| "cannot add the panel to the page")?;
            Ok(Self {
                doc,
                select,
                listed,
                enter_vr,
                boxes,
                budget,
                scratch: String::with_capacity(64),
            })
        }

        /// Reflect the world (tier, toggles, budget) in the DOM.
        pub fn sync(&mut self, world: &World, toggles: &[Toggle]) {
            // Entering or leaving an XR session changes what is selectable.
            let now: Vec<String> = DebugPanel::tier_options(world).into_iter().map(|(id, _)| id).collect();
            if now != self.listed {
                if let Ok(l) = fill_tiers(&self.doc, &self.select, world) {
                    self.listed = l;
                }
            } else {
                // Select by id, never by index (the list skips VR).
                self.select.set_value(&world.res::<Tiers>().active().id);
            }
            let show = world.res::<Tiers>().xr_available() && !world.res::<Tiers>().xr_session();
            let _ = self
                .enter_vr
                .set_attribute("style", if show { "" } else { "display:none" });
            for (t, (input, text)) in toggles.iter().zip(&self.boxes) {
                input.set_checked(DebugPanel::is_on(world, t));
                DebugPanel::label(world, &t.effect, &mut self.scratch);
                text.set_text_content(Some(&self.scratch));
            }
            let b = *world.res::<BudgetStatus>();
            self.scratch.clear();
            let _ = core::fmt::Write::write_fmt(
                &mut self.scratch,
                format_args!("points {}/{}", b.used, b.total),
            );
            self.budget.set_text_content(Some(&self.scratch));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_app;

    #[test]
    fn tier_command_switches_tier() {
        let mut app = build_app();
        app.run_headless(2, 16.0);
        DebugPanel::apply(&app.world, vec![PanelCmd::Tier("low".into())]);
        app.run_headless(1, 16.0);
        assert_eq!(app.world.res::<Tiers>().active().id, "low");
        DebugPanel::apply(&app.world, vec![PanelCmd::Tier("potato".into())]);
        assert!(app.world.res::<ErrorLog>().error_count() > 0);
    }

    #[test]
    fn tier_list_offers_vr_only_in_an_xr_session() {
        let mut app = build_app();
        app.run_headless(2, 16.0);
        let ids = |app: &evani_core::App| -> Vec<String> {
            DebugPanel::tier_options(&app.world).into_iter().map(|(id, _)| id).collect()
        };
        assert_eq!(ids(&app), ["lite", "low", "medium", "high", "ultra"]);
        let errors = app.world.res::<ErrorLog>().error_count();
        DebugPanel::apply(&app.world, vec![PanelCmd::Tier("vr".into())]);
        assert_ne!(app.world.res::<Tiers>().active().id, "vr");
        assert!(app.world.res::<ErrorLog>().error_count() > errors);
        // The panel redraws when an effect is paused or the XR session starts.
        let before = DebugPanel::snapshot(&app.world);
        app.world
            .res_mut::<QualityState>()
            .apply(evani_core::quality::AqAction::ShedEffect("bloom"));
        let paused = DebugPanel::snapshot(&app.world);
        assert_ne!(paused, before);
        let before = paused;
        app.world.res_mut::<Tiers>().set_xr_session(true);
        assert!(ids(&app).contains(&"vr".to_string()));
        assert_ne!(DebugPanel::snapshot(&app.world), before);
    }

    #[cfg(feature = "vignette")]
    #[test]
    fn effect_command_sets_enabled_param() {
        let mut app = build_app();
        app.run_headless(2, 16.0);
        DebugPanel::apply(&app.world, vec![PanelCmd::Effect("vignette".into(), false)]);
        app.run_headless(1, 16.0);
        let reg = app.world.res::<ParamRegistry>();
        let id = reg.id("fx.vignette.enabled").unwrap();
        assert_eq!(reg.value(id), Some(ParamValue::Bool(false)));
    }

    #[cfg(feature = "bloom")]
    #[test]
    fn label_and_state_follow_params() {
        let mut app = build_app();
        app.run_headless(2, 16.0);
        let t = DebugPanel::find_toggles(&app.world);
        let bloom = t.iter().find(|t| t.effect == "bloom").unwrap();
        let mut s = String::new();
        DebugPanel::label(&app.world, "bloom", &mut s);
        assert!(s.starts_with("bloom ("), "{s}");
        DebugPanel::apply(&app.world, vec![PanelCmd::Effect("bloom".into(), false)]);
        assert!(!DebugPanel::is_on(&app.world, bloom));
    }

    #[test]
    fn finds_one_toggle_per_compiled_effect() {
        let mut app = build_app();
        app.run_headless(1, 16.0);
        let t = DebugPanel::find_toggles(&app.world);
        assert_eq!(
            t.iter().any(|t| t.effect == "bloom"),
            cfg!(feature = "bloom")
        );
        assert_eq!(
            t.iter().any(|t| t.effect == "vignette"),
            cfg!(feature = "vignette")
        );
    }
}
