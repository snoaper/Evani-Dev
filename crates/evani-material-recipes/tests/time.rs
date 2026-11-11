//! P9 acceptance: fire and the hologram take their motion from sim time only
//! (the fixed 60 Hz step, `rc_frame.misc.y`), never a wall clock. Two runs of
//! the same 120 sim frames give the same hash of what they draw from time.
//!
//! The check runs on the CPU mirror of the time inputs: the frame words the
//! engine uploads each step (`params::default_frame`), then the same integer
//! hash and value noise the shaders use (core.wgsl `rc_pcg`, `rc_hash3`,
//! `rc_vnoise2`) on the scalars the two recipes derive from `rc_time()`: the
//! fire flicker factor, the ember pulse phase, the hologram flicker step and
//! its dither-gap cell. If a shader read time any other way, this mirror would
//! no longer describe it, so the text checks below pin the sources.
#![cfg(feature = "engine")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use evani_material_recipes::params::default_frame;
use evani_material_recipes::shader::ALL_RECIPES;

fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    pcg(y as u32 ^ pcg(x as u32 ^ pcg(seed)))
}

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    pcg(z as u32 ^ hash2(x, y, seed))
}

fn u01(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

fn fade5(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn vnoise2(x: f32, y: f32, seed: u32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (ux, uy) = (fade5(x - fx), fade5(y - fy));
    let c = |dx: i32, dy: i32| u01(hash2(ix + dx, iy + dy, seed));
    let top = c(0, 0) + (c(1, 0) - c(0, 0)) * ux;
    let bot = c(0, 1) + (c(1, 1) - c(0, 1)) * ux;
    top + (bot - top) * uy
}

/// The time-driven scalars of fire and hologram on sim frame `n`, folded into `h`.
fn frame_hash(mut h: u64, n: u32, seed: u32) -> u64 {
    let w = default_frame("high", f64::from(n) / 60.0);
    let t = f32::from_bits(w[9]);
    let mut mix = |bits: u32| {
        h = (h ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
    };
    // Fire: the flicker factor (speed 8, amount 0.4) and an ember's pulse phase.
    mix((1.0 + (vnoise2(t * 8.0 * 0.5, seed as f32, seed) - 0.5) * 2.0 * 0.4).to_bits());
    mix((0.65 + 0.35 * (t * 5.0 * 0.5 + 0.3 * core::f32::consts::TAU).sin()).to_bits());
    // Hologram: the flicker step (24 per second) and a dither-gap cell (12 per second).
    mix(u32::from(u01(pcg((t * 24.0) as u32 + seed)) >= 0.7));
    mix(u32::from(u01(hash3(3, -2, 7, (t * 12.0) as u32 + seed)) <= 0.1));
    h
}

fn run(seed: u32) -> u64 {
    (0..120).fold(0xcbf2_9ce4_8422_2325, |h, n| frame_hash(h, n, seed))
}

#[test]
fn two_runs_of_120_sim_frames_give_the_same_hash() {
    assert_eq!(run(0x4d), run(0x4d), "same frames, same hash");
    assert_ne!(run(0x4d), run(0x4e), "a different seed draws differently");
    // The hash moves with time: frame 0 and frame 59 differ.
    assert_ne!(frame_hash(0, 0, 0x4d), frame_hash(0, 59, 0x4d));
}

#[test]
fn the_frame_carries_sim_time_only() {
    let w = default_frame("high", 2.0);
    assert_eq!(f32::from_bits(w[9]), 2.0);
    assert_eq!(f32::from_bits(default_frame("high", 120.0 / 60.0)[9]), 2.0);
}

#[test]
fn fire_and_glow_read_time_only_through_rc_time() {
    for code in [28u8, 29] {
        let r = ALL_RECIPES.iter().find(|r| r.code == code).expect("built");
        assert!(r.wgsl.contains("rc_time()"), "code {code} animates from sim time");
        for bad in ["misc.y", "bitcast", "time", "clock", "now"] {
            assert!(!r.wgsl.replace("rc_time()", "").contains(bad), "code {code} uses {bad}");
        }
    }
}
