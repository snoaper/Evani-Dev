//! Test hooks on `window.__engine` (feature `hooks`, dev builds only):
//!
//! * `slotRebuild(name, body)`: a Promise of the slot compile result (the
//!   `SlotCompileResult` JSON: `{ok, ms, kept, messages, pipelines_rebuilt}`).
//!   It goes through the same `SlotRebuilds` API evani-live will use.
//! * `loseDevice()`: destroy the GPU device on the next frame, to exercise
//!   device-lost recovery.
//! * `paramSet(name, value)`: set a registered param on the next frame (a
//!   number, a bool or an array of three numbers, by the param's type), so a
//!   headless shot script can try values without a rebuild. Returns false at
//!   once for a value of no param type; an unknown name is skipped.
//! * `pipelinesBusy()`: true while a shader compile, a pipeline build or a
//!   slot rebuild is queued or running (a look param that changes a shader
//!   body only shows once its rebuild is in, which can take tens of seconds
//!   on a big shader), so a shot script waits for false before it captures.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::slots::{RebuildHandle, ShaderSlots, SlotRebuilds};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

struct HookReq {
    slot: String,
    body: String,
    resolve: js_sys::Function,
    handle: Option<RebuildHandle>,
}

thread_local! {
    /// The renderer's last word on pipeline work (`pipelinesBusy()`).
    static BUSY: Cell<bool> = const { Cell::new(true) };
}

/// Called once a frame by the renderer: whether pipeline work is queued or
/// running (shader compiles, pipeline builds, slot rebuilds).
pub fn note_busy(busy: bool) {
    BUSY.with(|b| b.set(busy));
}

#[derive(Default)]
pub struct Hooks {
    queue: Rc<RefCell<Vec<HookReq>>>,
    lose: Rc<Cell<bool>>,
    params: Rc<RefCell<Vec<(String, JsValue)>>>,
    keep: Vec<JsValue>,
}

/// A JS value as the value of a param of type `like` (a number, a bool or
/// three numbers); `None` when it fits no such shape.
fn param_value(like: ParamValue, v: &JsValue) -> Option<ParamValue> {
    let three = || -> Option<[f32; 3]> {
        let a = js_sys::Array::from(v);
        (js_sys::Array::is_array(v) && a.length() == 3).then(|| std::array::from_fn(|i| a.get(i as u32).as_f64().unwrap_or(0.0) as f32))
    };
    Some(match like {
        ParamValue::F32(_) => ParamValue::F32(v.as_f64()? as f32),
        ParamValue::I32(_) => ParamValue::I32(v.as_f64()? as i32),
        ParamValue::Enum(_) => ParamValue::Enum(v.as_f64()? as u32),
        ParamValue::Bool(_) => ParamValue::Bool(v.as_bool()?),
        ParamValue::Vec3(_) => ParamValue::Vec3(three()?),
        ParamValue::Color(_) => ParamValue::Color(three()?),
    })
}

impl Hooks {
    pub fn install(&mut self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let key = JsValue::from_str("__engine");
        let obj = match js_sys::Reflect::get(&window, &key) {
            Ok(v) if v.is_object() => v,
            _ => {
                let o: JsValue = js_sys::Object::new().into();
                let _ = js_sys::Reflect::set(&window, &key, &o);
                o
            }
        };
        let queue = self.queue.clone();
        let rebuild = Closure::<dyn FnMut(String, String) -> js_sys::Promise>::new(move |slot: String, body: String| {
            let queue = queue.clone();
            let mut slot = Some(slot);
            let mut body = Some(body);
            js_sys::Promise::new(&mut |resolve, _reject| {
                queue.borrow_mut().push(HookReq {
                    slot: slot.take().unwrap_or_default(),
                    body: body.take().unwrap_or_default(),
                    resolve,
                    handle: None,
                });
            })
        });
        let lose = self.lose.clone();
        let lose_fn = Closure::<dyn FnMut()>::new(move || lose.set(true));
        let params = self.params.clone();
        let param_fn = Closure::<dyn FnMut(String, JsValue) -> bool>::new(move |name: String, v: JsValue| {
            let ok = v.as_f64().is_some() || v.as_bool().is_some() || js_sys::Array::is_array(&v);
            if ok {
                params.borrow_mut().push((name, v));
            }
            ok
        });
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("paramSet"), param_fn.as_ref().unchecked_ref());
        self.keep.push(param_fn.into_js_value());
        let busy_fn = Closure::<dyn FnMut() -> bool>::new(|| BUSY.with(Cell::get));
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("pipelinesBusy"), busy_fn.as_ref().unchecked_ref());
        self.keep.push(busy_fn.into_js_value());
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("slotRebuild"), rebuild.as_ref().unchecked_ref());
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("loseDevice"), lose_fn.as_ref().unchecked_ref());
        self.keep.push(rebuild.into_js_value());
        self.keep.push(lose_fn.into_js_value());
    }

    /// Queue new requests, resolve finished ones. Returns true when
    /// `loseDevice()` was called since the last frame.
    pub fn poll(&mut self, world: &mut World) -> bool {
        // Converted here (a read), set by the render module's system.
        let pending = std::mem::take(&mut *self.params.borrow_mut());
        if !pending.is_empty() {
            let vals: Vec<(String, ParamValue)> = match world.try_res::<ParamRegistry>() {
                Some(reg) => pending.iter().filter_map(|(n, v)| reg.id(n).and_then(|id| reg.value(id)).and_then(|like| param_value(like, v)).map(|x| (n.clone(), x))).collect(),
                None => Vec::new(),
            };
            if let Some(mut q) = world.try_res_mut::<crate::param_hooks::ParamSets>() {
                q.0.extend(vals);
            }
        }
        let mut q = self.queue.borrow_mut();
        if !q.is_empty() {
            let now = world.res::<evani_core::time::Time>().now_ms;
            q.retain_mut(|r| {
                let Some(h) = r.handle else {
                    let res = {
                        let slots = world.res::<ShaderSlots>();
                        world.res_mut::<SlotRebuilds>().request(&slots, &r.slot, &r.body, now)
                    };
                    match res {
                        Ok(h) => {
                            r.handle = Some(h);
                            return true;
                        }
                        Err(e) => {
                            let v = serde_json::json!({
                                "ok": false, "ms": 0.0, "kept": "previous",
                                "messages": [{"severity": "error", "text": e.to_string(),
                                              "line": 0, "column": 0, "length": 0, "source_line": null}],
                                "pipelines_rebuilt": []
                            });
                            resolve(&r.resolve, &v.to_string());
                            return false;
                        }
                    }
                };
                match world.res_mut::<SlotRebuilds>().take_result(h) {
                    Some(res) => {
                        let s = serde_json::to_string(&res).unwrap_or_else(|_| "{\"ok\":false}".into());
                        resolve(&r.resolve, &s);
                        false
                    }
                    None => true,
                }
            });
        }
        self.lose.replace(false)
    }
}

fn resolve(f: &js_sys::Function, json: &str) {
    let v = js_sys::JSON::parse(json).unwrap_or_else(|_| JsValue::from_str(json));
    let _ = f.call1(&JsValue::NULL, &v);
}
