//! 2D dynamic lights (feature `lights2d`, catalog effect `lights2d`, 2
//! points, 2D): "normal-mapped sprites lit by point lights accumulated in a
//! light buffer". The light buffer is the sprite pass's uniform: up to
//! [`MAX_LIGHTS`] lights, summed per pixel in `shaders/lights2d.wgsl`, so
//! there is no extra target or pass.
//!
//! * Kits declare the lights of a frame in the [`Lights2d`] resource
//!   ([`Lights2d::push`], CSS pixels like sprites); the renderer reads them
//!   and the render module clears the list after every frame.
//! * A sprite frame `<name>` gets its normal map from the frame
//!   `<name>.n` ([`NORMAL_SUFFIX`]) of the same size, in any sheet
//!   (tangent space, x right and y up, the usual green-up map). Sprites
//!   without one light as flat-facing (normal towards the viewer).
//! * Only scene sprites are lit: not the kit's HUD layers
//!   ([`crate::RenderWorld::hud_layers`]) nor layers from
//!   [`Lights2d::unlit_from`] up, and never a sharp-text glyph.
//! * Lights draw while the `lights2d` effect is on (evani-fx: the tier's
//!   start state, the look and the budget decide) and the active tier gives a
//!   light count ([`tier_cap`]: High and Ultra, the catalog's 2D render
//!   path rows). Off, every sprite draws exactly as without lights.
//!
//! Pure data, so it runs natively; the GPU side (`gpu/lights2d.rs`) only
//! writes what this computes.

use evani_core::effects::EffectsStatus;
use evani_core::params::{F32Param, ParamRegistry};

use crate::shaders::SpriteRaw;
use crate::world::Sprite;

/// The catalog effect id.
pub const EFFECT: &str = "lights2d";
/// A frame named `<name>` + this suffix is the normal map of `<name>`.
pub const NORMAL_SUFFIX: &str = ".n";
/// Lights the sprite uniform holds (its 256-byte binding window).
pub const MAX_LIGHTS: usize = 7;
/// Sprite uniform size in `f32` words with lights.
pub const UNIFORM_WORDS: usize = 8 + MAX_LIGHTS * 8;
/// Default of [`Lights2d::unlit_from`]: the sharp-text HUD's first layer.
pub const UNLIT_FROM: i16 = 16_384;

/// Defaults of the `fx.lights2d.*` params (registered by evani-fx). They
/// match `evani-look-pixelhd`'s `look.pixelhd.light.*` values.
pub const AMBIENT: f32 = 0.55;
pub const STRENGTH: f32 = 1.0;
pub const NORMAL_STRENGTH: f32 = 0.8;
pub const RADIUS_SCALE: f32 = 1.0;
pub const RIM: f32 = 0.25;
pub const HEIGHT: f32 = 0.35;

/// The `fx.lights2d.*` param names.
pub const PARAMS: [&str; 6] = [
    "fx.lights2d.ambient",
    "fx.lights2d.strength",
    "fx.lights2d.normal_strength",
    "fx.lights2d.radius_scale",
    "fx.lights2d.rim",
    "fx.lights2d.height",
];

/// Lights a tier draws at most: the catalog builds the 2D lights on High
/// and Ultra only (`assets/render_paths.json`, `2d-normal-lighting`).
pub fn tier_cap(tier: &str) -> usize {
    match tier {
        "high" => 4,
        "ultra" => MAX_LIGHTS,
        _ => 0,
    }
}

/// One point light, in CSS pixels from the top-left of the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light2d {
    pub pos: [f32; 2],
    /// Linear RGB.
    pub color: [f32; 3],
    /// Reach in CSS pixels (no light past it).
    pub radius: f32,
    /// Brightness at the centre (1 = the colour as given).
    pub intensity: f32,
}

impl Light2d {
    pub fn new(pos: [f32; 2], color: [f32; 3], radius: f32, intensity: f32) -> Self {
        Self { pos, color, radius, intensity }
    }

    /// How much this light matters when the tier keeps only a few.
    fn weight(&self) -> f32 {
        self.intensity.max(0.0) * self.radius.max(0.0)
    }
}

/// This frame's 2D lights (world resource of the render module).
#[derive(Debug, Clone)]
pub struct Lights2d {
    lights: Vec<Light2d>,
    /// Ambient tint (linear RGB); `fx.lights2d.ambient` scales it.
    pub ambient_color: [f32; 3],
    /// Sprites on this layer and above are not lit (HUD).
    pub unlit_from: i16,
}

impl Default for Lights2d {
    fn default() -> Self {
        Self { lights: Vec::new(), ambient_color: [1.0; 3], unlit_from: UNLIT_FROM }
    }
}

impl Lights2d {
    /// Add a light for this frame. Past the tier's count the strongest
    /// lights on screen are kept.
    pub fn push(&mut self, light: Light2d) {
        self.lights.push(light);
    }

    pub fn lights(&self) -> &[Light2d] {
        &self.lights
    }

    /// Drop this frame's lights (keeps capacity).
    pub fn clear(&mut self) {
        self.lights.clear();
    }
}

/// Handles of the `fx.lights2d.*` params, looked up once.
#[derive(Debug, Clone, Copy, Default)]
pub struct LightParams([Option<F32Param>; 6]);

impl LightParams {
    pub fn lookup(reg: &ParamRegistry) -> Self {
        Self(PARAMS.map(|n| reg.f32_handle(n)))
    }

    fn get(&self, reg: &ParamRegistry, i: usize, d: f32) -> f32 {
        self.0.get(i).copied().flatten().map_or(d, |h| reg.f32(h))
    }
}

/// How many lights draw this frame: 0 unless the effect is on.
pub fn active_cap(fx: Option<&EffectsStatus>, tier: &str) -> usize {
    if fx.is_some_and(|f| f.is_on(EFFECT)) { tier_cap(tier) } else { 0 }
}

/// The sprite uniform with lights: viewport and light count, the ambient
/// light, then up to `cap` lights (the strongest that reach the canvas).
pub fn uniform(
    reg: &ParamRegistry,
    p: &LightParams,
    lights: &Lights2d,
    cap: usize,
    viewport: [f32; 2],
) -> [f32; UNIFORM_WORDS] {
    let mut u = [0.0f32; UNIFORM_WORDS];
    let ambient = p.get(reg, 0, AMBIENT).max(0.0);
    let strength = p.get(reg, 1, STRENGTH).max(0.0);
    let radius_scale = p.get(reg, 3, RADIUS_SCALE).max(0.0);
    let height = p.get(reg, 5, HEIGHT).max(0.01);
    u[0] = viewport[0];
    u[1] = viewport[1];
    u[3] = p.get(reg, 2, NORMAL_STRENGTH).max(0.0);
    for (o, c) in u[4..7].iter_mut().zip(lights.ambient_color) {
        *o = c * ambient;
    }
    u[7] = p.get(reg, 4, RIM).max(0.0);
    // Keep the strongest `cap` lights that touch the canvas (insertion into
    // a fixed array: no allocation).
    let cap = cap.min(MAX_LIGHTS);
    let mut keep: [Option<Light2d>; MAX_LIGHTS] = [None; MAX_LIGHTS];
    for l in lights.lights() {
        let r = l.radius * radius_scale;
        let on = r > 0.0
            && l.intensity > 0.0
            && l.pos[0] + r > 0.0
            && l.pos[1] + r > 0.0
            && l.pos[0] - r < viewport[0]
            && l.pos[1] - r < viewport[1];
        if !on {
            continue;
        }
        let mut cur = Some(*l);
        for slot in keep.iter_mut().take(cap) {
            match (*slot, cur) {
                (None, Some(c)) => {
                    *slot = Some(c);
                    cur = None;
                }
                (Some(s), Some(c)) if c.weight() > s.weight() => {
                    *slot = Some(c);
                    cur = Some(s);
                }
                _ => {}
            }
        }
    }
    let mut n = 0;
    for (l, w) in keep.iter().flatten().zip(u[8..].as_chunks_mut::<8>().0) {
        let r = l.radius * radius_scale;
        let k = l.intensity * strength;
        w.copy_from_slice(&[l.pos[0], l.pos[1], r, r * height, l.color[0] * k, l.color[1] * k, l.color[2] * k, 0.0]);
        n += 1;
    }
    u[2] = n as f32;
    u
}

/// Normal-map cell of every named frame (index `cell - ART_CELL0`; 0 for
/// none): frame `<name>.n` of the same size as `<name>`.
pub fn normal_table(art: &crate::art::SpriteArt) -> Vec<u16> {
    let rects: Vec<[u32; 4]> = art.frames().map(|f| f.rect).collect();
    let mut table = vec![0u16; rects.len()];
    for (i, f) in art.frames().enumerate() {
        let Some(base) = f.name.strip_suffix(NORMAL_SUFFIX) else { continue };
        let Some(cell) = art.cell(base) else { continue };
        let b = usize::from(cell.wrapping_sub(crate::art::ART_CELL0));
        let same = rects.get(b).is_some_and(|r| r[2] == f.rect[2] && r[3] == f.rect[3]);
        if let (true, Some(slot), Ok(i)) = (same, table.get_mut(b), u16::try_from(i)) {
            *slot = crate::art::ART_CELL0.wrapping_add(i);
        }
    }
    table
}

/// Mark the packed sprites the lights reach: the spare words of
/// [`SpriteRaw::pad`] get `[-1, 0, 0]` (flat) or `[-2, du, dv]` (the uv
/// offset to the normal map). HUD layers, layers from `unlit_from` and
/// anything already using the words (a glyph) stay unlit.
pub fn mark(
    sprites: &[Sprite],
    raw: &mut [SpriteRaw],
    normals: &[u16],
    uvs: &[[f32; 4]],
    art_base: usize,
    hud: Option<[i16; 2]>,
    unlit_from: i16,
) {
    for (s, r) in sprites.iter().zip(raw.iter_mut()) {
        let in_hud = hud.is_some_and(|[lo, hi]| s.layer >= lo && s.layer <= hi);
        if in_hud || s.layer >= unlit_from || r.pad != [0.0; 3] {
            continue;
        }
        let normal = s
            .cell
            .checked_sub(crate::art::ART_CELL0)
            .and_then(|i| normals.get(usize::from(i)))
            .copied()
            .filter(|&c| c != 0);
        r.pad = match normal {
            Some(nc) => {
                let n = crate::art::cell_uv(nc, uvs, art_base);
                [-2.0, n[0] - r.uv[0], n[1] - r.uv[1]]
            }
            None => [-1.0, 0.0, 0.0],
        };
    }
}
