//! The candy karts and their racers: every kart is a different sweet and
//! every driver an original character, all built in code from the engine's
//! built-in shapes and the kit's own candy meshes ([`crate::kartmesh`]).
//!
//! Drawing only. The karts keep the primitive kart's footprint, wheel
//! centres and sizes, seat and steering wheel, so the collision radius,
//! the chase camera's limits ([`crate::chasecam`]) and the boost flames
//! line up as before; nothing here is read by the race, and the same race
//! draws the same picture on every machine.
//!
//! | # | Kart (roster colour) | Wheels | Racer |
//! |---|---|---|---|
//! | 0 | Cupcake: pleated case, strawberry icing, a frosting swirl and a cherry | chocolate donuts with sprinkles | Pip: cotton-candy cloud hair, goggles up |
//! | 1 | Wrapped bonbon: striped shell, cellophane twists, two lollipops | lollipop swirls | Mo: backwards cap, big headphones |
//! | 2 | Waffle cone: chocolate rim, three melting scoops, a wafer roll | chocolate-chip cookies | Taffy: swirl bun, star clip |
//! | 3 | Mint chocolate bar: foil and paper wrapper, mint drizzle, mint stack | chocolate mints | Bramble: bear cub with a racing scarf |
//! | 4 | Gummy: jelly-bean nose, gummy-ring side pods, a sour worm round the back | sugared gummy rings | Zuzu: bunny with goggles up |
//! | 5 | Macaron: two shells and cream, macaron lights and tower | candy buttons | Nib: young pastry chef in a tall hat |
//! | 6 | Gumdrop: sugared dome, gumdrop pods, licorice-twist wing | licorice wheels | Clover: kitten with a headband and a tail |
//! | 7 | Peppermint hot-rod: stripes, spinning peppermints, candy canes, flames | peppermints | Rumble: flame mohawk, goggles |
//!
//! A kart takes its design from its roster index (`Kart::spec`), so the
//! eight roster karts are the eight designs and a longer roster repeats
//! them. Each keeps its roster colour as its main candy colour, so the
//! minimap dots and the player marker still match it.
//!
//! [`Paint::full`] draws everything; off (the small-budget tier's rivals
//! and far karts) the sprinkles, sugar, stripes, highlights and small face
//! details are left out and the wheels lose their patterns.

use evani_render::math::{quat_axis_angle, quat_mul};
use evani_render::mesh::Mesh;
use evani_render::{MeshId, RenderWorld};

use crate::draw::{KartParts, Mat, add, inst, kart_pose, quat_rotate, role, rot, yaw_q};
use crate::kartmesh as km;
use crate::track::hash01;

use core::f32::consts::{FRAC_PI_2, PI, TAU};

/// How many kart designs (and racers) there are.
pub const DESIGNS: usize = 8;

/// Design names, by roster index (kart, then racer).
pub const NAMES: [(&str, &str); DESIGNS] = [
    ("Cupcake", "Pip"),
    ("Bonbon", "Mo"),
    ("Waffle cone", "Taffy"),
    ("Mint chocolate", "Bramble"),
    ("Gummy", "Zuzu"),
    ("Macaron", "Nib"),
    ("Gumdrop", "Clover"),
    ("Peppermint", "Rumble"),
];

// Candy palette (linear RGB, from sRGB hex).
pub const CREAM: [f32; 3] = [1.000, 0.871, 0.644];
pub const VANILLA: [f32; 3] = [1.000, 0.791, 0.434];
pub const CHOC_DARK: [f32; 3] = [0.110, 0.030, 0.010];
pub const CHOC_MILK: [f32; 3] = [0.200, 0.065, 0.017];
pub const WAFER: [f32; 3] = [0.761, 0.392, 0.102];
pub const COOKIE: [f32; 3] = [0.584, 0.254, 0.070];
pub const STRAWBERRY: [f32; 3] = [1.000, 0.159, 0.376];
pub const PINK_ICING: [f32; 3] = [1.000, 0.434, 0.617];
pub const BUBBLEGUM: [f32; 3] = [1.000, 0.262, 0.565];
pub const MINT: [f32; 3] = [0.270, 0.863, 0.571];
pub const MINT_DEEP: [f32; 3] = [0.045, 0.597, 0.270];
pub const LEMON: [f32; 3] = [1.000, 0.753, 0.102];
pub const ORANGE: [f32; 3] = [1.000, 0.323, 0.045];
pub const CHERRY: [f32; 3] = [0.745, 0.008, 0.028];
pub const SKY: [f32; 3] = [0.216, 0.658, 1.000];
pub const GRAPE: [f32; 3] = [0.352, 0.133, 1.000];
pub const LILAC: [f32; 3] = [0.584, 0.386, 1.000];
pub const LIME: [f32; 3] = [0.381, 0.807, 0.042];
pub const TEAL_LIGHT: [f32; 3] = [0.275, 0.888, 0.839];
pub const LICORICE: [f32; 3] = [0.030, 0.024, 0.036];
pub const LICORICE_RED: [f32; 3] = [0.687, 0.013, 0.042];
pub const CELLO: [f32; 3] = [0.624, 0.863, 1.000];
pub const SKIN_FAIR: [f32; 3] = [1.000, 0.716, 0.539];
pub const SKIN_LIGHT: [f32; 3] = [0.888, 0.515, 0.301];
pub const SKIN_MED: [f32; 3] = [0.651, 0.296, 0.133];
pub const SKIN_TAN: [f32; 3] = [0.479, 0.178, 0.063];
pub const SKIN_DEEP: [f32; 3] = [0.195, 0.068, 0.023];
pub const BEAR: [f32; 3] = [0.323, 0.105, 0.027];
pub const BEAR_MUZZLE: [f32; 3] = [0.871, 0.584, 0.323];
pub const BUNNY: [f32; 3] = [0.905, 0.839, 1.000];
pub const BUNNY_EAR: [f32; 3] = [1.000, 0.451, 0.578];
pub const KITTEN: [f32; 3] = [0.323, 0.352, 0.423];
pub const KITTEN_DARK: [f32; 3] = [0.107, 0.120, 0.162];
pub const PIP_HAIR: [f32; 3] = [0.950, 0.200, 0.540];
pub const PIP_HAIR2: [f32; 3] = [1.000, 0.400, 0.700];
pub const TAFFY_HAIR: [f32; 3] = [0.102, 0.032, 0.014];
pub const NIB_HAIR: [f32; 3] = [0.745, 0.195, 0.042];
pub const FLAME_RED: [f32; 3] = [1.000, 0.044, 0.014];
pub const FLAME_ORANGE: [f32; 3] = [1.000, 0.254, 0.014];
pub const FLAME_YELLOW: [f32; 3] = [1.000, 0.644, 0.050];
pub const EYE: [f32; 3] = [0.018, 0.009, 0.019];
pub const BLUSH: [f32; 3] = [1.000, 0.275, 0.392];
pub const MOUTH: [f32; 3] = [0.332, 0.028, 0.060];
pub const GOLD: [f32; 3] = [1.000, 0.584, 0.045];
pub const GLASS: [f32; 3] = [0.347, 0.815, 1.000];
pub const AMBER: [f32; 3] = [1.000, 0.451, 0.063];
pub const BROWN_NOSE: [f32; 3] = [0.042, 0.018, 0.014];
/// Painted highlights on glossy candy: brighter than white in the lit band.
const SHINE: [f32; 3] = [1.35, 1.35, 1.35];
/// Sprinkle colours.
const SPRINKLES: [[f32; 3]; 6] = [STRAWBERRY, LEMON, SKY, MINT, CREAM, GRAPE];

/// The candy meshes' library ids. A mesh the library had no room for falls
/// back to a built-in of about its shape (a decoration is left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KartMeshes {
    ids: [Option<MeshId>; km::COUNT],
}

impl Default for KartMeshes {
    fn default() -> Self {
        Self::builtin()
    }
}

impl KartMeshes {
    /// First id of [`Self::placeholder`]: past every library slot, so a
    /// placeholder never names a real mesh.
    pub const PLACEHOLDER_BASE: u16 = 4096;

    pub fn new(ids: [Option<MeshId>; km::COUNT]) -> Self {
        Self { ids }
    }

    /// No candy meshes: every shape is its built-in stand-in.
    pub fn builtin() -> Self {
        Self { ids: [None; km::COUNT] }
    }

    /// Stand-in ids for measuring the drawn karts without a mesh library
    /// (the review dump): [`Self::placeholder_mesh`] turns them back into
    /// meshes.
    pub fn placeholder() -> Self {
        Self { ids: core::array::from_fn(|k| Some(MeshId(Self::PLACEHOLDER_BASE + k as u16))) }
    }

    /// The candy mesh a [`Self::placeholder`] id stands for.
    pub fn placeholder_mesh(id: MeshId) -> Option<Mesh> {
        let k = usize::from(id.0.checked_sub(Self::PLACEHOLDER_BASE)?);
        (k < km::COUNT).then(|| km::build().swap_remove(k))
    }

    /// The id to draw candy mesh `k` with.
    pub fn get(&self, k: usize) -> Option<MeshId> {
        self.ids.get(k).copied().flatten().or(km::SUBSTITUTE[k])
    }
}

/// A shape: a built-in mesh or one of the candy meshes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sh {
    B(MeshId),
    K(usize),
}

const CYL: Sh = Sh::B(MeshId::CYLINDER);
/// The built-in 24 x 16 sphere: the racers' heads, up close.
const HEADBALL: Sh = Sh::B(MeshId::SPHERE);
const BALL: Sh = Sh::K(km::BALL);
const LOWBALL: Sh = Sh::B(MeshId::LOW_SPHERE);
const BOX: Sh = Sh::B(MeshId::BEVEL_CUBE);
const CONE: Sh = Sh::B(MeshId::CONE);
const DOME: Sh = Sh::B(MeshId::DOME);
const ROUND: Sh = Sh::K(km::ROUND);
const DONUT: Sh = Sh::K(km::DONUT);
const RING: Sh = Sh::K(km::RING);
const SWIRL: Sh = Sh::K(km::SWIRL);
const PINWHEEL: Sh = Sh::K(km::PINWHEEL);
const SOFTSERVE: Sh = Sh::K(km::SOFTSERVE);
const ICING: Sh = Sh::K(km::ICING);
const PLEAT: Sh = Sh::K(km::PLEAT);
const FAN: Sh = Sh::K(km::FAN);
const STRIPE: Sh = Sh::K(km::STRIPE);
const STAR: Sh = Sh::K(km::STAR);
const HEART: Sh = Sh::K(km::HEART);
const DOT: Sh = Sh::K(km::SPRINKLE);
const QUILT: Sh = Sh::K(km::QUILT);

const NO_ROT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn qx(a: f32) -> [f32; 4] {
    quat_axis_angle([1.0, 0.0, 0.0], a)
}

fn qy(a: f32) -> [f32; 4] {
    quat_axis_angle([0.0, 1.0, 0.0], a)
}

fn qz(a: f32) -> [f32; 4] {
    quat_axis_angle([0.0, 0.0, 1.0], a)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(c: [f32; 3], s: f32) -> [f32; 3] {
    [c[0] * s, c[1] * s, c[2] * s]
}

/// The rotation that turns +Y to `d`.
fn q_from_y(d: [f32; 3]) -> [f32; 4] {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
    let c = (d[1] / l).clamp(-1.0, 1.0);
    if c > 0.99999 {
        return NO_ROT;
    }
    if c < -0.99999 {
        return qx(PI);
    }
    quat_axis_angle([d[2], 0.0, -d[0]], c.acos())
}

/// A frame for a model's parts: offsets (x left, y up, z forward) are
/// scaled per axis by `squash`, turned by `q` and added to `pos`; with
/// `squash` all 1 it is a plain placement, and with the kart's pose it
/// leans and squashes the parts like the kart body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Body {
    pub pos: [f32; 3],
    pub q: [f32; 4],
    pub squash: [f32; 3],
}

impl Body {
    fn at(&self, o: [f32; 3]) -> [f32; 3] {
        let s = self.squash;
        add(self.pos, quat_rotate(self.q, [o[0] * s[0], o[1] * s[1], o[2] * s[2]]))
    }
}

/// A sub-frame inside a [`Body`] (a head, a torso, a hook): offsets are
/// turned by `q` and added to `o`, both in the body's own terms.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fr {
    o: [f32; 3],
    q: [f32; 4],
}

impl Fr {
    fn at(&self, v: [f32; 3]) -> [f32; 3] {
        add(self.o, quat_rotate(self.q, v))
    }

    fn turn(&self, lq: [f32; 4]) -> [f32; 4] {
        quat_mul(self.q, lq)
    }
}

/// What a candy draw call needs: where to draw, the look's materials, the
/// candy meshes, the detail level and the render clock (for idle bobs and
/// flutter; it never touches the race).
pub(crate) struct Paint<'a> {
    pub rw: &'a mut RenderWorld,
    pub mats: &'a [Mat; 16],
    pub ids: &'a KartMeshes,
    pub full: bool,
    pub time: f32,
}

impl Paint<'_> {
    /// The mesh to draw shape `s` with. At low detail the soft shapes
    /// turn into their cheap built-in look-alikes (seen from far off they
    /// read the same) and the stripes are left out.
    fn mesh(&self, s: Sh) -> Option<MeshId> {
        let s = if self.full {
            s
        } else {
            match s {
                ROUND => BOX,
                BALL | HEADBALL => LOWBALL,
                SOFTSERVE | ICING => DOME,
                DONUT => CYL,
                STRIPE | SWIRL | PINWHEEL => return None,
                other => other,
            }
        };
        match s {
            Sh::B(id) => Some(id),
            Sh::K(k) => self.ids.get(k),
        }
    }

    /// Shape `s` at body offset `o`, turned by `lq`, of size `size` (all
    /// squashed with the body), in material `m` and colour `c`.
    #[allow(clippy::too_many_arguments)]
    fn put(&mut self, b: &Body, s: Sh, o: [f32; 3], lq: [f32; 4], size: [f32; 3], m: Mat, c: [f32; 3]) {
        let Some(mesh) = self.mesh(s) else { return };
        let q = &b.squash;
        self.rw.draw_mesh(inst(mesh, b.at(o), quat_mul(b.q, lq), [size[0] * q[0], size[1] * q[1], size[2] * q[2]], m, c));
    }

    /// [`Self::put`] with the size as given (not squashed): small pieces
    /// stuck on a squashed or scaled frame (sprinkles on a wheel).
    #[allow(clippy::too_many_arguments)]
    fn put_abs(&mut self, b: &Body, s: Sh, o: [f32; 3], lq: [f32; 4], size: [f32; 3], m: Mat, c: [f32; 3]) {
        let Some(mesh) = self.mesh(s) else { return };
        self.rw.draw_mesh(inst(mesh, b.at(o), quat_mul(b.q, lq), size, m, c));
    }

    /// A lit candy part (the look's near-white kart material, tinted).
    fn lit(&mut self, b: &Body, s: Sh, o: [f32; 3], lq: [f32; 4], size: [f32; 3], c: [f32; 3]) {
        let m = self.mats[role::KART_BODY];
        self.put(b, s, o, lq, size, m, c);
    }

    /// A white sugar part (the look's white), brightened by `c`.
    fn sugar(&mut self, b: &Body, s: Sh, o: [f32; 3], lq: [f32; 4], size: [f32; 3], c: [f32; 3]) {
        let m = self.mats[role::KERB_WHITE];
        self.put(b, s, o, lq, size, m, c);
    }

    /// A glowing candy light (unlit, bloom catches it); a small sprinkle
    /// shape at low detail.
    fn glow(&mut self, b: &Body, s: Sh, o: [f32; 3], lq: [f32; 4], size: [f32; 3], c: [f32; 3]) {
        let m = self.mats[role::SPARK];
        let s = if self.full { s } else { DOT };
        self.put(b, s, o, lq, size, m, c);
    }

    /// A stick of diameter `d` from `a` to `z`, with candy stripes in
    /// `stripe` (full detail only).
    fn stick(&mut self, b: &Body, a: [f32; 3], z: [f32; 3], d: f32, c: [f32; 3], stripe: Option<[f32; 3]>) {
        let v = sub(z, a);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let mid = [(a[0] + z[0]) * 0.5, (a[1] + z[1]) * 0.5, (a[2] + z[2]) * 0.5];
        let q = q_from_y(v);
        self.lit(b, CYL, mid, q, [d, len, d], c);
        if let Some(sc) = stripe
            && self.full
        {
            // One stripe turn every 3.5 diameters or so, whatever the length.
            let n = (len / (3.5 * d)).round().clamp(1.0, 8.0) as usize;
            let step = len / n as f32;
            let dir = [v[0] / len.max(1e-6), v[1] / len.max(1e-6), v[2] / len.max(1e-6)];
            for i in 0..n {
                let o = step * (i as f32 + 0.5);
                self.lit(b, STRIPE, add(a, scale(dir, o)), q, [d, step, d], sc);
            }
        }
    }

    /// A glossy highlight: a flat white streak on a candy surface.
    fn shine(&mut self, b: &Body, o: [f32; 3], lq: [f32; 4], size: [f32; 3]) {
        if self.full {
            self.sugar(b, LOWBALL, o, lq, size, SHINE);
        }
    }

    /// A sprinkle lying flat at `o`, turned `yaw` about the body's up.
    fn sprinkle(&mut self, b: &Body, o: [f32; 3], yaw: f32, c: [f32; 3]) {
        self.lit(b, DOT, o, quat_mul(qy(yaw), qx(FRAC_PI_2)), [0.05, 0.15, 0.05], c);
    }
}

/// Pseudo-random 0..1 for decoration layouts (fixed, so every frame and
/// every machine scatters them the same).
fn h(i: u32, salt: u32) -> f32 {
    hash01(i, salt)
}

/// The kart's wheels: centre (x, z), radius, width, front (steers).
pub const WHEELS: [(f32, f32, f32, f32, bool); 4] = [(0.8, 0.95, 0.33, 0.32, true), (-0.8, 0.95, 0.33, 0.32, true), (0.84, -0.86, 0.42, 0.44, false), (-0.84, -0.86, 0.42, 0.44, false)];

/// A rounded tread round a wheel ([`km::RING`] in the wheel's unit frame):
/// its outer edge is the wheel's own radius, so it meets the road where
/// the primitive tyre did, and it is as wide as the wheel.
const TREAD: [f32; 3] = [1.0, 0.5 / km::RING_TUBE, 1.0];

/// How a design's wheels look.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Wheel {
    /// A glazed donut with sprinkles, a candy button in the hole.
    Donut { glaze: [f32; 3], button: [f32; 3] },
    /// A disc with a lollipop swirl and a rounded tread.
    Swirl { disc: [f32; 3], swirl: [f32; 3], tread: [f32; 3] },
    /// A peppermint: white disc, curved wedges, rounded tread.
    Mint { disc: [f32; 3], wedge: [f32; 3], tread: [f32; 3] },
    /// A cookie with chocolate chips.
    Cookie { dough: [f32; 3], chip: [f32; 3] },
    /// A white disc dotted with candy buttons, rounded tread.
    Buttons { tread: [f32; 3] },
    /// A sugared gummy ring round a hub.
    Gummy { gum: [f32; 3], hub: [f32; 3] },
    /// A licorice wheel: a black spiral round a candy middle.
    Licorice { middle: [f32; 3] },
}

impl Wheel {
    /// The colour a plain (low detail) wheel takes.
    fn plain(&self) -> [f32; 3] {
        match *self {
            Wheel::Donut { glaze, .. } => glaze,
            Wheel::Swirl { disc, .. } => disc,
            Wheel::Mint { wedge, .. } => wedge,
            Wheel::Cookie { dough, .. } => dough,
            Wheel::Buttons { tread } => tread,
            Wheel::Gummy { gum, .. } => gum,
            Wheel::Licorice { .. } => LICORICE,
        }
    }
}

/// The wheels of design `d` with roster colour `col`.
fn wheel_of(d: usize, col: [f32; 3]) -> Wheel {
    match d % DESIGNS {
        0 => Wheel::Donut { glaze: CHOC_MILK, button: CREAM },
        1 => Wheel::Swirl { disc: CREAM, swirl: col, tread: scale(col, 0.55) },
        2 => Wheel::Cookie { dough: COOKIE, chip: CHOC_DARK },
        3 => Wheel::Swirl { disc: CHOC_DARK, swirl: MINT, tread: CHOC_MILK },
        4 => Wheel::Gummy { gum: LIME, hub: col },
        5 => Wheel::Buttons { tread: col },
        6 => Wheel::Licorice { middle: BUBBLEGUM },
        _ => Wheel::Mint { disc: [1.0, 0.97, 0.95], wedge: CHERRY, tread: CHERRY },
    }
}

/// One wheel: its frame is the unit cylinder's (axis along the axle, the
/// outer cap at local y `out` 0.5), rolled and steered, squashed to the
/// wheel's size, so a decoration at a unit offset rolls with it.
fn wheel(p: &mut Paint, wb: &Body, style: Wheel, out: f32, r: f32) {
    if !p.full {
        // The tyre and a light hub bar that shows the roll.
        p.lit(wb, CYL, [0.0; 3], NO_ROT, [1.0, 1.0, 1.0], style.plain());
        p.sugar(wb, BOX, [0.0; 3], NO_ROT, [0.7, 1.08, 0.16], [0.95; 3]);
        return;
    }
    // A piece lying on the outer cap at angle `a`, radius `rho` (unit).
    let face = |a: f32, rho: f32, lift: f32| [rho * a.cos(), out * (0.5 + lift), rho * a.sin()];
    let on_face = |a: f32| quat_mul(if out > 0.0 { NO_ROT } else { qx(PI) }, qy(-a));
    match style {
        Wheel::Donut { glaze, button } => {
            p.lit(wb, DONUT, [0.0; 3], NO_ROT, [1.0, 1.0 / (2.0 * km::DONUT_TUBE), 1.0], glaze);
            p.lit(wb, CYL, [0.0; 3], NO_ROT, [0.34, 0.9, 0.34], button);
            for i in 0..7u32 {
                let a = i as f32 / 7.0 * TAU + h(i, 11) * 0.5;
                let rho = 0.26 + 0.14 * h(i, 12);
                let pos = face(a, rho, 0.02);
                p.put_abs(wb, DOT, pos, quat_mul(on_face(a), qz(FRAC_PI_2)), [0.05, 0.14 * r / 0.42, 0.05], p.mats[role::KART_BODY], SPRINKLES[i as usize % SPRINKLES.len()]);
            }
        }
        Wheel::Swirl { disc, swirl, tread } => {
            p.lit(wb, CYL, [0.0; 3], NO_ROT, [0.96, 1.0, 0.96], disc);
            p.lit(wb, SWIRL, [0.0; 3], NO_ROT, [0.96, 1.0, 0.96], swirl);
            p.lit(wb, RING, [0.0; 3], NO_ROT, TREAD, tread);
        }
        Wheel::Mint { disc, wedge, tread } => {
            p.lit(wb, CYL, [0.0; 3], NO_ROT, [0.96, 1.0, 0.96], disc);
            p.lit(wb, PINWHEEL, [0.0; 3], NO_ROT, [0.96, 1.0, 0.96], wedge);
            p.lit(wb, RING, [0.0; 3], NO_ROT, TREAD, tread);
        }
        Wheel::Cookie { dough, chip } => {
            p.lit(wb, CYL, [0.0; 3], NO_ROT, [0.97, 1.0, 0.97], dough);
            p.lit(wb, RING, [0.0; 3], NO_ROT, TREAD, scale(dough, 0.8));
            for i in 0..7u32 {
                let a = i as f32 / 7.0 * TAU + h(i, 21);
                let rho = 0.1 + 0.3 * h(i, 22);
                let s = 0.07 + 0.04 * h(i, 23);
                p.put_abs(wb, DOT, face(a, rho, 0.0), on_face(a), [s, s * 0.5, s], p.mats[role::KART_BODY], chip);
            }
        }
        Wheel::Buttons { tread } => {
            p.sugar(wb, CYL, [0.0; 3], NO_ROT, [0.96, 1.0, 0.96], [1.0; 3]);
            p.lit(wb, RING, [0.0; 3], NO_ROT, TREAD, tread);
            let dots = [STRAWBERRY, LEMON, SKY, MINT, LILAC];
            for (i, c) in dots.iter().enumerate() {
                let a = i as f32 / 5.0 * TAU;
                p.put_abs(wb, DOT, face(a, 0.24, 0.0), on_face(a), [0.11, 0.06, 0.11], p.mats[role::KART_BODY], *c);
            }
            p.put_abs(wb, DOT, face(0.0, 0.0, 0.0), on_face(0.0), [0.1, 0.06, 0.1], p.mats[role::KART_BODY], ORANGE);
        }
        Wheel::Gummy { gum, hub } => {
            p.lit(wb, DONUT, [0.0; 3], NO_ROT, [1.0, 1.0 / (2.0 * km::DONUT_TUBE), 1.0], gum);
            p.lit(wb, CYL, [0.0; 3], NO_ROT, [0.42, 0.8, 0.42], hub);
            for i in 0..8u32 {
                let a = i as f32 / 8.0 * TAU + h(i, 31);
                let pos = face(a, 0.3 + 0.1 * h(i, 32), 0.02);
                p.put_abs(wb, DOT, pos, on_face(a), [0.035, 0.035, 0.035], p.mats[role::KERB_WHITE], SHINE);
            }
        }
        Wheel::Licorice { middle } => {
            p.lit(wb, CYL, [0.0; 3], NO_ROT, [1.0, 1.0, 1.0], LICORICE);
            p.lit(wb, SWIRL, [0.0; 3], NO_ROT, [1.0, 1.0, 1.0], [0.11, 0.1, 0.13]);
            p.put_abs(wb, DOME, face(0.0, 0.0, 0.0), on_face(0.0), [0.15, 0.07, 0.15], p.mats[role::KART_BODY], middle);
            for i in 0..5u32 {
                let a = i as f32 / 5.0 * TAU;
                p.put_abs(wb, DOT, face(a, 0.06, 0.06), on_face(a), [0.025; 3], p.mats[role::KERB_WHITE], [1.0; 3]);
            }
        }
    }
}

/// Draw the kart's four wheels: big rear, smaller steering fronts, each
/// rolling with the distance driven; wheels keep to the road (no body lean
/// or squash).
fn wheels(p: &mut Paint, k: &crate::kart::Kart, col: [f32; 3], pos: [f32; 3], vyaw: f32) {
    let kq = yaw_q(vyaw);
    let style = wheel_of(k.spec, col);
    for (x, z, r, w, front) in WHEELS {
        let steer = if front { yaw_q(-k.steer_vis * 0.5) } else { NO_ROT };
        let roll_q = qx(k.wheel_roll * crate::kart::WHEEL_RADIUS / r);
        let wq = quat_mul(kq, quat_mul(steer, roll_q));
        let tyre_q = quat_mul(wq, qz(FRAC_PI_2));
        let wb = Body { pos: add(pos, rot(vyaw, [x, r, z])), q: tyre_q, squash: [2.0 * r, w, 2.0 * r] };
        // The unit cylinder's +Y turns to the kart's -X: the outer cap of
        // a left wheel is local -Y.
        let out = if x > 0.0 { -1.0 } else { 1.0 };
        wheel(p, &wb, style, out, r);
    }
}

/// The axles, floor and seat every design shares.
fn base(p: &mut Paint, b: &Body, under: [f32; 3], seat: [f32; 3], axle: [f32; 3]) {
    p.lit(b, BOX, [0.0, 0.27, 0.05], NO_ROT, [1.1, 0.18, 2.5], under);
    if p.full {
        p.stick(b, [-0.66, 0.33, 0.95], [0.66, 0.33, 0.95], 0.09, axle, None);
        p.stick(b, [-0.66, 0.42, -0.86], [0.66, 0.42, -0.86], 0.11, axle, None);
    }
    p.lit(b, BOX, [0.0, 0.6, -0.3], NO_ROT, [0.72, 0.16, 0.6], seat);
    p.lit(b, BOX, [0.0, 0.92, -0.63], qx(-0.28), [0.74, 0.62, 0.16], seat);
}

/// Two glowing candy tail lights at `(±x, y, z)`.
fn tail_lights(p: &mut Paint, b: &Body, x: f32, y: f32, z: f32, c: [f32; 3]) {
    for s in [1.0f32, -1.0] {
        p.glow(b, LOWBALL, [s * x, y, z], NO_ROT, [0.15, 0.12, 0.08], c);
    }
}

/// Two exhaust sticks ending by the boost flames' spawn points (±0.26,
/// 0.78, -1.55 and back).
fn exhausts(p: &mut Paint, b: &Body, from: [f32; 3], d: f32, c: [f32; 3], stripe: Option<[f32; 3]>) {
    for s in [1.0f32, -1.0] {
        p.stick(b, [s * from[0], from[1], from[2]], [s * 0.27, 0.8, -1.56], d, c, stripe);
    }
}

/// Sprinkles scattered over an ellipse (centre `c`, half sizes `rx`, `rz`)
/// on a dome of height `top` over `c[1]`.
#[allow(clippy::too_many_arguments)]
fn sprinkle_dome(p: &mut Paint, b: &Body, c: [f32; 3], rx: f32, rz: f32, top: f32, n: u32, salt: u32) {
    if !p.full {
        return;
    }
    for i in 0..n {
        let a = h(i, salt) * TAU;
        let rho = h(i, salt + 1).sqrt() * 0.85;
        let (x, z) = (rho * a.cos(), rho * a.sin());
        let y = c[1] + top * (1.0 - rho * rho).max(0.0).sqrt() + 0.012;
        p.sprinkle(b, [c[0] + x * rx, y, c[2] + z * rz], h(i, salt + 2) * PI, SPRINKLES[i as usize % SPRINKLES.len()]);
    }
}

/// The body of design `d` (everything but the wheels and the driver).
/// `spin` turns the spinning parts (peppermints) with the wheels.
fn chassis(p: &mut Paint, b: &Body, d: usize, col: [f32; 3], spin: f32) {
    let light = mix(col, [1.0; 3], 0.45);
    match d % DESIGNS {
        0 => cupcake(p, b, col),
        1 => bonbon(p, b, col, light),
        2 => waffle(p, b),
        3 => mint_bar(p, b, col),
        4 => gummy(p, b, col, light),
        5 => macaron(p, b, col, light),
        6 => gumdrop(p, b, col),
        _ => peppermint(p, b, col, spin),
    }
}

/// Cupcake: a pleated case for a tub, strawberry icing dripping over the
/// hood and the back, a vanilla frosting swirl for an engine with a cherry
/// on top, sprinkles, a sugar heart on the nose, striped straws.
fn cupcake(p: &mut Paint, b: &Body, col: [f32; 3]) {
    let icing = [1.0, 0.3, 0.55];
    base(p, b, WAFER, VANILLA, CHOC_MILK);
    p.lit(b, PLEAT, [0.0, 0.47, 0.05], NO_ROT, [1.36, 0.5, 2.5], col);
    p.lit(b, ICING, [0.0, 0.7, 0.58], NO_ROT, [1.3, 0.52, 1.25], icing);
    p.lit(b, ICING, [0.0, 0.7, -0.72], NO_ROT, [1.3, 0.48, 1.12], icing);
    sprinkle_dome(p, b, [0.0, 0.7, 0.62], 0.55, 0.52, 0.15, 14, 100);
    let swirl = if p.full { SOFTSERVE } else { DOME };
    let sy = if p.full { 0.5 } else { 0.9 };
    p.lit(b, swirl, [0.0, 0.74, -0.98], qy(0.6), [0.9, sy, 0.8], VANILLA);
    p.lit(b, if p.full { BALL } else { LOWBALL }, [0.0, 1.3, -1.0], NO_ROT, [0.24, 0.22, 0.24], CHERRY);
    p.shine(b, [0.05, 1.37, -0.94], qx(0.4), [0.07, 0.04, 0.05]);
    p.stick(b, [0.0, 1.38, -1.0], [0.04, 1.52, -1.12], 0.025, MINT_DEEP, None);
    p.sugar(b, HEART, [0.0, 0.48, 1.13], qx(-1.1), [0.34, 0.06, 0.32], [1.0; 3]);
    exhausts(p, b, [0.28, 0.66, -1.2], 0.11, CREAM, Some(col));
    tail_lights(p, b, 0.42, 0.5, -1.2, [1.0, 0.3, 0.5]);
}

/// Bonbon: a glossy hard candy with two cream bands, crinkled cellophane
/// twists at the nose and the tail, and a lollipop on each rear corner.
fn bonbon(p: &mut Paint, b: &Body, col: [f32; 3], light: [f32; 3]) {
    base(p, b, scale(col, 0.5), CREAM, LICORICE);
    let (half, cz) = ([0.65f32, 0.31, 1.125], 0.05f32);
    p.lit(b, if p.full { BALL } else { LOWBALL }, [0.0, 0.5, cz], NO_ROT, [2.0 * half[0], 2.0 * half[1], 2.0 * half[2]], col);
    for z in [0.62f32, -0.5] {
        let k = (1.0 - ((z - cz) / half[2]).powi(2)).max(0.0).sqrt() * 1.04;
        p.lit(b, if p.full { BALL } else { LOWBALL }, [0.0, 0.5, z], NO_ROT, [2.0 * half[0] * k, 2.0 * half[1] * k, 0.16], CREAM);
    }
    p.lit(b, FAN, [0.0, 0.5, 1.33], qx(FRAC_PI_2), [0.62, 0.5, 0.5], CELLO);
    p.lit(b, FAN, [0.0, 0.55, -1.28], qx(-FRAC_PI_2), [0.8, 0.46, 0.66], CELLO);
    p.shine(b, [0.22, 0.775, 0.42], qy(0.15), [0.14, 0.035, 0.5]);
    p.shine(b, [0.34, 0.74, -0.05], qy(0.1), [0.08, 0.03, 0.18]);
    for (s, disc) in [(1.0f32, LEMON), (-1.0, STRAWBERRY)] {
        p.stick(b, [s * 0.5, 0.62, -0.9], [s * 0.55, 1.08, -1.12], 0.06, CREAM, None);
        let q = q_from_y([0.0, 0.25, -1.0]);
        p.lit(b, CYL, [s * 0.56, 1.2, -1.16], q, [0.44, 0.09, 0.44], disc);
        p.sugar(b, SWIRL, [s * 0.56, 1.2, -1.16], q, [0.44, 0.09, 0.44], [1.0; 3]);
    }
    exhausts(p, b, [0.24, 0.6, -1.15], 0.12, CREAM, Some(col));
    tail_lights(p, b, 0.42, 0.42, -1.1, light);
}

/// Waffle cone: the cone lies nose first, its mouth dipped in chocolate;
/// three melting scoops sit in it behind the driver, a wafer roll in the
/// top one, chocolate drizzled over the cone.
fn waffle(p: &mut Paint, b: &Body) {
    base(p, b, CHOC_MILK, CREAM, CHOC_DARK);
    let (cy, cz, len, h_, w) = (0.52f32, 0.42f32, 2.15f32, 0.8f32, 1.18f32);
    p.lit(b, QUILT, [0.0, cy, cz], qx(FRAC_PI_2), [w, len, h_], WAFER);
    let mouth = cz - len * 0.5;
    p.lit(b, RING, [0.0, cy, mouth], qx(FRAC_PI_2), [w + 0.06, 1.3, h_ + 0.06], CHOC_MILK);
    let scoops = [([-0.28f32, 0.82, -0.88], STRAWBERRY, 0.56), ([0.28, 0.82, -0.88], MINT, 0.56), ([0.0, 1.05, -1.0], VANILLA, 0.5)];
    for (c, flavour, s) in scoops {
        p.lit(b, if p.full { BALL } else { LOWBALL }, sub(c, [0.0, 0.03, 0.0]), NO_ROT, [s * 1.07, s * 0.93, s], flavour);
        if p.full {
            p.lit(b, ICING, c, NO_ROT, [s * 1.12, s * 1.55, s * 1.05], mix(flavour, [1.0; 3], 0.15));
        }
    }
    p.stick(b, [0.1, 1.15, -0.98], [0.25, 1.42, -1.25], 0.07, WAFER, Some(CHOC_MILK));
    exhausts(p, b, [0.26, 0.5, -1.0], 0.1, WAFER, Some(CHOC_MILK));
    tail_lights(p, b, 0.48, 0.36, -0.75, [1.0, 0.6, 0.2]);
}

/// Mint chocolate bar: a dark slab with pillowy squares, a green foil and
/// paper wrapper torn open over the back, mint drizzle, a stack of
/// chocolate mints for an engine.
fn mint_bar(p: &mut Paint, b: &Body, col: [f32; 3]) {
    base(p, b, CHOC_DARK, MINT, CHOC_DARK);
    p.lit(b, ROUND, [0.0, 0.4, 0.08], NO_ROT, [1.24, 0.28, 2.62], CHOC_DARK);
    for (x, z) in [(0.29f32, 1.02f32), (-0.29, 1.02), (0.29, 0.45), (-0.29, 0.45)] {
        p.lit(b, ROUND, [x, 0.58, z], NO_ROT, [0.52, 0.14, 0.5], CHOC_MILK);
    }
    let foil = mix(col, [1.0; 3], 0.1);
    p.lit(b, ROUND, [0.0, 0.46, -0.72], NO_ROT, [1.32, 0.42, 1.18], foil);
    p.sugar(b, ROUND, [0.0, 0.46, -0.16], NO_ROT, [1.34, 0.44, 0.16], [1.0; 3]);
    p.shine(b, [0.38, 0.675, -0.72], qy(0.05), [0.1, 0.03, 0.55]);
    if p.full {
        for i in 0..5 {
            let x = -0.52 + i as f32 * 0.26;
            p.lit(b, BOX, [x, 0.66, -0.06], quat_mul(qy(0.3 * (i as f32 - 2.0)), qx(-0.6 - 0.15 * (i % 2) as f32)), [0.24, 0.035, 0.16], mix(foil, [1.0; 3], 0.25));
        }
        let pts = [[-0.56f32, 0.67, 1.26], [0.56, 0.67, 1.06], [-0.56, 0.67, 0.84], [0.56, 0.67, 0.62], [-0.56, 0.67, 0.4]];
        for w in pts.windows(2) {
            p.stick(b, w[0], w[1], 0.045, MINT, None);
        }
    }
    for i in 0..3 {
        let y = 0.72 + i as f32 * 0.14;
        p.lit(b, CYL, [0.0, y, -0.95], NO_ROT, [0.78, 0.09, 0.66], CHOC_DARK);
        if i < 2 {
            p.lit(b, CYL, [0.0, y + 0.07, -0.95], NO_ROT, [0.74, 0.05, 0.62], MINT);
        }
    }
    p.lit(b, STAR, [0.0, 0.42, 1.38], qx(FRAC_PI_2), [0.3, 0.05, 0.3], MINT);
    exhausts(p, b, [0.3, 0.55, -1.2], 0.11, CHOC_MILK, Some(MINT));
    tail_lights(p, b, 0.45, 0.5, -1.32, [0.3, 1.0, 0.6]);
}

/// Gummy: a soft jelly tub with a jelly-bean nose, gummy rings for side
/// pods and a two-tone sour worm curled round the back.
fn gummy(p: &mut Paint, b: &Body, col: [f32; 3], light: [f32; 3]) {
    base(p, b, scale(col, 0.55), LIME, scale(col, 0.4));
    p.lit(b, ROUND, [0.0, 0.48, 0.05], NO_ROT, [1.22, 0.46, 2.4], col);
    p.lit(b, if p.full { BALL } else { LOWBALL }, [0.0, 0.45, 1.22], NO_ROT, [1.0, 0.36, 0.42], LIME);
    p.shine(b, [0.3, 0.715, 0.55], qy(0.1), [0.12, 0.03, 0.55]);
    p.shine(b, [0.2, 0.6, 1.3], qy(0.0), [0.18, 0.03, 0.07]);
    for (s, c) in [(1.0f32, ORANGE), (-1.0, LEMON)] {
        p.lit(b, DONUT, [s * 0.8, 0.5, 0.05], qz(FRAC_PI_2), [0.62, 0.8, 0.62], c);
    }
    // The worm: a horseshoe of beads round the rear deck, lime into grape.
    let n = if p.full { 13 } else { 5 };
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let a = PI * t;
        let pos = [0.58 * a.cos(), 0.78 + 0.05 * (3.0 * a).sin(), -0.62 - 0.55 * a.sin()];
        let c = if t < 0.5 { LIME } else { light };
        let s = 0.27 * (1.0 - 0.25 * (2.0 * t - 1.0).abs().powi(4));
        p.lit(b, LOWBALL, pos, NO_ROT, [s, s, s], c);
        if p.full && i % 2 == 0 {
            p.sugar(b, DOT, add(pos, [0.04, s * 0.48, 0.02]), NO_ROT, [0.04; 3], SHINE);
        }
    }
    exhausts(p, b, [0.26, 0.6, -1.2], 0.13, ORANGE, None);
    tail_lights(p, b, 0.45, 0.5, -1.22, light);
}

/// Macaron: two shells with cream between and a ruffled foot round the
/// bottom shell, mini macarons for headlights and a macaron tower behind.
fn macaron(p: &mut Paint, b: &Body, col: [f32; 3], light: [f32; 3]) {
    base(p, b, scale(col, 0.6), LILAC, CREAM);
    p.lit(b, ROUND, [0.0, 0.34, 0.05], NO_ROT, [1.3, 0.22, 2.5], col);
    p.lit(b, ROUND, [0.0, 0.49, 0.05], NO_ROT, [1.2, 0.1, 2.38], CREAM);
    p.lit(b, ROUND, [0.0, 0.61, 0.05], NO_ROT, [1.32, 0.18, 2.52], light);
    p.lit(b, DOME, [0.0, 0.68, 0.45], NO_ROT, [1.1, 0.24, 1.3], light);
    if p.full {
        for i in 0..18u32 {
            let a = i as f32 / 18.0 * TAU;
            p.lit(b, DOT, [0.63 * a.cos(), 0.455, 0.05 + 1.22 * a.sin()], NO_ROT, [0.13, 0.06, 0.13], mix(col, [1.0; 3], 0.2));
        }
    }
    for (s, c) in [(1.0f32, STRAWBERRY), (-1.0, LEMON)] {
        let q = qx(FRAC_PI_2);
        if p.full {
            p.lit(b, CYL, [s * 0.38, 0.5, 1.29], q, [0.28, 0.07, 0.26], c);
            p.lit(b, CYL, [s * 0.38, 0.5, 1.35], q, [0.24, 0.04, 0.22], CREAM);
            p.lit(b, CYL, [s * 0.38, 0.5, 1.4], q, [0.28, 0.07, 0.26], c);
        } else {
            p.lit(b, CYL, [s * 0.38, 0.5, 1.35], q, [0.28, 0.18, 0.26], c);
        }
    }
    for (i, c) in [STRAWBERRY, LEMON, LILAC].iter().enumerate() {
        let y = 0.78 + i as f32 * 0.15;
        if p.full {
            p.lit(b, DOME, [0.0, y + 0.03, -1.0], qx(PI), [0.5, 0.12, 0.5], *c);
            p.lit(b, CYL, [0.0, y + 0.05, -1.0], NO_ROT, [0.45, 0.04, 0.45], CREAM);
            p.lit(b, DOME, [0.0, y + 0.07, -1.0], NO_ROT, [0.5, 0.14, 0.5], *c);
        } else {
            p.lit(b, CYL, [0.0, y + 0.05, -1.0], NO_ROT, [0.5, 0.13, 0.5], *c);
        }
    }
    exhausts(p, b, [0.27, 0.56, -1.2], 0.1, CREAM, Some(col));
    tail_lights(p, b, 0.45, 0.38, -1.22, light);
}

/// Gumdrop: a sugared gumdrop dome over a licorice deck, gumdrop side pods
/// and a strawberry licorice twist for a rear wing.
fn gumdrop(p: &mut Paint, b: &Body, col: [f32; 3]) {
    base(p, b, LICORICE, BUBBLEGUM, LICORICE);
    p.lit(b, ROUND, [0.0, 0.36, 0.0], NO_ROT, [1.2, 0.26, 2.5], LICORICE);
    p.lit(b, DOME, [0.0, 0.45, 0.42], NO_ROT, [1.28, 0.88, 1.72], col);
    p.shine(b, [0.3, 0.83, 0.62], qy(0.2), [0.15, 0.04, 0.36]);
    if p.full {
        for i in 0..20u32 {
            let a = h(i, 300) * TAU;
            let el = 0.2 + 1.2 * h(i, 301);
            let (sx, sz) = (a.cos() * el.cos(), a.sin() * el.cos());
            let o = [0.0 + sx * 0.64, 0.45 + el.sin() * 0.44, 0.42 + sz * 0.86];
            p.sugar(b, DOT, o, NO_ROT, [0.035; 3], SHINE);
        }
    }
    let pods = [(0.68f32, 0.12f32, CHERRY, 0.42f32), (-0.68, 0.12, ORANGE, 0.42), (0.66, -0.3, LEMON, 0.32), (-0.66, -0.3, GRAPE, 0.32)];
    for (x, z, c, s) in pods.into_iter().take(if p.full { 4 } else { 2 }) {
        p.lit(b, DOME, [x, 0.36, z], NO_ROT, [s, s * 1.4, s], c);
    }
    for s in [1.0f32, -1.0] {
        p.lit(b, DOME, [s * 0.5, 0.5, -1.15], NO_ROT, [0.34, 0.5, 0.34], CHERRY);
        p.stick(b, [s * 0.5, 0.6, -1.15], [s * 0.5, 1.0, -1.18], 0.07, LICORICE_RED, None);
    }
    p.stick(b, [-0.74, 1.0, -1.18], [0.74, 1.0, -1.18], 0.15, LICORICE_RED, Some([1.0, 0.16, 0.22]));
    exhausts(p, b, [0.26, 0.52, -1.15], 0.12, LICORICE, None);
    tail_lights(p, b, 0.42, 0.42, -1.28, [1.0, 0.25, 0.4]);
}

/// Peppermint hot-rod: a red hard-candy body with cream racing stripes and
/// flames, a spinning peppermint grille and turbine, candy-cane hoops and
/// candy-cane exhausts.
fn peppermint(p: &mut Paint, b: &Body, col: [f32; 3], spin: f32) {
    base(p, b, CREAM, CREAM, CHERRY);
    p.lit(b, ROUND, [0.0, 0.46, 0.15], NO_ROT, [1.18, 0.36, 2.6], col);
    for x in [0.17f32, -0.17] {
        p.sugar(b, ROUND, [x, 0.643, 0.6], NO_ROT, [0.13, 0.03, 1.5], [1.0; 3]);
    }
    p.shine(b, [0.42, 0.645, 0.85], qy(0.0), [0.09, 0.02, 0.45]);
    // Grille and turbine: peppermints that spin as the kart rolls.
    let grille = quat_mul(qx(FRAC_PI_2), qy(spin));
    p.sugar(b, CYL, [0.0, 0.44, 1.47], grille, [0.56, 0.1, 0.56], [1.0; 3]);
    p.lit(b, PINWHEEL, [0.0, 0.44, 1.47], grille, [0.56, 0.1, 0.56], CHERRY);
    p.sugar(b, ROUND, [0.0, 0.72, -0.98], NO_ROT, [0.8, 0.36, 0.56], [1.0; 3]);
    let turbine = quat_mul(qx(-FRAC_PI_2), qy(-spin * 0.5));
    p.sugar(b, CYL, [0.0, 0.98, -1.18], turbine, [0.56, 0.12, 0.56], [1.0; 3]);
    p.lit(b, PINWHEEL, [0.0, 0.98, -1.18], turbine, [0.56, 0.12, 0.56], CHERRY);
    // Candy-cane hoops behind the seat, hooking inwards.
    for s in [1.0f32, -1.0] {
        p.stick(b, [s * 0.48, 0.62, -0.74], [s * 0.48, 1.12, -0.74], 0.1, CREAM, Some(CHERRY));
        let (cx, cy, r) = (s * 0.31, 1.12f32, 0.17f32);
        let n = if p.full { 7 } else { 3 };
        for i in 0..n {
            let a0 = PI * i as f32 / n as f32;
            let a1 = PI * (i + 1) as f32 / n as f32;
            let at = |a: f32| [cx + s * r * a.cos(), cy + r * a.sin(), -0.74];
            let c = if i % 2 == 0 { CHERRY } else { CREAM };
            p.stick(b, at(a0), at(a1), 0.1, c, None);
        }
    }
    // Flames along each side.
    for s in [1.0f32, -1.0] {
        for (i, c) in [FLAME_YELLOW, FLAME_ORANGE, FLAME_RED].iter().enumerate() {
            let z = 0.35 - i as f32 * 0.3;
            p.lit(b, CONE, [s * 0.6, 0.47, z], q_from_y([0.0, 0.12, -1.0]), [0.1, 0.42 - 0.06 * i as f32, 0.2], *c);
        }
    }
    exhausts(p, b, [0.26, 0.6, -1.15], 0.14, CREAM, Some(CHERRY));
    tail_lights(p, b, 0.45, 0.48, -1.16, [1.0, 0.2, 0.15]);
}

/// The racer of design `r`: skin or fur, hair colour.
fn skin_of(r: usize) -> [f32; 3] {
    [SKIN_FAIR, SKIN_DEEP, SKIN_MED, BEAR, BUNNY, SKIN_LIGHT, KITTEN, SKIN_TAN][r % DESIGNS]
}

/// Size of a racer's head (the old helmet's footprint).
const HEAD: [f32; 3] = [0.84, 0.8, 0.82];

/// A racer's head in frame `f` (origin the head's middle, +Z the face),
/// on body `b`. `speed` (0..1) blows ears and hair back.
fn head(p: &mut Paint, b: &Body, f: &Fr, r: usize, speed: f32) {
    let r = r % DESIGNS;
    let skin = skin_of(r);
    let full = p.full;
    let part = |p: &mut Paint, s: Sh, o: [f32; 3], lq: [f32; 4], size: [f32; 3], c: [f32; 3]| p.lit(b, s, f.at(o), f.turn(lq), size, c);
    part(p, HEADBALL, [0.0; 3], NO_ROT, HEAD, skin);
    let critter = matches!(r, 3 | 4 | 6);
    // Eyes (bigger and higher on the critters), glints, cheeks, mouth.
    let ey = if critter { 0.07 } else { 0.03 };
    for s in [1.0f32, -1.0] {
        if !full {
            break;
        }
        part(p, LOWBALL, [s * 0.15, ey, 0.355], qy(s * 0.35), [0.13, 0.19, 0.08], EYE);
        {
            let m = p.mats[role::KERB_WHITE];
            p.put(b, DOT, f.at([s * 0.13 + 0.02, ey + 0.05, 0.395]), f.turn(NO_ROT), [0.05, 0.06, 0.03], m, SHINE);
            part(p, LOWBALL, [s * 0.25, -0.1, 0.3], qy(s * 0.6), [0.12, 0.07, 0.04], BLUSH);
        }
    }
    if full {
        for (x, y, a) in [(-0.06f32, -0.145f32, -0.5f32), (0.0, -0.165, 0.0), (0.06, -0.145, 0.5)] {
            part(p, BOX, [x, y, 0.372], qz(a), [0.07, 0.028, 0.03], MOUTH);
        }
    }
    let back = |speed: f32| 0.15 + 0.55 * speed;
    match r {
        // Pip: a cloud of cotton-candy hair, white goggles pushed up.
        0 => {
            let puffs: [([f32; 3], f32); 9] = [
                ([0.0, 0.3, -0.04], 0.56),
                ([0.22, 0.22, -0.12], 0.46),
                ([-0.22, 0.22, -0.12], 0.46),
                ([0.0, 0.16, -0.3], 0.52),
                ([0.27, 0.0, -0.2], 0.38),
                ([-0.27, 0.0, -0.2], 0.38),
                ([0.0, -0.08, -0.3], 0.42),
                ([0.13, 0.3, 0.2], 0.28),
                ([-0.13, 0.3, 0.2], 0.28),
            ];
            let n = if full { puffs.len() } else { 4 };
            for (i, (o, d)) in puffs.iter().take(n).enumerate() {
                part(p, LOWBALL, *o, qy(i as f32), [*d, *d * 0.9, *d], if i % 2 == 0 { PIP_HAIR } else { PIP_HAIR2 });
            }
            if full {
                for s in [1.0f32, -1.0] {
                    let q = qx(FRAC_PI_2 - 0.7);
                    part(p, RING, [s * 0.14, 0.3, 0.29], q, [0.2, 0.32, 0.2], [1.0; 3]);
                    part(p, CYL, [s * 0.14, 0.3, 0.29], q, [0.15, 0.03, 0.15], GLASS);
                }
            }
        }
        // Mo: a backwards cap in his suit colour, big headphones.
        1 => {
            part(p, DOME, [0.0, -0.02, -0.04], qx(-0.95), [0.88, 0.86, 0.88], [0.02, 0.014, 0.012]);
            part(p, DOME, [0.0, 0.08, -0.02], qx(-0.15), [0.9, 0.64, 0.88], [1.0, 0.66, 0.0]);
            part(p, ROUND, [0.0, 0.1, -0.52], qx(-0.3), [0.5, 0.06, 0.36], [0.023, 0.198, 1.0]);
            if full {
                part(p, DOT, [0.0, 0.4, -0.07], NO_ROT, [0.08, 0.06, 0.08], [0.023, 0.198, 1.0]);
                part(p, RING, [0.0, 0.0, -0.02], qx(FRAC_PI_2), [0.98, 0.35, 0.98], [0.05, 0.05, 0.07]);
            }
            for s in [1.0f32, -1.0] {
                part(p, CYL, [s * 0.43, 0.0, -0.02], qz(FRAC_PI_2), [0.28, 0.1, 0.28], [0.06, 0.08, 0.2]);
                part(p, CYL, [s * 0.48, 0.0, -0.02], qz(FRAC_PI_2), [0.2, 0.04, 0.2], SKY);
            }
        }
        // Taffy: dark hair with a swirled bun and a lemon star clip.
        2 => {
            part(p, DOME, [0.0, 0.0, -0.03], qx(-0.55), [0.9, 0.88, 0.9], TAFFY_HAIR);
            for s in [1.0f32, -1.0] {
                part(p, ROUND, [s * 0.37, -0.06, -0.06], NO_ROT, [0.16, 0.42, 0.42], TAFFY_HAIR);
            }
            let bun = if full { SOFTSERVE } else { LOWBALL };
            let bs = if full { [0.44, 0.38, 0.44] } else { [0.36, 0.3, 0.36] };
            part(p, bun, [0.0, 0.3, -0.24], qx(-0.7), bs, mix(TAFFY_HAIR, [0.3, 0.12, 0.05], 0.4));
            if full {
                let dir = [0.3f32, 0.26, 0.18];
                part(p, STAR, dir, q_from_y(dir), [0.17, 0.05, 0.17], LEMON);
            }
        }
        // Bramble: a bear cub, round ears and a soft muzzle.
        3 => {
            for s in [1.0f32, -1.0] {
                part(p, LOWBALL, [s * 0.27, 0.31, -0.04], NO_ROT, [0.24, 0.24, 0.14], BEAR);
                if full {
                    part(p, LOWBALL, [s * 0.27, 0.31, 0.0], NO_ROT, [0.13, 0.13, 0.08], BEAR_MUZZLE);
                }
            }
            part(p, LOWBALL, [0.0, -0.11, 0.31], NO_ROT, [0.34, 0.24, 0.2], BEAR_MUZZLE);
            part(p, DOT, [0.0, -0.05, 0.42], NO_ROT, [0.13, 0.08, 0.08], BROWN_NOSE);
        }
        // Zuzu: a bunny, long ears swept back with speed (one flops out),
        // goggles up on the forehead and two front teeth.
        4 => {
            for s in [1.0f32, -1.0] {
                let tilt = 0.55 + 0.5 * speed + 0.05 * (p.time * 9.0 + s).sin();
                let q = quat_mul(qz(-s * if s > 0.0 { 0.45 } else { 0.18 }), qx(-tilt));
                let base_ = [s * 0.13, 0.26, -0.06];
                part(p, ROUND, add(base_, quat_rotate(q, [0.0, 0.19, 0.0])), q, [0.17, 0.38, 0.09], BUNNY);
                if full {
                    part(p, ROUND, add(base_, quat_rotate(q, [0.0, 0.19, 0.035])), q, [0.09, 0.28, 0.04], BUNNY_EAR);
                }
            }
            // Goggle strap of red liquorice: a black one read as a black
            // line across her white head (owner, 2026-10-29).
            part(p, RING, [0.0, 0.2, 0.0], qx(0.25), [0.9, 0.32, 0.88], LICORICE_RED);
            for s in [1.0f32, -1.0] {
                if !full {
                    break;
                }
                let q = qx(FRAC_PI_2 - 0.55);
                part(p, CYL, [s * 0.15, 0.27, 0.31], q, [0.17, 0.05, 0.17], LIME);
            }
            if full {
                part(p, DOT, [0.0, -0.05, 0.4], NO_ROT, [0.08, 0.06, 0.05], BUNNY_EAR);
                for s in [1.0f32, -1.0] {
                    p.sugar(b, BOX, f.at([s * 0.026, -0.2, 0.37]), f.turn(NO_ROT), [0.045, 0.06, 0.02], [1.0; 3]);
                }
            }
        }
        // Nib: a young pastry chef, ginger tufts under a tall hat.
        5 => {
            part(p, DOME, [0.0, -0.02, -0.03], qx(-0.95), [0.87, 0.86, 0.87], NIB_HAIR);
            for s in [1.0f32, -1.0] {
                part(p, LOWBALL, [s * 0.33, 0.04, -0.12], NO_ROT, [0.2, 0.26, 0.26], NIB_HAIR);
            }
            let white = p.mats[role::KERB_WHITE];
            p.put(b, CYL, f.at([0.0, 0.3, -0.04]), f.turn(qx(-0.12)), [0.74, 0.2, 0.74], white, [1.0; 3]);
            p.put(b, if full { BALL } else { LOWBALL }, f.at([0.0, 0.47, -0.08]), f.turn(NO_ROT), [0.82, 0.34, 0.8], white, [1.0; 3]);
            if full {
                for o in [[0.24f32, 0.48, -0.08], [-0.24, 0.48, -0.08], [0.0, 0.48, 0.16], [0.0, 0.48, -0.32]] {
                    p.put(b, LOWBALL, f.at(o), f.turn(NO_ROT), [0.36, 0.3, 0.36], white, [1.0; 3]);
                }
                for s in [1.0f32, -1.0] {
                    for (i, (x, y)) in [(0.22f32, -0.04f32), (0.27, -0.07), (0.21, -0.1)].iter().enumerate() {
                        let _ = i;
                        part(p, DOT, [s * x, *y, 0.34], NO_ROT, [0.025; 3], NIB_HAIR);
                    }
                }
            }
        }
        // Clover: a kitten, pointed ears, tabby stripes and a headband.
        6 => {
            for s in [1.0f32, -1.0] {
                part(p, CONE, [s * 0.22, 0.38, -0.04], qz(-s * 0.35), [0.26, 0.3, 0.14], KITTEN);
                if full {
                    part(p, CONE, [s * 0.21, 0.37, 0.0], qz(-s * 0.35), [0.15, 0.2, 0.06], BUNNY_EAR);
                }
                if full {
                    part(p, LOWBALL, [s * 0.07, -0.11, 0.33], NO_ROT, [0.18, 0.13, 0.12], [0.95, 0.95, 0.97]);
                }
            }
            part(p, DOME, [0.0, 0.0, -0.03], qx(-1.0), [0.86, 0.85, 0.86], KITTEN_DARK);
            if full {
                part(p, RING, [0.0, 0.17, 0.0], NO_ROT, [0.88, 0.3, 0.86], [0.02, 0.2, 1.0]);
            }
            if full {
                part(p, DOT, [0.0, -0.05, 0.41], NO_ROT, [0.08, 0.06, 0.05], BUNNY_EAR);
                for s in [1.0f32, -1.0] {
                    for (y, a) in [(-0.08f32, 0.12f32), (-0.13, -0.08)] {
                        p.sugar(b, BOX, f.at([s * 0.33, y, 0.28]), f.turn(qz(s * a)), [0.22, 0.012, 0.012], [1.0; 3]);
                    }
                }
            }
        }
        // Rumble: a flame mohawk, gold goggles up, determined brows.
        _ => {
            part(p, DOME, [0.0, 0.0, -0.03], qx(-0.7), [0.87, 0.86, 0.87], [0.05, 0.02, 0.01]);
            let flames = [FLAME_YELLOW, FLAME_ORANGE, FLAME_RED, FLAME_ORANGE, FLAME_RED, FLAME_ORANGE];
            for (i, c) in flames.iter().enumerate().filter(|(i, _)| full || i % 2 == 0) {
                let phi = 0.6 - i as f32 * 0.42;
                let d = [0.0, phi.cos(), phi.sin()];
                let axis = [0.0, d[1] - 0.05, d[2] - back(speed) * 0.6];
                let len = 0.44 - 0.05 * (i as f32 - 1.5).abs();
                part(p, CONE, add(scale(d, 0.33), scale(axis, len * 0.3)), q_from_y(axis), [0.18, len, 0.32], *c);
            }
            // Goggle strap in the hot-rod's cherry red: a near-black strap
            // read as a black line across his head (owner, 2026-10-29).
            part(p, RING, [0.0, 0.2, 0.0], qx(0.2), [0.88, 0.3, 0.86], CHERRY);
            for s in [1.0f32, -1.0] {
                if !full {
                    break;
                }
                let q = qx(FRAC_PI_2 - 0.6);
                part(p, RING, [s * 0.15, 0.27, 0.3], q, [0.22, 0.32, 0.22], GOLD);
                part(p, CYL, [s * 0.15, 0.27, 0.3], q, [0.17, 0.03, 0.17], AMBER);
                part(p, BOX, [s * 0.15, 0.135, 0.37], qz(-s * 0.25), [0.15, 0.035, 0.03], [0.04, 0.02, 0.015]);
            }
        }
    }
}

/// The racer's back badge emblem, centred on a white disc on the suit.
fn emblem(p: &mut Paint, b: &Body, f: &Fr, r: usize, col: [f32; 3]) {
    let q = f.turn(qx(-FRAC_PI_2));
    let at = f.at([0.0, 0.0, -0.025]);
    match r % DESIGNS {
        0 => p.lit(b, HEART, at, q, [0.2, 0.04, 0.19], col),
        2 => {
            p.lit(b, CYL, at, q, [0.2, 0.03, 0.2], LEMON);
            p.sugar(b, SWIRL, at, q, [0.2, 0.03, 0.2], [1.0; 3]);
        }
        3 => {
            p.lit(b, LOWBALL, f.at([0.0, -0.03, -0.03]), q, [0.12, 0.1, 0.04], BEAR);
            for (x, y) in [(-0.07f32, 0.05f32), (0.0, 0.08), (0.07, 0.05)] {
                p.lit(b, LOWBALL, f.at([x, y, -0.03]), q, [0.05, 0.05, 0.03], BEAR);
            }
        }
        5 => {
            p.sugar(b, CYL, at, q, [0.2, 0.03, 0.2], [1.0; 3]);
            p.lit(b, PINWHEEL, at, q, [0.2, 0.03, 0.2], CHERRY);
        }
        6 => {
            for a in [0.0f32, 2.1, 4.2] {
                let o = [0.05 * (a + FRAC_PI_2).cos(), 0.02 + 0.05 * (a + FRAC_PI_2).sin(), -0.03];
                p.lit(b, LOWBALL, f.at(o), q, [0.09, 0.09, 0.03], LIME);
            }
        }
        _ => p.lit(b, STAR, at, q, [0.2, 0.04, 0.2], col),
    }
}

/// The seated racer, the steering column and the steering wheel: the
/// suit leans into the turn, hands on the wheel as it turns, the head
/// looks into the turn and bobs gently at idle.
#[allow(clippy::too_many_arguments)]
fn driver(p: &mut Paint, b: &Body, k: &crate::kart::Kart, col: [f32; 3], suit: [f32; 3], speed: f32) {
    let r = k.spec % DESIGNS;
    let skin = skin_of(r);
    // Steering column (a candy stick) and a gummy-ring steering wheel.
    p.stick(b, [0.0, 0.6, 0.78], [0.0, 0.95, 0.38], 0.08, CREAM, Some(col));
    let wheel_q = quat_mul(qx(-1.0), qy(k.steer_vis * 0.9));
    let wc = [0.0, 0.98, 0.34];
    if p.full {
        p.lit(b, RING, wc, wheel_q, [0.5, 0.42, 0.5], mix(col, [1.0; 3], 0.3));
        p.lit(b, DOT, wc, wheel_q, [0.13, 0.08, 0.13], CREAM);
    }
    let lean = if k.drift != 0.0 { k.drift * 0.12 } else { -k.steer_vis * 0.09 };
    let lean_q = qz(-lean * 1.6);
    let wide = if r == 7 { 1.14 } else { 1.0 };
    let torso = Fr { o: [lean * 0.5, 1.0, -0.32], q: lean_q };
    p.lit(b, ROUND, torso.o, lean_q, [0.66 * wide, 0.62, 0.48], suit);
    if p.full {
        p.sugar(b, ROUND, torso.at([0.0, -0.22, 0.0]), lean_q, [0.7 * wide, 0.1, 0.52], [1.0; 3]);
        // The back badge: a white disc with the racer's emblem.
        let badge = Fr { o: torso.at([0.0, 0.06, -0.255]), q: lean_q };
        p.lit(b, CYL, badge.o, badge.turn(qx(FRAC_PI_2)), [0.3, 0.03, 0.3], col);
        p.sugar(b, CYL, badge.at([0.0, 0.0, -0.012]), badge.turn(qx(FRAC_PI_2)), [0.25, 0.012, 0.25], [1.0; 3]);
        emblem(p, b, &badge, r, col);
    }
    // Shoulders, arms to the wheel, gloves.
    let glove = if r == 7 { [0.06, 0.05, 0.08] } else { [1.0; 3] };
    for s in [1.0f32, -1.0] {
        let sh = torso.at([s * 0.31 * wide, 0.18, 0.02]);
        let hand = add(wc, quat_rotate(wheel_q, [s * 0.17, 0.0, 0.12]));
        p.stick(b, sh, hand, 0.15, scale(suit, 0.85), None);
        if p.full {
            p.lit(b, LOWBALL, sh, lean_q, [0.24, 0.24, 0.24], suit);
            p.lit(b, LOWBALL, hand, wheel_q, [0.16, 0.14, 0.16], glove);
        }
    }
    if p.full {
        p.lit(b, CYL, torso.at([lean * 0.3, 0.31, 0.05]), lean_q, [0.2, 0.12, 0.2], skin);
    }
    // Extras on the body: a scarf, a neckerchief, a tail.
    let flutter = 0.15 * (p.time * 14.0 + r as f32).sin();
    match r {
        3 => {
            p.lit(b, DONUT, torso.at([0.0, 0.3, 0.02]), lean_q, [0.42, 0.5, 0.4], MINT_DEEP);
            for (s, off) in [(1.0f32, 0.0f32), (-1.0, 0.6)] {
                let a = 0.5 + 1.0 * speed + flutter * (1.0 + off);
                let q = quat_mul(lean_q, quat_mul(qy(s * 0.25), qx(-a)));
                let root = torso.at([s * 0.08, 0.28, -0.2]);
                p.lit(b, ROUND, add(root, quat_rotate(q, [0.0, -0.2, 0.0])), q, [0.14, 0.42, 0.05], MINT_DEEP);
            }
        }
        5 => {
            p.lit(b, CONE, torso.at([0.0, 0.18, 0.25]), quat_mul(lean_q, qx(PI - 0.3)), [0.3, 0.26, 0.08], [1.0, 0.2, 0.01]);
        }
        6 => {
            let n = if p.full { 7 } else { 3 };
            let wag = 0.35 * (p.time * 3.0).sin() - k.steer_vis * 0.4;
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32;
                let o = [wag * t * 0.6, 0.62 + 0.55 * t * t, -0.78 - 0.25 * t + 0.12 * t * t];
                let c = if i + 1 == n { [0.95, 0.95, 0.97] } else if i % 2 == 0 { KITTEN } else { KITTEN_DARK };
                p.lit(b, LOWBALL, o, NO_ROT, [0.14, 0.14, 0.14], c);
            }
        }
        _ => {}
    }
    // The head, looking into the turn, bobbing gently.
    let bob = 0.012 * (p.time * 2.6 + r as f32 * 0.7).sin();
    let hq = quat_mul(lean_q, quat_mul(qy(-k.steer_vis * 0.3), qx(0.04 + 3.0 * bob)));
    let hf = Fr { o: [lean, 1.58 + bob, -0.24], q: hq };
    head(p, b, &hf, r, speed);
}

/// Draw kart `k` (design `k.spec`), the groups `parts` names: the body at
/// the kart's pose (lean, pitch, squash, plus a small engine shake at a
/// standstill), the driver, and the wheels on the road.
#[allow(clippy::too_many_arguments)]
pub(crate) fn kart(p: &mut Paint, k: &crate::kart::Kart, (col, suit): ([f32; 3], Option<[f32; 3]>), pos: [f32; 3], vyaw: f32, max_speed: f32, parts: KartParts) {
    let (q, squash) = kart_pose(k, vyaw, max_speed);
    let speed = (k.speed.abs() / max_speed.max(0.1)).min(1.0);
    // Engine idle: a little shudder when the kart is (nearly) still.
    let idle = (1.0 - k.speed.abs() / 1.5).clamp(0.0, 1.0);
    let shake = idle * 0.012 * (p.time * 31.0 + k.spec as f32 * 1.7).sin();
    let b = Body { pos: [pos[0], pos[1] + shake, pos[2]], q, squash };
    let suit = suit.unwrap_or([0.25 + col[0] * 0.5, 0.25 + col[1] * 0.5, 0.25 + col[2] * 0.5]);
    if parts.chassis {
        chassis(p, &b, k.spec, col, k.wheel_roll);
    }
    if parts.driver {
        driver(p, &b, k, col, suit, speed);
    }
    if parts.wheels {
        wheels(p, k, col, pos, vyaw);
    }
}

/// A racer standing on the podium, cheering: round legs and boots, a belted
/// hip and an egg-shaped body in the kart colour, arms up in the suit
/// colour (the winner punches straight up), and the racer's own head.
#[allow(clippy::too_many_arguments)]
pub(crate) fn podium_racer(p: &mut Paint, pos: [f32; 3], yaw: f32, col: [f32; 3], suit: Option<[f32; 3]>, r: usize, t: f32, winner: bool) {
    let b = Body { pos, q: yaw_q(yaw), squash: [1.2; 3] };
    let deeper = scale(col, 0.72);
    let suit = suit.unwrap_or([0.25 + col[0] * 0.5, 0.25 + col[1] * 0.5, 0.25 + col[2] * 0.5]);
    let dark = p.mats[role::WHEEL];
    for x in [0.21f32, -0.21] {
        p.lit(&b, CYL, [x, 0.42, 0.0], NO_ROT, [0.3, 0.62, 0.3], deeper);
        p.put(&b, LOWBALL, [x, 0.1, 0.07], NO_ROT, [0.36, 0.22, 0.52], dark, [1.0; 3]);
    }
    p.lit(&b, CYL, [0.0, 0.76, 0.0], NO_ROT, [0.66, 0.2, 0.46], suit);
    p.lit(&b, if p.full { BALL } else { LOWBALL }, [0.0, 1.08, 0.0], NO_ROT, [0.82, 0.84, 0.58], col);
    let badge = Fr { o: [0.0, 1.12, 0.29], q: qy(PI) };
    p.sugar(&b, CYL, badge.o, qx(FRAC_PI_2), [0.3, 0.03, 0.3], [1.0; 3]);
    emblem(p, &b, &Fr { o: [0.0, 1.12, 0.305], q: qy(PI) }, r, col);
    for (j, side) in [1.0f32, -1.0].into_iter().enumerate() {
        let wave = 0.22 * (t * 6.0 + j as f32 * 2.2).sin();
        let a = if winner { 0.18 + wave * 0.5 } else { 0.55 + wave };
        let arm_q = qz(-side * a);
        let shoulder = [side * 0.5, 1.28];
        let (sa, ca) = (a.sin(), a.cos());
        p.lit(&b, CYL, [shoulder[0] + side * sa * 0.34, shoulder[1] + ca * 0.34, 0.0], arm_q, [0.2, 0.68, 0.2], suit);
        p.lit(&b, LOWBALL, [shoulder[0] + side * sa * 0.74, shoulder[1] + ca * 0.74, 0.0], NO_ROT, [0.28, 0.28, 0.28], [0.95; 3]);
    }
    let bob = 0.03 * (t * 5.0).sin();
    let hf = Fr { o: [0.0, 1.8 + bob, 0.0], q: qx(-0.08) };
    head(p, &b, &hf, r, 0.0);
}

/// Triangles of everything [`kart`] draws for kart `k` at rest, at both
/// detail levels (full, low): for the budget tests.
#[cfg(test)]
pub(crate) fn triangles_at_rest(spec: usize) -> (u32, u32) {
    let mats = core::array::from_fn(|r| Mat { index: r as u32, tint: [1.0; 3] });
    let ids = KartMeshes::placeholder();
    let meshes = km::build();
    let tri = |rw: &RenderWorld| -> u32 {
        rw.meshes
            .iter()
            .map(|m| match m.mesh.0.checked_sub(KartMeshes::PLACEHOLDER_BASE) {
                Some(k) => meshes[usize::from(k)].triangles(),
                None => evani_render::mesh::triangles_of(m.mesh),
            })
            .sum()
    };
    let k = crate::kart::Kart::new(spec, None, [0.0; 3], 0.0);
    let mut out = [0u32; 2];
    for (i, full) in [true, false].into_iter().enumerate() {
        let mut rw = RenderWorld::default();
        let mut p = Paint { rw: &mut rw, mats: &mats, ids: &ids, full, time: 0.0 };
        kart(&mut p, &k, ([1.0, 0.3, 0.5], None), [0.0; 3], 0.0, 26.0, KartParts::ALL);
        out[i] = tri(&rw);
    }
    (out[0], out[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every design draws, and each stays within its triangle budget: full
    /// detail under 12.5k (the pack at the start is about 100k), low detail
    /// under 2.4k (the Low tier draws seven rivals at low detail and the
    /// player in full, under 30k, on top of its 70k of scenery).
    #[test]
    fn every_design_fits_its_triangle_budget() {
        let counts: Vec<(u32, u32)> = (0..DESIGNS).map(triangles_at_rest).collect();
        for (d, &(full, low)) in counts.iter().enumerate() {
            assert!(full > low && low > 500, "design {d}: {full} / {low}");
            assert!(full <= 12_500, "design {d} ({}) full detail is {full} triangles; all: {counts:?}", NAMES[d].0);
            assert!(low <= 2_400, "design {d} ({}) low detail is {low} triangles; all: {counts:?}", NAMES[d].0);
        }
        let worst_full = counts.iter().map(|c| c.0).max().unwrap_or(0);
        let worst_low = counts.iter().map(|c| c.1).max().unwrap_or(0);
        assert!(worst_full + 7 * worst_low <= 30_000, "Low tier pack {worst_full} + 7 x {worst_low}");
    }

    /// The karts keep to the primitive kart's outline: inside the chase
    /// camera's box round the kart (with the wheels' own reach), under its
    /// kart top, nothing below the road, the nose under its nose height.
    #[test]
    fn every_design_keeps_to_the_kart_outline() {
        let mats = core::array::from_fn(|r| Mat { index: r as u32, tint: [1.0; 3] });
        let ids = KartMeshes::placeholder();
        let meshes = km::build();
        for (d, (name, _)) in NAMES.iter().enumerate() {
            let k = crate::kart::Kart::new(d, None, [0.0; 3], 0.0);
            let mut rw = RenderWorld::default();
            let mut p = Paint { rw: &mut rw, mats: &mats, ids: &ids, full: true, time: 0.0 };
            kart(&mut p, &k, ([1.0, 0.3, 0.5], None), [0.0; 3], 0.0, 26.0, KartParts::ALL);
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for m in &rw.meshes {
                let mesh = match m.mesh.0.checked_sub(KartMeshes::PLACEHOLDER_BASE) {
                    Some(k) => meshes[usize::from(k)].clone(),
                    None => evani_render::mesh::builtin(m.mesh).unwrap(),
                };
                for v in &mesh.vertices {
                    let w = add(m.position, quat_rotate(m.rotation, [v[0] * m.scale[0], v[1] * m.scale[1], v[2] * m.scale[2]]));
                    for a in 0..3 {
                        lo[a] = lo[a].min(w[a]);
                        hi[a] = hi[a].max(w[a]);
                    }
                }
            }
            assert!(lo[1] >= -0.01, "{name} reaches {} under the road", lo[1]);
            assert!(hi[1] <= crate::chasecam::KART_TOP - 0.05, "{name} is {} tall", hi[1]);
            assert!(lo[0] >= -1.12 && hi[0] <= 1.12, "{name} is {} .. {} wide", lo[0], hi[0]);
            assert!(lo[2] >= -1.66 && hi[2] <= 1.62, "{name} is {} .. {} long", lo[2], hi[2]);
        }
    }

    /// The pieces are seen from a distance in the chase view: each design
    /// draws a head (a sphere at the old helmet's place and size) and four
    /// wheels whose centres and radii are the primitive kart's.
    #[test]
    fn heads_and_wheels_sit_where_the_primitive_kart_had_them() {
        let mats = core::array::from_fn(|r| Mat { index: r as u32, tint: [1.0; 3] });
        let ids = KartMeshes::placeholder();
        for d in 0..DESIGNS {
            let k = crate::kart::Kart::new(d, None, [0.0; 3], 0.0);
            let mut rw = RenderWorld::default();
            let mut p = Paint { rw: &mut rw, mats: &mats, ids: &ids, full: true, time: 0.0 };
            kart(&mut p, &k, ([1.0, 0.3, 0.5], None), [0.0; 3], 0.0, 26.0, KartParts { chassis: false, driver: true, wheels: false });
            assert!(rw.meshes.iter().any(|m| m.mesh == MeshId::SPHERE && (m.position[1] - 1.58).abs() < 0.03 && m.scale == HEAD), "design {d} head");
            let mut rw = RenderWorld::default();
            let mut p = Paint { rw: &mut rw, mats: &mats, ids: &ids, full: true, time: 0.0 };
            kart(&mut p, &k, ([1.0, 0.3, 0.5], None), [0.0; 3], 0.0, 26.0, KartParts { chassis: false, driver: false, wheels: true });
            for (x, z, r, w, _) in WHEELS {
                // The tyre (cylinder, tread ring or donut): centred on the
                // wheel, as wide across as the wheel, at least as thick.
                let tyre = |m: &&evani_render::MeshInstance| (m.position[0] - x).abs() < 1e-4 && (m.position[2] - z).abs() < 1e-4 && (m.position[1] - r).abs() < 1e-4;
                assert!(rw.meshes.iter().filter(tyre).any(|m| (m.scale[0] - 2.0 * r).abs() < 1e-3 && m.scale[1] >= w - 1e-4), "design {d} wheel {x} {z}");
            }
        }
    }
}
