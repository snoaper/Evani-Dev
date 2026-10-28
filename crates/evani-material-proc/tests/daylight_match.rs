//! The photoreal `daylight` preset keeps its sky-matched colours as
//! constants (a look never depends on this module); this checks them
//! against the dome they were matched to: the default sky at the preset's
//! sun.
#![cfg(feature = "sky")]

use evani_look_photoreal::daylight;
use evani_material_proc::sky::{SkyParams, sun_direction};

fn fit(c: [f32; 3]) -> [f32; 3] {
    let m = c[0].max(c[1]).max(c[2]);
    if m > 1.0 { c.map(|v| v / m) } else { c }
}

fn shape(c: [f32; 3]) -> [f32; 3] {
    let m = c[0].max(c[1]).max(c[2]).max(1e-6);
    c.map(|v| v / m)
}

fn soften(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| 0.5 + 0.5 * v.clamp(0.0, 1.0))
}

fn close(name: &str, got: [f32; 3], want: [f32; 3]) -> Option<String> {
    (!got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.01))
        .then(|| format!("{name}: the preset has {want:?}, the dome gives {got:?}"))
}

#[test]
fn the_daylight_preset_matches_the_dome() {
    let sky = SkyParams::default();
    let sun = sun_direction(daylight::SUN_ELEVATION, daylight::SUN_AZIMUTH);
    assert_eq!(sun, daylight::sun());
    let horizon = sky.ring(sun, 0.03, 32);
    let zenith = sky.radiance(sun, [0.0, 1.0, 0.0]);
    let off: Vec<String> = [
        close("clear colour (HORIZON)", fit(horizon), daylight::HORIZON),
        close("SKY_ZENITH", soften(shape(zenith)), daylight::SKY_ZENITH),
        close("SKY_HORIZON", soften(shape(horizon)), daylight::SKY_HORIZON),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(off.is_empty(), "{}", off.join("; "));
}

#[test]
fn the_haze_stops_short_of_the_dome() {
    assert!(
        daylight::FOG_BODY.contains("distance > 450.0") && evani_material_proc::sky::RADIUS > 460.0
    );
}
