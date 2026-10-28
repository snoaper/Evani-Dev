//! The `Frame` system: chase cameras (split-screen views for two players,
//! an orbit over the track on the title screen), screen shake and the FOV
//! kick when those effects are on, the speed-line drive params, every mesh
//! (track, scenery, karts, items), the sprite minimap, the HUD resource and
//! the sounds.

use evani_core::camera::{CameraPath, CameraPose};
use evani_core::audio::AudioMixer;
use evani_core::ecs::World;
use evani_core::effects::EffectsStatus;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::LookData;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_render::atlas::cell;
use evani_render::math::{quat_axis_angle, quat_mul};
use evani_render::mesh::{BUILTIN_MESH_COUNT, Mesh};
use evani_render::{MeshId, MeshInstance, MeshLibrary, RenderWorld, Sprite, View};

use crate::hud::RaceHud;
use crate::race::{Phase, Race, RaceEvent, RaceView};
use crate::sfx::{Sfx, Sounds};
use crate::track::{Track, hash01};
use crate::{KartKit, Tune};

/// Material roles the kit asks the look for, with the colour it wants when
/// the look lacks the role (the instance is then tinted to match).
pub const ROLES: [(&str, [f32; 3], bool); 16] = [
    ("road", [0.42, 0.38, 0.52], false),
    ("kerb_red", [0.95, 0.18, 0.28], false),
    ("kerb_white", [0.95, 0.95, 0.95], false),
    ("grass", [0.32, 0.78, 0.32], false),
    ("mountain_pink", [0.95, 0.58, 0.72], false),
    ("mountain_mint", [0.55, 0.9, 0.75], false),
    ("mountain_lilac", [0.7, 0.6, 0.95], false),
    ("tree", [0.2, 0.68, 0.36], false),
    ("trunk", [0.55, 0.35, 0.2], false),
    ("kart_body", [1.0, 1.0, 1.0], false),
    ("wheel", [0.12, 0.1, 0.15], false),
    ("item_box", [0.45, 0.9, 1.0], true),
    ("boost_flame", [1.0, 0.55, 0.1], true),
    ("spark", [0.6, 0.8, 1.0], true),
    ("goo", [0.55, 0.85, 0.2], false),
    ("bomb", [0.16, 0.13, 0.26], false),
];

/// Role indices into [`ROLES`].
pub mod role {
    pub const ROAD: usize = 0;
    pub const KERB_RED: usize = 1;
    pub const KERB_WHITE: usize = 2;
    pub const GRASS: usize = 3;
    pub const MOUNTAIN_PINK: usize = 4;
    pub const MOUNTAIN_MINT: usize = 5;
    pub const MOUNTAIN_LILAC: usize = 6;
    pub const TREE: usize = 7;
    pub const TRUNK: usize = 8;
    pub const KART_BODY: usize = 9;
    pub const WHEEL: usize = 10;
    pub const ITEM_BOX: usize = 11;
    pub const BOOST_FLAME: usize = 12;
    pub const SPARK: usize = 13;
    pub const GOO: usize = 14;
    pub const BOMB: usize = 15;
}

/// A resolved role: the look material and the tint that makes it read as
/// the role's colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat {
    pub index: u32,
    pub tint: [f32; 3],
}

/// Resolve every role against a look.
pub fn resolve_materials(look: &LookData) -> [Mat; 16] {
    let glow = look.materials.iter().position(|m| m.name == "glow");
    let mut out = [Mat {
        index: 0,
        tint: [1.0; 3],
    }; 16];
    for (r, (name, want, emissive)) in ROLES.iter().enumerate() {
        if let Some(i) = look.materials.iter().position(|m| m.name == *name) {
            out[r] = Mat {
                index: i as u32,
                tint: [1.0; 3],
            };
            continue;
        }
        let i = match (emissive, glow) {
            (true, Some(g)) => g,
            _ => 0,
        };
        let base = look.materials.get(i).map_or([1.0; 4], |m| m.base_color);
        out[r] = Mat {
            index: i as u32,
            tint: [
                want[0] / base[0].max(0.05),
                want[1] / base[1].max(0.05),
                want[2] / base[2].max(0.05),
            ],
        };
    }
    out
}

/// The other karts as the chase camera sees them (a footprint and a height
/// each), everyone but kart `me`, as drawn this frame; `n` of them.
pub(crate) fn rival_blocks(race: &RaceView, me: usize, radius: f32, alpha: f32) -> ([crate::chasecam::Block; crate::race::MAX_KARTS], usize) {
    let none = crate::chasecam::Block::kart(crate::hull::Hull::new([0.0; 3], 0.0, radius), 0.0);
    let mut out = [none; crate::race::MAX_KARTS];
    let mut n = 0;
    for j in 0..race.karts.len() {
        if j == me || n == out.len() {
            continue;
        }
        out[n] = kart_block(race, j, radius, alpha);
        n += 1;
    }
    (out, n)
}

/// Kart `j` as the chase camera sees it (a footprint and a height), as
/// drawn this frame.
pub(crate) fn kart_block(race: &RaceView, j: usize, radius: f32, alpha: f32) -> crate::chasecam::Block {
    let k = &race.karts[j];
    let pos = lerp3(k.prev_pos, k.pos, alpha);
    let yaw = k.prev_yaw + crate::kart::wrap_angle(k.yaw - k.prev_yaw) * alpha;
    crate::chasecam::Block::kart(crate::hull::Hull::new(pos, yaw + k.slip, radius), pos[1] - k.hop)
}

/// How much of each kart to draw this frame (1 all, 0 none; the forward
/// shader's screen-door fade): rivals near a chase camera's eye, or (one
/// player) covering the player's kart from in front of the camera, fade out
/// (see [`crate::chasecam`]). A kart takes its lowest fade over the views.
fn kart_fades(race: &RaceView, poses: &[CameraPose; 2], views: &[[f32; 4]], radius: f32, alpha: f32) -> [f32; crate::race::MAX_KARTS] {
    let mut fade = [1.0f32; crate::race::MAX_KARTS];
    let n = race.karts.len().min(fade.len());
    let cover = race.players == 1;
    for (p, view) in views.iter().enumerate().take(race.players.min(2)) {
        let Some(i) = race.player_kart(p) else {
            continue;
        };
        let pose = &poses[p];
        let vp = evani_render::shaders::view_proj(pose, view[2] / view[3].max(1.0));
        let me = kart_block(race, i, radius, alpha);
        for j in (0..n).filter(|&j| j != i) {
            let mut f = [1.0];
            crate::chasecam::fades(&vp, pose.eye, &me, &[kart_block(race, j, radius, alpha)], cover, &mut f);
            fade[j] = fade[j].min(f[0]);
        }
    }
    fade
}

/// Per-frame state of the draw system (resource).
#[derive(Debug, Clone, Default)]
pub struct DrawState {
    /// Look the cached meshes were built for (name, material count).
    look_key: Option<(String, usize, u32, bool)>,
    mats: Option<[Mat; 16]>,
    /// Track and scenery instances, built once per look.
    statics: Statics,
    /// Library ids of the track meshes (see [`Statics`]).
    track_ids: Vec<MeshId>,
    /// Library ids of the item box body and decor, and the shape they were
    /// built for (rebuilt in place when a shape tunable changes).
    box_meshes: Option<(MeshId, MeshId)>,
    box_shape: Option<crate::itembox::BoxShape>,
    /// The road mesh's kept rings ([`crate::roadmarks::mesh_rings`]), set
    /// with the statics, and each item box's disc seat for the boxes where
    /// they are now (worked out again when a box moves).
    road_ring: Vec<f32>,
    box_seats: Vec<Option<BoxSeat>>,
    /// The item box decor's material: base, emissive, unlit.
    decor_mat: Option<([f32; 4], [f32; 3], bool)>,
    /// Chase camera per player: eye, target, FOV kick; reset per race.
    eye: [[f32; 3]; 2],
    target: [[f32; 3]; 2],
    /// Smoothed camera heading and anchor height per player.
    cam_yaw: [f32; 2],
    cam_y: [f32; 2],
    /// How far the eye is lifted over its resting height to see over rivals.
    lift: [f32; 2],
    kick: [f32; 2],
    cam_race: u64,
    sounds: Option<Sounds>,
    /// The engines, surface beds and the rivals' engines (looping voices).
    voices: crate::voices::Voices,
    /// Seconds to the next incoming-item cue, per player.
    warn: [f32; 2],
    /// Last written speed-line drive values.
    drive: [f32; 3],
    /// The active tier's triangle budget is small (see [`draw_dynamic`]).
    lean: bool,
    /// The tier draws a shadow map (without one, item boxes stand on dark
    /// discs and karts on blobs).
    shadows: bool,
    /// The active tier's per-frame triangle limit (sizes the confetti pool).
    budget: u32,
    /// Each tier's own shadow filter radius, by id, before
    /// [`soften_shadow_filter`] raised it.
    shadow_pcf_base: Vec<(String, Option<u32>)>,
    /// How much of each kart was drawn last frame (see [`kart_fades`]; set
    /// every chase frame, all 1 elsewhere).
    pub(crate) fade: [f32; crate::race::MAX_KARTS],
}

impl DrawState {
    /// Library ids of the item box body and decor, once registered.
    pub fn item_box_meshes(&self) -> Option<(MeshId, MeshId)> {
        self.box_meshes
    }

    /// Library ids of the meshes that are open on purpose: the item box's
    /// decor (a sheet of paint off each face), the grass blob and the shadow
    /// disc (one-sided tops, no underside), the lollipop swirl washers, the
    /// meadow's flat pieces, and the painted lines and start chequer
    /// (one-sided tops over the asphalt).
    pub fn open_meshes(&self) -> Vec<MeshId> {
        let meadow = (0..MEADOW_MESHES).filter_map(|k| self.track_ids.get(crate::trackmesh::COUNT + crate::propmesh::COUNT + k).copied());
        let paint = (crate::trackmesh::PAINT..crate::trackmesh::COUNT).filter_map(|k| self.track_ids.get(k).copied());
        let prop = |k: usize| self.track_ids.get(crate::trackmesh::COUNT + k).copied();
        // The grass blob and the shadow disc (one-sided tops) and the swirl
        // washers (open round their holes, which the next ring fills).
        let open_props = [crate::propmesh::BLOB, crate::propmesh::DISC].into_iter().chain(crate::propmesh::WASHERS..crate::propmesh::WASHERS + crate::propmesh::WASHER_COUNT).filter_map(prop);
        self.box_meshes.map(|(_, decor)| decor).into_iter().chain(open_props).chain(meadow).chain(paint).collect()
    }
}

pub(crate) fn yaw_q(yaw: f32) -> [f32; 4] {
    quat_axis_angle([0.0, 1.0, 0.0], yaw)
}

/// Rotate a local offset (x left, y up, z forward) by `yaw`.
pub(crate) fn rot(yaw: f32, o: [f32; 3]) -> [f32; 3] {
    let (s, c) = yaw.sin_cos();
    [o[0] * c + o[2] * s, o[1], -o[0] * s + o[2] * c]
}

pub(crate) fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

pub(crate) fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub(crate) fn inst(mesh: MeshId, pos: [f32; 3], rot: [f32; 4], scale: [f32; 3], m: Mat, color: [f32; 3]) -> MeshInstance {
    MeshInstance {
        mesh,
        position: pos,
        rotation: rot,
        scale,
        material: m.index,
        color: [m.tint[0] * color[0], m.tint[1] * color[1], m.tint[2] * color[2], 1.0],
    }
}

/// The orbit over the track used on the title screen (and by the demo's
/// bench path).
pub fn orbit_path(track: &Track) -> CameraPath {
    let b = track.bounds;
    let c = [(b[0] + b[2]) * 0.5, 0.0, (b[1] + b[3]) * 0.5];
    let r = ((b[2] - b[0]).max(b[3] - b[1])) * 0.62;
    let mut p = CameraPath::orbit(c, r, r * 0.42, 90.0).with_bob(6.0, 23.0);
    p.fov_y_deg = 55.0;
    p
}

pub use crate::scenery::{PROP_SPACING, THIN_BELOW_TRIANGLES as THIN_TREES_BELOW_TRIANGLES, THIN_TREE_STEP};

/// Grass slope from a shoulder down to the meadow (rise over run): about
/// 3.5 degrees where the road is low, so seen at a grazing angle the bend
/// is too slight for the outline pass to ink...
pub const BANK_SLOPE: f32 = 0.06;
/// ...but never longer than this (m), up to the steepest slope below.
pub const BANK_RUN_MAX: f32 = 14.0;
/// Steepest slope, under raised road (about 19 degrees).
pub const BANK_SLOPE_MAX: f32 = 0.35;
/// Height of the small step at the foot of a bank's slope (m): geocheck
/// found the slope's last metre fighting the meadow for depth.
pub const BANK_FOOT_STEP: f32 = 0.06;
/// The static scene: instances of the built-in meshes and of the track's
/// own meshes, the painted marks (decals) and the track meshes themselves.
/// An instance of track mesh `k` ([`crate::trackmesh`]) carries
/// `MeshId(BUILTIN_MESH_COUNT + k)` until the draw system registers the
/// meshes and maps it to the id the library gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Statics {
    pub instances: Vec<MeshInstance>,
    pub decals: Vec<MeshInstance>,
    pub meshes: Vec<Mesh>,
}

impl Statics {
    pub fn clear(&mut self) {
        self.instances.clear();
        self.decals.clear();
        self.meshes.clear();
    }

    /// Triangles of an instance's mesh (built-in or one of [`Self::meshes`]).
    pub fn triangles_of(&self, id: MeshId) -> u32 {
        usize::from(id.0)
            .checked_sub(BUILTIN_MESH_COUNT)
            .and_then(|k| self.meshes.get(k))
            .map_or_else(|| evani_render::mesh::triangles_of(id), Mesh::triangles)
    }

    /// An instance's mesh (built-in or one of [`Self::meshes`]).
    pub fn mesh_of(&self, id: MeshId) -> Option<Mesh> {
        match usize::from(id.0).checked_sub(BUILTIN_MESH_COUNT) {
            Some(k) => self.meshes.get(k).cloned(),
            None => evani_render::mesh::builtin(id),
        }
    }

    /// Triangles the statics draw (instances and decals).
    pub fn triangles(&self) -> u32 {
        self.instances.iter().chain(&self.decals).map(|m| self.triangles_of(m.mesh)).sum()
    }
}

/// Placeholder id of track mesh `k` (see [`Statics`]).
pub fn track_mesh_id(k: usize) -> MeshId {
    MeshId((BUILTIN_MESH_COUNT + k) as u16)
}

/// Build the static track and scenery instances. `max_triangles` is the
/// active tier's limit; under [`THIN_TREES_BELOW_TRIANGLES`] two trees in
/// three, the crowd and grass tufts are left out (see [`crate::scenery`]). `shadows` says the tier draws
/// a shadow map; without one, trees and props stand on dark ground discs.
pub fn build_statics(track: &Track, mats: &[Mat; 16], verge: f32, max_triangles: u32, shadows: bool, out: &mut Statics) {
    let thin = max_triangles < THIN_TREES_BELOW_TRIANGLES;
    build_statics_with(track, mats, verge, &crate::scenery::layout(track, verge, thin), shadows, out);
    add_road_marks(track, mats, thin, verge, out);
}

/// The road's wear (stains, repair patches, skid marks: decals), after the
/// statics are built; `lean` for a small triangle budget (Low).
pub fn add_road_marks(track: &Track, mats: &[Mat; 16], lean: bool, verge: f32, st: &mut Statics) {
    st.decals.extend(crate::roadmarks::road_marks(track, mats[role::ROAD], lean, verge));
}

/// [`build_statics`] with the props given: the game passes the props of its
/// prop entities ([`crate::scenery::props`]).
pub fn build_statics_with(track: &Track, mats: &[Mat; 16], verge: f32, props: &[crate::scenery::Prop], shadows: bool, out: &mut Statics) {
    build_statics_parts(track, mats, verge, props, props, shadows, out);
}

/// [`build_statics_with`] drawing `props` but laying the meadow's grass
/// patches out clear of `patches_clear_of` (the review dump draws the
/// ground alone, with the patches where the game draws them).
pub fn build_statics_parts(
    track: &Track,
    mats: &[Mat; 16],
    verge: f32,
    props: &[crate::scenery::Prop],
    patches_clear_of: &[crate::scenery::Prop],
    shadows: bool,
    st: &mut Statics,
) {
    st.clear();
    let hw = track.half_width;
    let sc = &track.scenery;
    let out = &mut st.instances;
    // The road, its grass shoulder and bank, the kerbs and candy-cane rails:
    // continuous meshes along the track (crate::trackmesh), drawn in place.
    st.meshes.extend(crate::trackmesh::build(track, verge));
    // Then the props' own low-poly meshes (crate::propmesh).
    st.meshes.extend(crate::propmesh::build());
    // Ground: the meadow mown in wide light and dark stripes over the
    // track's field, plain beyond (see [`meadow`]), drawn in place.
    let ground = sc.ground_size.max(crate::scenery::FAR_GROUND_M);
    st.meshes.extend(meadow(track, ground * 0.5));
    for (k, tint) in MEADOW_TINTS.iter().enumerate() {
        out.push(inst(meadow_mesh_id(k), [0.0; 3], NO_ROT, [1.0; 3], mats[role::GRASS], *tint));
    }
    let whole = |k: usize, m: Mat, c: [f32; 3]| inst(track_mesh_id(k), [0.0; 3], NO_ROT, [1.0; 3], m, c);
    use crate::trackmesh::{GRASS, PAINT, RED, ROAD, WHITE};
    out.push(whole(ROAD, mats[role::ROAD], [0.98; 3]));
    out.push(whole(GRASS, mats[role::GRASS], [1.0; 3]));
    out.push(whole(RED, mats[role::KERB_RED], [1.0; 3]));
    out.push(whole(WHITE, mats[role::KERB_WHITE], [1.0; 3]));
    // Painted lines lie on the asphalt as a decal: white edge lines and a
    // dashed centre line.
    st.decals.push(whole(PAINT, mats[role::KERB_WHITE], [0.92; 3]));
    // Start line: a chequered strip (decal meshes swept along the road like
    // the painted lines, so the lines start where the squares end) and a
    // gantry.
    for k in 0..crate::trackmesh::CHEQ_MESHES {
        let mat = if k % 2 == 0 { mats[role::KERB_WHITE] } else { mats[role::WHEEL] };
        st.decals.push(whole(crate::trackmesh::CHEQ + k, mat, [1.0; 3]));
    }
    let p0 = track.point_at(0.0, 0.0);
    let tan = track.tangent_at(0.0);
    let yaw = tan[0].atan2(tan[1]);
    let q = yaw_q(yaw);
    // The gantry's posts stand outside the rails, beyond any kart's reach
    // (karts drive the whole verge), their bases on the grass shoulder and
    // their tops just into the banner's flat underside.
    let post_lat = hw + verge + GANTRY_POST_OUT;
    let banner_y = p0[1] + 6.7;
    for side in [-1.0f32, 1.0] {
        let p = track.point_at(0.0, side * post_lat);
        let ground = p[1] - 0.06;
        // From inside the base (their bottoms never share its plane).
        let foot = ground + 0.1;
        let tall = banner_y - 0.75 + 0.05 - foot;
        out.push(inst(MeshId::BEVEL_CUBE, [p[0], foot + 0.5 * tall, p[2]], q, [0.9, tall, 0.9], mats[role::KERB_WHITE], [1.0; 3]));
        out.push(inst(MeshId::BEVEL_CUBE, [p[0], ground + 0.25, p[2]], q, [1.3, 0.5, 1.3], mats[role::KERB_RED], [1.0; 3]));
    }
    // The banner: plain boxes (a bevel's chamfer over a 31 m banner would
    // lift its underside off the posts), 1.5 m tall and 0.8 m deep, built
    // from solid pieces that meet face to face: a red strip along the top
    // and the bottom, a red block at each end, and between them a chequered
    // band of two rows of 16 squares, each square a box the banner's full
    // depth, so the band shows the same on both faces. (Squares painted on
    // the faces as decals showed through the banner from the far side: the
    // decal depth offset is worth more than its 0.8 m depth from afar.)
    let banner_len = 2.0 * post_lat + 0.8;
    let span = 2.0 * post_lat - 2.4;
    let red = mats[role::KERB_RED];
    let piece = |lat: f32, y: f32, size: [f32; 3], m: Mat| inst(MeshId::CUBE, add(p0, rot(yaw, [lat, banner_y - p0[1] + y, 0.0])), q, size, m, [1.0; 3]);
    for y in [-0.625f32, 0.625] {
        out.push(piece(0.0, y, [banner_len, 0.25, 0.8], red));
    }
    let end = 0.5 * (banner_len - span);
    for side in [-1.0f32, 1.0] {
        out.push(piece(side * 0.5 * (span + end), 0.0, [end, 1.0, 0.8], red));
    }
    let n_sq = 16;
    for c in 0..n_sq {
        for row in 0..2 {
            let lat = ((c as f32 + 0.5) / n_sq as f32 - 0.5) * span;
            let m = if (c + row) % 2 == 0 { mats[role::KERB_WHITE] } else { mats[role::WHEEL] };
            out.push(piece(lat, -0.25 + row as f32 * 0.5, [span / n_sq as f32, 0.5, 0.8], m));
        }
    }
    // Start grid: a painted bracket (a decal) in front of each of the 12 grid slots
    // (the same places `Race::reset` puts the karts): flat one-sided tops
    // lying along the road's slope (the grid is on a gentle hill), each
    // piece seated ROAD_CLEAR over the road mesh's triangles.
    let ring = crate::roadmarks::mesh_rings(track, verge);
    for g in 0..12 {
        let (row, col) = ((g / 2) as f32, g % 2);
        let s = track.length - (track.start_offset + row * 4.5 + col as f32 * 2.0);
        let lat = if col == 0 { hw * 0.45 } else { -hw * 0.45 };
        let paint = mats[role::KERB_WHITE];
        st.decals.push(crate::roadmarks::plate(track, &ring, s + 1.7, lat, 2.4, 0.22, paint, [0.92; 3]));
        for side in [-1.0f32, 1.0] {
            st.decals.push(crate::roadmarks::plate(track, &ring, s + 0.8, lat + side * 1.1, 0.22, 1.8, paint, [0.92; 3]));
        }
    }
    // Props; their shadow discs lie on the ground as decals.
    let mut parts = Vec::new();
    for prop in props {
        prop.instances(track, mats, shadows, &mut parts);
    }
    for m in parts {
        if crate::scenery::is_disc(&m) {
            st.decals.push(m);
        } else {
            out.push(m);
        }
    }
    grass_patches(track, verge, mats[role::GRASS], patches_clear_of, &mut st.decals);
}

/// Grass variation: soft lobed blobs in three greens painted on the meadow
/// (decals), each patch one to three overlapping blobs of one green. First a
/// few in the podium camera's foreground, then across the infield, along
/// both sides of the track, and a light scatter over the rest of the meadow.
/// None on the road's bank, in a mountain, under a tree, bush, tuft,
/// mushroom or cupcake (`clear_of`), on the podium's ground, or over a patch
/// of another green (two decals of different colours in one plane would
/// fight for depth; blobs of one green may overlap).
fn grass_patches(track: &Track, verge: f32, grass: Mat, clear_of: &[crate::scenery::Prop], out: &mut Vec<MeshInstance>) {
    let meadow = track.meadow_y;
    let site = crate::finale::podium_site(track, verge);
    let eye = crate::finale::camera_eye(&site);
    // Candidates (seed, x, z, size of the main blob, kinds of patch: 1 for a
    // single blob, 3 for one to three), in the order they are
    // laid: earlier ones win the room.
    let mut cand: Vec<(u32, f32, f32, f32, u32)> = Vec::new();
    // The podium shot's bottom third: 4 to 10.5 m in front of the camera,
    // from a little left of the podium to well right of it (the wide shot
    // looks right of the podium, the tall one straight at it).
    let a_pod = (site.pos[0] - eye[0]).atan2(site.pos[2] - eye[2]);
    let fwd = crate::finale::podium_camera(&site, 16.0 / 9.0);
    let a_fwd = (fwd.target[0] - eye[0]).atan2(fwd.target[2] - eye[2]);
    let turn = (a_fwd - a_pod + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU) - core::f32::consts::PI;
    for k in 0..PODIUM_PATCHES {
        let seed = 5000 + k;
        let h = |salt: u32| hash01(seed, salt);
        let a = a_pod + turn * (-1.0 + 3.0 * (k as f32 + h(3)) / PODIUM_PATCHES as f32);
        let d = 4.0 + 6.5 * h(5);
        cand.push((seed, eye[0] + d * a.sin(), eye[2] + d * a.cos(), 1.8 + 1.6 * h(7), 1));
    }
    // The infield: a jittered grid inside the track's loop.
    let b = track.bounds;
    let (nx, nz) = (((b[2] - b[0]) / GRASS_CELL).ceil() as u32, ((b[3] - b[1]) / GRASS_CELL).ceil() as u32);
    for iz in 0..nz {
        for ix in 0..nx {
            let seed = 5100 + iz * 64 + ix;
            let h = |salt: u32| hash01(seed, salt);
            let x = b[0] + (ix as f32 + 0.5) * GRASS_CELL + (h(3) - 0.5) * GRASS_CELL * 0.7;
            let z = b[1] + (iz as f32 + 0.5) * GRASS_CELL + (h(5) - 0.5) * GRASS_CELL * 0.7;
            if h(9) < 0.7 && crate::scenery::inside_loop(track, x, z) {
                cand.push((seed, x, z, 4.0 + 6.0 * h(7), 3));
            }
        }
    }
    // Along the track, past the foot of its bank, on either side.
    for k in 0..TRACK_PATCHES {
        let seed = 9000 + k;
        let h = |salt: u32| hash01(seed, salt);
        let s = (k as f32 + h(3)) / TRACK_PATCHES as f32 * track.length;
        let side = if h(5) < 0.5 { 1.0 } else { -1.0 };
        let p = track.point_at(s, side * (track.half_width + verge + 9.0 + 22.0 * h(9)));
        cand.push((seed, p[0], p[2], 4.0 + 6.0 * h(7), 3));
    }
    // A light scatter over the rest of the meadow.
    let half = track.scenery.ground_size * 0.5 - 10.0;
    for i in 0..SCATTER_PATCHES {
        let h = |salt: u32| hash01(i, salt);
        cand.push((i, (h(107) * 2.0 - 1.0) * half, (h(109) * 2.0 - 1.0) * half, 5.0 + 7.0 * h(113), 3));
    }
    // (x, z, reach, green) of each patch laid.
    let mut laid: Vec<(f32, f32, f32, usize)> = Vec::new();
    for (seed, x, z, size, kinds) in cand {
        let h = |salt: u32| hash01(seed, salt);
        // The main blob and up to two smaller ones overlapping its rim.
        let mut blobs = vec![([x, z], size, h(127) * core::f32::consts::TAU)];
        let extra = (h(131) * kinds as f32) as u32;
        for j in 0..extra {
            let a = h(137 + j) * core::f32::consts::TAU;
            let r = size * (0.32 + 0.12 * h(149 + j));
            let sz = size * (0.5 + 0.2 * h(151 + j));
            blobs.push(([x + r * a.cos(), z + r * a.sin()], sz, h(157 + j) * core::f32::consts::TAU));
        }
        let reach = blobs.iter().map(|(c, sz, _)| (c[0] - x).hypot(c[1] - z) + 0.5 * sz).fold(0.0f32, f32::max);
        let mut tone = (h(163) * 3.0) as usize % 3;
        if (site.pos[0] - x).hypot(site.pos[2] - z) < PODIUM_GRASS_CLEAR + reach {
            continue;
        }
        if !crate::scenery::meadow_clear(track, verge, clear_of, [x, meadow, z], reach) {
            continue;
        }
        // Touching patches of one green only: take the green of those it
        // touches, and leave it out if they differ.
        let mut touched = laid.iter().filter(|l| (l.0 - x).hypot(l.1 - z) < l.2 + reach + 0.3).map(|l| l.3);
        if let Some(first) = touched.next() {
            if touched.any(|g| g != first) {
                continue;
            }
            tone = first;
        }
        if laid.len() as u32 >= GRASS_PATCHES {
            break;
        }
        laid.push((x, z, reach, tone));
        for (c, sz, yaw) in blobs {
            let depth = sz * (0.75 + 0.25 * h(167));
            out.push(inst(crate::propmesh::id(crate::propmesh::BLOB), [c[0], meadow + 0.001, c[1]], yaw_q(yaw), [sz, BLOB_THICK, depth], grass, GRASS_TONES[tone]));
        }
    }
}

/// Most grass patches on the meadow, and the candidates of each kind (those
/// on the road's bank, in a mountain, under a tree or by a patch of another
/// green are left out).
pub const GRASS_PATCHES: u32 = 80;
const PODIUM_PATCHES: u32 = 12;
const TRACK_PATCHES: u32 = 70;
const SCATTER_PATCHES: u32 = 50;
/// Infield grid cell for grass patches (m).
const GRASS_CELL: f32 = 22.0;
/// Grass patches keep this far (m, plus their reach) from the podium's
/// middle: its blocks and the drivers' feet stand on plain meadow.
pub const PODIUM_GRASS_CLEAR: f32 = 7.0;
/// A grass blob's thickness (m): not [`crate::scenery::DISC_THICK`], so the
/// look's shadow tint never takes it for a shadow disc.
pub const BLOB_THICK: f32 = 0.012;
/// The three greens of the grass patches, as tints of the grass material
/// (the meadow itself is tinted (0.82, 0.9, 0.8)): darker, lighter and a
/// warm yellow-green (the look bible's grass dark, light and lip).
pub const GRASS_TONES: [[f32; 3]; 3] = [[0.68, 0.8, 0.66], [0.96, 1.04, 0.9], [1.0, 1.07, 0.74]];

/// Slots of the meadow's meshes (see [`meadow`]), after the track's and the
/// props': the light stripes, the dark stripes and the plain meadow round
/// the striped field.
pub const MEADOW_LIGHT: usize = 0;
pub const MEADOW_DARK: usize = 1;
pub const MEADOW_PLAIN: usize = 2;
pub const MEADOW_MESHES: usize = 3;
/// Width of one mown stripe (m), and how far the striped field reaches past
/// the track's bounds (m).
pub const STRIPE_W: f32 = 9.0;
pub const STRIPE_MARGIN: f32 = 30.0;
/// The meadow meshes' tints of the grass material, by slot: the look
/// bible's light and dark stripes (#7cd161 / #5fbb55), and the plain meadow
/// beyond the field halfway between them.
pub const MEADOW_TINTS: [[f32; 3]; MEADOW_MESHES] = [[0.92, 1.0, 0.86], [0.72, 0.82, 0.7], [0.82, 0.9, 0.8]];

/// Placeholder id of meadow mesh `k` (see [`Statics`]).
pub fn meadow_mesh_id(k: usize) -> MeshId {
    track_mesh_id(crate::trackmesh::COUNT + crate::propmesh::COUNT + k)
}

/// The meadow at `track.meadow_y`, a square `half` m each way from the
/// middle of the track's bounds, as flat open meshes in world space, by
/// [`MEADOW_LIGHT`] to [`MEADOW_PLAIN`]: stripes [`STRIPE_W`] wide running
/// along the start straight over the track's bounds and [`STRIPE_MARGIN`],
/// light and dark in turn, and the plain meadow round them. Neighbouring
/// pieces share their edges' corners (every point comes from one function
/// of the same numbers), so no crack opens between them and none lies over
/// another. About `4n + 10` triangles for `n` stripes.
pub fn meadow(track: &Track, half: f32) -> [Mesh; MEADOW_MESHES] {
    let b = track.bounds;
    let c = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
    let y = track.meadow_y;
    let t = track.tangent_at(0.0);
    let l = t[0].hypot(t[1]).max(1e-6);
    // Along the stripes (the start straight) and across them.
    let f = [t[0] / l, t[1] / l];
    let r = [f[1], -f[0]];
    // The bounds in the stripes' frame (symmetric about the middle).
    let (mut reach_u, mut reach_v) = (0.0f32, 0.0f32);
    for (x, z) in [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])] {
        let d = [x - c[0], z - c[1]];
        reach_u = reach_u.max((d[0] * r[0] + d[1] * r[1]).abs());
        reach_v = reach_v.max((d[0] * f[0] + d[1] * f[1]).abs());
    }
    let n = ((2.0 * (reach_u + STRIPE_MARGIN) / STRIPE_W).ceil() as usize).max(2);
    let u: Vec<f32> = (0..=n).map(|k| (k as f32 - n as f32 * 0.5) * STRIPE_W).collect();
    let (u0, un) = (u[0], u[n]);
    let (v0, v1) = (-(reach_v + STRIPE_MARGIN), reach_v + STRIPE_MARGIN);
    let p = |u: f32, v: f32| [c[0] + u * r[0] + v * f[0], y, c[1] + u * r[1] + v * f[1]];
    let mut out: [Mesh; MEADOW_MESHES] = Default::default();
    let mut seen: [Vec<([u32; 2], u32)>; MEADOW_MESHES] = Default::default();
    let mut tri = |k: usize, t: [[f32; 3]; 3]| flat_tri(&mut out[k], &mut seen[k], t);
    for (i, w) in u.windows(2).enumerate() {
        let k = if i % 2 == 0 { MEADOW_LIGHT } else { MEADOW_DARK };
        let (a, b, cc, d) = (p(w[0], v0), p(w[1], v0), p(w[1], v1), p(w[0], v1));
        tri(k, [a, b, cc]);
        tri(k, [a, cc, d]);
    }
    // Beyond each end of the stripes: a fan over that end's corners.
    for (v, far) in [(v1, half), (v0, -half)] {
        let o = p(u0, far);
        for w in u.windows(2) {
            tri(MEADOW_PLAIN, [o, p(w[0], v), p(w[1], v)]);
        }
        tri(MEADOW_PLAIN, [o, p(un, v), p(un, far)]);
    }
    // Beside the field, out to the meadow's edge, with the field's corners.
    for (e, out_u) in [(u0, -half), (un, half)] {
        let (a, bb, cc, d, ee, ff) = (p(out_u, -half), p(e, -half), p(e, v0), p(e, v1), p(e, half), p(out_u, half));
        tri(MEADOW_PLAIN, [a, bb, cc]);
        tri(MEADOW_PLAIN, [a, cc, d]);
        tri(MEADOW_PLAIN, [a, d, ff]);
        tri(MEADOW_PLAIN, [ff, d, ee]);
    }
    out
}

/// Add the flat triangle `t` to `m` facing up, reusing vertices at the
/// same place (`seen`: a vertex's x and z bits and its index).
fn flat_tri(m: &mut Mesh, seen: &mut Vec<([u32; 2], u32)>, mut t: [[f32; 3]; 3]) {
    let ny = (t[1][2] - t[0][2]) * (t[2][0] - t[0][0]) - (t[1][0] - t[0][0]) * (t[2][2] - t[0][2]);
    if ny < 0.0 {
        t.swap(1, 2);
    }
    for q in t {
        let key = [q[0].to_bits(), q[2].to_bits()];
        let i = match seen.iter().find(|s| s.0 == key) {
            Some(s) => s.1,
            None => {
                let i = m.vertices.len() as u32;
                m.vertices.push([q[0], q[1], q[2], 0.0, 1.0, 0.0]);
                seen.push((key, i));
                i
            }
        };
        m.indices.push(i);
    }
}

/// Thickness of the painted marks drawn as decals (start strip, grid
/// brackets; the lines are the track's paint mesh): they lie centred on the
/// asphalt and the decal depth offset keeps them in front of it.
pub const DECAL_THICK: f32 = 2.0 * crate::trackmesh::PAINT_HALF;
/// Start gantry posts: lateral distance past the verge's outer edge (m),
/// clear of the rail.
pub const GANTRY_POST_OUT: f32 = 1.3;

/// HDR spark colour per drift tier.
pub fn tier_color(tier: u8) -> [f32; 3] {
    match tier {
        1 => [0.5, 1.2, 5.0],
        2 => [5.0, 2.0, 0.3],
        3 => [5.0, 0.8, 3.2],
        _ => [1.2, 1.2, 1.2],
    }
}

/// The bloom the old item box ribbons gave (fix round 1): their HDR colour
/// on the toon look's `item_box` material (unlit: base [0.5, 1, 1] times a
/// white tint, plus emissive [2.4, 1.4, 2.8]).
pub const RIBBON_RADIANCE: [f32; 3] = [2.9, 2.4, 3.8];
/// The toon look's `fx.bloom.threshold` (used when the bloom effect is off
/// and its param is not registered).
pub const LOOK_BLOOM_THRESHOLD: f32 = 2.0;
/// `fx.bloom.knee`'s default.
pub const BLOOM_KNEE: f32 = 0.5;
/// What the bloom bright pass kept of [`RIBBON_RADIANCE`] at the toon
/// look's threshold and the default knee (Rec. 709 luminance, see
/// [`bloom_kept`]): the old ribbons' glow, the unit of
/// `kit.kart_racer.item_box_glyph_glow`.
pub const RIBBON_BLOOM: f32 = 1.235;

/// What the bloom bright pass keeps of an HDR colour (the soft-knee
/// prefilter in `evani-fx`'s `bloom_bright.wgsl`, without the clamp).
pub fn bloom_kept(c: [f32; 3], threshold: f32, knee: f32) -> [f32; 3] {
    let lum = c[0].max(c[1]).max(c[2]);
    let knee = knee.max(1e-4);
    let soft = (lum - threshold + knee).clamp(0.0, 2.0 * knee);
    let soft = soft * soft / (4.0 * knee);
    let w = soft.max(lum - threshold) / lum.max(1e-4);
    c.map(|v| v * w)
}

/// Rec. 709 luminance.
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The instance colour that makes material (`base`, `emissive`, `unlit`)
/// show `target`: unlit materials add their emissive to tint times base.
pub fn paint_on(target: [f32; 3], base: [f32; 4], emissive: [f32; 3], unlit: bool) -> [f32; 3] {
    core::array::from_fn(|k| {
        let t = if unlit { target[k] - emissive[k] } else { target[k] };
        (t / base[k].max(0.05)).max(0.0)
    })
}

/// Material role the item box decor (the "?" and rivets) is painted with:
/// unlit, so it reads as clean white paint on every side of the box.
pub const ITEM_BOX_DECOR_ROLE: usize = role::SPARK;

/// How an item box looks and moves: the `kit.kart_racer.item_box_*`
/// tunables (live-tunable, baked like every other kit param). A solid
/// candy-coloured block with bevelled edges and a white "?" and four
/// rivets on every face ([`crate::itembox`]). The default lift (0.7 m)
/// keeps the spinning, bobbing box's lowest corner clear of the road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemBoxLook {
    pub size: f32,
    pub lift: f32,
    pub bob_m: f32,
    pub bob_speed: f32,
    pub spin_speed: f32,
    pub tilt: f32,
    /// The six box colours (linear).
    pub palette: [[f32; 3]; 6],
    /// Seconds per palette colour (0 = every box keeps its colour).
    pub color_cycle_s: f32,
    pub saturation: f32,
    pub brightness: f32,
    /// Bevel, "?" size and rivet size (the meshes are built from these).
    pub shape: crate::itembox::BoxShape,
    pub glyph_color: [f32; 3],
    /// Glyph and rivet glow in units of the old ribbons' bloom.
    pub glyph_glow: f32,
    pub pop_s: f32,
    pub shadow_radius: f32,
    pub shadow_shade: f32,
    /// The registered body and decor meshes (the draw system sets them);
    /// without them the box is a plain bevelled cube with no decor.
    pub meshes: Option<(MeshId, MeshId)>,
    /// The library id of the shadow disc mesh ([`crate::propmesh::DISC`]);
    /// its placeholder id until the draw system sets the registered one.
    pub disc: MeshId,
    /// Instance colour of the decor on [`ITEM_BOX_DECOR_ROLE`]'s material
    /// (see [`ItemBoxLook::paint_for`]).
    pub paint: [f32; 3],
}

impl ItemBoxLook {
    pub fn new(t: &Tune, palette: [[f32; 3]; 6], glyph_color: [f32; 3]) -> Self {
        let mut look = Self {
            size: t.item_box_size,
            lift: t.item_box_lift,
            bob_m: t.item_box_bob_m,
            bob_speed: t.item_box_bob_speed,
            spin_speed: t.item_box_spin_speed,
            tilt: t.item_box_tilt,
            palette,
            color_cycle_s: t.item_box_color_cycle_s,
            saturation: t.item_box_saturation,
            brightness: t.item_box_brightness,
            shape: crate::itembox::BoxShape {
                bevel: t.item_box_bevel,
                glyph_size: t.item_box_glyph_size,
                rivet: t.item_box_rivet_size,
            },
            glyph_color,
            glyph_glow: t.item_box_glyph_glow,
            pop_s: t.item_box_pop_s,
            shadow_radius: t.item_box_shadow_radius,
            shadow_shade: t.item_box_shadow_shade,
            meshes: None,
            disc: crate::propmesh::id(crate::propmesh::DISC),
            paint: [1.0; 3],
        };
        look.paint = look.glyph_radiance(LOOK_BLOOM_THRESHOLD);
        look
    }

    /// The HDR colour the "?" and rivets show: their colour at the bloom
    /// threshold plus `glyph_glow` times the old ribbons' bloom, so for
    /// white the bloom pass keeps `glyph_glow * RIBBON_BLOOM` (0.5 = half).
    pub fn glyph_radiance(&self, threshold: f32) -> [f32; 3] {
        let level = threshold + self.glyph_glow.max(0.0) * RIBBON_BLOOM;
        self.glyph_color.map(|c| c * level)
    }

    /// Set [`Self::paint`] for the decor material (`base`, `emissive`,
    /// `unlit`) at the active bloom threshold.
    pub fn paint_for(&mut self, base: [f32; 4], emissive: [f32; 3], unlit: bool, threshold: f32) {
        self.paint = paint_on(self.glyph_radiance(threshold), base, emissive, unlit);
    }

    /// Box `i`'s colour at `time`: its palette colour, held, then faded to
    /// the next over the last quarter of each `color_cycle_s`. Boxes start
    /// 1.37 palette steps apart, so neighbours never share a colour.
    pub fn color(&self, i: usize, time: f32) -> [f32; 3] {
        let cycle = self.color_cycle_s > 0.0;
        let ph = i as f32 * 1.37 + if cycle { time / self.color_cycle_s } else { 0.0 };
        let k = ph.floor();
        let f = if cycle { ((ph - k - 0.75) / 0.25).clamp(0.0, 1.0) } else { 0.0 };
        let f = f * f * (3.0 - 2.0 * f);
        let n = self.palette.len() as i64;
        let a = self.palette[(k as i64).rem_euclid(n) as usize];
        let b = self.palette[(k as i64 + 1).rem_euclid(n) as usize];
        let raw: [f32; 3] = core::array::from_fn(|c| a[c] + (b[c] - a[c]) * f);
        let grey = (raw[0] + raw[1] + raw[2]) / 3.0;
        raw.map(|c| ((grey + (c - grey) * self.saturation) * self.brightness).max(0.0))
    }
}

impl Default for ItemBoxLook {
    fn default() -> Self {
        Self::new(&Tune::default(), crate::ITEM_BOX_PALETTE, crate::ITEM_BOX_GLYPH_COLOR)
    }
}

/// Blob (contact) shadows for what moves, most important first, at most
/// [`BLOB_CAP`]: every kart (one disc each, up to [`BLOB_KARTS`], players
/// first, then the nearest rivals), then bombs and seekers within
/// [`BLOB_NEAR_M`] of a player. Item boxes stand on a dark disc of their
/// own instead (see [`draw_dynamic`]). The renderer only draws blobs when
/// the tier has no shadow map.
pub(crate) fn push_blobs(race: &RaceView, alpha: f32, rw: &mut RenderWorld) {
    let kart_pos = |i: usize| {
        let k = &race.karts[i];
        lerp3(k.prev_pos, k.pos, alpha)
    };
    let focus: Vec<[f32; 3]> = (0..race.players.min(2)).filter_map(|p| race.player_kart(p)).map(kart_pos).collect();
    let near = |p: [f32; 3]| {
        focus
            .iter()
            .map(|f| (f[0] - p[0]).powi(2) + (f[2] - p[2]).powi(2))
            .fold(f32::MAX, f32::min)
    };
    let mut karts: Vec<(f32, usize)> = (0..race.karts.len())
        .map(|i| (if race.karts[i].player.is_some() { -1.0 } else { near(kart_pos(i)) }, i))
        .collect();
    karts.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Every blob is a loop step for every pixel on the Low path, so keep
    // few: one disc per kart, then items and boxes only near a player.
    let mut left = BLOB_CAP;
    for (_, i) in karts.into_iter().take(BLOB_KARTS) {
        let k = &race.karts[i];
        let p = kart_pos(i);
        // One disc a little behind the kart's middle, so the shadow shows
        // round the rear wheels from the chase camera instead of hiding
        // under the tub; smaller while hopping.
        let r = 1.85 * (1.0 - (k.hop * 0.4).min(0.4));
        let f = k.forward();
        rw.draw_blob([p[0] - f[0] * 0.25, p[1] - k.hop, p[2] - f[1] * 0.25], r);
        left -= 1;
    }
    let close = |p: [f32; 3]| near(p) < BLOB_NEAR_M * BLOB_NEAR_M;
    for b in race.bombs.iter().filter(|b| close(b.pos)) {
        if left == 0 {
            return;
        }
        let g = race.track.locate(b.pos, b.seg).height;
        rw.draw_blob([b.pos[0], g, b.pos[2]], 0.7);
        left -= 1;
    }
    for s in race.seekers.iter().filter(|s| close(s.pos)) {
        if left == 0 {
            return;
        }
        let g = race.track.point_at(s.s, s.lateral)[1];
        rw.draw_blob([s.pos[0], g, s.pos[2]], 0.6);
        left -= 1;
    }
}

/// Most blob shadows the kit sends per frame (the renderer allows 32; each
/// one costs every pixel of the Low path a loop step, after the renderer's
/// early-outs): every kart plus a few items in flight.
pub const BLOB_CAP: usize = 16;
/// Most karts with a blob shadow: all of them (the field is at most
/// [`crate::race::MAX_KARTS`]), so no kart ever stands shadowless on Low.
pub const BLOB_KARTS: usize = crate::race::MAX_KARTS;
/// Items and boxes further than this from every player get no blob (m).
pub const BLOB_NEAR_M: f32 = 30.0;

/// Phase offset of a kart's particles, so karts don't puff in step.
fn particle_seed(k: &crate::kart::Kart) -> f32 {
    k.spec as f32 * 0.29
}

/// Drift sparks behind a sliding kart (its dust is drawn by the caller), drawn without any per-kart
/// state: each particle's age is the time's fraction phased by its index, so
/// a stream of sparks flies back along an arc from each rear wheel. `at` maps
/// a kart-local offset (x left, y up, z forward) to the world.
fn drift_particles(
    k: &crate::kart::Kart,
    t: &Tune,
    mats: &[Mat; 16],
    time: f32,
    at: &dyn Fn([f32; 3]) -> [f32; 3],
    q: [f32; 4],
    rw: &mut RenderWorld,
) {
    const SPARKS: usize = 7;
    let seed = particle_seed(k);
    let tier = k.tier(t);
    let c = tier_color(tier);
    let burst = if k.tier_flash < 0.25 { 1.5 } else { 1.0 };
    let size = (if tier > 0 { 0.13 } else { 0.08 }) * burst;
    // The trail curls to the outside of the slide.
    let out = -k.drift.signum();
    for (j, x) in [0.85f32, -0.85].iter().enumerate() {
        for n in 0..SPARKS {
            let age = (time * 3.1 + n as f32 / SPARKS as f32 + j as f32 * 0.37 + seed).fract();
            let fade = 1.0 - age;
            let z = -1.05 - age * 3.2;
            let y = 0.12 + age * 0.9 - age * age * 1.1;
            let xo = x + out * age * age * 1.4;
            let len = 0.3 + 0.5 * fade;
            rw.draw_mesh(inst(MeshId::CUBE, at([xo, y, z]), q, [size * fade + 0.03, size * fade + 0.03, len], mats[role::SPARK], c));
        }
        // A bright core at the wheel.
        let fl = 0.75 + 0.25 * (time * 37.0 + j as f32 * 1.7 + seed).sin();
        rw.draw_mesh(inst(MeshId::CUBE, at([*x, 0.15, -1.0]), quat_mul(q, yaw_q(time * 9.0)), [size * 2.0 * fl; 3], mats[role::SPARK], c));
    }
}

/// Boost: a trail of orange sparks streaming back from both exhausts (the
/// boost flame's colour), long streaks that thin out and drop as they age,
/// stateless like the drift sparks (each one's age is the time's fraction
/// phased by its index). Orange, so it never reads as a drift tier.
fn boost_trail(k: &crate::kart::Kart, mats: &[Mat; 16], time: f32, lean: bool, at: &dyn Fn([f32; 3]) -> [f32; 3], q: [f32; 4], rw: &mut RenderWorld) {
    let seed = particle_seed(k);
    if lean {
        low_boost_sparks(seed, mats, time, at, q, rw);
        return;
    }
    let per = 9;
    let (m, c) = (mats[role::SPARK], [3.0, 1.05, 0.12]);
    for (j, x) in [0.26f32, -0.26].iter().enumerate() {
        for n in 0..per {
            let age = (time * 2.6 + n as f32 / per as f32 + j as f32 * 0.31 + seed).fract();
            let fade = 1.0 - age;
            let sway = ((n * 7 + j * 3) as f32 + seed * 5.0).sin() * 0.28 * age;
            let z = -2.0 - age * 6.0;
            let y = 0.78 - age * 0.5 + 0.22 * (age * core::f32::consts::PI).sin();
            let len = 0.35 + 0.9 * fade;
            let w = 0.09 * fade + 0.03;
            rw.draw_mesh(inst(MeshId::CUBE, at([x * (1.0 + age * 2.2) + sway, y.max(0.12), z]), q, [w, w, len], m, c));
        }
    }
}

/// Sparks per exhaust in Low's boost trail.
pub const LOW_SPARKS: usize = 6;
/// How far behind the exhaust Low's boost sparks fly before they burn out (m):
/// a short trail that stays well in front of the chase camera.
pub const LOW_SPARK_REACH: f32 = 2.4;

/// Low's boost trail: a short spray of small tumbling sparks from each
/// exhaust. Low has no bloom (an unlit HDR orange clips to cream), so each
/// spark is a lit, ink-outlined candy-orange diamond (every third one yellow)
/// that shrinks as it flies back, drops and drifts out (stateless, phased by
/// time like the drift sparks). 12 triangles each, 144 per boosting kart.
fn low_boost_sparks(seed: f32, mats: &[Mat; 16], time: f32, at: &dyn Fn([f32; 3]) -> [f32; 3], q: [f32; 4], rw: &mut RenderWorld) {
    let body = mats[role::KART_BODY];
    for (j, x) in [0.26f32, -0.26].iter().enumerate() {
        for n in 0..LOW_SPARKS {
            let age = (time * 3.4 + n as f32 / LOW_SPARKS as f32 + j as f32 * 0.41 + seed).fract();
            let fade = 1.0 - age;
            let jitter = ((n * 5 + j * 3) as f32 * 1.7 + seed * 3.0).sin();
            let pos = [x * (1.0 + age * 1.6) + jitter * 0.22 * age, 0.78 + 0.32 * (age * core::f32::consts::PI).sin() - 0.45 * age * age, -1.95 - age * LOW_SPARK_REACH];
            let size = 0.07 + 0.17 * fade;
            let spin = quat_axis_angle([1.0, 0.0, 0.0], core::f32::consts::FRAC_PI_4 + age * 6.0 + jitter);
            let diamond = quat_mul(q, quat_mul(spin, quat_axis_angle([0.0, 0.0, 1.0], 0.615)));
            let c = if n % 3 == 1 { LOW_FLAME_YELLOW } else { LOW_FLAME_ORANGE };
            rw.draw_mesh(inst(MeshId::CUBE, at(pos), diamond, [size; 3], body, c));
        }
    }
}

/// Low's boost flame colours (lit `kart_body`, which is near-white): through
/// the toon look's tonemap the orange lands between sRGB (161, 62, 0) in the
/// shadow band and (255, 165, 0) in full sun, the yellow at about
/// (255, 226, 37), so neither clips to the cream an unlit HDR flame gives
/// without bloom.
pub const LOW_FLAME_ORANGE: [f32; 3] = [1.1, 0.2, 0.02];
pub const LOW_FLAME_YELLOW: [f32; 3] = [1.4, 0.95, 0.12];

/// Low's boost flames. With no bloom an unlit HDR flame clips to a flat
/// pale disc, so on Low each exhaust gets a lit, ink-outlined flame shape
/// instead: a yellow root ball at the exhaust and an orange cone leaning
/// back and up from it (it reads as a flame in a chase view seen from behind
/// and above, where a cone pointing straight back would be seen tip-on).
/// Two cones and two low spheres: 248 triangles per boosting kart. `fl` is
/// the flicker (about 0.7..1.3) that stretches the cone.
fn low_flames(fl: f32, mats: &[Mat; 16], at: &dyn Fn([f32; 3]) -> [f32; 3], q: [f32; 4], rw: &mut RenderWorld) {
    const TILT: f32 = 0.95;
    let body = mats[role::KART_BODY];
    let tilt = quat_mul(q, quat_axis_angle([1.0, 0.0, 0.0], -TILT));
    // The cone's axis (local +y) after the tilt: up and back.
    let (up, back) = (TILT.cos(), TILT.sin());
    for x in [0.26f32, -0.26] {
        let root = [x, 0.72, -1.82];
        let len = 0.95 * fl;
        let mid = [root[0], root[1] + up * len * 0.5, root[2] - back * len * 0.5];
        rw.draw_mesh(inst(MeshId::CONE, at(mid), tilt, [0.38, len, 0.38], body, LOW_FLAME_ORANGE));
        // Wider than the cone's base, so it rings the root in yellow.
        rw.draw_mesh(inst(MeshId::LOW_SPHERE, at(root), q, [0.48, 0.4, 0.48], body, LOW_FLAME_YELLOW));
    }
}

/// Dust puffs from the rear wheels: two small round grey-beige puffs per
/// wheel that grow a little, rise and shrink away (stateless, phased by time
/// like the sparks).
fn dust(k: &crate::kart::Kart, mats: &[Mat; 16], time: f32, at: &dyn Fn([f32; 3]) -> [f32; 3], rw: &mut RenderWorld) {
    const PUFFS: usize = 2;
    let seed = particle_seed(k);
    let tone = if k.on_road { [0.74, 0.7, 0.66] } else { [0.68, 0.6, 0.46] };
    for (j, x) in [0.85f32, -0.85].iter().enumerate() {
        for n in 0..PUFFS {
            let age = (time * 1.8 + n as f32 / PUFFS as f32 + j as f32 * 0.5 + seed * 0.7).fract();
            let s = (0.16 + age * 0.38) * (1.0 - age * age);
            rw.draw_mesh(inst(
                MeshId::SPHERE,
                at([x * (1.0 + age * 0.4), 0.18 + age * 0.5, -1.15 - age * 1.8]),
                [0.0, 0.0, 0.0, 1.0],
                [s, s * 0.8, s],
                mats[role::KERB_WHITE],
                tone,
            ));
        }
    }
}

/// Rotate `v` by the unit quaternion `q`.
pub(crate) fn quat_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [q[0], q[1], q[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let c = cross(u, t);
    [v[0] + q[3] * t[0] + c[0], v[1] + q[3] * t[1] + c[1], v[2] + q[3] * t[2] + c[2]]
}

fn scale3(c: [f32; 3], s: f32) -> [f32; 3] {
    [c[0] * s, c[1] * s, c[2] * s]
}

/// A procedural model's frame: parts are placed by a kart-local offset (x
/// left, y up, z forward), turned by the body rotation and squashed per axis.
struct Rig {
    pos: [f32; 3],
    q: [f32; 4],
    squash: [f32; 3],
}

impl Rig {
    fn at(&self, o: [f32; 3]) -> [f32; 3] {
        let s = self.squash;
        add(self.pos, quat_rotate(self.q, [o[0] * s[0], o[1] * s[1], o[2] * s[2]]))
    }

    #[allow(clippy::too_many_arguments)]
    fn part(&self, rw: &mut RenderWorld, mesh: MeshId, o: [f32; 3], lq: [f32; 4], size: [f32; 3], m: Mat, c: [f32; 3]) {
        let s = self.squash;
        rw.draw_mesh(inst(mesh, self.at(o), quat_mul(self.q, lq), [size[0] * s[0], size[1] * s[1], size[2] * s[2]], m, c));
    }
}

const NO_ROT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// What the rival parts need to know about one kart this frame
/// (`rivals.rs`).
#[cfg_attr(not(feature = "models"), allow(dead_code))]
pub(crate) struct RivalKart<'a> {
    pub k: &'a crate::kart::Kart,
    pub spec: &'a crate::ai::KartSpec,
    pub pos: [f32; 3],
    /// The heading with slip and spin-out, as the primitive kart is drawn.
    pub vyaw: f32,
    pub max_speed: f32,
    /// How much of the kart the chase camera lets through (1 = all).
    pub fade: f32,
}

#[cfg(feature = "models")]
pub(crate) use crate::rivals::RivalDraw;

/// Without the `models` module there are no model parts to draw: nothing is
/// ever acquired, so every kart is the primitive one.
#[cfg(not(feature = "models"))]
pub(crate) struct RivalDraw<'a>(core::marker::PhantomData<&'a ()>);

#[cfg(not(feature = "models"))]
impl<'a> RivalDraw<'a> {
    pub(crate) fn acquire(_world: &'a World, _on: bool, _scale: f32) -> Option<Self> {
        None
    }

    pub(crate) fn kart(&mut self, _kart: &RivalKart<'_>) -> Option<KartParts> {
        None
    }
}

/// One kart and its driver from bevelled boxes and cylinders, in cartoon
/// proportions (big rear tyres, low tub, oversized helmet). The wheels roll
/// with distance and the front pair steers; the body rolls out of a drift
/// and with steering, pitches up on a boost, stretches on a hop and squashes
/// on landing; the driver leans into the turn.
pub fn draw_kart(k: &crate::kart::Kart, colours: ([f32; 3], Option<[f32; 3]>), pos: [f32; 3], vyaw: f32, max_speed: f32, mats: &[Mat; 16], rw: &mut RenderWorld) {
    draw_kart_parts(k, colours, pos, vyaw, max_speed, KartParts::ALL, mats, rw);
}

/// Which groups of primitives [`draw_kart_parts`] draws. A rival built from a
/// recipe's model parts (`rivals.rs`) takes its chassis and wheels from the
/// models and keeps the driver, steering wheel and column as primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KartParts {
    /// Tub, nose, pods, bumper, engine, exhausts, wing and seat.
    pub chassis: bool,
    /// Steering column and wheel, and the driver.
    pub driver: bool,
    /// The four wheels.
    pub wheels: bool,
}

impl KartParts {
    pub(crate) const ALL: Self = Self { chassis: true, driver: true, wheels: true };
    /// The driver group only: the chassis and the wheels come from models.
    #[cfg(feature = "models")]
    pub(crate) const DRIVER: Self = Self { chassis: false, driver: true, wheels: false };
    /// The driver and the wheels: the chassis comes from models, the wheels
    /// stay primitive (the recipe names no wheel model).
    #[cfg(feature = "models")]
    pub(crate) const DRIVER_WHEELS: Self = Self { chassis: false, driver: true, wheels: true };
}

/// How a kart stands in the world: the body's rotation (heading `vyaw`, the
/// pitch up on a boost, the roll out of a drift or with the steering) and its
/// stretch and squash on a hop and a landing. The primitive kart and the
/// model parts both use it, so they lean and squash alike.
pub(crate) fn kart_pose(k: &crate::kart::Kart, vyaw: f32, max_speed: f32) -> ([f32; 4], [f32; 3]) {
    let kq = yaw_q(vyaw);
    let speed_frac = (k.speed.abs() / max_speed.max(0.1)).min(1.2);
    // Lean: positive roll about +z tips the top to the kart's right.
    let roll = if k.drift != 0.0 { k.drift * 0.13 } else { -k.steer_vis * 0.07 * speed_frac };
    let pitch = if k.boost > 0.0 { -0.07 } else { 0.0 };
    let stretch = if k.hop > 0.0 && k.vy > 0.0 { (k.vy / 4.0).clamp(0.0, 1.0) * 0.14 } else { 0.0 };
    let sq = k.squash;
    (
        quat_mul(kq, quat_mul(quat_axis_angle([1.0, 0.0, 0.0], pitch), quat_axis_angle([0.0, 0.0, 1.0], roll))),
        [1.0 + 0.1 * sq - 0.05 * stretch, 1.0 - 0.24 * sq + stretch, 1.0 + 0.08 * sq - 0.04 * stretch],
    )
}

/// [`draw_kart`] for the groups `parts` names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_kart_parts(k: &crate::kart::Kart, (col, suit): ([f32; 3], Option<[f32; 3]>), pos: [f32; 3], vyaw: f32, max_speed: f32, parts: KartParts, mats: &[Mat; 16], rw: &mut RenderWorld) {
    let body = mats[role::KART_BODY];
    let dark = mats[role::WHEEL];
    let white = mats[role::KERB_WHITE];
    let kq = yaw_q(vyaw);
    let (q, squash) = kart_pose(k, vyaw, max_speed);
    let rig = Rig { pos, q, squash };
    let lighter = [0.35 + col[0] * 0.65, 0.35 + col[1] * 0.65, 0.35 + col[2] * 0.65];
    let deeper = scale3(col, 0.72);
    let pitch_q = |a: f32| quat_axis_angle([1.0, 0.0, 0.0], a);
    let bc = MeshId::BEVEL_CUBE;
    if parts.chassis {
        // Tub, nose fairing, side pods, front bumper.
        rig.part(rw, bc, [0.0, 0.36, 0.0], NO_ROT, [1.3, 0.3, 2.3], body, col);
        rig.part(rw, bc, [0.0, 0.52, 0.92], pitch_q(0.22), [1.05, 0.34, 0.85], body, lighter);
        for x in [0.74f32, -0.74] {
            rig.part(rw, bc, [x, 0.46, -0.02], NO_ROT, [0.34, 0.38, 0.76], body, deeper);
        }
        rig.part(rw, bc, [0.0, 0.3, 1.42], NO_ROT, [1.6, 0.22, 0.28], white, [1.0; 3]);
        // Engine block, twin exhausts and a rear wing on two struts.
        rig.part(rw, bc, [0.0, 0.6, -0.98], NO_ROT, [0.95, 0.44, 0.5], white, [0.55, 0.55, 0.62]);
        for x in [0.26f32, -0.26] {
            rig.part(rw, MeshId::CYLINDER, [x, 0.78, -1.32], pitch_q(core::f32::consts::FRAC_PI_2 - 0.2), [0.2, 0.42, 0.2], white, [0.78, 0.78, 0.84]);
            rig.part(rw, bc, [x * 1.5, 0.98, -1.12], NO_ROT, [0.08, 0.42, 0.1], dark, [1.0; 3]);
        }
        rig.part(rw, bc, [0.0, 1.2, -1.18], pitch_q(-0.12), [1.6, 0.09, 0.42], body, col);
        // Seat: base and a raked back.
        rig.part(rw, bc, [0.0, 0.56, -0.3], NO_ROT, [0.72, 0.16, 0.62], dark, [1.0; 3]);
        rig.part(rw, bc, [0.0, 0.86, -0.62], pitch_q(-0.28), [0.74, 0.7, 0.16], dark, [1.0; 3]);
    }
    if parts.driver {
        // Steering column and wheel, turning with the steer.
        rig.part(rw, bc, [0.0, 0.78, 0.5], pitch_q(0.9), [0.08, 0.08, 0.5], dark, [1.0; 3]);
        let wheel_q = quat_mul(pitch_q(-1.0), quat_axis_angle([0.0, 1.0, 0.0], k.steer_vis * 0.9));
        rig.part(rw, MeshId::CYLINDER, [0.0, 0.98, 0.34], wheel_q, [0.46, 0.06, 0.46], dark, [1.0; 3]);
        // Driver: suit, arms to the wheel, helmet with a dark visor, leaning
        // into the turn.
        let lean = if k.drift != 0.0 { k.drift * 0.12 } else { -k.steer_vis * 0.09 };
        let lean_q = quat_axis_angle([0.0, 0.0, 1.0], -lean * 1.6);
        let suit = suit.unwrap_or([0.25 + col[0] * 0.5, 0.25 + col[1] * 0.5, 0.25 + col[2] * 0.5]);
        rig.part(rw, bc, [lean * 0.5, 1.0, -0.3], lean_q, [0.68, 0.64, 0.46], body, suit);
        for x in [0.31f32, -0.31] {
            rig.part(rw, bc, [x + lean * 0.4, 1.06, 0.06], pitch_q(0.28), [0.17, 0.17, 0.62], body, scale3(suit, 0.8));
        }
        let helmet = [0.35 + col[0] * 0.65, 0.35 + col[1] * 0.65, 0.35 + col[2] * 0.65];
        rig.part(rw, MeshId::SPHERE, [lean, 1.6, -0.24], lean_q, [0.84, 0.8, 0.88], body, helmet);
        rig.part(rw, bc, [lean, 1.6, 0.09], lean_q, [0.64, 0.25, 0.26], body, [0.1, 0.13, 0.26]);
        // A suit-coloured stripe over the top, front to back, and a round
        // crest: the helmet reads as a volume from the chase camera.
        rig.part(rw, MeshId::LOW_SPHERE, [lean, 1.61, -0.25], lean_q, [0.24, 0.84, 0.92], body, suit);
        rig.part(rw, MeshId::LOW_SPHERE, [lean, 2.02, -0.3], lean_q, [0.24, 0.24, 0.24], body, suit);
    }
    if parts.wheels {
        // Wheels: big rear tyres, smaller steering fronts, each a tyre, a light
        // rim and a hub bar that shows the roll. Wheels keep to the road
        // (no body lean or squash).
        let wheels = [(0.8f32, 0.95f32, 0.33f32, 0.32f32, true), (-0.8, 0.95, 0.33, 0.32, true), (0.84, -0.86, 0.42, 0.44, false), (-0.84, -0.86, 0.42, 0.44, false)];
        for (x, z, r, w, front) in wheels {
            let steer = if front { yaw_q(-k.steer_vis * 0.5) } else { NO_ROT };
            let roll_q = quat_axis_angle([1.0, 0.0, 0.0], k.wheel_roll * crate::kart::WHEEL_RADIUS / r);
            let wq = quat_mul(kq, quat_mul(steer, roll_q));
            let tyre_q = quat_mul(wq, quat_axis_angle([0.0, 0.0, 1.0], core::f32::consts::FRAC_PI_2));
            let c = add(pos, rot(vyaw, [x, r, z]));
            rw.draw_mesh(inst(MeshId::CYLINDER, c, tyre_q, [2.0 * r, w, 2.0 * r], dark, [1.0; 3]));
            rw.draw_mesh(inst(MeshId::CYLINDER, c, tyre_q, [1.2 * r, w + 0.04, 1.2 * r], white, [0.92; 3]));
            rw.draw_mesh(inst(MeshId::BEVEL_CUBE, c, wq, [w + 0.09, 1.25 * r, 0.24 * r], body, col));
        }
    }
}

/// Disc radii (m) an item box's [`BoxSeat`] is worked out for, up to the
/// largest the tunables allow (2 x 5 m x 1.7 / 2).
const SEAT_RADII: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.5];

/// Where an item box's shadow disc lies on the road mesh: tilted to the
/// road's own slope under the box, and lifted so its lowest point over the
/// road's triangles is [`crate::trackmesh::ROAD_CLEAR`] above them, for a
/// disc of each of [`SEAT_RADII`] (each lift covers every smaller disc
/// too). A flat disc on a sloped stretch dipped under the asphalt at its
/// uphill edge and flickered there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxSeat {
    /// The box position it was worked out for.
    pub at: [f32; 3],
    centre: [f32; 3],
    rot: [f32; 4],
    lift: [f32; SEAT_RADII.len()],
}

impl BoxSeat {
    /// The seat for a box at `at` (1 m over the road), on the road mesh
    /// with the kept rings `ring` ([`crate::roadmarks::mesh_rings`]); `None`
    /// off the road.
    pub fn new(track: &Track, ring: &[f32], at: [f32; 3]) -> Option<Self> {
        let g = [at[0], at[1] - 1.0, at[2]];
        let s = track.locate(g, 0).s;
        let h = |x: f32, z: f32| crate::trackmesh::road_top(track, ring, x, z, s);
        let c = h(g[0], g[2])?;
        let t = track.tangent_at(s);
        let (fwd, right) = ([t[0], 0.0, t[1]], [-t[1], 0.0, t[0]]);
        let p = |d: [f32; 3], k: f32| -> Option<[f32; 3]> {
            let (x, z) = (g[0] + d[0] * k, g[2] + d[2] * k);
            Some([x, h(x, z)?, z])
        };
        let (pf, pb, pr, pl) = (p(fwd, 0.5)?, p(fwd, -0.5)?, p(right, 0.5)?, p(right, -0.5)?);
        let (a, b) = ([pf[0] - pb[0], pf[1] - pb[1], pf[2] - pb[2]], [pr[0] - pl[0], pr[1] - pl[1], pr[2] - pl[2]]);
        let mut n = normalize3([a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]);
        if n[1] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        // The shortest turn from straight up to n.
        let q = [n[2], 0.0, -n[0], 1.0 + n[1]];
        let l = (q[0] * q[0] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-9);
        let rot = [q[0] / l, 0.0, q[2] / l, q[3] / l];
        let centre = [g[0], c, g[2]];
        let mut lift = [crate::trackmesh::ROAD_CLEAR; SEAT_RADII.len()];
        let mut most = f32::MIN;
        for (k, r) in SEAT_RADII.iter().enumerate() {
            let disc = MeshInstance { mesh: MeshId::PLANE, position: centre, rotation: rot, scale: [2.0 * r, 1.0, 2.0 * r], material: 0, color: [1.0; 4] };
            most = most.max(crate::roadmarks::seat_lift(track, ring, &disc, s, true).unwrap_or(crate::trackmesh::ROAD_CLEAR));
            lift[k] = most;
        }
        Some(Self { at, centre, rot, lift })
    }

    /// Put the disc `d` (its diameter in its x scale) on the seat.
    pub fn place(&self, d: &mut MeshInstance) {
        let r = 0.5 * d.scale[0];
        let k = SEAT_RADII.iter().position(|&s| s >= r).unwrap_or(SEAT_RADII.len() - 1);
        d.position = [self.centre[0], self.centre[1] + self.lift[k], self.centre[2]];
        d.rotation = self.rot;
    }
}

/// Item box `i` at `time`: a solid, lit candy block in one palette colour
/// at a time (the toon bands and ink show its bevels) with a white "?" and
/// four rivets on every face, painted unlit at a gentle glow (one decor
/// instance), hovering, bobbing and turning over its pick-up point.
/// Without a shadow map (`shadows` false) it casts a dark disc on the road
/// below it, on its `seat` when there is one (flat under the box if not).
/// Before the meshes are registered it is a plain bevelled cube.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_item_box(b: &crate::items::ItemBox, i: usize, look: &ItemBoxLook, seat: Option<&BoxSeat>, mats: &[Mat; 16], time: f32, shadows: bool, rw: &mut RenderWorld) {
    let grow = if b.hidden > 0.0 { (1.0 - b.hidden / look.pop_s.max(0.01)).clamp(0.0, 1.0) } else { 1.0 };
    if grow <= 0.0 {
        return;
    }
    let colour = look.color(i, time);
    let p = add(b.pos, [0.0, look.lift + look.bob_m * (time * look.bob_speed + i as f32).sin(), 0.0]);
    let q = quat_axis_angle([0.4 * look.tilt, 1.0, 0.3 * look.tilt], time * look.spin_speed + i as f32);
    let s = look.size * grow;
    if !shadows && look.shadow_radius > 0.0 && look.shadow_shade < 1.0 {
        // Box positions are 1 m over the road.
        let ground = [b.pos[0], b.pos[1] - 1.0, b.pos[2]];
        crate::scenery::shadow_disc(&mut rw.decals, ground, look.shadow_radius * s, mats[role::ROAD], [look.shadow_shade; 3]);
        // Drawn at run time: the disc mesh's registered id, not its placeholder.
        if let Some(d) = rw.decals.last_mut() {
            d.mesh = look.disc;
            if let Some(seat) = seat {
                seat.place(d);
            }
        }
    }
    let (body, decor) = match look.meshes {
        Some((body, decor)) => (body, Some(decor)),
        None => (MeshId::BEVEL_CUBE, None),
    };
    rw.draw_mesh(inst(body, p, q, [s; 3], mats[role::KART_BODY], colour));
    if let Some(decor) = decor {
        // The paint already allows for the material, so no role tint.
        let pc = look.paint;
        rw.draw_mesh(MeshInstance {
            mesh: decor,
            position: p,
            rotation: q,
            scale: [s; 3],
            material: mats[ITEM_BOX_DECOR_ROLE].index,
            color: [pc[0], pc[1], pc[2], 1.0],
        });
    }
}

/// A Goo Blob lying on the road at `pos`.
pub(crate) fn draw_goo(pos: [f32; 3], mats: &[Mat; 16], rw: &mut RenderWorld) {
    rw.draw_mesh(inst(MeshId::SPHERE, add(pos, [0.0, 0.1, 0.0]), NO_ROT, [1.9, 0.4, 1.9], mats[role::GOO], [1.0; 3]));
}

/// A Boing Bomb at `pos` with its fuse cap and flickering spark.
pub(crate) fn draw_bomb(pos: [f32; 3], mats: &[Mat; 16], time: f32, rw: &mut RenderWorld) {
    rw.draw_mesh(inst(MeshId::SPHERE, pos, NO_ROT, [0.95; 3], mats[role::BOMB], [1.0; 3]));
    rw.draw_mesh(inst(MeshId::CYLINDER, add(pos, [0.0, 0.5, 0.0]), NO_ROT, [0.3, 0.18, 0.3], mats[role::KERB_WHITE], [0.6; 3]));
    let fl = 0.6 + 0.4 * (time * 30.0).sin();
    rw.draw_mesh(inst(MeshId::SPHERE, add(pos, [0.0, 0.6, 0.0]), NO_ROT, [0.3 * fl; 3], mats[role::SPARK], [4.0, 1.5, 0.3]));
}

/// A Seeker Star at `pos`: a glowing core with two turning blades.
pub(crate) fn draw_seeker(pos: [f32; 3], mats: &[Mat; 16], time: f32, rw: &mut RenderWorld) {
    let spin = time * 9.0;
    rw.draw_mesh(inst(MeshId::SPHERE, pos, NO_ROT, [0.7; 3], mats[role::SPARK], [4.0, 3.2, 0.6]));
    for j in 0..2 {
        let q = quat_axis_angle([0.0, 1.0, 0.0], spin + j as f32 * 1.57);
        rw.draw_mesh(inst(MeshId::CUBE, pos, q, [1.5, 0.18, 0.18], mats[role::SPARK], [4.0, 3.2, 0.6]));
    }
}

/// Whether a kart wears the "this is you" arrow. Never over the results
/// panel. In a one-player race the player already sees their own kart low in
/// the centre of the chase view, so the arrow stays off there; it comes back
/// once the kart is over the line and the camera orbits it (a replay-style
/// view, where the kart is not fixed in the frame). In two-player split-screen
/// both karts wear theirs: the scene is drawn once for both halves, so each
/// half sees the other player's arrow (what it is for) and its own (the price
/// of one shared draw list; the arrow is small and sits over the helmet, clear
/// of the road ahead). Rivals never wear one. The race is `Finished` as soon as
/// every human is over the line, so the orbit (`Stage::Celebrate`) shows it
/// and only the podium hides it.
pub(crate) fn marker_shown(players: usize, stage: crate::finale::Stage, is_player: bool, finished: bool) -> bool {
    if !is_player || stage == crate::finale::Stage::Podium {
        return false;
    }
    players >= 2 || finished
}

/// Height of the player marker (cone and cap, m) at scale 1.
const MARKER_H: f32 = 0.545;

/// The player marker's height on screen (CSS px) for a canvas whose short
/// side is `vmin` (`hud-layout.md`: 36 px at 1080p, 20 px on a 390 px tall
/// phone, in between on the way).
pub(crate) fn marker_px(vmin: f32) -> f32 {
    (20.0 + 16.0 * (vmin - 390.0) / 690.0).clamp(20.0, 36.0)
}

/// Scale of the player marker (1 = its built size) that draws it
/// [`marker_px`] tall, `distance` m from an eye with a vertical field of view
/// of `fov_deg` over a view `view_h` px tall. 1 when there is no view yet.
pub(crate) fn marker_scale(distance: f32, fov_deg: f32, view_h: f32, vmin: f32) -> f32 {
    if view_h <= 0.0 || vmin <= 0.0 {
        return 1.0;
    }
    let world_h = marker_px(vmin) / view_h * 2.0 * distance.max(0.5) * (fov_deg.to_radians() * 0.5).tan();
    world_h / MARKER_H
}

/// `chase`: the chase views are on (player markers). `lean`: a small
/// triangle budget (Low): only local players' karts puff dust and a boost
/// flame is one sphere, so a pack of drifting, boosting karts stays inside
/// the tier's limit.
#[allow(clippy::too_many_arguments)]
fn draw_dynamic(
    race: &RaceView,
    t: &Tune,
    look: &ItemBoxLook,
    seats: &[Option<BoxSeat>],
    mats: &[Mat; 16],
    alpha: f32,
    time: f32,
    (chase, lean, shadows, budget): (bool, bool, bool, u32),
    fade: &[f32; crate::race::MAX_KARTS],
    mut rivals: Option<&mut RivalDraw<'_>>,
    rw: &mut RenderWorld,
) {
    let ident = [0.0, 0.0, 0.0, 1.0];
    let site = crate::finale::podium_site(&race.track, t.verge_width);
    let stage = crate::finale::stage(race.phase, race.phase_time, race.players);
    // The podium scene shows only itself (no karts, boxes or blobs).
    if stage == crate::finale::Stage::Podium {
        start_lights(race, mats, lean, rw);
        crate::finale::draw(race, &site, mats, budget, time, shadows, rw);
        return;
    }
    push_blobs(race, alpha, rw);
    for (i, b) in race.boxes.iter().enumerate() {
        draw_item_box(b, i, look, seats.get(i).copied().flatten().as_ref(), mats, time, shadows, rw);
    }
    // Karts. Everything drawn for a kart takes its fade (instance alpha:
    // the forward shader's screen-door dither).
    for (ki, k) in race.karts.iter().enumerate() {
        let first = rw.meshes.len();
        let spec = &race.roster[k.spec];
        let pos = lerp3(k.prev_pos, k.pos, alpha);
        let yaw = k.prev_yaw + crate::kart::wrap_angle(k.yaw - k.prev_yaw) * alpha;
        let vyaw = yaw + k.slip + k.spin_angle;
        let q = yaw_q(vyaw);
        let at = |o: [f32; 3]| add(pos, rot(vyaw, o));
        // A rival whose recipe parts have streamed in is drawn from them
        // (`rivals.rs`); until then, and for the players, the primitive kart.
        let f = fade.get(ki).copied().unwrap_or(1.0);
        let from_parts = rivals.as_mut().and_then(|r| r.kart(&RivalKart { k, spec, pos, vyaw, max_speed: t.max_speed, fade: f }));
        draw_kart_parts(k, (spec.color, spec.suit), pos, vyaw, t.max_speed, from_parts.unwrap_or(KartParts::ALL), mats, rw);
        if k.boost > 0.0 {
            let fl = 1.0 + 0.3 * (time * 40.0 + k.spec as f32).sin();
            if lean {
                low_flames(fl, mats, &at, q, rw);
            } else {
                for x in [0.26, -0.26] {
                    rw.draw_mesh(inst(MeshId::SPHERE, at([x, 0.78, -1.75 - 0.25 * fl]), q, [0.36, 0.34, 0.8 * fl], mats[role::BOOST_FLAME], [4.0, 2.0, 0.6]));
                }
            }
            boost_trail(k, mats, time, lean, &at, q, rw);
        }
        let puffs = !lean || k.player.is_some();
        if k.drift != 0.0 {
            drift_particles(k, t, mats, time, &at, q, rw);
        }
        if puffs && (k.drift != 0.0 || (!k.on_road && k.speed.abs() > 6.0 && k.hop <= 0.0)) {
            dust(k, mats, time, &at, rw);
        }
        // The player marker: an arrow pointing down at each local player's
        // kart (chase views), in the player's kart colour (P1 pink), lit so
        // the toon bands and the ink outline give it a clear shape, with a
        // white cap; it turns slowly and bobs. Small and just over the
        // helmet (tip 2.3 m up, `hud-layout.md`), so it marks the kart
        // without covering the road ahead in the chase view.
        // Not over the results panel.
        // In the one-player finish orbit it is sized for the screen (36 px
        // tall at 1080p, 20 px on a phone) whatever the orbit's distance;
        // split-screen keeps its built size.
        if chase && marker_shown(race.players, stage, k.player.is_some(), k.finished.is_some()) {
            let s = if race.players == 1 {
                let vp = rw.viewport_css;
                let e = rw.camera.eye;
                let d = [pos[0] - e[0], pos[1] + 2.3 - e[1], pos[2] - e[2]];
                marker_scale((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(), rw.camera.fov_y_deg, vp[1], vp[0].min(vp[1]))
            } else {
                1.0
            };
            let bob = 0.05 * s * (time * 3.0).sin();
            let down = quat_mul(yaw_q(time * 1.5), quat_axis_angle([1.0, 0.0, 0.0], core::f32::consts::PI));
            rw.draw_mesh(inst(MeshId::CONE, add(pos, [0.0, 2.3 + 0.25 * s + bob, 0.0]), down, [0.46 * s, 0.5 * s, 0.46 * s], mats[role::KART_BODY], spec.color));
            rw.draw_mesh(inst(MeshId::CYLINDER, add(pos, [0.0, 2.3 + 0.51 * s + bob, 0.0]), yaw_q(time * 1.5), [0.5 * s, 0.07 * s, 0.5 * s], mats[role::KERB_WHITE], [1.0; 3]));
        }
        if k.shield > 0.0 {
            for j in 0..8 {
                let a = time * 2.5 + j as f32 * core::f32::consts::TAU / 8.0;
                let p = add(pos, [1.7 * a.cos(), 0.8 + 0.3 * (a * 2.0).sin(), 1.7 * a.sin()]);
                rw.draw_mesh(inst(MeshId::SPHERE, p, ident, [0.3; 3], mats[role::SPARK], [0.6, 2.4, 3.0]));
            }
        }
        if k.spin > 0.0 {
            for j in 0..3 {
                let a = time * 6.0 + j as f32 * core::f32::consts::TAU / 3.0;
                let p = add(pos, [0.8 * a.cos(), 2.3, 0.8 * a.sin()]);
                rw.draw_mesh(inst(MeshId::SPHERE, p, ident, [0.25; 3], mats[role::SPARK], [3.0, 2.6, 0.6]));
            }
        }
        if f <= 0.0 {
            rw.meshes.truncate(first);
        } else if f < 1.0 {
            for m in &mut rw.meshes[first..] {
                m.color[3] = f;
            }
        }
    }
    for g in &race.goos {
        draw_goo(g.pos, mats, rw);
    }
    for b in &race.bombs {
        draw_bomb(b.pos, mats, time, rw);
    }
    for s in &race.seekers {
        draw_seeker(s.pos, mats, time, rw);
    }
    for b in &race.blasts {
        draw_blast(b, mats, time, lean, rw);
    }
    start_lights(race, mats, lean, rw);
    crate::finale::draw(race, &site, mats, budget, time, shadows, rw);
}


/// Candy colours of a blast's pieces (sRGB hex): lemon, cherry, sky, mint,
/// sugar. Lit, not glowing, so the toon bands and the ink outline show.
const BLAST_COLOURS: [u32; 5] = [0xffda50, 0xf1506c, 0x89e5ff, 0xa0edce, 0xf7f7fa];

/// One item blast at `b`: flat candy shapes in the look's materials, so the
/// ink outline and the toon bands draw them (a glowing ball bloomed out the
/// karts behind it). A shockwave ring of flattened gumdrops racing out over
/// the road, a crown of flat spikes and a lemon dome that shrink away, and
/// debris puffs and wrapper chips thrown up in arcs. Everything is a pure
/// function of the blast's age, so nothing is stored or allocated per frame,
/// and nothing rises above about 2.5 m (the chase camera sees over it).
/// `lean` (a small-budget tier) draws fewer pieces; they stay shapes.
fn draw_blast(b: &crate::items::Blast, mats: &[Mat; 16], time: f32, lean: bool, rw: &mut RenderWorld) {
    let f = (b.t / crate::items::BLAST_LIFE_S).clamp(0.0, 1.0);
    let out = 1.0 - (1.0 - f) * (1.0 - f);
    let fade = 1.0 - f;
    let ident = [0.0, 0.0, 0.0, 1.0];
    let tau = core::f32::consts::TAU;
    let base = b.pos;
    // The blast's centre sits half a metre over the road: the pieces stand on
    // the road a little below it.
    let ground = base[1] - 0.45;
    let colour = |n: usize| linear(BLAST_COLOURS[n % BLAST_COLOURS.len()]);
    let salt = (b.pos[0] * 13.0 + b.pos[2] * 7.0).abs() as u32;
    let cream = mats[role::KERB_WHITE];
    let body = mats[role::KART_BODY];

    // Shockwave ring.
    let ring = if lean { 10 } else { 16 };
    let rr = b.radius * (0.25 + 1.15 * out);
    let gum = (0.8 * fade + 0.25) * (1.0 + 0.2 * (time * 20.0).sin().abs());
    for n in 0..ring {
        let a = tau * n as f32 / ring as f32 + 0.4;
        let p = [base[0] + a.cos() * rr, ground + 0.22, base[2] + a.sin() * rr];
        let c = if n % 2 == 0 { colour(4) } else { colour(2) };
        rw.draw_mesh(inst(MeshId::LOW_SPHERE, p, yaw_q(-a), [gum * 1.5, gum * 0.55, gum * 0.95], cream, c));
    }

    // Crown of flat spikes (cones lying out and up), turning a little.
    let spikes = if lean { 6 } else { 10 };
    let len = b.radius * (0.35 + 0.5 * out);
    let thick = 0.28 * fade + 0.06;
    for n in 0..spikes {
        let a = tau * n as f32 / spikes as f32 + f * 0.8;
        let elev = 0.55 + 0.25 * hash01(n as u32, salt);
        let l = len * (0.8 + 0.4 * hash01(n as u32, salt + 1));
        let q = quat_mul(yaw_q(a), quat_axis_angle([1.0, 0.0, 0.0], core::f32::consts::FRAC_PI_2 - elev));
        // The cone's base sits at the centre, the tip out at `l`.
        let dir = [a.sin() * elev.cos(), elev.sin(), a.cos() * elev.cos()];
        let mid = [base[0] + dir[0] * l * 0.5, ground + 0.6 + dir[1] * l * 0.5, base[2] + dir[2] * l * 0.5];
        let col = if n % 2 == 0 { colour(0) } else { colour(1) };
        rw.draw_mesh(inst(MeshId::CONE, mid, q, [0.8 * fade + 0.2, l, thick * 1.9], body, col));
    }

    // The lemon dome in the middle: a quick pop that shrinks away.
    let pop = (1.0 - f * 1.6).max(0.0);
    if pop > 0.0 {
        let r = b.radius * 0.4 * (0.6 + 0.8 * (1.0 - pop)) * pop.sqrt();
        rw.draw_mesh(inst(MeshId::DOME, [base[0], ground, base[2]], ident, [r * 2.0, r * 1.5, r * 2.0], body, colour(0)));
        rw.draw_mesh(inst(MeshId::DOME, [base[0], ground, base[2]], ident, [r * 1.2, r * 1.1, r * 1.2], cream, colour(4)));
    }

    // Debris: puffs and wrapper chips thrown up and out.
    let puffs = if lean { 5 } else { 9 };
    let t = b.t;
    for n in 0..puffs {
        let a = tau * (n as f32 + hash01(n as u32, salt + 2)) / puffs as f32;
        let speed = b.radius * (1.2 + 1.2 * hash01(n as u32, salt + 3));
        let up = 4.5 + 3.5 * hash01(n as u32, salt + 4);
        let y = (ground + 0.5 + up * t - 9.0 * t * t).max(ground + 0.2);
        let p = [base[0] + a.sin() * speed * t, y, base[2] + a.cos() * speed * t];
        let s = (0.5 + 0.3 * hash01(n as u32, salt + 5)) * (1.0 - 0.7 * f);
        rw.draw_mesh(inst(MeshId::LOW_SPHERE, p, ident, [s, s * 0.85, s], body, colour(n + 1)));
    }
    if !lean {
        for n in 0..6u32 {
            let a = tau * (n as f32 + hash01(n, salt + 6)) / 6.0 + 0.5;
            let speed = b.radius * (0.9 + 0.8 * hash01(n, salt + 7));
            let up = 5.5 + 3.0 * hash01(n, salt + 8);
            let y = (ground + 0.6 + up * t - 9.5 * t * t).max(ground + 0.2);
            let p = [base[0] + a.sin() * speed * t, y, base[2] + a.cos() * speed * t];
            let spin = quat_mul(yaw_q(a + t * 9.0), quat_axis_angle([1.0, 0.0, 0.0], t * 11.0 + n as f32));
            let s = 0.34 * (1.0 - 0.6 * f);
            rw.draw_mesh(inst(MeshId::BEVEL_CUBE, p, spin, [s * 1.6, s * 0.5, s], body, colour(n as usize + 3)));
        }
    }
}

/// One start light's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Off,
    Red,
    Green,
    /// The finish flash: gold, alternating with green.
    Gold,
}

/// How long the start lights stay green after GO (s).
pub const GREEN_S: f32 = 1.5;

/// The three start lights: one more turns red on each of 3, 2 and 1, all
/// three turn green on GO for [`GREEN_S`], then they go dark.
pub fn start_light_states(phase: Phase, countdown: f32, phase_time: f32) -> [Light; 3] {
    match phase {
        Phase::Countdown => {
            let lit = (4.0 - countdown.ceil()).clamp(0.0, 3.0) as usize;
            core::array::from_fn(|i| if i < lit { Light::Red } else { Light::Off })
        }
        Phase::Racing if phase_time < GREEN_S => [Light::Green; 3],
        _ => [Light::Off; 3],
    }
}

/// Low's start-light colours on the lit `kart_body` material. The glowing
/// `spark` glass adds a pale blue-white glow of its own, which bloom turns
/// into a halo on Medium and up but which, with no bloom on Low, washed the
/// red lights out to a pale pink; lit paint in clean colours reads as a
/// clearly red (green, gold) ball with the ink outline round it.
pub fn low_light_colour(s: Light) -> [f32; 3] {
    match s {
        Light::Off => [0.22, 0.04, 0.06],
        Light::Red => [1.0, 0.02, 0.03],
        Light::Green => [0.08, 1.0, 0.22],
        Light::Gold => [1.2, 0.72, 0.04],
    }
}

/// A start light's colour on Medium and up: lit paint like Low's, but bright
/// enough that its lit and mid bands end over the look's bloom threshold, so
/// it glows in its own colour. The other channels are (near) zero: the
/// tonemap lifts any small share of them at this brightness, which turned
/// the red lights pale pink-white before (critic round 4). Unlit ones stay
/// Low's dark red.
pub fn glow_light_colour(s: Light) -> [f32; 3] {
    match s {
        Light::Off => low_light_colour(Light::Off),
        Light::Red => [3.2, 0.0, 0.02],
        Light::Green => [0.05, 3.2, 0.25],
        Light::Gold => [3.6, 1.9, 0.0],
    }
}

/// The start lights under the gate's banner, on the grid side (local -z): a
/// dark housing and three glowing lights (red on 3-2-1, green on GO).
/// `lean` (Low): lit paint instead of glowing glass (see [`low_light_colour`]).
fn start_lights(race: &RaceView, mats: &[Mat; 16], lean: bool, rw: &mut RenderWorld) {
    let track = &race.track;
    let p0 = track.point_at(0.0, 0.0);
    let tan = track.tangent_at(0.0);
    let yaw = tan[0].atan2(tan[1]);
    let q = yaw_q(yaw);
    rw.draw_mesh(inst(MeshId::BEVEL_CUBE, add(p0, rot(yaw, [0.0, 5.55, -0.55])), q, [3.3, 0.95, 0.5], mats[role::WHEEL], [1.0; 3]));
    // Once a human is over the line the gate is lit on both faces, flashing
    // gold and green; the far face (the one the finisher sees after the
    // line) has its own housing.
    let since = race.karts.iter().filter(|k| k.player.is_some()).filter_map(|k| k.finished).reduce(f32::min).map(|f| (race.clock - f).max(0.0));
    let faces: &[f32] = if since.is_some() { &[-0.86, 0.86] } else { &[-0.86] };
    if since.is_some() {
        rw.draw_mesh(inst(MeshId::BEVEL_CUBE, add(p0, rot(yaw, [0.0, 5.55, 0.55])), q, [3.3, 0.95, 0.5], mats[role::WHEEL], [1.0; 3]));
    }
    for (i, s) in start_light_states(race.phase, race.countdown, race.phase_time).into_iter().enumerate() {
        let s = match since {
            Some(since) => if crate::finale::light_is_gold(i, since) { Light::Gold } else { Light::Green },
            None => s,
        };
        // On Medium and up the lit ones glow: the same clean paint as Low,
        // pushed over the bloom threshold (see [`glow_light_colour`]). Not
        // the spark material: its untinted white-blue glow, added on top,
        // turned the red lights pale pink-white (critic round 4).
        let (m, c) = (mats[role::KART_BODY], if lean { low_light_colour(s) } else { glow_light_colour(s) });
        // Local +x is left as the grid sees it: light 1 is the leftmost.
        for &z in faces {
            let at = add(p0, rot(yaw, [1.0 - i as f32, 5.55, z]));
            rw.draw_mesh(inst(MeshId::LOW_SPHERE, at, q, [0.66; 3], m, c));
        }
    }
}

/// Smooth-ish value noise for camera shake (-1..1).
fn shake_noise(t: f32, salt: f32) -> f32 {
    ((t * 1.0 + salt).sin() * 0.6 + (t * 2.3 + salt * 1.7).sin() * 0.3 + (t * 4.1 + salt * 2.9).sin() * 0.1).clamp(-1.0, 1.0)
}

/// The look's shadow tint (`render.shadow_tint` colour, `render.shadow_tint_strength`
/// 0..1) as a per-channel factor, read live; `None` while the tint is off.
fn live_shadow_shade(reg: &ParamRegistry) -> Option<Shade> {
    let tint = match reg.id("render.shadow_tint").and_then(|id| reg.value(id)) {
        Some(ParamValue::Vec3(v) | ParamValue::Color(v)) => v,
        _ => return None,
    };
    shadow_shade(tint, reg.f32_or("render.shadow_tint_strength", 0.0))
}

/// The look's shadow tint (linear colour) and its strength (0..1).
pub(crate) type Shade = ([f32; 3], f32);

/// The tint to apply, or `None` when it is off (strength 0), so other looks
/// keep their colours exactly.
pub(crate) fn shadow_shade(tint: [f32; 3], strength: f32) -> Option<Shade> {
    let k = strength.clamp(0.0, 1.0);
    (k > 0.0).then_some((tint.map(|c| c.max(0.0)), k))
}

/// A shadowed colour turned toward the look's shadow tint, the same way the
/// shader tints the shadows it draws: in a gamma-like (square root) space the
/// colour keeps its brightness but takes the tint's hue (scaled down if that
/// would leave the screen's range), then `strength` mixes it in.
pub(crate) fn tint_shadow(c: [f32; 3], (tint, k): Shade) -> [f32; 3] {
    let ts = tint.map(f32::sqrt);
    let hs = ts.map(|v| v / luma(ts).max(1e-3));
    let cs = c.map(|v| v.max(0.0).sqrt());
    let t = hs.map(|h| h * luma(cs));
    let top = t[0].max(t[1]).max(t[2]).max(1.0);
    let mut out = [0.0; 3];
    for i in 0..3 {
        let v = t[i] / top;
        out[i] = c[i] + (v * v - c[i]) * k;
    }
    out
}

/// How much darker (linear) a tinted shadow disc is than its plain colour.
const DISC_DEEPEN: f32 = 0.7;

/// A shadow disc's instance colour with [`tint_shadow`] applied to what it
/// draws as: the colour times its material's base colour `base` (a green
/// meadow material would otherwise swallow the tint's red and blue), so the
/// returned colour times `base` is the tinted shadow.
pub(crate) fn disc_tinted(c: [f32; 4], base: [f32; 3], shade: Shade) -> [f32; 4] {
    // A touch deeper than the grey-green the discs were before, so the
    // lavender one reads as dark as the shadow map's shadows beside it.
    let shown = [c[0] * base[0] * DISC_DEEPEN, c[1] * base[1] * DISC_DEEPEN, c[2] * base[2] * DISC_DEEPEN];
    let o = tint_shadow(shown, shade);
    [o[0] / base[0].max(0.02), o[1] / base[1].max(0.02), o[2] / base[2].max(0.02), c[3]]
}

/// The HUD unit `--u` (CSS px) of `hud-layout.md` for a canvas whose short
/// side is `vmin`: 1 vmin clamped to 5..11 px, or 0.62 vmin clamped to 4..7
/// px in split screen.
pub fn hud_unit(vmin: f32, split: bool) -> f32 {
    if split {
        (vmin * 0.0062).clamp(4.0, 7.0)
    } else {
        (vmin * 0.01).clamp(5.0, 11.0)
    }
}

/// Minimap panel side (px) for a HUD unit: 150 at 1080p (u = 10.8) and 90 on
/// a phone (u = 5); in split screen 100 (u = 6.7) and 64 (u = 4).
pub fn minimap_size(u: f32, split: bool) -> f32 {
    if split {
        (64.0 + (u - 4.0) * 36.0 / 2.7).clamp(64.0, 100.0)
    } else {
        (90.0 + (u - 5.0) * 60.0 / 5.8).clamp(90.0, 150.0)
    }
}

/// Where the minimap's top-left corner sits in a view: top-right, under the
/// race timer (its offset 1.4 u from the top and 2.2 u from the right, its
/// text 3.2 u tall at a 1.3 line height) with a 0.8 u gap.
pub fn minimap_origin(view: [f32; 4], u: f32, size: f32) -> [f32; 2] {
    [view[0] + view[2] - 2.2 * u - size, view[1] + (1.4 + 3.2 * 1.3 + 0.8) * u]
}

/// An sRGB hex colour as the linear RGB the renderer takes.
fn linear(hex: u32) -> [f32; 3] {
    let c = |shift: u32| {
        let s = ((hex >> shift) & 0xff) as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    [c(16), c(8), c(0)]
}

/// Minimap of the track and karts in one view's top-right corner, under the
/// timer: the track in #6f6c7e on a 60 % #2b1240 panel with a 3 px ink
/// border, the local player's kart ringed in white, rivals in their colours
/// (`hud-layout.md`). `vmin` is the whole canvas's short side, `split` two
/// local players.
/// Its size and place come from the HUD layout tunables (`lay`).
fn minimap(race: &RaceView, view: [f32; 4], me: Option<usize>, (vmin, split): (f32, bool), lay: &crate::hud_layout::HudLayout, rw: &mut RenderWorld) {
    let u = hud_unit(vmin, split);
    let m = lay.minimap_size(u, split);
    let [x0, y0] = lay.minimap_origin(view, u, m);
    let k = m / 150.0;
    let b = race.track.bounds;
    let span = (b[2] - b[0]).max(b[3] - b[1]).max(1.0);
    let scale = (m - 16.0 * k) / span;
    let cx = (b[0] + b[2]) * 0.5;
    let cz = (b[1] + b[3]) * 0.5;
    // Seen from above with +z up: +x points left on screen.
    let to_screen = |p: [f32; 3]| [x0 + m * 0.5 - (p[0] - cx) * scale, y0 + m * 0.5 - (p[2] - cz) * scale];
    let sprite = |pos: [f32; 2], size: [f32; 2], cell: u16, color: [f32; 4], layer: i16| Sprite {
        pos,
        size,
        rotation: 0.0,
        cell,
        color,
        layer,
    };
    let ink = linear(0x2b1240);
    // The panel: the atlas square fills 0.82 of its sprite (rounded corners),
    // so the sprite is drawn larger to make the panel `m` px across. The
    // renderer blends in linear light, where the layout's 60 % reads as
    // about 25 % on a bright sky, so the fill is the `minimap_ink` tunable
    // (0.96: about 85 % ink, so the track line reads over bright signs).
    let c = [x0 + m * 0.5, y0 + m * 0.5];
    rw.draw_sprite(sprite(c, [m / 0.82, m / 0.82], cell::SQUARE, [ink[0], ink[1], ink[2], lay.minimap_ink.clamp(0.0, 1.0)], 20));
    let road = linear(0x6f6c7e);
    let dot = 6.0 * k;
    let step = (race.track.length / 70.0).max(2.0);
    let mut s = 0.0;
    while s < race.track.length {
        let p = to_screen(race.track.point_at(s, 0.0));
        rw.draw_sprite(sprite(p, [dot, dot], cell::CIRCLE, [road[0], road[1], road[2], 1.0], 21));
        s += step;
    }
    let start = to_screen(race.track.point_at(0.0, 0.0));
    rw.draw_sprite(sprite(start, [9.0 * k, 9.0 * k], cell::SQUARE, [1.0, 0.25, 0.35, 0.95], 22));
    for (i, kart) in race.karts.iter().enumerate() {
        let col = race.roster[kart.spec].color;
        let p = to_screen(kart.pos);
        if Some(i) == me {
            rw.draw_sprite(sprite(p, [17.0 * k, 17.0 * k], cell::RING, [1.0, 1.0, 1.0, 1.0], 24));
            rw.draw_sprite(sprite(p, [11.0 * k, 11.0 * k], cell::CIRCLE, [col[0], col[1], col[2], 1.0], 24));
        } else {
            rw.draw_sprite(sprite(p, [9.0 * k, 9.0 * k], cell::CIRCLE, [col[0], col[1], col[2], 1.0], 23));
        }
    }
}

fn play_events(race: &mut RaceView, sounds: &Sounds, mixer: &mut AudioMixer) {
    let events = core::mem::take(&mut race.events);
    if race.phase == Phase::Title {
        return;
    }
    let human = |i: usize| race.karts.get(i).is_some_and(|k| k.player.is_some());
    let near_human = |p: [f32; 3]| {
        race.karts.iter().filter(|k| k.player.is_some()).any(|k| {
            let d = ((k.pos[0] - p[0]).powi(2) + (k.pos[2] - p[2]).powi(2)).sqrt();
            d < 60.0
        })
    };
    for e in events {
        let (sfx, gain) = match e {
            RaceEvent::Countdown(_) => (Sfx::Beep, 0.6),
            RaceEvent::Go => (Sfx::Go, 0.7),
            RaceEvent::ItemBox(i) if human(i) => (Sfx::ItemBox, 0.5),
            RaceEvent::ItemReady(i) if human(i) => (Sfx::ItemReady, 0.45),
            RaceEvent::ItemUse(i, _) if human(i) => (Sfx::ItemUse, 0.5),
            RaceEvent::Hop(i) if human(i) => (Sfx::Hop, 0.3),
            RaceEvent::DriftTier(i, _) if human(i) => (Sfx::Tier, 0.4),
            RaceEvent::Boost(i, _) if human(i) => (Sfx::Boost, 0.55),
            RaceEvent::Hit(i) if human(i) => (Sfx::Hit, 0.6),
            RaceEvent::ShieldPop(i) if human(i) => (Sfx::ShieldPop, 0.5),
            RaceEvent::Bump(a, b) if human(a) || human(b) => (Sfx::Bump, 0.5),
            RaceEvent::Wall(i, _) if human(i) => (Sfx::Wall, 0.5),
            RaceEvent::Blast(p) if near_human(p) => (Sfx::Blast, 0.7),
            RaceEvent::Lap(i, _) if human(i) => (Sfx::Lap, 0.55),
            RaceEvent::Finish(i, place) if human(i) => (if place <= 3 { Sfx::Win } else { Sfx::Lose }, 0.6),
            _ => continue,
        };
        sounds.play(mixer, sfx, gain);
    }
}

/// The `Frame` system.
pub fn draw_system(world: &mut World) {
    let Some(kit) = world.try_res::<KartKit>().map(|k| *k) else {
        return;
    };
    let (alpha, frame_dt, time) = {
        let tm = world.res::<Time>();
        (tm.alpha, tm.frame_dt.clamp(0.0, 0.1), tm.render_seconds())
    };
    let time = world.res::<crate::race::Race>().moment_time.map_or(time, f64::from);
    let tf = (time % 3600.0) as f32;
    let (t, look, still) = {
        let reg = world.res::<ParamRegistry>();
        let t = kit.tune.read(&reg);
        let mut look = ItemBoxLook::new(&t, kit.item_box_palette.map(|c| reg.vec3(c)), reg.vec3(kit.item_box_glyph_color));
        look.paint = look.glyph_radiance(reg.f32_or("fx.bloom.threshold", LOOK_BLOOM_THRESHOLD));
        (t, look, reg.bool(kit.static_world))
    };
    #[cfg(feature = "models")]
    let rival_parts = world.res::<ParamRegistry>().bool(kit.rival_parts);
    #[cfg(not(feature = "models"))]
    let rival_parts = false;
    let shade = live_shadow_shade(&world.res::<ParamRegistry>());
    let shake_on = world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on("shake"));
    let (amount, roll_deg, decay, freq, kick_deg) = {
        let reg = world.res::<ParamRegistry>();
        (
            reg.f32_or("fx.shake.amount", 0.35),
            reg.f32_or("fx.shake.roll_deg", 3.0),
            reg.f32_or("fx.shake.decay", 1.6),
            reg.f32_or("fx.shake.frequency", 18.0),
            reg.f32_or("fx.speedlines.fov_kick_deg", 8.0),
        )
    };
    // The least shadow filter radius (render reads it every frame).
    {
        let want = t.shadow_soften.round().clamp(0.0, 2.0) as u32;
        let mut base = core::mem::take(&mut world.res_mut::<DrawState>().shadow_pcf_base);
        if let Some(mut tiers) = world.try_res_mut::<Tiers>() {
            soften_shadow_filter(&mut tiers, &mut base, want);
        }
        world.res_mut::<DrawState>().shadow_pcf_base = base;
    }
    // Materials, prop entities and static meshes, once per look and tier
    // (the small-budget tiers use the thin prop layout).
    {
        let (max_tris, shadow_map) = world
            .try_res::<Tiers>()
            .map_or((u32::MAX, false), |t| (t.active().max_triangles, t.active().shadow_map.is_some()));
        let shadows = shadow_map && world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on("shadows"));
        let lean = max_tris < THIN_TREES_BELOW_TRIANGLES;
        let key = world.try_res::<LookData>().map(|l| (l.name.clone(), l.materials.len(), max_tris, shadows));
        let stale = {
            let st = world.res::<DrawState>();
            st.look_key != key || st.mats.is_none()
        };
        let moved = crate::scenery::spawn_props(world, t.verge_width, lean);
        if stale || moved {
            let mats = world.try_res::<LookData>().map_or_else(|| resolve_materials(&LookData::default()), |l| resolve_materials(&l));
            let props = crate::scenery::props(world);
            let mut st = world.res_mut::<DrawState>();
            let mut statics = core::mem::take(&mut st.statics);
            build_statics_with(&world.res::<Race>().track, &mats, t.verge_width, &props, shadows, &mut statics);
            add_road_marks(&world.res::<Race>().track, &mats, lean, t.verge_width, &mut statics);
            st.road_ring = crate::roadmarks::mesh_rings(&world.res::<Race>().track, t.verge_width);
            st.box_seats.clear();
            // Register the track meshes once, then replace them in place;
            // point their instances at the ids the library gave.
            // (The plugin adds the library; without one they can't draw.)
            if let Some(mut lib) = world.try_res_mut::<MeshLibrary>() {
                for (k, m) in statics.meshes.iter().enumerate() {
                    match st.track_ids.get(k) {
                        Some(&id) => {
                            lib.replace(id, m.clone());
                        }
                        None => match lib.add(m.clone()) {
                            Some(id) => st.track_ids.push(id),
                            None => break,
                        },
                    }
                }
            }
            let ids = st.track_ids.clone();
            for m in statics.instances.iter_mut().chain(statics.decals.iter_mut()) {
                if let Some(k) = usize::from(m.mesh.0).checked_sub(BUILTIN_MESH_COUNT) {
                    m.mesh = ids.get(k).copied().unwrap_or(m.mesh);
                }
            }
            st.statics = statics;
            st.decor_mat = world.try_res::<LookData>().and_then(|l| {
                l.materials.get(mats[ITEM_BOX_DECOR_ROLE].index as usize).map(|m| (m.base_color, m.emissive, m.unlit))
            });
            st.mats = Some(mats);
            st.look_key = key;
        }
        let mut st = world.res_mut::<DrawState>();
        st.lean = lean;
        st.shadows = shadows;
        st.budget = max_tris;
    }
    // The item box's own meshes (registered after the track's), rebuilt in
    // place when a shape tunable changes; the decor paint follows the
    // material it is drawn with and the live bloom threshold.
    let look = {
        let mut look = look;
        let mut st = world.res_mut::<DrawState>();
        if st.box_shape != Some(look.shape)
            && let Some(mut lib) = world.try_res_mut::<MeshLibrary>()
        {
            let (body, decor) = (crate::itembox::body(&look.shape), crate::itembox::decor(&look.shape));
            st.box_meshes = match st.box_meshes {
                Some((b, d)) => {
                    lib.replace(b, body);
                    lib.replace(d, decor);
                    Some((b, d))
                }
                None => lib.add(body).zip(lib.add(decor)),
            };
            st.box_shape = Some(look.shape);
        }
        look.meshes = st.box_meshes;
        if let Some(&disc) = st.track_ids.get(crate::trackmesh::COUNT + crate::propmesh::DISC) {
            look.disc = disc;
        }
        if let Some((base, emissive, unlit)) = st.decor_mat {
            let threshold = world.res::<ParamRegistry>().f32_or("fx.bloom.threshold", LOOK_BLOOM_THRESHOLD);
            look.paint_for(base, emissive, unlit, threshold);
        }
        look
    };
    let mut st = world.res_mut::<DrawState>();
    if st.sounds.is_none() {
        let mut mixer = world.res_mut::<AudioMixer>();
        st.sounds = Some(Sounds::register(&mut mixer));
    }
    let Some(mats) = st.mats else {
        return;
    };
    let mut race = RaceView::new(world);
    // Live asset focus (dev builds only): one of this kit's assets alone on
    // the turntable camera evani-core computes. The game is held meanwhile,
    // so it is silent too (no events, no engine or surface loops, no warning).
    #[cfg(feature = "live")]
    let focus = crate::focus::focused(world);
    #[cfg(feature = "live")]
    let muted = focus.is_some();
    #[cfg(not(feature = "live"))]
    let muted = false;
    // Sounds.
    if let Some(sounds) = st.sounds.take() {
        let mut mixer = world.res_mut::<AudioMixer>();
        if muted {
            race.events.clear();
        } else {
            play_events(&mut race, &sounds, &mut mixer);
        }
        st.sounds = Some(sounds);
    }
    // HUD.
    world.res_mut::<RaceHud>().update(&race, &t, frame_dt, tf);
    // The looping voices (engines, surfaces) and the incoming-item cue.
    if let Some(sounds) = st.sounds.take() {
        let mut mixer = world.res_mut::<AudioMixer>();
        if muted {
            st.voices.stop(&mut mixer);
        } else {
            st.voices.update(&mut mixer, &sounds, &race, &t);
        }
        let racing = race.phase == Phase::Racing && !race.paused && !muted;
        let hud = world.res::<RaceHud>();
        for p in 0..2 {
            // Not racing resets the warning, so it starts clean after a focus.
            if let Some(cue) = crate::threat::cue(&mut st.warn[p], hud.players_hud[p].threat, racing, frame_dt)
                && !muted
            {
                sounds.play(&mut mixer, cue, 0.55);
            }
        }
        st.sounds = Some(sounds);
    }

    // Cameras.
    let chase = race.phase != Phase::Title && race.players > 0;
    let podium = crate::finale::stage(race.phase, race.phase_time, race.players) == crate::finale::Stage::Podium;
    let site = crate::finale::podium_site(&race.track, t.verge_width);
    // A new race or a staged capture moment puts the camera straight behind
    // the kart with the FOV kick it should have, with no swing in.
    let snap = st.cam_race != race.race_no || race.cam_snap;
    st.cam_race = race.race_no;
    race.cam_snap = false;
    let mut poses = [CameraPose::default(); 2];
    let mut drives = [0.0f32; 2];
    for p in 0..race.players.min(2) {
        let Some(i) = race.player_kart(p) else {
            continue;
        };
        let k = &race.karts[i];
        let pos = lerp3(k.prev_pos, k.pos, alpha);
        let yaw = k.prev_yaw + crate::kart::wrap_angle(k.yaw - k.prev_yaw) * alpha;
        // The camera smooths its heading and the anchor's height, never the
        // kart's position: the kart stays fixed low in the centre of the
        // frame while the view swings round bends a little late (and a
        // drift's slide angle shows, since the heading ignores the slip).
        let a = if snap { 1.0 } else { 1.0 - (-t.cam_smooth * frame_dt).exp() };
        st.cam_yaw[p] = crate::kart::wrap_angle(st.cam_yaw[p] + crate::kart::wrap_angle(yaw - st.cam_yaw[p]) * a);
        st.cam_y[p] += (pos[1] - st.cam_y[p]) * if snap { 1.0 } else { (a * 0.6).min(1.0) };
        let cy = st.cam_yaw[p];
        let f = [cy.sin(), 0.0, cy.cos()];
        let anchor = [pos[0], st.cam_y[p], pos[2]];
        // A staged moment may frame its shot its own way (and keeps its eye
        // where it is put: no rival lift).
        let mc = race.moment_cam.unwrap_or(crate::race::MomentCam {
            distance: t.cam_distance,
            height: t.cam_height,
            look_ahead: t.cam_look_ahead,
            look_height: 1.0,
        });
        st.eye[p] = [anchor[0] - f[0] * mc.distance, anchor[1] + mc.height, anchor[2] - f[2] * mc.distance];
        st.target[p] = [anchor[0] + f[0] * mc.look_ahead, anchor[1] + mc.look_height, anchor[2] + f[2] * mc.look_ahead];
        // Rivals between the camera and the player: lift the eye over them
        // (softly, eased in time) and never let it sit inside one.
        let (blocks, nb) = rival_blocks(&race, i, t.kart_radius, alpha);
        let blocks = &blocks[..nb];
        let lift_to = if k.finished.is_some() || race.moment_cam.is_some() {
            0.0
        } else {
            crate::chasecam::lift_needed([pos[0], pos[1] + crate::chasecam::HEAD, pos[2]], st.eye[p], blocks)
        };
        st.lift[p] = crate::chasecam::ease_lift(st.lift[p], lift_to, frame_dt, snap);
        st.eye[p][1] += st.lift[p];
        // Over the line: the chase view eases into a slow orbit round the kart.
        if let Some(fin) = k.finished {
            let chase_cam = crate::finale::Chase { anchor, yaw: cy, distance: t.cam_distance, height: t.cam_height, look_ahead: t.cam_look_ahead };
            (st.eye[p], st.target[p]) = crate::finale::finish_camera(&chase_cam, (race.clock - fin).max(0.0));
        }
        st.eye[p] = crate::chasecam::keep_out(st.eye[p], blocks);
        // Speed lines only past the normal top speed (a boost or a skill
        // bonus lifts the cap; karts cruise at the cap most of a race, so
        // lines there would show all the time) or on a boost; the FOV kicks
        // out on a boost and a little past the cap, whether or not the
        // speed-lines effect is on.
        let frac = k.speed / t.max_speed.max(0.1);
        // Never over the results panel: a kart finishing on a boost would
        // otherwise keep streaks and the kick behind the podium table.
        let live = race.phase != Phase::Finished && k.finished.is_none();
        let fast = if live { ((frac - 1.02) / 0.1).clamp(0.0, 1.0) } else { 0.0 };
        let boosting = live && k.boost > 0.0;
        drives[p] = (fast * 0.6 + if boosting { 0.7 } else { 0.0 }).min(1.0);
        let want_kick = if boosting { t.boost_fov_kick.max(kick_deg) } else { kick_deg * 0.35 * fast };
        st.kick[p] = if snap { want_kick } else { st.kick[p] + (want_kick - st.kick[p]) * (1.0 - (-6.0 * frame_dt).exp()) };
        let mut eye = st.eye[p];
        let mut up = [0.0, 1.0, 0.0];
        // Trauma decays whether or not shake is on.
        let tr = race.trauma[p];
        race.trauma[p] = (tr - decay * frame_dt).max(0.0);
        if shake_on && tr > 0.0 {
            let s = tr * tr;
            let ft = tf * freq;
            eye = add(eye, [amount * s * shake_noise(ft, 1.0), amount * s * shake_noise(ft, 2.0), amount * s * shake_noise(ft, 3.0)]);
            let roll = (roll_deg * s * shake_noise(ft, 4.0)).to_radians();
            let fwd = normalize3([st.target[p][0] - eye[0], st.target[p][1] - eye[1], st.target[p][2] - eye[2]]);
            // Right = fwd x up.
            let right = normalize3([-fwd[2], 0.0, fwd[0]]);
            up = [right[0] * roll.sin(), roll.cos(), right[2] * roll.sin()];
        }
        poses[p] = CameraPose {
            eye,
            target: st.target[p],
            up,
            fov_y_deg: t.cam_fov + st.kick[p],
        };
    }
    let mut views: Vec<[f32; 4]>;
    // The focus turntable (looked up above) wins over every game camera
    // (chase, rival lift, finish orbit, podium): it is set last, below.
    #[cfg(feature = "live")]
    let backdrop = focus.as_ref().is_none_or(|f| f.backdrop);
    #[cfg(not(feature = "live"))]
    let backdrop = true;
    let shown;
    {
        let mut rw = match world.try_res_mut::<RenderWorld>() {
            Some(rw) => rw,
            None => return,
        };
        let vp = rw.viewport_css;
        if podium && vp[0] > 0.0 && vp[1] > 0.0 {
            // The podium scene: one fixed view over the whole canvas, the
            // podium clear of the results panel (both players see the same).
            // Wide shot, then the close-up move on the winners; the clock is
            // the seconds since the podium scene appeared (deterministic).
            let closeup = crate::finale::CloseUp {
                hold_s: t.finale_cam_hold_s,
                move_s: t.finale_cam_move_s,
                distance: t.finale_cam_distance,
                azimuth: t.finale_cam_azimuth,
                height: t.finale_cam_height,
                look_y: t.finale_cam_look_y,
                fov: t.finale_cam_fov,
            };
            let since = if race.players == 0 { race.phase_time } else { race.phase_time - crate::finale::CELEBRATE_S };
            rw.camera = crate::finale::podium_closeup_camera(&site, vp[0] / vp[1], since, &closeup, race.free_roam);
            views = vec![[0.0, 0.0, vp[0], vp[1]]];
        } else if chase && race.players == 2 {
            rw.camera = poses[0];
            rw.views.push(View::new(poses[0], [0.0, 0.0, 1.0, 0.5]));
            rw.views.push(View::new(poses[1], [0.0, 0.5, 1.0, 0.5]));
            views = vec![[0.0, 0.0, vp[0], vp[1] * 0.5], [0.0, vp[1] * 0.5, vp[0], vp[1] * 0.5]];
        } else if chase {
            rw.camera = poses[0];
            views = vec![[0.0, 0.0, vp[0], vp[1]]];
        } else {
            rw.camera = orbit_path(&race.track).pose(time);
            views = Vec::new();
        }
        if backdrop {
            rw.meshes.extend_from_slice(&st.statics.instances);
            rw.decals.extend_from_slice(&st.statics.decals);
        }
        // Each item box's disc seat on the road mesh, worked out again when
        // a box moves.
        if st.box_seats.len() != race.boxes.len() || st.box_seats.iter().zip(&race.boxes).any(|(s, b)| s.is_none_or(|s| s.at != b.pos)) {
            let seats: Vec<Option<BoxSeat>> = race.boxes.iter().map(|b| BoxSeat::new(&race.track, &st.road_ring, b.pos)).collect();
            st.box_seats = seats;
        }
        #[cfg(feature = "live")]
        let frame = focus.as_ref().and_then(|f| crate::focus::draw(f, &race, &t, &look, &st.box_seats, &mats, alpha, tf, st.shadows, &mut rw));
        #[cfg(feature = "live")]
        if let Some(fr) = frame
            && let Some(mut fs) = world.try_res_mut::<evani_core::focus::FocusState>()
        {
            rw.camera = fs.frame(fr);
            rw.views.clear();
        }
        #[cfg(feature = "live")]
        {
            shown = frame.is_some();
        }
        #[cfg(not(feature = "live"))]
        {
            shown = false;
        }
        if shown {
            // The focus turntable owns the camera and the whole canvas: no
            // chase or finish views, no minimap.
            views.clear();
        } else if still {
            // The geometry check's still world: the static scene and the
            // start lights only (no karts, items, effects or minimap).
            start_lights(&race, &mats, st.lean, &mut rw);
        } else {
            st.fade = if chase && !podium { kart_fades(&race, &poses, &views, t.kart_radius, alpha) } else { [1.0; crate::race::MAX_KARTS] };
            let mut rivals = RivalDraw::acquire(world, rival_parts, t.rival_model_scale);
            draw_dynamic(&race, &t, &look, &st.box_seats, &mats, alpha, tf, (chase, st.lean, st.shadows, st.budget), &st.fade, rivals.as_mut(), &mut rw);
        }
        // The ground shadow discs (Low's stand-ins for the shadow map, and the
        // item boxes') follow the look's shadow tint, like the shadows the
        // shader draws.
        if let Some(shade) = shade {
            let bases: Vec<[f32; 3]> = world
                .try_res::<LookData>()
                .map_or_else(Vec::new, |l| l.materials.iter().map(|m| [m.base_color[0], m.base_color[1], m.base_color[2]]).collect());
            for m in rw.decals.iter_mut().filter(|m| crate::scenery::is_shadow_disc(m)) {
                let base = bases.get(m.material as usize).copied().unwrap_or([1.0; 3]);
                m.color = disc_tinted(m.color, base, shade);
            }
        }
        // Not over the results panel, which would dim it but still show it.
        if vp[0] > 0.0 && vp[1] > 0.0 && !still && !shown && race.phase != Phase::Finished {
            let sizing = (vp[0].min(vp[1]), views.len() == 2);
            let lay = world
                .try_res::<crate::hud_layout::HudLayoutParams>()
                .map(|p| p.read(&world.res::<ParamRegistry>()))
                .unwrap_or_default();
            for (p, v) in views.iter().enumerate() {
                minimap(&race, *v, race.player_kart(p), sizing, &lay, &mut rw);
            }
        }
    }
    drop(race);
    #[cfg(feature = "models")]
    crate::rivals::flush_errors(world);
    // Speed lines read their drive from params (effects only see params).
    if !chase || shown || still {
        drives = [0.0; 2];
    }
    let split = if chase && views.len() == 2 { 1.0 } else { 0.0 };
    let want = [drives[0], drives[1], split];
    // Effects get no clock: the streaks scroll with a phase the kit drives.
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        if (want[0] > 0.0 || want[1] > 0.0) && reg.id("fx.speedlines.phase").is_some() {
            let ph = (tf * 1.3).fract();
            let _ = reg.set_by_name("fx.speedlines.phase", ParamValue::F32(ph));
        }
    }
    let mut reg = world.res_mut::<ParamRegistry>();
    for (j, name) in ["fx.speedlines.drive_a", "fx.speedlines.drive_b", "fx.speedlines.split"].iter().enumerate() {
        if (want[j] - st.drive[j]).abs() < 0.01 && !(want[j] == 0.0 && st.drive[j] != 0.0) {
            continue;
        }
        if reg.id(name).is_some() && reg.set_by_name(name, ParamValue::F32(want[j])).is_ok() {
            st.drive[j] = want[j];
        }
    }
}

/// Raise every shadow-mapped tier's filter radius to at least `want`
/// (`kit.kart_racer.shadow_soften`): Medium's single hardware tap on a
/// 1024 map over the track draws stair-stepped kart shadows, a 3x3 filter
/// draws them soft. `base` keeps each tier's own radius by id, so a smaller
/// `want` gives it back; tiers without a shadow map, and the stereo (VR)
/// tier with its two views per frame, are left alone.
fn soften_shadow_filter(tiers: &mut Tiers, base: &mut Vec<(String, Option<u32>)>, want: u32) {
    let mut changed = Vec::new();
    for p in tiers.presets() {
        if p.shadow_map.is_none() || p.stereo {
            continue;
        }
        let own = match base.iter().find(|(id, _)| *id == p.id) {
            Some(&(_, own)) => own,
            None => {
                base.push((p.id.clone(), p.shadow_pcf));
                p.shadow_pcf
            }
        };
        let pcf = if want > own.unwrap_or(0) { Some(want) } else { own };
        if p.shadow_pcf != pcf {
            let mut q = p.clone();
            q.shadow_pcf = pcf;
            changed.push(q);
        }
    }
    for q in changed {
        tiers.upsert(q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_soften_raises_single_tap_tiers_and_gives_them_back() {
        let Ok(mut tiers) = Tiers::builtin() else { panic!("builtin tiers") };
        let pcf = |t: &Tiers, id: &str| t.get(id).and_then(|p| p.shadow_pcf);
        let own: Vec<_> = tiers.presets().iter().map(|p| (p.id.clone(), p.shadow_pcf)).collect();
        let mut base = Vec::new();
        soften_shadow_filter(&mut tiers, &mut base, 1);
        assert_eq!(pcf(&tiers, "medium"), Some(1), "Medium gets the 3x3 filter");
        assert_eq!(pcf(&tiers, "ultra"), Some(1), "Ultra keeps its own");
        assert_eq!(pcf(&tiers, "vr"), Some(0), "stereo keeps its own");
        assert_eq!(pcf(&tiers, "low"), own.iter().find(|(i, _)| i == "low").and_then(|o| o.1), "no shadow map: untouched");
        soften_shadow_filter(&mut tiers, &mut base, 2);
        assert_eq!(pcf(&tiers, "ultra"), Some(2));
        soften_shadow_filter(&mut tiers, &mut base, 0);
        let back: Vec<_> = tiers.presets().iter().map(|p| (p.id.clone(), p.shadow_pcf)).collect();
        assert_eq!(back, own, "0 gives every tier its own radius back");
    }
    use evani_core::plugin::MaterialDesc;

    #[test]
    fn the_minimap_is_top_right_under_the_timer_at_the_layout_sizes() {
        // 1080p: u = 10.8 clamps to 11; a phone at 844x390: u = 3.9 clamps to 5.
        let (u1, u2) = (hud_unit(1080.0, false), hud_unit(390.0, false));
        assert_eq!((u1, u2), (10.8, 5.0));
        assert!((minimap_size(u1, false) - 150.0).abs() < 0.5);
        assert_eq!(minimap_size(11.0, false), 150.0);
        assert!((minimap_size(u2, false) - 90.0).abs() < 0.5);
        // Split screen: 100 px at 1080p and 64 px on a phone (full canvas).
        assert!((minimap_size(hud_unit(1080.0, true), true) - 100.0).abs() < 0.5);
        assert!((minimap_size(hud_unit(390.0, true), true) - 64.0).abs() < 0.5);
        // Right inset 2.2 u, top 1.4 u + timer + 0.8 u gap: about 69 px at
        // 1080p and 32 px on a phone, the layout's 24 + 15 + 45 + 9 and 11 + 7 + 21 + 4.
        let m = minimap_size(u1, false);
        let [x, y] = minimap_origin([0.0, 0.0, 1920.0, 1080.0], u1, m);
        assert!((1920.0 - (x + m) - 2.2 * u1).abs() < 0.01);
        assert!((y - 70.0).abs() < 3.0, "{y}");
        let [_, y] = minimap_origin([0.0, 0.0, 844.0, 390.0], u2, minimap_size(u2, false));
        assert!((y - 32.0).abs() < 2.0, "{y}");
    }

    /// Asset focus over a running race: the game's loops stop and stay
    /// stopped, and the turntable (not the chase camera, with its rival lift
    /// and shake) sets the camera and the single full-canvas view.
    #[cfg(feature = "live")]
    #[test]
    fn a_focused_asset_is_silent_and_the_turntable_owns_the_camera() {
        use evani_core::app::App;
        use evani_core::focus::FocusState;
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(crate::KartRacerPlugin)
            .with_look(evani_look_basic::BasicLook);
        a.start();
        a.world.res_mut::<RenderWorld>().viewport_css = [1280.0, 720.0];
        a.world.res_mut::<crate::race::Race>().request(crate::race::Request::Start { players: 1 });
        a.run_headless(60 * 5, 1000.0 / 60.0);
        assert!(a.world.res::<DrawState>().voices.is_running(), "the race has its engine going");
        a.world.insert_resource(FocusState::default());
        a.world.res_mut::<FocusState>().focus("kart");
        a.run_headless(30, 1000.0 / 60.0);
        assert!(!a.world.res::<DrawState>().voices.is_running(), "silent while focused");
        let fs = a.world.res::<FocusState>().clone();
        let f = fs.last.expect("the kart was drawn on the turntable");
        let eye = a.world.res::<RenderWorld>().camera.eye;
        let d = ((eye[0] - f.center[0]).powi(2) + (eye[1] - f.center[1]).powi(2) + (eye[2] - f.center[2]).powi(2)).sqrt();
        assert!((d - fs.distance_for(f.radius)).abs() < 0.01, "turntable distance {d}");
        assert!(a.world.res::<RenderWorld>().views.is_empty(), "one full-canvas view");
        a.world.res_mut::<FocusState>().clear();
        a.run_headless(30, 1000.0 / 60.0);
        assert!(a.world.res::<DrawState>().voices.is_running(), "the race sounds come back");
    }

    /// The chase camera in a one-player pack race on the autopilot, from the
    /// grid through the first laps: every 4th frame, rays over Pip's screen
    /// box (24 x 24) are cast against Pip's and the rivals' boxes, each rival
    /// in front of Pip hiding it by its drawn fade. No rival may hide more
    /// than 30% of Pip, no rival still drawn may come within 1 m of the eye
    /// (the near plane is 0.1 m), and the fade must have done some work.
    #[test]
    fn in_a_pack_race_no_rival_hides_pip_or_meets_the_camera() {
        use evani_core::app::App;
        let mut a = App::new("t")
            .with_module(evani_render::RenderModule::new())
            .with_kit(crate::KartRacerPlugin)
            .with_look(evani_look_basic::BasicLook);
        a.start();
        let (w, h) = (1280.0f32, 720.0f32);
        a.world.res_mut::<RenderWorld>().viewport_css = [w, h];
        a.world.res_mut::<Race>().request(crate::race::Request::Start { players: 1 });
        a.run_headless(1, 1000.0 / 60.0);
        let (mut worst, mut worst_at, mut faded, mut samples) = (0.0f32, 0usize, 0usize, 0usize);
        let (mut nearest_drawn, mut nearest_raw, mut worst_raw) = (f32::MAX, f32::MAX, 0.0f32);
        let mut brain = crate::ai::AiState::default();
        let mut rng = evani_core::rng::Pcg32::new(0xCA3, 1);
        let mut slow = false;
        for frame in 0..60 * 50 {
            // Pip launches with a start boost into the pack, then follows the
            // racing line: flat out while most of the pack is ahead, lifting
            // off once it leads them, so the pack keeps coming past the
            // camera.
            let (steer, throttle) = {
                let (_, c, _) = crate::ctx(&a.world).expect("kit built");
                let r = RaceView::new(&a.world);
                let me = r.player_kart(0).expect("one player");
                let k = &r.karts[me];
                let ahead = r.karts.iter().filter(|o| o.s > k.s).count();
                if r.phase == Phase::Countdown {
                    (0.0, if r.countdown < c.t.start_boost_window_s * 0.6 { 1.0 } else { 0.0 })
                } else if r.phase == Phase::Racing && k.finished.is_none() {
                    let around = crate::ai::surroundings(&r.karts, me, &r.track);
                    let ctl = crate::ai::drive(k, &r.roster[k.spec], &mut brain, around, &r.track, &c.t, rng.next_f32(), c.dt);
                    slow = if ahead <= 2 { true } else if ahead >= 6 { false } else { slow };
                    (ctl.steer, if slow { 0.35 } else { ctl.throttle })
                } else {
                    (0.0, 0.0)
                }
            };
            let mut buttons = [0.0f32; 16];
            buttons[7] = throttle;
            a.world.res_mut::<evani_core::input::Input>().set_pad(0, true, &buttons, &[steer, 0.0, 0.0, 0.0]);
            a.run_headless(1, 1000.0 / 60.0);
            if frame % 4 != 0 {
                continue;
            }
            let race = RaceView::new(&a.world);
            let Some(me) = race.player_kart(0) else { continue };
            if !matches!(race.phase, Phase::Countdown | Phase::Racing) || race.karts[me].finished.is_some() {
                continue;
            }
            let alpha = a.world.res::<Time>().alpha;
            let cam = a.world.res::<RenderWorld>().camera;
            let fade = a.world.res::<DrawState>().fade;
            let pip = kart_block(&race, me, 1.1, alpha);
            let rivals: Vec<(crate::chasecam::Block, f32)> = (0..race.karts.len()).filter(|&j| j != me).map(|j| (kart_block(&race, j, 1.1, alpha), fade[j])).collect();
            for (b, f) in &rivals {
                nearest_raw = nearest_raw.min(b.box_distance(cam.eye));
                if *f > 0.0 {
                    nearest_drawn = nearest_drawn.min(b.box_distance(cam.eye));
                }
                if *f < 1.0 {
                    faded += 1;
                }
            }
            // Rays through the screen: the camera's basis, signed so a point
            // along `right` projects to the right and along `up` upwards.
            let vp = evani_render::shaders::view_proj(&cam, w / h);
            let fwd = normalize3([cam.target[0] - cam.eye[0], cam.target[1] - cam.eye[1], cam.target[2] - cam.eye[2]]);
            let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let mut right = normalize3(cross(fwd, cam.up));
            let mut up = cross(right, fwd);
            if evani_render::math::transform_point(&vp, add(add(cam.eye, fwd), right))[0] < 0.0 {
                right = [-right[0], -right[1], -right[2]];
            }
            if evani_render::math::transform_point(&vp, add(add(cam.eye, fwd), up))[1] < 0.0 {
                up = [-up[0], -up[1], -up[2]];
            }
            let th = (cam.fov_y_deg.to_radians() * 0.5).tan();
            let Some(r) = crate::chasecam::screen_rect(&vp, &pip) else { continue };
            let (mut on_pip, mut hidden, mut raw) = (0usize, 0.0f32, 0usize);
            for gy in 0..24 {
                for gx in 0..24 {
                    let x = (r[0] + (r[2] - r[0]) * (gx as f32 + 0.5) / 24.0).clamp(-1.0, 1.0);
                    let y = (r[1] + (r[3] - r[1]) * (gy as f32 + 0.5) / 24.0).clamp(-1.0, 1.0);
                    let (sx, sy) = (x * th * w / h, y * th);
                    let d = normalize3([fwd[0] + right[0] * sx + up[0] * sy, fwd[1] + right[1] * sx + up[1] * sy, fwd[2] + right[2] * sx + up[2] * sy]);
                    let Some(tp) = pip.ray_hit(cam.eye, d) else { continue };
                    on_pip += 1;
                    let seen: f32 = rivals.iter().filter(|(b, _)| b.ray_hit(cam.eye, d).is_some_and(|t| t < tp)).map(|(_, f)| 1.0 - f).product();
                    hidden += 1.0 - seen;
                    raw += usize::from(rivals.iter().any(|(b, _)| b.ray_hit(cam.eye, d).is_some_and(|t| t < tp)));
                }
            }
            if on_pip > 20 {
                samples += 1;
                worst_raw = worst_raw.max(raw as f32 / on_pip as f32);
                let share = hidden / on_pip as f32;
                if share > worst {
                    (worst, worst_at) = (share, frame);
                }
            }
        }
        eprintln!(
            "pack race: {samples} samples, worst cover {worst:.3} (frame {worst_at}; {worst_raw:.3} undimmed), {faded} fades, nearest drawn rival {nearest_drawn:.2} m (any rival {nearest_raw:.2} m)"
        );
        assert!(samples > 400, "{samples} samples");
        assert!(worst < 0.3, "a rival hid {:.0}% of Pip at frame {worst_at}", worst * 100.0);
        assert!(nearest_drawn >= 1.0, "a drawn rival came {nearest_drawn} m from the eye");
        // The race really put rivals in the way: without the fade, the test
        // would fail.
        assert!(faded > 0 && (worst_raw >= 0.3 || nearest_raw < 1.0), "the race never tested the fade ({faded} fades, {worst_raw} cover, {nearest_raw} m)");
    }

    fn blast_at(t: f32) -> crate::items::Blast {
        crate::items::Blast { pos: [10.0, 1.0, -4.0], radius: 4.0, t }
    }

    #[test]
    fn a_blast_is_flat_candy_shapes_that_stay_low_and_never_glow() {
        let mut mats = resolve_materials(&LookData::default());
        // Told apart by index: lit candy roles 3 and 4, glowing roles 5 and 6.
        let plain = |index| Mat { index, tint: [1.0; 3] };
        mats[role::KART_BODY] = plain(3);
        mats[role::KERB_WHITE] = plain(4);
        mats[role::SPARK] = plain(5);
        mats[role::BOOST_FLAME] = plain(6);
        for &lean in &[false, true] {
            let (mut min_pieces, mut max_pieces) = (usize::MAX, 0);
            for step in 0..=16 {
                let t = crate::items::BLAST_LIFE_S * step as f32 / 16.0;
                let mut rw = RenderWorld::default();
                draw_blast(&blast_at(t), &mats, 0.3, lean, &mut rw);
                let n = rw.meshes.len();
                min_pieces = min_pieces.min(n);
                max_pieces = max_pieces.max(n);
                for m in &rw.meshes {
                    // Lit candy materials: the toon bands and the outline draw
                    // them, and bloom has nothing bright to wash out.
                    assert!(m.material == 3 || m.material == 4, "a glowing piece at t = {t}");
                    assert!(m.color[..3].iter().all(|&c| c <= 1.01), "bright tint {:?} at t = {t}", m.color);
                    // Low: nothing above about 2.5 m over the road (the camera
                    // sees over it; the blast sat at y = 1.0, road 0.55 m below).
                    assert!(m.position[1] < 0.55 + 2.7, "a piece {} m up at t = {t}", m.position[1] - 0.55);
                }
                // A ring, a crown, a dome and debris: never a single disc.
                assert!(n >= 12, "only {n} pieces at t = {t}");
            }
            assert!(max_pieces <= 48, "{max_pieces} pieces in one blast");
            assert!(min_pieces >= 12);
        }
    }

    #[test]
    fn a_blast_draws_the_same_pieces_every_time_and_the_ring_races_out() {
        let mats = resolve_materials(&LookData::default());
        let (mut a, mut b) = (RenderWorld::default(), RenderWorld::default());
        draw_blast(&blast_at(0.3), &mats, 1.0, false, &mut a);
        draw_blast(&blast_at(0.3), &mats, 1.0, false, &mut b);
        assert_eq!(a.meshes, b.meshes, "deterministic");
        // The ring races out.
        let ring_r = |t: f32| {
            let mut rw = RenderWorld::default();
            draw_blast(&blast_at(t), &mats, 0.0, false, &mut rw);
            let m = rw.meshes[0];
            ((m.position[0] - 10.0).powi(2) + (m.position[2] + 4.0).powi(2)).sqrt()
        };
        assert!(ring_r(0.5) > ring_r(0.1) + 1.0);
    }

    #[test]
    fn a_boost_leaves_an_orange_spark_trail_behind_the_exhausts() {
        let mut mats = resolve_materials(&LookData::default());
        mats[role::SPARK] = Mat { index: 5, tint: [1.0; 3] };
        let mut k = crate::kart::Kart::new(0, Some(0), [0.0; 3], 0.0);
        k.boost = 1.0;
        let at = |o: [f32; 3]| o;
        let mut rw = RenderWorld::default();
        boost_trail(&k, &mats, 0.37, false, &at, [0.0, 0.0, 0.0, 1.0], &mut rw);
        assert_eq!(rw.meshes.len(), 18);
        for m in &rw.meshes {
            assert!(m.position[2] < -1.9, "behind the kart: z = {}", m.position[2]);
            // Orange: red well over green well over blue.
            assert!(m.color[0] > 2.0 * m.color[1] && m.color[1] > 4.0 * m.color[2], "{:?}", m.color);
        }
        let mut lean = RenderWorld::default();
        boost_trail(&k, &mats, 0.37, true, &at, [0.0, 0.0, 0.0, 1.0], &mut lean);
        assert_eq!(lean.meshes.len(), 2 * LOW_SPARKS);
        // Low (no bloom): small lit, outlined orange and yellow sparks (not the
        // unlit spark material, which clips to cream there), in a short trail
        // close behind the exhausts: never long bars reaching to the camera.
        for m in &lean.meshes {
            assert_eq!(m.material, mats[role::KART_BODY].index);
            assert!(m.color[..3] == LOW_FLAME_ORANGE || m.color[..3] == LOW_FLAME_YELLOW, "{:?}", m.color);
            assert!(m.scale.iter().all(|&s| s <= 0.25), "small sparks, not planks: {:?}", m.scale);
            assert!((-1.95 - LOW_SPARK_REACH - 0.01..-1.9).contains(&m.position[2]), "a short trail: z = {}", m.position[2]);
            assert!(m.position[1] > 0.2, "in the air, not lying on the road: y = {}", m.position[1]);
        }
        assert!(lean.meshes.iter().filter(|m| m.color[..3] == LOW_FLAME_ORANGE).count() > LOW_SPARKS, "mostly orange");
    }

    #[test]
    fn low_draws_a_lit_outlined_flame_shape_leaning_back_and_up() {
        let mats = resolve_materials(&LookData::default());
        let at = |o: [f32; 3]| o;
        let mut rw = RenderWorld::default();
        low_flames(1.0, &mats, &at, [0.0, 0.0, 0.0, 1.0], &mut rw);
        assert_eq!(rw.meshes.len(), 4);
        for m in &rw.meshes {
            // Lit (toon bands and ink), behind the kart.
            assert_eq!(m.material, mats[role::KART_BODY].index);
            assert!(m.position[2] < -1.8, "behind the kart: z = {}", m.position[2]);
        }
        let cones: Vec<_> = rw.meshes.iter().filter(|m| m.mesh == MeshId::CONE).collect();
        assert_eq!(cones.len(), 2);
        for c in cones {
            // The tip (local +y) points up and back.
            let tip = quat_rotate(c.rotation, [0.0, 1.0, 0.0]);
            assert!(tip[1] > 0.4 && tip[2] < -0.6, "tip {tip:?}");
            assert_eq!(c.color[..3], LOW_FLAME_ORANGE);
        }
        // About 250 triangles per boosting kart.
        let tris: u32 = rw.meshes.iter().map(|m| if m.mesh == MeshId::CONE { 24 } else { 100 }).sum();
        assert!(tris <= 250, "{tris}");
    }
    #[test]
    fn the_player_marker_is_hidden_in_the_one_player_chase_view_and_shows_elsewhere() {
        use crate::finale::{Stage, stage};
        let racing = stage(Phase::Racing, 1.0, 1);
        // 1P chase (countdown, racing): off, the kart is fixed in the frame.
        assert!(!marker_shown(1, racing, true, false));
        assert!(!marker_shown(1, stage(Phase::Countdown, 1.0, 1), true, false));
        // Split screen: both players' karts wear one.
        assert!(marker_shown(2, racing, true, false));
        assert!(marker_shown(2, stage(Phase::Countdown, 1.0, 2), true, false));
        // Over the line the camera orbits (a replay-style view): on, also
        // once the one-player race is Finished (critic round 4: the finish
        // orbit had none, as the race ends when the player crosses).
        assert!(marker_shown(1, racing, true, true));
        assert!(marker_shown(1, stage(Phase::Finished, 0.5, 1), true, true));
        assert!(marker_shown(2, stage(Phase::Finished, 0.5, 2), true, true));
        // Rivals never wear one; nobody does over the results panel.
        assert!(!marker_shown(2, racing, false, false));
        assert!(!marker_shown(1, racing, false, true));
        assert!(!marker_shown(2, Stage::Podium, true, true));
        assert!(!marker_shown(1, stage(Phase::Finished, crate::finale::CELEBRATE_S, 1), true, true));
    }

    #[test]
    fn the_finish_marker_is_36_px_at_1080p_and_20_px_on_a_phone() {
        assert_eq!(marker_px(1080.0), 36.0);
        assert_eq!(marker_px(390.0), 20.0);
        assert_eq!(marker_px(2160.0), 36.0);
        // Its world height over the view's height at that distance, times
        // the view's pixels, is the target: 9.5 m away with a 62 degree FOV.
        for (vh, vmin) in [(1080.0f32, 1080.0f32), (390.0, 390.0)] {
            let s = marker_scale(9.5, 62.0, vh, vmin);
            let px = s * MARKER_H / (2.0 * 9.5 * 31f32.to_radians().tan()) * vh;
            assert!((px - marker_px(vmin)).abs() < 0.01, "{px}");
        }
        assert_eq!(marker_scale(9.5, 62.0, 0.0, 0.0), 1.0);
    }

    #[test]
    fn the_shadow_tint_turns_a_shadow_toward_purple_at_equal_brightness_and_is_off_at_zero() {
        let purple = linear(0xa98bf0);
        assert_eq!(shadow_shade(purple, 0.0), None, "strength 0 leaves colours alone");
        assert_eq!(shadow_shade(purple, -1.0), None);
        let full = shadow_shade(purple, 1.0).unwrap();
        // The dark green of a shadowed meadow: at full strength it is lavender
        // (blue and red over green) at about the same brightness.
        let grass = [0.12, 0.3, 0.06];
        let out = tint_shadow(grass, full);
        assert!(out[2] > out[1] && out[0] > out[1], "purple: blue and red over green, {out:?}");
        let (lg, lo) = (luma(grass.map(f32::sqrt)), luma(out.map(f32::sqrt)));
        assert!((lo - lg).abs() < 0.12, "brightness kept: {lg} -> {lo}");
        assert!(out.iter().all(|c| (0.0..=1.0).contains(c)), "{out:?}");
        // Half strength lies between the shadow and the full tint.
        let half = tint_shadow(grass, shadow_shade(purple, 0.5).unwrap());
        assert!(half[2] > grass[2] && half[2] < out[2], "{half:?}");
        // A neutral tint only greys a colour at the same brightness.
        let grey = tint_shadow([0.3; 3], shadow_shade([0.4; 3], 1.0).unwrap());
        for c in grey {
            assert!((c - 0.3).abs() < 1e-4, "{grey:?}");
        }
        // A disc on a green meadow material: what it draws as (colour times
        // the material) turns lavender, not just its instance colour.
        let (base, colour) = ([0.3, 0.7, 0.2], [0.5, 0.6, 0.5, 1.0]);
        let disc = disc_tinted(colour, base, full);
        let shown = [disc[0] * base[0], disc[1] * base[1], disc[2] * base[2]];
        assert!(shown[2] > shown[1] && shown[0] > shown[1], "the disc draws purple: {shown:?}");
        assert_eq!(disc[3], 1.0);
    }

    #[test]
    fn hex_colours_convert_to_linear() {
        let pip = linear(0xff4fa3);
        assert!((pip[0] - 1.0).abs() < 1e-3 && (pip[1] - 0.078).abs() < 2e-3 && (pip[2] - 0.366).abs() < 3e-3, "{pip:?}");
    }

    #[test]
    fn start_lights_count_down_then_go_green() {
        use Light::{Green as G, Off as O, Red as R};
        assert_eq!(start_light_states(Phase::Countdown, 3.0, 0.0), [R, O, O]);
        assert_eq!(start_light_states(Phase::Countdown, 2.5, 0.5), [R, O, O]);
        assert_eq!(start_light_states(Phase::Countdown, 1.5, 1.5), [R, R, O]);
        assert_eq!(start_light_states(Phase::Countdown, 0.2, 2.8), [R, R, R]);
        assert_eq!(start_light_states(Phase::Racing, 0.0, 0.1), [G, G, G]);
        assert_eq!(start_light_states(Phase::Racing, 0.0, GREEN_S + 0.1), [O, O, O]);
        assert_eq!(start_light_states(Phase::Title, 0.0, 0.1), [O, O, O]);
    }

    #[test]
    fn low_start_lights_are_clean_red_and_green_paint() {
        // No pale glow added on top: the other channels stay a few percent
        // of the main one, so the lights read red and green without bloom.
        let [r, g, b] = low_light_colour(Light::Red);
        assert!(r >= 0.9 && g < r * 0.05 && b < r * 0.05, "{:?}", [r, g, b]);
        let [r, g, b] = low_light_colour(Light::Green);
        assert!(g >= 0.9 && r < g * 0.1 && b < g * 0.3, "{:?}", [r, g, b]);
        // An unlit light is a dark red, well under the lit one.
        assert!(low_light_colour(Light::Off)[0] < 0.3);
    }

    #[test]
    fn glowing_start_lights_stay_red_through_the_tonemap() {
        // Critic round 4: on High and Ultra the red lights bloomed pale
        // pink-white. Lit paint pushed over the bloom threshold (2.0 in the
        // toon look) keeps its hue: the toon tonemap's per-channel curve of
        // a channel at a few percent of the red one stays dark.
        let filmic = |c: f32| (c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14);
        for lit in [1.0f32, 1.71] {
            let [r, g, b] = glow_light_colour(Light::Red).map(|c| c * 0.95 * lit);
            assert!(r > 2.0, "glows: {r}");
            assert!(filmic(r) > 0.9 && filmic(g) < 0.08 && filmic(b) < 0.08, "{:?}", [filmic(r), filmic(g), filmic(b)]);
        }
        assert_eq!(glow_light_colour(Light::Off), low_light_colour(Light::Off), "an unlit light does not glow");
        let [r, g, b] = glow_light_colour(Light::Green);
        assert!(g > 2.0 && r < g * 0.1, "{:?}", [r, g, b]);
    }

    #[test]
    fn missing_roles_fall_back_with_a_tint() {
        let mut look = LookData::default();
        look.materials.push(MaterialDesc::new("ground", [0.5, 0.5, 0.5, 1.0]));
        let mut g = MaterialDesc::new("glow", [1.0, 0.5, 0.25, 1.0]);
        g.unlit = true;
        look.materials.push(g);
        look.materials.push(MaterialDesc::new("road", [0.4, 0.4, 0.5, 1.0]));
        let m = resolve_materials(&look);
        assert_eq!(m[role::ROAD], Mat { index: 2, tint: [1.0; 3] });
        assert_eq!(m[role::GRASS].index, 0);
        assert!((m[role::GRASS].tint[1] - 0.78 / 0.5).abs() < 1e-5);
        // Emissive roles prefer the look's glow material.
        assert_eq!(m[role::SPARK].index, 1);
    }

    #[test]
    fn statics_cover_the_whole_track() {
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let mats = resolve_materials(&LookData::default());
        let mut st = Statics::default();
        build_statics(&track, &mats, 7.0, u32::MAX, true, &mut st);
        // Ground, the road, grass, red and white track meshes, start line,
        // trees and mountains; the paint mesh and the start marks as decals.
        let out = &st.instances;
        assert!(out.len() >= 5 + track.trees.len() * 2 + track.scenery.mountains.len());
        assert!(out.iter().all(|m| m.position.iter().all(|v| v.is_finite())));
        assert_eq!(st.meshes.len(), crate::trackmesh::COUNT + crate::propmesh::COUNT + MEADOW_MESHES);
        for k in 0..MEADOW_MESHES {
            assert_eq!(out.iter().filter(|m| m.mesh == meadow_mesh_id(k)).count(), 1, "meadow mesh {k}");
        }
        for k in 0..crate::trackmesh::COUNT {
            let drawn = out.iter().chain(&st.decals).filter(|m| m.mesh == track_mesh_id(k)).count();
            assert_eq!(drawn, 1, "track mesh {k} drawn {drawn} times");
        }
        // The paint is a decal; so are the start strip, the grid brackets
        // and the grass blobs (one to three a patch). The banner's chequers
        // are solid boxes.
        assert!(st.decals.iter().any(|m| m.mesh == track_mesh_id(crate::trackmesh::PAINT)));
        let patches = st.decals.iter().filter(|m| m.mesh == crate::propmesh::id(crate::propmesh::BLOB)).count();
        assert!(patches >= 60 && patches <= 3 * GRASS_PATCHES as usize, "{patches} grass blobs");
        // And the road's wear (stains, repair patches, skid marks).
        let wear = crate::roadmarks::road_marks(&track, mats[role::ROAD], false, 7.0).len();
        assert!(wear > 60);
        assert_eq!(st.decals.len() - patches - wear, 1 + crate::trackmesh::CHEQ_MESHES + 12 * 3);
    }

    /// The grid brackets and every item box's disc (at its default size and
    /// twice that) lie just over the road mesh's own
    /// triangles, found by brute force over the built mesh: never closer
    /// than ROAD_CLEAR (dipping in, the asphalt would fight them), never
    /// floating more than a centimetre over it.
    #[test]
    fn grid_brackets_and_item_box_discs_lie_just_over_the_road() {
        use crate::trackmesh::tests::{height_over, road_tops};
        use crate::trackmesh::ROAD_CLEAR;
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let mats = resolve_materials(&LookData::default());
        let tops = road_tops(&crate::trackmesh::build(&track, 7.0)[crate::trackmesh::ROAD]);
        // Lowest and highest gap over the road across a footprint
        // (`round`: the disc's 12-gon, else the plane's square).
        let gaps = |m: &MeshInstance, round: bool| {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            let mut pts: Vec<[f32; 2]> = Vec::new();
            for i in 0..=20 {
                for j in 0..=20 {
                    pts.push([i as f32 / 20.0 - 0.5, j as f32 / 20.0 - 0.5]);
                }
            }
            if round {
                pts.retain(|p| p[0].hypot(p[1]) < 0.5 * (core::f32::consts::PI / 12.0).cos());
                pts.extend((0..12).map(|i| {
                    let a = i as f32 / 12.0 * core::f32::consts::TAU;
                    [0.5 * a.cos(), 0.5 * a.sin()]
                }));
            }
            for [u, v] in pts {
                let o = quat_rotate(m.rotation, [u * m.scale[0], 0.0, v * m.scale[2]]);
                let w = add(m.position, o);
                if let Some(h) = height_over(&tops, w[0], w[2]) {
                    lo = lo.min(w[1] - h);
                    hi = hi.max(w[1] - h);
                }
            }
            (lo, hi)
        };
        let mut st = Statics::default();
        build_statics(&track, &mats, 7.0, u32::MAX, true, &mut st);
        let brackets: Vec<&MeshInstance> = st.decals.iter().filter(|m| m.mesh == MeshId::PLANE && m.scale[1] == DECAL_THICK && ((m.scale[0] - 2.4).abs() < 1e-4 && (m.scale[2] - 0.22).abs() < 0.005 || (m.scale[0] - 0.22).abs() < 1e-4 && (m.scale[2] - 1.8).abs() < 0.005)).collect();
        assert_eq!(brackets.len(), 12 * 3, "{:?}", st.decals.iter().filter(|m| m.mesh == MeshId::PLANE && m.scale[1] == DECAL_THICK).map(|m| m.scale).take(5).collect::<Vec<_>>());
        for m in brackets {
            let (lo, hi) = gaps(m, false);
            assert!(lo > ROAD_CLEAR - 0.0005 && hi < ROAD_CLEAR + 0.01, "a grid bracket at {:?} is {lo}..{hi} m over the road", m.position);
        }
        // The item boxes where Race::reset puts them.
        let ring = crate::roadmarks::mesh_rings(&track, 7.0);
        let per = track.item_boxes_per_row.max(1);
        let look = ItemBoxLook::default();
        let mut seen = 0;
        for &row in &track.item_rows {
            for b in 0..per {
                let lat = ((b as f32 + 0.5) / per as f32 * 2.0 - 1.0) * track.half_width * 0.75;
                let mut p = track.point_at(row * track.length, lat);
                p[1] += 1.0;
                let seat = BoxSeat::new(&track, &ring, p).expect("a box over the road");
                for radius in [look.shadow_radius * look.size, 2.0 * look.shadow_radius * look.size] {
                    let mut rw = RenderWorld::default();
                    let mut l = look;
                    l.shadow_radius = radius / l.size;
                    draw_item_box(&crate::items::ItemBox { pos: p, hidden: 0.0 }, 0, &l, Some(&seat), &mats, 0.0, false, &mut rw);
                    let d = rw.decals.last().expect("the disc");
                    let (lo, hi) = gaps(d, true);
                    // A disc twice the size reaches further over the crests.
                    let float = if radius > 1.5 { 0.02 } else { 0.01 };
                    assert!(lo > ROAD_CLEAR - 0.0005 && hi < ROAD_CLEAR + float, "box at {p:?} radius {radius}: disc {lo}..{hi} m over the road");
                    seen += 1;
                }
            }
        }
        assert!(seen >= 6, "{seen} discs");
    }

    #[test]
    fn the_meadow_is_mown_in_wide_stripes_with_no_cracks_or_overlaps() {
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let half = crate::scenery::FAR_GROUND_M * 0.5;
        let m = meadow(&track, half);
        let tris: Vec<(usize, [[f32; 3]; 3])> = m
            .iter()
            .enumerate()
            .flat_map(|(k, mesh)| {
                mesh.indices.as_chunks::<3>().0.iter().map(move |t| (k, t.map(|i| {
                    let v = mesh.vertices[i as usize];
                    [v[0], v[1], v[2]]
                })))
            })
            .collect();
        // Flat at the meadow's height, facing up, none degenerate.
        let area = |t: &[[f32; 3]; 3]| 0.5 * ((t[1][2] - t[0][2]) * (t[2][0] - t[0][0]) - (t[1][0] - t[0][0]) * (t[2][2] - t[0][2]));
        for (_, t) in &tris {
            assert!(t.iter().all(|v| v[1] == track.meadow_y));
            assert!(area(t) > 1e-3, "{t:?}");
        }
        // Together they cover the square once: their areas add up to it,
        // and points across it lie in exactly one triangle.
        let total: f64 = tris.iter().map(|(_, t)| area(t) as f64).sum();
        assert!((total / (4.0 * half as f64 * half as f64) - 1.0).abs() < 1e-4, "{total}");
        let inside = |t: &[[f32; 3]; 3], x: f32, z: f32| {
            let s = |a: [f32; 3], b: [f32; 3]| (b[2] - a[2]) * (x - a[0]) - (b[0] - a[0]) * (z - a[2]);
            s(t[0], t[1]) >= 0.0 && s(t[1], t[2]) >= 0.0 && s(t[2], t[0]) >= 0.0
        };
        let b = track.bounds;
        let mid = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
        let mut by_kind = [0; MEADOW_MESHES];
        for i in 0..60 {
            for j in 0..60 {
                // Off any grid line: a little jitter.
                let (x, z) = (mid[0] - 600.0 + i as f32 * 20.13 + 0.37, mid[1] - 600.0 + j as f32 * 20.11 + 0.29);
                let hits: Vec<usize> = tris.iter().filter(|(_, t)| inside(t, x, z)).map(|(k, _)| *k).collect();
                assert_eq!(hits.len(), 1, "({x}, {z}) lies in {} triangles", hits.len());
                by_kind[hits[0]] += 1;
            }
        }
        // Light and dark stripes over the track's field, plain beyond.
        assert!(by_kind[MEADOW_LIGHT] > 100 && by_kind[MEADOW_DARK] > 100 && by_kind[MEADOW_PLAIN] > 100, "{by_kind:?}");
        // Every corner of the track's bounds lies on a stripe; and stepping
        // across the start straight, light and dark take turns every stripe.
        for (x, z) in [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])] {
            let k = tris.iter().find(|(_, t)| inside(t, x, z)).unwrap().0;
            assert_ne!(k, MEADOW_PLAIN, "({x}, {z})");
        }
        let p0 = track.point_at(0.0, 0.0);
        let tan = track.tangent_at(0.0);
        let mut last = None;
        for s in 0..12 {
            let lat = (s as f32 - 6.0) * STRIPE_W;
            let (x, z) = (p0[0] + tan[1] * lat, p0[2] - tan[0] * lat);
            let k = tris.iter().find(|(_, t)| inside(t, x, z)).unwrap().0;
            assert_ne!(Some(k), last, "stripe {s}");
            last = Some(k);
        }
        // Flat and cheap: Low's budget has room for them.
        let n: u32 = m.iter().map(Mesh::triangles).sum();
        assert!(n < 250, "{n} triangles");
    }

    #[test]
    fn grass_patches_break_up_the_meadow_in_three_greens() {
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let mats = resolve_materials(&LookData::default());
        for budget in [100_000, u32::MAX] {
            let mut st = Statics::default();
            build_statics(&track, &mats, 7.0, budget, budget == u32::MAX, &mut st);
            let blobs: Vec<_> = st.decals.iter().filter(|m| m.mesh == crate::propmesh::id(crate::propmesh::BLOB)).collect();
            let g = mats[role::GRASS].tint;
            for (k, t) in GRASS_TONES.iter().enumerate() {
                let c = [g[0] * t[0], g[1] * t[1], g[2] * t[2], 1.0];
                assert!(blobs.iter().filter(|m| m.color == c).count() >= 15, "green {k}");
            }
            // Never taken for a shadow disc; lying on the meadow.
            assert!(blobs.iter().all(|m| !crate::scenery::is_shadow_disc(m) && (m.position[1] - track.meadow_y).abs() < 0.01));
            // Blobs of different greens never overlap.
            let reach = |m: &MeshInstance| 0.5 * m.scale[0].max(m.scale[2]);
            for (i, a) in blobs.iter().enumerate() {
                for b in &blobs[i + 1..] {
                    if a.color != b.color {
                        let d = (a.position[0] - b.position[0]).hypot(a.position[2] - b.position[2]);
                        assert!(d >= reach(a) + reach(b), "{:?} and {:?} overlap", a.position, b.position);
                    }
                }
            }
            // Across the infield, and in the podium shot's foreground: 4 to
            // 13 m from its camera, in front of it, clear of the podium.
            let infield = blobs.iter().filter(|m| crate::scenery::inside_loop(&track, m.position[0], m.position[2])).count();
            assert!(infield >= 20, "{infield} blobs in the infield");
            let site = crate::finale::podium_site(&track, 7.0);
            let cam = crate::finale::podium_camera(&site, 16.0 / 9.0);
            let f = [cam.target[0] - cam.eye[0], 0.0, cam.target[2] - cam.eye[2]];
            let fore = blobs
                .iter()
                .filter(|m| {
                    let d = [m.position[0] - cam.eye[0], 0.0, m.position[2] - cam.eye[2]];
                    let flat = d[0].hypot(d[2]);
                    let ahead = (d[0] * f[0] + d[2] * f[2]) / flat / f[0].hypot(f[2]);
                    (4.0..=13.0).contains(&flat) && ahead > 0.7 && (m.position[0] - site.pos[0]).hypot(m.position[2] - site.pos[2]) > PODIUM_GRASS_CLEAR
                })
                .count();
            assert!(fore >= 4, "{fore} blobs in the podium shot's foreground");
        }
    }

    #[test]
    fn small_triangle_budgets_draw_a_third_of_the_trees() {
        use crate::scenery::{PropKind, layout};
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let trees = |thin: bool| -> Vec<crate::scenery::Prop> {
            layout(&track, 7.0, thin)
                .into_iter()
                .filter(|p| matches!(p.kind, PropKind::Tree | PropKind::LollipopTree | PropKind::FlossBush))
                .collect()
        };
        // Every tree of the track file, less the few that would stand in a
        // mountain or on the road's bank; Low keeps every third of them.
        let full = trees(false);
        assert!(full.len() + 12 >= track.trees.len() && full.len() <= track.trees.len(), "{} of {}", full.len(), track.trees.len());
        let third: Vec<_> = full.iter().filter(|p| (p.seed as usize).is_multiple_of(THIN_TREE_STEP)).copied().collect();
        assert_eq!(trees(true), third);
        // No 768-triangle spheres in the scenery at all: round things use
        // the low sphere and the dome.
        let mats = resolve_materials(&LookData::default());
        let mut full = Statics::default();
        build_statics(&track, &mats, 7.0, u32::MAX, true, &mut full);
        assert!(full.instances.iter().all(|m| m.mesh != MeshId::SPHERE));
    }

    #[test]
    fn low_leaves_room_for_the_pack_under_its_triangle_limit() {
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let mats = resolve_materials(&LookData::default());
        let mut statics = Statics::default();
        // Low: 100k triangles, no shadow map (ground discs on).
        build_statics(&track, &mats, 7.0, 100_000, false, &mut statics);
        let fixed = statics.triangles();
        // Eight karts with drivers, a blast, item boxes, sparks and the
        // player's dust need roughly 20k on top.
        assert!(fixed <= 70_000, "Low statics are {fixed} triangles");
        // Medium and up: about 110k for the scenery (look bible).
        build_statics(&track, &mats, 7.0, u32::MAX, true, &mut statics);
        let full = statics.triangles();
        assert!(full <= 115_000, "full statics are {full} triangles");
    }

    /// The white "?" and rivets bloom at half the old ribbons' glow by
    /// default: what the bloom bright pass keeps of them (toon threshold 2.0,
    /// default knee) is half of what it kept of the ribbons.
    #[test]
    fn item_box_glyph_glow_defaults_to_half_the_old_ribbon_bloom() {
        let ribbon = luma(bloom_kept(RIBBON_RADIANCE, LOOK_BLOOM_THRESHOLD, BLOOM_KNEE));
        assert!((ribbon - RIBBON_BLOOM).abs() < 0.002, "ribbon bloom {ribbon}");
        let look = ItemBoxLook::default();
        assert_eq!(look.glyph_glow, 0.5);
        assert_eq!(look.glyph_color, [1.0; 3]);
        let glyph = look.glyph_radiance(LOOK_BLOOM_THRESHOLD);
        let kept = luma(bloom_kept(glyph, LOOK_BLOOM_THRESHOLD, BLOOM_KNEE));
        assert!((kept - 0.5 * ribbon).abs() < 0.002, "glyph bloom {kept} vs half the ribbon's {}", 0.5 * ribbon);
        // Neutral white: every channel equal, so it reads white, not cyan.
        assert!(glyph.iter().all(|&c| (c - glyph[0]).abs() < 1e-6) && glyph[0] > LOOK_BLOOM_THRESHOLD);
        // On the toon look's unlit `spark` material (base [1.2, 1.4, 1.8],
        // emissive [0.6, 0.8, 1.2]) the paint lands exactly on that colour.
        let (base, emissive) = ([1.2, 1.4, 1.8, 1.0], [0.6, 0.8, 1.2]);
        let mut lit = look;
        lit.paint_for(base, emissive, true, LOOK_BLOOM_THRESHOLD);
        for k in 0..3 {
            assert!((lit.paint[k] * base[k] + emissive[k] - glyph[k]).abs() < 1e-5);
        }
        // Glow 0: no bloom at all; glow 1: the ribbons' bloom again.
        let mut off = look;
        off.glyph_glow = 0.0;
        assert!(luma(bloom_kept(off.glyph_radiance(2.0), 2.0, 0.0)) < 1e-3);
        off.glyph_glow = 1.0;
        assert!((luma(bloom_kept(off.glyph_radiance(2.0), 2.0, BLOOM_KNEE)) - ribbon).abs() < 0.002);
    }

    /// Solid palette colours: a box shows one colour (or fades between two
    /// neighbours in the palette), and boxes side by side never match.
    #[test]
    fn item_boxes_show_solid_palette_colours_and_neighbours_differ() {
        let look = ItemBoxLook::default();
        for f in 0..2400 {
            let time = f as f32 * 0.05;
            for i in 0..11 {
                let (a, b) = (look.color(i, time), look.color(i + 1, time));
                let d: f32 = (0..3).map(|k| (a[k] - b[k]).abs()).sum();
                assert!(d > 0.15, "boxes {i} and {} match at {time} s: {a:?} {b:?}", i + 1);
            }
        }
        // Held (not fading) most of the time: exactly a palette colour.
        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4);
        let held = (0..100).filter(|&f| crate::ITEM_BOX_PALETTE.iter().any(|&p| near(p, look.color(0, f as f32 * 0.04)))).count();
        assert!(held >= 70, "{held}");
        // Cycle 0: every box keeps its colour.
        let mut still = look;
        still.color_cycle_s = 0.0;
        assert_eq!(still.color(3, 0.0), still.color(3, 77.0));
    }

    /// One body and one decor instance per box (the decor on an unlit
    /// material, untinted by the role); a plain bevelled cube before the
    /// meshes are registered.
    #[test]
    fn an_item_box_is_a_body_and_one_decor_instance() {
        let mats = resolve_materials(&LookData::default());
        let b = crate::items::ItemBox { pos: [0.0, 1.0, 0.0], hidden: 0.0 };
        let mut look = ItemBoxLook::default();
        let mut rw = RenderWorld::default();
        draw_item_box(&b, 0, &look, None, &mats, 0.0, true, &mut rw);
        assert_eq!(rw.meshes.len(), 1);
        assert_eq!(rw.meshes[0].mesh, MeshId::BEVEL_CUBE);
        look.meshes = Some((MeshId(40), MeshId(41)));
        let mut rw = RenderWorld::default();
        draw_item_box(&b, 0, &look, None, &mats, 0.0, true, &mut rw);
        assert_eq!(rw.meshes.iter().map(|m| m.mesh).collect::<Vec<_>>(), [MeshId(40), MeshId(41)]);
        assert_eq!(rw.meshes[1].material, mats[ITEM_BOX_DECOR_ROLE].index);
        assert_eq!(rw.meshes[1].color[..3], look.paint);
    }

    /// Low (100k triangles): the twelve boxes with their "?" and rivets still
    /// leave the pack its 20k on top of the statics.
    #[test]
    fn item_boxes_fit_low_triangle_limit() {
        let track = Track::from_json(include_str!("../data/track.json")).unwrap();
        let mats = resolve_materials(&LookData::default());
        let mut statics = Statics::default();
        build_statics(&track, &mats, 7.0, 100_000, false, &mut statics);
        let shape = ItemBoxLook::default().shape;
        let per_box = crate::itembox::body(&shape).triangles() + crate::itembox::decor(&shape).triangles();
        let boxes = track.item_rows.len() as u32 * track.item_boxes_per_row;
        let total = statics.triangles() + boxes * per_box + 20_000;
        assert!(total <= 100_000, "Low: {} statics + {boxes} boxes x {per_box} + 20k pack = {total}", statics.triangles());
    }
}
