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
//! `render.light.direction` (`sky.*` params).
//!
//! Turning the module on: list it as an optional dependency behind a game
//! feature; it turns on evani-render's `forward` and `kit-material`. The
//! look provides `world` and `decal` materials (white, so the procedural
//! albedo shows unchanged) and, for the sky, an unlit white `sky` material:
//! evani-look-photoreal's `daylight` preset has all three.

pub mod bake;
pub mod draw;
pub mod layout;
pub mod params;
pub mod scene;
pub mod shader;
#[cfg(feature = "sky")]
pub mod sky;

pub use bake::{Bake, FaceBake};
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
