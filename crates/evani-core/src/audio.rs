//! Basic Web Audio mixer: master -> {music, effects} buses.
//!
//! Browsers only allow audio after a user gesture, so the `AudioContext` is
//! created by the first pointer/key/touch event (see `web::audio`). Before
//! that, effects are dropped (stale sounds make no sense) and the most recent
//! music request is remembered and started on unlock.
//!
//! The mixer logic is backend-agnostic and tested natively with
//! [`NullAudio`]; on wasm the backend is `web::WebAudio`.

use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bus {
    Music,
    Effects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SoundId(pub u32);

/// A looping voice whose gain and playback rate (pitch) can change while it
/// plays, such as an engine that follows speed. Made by
/// [`AudioMixer::voice_start`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(pub u32);

/// Playback rates are kept inside what every browser accepts.
pub const VOICE_RATE_MIN: f32 = 0.05;
pub const VOICE_RATE_MAX: f32 = 8.0; #[cfg(feature = "audio-3d")] mod spatial; #[cfg(feature = "audio-3d")] pub use spatial::{Falloff, Listener, Spatial};

/// What the mixer needs from the platform.
pub trait AudioBackend {
    /// True once the context exists and is running (after a user gesture).
    fn is_unlocked(&self) -> bool;
    /// Linear gain for the master output or a bus.
    fn set_gain(&mut self, bus: Option<Bus>, gain: f32);
    /// Start decoding encoded audio (ogg/mp3/wav bytes) for `id`.
    fn decode(&mut self, id: SoundId, bytes: Rc<[u8]>);
    /// True if `id` is decoded and playable.
    fn is_ready(&self, id: SoundId) -> bool;
    /// Play once (or looped) on a bus with an extra per-voice gain.
    fn play(&mut self, id: SoundId, bus: Bus, gain: f32, looped: bool) -> bool;
    fn stop_music(&mut self);
    /// Suspend while the tab is hidden; resume when visible.
    fn set_suspended(&mut self, suspended: bool);
    /// Start a looping voice (`rate` 1.0 = the sound as recorded). False if
    /// it could not start. Backends without voices keep the default.
    fn voice_start(&mut self, _voice: VoiceId, _id: SoundId, _bus: Bus, _gain: f32, _rate: f32) -> bool {
        false
    }
    /// Change a running voice's gain and rate; the backend eases the change.
    fn voice_set(&mut self, _voice: VoiceId, _gain: f32, _rate: f32) {}
    /// Stop a voice and free what it used.
    fn voice_stop(&mut self, _voice: VoiceId) {} #[doc = " Move the listener (positional audio). Backends without it keep the default."] #[cfg(feature = "audio-3d")] fn set_listener(&mut self, _listener: Listener) {} #[doc = " Play once at a world position. False if it could not start."] #[cfg(feature = "audio-3d")] fn play_at(&mut self, _id: SoundId, _bus: Bus, _gain: f32, _pos: [f32; 3], _spatial: Spatial) -> bool { false } #[doc = " Start a looping voice at a world position (stopped with [`voice_stop`](Self::voice_stop), moved with [`voice_set_position`](Self::voice_set_position))."] #[cfg(feature = "audio-3d")] #[allow(clippy::too_many_arguments)] fn voice_start_at(&mut self, _voice: VoiceId, _id: SoundId, _bus: Bus, _gain: f32, _rate: f32, _pos: [f32; 3], _spatial: Spatial) -> bool { false } #[doc = " Move a positional voice; the backend eases the move."] #[cfg(feature = "audio-3d")] fn voice_set_position(&mut self, _voice: VoiceId, _pos: [f32; 3]) {}
}

/// Silent backend for native runs and tests; counts plays.
#[derive(Debug, Default)]
pub struct NullAudio {
    pub unlocked: bool,
    pub decoded: Vec<SoundId>,
    pub plays: Vec<(SoundId, Bus, bool)>,
    pub gains: [f32; 3],
    pub suspended: bool,
    /// Every `voice_start`: (voice, sound, bus).
    pub voice_starts: Vec<(VoiceId, SoundId, Bus)>,
    /// The latest `(gain, rate)` of each running voice.
    pub voices: Vec<(VoiceId, f32, f32)>, #[doc = " The latest listener the mixer sent."] #[cfg(feature = "audio-3d")] pub listener: Option<Listener>, #[doc = " Every `play_at`: (sound, bus, position)."] #[cfg(feature = "audio-3d")] pub plays_at: Vec<(SoundId, Bus, [f32; 3])>, #[doc = " The latest position of each running positional voice."] #[cfg(feature = "audio-3d")] pub voice_positions: Vec<(VoiceId, [f32; 3])>,
}

impl AudioBackend for NullAudio {
    fn is_unlocked(&self) -> bool {
        self.unlocked
    }
    fn set_gain(&mut self, bus: Option<Bus>, gain: f32) {
        let i = match bus {
            None => 0,
            Some(Bus::Music) => 1,
            Some(Bus::Effects) => 2,
        };
        self.gains[i] = gain;
    }
    fn decode(&mut self, id: SoundId, _bytes: Rc<[u8]>) {
        self.decoded.push(id);
    }
    fn is_ready(&self, id: SoundId) -> bool {
        self.decoded.contains(&id)
    }
    fn play(&mut self, id: SoundId, bus: Bus, _gain: f32, looped: bool) -> bool {
        self.plays.push((id, bus, looped));
        true
    }
    fn stop_music(&mut self) {}
    fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
    }
    fn voice_start(&mut self, voice: VoiceId, id: SoundId, bus: Bus, gain: f32, rate: f32) -> bool {
        self.voice_starts.push((voice, id, bus));
        self.voices.push((voice, gain, rate));
        true
    }
    fn voice_set(&mut self, voice: VoiceId, gain: f32, rate: f32) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.0 == voice) {
            v.1 = gain;
            v.2 = rate;
        }
    }
    fn voice_stop(&mut self, voice: VoiceId) {
        self.voices.retain(|v| v.0 != voice); #[cfg(feature = "audio-3d")] self.voice_positions.retain(|v| v.0 != voice);
    } #[cfg(feature = "audio-3d")] fn set_listener(&mut self, listener: Listener) { self.listener = Some(listener); } #[cfg(feature = "audio-3d")] fn play_at(&mut self, id: SoundId, bus: Bus, _gain: f32, pos: [f32; 3], _spatial: Spatial) -> bool { self.plays_at.push((id, bus, pos)); true } #[cfg(feature = "audio-3d")] fn voice_start_at(&mut self, voice: VoiceId, id: SoundId, bus: Bus, gain: f32, rate: f32, pos: [f32; 3], _spatial: Spatial) -> bool { self.voice_positions.push((voice, pos)); self.voice_start(voice, id, bus, gain, rate) } #[cfg(feature = "audio-3d")] fn voice_set_position(&mut self, voice: VoiceId, pos: [f32; 3]) { if let Some(v) = self.voice_positions.iter_mut().find(|v| v.0 == voice) { v.1 = pos; } }
}

struct Sound {
    name: String,
    bytes: Option<Rc<[u8]>>,
    decode_started: bool,
}

#[derive(Clone, Copy)]
struct PendingPlay {
    id: SoundId,
    bus: Bus,
    gain: f32,
}

/// The mixer's side of one looping voice.
#[derive(Clone, Copy)]
struct Voice {
    sound: SoundId,
    bus: Bus,
    gain: f32,
    rate: f32,
    /// Running in the backend.
    started: bool,
    /// Gain or rate changed since the backend last heard.
    dirty: bool,
    /// Asked to stop; the backend voice (if any) is stopped next update and
    /// the slot reused.
    stopping: bool,
    free: bool, #[doc = " Set for a positional voice: where it is and how it is heard."] #[cfg(feature = "audio-3d")] spatial: Option<([f32; 3], Spatial)>, #[doc = " The position changed since the backend last heard."] #[cfg(feature = "audio-3d")] pos_dirty: bool,
}

/// World resource.
pub struct AudioMixer {
    backend: Box<dyn AudioBackend>,
    master: f32,
    music: f32,
    effects: f32,
    muted: bool,
    gains_dirty: bool,
    was_unlocked: bool,
    sounds: Vec<Sound>,
    queue: Vec<PendingPlay>,
    music_request: Option<(SoundId, bool)>,
    music_started: bool,
    voices: Vec<Voice>, #[cfg(feature = "audio-3d")] listener: Listener, #[cfg(feature = "audio-3d")] listener_dirty: bool, #[cfg(feature = "audio-3d")] spatial_queue: Vec<spatial::PendingSpatialPlay>,
}

impl AudioMixer {
    pub fn new(backend: Box<dyn AudioBackend>) -> Self {
        Self {
            backend,
            master: 1.0,
            music: 0.7,
            effects: 1.0,
            muted: false,
            gains_dirty: true,
            was_unlocked: false,
            sounds: Vec::new(),
            queue: Vec::with_capacity(32),
            music_request: None,
            music_started: false,
            voices: Vec::new(), #[cfg(feature = "audio-3d")] listener: Listener::default(), #[cfg(feature = "audio-3d")] listener_dirty: true, #[cfg(feature = "audio-3d")] spatial_queue: Vec::with_capacity(32),
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.backend.is_unlocked()
    }

    /// Register encoded sound bytes (e.g. from [`crate::assets::Assets`]).
    /// Decoding starts once audio is unlocked.
    pub fn add_sound(&mut self, name: &str, bytes: Rc<[u8]>) -> SoundId {
        let id = SoundId(self.sounds.len() as u32);
        self.sounds.push(Sound {
            name: name.to_string(),
            bytes: Some(bytes),
            decode_started: false,
        });
        id
    }

    pub fn sound_id(&self, name: &str) -> Option<SoundId> {
        self.sounds
            .iter()
            .position(|s| s.name == name)
            .map(|i| SoundId(i as u32))
    }

    pub fn set_volume(&mut self, bus: Option<Bus>, v: f32) {
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        match bus {
            None => self.master = v,
            Some(Bus::Music) => self.music = v,
            Some(Bus::Effects) => self.effects = v,
        }
        self.gains_dirty = true;
    }

    pub fn volume(&self, bus: Option<Bus>) -> f32 {
        match bus {
            None => self.master,
            Some(Bus::Music) => self.music,
            Some(Bus::Effects) => self.effects,
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.gains_dirty = true;
    }

    /// Play a sound effect (dropped if audio is still locked).
    pub fn play(&mut self, id: SoundId) {
        self.play_on(id, Bus::Effects, 1.0);
    }

    pub fn play_on(&mut self, id: SoundId, bus: Bus, gain: f32) {
        if self.backend.is_unlocked() && self.queue.len() < self.queue.capacity() {
            self.queue.push(PendingPlay { id, bus, gain });
        }
    }

    /// Start looping music now, or as soon as audio unlocks.
    pub fn play_music(&mut self, id: SoundId, looped: bool) {
        self.backend.stop_music();
        self.music_request = Some((id, looped));
        self.music_started = false;
    }

    pub fn stop_music(&mut self) {
        self.backend.stop_music();
        self.music_request = None;
    }

    /// Start a looping voice of `id` on `bus`. It begins as soon as audio is
    /// unlocked and the sound is decoded (so asking before the first user
    /// gesture is fine) and loops until [`voice_stop`](Self::voice_stop).
    /// `gain` is linear and `rate` the playback rate (pitch): 1.0 plays the
    /// sound as recorded. The bus volumes, master volume and mute apply as
    /// for every sound. Stopped voices' slots are reused, so drop the id
    /// after stopping.
    pub fn voice_start(&mut self, id: SoundId, bus: Bus, gain: f32, rate: f32) -> VoiceId {
        let v = Voice {
            sound: id,
            bus,
            gain: clean_gain(gain),
            rate: clean_rate(rate),
            started: false,
            dirty: false,
            stopping: false,
            free: false, #[cfg(feature = "audio-3d")] spatial: None, #[cfg(feature = "audio-3d")] pos_dirty: false,
        };
        let slot = match self.voices.iter().position(|v| v.free) {
            Some(i) => {
                self.voices[i] = v;
                i
            }
            None => {
                self.voices.push(v);
                self.voices.len() - 1
            }
        };
        VoiceId(slot as u32)
    }

    /// Change a voice's gain and rate (pitch). Out-of-range and non-finite
    /// values are clamped.
    pub fn voice_set(&mut self, voice: VoiceId, gain: f32, rate: f32) {
        if let Some(v) = self.voices.get_mut(voice.0 as usize)
            && !v.free
            && !v.stopping
        {
            let (g, r) = (clean_gain(gain), clean_rate(rate));
            if g != v.gain || r != v.rate {
                v.gain = g;
                v.rate = r;
                v.dirty = true;
            }
        }
    }

    /// Stop a voice (at the next [`update`](Self::update)).
    pub fn voice_stop(&mut self, voice: VoiceId) {
        if let Some(v) = self.voices.get_mut(voice.0 as usize)
            && !v.free
        {
            v.stopping = true;
        }
    }

    /// Voices that are going (started or waiting to start), not stopped.
    pub fn voice_count(&self) -> usize {
        self.voices.iter().filter(|v| !v.free && !v.stopping).count()
    }

    /// A voice's current `(gain, rate)`, if it is going.
    pub fn voice_state(&self, voice: VoiceId) -> Option<(f32, f32)> {
        self.voices
            .get(voice.0 as usize)
            .filter(|v| !v.free && !v.stopping)
            .map(|v| (v.gain, v.rate))
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.backend.set_suspended(hidden);
    }

    /// Once per frame: apply volumes, start decodes, flush queued plays.
    pub fn update(&mut self) {
        let unlocked = self.backend.is_unlocked();
        if !unlocked {
            return;
        }
        if !self.was_unlocked {
            self.was_unlocked = true;
            self.gains_dirty = true;
        }
        if self.gains_dirty {
            let m = if self.muted { 0.0 } else { self.master };
            self.backend.set_gain(None, m);
            self.backend.set_gain(Some(Bus::Music), self.music);
            self.backend.set_gain(Some(Bus::Effects), self.effects);
            self.gains_dirty = false;
        }
        for (i, s) in self.sounds.iter_mut().enumerate() {
            if !s.decode_started {
                if let Some(b) = s.bytes.take() {
                    self.backend.decode(SoundId(i as u32), b);
                }
                s.decode_started = true;
            }
        } #[cfg(feature = "audio-3d")] self.flush_spatial();
        for p in self.queue.drain(..) {
            if self.backend.is_ready(p.id) {
                self.backend.play(p.id, p.bus, p.gain, false);
            }
        }
        for (i, v) in self.voices.iter_mut().enumerate() {
            let id = VoiceId(i as u32);
            if v.free {
                continue;
            }
            if v.stopping {
                if v.started {
                    self.backend.voice_stop(id);
                }
                v.free = true;
            } else if !v.started {
                if self.backend.is_ready(v.sound) { #[cfg(feature = "audio-3d")] if let Some((pos, sp)) = v.spatial { v.started = self.backend.voice_start_at(id, v.sound, v.bus, v.gain, v.rate, pos, sp); v.dirty = false; v.pos_dirty = false; continue; }
                    v.started = self.backend.voice_start(id, v.sound, v.bus, v.gain, v.rate);
                    v.dirty = false;
                }
            } else if v.dirty {
                self.backend.voice_set(id, v.gain, v.rate);
                v.dirty = false;
            } #[cfg(feature = "audio-3d")] if v.started && !v.free && !v.stopping && v.pos_dirty { if let Some((pos, _)) = v.spatial { self.backend.voice_set_position(id, pos); } v.pos_dirty = false; }
        }
        if let Some((id, looped)) = self.music_request
            && !self.music_started
            && self.backend.is_ready(id)
        {
            self.music_started = self.backend.play(id, Bus::Music, 1.0, looped);
        }
    }
}

fn clean_gain(g: f32) -> f32 {
    if g.is_finite() { g.clamp(0.0, 4.0) } else { 0.0 }
}

fn clean_rate(r: f32) -> f32 {
    if r.is_finite() { r.clamp(VOICE_RATE_MIN, VOICE_RATE_MAX) } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;

    /// NullAudio behind an Rc so the test can flip `unlocked` like a gesture.
    struct Shared(Rc<RefCell<NullAudio>>);
    impl AudioBackend for Shared {
        fn is_unlocked(&self) -> bool {
            self.0.borrow().is_unlocked()
        }
        fn set_gain(&mut self, bus: Option<Bus>, gain: f32) {
            self.0.borrow_mut().set_gain(bus, gain)
        }
        fn decode(&mut self, id: SoundId, bytes: Rc<[u8]>) {
            self.0.borrow_mut().decode(id, bytes)
        }
        fn is_ready(&self, id: SoundId) -> bool {
            self.0.borrow().is_ready(id)
        }
        fn play(&mut self, id: SoundId, bus: Bus, gain: f32, looped: bool) -> bool {
            self.0.borrow_mut().play(id, bus, gain, looped)
        }
        fn stop_music(&mut self) {}
        fn set_suspended(&mut self, s: bool) {
            self.0.borrow_mut().set_suspended(s)
        }
        fn voice_start(&mut self, v: VoiceId, id: SoundId, bus: Bus, gain: f32, rate: f32) -> bool {
            self.0.borrow_mut().voice_start(v, id, bus, gain, rate)
        }
        fn voice_set(&mut self, v: VoiceId, gain: f32, rate: f32) {
            self.0.borrow_mut().voice_set(v, gain, rate)
        }
        fn voice_stop(&mut self, v: VoiceId) {
            self.0.borrow_mut().voice_stop(v)
        }
    }

    #[test]
    fn nothing_plays_before_gesture_music_starts_after() {
        let null = Rc::new(RefCell::new(NullAudio::default()));
        let mut m = AudioMixer::new(Box::new(Shared(null.clone())));
        let sfx = m.add_sound("hit", Rc::from(vec![1u8, 2, 3]));
        let song = m.add_sound("song", Rc::from(vec![4u8]));
        m.play(sfx);
        m.play_music(song, true);
        m.update();
        assert!(null.borrow().plays.is_empty());
        assert!(null.borrow().decoded.is_empty());

        null.borrow_mut().unlocked = true; // first user gesture
        m.update();
        assert_eq!(null.borrow().decoded, vec![sfx, song]);
        assert_eq!(null.borrow().plays, vec![(song, Bus::Music, true)]);
        m.play(sfx);
        m.update();
        assert_eq!(null.borrow().plays.last(), Some(&(sfx, Bus::Effects, false)));
        m.update();
        assert_eq!(null.borrow().plays.len(), 2, "music not restarted");
    }

    #[test]
    fn volumes_clamp_and_mute() {
        let null = Rc::new(RefCell::new(NullAudio {
            unlocked: true,
            ..Default::default()
        }));
        let mut m = AudioMixer::new(Box::new(Shared(null.clone())));
        m.set_volume(Some(Bus::Effects), 3.0);
        m.set_volume(None, f32::NAN);
        assert_eq!(m.volume(Some(Bus::Effects)), 1.0);
        assert_eq!(m.volume(None), 0.0);
        m.set_volume(None, 0.5);
        m.set_muted(true);
        m.update();
        assert_eq!(null.borrow().gains, [0.0, 0.7, 1.0]);
        m.set_muted(false);
        m.update();
        assert_eq!(null.borrow().gains[0], 0.5);
        m.set_hidden(true);
        assert!(null.borrow().suspended);
        assert_eq!(m.sound_id("x"), None);
    }

    #[test]
    fn a_voice_waits_for_the_gesture_then_follows_gain_and_rate() {
        let null = Rc::new(RefCell::new(NullAudio::default()));
        let mut m = AudioMixer::new(Box::new(Shared(null.clone())));
        let engine = m.add_sound("engine", Rc::from(vec![1u8]));
        let v = m.voice_start(engine, Bus::Effects, 0.4, 1.0);
        m.update();
        assert!(null.borrow().voice_starts.is_empty(), "locked: nothing starts");
        assert_eq!(m.voice_state(v), Some((0.4, 1.0)));

        null.borrow_mut().unlocked = true; // first user gesture
        m.update(); // the decode begins (NullAudio decodes at once)
        m.update();
        assert_eq!(null.borrow().voice_starts, vec![(v, engine, Bus::Effects)]);
        assert_eq!(null.borrow().voices, vec![(v, 0.4, 1.0)]);

        // The pitch follows what the game sets; an unchanged value sends nothing new.
        m.voice_set(v, 0.5, 2.5);
        m.update();
        assert_eq!(null.borrow().voices, vec![(v, 0.5, 2.5)]);
        m.voice_set(v, 0.5, 2.5);
        m.update();
        assert_eq!(null.borrow().voice_starts.len(), 1, "never restarted");

        // Bad values are clamped, never passed on.
        m.voice_set(v, f32::NAN, f32::INFINITY);
        m.update();
        assert_eq!(null.borrow().voices, vec![(v, 0.0, 1.0)]);
        m.voice_set(v, 1.0, 100.0);
        m.update();
        assert_eq!(null.borrow().voices, vec![(v, 1.0, VOICE_RATE_MAX)]);
    }

    #[test]
    fn stopping_a_voice_frees_it_and_the_slot_is_reused() {
        let null = Rc::new(RefCell::new(NullAudio {
            unlocked: true,
            ..Default::default()
        }));
        let mut m = AudioMixer::new(Box::new(Shared(null.clone())));
        let s = m.add_sound("s", Rc::from(vec![1u8]));
        m.update();
        let a = m.voice_start(s, Bus::Effects, 1.0, 1.0);
        let b = m.voice_start(s, Bus::Effects, 1.0, 1.0);
        m.update();
        assert_eq!((m.voice_count(), null.borrow().voices.len()), (2, 2));
        m.voice_stop(a);
        assert_eq!(m.voice_count(), 1);
        assert_eq!(m.voice_state(a), None);
        m.voice_set(a, 0.2, 2.0); // a stopped voice ignores changes
        m.update();
        assert_eq!(null.borrow().voices.iter().map(|v| v.0).collect::<Vec<_>>(), vec![b]);
        let c = m.voice_start(s, Bus::Music, 0.3, 1.5);
        assert_eq!(c, a, "the freed slot is reused");
        m.update();
        assert_eq!(null.borrow().voices.len(), 2);
        // A voice stopped before it ever started never reaches the backend.
        let d = m.voice_start(SoundId(99), Bus::Effects, 1.0, 1.0);
        m.voice_stop(d);
        m.update();
        assert_eq!(m.voice_count(), 2);
        assert_eq!(null.borrow().voices.len(), 2);
    }
}
