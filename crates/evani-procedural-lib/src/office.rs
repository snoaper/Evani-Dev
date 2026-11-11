//! Office buildings (`buildings-office-buildings`, Asset Studio column 13,
//! 2026-11-06): a photoreal mid-rise office block. A glass tower stands
//! between two lower stone-clad wings: the wings are a grid of pale stone
//! pilasters and spandrels (cladding panels with real joints) holding dark
//! aluminium windows of three panes and a transom; the tower is a curtain
//! wall of glass on a dark mullion grid, framed by stone corner piers and a
//! stone frieze under a thin overhanging roof plate, its front opening into a
//! double-height lobby under a cantilevered canopy. Behind every pane is a
//! real interior (slabs, carpet, white ceilings with glowing light panels,
//! desks, chairs, screens and potted plants, a service core), seen through
//! see-through glass. On the roofs stand railings, plant units and a louvred
//! screen; stone planters of shrubs line the wings on a paved plinth.
//!
//! Everything is real geometry in catalogue materials (limestone cladding,
//! aluminium, window glass, plaster, paint, glow, moss-leaved shrubs, soil,
//! concrete paving, gravel), so a game that lists the stone, metal, glass,
//! plaster, glow, foliage and ground recipes (and the see-through pass for
//! the glass) draws the same building the library shows; without the
//! see-through pass the glass draws as an opaque tinted gloss.
//!
//! The front faces +z, the left side -x; the left wing's front corner is the
//! reference's near corner.

use evani_procedural::surface::presets as sp;
use evani_procedural::{Output, ParamSpec as P, Part, Surface, Values, build as b, geo, shapes as sh};

type Mesh = evani_render::mesh::Mesh;

/// The cladding's pale, warm grey stone (linear).
pub const STONE: [f32; 3] = [0.35, 0.345, 0.33];
/// The glass tint (linear): a dark blue-green.
pub const GLASS: [f32; 3] = [0.055, 0.08, 0.085];

pub const PARAMS: [P; 6] = [
    P::num("floors", 5.0, 2.0, 20.0).step(1.0).describe("Floors in the two stone wings"),
    P::num("tower", 2.0, 0.0, 20.0).step(1.0).describe("Floors the glass tower rises over the wings"),
    P::num("width", 44.0, 30.0, 120.0).unit("m").describe("Frontage of the whole building (m)"),
    P::num("depth", 18.0, 12.0, 40.0).unit("m").describe("Depth of the wings (m); the tower is 6 m deeper"),
    P::color("color", STONE).describe("Stone cladding colour"),
    P::color("glass", GLASS).describe("Glass tint"),
];

/// Floor to floor (m).
const FH: f32 = 3.8;
/// The paved plinth's top: every block stands on it.
const BASE: f32 = 0.3;
/// The wings' bay (m, the pilaster spacing it aims for).
const BAY: f32 = 4.4;
/// The tower's mullion module (m).
const MODULE: f32 = 1.5;
/// A spandrel's height below and above its floor line.
const SPAN_LO: f32 = 0.55;
const SPAN_HI: f32 = 0.45;
/// Pilaster width, the wings' corner pier, their stand-out past the
/// spandrel face, and the cladding's depth behind that face.
const PIL: f32 = 0.7;
const PIER: f32 = 1.3;
const OUT: f32 = 0.12;
const BACK: f32 = 0.3;
/// The parapet over the top floor line (to the coping).
const PARAPET: f32 = 0.85;
/// The tower's corner piers: width on each face and stand-out past the glass.
const TPIER: f32 = 2.2;
const TOUT: f32 = 0.4;
/// The tower's frieze over its top floor line and its roof plate.
const FRIEZE: f32 = 2.0;
const PLATE: f32 = 0.4;
/// Cladding joints (m).
const JOINT: f32 = 0.012;
/// Interior slabs stop this far inside the walls' face.
const INSET: f32 = 0.34;

/// The meshes, one per material.
#[derive(Default)]
struct Kit {
    stone: Mesh,
    joint: Mesh,
    frame: Mesh,
    band: Mesh,
    glass: Mesh,
    tower_glass: Mesh,
    ceiling: Mesh,
    carpet: Mesh,
    lobby: Mesh,
    lobby_wall: Mesh,
    wall: Mesh,
    desk: Mesh,
    dark: Mesh,
    light: Mesh,
    warm: Mesh,
    plants: [Mesh; 3],
    flowers: [Mesh; 2],
    pot: Mesh,
    soil: Mesh,
    paving: Mesh,
    roof: Mesh,
    plant_room: Mesh,
    rail: Mesh,
    fascia: Mesh,
    canopy: Mesh,
}

fn add(m: &mut Mesh, part: Mesh) {
    geo::merge(m, &part);
}

/// One wall of a block: its start corner `o` (at the plinth's top), its
/// direction `t` (horizontal) and outward normal `n`, its length.
#[derive(Clone, Copy)]
struct Wall {
    o: [f32; 3],
    t: [f32; 3],
    n: [f32; 3],
    len: f32,
}

impl Wall {
    fn at(&self, u: f32, v: f32, d: f32) -> [f32; 3] {
        [self.o[0] + self.t[0] * u + self.n[0] * d, self.o[1] + v, self.o[2] + self.t[2] * u + self.n[2] * d]
    }

    /// A box over `u` along the wall, `v` up and `d` out from its face.
    fn bx(&self, u: [f32; 2], v: [f32; 2], d: [f32; 2]) -> Mesh {
        let (a, c) = (self.at(u[0], v[0], d[0]), self.at(u[1], v[1], d[1]));
        let lo = [a[0].min(c[0]), a[1].min(c[1]), a[2].min(c[2])];
        let hi = [a[0].max(c[0]), a[1].max(c[1]), a[2].max(c[2])];
        b::cbox([(hi[0] - lo[0]).max(1e-3), (hi[1] - lo[1]).max(1e-3), (hi[2] - lo[2]).max(1e-3)], [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5])
    }

    /// A one-sided pane facing out at depth `d`.
    fn pane(&self, u: [f32; 2], v: [f32; 2], d: f32) -> Mesh {
        let mut m = geo::MeshBuilder::new();
        let a = m.vertex(self.at(u[0], v[0], d), self.n);
        let b2 = m.vertex(self.at(u[1], v[0], d), self.n);
        let c = m.vertex(self.at(u[1], v[1], d), self.n);
        let e = m.vertex(self.at(u[0], v[1], d), self.n);
        m.quad(a, b2, c, e);
        m.finish()
    }
}

/// The four walls of the footprint `x` by `z` (front +z, right +x, back,
/// left), counter-clockwise from above, standing at `y`.
fn walls(x: [f32; 2], z: [f32; 2], y: f32) -> [Wall; 4] {
    let (w, d) = (x[1] - x[0], z[1] - z[0]);
    [
        Wall { o: [x[0], y, z[1]], t: [1.0, 0.0, 0.0], n: [0.0, 0.0, 1.0], len: w },
        Wall { o: [x[1], y, z[1]], t: [0.0, 0.0, -1.0], n: [1.0, 0.0, 0.0], len: d },
        Wall { o: [x[1], y, z[0]], t: [-1.0, 0.0, 0.0], n: [0.0, 0.0, -1.0], len: w },
        Wall { o: [x[0], y, z[0]], t: [0.0, 0.0, 1.0], n: [-1.0, 0.0, 0.0], len: d },
    ]
}

/// Stone cladding over `u` by `v` (its face at `d[1]`): panels at most `pu`
/// by `pv` with real joints, the darker backing showing in them.
fn clad(k: &mut Kit, w: &Wall, u: [f32; 2], v: [f32; 2], d: [f32; 2], pu: f32, pv: f32) {
    let nu = ((u[1] - u[0]) / pu).ceil().max(1.0) as u32;
    let nv = ((v[1] - v[0]) / pv).ceil().max(1.0) as u32;
    let (du, dv) = ((u[1] - u[0]) / nu as f32, (v[1] - v[0]) / nv as f32);
    let g = JOINT * 0.5;
    for i in 0..nu {
        for j in 0..nv {
            let (u0, v0) = (u[0] + i as f32 * du, v[0] + j as f32 * dv);
            let (a, c) = (if i == 0 { 0.0 } else { g }, if i + 1 == nu { 0.0 } else { g });
            let (e, f) = (if j == 0 { 0.0 } else { g }, if j + 1 == nv { 0.0 } else { g });
            add(&mut k.stone, w.bx([u0 + a, u0 + du - c], [v0 + e, v0 + dv - f], d));
        }
    }
    if nu * nv > 1 {
        add(&mut k.joint, w.bx([u[0] + 0.004, u[1] - 0.004], [v[0] + 0.004, v[1] - 0.004], [d[0] - 0.012, d[1] - 0.016]));
    }
}

/// A down-facing quad at height `y` over `x` by `z` (a ceiling light).
fn ceiling_quad(x: [f32; 2], y: f32, z: [f32; 2]) -> Mesh {
    let mut m = geo::MeshBuilder::new();
    let n = [0.0, -1.0, 0.0];
    let a = m.vertex([x[0], y, z[0]], n);
    let b2 = m.vertex([x[1], y, z[0]], n);
    let c = m.vertex([x[1], y, z[1]], n);
    let e = m.vertex([x[0], y, z[1]], n);
    m.quad(a, b2, c, e);
    m.finish()
}

/// A potted plant against wall `w` at `u`, on the floor at `v`, `d` inside.
fn potted(k: &mut Kit, w: &Wall, [u, v, d]: [f32; 3], big: f32, seed: u32, i: u32) {
    let r = big * sh::range(seed, i, 1, 0.26, 0.4);
    let h = big * sh::range(seed, i, 2, 0.55, 0.95);
    let p = 0.42 * big;
    add(&mut k.pot, w.bx([u - p * 0.5, u + p * 0.5], [v, v + p], [d - p * 0.5, d + p * 0.5]));
    let c = w.at(u, v + p + h * 0.45, d);
    let m = sh::blob([r, h * 0.55, r], c, 0.6, seed.wrapping_add(i * 13), 1);
    add(&mut k.plants[(sh::rnd(seed, i, 3) * 3.0) as usize % 3], m);
}

/// A workstation against wall `w`, centred at `u` on the floor at `v`: a
/// desk with its modesty panel, a screen and a chair, facing the window.
fn desk(k: &mut Kit, w: &Wall, u: f32, v: f32, half: f32) {
    add(&mut k.desk, w.bx([u - half, u + half], [v + 0.71, v + 0.75], [-2.7, -1.9]));
    add(&mut k.desk, w.bx([u - half, u + half], [v + 0.08, v + 0.71], [-1.97, -1.93]));
    add(&mut k.dark, w.bx([u - 0.3, u + 0.3], [v + 0.82, v + 1.18], [-2.25, -2.21]));
    add(&mut k.dark, w.bx([u - 0.24, u + 0.24], [v + 0.42, v + 0.5], [-3.2, -2.75]));
    add(&mut k.dark, w.bx([u - 0.24, u + 0.24], [v + 0.5, v + 1.02], [-3.3, -3.22]));
}

/// A window in the opening `u` by `v` of wall `w`: an aluminium frame, two
/// or three panes split by mullions, a transom (a dark fascia panel on the
/// ground floor), and the glass.
fn window(k: &mut Kit, w: &Wall, u: [f32; 2], v: [f32; 2], ground: bool) {
    let f = 0.07;
    let fd = [-0.27, -0.15];
    add(&mut k.frame, w.bx([u[0], u[0] + f], v, fd));
    add(&mut k.frame, w.bx([u[1] - f, u[1]], v, fd));
    add(&mut k.frame, w.bx([u[0] + f, u[1] - f], [v[0], v[0] + f], fd));
    add(&mut k.frame, w.bx([u[0] + f, u[1] - f], [v[1] - f, v[1]], fd));
    let panes = if u[1] - u[0] > 2.6 { 3 } else { 2 };
    for p in 1..panes {
        let x = u[0] + (u[1] - u[0]) * p as f32 / panes as f32;
        add(&mut k.frame, w.bx([x - 0.03, x + 0.03], [v[0] + f, v[1] - f], [-0.25, -0.16]));
    }
    let top = v[1] - 0.55;
    if ground {
        add(&mut k.band, w.bx([u[0] + f, u[1] - f], [top - 0.32, top], [-0.245, -0.165]));
    } else {
        add(&mut k.frame, w.bx([u[0] + f, u[1] - f], [top - 0.03, top + 0.03], [-0.245, -0.165]));
    }
    add(&mut k.glass, w.pane([u[0] + f * 0.5, u[1] - f * 0.5], [v[0] + f * 0.5, v[1] - f * 0.5], -0.205));
}

/// One stone-grid wall of a wing, `floors` storeys: a pilaster at every bay,
/// the free ends left to the corner piers (`free`: whether each end is a
/// free corner; a joined end stops short of the tower), spandrels at every
/// floor line, a base course, the parapet, the windows, and a workstation
/// and plants in the bays inside.
fn grid_wall(k: &mut Kit, w: &Wall, floors: u32, free: [bool; 2], seed: u32, furnish: f32) {
    let us = if free[0] { PIER } else { 0.2 };
    let ue = if free[1] { w.len - PIER } else { w.len - 0.2 };
    if ue - us < 1.5 {
        return;
    }
    let n = ((ue - us) / BAY).round().max(1.0) as u32;
    let bw = (ue - us) / n as f32;
    let h = floors as f32 * FH;
    let top = h + PARAPET;
    // The boundaries: a pilaster at each inner one, and at a joined end.
    let half = |i: u32| -> f32 {
        if (i == 0 && free[0]) || (i == n && free[1]) { 0.0 } else { PIL * 0.5 }
    };
    for i in 0..=n {
        let hw = half(i);
        if hw == 0.0 {
            continue;
        }
        let c = us + i as f32 * bw;
        // (A joined end's pilaster stops 0.15 m short of the tower.)
        let (a, e) = if i == 0 { (us - 0.05, us + PIL - 0.05) } else if i == n { (ue - PIL + 0.05, ue + 0.05) } else { (c - hw, c + hw) };
        clad(k, w, [a, e], [0.0, top], [-BACK - 0.02, OUT], PIL, FH * 0.5);
    }
    for i in 0..n {
        let lo = us + i as f32 * bw + if i == 0 && !free[0] { PIL - 0.05 } else { half(i) };
        let hi = us + (i + 1) as f32 * bw - if i + 1 == n && !free[1] { PIL - 0.05 } else { half(i + 1) };
        if hi - lo < 0.6 {
            continue;
        }
        let sd = [-BACK, 0.0];
        // Base course, spandrels, the top band with the parapet.
        clad(k, w, [lo, hi], [0.0, 0.4], sd, (hi - lo) * 0.5 + 0.01, 1.0);
        for j in 1..floors {
            let y = j as f32 * FH;
            clad(k, w, [lo, hi], [y - SPAN_LO, y + SPAN_HI], sd, (hi - lo) * 0.5 + 0.01, 2.0);
        }
        clad(k, w, [lo, hi], [h - SPAN_LO, top], sd, (hi - lo) * 0.5 + 0.01, 2.0);
        for j in 0..floors {
            let y = j as f32 * FH;
            let v0 = if j == 0 { 0.4 } else { y + SPAN_HI };
            window(k, w, [lo, hi], [v0, y + FH - SPAN_LO], j == 0);
            let id = seed.wrapping_add(i * 131 + j * 17);
            // (No desk in the end bays: the next wall's would meet it.)
            if i > 0 && i + 1 < n && sh::rnd(id, 1, 7) < 0.85 * furnish {
                desk(k, w, (lo + hi) * 0.5 + sh::range(id, 1, 8, -0.4, 0.4), y, 0.75);
            }
            if sh::rnd(id, 2, 7) < 0.6 * furnish {
                let pu = if sh::rnd(id, 3, 7) < 0.5 { lo + 0.45 } else { hi - 0.45 };
                potted(k, w, [pu, y, -0.75], 1.0, seed, i * 31 + j);
            }
        }
    }
    // The coping along the whole wall.
    let (c0, c1) = (if free[0] { us - 0.25 } else { us - 0.05 }, if free[1] { ue + 0.25 } else { ue + 0.05 });
    add(&mut k.fascia, w.bx([c0, c1], [top, top + 0.06], [-BACK - 0.06, OUT + 0.05]));
}

/// A wing's corner pier (stone, `h` tall) at the corner where wall `w` ends.
fn corner_pier(k: &mut Kit, w: &Wall, h: f32) {
    clad(k, w, [w.len - PIER, w.len + OUT], [0.0, h], [-PIER, OUT], PIER * 0.5 + 0.01, FH * 0.5);
    add(&mut k.fascia, w.bx([w.len - PIER - 0.05, w.len + OUT + 0.05], [h, h + 0.06], [-PIER - 0.05, OUT + 0.05]));
}

/// A block's floors inside: slabs with white soffits, carpet, a light grid
/// in the ceilings, a service core if it is deep enough; `lobby` leaves the
/// first floor's slab out over the front `lobby` metres (the double-height
/// hall) and gives that hall stone and warm downlights.
fn interior(k: &mut Kit, x: [f32; 2], z: [f32; 2], floors: u32, lobby: f32, roofed: bool) {
    let (xi, zi) = ([x[0] + INSET, x[1] - INSET], [z[0] + INSET, z[1] - INSET]);
    if xi[1] - xi[0] < 1.0 || zi[1] - zi[0] < 1.0 {
        return;
    }
    let y0 = BASE;
    // The ground floor's finish: carpet, the lobby's stone.
    if lobby > 0.0 {
        let zl = zi[1] - lobby;
        add(&mut k.carpet, b::cbox([xi[1] - xi[0], 0.02, zl - zi[0]], [(xi[0] + xi[1]) * 0.5, y0 + 0.01, (zi[0] + zl) * 0.5]));
        add(&mut k.lobby, b::cbox([xi[1] - xi[0], 0.03, zi[1] - zl], [(xi[0] + xi[1]) * 0.5, y0 + 0.015, (zl + zi[1]) * 0.5]));
    } else {
        add(&mut k.carpet, b::cbox([xi[1] - xi[0], 0.02, zi[1] - zi[0]], [(xi[0] + xi[1]) * 0.5, y0 + 0.01, (zi[0] + zi[1]) * 0.5]));
    }
    for j in 1..=floors {
        let y = y0 + j as f32 * FH;
        let zt = if j == 1 && lobby > 0.0 { zi[1] - lobby } else { zi[1] };
        add(&mut k.ceiling, b::cbox([xi[1] - xi[0], 0.3, zt - zi[0]], [(xi[0] + xi[1]) * 0.5, y - 0.15, (zi[0] + zt) * 0.5]));
        if j < floors {
            add(&mut k.carpet, b::cbox([xi[1] - xi[0], 0.02, zt - zi[0]], [(xi[0] + xi[1]) * 0.5, y + 0.01, (zi[0] + zt) * 0.5]));
        } else if roofed {
            add(&mut k.roof, b::cbox([xi[1] - xi[0], 0.1, zi[1] - zi[0]], [(xi[0] + xi[1]) * 0.5, y + 0.05, (zi[0] + zi[1]) * 0.5]));
        }
    }
    // Light panels: 0.6 by 1.2 m every 2.4 by 3 m.
    let (nx, nz) = (((xi[1] - xi[0] - 1.0) / 3.0).floor().max(1.0) as u32, ((zi[1] - zi[0] - 1.0) / 2.4).floor().max(1.0) as u32);
    for j in 0..floors {
        let y = y0 + (j + 1) as f32 * FH - 0.305;
        for a in 0..nx {
            for c in 0..nz {
                let cx = xi[0] + (xi[1] - xi[0]) * (a as f32 + 0.5) / nx as f32;
                let cz = zi[0] + (zi[1] - zi[0]) * (c as f32 + 0.5) / nz as f32;
                let in_lobby = lobby > 0.0 && j == 0 && cz > zi[1] - lobby;
                if in_lobby {
                    continue;
                }
                add(&mut k.light, ceiling_quad([cx - 0.6, cx + 0.6], y, [cz - 0.3, cz + 0.3]));
            }
        }
    }
    if lobby > 0.0 {
        // Warm downlights in the hall's high ceiling.
        let y = y0 + 2.0 * FH - 0.305;
        let (mx, mz) = (((xi[1] - xi[0]) / 2.0).floor().max(1.0) as u32, (lobby / 2.0).floor().max(1.0) as u32);
        for a in 0..mx {
            for c in 0..mz {
                let cx = xi[0] + (xi[1] - xi[0]) * (a as f32 + 0.5) / mx as f32;
                let cz = zi[1] - lobby + lobby * (c as f32 + 0.5) / mz as f32;
                add(&mut k.warm, ceiling_quad([cx - 0.22, cx + 0.22], y, [cz - 0.22, cz + 0.22]));
            }
        }
    }
    // The service core (lifts, stairs, washrooms) behind the open floors.
    let (cx, cz) = ([x[0] + 7.5, x[1] - 7.5], [z[0] + 7.5, z[1] - 7.5 - lobby.max(0.0) * 0.3]);
    if cx[1] - cx[0] > 2.0 && cz[1] - cz[0] > 2.0 {
        let hgt = floors as f32 * FH - 0.3;
        add(&mut k.wall, b::cbox([cx[1] - cx[0], hgt - 0.02, cz[1] - cz[0]], [(cx[0] + cx[1]) * 0.5, y0 + 0.02 + hgt * 0.5, (cz[0] + cz[1]) * 0.5]));
    }
}

/// A stone wing: footprint `x` by `z`, `floors` storeys, the corner piers
/// at its free corners; `joined` says which side (+x or -x) meets the tower.
fn wing(k: &mut Kit, x: [f32; 2], z: [f32; 2], floors: u32, joined_right: bool, seed: u32, furnish: f32) {
    let ws = walls(x, z, BASE);
    let h = floors as f32 * FH + PARAPET;
    // Front, right, back, left; a wing has no wall on its tower side.
    for (i, w) in ws.iter().enumerate() {
        let tower_side = if joined_right { i == 1 } else { i == 3 };
        if tower_side {
            continue;
        }
        // Which ends are free corners (the wall's start and end).
        let free = match (i, joined_right) {
            (0, true) => [true, false],
            (2, true) => [false, true],
            (0, false) => [false, true],
            (2, false) => [true, false],
            _ => [true, true],
        };
        grid_wall(k, w, floors, free, seed.wrapping_add(i as u32 * 1009), furnish);
        if free[1] {
            corner_pier(k, w, h);
        }
    }
    interior(k, x, z, floors, 0.0, true);
    // Roof railings, set back from the parapets.
    let y = BASE + floors as f32 * FH + 0.1;
    let (a, c) = ([x[0] + 0.9, x[1] - 0.9], [z[0] + 0.9, z[1] - 0.9]);
    railing(k, [a[0], y, c[1]], [a[1], y, c[1]]);
    railing(k, [a[0], y, c[0]], [a[0], y, c[1]]);
    railing(k, [a[1], y, c[0]], [a[1], y, c[1]]);
    railing(k, [a[0], y, c[0]], [a[1], y, c[0]]);
}

/// A roof railing from `a` to `c`: posts, a top and a middle rail.
fn railing(k: &mut Kit, a: [f32; 3], c: [f32; 3]) {
    let len = ((c[0] - a[0]).powi(2) + (c[2] - a[2]).powi(2)).sqrt();
    if len < 0.5 {
        return;
    }
    let n = (len / 2.0).ceil().max(1.0) as u32;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let p = [a[0] + (c[0] - a[0]) * t, a[1], a[2] + (c[2] - a[2]) * t];
        add(&mut k.rail, b::cbox([0.05, 1.05, 0.05], [p[0], p[1] + 0.525, p[2]]));
    }
    for hgt in [1.05, 0.55] {
        let (p, q) = ([a[0], a[1] + hgt, a[2]], [c[0], c[1] + hgt, c[2]]);
        let lo = [p[0].min(q[0]) - 0.025, p[1] - 0.025, p[2].min(q[2]) - 0.025];
        let hi = [p[0].max(q[0]) + 0.025, p[1] + 0.025, p[2].max(q[2]) + 0.025];
        add(&mut k.rail, b::cbox([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5]));
    }
}

/// A rooftop plant unit `size` standing at `at` (bottom centre): a cased
/// box with louvred sides and a fan on top.
fn plant_unit(k: &mut Kit, at: [f32; 3], size: [f32; 3]) {
    add(&mut k.plant_room, sh::block(size, at));
    // Louvres along both long sides.
    let n = (size[1] / 0.18) as u32;
    for i in 1..n {
        let y = at[1] + i as f32 * size[1] / n as f32;
        for s in [-1.0f32, 1.0] {
            add(&mut k.rail, b::cbox([size[0] * 0.86, 0.035, 0.05], [at[0], y, at[2] + s * (size[2] * 0.5 + 0.02)]));
        }
    }
    let r = size[2].min(size[0]) * 0.3;
    add(&mut k.dark, b::rod([at[0], at[1] + size[1], at[2]], [at[0], at[1] + size[1] + 0.25, at[2]], r, 16));
}

/// The glass tower: footprint `x` by `z`, `floors` storeys; its front
/// opens into a double-height lobby under a canopy.
fn tower(k: &mut Kit, x: [f32; 2], z: [f32; 2], floors: u32, seed: u32, furnish: f32) {
    let ws = walls(x, z, BASE);
    let h = floors as f32 * FH;
    let lobby = (z[1] - z[0]).min(9.0);
    for (i, w) in ws.iter().enumerate() {
        let (u0, u1) = (TPIER, w.len - TPIER);
        let front = i == 0;
        // The corner pier at the wall's end, to the frieze's top.
        clad(k, w, [w.len - TPIER, w.len + TOUT], [0.0, h + FRIEZE], [-TPIER, TOUT], TPIER * 0.5 + 0.01, 1.25);
        if u1 - u0 < 1.0 {
            continue;
        }
        // The frieze over the curtain wall.
        clad(k, w, [u0, u1], [h - 0.4, h + FRIEZE], [-0.3, TOUT - 0.08], 2.4, FRIEZE + 0.41);
        // Mullions, floor bands, transoms and the glass.
        let m = ((u1 - u0) / MODULE).round().max(1.0) as u32;
        for a in 1..m {
            let uu = u0 + (u1 - u0) * a as f32 / m as f32;
            add(&mut k.frame, w.bx([uu - 0.04, uu + 0.04], [0.0, h - 0.4], [-0.1, 0.12]));
        }
        for j in 0..floors {
            let y = j as f32 * FH;
            let in_lobby = front && j < 2;
            if j > 0 && !(front && j == 1) {
                add(&mut k.band, w.bx([u0, u1], [y - 0.26, y + 0.06], [-0.08, 0.06]));
            }
            if in_lobby {
                if j == 0 {
                    // A band of pale metal panels across the hall's glass.
                    add(&mut k.canopy, w.bx([u0, u1], [3.2, 3.75], [-0.09, 0.08]));
                    add(&mut k.frame, w.bx([u0, u1], [FH + 2.0 - 0.04, FH + 2.0 + 0.04], [-0.09, 0.07]));
                    // The hall's tall glass is clear, its warm light showing.
                    add(&mut k.glass, w.pane([u0, u1], [0.0, 2.0 * FH - 0.4], -0.02));
                }
            } else {
                add(&mut k.frame, w.bx([u0, u1], [y + 2.75 - 0.03, y + 2.75 + 0.03], [-0.09, 0.07]));
                add(&mut k.tower_glass, w.pane([u0, u1], [y + 0.06, y + FH - 0.26], -0.02));
            }
            // Workstations and plants in each three-module bay.
            if !in_lobby {
                let bays = (m / 3).max(1);
                for q in 0..bays {
                    let id = seed.wrapping_add(i as u32 * 977 + j * 41 + q * 7);
                    let lo = u0 + (u1 - u0) * q as f32 / bays as f32;
                    let hi = u0 + (u1 - u0) * (q + 1) as f32 / bays as f32;
                    let end = bays > 2 && (q == 0 || q + 1 == bays);
                    if !end && sh::rnd(id, 1, 9) < 0.85 * furnish {
                        desk(k, w, (lo + hi) * 0.5 + sh::range(id, 2, 9, -0.5, 0.5), y, 0.8);
                    }
                    if sh::rnd(id, 3, 9) < 0.55 * furnish {
                        potted(k, w, [if sh::rnd(id, 4, 9) < 0.5 { lo + 0.5 } else { hi - 0.5 }, y, -0.6], 1.0, seed ^ 0x55, i as u32 * 997 + j * 37 + q);
                    }
                }
            }
        }
        if front {
            // The entrance: doors in the middle, a stone column in the hall's
            // glass, the canopy over it with warm lights underneath.
            let mid = (u0 + u1) * 0.5;
            for du in [-1.6f32, 0.0, 1.6] {
                add(&mut k.frame, w.bx([mid + du - 0.06, mid + du + 0.06], [0.0, 2.7], [-0.08, 0.14]));
            }
            add(&mut k.frame, w.bx([mid - 1.66, mid + 1.66], [2.62, 2.76], [-0.08, 0.14]));
            for du in [-0.8f32, 0.8] {
                add(&mut k.frame, w.bx([mid + du - 0.02, mid + du + 0.02], [0.95, 1.15], [0.14, 0.2]));
            }
            let col = u0 + (u1 - u0) * 0.8;
            clad(k, w, [col - 0.5, col + 0.5], [0.0, 2.0 * FH], [-0.3, 0.3], 1.01, 1.25);
            let cy = 2.0 * FH;
            // The canopy: pale metal panels with dark joints round a core.
            let (ca, cb) = (u0 - TPIER * 0.6, u1 + TPIER * 0.6);
            add(&mut k.band, w.bx([ca + 0.01, cb - 0.01], [cy + 0.01, cy + 0.64], [TOUT, TOUT + 3.19]));
            let np = ((cb - ca) / 2.4).ceil().max(1.0) as u32;
            for a in 0..np {
                let (p0, p1) = (ca + (cb - ca) * a as f32 / np as f32, ca + (cb - ca) * (a + 1) as f32 / np as f32);
                let g = if a == 0 { 0.0 } else { 0.006 };
                let g1 = if a + 1 == np { 0.0 } else { 0.006 };
                add(&mut k.canopy, w.bx([p0 + g, p1 - g1], [cy, cy + 0.65], [TOUT + 0.6, TOUT + 3.2]));
                add(&mut k.canopy, w.bx([p0 + g, p1 - g1], [cy + 0.05, cy + 0.6], [TOUT, TOUT + 0.6]));
            }
            let n = ((u1 - u0) / 2.2).floor().max(1.0) as u32;
            for a in 0..n {
                let uu = u0 + (u1 - u0) * (a as f32 + 0.5) / n as f32;
                for dd in [TOUT + 1.0, TOUT + 2.6] {
                    let p = w.at(uu, cy - 0.005, dd);
                    add(&mut k.warm, ceiling_quad([p[0] - 0.18, p[0] + 0.18], p[1], [p[2] - 0.18, p[2] + 0.18]));
                }
            }
            // The hall: a warm back wall, a stone reception desk and two big
            // plants.
            add(&mut k.lobby_wall, w.bx([u0 + 0.3, u1 - 0.3], [0.03, 2.0 * FH - 0.31], [-lobby + 0.1, -lobby + 0.35]));
            add(&mut k.lobby, w.bx([mid - 2.0, mid + 2.0], [0.0, 1.05], [-6.4, -5.4]));
            potted(k, w, [u0 + 1.2, 0.0, -1.2], 1.9, seed ^ 0x77, 9001);
            potted(k, w, [u1 - 1.2, 0.0, -1.2], 1.9, seed ^ 0x78, 9002);
        }
    }
    interior(k, x, z, floors, lobby, false);
    // The roof plate over the frieze, and what stands on it.
    let top = BASE + h + FRIEZE;
    let o = 1.2;
    add(&mut k.fascia, b::cbox([x[1] - x[0] + 2.0 * o, PLATE, z[1] - z[0] + 2.0 * o], [(x[0] + x[1]) * 0.5, top + PLATE * 0.5, (z[0] + z[1]) * 0.5]));
    add(&mut k.roof, b::cbox([x[1] - x[0] + 2.0 * o - 0.4, 0.06, z[1] - z[0] + 2.0 * o - 0.4], [(x[0] + x[1]) * 0.5, top + PLATE + 0.03, (z[0] + z[1]) * 0.5]));
    let y = top + PLATE + 0.06;
    let (a, c) = ([x[0] - o + 0.5, x[1] + o - 0.5], [z[0] - o + 0.5, z[1] + o - 0.5]);
    railing(k, [a[0], y, c[1]], [a[1], y, c[1]]);
    railing(k, [a[0], y, c[0]], [a[0], y, c[1]]);
    railing(k, [a[1], y, c[0]], [a[1], y, c[1]]);
    railing(k, [a[0], y, c[0]], [a[1], y, c[0]]);
    // Plant units towards the roof's front left, the biggest at the front.
    let units = ((x[1] - x[0]) / 6.0).floor().clamp(1.0, 4.0) as u32;
    for i in 0..units {
        let big = [1.0, 0.85, 0.75, 0.75][i as usize % 4];
        let ux = x[0] + 3.2 + i as f32 * 3.9;
        let uz = z[1] - 4.5 - (i % 2) as f32 * 3.5 + sh::range(seed, i, 61, -0.6, 0.6);
        plant_unit(k, [ux.min(x[1] - 3.0), y, uz], [3.0 * big, 2.3 * big, 2.3 * big]);
    }
}

/// A stone planter of shrubs along wall `w` over `u`, `d` out from its face.
fn planter(k: &mut Kit, w: &Wall, u: [f32; 2], d: [f32; 2], seed: u32, density: f32) {
    if u[1] - u[0] < 1.0 {
        return;
    }
    let (t, hgt) = (0.15, 0.6);
    add(&mut k.stone, w.bx(u, [0.0, hgt], [d[1] - t, d[1]]));
    add(&mut k.stone, w.bx(u, [0.0, hgt], [d[0], d[0] + t]));
    add(&mut k.stone, w.bx([u[0], u[0] + t], [0.0, hgt], [d[0] + t, d[1] - t]));
    add(&mut k.stone, w.bx([u[1] - t, u[1]], [0.0, hgt], [d[0] + t, d[1] - t]));
    add(&mut k.soil, w.bx([u[0] + t, u[1] - t], [0.0, hgt - 0.08], [d[0] + t, d[1] - t]));
    let step = 0.62 / density.clamp(0.3, 1.0);
    let n = ((u[1] - u[0] - 0.5) / step).floor().max(1.0) as u32;
    let mid = (d[0] + d[1]) * 0.5;
    for i in 0..n {
        let uu = u[0] + 0.35 + (i as f32 + 0.5) * (u[1] - u[0] - 0.7) / n as f32 + sh::range(seed, i, 71, -0.15, 0.15);
        let dd = mid + sh::range(seed, i, 72, -0.25, 0.25);
        let r = sh::range(seed, i, 73, 0.3, 0.5);
        let hh = r * sh::range(seed, i, 74, 0.8, 1.2);
        let c = w.at(uu, hgt - 0.1 + hh * 0.75, dd);
        add(&mut k.plants[(sh::rnd(seed, i, 75) * 3.0) as usize % 3], sh::blob([r, hh, r], c, 0.65, seed.wrapping_add(i * 19), 2));
        if sh::rnd(seed, i, 76) < 0.3 {
            let kind = usize::from(sh::rnd(seed, i, 77) < 0.5);
            for f in 0..6u32 {
                let a = sh::rnd(seed, i * 8 + f, 78) * std::f32::consts::TAU;
                let up = sh::range(seed, i * 8 + f, 79, 0.35, 0.85);
                let rr = (1.0 - up * up).sqrt() * r * 0.92;
                let p = [c[0] + rr * a.cos(), c[1] + up * hh * 0.92, c[2] + rr * a.sin()];
                let fr = sh::range(seed, i * 8 + f, 80, 0.07, 0.11);
                add(&mut k.flowers[kind], sh::blob([fr, fr * 0.8, fr], p, 0.4, seed.wrapping_add(i * 8 + f), 0));
            }
        }
    }
}

fn push(parts: &mut Vec<Part>, m: Mesh, s: Surface) {
    if !m.indices.is_empty() {
        parts.push(Part::surfaced(m, s));
    }
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// The asset.
pub fn generate(v: &Values, seed: u32) -> Output {
    let floors = v.f("floors").round().clamp(2.0, 20.0) as u32;
    let extra = v.f("tower").round().clamp(0.0, 20.0) as u32;
    let width = v.f("width").clamp(30.0, 120.0);
    let depth = v.f("depth").clamp(12.0, 40.0);
    let density = v.density().clamp(0.2, 1.0);
    // Fewer furnishings on very tall or long buildings (the budget).
    let furnish = (density * (12.0 / (floors + extra) as f32).min(1.0) * (60.0 / width).min(1.0)).clamp(0.15, 1.0);
    let mut k = Kit::default();

    // The plan: the left wing, the tower, the right wing (set back, so the
    // tower hides its near end from the front-left).
    let tw = (width * 0.36).clamp(12.0, 36.0);
    let lw = (width - tw) * 0.5;
    let x0 = -width * 0.5;
    let (tx0, tx1) = (x0 + lw, x0 + lw + tw);
    let zf = depth * 0.5;
    let left = ([x0, tx0], [-zf, zf]);
    let towerp = ([tx0, tx1], [-zf - 2.0, zf + 4.0]);
    let right = ([tx1, width * 0.5], [-zf, zf - 1.0]);
    wing(&mut k, left.0, left.1, floors, true, seed, furnish);
    wing(&mut k, right.0, right.1, floors, false, seed ^ 0x2b, furnish);
    tower(&mut k, towerp.0, towerp.1, floors + extra, seed ^ 0x3c, furnish);

    // A louvred plant screen on the left wing's roof by the tower.
    let ry = BASE + floors as f32 * FH + 0.1;
    let sw = (lw * 0.35).min(6.0);
    add(&mut k.band, sh::block([sw, 2.0, 2.6], [tx0 - 2.0 - sw * 0.5, ry, -1.0]));
    for i in 0..(sw / 0.25) as u32 {
        let xx = tx0 - 2.0 - sw + 0.125 + i as f32 * 0.25;
        add(&mut k.rail, b::cbox([0.05, 1.9, 0.08], [xx, ry + 1.0, 0.33]));
    }

    // The planters: along the left wing's side and front (meeting at its
    // corner) and the right wing's front.
    let lws = walls(left.0, left.1, BASE);
    let rws = walls(right.0, right.1, BASE);
    planter(&mut k, &lws[0], [-1.9, lws[0].len - 0.6], [0.45, 1.9], seed ^ 0x101, density);
    planter(&mut k, &lws[3], [1.2, lws[3].len + 0.46], [0.45, 1.9], seed ^ 0x102, density);
    planter(&mut k, &rws[0], [0.6, rws[0].len - 1.0], [0.45, 1.9], seed ^ 0x103, density);

    // The paved plinth under it all, and a landing before the doors.
    let (px, pz) = ([x0 - 2.6, width * 0.5 + 2.6], [-zf - 4.5, zf + 2.4]);
    add(&mut k.paving, b::cbox([px[1] - px[0], BASE, pz[1] - pz[0]], [(px[0] + px[1]) * 0.5, BASE * 0.5, (pz[0] + pz[1]) * 0.5]));
    let (lx, lz) = ([tx0 - 1.5, tx1 + 1.5], [zf + 2.4, zf + 4.0 + 6.5]);
    add(&mut k.paving, b::cbox([lx[1] - lx[0], BASE, lz[1] - lz[0]], [(lx[0] + lx[1]) * 0.5, BASE * 0.5, (lz[0] + lz[1]) * 0.5]));

    // The materials.
    let stone_rgb = v.color("color");
    let glass_rgb = v.color("glass");
    // Fine, even, pale stone: the stone recipe's granular variant, its
    // second colour close to the first (no dark blotches), no pits or veins.
    let pale = |rgb: [f32; 3], o: u32| sp::ROCK_LIMESTONE.variant(0).extras(0).set_colour(0, rgb).set_colour(1, mul(rgb, [0.9, 0.9, 0.91])).set_colour(2, mul(rgb, [1.08, 1.08, 1.07])).number(1, 0.0).number(3, 0.6).number(7, 0.0).seed(seed ^ o).scale(1.0);
    let stone = pale(stone_rgb, 0);
    let mut parts = Vec::new();
    push(&mut parts, k.stone, stone);
    push(&mut parts, k.joint, pale(mul(stone_rgb, [0.45, 0.45, 0.46]), 1));
    push(&mut parts, k.frame, sp::METAL_ALUMINIUM.tint([0.09, 0.095, 0.1]).seed(seed ^ 2));
    push(&mut parts, k.band, sp::METAL_ALUMINIUM.tint([0.13, 0.14, 0.15]).seed(seed ^ 3));
    push(&mut parts, k.fascia, sp::METAL_ALUMINIUM.tint([0.62, 0.63, 0.64]).seed(seed ^ 4));
    push(&mut parts, k.canopy, sp::METAL_ALUMINIUM.tint([0.36, 0.365, 0.37]).seed(seed ^ 5));
    push(&mut parts, k.rail, sp::METAL_ALUMINIUM.tint([0.22, 0.225, 0.23]).seed(seed ^ 6));
    push(&mut parts, k.plant_room, sp::METAL_ALUMINIUM.tint([0.38, 0.39, 0.4]).seed(seed ^ 7));
    push(&mut parts, k.ceiling, sp::PLASTER_WHITE.tint([0.62, 0.62, 0.6]).seed(seed ^ 8));
    push(&mut parts, k.carpet, sp::WALL_PAINT_MATTE.tint([0.3, 0.28, 0.25]).seed(seed ^ 9));
    push(&mut parts, k.lobby, pale(mul(stone_rgb, [1.05, 1.02, 0.96]), 10));
    push(&mut parts, k.lobby_wall, sp::WALL_PAINT_MATTE.tint([0.42, 0.3, 0.17]).seed(seed ^ 17));
    push(&mut parts, k.wall, sp::WALL_PAINT_MATTE.tint([0.46, 0.45, 0.43]).seed(seed ^ 11));
    push(&mut parts, k.desk, sp::PLASTER_WHITE.tint([0.58, 0.57, 0.54]).seed(seed ^ 12));
    push(&mut parts, k.dark, sp::WALL_PAINT_MATTE.tint([0.03, 0.03, 0.033]).seed(seed ^ 13));
    push(&mut parts, k.pot, sp::WALL_PAINT_MATTE.tint([0.07, 0.07, 0.075]).seed(seed ^ 14));
    push(&mut parts, k.light, sp::GLOW_BULB_WHITE.set_colour(0, [0.92, 0.95, 1.0]).seed(seed ^ 15));
    push(&mut parts, k.warm, sp::GLOW_LAMP_WARM.seed(seed ^ 16));
    // Shrubs: the moss's fine leafy cushion in deep greens (its highlights
    // a little lighter and greener, its hollows darker).
    let leafy = |c: [f32; 3], o: u32, sc: f32| sp::GROUND_MOSS_CARPET.set_colour(0, c).set_colour(1, mul(c, [1.45, 1.4, 1.2])).set_colour(2, mul(c, [0.55, 0.6, 0.6])).seed(seed ^ o).scale(sc);
    for (i, (m, c)) in k.plants.into_iter().zip([[0.024, 0.05, 0.015], [0.034, 0.064, 0.018], [0.02, 0.042, 0.017]]).enumerate() {
        push(&mut parts, m, leafy(c, 20 + i as u32, 0.14));
    }
    for (i, (m, c)) in k.flowers.into_iter().zip([[0.5, 0.14, 0.26], [0.3, 0.16, 0.46]]).enumerate() {
        push(&mut parts, m, leafy(c, 24 + i as u32, 0.08));
    }
    push(&mut parts, k.soil, sp::GROUND_SOIL_FOREST_FLOOR.tint([0.05, 0.04, 0.03]).seed(seed ^ 26).scale(0.6));
    // Paving: fresh cast concrete (no map cracks), pale grey.
    push(&mut parts, k.paving, evani_procedural::surface::concrete_preset(sp::CONCRETE_SMOOTH, 0).tint([0.4, 0.4, 0.39]).seed(seed ^ 27).scale(2.0));
    push(&mut parts, k.roof, sp::GROUND_GRAVEL_SCREE.tint([0.22, 0.22, 0.22]).seed(seed ^ 28).scale(0.8));
    // The glass last (drawn over what is behind it): the wings' clear panes
    // (window glass, a touch of frost under the refraction threshold) and
    // the tower's darker curtain wall (the stained variant's even tint).
    push(&mut parts, k.glass, sp::GLASS_WINDOW_CLEAR.set_colour(0, glass_rgb).extras(0).number(0, 1.0).number(1, 0.098).seed(seed ^ 29).scale(3.0).see_through());
    push(&mut parts, k.tower_glass, sp::GLASS_WINDOW_CLEAR.variant(3).set_colour(0, glass_rgb).extras(0).number(0, 0.0).number(1, 0.0).number(2, 0.0).number(3, 0.0).seed(seed ^ 30).scale(6.0).see_through());
    Output::Parts(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(d: f32) -> Output {
        generate(&Values::defaults(&PARAMS).with_density(d), 7)
    }

    #[test]
    fn the_office_stands_on_the_ground_within_its_budget() {
        let (hi, lo) = (make(1.0), make(evani_procedural::detail::Density::for_tier("low")));
        let (t_hi, t_lo) = (hi.triangles(), lo.triangles());
        println!("buildings-office-buildings: {t_hi} triangles at High, {t_lo} at Low");
        assert!(hi.is_valid() && t_hi < 300_000 && t_lo <= t_hi, "{t_hi} / {t_lo}");
        let (lo_b, hi_b) = hi.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo_b[1] > -1e-3, "base at {}", lo_b[1]);
        // Five floors of wings and two more of tower, its roof plant on top.
        assert!(hi_b[1] > 7.0 * FH && hi_b[1] < 7.0 * FH + 8.0, "top at {}", hi_b[1]);
        // The glass is see-through, every other part opaque.
        let Output::Parts(ps) = &hi else { panic!("parts") };
        let see: Vec<bool> = ps.iter().filter_map(|p| p.surface.map(|s| s.blend)).collect();
        assert_eq!(see.iter().filter(|b| **b).count(), 2, "two glazing parts");
    }

    #[test]
    fn the_biggest_office_keeps_its_budget() {
        let mut v = Values::defaults(&PARAMS);
        for (name, x) in [("floors", 20.0), ("tower", 20.0), ("width", 120.0), ("depth", 40.0)] {
            v.set(name, evani_procedural::spec::Value::F32(x));
        }
        let o = generate(&v, 3);
        assert!(o.is_valid() && o.triangles() < 400_000, "{} triangles", o.triangles());
    }
}
