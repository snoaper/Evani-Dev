//! The candy world around the road: every prop is a [`Prop`] (a kind, a
//! place, a turn, a size and a seed), laid out once from the track by
//! [`layout`] and turned into instanced meshes by [`Prop::instances`]. The
//! layout is pure data (the same track always gives the same props), so the
//! plugin keeps the props as ECS entities and the statics cache is built
//! from them.
//!
//! What is here: roadside candy (wrapped-sweet signs with chevrons and
//! candy-cane barriers on the outside of bends, flags, picket fences,
//! lollipops, gumdrops), trees (round trees, lollipop trees, candy-floss
//! bushes) with grass tufts, two gingerbread grandstands with a crowd of
//! gumdrop people by the start line, the big gumdrop mountains from the
//! track file, a closed ring of gumdrop hills behind them, clouds, and the
//! start gate's light housing. The far ground and the look's fog hide the
//! end of the world from every camera.

use evani_core::ecs::{Entity, World};
use evani_render::math::{quat_axis_angle, quat_mul};
use evani_render::{MeshId, MeshInstance};

use crate::draw::{Mat, add, inst, role, rot, yaw_q};
use crate::track::{Track, hash01};

/// Side of the square far ground (m): twice the renderer's far plane, so
/// its edge is always clipped, and fully fogged long before that.
pub const FAR_GROUND_M: f32 = 2000.0;
/// Distance between roadside prop slots along the track (m).
pub const PROP_SPACING: f32 = 14.0;
/// The hill ring: (radius from the track's centre in m, hills, lowest and
/// tallest height in m). Two overlapping rows close the horizon all round;
/// a third, far row of tall hazy hills rises to the eye height of the high
/// title orbit, so even from up there no sky shows under the horizon (the
/// far ground beyond is clipped at the renderer's 1000 m far plane).
pub const HILL_RING: [(f32, u32, f32, f32); 3] = [(300.0, 24, 16.0, 40.0), (362.0, 26, 34.0, 72.0), (640.0, 22, 96.0, 130.0)];
/// The hill row Low leaves out to stay in its triangle budget: the middle
/// one only layers the backdrop, the near and far rows still close it.
pub const HILL_ROW_THIN_SKIPS: usize = 1;
/// Clouds on Medium and up; Low draws [`CLOUDS_THIN`].
pub const CLOUDS: u32 = 28;
/// Clouds on Low.
pub const CLOUDS_THIN: u32 = 14;
/// Below this per-frame triangle limit (the Low tier's 100 000) only every
/// [`THIN_TREE_STEP`]th tree is kept, candy-cane runs are shorter and the
/// crowd and grass tufts are left out.
pub const THIN_BELOW_TRIANGLES: u32 = 120_000;
/// See [`THIN_BELOW_TRIANGLES`].
pub const THIN_TREE_STEP: usize = 3;
/// Length of a grandstand along the track (m).
pub const STAND_LEN: f32 = 20.0;
/// Gumdrop people per stand row (three rows per stand).
pub const CROWD_PER_ROW: u32 = 12;

/// Candy colours for props (instance tints on the white kart-body material).
pub const CANDY: [[f32; 3]; 6] = [
    [1.0, 0.45, 0.7],
    [0.45, 0.95, 0.75],
    [0.72, 0.58, 1.0],
    [1.0, 0.85, 0.3],
    [0.45, 0.75, 1.0],
    [1.0, 1.0, 1.0],
];
/// Lollipop swirl pairs (outer, ring): cherry/sugar, lemon/white,
/// sky/sugar, purple/bubblegum.
const SWIRLS: [([f32; 3], [f32; 3]); 4] = [
    ([0.88, 0.08, 0.15], [0.93, 0.93, 0.96]),
    ([1.0, 0.7, 0.08], [1.0, 1.0, 1.0]),
    ([0.25, 0.78, 1.0], [0.93, 0.93, 0.96]),
    ([0.36, 0.05, 1.0], [0.95, 0.45, 0.62]),
];
/// Sweet wrapper colours: lemon, sky, cherry, mint.
const WRAPPERS: [[f32; 3]; 4] = [[1.0, 0.7, 0.08], [0.25, 0.78, 1.0], [0.88, 0.08, 0.15], [0.35, 0.85, 0.62]];
/// Gumdrop-hill lemon (the look has pink, mint and lilac hill materials).
const LEMON: [f32; 3] = [1.0, 0.87, 0.35];
/// Gingerbread brown and its darker back wall.
const GINGER: [f32; 3] = [0.5, 0.25, 0.1];

/// What a prop is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    /// Trunk and two round canopies.
    Tree,
    /// White stick and a big swirl disc.
    LollipopTree,
    /// Three overlapping pink or lilac puffs.
    FlossBush,
    /// Three small cones of grass.
    Tuft,
    /// A big gumdrop mountain from the track file; `seed` is its material
    /// role.
    Mountain,
    /// A hill of the backdrop ring.
    Hill,
    /// Flat-bottomed puffs high up.
    Cloud,
    /// Wrapped sweet on two posts with chevrons into the bend; `size[0]` is
    /// the bend direction (+1 left, -1 right).
    SweetSign,
    /// Candy cane by the rail on the outside of a bend.
    CandyCane,
    /// Flag on a pole.
    Flag,
    /// Seven-post picket fence following the shoulder from `s` at `lat`.
    Fence,
    /// Lollipop on a stick, facing the track.
    Lollipop,
    /// Two or three squashed gumdrops.
    Gumdrops,
    /// Stepped gingerbread stand with a striped roof; `size[0]` is the side
    /// it stands on (+1 left), `size[1]` > 0 adds the crowd.
    Grandstand,
    /// Sprinkle mushroom in the infield: a cream stem, a candy cap and five
    /// white dots; `size[0]` is its size factor.
    Mushroom,
    /// Cupcake building: a pleated case with a chocolate door and two round
    /// windows, three tiers of frosting and a cherry; `size[0]` is its size
    /// factor. Its door faces local +z.
    Cupcake,
}

/// One placed prop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    pub kind: PropKind,
    /// Ground point under the prop.
    pub pos: [f32; 3],
    pub yaw: f32,
    /// Kind-specific size (see [`PropKind`]).
    pub size: [f32; 3],
    /// Hash seed for colours and variation.
    pub seed: u32,
    /// Track distance and lateral offset, for props that follow the track.
    pub s: f32,
    pub lat: f32,
}

impl Prop {
    fn new(kind: PropKind, pos: [f32; 3], yaw: f32, size: [f32; 3], seed: u32) -> Self {
        Self {
            kind,
            pos,
            yaw,
            size,
            seed,
            s: 0.0,
            lat: 0.0,
        }
    }
}

/// Centre of the track's bounds (on the ground).
pub fn centre(track: &Track) -> [f32; 3] {
    let b = track.bounds;
    [(b[0] + b[2]) * 0.5, track.meadow_y, (b[1] + b[3]) * 0.5]
}

/// The side (+1 left, -1 right) of the track at `s` that faces away from
/// the middle of the circuit.
fn outside(track: &Track, s: f32) -> f32 {
    let c = centre(track);
    let d = |lat: f32| {
        let p = track.point_at(s, lat);
        (p[0] - c[0]).powi(2) + (p[2] - c[2]).powi(2)
    };
    if d(10.0) > d(-10.0) { 1.0 } else { -1.0 }
}

/// Track distances of the two grandstands: beside the grid and just past
/// the line.
pub fn stand_s(track: &Track) -> [f32; 2] {
    [track.length - 20.0, 30.0]
}

/// The props in the world: one entity with a [`Prop`] component
/// (`kart_racer.prop`) each, listed in layout order so the statics are
/// built the same way every time. World resource.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scenery {
    /// The layout the entities were spawned for: verge width and the
    /// small-budget layout.
    pub verge: f32,
    pub thin: bool,
    pub props: Vec<Entity>,
}

/// Make the prop entities match the layout for `verge` and `thin`: a no-op
/// when they already do, otherwise the old props are despawned and the new
/// layout is spawned. Returns whether anything changed.
pub fn spawn_props(world: &mut World, verge: f32, thin: bool) -> bool {
    let (same, old) = match world.try_res::<Scenery>() {
        Some(s) => (s.verge == verge && s.thin == thin && !s.props.is_empty(), s.props.clone()),
        None => (false, Vec::new()),
    };
    if same {
        return false;
    }
    let props = {
        let race = world.res::<crate::race::Race>();
        layout(&race.track, verge, thin)
    };
    for e in old {
        world.despawn(e);
    }
    let props = props.into_iter().map(|p| world.spawn_with(p)).collect();
    world.insert_resource(Scenery { verge, thin, props });
    true
}

/// The props, read from their entities in layout order.
pub fn props(world: &World) -> Vec<Prop> {
    world
        .try_res::<Scenery>()
        .map(|s| s.props.iter().filter_map(|&e| world.get::<Prop>(e).map(|p| *p)).collect())
        .unwrap_or_default()
}

/// Every prop for this track. `verge` is the grass between road and rail;
/// `thin` is the small-budget layout (see [`THIN_BELOW_TRIANGLES`]).
pub fn layout(track: &Track, verge: f32, thin: bool) -> Vec<Prop> {
    let mut out = Vec::new();
    roadside(track, verge, thin, &mut out);
    let stands = stand_s(track);
    for (j, s) in stands.into_iter().enumerate() {
        let side = outside(track, s);
        let lat = side * (track.half_width + verge + 2.2);
        let tan = track.tangent_at(s);
        let p = track.point_at(s, lat);
        let mut st = Prop::new(PropKind::Grandstand, [p[0], p[1] - 0.06, p[2]], tan[0].atan2(tan[1]), [side, if thin { 0.0 } else { 1.0 }, 0.0], 500 + j as u32);
        st.s = s;
        st.lat = lat;
        out.push(st);
    }
    // Trees, lollipop trees and floss bushes, with grass tufts at their feet
    // (Low keeps every THIN_TREE_STEP-th tree and no tufts).
    let trees = trees(track, verge);
    if thin {
        out.extend(trees.iter().filter(|p| p.kind != PropKind::Tuft && (p.seed as usize).is_multiple_of(THIN_TREE_STEP)));
    } else {
        out.extend(trees.iter());
    }
    for (i, m) in track.scenery.mountains.iter().enumerate() {
        let r = match m.material.as_str() {
            "mountain_mint" => role::MOUNTAIN_MINT,
            "mountain_lilac" => role::MOUNTAIN_LILAC,
            _ => role::MOUNTAIN_PINK,
        };
        out.push(Prop::new(PropKind::Mountain, [m.pos[0], track.meadow_y, m.pos[2]], hash01(i as u32, 5) * 3.0, m.size, r as u32));
    }
    // The hill ring, closed all round.
    let c = centre(track);
    for (row, (radius, n, lo, hi)) in HILL_RING.into_iter().enumerate() {
        if thin && row == HILL_ROW_THIN_SKIPS {
            continue;
        }
        for k in 0..n {
            let seed = 1000 + row as u32 * 100 + k;
            let a = (k as f32 + 0.5 * row as f32 + 0.3 * hash01(seed, 3)) / n as f32 * core::f32::consts::TAU;
            let r = radius + 18.0 * (hash01(seed, 5) - 0.5);
            let w = core::f32::consts::TAU * radius / n as f32 * (1.45 + 0.3 * hash01(seed, 7));
            let h = lo + (hi - lo) * hash01(seed, 11);
            let pos = [c[0] + r * a.cos(), track.meadow_y, c[2] + r * a.sin()];
            out.push(Prop::new(PropKind::Hill, pos, core::f32::consts::FRAC_PI_2 - a, [w, h, w * (0.55 + 0.2 * hash01(seed, 13))], seed));
        }
    }
    // Clouds on a wide ring, high up.
    let n = if thin { CLOUDS_THIN } else { CLOUDS };
    for k in 0..n {
        let seed = 2000 + k;
        let a = (k as f32 + 0.6 * hash01(seed, 3)) / n as f32 * core::f32::consts::TAU;
        let r = 400.0 + 170.0 * hash01(seed, 5);
        let pos = [c[0] + r * a.cos(), 105.0 + 55.0 * hash01(seed, 7), c[2] + r * a.sin()];
        let puffs = if thin { 3.0 } else { 3.0 + (hash01(seed, 11) * 3.0).floor() };
        out.push(Prop::new(PropKind::Cloud, pos, core::f32::consts::FRAC_PI_2 - a + 0.5 * (hash01(seed, 13) - 0.5), [0.8 + 0.6 * hash01(seed, 17), puffs, 0.0], seed));
    }
    // Mushrooms and cupcake houses in the infield (Medium and up), and a ring
    // of cupcake houses out on the meadow before the mountains (every tier).
    let site = crate::finale::podium_site(track, verge);
    if !thin {
        infield(track, verge, &trees, &site, &mut out);
    }
    cupcake_ring(track, verge, &trees, thin, &mut out);
    // The podium's meadow and the camera's view of it stay clear of trees,
    // bushes, tufts, mushrooms and cupcakes.
    out.retain(|p| {
        !matches!(p.kind, PropKind::Tree | PropKind::LollipopTree | PropKind::FlossBush | PropKind::Tuft | PropKind::Mushroom | PropKind::Cupcake) || !crate::finale::blocks_podium(&site, p.pos)
    });
    out
}

/// The trees, lollipop trees and floss bushes of the full layout, each
/// followed by the grass tuft at its feet (if it has one). Low draws every
/// [`THIN_TREE_STEP`]th tree of these (by seed) and no tufts.
fn trees(track: &Track, verge: f32) -> Vec<Prop> {
    let mut out = Vec::new();
    // Where each tree stands and how far it reaches: one that would grow
    // into an earlier one is left out (judged on the full layout, so Low's
    // trees are a subset of it).
    let mut grown: Vec<[f32; 3]> = Vec::new();
    for (i, t) in track.trees.iter().enumerate() {
        // Trees stand on the flat meadow: one on the road's bank moves out
        // past the bank's foot; none grows into a gumdrop mountain.
        let Some(tpos) = off_bank(track, verge, t.pos, 1.8 * t.scale) else {
            continue;
        };
        if in_mountain(track, tpos, 2.0 * t.scale) {
            continue;
        }
        let t = crate::track::Tree { pos: tpos, scale: t.scale };
        let i = i as u32;
        let pick = hash01(i, 13);
        let kind = if pick < 0.38 {
            PropKind::LollipopTree
        } else if pick < 0.62 {
            PropKind::FlossBush
        } else {
            PropKind::Tree
        };
        let reach = match kind {
            PropKind::LollipopTree => (1.8 * t.scale * 1.15).min(1.8 * LOLLIPOP_TREE_MAX),
            PropKind::FlossBush => 2.3 * t.scale,
            _ => 1.8 * t.scale,
        };
        if grown.iter().any(|g| (g[0] - t.pos[0]).hypot(g[1] - t.pos[2]) < g[2] + reach) {
            continue;
        }
        grown.push([t.pos[0], t.pos[2], reach]);
        let ground = [t.pos[0], track.meadow_y, t.pos[2]];
        out.push(Prop::new(kind, ground, hash01(i, 17) * core::f32::consts::TAU, [t.scale; 3], i));
        let a = hash01(i, 19) * core::f32::consts::TAU;
        let r = 2.4 * t.scale + 1.0;
        let at = add(ground, [r * a.cos(), 0.0, r * a.sin()]);
        if !on_bank(track, verge, at, 0.4) && !in_mountain(track, at, 0.4) {
            out.push(Prop::new(PropKind::Tuft, at, a, [1.0; 3], i));
        }
    }
    out
}

/// Infield grid cell (m) for mushrooms and cupcake houses, and how far a
/// prop may stand off its cell's middle.
pub const INFIELD_CELL: f32 = 16.0;
const INFIELD_JITTER: f32 = 3.5;
/// Most cupcake houses in the infield, and most mushrooms.
pub const INFIELD_CUPCAKES: usize = 5;
pub const MUSHROOMS: usize = 30;
/// Room (m) a mushroom keeps from the road's bank, and a cupcake house
/// (big enough to hide a kart from the chase camera on the inside of a
/// bend, so it stands further off).
const MUSHROOM_BANK_GAP: f32 = 3.0;
const CUPCAKE_BANK_GAP: f32 = 14.0;
/// The ring of cupcake houses out on the meadow: nearest and furthest
/// distance from the track's centre (m), and slots round it. The far end
/// stays inside the mountain ranges' geocheck tag (240 m) and the hill ring.
pub const CUPCAKE_RING: (f32, f32, u32) = (170.0, 230.0, 14);

/// How far (m) a mushroom or cupcake house reaches out from its middle on
/// the ground (its cap or frosting), for a size factor `s`.
pub fn foot(kind: PropKind, s: f32) -> f32 {
    match kind {
        PropKind::Mushroom => MUSHROOM_CAP_R * s + 0.1,
        PropKind::Cupcake => 4.3 * s,
        _ => 0.0,
    }
}

/// Whether `(x, z)` lies inside the track's loop (the centreline taken as
/// a polygon, even-odd rule).
pub fn inside_loop(track: &Track, x: f32, z: f32) -> bool {
    let s = &track.samples;
    let mut inside = false;
    let mut j = s.len() - 1;
    for i in 0..s.len() {
        let (a, b) = (s[i].pos, s[j].pos);
        if (a[2] > z) != (b[2] > z) && x < (b[0] - a[0]) * (z - a[2]) / (b[2] - a[2]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Yaw that turns a prop's local +z (a cupcake's door) towards `to`.
fn facing(from: [f32; 3], to: [f32; 3]) -> f32 {
    (to[0] - from[0]).atan2(to[2] - from[2])
}

/// Whether a prop of `kind` and size `s` at `p` stands in the podium's way
/// (its middle or any edge of its foot).
fn in_podium_view(site: &crate::finale::Site, kind: PropKind, s: f32, p: [f32; 3]) -> bool {
    let r = foot(kind, s);
    [[0.0f32, 0.0], [r, 0.0], [-r, 0.0], [0.0, r], [0.0, -r]].iter().any(|o| crate::finale::blocks_podium(site, [p[0] + o[0], p[1], p[2] + o[1]]))
}

/// Up to [`INFIELD_CUPCAKES`] cupcake houses, then up to [`MUSHROOMS`]
/// mushrooms, on a jittered grid inside the track's loop: on open meadow
/// (off the road's bank, outside the mountains, clear of the full layout's
/// trees and of each other) and out of the podium's view. Medium and up
/// only.
fn infield(track: &Track, verge: f32, trees: &[Prop], site: &crate::finale::Site, out: &mut Vec<Prop>) {
    let b = track.bounds;
    let nx = ((b[2] - b[0]) / INFIELD_CELL).ceil() as u32;
    let nz = ((b[3] - b[1]) / INFIELD_CELL).ceil() as u32;
    let mut placed: Vec<[f32; 3]> = Vec::new();
    // The cupcake houses first (they need the most room): one cell in
    // three is a candidate; then a mushroom in about half the cells.
    for (kind, chance, most, gap) in [(PropKind::Cupcake, 0.35f32, INFIELD_CUPCAKES, CUPCAKE_BANK_GAP), (PropKind::Mushroom, 0.55, MUSHROOMS, MUSHROOM_BANK_GAP)] {
        let mut count = 0;
        for iz in 0..nz {
            for ix in 0..nx {
                let seed = 3000 + iz * 64 + ix;
                let h = |salt: u32| hash01(seed, salt);
                let x = b[0] + (ix as f32 + 0.5) * INFIELD_CELL + (h(3) * 2.0 - 1.0) * INFIELD_JITTER;
                let z = b[1] + (iz as f32 + 0.5) * INFIELD_CELL + (h(5) * 2.0 - 1.0) * INFIELD_JITTER;
                let cupcake = kind == PropKind::Cupcake;
                if count >= most || !inside_loop(track, x, z) || h(if cupcake { 7 } else { 9 }) >= chance {
                    continue;
                }
                let s = if cupcake { 0.9 + 0.3 * h(11) } else { 0.85 + 0.55 * h(11) };
                let p = [x, track.meadow_y, z];
                let r = foot(kind, s);
                if on_bank(track, verge, p, r + gap) || !meadow_clear(track, verge, trees, p, r) || in_podium_view(site, kind, s, p) {
                    continue;
                }
                if placed.iter().any(|q| (q[0] - x).hypot(q[1] - z) < q[2] + r + 1.0) {
                    continue;
                }
                placed.push([x, z, r]);
                count += 1;
                // A cupcake's door faces the road; mushrooms turn any way.
                let yaw = if cupcake {
                    let at = track.locate(p, 0);
                    facing(p, track.point_at(at.s, 0.0))
                } else {
                    h(13) * core::f32::consts::TAU
                };
                out.push(Prop::new(kind, p, yaw, [s, 0.0, 0.0], seed));
            }
        }
    }
}

/// Cupcake houses on a ring out on the meadow, doors towards the track's
/// centre, one per slot where there is open meadow (a slot in a mountain
/// or by a tree tries other distances). Every tier draws them; `thin`
/// (Low) keeps every other slot.
fn cupcake_ring(track: &Track, verge: f32, trees: &[Prop], thin: bool, out: &mut Vec<Prop>) {
    let c = centre(track);
    let (near, far, n) = CUPCAKE_RING;
    for k in 0..n {
        if thin && k % 2 == 1 {
            continue;
        }
        let seed = 3800 + k;
        let h = |salt: u32| hash01(seed, salt);
        let a = (k as f32 + 0.5 * h(3)) / n as f32 * core::f32::consts::TAU;
        let s = 1.0 + 0.2 * h(7);
        for t in [h(5), 0.5, 1.0, 0.0, 0.25, 0.75] {
            let r = near + (far - near) * t;
            let p = [c[0] + r * a.cos(), track.meadow_y, c[2] + r * a.sin()];
            if meadow_clear(track, verge, trees, p, foot(PropKind::Cupcake, s)) {
                out.push(Prop::new(PropKind::Cupcake, p, facing(p, c), [s, 0.0, 0.0], seed));
                break;
            }
        }
    }
}

/// Whether a circle of `radius` m round `p` (on the ground plane) reaches
/// into the footprint of one of the track's mountains (the dome's larger
/// half-size all round, since the layout turns each dome).
fn in_mountain(track: &Track, p: [f32; 3], radius: f32) -> bool {
    track.scenery.mountains.iter().any(|m| {
        let r = m.size[0].max(m.size[2]) * 0.5 + radius;
        (p[0] - m.pos[0]).powi(2) + (p[2] - m.pos[2]).powi(2) < r * r
    })
}

/// Whether a circle of `radius` m round `p` reaches the road's raised
/// ground: the grass shoulder and the bank's slope down to the meadow (as
/// [`crate::draw::build_statics_with`] builds them), where something made
/// for the meadow would be buried.
fn on_bank(track: &Track, verge: f32, p: [f32; 3], radius: f32) -> bool {
    bank_reach(track, verge, p, radius).0 > 0.0
}

/// How far (m) a circle of `radius` round `p` reaches into the road's
/// raised ground, and the lateral distance it must stand at to clear it.
fn bank_reach(track: &Track, verge: f32, p: [f32; 3], radius: f32) -> (f32, f32, crate::track::Located) {
    let at = track.locate(p, 0);
    let bank_edge = track.half_width + verge + 4.0;
    let drop = at.height - 0.06 - track.meadow_y - crate::draw::BANK_FOOT_STEP;
    let run = if drop > 0.02 {
        (drop / crate::draw::BANK_SLOPE).min(crate::draw::BANK_RUN_MAX).max(drop / crate::draw::BANK_SLOPE_MAX)
    } else {
        0.0
    };
    let clear = bank_edge + run + radius;
    (clear - at.lateral.abs(), clear, at)
}

/// `p` if a circle of `radius` round it is clear of the road's raised
/// ground; otherwise `p` moved straight out from the road until it is (on
/// the meadow), or `None` when the move lands on raised ground again (a
/// different stretch of road).
fn off_bank(track: &Track, verge: f32, p: [f32; 3], radius: f32) -> Option<[f32; 3]> {
    let (reach, clear, at) = bank_reach(track, verge, p, radius);
    if reach <= 0.0 {
        return Some(p);
    }
    let side = if at.lateral >= 0.0 { 1.0 } else { -1.0 };
    let moved = track.point_at(at.s, side * (clear + 0.5));
    let q = [moved[0], p[1], moved[2]];
    (!on_bank(track, verge, q, radius)).then_some(q)
}

/// Whether a circle of `radius` m round `p` lies on open meadow: off the
/// road's raised ground, outside every mountain, and clear of every
/// prop standing on the meadow (so a raised grass patch never lies under
/// a tree's, mushroom's or cupcake's ground disc or foot).
pub fn meadow_clear(track: &Track, verge: f32, props: &[Prop], p: [f32; 3], radius: f32) -> bool {
    !on_bank(track, verge, p, radius)
        && !in_mountain(track, p, radius)
        && props.iter().all(|pr| {
            let foot = match pr.kind {
                PropKind::Tree | PropKind::LollipopTree | PropKind::FlossBush => 3.0 * pr.size[0],
                PropKind::Tuft => 1.0,
                PropKind::Mushroom | PropKind::Cupcake => foot(pr.kind, pr.size[0]),
                _ => return true,
            };
            let r = radius + foot;
            (pr.pos[0] - p[0]).powi(2) + (pr.pos[2] - p[2]).powi(2) >= r * r
        })
}

/// How far (m) a roadside prop's foot reaches out from its centre: it must
/// stand this far inside the shoulder's edge.
const ROADSIDE_FOOT: f32 = 0.6;
/// A gumdrop row's distance out past the verge (m), clear of the rail.
const GUMDROP_OUT: f32 = 2.2;
/// A gumdrop dome's reach from the row's line (m).
const GUMDROP_FOOT: f32 = 1.2;

/// Roadside candy just outside the rails, one slot every [`PROP_SPACING`]
/// metres (none at the start gantry or by the grandstands): a wrapped-sweet
/// sign and a run of candy canes on the outside of tight bends; elsewhere
/// flags, picket fences, lollipops and gumdrops, picked by hashing the slot.
fn roadside(track: &Track, verge: f32, thin: bool, out: &mut Vec<Prop>) {
    let hw = track.half_width;
    let slots = (track.length / PROP_SPACING).floor() as u32;
    let stands = stand_s(track);
    // The shoulder comes in on the inside of the hairpins (the track
    // meshes' limits): a prop that would stand past its edge, over the
    // drop to the meadow, is left out.
    let ss = crate::trackmesh::rings(track);
    let lim = crate::trackmesh::limits(track, &ss);
    let on_shoulder = |s: f32, lat: f32, foot: f32| {
        let n = ss.len();
        let j = ((s.rem_euclid(track.length) / track.length * n as f32).round() as usize) % n;
        let mut cap = [f32::MAX; 2];
        for d in [n - 1, 0, 1] {
            let l = lim[(j + d) % n];
            cap = [cap[0].min(l[0]), cap[1].min(l[1])];
        }
        let b = crate::trackmesh::bank(track, verge, s, cap);
        lat.abs() + foot <= b.edge[if lat >= 0.0 { 0 } else { 1 }]
    };
    for i in 0..slots {
        let h = |salt: u32| hash01(i, salt);
        let s = i as f32 * PROP_SPACING + h(71) * 4.0;
        if s < 8.0 || s > track.length - 8.0 {
            continue;
        }
        if stands.iter().any(|&st| (s - st).abs() < STAND_LEN * 0.5 + 4.0) {
            continue;
        }
        let curve = track.curvature_ahead(s - 10.0, 20.0);
        let bend = curve.abs() > 0.018;
        let side = if bend {
            // Outside of the bend (positive curvature turns left).
            -curve.signum()
        } else if h(73) < 0.5 {
            1.0
        } else {
            -1.0
        };
        let lat = side * (hw + verge + 1.4 + 1.6 * h(79));
        let base = track.point_at(s, lat);
        let ground = [base[0], base[1] - 0.06, base[2]];
        let tan = track.tangent_at(s);
        let yaw = tan[0].atan2(tan[1]);
        let mut p = Prop::new(PropKind::Flag, ground, yaw, [1.0; 3], i);
        p.s = s;
        p.lat = lat;
        if bend {
            p.kind = PropKind::SweetSign;
            p.size = [curve.signum(), 1.0, 1.0];
            if on_shoulder(s, lat, ROADSIDE_FOOT) {
                out.push(p);
            }
            let canes = if thin { 2 } else { 4 };
            for j in 0..canes {
                let cs = s + 6.0 + j as f32 * 3.2;
                let cl = side * (hw + verge + 0.95);
                if !on_shoulder(cs, cl, ROADSIDE_FOOT) {
                    continue;
                }
                let cp = track.point_at(cs, cl);
                let ct = track.tangent_at(cs);
                let mut cane = Prop::new(PropKind::CandyCane, [cp[0], cp[1] - 0.06, cp[2]], ct[0].atan2(ct[1]), [side, 1.0, 1.0], i * 8 + j);
                cane.s = cs;
                cane.lat = cl;
                out.push(cane);
            }
            continue;
        }
        let pick = h(89);
        p.kind = if pick < 0.3 {
            PropKind::Flag
        } else if pick < 0.55 {
            PropKind::Fence
        } else if pick < 0.75 {
            PropKind::Lollipop
        } else {
            PropKind::Gumdrops
        };
        let mut foot = ROADSIDE_FOOT;
        if p.kind == PropKind::Gumdrops {
            // A row of domes along the road, each about a metre round: they
            // stand clear of the rail, wholly on the shoulder.
            p.lat = side * (hw + verge + GUMDROP_OUT);
            let b = track.point_at(s, p.lat);
            p.pos = [b[0], b[1] - 0.06, b[2]];
            foot = GUMDROP_FOOT;
        }
        if on_shoulder(s, p.lat, foot) {
            out.push(p);
        }
    }
}

/// A dark disc on the grass: the static stand-in for a contact shadow
/// under scenery when the tier has no shadow map.
pub fn ground_disc(out: &mut Vec<MeshInstance>, at: [f32; 3], radius: f32, grass: Mat) {
    shadow_disc(out, at, radius, grass, [0.5, 0.6, 0.5]);
}

/// A dark disc of `radius` m on the ground at `at`, in `mat` tinted
/// `tint`: a flat 12-sided fan facing up ([`crate::propmesh::DISC`], 12
/// triangles, Low draws many), one piece with no underside, so nothing in it
/// fights for depth (a closed disc this thin showed its dark underside
/// through its top from afar). It is drawn as a decal (see [`is_disc`]): it
/// lies where the middle of a [`DISC_THICK`] disc resting [`DISC_LIFT`] over
/// the ground would, and the decal depth offset keeps it in front of the
/// grass or asphalt, near and far. Its y scale stays [`DISC_THICK`] (the
/// flat mesh ignores it), which tells it apart.
pub fn shadow_disc(out: &mut Vec<MeshInstance>, at: [f32; 3], radius: f32, mat: Mat, tint: [f32; 3]) {
    let d = radius * 1.7;
    let disc = crate::propmesh::id(crate::propmesh::DISC);
    out.push(inst(disc, [at[0], at[1] + DISC_LIFT + DISC_THICK * 0.5, at[2]], yaw_q(0.0), [d, DISC_THICK, d], mat, tint));
}

/// The y scale of a grandstand's shadow strip, a flat one-sided top (the
/// plane mesh ignores it): it tells the strip apart from the road marks,
/// which are planes [`DISC_THICK`] in that slot too.
pub const STAND_STRIP_MARK: f32 = 2.0 * DISC_THICK;

/// Height of a shadow disc's rim over the ground (m): just clear of it,
/// so it lies on the ground on a gentle slope too (a decal must lie within
/// 3 cm of its surface).
pub const DISC_LIFT: f32 = 0.005;

/// A grass tuft: the colour of its darker-green mound over the meadow's, a
/// blade's width and height (m), how far the blades lean out (rad), how far
/// from the centre their feet stand. The blades take three greens: a shell
/// over a blade (a tip band) lies within geocheck's 5 cm of it and counts as
/// z-fighting.
const TUFT_MOUND: [f32; 3] = [0.45, 0.66, 0.42];
const TUFT_MOUND_H: f32 = 0.16;
const TUFT_BLADE: f32 = 0.36;
const TUFT_TALL: f32 = 0.7;
const TUFT_LEAN: f32 = 0.3;
const TUFT_FOOT: f32 = 0.16;
const TUFT_TONES: [f32; 3] = [0.82, 1.12, 0.96];
/// A shadow disc's height (m).
pub const DISC_THICK: f32 = 0.01;

/// How far each ring of a lollipop's swirl (and a candy cane's band) stands
/// proud of the one inside it (m): faces 5 cm apart fought for depth far off.
pub const SWIRL_STEP: f32 = 0.06;
/// Widths of a lollipop tree's swirl rings, outer to inner (times its size).
pub const TREE_SWIRL: [f32; 3] = [3.6, 2.5, 1.3];
/// Widths of a lollipop's swirl rings, outer to inner (m).
pub const POP_SWIRL: [f32; 3] = [1.6, 1.05, 0.5];
/// Largest lollipop tree size factor: 7.4 x this tall stays under geocheck's
/// 12 m object size, so stick and swirl are judged as one object.
pub const LOLLIPOP_TREE_MAX: f32 = 1.6;
/// A floss bush puff's bottom over the ground (m): just over its ground disc
/// (DISC_LIFT + DISC_THICK), under geocheck's 3 cm floating gap.
pub const PUFF_LIFT: f32 = 0.02;

/// Whether an instance is a shadow disc, or a grandstand's shadow strip
/// (drawn as decals).
/// A disc is the kit's own flat disc mesh, by its placeholder id while the
/// statics are built or by its registered id after: a library mesh (no
/// built-in) whose y scale is [`DISC_THICK`] (nothing else of the kit's own
/// meshes is drawn with that y scale).
pub fn is_disc(m: &MeshInstance) -> bool {
    (usize::from(m.mesh.0) >= evani_render::mesh::BUILTIN_MESH_COUNT && m.scale[1] == DISC_THICK) || is_stand_strip(m)
}

/// Whether `m` is a grandstand's shadow strip (see [`STAND_STRIP_MARK`]).
fn is_stand_strip(m: &MeshInstance) -> bool {
    m.mesh == MeshId::PLANE && m.scale[1] == STAND_STRIP_MARK && m.scale[2] > STAND_LEN
}

/// Whether `m` is a ground shadow stand-in (a prop's disc, a kart's blob or a
/// stand's slab) that the look's shadow tint applies to: the round discs and
/// the long slabs under the grandstands, not the other thin decals (road wear,
/// grass patches), which have colours of their own.
pub fn is_shadow_disc(m: &MeshInstance) -> bool {
    is_disc(m)
}

impl Prop {
    /// Append this prop's meshes. `shadows`: the tier draws a shadow map
    /// (without one, props stand on dark ground discs).
    pub fn instances(&self, track: &Track, mats: &[Mat; 16], shadows: bool, out: &mut Vec<MeshInstance>) {
        let body = mats[role::KART_BODY];
        let white = mats[role::KERB_WHITE];
        let red = mats[role::KERB_RED];
        let (pos, yaw, sz, seed) = (self.pos, self.yaw, self.size, self.seed);
        let q = yaw_q(yaw);
        let up = |o: [f32; 3]| add(pos, rot(yaw, o));
        let h = |salt: u32| hash01(seed, salt);
        let candy = |salt: u32| CANDY[(h(salt) * CANDY.len() as f32) as usize % CANDY.len()];
        let disc = |out: &mut Vec<MeshInstance>, r: f32| {
            if !shadows {
                ground_disc(out, pos, r, mats[role::GRASS]);
            }
        };
        // Turn a Y-axis mesh to lie along local x (discs facing sideways).
        let sideways = quat_mul(q, quat_axis_angle([0.0, 0.0, 1.0], core::f32::consts::FRAC_PI_2));
        // Sticks, posts and trunks: the eight-sided pole (crate::propmesh).
        let pole = crate::propmesh::id(crate::propmesh::POLE);
        match self.kind {
            PropKind::Tree => {
                let s = sz[0];
                let tint = 0.85 + 0.3 * h(11);
                disc(out, 1.5 * s);
                out.push(inst(pole, up([0.0, 1.2 * s, 0.0]), q, [0.7 * s, 2.4 * s, 0.7 * s], mats[role::TRUNK], [1.0; 3]));
                out.push(inst(MeshId::LOW_SPHERE, up([0.0, 3.6 * s, 0.0]), q, [3.4 * s, 3.4 * s, 3.4 * s], mats[role::TREE], [tint; 3]));
                out.push(inst(MeshId::LOW_SPHERE, up([0.5 * s, 5.0 * s, 0.3 * s]), q, [2.2 * s; 3], mats[role::TREE], [tint * 1.08; 3]));
            }
            PropKind::LollipopTree => {
                // At most LOLLIPOP_TREE_MAX across, so the whole tree (7.4 s
                // tall) stays one object for geocheck (12 m).
                let s = (sz[0] * 1.15).min(LOLLIPOP_TREE_MAX);
                let (a, b) = SWIRLS[(h(23) * SWIRLS.len() as f32) as usize % SWIRLS.len()];
                disc(out, 1.0 * s);
                out.push(inst(pole, up([0.0, 2.3 * s, 0.0]), q, [0.32 * s, 4.6 * s, 0.32 * s], white, [1.0; 3]));
                let at = up([0.0, 5.6 * s, 0.0]);
                // Each ring of the swirl stands SWIRL_STEP proud of the one
                // round it on both faces, so their faces never fight.
                // The outer two are washers whose holes the ring inside fills
                // (crate::propmesh::WASHERS), so no hidden cap lies behind a
                // ring's face.
                let [w0, w1, w2] = TREE_SWIRL;
                let washer = |k: usize| crate::propmesh::id(crate::propmesh::WASHERS + k);
                out.push(inst(washer(0), at, sideways, [w0 * s, 0.5 * s, w0 * s], body, a));
                out.push(inst(washer(1), at, sideways, [w1 * s, 0.5 * s + 2.0 * SWIRL_STEP, w1 * s], body, b));
                out.push(inst(MeshId::CYLINDER, at, sideways, [w2 * s, 0.5 * s + 4.0 * SWIRL_STEP, w2 * s], body, a));
            }
            PropKind::FlossBush => {
                let s = sz[0];
                let c = if h(29) < 0.5 { [1.0, 0.6, 0.8] } else { [0.8, 0.66, 1.0] };
                disc(out, 1.6 * s);
                // Each puff rests on the ground just over its ground disc
                // (PUFF_LIFT), so the disc shows round it on Low.
                for (j, o) in [[0.0f32, 0.0], [1.2, 0.6], [-0.9, -0.7]].iter().enumerate() {
                    let r = (2.4 - 0.5 * j as f32) * s;
                    let y = 0.45 * r + PUFF_LIFT;
                    out.push(inst(MeshId::LOW_SPHERE, up([o[0] * s, y, o[1] * s]), q, [r, r * 0.9, r], body, c));
                }
            }
            PropKind::Tuft => {
                // A low darker-green mound (the soft outline: the dark ring the
                // blades stand in, no ink needed) with three chunky blades
                // splaying out of it, each in its own green. Short, wide and
                // few, so the outline pass inks a clean silhouette instead of a
                // scribble of thin cones.
                let foliage = mats[role::TREE];
                out.push(inst(MeshId::CONE, up([0.0, TUFT_MOUND_H * 0.5, 0.0]), q, [1.2, TUFT_MOUND_H, 1.2], mats[role::GRASS], TUFT_MOUND));
                let turn = h(31) * core::f32::consts::TAU;
                for j in 0..3 {
                    let a = turn + j as f32 * 2.1;
                    let (d, lean) = ([a.cos(), a.sin()], TUFT_LEAN + 0.12 * h(33 + j));
                    let tall = TUFT_TALL * (0.8 + 0.4 * h(37 + j)) * if j == 2 { 0.7 } else { 1.0 };
                    let tilt = quat_mul(q, quat_axis_angle([d[1], 0.0, -d[0]], lean));
                    // Along the blade from its foot (on the mound, TUFT_FOOT out
                    // from the centre).
                    let along = |t: f32| up([d[0] * (TUFT_FOOT + t * lean.sin()), 0.1 + t * lean.cos(), d[1] * (TUFT_FOOT + t * lean.sin())]);
                    out.push(inst(MeshId::CONE, along(tall * 0.5), tilt, [TUFT_BLADE, tall, TUFT_BLADE], foliage, [TUFT_TONES[j as usize]; 3]));
                }
            }
            PropKind::Mountain => {
                out.push(inst(MeshId::DOME, pos, q, [sz[0], sz[1] * 2.0, sz[2]], mats[seed as usize % 16], [1.0; 3]));
            }
            PropKind::Hill => {
                let (m, c) = match seed % 4 {
                    0 => (mats[role::MOUNTAIN_PINK], [1.0; 3]),
                    1 => (mats[role::MOUNTAIN_MINT], [1.0; 3]),
                    2 => (mats[role::MOUNTAIN_LILAC], [1.0; 3]),
                    _ => (body, LEMON),
                };
                out.push(inst(MeshId::DOME, pos, q, [sz[0], sz[1] * 2.0, sz[2]], m, c));
            }
            PropKind::Cloud => {
                let s = sz[0];
                let n = sz[1] as u32;
                for j in 0..n {
                    let x = (j as f32 - (n - 1) as f32 * 0.5) * 16.0 * s;
                    let big = 1.0 - 0.35 * ((j as f32 - (n - 1) as f32 * 0.5).abs() / n as f32 * 2.0);
                    let w = 34.0 * s * big * (0.85 + 0.3 * h(40 + j));
                    let o = [x, 0.0, (h(50 + j) - 0.5) * 10.0 * s];
                    out.push(inst(crate::propmesh::id(crate::propmesh::CAP), up(o), q, [w, w * 0.7, w * 0.7], white, [1.05, 1.05, 1.08]));
                }
            }
            PropKind::SweetSign => {
                let dir = sz[0];
                let wrap = WRAPPERS[(h(37) * WRAPPERS.len() as f32) as usize % WRAPPERS.len()];
                for x in [0.8f32, -0.8] {
                    out.push(inst(pole, up([x, 0.8, 0.08]), q, [0.18, 1.6, 0.18], white, [0.85; 3]));
                    if !shadows {
                        ground_disc(out, up([x, 0.0, 0.08]), 0.3, mats[role::GRASS]);
                    }
                }
                // The sweet: a rounded wrapper and two cones twisted in at
                // the ends (tips towards the sweet).
                out.push(inst(MeshId::BEVEL_CUBE, up([0.0, 2.0, 0.0]), q, [2.4, 1.1, 0.6], body, wrap));
                for (x, a) in [(1.55f32, 1.0f32), (-1.55, -1.0)] {
                    let cq = quat_mul(q, quat_axis_angle([0.0, 0.0, 1.0], a * core::f32::consts::FRAC_PI_2));
                    out.push(inst(MeshId::CONE, up([x, 2.0, 0.0]), cq, [1.0, 0.75, 0.7], body, [1.0; 3]));
                }
                // Chevrons into the bend on the face towards oncoming karts:
                // their backs a centimetre into the wrapper, their faces
                // 11 cm proud of it (5 cm fought it for depth far off).
                for (y, roll) in [(0.2f32, -0.75f32), (-0.2, 0.75)] {
                    let bq = quat_mul(q, quat_axis_angle([0.0, 0.0, 1.0], roll * dir));
                    for dx in [0.55f32, -0.25] {
                        out.push(inst(MeshId::CUBE, up([dx * dir, 2.0 + y, -0.35]), bq, [0.62, 0.2, 0.12], red, [1.0; 3]));
                    }
                }
            }
            PropKind::CandyCane => {
                // Post with red bands; the hook curls over the road side.
                let inward = -sz[0];
                disc(out, 0.4);
                out.push(inst(pole, up([0.0, 1.05, 0.0]), q, [0.34, 2.1, 0.34], white, [1.0; 3]));
                // The bands stand SWIRL_STEP proud of the post all round.
                for y in [0.45f32, 1.25] {
                    out.push(inst(MeshId::CYLINDER, up([0.0, y, 0.0]), q, [0.34 + 2.0 * SWIRL_STEP, 0.24, 0.34 + 2.0 * SWIRL_STEP], red, [1.0; 3]));
                }
                let hook = quat_mul(q, quat_axis_angle([0.0, 0.0, 1.0], core::f32::consts::FRAC_PI_2));
                out.push(inst(pole, up([inward * 0.3, 2.2, 0.0]), hook, [0.34, 0.75, 0.34], red, [1.0; 3]));
                out.push(inst(pole, up([inward * 0.62, 1.95, 0.0]), q, [0.34, 0.55, 0.34], white, [1.0; 3]));
            }
            PropKind::Flag => {
                let colour = candy(83);
                disc(out, 0.3);
                out.push(inst(pole, up([0.0, 2.1, 0.0]), q, [0.16, 4.2, 0.16], white, [1.0; 3]));
                out.push(inst(MeshId::LOW_SPHERE, up([0.0, 4.28, 0.0]), q, [0.3; 3], body, colour));
                // The cloth ripples: two panels at slightly different angles.
                out.push(inst(MeshId::BEVEL_CUBE, up([0.0, 3.75, -0.42]), quat_mul(q, yaw_q(0.12)), [0.07, 0.9, 0.8], body, colour));
                out.push(inst(MeshId::BEVEL_CUBE, up([-0.08, 3.72, -1.12]), quat_mul(q, yaw_q(-0.16)), [0.07, 0.86, 0.66], body, colour));
            }
            PropKind::Fence => {
                let mut prev: Option<[f32; 3]> = None;
                for j in 0..7 {
                    let pj = track.point_at(self.s + j as f32 * 1.1, self.lat);
                    let g = [pj[0], pj[1] - 0.06, pj[2]];
                    let c = if j % 2 == 0 { [1.0, 0.55, 0.75] } else { [1.0; 3] };
                    out.push(inst(MeshId::BEVEL_CUBE, add(g, [0.0, 0.55, 0.0]), q, [0.2, 1.1, 0.2], body, c));
                    if !shadows {
                        ground_disc(out, g, 0.22, mats[role::GRASS]);
                    }
                    if let Some(a) = prev {
                        // A rail runs from post to post, its ends 3 cm into
                        // each post (the rails from either side never meet
                        // inside it), its sides well in from the posts'.
                        let m = [(a[0] + g[0]) * 0.5, (a[1] + g[1]) * 0.5, (a[2] + g[2]) * 0.5];
                        let d = [g[0] - a[0], g[2] - a[2]];
                        let bq = yaw_q(d[0].atan2(d[1]));
                        let len = d[0].hypot(d[1]) - 0.2 + 0.06;
                        for y in [0.35f32, 0.75] {
                            out.push(inst(MeshId::CUBE, add(m, [0.0, y, 0.0]), bq, [0.08, 0.08, len], body, [1.0; 3]));
                        }
                    }
                    prev = Some(g);
                }
            }
            PropKind::Lollipop => {
                let (a, b) = SWIRLS[(h(83) * SWIRLS.len() as f32) as usize % SWIRLS.len()];
                out.push(inst(pole, up([0.0, 1.2, 0.0]), q, [0.14, 2.4, 0.14], white, [1.0; 3]));
                // Washers round the inner rings, as on the lollipop tree.
                let [w0, w1, w2] = POP_SWIRL;
                let washer = |k: usize| crate::propmesh::id(crate::propmesh::WASHERS + k);
                out.push(inst(washer(2), up([0.0, 2.85, 0.0]), sideways, [w0, 0.3, w0], body, a));
                out.push(inst(washer(3), up([0.0, 2.85, 0.0]), sideways, [w1, 0.3 + 2.0 * SWIRL_STEP, w1], body, b));
                out.push(inst(MeshId::CYLINDER, up([0.0, 2.85, 0.0]), sideways, [w2, 0.3 + 4.0 * SWIRL_STEP, w2], body, a));
                disc(out, 0.5);
            }
            PropKind::Gumdrops => {
                // Two or three domes in a row along the road, apart (their
                // flat bottoms never share a plane), each on its own patch
                // of the shoulder.
                let n = 2 + (h(101) * 2.0) as usize;
                for (j, &(along, out_m)) in [(0.0f32, 0.0f32), (2.0, 0.2), (-2.0, -0.2)].iter().enumerate().take(n) {
                    let r = 1.0 + 0.5 * hash01(seed * 3 + j as u32, 103);
                    let c = CANDY[(seed as usize + j * 2) % CANDY.len()];
                    let g = track.point_at(self.s + along, self.lat + out_m);
                    let at = [g[0], g[1] - 0.06, g[2]];
                    out.push(inst(MeshId::DOME, at, q, [r * 1.3, r * 1.5, r * 1.3], body, c));
                    if !shadows {
                        ground_disc(out, at, 0.6 * r, mats[role::GRASS]);
                    }
                }
            }
            PropKind::Grandstand => {
                if !shadows {
                    // A dark strip along the stand's foot on the flat
                    // shoulder (its back stands on the bank's slope, where
                    // the steps reach down into the ground).
                    // It lies on the ground as a decal, like the discs, in
                    // front of the steps (not under them). A flat one-sided
                    // top where the middle of a DISC_THICK slab would lie: a
                    // closed slab this thin showed its underside from afar.
                    let at = add(pos, rot(yaw, [-0.7 * sz[0], DISC_LIFT + DISC_THICK * 0.5, 0.0]));
                    out.push(inst(MeshId::PLANE, at, q, [1.4, STAND_STRIP_MARK, STAND_LEN + 1.0], mats[role::GRASS], [0.5, 0.6, 0.5]));
                }
                grandstand(self, track.meadow_y, mats, out);
            }
            PropKind::Mushroom => {
                disc(out, 1.3 * sz[0]);
                mushroom(self, mats, out);
            }
            PropKind::Cupcake => {
                disc(out, 3.6 * sz[0]);
                cupcake(self, mats, out);
            }
        }
    }
}

/// A sprinkle mushroom, per unit of size: the stem's width and height, the
/// cap's flat underside height, its radius and its height over that.
const MUSHROOM_STEM: [f32; 2] = [0.8, 2.0];
const MUSHROOM_CAP_Y: f32 = 1.8;
const MUSHROOM_CAP_R: f32 = 1.5;
const MUSHROOM_CAP_H: f32 = 1.2;
/// A dot's radius and height (per unit of size), and the facets of the cap
/// (band, segment of [`crate::propmesh::CAP`]) the five dots lie on.
const MUSHROOM_DOT: [f32; 2] = [0.22, 0.12];
const MUSHROOM_DOTS: [(u32, u32); 5] = [(1, 0), (1, 3), (1, 6), (2, 2), (2, 8)];
/// How deep a dot or window is sunk into the flat facet it lies on (m).
pub const DOT_SINK: f32 = 0.03;
/// Mushroom cap colours: cherry, bubblegum, lilac, orange.
const MUSHROOM_CAPS: [[f32; 3]; 4] = [[0.9, 0.12, 0.2], [1.0, 0.45, 0.7], [0.72, 0.5, 1.0], [1.0, 0.6, 0.15]];
/// Cream stems.
const MUSHROOM_STEM_C: [f32; 3] = [1.0, 0.93, 0.8];

/// A flat facet of a mushroom cap of size `s` (local, before the prop's
/// yaw): its middle and outward normal. The cap is
/// [`crate::propmesh::CAP`]'s dome, whose facets between two rings are flat
/// trapezoids.
fn cap_facet(s: f32, band: u32, seg: u32) -> ([f32; 3], [f32; 3]) {
    let c = [cap_corner(s, band, seg), cap_corner(s, band, seg + 1), cap_corner(s, band + 1, seg + 1), cap_corner(s, band + 1, seg)];
    let mid = [0, 1, 2].map(|i| (c[0][i] + c[1][i] + c[2][i] + c[3][i]) * 0.25);
    let d1 = [0, 1, 2].map(|i| c[2][i] - c[0][i]);
    let d2 = [0, 1, 2].map(|i| c[3][i] - c[1][i]);
    let mut n = crate::draw::normalize3([d1[1] * d2[2] - d1[2] * d2[1], d1[2] * d2[0] - d1[0] * d2[2], d1[0] * d2[1] - d1[1] * d2[0]]);
    let out = [mid[0], mid[1] - MUSHROOM_CAP_Y * s, mid[2]];
    if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] < 0.0 {
        n = n.map(|v| -v);
    }
    (mid, n)
}

/// Vertex `k` of ring `r` of a mushroom cap of size `s` (local), as
/// [`evani_render::mesh::dome`] places it.
fn cap_corner(s: f32, r: u32, k: u32) -> [f32; 3] {
    let th = r as f32 / crate::propmesh::CAP_RINGS as f32 * core::f32::consts::FRAC_PI_2;
    let ph = k as f32 / crate::propmesh::CAP_SEGMENTS as f32 * core::f32::consts::TAU;
    [MUSHROOM_CAP_R * s * th.sin() * ph.cos(), (MUSHROOM_CAP_Y + MUSHROOM_CAP_H * th.cos()) * s, -MUSHROOM_CAP_R * s * th.sin() * ph.sin()]
}

/// Turn that takes +y to the unit vector `n`.
fn align_y(n: [f32; 3]) -> [f32; 4] {
    let l = n[0].hypot(n[2]);
    if l < 1e-6 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    quat_axis_angle([n[2] / l, 0.0, -n[0] / l], n[1].clamp(-1.0, 1.0).acos())
}

/// A small dome of `radius` and `height` lying on a flat facet at `mid`
/// with outward normal `n` (local), sunk [`DOT_SINK`] into it.
fn facet_dot(p: &Prop, mid: [f32; 3], n: [f32; 3], radius: f32, height: f32, m: Mat, c: [f32; 3]) -> MeshInstance {
    let at = add(p.pos, rot(p.yaw, [mid[0] - n[0] * DOT_SINK, mid[1] - n[1] * DOT_SINK, mid[2] - n[2] * DOT_SINK]));
    let q = quat_mul(yaw_q(p.yaw), align_y(n));
    inst(crate::propmesh::id(crate::propmesh::DOT), at, q, [2.0 * radius, 2.0 * height, 2.0 * radius], m, c)
}

/// A sprinkle mushroom: a cream stem into a candy cap, five white dots on
/// the cap's facets.
fn mushroom(p: &Prop, mats: &[Mat; 16], out: &mut Vec<MeshInstance>) {
    let body = mats[role::KART_BODY];
    let s = p.size[0];
    let q = yaw_q(p.yaw);
    let up = |o: [f32; 3]| add(p.pos, rot(p.yaw, o));
    let cap = MUSHROOM_CAPS[(hash01(p.seed, 41) * MUSHROOM_CAPS.len() as f32) as usize % MUSHROOM_CAPS.len()];
    let [w, tall] = MUSHROOM_STEM;
    out.push(inst(crate::propmesh::id(crate::propmesh::POLE), up([0.0, tall * s * 0.5, 0.0]), q, [w * s, tall * s, w * s], body, MUSHROOM_STEM_C));
    let r = MUSHROOM_CAP_R * s;
    out.push(inst(crate::propmesh::id(crate::propmesh::CAP), up([0.0, MUSHROOM_CAP_Y * s, 0.0]), q, [2.0 * r, 2.0 * MUSHROOM_CAP_H * s, 2.0 * r], body, cap));
    for (band, seg) in MUSHROOM_DOTS {
        let (mid, n) = cap_facet(s, band, seg);
        out.push(facet_dot(p, mid, n, MUSHROOM_DOT[0] * s, MUSHROOM_DOT[1] * s, mats[role::KERB_WHITE], [1.0; 3]));
    }
}

/// A cupcake house, per unit of size: the case's top width and height; the
/// frosting tiers (width, height, underside height); the cherry's width and
/// middle height; the door's width, height, depth and middle out from the
/// case's axis; the windows' turn from the door (radians), middle height and
/// width.
const CUPCAKE_CASE: [f32; 2] = [7.0, 3.6];
const CUPCAKE_TIERS: [[f32; 3]; 3] = [[8.2, 1.7, 3.45], [5.6, 1.5, 4.4], [2.8, 1.0, 5.6]];
const CUPCAKE_CHERRY: [f32; 2] = [1.3, 7.05];
const CUPCAKE_DOOR: [f32; 4] = [1.5, 2.3, 0.8, 2.95];
const CUPCAKE_WINDOWS: [f32; 3] = [core::f32::consts::FRAC_PI_3, 2.4, 1.0];
/// Case and frosting colour pairs: pink and vanilla, mint and strawberry,
/// sky and vanilla, lilac and strawberry, lemon and chocolate.
const CUPCAKE_LOOKS: [([f32; 3], [f32; 3]); 5] = [
    ([1.0, 0.6, 0.78], [1.0, 0.97, 0.92]),
    ([0.5, 0.9, 0.75], [1.0, 0.6, 0.76]),
    ([0.55, 0.78, 1.0], [1.0, 0.97, 0.92]),
    ([0.8, 0.66, 1.0], [1.0, 0.6, 0.76]),
    ([1.0, 0.85, 0.45], [0.55, 0.32, 0.2]),
];

/// A facet of a cupcake case of size `s` (local): the point on its middle
/// line at height `y` and its outward normal, for the facet facing `a`
/// (radians from local +x towards +z; the door's facet faces +z, at pi/2).
fn case_facet(s: f32, a: f32, y: f32) -> ([f32; 3], [f32; 3]) {
    let [w, tall] = CUPCAKE_CASE;
    let half = (core::f32::consts::PI / crate::propmesh::WRAPPER_FACETS as f32).cos();
    let foot = crate::propmesh::WRAPPER_FOOT;
    let r = (foot + (0.5 - foot) * y / (tall * s)) * w * s * half;
    let n = crate::draw::normalize3([a.cos(), -(0.5 - foot) * w * half / tall, a.sin()]);
    ([r * a.cos(), y, r * a.sin()], n)
}

/// A cupcake house: a pleated case (one facet straight at the door), three
/// tiers of frosting, a cherry with a stalk, a chocolate door and two round
/// lit windows.
fn cupcake(p: &Prop, mats: &[Mat; 16], out: &mut Vec<MeshInstance>) {
    let body = mats[role::KART_BODY];
    let s = p.size[0];
    let q = yaw_q(p.yaw);
    let up = |o: [f32; 3]| add(p.pos, rot(p.yaw, o));
    let (case, frosting) = CUPCAKE_LOOKS[(hash01(p.seed, 43) * CUPCAKE_LOOKS.len() as f32) as usize % CUPCAKE_LOOKS.len()];
    // The case mesh's facets face +x plus half a facet: turned half a facet
    // back, one faces the door (+z).
    let half_facet = core::f32::consts::PI / crate::propmesh::WRAPPER_FACETS as f32;
    let [w, tall] = CUPCAKE_CASE;
    out.push(inst(crate::propmesh::id(crate::propmesh::WRAPPER), p.pos, quat_mul(q, yaw_q(-half_facet)), [w * s, tall * s, w * s], body, case));
    let cap = crate::propmesh::id(crate::propmesh::CAP);
    for (k, [tw, th, ty]) in CUPCAKE_TIERS.into_iter().enumerate() {
        let shade = if k == 1 { 0.92 } else { 1.0 };
        out.push(inst(cap, up([0.0, ty * s, 0.0]), q, [tw * s, 2.0 * th * s, tw * s], body, frosting.map(|c| c * shade)));
    }
    let [cw, cy] = CUPCAKE_CHERRY;
    out.push(inst(crate::propmesh::id(crate::propmesh::CHERRY), up([0.0, cy * s, 0.0]), q, [cw * s; 3], body, [0.88, 0.08, 0.15]));
    // The stalk leans out of the cherry's top.
    let lean = 0.35f32;
    let stalk = 0.8 * s;
    let foot = cy * s + 0.3 * cw * s;
    out.push(inst(
        crate::propmesh::id(crate::propmesh::POLE),
        up([lean.sin() * stalk * 0.5, foot + lean.cos() * stalk * 0.5, 0.0]),
        quat_mul(q, quat_axis_angle([0.0, 0.0, 1.0], -lean)),
        [0.1 * s, stalk, 0.1 * s],
        body,
        [0.35, 0.5, 0.2],
    ));
    let [dw, dh, dd, dz] = CUPCAKE_DOOR;
    out.push(inst(MeshId::BEVEL_CUBE, up([0.0, dh * s * 0.5, dz * s]), q, [dw * s, dh * s, dd * s], body, [0.42, 0.22, 0.12]));
    let [turn, wy, ww] = CUPCAKE_WINDOWS;
    for a in [core::f32::consts::FRAC_PI_2 - turn, core::f32::consts::FRAC_PI_2 + turn] {
        let (mid, n) = case_facet(s, a, wy * s);
        out.push(facet_dot(p, mid, n, ww * s * 0.5, 0.18 * s, body, [1.0, 0.9, 0.5]));
    }
}

/// A grandstand's back wall's front face, out from the stand's road edge (m).
const STAND_WALL_X: f32 = 6.6;
/// The roof's tilt (radians, down to the back) and thickness (m).
const STAND_ROOF_TILT: f32 = 0.14;
const STAND_ROOF_T: f32 = 0.28;
/// The roof's front edge, out from the stand's road edge (m; negative is
/// over the shoulder in front of the steps).
const STAND_ROOF_FRONT: f32 = -0.6;
/// The roof posts: their centre (in front of the steps and their icing,
/// which stands 7 cm proud) and radius (m).
const STAND_POST_X: f32 = -0.3;
const STAND_POST_R: f32 = 0.16;

/// A grandstand roof plank's placement: the plane the roof always had
/// (centre 3.5 m out at 6.3 m up), cut to run from [`STAND_ROOF_FRONT`] to
/// 5 mm short of the back wall.
struct StandRoof {
    cx: f32,
    cy: f32,
    depth: f32,
}

impl StandRoof {
    fn new() -> StandRoof {
        let (c, s) = (STAND_ROOF_TILT.cos(), STAND_ROOF_TILT.sin());
        let back = STAND_WALL_X - 0.005;
        let cx = (STAND_ROOF_FRONT + back) * 0.5;
        StandRoof { cx, cy: 6.3 + (3.5 - cx) * STAND_ROOF_TILT.tan(), depth: (back - STAND_ROOF_FRONT - STAND_ROOF_T * s) / c }
    }

    /// The roof's underside height at `x` out from the road edge.
    fn under(&self, x: f32) -> f32 {
        self.cy - (x - self.cx) * STAND_ROOF_TILT.tan() - STAND_ROOF_T * 0.5 / STAND_ROOF_TILT.cos()
    }
}

/// A stepped gingerbread stand along the track: three iced steps rising
/// away from the road, a back wall, posts and a pink-and-white striped
/// roof sloping down to the back wall, and (when `size[1]` > 0) three rows of
/// gumdrop people in candy colours.
fn grandstand(p: &Prop, meadow_y: f32, mats: &[Mat; 16], out: &mut Vec<MeshInstance>) {
    let body = mats[role::KART_BODY];
    let white = mats[role::KERB_WHITE];
    let side = p.size[0];
    let q = yaw_q(p.yaw);
    let at = |o: [f32; 3]| add(p.pos, rot(p.yaw, [o[0] * side, o[1], o[2]]));
    let l = STAND_LEN;
    // The steps and back wall reach down to just over the meadow (the
    // stand's back is over the bank's slope), so no gap shows under them.
    // Plain boxes: a bevel's chamfer along 20 m would slope the step tops
    // away under the crowd at the ends.
    let foot = meadow_y + 0.005 - p.pos[1];
    for r in 0..3 {
        let rf = r as f32;
        let top = (rf + 1.0) * 1.1;
        out.push(inst(MeshId::CUBE, at([1.1 + rf * 2.2, (top + foot) * 0.5, 0.0]), q, [2.2, top - foot, l], body, GINGER));
        // The icing lies on the step's top (touching, not sunk into it) and
        // stands 7 cm proud of its front and past each end: those faces stay
        // clear of the step's by more than depth precision needs at 150 m.
        out.push(inst(MeshId::CUBE, at([0.08 + rf * 2.2, top + 0.07, 0.0]), q, [0.3, 0.14, l + 0.14], white, [1.0; 3]));
    }
    // The back wall rises above the roof's low back edge like a backboard.
    out.push(inst(MeshId::CUBE, at([STAND_WALL_X + 0.3, (6.8 + foot) * 0.5, 0.0]), q, [0.6, 6.8 - foot, l + 0.6], body, [GINGER[0] * 0.8, GINGER[1] * 0.8, GINGER[2] * 0.8]));
    // The striped roof slopes down to the back, from over the posts to the
    // back wall's front face, which it meets (it doesn't run into the wall).
    // Candy-white posts hold up its front: in front of the steps (clear of
    // the icing), up to just under it, one at every other plank seam, so
    // every plank rests on a post. Each post is drawn just before the
    // planks it holds.
    let roof = StandRoof::new();
    let slope = quat_mul(q, quat_axis_angle([0.0, 0.0, 1.0], -STAND_ROOF_TILT * side));
    let n = 8;
    let plank = (l + 1.0) / n as f32;
    let post = |k: usize, out: &mut Vec<MeshInstance>| {
        let top = roof.under(STAND_POST_X + STAND_POST_R) - 0.005;
        let z = (k as f32 / n as f32 - 0.5) * (l + 1.0);
        out.push(inst(MeshId::CYLINDER, at([STAND_POST_X, top * 0.5, z]), q, [STAND_POST_R * 2.0, top, STAND_POST_R * 2.0], white, [1.0; 3]));
    };
    for k in 0..n {
        if k % 2 == 0 {
            post(k, out);
        }
        let z = ((k as f32 + 0.5) / n as f32 - 0.5) * (l + 1.0);
        let c = if k % 2 == 0 { [1.0, 0.45, 0.7] } else { [1.0; 3] };
        out.push(inst(MeshId::BEVEL_CUBE, at([roof.cx, roof.cy, z]), slope, [roof.depth, STAND_ROOF_T, plank], body, c));
    }
    post(n, out);
    if p.size[1] <= 0.0 {
        return;
    }
    for r in 0..3 {
        let rf = r as f32;
        let top = (rf + 1.0) * 1.1;
        for k in 0..CROWD_PER_ROW {
            let seed = p.seed * 64 + r * 16 + k;
            let z = ((k as f32 + 0.5) / CROWD_PER_ROW as f32 - 0.5) * (l - 1.0) + 0.3 * (hash01(seed, 3) - 0.5);
            let c = CANDY[(hash01(seed, 5) * 5.0) as usize % 5];
            let tall = 0.95 + 0.25 * hash01(seed, 7);
            out.push(inst(MeshId::CONE, at([1.2 + rf * 2.2, top + tall * 0.5, z]), q, [0.85, tall, 0.85], body, c));
            out.push(inst(MeshId::LOW_SPHERE, at([1.2 + rf * 2.2, top + tall + 0.22, z]), q, [0.62; 3], body, [1.0, 0.86, 0.72]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track::from_json(include_str!("../data/track.json")).unwrap()
    }

    #[test]
    fn layout_is_the_same_every_time() {
        let t = track();
        assert_eq!(layout(&t, 7.0, false), layout(&t, 7.0, false));
    }

    #[test]
    fn the_hill_ring_closes_the_horizon() {
        let t = track();
        let props = layout(&t, 7.0, true);
        let c = centre(&t);
        // Every 5 degrees, some hill of the ring covers that bearing.
        for d in 0..72 {
            let a = d as f32 * 5.0f32.to_radians();
            let covered = props.iter().filter(|p| p.kind == PropKind::Hill).any(|p| {
                let b = (p.pos[2] - c[2]).atan2(p.pos[0] - c[0]);
                let r = ((p.pos[0] - c[0]).powi(2) + (p.pos[2] - c[2]).powi(2)).sqrt();
                let half = (p.size[0] * 0.5 / r).atan();
                let mut diff = (a - b).rem_euclid(core::f32::consts::TAU);
                if diff > core::f32::consts::PI {
                    diff = core::f32::consts::TAU - diff;
                }
                diff < half
            });
            assert!(covered, "gap in the hill ring at {} degrees", d * 5);
        }
        // The ring stands outside every part of the track and the meadow.
        let inner = HILL_RING[0].0 - 10.0;
        for s in t.samples.iter() {
            assert!(((s.pos[0] - c[0]).powi(2) + (s.pos[2] - c[2]).powi(2)).sqrt() < inner);
        }
    }

    /// From the title orbit (its highest point) the hills reach up to within
    /// 1 degree of the horizon in every direction the camera can see: the
    /// review's world-edge check fails sky more than 1.5 degrees under the
    /// horizon right above solid world.
    #[test]
    fn hills_reach_the_title_orbits_horizon() {
        let t = track();
        let props = layout(&t, 7.0, true);
        let hills: Vec<&Prop> = props.iter().filter(|p| matches!(p.kind, PropKind::Hill | PropKind::Mountain)).collect();
        let b = t.bounds;
        let c = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
        let r = ((b[2] - b[0]).max(b[3] - b[1])) * 0.62;
        // As crate::draw::orbit_path: radius 0.62 of the track's size, height
        // 0.42 of the radius, bobbing 6 m.
        let eye_y = r * 0.42 + 6.0;
        // Dome height at a ground point (an ellipsoid cap of the hill's size).
        let dome = |h: &Prop, x: f32, z: f32| {
            let (dx, dz) = (x - h.pos[0], z - h.pos[2]);
            let (s, co) = h.yaw.sin_cos();
            let (lx, lz) = (dx * co - dz * s, dx * s + dz * co);
            let k = 1.0 - (lx / (h.size[0] * 0.5)).powi(2) - (lz / (h.size[2] * 0.5)).powi(2);
            if k > 0.0 { h.pos[1] + h.size[1] * k.sqrt() } else { f32::MIN }
        };
        let mut worst = f32::MAX;
        for step in 0..72 {
            let a = step as f32 / 72.0 * core::f32::consts::TAU;
            let eye = [c[0] + r * a.cos(), eye_y, c[1] + r * a.sin()];
            let view = (c[1] - eye[2]).atan2(c[0] - eye[0]);
            // The camera's horizontal field of view at 16:9 and 55 degrees.
            for j in -24..=24 {
                let phi = view + (j as f32 * 2.0).to_radians();
                let (dx, dz) = (phi.cos(), phi.sin());
                let mut best = f32::MIN;
                let mut d = 20.0;
                while d < 990.0 {
                    let (x, z) = (eye[0] + dx * d, eye[2] + dz * d);
                    for h in &hills {
                        let top = dome(h, x, z);
                        if top > f32::MIN {
                            best = best.max(((top - eye[1]) / d).atan().to_degrees());
                        }
                    }
                    d += 4.0;
                }
                worst = worst.min(best);
            }
        }
        assert!(worst > -1.0, "sky shows {:.2} degrees under the title orbit's horizon", -worst);
    }

    #[test]
    fn grandstands_stand_clear_of_the_road_and_props() {
        let t = track();
        let props = layout(&t, 7.0, false);
        let stands: Vec<&Prop> = props.iter().filter(|p| p.kind == PropKind::Grandstand).collect();
        assert_eq!(stands.len(), 2);
        for st in &stands {
            assert!(st.lat.abs() >= t.half_width + 7.0);
            assert!(st.size[1] > 0.0, "crowd on");
        }
        for p in props.iter().filter(|p| matches!(p.kind, PropKind::Flag | PropKind::Fence | PropKind::Lollipop | PropKind::Gumdrops | PropKind::SweetSign)) {
            for st in &stands {
                assert!((p.s - st.s).abs() >= STAND_LEN * 0.5, "a prop inside a stand at s {}", p.s);
            }
        }
        // Low keeps the stands, not the crowd.
        assert!(layout(&t, 7.0, true).iter().filter(|p| p.kind == PropKind::Grandstand).all(|p| p.size[1] <= 0.0));
    }

    /// The eight corners of a box instance (cube or bevel cube, unit size).
    fn box_corners(m: &MeshInstance) -> Vec<[f32; 3]> {
        let [x, y, z, w] = m.rotation;
        let rotate = |v: [f32; 3]| {
            // v + 2w(q x v) + 2 q x (q x v)
            let c1 = [y * v[2] - z * v[1], z * v[0] - x * v[2], x * v[1] - y * v[0]];
            let c2 = [y * c1[2] - z * c1[1], z * c1[0] - x * c1[2], x * c1[1] - y * c1[0]];
            [v[0] + 2.0 * (w * c1[0] + c2[0]), v[1] + 2.0 * (w * c1[1] + c2[1]), v[2] + 2.0 * (w * c1[2] + c2[2])]
        };
        let mut out = Vec::new();
        for sx in [-0.5f32, 0.5] {
            for sy in [-0.5f32, 0.5] {
                for sz in [-0.5f32, 0.5] {
                    let r = rotate([sx * m.scale[0], sy * m.scale[1], sz * m.scale[2]]);
                    out.push([m.position[0] + r[0], m.position[1] + r[1], m.position[2] + r[2]]);
                }
            }
        }
        out
    }

    /// Geocheck's stand findings (critic round 2): the roof ran into the
    /// back wall, the icing sat 5 cm into each step, the posts stood in the
    /// first step, and the roof hung in the air between its two end posts.
    /// Built at the origin facing +x (both sides), the parts now only touch.
    #[test]
    fn a_grandstands_parts_touch_without_running_into_each_other() {
        let mats = crate::draw::resolve_materials(&evani_core::plugin::LookData::default());
        for side in [1.0f32, -1.0] {
            let p = Prop::new(PropKind::Grandstand, [0.0, 0.0, 0.0], 0.0, [side, 0.0, 0.0], 500);
            let mut out = Vec::new();
            grandstand(&p, -1.0, &mats, &mut out);
            // Out from the road edge (x times the side), lowest and highest.
            let span = |m: &MeshInstance| {
                let c = box_corners(m);
                let xs = c.iter().map(|v| v[0] * side);
                let ys = c.iter().map(|v| v[1]);
                (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max), ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max))
            };
            // Steps are 2.2 m deep, the icing 0.14 m tall.
            let steps: Vec<_> = out.iter().filter(|m| m.mesh == MeshId::CUBE && m.scale[0] == 2.2).collect();
            let icing: Vec<_> = out.iter().filter(|m| m.mesh == MeshId::CUBE && m.scale[1] == 0.14).collect();
            let wall = out.iter().find(|m| m.mesh == MeshId::CUBE && m.scale[2] > STAND_LEN + 0.5).unwrap();
            let roof: Vec<_> = out.iter().filter(|m| m.mesh == MeshId::BEVEL_CUBE).collect();
            let posts: Vec<_> = out.iter().filter(|m| m.mesh == MeshId::CYLINDER).collect();
            assert_eq!((steps.len(), icing.len(), roof.len(), posts.len()), (3, 3, 8, 5));
            // Icing lies on its step.
            for (s, i) in steps.iter().zip(&icing) {
                assert!((span(i).2 - span(s).3).abs() < 1e-4, "icing bottom {} vs step top {}", span(i).2, span(s).3);
            }
            // The roof meets the wall's front face and doesn't reach in.
            let wall_front = span(wall).0;
            for r in &roof {
                let (_, back, _, _) = span(r);
                assert!(back <= wall_front && back > wall_front - 0.01, "roof back {back} vs wall {wall_front}");
            }
            // Posts stand in front of the steps and the icing, up to just
            // under the roof, and every plank rests on one.
            let icing_front = icing.iter().map(|m| span(m).0).fold(f32::MAX, f32::min);
            let r = StandRoof::new();
            for p in &posts {
                let c = p.position[0] * side;
                assert!(c + STAND_POST_R < icing_front - 0.05, "post at {c} vs icing at {icing_front}");
                let top = p.position[1] + p.scale[1] * 0.5;
                let under = r.under(c + STAND_POST_R);
                assert!(top < under && top > under - 0.02, "post top {top} vs roof underside {under}");
                assert!(r.under(c - STAND_POST_R) - top < 0.15, "touching");
                assert!(c - STAND_POST_R > STAND_ROOF_FRONT + 0.05, "under the roof");
            }
            for k in &roof {
                let (z0, z1) = (k.position[2] - k.scale[2] * 0.5, k.position[2] + k.scale[2] * 0.5);
                assert!(posts.iter().any(|p| (p.position[2] - z0).abs() < 1e-3 || (p.position[2] - z1).abs() < 1e-3), "a plank at {} with no post", k.position[2]);
            }
        }
    }

    fn of(props: &[Prop], kind: PropKind) -> Vec<Prop> {
        props.iter().filter(|p| p.kind == kind).copied().collect()
    }

    /// Critic round 3 (R3): the infield was open grass from above. Medium
    /// and up fill it with mushrooms and a few cupcake houses; every tier has
    /// a ring of cupcake houses out on the meadow (look bible (d) rows 9, 10).
    #[test]
    fn mushrooms_and_cupcakes_fill_the_infield_and_ring_the_meadow() {
        let t = track();
        let full = layout(&t, 7.0, false);
        let thin = layout(&t, 7.0, true);
        let c = centre(&t);
        let mush = of(&full, PropKind::Mushroom);
        let cups = of(&full, PropKind::Cupcake);
        let (inner, ring): (Vec<Prop>, Vec<Prop>) = cups.iter().partition(|p| inside_loop(&t, p.pos[0], p.pos[2]));
        assert!(mush.len() >= 15 && mush.len() <= MUSHROOMS, "{} mushrooms", mush.len());
        assert!(inner.len() >= 2 && inner.len() <= INFIELD_CUPCAKES, "{} infield cupcakes", inner.len());
        // Mountains and trees take some of the ring's slots.
        assert!(ring.len() >= 8, "{} cupcakes on the ring", ring.len());
        for p in mush.iter().chain(&inner) {
            assert!(!on_bank(&t, 7.0, p.pos, foot(p.kind, p.size[0]) + 2.0), "{:?} at {:?} on the bank", p.kind, p.pos);
        }
        let (near, far, _) = CUPCAKE_RING;
        for p in &ring {
            let r = (p.pos[0] - c[0]).hypot(p.pos[2] - c[2]);
            assert!(r >= near - 0.01 && r <= far + 0.01, "a ring cupcake {r} m out");
            // Inside the mountain ranges' geocheck tag (240 m out).
            assert!(r + foot(p.kind, p.size[0]) < 240.0);
        }
        // Each stands on open meadow, clear of every tree, tuft and other
        // mushroom or cupcake.
        for (i, p) in full.iter().enumerate().filter(|(_, p)| matches!(p.kind, PropKind::Mushroom | PropKind::Cupcake)) {
            let others: Vec<Prop> = full.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, q)| *q).collect();
            assert!(meadow_clear(&t, 7.0, &others, p.pos, foot(p.kind, p.size[0])), "{:?} at {:?} is crowded", p.kind, p.pos);
        }
        // And out of the podium's view.
        let site = crate::finale::podium_site(&t, 7.0);
        assert!(mush.iter().chain(&inner).all(|p| !in_podium_view(&site, p.kind, p.size[0], p.pos)));
        // Low: no mushrooms and no infield cupcakes, every other slot of the
        // same ring.
        assert!(of(&thin, PropKind::Mushroom).is_empty());
        let kept: Vec<Prop> = ring.iter().filter(|p| (p.seed - 3800) % 2 == 0).copied().collect();
        assert!(kept.len() >= 4, "{} cupcakes on Low's ring", kept.len());
        assert_eq!(of(&thin, PropKind::Cupcake), kept);
    }

    /// Geocheck: a cupcake house is one object (12 m at most) of parts under
    /// 10 m (a structure stands alone); the door stands proud of the case,
    /// its back inside it; the windows lie within their facets; each tier
    /// of frosting stands on the one below with its underside inside it.
    #[test]
    fn a_cupcake_house_holds_together() {
        let mats = crate::draw::resolve_materials(&evani_core::plugin::LookData::default());
        for s in [0.9f32, 1.2] {
            let p = Prop::new(PropKind::Cupcake, [3.0, 1.0, -2.0], 0.7, [s, 0.0, 0.0], 3801);
            let mut out = Vec::new();
            cupcake(&p, &mats, &mut out);
            assert!(out.iter().all(|m| m.scale.iter().all(|&v| v < 10.0)));
            // Its height (to the stalk's tip) and width (the first tier).
            let tall = CUPCAKE_CHERRY[1] * s + 0.3 * CUPCAKE_CHERRY[0] * s + 0.8 * s;
            let top = out.iter().map(|m| m.position[1] - p.pos[1]).fold(f32::MIN, f32::max);
            assert!(top < tall, "a part {top} m up");
            assert!(tall < 12.0 && CUPCAKE_TIERS[0][0] * s < 10.0, "{tall} m tall");
            // The door: front 10 cm proud of its facet at the top, back
            // inside the case at the foot, its back corners inside the
            // neighbouring facets.
            let [dw, dh, dd, dz] = CUPCAKE_DOOR;
            let top = case_facet(s, core::f32::consts::FRAC_PI_2, dh * s).0[2];
            assert!((dz + dd * 0.5) * s - top >= 0.1, "door front {} vs facet {top}", (dz + dd * 0.5) * s);
            let back = (dz - dd * 0.5) * s;
            for a in [core::f32::consts::FRAC_PI_2, core::f32::consts::FRAC_PI_3, core::f32::consts::FRAC_PI_2 + core::f32::consts::FRAC_PI_6] {
                let (mid, n) = case_facet(s, a, 0.0);
                for x in [dw * 0.5 * s, -dw * 0.5 * s] {
                    let d = (x - mid[0]) * n[0] + (back - mid[2]) * n[2];
                    assert!(d < -0.02, "door corner {x} outside the facet at {a}");
                }
            }
            // Windows: their rim inside the facet (the facet's half width at
            // the window's lowest point is wider than the window).
            let [_, wy, ww] = CUPCAKE_WINDOWS;
            let [w, h] = CUPCAKE_CASE;
            let foot_r = crate::propmesh::WRAPPER_FOOT;
            let r_low = (foot_r + (0.5 - foot_r) * (wy - ww * 0.5) / h) * w * s;
            let half = r_low * (core::f32::consts::PI / crate::propmesh::WRAPPER_FACETS as f32).sin();
            assert!(half > ww * 0.5 * s + 0.05);
            assert!((wy + ww * 0.5) * s < h * s);
            // Tiers: the underside of each lies inside the one below (its
            // flat facets are within cos(18) cos(15) of the round shape).
            for k in 1..3 {
                let [w0, h0, y0] = CUPCAKE_TIERS[k - 1];
                let [w1, _, y1] = CUPCAKE_TIERS[k];
                let dy = (y1 - y0) / h0;
                let inner = w0 * 0.5 * (1.0 - dy * dy).sqrt() * 0.9;
                assert!(w1 * 0.5 < inner, "tier {k} overhangs the one below");
            }
            // The first tier's underside is under the case's top; the cherry
            // sits into the top tier.
            assert!(CUPCAKE_TIERS[0][2] < h);
            let [_, t3h, t3y] = CUPCAKE_TIERS[2];
            assert!(CUPCAKE_CHERRY[1] - CUPCAKE_CHERRY[0] * 0.5 < t3y + t3h - 0.1);
        }
    }

    /// A mushroom's five dots lie on flat facets of its cap, sunk
    /// [`DOT_SINK`], their rims inside the facet, turned to face out.
    #[test]
    fn a_mushrooms_dots_lie_on_its_cap() {
        let mats = crate::draw::resolve_materials(&evani_core::plugin::LookData::default());
        let s = 1.3;
        let p = Prop::new(PropKind::Mushroom, [10.0, 0.5, 4.0], 1.1, [s, 0.0, 0.0], 3005);
        let mut out = Vec::new();
        mushroom(&p, &mats, &mut out);
        let dots: Vec<&MeshInstance> = out.iter().filter(|m| m.mesh == crate::propmesh::id(crate::propmesh::DOT)).collect();
        assert_eq!(dots.len(), 5);
        for (m, (band, seg)) in dots.iter().zip(MUSHROOM_DOTS) {
            let (mid, n) = cap_facet(s, band, seg);
            assert!(n[1] > 0.0);
            let c = [cap_corner(s, band, seg), cap_corner(s, band, seg + 1), cap_corner(s, band + 1, seg + 1), cap_corner(s, band + 1, seg)];
            for e in 0..4 {
                let (a, b) = (c[e], c[(e + 1) % 4]);
                // The facet is flat: its corners lie in the plane through mid.
                let off = (a[0] - mid[0]) * n[0] + (a[1] - mid[1]) * n[1] + (a[2] - mid[2]) * n[2];
                assert!(off.abs() < 1e-4, "facet not flat: {off}");
                // The rim stays inside: each edge is further from the
                // middle than the dot's radius.
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let am = [mid[0] - a[0], mid[1] - a[1], mid[2] - a[2]];
                let k = (am[0] * ab[0] + am[1] * ab[1] + am[2] * ab[2]) / (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]);
                let f = [a[0] + ab[0] * k, a[1] + ab[1] * k, a[2] + ab[2] * k];
                let d = ((mid[0] - f[0]).powi(2) + (mid[1] - f[1]).powi(2) + (mid[2] - f[2]).powi(2)).sqrt();
                assert!(d > MUSHROOM_DOT[0] * s + 0.02, "dot ({band}, {seg}) over the facet edge: {d}");
            }
            // Placed at the facet's middle, DOT_SINK in, turned to its normal.
            let want = add(p.pos, rot(p.yaw, [mid[0] - n[0] * DOT_SINK, mid[1] - n[1] * DOT_SINK, mid[2] - n[2] * DOT_SINK]));
            assert!((0..3).all(|i| (m.position[i] - want[i]).abs() < 1e-4));
            let [x, y, z, w] = m.rotation;
            // The turned +y: the rotation matrix's second column.
            let up = [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)];
            let nw = rot(p.yaw, n);
            assert!((0..3).all(|i| (up[i] - nw[i]).abs() < 1e-4), "{up:?} vs {nw:?}");
        }
    }

    #[test]
    fn every_bend_has_a_sign_and_candy_canes() {
        let t = track();
        let props = layout(&t, 7.0, false);
        let signs = props.iter().filter(|p| p.kind == PropKind::SweetSign).count();
        let canes = props.iter().filter(|p| p.kind == PropKind::CandyCane).count();
        assert!(signs >= 6, "{signs} signs");
        // Four canes a bend, less the few that would stand past the
        // shoulder's edge on the inside of a hairpin.
        assert!(canes + 4 >= signs * 4 && canes <= signs * 4 + 4, "{canes} canes for {signs} signs");
    }
}
