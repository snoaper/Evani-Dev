//! Every grass setting as a live param (`material.grass.*`): the field-wide
//! ones (wind, wetness, trampling, light, quality) and one set per kind
//! (`material.grass.meadow.height`, `material.grass.wheat.ear_length`...).
//! The four kinds are parameter sets on the same blade system: the long
//! meadow, the short lawn, the muddy trampled ground and uncut wheat or hay
//! (a stalk with a drooping ear). A change shows on the next frame: the
//! values go into the generation uniform every frame, nothing is recompiled.

use evani_core::params::{BakeTarget, ParamDef, ParamError, ParamId, ParamRegistry, ParamValue};

use crate::Kind;

/// Owner of every grass param.
pub const OWNER: &str = "material.grass";

#[derive(Debug, Clone, Copy)]
enum Ty {
    F,
    B,
    C,
}

#[derive(Debug, Clone, Copy)]
struct Def {
    name: &'static str,
    ty: Ty,
    d: [f32; 3],
    min: f32,
    max: f32,
    step: f32,
    unit: &'static str,
    desc: &'static str,
}

const fn f(name: &'static str, d: f32, min: f32, max: f32, step: f32, unit: &'static str, desc: &'static str) -> Def {
    Def { name, ty: Ty::F, d: [d, 0.0, 0.0], min, max, step, unit, desc }
}

const fn b(name: &'static str, d: bool, desc: &'static str) -> Def {
    Def { name, ty: Ty::B, d: [if d { 1.0 } else { 0.0 }, 0.0, 0.0], min: 0.0, max: 1.0, step: 1.0, unit: "", desc }
}

const fn c(name: &'static str, d: [f32; 3], desc: &'static str) -> Def {
    Def { name, ty: Ty::C, d, min: 0.0, max: 1.0, step: 0.01, unit: "", desc }
}

/// Field-wide settings, in [`G`] order (`material.grass.<name>`).
static GLOBAL: [Def; 27] = [
    b("enabled", true, "Grass on or off"),
    f("quality", -1.0, -1.0, 4.0, 1.0, "", "Grass quality: -1 by the device tier, else 0 Lite, 1 Low (phones), 2 Medium, 3 High, 4 Ultra"),
    f("density", 1.0, 0.0, 2.0, 0.01, "x", "Blades per square metre against each kind's own density"),
    f("distance", 1.0, 0.25, 2.0, 0.01, "x", "How far grass is drawn, against the tier's distance"),
    f("wind.strength", 0.55, 0.0, 3.0, 0.01, "", "Steady wind: how far blades lean with the breeze (tall blades bend most)"),
    f("wind.direction", 35.0, 0.0, 360.0, 1.0, "deg", "Heading the wind blows towards (0 = +x, 90 = +z)"),
    f("wind.gust", 0.9, 0.0, 3.0, 0.01, "", "Gusts: the travelling waves that sweep across the field"),
    f("wind.gust_speed", 5.5, 0.0, 30.0, 0.1, "m/s", "How fast the gust waves travel"),
    f("wind.gust_scale", 16.0, 2.0, 80.0, 0.5, "m", "Size of the gust waves"),
    f("wind.flutter", 0.5, 0.0, 2.0, 0.01, "", "Fast flutter of single blades"),
    f("wind.sway", 0.32, 0.0, 2.0, 0.01, "Hz", "Speed of the slow back-and-forth sway"),
    f("wetness", 0.0, 0.0, 1.0, 0.01, "", "Rain: 0 dry, 1 soaked. Wet grass is darker, glossier, droops and springs back slower"),
    f("wet.darken", 0.45, 0.0, 1.0, 0.01, "", "How much darker soaked grass is"),
    f("wet.gloss", 0.8, 0.0, 1.0, 0.01, "", "How glossy soaked grass is (the wet sheen)"),
    f("wet.droop", 0.55, 0.0, 1.5, 0.01, "rad", "How far soaked blades droop"),
    f("trample.strength", 1.0, 0.0, 1.5, 0.01, "", "How flat a character presses the grass"),
    f("trample.half_life", 8.0, 0.2, 120.0, 0.1, "s", "Spring-back: seconds for trampled grass to rise half way"),
    b("trample.keep_flat", false, "Keep trampled grass flat (no spring-back)"),
    f("trample.wet_slowdown", 2.0, 0.0, 6.0, 0.05, "x", "Soaked grass springs back this many times slower (1 + wetness x this)"),
    f("translucency", 1.0, 0.0, 2.0, 0.01, "", "Sunlight glowing through blades lit from behind (High and Ultra; Medium half)"),
    f("round", 0.7, 0.0, 1.5, 0.01, "", "Rounded-blade shading across each blade's width"),
    f("ao", 0.65, 0.0, 1.0, 0.01, "", "Darkening deep in the sward near the roots"),
    b("ground", true, "Draw a matching ground carpet under each patch, so thinned far grass still reads as grass"),
    f("specular", 1.0, 0.0, 2.0, 0.01, "", "Sun highlight on the blades"),
    c("soil", [0.16, 0.125, 0.09], "Colour of the ground round the patches: each patch's own soil fades into it at the patch's edge (linear)"),
    f("blades", 1.0, 0.05, 1.0, 0.01, "x", "Most blades drawn a frame, against the tier's own caps (a short sward needs fewer)"),
    f("segments", 1.0, 0.1, 1.0, 0.01, "x", "Segments along a blade, against the tier's own (short blades need fewer)"),
];

/// Indices into [`GLOBAL`].
pub mod g {
    pub const ENABLED: usize = 0;
    pub const QUALITY: usize = 1;
    pub const DENSITY: usize = 2;
    pub const DISTANCE: usize = 3;
    pub const WIND: usize = 4;
    pub const WIND_DIR: usize = 5;
    pub const GUST: usize = 6;
    pub const GUST_SPEED: usize = 7;
    pub const GUST_SCALE: usize = 8;
    pub const FLUTTER: usize = 9;
    pub const SWAY: usize = 10;
    pub const WETNESS: usize = 11;
    pub const WET_DARKEN: usize = 12;
    pub const WET_GLOSS: usize = 13;
    pub const WET_DROOP: usize = 14;
    pub const TRAMPLE: usize = 15;
    pub const HALF_LIFE: usize = 16;
    pub const KEEP_FLAT: usize = 17;
    pub const WET_SLOW: usize = 18;
    pub const TRANSLUCENCY: usize = 19;
    pub const ROUND: usize = 20;
    pub const AO: usize = 21;
    pub const GROUND: usize = 22;
    pub const SPECULAR: usize = 23;
    pub const SOIL: usize = 24;
    pub const BLADES: usize = 25;
    pub const SEGMENTS: usize = 26;
}

/// Per-kind settings, in [`k`] order (`material.grass.<kind>.<name>`); the
/// defaults are per kind ([`kind_default`]).
const KIND_FIELDS: [(&str, Ty, f32, f32, f32, &str, &str); 26] = [
    ("height", Ty::F, 0.0, 2.0, 0.005, "m", "Blade height"),
    ("height_var", Ty::F, 0.0, 1.0, 0.01, "", "How much blade heights vary"),
    ("width", Ty::F, 0.001, 0.05, 0.0005, "m", "Blade width at the root"),
    ("density", Ty::F, 0.0, 3000.0, 1.0, "/m2", "Blades per square metre close up"),
    ("lean", Ty::F, 0.0, 1.5, 0.01, "rad", "How far blades lean from upright at the root"),
    ("curl", Ty::F, 0.0, 2.0, 0.01, "rad", "How far blades arc over towards the tip"),
    ("stiffness", Ty::F, 0.0, 1.0, 0.01, "", "Stiff blades bend less in the wind"),
    ("flat", Ty::F, 0.0, 1.0, 0.01, "", "Share of blades already lying flat (trodden ground)"),
    ("root", Ty::C, 0.0, 1.0, 0.01, "", "Colour at the root (linear)"),
    ("tip", Ty::C, 0.0, 1.0, 0.01, "", "Colour at the tip (linear)"),
    ("dry", Ty::C, 0.0, 1.0, 0.01, "", "Colour of dry, sun-bleached blades (linear)"),
    ("dryness", Ty::F, 0.0, 1.0, 0.01, "", "How many blades are dry, in patches"),
    ("variation", Ty::F, 0.0, 1.0, 0.01, "", "Brightness and hue variation blade to blade"),
    ("clump", Ty::F, 0.0, 1.0, 0.01, "", "How much blades grow in clumps of similar height and direction"),
    ("bare", Ty::F, 0.0, 1.0, 0.01, "", "Bare patches of ground between the blades"),
    ("ear_length", Ty::F, 0.0, 0.6, 0.01, "", "Seed head (ear) as a share of the stalk; 0 = a plain blade"),
    ("ear_width", Ty::F, 0.0, 0.05, 0.0005, "m", "Seed head (ear) width"),
    ("soil", Ty::C, 0.0, 1.0, 0.01, "", "Colour of the soil between the blades (linear)"),
    ("mud", Ty::F, 0.0, 1.0, 0.01, "", "How wet and muddy the bare soil is even without rain (puddles, sheen)"),
    ("heads", Ty::F, 0.0, 0.6, 0.01, "", "Share of stalks carrying a small seed head above the leaves"),
    ("droop", Ty::F, 0.0, 1.5, 0.01, "", "How far long leaves sag over at the tip under their own weight"),
    ("tuft", Ty::F, 0.0, 1.0, 0.01, "", "How far the sward grows in tussocks: long blades fanning out of each tuft, short grazed grass between, each tuft shading its lee (0 = an even sward)"),
    ("weeds", Ty::F, 0.0, 1.0, 0.01, "", "Broadleaf weeds: the share of 0.55 m plots carrying a rosette of broad, deeper green leaves, growing in clusters (0 = none)"),
    ("fine", Ty::F, 0.0, 1.0, 0.01, "", "How fine the sward is: each blade a tiller of a few thin leaves fanning out of one foot, lying in sprays, over matted fine undergrowth; it grows rank in patches of every size, on to the horizon; its tufts are of every size and are drawn as low mounds past the blades' reach, and its bare soil lies open in irregular openings (0 = the plain blade and ground)"),
    ("mottle", Ty::F, 0.0, 1.0, 0.01, "", "Patches of deeper green, olive and straw across the sward, from a hand's width to a few metres (0 = an even colour)"),
    ("sheen", Ty::F, 0.0, 1.0, 0.01, "", "How much lighter and paler the sward turns seen at a low angle, far off (0 = the same from every angle)"),
];

/// Indices into the per-kind fields.
pub mod k {
    pub const HEIGHT: usize = 0;
    pub const HEIGHT_VAR: usize = 1;
    pub const WIDTH: usize = 2;
    pub const DENSITY: usize = 3;
    pub const LEAN: usize = 4;
    pub const CURL: usize = 5;
    pub const STIFFNESS: usize = 6;
    pub const FLAT: usize = 7;
    pub const ROOT: usize = 8;
    pub const TIP: usize = 9;
    pub const DRY: usize = 10;
    pub const DRYNESS: usize = 11;
    pub const VARIATION: usize = 12;
    pub const CLUMP: usize = 13;
    pub const BARE: usize = 14;
    pub const EAR_LENGTH: usize = 15;
    pub const EAR_WIDTH: usize = 16;
    pub const SOIL: usize = 17;
    pub const MUD: usize = 18;
    pub const HEADS: usize = 19;
    pub const DROOP: usize = 20;
    pub const TUFT: usize = 21;
    pub const WEEDS: usize = 22;
    pub const FINE: usize = 23;
    pub const MOTTLE: usize = 24;
    pub const SHEEN: usize = 25;
    pub const COUNT: usize = 26;
}

/// The default of per-kind field `i` for `kind` (colours as `[r, g, b]`,
/// numbers in `[0]`).
pub fn kind_default(kind: Kind, i: usize) -> [f32; 3] {
    let n = |v: f32| [v, 0.0, 0.0];
    match (kind, i) {
        // Long meadow: knee-high, soft, varied green with sun-bleached
        // patches, leaning and arcing over.
        (Kind::Meadow, k::HEIGHT) => n(0.62),
        (Kind::Meadow, k::HEIGHT_VAR) => n(0.42),
        (Kind::Meadow, k::WIDTH) => n(0.011),
        (Kind::Meadow, k::DENSITY) => n(340.0),
        (Kind::Meadow, k::LEAN) => n(0.38),
        (Kind::Meadow, k::CURL) => n(1.1),
        (Kind::Meadow, k::STIFFNESS) => n(0.25),
        (Kind::Meadow, k::FLAT) => n(0.0),
        (Kind::Meadow, k::ROOT) => [0.025, 0.05, 0.012],
        (Kind::Meadow, k::TIP) => [0.13, 0.27, 0.045],
        (Kind::Meadow, k::DRY) => [0.46, 0.38, 0.15],
        (Kind::Meadow, k::DRYNESS) => n(0.35),
        (Kind::Meadow, k::VARIATION) => n(0.7),
        (Kind::Meadow, k::CLUMP) => n(0.55),
        (Kind::Meadow, k::BARE) => n(0.0),
        (Kind::Meadow, k::SOIL) => [0.12, 0.09, 0.06],
        (Kind::Meadow, k::HEADS) => n(0.1),
        (Kind::Meadow, k::DROOP) => n(0.45),
        // Short lawn: dense, even, upright, rich green.
        (Kind::Lawn, k::HEIGHT) => n(0.085),
        (Kind::Lawn, k::HEIGHT_VAR) => n(0.25),
        (Kind::Lawn, k::WIDTH) => n(0.0042),
        (Kind::Lawn, k::DENSITY) => n(1500.0),
        (Kind::Lawn, k::LEAN) => n(0.18),
        (Kind::Lawn, k::CURL) => n(0.25),
        (Kind::Lawn, k::STIFFNESS) => n(0.55),
        (Kind::Lawn, k::FLAT) => n(0.0),
        (Kind::Lawn, k::ROOT) => [0.025, 0.065, 0.012],
        (Kind::Lawn, k::TIP) => [0.10, 0.25, 0.04],
        (Kind::Lawn, k::DRY) => [0.20, 0.22, 0.08],
        (Kind::Lawn, k::DRYNESS) => n(0.05),
        (Kind::Lawn, k::VARIATION) => n(0.18),
        (Kind::Lawn, k::CLUMP) => n(0.15),
        (Kind::Lawn, k::BARE) => n(0.0),
        (Kind::Lawn, k::SOIL) => [0.11, 0.085, 0.06],
        (Kind::Lawn, k::DROOP) => n(0.05),
        // Muddy, trampled ground: sparse, short, broken and lying blades
        // over bare wet mud, olive and brown.
        (Kind::Mud, k::HEIGHT) => n(0.22),
        (Kind::Mud, k::HEIGHT_VAR) => n(0.6),
        (Kind::Mud, k::WIDTH) => n(0.006),
        (Kind::Mud, k::DENSITY) => n(260.0),
        (Kind::Mud, k::LEAN) => n(0.55),
        (Kind::Mud, k::CURL) => n(0.9),
        (Kind::Mud, k::STIFFNESS) => n(0.2),
        (Kind::Mud, k::FLAT) => n(0.65),
        (Kind::Mud, k::ROOT) => [0.05, 0.045, 0.02],
        (Kind::Mud, k::TIP) => [0.24, 0.23, 0.09],
        (Kind::Mud, k::DRY) => [0.30, 0.24, 0.12],
        (Kind::Mud, k::DRYNESS) => n(0.5),
        (Kind::Mud, k::VARIATION) => n(0.4),
        (Kind::Mud, k::CLUMP) => n(0.6),
        (Kind::Mud, k::BARE) => n(0.6),
        (Kind::Mud, k::SOIL) => [0.075, 0.055, 0.04],
        (Kind::Mud, k::MUD) => n(0.75),
        (Kind::Mud, k::DROOP) => n(0.3),
        // Uncut wheat or hay: tall stiff golden stalks with a nodding ear.
        (Kind::Wheat, k::HEIGHT) => n(0.95),
        (Kind::Wheat, k::HEIGHT_VAR) => n(0.12),
        (Kind::Wheat, k::WIDTH) => n(0.0045),
        (Kind::Wheat, k::DENSITY) => n(260.0),
        (Kind::Wheat, k::LEAN) => n(0.1),
        (Kind::Wheat, k::CURL) => n(0.22),
        (Kind::Wheat, k::STIFFNESS) => n(0.7),
        (Kind::Wheat, k::FLAT) => n(0.0),
        (Kind::Wheat, k::ROOT) => [0.16, 0.15, 0.05],
        (Kind::Wheat, k::TIP) => [0.55, 0.40, 0.14],
        (Kind::Wheat, k::DRY) => [0.62, 0.50, 0.26],
        (Kind::Wheat, k::DRYNESS) => n(0.3),
        (Kind::Wheat, k::VARIATION) => n(0.2),
        (Kind::Wheat, k::CLUMP) => n(0.3),
        (Kind::Wheat, k::BARE) => n(0.0),
        (Kind::Wheat, k::EAR_LENGTH) => n(0.13),
        (Kind::Wheat, k::EAR_WIDTH) => n(0.012),
        (Kind::Wheat, k::SOIL) => [0.17, 0.13, 0.09],
        (Kind::Wheat, k::DROOP) => n(0.1),
        _ => n(0.0),
    }
}

/// The registered handles.
#[derive(Debug, Clone)]
pub struct GrassParams {
    global: Vec<ParamId>,
    kinds: Vec<[ParamId; k::COUNT]>,
    /// `render.*` params the shading reads (missing ones fall back).
    light_dir: Option<ParamId>,
    light_color: Option<ParamId>,
    light_intensity: Option<ParamId>,
    ambient: Option<ParamId>,
    clear: Option<ParamId>,
    fog_own: Option<ParamId>,
    fog_color: Option<ParamId>,
}

/// Every grass param name, field-wide first, then each kind's.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = GLOBAL.iter().map(|d| format!("{OWNER}.{}", d.name)).collect();
    for kind in Kind::ALL {
        for (n, ..) in KIND_FIELDS {
            v.push(format!("{OWNER}.{}.{n}", kind.name()));
        }
    }
    v
}

/// `range`: min, max and step of a number.
fn def_of(name: &str, ty: Ty, d: [f32; 3], range: (f32, f32, f32), unit: &str, desc: &str) -> ParamDef {
    let (min, max, step) = range;
    let def = match ty {
        Ty::F => ParamDef::f32(name, d[0]).range(min, max).step(step).unit(unit),
        Ty::B => ParamDef::bool(name, d[0] > 0.5),
        Ty::C => ParamDef::color(name, d),
    };
    def.describe(desc).owner(OWNER)
}

impl GrassParams {
    /// Register every grass param (the render params are looked up, not
    /// registered: add the render module first for the real light).
    pub fn register(reg: &mut ParamRegistry) -> Result<Self, ParamError> {
        let mut global = Vec::with_capacity(GLOBAL.len());
        for d in &GLOBAL {
            let name = format!("{OWNER}.{}", d.name);
            let bake = if d.name == "quality" { BakeTarget::None } else { BakeTarget::Look };
            global.push(reg.register(def_of(&name, d.ty, d.d, (d.min, d.max, d.step), d.unit, d.desc).bake(bake))?);
        }
        let mut kinds = Vec::with_capacity(Kind::ALL.len());
        for kind in Kind::ALL {
            let mut ids = [ParamId(0); k::COUNT];
            for (i, (n, ty, min, max, step, unit, desc)) in KIND_FIELDS.iter().enumerate() {
                let name = format!("{OWNER}.{}.{n}", kind.name());
                let desc = format!("{}: {}", kind.label(), desc);
                ids[i] = reg.register(def_of(&name, *ty, kind_default(kind, i), (*min, *max, *step), unit, &desc).bake(BakeTarget::Look))?;
            }
            kinds.push(ids);
        }
        Ok(Self {
            global,
            kinds,
            light_dir: reg.id("render.light.direction"),
            light_color: reg.id("render.light.color"),
            light_intensity: reg.id("render.light.intensity"),
            ambient: reg.id("render.ambient"),
            clear: reg.id("render.clear_color"),
            fog_own: reg.id("render.fog.own_color"),
            fog_color: reg.id("render.fog.color"),
        })
    }

    /// Field-wide number `i` ([`g`]).
    pub fn f(&self, reg: &ParamRegistry, i: usize) -> f32 {
        match self.global.get(i).and_then(|id| reg.value(*id)) {
            Some(ParamValue::F32(x)) => x,
            Some(ParamValue::Bool(b)) => f32::from(u8::from(b)),
            _ => GLOBAL.get(i).map_or(0.0, |d| d.d[0]),
        }
    }

    /// Field-wide switch `i` ([`g`]).
    pub fn on(&self, reg: &ParamRegistry, i: usize) -> bool {
        self.f(reg, i) > 0.5
    }

    /// Field-wide colour `i` ([`g`]).
    pub fn color(&self, reg: &ParamRegistry, i: usize) -> [f32; 3] {
        match self.global.get(i).and_then(|id| reg.value(*id)) {
            Some(ParamValue::Color(c) | ParamValue::Vec3(c)) => c,
            _ => GLOBAL.get(i).map_or([0.0; 3], |d| d.d),
        }
    }

    /// Per-kind number `i` ([`k`]).
    pub fn kf(&self, reg: &ParamRegistry, kind: Kind, i: usize) -> f32 {
        match self.kinds.get(kind as usize).and_then(|ids| ids.get(i)).and_then(|id| reg.value(*id)) {
            Some(ParamValue::F32(x)) => x,
            _ => kind_default(kind, i)[0],
        }
    }

    /// Per-kind colour `i` ([`k`]).
    pub fn kc(&self, reg: &ParamRegistry, kind: Kind, i: usize) -> [f32; 3] {
        match self.kinds.get(kind as usize).and_then(|ids| ids.get(i)).and_then(|id| reg.value(*id)) {
            Some(ParamValue::Color(c) | ParamValue::Vec3(c)) => c,
            _ => kind_default(kind, i),
        }
    }

    /// The scene light: direction towards the sun (normalised), colour times
    /// intensity, ambient and the fog colour (the background, or
    /// `render.fog.color` when the game's fog has its own colour).
    pub fn light(&self, reg: &ParamRegistry) -> Light {
        let v3 = |id: Option<ParamId>, d: [f32; 3]| match id.and_then(|i| reg.value(i)) {
            Some(ParamValue::Vec3(v) | ParamValue::Color(v)) => v,
            _ => d,
        };
        let k = match self.light_intensity.and_then(|i| reg.value(i)) {
            Some(ParamValue::F32(x)) => x,
            _ => 1.0,
        };
        let dir = crate::plan::norm3(v3(self.light_dir, [0.4, 1.0, 0.3]));
        let col = v3(self.light_color, [1.0; 3]);
        let own = matches!(self.fog_own.and_then(|i| reg.value(i)), Some(ParamValue::Bool(true)));
        let fog = if own { v3(self.fog_color, [0.6, 0.7, 0.8]) } else { v3(self.clear, [0.6, 0.7, 0.8]) };
        Light { dir, color: col.map(|c| c * k), ambient: v3(self.ambient, [0.25, 0.28, 0.32]), fog }
    }
}

/// The scene light the blades are shaded with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub dir: [f32; 3],
    pub color: [f32; 3],
    pub ambient: [f32; 3],
    pub fog: [f32; 3],
}
