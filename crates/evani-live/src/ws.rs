//! The page's connection to the live bridge (`/engine`, docs/LIVE_API.md
//! 3.2 and 4.2). Browser only.
//!
//! - Opens only when the bridge served the page: it reads the token from
//!   `<meta name="evani-live-token">` (4.3). No meta, or `?live=0` in the
//!   address, means no socket at all.
//! - First message is `{"type":"auth","v":1,"token":…}`; the `hello`
//!   (game, schema_version, tier) follows at the next frame, where the
//!   world is at hand.
//! - Each incoming `cmd` joins the same queue as `window.evani.live.send`,
//!   so it is answered at a frame boundary; the reply goes back on the
//!   socket it came from.
//! - Reconnects with backoff (1 s doubling to 30 s), except after close
//!   code 4401 (wrong token) or 4000 (a newer page took over).
//! - Never fails the page: a blocked socket just means no live control
//!   (Evani.App's hidden check copy blocks sockets). Nothing goes to
//!   `console.error`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::ecs::World;
use js_sys::Function;
use serde_json::json;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use super::page::{Listeners, Queue, Responder};

/// Close codes after which the page must not reconnect.
const CLOSE_UNAUTHORIZED: u16 = 4401;
const CLOSE_REPLACED: u16 = 4000;
const BACKOFF_MIN_MS: i32 = 1_000;
const BACKOFF_MAX_MS: i32 = 30_000;

struct Handlers {
    _open: Closure<dyn FnMut()>,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _close: Closure<dyn FnMut(CloseEvent)>,
}

/// The live link to the bridge. Shared by the socket callbacks and
/// [`super::LiveModule`].
pub struct Link {
    url: String,
    token: String,
    queue: Queue,
    listeners: Listeners,
    socket: RefCell<Option<WebSocket>>,
    handlers: RefCell<Option<Handlers>>,
    needs_hello: Cell<bool>,
    connected: Cell<bool>,
    stopped: Cell<bool>,
    backoff_ms: Cell<i32>,
}

impl Link {
    /// Is the socket open and authenticated (as far as the page knows)?
    pub fn connected(&self) -> bool {
        self.connected.get()
    }

    /// Send one text frame (events) if the socket is open.
    pub fn send_text(&self, text: &str) {
        if !self.connected.get() {
            return;
        }
        if let Some(ws) = self.socket.borrow().as_ref() {
            let _ = ws.send_with_str(text);
        }
    }

    /// Like [`Link::send_text`], but never waits on a borrow (the panic
    /// hook uses it).
    #[cfg(feature = "hotpatch")]
    pub fn try_send_text(&self, text: &str) {
        if !self.connected.get() {
            return;
        }
        if let Ok(s) = self.socket.try_borrow()
            && let Some(ws) = s.as_ref()
        {
            let _ = ws.send_with_str(text);
        }
    }

    /// Called every frame: sends the `hello` once after each (re)connect.
    pub fn frame(&self, world: &World) {
        if !self.needs_hello.get() {
            return;
        }
        self.needs_hello.set(false);
        let s = super::schema(world);
        #[allow(unused_mut)]
        let mut hello = json!({
            "type": "hello", "v": 1, "protocol": super::PROTOCOL,
            "game": s["game"], "schema_version": s["schema_version"],
            "tier": s["tiers"]["active"],
            // The page files' version this page shows (14.2).
            "page_rev": crate::page_patch::web::page_rev(),
        });
        // Live code swapping (10.4): this page takes patches.
        #[cfg(feature = "hotpatch")]
        {
            hello["code"] = crate::hotpatch::hello();
        }
        if let Some(ws) = self.socket.borrow().as_ref() {
            let _ = ws.send_with_str(&hello.to_string());
        }
    }
}

/// Open the link if this page was served by the bridge. Returns `None`
/// when live control is off for this page.
pub fn connect(queue: Queue, listeners: Listeners) -> Option<Rc<Link>> {
    let win = web_sys::window()?;
    let loc = win.location();
    let search = loc.search().unwrap_or_default();
    if search
        .trim_start_matches('?')
        .split('&')
        .any(|kv| kv == "live=0")
    {
        return None;
    }
    let token = win
        .document()?
        .query_selector("meta[name=evani-live-token]")
        .ok()??
        .get_attribute("content")?;
    if token.is_empty() {
        return None;
    }
    let scheme = if loc.protocol().ok()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    let url = format!("{scheme}://{}/engine", loc.host().ok()?);
    let link = Rc::new(Link {
        url,
        token,
        queue,
        listeners,
        socket: RefCell::new(None),
        handlers: RefCell::new(None),
        needs_hello: Cell::new(false),
        connected: Cell::new(false),
        stopped: Cell::new(false),
        backoff_ms: Cell::new(BACKOFF_MIN_MS),
    });
    open(&link);
    Some(link)
}

fn open(link: &Rc<Link>) {
    if link.stopped.get() {
        return;
    }
    let ws = match WebSocket::new(&link.url) {
        Ok(ws) => ws,
        // Sockets blocked: try again later, quietly.
        Err(_) => return retry(link),
    };

    let l = link.clone();
    let ws_open = ws.clone();
    let on_open = Closure::<dyn FnMut()>::new(move || {
        let auth = json!({ "type": "auth", "v": 1, "token": l.token });
        let _ = ws_open.send_with_str(&auth.to_string());
        l.connected.set(true);
        l.needs_hello.set(true);
        l.backoff_ms.set(BACKOFF_MIN_MS);
        // The page's state goes at once, frames or not (3.5).
        crate::host::send("hello");
    });

    let l = link.clone();
    let ws_msg = ws.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let Some(text) = e.data().as_string() else {
            return;
        };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        match msg["type"].as_str() {
            Some("cmd") => {
                let out = ws_msg.clone();
                let reply = Closure::once_into_js(move |v: JsValue| {
                    if let Ok(s) = js_sys::JSON::stringify(&v) {
                        let _ = out.send_with_str(&String::from(s));
                    }
                });
                l.queue.borrow_mut().push((
                    text,
                    Responder::Envelope(reply.unchecked_into::<Function>()),
                ));
            }
            Some("chat_reply") => {
                if let Some(t) = msg["text"].as_str() {
                    web_sys::console::info_1(&JsValue::from_str(&format!("[Evani.dev] {t}")));
                }
            }
            // A saved page file swapped in place, no reload (14).
            Some("page_patch") => {
                if let Some(mut dom) = crate::page_patch::web::WebDom::new() {
                    let reply = crate::page_patch::handle(&mut dom, &msg);
                    let _ = ws_msg.send_with_str(&reply.to_string());
                }
            }
            // Live code swapping (10.4).
            #[cfg(feature = "hotpatch")]
            Some("code_patch" | "code_patching" | "code_error" | "code_reload") => {
                crate::hotpatch::on_message(&msg);
            }
            // The bridge's own events (`convert_progress`, `convert_done`,
            // LIVE_API.md 16) go to the page's `on` listeners.
            Some("event") => {
                if let Some(name) = msg["event"].as_str() {
                    let data = js_sys::JSON::parse(&msg["data"].to_string()).unwrap_or(JsValue::NULL);
                    super::page::dispatch(&l.listeners, name, &data);
                }
            }
            // `session.mirror` (10.3), `live_status` (`page_changed` is the
            // tab's to act on) and anything else: nothing to do here.
            _ => {}
        }
    });

    let l = link.clone();
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
        l.connected.set(false);
        l.needs_hello.set(false);
        if matches!(e.code(), CLOSE_UNAUTHORIZED | CLOSE_REPLACED) {
            l.stopped.set(true);
            return;
        }
        retry(&l);
    });

    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    // The old socket (if any) is closed; drop its handlers outside its callbacks.
    if let Some(old) = link.socket.replace(Some(ws)) {
        old.set_onopen(None);
        old.set_onmessage(None);
        old.set_onclose(None);
    }
    link.handlers.replace(Some(Handlers {
        _open: on_open,
        _message: on_message,
        _close: on_close,
    }));
}

/// Schedule a reconnect with backoff. Runs from a timer, never from inside
/// the socket's own callbacks, so replacing the handlers is safe.
fn retry(link: &Rc<Link>) {
    if link.stopped.get() {
        return;
    }
    let delay = link.backoff_ms.get();
    link.backoff_ms.set((delay * 2).min(BACKOFF_MAX_MS));
    let Some(win) = web_sys::window() else { return };
    let l = link.clone();
    let cb = Closure::once_into_js(move || open(&l));
    let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), delay);
}
