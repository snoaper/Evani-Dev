//! Drawing (K5b art pass): every object is a part recipe from `data/art.json`
//! drawn through [`crate::art`], which asks the active look for materials by
//! role (the kit never names a look) and keeps the target pack's palette as
//! the colour the player sees. The terrain is per-row runs of slabs coloured
//! by the player's fog, with props (tufts, outcrops, pines, ford stones) on
//! seen cells and a ring of basalt ridges and pines outside the map, so no
//! camera sees the world edge. Decals carry selection rings, markers and the
//! build footprint; point lights mark crystal, hubs, foundries and shots. The
//! HUD is bitmap text and SQUARE sprites on layers 100 to 120 (with a
//! minimap), coloured from the same palette.
//!
//! The player sees unexplored cells as dark fog, explored ones dimmed with
//! last-known enemy buildings, visible ones in full with enemy units.

use std::f32::consts::{FRAC_PI_4, SQRT_2, TAU};

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::tiers::Tiers;
use evani_core::touch;
use evani_render::atlas::cell::SQUARE;
use evani_render::light::Light;
use evani_render::math::quat_axis_angle;
use evani_render::text::{self, Align, TextStyle};
use evani_render::{MeshId, MeshInstance, RenderWorld, Sprite};

use crate::art::{self, hash01, mix, role, scale, Art, Colours, LightRole, Mats, Part, Place};
use crate::control::{mode_line, place_corner, pose, rect_center, to_ground, to_screen, Mode, Phase, Ui};
use crate::data::{bs, us, Terrain, Types, BASTION, RUMBLER, TROOPER, WORKER};
use crate::feather::{strips, EdgeLook, FogField, Strip};
use crate::grid::{Fog, Grid, EXPLORED, UNEXPLORED, VISIBLE};
use crate::sim::{Match, Order, PLAYER};
use crate::{Edges, Tune};

/// Base height of the backdrop ridges and pines outside the map.
const FAR_Y: f32 = -0.3;
/// Height of the ground outside the map: just under the grass, ford and
/// fog tops, so the map's edge slabs show no side (their south sides face
/// away from the sun and drew a dark line along the edge).
const FAR_GROUND_Y: f32 = -0.03;
/// Terrain slab thickness (tops sit at the art's `*_top` heights).
const SLAB: f32 = 1.0;
/// Rubble heaps remembered at once.
const MAX_RUBBLE: usize = 24;

fn terrain_col(c: &Colours, t: Terrain) -> [f32; 4] {
    match t {
        Terrain::Grass => c.grass,
        Terrain::Rock => c.rock,
        Terrain::Water => c.water,
        Terrain::Ford => c.ford,
    }
}

/// A colour as the player's fog shows it.
fn fogged(art: &Art, col: [f32; 4], fog: u8) -> [f32; 4] {
    match fog {
        UNEXPLORED => art.colours.fog,
        EXPLORED => mix(scale(col, art.fog.explored_dim), art.colours.fog, art.fog.explored_to_fog),
        _ => col,
    }
}

/// The colour of a cell as the player sees it (terrain, minimap).
fn cell_color(art: &Art, t: Terrain, fog: u8) -> [f32; 4] {
    fogged(art, terrain_col(&art.colours, t), fog)
}

fn terrain_top(art: &Art, t: Terrain) -> f32 {
    match t {
        Terrain::Grass => art.terrain.grass_top,
        Terrain::Rock => art.terrain.rock_top,
        Terrain::Water => art.terrain.water_top,
        Terrain::Ford => art.terrain.ford_top,
    }
}

/// One horizontal run of same-looking cells.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run {
    y: i32,
    x0: i32,
    x1: i32,
    terrain: Terrain,
    fog: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropKind {
    Tuft,
    Patch,
    Pine,
    Outcrop,
    FordStone,
    Ridge,
}

/// One placed scenery prop (draw-only scatter, never gameplay).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Prop {
    kind: PropKind,
    at: [f32; 3],
    scale: [f32; 3],
    yaw: f32,
    fog: u8,
    /// Extra blend towards the fog colour near the fog line (0..1).
    fade: f32,
}

/// One piece of ground beyond the map edge: `[x0, z0, x1, z1]`, its
/// height, and its fog look `(dim, fade)`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Far {
    rect: [f32; 4],
    y: f32,
    look: (f32, f32),
}

/// A heap left where a building fell.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rubble {
    at: [f32; 2],
    side: f32,
}

/// Terrain runs and props (rebuilt only when the fog or the grid changes),
/// the backdrop (built once), and draw-only memory: turret aims, fallen
/// buildings, whether the art's sun has been applied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawCache {
    key: Option<(u32, u32, Edges)>,
    runs: Vec<Run>,
    props: Vec<Prop>,
    backdrop: Vec<Prop>,
    /// The near-side scale the backdrop was built with.
    backdrop_near: Option<f32>,
    /// Each backdrop prop's fog look `(dim, fade)`.
    backdrop_look: Vec<(f32, f32)>,
    /// The fog line's gradient strips.
    strips: Vec<Strip>,
    /// Last turret turn per bastion id.
    aims: Vec<(u32, f32)>,
    /// Buildings standing last frame: id, centre, side.
    standing: Vec<(u32, [f32; 2], f32)>,
    rubble: Vec<Rubble>,
    /// Ground beyond the map edge, shaded like the edge it borders.
    far: Vec<Far>,
    /// Corners where a seen cell meets three unexplored ones: the fog's
    /// staircase is cut there on the diagonal.
    soft: Vec<[f32; 2]>,
    last_tick: u64,
    sun_done: bool,
}

impl DrawCache {
    fn update(&mut self, m: &Match, art: &Art, e: Edges) {
        if self.backdrop_near != Some(e.near_scale) {
            self.backdrop = backdrop(&m.grid, art, e.near_scale);
            self.backdrop_near = Some(e.near_scale);
            self.key = None;
        }
        self.remember_fallen(m);
        self.remember_aims(m);
        let fog = &m.fog[PLAYER as usize];
        let key = (fog.rev, m.grid.rev, e);
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.runs.clear();
        self.props.clear();
        let g = &m.grid;
        let field = FogField::build(g, fog, e.feather_cells);
        let look = EdgeLook { explored_dim: art.fog.explored_dim, explored_to_fog: art.fog.explored_to_fog, unexplored_to_fog: art.fog.unexplored_to_fog };
        self.strips = if e.feather_cells > 0 { strips(g, fog, &field, e.feather_strips) } else { Vec::new() };
        self.backdrop_look = self.backdrop.iter().map(|p| field.edge_look([p.at[0], p.at[2]], p.scale[0] * 0.5, &look)).collect();
        for y in 0..g.h {
            let mut x = 0;
            while x < g.w {
                let t = g.terrain_at([x, y]);
                let f = fog.at(g, [x, y]);
                let mut x1 = x;
                while x1 + 1 < g.w && g.terrain_at([x1 + 1, y]) == t && fog.at(g, [x1 + 1, y]) == f {
                    x1 += 1;
                }
                self.runs.push(Run { y, x0: x, x1, terrain: t, fog: f });
                x = x1 + 1;
            }
            for x in 0..g.w {
                let f = fog.at(g, [x, y]);
                if f != UNEXPLORED {
                    let from = self.props.len();
                    cell_props(&mut self.props, g, art, [x, y], f);
                    if e.feather_cells > 0 {
                        for p in &mut self.props[from..] {
                            p.fade = field.fogginess([p.at[0], p.at[2]]);
                        }
                    }
                }
            }
        }
        let fog_look = EdgeLook { unexplored_to_fog: 1.0, ..look };
        self.far = far_strips(g, &field, art.backdrop.depth_m, &fog_look, e.feather_strips);
        self.soft = if e.feather_cells > 0 { Vec::new() } else { soft_corners(g, fog) };
    }

    /// A building that was standing and is gone leaves rubble (a new match
    /// clears the memory).
    fn remember_fallen(&mut self, m: &Match) {
        if m.tick < self.last_tick {
            self.standing.clear();
            self.rubble.clear();
            self.aims.clear();
        }
        self.last_tick = m.tick;
        for (id, at, side) in &self.standing {
            if !m.buildings.iter().any(|b| b.id == *id) {
                if self.rubble.len() >= MAX_RUBBLE {
                    self.rubble.remove(0);
                }
                self.rubble.push(Rubble { at: *at, side: *side });
            }
        }
        self.standing.clear();
        for b in &m.buildings {
            let r = m.building_rect(b);
            self.standing.push((b.id, rect_center(r), r[2] - r[0]));
        }
    }

    /// Bastions turn towards their latest shot.
    fn remember_aims(&mut self, m: &Match) {
        self.aims.retain(|(id, _)| m.buildings.iter().any(|b| b.id == *id));
        for b in m.buildings.iter().filter(|b| b.kind == BASTION && b.built) {
            let r = m.building_rect(b);
            let shot = m.shots.iter().find(|s| s.from[0] >= r[0] && s.from[0] <= r[2] && s.from[1] >= r[1] && s.from[1] <= r[3]);
            if let Some(s) = shot {
                let yaw = (s.to[0] - s.from[0]).atan2(s.to[1] - s.from[1]);
                match self.aims.iter_mut().find(|(id, _)| *id == b.id) {
                    Some(a) => a.1 = yaw,
                    None => self.aims.push((b.id, yaw)),
                }
            }
        }
    }

    fn aim(&self, id: u32) -> f32 {
        self.aims.iter().find(|(i, _)| *i == id).map_or(0.0, |a| a.1)
    }
}

/// The props on one seen cell.
fn cell_props(out: &mut Vec<Prop>, g: &Grid, art: &Art, c: [i32; 2], fog: u8) {
    let ta = &art.terrain;
    let ctr = g.center(c);
    let (x, y) = (c[0], c[1]);
    let jitter = |salt: u32, k: f32| [ctr[0] + (hash01(x, y, salt) - 0.5) * g.cell * k, ctr[1] + (hash01(x, y, salt + 1) - 0.5) * g.cell * k];
    let yaw = hash01(x, y, 7) * TAU;
    match g.terrain_at(c) {
        Terrain::Grass => {
            if hash01(x, y, 13) < ta.patch_density {
                let p = jitter(14, 0.5);
                let s = g.cell * (0.9 + hash01(x, y, 16) * 0.8);
                out.push(Prop { kind: PropKind::Patch, at: [p[0], ta.grass_top, p[1]], scale: [s, 1.0, s * (0.7 + hash01(x, y, 17) * 0.5)], yaw, fog, fade: 0.0 });
            }
            if hash01(x, y, 1) < ta.tuft_density {
                let p = jitter(2, 0.7);
                let s = 1.2 + hash01(x, y, 4) * 0.6;
                out.push(Prop { kind: PropKind::Tuft, at: [p[0], ta.grass_top, p[1]], scale: [s; 3], yaw, fog, fade: 0.0 });
            }
        }
        Terrain::Rock => {
            let edge = x == 0 || y == 0 || x == g.w - 1 || y == g.h - 1;
            let pine_odds = if edge { ta.border_pine_density } else { ta.outcrop_pine_density };
            let s = g.cell * (0.8 + hash01(x, y, 5) * 0.35);
            let p = jitter(8, 0.3);
            out.push(Prop { kind: PropKind::Outcrop, at: [p[0], ta.rock_top, p[1]], scale: [s, s * (0.7 + hash01(x, y, 6) * 0.6), s], yaw, fog, fade: 0.0 });
            if hash01(x, y, 3) < pine_odds {
                let p = jitter(10, 0.6);
                let k = 0.85 + hash01(x, y, 12) * 0.45;
                out.push(Prop { kind: PropKind::Pine, at: [p[0], ta.rock_top, p[1]], scale: [k; 3], yaw, fog, fade: 0.0 });
            }
        }
        Terrain::Ford => {
            for i in 0..ta.ford_stones {
                let salt = 20 + i * 3;
                if hash01(x, y, salt) < 0.35 {
                    let p = jitter(salt + 1, 0.8);
                    let k = 0.8 + hash01(x, y, salt + 2) * 0.5;
                    out.push(Prop { kind: PropKind::FordStone, at: [p[0], ta.ford_top, p[1]], scale: [k; 3], yaw: yaw + i as f32, fog, fade: 0.0 });
                }
            }
        }
        Terrain::Water => {}
    }
}

/// The ground outside the map as strips along each side plus the four
/// corner squares, reaching well past the backdrop. Each side is cut into
/// `n` pieces a cell, shaded like the smooth fog on the edge beside it, and
/// equal neighbours merge (one strip where the edge is all fog or all seen).
fn far_strips(g: &Grid, field: &FogField, depth_m: f32, look: &EdgeLook, n: i32) -> Vec<Far> {
    let h = g.half();
    let ext = (h[0].max(h[1]) + depth_m.max(0.0)) * 2.0;
    let s = g.cell / n.clamp(1, 8) as f32;
    let q = |l: (f32, f32)| ((l.0 * 64.0).round() / 64.0, (l.1 * 64.0).round() / 64.0);
    let mut out = Vec::new();
    // `at(a)`: the point just beyond the edge `a` metres along it; `rect(a,
    // b)` the strip beside the edge from `a` to `b` metres along it.
    // `over`: metres the end pieces reach past the side's ends (the east
    // and west sides cover the corners, so no seam runs across the view
    // along the north and south edges).
    let mut side = |len: f32, over: f32, at: &dyn Fn(f32) -> [f32; 2], rect: &dyn Fn(f32, f32) -> [f32; 4]| {
        let k = (len / s).round() as i32;
        let piece = |i: i32| q(field.edge_look(at((i as f32 + 0.5) * s), 0.0, look));
        let mut i = 0;
        while i < k {
            let l = piece(i);
            let mut j = i;
            while j + 1 < k && piece(j + 1) == l {
                j += 1;
            }
            let a = if i == 0 { -over } else { i as f32 * s };
            let b = if j + 1 == k { len + over } else { (j + 1) as f32 * s };
            out.push(Far { rect: rect(a, b), y: FAR_GROUND_Y, look: l });
            i = j + 1;
        }
    };
    let e = 0.01;
    side(h[0] * 2.0, 0.0, &|a| [a - h[0], -h[1] - e], &|a, b| [a - h[0], -h[1] - ext, b - h[0], -h[1]]);
    side(h[0] * 2.0, 0.0, &|a| [a - h[0], h[1] + e], &|a, b| [a - h[0], h[1], b - h[0], h[1] + ext]);
    side(h[1] * 2.0, ext, &|a| [-h[0] - e, a - h[1]], &|a, b| [-h[0] - ext, a - h[1], -h[0], b - h[1]]);
    side(h[1] * 2.0, ext, &|a| [h[0] + e, a - h[1]], &|a, b| [h[0], a - h[1], h[0] + ext, b - h[1]]);
    out
}

/// Corners `[x, z]` where a seen, non-rock cell has its two neighbours and
/// the diagonal cell round that corner all inside the map and unexplored.
fn soft_corners(g: &Grid, fog: &Fog) -> Vec<[f32; 2]> {
    let h = g.half();
    let unknown = |c: [i32; 2]| g.idx(c).is_some() && fog.at(g, c) == UNEXPLORED;
    let mut out = Vec::new();
    for y in 0..g.h {
        for x in 0..g.w {
            if fog.at(g, [x, y]) == UNEXPLORED || g.terrain_at([x, y]) == Terrain::Rock {
                continue;
            }
            for (dx, dy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                if unknown([x + dx, y]) && unknown([x, y + dy]) && unknown([x + dx, y + dy]) {
                    let px = (x + i32::from(dx > 0)) as f32 * g.cell - h[0];
                    let pz = (y + i32::from(dy > 0)) as f32 * g.cell - h[1];
                    out.push([px, pz]);
                }
            }
        }
    }
    out
}

/// Basalt ridges and pines in a ring outside the map (always drawn, under
/// the fog of the edge they stand beyond). The ridges on the camera's
/// (south) side stand between the camera and the map, so they are lowered
/// by `near` (and narrowed a little) to keep them from filling the view.
fn backdrop(g: &Grid, art: &Art, near: f32) -> Vec<Prop> {
    let b = &art.backdrop;
    let half = g.half();
    let step = b.step_m.max(1.0);
    let mut out = Vec::new();
    for side in 0..4i32 {
        let along = if side % 2 == 0 { half[0] } else { half[1] };
        let across = if side % 2 == 0 { half[1] } else { half[0] };
        let n = ((along + b.depth_m) * 2.0 / step).ceil() as i32;
        for i in 0..n {
            let t = -along - b.depth_m + (i as f32 + 0.5) * step;
            // A point `d` metres beyond this side's edge, `t` along it.
            let place = |d: f32, t: f32| -> [f32; 2] {
                match side {
                    0 => [t, -across - d],
                    1 => [across + d, t],
                    2 => [t, across + d],
                    _ => [-across - d, t],
                }
            };
            let h = b.ridge_h[0] + (b.ridge_h[1] - b.ridge_h[0]) * hash01(i, side, 31);
            let d = b.depth_m * (0.3 + 0.4 * hash01(i, side, 32));
            let p = place(d, t);
            let (h, w) = if side == 2 { (h * near, step * (1.4 + 0.5 * hash01(i, side, 33)) * (0.5 + 0.5 * near)) } else { (h, step * (1.4 + 0.5 * hash01(i, side, 33))) };
            out.push(Prop { kind: PropKind::Ridge, at: [p[0], FAR_Y, p[1]], scale: [w, h, w], yaw: hash01(i, side, 34) * TAU, fog: VISIBLE, fade: 0.0 });
            for k in 0..b.pines_per_step {
                let salt = 40 + k * 4;
                let d = b.depth_m * (0.05 + 0.9 * hash01(i, side, salt));
                let p = place(d, t + (hash01(i, side, salt + 1) - 0.5) * step);
                let s = 1.0 + hash01(i, side, salt + 2) * 0.6;
                out.push(Prop { kind: PropKind::Pine, at: [p[0], FAR_Y, p[1]], scale: [s; 3], yaw: hash01(i, side, salt + 3) * TAU, fog: VISIBLE, fade: 0.0 });
            }
        }
    }
    out
}

/// Whether this frame draws a shadow map; without one, units get blobs.
fn shadow_map_on(world: &World) -> bool {
    world.try_res::<Tiers>().is_some_and(|t| t.active().shadow_map.is_some())
        && world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on("shadows"))
}

fn sprite(pos: [f32; 2], size: [f32; 2], color: [f32; 4], layer: i16) -> Sprite {
    Sprite { pos, size, rotation: 0.0, cell: SQUARE, color, layer }
}

/// A plain mesh through a material role.
fn mesh(mats: &Mats, r: usize, id: MeshId, pos: [f32; 3], s: [f32; 3], color: [f32; 4]) -> MeshInstance {
    let mut i = MeshInstance::new(id, pos);
    i.scale = s;
    i.material = mats.id(r);
    i.color = mats.tint(r, color);
    i
}

fn light(rw: &mut RenderWorld, l: &LightRole, at: [f32; 2], k: f32) {
    rw.draw_light(Light::new([at[0], l.y, at[1]], l.color, l.range, l.intensity * k));
}

/// Apply the art's key light direction and its `render` block (each key
/// sets `render.<key>`, e.g. exposure) once; the look keeps everything else,
/// and live tuning can still move them afterwards.
fn apply_sun(world: &World, art: &Art) {
    let Some(mut reg) = world.try_res_mut::<ParamRegistry>() else { return };
    let d = art.sun_dir;
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len > 0.0 && reg.id("render.light.direction").is_some() {
        let v = serde_json::json!([d[0] / len, d[1] / len, d[2] / len]);
        let _ = reg.set_json("render.light.direction", &v);
    }
    for (key, v) in &art.render {
        let name = format!("render.{key}");
        if reg.id(&name).is_some() {
            let _ = reg.set_json(&name, v);
        }
    }
}

pub fn draw_system(world: &mut World) {
    let Some((kit, t)) = crate::tune(world) else { return };
    let Some(edges) = world.try_res::<ParamRegistry>().map(|reg| kit.edges.read(&reg)) else { return };
    let blobs = !shadow_map_on(world);
    let touching = touch::active(world);
    let Some(art) = world.try_res::<Art>() else { return };
    let mats = Mats::resolve(&art, &world.res::<LookData>());
    let (Some(ui), Some(m)) = (world.try_res::<Ui>(), world.try_res::<Match>()) else { return };
    let mut cache = world.res_mut::<DrawCache>();
    if !cache.sun_done {
        cache.sun_done = true;
        apply_sun(world, &art);
    }
    // The HUD is drawn before the tonemap, so it is lifted by 1 / exposure
    // to look the same whatever exposure the art (or live tuning) picks.
    let hud_lift = world
        .try_res::<ParamRegistry>()
        .and_then(|reg| reg.f32_handle("render.exposure").map(|h| reg.f32(h)))
        .map_or(1.0, |e| 1.0 / e.max(0.05));
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    rw.hud_layers = Some([100, 120]);
    let vp = rw.viewport_css;
    let aspect = if vp[1] > 0.0 { vp[0] / vp[1] } else { 16.0 / 9.0 };
    let cam = pose(ui.cam, ui.zoom, &t, aspect);
    rw.camera = cam;

    cache.update(&m, &art, edges);
    world_draw(&mut rw, &m, &ui, &cache, &art, &mats, blobs);
    if vp[0] > 0.0 && vp[1] > 0.0 {
        let from = rw.sprites.len();
        hud(&mut rw, &m, &ui, &t, &cam, vp, touching, &art);
        if (hud_lift - 1.0).abs() > 1e-3 {
            for s in rw.sprites[from..].iter_mut().filter(|s| (100..=120).contains(&s.layer)) {
                for c in &mut s.color[..3] {
                    *c *= hud_lift;
                }
            }
        }
    }
}

/// How a prop or remembered object shows under the player's fog.
fn fog_place(art: &Art, mut p: Place, fog: u8) -> Place {
    if fog == EXPLORED {
        p.dim = art.fog.explored_dim;
        p.fade = Some((art.colours.fog, art.fog.explored_to_fog));
    }
    p
}

fn prop_recipe(art: &Art, k: PropKind) -> &[Part] {
    let r = &art.recipes;
    match k {
        PropKind::Tuft => &r.tuft,
        PropKind::Patch => &r.patch,
        PropKind::Pine => &r.pine,
        PropKind::Outcrop => &r.outcrop,
        PropKind::FordStone => &r.ford_stone,
        PropKind::Ridge => &r.ridge,
    }
}

fn world_draw(rw: &mut RenderWorld, m: &Match, ui: &Ui, cache: &DrawCache, art: &Art, mats: &Mats, blobs: bool) {
    let col = &art.colours;
    let g = &m.grid;
    let c = g.cell;
    let half = g.half();
    let fog = &m.fog[PLAYER as usize];

    // Ground beyond the map, under the backdrop ridges, fogged like the
    // edge it borders (so unexplored ground never ends in a lit plain).
    for f in &cache.far {
        let r = f.rect;
        let at = [(r[0] + r[2]) * 0.5, f.y, (r[1] + r[3]) * 0.5];
        let colour = mix(scale(col.far_ground, f.look.0), col.fog, f.look.1);
        rw.draw_mesh(mesh(mats, role::GROUND, MeshId::PLANE, at, [r[2] - r[0], 1.0, r[3] - r[1]], colour));
    }
    // Terrain slabs: each run's top at its terrain's height (fog lies flat).
    for r in &cache.runs {
        let x0 = r.x0 as f32 * c - half[0];
        let w = (r.x1 - r.x0 + 1) as f32 * c;
        let z = (r.y as f32 + 0.5) * c - half[1];
        let (top, rl) = match (r.fog, r.terrain) {
            (UNEXPLORED, _) => (art.terrain.fog_top, role::GROUND),
            (_, Terrain::Rock) => (art.terrain.rock_top, role::ROCK),
            (_, Terrain::Water) => (art.terrain.water_top, role::WATER),
            (_, t) => (terrain_top(art, t), role::GROUND),
        };
        let colour = cell_color(art, r.terrain, r.fog);
        rw.draw_mesh(mesh(mats, rl, MeshId::CUBE, [x0 + w * 0.5, top - SLAB * 0.5, z], [w, SLAB, c], colour));
    }
    // The fog's gradients: thin strips lying on each cell's top, from the
    // ground in sight to the ground remembered to the fog (an unexplored
    // cell never shows its own terrain: its strips start from grass).
    for s in &cache.strips {
        let f0 = fog.at(g, s.cell);
        let t = g.terrain_at(s.cell);
        let (top, rl, t) = match (f0, t) {
            (UNEXPLORED, _) => (art.terrain.fog_top, role::GROUND, Terrain::Grass),
            (_, Terrain::Rock) => (art.terrain.rock_top, role::ROCK, t),
            (_, Terrain::Water) => (art.terrain.water_top, role::WATER, t),
            _ => (terrain_top(art, t), role::GROUND, t),
        };
        let base = mix(cell_color(art, t, EXPLORED), cell_color(art, t, VISIBLE), s.lit);
        rw.draw_decal(mesh(mats, rl, MeshId::PLANE, [s.x0 + s.len * 0.5, top, s.z], [s.len, 1.0, s.depth], mix(base, col.fog, s.f)));
    }
    // The fog's staircase cut on the diagonal at outer corners (only with
    // the gradient off; the gradient already rounds them): a fog
    // diamond just under the fog's top (hidden under the fog slabs, showing
    // over the seen cell's corner).
    let soft_top = 0.5 * (art.terrain.fog_top + art.terrain.grass_top.max(art.terrain.ford_top));
    for p in &cache.soft {
        let mut d = mesh(mats, role::GROUND, MeshId::CUBE, [p[0], soft_top - 0.05, p[1]], [c * SQRT_2, 0.1, c * SQRT_2], col.fog);
        d.rotation = quat_axis_angle([0.0, 1.0, 0.0], FRAC_PI_4);
        rw.draw_mesh(d);
    }
    // Scenery: map props show under their own cell's fog; the backdrop
    // under the fog of the edge it stands beyond (a dark silhouette while
    // that edge is unexplored).
    for p in &cache.props {
        let mut pl = Place::new(p.at, 1.0);
        pl.scale = p.scale;
        pl.yaw = p.yaw;
        pl.decal = p.kind == PropKind::Patch;
        let mut pl = fog_place(art, pl, p.fog);
        if p.fade > 0.0 {
            let t0 = pl.fade.map_or(0.0, |f| f.1);
            pl.fade = Some((col.fog, 1.0 - (1.0 - t0) * (1.0 - p.fade)));
        }
        art::draw(rw, art, mats, prop_recipe(art, p.kind), &pl);
    }
    for (i, p) in cache.backdrop.iter().enumerate() {
        let mut pl = Place::new(p.at, 1.0);
        pl.scale = p.scale;
        pl.yaw = p.yaw;
        let (dim, fade) = cache.backdrop_look.get(i).copied().unwrap_or((1.0, art.fog.unexplored_to_fog));
        pl.dim = dim;
        pl.fade = Some((col.fog, fade));
        art::draw(rw, art, mats, prop_recipe(art, p.kind), &pl);
    }
    // Rubble where buildings fell (once the player has seen the ground).
    for rb in cache.rubble.iter().filter(|rb| fog.at(g, g.cell_of(rb.at)) != UNEXPLORED) {
        let f = fog.at(g, g.cell_of(rb.at));
        rw.draw_decal(mesh(mats, role::GROUND, MeshId::CYLINDER, [rb.at[0], 0.01, rb.at[1]], [rb.side * 0.95, 0.02, rb.side * 0.95], fogged(art, col.scorch, f)));
        let mut pl = Place::new([rb.at[0], 0.0, rb.at[1]], rb.side);
        pl.yaw = hash01(rb.at[0] as i32, rb.at[1] as i32, 50) * TAU;
        art::draw(rw, art, mats, &art.recipes.rubble, &fog_place(art, pl, f));
    }
    // Crystal nodes the player has explored; they shrink as they are mined.
    for n in m.nodes.iter().filter(|n| n.amount > 0.0 && fog.at(g, n.cell) != UNEXPLORED) {
        let k = (n.amount / n.start.max(1.0)).clamp(0.0, 1.0);
        let f = fog.at(g, n.cell);
        let mut pl = Place::new([n.pos[0], 0.0, n.pos[1]], 1.0);
        pl.scale = [1.0, 0.45 + 0.55 * k, 1.0];
        pl.yaw = hash01(n.cell[0], n.cell[1], 60) * TAU;
        art::draw(rw, art, mats, &art.recipes.crystal, &fog_place(art, pl, f));
        if f == VISIBLE {
            light(rw, &art.light_crystal, n.pos, 0.5 + 0.5 * k);
        }
    }
    // Buildings: own always, the rival's once explored (last known).
    for b in &m.buildings {
        let f = fog.at(g, b.corner);
        if b.team != PLAYER && f < EXPLORED {
            continue;
        }
        let r = m.building_rect(b);
        let ctr = rect_center(r);
        let side = r[2] - r[0];
        let mut pl = Place::new([ctr[0], 0.0, ctr[1]], side);
        pl.team = b.team as usize;
        pl.built = b.built;
        pl.grow = if b.built { 1.0 } else { 0.15 + 0.85 * b.progress };
        pl.aim = cache.aim(b.id);
        if b.team != PLAYER && !fog.visible(g, ctr) {
            pl.dim = art.fog.rival_unseen_dim;
        }
        art::draw(rw, art, mats, &art.recipes.buildings[b.kind.min(2)], &pl);
        if b.built && (b.team == PLAYER || fog.visible(g, ctr)) {
            match b.kind {
                crate::data::HUB => light(rw, &art.light_beacon, ctr, 1.0),
                crate::data::FOUNDRY => light(rw, &art.light_glow, ctr, 1.0),
                _ => {}
            }
        }
        if ui.selected.contains(&b.id) {
            square(rw, mats, col.team[b.team as usize & 1], ctr, side * 0.5 + art.rings.building_pad, art.rings.width);
            if let (true, Some(p)) = (b.team == PLAYER, b.rally) {
                let mut fl = Place::new([p[0], 0.0, p[1]], 1.0);
                fl.team = b.team as usize;
                art::draw(rw, art, mats, &art.recipes.flag, &fl);
            }
        }
    }
    // Units: own always, the rival's only where visible.
    for u in &m.units {
        if u.team != PLAYER && !fog.visible(g, u.pos) {
            continue;
        }
        let rad = m.types.unit(u.kind, us::RADIUS).max(0.1);
        let mut pl = Place::new([u.pos[0], 0.0, u.pos[1]], rad * 2.0);
        pl.yaw = u.facing;
        pl.aim = u.facing;
        pl.team = u.team as usize;
        pl.carry = u.carry > 0.0;
        art::draw(rw, art, mats, &art.recipes.units[u.kind.min(2)], &pl);
        if u.moving {
            // Path dust behind the unit.
            let back = [u.pos[0] - u.facing.sin() * rad, u.pos[1] - u.facing.cos() * rad];
            rw.draw_mesh(mesh(mats, role::GROUND, MeshId::LOW_SPHERE, [back[0], 0.02, back[1]], [rad * 0.9, 0.12, rad * 0.9], col.dust));
        }
        if ui.selected.contains(&u.id) {
            ring(rw, mats, col.team[u.team as usize & 1], u.pos, rad + art.rings.unit_pad, art.rings.unit_segments, art.rings.width);
        }
        if blobs {
            rw.draw_blob([u.pos[0], 0.0, u.pos[1]], rad * 1.2);
        }
    }
    // Shots the player can see: a bright tracer and a flash where it lands.
    for s in &m.shots {
        if !(fog.visible(g, s.from) || fog.visible(g, s.to)) {
            continue;
        }
        let d = [s.to[0] - s.from[0], s.to[1] - s.from[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(0.1);
        let mid = [(s.from[0] + s.to[0]) * 0.5, 1.0, (s.from[1] + s.to[1]) * 0.5];
        let w = if s.heavy { 0.3 } else { 0.12 };
        let mut i = mesh(mats, role::CRYSTAL, MeshId::CUBE, mid, [w, w, len], scale(col.team[s.team as usize & 1], 1.6));
        i.rotation = quat_axis_angle([0.0, 1.0, 0.0], d[0].atan2(d[1]));
        rw.draw_mesh(i);
        light(rw, &art.light_shot, s.to, if s.heavy { 1.0 } else { 0.5 });
    }
    // Command markers shrink as they fade.
    for mk in &ui.markers {
        let r = 0.3 + 0.8 * mk.t;
        let c = if mk.attack { col.damage } else { col.ui };
        ring(rw, mats, c, mk.pos, r, art.rings.unit_segments, art.rings.width);
    }
    // The build ghost: the footprint and a see-through building.
    if let (Mode::Place(kind), Some(cell)) = (ui.mode, ui.hover) {
        let r = g.rect(place_corner(&m.types, kind, cell), m.types.size(kind));
        let ctr = rect_center(r);
        let c = if ui.hover_ok { col.valid } else { col.damage };
        rw.draw_decal(mesh(mats, role::PAINT, MeshId::CUBE, [ctr[0], 0.01, ctr[1]], [r[2] - r[0], 0.02, r[3] - r[1]], scale(c, 0.8)));
        let mut pl = Place::new([ctr[0], 0.0, ctr[1]], r[2] - r[0]);
        pl.alpha = 0.45;
        art::draw(rw, art, mats, &art.recipes.buildings[kind.min(2)], &pl);
    }
}

/// A ring of flat segments on the ground (selection, markers).
fn ring(rw: &mut RenderWorld, mats: &Mats, c: [f32; 4], at: [f32; 2], r: f32, segments: u32, w: f32) {
    let n = segments.max(6);
    let len = TAU * r / n as f32 * 1.08;
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        let mut m = mesh(mats, role::PAINT, MeshId::CUBE, [at[0] + a.sin() * r, 0.02, at[1] + a.cos() * r], [len, 0.03, w], c);
        m.rotation = quat_axis_angle([0.0, 1.0, 0.0], a);
        rw.draw_decal(m);
    }
}

/// A square frame on the ground round a building (half side `h`).
fn square(rw: &mut RenderWorld, mats: &Mats, c: [f32; 4], at: [f32; 2], h: f32, w: f32) {
    let l = h * 2.0 + w;
    for (p, s) in [([at[0], at[1] - h], [l, w]), ([at[0], at[1] + h], [l, w]), ([at[0] - h, at[1]], [w, l]), ([at[0] + h, at[1]], [w, l])] {
        rw.draw_decal(mesh(mats, role::PAINT, MeshId::CUBE, [p[0], 0.02, p[1]], [s[0], 0.03, s[1]], c));
    }
}

fn label(size: f32, color: [f32; 4], align: Align) -> TextStyle {
    let mut s = TextStyle::new(size, color);
    s.align = align;
    s.shadow = Some([0.0, 0.0, 0.0, 0.8]);
    s
}

/// A line of text on a dark backing box (tinted by the fog colour); returns
/// its size.
fn boxed(rw: &mut RenderWorld, pos: [f32; 2], s: &str, st: &TextStyle, pad: f32, back: [f32; 4]) -> [f32; 2] {
    let size = text::measure(s, st);
    let x = match st.align {
        Align::Left => pos[0],
        Align::Center => pos[0] - size[0] * 0.5,
        Align::Right => pos[0] - size[0],
    };
    rw.draw_sprite(sprite([x + size[0] * 0.5, pos[1] + size[1] * 0.5], [size[0] + pad * 2.0, size[1] + pad * 2.0], back, 100));
    text::draw(rw, pos, 110, s, st)
}

fn selection_text(m: &Match, ui: &Ui) -> Option<String> {
    if let Some(b) = ui.selected.iter().find_map(|id| m.building(*id)).filter(|b| b.team == PLAYER) {
        let name = Types::building_name(b.kind);
        if !b.built {
            return Some(format!("{name}: building {}%", (b.progress * 100.0) as i32));
        }
        let Some(&front) = b.queue.first() else {
            return Some(format!("{name}: idle"));
        };
        let train = m.types.unit(front, us::TRAIN_S).max(0.01);
        let names: Vec<&str> = b.queue.iter().map(|k| Types::unit_name(*k)).collect();
        return Some(format!("{name}: {} {}%", names.join(", "), (b.train_t / train * 100.0).min(100.0) as i32));
    }
    let mut n = [0usize; 3];
    let mut busy = 0;
    for u in ui.selected.iter().filter_map(|id| m.unit(*id)).filter(|u| u.team == PLAYER) {
        n[u.kind.min(2)] += 1;
        if u.order != Order::Idle {
            busy += 1;
        }
    }
    if n.iter().sum::<usize>() == 0 {
        return None;
    }
    let parts: Vec<String> = [WORKER, TROOPER, RUMBLER].iter().filter(|k| n[**k] > 0).map(|k| format!("{} {}", n[*k], Types::unit_name(*k))).collect();
    Some(format!("{} ({busy} busy)", parts.join(", ")))
}

/// The HUD's colours, all from the art palette.
#[derive(Debug, Clone, Copy)]
struct HudInk {
    text: [f32; 4],
    soft: [f32; 4],
    amber: [f32; 4],
    back: [f32; 4],
}

impl HudInk {
    fn new(c: &Colours) -> Self {
        let mut back = scale(c.fog, 0.5);
        back[3] = 0.72;
        let mut soft = scale(c.ui, 0.82);
        soft[3] = 1.0;
        Self { text: c.ui, soft, amber: c.amber, back }
    }
}

#[allow(clippy::too_many_arguments)]
fn hud(rw: &mut RenderWorld, m: &Match, ui: &Ui, t: &Tune, cam: &CameraPose, vp: [f32; 2], touching: bool, art: &Art) {
    let col = &art.colours;
    let ink = HudInk::new(col);
    let k = t.hud_scale.max(0.5);
    let mg = t.hud_margin * k;
    let big = 20.0 * k;
    let small = if vp[0] < 700.0 { 10.0 * k } else { 20.0 * k * 0.8 };

    // Health bars (under the HUD layers).
    let fog = &m.fog[PLAYER as usize];
    for u in &m.units {
        let max = m.types.unit(u.kind, us::HP).max(1.0);
        if !(u.hp < max || ui.selected.contains(&u.id)) || (u.team != PLAYER && !fog.visible(&m.grid, u.pos)) {
            continue;
        }
        if let Some(s) = to_screen(cam, [u.pos[0], 2.0, u.pos[1]], vp) {
            bar(rw, col, s, 24.0 * k, u.hp / max, u.team);
        }
    }
    for b in &m.buildings {
        let max = m.types.building(b.kind, bs::HP).max(1.0);
        if (b.hp >= max && !ui.selected.contains(&b.id)) || (b.team != PLAYER && fog.at(&m.grid, b.corner) < EXPLORED) {
            continue;
        }
        let r = m.building_rect(b);
        let c = rect_center(r);
        if let Some(s) = to_screen(cam, [c[0], (r[2] - r[0]) * 0.8, c[1]], vp) {
            bar(rw, col, s, 48.0 * k, b.hp / max, b.team);
        }
    }

    // Counter, top left.
    let counter = format!("Crystal {}   Units {}/{}", m.crystal[PLAYER as usize] as i32, m.supply(PLAYER), t.pop_cap as i32);
    boxed(rw, [mg, mg], &counter, &label(big, ink.amber, Align::Left), 6.0 * k, ink.back);
    // Mode line and notices, top centre (below the pause button row on touch).
    let mode = match mode_line(ui) {
        "" if m.free_roam => "Free roam",
        "" => "Skirmish",
        s => s,
    };
    let mut y = mg + if touching { 40.0 * k } else { 0.0 };
    y += boxed(rw, [vp[0] * 0.5, y], mode, &label(small, ink.text, Align::Center), 4.0 * k, ink.back)[1] + 10.0 * k;
    for (n, left) in &ui.notices {
        let mut c = ink.amber;
        c[3] = left.clamp(0.0, 0.5) * 2.0;
        y += boxed(rw, [vp[0] * 0.5, y], n, &label(big, c, Align::Center), 4.0 * k, ink.back)[1] + 8.0 * k;
    }

    // Minimap: bottom right (top right under the pause button on touch).
    let side = (t.minimap_px * k).min(vp[0] * 0.35).min(vp[1] * 0.4);
    let at = if touching { [vp[0] - mg - side, mg + 48.0 * k] } else { [vp[0] - mg - side, vp[1] - mg - side] };
    minimap(rw, m, cam, vp, at, side, art);

    // Key hints and the selection panel, bottom left.
    let mut by = vp[1] - mg - small;
    if !touching {
        let hints = "E train  R heavy  B build  F attack  X stop  G army  H home  Ctrl+1-3 group";
        boxed(rw, [mg, by], hints, &label(small, ink.soft, Align::Left), 4.0 * k, ink.back);
        by -= big + 14.0 * k;
    } else {
        // Clear of the pan stick.
        by -= vp[1] * 0.3;
    }
    if let Some(s) = selection_text(m, ui) {
        boxed(rw, [mg, by], &s, &label(big, ink.text, Align::Left), 6.0 * k, ink.back);
    }

    // Box select, in the player's team colour.
    if let Some((a, b)) = ui.drag {
        let (x0, x1, y0, y1) = (a[0].min(b[0]), a[0].max(b[0]), a[1].min(b[1]), a[1].max(b[1]));
        let mut edge = mix(col.team[0], col.ui, 0.5);
        edge[3] = 0.95;
        frame(rw, [x0, y0], [x1, y1], 2.0, edge, 101);
        let mut fill = col.team[0];
        fill[3] = 0.14;
        rw.draw_sprite(sprite([(x0 + x1) * 0.5, (y0 + y1) * 0.5], [x1 - x0, y1 - y0], fill, 100));
    }

    // Banner.
    let (title, sub) = match ui.phase {
        Phase::Title => ("Cinder Ridge", if touching { "Tap to start" } else { "Click or press Enter to start" }),
        Phase::Over(w) => (
            if w == PLAYER { "Victory" } else { "Defeat" },
            if touching { "Tap for a new match" } else { "Enter or click for a new match" },
        ),
        Phase::Playing if ui.paused => ("Paused", if touching { "Tap pause to resume" } else { "P or Esc to resume" }),
        Phase::Playing => return,
    };
    let cy = vp[1] * 0.4;
    let mut band = ink.back;
    band[3] = 0.6;
    rw.draw_sprite(sprite([vp[0] * 0.5, cy + 30.0 * k], [vp[0], 110.0 * k], band, 115));
    let title_col = match ui.phase {
        Phase::Over(w) if w == PLAYER => ink.amber,
        Phase::Over(_) => col.damage,
        _ => ink.text,
    };
    text::draw(rw, [vp[0] * 0.5, cy], 120, title, &label(40.0 * k, title_col, Align::Center));
    text::draw(rw, [vp[0] * 0.5, cy + 52.0 * k], 120, sub, &label(big, ink.soft, Align::Center));
}

/// A hollow rectangle from `a` to `b` (screen pixels).
fn frame(rw: &mut RenderWorld, a: [f32; 2], b: [f32; 2], w: f32, c: [f32; 4], layer: i16) {
    let (mx, my) = ((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    for (p, s) in [([mx, a[1]], [dx, w]), ([mx, b[1]], [dx, w]), ([a[0], my], [w, dy]), ([b[0], my], [w, dy])] {
        rw.draw_sprite(sprite(p, s, c, layer));
    }
}

/// A health bar: dark edge, light back (the palette's UI colour), fill in
/// the team's state colour.
fn bar(rw: &mut RenderWorld, col: &Colours, at: [f32; 2], w: f32, frac: f32, team: u8) {
    let f = frac.clamp(0.0, 1.0);
    rw.draw_sprite(sprite(at, [w + 3.0, 7.0], [0.0, 0.0, 0.0, 0.7], 89));
    let mut back = col.ui;
    back[3] = 0.85;
    rw.draw_sprite(sprite(at, [w + 1.0, 5.0], back, 90));
    let fill = if team == PLAYER { col.valid } else { col.damage };
    rw.draw_sprite(sprite([at[0] - w * 0.5 * (1.0 - f), at[1]], [w * f, 3.0], fill, 91));
}

/// A 32 x 32 minimap (one block per 2 x 2 cells) with team dots and the
/// camera's ground footprint.
fn minimap(rw: &mut RenderWorld, m: &Match, cam: &CameraPose, vp: [f32; 2], at: [f32; 2], side: f32, art: &Art) {
    const N: i32 = 32;
    let col = &art.colours;
    let g = &m.grid;
    let fog = &m.fog[PLAYER as usize];
    // The SQUARE sprite has rounded corners and fills only its middle, so
    // every block is drawn 1 / SQ_CORE larger: blocks meet edge to edge and
    // the backing never shows through as a grid of dark dots.
    const SQ_CORE: f32 = 0.76;
    let mut edge = scale(col.fog, 0.4);
    edge[3] = 1.0;
    let back = (side + 4.0) / SQ_CORE;
    rw.draw_sprite(sprite([at[0] + side * 0.5, at[1] + side * 0.5], [back, back], edge, 100));
    let (sx, sy) = ((g.w / N).max(1), (g.h / N).max(1));
    let px = side / N as f32;
    for by in 0..N {
        for bx in 0..N {
            let c = [bx * sx, by * sy];
            let cc = cell_color(art, g.terrain_at(c), fog.at(g, c));
            rw.draw_sprite(sprite([at[0] + (bx as f32 + 0.5) * px, at[1] + (by as f32 + 0.5) * px], [px / SQ_CORE, px / SQ_CORE], cc, 101));
        }
    }
    let half = g.half();
    let map = |p: [f32; 2]| [at[0] + (p[0] + half[0]) / (half[0] * 2.0) * side, at[1] + (p[1] + half[1]) / (half[1] * 2.0) * side];
    for b in m.buildings.iter().filter(|b| b.team == PLAYER || fog.at(g, b.corner) >= EXPLORED) {
        let s = (b.size as f32 / g.w as f32 * side).max(3.0);
        rw.draw_sprite(sprite(map(rect_center(m.building_rect(b))), [s, s], col.team[b.team as usize & 1], 102));
    }
    for u in m.units.iter().filter(|u| u.team == PLAYER || fog.visible(g, u.pos)) {
        rw.draw_sprite(sprite(map(u.pos), [3.0, 3.0], scale(col.team[u.team as usize & 1], 1.3), 103));
    }
    // The camera footprint: where the canvas corners meet the ground.
    let corners = [[0.0, 0.0], [vp[0], 0.0], [0.0, vp[1]], [vp[0], vp[1]]];
    let hits: Vec<[f32; 2]> = corners.iter().filter_map(|c| to_ground(cam, *c, vp)).collect();
    if hits.len() == 4 {
        let (mut x0, mut x1, mut z0, mut z1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for h in &hits {
            (x0, x1, z0, z1) = (x0.min(h[0]), x1.max(h[0]), z0.min(h[1]), z1.max(h[1]));
        }
        let a = map([x0.max(-half[0]), z0.max(-half[1])]);
        let b = map([x1.min(half[0]), z1.min(half[1])]);
        let mut c = col.ui;
        c[3] = 0.85;
        frame(rw, a, b, 1.5, c, 104);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MapData, MAP_JSON, UNITS_JSON};

    fn start() -> Match {
        let map = MapData::from_json(MAP_JSON).unwrap();
        let types = Types::from_json(UNITS_JSON).unwrap();
        Match::new(&map, types, &Tune::default(), false)
    }

    /// The ground beyond the map is strips that tile each side exactly (no
    /// gap for a lit plain to show through), flat fog beyond unexplored
    /// edges, shading in small steps along each side (no seam), and lying
    /// just under the edge slabs' tops (no dark slab side along the edge).
    #[test]
    fn far_ground_tiles_every_side_and_shades_smoothly() {
        let m = start();
        let g = &m.grid;
        let h = g.half();
        let fog = &m.fog[PLAYER as usize];
        let field = FogField::build(g, fog, 2);
        let look = EdgeLook { explored_dim: 0.5, explored_to_fog: 0.2, unexplored_to_fog: 1.0 };
        let strips = far_strips(g, &field, 30.0, &look, 3);
        let along = |r: &[f32; 4], horiz: bool| if horiz { r[2] - r[0] } else { r[3] - r[1] };
        let north: f32 = strips.iter().filter(|f| f.rect[3] == -h[1] && f.rect[0] >= -h[0] && f.rect[2] <= h[0]).map(|f| along(&f.rect, true)).sum();
        // East and west reach over the corners: one strip, no seam across.
        let ext = (h[0].max(h[1]) + 30.0) * 2.0;
        let east: Vec<_> = strips.iter().filter(|f| f.rect[0] == h[0]).collect();
        let east_len: f32 = east.iter().map(|f| along(&f.rect, false)).sum();
        let (lo, hi) = east.iter().fold((f32::MAX, f32::MIN), |(lo, hi), f| (lo.min(f.rect[1]), hi.max(f.rect[3])));
        assert!((north - h[0] * 2.0).abs() < 1e-3 && (east_len - (h[1] + ext) * 2.0).abs() < 1e-3, "{north} {east_len}");
        assert!((lo + h[1] + ext).abs() < 1e-3 && (hi - h[1] - ext).abs() < 1e-3, "{lo} {hi}");
        assert!(strips.iter().any(|f| f.look.1 >= 1.0 - 1e-3), "the far side of the map starts fogged");
        assert!(strips.iter().any(|f| f.look.1 < 0.5), "the home side starts seen");
        // Neighbouring pieces along the south side differ by a small step.
        let mut south: Vec<_> = strips.iter().filter(|f| f.rect[1] == h[1] && f.rect[0] >= -h[0] && f.rect[2] <= h[0]).collect();
        south.sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
        for w in south.windows(2) {
            assert!((w[0].look.1 - w[1].look.1).abs() < 0.08 && (w[0].look.0 - w[1].look.0).abs() < 0.08, "{:?} {:?}", w[0], w[1]);
        }
        let ta = Art::from_json(crate::art::ART_JSON).unwrap().terrain;
        let lowest_top = ta.grass_top.min(ta.ford_top).min(ta.fog_top);
        assert!(strips.iter().all(|f| f.y < lowest_top && f.y > lowest_top - 0.05), "far ground sits just under the slab tops");
    }

    /// At the start every soft corner sits on a seen cell's corner with
    /// unexplored cells round it, and there are some to soften.
    #[test]
    fn soft_corners_sit_on_the_fog_edge() {
        let m = start();
        let g = &m.grid;
        let fog = &m.fog[PLAYER as usize];
        let soft = soft_corners(g, fog);
        assert!(!soft.is_empty());
        for p in &soft {
            let around = [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]].map(|d| fog.at(g, g.cell_of([p[0] + d[0], p[1] + d[1]])));
            assert_eq!(around.iter().filter(|f| **f == UNEXPLORED).count(), 3, "{p:?} {around:?}");
        }
    }
}
