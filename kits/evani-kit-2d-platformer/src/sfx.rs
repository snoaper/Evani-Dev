//! Sound effects synthesised at start as small WAV files (no downloads):
//! jump, land, coin, stomp, hurt, bump, checkpoint, extra life, respawn,
//! win and lose. Played on the effects bus.

use std::rc::Rc;

use evani_core::audio::{AudioMixer, Bus, SoundId};

use crate::game::Event;

/// Sample rate of the synthesised sounds.
pub const RATE: u32 = 22_050;

/// Every sound the kit plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    Jump,
    Land,
    Coin,
    Stomp,
    Hurt,
    Bump,
    Checkpoint,
    Life,
    Respawn,
    Win,
    Lose,
}

impl Sfx {
    pub const ALL: [Sfx; 11] = [
        Sfx::Jump,
        Sfx::Land,
        Sfx::Coin,
        Sfx::Stomp,
        Sfx::Hurt,
        Sfx::Bump,
        Sfx::Checkpoint,
        Sfx::Life,
        Sfx::Respawn,
        Sfx::Win,
        Sfx::Lose,
    ];

    /// The sound a game event plays.
    pub fn of(e: Event) -> Sfx {
        match e {
            Event::Jump => Sfx::Jump,
            Event::Land => Sfx::Land,
            Event::Coin => Sfx::Coin,
            Event::Stomp => Sfx::Stomp,
            Event::Hurt => Sfx::Hurt,
            Event::Bump => Sfx::Bump,
            Event::Checkpoint => Sfx::Checkpoint,
            Event::Life => Sfx::Life,
            Event::Respawn => Sfx::Respawn,
            Event::Win => Sfx::Win,
            Event::Lose => Sfx::Lose,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Sfx::Jump => "platformer.jump",
            Sfx::Land => "platformer.land",
            Sfx::Coin => "platformer.coin",
            Sfx::Stomp => "platformer.stomp",
            Sfx::Hurt => "platformer.hurt",
            Sfx::Bump => "platformer.bump",
            Sfx::Checkpoint => "platformer.checkpoint",
            Sfx::Life => "platformer.life",
            Sfx::Respawn => "platformer.respawn",
            Sfx::Win => "platformer.win",
            Sfx::Lose => "platformer.lose",
        }
    }

    /// The sound's samples (-1..1, mono, [`RATE`]).
    pub fn samples(self) -> Vec<f32> {
        match self {
            Sfx::Jump => sweep(260.0, 620.0, 0.14, Wave::Square, 0.22, 0.0),
            Sfx::Land => noise(0.07, 0.35, 30.0),
            Sfx::Coin => tone(&[(988.0, 0.05), (1319.0, 0.16)], Wave::Square, 0.22),
            Sfx::Stomp => {
                let mut n = noise(0.12, 0.4, 20.0);
                let low = sweep(420.0, 110.0, 0.12, Wave::Square, 0.3, 0.0);
                for (a, b) in n.iter_mut().zip(low) {
                    *a = (*a + b).clamp(-1.0, 1.0);
                }
                n
            }
            Sfx::Hurt => sweep(700.0, 120.0, 0.45, Wave::Square, 0.3, 0.15),
            Sfx::Bump => tone(&[(140.0, 0.06)], Wave::Square, 0.3),
            Sfx::Checkpoint => tone(&[(659.0, 0.08), (880.0, 0.08), (1319.0, 0.16)], Wave::Triangle, 0.4),
            Sfx::Life => tone(&[(1319.0, 0.07), (1568.0, 0.07), (2637.0, 0.07), (2093.0, 0.07), (2349.0, 0.07), (3136.0, 0.12)], Wave::Square, 0.18),
            Sfx::Respawn => sweep(330.0, 990.0, 0.3, Wave::Triangle, 0.35, 0.0),
            Sfx::Win => tone(
                &[(523.0, 0.12), (659.0, 0.12), (784.0, 0.12), (1046.0, 0.2), (784.0, 0.1), (1046.0, 0.45)],
                Wave::Square,
                0.25,
            ),
            Sfx::Lose => tone(&[(494.0, 0.2), (466.0, 0.2), (440.0, 0.2), (415.0, 0.55)], Wave::Triangle, 0.4),
        }
    }
}



#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wave {
    Square,
    Triangle,
}

fn osc(w: Wave, phase: f32) -> f32 {
    let p = phase.fract();
    match w {
        Wave::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
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
            out.push(osc(w, phase) * env(i, n) * gain);
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
        Self { ids }
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
}
