//! Positional (3D) audio, behind the `audio-3d` feature (S5-E2). A kit moves
//! the listener once a frame ([`set_listener`](AudioMixer::set_listener)) and
//! places sounds in the world; the platform pans and fades each one.
//!
//! ```ignore
//! mixer.set_listener(cam_pos, cam_forward, [0.0, 1.0, 0.0]);
//! mixer.play_at(boom, [12.0, 0.0, -30.0]);                    // one-shot
//! let engine = mixer.voice_start_at(hum, Bus::Effects, 0.6, 1.0, pos, Spatial::default());
//! mixer.voice_set_position(engine, new_pos);                  // each frame
//! mixer.voice_stop(engine);                                   // as for any voice
//! ```
//!
//! Its own file (a child of `audio`) so that `audio.rs` keeps its line
//! numbers: games without the feature are byte for byte what they were.

use super::{AudioMixer, Bus, SoundId, VoiceId, clean_gain};

/// How a positional sound fades with distance (the Web Audio distance models).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Falloff {
    /// `ref / (ref + rolloff * (d - ref))`: natural, never quite silent.
    #[default]
    Inverse,
    /// Straight line from full volume at `ref_distance` to silent at `max_distance`.
    Linear,
    /// `(d / ref) ^ -rolloff`: steeper than inverse.
    Exponential,
}

/// Where the listener is: a position and the way they face. World units are
/// metres; right-handed, y up, and a listener facing -z with +y up is the
/// default (the Web Audio convention). `forward` and `up` need not be unit
/// length; a zero or non-finite vector falls back to the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listener {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        }
    }
}

/// How one positional sound is heard. [`Spatial::default`] suits a footstep
/// or a door: full volume inside 1 m, fading out by 60 m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spatial {
    /// Full volume at or inside this distance (metres, > 0).
    pub ref_distance: f32,
    /// Beyond this distance a one-shot is not played at all (saves a voice),
    /// and the `Linear` falloff reaches silence here.
    pub max_distance: f32,
    /// How fast the sound fades; 1.0 is the physical rate.
    pub rolloff: f32,
    pub falloff: Falloff,
}

impl Default for Spatial {
    fn default() -> Self {
        Self {
            ref_distance: 1.0,
            max_distance: 60.0,
            rolloff: 1.0,
            falloff: Falloff::Inverse,
        }
    }
}

impl Spatial {
    /// The same values with everything finite and in the range every browser
    /// accepts.
    pub fn cleaned(self) -> Self {
        let pos = |v: f32, d: f32| if v.is_finite() && v > 0.0 { v } else { d };
        let ref_distance = pos(self.ref_distance, 1.0);
        Self {
            ref_distance,
            max_distance: pos(self.max_distance, 60.0).max(ref_distance),
            rolloff: if self.rolloff.is_finite() { self.rolloff.max(0.0) } else { 1.0 },
            falloff: self.falloff,
        }
    }

    /// The volume factor (0 to 1) at `distance` metres, the same curve the
    /// web backend asks the browser for. Kits can use it to decide whether a
    /// sound is worth starting.
    pub fn gain_at(&self, distance: f32) -> f32 {
        let s = self.cleaned();
        let d = if distance.is_finite() { distance.max(0.0) } else { s.max_distance };
        let r = s.ref_distance;
        let g = match s.falloff {
            Falloff::Inverse => r / (r + s.rolloff * (d.max(r) - r)),
            Falloff::Linear => {
                let span = (s.max_distance - r).max(f32::EPSILON);
                1.0 - s.rolloff.min(1.0) * (d.clamp(r, s.max_distance) - r) / span
            }
            Falloff::Exponential => (d.max(r) / r).powf(-s.rolloff),
        };
        g.clamp(0.0, 1.0)
    }
}

/// Distance between two world points (non-finite coordinates count as far).
fn distance3(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    if d.is_finite() { d.sqrt() } else { f32::INFINITY }
}

fn clean_vec3(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    if v.iter().all(|c| c.is_finite()) { v } else { fallback }
}

fn clean_dir(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let v = clean_vec3(v, fallback);
    if v.iter().any(|c| *c != 0.0) { v } else { fallback }
}

/// A positional one-shot waiting for audio to unlock or its sound to decode.
#[derive(Clone, Copy)]
pub(super) struct PendingSpatialPlay {
    id: SoundId,
    bus: Bus,
    gain: f32,
    pos: [f32; 3],
    spatial: Spatial,
}

impl AudioMixer {
    /// Set where the listener is and which way they face. Call once per
    /// frame (an unchanged listener sends nothing). Bad values keep the old ones.
    pub fn set_listener(&mut self, position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
        let d = Listener::default();
        let l = Listener {
            position: clean_vec3(position, self.listener.position),
            forward: clean_dir(forward, d.forward),
            up: clean_dir(up, d.up),
        };
        if l != self.listener {
            self.listener = l;
            self.listener_dirty = true;
        }
    }

    pub fn listener(&self) -> Listener {
        self.listener
    }

    /// Play a sound effect at a world position with the default falloff.
    pub fn play_at(&mut self, id: SoundId, pos: [f32; 3]) {
        self.play_at_on(id, Bus::Effects, 1.0, pos, Spatial::default());
    }

    /// Play once at `pos` on `bus` with an extra linear `gain` and a falloff
    /// of your choosing. Dropped while audio is locked (as for [`play`](Self::play))
    /// and when `pos` is beyond `spatial.max_distance` from the listener.
    pub fn play_at_on(&mut self, id: SoundId, bus: Bus, gain: f32, pos: [f32; 3], spatial: Spatial) {
        let spatial = spatial.cleaned();
        if !self.backend.is_unlocked()
            || self.spatial_queue.len() >= self.spatial_queue.capacity()
            || !pos.iter().all(|c| c.is_finite())
            || distance3(self.listener.position, pos) > spatial.max_distance
        {
            return;
        }
        self.spatial_queue.push(PendingSpatialPlay {
            id,
            bus,
            gain: clean_gain(gain),
            pos,
            spatial,
        });
    }

    /// Start a looping voice at a world position (everything as for
    /// [`voice_start`](Self::voice_start), plus where it is and how it fades).
    /// Move it with [`voice_set_position`](Self::voice_set_position); change
    /// pitch with [`voice_set`](Self::voice_set); stop it with
    /// [`voice_stop`](Self::voice_stop).
    pub fn voice_start_at(
        &mut self,
        id: SoundId,
        bus: Bus,
        gain: f32,
        rate: f32,
        pos: [f32; 3],
        spatial: Spatial,
    ) -> VoiceId {
        let voice = self.voice_start(id, bus, gain, rate);
        if let Some(v) = self.voices.get_mut(voice.0 as usize) {
            v.spatial = Some((clean_vec3(pos, [0.0; 3]), spatial.cleaned()));
        }
        voice
    }

    /// Move a positional voice. A voice started without a position, a stopped
    /// voice and a non-finite position are ignored.
    pub fn voice_set_position(&mut self, voice: VoiceId, pos: [f32; 3]) {
        if let Some(v) = self.voices.get_mut(voice.0 as usize)
            && !v.free
            && !v.stopping
            && pos.iter().all(|c| c.is_finite())
            && let Some((p, _)) = v.spatial.as_mut()
            && *p != pos
        {
            *p = pos;
            v.pos_dirty = true;
        }
    }

    /// A positional voice's current position, if it is going.
    pub fn voice_position(&self, voice: VoiceId) -> Option<[f32; 3]> {
        self.voices
            .get(voice.0 as usize)
            .filter(|v| !v.free && !v.stopping)
            .and_then(|v| v.spatial.map(|s| s.0))
    }

    /// From [`update`](Self::update), before the plain plays: send a moved
    /// listener and start the queued positional one-shots.
    pub(super) fn flush_spatial(&mut self) {
        if self.listener_dirty {
            self.backend.set_listener(self.listener);
            self.listener_dirty = false;
        }
        for p in self.spatial_queue.drain(..) {
            if self.backend.is_ready(p.id) {
                self.backend.play_at(p.id, p.bus, p.gain, p.pos, p.spatial);
            }
        }
    }
}
