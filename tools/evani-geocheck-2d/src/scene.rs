//! The scene format (`evani-geocheck-2d/1`): what a kit's dump harness
//! writes. All world positions are art pixels, `y` down; rects are
//! `[x, y, w, h]`. Sprite positions inside a frame are device pixels.
//! See README.md for the full description.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// `[x, y, w, h]`, art pixels unless noted.
pub type Rect = [f32; 4];

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub format: String,
    pub name: String,
    /// Where the dump came from (free text).
    pub source: String,
    /// Tile size in art pixels.
    pub tile_size: f32,
    /// Map size in tiles.
    pub map: [u32; 2],
    /// Device pixels per art pixel the game uses (the one pixel scale).
    pub pixel_scale: f32,
    /// Pixel art (default true). A smooth-art game (shapes scaled to any
    /// size, like a cartoon look) sets `false`: check 4 (pixels) does not
    /// apply, and the report says it was not run and why.
    pub pixel_art: bool,
    /// Crop background colour `#rrggbb`.
    pub background: Option<String>,
    pub atlas: Option<AtlasDef>,
    pub layers: Layers,
    /// Names for draw layers, for messages (`"90": "hero"`).
    pub layer_names: BTreeMap<String, String>,
    /// Autotile groups by name.
    pub groups: BTreeMap<String, Group>,
    pub tiles: Vec<TileDef>,
    pub objects: Vec<Object>,
    pub platforms: Vec<PlatformDef>,
    pub player: Option<Player>,
    pub bot: Option<BotTrace>,
    pub frames: Vec<FrameDef>,
    /// Frames drawn while the camera steps one whole art pixel at a time
    /// (with their sprites), so draw joins that break only at some camera
    /// positions are seen (check 8). Kept out of `frames`: they are not
    /// play, so the camera and popup checks never read them.
    pub sweep: Vec<FrameDef>,
    pub popups: Vec<Popup>,
    pub limits: Limits,
    /// Intentional cases (tags), see [`crate::tags`].
    pub geocheck: Vec<crate::tags::Tag>,
    /// Free-text notes for the report (where limits came from, gaps in the dump).
    pub notes: Vec<String>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            format: "evani-geocheck-2d/1".into(),
            name: String::new(),
            source: String::new(),
            tile_size: 16.0,
            map: [0, 0],
            pixel_scale: 1.0,
            pixel_art: true,
            background: None,
            atlas: None,
            layers: Layers::default(),
            layer_names: BTreeMap::new(),
            groups: BTreeMap::new(),
            tiles: Vec::new(),
            objects: Vec::new(),
            platforms: Vec::new(),
            player: None,
            bot: None,
            frames: Vec::new(),
            sweep: Vec::new(),
            popups: Vec::new(),
            limits: Limits::default(),
            geocheck: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// Draw-layer bands: sprites at or above `hud_min` are screen UI.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct Layers {
    pub world_min: i32,
    pub hud_min: i32,
}

impl Default for Layers {
    fn default() -> Self {
        Self { world_min: i32::MIN, hud_min: i32::MAX }
    }
}

/// The sprite sheet: `image` (PNG, or raw RGBA8 when it ends in `.rgba`)
/// relative to the scene file, and frame rects inside it.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AtlasDef {
    pub image: String,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<[u32; 4]>,
    /// Frames that are flat colour fills (stretching them is fine).
    pub fills: Vec<usize>,
}

/// An autotile group (a terrain material).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Group {
    /// Sides (`t`, `l`, `r`, `b`) that must show an edge or cap when the
    /// neighbour there is empty.
    pub caps: String,
    /// Other groups this one is designed to touch without a transition.
    pub meets: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Not collidable (background, water, decoration tiles).
    #[default]
    None,
    /// A full solid block.
    Full,
    /// A one-way top surface.
    Top,
    /// A floor line, `slope` = heights above the tile bottom at its left and right edges.
    Slope,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TileDef {
    pub x: i32,
    pub y: i32,
    /// The level-data character or name.
    pub id: String,
    pub layer: i32,
    pub shape: Shape,
    pub slope: Option<[f32; 2]>,
    pub group: Option<String>,
    /// Sides drawn as an edge/cap (`t`, `l`, `r`, `b`).
    pub edges: String,
    /// A transition tile: may touch any group.
    pub transition: bool,
    pub hazard: bool,
    pub frame: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Collectible,
    Enemy,
    Prop,
    Decor,
    Hazard,
    Platform,
    Goal,
    Player,
    #[default]
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Collectible => "collectible",
            Kind::Enemy => "enemy",
            Kind::Prop => "prop",
            Kind::Decor => "decor",
            Kind::Hazard => "hazard",
            Kind::Platform => "platform",
            Kind::Goal => "goal",
            Kind::Player => "player",
            Kind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    /// Must stand on a floor.
    Ground,
    /// Must be in front of a wall (any tile or decor behind it).
    Wall,
    /// Free (coins, flying things).
    None,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Object {
    pub name: String,
    pub kind: Kind,
    /// Drawn rect.
    pub rect: Rect,
    pub frame: Option<usize>,
    pub flip: bool,
    pub layer: i32,
    /// Default: ground for enemies, decor, props and the goal; none otherwise.
    pub support: Option<Support>,
    /// Foot point (bottom centre of the collision box).
    pub foot: Option<[f32; 2]>,
    pub hitbox: Option<Rect>,
    /// Objects that share an exclusive group must not overlap each other by
    /// more than `overlap_px` (`overlap_object`): pieces settled in a grid,
    /// screen panels laid out side by side.
    pub exclusive: Option<String>,
}

impl Object {
    pub fn support(&self) -> Support {
        self.support.unwrap_or(match self.kind {
            Kind::Enemy | Kind::Decor | Kind::Prop | Kind::Goal => Support::Ground,
            _ => Support::None,
        })
    }
}

/// A moving platform: its top-left follows `path` (art px), `width` wide.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PlatformDef {
    pub name: String,
    pub width: f32,
    pub path: Vec<[f32; 2]>,
}

/// The player's movement, for the reachability and jump-arc models.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Player {
    /// Collision box.
    pub w: f32,
    pub h: f32,
    /// px/sÂ² while rising; `fall_mult` times that while falling.
    pub gravity: f32,
    pub fall_mult: f32,
    pub max_fall: f32,
    pub jump_speed: f32,
    pub run_jump_bonus: f32,
    /// Rising speed kept when jump is released early.
    pub jump_cut: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    /// Start foot point.
    pub start: [f32; 2],
    /// Drawn sprite size.
    pub sprite: Option<[f32; 2]>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            w: 10.0,
            h: 14.0,
            gravity: 980.0,
            fall_mult: 1.0,
            max_fall: 1.0e6,
            jump_speed: 330.0,
            run_jump_bonus: 0.0,
            jump_cut: 1.0,
            walk_speed: 90.0,
            run_speed: 150.0,
            start: [0.0, 0.0],
            sprite: None,
        }
    }
}

/// A bot run: foot point and grounded flag per tick.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct BotTrace {
    pub dt: f32,
    pub completed: bool,
    pub end_tick: u64,
    /// `[x, y, grounded 0/1]` per tick.
    pub ticks: Vec<[f32; 3]>,
    /// Collectibles the bot picked up (object names).
    pub touched: Vec<String>,
}

/// One drawn frame.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct FrameDef {
    pub tick: u64,
    pub t: f32,
    /// Device pixels per art pixel this frame.
    pub scale: f32,
    /// View top-left and size, world art px.
    pub cam: [f32; 2],
    pub view: [f32; 2],
    /// Player's drawn rect, world art px.
    pub player: Rect,
    pub grounded: bool,
    pub sprites: Option<Vec<SpriteDef>>,
}

/// A drawn sprite in device pixels (top-left, size; negative `w` = mirrored).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SpriteDef {
    pub f: Option<usize>,
    pub builtin: Option<u32>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub l: i32,
    /// Linear RGBA tint.
    pub c: [f32; 4],
}

impl Default for SpriteDef {
    fn default() -> Self {
        Self { f: None, builtin: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0, l: 0, c: [1.0; 4] }
    }
}

/// A text popup (score, 1UP) sampled while it lives.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Popup {
    pub id: String,
    pub text: String,
    pub samples: Vec<PopSample>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PopSample {
    pub tick: u64,
    pub t: f32,
    pub rect: Rect,
    pub alpha: f32,
    /// The player's drawn rect at the same moment.
    pub player: Option<Rect>,
}

/// Thresholds. Defaults follow Chris's checks (README.md).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Limits {
    /// Overlap depth allowed between sprites and solids/decor (px).
    pub overlap_px: f32,
    /// Clear air needed around a collectible (px).
    pub air_px: f32,
    /// Gap allowed under a grounded thing (px).
    pub float_px: f32,
    /// How far a grounded thing may sink into its floor (px).
    pub sink_px: f32,
    /// Share of a grounded thing's foot that must stand on something.
    pub support_frac: f32,
    /// Framing zone for the player's feet, as fractions of the view height from the top.
    pub zone_top: f32,
    pub zone_bottom: f32,
    /// Most of the screen terrain below the player may take.
    pub max_terrain: f32,
    /// Shortest run of badly framed grounded frames worth reporting (s).
    pub camera_min_s: f32,
    /// Grace after landing on a new height (s): the feet may sit outside the
    /// framing zone this long while the camera moves towards them (a vertical
    /// dead zone only follows after a landing).
    pub land_grace_s: f32,
    /// Coin arc deviation allowed from a real jump arc (px).
    pub arc_tol_px: f32,
    /// Share of a hazard decor in front may cover.
    pub hazard_cover: f32,
    /// Longest a popup may stay visible (s).
    pub popup_max_s: f32,
    /// Alpha below which a popup counts as gone.
    pub popup_min_alpha: f32,
    /// Rounding allowed on "whole pixel" tests.
    pub pixel_eps: f32,
    /// Widest gap between two sprites of one layer that counts as a broken
    /// join (art px; check 8). Wider gaps are spacing.
    pub join_gap_px: f32,
    /// Shortest gap line worth reporting (art px along the join; check 8).
    pub join_line_px: f32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            overlap_px: 3.0,
            air_px: 2.0,
            float_px: 1.0,
            sink_px: 4.0,
            support_frac: 0.5,
            zone_top: 2.0 / 3.0,
            zone_bottom: 0.97,
            max_terrain: 0.35,
            camera_min_s: 0.3,
            land_grace_s: 1.0,
            arc_tol_px: 6.0,
            hazard_cover: 0.3,
            popup_max_s: 1.0,
            popup_min_alpha: 0.1,
            pixel_eps: 0.01,
            join_gap_px: 2.0,
            join_line_px: 4.0,
        }
    }
}

/// A decoded atlas.
#[derive(Debug, Clone, Default)]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub frames: Vec<[u32; 4]>,
    pub fills: Vec<usize>,
}

impl Atlas {
    /// RGBA of sheet pixel `(x, y)`; transparent outside.
    pub fn px(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let o = ((y * self.width + x) * 4) as usize;
        match self.rgba.get(o..o + 4) {
            Some(p) => [p[0], p[1], p[2], p[3]],
            None => [0; 4],
        }
    }

    /// RGBA of frame `f` at frame pixel `(u, v)`.
    pub fn frame_px(&self, f: usize, u: u32, v: u32) -> [u8; 4] {
        match self.frames.get(f) {
            Some(r) if u < r[2] && v < r[3] => self.px(r[0] + u, r[1] + v),
            _ => [0; 4],
        }
    }

    pub fn frame_size(&self, f: usize) -> Option<[u32; 2]> {
        self.frames.get(f).map(|r| [r[2], r[3]])
    }

    pub fn is_fill(&self, f: usize) -> bool {
        self.fills.contains(&f)
    }
}

/// Parse a scene from JSON text.
pub fn parse(src: &str) -> Result<Scene, String> {
    let s: Scene = serde_json::from_str(src).map_err(|e| format!("scene: {e}"))?;
    if s.tile_size <= 0.0 {
        return Err("scene: tile_size must be positive".into());
    }
    Ok(s)
}

/// Load a scene file and its atlas (if any).
pub fn load(path: &Path) -> Result<(Scene, Option<Atlas>), String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let scene = parse(&src)?;
    let atlas = match &scene.atlas {
        Some(a) if !a.image.is_empty() => {
            let p = path.parent().unwrap_or(Path::new(".")).join(&a.image);
            Some(load_atlas(&p, a)?)
        }
        _ => None,
    };
    Ok((scene, atlas))
}

fn load_atlas(p: &Path, def: &AtlasDef) -> Result<Atlas, String> {
    let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let (width, height, rgba) = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rgba")) {
        if bytes.len() != (def.width * def.height * 4) as usize {
            return Err(format!("{}: {} bytes, expected {} x {} x 4", p.display(), bytes.len(), def.width, def.height));
        }
        (def.width, def.height, bytes)
    } else {
        crate::image::decode_png(&bytes).map_err(|e| format!("{}: {e}", p.display()))?
    };
    Ok(Atlas { width, height, rgba, frames: def.frames.clone(), fills: def.fills.clone() })
}
