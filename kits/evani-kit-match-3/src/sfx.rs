//! Sound effects synthesised at start as small WAV files (no downloads):
//! pick, swap, bounce-back, pops pitched up per cascade, Streak zap, Pop
//! Bomb thump, Prism shimmer, special made, landing tick, frosting crack,
//! shuffle, star, goal, level start, win and lose. Played on the effects bus.

use std::rc::Rc;

use evani_core::audio::{AudioMixer, Bus, SoundId};

/// Sample rate of the synthesised sounds.
pub const RATE: u32 = 22_050;

/// Pop sounds, one per cascade depth (pitch rises).
pub const POP_LEVELS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    Select,
    Swap,
    Invalid,
    Pop(u8),
    Streak,
    Bomb,
    Prism,
    Special,
    Land,
    Frost,
    Shuffle,
    Star,
    Goal,
    Start,
    Win,
    Lose,
}

impl Sfx {
    pub const ALL: [Sfx; 20] = [
        Sfx::Select,
        Sfx::Swap,
        Sfx::Invalid,
        Sfx::Pop(0),
        Sfx::Pop(1),
        Sfx::Pop(2),
        Sfx::Pop(3),
        Sfx::Pop(4),
        Sfx::Streak,
        Sfx::Bomb,
        Sfx::Prism,
        Sfx::Special,
        Sfx::Land,
        Sfx::Frost,
        Sfx::Shuffle,
        Sfx::Star,
        Sfx::Goal,
        Sfx::Start,
        Sfx::Win,
        Sfx::Lose,
    ];

    pub fn name(self) -> String {
        match self {
            Sfx::Pop(n) => format!("match_3.pop{n}"),
            other => format!("match_3.{}", format!("{other:?}").to_lowercase()),
        }
    }

    /// The sound's samples (-1..1, mono, [`RATE`]).
    pub fn samples(self) -> Vec<f32> {
        match self {
            Sfx::Select => tone(&[(988.0, 0.05)], Wave::Sine, 0.35),
            Sfx::Swap => sweep(420.0, 760.0, 0.12, Wave::Triangle, 0.35, 0.1),
            Sfx::Invalid => tone(&[(220.0, 0.08), (196.0, 0.12)], Wave::Square, 0.18),
            Sfx::Pop(n) => {
                // A bubbly pop: a fast downward chirp over a little noise,
                // up a whole tone per cascade.
                let f = 660.0 * 2f32.powf(f32::from(n) * 2.0 / 12.0);
                let mut a = sweep(f * 1.6, f, 0.11, Wave::Sine, 0.55, 0.12);
                let b = tone(&[(f * 2.0, 0.05)], Wave::Triangle, 0.15);
                mix(&mut a, &b);
                a
            }
            Sfx::Streak => sweep(1800.0, 300.0, 0.32, Wave::Saw, 0.28, 0.25),
            Sfx::Bomb => {
                let mut n = noise(0.45, 0.6, 7.0);
                let low = sweep(140.0, 45.0, 0.45, Wave::Sine, 0.7, 0.0);
                mix(&mut n, &low);
                n
            }
            Sfx::Prism => {
                let mut a = tone(
                    &[(1046.0, 0.07), (1318.0, 0.07), (1568.0, 0.07), (2093.0, 0.07), (2637.0, 0.16)],
                    Wave::Sine,
                    0.35,
                );
                let b = sweep(200.0, 1600.0, 0.44, Wave::Triangle, 0.2, 0.2);
                mix(&mut a, &b);
                a
            }
            Sfx::Special => tone(&[(784.0, 0.06), (1175.0, 0.06), (1568.0, 0.14)], Wave::Triangle, 0.35),
            Sfx::Land => noise(0.04, 0.25, 60.0),
            Sfx::Frost => {
                let mut n = noise(0.12, 0.45, 30.0);
                let b = tone(&[(2400.0, 0.04)], Wave::Square, 0.08);
                mix(&mut n, &b);
                n
            }
            Sfx::Shuffle => sweep(300.0, 900.0, 0.5, Wave::Triangle, 0.3, 0.4),
            Sfx::Star => tone(&[(1318.0, 0.08), (1760.0, 0.08), (2637.0, 0.2)], Wave::Sine, 0.4),
            Sfx::Goal => tone(&[(880.0, 0.08), (1109.0, 0.08), (1318.0, 0.16)], Wave::Square, 0.2),
            Sfx::Start => tone(&[(523.0, 0.1), (659.0, 0.1), (784.0, 0.18)], Wave::Triangle, 0.35),
            Sfx::Win => tone(
                &[(523.0, 0.1), (659.0, 0.1), (784.0, 0.1), (1046.0, 0.18), (784.0, 0.08), (1046.0, 0.4)],
                Wave::Square,
                0.25,
            ),
            Sfx::Lose => tone(&[(392.0, 0.18), (370.0, 0.18), (349.0, 0.18), (294.0, 0.5)], Wave::Triangle, 0.4),
        }
    }
}

fn mix(a: &mut Vec<f32>, b: &[f32]) {
    if a.len() < b.len() {
        a.resize(b.len(), 0.0);
    }
    for (x, y) in a.iter_mut().zip(b) {
        *x = (*x + y).clamp(-1.0, 1.0);
    }
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
    let a = (i as f32 / (RATE as f32 * 0.004)).min(1.0);
    let r = ((n - i) as f32 / (RATE as f32 * 0.025)).min(1.0);
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
            out.push(osc(w, phase) * env(i, n) * gain);
        }
    }
    out
}

/// A pitch sweep with a fade and optional noise mixed in.
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

/// A low-passed noise burst with exponential decay (`decay` per second).
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
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
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

/// The registered sounds.
#[derive(Debug, Clone, Default)]
pub struct Sounds {
    ids: Vec<SoundId>,
}

impl Sounds {
    /// Synthesise and register every sound with the mixer.
    pub fn register(mixer: &mut AudioMixer) -> Self {
        let ids = Sfx::ALL
            .iter()
            .map(|s| {
                let bytes: Rc<[u8]> = wav(&s.samples()).into();
                mixer.add_sound(&s.name(), bytes)
            })
            .collect();
        Self { ids }
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn play(&self, mixer: &mut AudioMixer, s: Sfx, gain: f32) {
        let s = match s {
            Sfx::Pop(n) => Sfx::Pop(n.min(POP_LEVELS as u8 - 1)),
            other => other,
        };
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
    fn every_sound_is_a_short_valid_wav_with_a_unique_name() {
        let mut names = Vec::new();
        for s in Sfx::ALL {
            let smp = s.samples();
            assert!(!smp.is_empty(), "{s:?}");
            assert!(smp.len() < RATE as usize * 2, "{s:?} too long");
            assert!(smp.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{s:?}");
            let w = wav(&smp);
            assert_eq!(&w[0..4], b"RIFF");
            assert_eq!(w.len(), 44 + smp.len() * 2);
            names.push(s.name());
        }
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
        assert!(names.contains(&"match_3.pop4".to_string()));
        assert!(names.contains(&"match_3.streak".to_string()));
    }
}
