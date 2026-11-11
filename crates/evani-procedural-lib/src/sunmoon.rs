//! The sun and moon discs' photoreal recipe (Asset Studio column 38,
//! 2026-11-08): evani-material-proc's sky dome with its `sky-discs` part
//! (which this asset turns on): the sun as a small limb-darkened disc at its
//! true size (0.53 degrees) with the haze's bright aureole round it, and the
//! moon at its true size (0.52 degrees) lit by the sun, so its phase follows
//! from where the two stand (a thin pale crescent close to a low sun).
//!
//! The recipe's sky is a clear golden hour just before sunset: warm orange
//! at the horizon under the sun, fading through a pale band to a soft blue
//! overhead, no clouds. A game that lists `sky-sun-and-moon-discs` and
//! installs the dome (`evani_material_proc::ProcWorld::sky_only` for the
//! dome alone, or any scene with the look's `sky` material) starts it with
//! [`photoreal_sky`] and [`photoreal`], or applies the recipe to the live
//! params with [`apply_photoreal`]. The moon's place is the game's: set
//! `sky.moon_elevation` / `sky.moon_azimuth` (or [`moon_above_sun`]).
//! Nothing here is viewer-only: these are the dome's own `sky.*` params.

use evani_core::params::{ParamRegistry, ParamValue};
use evani_material_proc::SkyDiscs;
use evani_material_proc::sky::{CloudHorizon, CloudVolume, SkyParams};

/// The golden-hour clear sky the discs are shown in. Fitted to the
/// reference photograph's colours (round 2: 230 boxes from the zenith to
/// the skyline through the dome's own model, the discs' part and the
/// photoreal look's daylight tone curve and grade, then measured in the
/// rendered scene: 7 levels RMS over 190 sky boxes (round 3: the tint's
/// blue down as the warmth became a narrow band; round 1: 13); the
/// dome's default params blow out to white round a low sun): a hazy sky
/// (turbidity 8) with no aerosol glow of the model's own, a blue scattered
/// light deepening towards the zenith, the orange carried by the discs'
/// aureole and the low sun's warmth (`photoreal`).
///
/// Column 73 (2026-11-11): fitted again with the discs' new parts, by a
/// damped least-squares fit on real captures of the scene (27 values, 652
/// boxes: the whole sky, the band along the skyline and a fan round the
/// sun) under the recipe's own grade ([`GRADE_SATURATION`]): 3.3 levels
/// RMS over the 190 sky boxes, measured on round 1's picture (6.7 before).
///
/// Column 73's new run (2026-11-11): fitted once more with the horizon's
/// haze on (28 values, 849 boxes: the same three sets and the lowest two
/// and a half degrees of the sky in fine rows): 2.7 levels RMS over the 190
/// sky boxes, the row along the skyline 3.5 (7.3 before).
pub fn photoreal_sky() -> SkyParams {
    SkyParams {
        turbidity: 8.85,
        rayleigh: 2.086,
        mie_coefficient: 0.0,
        mie_directional_g: 0.37,
        cloud_coverage: 0.0,
        cloud_density: 0.0,
        brightness: 2.84,
        sun_disc: true,
        horizon: CloudHorizon {
            tint: [1.188, 0.827, 1.122],
            deep: 0.95,
            volume: CloudVolume { on: false, ..CloudVolume::default() },
            ..CloudHorizon::default()
        },
        ..SkyParams::default()
    }
}

/// The discs: true sizes, a gently limb-darkened sun a few times brighter
/// than white (`disc_bright` 0.01; round 4) whose middle burns out white
/// (`core`) and whose soft edge runs through yellow into the haze's aureole (fitted with the sky: its core
/// falling off over 0.8 degrees, its skirt, 20 percent of it, over 2.9: round 4, the glow gathered at the sun),
/// the low sun's full warmth along the skyline under the sun, a bell over
/// the lowest dozen degrees, and to about a third 30 degrees round from
/// the sun (blue gone, green to 65 percent: an orange band along the
/// skyline, deepest at the sun, no yellow lobe over it; round 3), a pale
/// moon about as bright as the sky round it (round 4: 2.0, a faint arc), faint maria and a trace of earthshine (its dark side lost in the
/// daylight, as a real daytime moon's), sampled over a 60 degree view's
/// pixel on a 1672 wide picture (a game sets `sky.disc_pixel` to its own).
///
/// Column 73: the sun a clean white disc about 0.7 of its size across
/// with a thin soft yellow edge, no yellow ring (a dim disc, `disc_bright`
/// 0.002, under a strong white core); its aureole a flattened oval of the
/// low sun's own deep orange (`skirt_flat`, `glow_sun`) with a faint broad
/// lobe a dozen degrees wide (`broad`); a fifth of the warmth all round
/// the skyline (`warmth_round`: a dull orange horizon away from the sun,
/// not the dome's mauve); the moon drawn by its lit strip's exact share of
/// each pixel, a crisp thin arc (`moon_soft` 0.85) whose horns fade a
/// little early (`moon_horns`), its dark side all but lost in the
/// daylight; a fine grain (`grain`) in place of the gradients' bands.
///
/// Column 73's new run: the horizon's haze is on (`haze`, a bell under two
/// degrees tall): the lowest sky is no longer the darkest and most
/// saturated, it dulls to a dusky mauve-grey band over the land away from
/// the sun (`haze_tint`) and a rose red one under it (`haze_sun`, within a
/// few degrees of the sun: the dust lit from behind), apart from the orange
/// glow above; the glow keeps a share of the sun's own colour below the
/// sun too (`glow_low`); the warmth and glow are a little uneven from
/// bearing to bearing (`uneven`: slow swells, so the broad gradients are
/// not quite regular, a little more glow to the right of the sun in the
/// recipe's own view); the crescent's horns reach nearly to the ends of
/// its limb (`moon_horns` 0.1, was 0.4: its tips were short).
pub fn photoreal() -> SkyDiscs {
    SkyDiscs {
        on: true,
        sun_size: 0.53,
        limb: 0.6,
        glow: 21.0,
        glow_width: 1.14,
        skirt: 2.5,
        skirt_share: 0.254,
        skirt_flat: 2.28,
        glow_sun: 0.58,
        pixel: 0.036,
        core: 0.3,
        core_size: 0.72,
        disc_bright: 0.002,
        warmth: 1.0,
        warmth_height: 0.118,
        warmth_sun: 1.0,
        warmth_round: 0.219,
        warm_green: 0.562,
        warm_blue: 0.0,
        grain: 0.05,
        moon: true,
        moon_elevation: 25.0,
        moon_azimuth: 0.0,
        moon_size: 0.52,
        moon_bright: 2.8,
        earthshine: 0.001,
        maria: 0.25,
        moon_tilt: 0.0,
        moon_phase: 0.0,
        moon_soft: 0.85,
        moon_horns: 0.1,
        haze: 0.625,
        haze_height: 0.0284,
        haze_tint: [0.175, 0.108, 0.089],
        broad: 0.0069,
        broad_width: 11.77,
        glow_low: 0.159,
        uneven: 0.25,
        haze_sun: [1.9, 0.07, 0.0],
        haze_sun_width: 2.45,
    }
}

/// `(elevation, azimuth)` in degrees of a moon `elongation` degrees from the
/// sun, straight above it on the sky (the evening crescent's place).
pub fn moon_above_sun(sun_elevation: f32, sun_azimuth: f32, elongation: f32) -> (f32, f32) {
    ((sun_elevation + elongation).min(90.0), sun_azimuth)
}

/// The recipe as `(param, value)` pairs (the dome's `sky.*` params).
pub fn photoreal_params() -> Vec<(&'static str, ParamValue)> {
    let s = photoreal_sky();
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
    out.push(("sky.tint", ParamValue::Color(s.horizon.tint)));
    out.push(("sky.zenith_deep", ParamValue::F32(s.horizon.deep)));
    out.push(("sky.cloud_volume", ParamValue::Bool(s.horizon.volume.on)));
    let d = photoreal();
    out.push(("sky.discs", ParamValue::Bool(d.on)));
    out.push(("sky.moon", ParamValue::Bool(d.moon)));
    out.push(("sky.disc_size", ParamValue::F32(d.sun_size)));
    out.push(("sky.disc_limb", ParamValue::F32(d.limb)));
    out.push(("sky.disc_glow", ParamValue::F32(d.glow)));
    out.push(("sky.disc_glow_width", ParamValue::F32(d.glow_width)));
    out.push(("sky.disc_skirt", ParamValue::F32(d.skirt)));
    out.push(("sky.disc_skirt_share", ParamValue::F32(d.skirt_share)));
    out.push(("sky.disc_skirt_flat", ParamValue::F32(d.skirt_flat)));
    out.push(("sky.disc_glow_sun", ParamValue::F32(d.glow_sun)));
    out.push(("sky.disc_pixel", ParamValue::F32(d.pixel)));
    out.push(("sky.disc_core", ParamValue::F32(d.core)));
    out.push(("sky.disc_core_size", ParamValue::F32(d.core_size)));
    out.push(("sky.disc_bright", ParamValue::F32(d.disc_bright)));
    out.push(("sky.horizon_warmth", ParamValue::F32(d.warmth)));
    out.push(("sky.horizon_warmth_height", ParamValue::F32(d.warmth_height)));
    out.push(("sky.horizon_warmth_sun", ParamValue::F32(d.warmth_sun)));
    out.push(("sky.horizon_warmth_round", ParamValue::F32(d.warmth_round)));
    out.push(("sky.horizon_warm_green", ParamValue::F32(d.warm_green)));
    out.push(("sky.horizon_warm_blue", ParamValue::F32(d.warm_blue)));
    out.push(("sky.grain", ParamValue::F32(d.grain)));
    out.push(("sky.moon_tilt", ParamValue::F32(d.moon_tilt)));
    out.push(("sky.moon_phase", ParamValue::F32(d.moon_phase)));
    out.push(("sky.moon_soft", ParamValue::F32(d.moon_soft)));
    out.push(("sky.moon_horns", ParamValue::F32(d.moon_horns)));
    out.push(("sky.horizon_haze", ParamValue::F32(d.haze)));
    out.push(("sky.horizon_haze_height", ParamValue::F32(d.haze_height)));
    out.push(("sky.horizon_haze_red", ParamValue::F32(d.haze_tint[0])));
    out.push(("sky.horizon_haze_green", ParamValue::F32(d.haze_tint[1])));
    out.push(("sky.horizon_haze_blue", ParamValue::F32(d.haze_tint[2])));
    out.push(("sky.disc_broad", ParamValue::F32(d.broad)));
    out.push(("sky.disc_broad_width", ParamValue::F32(d.broad_width)));
    out.push(("sky.disc_glow_low", ParamValue::F32(d.glow_low)));
    out.push(("sky.uneven", ParamValue::F32(d.uneven)));
    out.push(("sky.horizon_haze_sun_red", ParamValue::F32(d.haze_sun[0])));
    out.push(("sky.horizon_haze_sun_green", ParamValue::F32(d.haze_sun[1])));
    out.push(("sky.horizon_haze_sun_blue", ParamValue::F32(d.haze_sun[2])));
    out.push(("sky.horizon_haze_sun_width", ParamValue::F32(d.haze_sun_width)));
    out.push(("sky.moon_size", ParamValue::F32(d.moon_size)));
    out.push(("sky.moon_bright", ParamValue::F32(d.moon_bright)));
    out.push(("sky.moon_earthshine", ParamValue::F32(d.earthshine)));
    out.push(("sky.moon_maria", ParamValue::F32(d.maria)));
    out
}

/// Set the dome's live params to the recipe (the moon's place is left as
/// it is). Returns the params this build doesn't have (none when the dome
/// is installed).
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

/// The colour saturation the recipe's sky is fitted under, on the
/// photoreal look's grade (column 73): the look's daylight grade (0.88)
/// puts about an eighth of the brightness into every colour, which lifts
/// the blue of the orange round a low sun to a quarter of white (a pale
/// orange); at 0.955 it sits at a sixth, the deep orange of a photographed
/// sunset, and the blue overhead stays soft.
pub const GRADE_SATURATION: f32 = 0.955;

/// Set the game's grade to the recipe's saturation ([`GRADE_SATURATION`])
/// where its look has the photoreal grade (`look.photoreal.grade.saturation`).
/// Returns whether it was set; a game with another look leaves its own
/// grade alone and gets the recipe's sky a little paler or deeper.
pub fn apply_photoreal_grade(reg: &mut ParamRegistry) -> bool {
    match reg.id("look.photoreal.grade.saturation") {
        Some(id) => reg.set(id, ParamValue::F32(GRADE_SATURATION)).is_ok(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_material_proc::{DiscHandles, SkyHandles};

    /// The recipe sets every param it names, inside its range, and the
    /// registry reads back the recipe's sky and discs.
    #[test]
    fn the_recipe_sets_its_params_in_range() {
        let mut reg = ParamRegistry::default();
        let sky = SkyHandles::register(&mut reg, &SkyParams::default());
        let discs = DiscHandles::register(&mut reg, &SkyDiscs::default());
        assert!(sky.is_ok() && discs.is_ok());
        let missing = apply_photoreal(&mut reg);
        assert!(missing.is_empty(), "{missing:?}");
        let (Ok(sky), Ok(discs)) = (sky, discs) else { return };
        assert_eq!(sky.read(&reg), photoreal_sky());
        let mut want = photoreal();
        let start = SkyDiscs::default();
        want.moon_elevation = start.moon_elevation;
        want.moon_azimuth = start.moon_azimuth;
        assert_eq!(discs.read(&reg), want);
    }

    /// Both discs are drawn at their true apparent size.
    #[test]
    fn the_discs_are_true_size() {
        let d = photoreal();
        assert!((d.sun_size - 0.53).abs() < 0.01 && (d.moon_size - 0.52).abs() < 0.01);
        assert_eq!(moon_above_sun(2.0, 180.0, 23.0), (25.0, 180.0));
    }
}
