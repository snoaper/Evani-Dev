//! The kit's art as data (`data/art.json`, K5b): the look bible's palette,
//! the material roles the kit asks of whatever look is active, and part
//! recipes built from the engine's built-in meshes.
//!
//! The kit never names a look. It asks for a material by role (a short list
//! of names to try, first match wins, else the look's first material) and
//! divides the target colour by that material's base colour, so the look
//! decides how a surface reacts to light (roughness, metal, rim, ramp)
//! while the palette from the target pack stays the colour the player sees.

use std::collections::BTreeMap;

use evani_core::plugin::LookData;
use evani_render::math::{quat_axis_angle, quat_mul};
use evani_render::{MeshId, MeshInstance, RenderWorld};
use serde::Deserialize;

/// The art file.
pub const ART_JSON: &str = include_str!("../data/art.json");

/// Material roles, in [`MAT_ROLES`] order.
pub const MAT_ROLES: [&str; 9] = ["ground", "rock", "water", "paint", "wall", "metal", "crystal", "wood", "leaf"];

/// Indices into [`MAT_ROLES`] for the drawing code.
pub mod role {
    pub const GROUND: usize = 0;
    pub const ROCK: usize = 1;
    pub const WATER: usize = 2;
    pub const PAINT: usize = 3;
    pub const CRYSTAL: usize = 6;
}

/// sRGB hex text (`#rrggbb`) to linear RGBA.
pub fn hex_lin(s: &str) -> Result<[f32; 4], String> {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("art.json: bad colour {s}"))?;
    if h.len() != 6 {
        return Err(format!("art.json: bad colour {s}"));
    }
    let c = |sh: u32| {
        let x = ((v >> sh) & 0xff) as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    Ok([c(16), c(8), c(0), 1.0])
}

/// Scale a colour's RGB.
pub fn scale(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// Blend two colours' RGB (`t` = 0 is `a`).
pub fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3]]
}

/// A small integer hash for draw-only scatter (props, never gameplay).
pub fn hash(a: i32, b: i32, salt: u32) -> u32 {
    let mut h = (a as u32).wrapping_mul(0x9e37_79b1) ^ (b as u32).wrapping_mul(0x85eb_ca77) ^ salt.wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h
}

/// `hash` as 0..1.
pub fn hash01(a: i32, b: i32, salt: u32) -> f32 {
    (hash(a, b, salt) & 0xffff) as f32 / 65535.0
}

/// Where a part's colour comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paint {
    /// The owner's team colour.
    Team,
    Fixed([f32; 4]),
}

/// When a part shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Only {
    Always,
    /// A finished building.
    Built,
    /// A building site.
    Site,
    /// A worker carrying crystal.
    Carry,
}

/// One resolved part of a recipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub mesh: MeshId,
    pub at: [f32; 3],
    pub size: [f32; 3],
    pub paint: Paint,
    pub shade: f32,
    /// Index into [`MAT_ROLES`].
    pub mat: usize,
    /// The part's own turn (yaw, then tilt about its local x).
    pub rot: [f32; 4],
    pub only: Only,
    pub grow: bool,
    pub aim: bool,
}

#[derive(Deserialize)]
struct PartJson {
    m: String,
    at: [f32; 3],
    s: [f32; 3],
    c: String,
    mat: String,
    #[serde(default = "one")]
    k: f32,
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    tilt: f32,
    #[serde(default)]
    only: Option<String>,
    #[serde(default)]
    grow: bool,
    #[serde(default)]
    aim: bool,
}

fn one() -> f32 {
    1.0
}

/// A point light role.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LightJson {
    pub c: String,
    pub range: f32,
    pub intensity: f32,
    pub y: f32,
}

/// A resolved light role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightRole {
    pub color: [f32; 3],
    pub range: f32,
    pub intensity: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct FogArt {
    pub explored_dim: f32,
    pub explored_to_fog: f32,
    /// How far the backdrop beyond unexplored ground blends into the fog
    /// colour (1 = flat fog; below 1 keeps a dark silhouette).
    pub unexplored_to_fog: f32,
    pub rival_unseen_dim: f32,
    /// Cells either side of the fog line its gradient spreads over (0 = a
    /// hard edge). Default of `kit.base_building_rts.fog_feather_cells`.
    pub feather_cells: f32,
    /// Gradient strips per cell along each axis. Default of
    /// `kit.base_building_rts.fog_feather_strips`.
    pub feather_strips: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct TerrainArt {
    pub grass_top: f32,
    pub ford_top: f32,
    pub water_top: f32,
    pub rock_top: f32,
    pub fog_top: f32,
    pub tuft_density: f32,
    /// Odds a grass cell carries a flat patch of lighter and darker grass.
    pub patch_density: f32,
    pub border_pine_density: f32,
    pub outcrop_pine_density: f32,
    pub ford_stones: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct BackdropArt {
    pub depth_m: f32,
    pub step_m: f32,
    pub ridge_h: [f32; 2],
    pub pines_per_step: u32,
    /// Height scale of the ridges on the camera's (south) side, which stand
    /// between the camera and the map. Default of
    /// `kit.base_building_rts.backdrop_near_scale`.
    pub near_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RingArt {
    pub unit_segments: u32,
    pub unit_pad: f32,
    pub width: f32,
    pub building_pad: f32,
}

#[derive(Deserialize)]
struct LightsJson {
    crystal: LightJson,
    glow: LightJson,
    beacon: LightJson,
    shot: LightJson,
}

#[derive(Deserialize)]
struct ArtJson {
    colours: BTreeMap<String, String>,
    materials: BTreeMap<String, Vec<String>>,
    sun_dir: [f32; 3],
    #[serde(default)]
    render: BTreeMap<String, serde_json::Value>,
    fog: FogArt,
    terrain: TerrainArt,
    backdrop: BackdropArt,
    lights: LightsJson,
    rings: RingArt,
    recipes: BTreeMap<String, Vec<PartJson>>,
}

/// The palette roles the drawing uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    pub team: [[f32; 4]; 2],
    pub amber: [f32; 4],
    pub grass: [f32; 4],
    pub water: [f32; 4],
    pub ford: [f32; 4],
    pub rock: [f32; 4],
    pub rock_dark: [f32; 4],
    pub fog: [f32; 4],
    pub ui: [f32; 4],
    pub damage: [f32; 4],
    pub dust: [f32; 4],
    pub scorch: [f32; 4],
    pub valid: [f32; 4],
    pub far_ground: [f32; 4],
}

/// Every recipe the drawing uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipes {
    /// Units by kind (worker, trooper, rumbler).
    pub units: [Vec<Part>; 3],
    /// Buildings by kind (hub, foundry, bastion).
    pub buildings: [Vec<Part>; 3],
    pub crystal: Vec<Part>,
    pub pine: Vec<Part>,
    pub outcrop: Vec<Part>,
    pub ridge: Vec<Part>,
    pub ford_stone: Vec<Part>,
    pub tuft: Vec<Part>,
    /// Flat ground patches, drawn as decals.
    pub patch: Vec<Part>,
    pub flag: Vec<Part>,
    pub rubble: Vec<Part>,
}

/// The resolved art (a resource).
#[derive(Debug, Clone, PartialEq)]
pub struct Art {
    pub colours: Colours,
    /// Material names to try per role ([`MAT_ROLES`] order).
    pub materials: Vec<Vec<String>>,
    pub sun_dir: [f32; 3],
    /// Engine render params to set once (`render.<key>`), e.g. exposure.
    pub render: Vec<(String, serde_json::Value)>,
    pub fog: FogArt,
    pub terrain: TerrainArt,
    pub backdrop: BackdropArt,
    pub rings: RingArt,
    pub light_crystal: LightRole,
    pub light_glow: LightRole,
    pub light_beacon: LightRole,
    pub light_shot: LightRole,
    pub recipes: Recipes,
}

fn mesh_id(name: &str) -> Result<MeshId, String> {
    Ok(match name {
        "cube" => MeshId::CUBE,
        "sphere" => MeshId::SPHERE,
        "low_sphere" => MeshId::LOW_SPHERE,
        "plane" => MeshId::PLANE,
        "bevel" => MeshId::BEVEL_CUBE,
        "cylinder" => MeshId::CYLINDER,
        "cone" => MeshId::CONE,
        "dome" => MeshId::DOME,
        _ => return Err(format!("art.json: unknown mesh {name}")),
    })
}

impl Art {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let j: ArtJson = serde_json::from_str(json).map_err(|e| format!("art.json: {e}"))?;
        let col = |name: &str| -> Result<[f32; 4], String> {
            let hex = j.colours.get(name).ok_or_else(|| format!("art.json: missing colour {name}"))?;
            hex_lin(hex)
        };
        let colours = Colours {
            team: [col("team0")?, col("team1")?],
            amber: col("amber")?,
            grass: col("grass")?,
            water: col("water")?,
            ford: col("ford")?,
            rock: col("rock")?,
            rock_dark: col("rock_dark")?,
            fog: col("fog")?,
            ui: col("ui")?,
            damage: col("damage")?,
            dust: col("dust")?,
            scorch: col("scorch")?,
            valid: col("valid")?,
            far_ground: col("far_ground")?,
        };
        let materials = MAT_ROLES.iter().map(|r| j.materials.get(*r).cloned().unwrap_or_default()).collect();
        let part = |p: &PartJson| -> Result<Part, String> {
            let paint = if p.c == "team" { Paint::Team } else { Paint::Fixed(col(&p.c)?) };
            let mat = MAT_ROLES.iter().position(|r| *r == p.mat).ok_or_else(|| format!("art.json: unknown material role {}", p.mat))?;
            let only = match p.only.as_deref() {
                None => Only::Always,
                Some("built") => Only::Built,
                Some("site") => Only::Site,
                Some("carry") => Only::Carry,
                Some(o) => return Err(format!("art.json: unknown only {o}")),
            };
            let yaw = quat_axis_angle([0.0, 1.0, 0.0], p.yaw.to_radians());
            let tilt = quat_axis_angle([1.0, 0.0, 0.0], p.tilt.to_radians());
            Ok(Part {
                mesh: mesh_id(&p.m)?,
                at: p.at,
                size: p.s,
                paint,
                shade: p.k,
                mat,
                rot: quat_mul(yaw, tilt),
                only,
                grow: p.grow,
                aim: p.aim,
            })
        };
        let recipe = |name: &str| -> Result<Vec<Part>, String> {
            j.recipes.get(name).ok_or_else(|| format!("art.json: missing recipe {name}"))?.iter().map(part).collect()
        };
        let light = |l: &LightJson| -> Result<LightRole, String> {
            let c = col(&l.c)?;
            Ok(LightRole { color: [c[0], c[1], c[2]], range: l.range, intensity: l.intensity, y: l.y })
        };
        let recipes = Recipes {
            units: [recipe("worker")?, recipe("trooper")?, recipe("rumbler")?],
            buildings: [recipe("hub")?, recipe("foundry")?, recipe("bastion")?],
            crystal: recipe("crystal")?,
            pine: recipe("pine")?,
            outcrop: recipe("outcrop")?,
            ridge: recipe("ridge")?,
            ford_stone: recipe("ford_stone")?,
            tuft: recipe("tuft")?,
            patch: recipe("patch")?,
            flag: recipe("flag")?,
            rubble: recipe("rubble")?,
        };
        Ok(Self {
            colours,
            materials,
            sun_dir: j.sun_dir,
            render: j.render.into_iter().collect(),
            fog: j.fog,
            terrain: j.terrain,
            backdrop: j.backdrop,
            rings: j.rings,
            light_crystal: light(&j.lights.crystal)?,
            light_glow: light(&j.lights.glow)?,
            light_beacon: light(&j.lights.beacon)?,
            light_shot: light(&j.lights.shot)?,
            recipes,
        })
    }
}

/// The active look's material per role, with the colour factor that undoes
/// the material's own base colour (so the palette shows as written).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mats {
    pub id: [u32; MAT_ROLES.len()],
    pub undo: [[f32; 3]; MAT_ROLES.len()],
}

impl Mats {
    pub fn resolve(art: &Art, look: &LookData) -> Self {
        let mut id = [0u32; MAT_ROLES.len()];
        let mut undo = [[1.0f32; 3]; MAT_ROLES.len()];
        for (r, names) in art.materials.iter().enumerate().take(MAT_ROLES.len()) {
            let found = names.iter().find_map(|n| look.materials.iter().position(|m| m.name == *n));
            let i = found.unwrap_or(0);
            id[r] = i as u32;
            if let Some(m) = look.materials.get(i) {
                for (u, b) in undo[r].iter_mut().zip(m.base_color) {
                    *u = 1.0 / b.max(0.04);
                }
            }
        }
        Self { id, undo }
    }

    /// The instance colour that shows `c` through role `r`'s material.
    pub fn tint(&self, r: usize, c: [f32; 4]) -> [f32; 4] {
        let u = self.undo.get(r).copied().unwrap_or([1.0; 3]);
        [c[0] * u[0], c[1] * u[1], c[2] * u[2], c[3]]
    }

    pub fn id(&self, r: usize) -> u32 {
        self.id.get(r).copied().unwrap_or(0)
    }
}

/// How to place one recipe this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Ground point `[x, y, z]` of the object's origin.
    pub at: [f32; 3],
    /// Scale per axis (a unit's diameter, a footprint's side, a prop's size).
    pub scale: [f32; 3],
    /// Turn about y (radians; 0 faces +z, the camera side).
    pub yaw: f32,
    /// Turret turn for `aim` parts (radians).
    pub aim: f32,
    /// Construction progress 0..1 for `grow` parts (1 = finished).
    pub grow: f32,
    pub team: usize,
    pub built: bool,
    pub carry: bool,
    /// Colour multiplier (fog, last-known, sites).
    pub dim: f32,
    /// Blend towards this colour by `fade` (explored fog).
    pub fade: Option<([f32; 4], f32)>,
    /// Alpha below 1 is the forward pass's screen-door fade (ghosts).
    pub alpha: f32,
    /// Draw as decals on the ground (flat patches) instead of meshes.
    pub decal: bool,
}

impl Place {
    pub fn new(at: [f32; 3], scale: f32) -> Self {
        Self {
            at,
            scale: [scale; 3],
            yaw: 0.0,
            aim: 0.0,
            grow: 1.0,
            team: 0,
            built: true,
            carry: false,
            dim: 1.0,
            fade: None,
            alpha: 1.0,
            decal: false,
        }
    }
}

/// Turn `[x, z]` by `yaw` (local +z goes to `[sin, cos]`).
pub fn turn(x: f32, z: f32, yaw: f32) -> [f32; 2] {
    let (s, c) = yaw.sin_cos();
    [x * c + z * s, -x * s + z * c]
}

/// Draw a recipe.
pub fn draw(rw: &mut RenderWorld, art: &Art, mats: &Mats, parts: &[Part], p: &Place) {
    for part in parts {
        let show = match part.only {
            Only::Always => true,
            Only::Built => p.built,
            Only::Site => !p.built,
            Only::Carry => p.carry,
        };
        if !show {
            continue;
        }
        let g = if part.grow { p.grow.clamp(0.0, 1.0) } else { 1.0 };
        let yaw = if part.aim { p.aim } else { p.yaw };
        let lx = part.at[0] * p.scale[0];
        let ly = part.at[1] * p.scale[1] * g;
        let lz = part.at[2] * p.scale[2];
        let o = turn(lx, lz, yaw);
        let mut c = match part.paint {
            Paint::Team => art.colours.team[p.team & 1],
            Paint::Fixed(c) => c,
        };
        c = scale(c, part.shade * p.dim);
        if let Some((to, t)) = p.fade {
            c = mix(c, to, t);
        }
        c[3] = p.alpha;
        let mut i = MeshInstance::new(part.mesh, [p.at[0] + o[0], p.at[1] + ly, p.at[2] + o[1]]);
        i.scale = [part.size[0] * p.scale[0], part.size[1] * p.scale[1] * g, part.size[2] * p.scale[2]];
        i.rotation = quat_mul(quat_axis_angle([0.0, 1.0, 0.0], yaw), part.rot);
        i.material = mats.id(part.mat);
        i.color = mats.tint(part.mat, c);
        if p.decal {
            rw.draw_decal(i);
        } else {
            rw.draw_mesh(i);
        }
    }
}
