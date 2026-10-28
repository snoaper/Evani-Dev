//! The kart racer adapter: runs the game natively (no browser) and records
//! what it draws, then measures its collision against the drawn walls.
//!
//! - The run: RenderModule, the kart kit and the toon look at a 1280 x 720
//!   viewport. The title screen's attract race (orbit camera) first, then a
//!   one-player race on the autopilot (chase camera). A snapshot system,
//!   last in the Frame stage, copies the frame's draw list and camera every
//!   `every` frames.
//! - Collision: the kit stops a kart's centre at `half_width + verge_width -
//!   kart_radius` from the centreline (kart.rs, "walls"), so the kart's
//!   visible side stops that far plus its drawn half-width. Probes every 2 m
//!   on both sides compare that with the drawn wall, and a bot drives one lap
//!   hugging each wall through the kit's own `kart::step`.
//!
//! Only kit APIs that every kart-racer version has are used here: the track,
//! the tune, `kart::step`, `Race::request` and `Race::autopilot`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::App;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::formulas::Formulas;
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_geocheck::checks::{BotSample, Snapshot, WallProbe};
use evani_geocheck::geom::{V3, add, dot, len, mul, norm, sub};
use evani_geocheck::image::Camera;
use evani_geocheck::scene::{Kind, Scene};
use evani_kit_kart_racer::kart::{self, Controls, Kart};
use evani_kit_kart_racer::race::{Phase, Race, Request};
use evani_kit_kart_racer::track::Track;
use evani_kit_kart_racer::{KartKit, KartRacerPlugin, TRACK_JSON, Tune};
use evani_look_toon::ToonLook;
use evani_render::RenderModule;
use evani_kit_kart_racer::draw::DrawState;
use evani_render::mesh::{MeshId, MeshLibrary};
use evani_render::world::RenderWorld;

pub const VIEWPORT: [f32; 2] = [1280.0, 720.0];
pub const ASPECT: f64 = 1280.0 / 720.0;
/// The kit's still-world switch: on, the race holds and only the static
/// world is drawn (no karts, items or their effects), for the gap, overlap
/// and flicker captures.
pub const STILL_PARAM: &str = "kit.kart_racer.static_world";
const DT_MS: f64 = 1000.0 / 60.0;

/// What the native run recorded.
pub struct Recording {
    pub frames: Vec<Snapshot>,
    /// "title" or "race" per frame.
    pub phases: Vec<&'static str>,
    /// Look material names by index (labels).
    pub materials: Vec<String>,
    /// The tier the run drew with (the render phase uses the same).
    pub tier: String,
    pub tune: Tune,
    pub track: Track,
    pub steer_curve: Box<dyn Fn(f32) -> f32>,
    pub gravity: f32,
    /// The app's custom meshes at the end of the run (registered with the
    /// checker's `Shapes`; empty when the game has none).
    pub library: MeshLibrary,
    /// The library changed while the run was recorded.
    pub library_changed: bool,
    /// Library meshes whose open edges are intended: the item box's decor,
    /// a flat sheet of paint (the "?" and rivets) that stands off the box.
    pub open_meshes: Vec<MeshId>,
}

/// Run the game natively and record the draw lists.
pub fn record(title_s: f64, race_s: f64, every: u32) -> Result<Recording, String> {
    let mut app = App::new("kart-racer")
        .with_module(RenderModule::new())
        .with_kit(KartRacerPlugin)
        .with_look(ToonLook);
    let store: Rc<RefCell<Vec<Snapshot>>> = Rc::new(RefCell::new(Vec::new()));
    let st = store.clone();
    // The mesh library's revision at the first snapshot; a later change means
    // custom meshes were edited mid-run and only the final library is checked.
    let first_rev: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
    let changed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let (first_rev_w, changed_w) = (first_rev.clone(), changed.clone());
    let mut n: u32 = 0;
    {
        let mut ctx = app.context("geocheck");
        ctx.system(
            SystemDesc::new("geocheck.viewport", Stage::Frame, |w: &mut World| {
                if let Some(mut rw) = w.try_res_mut::<RenderWorld>() {
                    rw.viewport_css = VIEWPORT;
                }
            })
            .order(i32::MIN),
        );
        ctx.system(
            SystemDesc::new("geocheck.snapshot", Stage::Frame, move |w: &mut World| {
                n += 1;
                if !n.is_multiple_of(every.max(1)) {
                    return;
                }
                let time = w.try_res::<Time>().map_or(0.0, |t| t.render_seconds());
                let Some(rw) = w.try_res::<RenderWorld>() else { return };
                let mut meshes = rw.meshes.clone();
                let decal_from = meshes.len();
                meshes.extend_from_slice(&rw.decals);
                let camera = Camera::from_pose(&rw.camera, ASPECT);
                if let Some(lib) = w.try_res::<MeshLibrary>() {
                    let r = lib.revision();
                    match first_rev_w.get() {
                        None => first_rev_w.set(Some(r)),
                        Some(f) if f != r => changed_w.set(true),
                        _ => {}
                    }
                }
                st.borrow_mut().push(Snapshot { time, meshes, decal_from, camera });
            })
            .order(i32::MAX),
        );
    }
    let phases_store: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let run = |app: &mut App, seconds: f64, phase: &'static str| {
        let before = store.borrow().len();
        app.run_headless((seconds * 60.0).round() as u32, DT_MS);
        let after = store.borrow().len();
        phases_store.borrow_mut().extend(std::iter::repeat_n(phase, after - before));
    };
    run(&mut app, title_s, "title");
    if race_s > 0.0 {
        app.world
            .try_res_mut::<Race>()
            .ok_or("the kart kit has no Race resource")?
            .request(Request::Start { players: 1 });
        // The request resets the race on the next tick, which clears the
        // autopilot: set it after that tick.
        run(&mut app, 2.0 / 60.0, "race");
        if let Some(mut r) = app.world.try_res_mut::<Race>() {
            r.autopilot = true;
        }
        run(&mut app, race_s, "race");
        let phase = app.world.try_res::<Race>().map(|r| r.phase);
        if phase == Some(Phase::Title) {
            return Err("the one-player race did not start (still on the title screen)".into());
        }
    }
    let materials = app
        .world
        .try_res::<LookData>()
        .map(|l| l.materials.iter().map(|m| m.name.clone()).collect())
        .unwrap_or_default();
    let tier = app.world.try_res::<Tiers>().map(|t| t.active().id.clone()).unwrap_or_default();
    let kit = *app.world.try_res::<KartKit>().ok_or("the kart kit is not loaded")?;
    let (tune, gravity) = {
        let reg = app.world.res::<ParamRegistry>();
        let t = kit.tune.read(&reg);
        let g = kit.gravity(&reg, &t);
        (t, g)
    };
    // The steering-by-speed formula as a table (0 to 1.5 of top speed).
    let table: Vec<f32> = {
        let f = app.world.res::<Formulas>();
        (0..=150).map(|i| f.eval(kit.steer_by_speed, &[i as f32 / 100.0])).collect()
    };
    let track = Track::from_json(TRACK_JSON)?;
    let frames = std::mem::take(&mut *store.borrow_mut());
    let phases = phases_store.borrow().clone();
    let library = app.world.try_res::<MeshLibrary>().map(|l| (*l).clone()).unwrap_or_default();
    let open_meshes: Vec<MeshId> = app.world.try_res::<DrawState>().map(|d| d.open_meshes()).unwrap_or_default();
    Ok(Recording {
        frames,
        phases,
        materials,
        tier,
        tune,
        track,
        steer_curve: Box::new(move |x| {
            let p = (x.clamp(0.0, 1.5) * 100.0).min(149.999);
            let i = p as usize;
            table[i] + (table[i + 1] - table[i]) * (p - i as f32)
        }),
        gravity,
        library,
        library_changed: changed.get(),
        open_meshes,
    })
}

/// The kart's role for the character-in-vehicle test: the helmet (a sphere
/// in the body material) is the character. The vehicle is the hard shell
/// the driver must not pass through: the tyres, seat and column (wheel
/// material) and the wide, flat body and trim pieces (tub, nose, wing,
/// bumper, engine: x at least 0.95, y at most 0.45). The suit, arms and
/// visor sit against the helmet by design and are neither. Anything longer
/// than 3 m, or as chunky as an item box, is not part of a kart.
pub fn role(materials: &[String]) -> impl Fn(&evani_render::world::MeshInstance) -> evani_geocheck::checks::Role + '_ {
    use evani_geocheck::checks::Role;
    use evani_render::mesh::MeshId;
    move |m| {
        let name = materials.get(m.material as usize).map_or("", String::as_str);
        let big = m.scale.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        let small = m.scale.iter().fold(f32::INFINITY, |a, b| a.min(b.abs()));
        if big > 3.0 || small >= 1.5 {
            return Role::Other;
        }
        let flat_wide = m.scale[0].abs() >= 0.95 && m.scale[1].abs() <= 0.45;
        match name {
            "kart_body" if m.mesh == MeshId::SPHERE => Role::Character,
            "wheel" => Role::Vehicle,
            "kart_body" | "kerb_white" if flat_wide => Role::Vehicle,
            _ => Role::Other,
        }
    }
}

fn h(p: [f32; 3]) -> V3 {
    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
}

/// The kit's collision limit for a kart's centre (m from the centreline).
pub fn limit(track: &Track, t: &Tune) -> f64 {
    f64::from((track.half_width + t.verge_width - t.kart_radius).max(track.half_width))
}

/// Unit left direction (horizontal) of the track at `s`.
fn left_at(track: &Track, s: f32) -> V3 {
    let a = h(track.point_at(s, 0.0));
    let b = h(track.point_at(s, 1.0));
    let d = sub(b, a);
    norm([d[0], 0.0, d[2]])
}

/// The karts' drawn half-width: in the race's first frame (karts parked on
/// the grid, square to the track), tyres (`wheel` material) are grouped
/// into karts and every dynamic part within 1.8 m of a kart's centre is
/// measured across the track. `None` when no kart was found.
pub fn visible_half_width(dy: &Scene, track: &Track, materials: &[String]) -> Option<f64> {
    let wheels: Vec<usize> = (0..dy.items.len())
        .filter(|&i| dy.items[i].kind != Kind::Decal && materials.get(dy.items[i].inst.material as usize).is_some_and(|m| m == "wheel"))
        .collect();
    // Single-link clusters of tyre centres within 2 m.
    let mut cluster: Vec<usize> = (0..wheels.len()).collect();
    let find = |c: &mut Vec<usize>, mut i: usize| {
        while c[i] != i {
            c[i] = c[c[i]];
            i = c[i];
        }
        i
    };
    for a in 0..wheels.len() {
        for b in a + 1..wheels.len() {
            let pa = dy.items[wheels[a]].shape.aabb.center();
            let pb = dy.items[wheels[b]].shape.aabb.center();
            if len(sub(pa, pb)) < 2.0 {
                let (ra, rb) = (find(&mut cluster, a), find(&mut cluster, b));
                cluster[ra] = rb;
            }
        }
    }
    let roots: Vec<usize> = (0..wheels.len()).map(|i| find(&mut cluster, i)).collect();
    let mut best: Option<f64> = None;
    for root in 0..wheels.len() {
        let members: Vec<usize> = (0..wheels.len()).filter(|&i| roots[i] == root).collect();
        if members.len() < 4 {
            continue;
        }
        let centre = mul(
            members.iter().fold([0.0; 3], |acc, &i| add(acc, dy.items[wheels[i]].shape.aabb.center())),
            1.0 / members.len() as f64,
        );
        let loc = track.locate([centre[0] as f32, centre[1] as f32, centre[2] as f32], 0);
        let left = left_at(track, loc.s);
        let mut w: f64 = 0.0;
        for it in &dy.items {
            let c = it.shape.aabb.center();
            if it.kind == Kind::Decal || len(sub([c[0], centre[1], c[2]], centre)) > 1.8 {
                continue;
            }
            for f in &it.shape.faces {
                for p in &f.pts {
                    w = w.max(dot(sub(*p, centre), left).abs());
                }
            }
        }
        best = Some(best.map_or(w, |b| b.max(w)));
    }
    best
}

/// Sideways probes every `step` m on both sides at kart-body height.
pub fn probes(track: &Track, t: &Tune, half_width: f64, step: f32) -> Vec<WallProbe> {
    let stop = limit(track, t) + half_width;
    let reach = f64::from(track.half_width + t.verge_width) + 6.0;
    let mut out = Vec::new();
    let mut s = 0.0f32;
    while s < track.length {
        let o = add(h(track.point_at(s, 0.0)), [0.0, 0.5, 0.0]);
        let l = left_at(track, s);
        for (side, sign) in [("left", 1.0), ("right", -1.0)] {
            out.push(WallProbe {
                origin: o,
                dir: mul(l, sign),
                reach,
                collision: Some(stop),
                side: side.into(),
                station: f64::from(s),
            });
        }
        s += step;
    }
    out
}

/// A bot that drives one lap pressed against each wall (steering towards a
/// line 3 m past the collision limit) through the kit's `kart::step`.
pub fn wall_bot(rec: &Recording, half_width: f64) -> Vec<BotSample> {
    let track = &rec.track;
    let t = &rec.tune;
    let lim = limit(track, t) as f32;
    let dt = 1.0 / 60.0;
    let mut out = Vec::new();
    for sign in [1.0f32, -1.0] {
        let s0 = 0.0f32;
        let p = track.point_at(s0, sign * (lim - 1.0));
        let tan = track.tangent_at(s0);
        let mut k = Kart::new(0, None, p, tan[0].atan2(tan[1]));
        let mut travelled = 0.0f32;
        let mut prev = k.pos;
        let mut time = 0.0f64;
        let max_steps = (track.length / 5.0 / dt).ceil() as u32 + 600;
        for step in 0..max_steps {
            let loc = track.locate(k.pos, k.hint);
            k.hint = loc.idx;
            let target = sign * (lim + 3.0);
            let err = target - loc.lateral;
            let along = loc.tangent[0].atan2(loc.tangent[1]);
            let want = along + (0.15 * err).atan();
            let steer = (-2.0 * kart::wrap_angle(want - k.yaw)).clamp(-1.0, 1.0);
            let c = Controls {
                steer,
                throttle: 1.0,
                ..Default::default()
            };
            kart::step(&mut k, c, t, &*rec.steer_curve, track, rec.gravity, dt);
            time += f64::from(dt);
            let after = track.locate(k.pos, k.hint);
            let blocked = after.lateral.abs() >= lim - 0.02;
            travelled += ((k.pos[0] - prev[0]).powi(2) + (k.pos[2] - prev[2]).powi(2)).sqrt();
            prev = k.pos;
            if step % 6 == 0 {
                out.push(BotSample {
                    time,
                    pos: add(h(k.pos), [0.0, 0.5, 0.0]),
                    radius: half_width,
                    blocked,
                });
            }
            if travelled >= track.length {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_geocheck::checks::Role;
    use evani_render::world::MeshInstance;

    #[test]
    fn helmet_is_the_driver_and_only_the_hard_shell_is_the_kart() {
        let mats: Vec<String> = ["kart_body", "wheel", "kerb_white"].iter().map(|s| s.to_string()).collect();
        let part = |mesh, material: u32, s: [f32; 3]| {
            let mut m = MeshInstance::new(mesh, [0.0; 3]);
            m.material = material;
            m.scale = s;
            m
        };
        let r = role(&mats);
        assert_eq!(r(&part(MeshId::SPHERE, 0, [0.84, 0.8, 0.88])), Role::Character); // helmet
        assert_eq!(r(&part(MeshId::BEVEL_CUBE, 0, [1.3, 0.3, 2.3])), Role::Vehicle); // tub
        assert_eq!(r(&part(MeshId::BEVEL_CUBE, 2, [1.6, 0.22, 0.28])), Role::Vehicle); // bumper
        assert_eq!(r(&part(MeshId::CYLINDER, 1, [0.66, 0.64, 0.66])), Role::Vehicle); // tyre
        assert_eq!(r(&part(MeshId::BEVEL_CUBE, 0, [0.68, 0.64, 0.46])), Role::Other); // suit
        assert_eq!(r(&part(MeshId::BEVEL_CUBE, 0, [0.17, 0.17, 0.62])), Role::Other); // arm
        assert_eq!(r(&part(MeshId::BEVEL_CUBE, 0, [0.64, 0.25, 0.26])), Role::Other); // visor
        assert_eq!(r(&part(MeshId::BEVEL_CUBE, 1, [2.0, 2.0, 2.0])), Role::Other); // too big for a kart
    }

    /// The item box's meshes pass mesh validation: the body is closed, the
    /// decor is a flat sheet (open on purpose) whose pieces do not overlap.
    #[test]
    fn item_box_meshes_validate() {
        use evani_geocheck::meshcheck::validate;
        use evani_kit_kart_racer::itembox::{BoxShape, body, decor};
        for shape in [
            BoxShape { bevel: 0.12, glyph_size: 1.0, rivet: 0.045 },
            BoxShape { bevel: 0.02, glyph_size: 1.15, rivet: 0.08 },
            BoxShape { bevel: 0.2, glyph_size: 0.5, rivet: 0.0 },
        ] {
            let b = validate(&body(&shape), false);
            assert!(b.problems().is_empty(), "{shape:?} body: {:?}", b.problems());
            let d = validate(&decor(&shape), true);
            assert!(d.problems().is_empty(), "{shape:?} decor: {:?}", d.problems());
        }
    }
}
