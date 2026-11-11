//! evani-material-proc: procedural surface materials for worlds built from
//! boxes, through evani-render's public kit-material hook. No textures and
//! no engine changes; the start of the engine's shared materials library.
//!
//! A game describes its world as a [`Scene`]: boxes with a surface id per
//! face ([`surface`]) and floor-plan [`Region`]s that style the generic
//! `WALL` and `FLOOR` surfaces. [`install`] (or [`ProcWorld::new`] plus
//! [`system`]) bakes per-face ambient occlusion and convex-edge flags on the
//! CPU ([`bake`], deterministic) and packs them with the boxes into
//! `kit_data` ([`layout`]); the system installs the WGSL once and draws
//! every box each frame. The shader finds the face under each fragment,
//! paints it (plaster, sandstone, concrete, wood and painted-metal crates,
//! steps, paving, trims, stencil letters) with grime, streaks and edge wear,
//! and darkens it by the baked AO.
//!
//! Every surface's colour, wear, grime and streaks, and the AO's strength
//! and warmth, are live params (`material.*`, [`params`]).
//!
//! Feature `sky`: the [`sky`] dome, a Preetham daylight sky with drifting
//! clouds (ported from three.js Sky.js, MIT), drawn as one unlit sphere
//! round the camera when the look has a `sky` material, its sun following
//! `render.light.direction` (`sky.*` params). Feature `sky-horizon` adds
//! live control of how the clouds meet the horizon (`sky.cloud_horizon_*`,
//! [`sky::CloudHorizon`]) for cameras that see mostly the skyline.
//!
//! Turning the module on: list it as an optional dependency behind a game
//! feature; it turns on evani-render's `forward` and `kit-material`. The
//! look provides `world` and `decal` materials (white, so the procedural
//! albedo shows unchanged) and, for the sky, an unlit white `sky` material:
//! evani-look-photoreal's `daylight` preset has all three.

pub mod bake;
#[cfg(feature = "sky-discs")]
pub mod discs;
pub mod draw;
pub mod layout;
pub mod params;
pub mod scene;
pub mod shader;
#[cfg(feature = "sky")]
pub mod sky;

pub use bake::{Bake, FaceBake};
#[cfg(feature = "sky-discs")]
pub use discs::{DiscHandles, SkyDiscs};
pub use draw::{DECAL_MATERIAL, ProcWorld, WORLD_MATERIAL, draw_world, install, system};
pub use params::{MaterialParams, SurfaceTune, Tuning};
pub use scene::{ProcBox, Region, Scene, V3, surface};
#[cfg(feature = "sky")]
pub use sky::{SKY_MATERIAL, SkyHandles, SkyParams};

#[cfg(test)]
mod tests {
    use super::*;

    /// A 12 x 12 m yard with a 4 m wall ring, a crate and a decal.
    fn yard() -> Scene {
        let mut boxes = vec![ProcBox::aabb(
            [-6.0, -0.3, -6.0],
            [6.0, 0.0, 6.0],
            surface::FLOOR,
        )];
        boxes[0].world_pattern = true;
        for (min, max) in [
            ([-6.0, 0.0, -6.0], [6.0, 4.0, -5.5]),
            ([-6.0, 0.0, 5.5], [6.0, 4.0, 6.0]),
            ([-6.0, 0.0, -5.5], [-5.5, 4.0, 5.5]),
            ([5.5, 0.0, -5.5], [6.0, 4.0, 5.5]),
        ] {
            let mut w = ProcBox::aabb(min, max, surface::WALL);
            w.world_pattern = true;
            boxes.push(w);
        }
        boxes.push(ProcBox::aabb(
            [0.0, 0.0, 0.0],
            [1.2, 1.2, 1.2],
            surface::WOOD_CRATE,
        ));
        let mut d = ProcBox::aabb([-1.0, 1.0, 5.49], [1.0, 3.0, 5.5], surface::SIGN_A);
        d.decal = true;
        d.occluder = false;
        d.bake_ao = false;
        boxes.push(d);
        let regions = vec![Region {
            rect: [-6.0, -6.0, 0.0, 6.0],
            wall: surface::SANDSTONE,
            floor: surface::GROUND,
            variant: 0,
        }];
        Scene {
            boxes,
            regions,
            default_region: None,
        }
    }

    #[test]
    fn lookup_finds_the_face_under_a_point() {
        let s = yard();
        let w = ProcWorld::new(s).unwrap();
        let d = w.data();
        // The floor's top (+y, face 3).
        assert_eq!(
            layout::find(d, [2.0, 0.0, -3.0], [0.0, 1.0, 0.0], false),
            Some((0, 3))
        );
        // The south wall's inner face (+z of the wall at z -5.5, face 5).
        assert_eq!(
            layout::find(d, [1.0, 2.0, -5.5], [0.0, 0.0, 1.0], false),
            Some((1, 5))
        );
        // The crate's -x side (face 0) and its top (face 3).
        assert_eq!(
            layout::find(d, [0.0, 0.6, 0.6], [-1.0, 0.0, 0.0], false),
            Some((5, 0))
        );
        assert_eq!(
            layout::find(d, [0.6, 1.2, 0.6], [0.0, 1.0, 0.0], false),
            Some((5, 3))
        );
        // The decal: only in decal lookups; the wall behind in world lookups.
        assert_eq!(
            layout::find(d, [0.0, 2.0, 5.49], [0.0, 0.0, -1.0], true),
            Some((6, 4))
        );
        assert_eq!(
            layout::find(d, [0.0, 2.0, 5.5], [0.0, 0.0, -1.0], false),
            Some((2, 4))
        );
        // Mid-air matches nothing.
        assert_eq!(
            layout::find(d, [2.0, 2.0, 2.0], [0.0, 1.0, 0.0], false),
            None
        );
    }

    #[test]
    fn bake_darkens_corners_and_flags_convex_edges() {
        let s = yard();
        let b = bake::bake(&s);
        assert!(b.visible > 0 && b.rays > 0);
        let floor = b.faces[0][3];
        let t = |i: u32, j: u32| b.texels[(floor.start.unwrap() + j * floor.nu + i) as usize];
        // Open middle vs the corner by two walls.
        let (mid, corner) = (t(floor.nu / 2 + 3, floor.nv / 2 - 3), t(1, 1));
        assert!(corner < mid - 0.1, "corner {corner} mid {mid}");
        // The crate's top: all four edges convex. Its -x side: the bottom
        // edge (v-min) stands on the floor, so it is not.
        let c = b.faces[5];
        assert_eq!(c[3].edges, 0b1111);
        assert_eq!(c[0].edges & 0b0100, 0);
        assert_ne!(c[0].edges & 0b1000, 0);
    }

    #[test]
    fn pack_is_deterministic_and_in_bounds() {
        let a = ProcWorld::new(yard()).unwrap();
        let b = ProcWorld::new(yard()).unwrap();
        assert_eq!(a.data(), b.data());
        assert!(a.data().len() <= evani_render::kit_material::MAX_DATA);
        assert!(a.data().iter().flatten().all(|v| v.is_finite()));
    }

    /// FNV-1a over the bits of every texel and record.
    fn hash(words: impl Iterator<Item = f32>) -> u64 {
        words.fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
            (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// The AO bake is bit-for-bit deterministic: the same scene bakes to the
    /// same texels (pinned by hash, so a change to the bake shows up here),
    /// and every texel is a valid occlusion.
    #[test]
    fn ao_bake_is_deterministic() {
        let a = bake::bake(&yard());
        let b = bake::bake(&yard());
        assert_eq!(a.texels, b.texels);
        assert_eq!((a.rays, a.visible), (b.rays, b.visible));
        assert!(
            a.texels.iter().all(|t| (0.0..=1.0).contains(t)),
            "texels outside 0..1"
        );
        let h = hash(a.texels.iter().copied());
        assert_eq!(
            h,
            AO_HASH,
            "the yard's AO bake changed: {h:#x} ({} texels, {} rays)",
            a.texels.len(),
            a.rays
        );
    }

    /// The yard's AO bake (update only for an intended change to the bake).
    const AO_HASH: u64 = 0x8252_1005_e2c1_6434;

    /// The tuning header and records sit where the shader reads them, and a
    /// changed tuning reaches only those records.
    #[test]
    fn tuning_lives_in_the_header_records() {
        let mut w = ProcWorld::new(yard()).unwrap();
        let d0 = w.data().to_vec();
        assert_eq!(
            d0[layout::TUNE_HEADER],
            [layout::TUNE_BASE as f32, surface::COUNT as f32, 1.2, 1.0]
        );
        assert!(
            d0[layout::tune_range()]
                .iter()
                .flatten()
                .all(|v| *v == 1.0 || *v == 0.0)
        );
        let mut t = Tuning::default();
        t.surfaces[surface::WOOD_CRATE as usize].grime = 2.0;
        t.ao_warmth = 0.0;
        w.set_tuning(t);
        let d1 = w.data();
        let changed: Vec<usize> = (0..d0.len()).filter(|&i| d0[i] != d1[i]).collect();
        assert_eq!(
            changed,
            vec![
                layout::TUNE_HEADER,
                layout::TUNE_BASE + 2 * surface::WOOD_CRATE as usize + 1
            ]
        );
    }

    fn validate(src: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn wgsl_validates_on_its_own() {
        let src = format!(
            "{}{}",
            evani_render::kit_material::BINDING_WGSL,
            shader::wgsl(3, 4, 5)
        );
        validate(&src);
        assert!(!shader::wgsl(3, 4, 5).contains('$'));
        // The sky code ships only with its feature.
        assert_eq!(
            shader::wgsl(3, 4, 5).contains("fn mp_sky("),
            cfg!(feature = "sky")
        );
        assert_eq!(
            shader::wgsl(3, 4, 5).contains("three.js"),
            cfg!(feature = "sky")
        );
    }

    /// The forward module with the engine's built-in slots spliced in.
    #[cfg(feature = "sky")]
    fn forward_module() -> String {
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
        fwd
    }

    /// A scene of no boxes ships only the sky part: it validates on its
    /// own and spliced into the forward and shadow modules, carries none of
    /// the surface painter, and is far smaller than the whole code.
    #[cfg(feature = "sky")]
    #[test]
    fn sky_only_code_validates_and_carries_no_painter() {
        let code = shader::sky_wgsl(4);
        validate(&format!(
            "{}{}",
            evani_render::kit_material::BINDING_WGSL,
            code
        ));
        assert!(code.contains("fn mp_sky_rgba(") && code.contains("const MP_SKY: u32 = 4u;"));
        assert!(!code.contains("fn mp_world(") && !code.contains('$'));
        assert!(
            code.len() * 3 < shader::wgsl(0, 1, 4).len(),
            "{} bytes of {}",
            code.len(),
            shader::wgsl(0, 1, 4).len()
        );
        let patched = evani_render::kit_material::patch_forward(&forward_module(), &code).unwrap();
        validate(&patched);
        validate(&evani_render::kit_material::shadows_source(&code));
        let empty = Scene {
            boxes: Vec::new(),
            regions: Vec::new(),
            default_region: None,
        };
        assert_eq!(draw::code_for(&empty, (0, 1, 4)), code);
        assert_eq!(draw::code_for(&yard(), (0, 1, 4)), shader::wgsl(0, 1, 4));
    }

    /// A sky-only world draws just the dome round the eye (none while
    /// `sky.visible` is off), installs the sky-only code, and sends the
    /// open-below flag in record 6.
    #[cfg(feature = "sky")]
    #[test]
    fn a_sky_only_world_draws_the_dome_and_follows_its_switches() {
        use evani_core::ecs::World;
        use evani_core::params::{ParamRegistry, ParamValue};
        use evani_core::plugin::{LookData, MaterialDesc};
        use evani_render::RenderWorld;
        use evani_render::kit_material::KitMaterial;
        let mut w = World::new();
        let mut sky = MaterialDesc::new(SKY_MATERIAL, [1.0; 4]);
        sky.unlit = true;
        w.insert_resource(LookData {
            materials: vec![MaterialDesc::new("stone", [0.6; 4]), sky],
            ..Default::default()
        });
        let mut reg = ParamRegistry::new();
        let mut pw = ProcWorld::sky_only().unwrap();
        pw.register_params(&mut reg).unwrap();
        w.insert_resource(reg);
        w.insert_resource(pw);
        w.insert_resource(KitMaterial::default());
        w.insert_resource(RenderWorld::default());
        draw_world(&mut w);
        {
            let rw = w.res::<RenderWorld>();
            assert_eq!(rw.meshes.len(), 1, "only the dome");
            assert_eq!(rw.meshes[0].material, 1);
            let km = w.res::<KitMaterial>();
            assert!(km.wgsl().contains("fn mp_sky_rgba(") && !km.wgsl().contains("fn mp_world("));
            assert_eq!(km.data()[sky::RECORD + 3][3], 0.0);
        }
        {
            let mut reg = w.res_mut::<ParamRegistry>();
            let id = reg.id("sky.open_below").unwrap();
            reg.set(id, ParamValue::Bool(true)).unwrap();
            let id = reg.id("sky.visible").unwrap();
            reg.set(id, ParamValue::Bool(false)).unwrap();
        }
        w.res_mut::<RenderWorld>().meshes.clear();
        draw_world(&mut w);
        assert!(w.res::<RenderWorld>().meshes.is_empty(), "hidden");
        assert_eq!(
            w.res::<KitMaterial>().data()[sky::RECORD + 3][3],
            1.0,
            "open below"
        );
    }

    /// Without feature `sky-horizon` the sky part ships exactly as written
    /// (other games' shaders are unchanged); with it, every edit matched
    /// once, no fixed horizon number is left, and the code validates on its
    /// own and spliced into the forward and shadow modules.
    #[cfg(feature = "sky")]
    #[test]
    fn horizon_edits_apply_only_with_their_feature() {
        #[cfg(not(feature = "sky-horizon"))]
        {
            assert!(shader::wgsl(0, 1, 2).contains(shader::SKY));
            assert!(shader::sky_wgsl(2).starts_with(shader::SKY));
            assert_eq!(layout::HEADER, 8);
        }
        #[cfg(feature = "sky-horizon")]
        {
            for (from, to) in shader::HORIZON_EDITS {
                assert_eq!(shader::SKY.matches(from).count(), 1, "{from}");
                assert!(shader::sky_source().contains(to), "{to}");
            }
            let src = shader::sky_source();
            // The notes go, the three.js licence ships whole (MIT).
            let licence_end = shader::SKY.find("DEALINGS IN THE SOFTWARE.").unwrap() + 25;
            assert!(src.starts_with(&shader::SKY[..licence_end]));
            assert!(!src.contains("// Records:") && !src.contains("// ---- sky cumulus"));
            assert!(!src.contains("smoothstep(0.03, 0.27") && !src.contains("smoothstep(0.1, 0.4"));
            assert!(src.contains("kit_data[8]") && layout::SKY_HORIZON == 8);
            assert!(src.contains("kit_data[9]") && layout::SKY_CLOUD_LOOK == 9);
            assert!(src.contains("kit_data[10]") && layout::SKY_TONE == 10);
            assert!(!src.contains("thr + 0.16") && !src.contains("0.3 * main.z"));
            assert!(src.contains("kit_data[11]") && layout::SKY_CLOUD_LIGHT == 11);
            assert!(src.contains("kit_data[12]") && layout::SKY_CUMULUS == 12);
            assert!(src.contains("kit_data[13]") && layout::SKY_CUMULUS_LOOK == 13);
            #[cfg(not(feature = "sky-discs"))]
            assert_eq!(layout::HEADER, 14);
            // The disc part: its one edit made, its code spliced in and
            // reading its three records (feature `sky-discs`).
            #[cfg(feature = "sky-discs")]
            {
                for (from, to) in shader::DISC_EDITS {
                    assert!(src.matches(to).count() == 1 && src.matches(from).count() == usize::from(to.contains(from)), "{to}");
                }
                assert!(src.contains("fn mp_discs(") && src.contains("fn mp_dk_warm(") && !src.contains("// ---- sky discs"));
                assert!(src.contains("fn mp_dk_grain(") && src.contains("fn mp_dk_air(") && src.contains("fn mp_dk_un("));
                assert!(src.contains("kit_data[24]") && src.contains("fn mp_dk_haze(") && layout::SKY_DISCS == 14);
                assert_eq!(layout::HEADER, 25);
                assert_eq!(layout::SKY_DISCS + crate::discs::RECORDS, layout::HEADER);
            }
            let code = shader::sky_wgsl(4);
            validate(&format!(
                "{}{}",
                evani_render::kit_material::BINDING_WGSL,
                code
            ));
            let fwd = forward_module();
            validate(&evani_render::kit_material::patch_forward(&fwd, &code).unwrap());
            validate(&evani_render::kit_material::shadows_source(&code));
        }
    }

    /// Feature `sky-horizon`: the horizon record sits in the header, the
    /// tune records move one on, the lookup still finds every face, and
    /// the draw system sends the live horizon params into record 8.
    #[cfg(feature = "sky-horizon")]
    #[test]
    fn horizon_record_is_in_the_header_and_follows_its_params() {
        use evani_core::ecs::World;
        use evani_core::params::{ParamRegistry, ParamValue};
        use evani_core::plugin::{LookData, MaterialDesc};
        use evani_render::RenderWorld;
        use evani_render::kit_material::KitMaterial;
        let yard_world = ProcWorld::new(yard()).unwrap();
        assert_eq!(yard_world.data()[layout::TUNE_HEADER][0], layout::TUNE_BASE as f32);
        assert_eq!(
            layout::find(yard_world.data(), [2.0, 0.0, -3.0], [0.0, 1.0, 0.0], false),
            Some((0, 3))
        );
        let mut w = World::new();
        let mut sky = MaterialDesc::new(SKY_MATERIAL, [1.0; 4]);
        sky.unlit = true;
        w.insert_resource(LookData {
            materials: vec![MaterialDesc::new("stone", [0.6; 4]), sky],
            ..Default::default()
        });
        let mut reg = ParamRegistry::new();
        let mut pw = ProcWorld::sky_only().unwrap();
        pw.register_params(&mut reg).unwrap();
        w.insert_resource(reg);
        w.insert_resource(pw);
        w.insert_resource(KitMaterial::default());
        w.insert_resource(RenderWorld::default());
        draw_world(&mut w);
        assert_eq!(
            w.res::<KitMaterial>().data()[layout::SKY_HORIZON],
            [0.03, 0.27, 1.0, 1.0]
        );
        // The cloud look and the tone start as the sky part draws without
        // them.
        assert_eq!(
            w.res::<KitMaterial>().data()[layout::SKY_CLOUD_LOOK],
            [0.3, 0.16, 1.0, 1.0]
        );
        assert_eq!(
            w.res::<KitMaterial>().data()[layout::SKY_TONE],
            [1.0, 1.0, 1.0, 0.0]
        );
        assert_eq!(
            w.res::<KitMaterial>().data()[layout::SKY_CLOUD_LIGHT],
            [0.8, 0.4, 0.0, 0.0]
        );
        {
            let mut reg = w.res_mut::<ParamRegistry>();
            for (n, v) in [
                ("sky.cloud_horizon_high", 0.1),
                ("sky.cloud_horizon_haze", 0.5),
            ] {
                let id = reg.id(n).unwrap();
                reg.set(id, ParamValue::F32(v)).unwrap();
            }
        }
        draw_world(&mut w);
        assert_eq!(
            w.res::<KitMaterial>().data()[layout::SKY_HORIZON],
            [0.03, 0.1, 1.0, 0.5]
        );
        {
            let mut reg = w.res_mut::<ParamRegistry>();
            for (n, v) in [("sky.cloud_lobe_shade", 0.1), ("sky.zenith_deep", 0.4)] {
                let id = reg.id(n).unwrap();
                reg.set(id, ParamValue::F32(v)).unwrap();
            }
            let id = reg.id("sky.tint").unwrap();
            reg.set(id, ParamValue::Color([1.0, 0.75, 1.0])).unwrap();
        }
        draw_world(&mut w);
        assert_eq!(w.res::<KitMaterial>().data()[layout::SKY_CLOUD_LOOK][0], 0.1);
        assert_eq!(
            w.res::<KitMaterial>().data()[layout::SKY_TONE],
            [1.0, 0.75, 1.0, 0.4]
        );
        // The cloud volume: off by default (its records at their defaults),
        // then on with its own seed and layer.
        let d = sky::CloudVolume::default();
        assert_eq!(w.res::<KitMaterial>().data()[layout::SKY_CUMULUS], d.record());
        assert_eq!(w.res::<KitMaterial>().data()[layout::SKY_CUMULUS_LOOK], d.look_record());
        {
            let mut reg = w.res_mut::<ParamRegistry>();
            let id = reg.id("sky.cloud_volume").unwrap();
            reg.set(id, ParamValue::Bool(true)).unwrap();
            for (n, v) in [("sky.cloud_seed", 102.49), ("sky.cloud_base", 1.2), ("sky.cirrus", 0.2)] {
                let id = reg.id(n).unwrap();
                reg.set(id, ParamValue::F32(v)).unwrap();
            }
        }
        draw_world(&mut w);
        assert_eq!(w.res::<KitMaterial>().data()[layout::SKY_CLOUD_LIGHT], [0.8, 0.4, 1.0, 102.49]);
        assert_eq!(w.res::<KitMaterial>().data()[layout::SKY_CUMULUS], [1.2, d.depth, d.size, 0.2]);
    }

    #[test]
    fn wgsl_validates_spliced_into_the_forward_and_shadow_modules() {
        let mut slots = evani_core::slots::ShaderSlots::with_builtins();
        // With evani-render's `pcss` feature on (an app in the same build
        // turns it on), forward.wgsl also calls the soft-shadow slot.
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
        let code = shader::wgsl(0, 1, 2);
        let patched = evani_render::kit_material::patch_forward(&fwd, &code).unwrap();
        validate(&patched);
        validate(&evani_render::kit_material::shadows_source(&code));
    }
}

#[cfg(feature = "relief")]
pub mod relief;
