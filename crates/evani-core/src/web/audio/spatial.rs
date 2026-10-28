//! Positional (3D) audio for the Web Audio backend, behind the `audio-3d`
//! feature (S5-E2): one PannerNode per positional sound (gain -> panner ->
//! the bus) and the context's listener. Its own file (a child of
//! `web::audio`) so that `audio.rs` keeps its line numbers: games without the
//! feature are byte for byte what they were.

use web_sys::{AudioContext, DistanceModelType, PannerNode, PanningModelType};

use super::{Inner, WebAudio, WebVoice};
use crate::audio::{Bus, Falloff, Listener, SoundId, Spatial, VoiceId};
use crate::web::{console_log, js_err};

impl Inner {
    /// Push the stored listener to the context (no-op before unlock).
    pub(super) fn apply_listener(&self) {
        if let Some(g) = &self.graph {
            let l = &self.listener;
            let a = g.ctx.listener();
            a.set_position(
                f64::from(l.position[0]),
                f64::from(l.position[1]),
                f64::from(l.position[2]),
            );
            a.set_orientation(
                f64::from(l.forward[0]),
                f64::from(l.forward[1]),
                f64::from(l.forward[2]),
                f64::from(l.up[0]),
                f64::from(l.up[1]),
                f64::from(l.up[2]),
            );
        }
    }
}

impl WebAudio {
    pub(super) fn set_listener_3d(&mut self, listener: Listener) {
        let mut inner = self.inner.borrow_mut();
        inner.listener = listener;
        inner.apply_listener();
    }

    pub(super) fn play_at_3d(&mut self, id: SoundId, bus: Bus, gain: f32, pos: [f32; 3], spatial: Spatial) -> bool {
        let inner = self.inner.borrow();
        let Some(g) = &inner.graph else { return false };
        let Some(Some(buf)) = inner.buffers.get(id.0 as usize) else {
            return false;
        };
        let target = match bus {
            Bus::Music => &g.music,
            Bus::Effects => &g.effects,
        };
        let result = (|| -> Result<(), String> {
            let src = g.ctx.create_buffer_source().map_err(js_err)?;
            src.set_buffer(Some(buf));
            let vg = g.ctx.create_gain().map_err(js_err)?;
            vg.gain().set_value(gain);
            let panner = make_panner(&g.ctx, pos, &spatial)?;
            src.connect_with_audio_node(&vg).map_err(js_err)?;
            vg.connect_with_audio_node(&panner).map_err(js_err)?;
            panner.connect_with_audio_node(target).map_err(js_err)?;
            web_sys::AudioScheduledSourceNode::start(&src).map_err(js_err)?;
            Ok(())
        })();
        match result {
            Ok(()) => true,
            Err(e) => {
                console_log(true, &format!("audio play_at: {e}"));
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn voice_start_at_3d(
        &mut self,
        voice: VoiceId,
        id: SoundId,
        bus: Bus,
        gain: f32,
        rate: f32,
        pos: [f32; 3],
        spatial: Spatial,
    ) -> bool {
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
            let panner = make_panner(&g.ctx, pos, &spatial)?;
            src.connect_with_audio_node(&vg).map_err(js_err)?;
            vg.connect_with_audio_node(&panner).map_err(js_err)?;
            panner.connect_with_audio_node(target).map_err(js_err)?;
            web_sys::AudioScheduledSourceNode::start(&src).map_err(js_err)?;
            Ok(WebVoice {
                src,
                gain: vg,
                panner: Some(panner),
            })
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
                console_log(true, &format!("audio voice_start_at: {e}"));
                false
            }
        }
    }

    pub(super) fn voice_set_position_3d(&mut self, voice: VoiceId, pos: [f32; 3]) {
        let inner = self.inner.borrow();
        if let Some(p) = inner.voices.get(&voice.0).and_then(|v| v.panner.as_ref()) {
            // A plain attribute set (no easing); a source updated once a
            // frame pans smoothly enough.
            p.set_position(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
        }
    }
}

/// A panner for one positional sound: equal-power panning (cheap on phones),
/// the chosen distance model, at `pos`.
fn make_panner(ctx: &AudioContext, pos: [f32; 3], spatial: &Spatial) -> Result<PannerNode, String> {
    let p = ctx.create_panner().map_err(js_err)?;
    p.set_panning_model(PanningModelType::Equalpower);
    p.set_distance_model(match spatial.falloff {
        Falloff::Inverse => DistanceModelType::Inverse,
        Falloff::Linear => DistanceModelType::Linear,
        Falloff::Exponential => DistanceModelType::Exponential,
    });
    p.set_ref_distance(f64::from(spatial.ref_distance));
    p.set_max_distance(f64::from(spatial.max_distance));
    p.set_rolloff_factor(f64::from(spatial.rolloff));
    p.set_position(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
    Ok(p)
}
