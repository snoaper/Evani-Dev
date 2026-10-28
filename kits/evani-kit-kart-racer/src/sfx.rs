//! Sound effects synthesised at start as small WAV files (no downloads):
//! countdown beeps, item box, item use, drift tiers, boosts, hits, bumps,
//! blasts, laps and the finish. Played on the effects bus.

use std::rc::Rc;

use evani_core::audio::{AudioMixer, Bus, SoundId};

/// Sample rate of the synthesised sounds.
pub const RATE: u32 = 22_050;

/// Every sound the kit plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    Beep,
    Go,
    ItemBox,
    ItemReady,
    ItemUse,
    Hop,
    Tier,
    Boost,
    Hit,
    ShieldPop,
    Bump,
    Wall,
    Blast,
    Lap,
    Win,
    Lose,
    /// The incoming-item warning, three steps rising in pitch as the item nears.
    WarnFar,
    WarnNear,
    WarnNow,
}

impl Sfx {
    pub const ALL: [Sfx; 19] = [
        Sfx::Beep,
        Sfx::Go,
        Sfx::ItemBox,
        Sfx::ItemReady,
        Sfx::ItemUse,
        Sfx::Hop,
        Sfx::Tier,
        Sfx::Boost,
        Sfx::Hit,
        Sfx::ShieldPop,
        Sfx::Bump,
        Sfx::Wall,
        Sfx::Blast,
        Sfx::Lap,
        Sfx::Win,
        Sfx::Lose,
        Sfx::WarnFar,
        Sfx::WarnNear,
        Sfx::WarnNow,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Sfx::Beep => "kart_racer.beep",
            Sfx::Go => "kart_racer.go",
            Sfx::ItemBox => "kart_racer.item_box",
            Sfx::ItemReady => "kart_racer.item_ready",
            Sfx::ItemUse => "kart_racer.item_use",
            Sfx::Hop => "kart_racer.hop",
            Sfx::Tier => "kart_racer.tier",
            Sfx::Boost => "kart_racer.boost",
            Sfx::Hit => "kart_racer.hit",
            Sfx::ShieldPop => "kart_racer.shield_pop",
            Sfx::Bump => "kart_racer.bump",
            Sfx::Wall => "kart_racer.wall",
            Sfx::Blast => "kart_racer.blast",
            Sfx::Lap => "kart_racer.lap",
            Sfx::Win => "kart_racer.win",
            Sfx::Lose => "kart_racer.lose",
            Sfx::WarnFar => "kart_racer.warn_far",
            Sfx::WarnNear => "kart_racer.warn_near",
            Sfx::WarnNow => "kart_racer.warn_now",
        }
    }

    /// The sound's samples (-1..1, mono, [`RATE`]).
    pub fn samples(self) -> Vec<f32> {
        match self {
            Sfx::Beep => tone(&[(660.0, 0.16)], Wave::Square, 0.35),
            Sfx::Go => tone(&[(1320.0, 0.42)], Wave::Square, 0.35),
            Sfx::ItemBox => tone(&[(880.0, 0.05), (1175.0, 0.05), (1480.0, 0.05), (1760.0, 0.09)], Wave::Triangle, 0.45),
            Sfx::ItemReady => tone(&[(1568.0, 0.06), (2093.0, 0.12)], Wave::Sine, 0.4),
            Sfx::ItemUse => sweep(300.0, 1200.0, 0.22, Wave::Triangle, 0.4, 0.3),
            Sfx::Hop => sweep(220.0, 440.0, 0.1, Wave::Sine, 0.4, 0.0),
            Sfx::Tier => tone(&[(1046.0, 0.05), (1568.0, 0.1)], Wave::Square, 0.25),
            Sfx::Boost => sweep(120.0, 520.0, 0.55, Wave::Saw, 0.3, 0.55),
            Sfx::Hit => sweep(500.0, 90.0, 0.5, Wave::Square, 0.35, 0.35),
            Sfx::ShieldPop => sweep(900.0, 1800.0, 0.12, Wave::Sine, 0.4, 0.2),
            Sfx::Bump => noise(0.12, 0.45, 18.0),
            Sfx::Wall => noise(0.2, 0.5, 12.0),
            Sfx::Blast => {
                let mut n = noise(0.8, 0.7, 4.5);
                let low = sweep(160.0, 40.0, 0.8, Wave::Sine, 0.6, 0.0);
                for (a, b) in n.iter_mut().zip(low) {
                    *a = (*a + b).clamp(-1.0, 1.0);
                }
                n
            }
            Sfx::Lap => tone(&[(784.0, 0.1), (988.0, 0.1), (1175.0, 0.18)], Wave::Triangle, 0.4),
            Sfx::Win => tone(
                &[(523.0, 0.12), (659.0, 0.12), (784.0, 0.12), (1046.0, 0.2), (784.0, 0.1), (1046.0, 0.4)],
                Wave::Square,
                0.3,
            ),
            Sfx::Lose => tone(&[(392.0, 0.2), (370.0, 0.2), (349.0, 0.2), (330.0, 0.5)], Wave::Triangle, 0.4),
            // Two quick blips; each step sits higher than the last.
            Sfx::WarnFar => tone(&[(740.0, 0.07), (0.0, 0.03), (740.0, 0.07)], Wave::Square, 0.3),
            Sfx::WarnNear => tone(&[(988.0, 0.07), (0.0, 0.03), (988.0, 0.07)], Wave::Square, 0.32),
            Sfx::WarnNow => tone(&[(1319.0, 0.07), (0.0, 0.03), (1319.0, 0.07)], Wave::Square, 0.35),
        }
    }
}

/// The looping sounds, played on voices whose gain and pitch the game sets
/// every frame (see `voices.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loop {
    /// The kart's engine: pitch follows speed.
    Engine,
    /// Smooth tarmac under the wheels.
    Road,
    /// Rustle and grit on the grass.
    Grass,
    /// The kerb's rumble.
    Kerb,
}

/// Samples in one engine cycle: 22 050 / 276 = 79.9 Hz at playback rate 1.
pub const ENGINE_CYCLE: usize = 276;
/// Engine cycles in the loop (so the loop joins up with no click).
const ENGINE_CYCLES: usize = 24;
/// Samples in each surface loop (half a second).
const SURFACE_LEN: usize = 11_025;

impl Loop {
    pub const ALL: [Loop; 4] = [Loop::Engine, Loop::Road, Loop::Grass, Loop::Kerb];

    pub fn name(self) -> &'static str {
        match self {
            Loop::Engine => "kart_racer.engine",
            Loop::Road => "kart_racer.road",
            Loop::Grass => "kart_racer.grass",
            Loop::Kerb => "kart_racer.kerb",
        }
    }

    /// The loop's samples (-1..1, mono, [`RATE`]); the last sample runs
    /// straight into the first.
    pub fn samples(self) -> Vec<f32> {
        match self {
            Loop::Engine => engine_loop(),
            Loop::Road => {
                // Soft low hiss with a faint 100 Hz hum.
                let mut v = seamless_noise(0.10, 0.5, 0x2468_ACE1);
                for (i, x) in v.iter_mut().enumerate() {
                    *x += 0.12 * (i as f32 * 100.0 / RATE as f32 * core::f32::consts::TAU).sin();
                }
                v
            }
            Loop::Grass => {
                // Bright hiss, flickering, with sparse crackles.
                let mut v = seamless_noise(0.6, 0.55, 0x1357_9BDF);
                let flutter = seamless_noise(0.002, 9.0, 0x0F0F_1234);
                let mut rng = 0xBEEF_0042u32;
                let mut click = 0.0f32;
                for (x, f) in v.iter_mut().zip(flutter) {
                    if (next_noise(&mut rng) + 1.0) * 0.5 < 0.004 {
                        click = 0.9;
                    }
                    click *= 0.97;
                    *x = *x * (0.65 + 0.35 * f) + click * next_noise(&mut rng);
                }
                v
            }
            Loop::Kerb => {
                // A 40 Hz buzz (20 cycles in the loop), softened, over a little grit.
                let grit = seamless_noise(0.3, 0.25, 0x7777_1111);
                // The softening filter runs one lap first, so the loop starts
                // where it ends.
                let mut lp = 0.0f32;
                let mut out = Vec::with_capacity(SURFACE_LEN);
                for i in 0..SURFACE_LEN * 2 {
                    let j = i % SURFACE_LEN;
                    let sq = if (j as f32 * 40.0 / RATE as f32).fract() < 0.5 { 1.0 } else { -1.0 };
                    lp += (sq - lp) * 0.12;
                    if i >= SURFACE_LEN {
                        out.push((lp * 0.8 + grit[j]).clamp(-1.0, 1.0));
                    }
                }
                out
            }
        }
    }
}

/// An engine: a buzzy waveform with a burble, exactly [`ENGINE_CYCLES`]
/// cycles long. Every part repeats in the loop, so it joins up with no click.
fn engine_loop() -> Vec<f32> {
    let n = ENGINE_CYCLE * ENGINE_CYCLES;
    let tau = core::f32::consts::TAU;
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let ph = (i % ENGINE_CYCLE) as f32 / ENGINE_CYCLE as f32;
            let mut s = 0.0;
            for k in 1..=12u32 {
                let kf = k as f32;
                s += (tau * kf * ph + kf * kf * 0.35).sin() / kf.powf(1.05);
            }
            // The exhaust's burble: four swells in the loop.
            let burble = 1.0 + 0.2 * (tau * 4.0 * i as f32 / n as f32).sin();
            (s * 1.6).tanh() * burble
        })
        .collect();
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-3);
    for v in &mut out {
        *v = *v / peak * 0.9;
    }
    out
}

/// Low-passed noise ([`SURFACE_LEN`] samples; `lp` is the filter's step, 0..1)
/// whose end blends into its start so the loop has no seam.
fn seamless_noise(lp: f32, gain: f32, seed: u32) -> Vec<f32> {
    let fade = SURFACE_LEN / 8;
    let mut rng = seed;
    let mut y = 0.0f32;
    let raw: Vec<f32> = (0..SURFACE_LEN + fade)
        .map(|_| {
            y += (next_noise(&mut rng) - y) * lp;
            // Keep the loudness steady whatever the filter.
            y * gain * (1.0 / lp.sqrt()).min(4.0) * 0.6
        })
        .collect();
    (0..SURFACE_LEN)
        .map(|i| {
            let v = if i < fade {
                let t = i as f32 / fade as f32;
                raw[i] * t + raw[SURFACE_LEN + i] * (1.0 - t)
            } else {
                raw[i]
            };
            v.clamp(-1.0, 1.0)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wave {
    Sine,
    Square,
    Triangle,
    Saw,
}

fn osc(w: Wave, phase: f32) -> f32 {
    let p = phase.fract();
    match w {
        Wave::Sine => (p * core::f32::consts::TAU).sin(),
        Wave::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
        Wave::Saw => 2.0 * p - 1.0,
    }
}

/// Short attack and release around each note.
fn env(i: usize, n: usize) -> f32 {
    let a = (i as f32 / (RATE as f32 * 0.005)).min(1.0);
    let r = ((n - i) as f32 / (RATE as f32 * 0.03)).min(1.0);
    a * r
}

/// Notes `(Hz, seconds)` played one after another.
fn tone(notes: &[(f32, f32)], w: Wave, gain: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &(f, d) in notes {
        let n = (d * RATE as f32) as usize;
        let mut phase = 0.0f32;
        for i in 0..n {
            phase += f / RATE as f32;
            // A rest (0 Hz) is silence, not the wave's resting value.
            out.push(if f > 0.0 { osc(w, phase) * env(i, n) * gain } else { 0.0 });
        }
    }
    out
}

/// A pitch sweep with an exponential fade and optional noise mixed in.
fn sweep(f0: f32, f1: f32, d: f32, w: Wave, gain: f32, noise_mix: f32) -> Vec<f32> {
    let n = (d * RATE as f32) as usize;
    let mut phase = 0.0f32;
    let mut rng = 0x1234_5678u32;
    (0..n)
        .map(|i| {
            let t = i as f32 / n.max(1) as f32;
            let f = f0 * (f1 / f0).powf(t);
            phase += f / RATE as f32;
            let nz = next_noise(&mut rng);
            let s = osc(w, phase) * (1.0 - noise_mix) + nz * noise_mix;
            s * env(i, n) * gain * (1.0 - t).powf(1.5)
        })
        .collect()
}

/// A noise burst with exponential decay (`decay` per second), low-passed.
fn noise(d: f32, gain: f32, decay: f32) -> Vec<f32> {
    let n = (d * RATE as f32) as usize;
    let mut rng = 0x9E37_79B9u32;
    let mut lp = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            lp += (next_noise(&mut rng) - lp) * 0.35;
            lp * gain * (-decay * t).exp() * env(i, n) * 2.0
        })
        .collect()
}

fn next_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
}

/// Encode mono samples as a 16-bit PCM WAV file.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// The registered sounds (resource-free: kept in the draw state).
#[derive(Debug, Clone, Default)]
pub struct Sounds {
    ids: Vec<SoundId>,
    loops: Vec<SoundId>,
}

impl Sounds {
    /// Synthesise and register every sound with the mixer.
    pub fn register(mixer: &mut AudioMixer) -> Self {
        let ids = Sfx::ALL
            .iter()
            .map(|s| {
                let bytes: Rc<[u8]> = wav(&s.samples()).into();
                mixer.add_sound(s.name(), bytes)
            })
            .collect();
        let loops = Loop::ALL
            .iter()
            .map(|l| {
                let bytes: Rc<[u8]> = wav(&l.samples()).into();
                mixer.add_sound(l.name(), bytes)
            })
            .collect();
        Self { ids, loops }
    }

    /// The registered sound of a looping voice.
    pub fn loop_id(&self, l: Loop) -> Option<SoundId> {
        let i = Loop::ALL.iter().position(|x| *x == l)?;
        self.loops.get(i).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn play(&self, mixer: &mut AudioMixer, s: Sfx, gain: f32) {
        let i = Sfx::ALL.iter().position(|x| *x == s).unwrap_or(0);
        if let Some(&id) = self.ids.get(i) {
            mixer.play_on(id, Bus::Effects, gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_a_short_valid_wav() {
        for s in Sfx::ALL {
            let smp = s.samples();
            assert!(!smp.is_empty(), "{s:?}");
            assert!(smp.len() < RATE as usize * 2, "{s:?} too long");
            assert!(smp.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{s:?}");
            let w = wav(&smp);
            assert_eq!(&w[0..4], b"RIFF");
            assert_eq!(&w[8..12], b"WAVE");
            assert_eq!(w.len(), 44 + smp.len() * 2);
        }
    }

    #[test]
    fn the_loops_are_valid_and_join_up_without_a_click() {
        for l in Loop::ALL {
            let smp = l.samples();
            assert!(smp.len() >= RATE as usize / 4 && smp.len() <= RATE as usize, "{l:?} length {}", smp.len());
            assert!(smp.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{l:?}");
            let rms = (smp.iter().map(|v| v * v).sum::<f32>() / smp.len() as f32).sqrt();
            assert!(rms > 0.05, "{l:?} is too quiet: {rms}");
            // The jump from the last sample to the first is no bigger than
            // the biggest step inside the loop.
            let step = |a: f32, b: f32| (a - b).abs();
            let worst = smp.windows(2).map(|w| step(w[0], w[1])).fold(0.0f32, f32::max);
            let seam = step(smp[smp.len() - 1], smp[0]);
            assert!(seam <= worst + 1e-3, "{l:?} clicks at the seam: {seam} vs {worst}");
        }
    }

    #[test]
    fn engine_cycles_repeat_exactly() {
        let e = Loop::Engine.samples();
        assert_eq!(e.len(), ENGINE_CYCLE * ENGINE_CYCLES);
        // The waveform shifts only by the slow burble, so cycle 0 and cycle 6
        // (one burble period on) are the same.
        let six = ENGINE_CYCLE * 6;
        for i in 0..ENGINE_CYCLE {
            assert!((e[i] - e[i + six]).abs() < 1e-4, "{i}");
        }
    }

    #[test]
    fn the_surfaces_sound_different() {
        // Brightness: mean absolute step against mean absolute level. Grass
        // hisses (bright), the road is smooth, the kerb is a low buzz.
        let bright = |l: Loop| {
            let s = l.samples();
            let level = s.iter().map(|v| v.abs()).sum::<f32>() / s.len() as f32;
            let step = s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / s.len() as f32;
            step / level.max(1e-4)
        };
        assert!(bright(Loop::Grass) > bright(Loop::Road) * 1.5, "{} vs {}", bright(Loop::Grass), bright(Loop::Road));
        assert!(bright(Loop::Road) > bright(Loop::Kerb) * 3.0, "the kerb is the low buzz");
        let rms = |l: Loop| {
            let s = l.samples();
            (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
        };
        assert!(rms(Loop::Grass) > rms(Loop::Road));
    }

    #[test]
    fn the_warning_steps_rise_in_pitch() {
        // Count zero crossings: a higher blip crosses more often.
        let cross = |s: Sfx| s.samples().windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        assert!(cross(Sfx::WarnFar) < cross(Sfx::WarnNear));
        assert!(cross(Sfx::WarnNear) < cross(Sfx::WarnNow));
    }
}
