//! Test hooks on `window.__engine` (feature `hooks`, dev builds only):
//!
//! * `slotRebuild(name, body)`: a Promise of the slot compile result (the
//!   `SlotCompileResult` JSON: `{ok, ms, kept, messages, pipelines_rebuilt}`).
//!   It goes through the same `SlotRebuilds` API evani-live will use.
//! * `loseDevice()`: destroy the GPU device on the next frame, to exercise
//!   device-lost recovery.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::slots::{RebuildHandle, ShaderSlots, SlotRebuilds};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

struct HookReq {
    slot: String,
    body: String,
    resolve: js_sys::Function,
    handle: Option<RebuildHandle>,
}

#[derive(Default)]
pub struct Hooks {
    queue: Rc<RefCell<Vec<HookReq>>>,
    lose: Rc<Cell<bool>>,
    keep: Vec<JsValue>,
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
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("slotRebuild"), rebuild.as_ref().unchecked_ref());
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("loseDevice"), lose_fn.as_ref().unchecked_ref());
        self.keep.push(rebuild.into_js_value());
        self.keep.push(lose_fn.into_js_value());
    }

    /// Queue new requests, resolve finished ones. Returns true when
    /// `loseDevice()` was called since the last frame.
    pub fn poll(&mut self, world: &mut World) -> bool {
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
