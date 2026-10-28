//! Web Audio backend: AudioContext -> master gain -> destination, with music
//! and effects bus gains feeding master. The context is created only inside
//! a user-gesture handler ([`AudioUnlocker::unlock`]), as browsers require.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, GainNode};

use super::{console_log, js_err};
use crate::audio::{AudioBackend, Bus, SoundId, VoiceId}; #[cfg(feature = "audio-3d")] mod spatial; #[cfg(feature = "audio-3d")] use crate::audio::{Listener, Spatial};

struct Graph {
    ctx: AudioContext,
    master: GainNode,
    music: GainNode,
    effects: GainNode,
}

/// A running looping voice: source -> its own gain -> the bus.
struct WebVoice {
    src: AudioBufferSourceNode,
    gain: GainNode, #[doc = " Set for a positional voice: gain -> panner -> the bus."] #[cfg(feature = "audio-3d")] panner: Option<web_sys::PannerNode>,
}

/// Seconds the gain and the pitch take to settle on a new value (no zipper
/// noise when an engine's pitch moves every frame).
const VOICE_EASE_S: f64 = 0.04;

#[derive(Default)]
struct Inner {
    graph: Option<Graph>,
    /// Decoded buffers by `SoundId`.
    buffers: Vec<Option<AudioBuffer>>,
    music_src: Option<AudioBufferSourceNode>,
    voices: HashMap<u32, WebVoice>,
    gains: [f32; 3],
    suspended: bool,
    failed: bool, #[doc = " The latest listener the mixer sent; applied again when the context is built."] #[cfg(feature = "audio-3d")] listener: Listener,
}

impl Inner {
    fn bus(&self, bus: Option<Bus>) -> Option<&GainNode> {
        let g = self.graph.as_ref()?;
        Some(match bus {
            None => &g.master,
            Some(Bus::Music) => &g.music,
            Some(Bus::Effects) => &g.effects,
        })
    }

    fn build_graph() -> Result<Graph, String> {
        let ctx = AudioContext::new().map_err(js_err)?;
        let master = ctx.create_gain().map_err(js_err)?;
        let music = ctx.create_gain().map_err(js_err)?;
        let effects = ctx.create_gain().map_err(js_err)?;
        master
            .connect_with_audio_node(&ctx.destination())
            .map_err(js_err)?;
        music.connect_with_audio_node(&master).map_err(js_err)?;
        effects.connect_with_audio_node(&master).map_err(js_err)?;
        Ok(Graph {
            ctx,
            master,
            music,
            effects,
        })
    }
}

/// The [`AudioBackend`] used on wasm.
pub struct WebAudio {
    inner: Rc<RefCell<Inner>>,
}

/// World resource the web runner uses to start audio inside the first
/// pointer/key/touch handler.
#[derive(Clone)]
pub struct AudioUnlocker {
    inner: Rc<RefCell<Inner>>,
}

impl AudioUnlocker {
    /// Create (or resume) the AudioContext. Must run inside a user-gesture
    /// event handler. Cheap after the first success.
    pub fn unlock(&self) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        if inner.failed {
            return;
        }
        if inner.graph.is_none() {
            match Inner::build_graph() {
                Ok(g) => {
                    inner.graph = Some(g);
                    let gains = inner.gains;
                    for (i, bus) in [None, Some(Bus::Music), Some(Bus::Effects)]
                        .into_iter()
                        .enumerate()
                    {
                        if let Some(n) = inner.bus(bus) {
                            n.gain().set_value(gains[i]);
                        }
                    } #[cfg(feature = "audio-3d")] inner.apply_listener();
                }
                Err(e) => {
                    inner.failed = true;
                    console_log(true, &format!("audio unavailable: {e}"));
                    return;
                }
            }
        }
        if let Some(g) = &inner.graph
            && g.ctx.state() == AudioContextState::Suspended
            && !inner.suspended
        {
            let _ = g.ctx.resume();
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.inner.try_borrow().is_ok_and(|i| i.graph.is_some())
    }
}

impl WebAudio {
    pub fn new() -> Self {
        let inner = Inner {
            gains: [1.0, 0.7, 1.0],
            ..Inner::default()
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    pub fn unlocker(&self) -> AudioUnlocker {
        AudioUnlocker {
            inner: self.inner.clone(),
        }
    }
}

impl Default for WebAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for WebAudio {
    fn is_unlocked(&self) -> bool {
        self.inner.borrow().graph.is_some()
    }

    fn set_gain(&mut self, bus: Option<Bus>, gain: f32) {
        let mut inner = self.inner.borrow_mut();
        let i = match bus {
            None => 0,
            Some(Bus::Music) => 1,
            Some(Bus::Effects) => 2,
        };
        inner.gains[i] = gain;
        if let Some(n) = inner.bus(bus) {
            n.gain().set_value(gain);
        }
    }

    fn decode(&mut self, id: SoundId, bytes: Rc<[u8]>) {
        let promise = {
            let inner = self.inner.borrow();
            let Some(g) = &inner.graph else { return };
            let arr = js_sys::Uint8Array::from(&bytes[..]);
            match g.ctx.decode_audio_data(&arr.buffer()) {
                Ok(p) => p,
                Err(e) => {
                    console_log(true, &format!("audio decode: {}", js_err(e)));
                    return;
                }
            }
        };
        let inner = self.inner.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(v) => {
                    if let Ok(buf) = v.dyn_into::<AudioBuffer>()
                        && let Ok(mut i) = inner.try_borrow_mut()
                    {
                        let idx = id.0 as usize;
                        if i.buffers.len() <= idx {
                            i.buffers.resize(idx + 1, None);
                        }
                        i.buffers[idx] = Some(buf);
                    }
                }
                Err(e) => console_log(true, &format!("audio decode: {}", js_err(e))),
            }
        });
    }

    fn is_ready(&self, id: SoundId) -> bool {
        self.inner
            .borrow()
            .buffers
            .get(id.0 as usize)
            .is_some_and(Option::is_some)
    }

    fn play(&mut self, id: SoundId, bus: Bus, gain: f32, looped: bool) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(g) = &inner.graph else { return false };
        let Some(Some(buf)) = inner.buffers.get(id.0 as usize) else {
            return false;
        };
        let target = match bus {
            Bus::Music => &g.music,
            Bus::Effects => &g.effects,
        };
        let result = (|| -> Result<AudioBufferSourceNode, String> {
            let src = g.ctx.create_buffer_source().map_err(js_err)?;
            src.set_buffer(Some(buf));
            src.set_loop(looped);
            if (gain - 1.0).abs() > f32::EPSILON {
                let v = g.ctx.create_gain().map_err(js_err)?;
                v.gain().set_value(gain);
                src.connect_with_audio_node(&v).map_err(js_err)?;
                v.connect_with_audio_node(target).map_err(js_err)?;
            } else {
                src.connect_with_audio_node(target).map_err(js_err)?;
            }
            web_sys::AudioScheduledSourceNode::start(&src).map_err(js_err)?;
            Ok(src)
        })();
        match result {
            Ok(src) => {
                if bus == Bus::Music {
                    inner.music_src = Some(src);
                }
                true
            }
            Err(e) => {
                console_log(true, &format!("audio play: {e}"));
                false
            }
        }
    }

    fn stop_music(&mut self) {
        if let Some(src) = self.inner.borrow_mut().music_src.take() {
            let _ = web_sys::AudioScheduledSourceNode::stop(&src);
            let _ = src.disconnect();
        }
    }

    fn voice_start(&mut self, voice: VoiceId, id: SoundId, bus: Bus, gain: f32, rate: f32) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(g) = &inner.graph else { return false };
        let Some(Some(buf)) = inner.buffers.get(id.0 as usize) else {
            return false;
        };
        let target = match bus {
            Bus::Music => &g.music,
            Bus::Effects => &g.effects,
        };
        let result = (|| -> Result<WebVoice, String> {
            let src = g.ctx.create_buffer_source().map_err(js_err)?;
            src.set_buffer(Some(buf));
            src.set_loop(true);
            src.playback_rate().set_value(rate);
            let vg = g.ctx.create_gain().map_err(js_err)?;
            vg.gain().set_value(gain);
            src.connect_with_audio_node(&vg).map_err(js_err)?;
            vg.connect_with_audio_node(target).map_err(js_err)?;
            web_sys::AudioScheduledSourceNode::start(&src).map_err(js_err)?;
            Ok(WebVoice { src, gain: vg, #[cfg(feature = "audio-3d")] panner: None })
        })();
        match result {
            Ok(v) => {
                if let Some(old) = inner.voices.insert(voice.0, v) {
                    let _ = web_sys::AudioScheduledSourceNode::stop(&old.src);
                    let _ = old.src.disconnect();
                }
                true
            }
            Err(e) => {
                console_log(true, &format!("audio voice: {e}"));
                false
            }
        }
    }

    fn voice_set(&mut self, voice: VoiceId, gain: f32, rate: f32) {
        let inner = self.inner.borrow();
        let (Some(g), Some(v)) = (&inner.graph, inner.voices.get(&voice.0)) else {
            return;
        };
        let now = g.ctx.current_time();
        let _ = v.gain.gain().set_target_at_time(gain, now, VOICE_EASE_S);
        let _ = v.src.playback_rate().set_target_at_time(rate, now, VOICE_EASE_S);
    }

    fn voice_stop(&mut self, voice: VoiceId) {
        if let Some(v) = self.inner.borrow_mut().voices.remove(&voice.0) {
            let _ = web_sys::AudioScheduledSourceNode::stop(&v.src);
            let _ = v.src.disconnect();
            let _ = v.gain.disconnect(); #[cfg(feature = "audio-3d")] if let Some(p) = &v.panner { let _ = p.disconnect(); }
        }
    } #[cfg(feature = "audio-3d")] fn set_listener(&mut self, listener: Listener) { self.set_listener_3d(listener) } #[cfg(feature = "audio-3d")] fn play_at(&mut self, id: SoundId, bus: Bus, gain: f32, pos: [f32; 3], spatial: Spatial) -> bool { self.play_at_3d(id, bus, gain, pos, spatial) } #[cfg(feature = "audio-3d")] fn voice_start_at(&mut self, voice: VoiceId, id: SoundId, bus: Bus, gain: f32, rate: f32, pos: [f32; 3], spatial: Spatial) -> bool { self.voice_start_at_3d(voice, id, bus, gain, rate, pos, spatial) } #[cfg(feature = "audio-3d")] fn voice_set_position(&mut self, voice: VoiceId, pos: [f32; 3]) { self.voice_set_position_3d(voice, pos) }

    fn set_suspended(&mut self, suspended: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.suspended = suspended;
        if let Some(g) = &inner.graph {
            let _ = if suspended {
                g.ctx.suspend()
            } else {
                g.ctx.resume()
            };
        }
    }
}
