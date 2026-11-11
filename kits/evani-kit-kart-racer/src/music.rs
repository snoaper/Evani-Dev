//! Sugarpeak's music: a bright title theme, a bouncy race theme and short
//! jingles (countdown, final lap, win, placed), all original, composed here
//! as note patterns and played by a tiny synth at start (no files, nothing to
//! download). The themes loop without a seam: every note's tail that runs
//! past the end is added back at the start.
//!
//! The music runs on the mixer's music bus, so the page's Music slider and
//! mute reach it, and the mixer holds it back until the first user gesture
//! unlocks audio. The themes play on looping voices: the title theme on the
//! title screen and results, the race theme from GO, a final-lap fanfare when
//! a local player starts the last lap and then the race theme one semitone
//! faster, and the win or placed jingle when the local players are over the
//! line. Pausing silences the theme; the tab going hidden suspends all audio.

use std::rc::Rc;

use evani_core::audio::{AudioMixer, Bus, SoundId, VoiceId};

use crate::race::{Phase, RaceView};
use crate::sfx::{RATE, wav};

const SR: f32 = RATE as f32;
const TAU: f32 = core::f32::consts::TAU;

/// The looping themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Song {
    /// Title screen and results: 126 BPM, F major, 16 bars (30.5 s).
    Title,
    /// The race: 147 BPM, C major, 16 bars (26.1 s).
    Race,
}

/// The one-shot jingles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jingle {
    /// Three rising chords on the countdown beeps and a drum roll into GO.
    Start,
    /// The final-lap fanfare.
    FinalLap,
    /// Over the line in the top three.
    Win,
    /// Over the line further back: a light "nice try".
    Lose,
}

impl Song {
    pub const ALL: [Song; 2] = [Song::Title, Song::Race];

    pub fn name(self) -> &'static str {
        match self {
            Song::Title => "kart_racer.music.title",
            Song::Race => "kart_racer.music.race",
        }
    }

    /// The loop's samples (-1..1, mono, [`RATE`]); the last runs into the first.
    pub fn samples(self) -> Vec<f32> {
        self.mix().finish()
    }

    fn mix(self) -> Mix {
        match self {
            Song::Title => title_theme(),
            Song::Race => race_theme(),
        }
    }
}

impl Jingle {
    pub const ALL: [Jingle; 4] = [Jingle::Start, Jingle::FinalLap, Jingle::Win, Jingle::Lose];

    pub fn name(self) -> &'static str {
        match self {
            Jingle::Start => "kart_racer.music.start",
            Jingle::FinalLap => "kart_racer.music.final_lap",
            Jingle::Win => "kart_racer.music.win",
            Jingle::Lose => "kart_racer.music.lose",
        }
    }

    pub fn samples(self) -> Vec<f32> {
        self.mix().finish()
    }

    fn mix(self) -> Mix {
        match self {
            Jingle::Start => start_jingle(),
            Jingle::FinalLap => final_lap_jingle(),
            Jingle::Win => win_jingle(),
            Jingle::Lose => lose_jingle(),
        }
    }
}

// ---------------------------------------------------------------- notation

/// MIDI note number of a name like `C5`, `F#4` or `Bb3`.
fn midi(name: &str) -> Option<i32> {
    let b = name.as_bytes();
    let pc = match b.first()? {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let (acc, rest) = match b.get(1) {
        Some(b'#') => (1, &name[2..]),
        Some(b'b') => (-1, &name[2..]),
        _ => (0, &name[1..]),
    };
    let oct: i32 = rest.parse().ok()?;
    Some(12 * (oct + 1) + pc + acc)
}

fn hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

/// A note: start step, length in steps, MIDI number.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Note {
    step: usize,
    len: usize,
    midi: i32,
}

/// One token per step: a note name starts a note, `-` holds the one before,
/// `.` is a rest.
fn parse(bars: &[&str]) -> Vec<Note> {
    let mut out: Vec<Note> = Vec::new();
    let mut holding = false;
    for (step, tok) in bars.iter().flat_map(|b| b.split_whitespace()).enumerate() {
        match tok {
            "-" => {
                if holding && let Some(n) = out.last_mut() {
                    n.len += 1;
                }
            }
            "." => holding = false,
            name => {
                if let Some(m) = midi(name) {
                    out.push(Note { step, len: 1, midi: m });
                    holding = true;
                }
            }
        }
    }
    out
}

/// A chord: root pitch class and whether it is minor.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Chord {
    root: i32,
    minor: bool,
}

fn chord(name: &str) -> Chord {
    let minor = name.ends_with('m');
    let root = midi(&format!("{}0", name.trim_end_matches('m'))).unwrap_or(12) - 12;
    Chord { root, minor }
}

impl Chord {
    /// The triad in close position between G3 and F#4 (MIDI 55..67).
    fn triad(self) -> [i32; 3] {
        let third = if self.minor { 3 } else { 4 };
        [0, third, 7].map(|iv| {
            let mut m = 48 + self.root + iv;
            while m < 55 {
                m += 12;
            }
            while m >= 67 {
                m -= 12;
            }
            m
        })
    }
    /// The bass root around C2..B2.
    fn bass(self) -> i32 {
        36 + self.root
    }
    fn third(self) -> i32 {
        if self.minor { 3 } else { 4 }
    }
}

/// Chords per bar: `"F G"` splits the bar in half. Returns (step, chord) per
/// half bar (16 steps a bar).
fn chord_track(bars: &[&str]) -> Vec<(usize, usize, Chord)> {
    let mut out = Vec::new();
    for (b, bar) in bars.iter().enumerate() {
        let names: Vec<&str> = bar.split_whitespace().collect();
        let each = 16 / names.len().max(1);
        for (k, n) in names.iter().enumerate() {
            out.push((b * 16 + k * each, each, chord(n)));
        }
    }
    out
}

// ---------------------------------------------------------------- the synth

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inst {
    /// Bright pulse lead with a little vibrato.
    Lead,
    /// Glockenspiel: sparkly sugar bells.
    Bell,
    /// Round plucky bass.
    Bass,
    /// Short bright chord stab.
    Stab,
    /// Soft wooden marimba.
    Marimba,
    /// Fanfare brass.
    Brass,
    Kick,
    Snare,
    Hat,
    OpenHat,
    Crash,
}

fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Band-limited saw at phase `p` (0..1) with phase step `dt`.
fn saw(p: f32, dt: f32) -> f32 {
    2.0 * p - 1.0 - blep(p, dt)
}

/// Band-limited pulse of duty `d`.
fn pulse(p: f32, dt: f32, d: f32) -> f32 {
    let naive = if p < d { 1.0 } else { -1.0 };
    naive + blep(p, dt) - blep((p + 1.0 - d).fract(), dt)
}

/// A sine by rotation (no `sin` per sample) with an exponential decay.
struct Partial {
    c: f32,
    s: f32,
    cr: f32,
    sr: f32,
    amp: f32,
    fall: f32,
}

impl Partial {
    /// `f` Hz, starting level `amp`, falling by `decay` per second.
    fn new(f: f32, amp: f32, decay: f32) -> Self {
        let w = TAU * f / SR;
        Self { c: 1.0, s: 0.0, cr: w.cos(), sr: w.sin(), amp, fall: (-decay / SR).exp() }
    }
    fn next(&mut self) -> f32 {
        let (c, s) = (self.c * self.cr - self.s * self.sr, self.c * self.sr + self.s * self.cr);
        self.c = c;
        self.s = s;
        let out = s * self.amp;
        self.amp *= self.fall;
        out
    }
    /// Pull the rotation back onto the unit circle (call now and then).
    fn tidy(&mut self) {
        let m = (self.c * self.c + self.s * self.s).sqrt().max(1e-6);
        self.c /= m;
        self.s /= m;
    }
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// Attack, decay to `sustain`, hold for `hold` seconds, release.
fn adsr(t: f32, a: f32, d: f32, sustain: f32, hold: f32, r: f32) -> f32 {
    let on = if t < a {
        t / a
    } else {
        sustain + (1.0 - sustain) * (-(t - a) / d.max(1e-4)).exp()
    };
    if t <= hold {
        on
    } else {
        let at = if hold < a { hold / a } else { sustain + (1.0 - sustain) * (-(hold - a) / d.max(1e-4)).exp() };
        (at * (1.0 - (t - hold) / r)).max(0.0)
    }
}

/// One note of an instrument at `f` Hz held `hold` seconds (drums ignore
/// both). `seed` varies the noise.
fn voice(inst: Inst, f: f32, hold: f32, seed: u32) -> Vec<f32> {
    let dt = f / SR;
    let mut nz = Noise(seed.wrapping_mul(2_654_435_761).max(1));
    let len = |s: f32| (s * SR) as usize;
    let mut out;
    match inst {
        Inst::Lead => {
            let r = 0.07;
            let n = len(hold + r);
            out = Vec::with_capacity(n);
            let (mut p1, mut p2, mut lp) = (0.0f32, 0.37f32, 0.0f32);
            let mut lfo = Partial::new(5.5, 1.0, 0.0);
            for i in 0..n {
                let t = i as f32 / SR;
                let vib = 1.0 + 0.0045 * lfo.next() * (t / 0.25).min(1.0);
                let d1 = dt * vib;
                let d2 = d1 * 1.004;
                p1 = (p1 + d1).fract();
                p2 = (p2 + d2).fract();
                let s = pulse(p1, d1, 0.25) * 0.7 + saw(p2, d2) * 0.3;
                lp += (s - lp) * 0.55;
                out.push(lp * adsr(t, 0.006, 0.12, 0.75, hold, r));
            }
        }
        Inst::Bell => {
            let n = len(0.9);
            let mut ps = [Partial::new(f, 1.0, 4.0), Partial::new(f * 2.76, 0.35, 9.0), Partial::new(f * 5.4, 0.12, 16.0)];
            out = (0..n)
                .map(|i| {
                    if i % 2048 == 0 {
                        ps.iter_mut().for_each(Partial::tidy);
                    }
                    let a = (i as f32 / (0.002 * SR)).min(1.0);
                    let s: f32 = ps.iter_mut().map(Partial::next).sum();
                    s * a * 0.8
                })
                .collect();
        }
        Inst::Bass => {
            let r = 0.04;
            let n = len(hold + r);
            out = Vec::with_capacity(n);
            let (mut p, mut lp) = (0.0f32, 0.0f32);
            for i in 0..n {
                let t = i as f32 / SR;
                p = (p + dt).fract();
                let tri = 1.0 - 4.0 * (p - 0.5).abs();
                let s = tri * 0.8 + pulse(p, dt, 0.5) * 0.25;
                lp += (s - lp) * 0.3;
                out.push(lp * adsr(t, 0.004, 0.15, 0.6, hold, r));
            }
        }
        Inst::Stab => {
            let n = len(hold.min(0.25) + 0.05);
            out = Vec::with_capacity(n);
            let (mut p1, mut p2, mut lp) = (0.0f32, 0.5f32, 0.0f32);
            let (mut bright, mut env) = (0.5f32, 1.0f32);
            let (bf, ef) = ((-25.0 / SR).exp(), (-9.0 / SR).exp());
            let (d1, d2) = (dt * 0.994, dt * 1.006);
            for i in 0..n {
                p1 = (p1 + d1).fract();
                p2 = (p2 + d2).fract();
                let s = (saw(p1, d1) + saw(p2, d2)) * 0.5;
                lp += (s - lp) * (0.08 + bright);
                bright *= bf;
                let fade = ((n - i) as f32 / (0.01 * SR)).min(1.0);
                out.push(lp * (i as f32 / (0.003 * SR)).min(1.0) * env * fade);
                env *= ef;
            }
        }
        Inst::Marimba => {
            let n = len(0.6);
            let mut ps = [Partial::new(f, 1.0, 7.0), Partial::new(f * 4.0, 0.25, 22.0)];
            out = (0..n)
                .map(|i| {
                    if i % 2048 == 0 {
                        ps.iter_mut().for_each(Partial::tidy);
                    }
                    let a = (i as f32 / (0.002 * SR)).min(1.0);
                    let s: f32 = ps.iter_mut().map(Partial::next).sum();
                    s * a
                })
                .collect();
        }
        Inst::Brass => {
            let r = 0.1;
            let n = len(hold + r);
            out = Vec::with_capacity(n);
            let (mut p1, mut p2, mut lp) = (0.0f32, 0.3f32, 0.0f32);
            let mut lfo = Partial::new(5.0, 1.0, 0.0);
            for i in 0..n {
                let t = i as f32 / SR;
                let vib = 1.0 + 0.004 * lfo.next() * ((t - 0.2) / 0.3).clamp(0.0, 1.0);
                let (d1, d2) = (dt * vib * 0.997, dt * vib * 1.003);
                p1 = (p1 + d1).fract();
                p2 = (p2 + d2).fract();
                let s = (saw(p1, d1) + saw(p2, d2)) * 0.5;
                // The filter opens with the attack, like a player blowing in.
                let k = 0.12 + 0.3 * (t / 0.05).min(1.0);
                lp += (s - lp) * k;
                out.push(lp * adsr(t, 0.03, 0.2, 0.8, hold, r));
            }
        }
        Inst::Kick => {
            let n = len(0.3);
            let mut ph = 0.0f32;
            out = (0..n)
                .map(|i| {
                    let t = i as f32 / SR;
                    let f = 45.0 + 115.0 * (-t * 30.0).exp();
                    ph += f / SR;
                    let click = if i < 40 { nz.next() * 0.3 * (1.0 - i as f32 / 40.0) } else { 0.0 };
                    ((TAU * ph).sin() + click) * (-t * 9.0).exp()
                })
                .collect();
        }
        Inst::Snare => {
            let n = len(0.25);
            let mut lp = 0.0f32;
            let mut body = Partial::new(185.0, 0.5, 25.0);
            let mut env = 0.8f32;
            let fall = (-18.0 / SR).exp();
            out = (0..n)
                .map(|_| {
                    let x = nz.next();
                    lp += (x - lp) * 0.25;
                    let hiss = (x - lp) * env;
                    env *= fall;
                    hiss + body.next()
                })
                .collect();
        }
        Inst::Hat | Inst::OpenHat | Inst::Crash => {
            let (dur, decay) = match inst {
                Inst::Hat => (0.06, 60.0),
                Inst::OpenHat => (0.3, 12.0),
                _ => (1.6, 2.6),
            };
            let n = len(dur);
            let mut prev = 0.0f32;
            let mut env = 0.5f32;
            let fall = (-decay / SR).exp();
            out = (0..n)
                .map(|i| {
                    let x = nz.next();
                    let hp = x - prev;
                    prev = x;
                    let fade = ((n - i) as f32 / (0.005 * SR)).min(1.0);
                    let v = hp * env * fade;
                    env *= fall;
                    v
                })
                .collect();
        }
    }
    out
}

/// One note waiting to be played into a [`Mix`].
#[derive(Debug, Clone, Copy)]
struct Ev {
    inst: Inst,
    at: usize,
    f: f32,
    hold: f32,
    gain: f32,
    echo: f32,
    seed: u32,
}

/// A mix being built: the notes are listed first (cheap) and played in
/// later, a few at a time if wanted ([`Mix::render_some`]), so the game can
/// spread the work over frames. Looping mixes wrap every tail round to the
/// start.
#[derive(Debug, Clone)]
pub struct Mix {
    dry: Vec<f32>,
    /// The echo send (a dotted-eighth echo is added at the end).
    wet: Vec<f32>,
    /// Samples per sixteenth note.
    step: f32,
    looped: bool,
    seed: u32,
    peak: f32,
    queue: Vec<Ev>,
    next: usize,
}

impl Mix {
    fn new(steps: usize, step: f32, looped: bool) -> Self {
        let n = (steps as f32 * step).round() as usize;
        Self { dry: vec![0.0; n], wet: vec![0.0; n], step, looped, seed: 1, peak: 0.85, queue: Vec::new(), next: 0 }
    }

    /// The level [`finish`](Self::finish) scales the loudest sample to.
    fn with_peak(mut self, peak: f32) -> Self {
        self.peak = peak;
        self
    }

    /// Play up to `n` more notes in; true once every note is in.
    pub fn render_some(&mut self, n: usize) -> bool {
        let end = self.next.saturating_add(n).min(self.queue.len());
        for k in self.next..end {
            let e = self.queue[k];
            let s = voice(e.inst, e.f, e.hold, e.seed);
            self.add_at(e.at, &s, e.gain, e.echo);
        }
        self.next = end;
        self.next >= self.queue.len()
    }

    fn add_at(&mut self, at: usize, s: &[f32], gain: f32, echo: f32) {
        let n = self.dry.len();
        if !self.looped && at + s.len() > n {
            self.dry.resize(at + s.len(), 0.0);
            self.wet.resize(at + s.len(), 0.0);
        }
        let n_now = self.dry.len();
        for (k, v) in s.iter().enumerate() {
            let i = if self.looped { (at + k) % n } else { at + k };
            if i < n_now {
                self.dry[i] += v * gain;
                if echo > 0.0 {
                    self.wet[i] += v * gain * echo;
                }
            }
        }
    }

    /// A note at `step` lasting `len` steps.
    fn note(&mut self, inst: Inst, step: f32, len: f32, m: i32, gain: f32, echo: f32) {
        self.seed = self.seed.wrapping_add(1);
        let hold = (len * self.step / SR - 0.02).max(0.03);
        let at = (step * self.step).round() as usize;
        self.queue.push(Ev { inst, at, f: hz(m as f32), hold, gain, echo, seed: self.seed });
    }

    fn hit(&mut self, inst: Inst, step: f32, gain: f32) {
        self.note(inst, step, 1.0, 60, gain, 0.0);
    }

    /// Drums from a pattern per bar (`x` hit, `.` nothing), from bar `from`.
    fn drums(&mut self, inst: Inst, bars: &[&str], from: usize, gain: f32) {
        for (b, pat) in bars.iter().enumerate() {
            for (s, c) in pat.chars().filter(|c| !c.is_whitespace()).enumerate() {
                if c == 'x' {
                    self.hit(inst, ((from + b) * 16 + s) as f32, gain);
                }
            }
        }
    }

    fn melody(&mut self, inst: Inst, notes: &[Note], shift: i32, gain: f32, echo: f32) {
        for n in notes {
            self.note(inst, n.step as f32, n.len as f32 * 0.92, n.midi + shift, gain, echo);
        }
    }

    /// The finished mix: every note left played in, the echo folded in
    /// (round the loop for a loop), scaled to its peak, with the end of a
    /// one-shot faded.
    pub fn finish(mut self) -> Vec<f32> {
        self.render_some(usize::MAX);
        let peak = self.peak;
        let n = self.dry.len();
        let delay = (self.step * 3.0).round() as usize;
        if delay > 0 && delay < n {
            let fb = 0.32;
            let x = self.wet.clone();
            let passes = if self.looped { 4 } else { 1 };
            for _ in 0..passes {
                for (i, xi) in x.iter().enumerate() {
                    let back = if i >= delay {
                        self.wet[i - delay]
                    } else if self.looped {
                        self.wet[n + i - delay]
                    } else {
                        0.0
                    };
                    self.wet[i] = xi + fb * back;
                }
            }
            for ((d, w), xi) in self.dry.iter_mut().zip(&self.wet).zip(&x) {
                *d += (w - xi) * 0.9;
            }
        }
        let mut out = self.dry;
        let top = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-4);
        for v in &mut out {
            *v *= peak / top;
        }
        if !self.looped {
            let fade = (0.03 * SR) as usize;
            let len = out.len();
            for k in 0..fade.min(len) {
                out[len - 1 - k] *= k as f32 / fade as f32;
            }
        }
        out
    }
}

/// The loops' peak level: headroom for the effects on top.
const LOOP_PEAK: f32 = 0.85;
const JINGLE_PEAK: f32 = 0.85;

// ---------------------------------------------------------------- the race theme

/// 147 BPM: 2 250 samples a sixteenth at 22 050 Hz, so the loop is exactly
/// 256 x 2 250 samples.
pub const RACE_STEP: usize = 2_250;
/// 126 BPM: 2 625 samples a sixteenth.
pub const TITLE_STEP: usize = 2_625;

const RACE_CHORDS: [&str; 16] = [
    "C", "Am", "F", "G", "C", "Am", "F G", "C", //
    "F", "G", "Em", "Am", "F", "G", "Dm", "G",
];

const RACE_LEAD: [&str; 16] = [
    "E5 - G5 - C6 - - - B5 - G5 - A5 - G5 -",
    "E5 - - - . . C5 D5 E5 - G5 - E5 - D5 -",
    "C5 - F5 - A5 - - - G5 - F5 - A5 - C6 -",
    "B5 - - - G5 - D5 - G5 A5 B5 - D6 - - -",
    "E6 - D6 - C6 - G5 - . G5 C6 - E6 - D6 -",
    "C6 - A5 - E5 - A5 - C6 - - - B5 - A5 -",
    "A5 - C6 - F5 - A5 - B5 - D6 - G5 - B5 -",
    "C6 - - - - - . . E5 F5 G5 - A5 - B5 -",
    "C6 - A5 . F5 - A5 - C6 - . F6 - - E6 -",
    "D6 - B5 . G5 - B5 - D6 - . G6 - - F6 -",
    "E6 - - - B5 - G5 - E5 - G5 - B5 - E6 -",
    "C6 - - - A5 - - - C6 - D6 - E6 - - -",
    "F6 - E6 - D6 - C6 - A5 - - - C6 - - -",
    "B5 - C6 - D6 - - - G5 - - - B5 - D6 -",
    "F6 - - - E6 - D6 - C6 - D6 - A5 - - -",
    "B5 - - - D6 - - - G6 - - - - - . .",
];

fn race_theme() -> Mix {
    let bars = RACE_CHORDS.len();
    let mut m = Mix::new(bars * 16, RACE_STEP as f32, true);
    let lead = parse(&RACE_LEAD);
    // Lead an octave down from the written line (the written line is the
    // sparkle the bells play on the B part).
    m.melody(Inst::Lead, &lead, -12, 0.34, 0.5);
    m.melody(Inst::Bell, &lead.iter().copied().filter(|n| n.step >= 8 * 16).collect::<Vec<_>>(), 0, 0.13, 0.3);
    for &(at, len, c) in &chord_track(&RACE_CHORDS) {
        // Octave-bouncing bass on the eighths.
        for k in (0..len).step_by(2) {
            let iv = match (k / 2) % 4 {
                1 | 3 => 12,
                _ => 0,
            };
            m.note(Inst::Bass, (at + k) as f32, 1.6, c.bass() + iv, 0.42, 0.0);
        }
        // Off-beat chord stabs.
        for k in (2..len).step_by(4) {
            for t in c.triad() {
                m.note(Inst::Stab, (at + k) as f32, 1.0, t, 0.11, 0.0);
            }
        }
        // A part: bells arpeggio the chord on the off eighths.
        if at < 8 * 16 {
            let tones = [c.third(), 7, 12, 7];
            for (j, k) in (2..len).step_by(4).enumerate() {
                m.note(Inst::Bell, (at + k) as f32, 1.0, 72 + c.root + tones[j % 4], 0.08, 0.2);
            }
        }
    }
    let kick = "x.......x.x.....";
    let snare = "....x.......x...";
    let fill = "....x.......xxxx";
    for b in 0..bars {
        let last = b % 8 == 7;
        m.drums(Inst::Kick, &[kick], b, 0.55);
        m.drums(Inst::Snare, &[if last { fill } else { snare }], b, 0.32);
        for s in 0..16 {
            let st = (b * 16 + s) as f32;
            if s == 14 {
                m.hit(Inst::OpenHat, st, 0.12);
            } else {
                m.hit(Inst::Hat, st, if s % 2 == 0 { 0.11 } else { 0.05 });
            }
        }
        if b % 8 == 0 {
            m.hit(Inst::Crash, (b * 16) as f32, 0.2);
        }
    }
    m.with_peak(LOOP_PEAK)
}

// ---------------------------------------------------------------- the title theme

const TITLE_CHORDS: [&str; 8] = ["F", "Dm", "Bb", "C", "F", "Dm", "Gm C", "F"];

const TITLE_LEAD: [&str; 8] = [
    "A4 - C5 - F5 - - - E5 - F5 - A5 - - -",
    "G5 - F5 - D5 - - - . . D5 E5 F5 - A4 -",
    "Bb4 - D5 - F5 - Bb5 - A5 - G5 - F5 - D5 -",
    "E5 - - - C5 - - - G5 - - - - - . .",
    "A5 - G5 - F5 - C5 - F5 - A5 - C6 - - -",
    "D6 - C6 - A5 - F5 - D5 - F5 - A5 - - -",
    "Bb5 - A5 - G5 - D5 - E5 - G5 - Bb5 - A5 -",
    "F5 - - - - - - - . . . . C5 - E5 -",
];

fn title_theme() -> Mix {
    let bars = TITLE_CHORDS.len() * 2;
    let mut m = Mix::new(bars * 16, TITLE_STEP as f32, true);
    let lead = parse(&TITLE_LEAD);
    let second: Vec<Note> = lead.iter().map(|n| Note { step: n.step + 8 * 16, ..*n }).collect();
    // First time round: marimba with a soft lead; second: lead with bells above.
    m.melody(Inst::Marimba, &lead, 0, 0.4, 0.3);
    m.melody(Inst::Lead, &lead, 0, 0.12, 0.3);
    m.melody(Inst::Lead, &second, 0, 0.26, 0.45);
    m.melody(Inst::Bell, &second, 12, 0.09, 0.3);
    let chords: Vec<&str> = TITLE_CHORDS.iter().chain(TITLE_CHORDS.iter()).copied().collect();
    for &(at, len, c) in &chord_track(&chords) {
        // Bouncy bass: root, fifth, octave, fifth.
        for (k, iv, l) in [(0usize, 0, 3.0f32), (6, 7, 2.0), (8, 12, 3.0), (14, 7, 2.0)] {
            if k < len {
                m.note(Inst::Bass, (at + k) as f32, l, c.bass() + iv, 0.4, 0.0);
            }
        }
        // Marimba broken chords on the eighths.
        let t = c.triad();
        let order = [t[0], t[2], t[1], t[2]];
        for (j, k) in (0..len).step_by(2).enumerate() {
            m.note(Inst::Marimba, (at + k) as f32, 2.0, order[j % 4], 0.13, 0.0);
        }
    }
    for b in 0..bars {
        m.drums(Inst::Kick, &["x.......x......."], b, 0.45);
        m.drums(Inst::Snare, &["....x.......x..."], b, 0.17);
        m.drums(Inst::Hat, &["x.x.x.x.x.x.x.x."], b, 0.06);
        if b % 8 == 0 {
            m.hit(Inst::Crash, (b * 16) as f32, 0.13);
        }
    }
    m.with_peak(LOOP_PEAK)
}

// ---------------------------------------------------------------- the jingles

fn chord_hits(m: &mut Mix, hits: &[(usize, usize, &str)], inst: Inst, gain: f32, bass: f32) {
    for &(at, len, name) in hits {
        let c = chord(name);
        for t in c.triad() {
            m.note(inst, at as f32, len as f32, t, gain, 0.0);
        }
        if bass > 0.0 {
            m.note(Inst::Bass, at as f32, len as f32, c.bass(), bass, 0.0);
        }
    }
}

/// Lined up with the countdown: chords on 3, 2 and 1 (a second apart), a
/// drum roll rising into GO at three seconds.
fn start_jingle() -> Mix {
    // A step of 1/16 s: 16 steps a second.
    let step = SR / 16.0;
    let mut m = Mix::new(16 * 3 + 6, step, false);
    chord_hits(&mut m, &[(0, 10, "C"), (16, 10, "F"), (32, 14, "G")], Inst::Brass, 0.16, 0.4);
    for s in [0usize, 16, 32] {
        m.hit(Inst::Kick, s as f32, 0.6);
    }
    // Rising sugar bells on the last second.
    for (k, n) in ["G5", "B5", "D6", "G6"].iter().enumerate() {
        m.note(Inst::Bell, (32 + k * 4) as f32, 1.0, midi(n).unwrap_or(79), 0.16, 0.2);
    }
    for k in 0..16 {
        let g = 0.05 + 0.25 * k as f32 / 15.0;
        m.hit(Inst::Snare, (32 + k) as f32, g);
    }
    m.with_peak(JINGLE_PEAK * 0.9)
}

fn final_lap_jingle() -> Mix {
    let mut m = Mix::new(32, 2_205.0, false);
    let lead = parse(&[
        "C5 C5 C5 . E5 E5 E5 . G5 G5 G5 . C6 - - -",
        "A5 - - - B5 - - - C6 - D6 - E6 - - -",
    ]);
    m.melody(Inst::Brass, &lead, 0, 0.3, 0.3);
    m.melody(Inst::Bell, &lead, 12, 0.08, 0.2);
    chord_hits(&mut m, &[(0, 12, "C"), (12, 4, "C"), (16, 4, "F"), (20, 4, "G"), (24, 8, "C")], Inst::Stab, 0.12, 0.38);
    for (k, s) in [0usize, 1, 2, 4, 5, 6, 8, 9, 10].iter().enumerate() {
        m.hit(Inst::Snare, *s as f32, 0.12 + 0.03 * k as f32);
    }
    for s in [12usize, 16, 20, 24] {
        m.hit(Inst::Kick, s as f32, 0.55);
    }
    m.hit(Inst::Crash, 12.0, 0.22);
    m.hit(Inst::Crash, 24.0, 0.22);
    m.with_peak(JINGLE_PEAK)
}

fn win_jingle() -> Mix {
    let mut m = Mix::new(48, 1_890.0, false);
    let lead = parse(&[
        "G4 . C5 . E5 . G5 - - - E5 - G5 - - -",
        "A5 - G5 - F5 - E5 - D5 - - - G5 - - -",
        "C6 - - - - - - - - - - - - - - -",
    ]);
    m.melody(Inst::Brass, &lead, 0, 0.3, 0.35);
    m.melody(Inst::Bell, &lead, 12, 0.09, 0.3);
    chord_hits(&mut m, &[(0, 16, "C"), (16, 8, "F"), (24, 8, "G"), (32, 16, "C")], Inst::Brass, 0.07, 0.4);
    for b in 0..2 {
        m.drums(Inst::Kick, &["x...x...x...x..."], b, 0.5);
        m.drums(Inst::Snare, &["..x...x...x...x."], b, 0.16);
    }
    for k in 0..4 {
        m.hit(Inst::Snare, (28 + k) as f32, 0.15 + 0.06 * k as f32);
    }
    m.hit(Inst::Kick, 32.0, 0.6);
    m.hit(Inst::Crash, 0.0, 0.15);
    m.hit(Inst::Crash, 32.0, 0.26);
    m.with_peak(JINGLE_PEAK)
}

fn lose_jingle() -> Mix {
    let mut m = Mix::new(32, 2_400.0, false);
    let lead = parse(&[
        "E5 - C5 - A4 - - - G4 - A4 - B4 - D5 -",
        "C5 - - - - - - - - - - - . . . .",
    ]);
    m.melody(Inst::Marimba, &lead, 0, 0.45, 0.3);
    m.melody(Inst::Lead, &lead, 0, 0.1, 0.3);
    m.melody(Inst::Bell, &lead, 12, 0.06, 0.2);
    chord_hits(&mut m, &[(0, 8, "Am"), (8, 4, "F"), (12, 4, "G"), (16, 12, "C")], Inst::Marimba, 0.12, 0.35);
    m.hit(Inst::Kick, 16.0, 0.4);
    m.hit(Inst::Crash, 16.0, 0.08);
    m.with_peak(JINGLE_PEAK * 0.85)
}

// ---------------------------------------------------------------- the player

/// What the music should be doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Silent,
    Title,
    Countdown,
    Race,
    FinalLap,
    /// Every local player is over the line; `true` if the best placed in the
    /// top three (or `None` when nobody local raced).
    Results(Option<bool>),
}

/// How each local player's race stands: laps done and finishing place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub laps_done: u32,
    pub place: Option<usize>,
}

/// The cue for a race phase, the local players' karts and the lap count.
pub fn cue_for(phase: Phase, locals: &[Local], laps: u32) -> Cue {
    let best = locals.iter().filter_map(|l| l.place).min();
    let all_done = !locals.is_empty() && locals.iter().all(|l| l.place.is_some());
    match phase {
        Phase::Title => Cue::Title,
        Phase::Countdown => Cue::Countdown,
        Phase::Finished => Cue::Results(best.map(|p| p <= 3)),
        Phase::Racing if all_done => Cue::Results(best.map(|p| p <= 3)),
        Phase::Racing if locals.iter().any(|l| l.place.is_none() && l.laps_done + 1 >= laps.max(1)) => Cue::FinalLap,
        Phase::Racing => Cue::Race,
    }
}

/// Notes synthesised per frame while the music is being made (about 0.1 ms
/// each natively): the themes are ready about a second after start.
pub const NOTES_PER_FRAME: usize = 24;

/// The race theme's playback rate on the final lap: one semitone up and
/// that much faster.
pub const FINAL_LAP_RATE: f32 = 1.059_463_1;
/// Voice gains on the music bus.
/// With the music bus at its starting 0.7, the race theme peaks near 0.54
/// and sits a little above the engine at speed (`voices::UNDER_MUSIC`).
pub const TITLE_GAIN: f32 = 0.85;
pub const RACE_GAIN: f32 = 0.9;
pub const RESULTS_GAIN: f32 = 0.7;
pub const JINGLE_GAIN: f32 = 1.0;

#[derive(Debug, Clone, Copy)]
struct Playing {
    voice: VoiceId,
    song: Song,
    rate: f32,
    gain: f32,
}

/// A theme waiting to start (`delay` seconds on, counted while not paused).
#[derive(Debug, Clone, Copy)]
struct Want {
    song: Song,
    rate: f32,
    gain: f32,
    delay: f32,
}

/// The music player (draw state).
#[derive(Debug, Clone, Default)]
pub struct Music {
    /// The piece being synthesised: (its place in the order, its mix).
    job: Option<(usize, Mix)>,
    songs: Vec<Option<SoundId>>,
    jingles: Vec<Option<(SoundId, f32)>>,
    cue: Option<Cue>,
    race_no: u64,
    playing: Option<Playing>,
    want: Option<Want>,
}

impl Music {
    /// Synthesise the music a little each frame ([`NOTES_PER_FRAME`] notes)
    /// and register each piece with the mixer when it is done, so the start
    /// never stalls. True once everything is registered.
    pub fn register_next(&mut self, mixer: &mut AudioMixer) -> bool {
        if self.songs.len() < Song::ALL.len() {
            self.songs.resize(Song::ALL.len(), None);
            self.jingles.resize(Jingle::ALL.len(), None);
        }
        // The title theme first: it is wanted at once.
        const ORDER: [(bool, usize); 6] = [(true, 0), (false, 0), (true, 1), (false, 1), (false, 2), (false, 3)];
        let missing = |m: &Self, (song, i): (bool, usize)| if song { m.songs[i].is_none() } else { m.jingles[i].is_none() };
        if self.job.is_none() {
            let Some(k) = ORDER.iter().position(|&o| missing(self, o)) else {
                return true;
            };
            let (song, i) = ORDER[k];
            let mix = if song { Song::ALL[i].mix() } else { Jingle::ALL[i].mix() };
            self.job = Some((k, mix));
        }
        let Some((k, mut mix)) = self.job.take() else { return true };
        if !mix.render_some(NOTES_PER_FRAME) {
            self.job = Some((k, mix));
            return false;
        }
        let smp = mix.finish();
        let bytes: Rc<[u8]> = wav(&smp).into();
        let (song, i) = ORDER[k];
        if song {
            self.songs[i] = Some(mixer.add_sound(Song::ALL[i].name(), bytes));
        } else {
            let secs = smp.len() as f32 / SR;
            self.jingles[i] = Some((mixer.add_sound(Jingle::ALL[i].name(), bytes), secs));
        }
        !ORDER.iter().any(|&o| missing(self, o))
    }

    fn song_id(&self, s: Song) -> Option<SoundId> {
        let i = Song::ALL.iter().position(|x| *x == s)?;
        self.songs.get(i).copied().flatten()
    }

    fn jingle(&self, j: Jingle) -> Option<(SoundId, f32)> {
        let i = Jingle::ALL.iter().position(|x| *x == j)?;
        self.jingles.get(i).copied().flatten()
    }

    /// The cue playing now.
    pub fn cue(&self) -> Option<Cue> {
        self.cue
    }

    /// The theme on a voice now: (song, rate, gain as set, ignoring pause).
    pub fn playing(&self) -> Option<(Song, f32, f32)> {
        self.playing.map(|p| (p.song, p.rate, p.gain))
    }

    fn stop_loop(&mut self, mixer: &mut AudioMixer) {
        if let Some(p) = self.playing.take() {
            mixer.voice_stop(p.voice);
        }
        self.want = None;
    }

    fn play_jingle(&self, mixer: &mut AudioMixer, j: Jingle) -> f32 {
        match self.jingle(j) {
            Some((id, secs)) => {
                mixer.play_on(id, Bus::Music, JINGLE_GAIN);
                secs
            }
            None => 0.0,
        }
    }

    /// Move to `cue`.
    fn enter(&mut self, mixer: &mut AudioMixer, cue: Cue) {
        self.stop_loop(mixer);
        let want = |song, rate, gain, delay| Some(Want { song, rate, gain, delay });
        match cue {
            Cue::Silent => mixer.stop_music(),
            Cue::Title => {
                mixer.stop_music();
                self.want = want(Song::Title, 1.0, TITLE_GAIN, 0.0);
            }
            Cue::Countdown => {
                mixer.stop_music();
                self.play_jingle(mixer, Jingle::Start);
            }
            Cue::Race => self.want = want(Song::Race, 1.0, RACE_GAIN, 0.0),
            Cue::FinalLap => {
                let secs = self.play_jingle(mixer, Jingle::FinalLap);
                self.want = want(Song::Race, FINAL_LAP_RATE, RACE_GAIN, (secs - 0.15).max(0.0));
            }
            Cue::Results(won) => {
                let secs = match won {
                    Some(true) => self.play_jingle(mixer, Jingle::Win),
                    Some(false) => self.play_jingle(mixer, Jingle::Lose),
                    None => 0.0,
                };
                self.want = want(Song::Title, 1.0, RESULTS_GAIN, secs + 0.4);
            }
        }
        self.cue = Some(cue);
    }

    /// Once per frame: follow the race. `silent` holds everything quiet
    /// (live asset focus).
    pub fn update(&mut self, mixer: &mut AudioMixer, race: &RaceView, dt: f32, silent: bool) {
        let cue = if silent {
            Cue::Silent
        } else {
            let locals: Vec<Local> = (0..race.players)
                .filter_map(|p| race.player_kart(p))
                .filter_map(|i| race.karts.get(i).map(|k| (i, k)))
                .map(|(i, k)| Local {
                    laps_done: k.laps_done,
                    place: k.finished.map(|_| race.finish_order.iter().position(|&f| f == i).map_or(race.karts.len(), |p| p + 1)),
                })
                .collect();
            cue_for(race.phase, &locals, race.counts.laps)
        };
        self.step(mixer, cue, race.race_no, race.paused, dt);
    }

    /// The player's logic, apart from reading the race (tests drive it).
    pub fn step(&mut self, mixer: &mut AudioMixer, cue: Cue, race_no: u64, paused: bool, dt: f32) {
        // A new race restarts the cue even if the phase name matches.
        let fresh = race_no != self.race_no && cue == Cue::Countdown;
        self.race_no = race_no;
        if self.cue != Some(cue) || fresh {
            self.enter(mixer, cue);
        }
        if let Some(w) = self.want.as_mut() {
            if !paused {
                w.delay -= dt.max(0.0);
            }
            if w.delay <= 0.0 && self.playing.is_none() {
                let w = *w;
                if let Some(id) = self.song_id(w.song) {
                    let voice = mixer.voice_start(id, Bus::Music, if paused { 0.0 } else { w.gain }, w.rate);
                    self.playing = Some(Playing { voice, song: w.song, rate: w.rate, gain: w.gain });
                    self.want = None;
                }
            }
        }
        if let Some(p) = self.playing {
            mixer.voice_set(p.voice, if paused { 0.0 } else { p.gain }, p.rate);
        }
    }

    /// Stop everything.
    pub fn stop(&mut self, mixer: &mut AudioMixer) {
        self.stop_loop(mixer);
        mixer.stop_music();
        self.cue = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::audio::NullAudio;

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    #[test]
    fn notes_parse() {
        assert_eq!(midi("C4"), Some(60));
        assert_eq!(midi("A4"), Some(69));
        assert_eq!(midi("Bb3"), Some(58));
        assert_eq!(midi("F#5"), Some(78));
        let n = parse(&["C5 - . E5 - -"]);
        assert_eq!(n, vec![Note { step: 0, len: 2, midi: 72 }, Note { step: 3, len: 3, midi: 76 }]);
        assert_eq!(chord("Am").triad(), [57, 60, 64]);
        assert_eq!(chord("Bb").root, 10);
    }

    #[test]
    fn every_bar_of_every_line_has_sixteen_steps() {
        for (name, lines) in [("race", &RACE_LEAD[..]), ("title", &TITLE_LEAD[..])] {
            for (b, l) in lines.iter().enumerate() {
                assert_eq!(l.split_whitespace().count(), 16, "{name} bar {}", b + 1);
            }
        }
    }

    #[test]
    fn the_themes_loop_seamlessly_at_the_right_tempo_without_clipping() {
        for s in Song::ALL {
            let smp = s.samples();
            let (step, bpm) = match s {
                Song::Title => (TITLE_STEP, 126.0),
                Song::Race => (RACE_STEP, 147.0),
            };
            assert_eq!(smp.len(), 256 * step, "{s:?} is 16 bars");
            assert!((SR * 60.0 / (step as f32 * 4.0) - bpm).abs() < 1e-3);
            assert!(smp.iter().all(|v| v.is_finite() && v.abs() <= LOOP_PEAK + 1e-4), "{s:?}");
            let r = rms(&smp);
            assert!(r > 0.08 && r < 0.45, "{s:?} loudness {r}");
            let worst = smp.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            let seam = (smp[smp.len() - 1] - smp[0]).abs();
            assert!(seam <= worst, "{s:?} clicks at the seam: {seam} vs {worst}");
            // The loudness just before and just after the seam matches the
            // loudness elsewhere (no drop-out or pile-up at the join).
            let w = SR as usize / 4;
            let tail = rms(&smp[smp.len() - w..]);
            let head = rms(&smp[..w]);
            assert!(tail > r * 0.4 && head > r * 0.4, "{s:?} {tail} {head} vs {r}");
        }
    }

    #[test]
    fn the_jingles_are_short_clean_and_end_in_silence() {
        for j in Jingle::ALL {
            let smp = j.samples();
            let secs = smp.len() as f32 / SR;
            assert!((1.5..6.0).contains(&secs), "{j:?} {secs} s");
            assert!(smp.iter().all(|v| v.is_finite() && v.abs() <= JINGLE_PEAK + 1e-4), "{j:?}");
            assert!(rms(&smp) > 0.05, "{j:?} too quiet");
            assert!(smp[smp.len() - 1].abs() < 1e-3, "{j:?} ends with a click");
        }
        // The start jingle's chords sit on the countdown's three beeps and its
        // bells ring out just past GO (three seconds).
        let s = Jingle::Start.samples();
        assert!(s.len() as f32 / SR < 4.0);
    }

    #[test]
    fn the_cue_follows_the_race() {
        let l = |laps_done, place| Local { laps_done, place };
        assert_eq!(cue_for(Phase::Title, &[], 3), Cue::Title);
        assert_eq!(cue_for(Phase::Countdown, &[l(0, None)], 3), Cue::Countdown);
        assert_eq!(cue_for(Phase::Racing, &[l(0, None)], 3), Cue::Race);
        assert_eq!(cue_for(Phase::Racing, &[l(1, None)], 3), Cue::Race);
        assert_eq!(cue_for(Phase::Racing, &[l(2, None)], 3), Cue::FinalLap);
        assert_eq!(cue_for(Phase::Racing, &[l(3, Some(2))], 3), Cue::Results(Some(true)));
        assert_eq!(cue_for(Phase::Racing, &[l(3, Some(5))], 3), Cue::Results(Some(false)));
        // Two players: the final lap starts with the first, the results wait
        // for both and go by the better place.
        assert_eq!(cue_for(Phase::Racing, &[l(2, None), l(1, None)], 3), Cue::FinalLap);
        assert_eq!(cue_for(Phase::Racing, &[l(3, Some(6)), l(2, None)], 3), Cue::FinalLap);
        assert_eq!(cue_for(Phase::Finished, &[l(3, Some(6)), l(3, Some(3))], 3), Cue::Results(Some(true)));
        // Watching (nobody local): the race theme, then the results theme.
        assert_eq!(cue_for(Phase::Racing, &[], 3), Cue::Race);
        assert_eq!(cue_for(Phase::Finished, &[], 3), Cue::Results(None));
    }

    fn mixer() -> AudioMixer {
        AudioMixer::new(Box::new(NullAudio { unlocked: true, ..Default::default() }))
    }

    fn registered(mx: &mut AudioMixer) -> Music {
        let mut m = Music::default();
        let mut frames = 0;
        while !m.register_next(mx) {
            frames += 1;
            assert!(frames < 400, "the music takes too many frames to make");
        }
        assert!(frames > 6, "the work is spread over frames");
        assert!(m.register_next(mx), "done stays done");
        m
    }

    #[test]
    fn the_player_plays_the_right_theme_and_jingles() {
        let mut mx = mixer();
        let mut m = registered(&mut mx);
        let dt = 1.0 / 60.0;
        m.step(&mut mx, Cue::Title, 0, false, dt);
        assert_eq!(m.playing().map(|p| p.0), Some(Song::Title));
        // Countdown: the theme stops (the start jingle plays).
        m.step(&mut mx, Cue::Countdown, 1, false, dt);
        assert!(m.playing().is_none());
        m.step(&mut mx, Cue::Race, 1, false, dt);
        assert_eq!(m.playing(), Some((Song::Race, 1.0, RACE_GAIN)));
        // Final lap: the fanfare, then the race theme faster.
        m.step(&mut mx, Cue::FinalLap, 1, false, dt);
        assert!(m.playing().is_none(), "quiet under the fanfare");
        for _ in 0..60 * 4 {
            m.step(&mut mx, Cue::FinalLap, 1, false, dt);
        }
        assert_eq!(m.playing(), Some((Song::Race, FINAL_LAP_RATE, RACE_GAIN)));
        // Over the line: the jingle, then the results theme.
        m.step(&mut mx, Cue::Results(Some(true)), 1, false, dt);
        assert!(m.playing().is_none());
        for _ in 0..60 * 6 {
            m.step(&mut mx, Cue::Results(Some(true)), 1, false, dt);
        }
        assert_eq!(m.playing(), Some((Song::Title, 1.0, RESULTS_GAIN)));
        // One voice at a time.
        assert_eq!(mx.voice_count(), 1);
        m.stop(&mut mx);
        assert_eq!(mx.voice_count(), 0);
    }

    #[test]
    fn pausing_silences_the_theme_and_holds_a_waiting_one() {
        let mut mx = mixer();
        let mut m = registered(&mut mx);
        let dt = 1.0 / 60.0;
        m.step(&mut mx, Cue::Race, 1, false, dt);
        let Some(p) = m.playing else { panic!("no theme") };
        m.step(&mut mx, Cue::Race, 1, true, dt);
        mx.update();
        assert_eq!(mx.voice_state(p.voice).map(|s| s.0), Some(0.0), "paused is silent");
        m.step(&mut mx, Cue::Race, 1, false, dt);
        assert_eq!(mx.voice_state(p.voice).map(|s| s.0), Some(RACE_GAIN));
        // Paused during the fanfare: the faster theme waits for the unpause.
        m.step(&mut mx, Cue::FinalLap, 1, false, dt);
        for _ in 0..60 * 10 {
            m.step(&mut mx, Cue::FinalLap, 1, true, dt);
        }
        assert!(m.playing().is_none());
    }

    #[test]
    fn back_to_the_title_stops_the_race_theme_and_starts_the_title() {
        let mut mx = mixer();
        let mut m = registered(&mut mx);
        m.step(&mut mx, Cue::Race, 1, false, 0.016);
        m.step(&mut mx, Cue::Title, 1, false, 0.016);
        assert_eq!(m.playing().map(|p| p.0), Some(Song::Title));
        mx.update();
        assert_eq!(mx.voice_count(), 1);
        // Silent (asset focus) stops it.
        m.step(&mut mx, Cue::Silent, 1, false, 0.016);
        assert!(m.playing().is_none());
    }
}
