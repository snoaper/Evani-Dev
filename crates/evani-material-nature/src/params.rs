//! The live params (`material.nature.*`) and the `nature_mat` shader slot
//! they fill: an 18-row table of vec4s the shader reads with `mat(i)`
//! (layout in `shaders/nature_common.wgsl`). The body is rebuilt and
//! swapped in whenever a value changes, as evani-fx's fog does.

use evani_core::params::{
    BakeTarget, BoolParam, I32Param, ParamDef, ParamError, ParamId, ParamRegistry, ParamValue,
};
use evani_core::slots::SlotSignature;

/// Rows of the material table.
pub const ROWS: usize = 18;

/// The slot the three nature shaders call.
pub const SLOT: SlotSignature = SlotSignature {
    name: "nature_mat",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Nature material table row i (water, glass, foliage, grass, thin, lamp; built from material.nature.* params)",
    default_body: "    return vec4<f32>(0.0);",
};

const OWNER: &str = "material.nature";

#[derive(Debug, Clone, Copy)]
enum Ty {
    Color([f32; 3]),
    /// default, min, max, step, unit
    F32(f32, f32, f32, f32, &'static str),
}

/// One tunable: its name under `material.nature.`, its table cell (a
/// colour fills a row's xyz) and what it does.
struct Def {
    name: &'static str,
    row: usize,
    col: usize,
    ty: Ty,
    desc: &'static str,
}

const fn c(name: &'static str, row: usize, v: [f32; 3], desc: &'static str) -> Def {
    Def {
        name,
        row,
        col: 0,
        ty: Ty::Color(v),
        desc,
    }
}

#[allow(clippy::too_many_arguments)]
const fn f(
    name: &'static str,
    row: usize,
    col: usize,
    d: f32,
    min: f32,
    max: f32,
    step: f32,
    unit: &'static str,
    desc: &'static str,
) -> Def {
    Def {
        name,
        row,
        col,
        ty: Ty::F32(d, min, max, step, unit),
        desc,
    }
}

static DEFS: [Def; 45] = [
    c("sky.horizon", 0, [0.62, 0.72, 0.82], "Sky colour at the horizon that water and glass reflect (linear RGB)"),
    c("sky.zenith", 1, [0.24, 0.42, 0.72], "Sky colour overhead that water and glass reflect (linear RGB)"),
    f("sky.match_background", 0, 3, 1.0, 0.0, 1.0, 0.01, "", "1 = the reflected sky takes the colour and brightness of the game's background (render.clear_color), 0 = the sky colours as given"),
    c("water.deep_color", 2, [0.45, 0.74, 0.78], "Colour light keeps after crossing `water.clarity` metres of water"),
    f("water.clarity", 2, 3, 2.5, 0.05, 50.0, 0.05, "m", "Metres of water that tint light to `water.deep_color`"),
    c("water.scatter_color", 3, [0.08, 0.30, 0.34], "Colour deep water glows with from the light scattered inside it"),
    f("water.foam_width", 3, 3, 0.3, 0.0, 3.0, 0.01, "m", "Depth under the surface where shoreline foam fades out"),
    f("water.wave_scale", 4, 0, 0.9, 0.05, 8.0, 0.01, "1/m", "Wave frequency (higher = smaller waves)"),
    f("water.wave_speed", 4, 1, 1.0, 0.0, 4.0, 0.01, "x", "How fast the waves move"),
    f("water.wave_strength", 4, 2, 0.12, 0.0, 1.0, 0.005, "", "How steep the waves are"),
    f("water.glint", 4, 3, 1.0, 0.0, 8.0, 0.05, "x", "Strength of the sun's sparkle on the waves"),
    f("water.foam", 5, 0, 0.8, 0.0, 2.0, 0.01, "", "Shoreline foam strength (0 = none)"),
    f("water.refraction", 5, 1, 1.0, 0.0, 4.0, 0.01, "x", "How far the waves bend the view of the bed (High and Ultra)"),
    f("water.reflection", 5, 2, 1.0, 0.0, 2.0, 0.01, "x", "Strength of the sky reflection"),
    f("water.roughness", 5, 3, 0.08, 0.0, 1.0, 0.005, "", "Surface roughness (wider, softer sun glint)"),
    f("water.f0", 6, 0, 0.02, 0.0, 0.2, 0.001, "", "Reflectance looking straight down (water 0.02)"),
    c("glass.tint", 7, [0.90, 0.97, 0.94], "Colour light keeps after crossing `glass.thickness` of glass"),
    f("glass.thickness", 7, 3, 0.01, 0.001, 0.2, 0.001, "m", "Glass thickness that tints light to `glass.tint`"),
    f("glass.f0", 8, 0, 0.04, 0.0, 0.3, 0.001, "", "Reflectance looking straight at it (glass 0.04)"),
    f("glass.reflection", 8, 1, 1.0, 0.0, 2.0, 0.01, "x", "Strength of the sky reflection"),
    f("glass.roughness", 8, 2, 0.05, 0.0, 1.0, 0.005, "", "Roughness of the sun highlight"),
    f("glass.refraction", 8, 3, 1.0, 0.0, 4.0, 0.01, "x", "How far a bottle's curve bends the view behind it (High and Ultra)"),
    c("foliage.color", 9, [0.42, 0.66, 0.10], "Colour of sunlight through leaves"),
    f("foliage.strength", 9, 3, 1.0, 0.0, 4.0, 0.01, "x", "Strength of the glow through leaves"),
    f("foliage.power", 10, 0, 6.0, 0.5, 32.0, 0.1, "", "How tightly the glow gathers round the sun (higher = narrower)"),
    f("foliage.wrap", 10, 1, 0.5, 0.0, 1.0, 0.01, "", "Light wrapping round the lit side"),
    f("foliage.absorb", 10, 2, 1.2, 0.0, 20.0, 0.05, "1/m", "How fast a crown swallows light (higher = only the rim glows)"),
    f("foliage.variation", 10, 3, 0.5, 0.0, 1.0, 0.01, "", "Leaf-to-leaf colour and shade variation"),
    f("foliage.cells", 11, 0, 9.0, 0.5, 40.0, 0.1, "1/m", "Leaf clusters per metre for the variation"),
    c("grass.color", 12, [0.50, 0.72, 0.12], "Colour of sunlight through grass blades"),
    f("grass.strength", 12, 3, 1.0, 0.0, 4.0, 0.01, "x", "Strength of the glow through blade tips"),
    f("grass.power", 13, 0, 5.0, 0.5, 32.0, 0.1, "", "How tightly the glow gathers round the sun"),
    f("grass.spacing", 13, 1, 0.04, 0.005, 0.5, 0.001, "m", "Size of a blade cell for the variation"),
    f("grass.variation", 13, 2, 0.6, 0.0, 1.0, 0.01, "", "Blade-to-blade colour and shade variation"),
    f("grass.blade_thickness", 13, 3, 0.002, 0.0002, 0.02, 0.0001, "m", "Blade thickness (thicker lets less light through)"),
    c("thin.color", 14, [0.95, 0.32, 0.20], "Colour of light through thin things (ears, leaves, paper, wax)"),
    f("thin.strength", 14, 3, 1.0, 0.0, 4.0, 0.01, "x", "Strength of the glow through thin things"),
    f("thin.power", 15, 0, 8.0, 0.5, 32.0, 0.1, "", "How tightly the glow gathers round the sun"),
    f("thin.wrap", 15, 1, 0.5, 0.0, 1.0, 0.01, "", "Light wrapping round the lit side"),
    f("thin.absorb", 15, 2, 6.0, 0.0, 100.0, 0.1, "1/m", "How fast the material swallows light"),
    f("thin.thickness", 15, 3, 0.01, 0.0005, 0.2, 0.0005, "m", "Thickness of a thin patch (a leaf, a page)"),
    c("lamp.color", 16, [1.0, 0.70, 0.40], "Colour of the light inside a lampshade or candle"),
    f("lamp.intensity", 16, 3, 3.0, 0.0, 20.0, 0.05, "x", "Brightness of the light inside"),
    f("lamp.absorb", 17, 0, 60.0, 0.0, 500.0, 0.5, "1/m", "How fast the shade swallows the light inside"),
    f("lamp.shade_thickness", 17, 1, 0.004, 0.0005, 0.05, 0.0005, "m", "Shade thickness (the rim reads darker)"),
];

/// The registered handles.
#[derive(Debug, Clone)]
pub struct NatureParams {
    /// The table params in [`DEFS`] order.
    ids: Vec<ParamId>,
    pub enabled: BoolParam,
    pub refraction: BoolParam,
    pub max_surfaces: I32Param,
}

/// Default table (what the defaults give).
pub fn default_table() -> [[f32; 4]; ROWS] {
    let mut t = [[0.0f32; 4]; ROWS];
    for d in DEFS.iter() {
        put(&mut t, d, match d.ty {
            Ty::Color(v) => ParamValue::Color(v),
            Ty::F32(v, ..) => ParamValue::F32(v),
        });
    }
    t
}

fn put(t: &mut [[f32; 4]; ROWS], d: &Def, v: ParamValue) {
    let Some(row) = t.get_mut(d.row) else {
        return;
    };
    match v {
        ParamValue::Color(c) | ParamValue::Vec3(c) => row[..3].copy_from_slice(&c),
        ParamValue::F32(x) => {
            if let Some(cell) = row.get_mut(d.col) {
                *cell = x;
            }
        }
        _ => {}
    }
}

impl NatureParams {
    pub fn register(reg: &mut ParamRegistry) -> Result<Self, ParamError> {
        let mut ids = Vec::with_capacity(DEFS.len());
        for d in DEFS.iter() {
            let name = format!("{OWNER}.{}", d.name);
            let def = match d.ty {
                Ty::Color(v) => ParamDef::color(&name, v),
                Ty::F32(v, min, max, step, unit) => ParamDef::f32(&name, v).range(min, max).step(step).unit(unit),
            };
            ids.push(reg.register(def.describe(d.desc).owner(OWNER).bake(BakeTarget::Look))?);
        }
        let enabled = reg.register_bool(
            ParamDef::bool("material.nature.enabled", true)
                .describe("Nature and see-through materials on or off (water and glass then vanish: they are not in the solid draw)")
                .owner(OWNER)
                .bake(BakeTarget::None),
        )?;
        let refraction = reg.register_bool(
            ParamDef::bool("material.nature.refraction", true)
                .describe("High and Ultra: copy the screen once so water and bottles bend what is behind them (off = the Medium look)")
                .owner(OWNER)
                .bake(BakeTarget::Look),
        )?;
        let max_surfaces = reg.register_i32(
            ParamDef::i32("material.nature.max_surfaces", 40)
                .range(1.0, 200.0)
                .unit("surfaces")
                .describe("Most surfaces drawn (nearest first; the tier can lower it). Each five cost one more draw")
                .owner(OWNER)
                .bake(BakeTarget::Look),
        )?;
        Ok(Self {
            ids,
            enabled,
            refraction,
            max_surfaces,
        })
    }

    /// Every param that feeds the table (to watch for changes).
    pub fn table_ids(&self) -> &[ParamId] {
        &self.ids
    }

    /// The table from the current values.
    pub fn table(&self, reg: &ParamRegistry) -> [[f32; 4]; ROWS] {
        let mut t = default_table();
        for (d, id) in DEFS.iter().zip(&self.ids) {
            if let Some(v) = reg.value(*id) {
                put(&mut t, d, v);
            }
        }
        t
    }
}

/// A WGSL float literal (always with a decimal point).
fn lit(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{v:.5}")
}

/// The `nature_mat` slot body for a table (statements only).
pub fn body(t: &[[f32; 4]; ROWS]) -> String {
    let mut b = String::from("    // material.nature: the nature material table\n    var t = array<vec4<f32>, 18>(\n");
    for r in t {
        b.push_str(&format!(
            "        vec4<f32>({}, {}, {}, {}),\n",
            lit(r[0]),
            lit(r[1]),
            lit(r[2]),
            lit(r[3])
        ));
    }
    b.push_str("    );\n    return t[min(i, 17u)];");
    b
}
