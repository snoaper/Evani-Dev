//! The dynamic sky dome's photoreal recipe (Asset Studio column 21,
//! 2026-11-07): the library's settings for evani-material-proc's Preetham
//! dome (wrapped as it is, crate::wrapped) for a clear mid-morning sky seen
//! from the ground: pale, hazy blue-white at the horizon deepening to a deep
//! blue overhead, the sun's glow on one side, and a few scattered
//! fair-weather cumulus in the middle sky, down to a few degrees over the
//! horizon (the dome's `sky-horizon` part, which this asset turns on).
//! Since round 2 the cumulus are the part's cloud volume
//! ([`evani_material_proc::sky::CloudVolume`]): heaps with flat grey bases
//! and sunlit billowing crowns, lit through themselves, the low ones melting
//! into the horizon haze, faint cirrus behind.
//!
//! A game that lists `sky-dynamic-sky-dome` and installs the dome (any
//! scene: `evani_material_proc::ProcWorld::sky_only` for the dome alone)
//! starts it with [`photoreal`] or applies the recipe to the live params
//! with [`apply_photoreal`]. Nothing here is viewer-only: these are the
//! dome's own `sky.*` params.

use evani_core::params::{ParamRegistry, ParamValue};
use evani_material_proc::sky::{CloudHorizon, CloudVolume, SkyParams};

/// The photoreal sky: the dome's look as the recipe sets it. The clear
/// sky's numbers were fitted to the reference photograph's measured colours
/// (13 boxes from the horizon to the top of the frame, on both sides of the
/// sun) through the dome's own model and the photoreal look's daylight tone
/// curve and grade: a deeper, less cyan blue than the model's default
/// (tint), darkening towards the zenith (deep), the sun's glow wide.
pub fn photoreal() -> SkyParams {
    SkyParams {
        turbidity: 2.5,
        rayleigh: 1.8,
        mie_coefficient: 0.01,
        mie_directional_g: 0.7,
        cloud_scale: 0.003,
        cloud_speed: 0.00002,
        cloud_coverage: 0.2,
        cloud_density: 1.0,
        cloud_elevation: 0.5,
        brightness: 0.45,
        sun_disc: true,
        visible: true,
        open_below: false,
        horizon: CloudHorizon {
            // The volume's clouds thin out over the last seven degrees and
            // the air hides half of a heap about 6 km off, so the low ones
            // melt into the blue-grey horizon haze (round 2).
            fade_low: 0.0,
            fade_high: 0.12,
            curve: 1.0,
            haze: 0.7,
            // Broad, photographic shading (no scaly lobes), crisp outlines
            // with cauliflower lobes, white against the darker sky.
            lobe_shade: 0.15,
            edge: 0.09,
            bite: 1.2,
            gain: 2.6,
            tint: [0.7, 0.7, 1.0],
            deep: 0.48,
            // Lit crowns, grey bases: the lit side leans towards the eye
            // (the upper side on screen of a cloud seen from the ground).
            up_bias: -1.2,
            body_shade: 1.0,
            // Round 2: the clouds as a real volume (heaps with flat grey
            // bases and sunlit billowing crowns, lit through themselves),
            // a field about 1.4 km up whose seed puts one main heap in the
            // middle-left sky of the reference's view, the sky above it
            // open, smaller heaps low down melting into the haze, faint
            // cirrus streaks behind.
            volume: CloudVolume {
                on: true,
                seed: 102.49,
                base: 1.4,
                depth: 0.9,
                size: 3.2,
                cirrus: 0.2,
                billow: 1.4,
                soft: 0.15,
                base_shade: 0.2,
                powder: 0.25,
            },
        },
    }
}

/// The recipe as `(param, value)` pairs (the dome's `sky.*` params).
pub fn photoreal_params() -> Vec<(&'static str, ParamValue)> {
    let s = photoreal();
    let f = s.floats();
    let names = [
        "sky.turbidity",
        "sky.rayleigh",
        "sky.mie_coefficient",
        "sky.mie_directional_g",
        "sky.cloud_scale",
        "sky.cloud_speed",
        "sky.cloud_coverage",
        "sky.cloud_density",
        "sky.cloud_elevation",
        "sky.brightness",
    ];
    let mut out: Vec<(&'static str, ParamValue)> = names.into_iter().zip(f).map(|(n, v)| (n, ParamValue::F32(v))).collect();
    out.push(("sky.sun_disc", ParamValue::Bool(s.sun_disc)));
    out.push(("sky.cloud_horizon_low", ParamValue::F32(s.horizon.fade_low)));
    out.push(("sky.cloud_horizon_high", ParamValue::F32(s.horizon.fade_high)));
    out.push(("sky.cloud_horizon_curve", ParamValue::F32(s.horizon.curve)));
    out.push(("sky.cloud_horizon_haze", ParamValue::F32(s.horizon.haze)));
    out.push(("sky.cloud_lobe_shade", ParamValue::F32(s.horizon.lobe_shade)));
    out.push(("sky.cloud_edge", ParamValue::F32(s.horizon.edge)));
    out.push(("sky.cloud_bite", ParamValue::F32(s.horizon.bite)));
    out.push(("sky.cloud_gain", ParamValue::F32(s.horizon.gain)));
    out.push(("sky.tint", ParamValue::Color(s.horizon.tint)));
    out.push(("sky.zenith_deep", ParamValue::F32(s.horizon.deep)));
    out.push(("sky.cloud_up_bias", ParamValue::F32(s.horizon.up_bias)));
    out.push(("sky.cloud_body_shade", ParamValue::F32(s.horizon.body_shade)));
    let v = s.horizon.volume;
    out.push(("sky.cloud_volume", ParamValue::Bool(v.on)));
    out.push(("sky.cloud_seed", ParamValue::F32(v.seed)));
    out.push(("sky.cloud_base", ParamValue::F32(v.base)));
    out.push(("sky.cloud_depth", ParamValue::F32(v.depth)));
    out.push(("sky.cloud_size", ParamValue::F32(v.size)));
    out.push(("sky.cirrus", ParamValue::F32(v.cirrus)));
    out.push(("sky.cloud_billow", ParamValue::F32(v.billow)));
    out.push(("sky.cloud_soft", ParamValue::F32(v.soft)));
    out.push(("sky.cloud_base_shade", ParamValue::F32(v.base_shade)));
    out.push(("sky.cloud_powder", ParamValue::F32(v.powder)));
    out
}

/// Set the dome's live params to the recipe. Returns the params this build
/// doesn't have (none when the dome is installed).
pub fn apply_photoreal(reg: &mut ParamRegistry) -> Vec<&'static str> {
    let mut missing = Vec::new();
    for (name, v) in photoreal_params() {
        match reg.id(name) {
            Some(id) => {
                let _ = reg.set(id, v);
            }
            None => missing.push(name),
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recipe sets every float param of the dome, inside its range.
    #[test]
    fn the_recipe_sets_every_sky_param_in_range() {
        let mut reg = ParamRegistry::default();
        let h = evani_material_proc::SkyHandles::register(&mut reg, &SkyParams::default());
        assert!(h.is_ok());
        let missing = apply_photoreal(&mut reg);
        assert!(missing.is_empty(), "{missing:?}");
        let Ok(h) = h else { return };
        assert_eq!(h.read(&reg), photoreal());
    }
}
