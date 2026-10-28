//! VR module (`evani.module.xr`). M1 stub: feature-detect WebXR
//! (`navigator.xr`, `isSessionSupported("immersive-vr")`) and WebGPU-backed
//! XR layers (`XRGPUBinding`), register the VR tunables and report in the
//! overlay. The real module will be a separate wasm file fetched only when
//! the player presses Enter VR (see DECISIONS.md).

use evani_core::ecs::World;
use evani_core::params::{BakeTarget, EnumParam, F32Param, ParamDef};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use evani_core::tiers::Tiers;

/// What the browser offers for VR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XrSupport {
    /// `navigator.xr` exists.
    pub webxr: bool,
    /// `XRGPUBinding` exists (WebXR layers rendered with WebGPU).
    pub webgpu_binding: bool,
    /// `isSessionSupported("immersive-vr")`: `None` while checking.
    pub immersive_vr: Option<bool>,
}

impl XrSupport {
    /// Can this browser run Evani.Dev in VR?
    pub fn ready(&self) -> bool {
        self.webxr && self.webgpu_binding && self.immersive_vr == Some(true)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct XrParams {
    pub render_scale: F32Param,
    pub reference_space: EnumParam,
}

#[derive(Default)]
pub struct XrModule {
    params: Option<XrParams>,
    support: std::rc::Rc<std::cell::Cell<XrSupport>>,
}

impl XrModule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn support(&self) -> XrSupport {
        self.support.get()
    }
}

#[cfg(target_arch = "wasm32")]
fn detect(cell: std::rc::Rc<std::cell::Cell<XrSupport>>) {
    use js_sys::{Function, Promise, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    let Some(win) = web_sys::window() else { return };
    let nav = win.navigator();
    let xr = Reflect::get(&nav, &JsValue::from_str("xr"))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null());
    let mut s = XrSupport {
        webxr: xr.is_some(),
        webgpu_binding: Reflect::has(&win, &JsValue::from_str("XRGPUBinding")).unwrap_or(false),
        immersive_vr: None,
    };
    let promise = xr.as_ref().and_then(|xr| {
        let f = Reflect::get(xr, &JsValue::from_str("isSessionSupported"))
            .ok()?
            .dyn_into::<Function>()
            .ok()?;
        f.call1(xr, &JsValue::from_str("immersive-vr"))
            .ok()?
            .dyn_into::<Promise>()
            .ok()
    });
    match promise {
        None => s.immersive_vr = Some(false),
        Some(p) => {
            let cell = cell.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let r = wasm_bindgen_futures::JsFuture::from(p).await;
                let mut s = cell.get();
                s.immersive_vr = Some(r.ok().and_then(|v| v.as_bool()).unwrap_or(false));
                cell.set(s);
            });
        }
    }
    cell.set(s);
}

#[cfg(not(target_arch = "wasm32"))]
fn detect(cell: std::rc::Rc<std::cell::Cell<XrSupport>>) {
    cell.set(XrSupport {
        immersive_vr: Some(false),
        ..XrSupport::default()
    });
}

impl EngineModule for XrModule {
    fn id(&self) -> &'static str {
        "evani.module.xr"
    }

    fn label(&self) -> &'static str {
        "xr"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(XrParams {
            render_scale: ctx.param_f32(
                ParamDef::f32("xr.render_scale", 1.0)
                    .range(0.5, 1.0)
                    .step(0.05)
                    .unit("x")
                    .describe("Per-eye resolution scale in VR.")
                    .owner("xr")
                    .bake(BakeTarget::Tier),
            )?,
            reference_space: ctx.param_enum(
                ParamDef::enumeration("xr.reference_space", &["local-floor", "bounded-floor", "local"], 0)
                    .describe("WebXR reference space requested on Enter VR.")
                    .owner("xr")
                    .bake(BakeTarget::Kit),
            )?,
        });
        Ok(())
    }

    fn start(&mut self, _world: &mut World) {
        detect(self.support.clone());
    }

    /// Tell the tier table whether Enter VR can be offered (WebXR with a
    /// WebGPU binding and immersive-vr supported).
    fn frame(&mut self, world: &mut World) {
        let ready = self.support.get().ready();
        if let Some(mut t) = world.try_res_mut::<Tiers>()
            && t.xr_available() != ready
        {
            t.set_xr_available(ready);
        }
    }

    fn status(&self, _world: &World, out: &mut String) {
        let s = self.support.get();
        out.push_str(if s.webxr { "webxr yes" } else { "webxr no" });
        out.push_str(if s.webgpu_binding {
            " · XRGPUBinding yes"
        } else {
            " · XRGPUBinding no"
        });
        out.push_str(match s.immersive_vr {
            None => " · immersive-vr checking",
            Some(true) => " · immersive-vr yes",
            Some(false) => " · immersive-vr no",
        });
        out.push_str(" (stub)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;
    use evani_core::params::ParamRegistry;

    #[test]
    fn native_reports_no_vr() {
        let mut a = App::new("t");
        a.add_module(Box::new(XrModule::new()));
        a.run_headless(1, 16.0);
        assert!(a.world.res::<ParamRegistry>().id("xr.render_scale").is_some());
        let m = XrModule::new();
        m.support.set(XrSupport::default());
        detect(m.support.clone());
        assert!(!m.support().ready());
        let mut s = String::new();
        m.status(&a.world, &mut s);
        assert!(s.contains("immersive-vr no"), "{s}");
        assert!(!a.world.res::<Tiers>().xr_available());
    }

    #[test]
    fn enter_vr_offered_only_when_ready() {
        let m = XrModule::new();
        let support = m.support.clone();
        let mut a = App::new("t");
        a.add_module(Box::new(m));
        a.run_headless(2, 16.0);
        assert!(!a.world.res::<Tiers>().xr_available());
        support.set(XrSupport {
            webxr: true,
            webgpu_binding: true,
            immersive_vr: Some(true),
        });
        a.run_headless(1, 16.0);
        assert!(a.world.res::<Tiers>().xr_available());
        support.set(XrSupport::default());
        a.run_headless(1, 16.0);
        assert!(!a.world.res::<Tiers>().xr_available());
    }
}
