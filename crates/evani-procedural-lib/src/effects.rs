//! Category 24, Particles, effects and destruction: basic generators (batch A).
//! Each is one still moment of the effect as a CPU mesh: puffs rising and
//! growing (smoke, steam, dust), glowing blades and streaks (fire, sparks),
//! broken chunks flying apart, a trail, a splash crown, marks pressed into
//! the ground. Glowing parts use the look's `glow` material; faint ones the
//! renderer's screen-door fade. Inspired by webgpu-vfx's and
//! threejs-particles' ideas (seeded particles along simple paths) and
//! three.js (MIT) BufferGeometry, TubeGeometry and LatheGeometry; written
//! fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const SMOKE: [f32; 3] = [0.3, 0.3, 0.31];
    const FIRE: [f32; 3] = [0.95, 0.4, 0.05];

    /// Puffs rising from the origin up to `h`, growing from `r0` to `r1`,
    /// drifting by `drift` (x at the top) and wobbling by seed.
    fn column(n: u32, h: f32, r0: f32, r1: f32, drift: f32, s: u32) -> geo::Mesh {
        let mut pts = Vec::new();
        let mut rad = Vec::new();
        for i in 0..n {
            let t = (i as f32 + sh::rnd(s, i, 1)) / n as f32;
            let w = (noise::value2(t * 3.0, 1.0, s) - 0.5) * h * 0.25 * t;
            pts.push([drift * t * t + w, r0 + t * h, (noise::value2(t * 3.0, 7.0, s) - 0.5) * h * 0.2 * t]);
            rad.push(r0 + (r1 - r0) * t);
        }
        sh::puffs(&pts, &rad, s)
    }

    basic!(_r, "effects-smoke", "effects", "smoke", "Smoke", "ground",
        [P::num("height", 4.0, 0.5, 30.0).unit("m").describe("Height of the column (m)"), P::num("drift", 1.2, -10.0, 10.0).unit("m").describe("How far the wind carries the top (m)"), P::color("color", SMOKE).describe("Smoke colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            Output::Parts(vec![Part::new(column(22, h, h * 0.05, h * 0.22, v.f("drift"), s), v.color("color")).alpha(0.8)])
        });

    basic!(_r, "effects-fire", "effects", "fire", "Fire", "glow",
        [P::num("height", 0.9, 0.1, 6.0).unit("m").describe("Flame height (m)"), P::num("flames", 12.0, 3.0, 40.0).step(1.0).describe("Tongues of flame"), P::color("color", FIRE).describe("Flame colour"), P::color("core", [1.0, 0.8, 0.25]).describe("Inner flame colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("flames") as u32);
            let (mut outer, mut inner) = (geo::Mesh::default(), geo::Mesh::default());
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU + sh::rnd(s, i, 1);
                let d = h * 0.18 * sh::rnd(s, i, 2).sqrt();
                let t = h * sh::range(s, i, 3, 0.5, 1.0) * (1.0 - d / (h * 0.3));
                let bend = [sh::range(s, i, 4, -0.15, 0.15) * t, sh::range(s, i, 5, -0.15, 0.15) * t];
                let b = sh::blade(h * 0.16, t, bend, 4);
                geo::merge(&mut outer, &sh::place(&b, 1.0, a, 0.0, [a.cos() * d, 0.08 * h, a.sin() * d]));
                if i % 2 == 0 {
                    let c = sh::blade(h * 0.08, t * 0.55, [bend[0] * 0.5, bend[1] * 0.5], 3);
                    geo::merge(&mut inner, &sh::place(&c, 1.0, a + 0.8, 0.0, [a.cos() * d * 0.5, 0.08 * h, a.sin() * d * 0.5]));
                }
            }
            // Logs underneath, crossed.
            let mut logs = geo::Mesh::default();
            for i in 0..3u32 {
                let a = i as f32 / 3.0 * sh::PI;
                let l = h * 0.6;
                let pts = [[-a.cos() * l, h * 0.05, -a.sin() * l], [a.cos() * l, h * 0.05, a.sin() * l]];
                geo::merge(&mut logs, &sh::tube(&pts, |_| h * 0.05, 8));
            }
            Output::Parts(vec![Part::new(logs, [0.12, 0.07, 0.04]).material("ground"), Part::new(outer, v.color("color")).alpha(0.85).material("glow"), Part::new(inner, v.color("core")).material("glow")])
        });

    basic!(_r, "effects-sparks", "effects", "sparks", "Sparks", "glow",
        [P::num("count", 60.0, 5.0, 400.0).step(1.0).describe("Sparks"), P::num("reach", 1.0, 0.1, 6.0).unit("m").describe("How far they fly (m)"), P::color("color", [1.0, 0.75, 0.25]).describe("Spark colour")],
        |v: &Values, s: u32| {
            let r = v.f("reach");
            let mut m = geo::Mesh::default();
            for i in 0..v.f("count") as u32 {
                // A streak along its ballistic arc, at a seeded moment.
                let (a, e) = (sh::rnd(s, i, 1) * sh::TAU, sh::range(s, i, 2, 0.3, 1.3));
                let t = sh::range(s, i, 3, 0.2, 1.0);
                let sp = r * sh::range(s, i, 4, 0.5, 1.0);
                let pos = |t: f32| [a.cos() * e.cos() * sp * t, 0.05 + e.sin() * sp * t * 1.4 - 0.9 * r * t * t, a.sin() * e.cos() * sp * t];
                let (p0, p1) = (pos(t - 0.06), pos(t));
                if p1[1] > 0.0 {
                    geo::merge(&mut m, &sh::tube(&[p0, p1], |u| 0.006 * (0.3 + u), 3));
                }
            }
            if m.vertices.is_empty() {
                m = sh::block([0.01; 3], [0.0; 3]);
            }
            Output::Parts(vec![Part::new(m, v.color("color")).material("glow")])
        });

    basic!(_r, "effects-embers", "effects", "embers", "Embers", "glow",
        [P::num("count", 50.0, 5.0, 400.0).step(1.0).describe("Embers"), P::num("height", 1.5, 0.1, 8.0).unit("m").describe("How high they rise (m)"), P::color("color", [1.0, 0.35, 0.05]).describe("Ember colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let mut m = geo::Mesh::default();
            let mut e = sh::icosphere(0);
            sh::flat_shade(&mut e);
            for i in 0..v.f("count") as u32 {
                let t = sh::rnd(s, i, 1).powf(1.5);
                let a = sh::rnd(s, i, 2) * sh::TAU;
                let d = h * 0.25 * (0.3 + t) * sh::rnd(s, i, 3);
                // Half glow on the ground, the rest drift upwards.
                let y = if i % 2 == 0 { 0.01 } else { t * h };
                geo::merge(&mut m, &sh::place(&e, 0.012 * sh::range(s, i, 4, 0.6, 1.4), sh::rnd(s, i, 5), 0.0, [a.cos() * d, y, a.sin() * d]));
            }
            Output::Parts(vec![Part::new(m, v.color("color")).material("glow")])
        });

    basic!(_r, "effects-explosions", "effects", "explosions", "Explosions", "glow",
        [P::num("size", 3.0, 0.3, 30.0).unit("m").describe("Fireball size (m)"), P::color("color", [1.0, 0.5, 0.1]).describe("Fireball colour"), P::color("smoke", [0.18, 0.16, 0.15]).describe("Smoke colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let (mut fire, mut smoke) = (Vec::new(), Vec::new());
            let (mut rf, mut rs) = (Vec::new(), Vec::new());
            for i in 0..30u32 {
                let (a, e) = (sh::rnd(s, i, 1) * sh::TAU, sh::rnd(s, i, 2) * sh::PI * 0.5);
                let d = k * 0.35 * sh::rnd(s, i, 3).sqrt();
                let p = [a.cos() * e.cos() * d, k * 0.45 + e.sin() * d, a.sin() * e.cos() * d];
                if i < 18 {
                    fire.push(p);
                    rf.push(k * 0.18 * sh::range(s, i, 4, 0.6, 1.2));
                } else {
                    smoke.push([p[0] * 1.5, p[1] + k * 0.3, p[2] * 1.5]);
                    rs.push(k * 0.22 * sh::range(s, i, 4, 0.7, 1.2));
                }
            }
            let mut debris = geo::Mesh::default();
            for i in 0..16u32 {
                let (a, e) = (sh::rnd(s, i, 6) * sh::TAU, sh::range(s, i, 7, 0.2, 1.2));
                let d = k * sh::range(s, i, 8, 0.5, 0.9);
                let p = [a.cos() * e.cos() * d, k * 0.3 + e.sin() * d * 0.8, a.sin() * e.cos() * d];
                geo::merge(&mut debris, &sh::tube(&[[p[0] * 0.8, p[1] * 0.85, p[2] * 0.8], p], |_| k * 0.008, 3));
            }
            Output::Parts(vec![Part::new(sh::puffs(&fire, &rf, s), v.color("color")).material("glow"), Part::new(sh::puffs(&smoke, &rs, s ^ 3), v.color("smoke")).alpha(0.85), Part::new(debris, [0.1, 0.08, 0.07])])
        });

    basic!(_r, "effects-dust-clouds", "effects", "dust-clouds", "Dust clouds", "ground",
        [P::num("size", 3.0, 0.3, 30.0).unit("m").describe("Cloud width (m)"), P::color("color", [0.48, 0.38, 0.26]).describe("Dust colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let mut pts = Vec::new();
            let mut r = Vec::new();
            for i in 0..24u32 {
                let a = sh::rnd(s, i, 1) * sh::TAU;
                let d = k * 0.45 * sh::rnd(s, i, 2).sqrt();
                pts.push([a.cos() * d, k * 0.08 + k * 0.15 * sh::rnd(s, i, 3) * (1.0 - d / (k * 0.5)), a.sin() * d]);
                r.push(k * 0.13 * sh::range(s, i, 4, 0.6, 1.3));
            }
            Output::Parts(vec![Part::new(sh::puffs(&pts, &r, s), v.color("color")).alpha(0.7)])
        });

    basic!(_r, "effects-destructible-fragments", "effects", "destructible-fragments", "Destructible fragments", "stone",
        [P::num("size", 1.0, 0.2, 6.0).unit("m").describe("Size of the block before it broke (m)"), P::num("pieces", 14.0, 3.0, 60.0).step(1.0).describe("Fragments"), P::num("burst", 0.6, 0.0, 3.0).describe("How far apart they have flown"), P::color("color", [0.5, 0.48, 0.45]).describe("Material colour")],
        |v: &Values, s: u32| {
            let (k, n, burst) = (v.f("size"), v.f("pieces") as u32, v.f("burst"));
            let mut m = geo::Mesh::default();
            let side = (n as f32).cbrt().ceil().max(1.0);
            for i in 0..n {
                // Cells of the block, each a flat-cut chunk, pushed out from its middle.
                let c = [(i as f32 % side + 0.5) / side - 0.5, ((i as f32 / side).floor() % side + 0.5) / side, ((i as f32 / (side * side)).floor() % side + 0.5) / side - 0.5];
                let q = k / side * sh::range(s, i, 1, 0.75, 1.0);
                let chunk = sh::rock([q, q * 0.8, q], s.wrapping_add(i * 3), 0.6, true);
                let out = [c[0] * (1.0 + burst), c[1] * (1.0 + burst * 0.6), c[2] * (1.0 + burst)];
                geo::merge(&mut m, &sh::place(&chunk, 1.0, sh::rnd(s, i, 2) * sh::TAU, sh::rnd(s, i, 3) * burst, [out[0] * k, out[1] * k, out[2] * k]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "effects-projectile-trails", "effects", "projectile-trails", "Projectile trails", "ground",
        [P::num("length", 8.0, 1.0, 60.0).unit("m").describe("Trail length (m)"), P::num("arc", 1.5, 0.0, 10.0).unit("m").describe("Height of its arc (m)"), P::color("color", [0.85, 0.85, 0.82]).describe("Trail colour")],
        |v: &Values, s: u32| {
            let (l, arc) = (v.f("length"), v.f("arc"));
            let pts: Vec<[f32; 3]> = (0..=30).map(|j| {
                let t = j as f32 / 30.0;
                [(t - 0.5) * l, 0.4 + arc * 4.0 * t * (1.0 - t), 0.05 * l * (noise::value2(t * 5.0, 0.0, s) - 0.5)]
            }).collect();
            // A trail widening and fading behind the projectile at its head.
            let trail = sh::tube(&pts, |t| 0.02 + 0.12 * (1.0 - t), 6);
            let head = pts.last().copied().unwrap_or([0.0; 3]);
            let mut shell = sh::frustum(0.04, 0.0, 0.14, 8);
            sh::rotate(&mut shell, [0.0, 0.0, 1.0], -sh::PI * 0.5 - (arc * 4.0 / l).atan());
            let shell = sh::place(&shell, 1.0, 0.0, 0.0, head);
            Output::Parts(vec![Part::new(trail, v.color("color")).alpha(0.55), Part::new(shell, [0.2, 0.2, 0.22]).material("metal")])
        });

    basic!(_r, "effects-bullet-impacts", "effects", "bullet-impacts", "Bullet impacts", "stone",
        [P::num("size", 1.0, 0.2, 5.0).unit("m").describe("Wall panel size (m)"), P::num("hits", 5.0, 1.0, 30.0).step(1.0).describe("Impacts"), P::color("color", [0.5, 0.48, 0.45]).describe("Wall colour")],
        |v: &Values, s: u32| {
            let (k, n) = (v.f("size"), v.f("hits") as u32);
            let hits: Vec<[f32; 3]> = (0..n).map(|i| [sh::range(s, i, 1, 0.15, 0.85), sh::range(s, i, 2, 0.15, 0.85), sh::range(s, i, 3, 0.03, 0.06)]).collect();
            // An upright wall facing +z, its face cratered at each hit.
            let mut wall = geo::height_grid([k, k], 96, |x, z| {
                let (u, w) = (x / k + 0.5, z / k + 0.5);
                let mut d = 0.0f32;
                for h in &hits {
                    let r = ((u - h[0]).powi(2) + (w - h[1]).powi(2)).sqrt() / h[2];
                    d -= 0.02 * k * (1.0 - r * r).max(0.0);
                    // Radial cracks just past the crater.
                    let a = (w - h[1]).atan2(u - h[0]);
                    if r > 1.0 && r < 2.6 && (a * 5.0 + h[0] * 30.0).sin() > 0.96 {
                        d -= 0.002 * k;
                    }
                }
                0.05 * k + d
            });
            sh::rotate(&mut wall, [1.0, 0.0, 0.0], sh::PI * 0.5);
            let wall = sh::place(&wall, 1.0, 0.0, 0.0, [0.0, k * 0.5, 0.0]);
            let mut chips = geo::Mesh::default();
            for (i, h) in hits.iter().enumerate() {
                for j in 0..4u32 {
                    let c = sh::rock([0.02 * k; 3], s.wrapping_add(i as u32 * 9 + j), 0.6, true);
                    geo::merge(&mut chips, &sh::place(&c, 1.0, 0.0, 0.0, [(h[0] - 0.5) * k + sh::range(s, j, 4, -0.08, 0.08) * k, 0.0, 0.08 * k + sh::range(s, j, 5, 0.0, 0.15) * k]));
                }
            }
            Output::Parts(vec![Part::new(wall, v.color("color")), Part::new(chips, [0.4, 0.38, 0.36])])
        });

    basic!(_r, "effects-water-splashes", "effects", "water-splashes", "Water splashes", "glass",
        [P::num("size", 0.6, 0.1, 5.0).unit("m").describe("Splash radius (m)"), P::num("droplets", 24.0, 4.0, 120.0).step(1.0).describe("Droplets"), P::color("color", [0.6, 0.75, 0.82]).describe("Water colour")],
        |v: &Values, s: u32| {
            let (k, n) = (v.f("size"), v.f("droplets") as u32);
            // A crown: an open lathed rim rising round the impact, droplets off its edge.
            let prof: Vec<[f32; 2]> = (0..=6).map(|j| { let t = j as f32 / 6.0; [k * (0.35 + 0.3 * t), k * 0.5 * t.powf(0.7)] }).collect();
            let mut crown = geo::lathe(&prof, 24);
            sh::double_sided(&mut crown);
            let pool = sh::disc(k * 1.2, 0.01, 24, true);
            let mut drops = geo::Mesh::default();
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU + sh::rnd(s, i, 1) * 0.3;
                let d = k * sh::range(s, i, 2, 0.6, 1.1);
                let y = k * sh::range(s, i, 3, 0.45, 0.9);
                geo::merge(&mut drops, &sh::blob([k * 0.03; 3], [a.cos() * d, y, a.sin() * d], 0.0, s, 1));
            }
            Output::Parts(vec![Part::new(pool, [0.08, 0.2, 0.26]).alpha(0.8), Part::new(crown, v.color("color")).alpha(0.7), Part::new(drops, v.color("color")).alpha(0.85)])
        });

    basic!(_r, "effects-bubbles", "effects", "bubbles", "Bubbles", "glass",
        [P::num("count", 30.0, 3.0, 200.0).step(1.0).describe("Bubbles"), P::num("height", 1.5, 0.2, 10.0).unit("m").describe("How high they rise (m)"), P::color("color", [0.8, 0.9, 0.95]).describe("Bubble colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let mut m = geo::Mesh::default();
            let ball = sh::icosphere(2);
            for i in 0..v.f("count") as u32 {
                let t = sh::rnd(s, i, 1);
                // Bigger as they rise, wobbling side to side.
                let r = h * 0.015 * (0.5 + t) * sh::range(s, i, 2, 0.7, 1.3);
                let x = h * 0.08 * (t * 9.0 + i as f32).sin() + sh::range(s, i, 3, -0.1, 0.1) * h;
                geo::merge(&mut m, &sh::place(&ball, r, 0.0, 0.0, [x, r + t * h, sh::range(s, i, 4, -0.1, 0.1) * h]));
            }
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.55)])
        });

    basic!(_r, "effects-water-droplets", "effects", "water-droplets", "Water droplets", "glass",
        [P::num("size", 0.6, 0.1, 4.0).unit("m").describe("Surface width (m)"), P::num("beads", 40.0, 2.0, 200.0).step(1.0).describe("Beads on the surface"), P::color("color", [0.7, 0.82, 0.9]).describe("Water colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let plate = sh::block([k, 0.02, k], [0.0, 0.0, 0.0]);
            let mut beads = geo::Mesh::default();
            for (i, p) in sh::scatter_rect(v.f("beads") as u32, [k * 0.46; 2], s).into_iter().enumerate() {
                let r = k * 0.018 * sh::range(s, i as u32, 1, 0.5, 1.6);
                let mut b = sh::blob([r, r * 0.55, r], [p[0], 0.02, p[1]], 0.05, s, 2);
                for vx in &mut b.vertices {
                    vx[1] = vx[1].max(0.02);
                }
                geo::merge(&mut beads, &b);
            }
            // And a falling teardrop above the surface.
            let prof: Vec<[f32; 2]> = (0..=10).map(|j| { let t = j as f32 / 10.0; [k * 0.04 * ((t * sh::PI).sin() * (1.0 - t).powf(0.6)) + 0.0005, k * 0.15 * t] }).collect();
            let drop = sh::place(&geo::lathe(&prof, 16), 1.0, 0.0, 0.0, [0.0, k * 0.3, 0.0]);
            geo::merge(&mut beads, &drop);
            Output::Parts(vec![Part::new(plate, [0.3, 0.3, 0.32]).material("ground"), Part::new(beads, v.color("color")).alpha(0.85)])
        });

    basic!(_r, "effects-steam", "effects", "steam", "Steam", "ground",
        [P::num("height", 1.2, 0.2, 10.0).unit("m").describe("Height the steam rises (m)"), P::color("color", [0.9, 0.91, 0.93]).describe("Steam colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let pot = sh::frustum(h * 0.12, h * 0.14, h * 0.14, 20);
            let mut col = column(18, h, h * 0.03, h * 0.12, h * 0.15, s);
            geo::transform(&mut col, [1.0; 3], [0.0, h * 0.14, 0.0]);
            Output::Parts(vec![Part::new(pot, [0.15, 0.15, 0.16]).material("metal"), Part::new(col, v.color("color")).alpha(0.5)])
        });

    basic!(_r, "effects-magical-particles", "effects", "magical-particles", "Magical particles", "glow",
        [P::num("height", 2.0, 0.2, 10.0).unit("m").describe("Height of the spiral (m)"), P::num("count", 70.0, 5.0, 400.0).step(1.0).describe("Particles"), P::color("color", [0.5, 0.3, 1.0]).describe("Particle colour"), P::color("accent", [0.2, 0.9, 1.0]).describe("Second colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("count") as u32);
            let mut gem = sh::icosphere(0);
            sh::flat_shade(&mut gem);
            let (mut a_m, mut b_m) = (geo::Mesh::default(), geo::Mesh::default());
            for i in 0..n {
                // Two spirals winding up round each other.
                let t = i as f32 / n as f32;
                let a = t * sh::TAU * 3.0 + if i % 2 == 0 { 0.0 } else { sh::PI };
                let r = h * 0.25 * (1.0 - 0.6 * t) + sh::range(s, i, 1, -0.03, 0.03) * h;
                let p = [a.cos() * r, 0.05 + t * h, a.sin() * r];
                let g = sh::place(&gem, h * 0.012 * sh::range(s, i, 2, 0.6, 1.4), a, t * 3.0, p);
                geo::merge(if i % 2 == 0 { &mut a_m } else { &mut b_m }, &g);
            }
            Output::Parts(vec![Part::new(a_m, v.color("color")).material("glow"), Part::new(b_m, v.color("accent")).material("glow")])
        });

    basic!(_r, "effects-footprints-and-ground-decals", "effects", "footprints-and-ground-decals", "Footprints and ground decals", "ground",
        [P::num("steps", 8.0, 1.0, 40.0).step(1.0).describe("Footprints"), P::num("stride", 0.7, 0.2, 2.0).unit("m").describe("Stride length (m)"), P::color("color", [0.6, 0.5, 0.34]).describe("Ground colour")],
        |v: &Values, s: u32| {
            let (n, st) = (v.f("steps") as u32, v.f("stride"));
            let l = n as f32 * st * 0.5 + 1.0;
            // A trail of prints pressed into soft ground, left and right.
            let prints: Vec<[f32; 2]> = (0..n).map(|i| [(i as f32 - (n as f32 - 1.0) * 0.5) * st * 0.5, if i % 2 == 0 { 0.09 } else { -0.09 }]).collect();
            let mut m = geo::height_grid([l, 1.2], 160, |x, z| {
                let mut h = 0.05 + 0.004 * noise::fbm2(x * 6.0, z * 6.0, s, 3, 2.0, 0.5);
                for p in &prints {
                    let (dx, dz) = ((x - p[0]) / 0.13, (z - p[1]) / 0.05);
                    let sole = (1.0 - dx * dx - dz * dz).max(0.0);
                    h -= 0.025 * sole.sqrt();
                }
                h
            });
            sh::to_ground(&mut m);
            Output::Mesh(m)
        });
}
