//! Render Sugarpeak's music to WAV files to listen to outside the game, with
//! loudness figures: each theme twice round (to hear the loop join), each
//! jingle, the final-lap speed-up, and a race mix with the engine at the
//! game's levels.
//!
//!   cargo run -p evani-kit-kart-racer --example music_wavs -- <out dir>

use evani_kit_kart_racer::music::{FINAL_LAP_RATE, JINGLE_GAIN, Jingle, RACE_GAIN, Song};
use evani_kit_kart_racer::sfx::{Loop, Sfx, wav};
use evani_kit_kart_racer::voices::{engine_gain, engine_rate};

/// The music bus's starting volume in the engine's mixer.
const MUSIC_BUS: f32 = 0.7;

fn stats(s: &[f32]) -> (f32, f32) {
    let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let rms = (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt();
    (peak, rms)
}

/// Play `s` looped at `rate` for `n` samples (linear interpolation).
fn resample_loop(s: &[f32], rate: f32, n: usize) -> Vec<f32> {
    let len = s.len() as f32;
    (0..n)
        .map(|i| {
            let x = (i as f32 * rate) % len;
            let a = x.floor() as usize;
            let f = x - a as f32;
            s[a] * (1.0 - f) + s[(a + 1) % s.len()] * f
        })
        .collect()
}

fn main() -> std::io::Result<()> {
    let out = std::env::args().nth(1).ok_or_else(|| std::io::Error::other("usage: music_wavs <out dir>"))?;
    std::fs::create_dir_all(&out)?;
    let save = |name: &str, s: &[f32]| -> std::io::Result<()> {
        std::fs::write(format!("{out}/{name}.wav"), wav(s))?;
        let (p, r) = stats(s);
        println!("{{\"file\":\"{name}.wav\",\"seconds\":{:.2},\"peak\":{p:.3},\"rms\":{r:.3}}}", s.len() as f32 / 22_050.0);
        Ok(())
    };
    let mut race = Vec::new();
    for song in Song::ALL {
        let t = std::time::Instant::now();
        let s = song.samples();
        eprintln!("{song:?}: rendered in {:?}", t.elapsed());
        let twice: Vec<f32> = s.iter().chain(s.iter()).copied().collect();
        let name = match song {
            Song::Title => "title-theme-loop-x2",
            Song::Race => "race-theme-loop-x2",
        };
        save(name, &twice)?;
        if song == Song::Race {
            race = s;
        }
    }
    let fast = resample_loop(&race, FINAL_LAP_RATE, race.len());
    save("race-theme-final-lap-faster", &fast)?;
    for j in Jingle::ALL {
        let t = std::time::Instant::now();
        let s = j.samples();
        eprintln!("{j:?}: rendered in {:?}", t.elapsed());
        let name = match j {
            Jingle::Start => "jingle-countdown-start",
            Jingle::FinalLap => "jingle-final-lap",
            Jingle::Win => "jingle-win",
            Jingle::Lose => "jingle-placed",
        };
        save(name, &s)?;
    }
    save("chime-item-box", &Sfx::ItemBox.samples())?;
    // A race mix as heard in game (master 1): the music bus and voice gain,
    // the engine at 80 % speed, a beep-beep-beep start and an item chime.
    let n = 22_050 * 12;
    let music = resample_loop(&race, 1.0, n);
    let engine = resample_loop(&Loop::Engine.samples(), engine_rate(0.8), n);
    let start = Jingle::Start.samples();
    let mut mix: Vec<f32> = (0..n).map(|i| music[i] * RACE_GAIN * MUSIC_BUS + engine[i] * engine_gain(0.8, 1)).collect();
    let chime = Sfx::ItemBox.samples();
    for (k, v) in chime.iter().enumerate() {
        mix[22_050 * 4 + k] += v * 0.5;
    }
    let mut with_start = vec![0.0f32; start.len().min(22_050 * 3)];
    for (k, v) in with_start.iter_mut().enumerate() {
        *v = start[k] * JINGLE_GAIN * MUSIC_BUS;
    }
    with_start.extend_from_slice(&mix);
    save("race-mix-with-engine", &with_start)?;
    let (mp, mr) = stats(&music.iter().map(|v| v * RACE_GAIN * MUSIC_BUS).collect::<Vec<_>>());
    let (ep, er) = stats(&engine.iter().map(|v| v * engine_gain(0.8, 1)).collect::<Vec<_>>());
    println!("{{\"music_in_game\":{{\"peak\":{mp:.3},\"rms\":{mr:.3}}},\"engine_80pc\":{{\"peak\":{ep:.3},\"rms\":{er:.3}}}}}");
    Ok(())
}
