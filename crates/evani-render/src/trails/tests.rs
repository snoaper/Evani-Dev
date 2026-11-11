use super::*;
use evani_core::App;
use evani_core::params::{ParamDef, ParamValue};

const DT: f32 = 1.0 / 60.0;

fn validate(src: &str) -> naga::Module {
    let m = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&m)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v);
    if l < 1e-12 { [0.0; 3] } else { scale(v, 1.0 / l.sqrt()) }
}
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(scale(a, 1.0 - t), scale(b, t))
}

fn lut(sim: &SimUniform, curve: usize, u: f32) -> f32 {
    let x = u.clamp(0.0, 1.0) * 15.0;
    let i = (x as usize).min(14);
    mix(sim.luts[curve + i], sim.luts[curve + i + 1], x - i as f32)
}

/// The CPU reference of `trails_sim.wgsl`: emitter `t`, `k` points behind the
/// newest (the point buffer as the GPU holds it).
fn eval(sim: &SimUniform, h: &Head, points: &[Point], t: u32, k: u32) -> Vert {
    let mut o = Vert { key: KEY_NONE, ..Vert::default() };
    if k >= h.count {
        return o;
    }
    let at = (t * POINTS) as usize;
    let ring = |k: u32| points[at + ((h.head + POINTS - k) % POINTS) as usize];
    let p = ring(k);
    let has_older = k + 1 < h.count && p.width >= 0.0;
    let mut has_newer = false;
    let mut to_newer = [0.0; 3];
    if k > 0 {
        let n = ring(k - 1);
        has_newer = n.width >= 0.0;
        if has_newer {
            to_newer = unit(sub(p.pos, n.pos));
        }
    }
    let to_older = if has_older { unit(sub(ring(k + 1).pos, p.pos)) } else { [0.0; 3] };
    let mut tan = unit(add(to_newer, to_older));
    if dot(tan, tan) == 0.0 {
        tan = to_older;
    }
    let ticks = (sim.now.wrapping_sub(p.tick) as f32 + sim.frac - 1.0).max(0.0);
    let u = ticks * sim.dt / h.life.max(1e-4);
    let mut pos = p.pos;
    let mut s = p.width.abs() * sim.width * lut(sim, 0, u);
    if k == 0 {
        pos = mix3(h.prev, p.pos, ticks.clamp(0.0, 1.0));
        s *= (2.0 - ticks).clamp(0.0, 1.0);
    } else if !has_newer {
        s = 0.0;
    }
    if !has_older {
        s = 0.0;
    }
    let a = lut(sim, LUT, u) * ((1.0 - u) / sim.fade.max(1e-3)).clamp(0.0, 1.0);
    let c: [f32; 4] = core::array::from_fn(|i| mix(h.color[i], h.color_end[i], u.clamp(0.0, 1.0)));
    o.pos = pos;
    o.width = h.width * s;
    o.tan = tan;
    o.edge = scale(p.edge, s);
    let alpha = if h.key & KEY_ADDITIVE != 0 { 0.0 } else { c[3] * a };
    o.color = [c[0] * c[3] * a, c[1] * c[3] * a, c[2] * c[3] * a, alpha];
    if has_older && u < 1.0 {
        o.key = h.key & !KEY_ADDITIVE;
    }
    o
}

/// The curve of `trails_draw.wgsl` between two aged points at `s`: position
/// and direction.
fn curve(a: &Vert, b: &Vert, s: f32) -> ([f32; 3], [f32; 3]) {
    let l = dot(sub(b.pos, a.pos), sub(b.pos, a.pos)).sqrt();
    let (m0, m1) = (scale(a.tan, l), scale(b.tan, l));
    let (s2, s3) = (s * s, s * s * s);
    let pos = add(
        add(scale(a.pos, 2.0 * s3 - 3.0 * s2 + 1.0), scale(m0, s3 - 2.0 * s2 + s)),
        add(scale(b.pos, 3.0 * s2 - 2.0 * s3), scale(m1, s3 - s2)),
    );
    let dir = add(
        add(scale(sub(a.pos, b.pos), 6.0 * s2 - 6.0 * s), scale(m0, 3.0 * s2 - 4.0 * s + 1.0)),
        scale(m1, 3.0 * s2 - 2.0 * s),
    );
    (pos, dir)
}

/// The two corners of the strip at `s` in `trails_draw.wgsl` `vs_world`.
fn corners(a: &Vert, b: &Vert, s: f32, eye: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let (pos, dir) = curve(a, b, s);
    let dir = if s == 0.0 {
        a.tan
    } else if s == 1.0 {
        b.tan
    } else {
        dir
    };
    let w = mix(a.width, b.width, s);
    let edge = mix3(a.edge, b.edge, s);
    let side = if dot(edge, edge) == 0.0 {
        let r = cross(dir, sub(eye, pos));
        let r = if dot(r, r) > 1e-12 { scale(r, 1.0 / dot(r, r).sqrt()) } else { r };
        scale(r, w * 0.5)
    } else {
        edge
    };
    (sub(pos, side), add(pos, side))
}

/// A system fed `n` ticks of `path` for emitter 1; its point buffer as the GPU
/// would hold it.
struct Fed {
    sys: TrailSystem,
    points: Vec<Point>,
}

impl Fed {
    fn new(cap: u32) -> Self {
        let mut sys = TrailSystem::new();
        sys.set_cap(cap);
        sys.sim.luts = [1.0; LUT * 2];
        sys.sim.fade = 0.4;
        Self { sys, points: vec![Point::default(); (MAX_TRAILS * POINTS) as usize] }
    }

    fn op(&mut self, op: TrailOp) {
        self.sys.staged.clear();
        self.sys.apply(&op, DT);
        for (at, p) in &self.sys.staged {
            self.points[*at as usize] = *p;
        }
    }

    fn point(&mut self, id: u32, tick: u32, pos: [f32; 3], edge: [f32; 3]) {
        self.op(TrailOp::Point { id, tick: u64::from(tick), pos, edge, width: 1.0 });
    }

    /// The aged points of emitter slot `t`, newest first, at this render time.
    fn verts(&mut self, t: u32, now: u32, frac: f32) -> Vec<Vert> {
        self.sys.advance(now, frac, DT);
        let h = self.sys.heads()[t as usize];
        let sim = self.sys.sim_uniform();
        (0..POINTS).map(|k| eval(&sim, &h, &self.points, t, k)).collect()
    }
}

fn style(life: f32) -> TrailStyle {
    TrailStyle::new([1.0, 0.5, 0.25, 1.0], 0.5, life, Space::World3d)
}

/// A blade tip on an arc: `rate` radians per tick, radius `r`.
fn arc(tick: u32, rate: f32, r: f32) -> [f32; 3] {
    let a = tick as f32 * rate;
    [r * a.cos(), 1.0, r * a.sin()]
}

#[test]
fn layouts_match_the_wgsl() {
    assert_eq!(size_of::<Point>(), 32);
    assert_eq!(size_of::<Head>(), 64);
    assert_eq!(size_of::<Vert>(), 64);
    assert_eq!(size_of::<SimUniform>(), 32 + 4 * 2 * LUT);
    assert_eq!(DRAW_WORDS * 4, 96);
    // One module: with and without the screen entry point, as builds join it.
    for (parts, eps) in [
        (&[SIM_WGSL, DRAW_WGSL, SCREEN_WGSL][..], &["cs_main", "vs_world", "vs_screen", "fs_main"][..]),
        (&[SIM_WGSL, DRAW_WGSL][..], &["cs_main", "vs_world", "fs_main"][..]),
    ] {
        let m = validate(&parts.concat());
        for ep in eps {
            assert!(m.entry_points.iter().any(|e| e.name == *ep), "{ep}");
        }
    }
    assert!(SIM_WGSL.starts_with("// evani.shader.trails_sim"));
    assert!(DRAW_WGSL.starts_with("// evani.shader.trails_draw"));
    assert!(SCREEN_WGSL.starts_with("// evani.shader.trails_screen"));
    assert_eq!(SCREEN, cfg!(any(not(feature = "forward"), feature = "trails-screen")));
    assert!(SIM_WGSL.contains(&format!("const POINTS:u32={POINTS}u;")));
    assert!(SIM_WGSL.contains("@workgroup_size(64)"), "one workgroup per emitter's ring");
    assert!(![SIM_WGSL, DRAW_WGSL, SCREEN_WGSL].iter().any(|w| w[32..].contains("//")), "no comments ship");
    assert_eq!(SIM_WGSL.matches('\n').count(), 1, "the marker line, then one line");
    assert_eq!(layer_of(key_of(Space::Screen2d { layer: -7 })), Some(-7));
    assert_eq!(layer_of(KEY_WORLD), None);
    assert!(key_of(Space::Screen2d { layer: 2 }) > key_of(Space::Screen2d { layer: 1 }));
}

/// Two segments meeting at a point use the same corners there, at every
/// speed, from any camera, so no gap can ever open along a strip.
#[test]
fn neighbouring_segments_share_their_corners_at_speed() {
    for (rate, r) in [(0.02, 2.0), (0.25, 1.6), (0.6, 1.6), (1.5, 3.0)] {
        for edge in [[0.0; 3], [0.0, 0.4, 0.0]] {
            let mut f = Fed::new(4);
            f.op(TrailOp::Style { id: 1, style: style(0.9) });
            for tick in 0..50 {
                f.point(1, tick, arc(tick, rate, r), edge);
            }
            for frac in [0.0, 0.3, 0.999] {
                let v = f.verts(0, 50, frac);
                for eye in [[0.0, 3.0, 8.0], [5.0, 0.2, -3.0], [0.3, 12.0, 0.1]] {
                    for k in 0..48 {
                        if v[k].key == KEY_NONE || v[k + 1].key == KEY_NONE {
                            continue;
                        }
                        let (a0, a1) = corners(&v[k], &v[k + 1], 1.0, eye);
                        let (b0, b1) = corners(&v[k + 1], &v[k + 2], 0.0, eye);
                        assert_eq!((a0, a1), (b0, b1), "rate {rate} k {k} eye {eye:?}: a gap");
                    }
                }
            }
        }
    }
}

/// The curve rounds what a fast blade cuts between two ticks: at 0.25 radians
/// a tick the plain points turn 14 degrees at each joint, the High curve at
/// most 5. It stays on the arc too.
#[test]
fn the_curve_rounds_a_fast_blade_without_overshoot() {
    let (rate, r) = (0.25, 1.6);
    let mut f = Fed::new(1);
    f.op(TrailOp::Style { id: 1, style: style(0.8) });
    for tick in 0..30 {
        f.point(1, tick, arc(tick, rate, r), [0.0; 3]);
    }
    let v = f.verts(0, 30, 0.5);
    let steps = tier_steps("high");
    let mut pts = Vec::new();
    for k in 2..20 {
        for q in 0..steps {
            pts.push(curve(&v[k], &v[k + 1], q as f32 / steps as f32).0);
        }
    }
    let turn = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| dot(unit(sub(b, a)), unit(sub(c, b))).clamp(-1.0, 1.0).acos().to_degrees();
    let worst = pts.windows(3).map(|w| turn(w[0], w[1], w[2])).fold(0.0, f32::max);
    assert!(worst < 5.0, "the curve turns {worst} degrees at a joint");
    let plain = turn(v[3].pos, v[4].pos, v[5].pos);
    assert!(plain > 13.0, "plain points turn {plain} degrees");
    for p in &pts {
        let off = ((p[0] * p[0] + p[2] * p[2]).sqrt() - r).abs();
        assert!(off < 0.01, "{off} m off the arc");
    }
}

#[test]
fn life_taper_fade_and_colour_follow_the_age() {
    let mut f = Fed::new(1);
    let mut st = style(1.0);
    st.color_end = [0.0, 0.0, 1.0, 0.5];
    f.op(TrailOp::Style { id: 1, style: st });
    f.sys.sim.luts[..LUT].copy_from_slice(&core::array::from_fn::<f32, LUT, _>(|i| 1.0 - i as f32 / 15.0));
    for tick in 0..50 {
        f.point(1, tick, [tick as f32 * 0.1, 0.0, 0.0], [0.0; 3]);
    }
    let v = f.verts(0, 50, 0.0);
    // Newest is one tick behind, on the point before it.
    assert_eq!(v[0].pos, [4.8, 0.0, 0.0]);
    assert_eq!(v[0].key, KEY_WORLD);
    // Half way through the life: half the width, half way along the colours.
    let mid = &v[30];
    assert!((mid.width - 0.25).abs() < 0.01, "{}", mid.width);
    let a = ((1.0f32 - 0.5) / 0.4).clamp(0.0, 1.0);
    assert!((mid.color[3] - 0.75 * a).abs() < 0.01, "{:?}", mid.color);
    assert!((mid.color[2] - (0.5 * 0.75 + 0.125 * 0.0) * a).abs() < 0.2);
    // Fades only in its last part.
    let late = &v[48];
    assert!(late.color[3] < mid.color[3] && late.color[3] > 0.0);
    assert!(late.width < mid.width);
    // Additive: colour adds, alpha covers nothing.
    let mut f = Fed::new(1);
    let mut st = style(1.0);
    st.additive = true;
    f.op(TrailOp::Style { id: 1, style: st });
    f.point(1, 0, [0.0; 3], [0.0; 3]);
    f.point(1, 1, [1.0, 0.0, 0.0], [0.0; 3]);
    let v = f.verts(0, 3, 0.0);
    assert_eq!(v[0].color[3], 0.0);
    assert!(v[0].color[0] > 0.0);
    assert_eq!(v[0].key, KEY_WORLD, "the additive bit is not part of the group");
}

/// The head moves with the emitter's interpolated draw: a tick behind, from
/// where it stood to the newest point; and a strip's first point comes to a
/// point rather than a square end.
#[test]
fn the_head_follows_the_emitter_and_ends_are_pointed() {
    let mut f = Fed::new(1);
    f.op(TrailOp::Style { id: 1, style: style(1.0) });
    f.point(1, 10, [0.0; 3], [0.0; 3]);
    f.point(1, 11, [1.0, 0.0, 0.0], [0.0; 3]);
    f.point(1, 12, [2.0, 0.0, 0.0], [0.0; 3]);
    // Tick 12 has just run (now 13): at alpha 0 the emitter is drawn where it
    // was after tick 11, at alpha 0.5 half way on.
    assert_eq!(f.verts(0, 13, 0.0)[0].pos, [1.0, 0.0, 0.0]);
    assert_eq!(f.verts(0, 13, 0.5)[0].pos, [1.5, 0.0, 0.0]);
    let v = f.verts(0, 13, 0.5);
    assert_eq!(v[2].width, 0.0, "the oldest point has no width");
    assert_eq!(v[2].key, KEY_NONE, "and starts no segment");
    assert!(v[1].width > 0.0 && v[1].key == KEY_WORLD);
    // An emitter gone quiet: the head tapers within two ticks and stays put.
    let v = f.verts(0, 15, 0.0);
    assert_eq!(v[0].pos, [2.0, 0.0, 0.0]);
    assert_eq!(v[0].width, 0.0);
}

#[test]
fn a_cut_starts_a_new_strip_that_never_joins_the_old_one() {
    let mut f = Fed::new(1);
    f.op(TrailOp::Style { id: 1, style: style(1.0) });
    for tick in 0..5 {
        f.point(1, tick, [tick as f32, 0.0, 0.0], [0.0; 3]);
    }
    f.op(TrailOp::Cut { id: 1 });
    for tick in 5..10 {
        f.point(1, tick, [tick as f32, 5.0, 0.0], [0.0; 3]);
    }
    let v = f.verts(0, 10, 0.5);
    // k 4 is the new strip's first point (tick 5), k 5 the old strip's last.
    assert_eq!(v[4].pos, [5.0, 5.0, 0.0]);
    assert_eq!((v[4].key, v[4].width), (KEY_NONE, 0.0), "no segment across the cut");
    assert_eq!(v[5].pos, [4.0, 0.0, 0.0]);
    assert_eq!(v[5].width, 0.0, "the old strip ends in a point");
    assert_eq!(v[3].key, KEY_WORLD);
    assert_eq!(v[6].key, KEY_WORLD);
    // Its tangent looks only along its own strip.
    assert_eq!(v[5].tan, [-1.0, 0.0, 0.0]);
    assert_eq!(v[4].tan, [-1.0, 0.0, 0.0]);
}

/// A long life spreads the points: the ring covers it, the newest point keeps
/// following the emitter tick by tick, nothing older moves.
#[test]
fn a_long_life_spreads_the_points_over_the_ring() {
    let mut f = Fed::new(1);
    f.op(TrailOp::Style { id: 1, style: style(3.0) });
    let mut tick = 0;
    for _ in 0..200 {
        f.point(1, tick, [tick as f32 * 0.1, 0.0, 0.0], [0.0; 3]);
        tick += 1;
    }
    f.sys.advance(tick, 0.0, DT);
    let s = f.sys.slots[0];
    assert_eq!(s.every, 3, "62 points every 3 ticks cover 3 s at 60 Hz");
    let h = f.sys.heads()[0];
    assert!((h.life - 3.0).abs() < 1e-4, "{}", h.life);
    // The oldest point in the ring is past its life before it is overwritten.
    let run = 400;
    for _ in 0..run {
        f.point(1, tick, [tick as f32 * 0.1, 0.0, 0.0], [0.0; 3]);
        tick += 1;
    }
    let v = f.verts(0, tick, 0.0);
    let last_alive = v.iter().rposition(|p| p.key != KEY_NONE).unwrap();
    assert!((60..63).contains(&last_alive), "{last_alive}");
    assert!(v[last_alive].color[3] < 0.05, "fades before the ring ends: {:?}", v[last_alive].color);
    // The newest follows every tick.
    assert_eq!(f.points[f.sys.heads[0].head as usize].tick, tick - 1);
}

#[test]
fn a_still_emitter_does_not_use_up_its_ring() {
    let mut f = Fed::new(1);
    f.op(TrailOp::Style { id: 1, style: style(1.0) });
    for tick in 0..3 {
        f.point(1, tick, [tick as f32, 0.0, 0.0], [0.0; 3]);
    }
    let before = f.sys.heads[0].count;
    for tick in 3..100 {
        f.point(1, tick, [2.0, 0.0, 0.0], [0.0; 3]);
    }
    assert!(f.sys.heads[0].count <= before + 1, "{}", f.sys.heads[0].count);
    // Its old points age out; the head is still there.
    let v = f.verts(0, 100, 0.0);
    assert_eq!(v[0].pos, [2.0, 0.0, 0.0]);
    assert!(f.sys.live() <= 1);
}

#[test]
fn slots_free_recycle_and_clear() {
    let mut s = TrailSystem::new();
    s.set_cap(2);
    let p = |s: &mut TrailSystem, id: u32, tick: u64| {
        s.apply(&TrailOp::Point { id, tick, pos: [tick as f32, 0.0, 0.0], edge: [0.0; 3], width: 1.0 }, DT);
    };
    for t in 0..3 {
        p(&mut s, 1, t);
        p(&mut s, 2, t + 1);
    }
    s.advance(5, 0.0, DT);
    assert_eq!((s.live(), s.trails()), (2, 2));
    assert_eq!(s.groups(), &[] as &[u32], "off: no groups");
    s.on = true;
    assert_eq!(s.groups(), [KEY_WORLD]);
    // With every slot taken a third emitter waits: the two strips stay whole.
    p(&mut s, 3, 9);
    assert_eq!((s.slots[0].id, s.slots[1].id), (1, 2));
    assert_eq!((s.heads[0].count, s.heads[1].count), (3, 3));
    // Strips are over after their life (default 0.5 s): slots free up and the
    // reader's range shrinks.
    s.advance(100, 0.0, DT);
    assert_eq!((s.live(), s.trails()), (0, 0));
    assert!(s.slots.iter().all(|sl| !sl.used));
    // A style set in the frame of an emitter's first point holds for its strip.
    s.apply(&TrailOp::Style { id: 7, style: style(9.0) }, DT);
    p(&mut s, 7, 200);
    assert_eq!(s.slots[0].life, 9.0);
    s.advance(201, 0.0, DT);
    assert_eq!(s.slots[0].every, 9, "62 points every 9 ticks cover its 9 s");
    // A style with no point after it holds no slot.
    s.apply(&TrailOp::Style { id: 8, style: style(1.0) }, DT);
    s.advance(202, 0.0, DT);
    assert!(s.slots.iter().all(|sl| sl.id != 8 || !sl.used));
    // Clear drops every strip but keeps the rings.
    s.apply(&TrailOp::Clear, DT);
    s.advance(202, 0.0, DT);
    assert_eq!((s.live(), s.trails(), s.cap()), (0, 0, 2));
    // A cap change keeps the strips that fit, so a tier switch shows no pop.
    s.set_cap(4);
    p(&mut s, 11, 300);
    p(&mut s, 12, 300);
    p(&mut s, 13, 300);
    assert_eq!(s.slots.iter().filter(|sl| sl.used).count(), 3);
    s.set_cap(2);
    assert_eq!(s.slots.iter().filter(|sl| sl.used).count(), 2, "only the one past the new cap went");
    assert_eq!((s.slots[0].id, s.slots[1].id), (11, 12));
    s.set_cap(4);
    assert_eq!(s.slots.iter().filter(|sl| sl.used).count(), 2);
}

fn app(with_pass: bool) -> App {
    let a = App::new("t")
        .with_module(crate::RenderModule::new())
        .with_system(evani_core::ecs::SystemDesc::new("emit", evani_core::ecs::Stage::Update, |w: &mut World| {
            let tick = w.res::<Time>().tick;
            w.res_mut::<TrailQueue>().point(1, arc(tick as u32, 0.25, 1.6));
        }));
    if with_pass {
        let mut g = a.world.res_mut::<RenderGraph>();
        let _ = g.add_target(crate::graph::TargetDesc::new(BUFFER_TARGET, crate::graph::TargetKind::Buffer));
        let _ = g.add_pass(crate::graph::PassDesc::compute(SIM_PASS, "fx.trails").writes(&[BUFFER_TARGET]));
    }
    a
}

/// The same fixed ticks give the same points, however the frames fall: a
/// stepped capture repeats, and the game's clock alone decides the strip.
#[test]
fn the_fixed_step_alone_decides_the_points() {
    let run = |frames: &[f64]| {
        let mut a = app(true);
        let mut got: Vec<(u32, Point)> = Vec::new();
        let mut now = 0.0;
        for dt in frames {
            now += dt;
            a.frame(now);
            got.extend_from_slice(a.world.res::<TrailSystem>().staged());
            if a.world.res::<Time>().tick >= 40 {
                break;
            }
        }
        got.truncate(38);
        got
    };
    let even = vec![1000.0 / 60.0; 100];
    let lumpy: Vec<f64> = (0..200).map(|i| if i % 3 == 0 { 31.0 } else { 7.0 }).collect();
    let a = run(&even);
    assert!(a.len() >= 38, "{}", a.len());
    assert_eq!(a, run(&lumpy));
    assert_eq!(a, run(&even), "and every run alike");
}

#[test]
fn nothing_is_kept_without_the_pass_and_the_params_steer_it() {
    let mut a = app(false);
    a.run_headless(30, 1000.0 / 60.0);
    {
        let s = a.world.res::<TrailSystem>();
        assert!(!s.is_on());
        assert_eq!((s.live(), s.trails(), s.groups().len()), (0, 0, 0));
        assert!(s.slots.iter().all(|sl| !sl.used));
        let q = a.world.res::<TrailQueue>();
        assert!(q.is_attached() && q.pending().is_empty());
    }
    // With the pass: a live strip, and the tier's cap and steps.
    let mut a = app(true);
    assert!(a.world.res_mut::<Tiers>().set_active("medium"));
    a.run_headless(20, 1000.0 / 60.0);
    {
        let s = a.world.res::<TrailSystem>();
        assert!(s.is_on() && s.live() == 1);
        assert_eq!((s.cap(), s.steps()), (16, 2));
    }
    // A registered param steers it.
    {
        let mut reg = a.world.res_mut::<ParamRegistry>();
        reg.register_f32(ParamDef::f32(PARAM_MAX, 64.0).range(0.0, 64.0)).unwrap();
        reg.register_f32(ParamDef::f32(PARAM_WIDTH, 1.0).range(0.0, 4.0)).unwrap();
        reg.register_f32(ParamDef::f32(PARAM_SOFT, SOFT).range(0.0, 1.0)).unwrap();
        reg.set_by_name(PARAM_MAX, ParamValue::F32(4.0)).unwrap();
        reg.set_by_name(PARAM_WIDTH, ParamValue::F32(2.0)).unwrap();
        reg.set_by_name(PARAM_SOFT, ParamValue::F32(0.5)).unwrap();
    }
    a.world.res_mut::<TrailSystem>().handles = None;
    a.run_headless(2, 1000.0 / 60.0);
    let s = a.world.res::<TrailSystem>();
    assert_eq!((s.cap(), s.sim_uniform().width, s.soft()), (4, 2.0, 0.5));
}

#[test]
fn draw_words_carry_the_group_steps_and_softness() {
    let vp = [1.0; 16];
    let w = draw_words(&vp, [1.0, 2.0, 3.0], 42, [640.0, 360.0], 3, 0.25);
    assert_eq!(w[16..19], [1.0, 2.0, 3.0]);
    assert_eq!(w[19].to_bits(), 42);
    assert_eq!(w[20..], [640.0, 360.0, 3.0, 0.25]);
    // Budgets from the plan: 64 / 32 / 16 / 8 emitters of 64 points.
    assert_eq!(["ultra", "high", "medium", "low", "lite", "vr"].map(tier_cap), [64, 32, 16, 8, 8, 16]);
    assert_eq!(["ultra", "high", "medium", "low", "lite", "vr"].map(tier_steps), [4, 3, 2, 1, 1, 2]);
}

/// A world strip in a build with no 3D path says so once instead of being
/// dropped silently.
#[cfg(not(feature = "forward"))]
#[test]
fn a_world_strip_without_the_3d_path_is_reported_once() {
    let mut a = app(true);
    a.run_headless(10, 1000.0 / 60.0);
    assert_eq!(a.world.res::<evani_core::overlay::ErrorLog>().error_count(), 1);
}

/// A style alone never takes a slot: an idle emitter that styles every tick
/// cannot keep a drifting one waiting (Low has 8 slots).
#[test]
fn an_idle_style_holds_no_slot() {
    let mut s = TrailSystem::new();
    s.set_cap(2);
    let pt = |s: &mut TrailSystem, id: u32, tick: u64| {
        s.apply(&TrailOp::Point { id, tick, pos: [tick as f32, 0.0, 0.0], edge: [0.0; 3], width: 1.0 }, DT);
    };
    for tick in 0..3 {
        pt(&mut s, 1, tick);
        // Emitter 2 styles every tick and lays nothing.
        s.apply(&TrailOp::Style { id: 2, style: style(1.0) }, DT);
        // Emitter 3 comes later in the queue with a point.
        s.apply(&TrailOp::Style { id: 3, style: style(2.0) }, DT);
        pt(&mut s, 3, tick);
        s.advance(tick as u32 + 1, 0.0, DT);
    }
    let ids: Vec<u32> = s.slots.iter().take(2).filter(|sl| sl.used).map(|sl| sl.id).collect();
    assert_eq!(ids, [1, 3]);
    assert_eq!(s.slots[1].life, 2.0, "it took its style from the same frame");
}

/// Points that never reached the GPU (a device lost or not ready): every
/// strip starts again rather than joining points the GPU never had.
#[test]
fn writes_that_missed_the_gpu_restart_the_strips() {
    let mut a = app(true);
    a.run_headless(5, 1000.0 / 60.0);
    // A GPU took last frame's writes.
    a.world.res_mut::<TrailSystem>().mark_uploaded();
    a.run_headless(1, 1000.0 / 60.0);
    assert!(a.world.res::<TrailSystem>().heads[0].count > 1);
    // This frame's writes are never uploaded: next frame the strip restarts.
    a.run_headless(1, 1000.0 / 60.0);
    let s = a.world.res::<TrailSystem>();
    assert!(s.heads[0].count <= 1, "{}", s.heads[0].count);
}

/// `trails_draw.wgsl` `vs_world`'s thin-strip widening, on the CPU: the
/// widened corner in clip space, from the strip's centre, its half span and
/// which side.
fn widen(vp: &crate::math::Mat4, pos: [f32; 3], side: [f32; 3], s: f32, viewport: [f32; 2]) -> ([f32; 4], [f32; 4], f32) {
    let clip = |p: [f32; 3]| -> [f32; 4] {
        core::array::from_fn(|r| (0..4).map(|c| vp[c * 4 + r] * [p[0], p[1], p[2], 1.0][c]).sum())
    };
    let c0 = clip(sub(pos, side));
    let c1 = clip(add(pos, side));
    let plain = clip(add(pos, scale(side, s)));
    let mut c = plain;
    let mut k = 1.0f32;
    if c0[3] > 0.0 && c1[3] > 0.0 {
        let d = [(c1[0] / c1[3] - c0[0] / c0[3]) * viewport[0] * 0.5, (c1[1] / c1[3] - c0[1] / c0[3]) * viewport[1] * 0.5];
        let px = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if px > 0.0 && px < 1.5 {
            k = 1.5 / px;
            for i in 0..2 {
                c[i] += d[i] / px.max(1e-6) * s * (1.5 - px) / viewport[i].max(1.0) * c[3];
            }
        }
    }
    (c, plain, k)
}

/// A strip nearly end-on (a first-person thrust, a mark straight ahead) is
/// widened on screen only: its corners keep the depth they had, in front of
/// the eye, and it fades instead of stretching along the view ray.
#[test]
fn widening_a_thin_strip_keeps_its_depth() {
    let pose = evani_core::camera::CameraPose { eye: [0.0, 0.0, 0.0], target: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0], fov_y_deg: 60.0 };
    let vp = crate::shaders::view_proj(&pose, 1600.0 / 900.0);
    let viewport = [1600.0, 900.0];
    for (dist, half, off) in [(0.7f32, 0.4f32, 1e-4f32), (0.7, 0.4, 5e-4), (3.0, 0.4, 1e-4), (60.0, 0.13, 2e-3)] {
        // The span points along the view ray, `off` radians aside.
        let pos = [0.0, 0.0, -dist];
        let side = [half * off.sin(), 0.0, -half * off.cos()];
        for s in [-1.0, 1.0] {
            let (c, plain, k) = widen(&vp, pos, side, s, viewport);
            assert!(c[3] > 0.0, "{dist} m, {off} rad: a corner behind the eye");
            assert_eq!((c[2], c[3]), (plain[2], plain[3]), "{dist} m: the depth moved");
            assert!(k > 1.0, "{dist} m, {off} rad: thin, so widened");
            let ndc = [c[0] / c[3], c[1] / c[3]];
            let at = [plain[0] / plain[3], plain[1] / plain[3]];
            let px = ((ndc[0] - at[0]) * 800.0).hypot((ndc[1] - at[1]) * 450.0);
            assert!(px < 1.5, "{dist} m: moved {px} px on screen");
        }
    }
}

