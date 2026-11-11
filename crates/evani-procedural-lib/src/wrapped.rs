//! The engine's existing procedural modules as library entries, as they
//! are (Chris, 2026-11-03: "Yes, plug them in as they are"). A wrapped entry
//! names the module, its own live params and its evani.toml line; the
//! module itself is unchanged and draws as before. The library only adds
//! the entry's on/off switch (`procedural.<category>.<asset>.on`), which the
//! host (the game or the viewer) reads before handing the module a surface.
//!
//! - Storm sea and ocean: evani-material-nature's part `spectral-sea`.
//! - Lakes, ponds and glass: evani-material-nature's nature pass (the rivers
//!   are a CPU valley, crate::river, whose water is that pass's).
//! - Sky dome: evani-material-proc's part `sky` (installed by the kit).
//! - Short grass, long grass and wheat: evani-grass.

use evani_procedural::{AssetInfo, Output, ParamSpec, ProcAsset, Registry, Values};

/// A wrapped engine module's entry.
pub struct Wrapped(pub AssetInfo);

impl ProcAsset for Wrapped {
    fn info(&self) -> &AssetInfo {
        &self.0
    }

    fn params(&self) -> &[ParamSpec] {
        &[]
    }

    fn generate(&self, _v: &Values, _seed: u64, _style: &str) -> Output {
        Output::Module
    }
}

/// The wrapped module's params' prefixes.
pub const WATER: &str = "material.nature.water";
pub const GLASS: &str = "material.nature.glass";
pub const SKY: &str = "sky";
pub const GRASS: &str = "material.grass";

/// Every wrapped entry this build carries.
pub fn register(_r: &mut Registry) {
    #[cfg(feature = "water-oceans")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("water", "oceans", "Oceans", "nature", WATER))));
    #[cfg(feature = "water-ocean-waves")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("water", "ocean-waves", "Ocean waves (storm sea)", "nature", WATER))));
    #[cfg(feature = "water-lakes")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("water", "lakes", "Lakes", "nature", WATER))));
    #[cfg(feature = "water-ponds")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("water", "ponds", "Ponds", "nature", WATER))));
    #[cfg(feature = "textures-glass")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("textures", "glass", "Glass", "nature", GLASS))));
    #[cfg(feature = "sky-dynamic-sky-dome")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("sky", "dynamic-sky-dome", "Dynamic sky dome", "sky", SKY))));
    #[cfg(feature = "vegetation-short-grass")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("vegetation", "short-grass", "Short grass", "grass", GRASS))));
    #[cfg(feature = "vegetation-long-grass")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("vegetation", "long-grass", "Long grass", "grass", GRASS))));
    #[cfg(feature = "agriculture-wheat-plants")]
    _r.add(Box::new(Wrapped(AssetInfo::gpu("agriculture", "wheat-plants", "Wheat plants", "grass", GRASS))));
}

/// The grass kind a wrapped grass entry shows (`vegetation-long-grass` →
/// meadow); `None` for any other entry.
#[cfg(feature = "host-grass")]
pub fn grass_kind(feature: &str) -> Option<evani_grass::Kind> {
    match feature {
        "vegetation-short-grass" => Some(evani_grass::Kind::Lawn),
        "vegetation-long-grass" => Some(evani_grass::Kind::Meadow),
        "agriculture-wheat-plants" => Some(evani_grass::Kind::Wheat),
        _ => None,
    }
}

/// The short grass as a grazed pasture, its photoreal recipe (Asset Studio
/// column 68): the lawn kind's own live params (`material.grass.lawn.*`) and
/// the field-wide ones the pasture needs, by name. Uneven, tufted blades a
/// hand high in clumps, fresh yellow-green with olive and sun-bleached
/// blades among them, a few seed stalks, and worn patches of dark bare soil;
/// the module's own carpet carries the sward from the blades' reach to the
/// horizon. A game sets them once in its param registry and hands evani-grass
/// [`short_grass_patch`]; the mown lawn is the kind's defaults, untouched.
#[cfg(feature = "vegetation-short-grass")]
pub fn short_grass_pasture() -> Vec<(&'static str, evani_core::params::ParamValue)> {
    use evani_core::params::ParamValue::{Bool, Color, F32};
    vec![
        ("material.grass.lawn.height", F32(0.125)),
        ("material.grass.lawn.height_var", F32(0.5)),
        ("material.grass.lawn.width", F32(0.0075)),
        ("material.grass.lawn.density", F32(3000.0)),
        ("material.grass.lawn.lean", F32(0.6)),
        ("material.grass.lawn.curl", F32(1.0)),
        ("material.grass.lawn.stiffness", F32(0.4)),
        ("material.grass.lawn.root", Color([0.11, 0.155, 0.042])),
        ("material.grass.lawn.tip", Color([0.33, 0.4, 0.11])),
        ("material.grass.lawn.dry", Color([0.5, 0.44, 0.2])),
        ("material.grass.lawn.dryness", F32(0.22)),
        ("material.grass.lawn.variation", F32(0.55)),
        ("material.grass.lawn.clump", F32(0.8)),
        ("material.grass.lawn.bare", F32(0.06)),
        ("material.grass.lawn.soil", Color([0.27, 0.2, 0.14])),
        ("material.grass.lawn.mud", F32(0.1)),
        ("material.grass.lawn.heads", F32(0.03)),
        ("material.grass.lawn.droop", F32(0.25)),
        // Grazed into tussocks of every size, with clusters of dock and
        // plantain rosettes; the soil open in a few irregular openings.
        ("material.grass.lawn.tuft", F32(0.7)),
        ("material.grass.lawn.weeds", F32(0.35)),
        // A fine sward: thin twisting blades over matted undergrowth, in
        // patches of deeper green, olive and straw, paler far off.
        ("material.grass.lawn.fine", F32(1.0)),
        ("material.grass.lawn.mottle", F32(1.0)),
        ("material.grass.lawn.sheen", F32(0.55)),
        // The carpet under the blades and past their reach.
        ("material.grass.ground", Bool(true)),
        // Tillers of a few leaves each: under half the tier's blades, three
        // segments along a near one (at most 498,400 triangles at High, from
        // any camera), less shade at their feet.
        ("material.grass.density", F32(0.8)),
        ("material.grass.blades", F32(0.445)),
        ("material.grass.segments", F32(0.4)),
        ("material.grass.ao", F32(0.55)),
        ("material.grass.soil", Color([0.13, 0.095, 0.07])),
    ]
}

/// The short grass's patch for evani-grass: a square of the lawn kind `half`
/// metres each way round `centre` (plan x, z) on level ground at `ground_y`,
/// thinning out to bare ground over its outer twentieth. A game hands it to
/// `Grass::add_patch`; with [`short_grass_pasture`] set it is the pasture.
#[cfg(feature = "vegetation-short-grass")]
pub fn short_grass_patch(centre: [f32; 2], half: f32, ground_y: f32) -> evani_grass::Patch {
    let h = half.max(1.0);
    evani_grass::Patch::new(evani_grass::Kind::Lawn, [centre[0] - h, centre[1] - h], [centre[0] + h, centre[1] + h]).at(ground_y).feather((h * 0.05).clamp(0.5, 8.0))
}

/// The evani-grass blades a library terrain wears over its turf (procedural
/// realism round 8: real blades from the existing grass module, no new
/// grass shader): for the flat field (`terrain-flat-terrain`), a lawn patch
/// over the whole top of a plot `size` metres square placed at `at`, its
/// ground the turf's middle height (the soil depth `edge`, half the gentle
/// `bumps` and the fine lumps), so most blades stand clear of the turf (a
/// few sink into a bump, a few stand a little proud in a hollow), ending at
/// the plot's edge, at a third of the lawn's density, so the turf's dry and
/// worn patches show between the blades (at full density the blades hid
/// them and read as a golf green).
/// A game adds it to evani-grass's `Grass` resource beside the placed field;
/// the Material library does the same. `None` for any other asset (the
/// rolling hills wait for blades that follow the ground: CORE_REQUESTS
/// `grass-heightfield`).
/// The evani-grass blades the rolling hills wear (round 9, CORE_REQUESTS
/// `grass-heightfield`): a lawn patch over the plot placed at `at`, its
/// ground the hills' own top surface from their live params in `reg`
/// (`procedural.terrain.rolling_hills.*`), so the blades stand on the
/// slopes. A game adds it with `Grass::add_patch_on(patch, n, &heights)`
/// beside the placed hills; the Material library does the same.
#[cfg(all(feature = "host-grass", feature = "terrain-rolling-hills"))]
pub fn hills_grass(reg: &evani_core::params::ParamRegistry, at: [f32; 3]) -> (evani_grass::Patch, usize, Vec<f32>) {
    let a = crate::terrain::RollingHills;
    let specs = evani_procedural::Registry::specs(&a);
    let v = Values::read(reg, &a.info().prefix(), &specs);
    let (size, n, h) = crate::terrain::hills_ground(&v, v.f("seed").max(0.0) as u64);
    let half = size * 0.5;
    let patch = evani_grass::Patch::new(evani_grass::Kind::Lawn, [at[0] - half, at[2] - half], [at[0] + half, at[2] + half]).feather(0.06).density(0.6);
    (patch, n, h.iter().map(|y| y + at[1] + 0.02).collect())
}

#[cfg(feature = "host-grass")]
pub fn turf_grass(feature: &str, size: f32, edge: f32, bumps: f32, at: [f32; 3]) -> Option<evani_grass::Patch> {
    let h = size.max(0.1) * 0.5;
    match feature {
        "terrain-flat-terrain" => Some(evani_grass::Patch::new(evani_grass::Kind::Lawn, [at[0] - h, at[2] - h], [at[0] + h, at[2] + h]).at(at[1] + edge.max(0.0) + bumps.max(0.0) * 0.5 + 0.03).feather(0.06).density(0.8)),
        _ => None,
    }
}
