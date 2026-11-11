use super::*;
use evani_core::params::ParamRegistry;

fn validate(src: &str) {
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
}

fn reg() -> (ParamRegistry, SeaParams) {
    let mut reg = ParamRegistry::new();
    crate::NatureParams::register(&mut reg).unwrap();
    let p = SeaParams::register(&mut reg).unwrap();
    (reg, p)
}

fn set(reg: &mut ParamRegistry, n: &str, v: f32) {
    reg.set_by_name(&format!("material.nature.water.{n}"), ParamValue::F32(v)).unwrap();
}

fn frame(level: usize, eye: [f32; 3], target: [f32; 3], t: f64) -> FrameIn {
    FrameIn {
        camera: CameraPose { eye, target, up: [0.0, 1.0, 0.0], fov_y_deg: 55.0 },
        size: [1280.0, 720.0],
        time: t,
        sun: norm([0.3, 0.35, -0.9]),
        sun_col: [3.0, 2.9, 2.7],
        sky_tint: crate::pack_sky_tint([1.0; 3]),
        sea: Sea::new(0.0, [0.0, 0.0], 40_000.0),
        level,
        inv: inverse_distance(NEAR, FAR),
        jitter: [0.0; 2],
    }
}

/// The checked-in WGSL table is the generator's output (deterministic:
/// the engine's seeded Pcg32). `EVANI_SEA_TABLE_WRITE=1` rewrites it.
#[test]
fn the_table_file_matches_its_generator() {
    let want = spectrum::sea_table_wgsl(&spectrum::table());
    if std::env::var("EVANI_SEA_TABLE_WRITE").is_ok() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders/sea_table.wgsl");
        std::fs::write(path, &want).unwrap();
        return;
    }
    assert_eq!(TABLE_WGSL.replace("\r\n", "\n"), want, "shaders/sea_table.wgsl is stale: run this test with EVANI_SEA_TABLE_WRITE=1");
}

#[test]
fn the_table_is_deterministic_and_physical() {
    let a = spectrum::table();
    assert_eq!(a, spectrum::table());
    assert_eq!(a.len(), spectrum::COMPONENTS);
    let geo = |level: usize, swell: bool| -> &[spectrum::Comp] {
        let (b, (w, s)) = (spectrum::GEO_BASE[level], spectrum::GEO[level]);
        if swell { &a[b + w..b + w + s] } else { &a[b..b + w] }
    };
    let det = &a[spectrum::DET_BASE..];
    for part in [geo(3, false), geo(3, true), geo(2, false), geo(1, false), det] {
        assert!(part.windows(2).all(|w| w[0].xx <= w[1].xx), "longest first");
        for c in part {
            assert!(c.amp > 0.0 && c.xx > 0.4 && c.xx < 150.0, "{c:?}");
            assert!(((c.cos * c.cos + c.sin * c.sin) - 1.0).abs() < 1e-4);
            assert!((0.0..1.0).contains(&c.phase));
        }
    }
    // Every level's height set carries the wind sea's whole height (the
    // JONSWAP integral, about 0.2 x the peak enhancement's gain, up to a
    // quarter of the peak's length) and reaches down to short waves.
    let var = |p: &[spectrum::Comp]| p.iter().map(|c| c.amp * c.amp * 0.5).sum::<f32>();
    for level in 1..4 {
        let v = var(geo(level, false));
        assert!((0.2..0.45).contains(&v), "level {level}: {v}");
        assert!((v - var(geo(3, false))).abs() < 1e-3, "level {level} keeps the full height");
        assert!(geo(level, false).last().is_some_and(|c| c.xx > 3.0), "level {level} reaches short waves");
    }
    // The detail set is far smaller in height, but not in slope.
    let slope = |p: &[spectrum::Comp]| p.iter().map(|c| c.amp * c.amp * c.xx * c.xx * 0.5).sum::<f32>();
    assert!(var(det) < 0.01 * var(geo(3, false)));
    assert!(slope(det) > 0.5 * slope(geo(3, false)), "{} {}", slope(det), slope(geo(3, false)));
    // Mostly downwind: the mean heading's cosine is well above zero.
    let mean_cos = |p: &[spectrum::Comp]| p.iter().map(|c| c.cos * c.amp).sum::<f32>() / p.iter().map(|c| c.amp).sum::<f32>();
    assert!(mean_cos(geo(3, false)) > 0.6, "{}", mean_cos(geo(3, false)));
    // The swell is narrower than the wind sea.
    let spread = |p: &[spectrum::Comp]| p.iter().map(|c| c.sin.abs() * c.amp).sum::<f32>() / p.iter().map(|c| c.amp).sum::<f32>();
    assert!(spread(geo(3, true)) < spread(geo(3, false)));
    assert_eq!((0..spectrum::COMPONENTS).filter(|i| spectrum::is_swell(*i)).count(), [1, 2, 3].map(|l| spectrum::GEO[l].1).iter().sum::<usize>());
    // What the shader reads is the table, rounded as written.
    assert_eq!(shader_table(), spectrum::as_shader(&a));
}

/// A realistic sea: 9 m/s over 120 km gives a significant wave height of
/// about 1.5 to 2.5 m and waves some 60 m long; a 20 m/s storm over 400 km
/// 6 to 9 m and well over 150 m.
#[test]
fn wind_and_fetch_give_realistic_waves() {
    let t = spectrum::table();
    let (w, _) = spectrum::GEO[3];
    let var: f32 = t[..w].iter().map(|c| c.amp * c.amp * 0.5).sum();
    for (u, f, hs, len) in [(9.0, 120_000.0, 1.5..2.6, 50.0..75.0), (20.0, 400_000.0, 6.0..9.5, 150.0..320.0)] {
        let (kp, scale) = spectrum::scales(u, f);
        let h = 4.0 * (var * scale * scale).sqrt();
        let l = core::f32::consts::TAU / kp;
        assert!(hs.contains(&h), "U {u}: Hs {h}");
        assert!(len.contains(&l), "U {u}: peak length {l}");
    }
}

#[test]
fn every_sea_shader_validates() {
    for sh in SHADERS.iter() {
        assert!(sh.source.starts_with("// evani.shader.nature_sea"), "{} marker", sh.name);
        assert!(sh.slots.is_empty(), "{}: the sea reads no slot (its settings are uniforms)", sh.name);
        validate(sh.source);
    }
}

#[test]
fn params_register_under_water_with_descriptions() {
    let (reg, p) = reg();
    for n in param_names() {
        let id = reg.id(&n).unwrap_or_else(|| panic!("{n}"));
        let d = reg.def(id).unwrap();
        assert!(n.starts_with("material.nature.water.") || n.starts_with("material.nature.sky.") || n == "material.nature.enabled", "{n}");
        assert!(!d.description.is_empty(), "{n}");
    }
    let s = p.settings(&reg);
    assert_eq!((s.wind_speed, s.fetch_km, s.wave_height, s.beach_slope), (9.0, 120.0, 1.0, 0.0));
    assert_eq!(p.quality(&reg), -1.0);
    assert!(p.enabled(&reg));
}

#[test]
fn levels_follow_the_tiers_and_the_switch() {
    assert_eq!(["lite", "low", "medium", "vr", "high", "ultra"].map(level_for_tier), [0, 0, 1, 1, 2, 3]);
    assert_eq!(level(-1.0, "ultra"), 3);
    assert_eq!(level(0.0, "ultra"), 0);
    assert_eq!(level(2.0, "low"), 2);
    assert_eq!(level(9.0, "low"), 3);
    assert_eq!(shows_on("material.nature.water.spray"), "Ultra only");
    assert_eq!(shows_on("material.nature.water.glitter"), "High and Ultra");
    assert_eq!(shows_on("material.nature.water.wave_height"), "Medium and up");
    assert_eq!(shows_on("material.nature.water.deep_color"), "every level");
    for (n, _) in SHOWS_FROM {
        assert!(param_names().iter().any(|p| p == n), "{n}");
    }
}

#[test]
fn colours_pack_into_one_word() {
    for c in [[0.02, 0.1, 0.13], [0.62, 0.72, 0.82], [1.0, 0.0, 0.5]] {
        let w = pack_col(c) as u32;
        let back = [w & 255, (w >> 8) & 255, (w >> 16) & 255].map(|q| (q as f32 / 255.0).powi(2));
        for k in 0..3 {
            assert!((back[k] - c[k]).abs() < 0.01, "{c:?} {back:?}");
        }
    }
}

/// Every setting is in the uniforms: a change shows on the very next draw,
/// with no shader rebuild (the sea's shaders read no slot).
#[test]
fn settings_reach_the_next_draw() {
    let (mut reg, p) = reg();
    let fr = frame(3, [0.0, 6.0, 0.0], [0.0, 2.0, -30.0], 12.0);
    let before = uniforms(&p.settings(&reg), &fr);
    for (n, v, word) in [("wind_speed", 18.0, 32), ("fetch", 400.0, 33), ("wind_direction", 90.0, 34), ("sea_swell", 1.5, 36), ("sea_depth", 3.0, 41), ("glitter", 0.1, 53), ("beach_slope", 0.05, 60), ("breaker_height", 1.6, 63), ("spray", 0.0, 31), ("caustics", 0.3, 55)] {
        let mut r2 = reg.clone();
        set(&mut r2, n, v);
        let after = uniforms(&p.settings(&r2), &fr);
        assert_ne!(before[word], after[word], "{n}");
    }
    set(&mut reg, "choppiness", 0.8);
    let pr = profile(0.8);
    let u = uniforms(&p.settings(&reg), &fr);
    assert_eq!([u[38], u[39], u[40]], pr);
    // The breakers' length and height share a word.
    set(&mut reg, "breaker_length", 42.0);
    set(&mut reg, "breaker_height", 1.25);
    let u = uniforms(&p.settings(&reg), &fr);
    assert_eq!((u[63] / 8.0).floor(), 42.0);
    assert!((u[63] - 8.0 * 42.0 - 1.25).abs() < 1e-4);
}

/// The profile exp(s (sin - 1)) keeps a sine's variance at every
/// sharpness, has sharper crests than troughs, and is a sine at s = 0.
#[test]
fn the_crest_profile_keeps_its_energy() {
    for c in [0.0, 0.3, 0.6, 1.0] {
        let [s, m, k] = profile(c);
        let n = 4096;
        let (mut mean, mut var, mut hi, mut lo) = (0.0f64, 0.0f64, f64::MIN, f64::MAX);
        for i in 0..n {
            let th = core::f64::consts::TAU * (i as f64 + 0.5) / n as f64;
            let v = ((f64::from(s) * (th.sin() - 1.0)).exp() - f64::from(m)) * f64::from(k);
            mean += v / n as f64;
            var += v * v / n as f64;
            hi = hi.max(v);
            lo = lo.min(v);
        }
        assert!(mean.abs() < 1e-3, "{c}: mean {mean}");
        assert!((var - 0.5).abs() < 2e-3, "{c}: var {var}");
        if c > 0.2 {
            assert!(hi > -lo * 1.15, "{c}: crests {hi} sharper than troughs {lo}");
        }
    }
}

/// The waves depend only on position and sim time.
#[test]
fn heights_are_deterministic() {
    let (mut reg, p) = reg();
    set(&mut reg, "choppiness", 0.6);
    let u = uniforms(&p.settings(&reg), &frame(3, [0.0, 6.0, 0.0], [0.0, 2.0, -30.0], 37.5));
    let a = Mirror::new(&u);
    let b = Mirror::new(&u);
    let pts: Vec<[f32; 2]> = (0..64).map(|i| [i as f32 * 3.7 - 90.0, (i * 7 % 23) as f32 * 2.1 - 20.0]).collect();
    assert!(pts.iter().all(|q| a.height(*q, 0.01) == b.height(*q, 0.01)));
    // A later time moves the waves; the same time later gives the same sea.
    let u2 = uniforms(&p.settings(&reg), &frame(3, [0.0, 6.0, 0.0], [0.0, 2.0, -30.0], 39.0));
    let c = Mirror::new(&u2);
    assert!(pts.iter().any(|q| (a.height(*q, 0.01) - c.height(*q, 0.01)).abs() > 0.01));
    // The loop is seamless: 300 s later is the same sea.
    let u3 = uniforms(&p.settings(&reg), &frame(3, [0.0, 6.0, 0.0], [0.0, 2.0, -30.0], 337.5));
    let d = Mirror::new(&u3);
    assert!(pts.iter().all(|q| (a.height(*q, 0.01) - d.height(*q, 0.01)).abs() < 2e-3));
}

/// Chris's Ultra-Water shot: huge troughs opened a hole onto a flat glassy
/// plane. Here, at the worst storm the settings allow (and a beach with the
/// tallest breakers), from low and high cameras, many headings, pitches
/// and times, over beds from 0.5 m to 200 m deep:
///   * the deepest trough the waves can make stays above the bed everywhere
///     (the waves shrink over shallow water), and
///   * every view ray that reaches the bed meets the waves first, on the
///     wave surface (the march brackets and bisects, never returns a plane).
#[test]
fn no_bottoming_out_in_the_worst_storm() {
    let (mut reg, p) = reg();
    for (n, v) in [("wind_speed", 30.0), ("fetch", 1000.0), ("wave_height", 3.0), ("sea_swell", 2.0), ("choppiness", 1.0)] {
        if n == "choppiness" {
            reg.set_by_name("material.nature.water.choppiness", ParamValue::F32(v)).unwrap();
        } else {
            set(&mut reg, n, v);
        }
    }
    let mut rays = 0;
    let mut hits = 0;
    let mut under = 0;
    let mut worst = 0.0f32;
    let mut skipped = 0;
    let mut worst_skip = 0.0f32;
    for &(depth, beach) in &[(0.5, false), (2.0, false), (6.0, false), (30.0, false), (200.0, false), (40.0, true)] {
        let mut r = reg.clone();
        set(&mut r, "sea_depth", depth);
        if beach {
            set(&mut r, "beach_slope", 0.04);
            set(&mut r, "beach_distance", 120.0);
            set(&mut r, "beach_direction", 0.0);
            set(&mut r, "breaker_height", 3.0);
            set(&mut r, "breaker_length", 40.0);
        }
        let s = p.settings(&r);
        for level in 1..=3 {
            for &(h, time) in &[(0.6, 3.0), (2.5, 41.0), (8.0, 97.5), (25.0, 180.25)] {
                let fr = frame(level, [0.0, h, 0.0], [0.0, h, -10.0], time);
                let u = uniforms(&s, &fr);
                let m = Mirror::new(&u);
                // The bed: flat at -depth, or the beach rising to the land.
                let bed = |x: f32, z: f32| -> f32 { if beach { -m.depth_at([x, z]) } else { -depth } };
                // Troughs never reach the bed (sampled over a 600 m square).
                for i in 0..48 {
                    for j in 0..48 {
                        let q = [i as f32 * 12.5 - 300.0, j as f32 * 12.5 - 300.0];
                        if bed(q[0], q[1]) >= -0.01 {
                            continue;
                        }
                        let lowest = m.trough_bound(q);
                        assert!(lowest > bed(q[0], q[1]) * 0.99, "d {depth} beach {beach}: trough {lowest} at {q:?} under bed {}", bed(q[0], q[1]));
                        let hq = m.height(q, 0.01);
                        assert!(hq >= lowest - 1e-3, "height {hq} below its bound {lowest}");
                    }
                }
                // Rays, low and grazing to steep, all round.
                for yaw in 0..12 {
                    let a = yaw as f32 * core::f32::consts::TAU / 12.0;
                    for pitch in [-0.6, -2.0, -5.0, -12.0, -30.0, -65.0, 3.0] {
                        let pr = (pitch as f32).to_radians();
                        let rd = [a.cos() * pr.cos(), pr.sin(), a.sin() * pr.cos()];
                        let ro = [0.0, h, 0.0];
                        // Where the ray meets the bed (if it does, within 3 km).
                        let mut tb = f32::INFINITY;
                        let mut tt = 0.0;
                        while tt < 3000.0 {
                            let y = ro[1] + rd[1] * tt;
                            if y <= bed(ro[0] + rd[0] * tt, ro[2] + rd[2] * tt) {
                                tb = tt;
                                break;
                            }
                            tt += 0.5 + tt * 0.01;
                        }
                        let bed_wet = tb.is_finite() && bed(ro[0] + rd[0] * tb, ro[2] + rd[2] * tb) < -0.05;
                        rays += 1;
                        if m.eye_under(ro) {
                            // Inside a wave: the underwater view, no march.
                            under += 1;
                            continue;
                        }
                        let t = m.march(ro, rd, if tb.is_finite() { tb } else { 6.0e4 });
                        if bed_wet {
                            assert!(t > 0.0, "d {depth} beach {beach} level {level} eye {h} yaw {yaw} pitch {pitch}: the ray reached the bed at {tb} without meeting the sea");
                            assert!(t < tb, "the sea in front of the bed");
                        }
                        if t > 0.0 {
                            hits += 1;
                            // On the waves: a true crossing (above just before, in or
                            // under the water just after) within 0.4 % of the distance.
                            let gap = m.gap(ro, rd, t).abs();
                            assert!(gap < 0.05 + t * 0.004, "d {depth} level {level} eye {h} yaw {yaw} pitch {pitch}: the hit is {gap} m off the waves at t {t}");
                            let (_, ta, tb) = m.march_bracket(ro, rd, if tb.is_finite() { tb } else { 6.0e4 });
                            // A bracketed crossing, or within a pixel's height of the
                            // waves (the march stops there).
                            let close = ta == tb && m.gap(ro, rd, t) < 0.004 + 1.5 * t * u[19] + 1e-4;
                            assert!(close || (m.gap(ro, rd, ta) >= 0.0 && m.gap(ro, rd, tb) <= 0.0 && (ta..=tb).contains(&t)), "no crossing in [{ta}, {tb}] round {t}");
                            // The first crest along the ray, found by a fine scan: a
                            // march that ran out of steps may land on a later crest
                            // (still the waves, never a plane); that must stay rare.
                            let mut first = t;
                            let mut s = 0.0;
                            while s < t {
                                if m.gap(ro, rd, s) <= 0.0 {
                                    first = s;
                                    break;
                                }
                                s += (0.004 * s).max(0.04);
                            }
                            if first < t - (0.2 + 0.01 * t) {
                                // How deep into the skipped crest the ray went, in
                                // pixels on screen: under one, nothing shows.
                                let mut deep = 0.0f32;
                                let mut s = first;
                                while s < t {
                                    deep = deep.max(-m.gap(ro, rd, s));
                                    s += (0.004 * s).max(0.04);
                                }
                                let px = deep / (first * u[19]);
                                if px > 1.0 {
                                    skipped += 1;
                                    worst_skip = worst_skip.max(px);
                                }
                                continue;
                            }
                            // On the first crest, within a quarter of a pixel's reach
                            // along the ray (its footprint over the ray's slant) of
                            // where that crossing really is.
                            let (mut lo, mut hi) = ((first - (0.004 * first).max(0.04)).max(0.0), first);
                            for _ in 0..60 {
                                let mid = 0.5 * (lo + hi);
                                if m.gap(ro, rd, mid) > 0.0 { lo = mid } else { hi = mid }
                            }
                            let truth = 0.5 * (lo + hi);
                            let reach = t * u[19] / rd[1].abs().max(1e-3);
                            let tol = 0.1 + 0.003 * t + 0.25 * reach;
                            worst = worst.max((t - truth).abs() / tol);
                            assert!((t - truth).abs() < tol, "t {t} but the first crossing is at {truth} in [{ta}, {tb}] (reach {reach})");
                        }
                    }
                }
            }
        }
    }
    assert!(rays > 1500 && hits > 1000, "{rays} rays, {hits} hits, {under} from inside a wave");
    assert!(under < rays / 4, "{under} of {rays} rays from inside a wave");
    eprintln!("no_bottoming_out: {rays} rays, {hits} on the waves, {under} from inside a wave, {skipped} skipped a crest by over a pixel (worst {worst_skip:.1} px), worst error {:.3} of the tolerance", worst);
    // At these settings (crests bounded at 80 m) a grazing ray can run out
    // of steps and land on a later crest: still the waves. The presets'
    // accuracy is `the_march_finds_the_first_crest`.
    assert!(skipped * 100 < hits * 8, "{skipped} of {hits} hits skipped a crest by over a pixel");
}

/// Every level draws the copy, the quick stand-in and the sea (Low the
/// sea's flat path inside it); nothing at all without a sea. The stand-in
/// is small, so it is built long before the full sea.
#[test]
fn each_level_records_its_draws() {
    #[derive(Default)]
    struct Rec(Vec<(usize, Option<String>, String, usize)>);
    impl PostRecorder for Rec {
        fn target_size(&self, _: &str) -> [f32; 2] {
            [640.0, 360.0]
        }
        fn draw(&mut self, shader: usize, src: Option<&str>, dst: &str, u: &[f32]) {
            self.0.push((shader, src.map(String::from), dst.to_string(), u.len()));
        }
    }
    let (reg, p) = reg();
    let sea = SeaSurfaces::default();
    let fx = SeaEffect { sea: sea.clone(), params: p };
    let mut r = Rec::default();
    fx.record(PASS, &reg, &mut r);
    assert!(r.0.is_empty(), "no sea, no draws");
    sea.set(Sea::new(0.0, [0.0, 0.0], 1000.0));
    for level in 0..4 {
        sea.0.level.set(level);
        let mut r = Rec::default();
        fx.record(PASS, &reg, &mut r);
        let got: Vec<(usize, Option<&str>, &str, usize)> = r.0.iter().map(|(s, a, b, n)| (*s, a.as_deref(), b.as_str(), *n)).collect();
        assert_eq!(got, vec![(COPY, Some("hdr"), SCENE, 4), (QUICK, None, "hdr", 64), (FULL, Some(SCENE), "hdr", 64)], "level {level}");
        let mut out = [0.0; 64];
        let f = UniformFrame { source_size: [640.0, 360.0], target_size: [640.0, 360.0], mip: 0 };
        assert_eq!(fx.write_uniforms("nature_sea", &reg, &f, &mut out), 64);
        assert_eq!(out[27], level as f32, "the level word");
    }
    assert!(SHADERS[QUICK].source.len() * 10 < SHADERS[FULL].source.len(), "{} vs {}", SHADERS[QUICK].source.len(), SHADERS[FULL].source.len());
    // The stand-in reads the same uniform layout.
    let full = SHADERS[FULL].source;
    let quick = SHADERS[QUICK].source;
    let fields = |s: &str| -> Vec<String> {
        let a = s.find("struct Sea {").unwrap_or(0);
        let b = a + s[a..].find("};").unwrap_or(0);
        s[a..b].lines().map(str::trim).filter(|l| l.contains(':') && !l.starts_with("//")).map(str::to_string).collect()
    };
    assert_eq!(fields(full), fields(quick));
}

/// With the part on, a game that sets no sea draws exactly what it drew
/// before: the sea's pass stays off and the nature pass, its table and its
/// shaders are the ones without the part.
#[test]
fn without_a_sea_nothing_changes() {
    use evani_core::App;
    let mut app = App::new("t").with_module(evani_render::RenderModule::new()).with_module(crate::NatureModule::new());
    app.run_headless(2, 1000.0 / 60.0);
    let g = app.world.res::<evani_render::graph::RenderGraph>();
    assert!(!g.ordered().any(|p| p.name == PASS && p.enabled), "the sea's pass is off");
    drop(g);
    assert_eq!(app.world.res::<SeaSurfaces>().level(), None);
    // The nature table is the rows it was without the part: the 21 rows,
    // plus the coast upgrade's two (2026-11-06; every default off, proved
    // by `the_coast_upgrade_is_off_by_default`), none of them the sea's.
    assert_eq!(crate::params::ROWS, 23);
    // A sea on: the pass comes on.
    app.world.res::<SeaSurfaces>().set(Sea::new(0.0, [0.0, 0.0], 500.0));
    app.run_headless(1, 1000.0 / 60.0);
    let g = app.world.res::<evani_render::graph::RenderGraph>();
    assert!(g.ordered().any(|p| p.name == PASS && p.enabled), "the sea's pass is on");
}

/// The looks the studio's presets use (catalog/procedural/procedural.json
/// lists the same values; the studio test keeps them in step).
pub(crate) fn look(name: &str) -> Vec<(&'static str, ParamValue)> {
    let f = ParamValue::F32;
    let c = ParamValue::Color;
    let mut v: Vec<(&'static str, ParamValue)> = vec![
        ("material.nature.sky.horizon", c([0.58, 0.70, 0.86])),
        ("material.nature.sky.zenith", c([0.13, 0.29, 0.62])),
        ("material.nature.water.deep_color", c([0.62, 0.90, 0.93])),
        ("material.nature.water.clarity", f(1.0)),
        ("material.nature.water.scatter_color", c([0.0, 0.04, 0.075])),
        ("material.nature.water.sss_color", c([0.05, 0.62, 0.55])),
        ("material.nature.water.sss_strength", f(1.6)),
        ("material.nature.water.roughness", f(0.06)),
        ("material.nature.water.reflection", f(1.0)),
        ("material.nature.water.glint", f(1.0)),
        ("material.nature.water.choppiness", f(0.45)),
        ("material.nature.water.whitecap_amount", f(0.6)),
        ("material.nature.water.whitecap_threshold", f(0.55)),
        ("material.nature.water.foam", f(0.9)),
    ];
    let mut put = |n: &'static str, x: ParamValue| {
        if let Some(e) = v.iter_mut().find(|(k, _)| *k == n) {
            e.1 = x;
        } else {
            v.push((n, x));
        }
    };
    match name {
        // The storm presets (storm-v2): a Beaufort 7 rough sea and a
        // Beaufort 10 storm with the mid-size waves raised, foam streaks and
        // spindrift. "old-rough" and "old-storm" are the presets as they
        // were (the default proof draws them with the base and this branch).
        "rough" | "storm-low" | "storm" => {
            let storm = name != "rough";
            put("material.nature.water.wind_speed", f(if storm { 24.0 } else { 15.0 }));
            put("material.nature.water.fetch", f(if storm { 700.0 } else { 250.0 }));
            put("material.nature.water.sea_swell", f(if storm { 1.2 } else { 0.9 }));
            put("material.nature.water.choppiness", f(if storm { 1.0 } else { 0.95 }));
            put("material.nature.water.wave_field", f(1.0));
            put("material.nature.water.breaking", f(if storm { 0.9 } else { 0.7 }));
            put("material.nature.water.mid_waves", f(1.0));
            put("material.nature.water.wave_height", f(if storm { 1.4 } else { 1.7 }));
            put("material.nature.water.foam_streaks", f(if storm { 0.8 } else { 0.35 }));
            put("material.nature.water.spindrift", f(if storm { 1.4 } else { 0.4 }));
            put("material.nature.water.foam_linger", f(if storm { 2.2 } else { 1.6 }));
            put("material.nature.water.whitecap_amount", f(if storm { 1.0 } else { 0.55 }));
            put("material.nature.water.whitecap_threshold", f(if storm { 0.5 } else { 0.25 }));
            put("material.nature.water.roughness", f(if storm { 0.03 } else { 0.07 }));
            put("material.nature.water.scatter_color", c(if storm { [0.02, 0.07, 0.075] } else { [0.0, 0.032, 0.05] }));
            put("material.nature.water.sss_strength", f(if storm { 1.1 } else { 1.5 }));
            put("material.nature.water.glint", f(if storm { 0.25 } else { 0.3 }));
            if storm {
                put("material.nature.water.reflection", f(0.7));
            }
            put("material.nature.sky.horizon", c(if storm { [0.46, 0.44, 0.40] } else { [0.52, 0.60, 0.70] }));
            put("material.nature.sky.zenith", c(if storm { [0.22, 0.24, 0.27] } else { [0.20, 0.30, 0.48] }));
        }
        "old-rough" | "old-storm" => {
            put("material.nature.water.wind_speed", f(if name == "old-rough" { 17.0 } else { 24.0 }));
            put("material.nature.water.fetch", f(if name == "old-rough" { 400.0 } else { 700.0 }));
            put("material.nature.water.sea_swell", f(1.0));
            put("material.nature.water.choppiness", f(0.85));
            put("material.nature.water.whitecap_amount", f(1.1));
            put("material.nature.water.whitecap_threshold", f(0.45));
            put("material.nature.water.roughness", f(0.09));
            put("material.nature.water.scatter_color", c([0.0, 0.03, 0.05]));
            put("material.nature.water.sss_strength", f(2.4));
            put("material.nature.sky.horizon", c([0.52, 0.60, 0.70]));
            put("material.nature.sky.zenith", c([0.20, 0.30, 0.48]));
        }
        "beach" | "old-beach" | "beach-field" => {
            let heavy = name != "old-beach";
            if name == "beach-field" {
                put("material.nature.water.wave_field", f(1.0));
            }
            put("material.nature.water.wind_speed", f(7.0));
            put("material.nature.water.fetch", f(60.0));
            put("material.nature.water.sea_swell", f(0.6));
            put("material.nature.water.beach_slope", f(0.05));
            put("material.nature.water.beach_distance", f(70.0));
            put("material.nature.water.beach_direction", f(0.0));
            put("material.nature.water.breaker_height", f(if heavy { 1.8 } else { 1.4 }));
            put("material.nature.water.breaker_length", f(if heavy { 36.0 } else { 32.0 }));
            put("material.nature.water.sea_depth", f(30.0));
            put("material.nature.water.deep_color", c(if heavy { [0.50, 0.90, 0.86] } else { [0.70, 0.95, 0.94] }));
            put("material.nature.water.scatter_color", c(if heavy { [0.0, 0.07, 0.075] } else { [0.0, 0.085, 0.085] }));
            put("material.nature.water.sss_strength", f(2.6));
            if heavy {
                // Turquoise between torn lace, not a white sheet.
                put("material.nature.water.foam", f(0.55));
                put("material.nature.water.whitecap_amount", f(0.55));
                put("material.nature.water.spray", f(1.6));
                // The whitewater as torn lace, not the bubble net.
                put("material.nature.water.foam_streaks", f(0.2));
            }
        }
        // The library's calm, open and tropical seas drawn by the field sea
        // (Chris's library report, 2026-11-03).
        "open-field" => {
            put("material.nature.water.wave_field", f(1.0));
        }
        "calm-field" => {
            put("material.nature.water.wind_speed", f(5.0));
            put("material.nature.water.fetch", f(60.0));
            put("material.nature.water.sea_swell", f(0.5));
            put("material.nature.water.choppiness", f(0.3));
            put("material.nature.water.whitecap_amount", f(0.15));
            put("material.nature.water.whitecap_threshold", f(0.65));
            put("material.nature.water.wave_field", f(1.0));
        }
        "reef" | "reef-field" => {
            if name == "reef-field" {
                put("material.nature.water.wave_field", f(1.0));
            }
            put("material.nature.water.wind_speed", f(6.0));
            put("material.nature.water.fetch", f(40.0));
            put("material.nature.water.sea_swell", f(0.35));
            put("material.nature.water.wave_height", f(0.8));
            put("material.nature.water.sea_depth", f(3.0));
            put("material.nature.water.choppiness", f(0.3));
            put("material.nature.water.whitecap_amount", f(0.2));
            put("material.nature.water.deep_color", c([0.70, 0.956, 0.942]));
            put("material.nature.water.scatter_color", c([0.0, 0.085, 0.085]));
            put("material.nature.water.sss_strength", f(2.8));
            put("material.nature.water.caustics", f(1.4));
        }
        _ => {}
    }
    v
}

/// For the GPU harness (tools/nature/sea-shots.mjs): with EVANI_SEA_DUMP
/// set to a folder, writes the sea shaders as the engine assembles them and
/// `scenes.json`: per shot the uniform words of each draw (packed by
/// [`uniforms`], so the pictures use the engine's own numbers). Does nothing
/// otherwise.
#[test]
fn dump_for_the_gpu_harness() {
    let Ok(dir) = std::env::var("EVANI_SEA_DUMP") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("sea.wgsl"), SHADERS[FULL].source).unwrap();
    // The field sea's three (tools/nature/sea-shots.mjs draws a shot whose
    // level word has the field bit with them).
    std::fs::write(dir.join("waves.wgsl"), SHADERS[WAVES].source).unwrap();
    std::fs::write(dir.join("field.wgsl"), SHADERS[MESH].source).unwrap();
    std::fs::write(dir.join("fill.wgsl"), SHADERS[FILL].source).unwrap();
    std::fs::write(dir.join("spray.wgsl"), SHADERS[SPRAY].source).unwrap();
    let (base, p) = reg();
    let sun_at = |el: f32, az: f32| {
        let (e, a) = (el.to_radians(), az.to_radians());
        norm([a.cos() * e.cos(), e.sin(), a.sin() * e.cos()])
    };
    // name, look, eye, target, sun elevation/azimuth (deg), time, vertical
    // field of view (deg).
    type Shot<'a> = (&'a str, &'a str, [f32; 3], [f32; 3], [f32; 2], f64, f32);
    // The Ultra-Water storm reference's cameras (read from the demo page by
    // tools/nature/storm-ref.mjs: 50 deg, "Open ocean" 9 m up looking 1.7
    // deg down, "Golden hour" 5 m up 1.0 deg down, "Aerial" 420 m up 18.3
    // deg down) and its sun (17 deg up, dead ahead of the Golden hour view).
    let look_at = |eye: [f32; 3], d: [f32; 3]| [eye[0] + d[0] * 60.0, eye[1] + d[1] * 60.0, eye[2] + d[2] * 60.0];
    let uw_open = look_at([0.0, 9.0, 0.0], [0.188_512, -0.029_629, -0.981_624]);
    let uw_gold = look_at([0.0, 5.0, 0.0], [-0.422_551, -0.017_854, -0.906_163]);
    let uw_air = look_at([0.0, 420.0, 0.0], [-0.671_492, -0.313_363, -0.671_492]);
    let uw_sun = [17.0, -115.0];
    let shots: Vec<Shot> = vec![
        ("calm", "open", [0.0, 7.0, 0.0], [0.0, 3.2, -60.0], [16.0, -95.0], 21.0, 55.0),
        ("calm-sun", "open", [0.0, 9.0, 0.0], [0.0, 5.0, -60.0], [9.0, -90.0], 33.0, 55.0),
        ("rough", "rough", [0.0, 6.0, 0.0], [0.0, 3.0, -60.0], [14.0, -100.0], 47.0, 55.0),
        ("storm-low", "storm-low", [0.0, 4.0, 0.0], [0.0, 3.6, -60.0], [12.0, -80.0], 64.0, 55.0),
        ("beach", "beach", [-20.0, 4.5, 35.0], [45.0, 0.5, -8.0], [24.0, -110.0], 18.0, 55.0),
        ("beach-side", "beach", [55.0, 3.0, 60.0], [20.0, 0.0, -10.0], [30.0, -60.0], 26.0, 55.0),
        ("reef", "reef", [0.0, 6.0, 8.0], [0.0, -2.0, -25.0], [55.0, -70.0], 12.0, 55.0),
        ("top", "rough", [0.0, 40.0, 30.0], [0.0, 0.0, -40.0], [30.0, -90.0], 47.0, 55.0),
        // Storm-v2 against the reference (same cameras and sun).
        ("uw-storm-open", "storm", [0.0, 9.0, 0.0], uw_open, uw_sun, 64.0, 50.0),
        ("uw-storm-golden", "storm", [0.0, 5.0, 0.0], uw_gold, uw_sun, 91.0, 50.0),
        ("uw-storm-aerial", "storm", [0.0, 420.0, 0.0], uw_air, uw_sun, 64.0, 50.0),
        ("uw-rough-open", "rough", [0.0, 9.0, 0.0], uw_open, uw_sun, 47.0, 50.0),
        ("storm-1m5", "storm", [0.0, 1.5, 0.0], [0.0, 1.9, -60.0], [12.0, -80.0], 133.0, 55.0),
        // The presets as they were (the default proof draws these with the
        // base's shader and with this branch's: every pixel the same).
        ("old-rough", "old-rough", [0.0, 6.0, 0.0], [0.0, 3.0, -60.0], [14.0, -100.0], 47.0, 55.0),
        ("old-storm", "old-storm", [0.0, 4.0, 0.0], [0.0, 3.6, -60.0], [12.0, -80.0], 64.0, 55.0),
        ("old-beach", "old-beach", [-20.0, 4.5, 35.0], [45.0, 0.5, -8.0], [24.0, -110.0], 18.0, 55.0),
        ("old-storm-uw", "old-storm", [0.0, 9.0, 0.0], uw_open, uw_sun, 64.0, 50.0),
        // Chris's library report (2026-11-03): a low eye on the horizon and a
        // deck looking down, the open sea drawn by the classic sea and by the
        // field sea, and the two storm looks.
        ("cr-open-hz", "open", [0.0, 3.0, 0.0], [0.0, 2.6, -60.0], [16.0, -95.0], 21.0, 50.0),
        ("cr-open-near", "open", [0.0, 6.0, 0.0], [0.0, -4.0, -60.0], [16.0, -95.0], 21.0, 50.0),
        ("cr-openf-hz", "open-field", [0.0, 3.0, 0.0], [0.0, 2.6, -60.0], [16.0, -95.0], 21.0, 50.0),
        ("cr-openf-near", "open-field", [0.0, 6.0, 0.0], [0.0, -4.0, -60.0], [16.0, -95.0], 21.0, 50.0),
        ("cr-oldstorm-hz", "old-storm", [0.0, 6.0, 0.0], [0.0, 5.6, -60.0], [16.0, -95.0], 64.0, 50.0),
        ("cr-storm-hz", "storm", [0.0, 6.0, 0.0], [0.0, 5.6, -60.0], [16.0, -95.0], 64.0, 50.0),
        ("cr-rough-hz", "rough", [0.0, 6.0, 0.0], [0.0, 5.6, -60.0], [16.0, -95.0], 47.0, 50.0),
        ("cr-calmf-hz", "calm-field", [0.0, 3.0, 0.0], [0.0, 2.6, -60.0], [16.0, -95.0], 21.0, 50.0),
        ("cr-reef", "reef", [0.0, 6.0, 8.0], [0.0, -2.0, -25.0], [55.0, -70.0], 12.0, 55.0),
        ("cr-reeff", "reef-field", [0.0, 6.0, 8.0], [0.0, -2.0, -25.0], [55.0, -70.0], 12.0, 55.0),
        ("cr-beachf", "beach-field", [-20.0, 4.5, 35.0], [45.0, 0.5, -8.0], [24.0, -110.0], 18.0, 55.0),
    ];
    // Eight moments of the two reference views (the metrics average them).
    let moments = [40.0, 77.0, 118.0, 151.0, 190.0, 226.0, 263.0, 297.0];
    let names: Vec<(String, String)> = (0..8).map(|k| (format!("uw-storm-open-m{k}"), format!("uw-storm-golden-m{k}"))).collect();
    let mut shots = shots;
    for (k, (o, g)) in names.iter().enumerate() {
        shots.push((o.as_str(), "storm", [0.0, 9.0, 0.0], uw_open, uw_sun, moments[k], 50.0));
        shots.push((g.as_str(), "storm", [0.0, 5.0, 0.0], uw_gold, uw_sun, moments[k] + 7.0, 50.0));
    }
    // A folding-crest moment of each storm look, side on from a deck: the
    // first moment a tall crest in front of the camera is going over (the
    // CPU mirror of the event field finds it).
    // (Name, look, eye, target, time.)
    type FoldShot<'a> = (String, &'a str, [f32; 3], [f32; 3], f64);
    let fold_found: Vec<FoldShot> = [("fold-storm", "storm"), ("fold-rough", "rough")]
        .iter()
        .filter_map(|&(name, lk)| {
            let mut r = base.clone();
            for (n, v) in look(lk) {
                r.set_by_name(n, v).unwrap();
            }
            let s = p.settings(&r);
            // Looking upwind (to one side of it) at a wave coming on, so its
            // front shows: the lip over the hollow.
            let wd = field::Field::new(&s, &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -60.0], 3.0)).words[34] + core::f32::consts::PI;
            (0..600).find_map(|i| {
                let t = 3.0 + 0.5 * f64::from(i);
                let eye = [0.0, 9.0, 0.0];
                let f = field::Field::new(&s, &frame(3, eye, [wd.cos() * 60.0, 8.0, wd.sin() * 60.0], t));
                if f.eye_under {
                    return None;
                }
                for k in 0..30 {
                    for a in [-6, -5, -4, 4, 5, 6] {
                        let (rr, az) = (40.0 + 3.0 * k as f32, wd + a as f32 * 0.15);
                        let pt = [az.cos() * rr, az.sin() * rr];
                        let ev = f.brk_at(pt);
                        if !(ev[0] > 0.8 && ev[1] < 0.4 && ev[3] > 0.8) {
                            continue;
                        }
                        let wv = f.brk_wave(pt);
                        if wv[2] > 1.9 && wv[0].abs() < 0.3 {
                            println!("{name}: t {t}, yaw {} deg, {rr} m off", pt[1].atan2(pt[0]).to_degrees());
                            return Some((name.to_string(), lk, eye, [pt[0], f.at_rest(pt, 0.0).1 * 0.7, pt[1]], t));
                        }
                    }
                }
                None
            })
        })
        .collect();
    let fold_names: Vec<String> = fold_found.iter().flat_map(|(n, ..)| (0..4).map(move |k| format!("{n}-t{k}"))).collect();
    for (m, (name, lk, eye, target, t)) in fold_found.iter().enumerate() {
        shots.push((name.as_str(), lk, *eye, *target, uw_sun, *t, 50.0));
        // The same crest over four moments: leaning, going over, falling, gone.
        for k in 0..4 {
            shots.push((fold_names[m * 4 + k].as_str(), lk, *eye, *target, uw_sun, *t - 1.5 + 1.5 * k as f64, 50.0));
        }
    }
    let shots: Vec<Shot> = match std::env::var("EVANI_SEA_DUMP_SHOTS") {
        Ok(only) => shots.into_iter().filter(|s| only.split(',').any(|n| n == s.0)).collect(),
        Err(_) => shots,
    };
    let mut out = Vec::new();
    for (name, lk, eye, target, sun, time, fov) in shots {
        let mut r = base.clone();
        for (n, v) in look(lk) {
            r.set_by_name(n, v).unwrap();
        }
        let s = p.settings(&r);
        // A low eye, or an eye in the storm (the studio's deck camera rides
        // over a crest that would cover it): the next moment no wave stands
        // within a metre of it at any level (the view from inside a wave is
        // its own, flat picture).
        let time = if eye[1] < 3.0 || ((lk == "storm" || lk == "rough") && eye[1] < 20.0) {
            let h = |l: usize, t: f64| {
                let fr = frame(l, eye, target, t);
                if field::field_on(&s) { field::Field::new(&s, &fr).height_at([eye[0], eye[2]], 0.0) } else { Mirror::new(&uniforms(&s, &fr)).height([eye[0], eye[2]], 0.05) }
            };
            let clear = |t: f64| (1..4).all(|l| h(l, t) < eye[1] - 1.0);
            (0..20).map(|i| time + 1.3 * f64::from(i)).find(|t| clear(*t)).unwrap_or(time)
        } else {
            time
        };
        let mut levels = Vec::new();
        for level in 0..4 {
            let mut fr = frame(level, eye, target, time);
            fr.camera.fov_y_deg = fov;
            fr.sun = sun_at(sun[0], sun[1]);
            fr.size = [1600.0, 900.0];
            levels.push(uniforms(&s, &fr).to_vec());
        }
        // The reference's views (and the low storm view) show open sea only:
        // no bed and no rocks, as in the reference.
        let open = name.starts_with("uw-") || name.starts_with("fold-") || name == "old-storm-uw" || name == "storm-1m5";
        let bed = if lk.ends_with("beach") || lk == "beach-field" {
            "sand"
        } else if lk.starts_with("reef") {
            "reef"
        } else if open {
            "open"
        } else {
            "deep"
        };
        // The water as it was (the nature pass, an Ocean preset of the
        // studio, High's refracting draw) from the same camera: the
        // "before" of a matched pair.
        let old = match lk {
            "rough" | "storm-low" | "storm" | "old-rough" | "old-storm" => "storm",
            "reef" => "tropical",
            _ => "calm",
        };
        let mut fr = frame(3, eye, target, time);
        fr.camera.fov_y_deg = fov;
        fr.sun = sun_at(sun[0], sun[1]);
        let (body, words) = old_water_words(old, &fr);
        // The old refracting shader spliced with that preset's table.
        let mut slots = evani_core::slots::ShaderSlots::with_builtins();
        let id = slots.declare(crate::params::SLOT);
        slots.set_body(id, &body).unwrap();
        let mut src = String::new();
        for line in crate::effect::SHADERS[crate::effect::REFRACT].source.replace("\r\n", "\n").lines() {
            match line.trim().strip_prefix("//@slot ") {
                Some(n) => src.push_str(&slots.wgsl_function(slots.id(n.trim()).unwrap()).unwrap()),
                None => src.push_str(line),
            }
            src.push('\n');
        }
        std::fs::write(dir.join(format!("old-{old}.wgsl")), src).unwrap();
        out.push(serde_json::json!({"name": name, "look": lk, "bed": bed, "levels": levels, "old": old, "old_words": words}));
    }
    // The worst storm for the bottoming-out check on the GPU: opaque water
    // over a magenta bed 2 m down, so any bed pixel that shows is a hole.
    // The worst storm, as before (18 views: 6 cameras at 3 levels), and the
    // same with the storm settings at their tops (mid-size waves x2.5,
    // full streaks and spindrift), from those cameras and a 1.5 m low one
    // in the troughs: 21 more.
    let mut worst = Vec::new();
    for storm in [false, true] {
        let mut r = base.clone();
        for (n, v) in [("wind_speed", 30.0), ("fetch", 1000.0), ("wave_height", 4.0), ("sea_swell", 3.0), ("sea_depth", 2.0), ("clarity", 0.05)] {
            set(&mut r, n, v);
        }
        if storm {
            for (n, v) in [("mid_waves", 2.5), ("foam_streaks", 1.0), ("spindrift", 2.5)] {
                set(&mut r, n, v);
            }
        }
        r.set_by_name("material.nature.water.choppiness", ParamValue::F32(1.0)).unwrap();
        r.set_by_name("material.nature.water.deep_color", ParamValue::Color([0.05, 0.05, 0.05])).unwrap();
        let s = p.settings(&r);
        let mut views: Vec<(f32, f32, f32, f64)> = vec![(0.8, 0.0, -2.0, 3.0), (3.0, 1.0, -5.0, 40.0), (6.0, 2.0, -1.0, 77.0), (15.0, 3.0, -12.0, 131.0), (30.0, 4.5, -35.0, 222.0), (2.0, 5.5, -0.5, 290.0)];
        if storm {
            views.push((1.5, 0.7, 1.0, 163.0));
        }
        for (k, (h, yaw, pitch, time)) in views.into_iter().enumerate() {
            let pitch = pitch.to_radians();
            let eye = [0.0, h, 0.0];
            let target = [yaw.cos() * pitch.cos() * 50.0, h + pitch.sin() * 50.0, yaw.sin() * pitch.cos() * 50.0];
            for level in 1..4 {
                let mut fr = frame(level, eye, target, time);
                fr.size = [800.0, 450.0];
                let name = if storm { format!("worst-storm-{k}-l{level}") } else { format!("worst-{k}-l{level}") };
                worst.push(serde_json::json!({"name": name, "bed": "magenta", "words": uniforms(&s, &fr).to_vec()}));
            }
        }
    }
    std::fs::write(dir.join("scenes.json"), serde_json::to_string_pretty(&serde_json::json!({"shots": out, "worst": worst})).unwrap()).unwrap();
}

/// At the presets' settings, from the studio's cameras and lower (down to
/// 1.5 m over a rough sea, rays to within 0.8 deg of the horizon), the
/// march meets the first crest along almost every ray: a crest skipped by
/// more than a pixel on screen (a later wave drawn where a near crest
/// should stand) stays under 5 % on High and Ultra and 6 % on Medium (all
/// of them at the grazing angles; none on the open-sea preset).
#[test]
fn the_march_finds_the_first_crest() {
    let (base, p) = reg();
    // (The classic sea's presets: the storm presets and the heavy beach
    // draw the field sea, which has no march.)
    for lk in ["open", "old-rough", "old-storm", "old-beach"] {
        let mut r = base.clone();
        for (n, v) in look(lk) {
            r.set_by_name(n, v).unwrap();
        }
        let s = p.settings(&r);
        assert!(!field::field_on(&s), "{lk} is a classic sea");
        for level in 1..=3 {
            let (mut hits, mut skipped) = (0, 0);
            for &(h, time) in &[(1.5, 5.0), (4.0, 61.0), (9.0, 130.0), (20.0, 222.0)] {
                let u = uniforms(&s, &frame(level, [0.0, h, 0.0], [0.0, h, -10.0], time));
                let m = Mirror::new(&u);
                if m.eye_under([0.0, h, 0.0]) {
                    continue;
                }
                for yaw in 0..16 {
                    let a = yaw as f32 * core::f32::consts::TAU / 16.0;
                    for pitch in [-0.8f32, -2.0, -4.0, -8.0, -15.0, -30.0] {
                        let pr = pitch.to_radians();
                        let rd = [a.cos() * pr.cos(), pr.sin(), a.sin() * pr.cos()];
                        let ro = [0.0, h, 0.0];
                        let t = m.march(ro, rd, 3000.0);
                        if t <= 0.0 {
                            continue;
                        }
                        hits += 1;
                        let mut first = t;
                        let mut q = 0.0;
                        while q < t {
                            if m.gap(ro, rd, q) <= 0.0 {
                                first = q;
                                break;
                            }
                            q += (0.004 * q).max(0.04);
                        }
                        if first < t - (0.2 + 0.01 * t) {
                            let mut deep = 0.0f32;
                            let mut q = first;
                            while q < t {
                                deep = deep.max(-m.gap(ro, rd, q));
                                q += (0.004 * q).max(0.04);
                            }
                            if deep / (first * u[19]) > 1.0 {
                                skipped += 1;
                            }
                        }
                    }
                }
            }
            let limit = if level == 1 { 6 } else { 5 };
            eprintln!("first crest: {lk} level {level}: {skipped} of {hits} skipped");
            assert!(skipped * 100 <= hits * limit, "{lk} level {level}: {skipped} of {hits} rays skipped a crest by over a pixel");
        }
    }
}


/// The nature pass's water (the Ocean item's preset `preset` from the
/// studio's catalogue) seen from `fr`'s camera: its material table's slot
/// body and its refracting draw's uniform words, as the engine makes them.
fn old_water_words(preset: &str, fr: &FrameIn) -> (String, Vec<f32>) {
    use crate::effect::{Mode, NatureEffect};
    use crate::surface::{Kind, NatureSurfaces, Surface};
    let lib: serde_json::Value = serde_json::from_str(include_str!("../../../../catalog/procedural/procedural.json")).unwrap();
    let ocean = lib["items"].as_array().unwrap().iter().find(|i| i["id"] == "ocean").unwrap();
    let mut reg = ParamRegistry::new();
    let p = crate::NatureParams::register(&mut reg).unwrap();
    let pre = ocean["presets"].as_array().unwrap().iter().find(|x| x["id"] == preset).unwrap();
    for src in [&ocean["base"], &pre["params"]] {
        for (n, v) in src.as_object().unwrap() {
            let Some(id) = reg.id(n) else { continue };
            let val = match v {
                serde_json::Value::Array(a) => ParamValue::Color([0, 1, 2].map(|i| a[i].as_f64().unwrap() as f32)),
                x => ParamValue::F32(x.as_f64().unwrap() as f32),
            };
            reg.set(id, val).unwrap();
        }
    }
    let ns = NatureSurfaces::default();
    let st = &ns.0;
    st.camera.set(fr.camera);
    st.sun.set(fr.sun);
    st.sun_col.set(fr.sun_col);
    st.time.set(fr.time as f32);
    st.sky_tint.set(crate::pack_sky_tint(crate::sky_tint(&p.table(&reg), None)));
    ns.set(&[Surface::new(Kind::WaterRect, [0.0, 0.0, 0.0], [900.0, 0.0, 900.0])]);
    ns.plan(40);
    st.mode.set(Mode::Refract);
    let fx = NatureEffect { surfaces: ns.clone() };
    let mut out = [0.0f32; 64];
    let f = UniformFrame { source_size: [1600.0, 900.0], target_size: [1600.0, 900.0], mip: 0 };
    let n = fx.write_uniforms("nature_refract", &reg, &f, &mut out);
    assert_eq!(n, 64);
    (crate::params::body(&p.table(&reg)), out.to_vec())
}

// ---- the field sea (src/sea/field.rs) -------------------------------------------

/// The checked-in field table is the generator's output.
/// `EVANI_SEA_TABLE_WRITE=1` rewrites it.
#[test]
fn the_field_table_file_matches_its_generator() {
    let want = spectrum::field_table_wgsl(&spectrum::field_table());
    if std::env::var("EVANI_SEA_TABLE_WRITE").is_ok() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders/sea_field_table.wgsl");
        std::fs::write(path, &want).unwrap();
        return;
    }
    assert_eq!(field::FIELD_TABLE_WGSL.replace("\r\n", "\n"), want, "shaders/sea_field_table.wgsl is stale: run this test with EVANI_SEA_TABLE_WRITE=1");
    assert_eq!(field::table().len(), spectrum::FIELD_N);
}

/// A storm: the worst settings the sliders allow, or the storm preset's.
fn field_reg(worst: bool) -> (ParamRegistry, SeaParams) {
    let (mut r, p) = reg();
    let vals: &[(&str, f32)] = if worst {
        &[("wind_speed", 30.0), ("fetch", 1000.0), ("wave_height", 4.0), ("sea_swell", 3.0), ("mid_waves", 2.5), ("spindrift", 2.5), ("foam_streaks", 1.0)]
    } else {
        &[("wind_speed", 24.0), ("fetch", 700.0), ("wave_height", 1.4), ("sea_swell", 1.2), ("mid_waves", 1.0), ("wave_field", 1.0)]
    };
    for (n, v) in vals {
        set(&mut r, n, *v);
    }
    r.set_by_name("material.nature.water.choppiness", ParamValue::F32(1.0)).unwrap();
    (r, p)
}

#[test]
fn the_field_sea_is_opt_in() {
    let (r, p) = reg();
    assert!(!field::field_on(&p.settings(&r)), "a sea that doesn't opt in draws the classic sea");
    for n in ["wave_field", "mid_waves", "foam_streaks", "spindrift", "breaking"] {
        let mut r2 = r.clone();
        set(&mut r2, n, 1.0);
        assert!(field::field_on(&p.settings(&r2)), "{n} turns the field sea on");
    }
    // Without it, every draw is the classic one: copy, stand-in, full.
    assert_eq!(SHADERS[MESH].name, "nature_sea_field");
}

#[test]
fn the_field_words_pack_the_squeeze_and_the_bands() {
    let (r, p) = field_reg(false);
    let s = p.settings(&r);
    for level in 0..4 {
        let f = field::Field::new(&s, &frame(level, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 77.0));
        let word = f.words[27];
        assert_eq!(word, word.round(), "the level word is a whole number");
        let w = word as u32;
        assert_eq!((w & 3) as usize, level);
        assert!(w >> 2 & field::FIELD_BIT != 0, "the field bit is set");
        assert!(w < 1 << 24, "exact in f32");
        assert_eq!(f.words[38], f.lam);
        assert_eq!(f.words[39], f.hi0);
        assert!(f.lam > 0.0 && f.lam * f.slope_var.sqrt() <= field::FOLD_CAP + 1e-4, "the squeeze is held: {} x {}", f.lam, f.slope_var.sqrt());
        assert!(f.lamf >= f.lam);
        // The longest wave fits band 0 (its tile holds 8 of them).
        let (kp, ks) = (f.words[32], f.words[35]);
        let t = field::table();
        let lw = TAU / (kp * t[0].xx);
        let ls = TAU / (ks * t[spectrum::FIELD_WIND].xx);
        assert!(f.hi0 >= lw.max(ls) && f.hi0 < 1.25 * 1.02 * lw.max(ls), "hi0 {} for {} / {}", f.hi0, lw, ls);
        assert!(f.words[30] >= 0.0);
    }
}

/// The CPU numbers the shaders hard-code are the module's.
#[test]
fn the_field_layout_matches_the_shaders() {
    let bands = include_str!("../../shaders/sea_field_bands.wgsl");
    let mesh = include_str!("../../shaders/sea_field.wgsl");
    let has = |src: &str, s: &str| assert!(src.contains(s), "the shader has `{s}`");
    has(bands, &format!("const FB: u32 = {}u;", field::BANDS));
    has(bands, &format!("const FN: u32 = {}u;", field::FN));
    has(bands, &format!("const FT: u32 = {}u;", field::FT));
    has(bands, &format!("const F_TILE: f32 = {:.1};", field::TILE));
    has(bands, &format!("const F_INFO_Y: u32 = {}u;", field::BANDS * field::FT));
    has(include_str!("../../shaders/sea_field.wgsl"), &format!("const FD_LONG: u32 = {}u;", field::SQUEEZE_LONG));
    has(bands, &format!("const F_W: f32 = {}.0;", field::FIELD_SIZE[0]));
    has(bands, &format!("const F_H: f32 = {}.0;", field::FIELD_SIZE[1]));
    has(bands, "const F_RHO: f32 = 2.8284271;");
    let jit: Vec<String> = field::JIT.iter().map(|j| format!("{j:?}")).collect();
    has(bands, &format!("array<f32, 8>({})", jit.join(", ")));
    let bpl: Vec<String> = field::BANDS_PER_LEVEL.iter().map(|b| format!("{b}u")).collect();
    has(bands, &format!("array<u32, 4>({})", bpl.join(", ")));
    let grid: Vec<String> = field::GRID.iter().map(|(r, s)| format!("vec2<u32>({r}u, {s}u)")).collect();
    has(mesh, &format!("array<vec2<u32>, 4>({})", grid.join(", ")));
    for level in 0..4 {
        let (r, s) = field::GRID[level];
        assert_eq!(field::vertices(level), 6 * (r - 1) * s);
    }
    has(mesh, &format!("const FD_CMAX: f32 = {:?};", field::SQUEEZE_CMAX));
    has(mesh, &format!("const FD_CMIN: f32 = {:?};", field::SQUEEZE_CMIN));
}

/// Every patch of foam has a cause: folding foams only on a crest (now, or a
/// moment ago when the water then stood on one), the gale's streaks only shape
/// foam a break left, and a break's foam lies only where a tall crest went over
/// (no round patch laid whatever stood there). The foam view (dev builds)
/// paints anything else yellow; tools/nature/sea-foam-view.mjs counts it.
#[test]
fn every_patch_of_foam_has_a_cause() {
    let mesh = include_str!("../../shaders/sea_field.wgsl");
    let brk = include_str!("../../shaders/sea_field_break.wgsl");
    for needle in ["let now = fold_now * crest_now;", "let was = fold_was * crest_was;", "let crest_was = select(crest_now, smoothstep(0.0, 0.5, sf.h_was / hs0), level >= 2);", "brk_foam_src(p3.xz, chance, pr,"] {
        assert!(mesh.contains(needle), "sea_field.wgsl: {needle}");
    }
    assert!(!mesh.contains("smoothstep(0.7, 0.95, streak) * u.foam.x"), "no foam streak from noise alone");
    assert!(!brk.contains("fn brk_foam("), "no round foam patch laid whatever stood there");
    assert!(brk.contains("let tall = smoothstep(0.9 - 0.5 * a, 1.7 - 0.7 * a, brk_wave(gw, pr, nb).z);") && brk.contains("let caused = tall *"), "a break's foam needs a crest that stood tall");
    // The height a moment ago, for the foam carried from then.
    assert!(include_str!("../../shaders/sea_waves.wgsl").contains("return vec4<f32>(jw, lap, hw, 0.0);"));
}

/// The foam view is a dev tool: without `capture` the param isn't there and the
/// shader carries only the no-op; with it, the switch rides the spray word's
/// bit 16 and leaves the breaking chance as it was.
#[test]
fn the_foam_view_is_dev_only() {
    let (r, p) = field_reg(false);
    let s = p.settings(&r);
    let off = field::Field::new(&s, &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 5.0));
    assert_eq!((off.words[31] as u32) & field::FOAM_DEBUG_BIT, 0);
    let mut on = s;
    on.foam_debug = 1.0;
    on.breaking = 0.8;
    let f = field::Field::new(&on, &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 5.0));
    assert_ne!((f.words[31] as u32) & field::FOAM_DEBUG_BIT, 0);
    assert!(f.words[31] < 16_777_216.0, "exact in f32");
    let mut plain = on;
    plain.foam_debug = 0.0;
    assert_eq!(f.brk_chance(), field::Field::new(&plain, &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 5.0)).brk_chance());
    assert!(include_str!("../../shaders/sea_field_break.wgsl").contains(">> 8u) & 255u) * 0.01"), "the shader masks the switch off the chance");
    let mesh = SHADERS[MESH].source;
    if cfg!(feature = "capture") {
        assert!(r.id(super::FOAM_DEBUG).is_some());
        assert!(mesh.contains("F_DBG * 5.0"));
    } else {
        assert!(r.id(super::FOAM_DEBUG).is_none(), "no foam view param in ship builds");
        assert!(!mesh.contains("F_DBG * 5.0"), "no foam view code in ship builds");
    }
}

/// Fine bands never alias along the view (the dotted rows of streaks a
/// grazing view showed) and keep their detail across it (round 6 r1: the
/// earlier fix faded every band by a stretched footprint and blurred the fine
/// ripples away). Each band's slope is split along and across the view: the
/// part along the view fades by the footprint along the view (the plain one
/// times the stretch, read off the tilt of the surface the longer bands make:
/// no mesh facets, and a slope facing the eye isn't stretched), the part
/// across by the plain footprint;
/// a band read once (no two reads along the view) keeps its slope along the
/// view only while it is more than a footprint long there, and across the view
/// the footprint grows only with the stretch's 0.35 power (at most 6:1 along to
/// across: no brushed grain down the view). The shader carries the same numbers. Pictures:
/// tools/nature/sea-speckle.mjs and the streak before/after set.
#[test]
fn fine_bands_never_alias_along_the_view() {
    let wgsl = include_str!("../../shaders/sea_field.wgsl");
    for needle in [
        "let stretch = min(1.0 / max(abs(dot(normalize(vec3<f32>(-gs.x, 1.0, -gs.y)), rd)), 0.06), 16.0);",
        "let fpf = fpc * stretch;",
        "let wc = 1.0 - smoothstep(0.35, 0.9, fpc * pow(stretch, 0.35) / lo);",
        "let wf = 1.0 - smoothstep(0.35, 0.9, fpf / lo);",
        "let two = wf < wc - 0.02;",
        "let off = fdir * (0.25 * fpf);",
        "g0 = g0 + gr * (fdir * (wf * dot(gb, fdir)) + fs * (wc * dot(gb, fs)));",
        "let sf = fd_surface(p0, q, rd, fdir, fp_iso);",
    ] {
        assert!(wgsl.contains(needle), "sea_field.wgsl: {needle}");
    }
    let ss = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    for i in 0..=200 {
        let stretch = 1.0 + 0.1 * i as f32;
        let fpf = stretch.min(16.0);
        for j in 1..400 {
            let lo = 0.05 * j as f32;
            let fpc = stretch.min(16.0).powf(0.35);
            let wc = 1.0 - ss(0.35, 0.9, fpc / lo);
            let wf = 1.0 - ss(0.35, 0.9, fpf / lo);
            let two = wf < wc - 0.02;
            // (A band under 2 % of its weight is as good as gone.)
            if wf > 0.02 && !two {
                assert!(lo >= 1.1 * fpf - 1e-3, "stretch {stretch}: a band of {lo} footprints read once along a {fpf} footprint");
            }
            // Across the view the footprint grows at most 2.7x (a band the
            // plain footprint keeps fully, a quarter of the way into the fade,
            // keeps more than half its slope across the view at any stretch).
            assert!(fpc <= 2.7 && fpf / fpc <= 6.1);
            if 1.0 / lo <= 0.35 * 0.38 {
                assert!(wc > 0.5, "stretch {stretch}: a band of {lo} footprints keeps {wc} across");
            }
        }
    }
}

/// The mesh never folds over itself beside a storm crest: the water a ring of
/// rest points carries (the CPU mirror of the vertex squeeze, every band) keeps
/// its order, round a storm's tallest crests and in front of a low eye
/// (round 6 r2: one cap read off every band, times the long waves' tens of
/// metres of squeeze, stacked the near rings into shelves).
#[test]
fn the_storm_mesh_never_folds_beside_a_crest() {
    let (mut r, p) = reg();
    for (n, v) in look("storm") {
        r.set_by_name(n, v).unwrap();
    }
    r.set_by_name("material.nature.water.wave_height", ParamValue::F32(4.13)).unwrap();
    let s = p.settings(&r);
    let mut folds = 0;
    let mut cells = 0;
    for t in [64.0, 141.0, 297.0] {
        let f = field::Field::new(&s, &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -60.0], t));
        let step = 0.75;
        for i in 0..80 {
            for j in 0..80 {
                let p0 = [-30.0 + step * i as f32, -60.0 + step * j as f32];
                let at = |x: [f32; 2]| {
                    let (d, _) = f.at_rest(x, 0.0);
                    [x[0] + d[0], x[1] + d[1]]
                };
                let (a, b, c) = (at(p0), at([p0[0] + step, p0[1]]), at([p0[0], p0[1] + step]));
                let det = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                cells += 1;
                if det <= 0.0 {
                    folds += 1;
                }
            }
        }
    }
    assert!(folds * 1000 <= cells, "{folds} of {cells} cells fold over");
}

/// The mesh's squeeze cap: a light squeeze is kept, the strongest axis never
/// folds the mesh over, and water bunched from every side at once eases off
/// sooner than along one crest (a rounded ridge, not a pyramid).
#[test]
fn the_squeeze_cap_keeps_light_squeezes_and_never_folds() {
    assert!(field::squeeze_cap([0.0; 3]) == 1.0);
    assert!(field::squeeze_cap([0.1, 0.0, 0.0]) > 0.99, "a light squeeze along one axis is kept");
    assert!(field::squeeze_cap([-0.8, -0.8, 0.0]) == 1.0, "a stretch (trough) is never capped");
    for i in 0..200 {
        let e = 0.02 * i as f32;
        for &(a, b, c) in &[(e, 0.0, 0.0), (e, e, 0.0), (e, 0.3 * e, 0.4 * e), (0.5 * e, 0.5 * e, 0.5 * e)] {
            let s = field::squeeze_cap([a, b, c]);
            let (mean, dev) = (0.5 * (s * a + s * b), (0.25 * (s * a - s * b) * (s * a - s * b) + s * s * c * c).sqrt());
            assert!(mean + dev < field::SQUEEZE_CMAX + 1e-4, "kept squeeze {} past the cap at {a} {b} {c}", mean + dev);
            assert!(1.0 - (mean + dev) > 0.3, "the mesh never folds");
        }
    }
    let ridge = field::squeeze_cap([0.5, 0.0, 0.0]);
    let cone = field::squeeze_cap([0.5, 0.5, 0.0]);
    assert!(cone < 0.6 * ridge, "bunched from every side eases off sooner: ridge {ridge} cone {cone}");
}

/// However tall the waves are set and however shallow the water, the
/// field sea's troughs stay well over the bed (the mesh also clamps every
/// vertex 15 cm over it).
#[test]
fn field_troughs_never_reach_the_bed() {
    let (r0, p) = field_reg(true);
    for depth in [0.5, 2.0, 6.0, 20.0, 60.0, 200.0] {
        let mut r = r0.clone();
        set(&mut r, "sea_depth", depth);
        let s = p.settings(&r);
        for level in 1..4 {
            for k in 0..4 {
                let f = field::Field::new(&s, &frame(level, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 11.0 + 61.7 * f64::from(k)));
                let (lo, _) = f.extremes([37.0 * k as f32, -91.0], 600.0, 3.0);
                assert!(lo > -0.45 * depth - 1e-3, "depth {depth} level {level}: the deepest trough {lo} m");
                assert!(f.height_at([5.0, -40.0], 0.0) >= 0.15 - depth - 1e-3);
            }
        }
    }
}

/// The squeeze sharpens the crests without folding the surface through
/// itself except at the odd breaking tip (no spikes).
#[test]
fn field_crests_fold_rarely() {
    let (r, p) = field_reg(false);
    let s = p.settings(&r);
    let f = field::Field::new(&s, &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 40.0));
    let (mut folded, mut n) = (0, 0);
    for i in 0..60 {
        for j in 0..60 {
            n += 1;
            if f.folding([i as f32 * 4.1, j as f32 * 3.7 - 300.0]) < 0.0 {
                folded += 1;
            }
        }
    }
    let share = folded as f32 / n as f32;
    eprintln!("storm: Hs {:.1} m, squeeze {:.2}, rms slope {:.3}, folded {:.2} %", 4.0 * f.hrms, f.lam, f.slope_var.sqrt(), 100.0 * share);
    assert!(share < 0.03, "{:.2} % of the surface folds through itself", 100.0 * share);
}

/// The harness looks are the studio presets (catalog/procedural/procedural.json).
#[test]
fn the_harness_looks_are_the_studio_presets() {
    let lib: serde_json::Value = serde_json::from_str(include_str!("../../../../catalog/procedural/procedural.json")).unwrap();
    let ocean = lib["items"].as_array().unwrap().iter().find(|i| i["id"] == "ocean").unwrap();
    let (base, p) = reg();
    let put = |reg: &mut ParamRegistry, n: &str, v: &serde_json::Value| {
        let Some(id) = reg.id(n) else { return };
        let val = match v {
            serde_json::Value::Array(a) => ParamValue::Color([0, 1, 2].map(|i| a[i].as_f64().unwrap() as f32)),
            x => ParamValue::F32(x.as_f64().unwrap() as f32),
        };
        reg.set(id, val).unwrap();
    };
    for (lk, preset) in [("storm", "storm"), ("rough", "rough"), ("beach", "beach")] {
        let mut a = base.clone();
        for (n, v) in look(lk) {
            a.set_by_name(n, v).unwrap();
        }
        let mut b = base.clone();
        for (n, v) in ocean["base"].as_object().unwrap() {
            put(&mut b, n, v);
        }
        let pr = ocean["presets"].as_array().unwrap().iter().find(|x| x["id"] == preset).unwrap();
        for (n, v) in pr["params"].as_object().unwrap() {
            put(&mut b, n, v);
        }
        assert_eq!(p.settings(&a), p.settings(&b), "look {lk} against preset {preset}");
    }
}

/// Some waves break, not all: more as the wind rises, none with breaking at
/// 0, the same on every run, and they loop with the sea.
#[test]
fn some_crests_break_more_with_the_wind() {
    let (base, p) = reg();
    let share = |wind: f32, amount: f32, t: f64| {
        let mut r = base.clone();
        for (n, v) in [("wind_speed", wind), ("fetch", 400.0), ("wave_field", 1.0), ("breaking", amount)] {
            set(&mut r, n, v);
        }
        let f = field::Field::new(&p.settings(&r), &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], t));
        let (mut on, mut all) = (0, 0);
        for i in 0..200 {
            for j in 0..200 {
                all += 1;
                if f.brk_at([i as f32 * 11.3 - 1100.0, j as f32 * 9.7 - 970.0])[0] > 0.0 {
                    on += 1;
                }
            }
        }
        on as f32 / all as f32
    };
    let mean = |wind: f32, amount: f32| (0..6).map(|k| share(wind, amount, 7.0 + 41.0 * f64::from(k))).sum::<f32>() / 6.0;
    let (calm, rough, storm) = (mean(6.0, 1.0), mean(15.0, 1.0), mean(25.0, 1.0));
    eprintln!("breaking share of the sea: 6 m/s {calm:.3}, 15 m/s {rough:.3}, 25 m/s {storm:.3}");
    assert_eq!(calm, 0.0, "a light breeze breaks nothing");
    assert!(rough > 0.02 && storm > 1.3 * rough, "more as the wind rises: {rough} then {storm}");
    assert!(storm < 0.75, "not all of them: {storm}");
    assert_eq!(mean(25.0, 0.0), 0.0, "breaking 0: none");
    assert_eq!(share(25.0, 1.0, 40.0), share(25.0, 1.0, 40.0), "deterministic");
    let (a, b) = (share(25.0, 1.0, 40.0), share(25.0, 1.0, 340.0));
    assert!((a - b).abs() < 1e-5, "it loops with the sea (300 s): {a} and {b}");
}

/// The spray's and the breaking's numbers are in the shaders' text.
#[test]
fn a_breaking_lip_truly_overhangs() {
    // Along a wave's section (a sine of height `env`, length `lm`) with the fold
    // at its fullest: points further ahead at rest must end up behind the lip's
    // tip and under it. The lip is a modest part of the wave, never a long sheet.
    for (env, lm) in [(4.0f32, 52.0f32), (13.6, 130.0), (1.0, 20.0)] {
        let ev = [1.0, 0.0, 1.0, 1.0];
        let k = core::f32::consts::TAU / lm;
        let pts: Vec<[f32; 2]> = (-400..=400)
            .map(|i| {
                let xf = i as f32 / 400.0 * 0.5 * lm;
                let sh = field::brk_fold(xf, env, 1.0, ev, lm, 1.0);
                [xf + sh[0], env * (k * xf).cos() + sh[1]]
            })
            .collect();
        // The tip: furthest forward of the water that started by the crest.
        let tip = (320..440).fold(320, |t, i| if pts[i][0] > pts[t][0] { i } else { t });
        let under = pts[tip..].iter().fold(0.0f32, |u, p| u.max(pts[tip][0] - p[0]));
        let lip = (1.5 * env).min(0.12 * lm);
        assert!(under > 0.3 * lip, "env {env}, lm {lm}: the top overhangs by only {under} m of a {lip} m lip");
        // Water under the tip stands below it: a hollow, not a fold of the sheet on itself.
        let below = pts[tip..].iter().filter(|p| p[0] < pts[tip][0]).map(|p| pts[tip][1] - p[1]).fold(0.0f32, f32::max);
        assert!(below > 0.4 * lip, "env {env}, lm {lm}: only {below} m of hollow under the tip");
        let reach = pts.iter().zip(-400..=400).map(|(p, i)| p[0] - i as f32 / 400.0 * 0.5 * lm).fold(0.0f32, f32::max);
        assert!(reach <= lip * 1.01, "the push is the lip's length ({reach} m)");
        assert!(lip <= 0.12 * lm + 1e-3 && lip <= 1.5 * env + 1e-3);
    }
    // The shader carries the same numbers.
    let wgsl = include_str!("../../shaders/sea_field_break.wgsl");
    for needle in ["min(1.5 * env, 0.12 * pr.lm)", "max(0.45 * l, 0.02 * pr.lm)", "smoothstep(-0.2 * pr.lm, 0.0, xf)", "0.2 * top + 0.55 * drop", "smoothstep(xa + w, xa + w + 0.12 * pr.lm, xf)", "0.35 * env * s * ev.x", "0.8 * env * s * ev.y"] {
        assert!(wgsl.contains(needle), "brk_fold in the shader: {needle}");
    }
}

#[test]
fn the_mesh_goes_round_the_water_under_the_eye() {
    // At a storm's squeeze the water under a low eye has come metres from its
    // rest point; rings centred on the eye's own (x, z) left a hole in front.
    let (mut r, p) = reg();
    for (n, v) in look("storm") {
        r.set_by_name(n, v).unwrap();
    }
    let s = p.settings(&r);
    let mut far = 0.0f32;
    for i in 0..40 {
        let f = field::Field::new(&s, &frame(3, [0.0, 1.5, 0.0], [0.0, 1.5, -30.0], 3.0 + 7.3 * f64::from(i)));
        assert!(f.centre_miss([0.0, 0.0]) < 0.5, "moment {i}: the centre lands {} m from the eye", f.centre_miss([0.0, 0.0]));
        let w = (f.words[42] + 0.5) as u32;
        far = far.max(((w & 4095) as f32 - 2048.0).abs() * 0.0625);
    }
    assert!(far > 0.5, "the squeeze does carry the water (at most {far} m here)");
    let low = field::Field::new(&s, &frame(0, [0.0, 1.5, 0.0], [0.0, 1.5, -30.0], 3.0));
    assert_eq!(low.words[42], (2048 + 4096 * 2048) as f32, "Low has no squeeze");
}

#[test]
fn the_breaking_layout_matches_the_shaders() {
    let spray = include_str!("../../shaders/sea_spray.wgsl");
    let grid: Vec<String> = field::SPRAY.iter().map(|(g, n)| format!("vec2<u32>({g}u, {n}u)")).collect();
    assert!(spray.contains(&format!("array<vec2<u32>, 4>({})", grid.join(", "))), "sp_grid is field::SPRAY");
    assert_eq!(field::spray_vertices(0), 0, "Low has no splash");
    // Spray and breaking share a word, whole numbers exact in f32.
    let (mut r, p) = reg();
    set(&mut r, "breaking", 0.8);
    set(&mut r, "spray", 1.6);
    set(&mut r, "wind_speed", 25.0);
    let f = field::Field::new(&p.settings(&r), &frame(3, [0.0, 9.0, 0.0], [0.0, 8.0, -30.0], 5.0));
    assert_eq!(f.words[31], (160 + 256 * 80) as f32);
}

// ---- Ocean comparison rig (tools/nature/compare.mjs on branch ocean-compare) ----
// compare.mjs appends this test to a throwaway checkout's
// crates/evani-material-nature/src/sea/tests.rs (never to a working copy)
// and runs it once: with EVANI_CMP_VIEWS set to the shared views file
// (tools/nature/compare-views.json) and EVANI_SEA_DUMP to a folder, it writes
// the sea shader and `scenes.json` in the GPU harness's own format
// (tools/nature/sea-shots.mjs): one shot per view and moment, the uniform
// words of every quality level packed by the engine's own `uniforms`, from
// the Ocean item's catalogue preset plus the view's overrides. It uses the
// tests module's helpers (reg, frame, uniforms, Mirror); a checkout whose
// tests already hold `dump_compare_views` is used as it is.
#[test]
fn dump_compare_views() {
    let Ok(views) = std::env::var("EVANI_CMP_VIEWS") else { return };
    let Ok(dir) = std::env::var("EVANI_SEA_DUMP") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("sea.wgsl"), SHADERS[FULL].source).unwrap();
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&views).unwrap()).unwrap();
    let lib: serde_json::Value = serde_json::from_str(include_str!("../../../../catalog/procedural/procedural.json")).unwrap();
    let ocean = lib["items"].as_array().unwrap().iter().find(|i| i["id"] == "ocean").unwrap();
    let (base, p) = reg();
    // A catalogue value onto the registry; false when the name is not a
    // registered sea or nature value (the studio's sky.* values are not).
    let put = |reg: &mut ParamRegistry, n: &str, v: &serde_json::Value| -> bool {
        let Some(id) = reg.id(n) else { return false };
        let val = match v {
            serde_json::Value::Array(a) => ParamValue::Color([0, 1, 2].map(|i| a[i].as_f64().unwrap() as f32)),
            x => ParamValue::F32(x.as_f64().unwrap() as f32),
        };
        reg.set(id, val).unwrap();
        true
    };
    let num = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
    let moments: Vec<f64> = cfg["moments"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let size = [num(&cfg["size"][0]), num(&cfg["size"][1])];
    let mut out = Vec::new();
    let mut applied = serde_json::Map::new();
    for v in cfg["views"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        let mut r = base.clone();
        let mut used = serde_json::Map::new();
        for (n, x) in ocean["base"].as_object().unwrap() {
            if put(&mut r, n, x) {
                used.insert(n.clone(), x.clone());
            }
        }
        if let Some(pr) = v["ours"]["preset"].as_str() {
            let pr = ocean["presets"].as_array().unwrap().iter().find(|x| x["id"] == pr).unwrap_or_else(|| panic!("no Ocean preset {pr}"));
            for (n, x) in pr["params"].as_object().unwrap() {
                if put(&mut r, n, x) {
                    used.insert(n.clone(), x.clone());
                }
            }
        }
        if let Some(set) = v["ours"]["set"].as_object() {
            for (n, x) in set {
                assert!(put(&mut r, n, x), "view {id}: {n} is not a sea or nature value");
                used.insert(n.clone(), x.clone());
            }
        }
        applied.insert(id.to_string(), serde_json::Value::Object(used));
        let s = p.settings(&r);
        let cam = &v["camera"];
        let eye = [num(&cam["eye"][0]), num(&cam["eye"][1]), num(&cam["eye"][2])];
        let (yaw, pitch) = (num(&cam["yaw_deg"]).to_radians(), num(&cam["pitch_deg"]).to_radians());
        let d = [yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos()];
        let target = [eye[0] + d[0] * 60.0, eye[1] + d[1] * 60.0, eye[2] + d[2] * 60.0];
        let fov = num(&cam["fov_deg"]);
        let (el, az) = (num(&v["sun"]["elevation_deg"]).to_radians(), num(&v["sun"]["azimuth_deg"]).to_radians());
        let sun = norm([az.cos() * el.cos(), el.sin(), az.sin() * el.cos()]);
        let sun_col = v["ours"]["sun_col"].as_array().map(|a| [num(&a[0]), num(&a[1]), num(&a[2])]);
        let bed = v["ours"]["bed"].as_str().unwrap_or("deep");
        for (k, t0) in moments.iter().enumerate() {
            // The camera never inside a wave: the next moment (1.3 s steps)
            // where no level's sea stands within a metre of the eye, as the
            // storm harness does.
            let clear = |t: f64| {
                (1..4).all(|l| {
                    let fr = frame(l, eye, target, t);
                    let h = if field::field_on(&s) { field::Field::new(&s, &fr).height_at([eye[0], eye[2]], 0.0) } else { Mirror::new(&uniforms(&s, &fr)).height([eye[0], eye[2]], 0.05) };
                    h < eye[1] - 1.0
                })
            };
            let time = (0..20).map(|i| t0 + 1.3 * f64::from(i)).find(|t| clear(*t)).unwrap_or(*t0);
            let mut levels = Vec::new();
            for level in 0..4 {
                let mut fr = frame(level, eye, target, time);
                fr.camera.fov_y_deg = fov;
                fr.sun = sun;
                if let Some(c) = sun_col {
                    fr.sun_col = c;
                }
                fr.size = size;
                levels.push(uniforms(&s, &fr).to_vec());
            }
            out.push(serde_json::json!({"name": format!("{id}-m{k}"), "look": id, "bed": bed, "time": time, "levels": levels}));
        }
    }
    std::fs::write(dir.join("scenes.json"), serde_json::to_string(&serde_json::json!({"shots": out, "worst": [], "applied": applied})).unwrap()).unwrap();
}
