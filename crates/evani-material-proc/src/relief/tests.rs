use super::bake::{self, decode, encode, level_offset, levels_for, texel_count};
use super::pattern::{self, Pattern, pnoise};
use super::*;
use crate::scene::{ProcBox, Scene};

fn all() -> [Pattern; 4] {
    ReliefConfig::default().patterns()
}

/// FNV-1a over the bits of every float.
fn fnv(words: impl Iterator<Item = f32>) -> u64 {
    words.fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn periodic_noise_wraps_exactly() {
    for k in 0..50 {
        // Dyadic steps, so adding a whole period is exact in f32.
        let (x, y) = (0.25 + k as f32 * 0.375, 0.5 + k as f32 * 0.1875);
        let a = pnoise(x, y, 7, 5, 3);
        assert_eq!(a, pnoise(x + 7.0, y, 7, 5, 3));
        assert_eq!(a, pnoise(x, y + 5.0, 7, 5, 3));
        assert!((0.0..=1.0).contains(&a));
    }
}

/// Each pattern a tile apart (without the wrap) is the same pattern: the
/// lattice, the cell ids and the noise all repeat with the tile.
#[test]
fn patterns_repeat_with_their_tile() {
    for p in all() {
        let t = p.tile();
        for k in 0..400 {
            let (u, v) = (t[0] * (k as f32 * 0.0137 % 1.0), t[1] * (k as f32 * 0.0291 % 1.0));
            let a = p.sample_unwrapped(u, v);
            for (du, dv) in [(t[0], 0.0), (0.0, t[1])] {
                let b = p.sample_unwrapped(u + du, v + dv);
                assert!((a.height - b.height).abs() < 0.02, "{p:?} height at {u},{v}: {} vs {}", a.height, b.height);
                for i in 0..3 {
                    assert!((a.color[i] - b.color[i]).abs() < 0.02, "{p:?} colour at {u},{v}");
                }
            }
            assert!((0.0..=1.0).contains(&a.height) && (0.0..=1.0).contains(&a.rough));
        }
    }
}

/// Across the baked tile's wrap the steps are like any other column's or
/// row's: no seam.
#[test]
fn baked_tiles_have_no_seam() {
    for p in all() {
        let b = bake::bake(&p, 128);
        let n = b.n as i64;
        let h = |i: i64, j: i64| b.texel(0, i, j).height;
        let step = |i: i64, vertical: bool| -> f32 {
            (0..n)
                .map(|j| if vertical { (h(i, j) - h(i - 1, j)).abs() } else { (h(j, i) - h(j, i - 1)).abs() })
                .sum::<f32>()
                / n as f32
        };
        for vertical in [true, false] {
            let seam = step(0, vertical);
            let worst = (1..n).map(|i| step(i, vertical)).fold(0.0f32, f32::max);
            // Point sampling a joint thinner than a texel (5 mm tile grout
            // at 9.5 mm texels) gives each joint column a slightly
            // different step with its phase; a broken wrap is a jump far
            // past that (the strict check is `patterns_repeat_with_their_tile`).
            assert!(seam <= worst * 1.1 + 1e-6, "{p:?} seam {seam} > worst interior {worst}");
        }
    }
}

/// The bake is bit-for-bit deterministic, pinned by hash so a change to a
/// pattern or the encoding shows up here.
#[test]
fn bakes_are_deterministic_and_pinned() {
    let mut hashes = Vec::new();
    for p in all() {
        let a = bake::bake(&p, 64);
        let b = bake::bake(&p, 64);
        assert_eq!(a, b);
        hashes.push(fnv(a.texels.iter().flatten().copied().chain([a.slope_scale])));
    }
    assert_eq!(hashes, PINNED, "relief bakes changed: {hashes:#x?}");
}

/// The four 64 x 64 bakes (update only for an intended change).
const PINNED: [u64; 4] = [0x57e9_c358_e338_1116, 0xf6ce_a99f_15a4_9e1b, 0x29f1_26ed_e664_777c, 0x8b15_4160_faa7_a1e2];

#[test]
fn texels_are_exact_integers_and_decode_back() {
    for p in all() {
        let b = bake::bake(&p, 32);
        assert_eq!(b.texels.len(), texel_count(32));
        for e in &b.texels {
            for v in e {
                assert!(v.is_finite() && *v >= 0.0 && *v < 16_777_216.0 && v.fract() == 0.0, "{v}");
            }
        }
    }
    let t = Texel {
        color: [0.25, 0.04, 0.81],
        slope: [0.5, -1.0],
        height: 0.4,
        rough: 0.6,
    };
    let d = decode(encode(&t, 2.0), 2.0);
    for i in 0..3 {
        assert!((d.color[i].sqrt() - t.color[i].sqrt()).abs() <= 0.5 / 255.0 + 1e-6);
    }
    assert!((d.slope[0] - 0.5).abs() <= 2.0 / 63.0 && (d.slope[1] + 1.0).abs() <= 2.0 / 63.0);
    assert!((d.height - 0.4).abs() <= 0.5 / 63.0 + 1e-6 && (d.rough - 0.6).abs() <= 0.5 / 15.0 + 1e-6);
    // Flat encodes as exactly flat.
    assert_eq!(decode(encode(&Texel { slope: [0.0; 2], ..t }, 2.0), 2.0).slope, [0.0, 0.0]);
}

/// The shader's `rl_off` closed form matches the bake's layout.
#[test]
fn mip_offsets_match_the_shader() {
    for n in [16u32, 32, 64, 128, 256] {
        for l in 0..levels_for(n) {
            let m = n >> l;
            assert_eq!(level_offset(n, l), ((n * n - m * m) * 4 / 3) as usize, "n {n} l {l}");
        }
        assert_eq!(n >> (levels_for(n) - 1), bake::MIN_LEVEL);
    }
}

#[test]
fn bake_time_is_reported() {
    let t0 = std::time::Instant::now();
    let r = Relief::new(ReliefConfig::default());
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("relief bake: 4 surfaces at 256 in {ms:.0} ms ({} records)", r.block().len());
    assert_eq!(r.bakes().len(), 4);
    // The block leaves room for a big world's own records (32k).
    assert!(r.block().len() + 32_768 <= evani_render::kit_material::MAX_DATA);
    assert!(ms < 120_000.0);
}

#[test]
fn the_block_points_at_each_surfaces_texels() {
    let cfg = ReliefConfig::default();
    let bakes: Vec<_> = cfg.patterns().iter().map(|p| bake::bake(p, 32)).collect();
    let d = pack(&cfg, &bakes, 2);
    assert_eq!(d[0], [MAGIC, 4.0, 2.0, 1.0]);
    let floats: Vec<f32> = d.iter().flatten().copied().collect();
    for (k, b) in bakes.iter().enumerate() {
        let r = d[HEADER_RECS + SURF_RECS * k];
        let first = r[0] as usize;
        assert_eq!(first % 2, 0);
        assert_eq!((r[1], r[2], r[3]), (32.0, levels_for(32) as f32, b.slope_scale));
        assert_eq!(floats[first], b.texels[0][0]);
        assert_eq!(floats[first + 1], b.texels[0][1]);
        let last = b.texels.len() - 1;
        assert_eq!(floats[first + 2 * last + 1], b.texels[last][1]);
        // Colour multipliers are 1 at the default colours.
        assert_eq!(d[HEADER_RECS + SURF_RECS * k + 3], [1.0, 1.0, 1.0, 0.0]);
    }
    assert_eq!(tier_mode("lite"), 0);
    assert_eq!(tier_mode("low"), 1);
    assert_eq!(tier_mode("ultra"), 2);
    assert_eq!(tier_bake_size(256, "low"), 128);
}

fn validate(src: &str) {
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
}

/// The relief code validates on its own, spliced into the forward module
/// with the surface splice, and in the shadow module.
#[test]
fn relief_wgsl_validates_spliced_into_the_forward_and_shadow_modules() {
    let code = wgsl(0, 1, 2, 1234).unwrap();
    assert!(!code.contains('$'));
    assert!(code.contains("const MP_RELIEF: u32 = 1234u;"));
    assert_eq!(code.matches("fn kit_material(").count(), 1);
    assert_eq!(code.matches("fn kit_surface(").count(), 1);
    validate(&format!("{}{}", evani_render::kit_material::BINDING_WGSL, code));
    let mut slots = evani_core::slots::ShaderSlots::with_builtins();
    slots.declare(evani_core::slots::SlotSignature {
        name: "pcss_params",
        params: &[("i", "u32")],
        ret: "vec4<f32>",
        description: "Soft shadows (test stand-in)",
        default_body: "    return vec4<f32>(0.02, 6.0, 1.0, 12.0);",
    });
    let mut fwd = String::new();
    for line in evani_render::shaders::FORWARD.source.lines() {
        match line.trim().strip_prefix("//@slot ") {
            Some(name) => {
                let id = slots.id(name.trim()).unwrap();
                fwd.push_str(&slots.wgsl_function(id).unwrap());
            }
            None => fwd.push_str(line),
        }
        fwd.push('\n');
    }
    let mut patched = evani_render::kit_material::patch_forward(&fwd, &code).unwrap();
    assert!(evani_render::kit_surface::patch(&mut patched, &code));
    validate(&patched);
    validate(&evani_render::kit_material::shadows_source(&code));
}

fn scene() -> Scene {
    let mut floor = ProcBox::aabb([-4.0, -0.3, -4.0], [4.0, 0.0, 4.0], surface::COBBLE);
    floor.world_pattern = true;
    let mut wall = ProcBox::aabb([-4.0, 0.0, -4.0], [4.0, 3.0, -3.6], surface::BRICK);
    wall.world_pattern = true;
    Scene {
        boxes: vec![floor, wall, ProcBox::aabb([0.0, 0.0, 0.0], [1.0, 0.1, 2.0], surface::PLANKS)],
        regions: Vec::new(),
        default_region: None,
    }
}

fn world() -> World {
    use evani_core::plugin::MaterialDesc;
    let mut w = World::new();
    let look = evani_core::plugin::LookData {
        materials: vec![MaterialDesc::new("world", [1.0; 4]), MaterialDesc::new("decal", [1.0; 4])],
        ..Default::default()
    };
    w.insert_resource(look);
    let mut reg = evani_core::params::ParamRegistry::new();
    let mut pw = crate::ProcWorld::new(scene()).unwrap();
    pw.register_params(&mut reg).unwrap();
    let mut rel = Relief::new(ReliefConfig {
        bake_size: 32,
        ..ReliefConfig::default()
    });
    rel.register_params(&mut reg).unwrap();
    w.insert_resource(reg);
    w.insert_resource(pw);
    w.insert_resource(rel);
    w.insert_resource(evani_render::kit_material::KitMaterial::default());
    w
}

fn frame(w: &mut World) {
    crate::draw::draw_world(w);
    system::run(w);
}

use evani_core::ecs::World;
use evani_render::kit_material::KitMaterial;

/// The system appends the block after the world's records, swaps in the
/// relief code once, and on a param change rewrites only what changed.
#[test]
fn the_system_appends_the_block_and_follows_params_and_tiers() {
    let mut w = world();
    frame(&mut w);
    frame(&mut w);
    let base = w.res::<crate::ProcWorld>().data().len();
    let gens = w.res::<KitMaterial>().generations();
    {
        let km = w.res::<KitMaterial>();
        let rel = w.res::<Relief>();
        assert_eq!(rel.base(), Some(base));
        assert_eq!(km.data().len(), base + rel.block().len());
        assert_eq!(&km.data()[base..], rel.block());
        assert!(km.wgsl().contains(&format!("const MP_RELIEF: u32 = {base}u;")));
        assert!(km.wgsl().contains("fn kit_surface("));
    }
    // Steady: nothing more is sent.
    frame(&mut w);
    assert_eq!(w.res::<KitMaterial>().generations(), gens);
    // A layer param rewrites the header only; no rebake.
    let before = w.res::<KitMaterial>().data().to_vec();
    let rebakes = w.res::<Relief>().rebakes;
    w.res_mut::<evani_core::params::ParamRegistry>()
        .set_by_name("material.relief.wetness", evani_core::params::ParamValue::F32(0.6))
        .unwrap();
    frame(&mut w);
    let after = w.res::<KitMaterial>().data().to_vec();
    let changed: Vec<usize> = (0..after.len()).filter(|&i| after[i] != before[i]).collect();
    assert_eq!(changed, vec![base + 1]);
    assert_eq!(w.res::<Relief>().rebakes, rebakes);
    // A pattern param rebakes that pattern alone.
    w.res_mut::<evani_core::params::ParamRegistry>()
        .set_by_name("material.relief.brick.mortar", evani_core::params::ParamValue::F32(0.015))
        .unwrap();
    frame(&mut w);
    assert_eq!(w.res::<Relief>().rebakes, rebakes + 1);
    // Lite: flat mode, a 16 x 16 bake; the tail is rewritten to the new size.
    w.res_mut::<Relief>().set_tier("lite");
    frame(&mut w);
    let km = w.res::<KitMaterial>();
    let rel = w.res::<Relief>();
    assert_eq!(rel.block()[0][2], 0.0);
    assert_eq!(rel.bakes()[0].n, bake::MIN_SIZE);
    assert_eq!(km.data().len(), base + rel.block().len());
    assert_eq!(&km.data()[base..], rel.block());
    let _ = pattern::BRICK_COLOR;
}
