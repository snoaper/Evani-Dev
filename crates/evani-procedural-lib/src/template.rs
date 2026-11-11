//! The asset to copy when you build one (docs/PROCEDURAL.md "Adding an
//! asset"). It is compiled in tests only, so it always builds and its test
//! keeps it honest; nothing here reaches a game.
//!
//! To turn placeholder `<category>-<asset>` into a real generator:
//! 1. Copy [`Template`] into `src/<category>.rs` (make the file and a
//!    `pub mod <category>;` line in lib.rs if the category has none yet),
//!    behind `#[cfg(feature = "<category>-<asset>")]`, and rename it.
//! 2. Fill in its [`AssetInfo`] (the category and asset ids exactly as in
//!    catalog/procedural/library.json), its params and `generate`. Keep it
//!    deterministic: only the values and the seed (no wall clock, no
//!    `rand`); read MIT sources for maths only, never copy code.
//! 3. Register it in [`crate::registry`] under the same `#[cfg]`.
//! 4. In library.json set the asset's `status` to `done`, add
//!    `inspired_by` (the MIT source and what was taken from it) and, if it
//!    needs another crate, its `cargo` features. Run
//!    `python -I tools/procedural/gen_library.py` (drops the placeholder).
//! 5. `cargo test -p evani-procedural-lib --features <category>` and
//!    `cargo xtask check`.

#[cfg(test)]
pub use box_stack::Template;

#[cfg(test)]
mod box_stack {
    use evani_procedural::{AssetInfo, Output, ParamSpec, ProcAsset, Values, geo, noise};

    /// A worked example: a stack of seeded boxes.
    pub struct Template;

    // The ids must match library.json (here the `template` test category).
    const INFO: AssetInfo = AssetInfo::cpu("template", "box-stack", "Box stack");
    const PARAMS: [ParamSpec; 3] = [
        ParamSpec::num("count", 3.0, 1.0, 12.0).step(1.0).describe("Boxes in the stack"),
        ParamSpec::num("size", 1.0, 0.1, 5.0).unit("m").describe("The bottom box's width (m)"),
        ParamSpec::color("color", [0.5, 0.4, 0.3]).describe("Colour"),
    ];

    impl ProcAsset for Template {
        fn info(&self) -> &AssetInfo {
            &INFO
        }

        fn params(&self) -> &[ParamSpec] {
            &PARAMS
        }

        // Realism only (the default `styles`): every other style falls back.
        fn generate(&self, v: &Values, seed: u64, _style: &str) -> Output {
            let mut m = geo::Mesh::default();
            let (mut y, mut s) = (0.0, v.f("size"));
            for i in 0..v.f("count").round().max(1.0) as i32 {
                let mut b = geo::segmented_box([s, s * 0.5, s], 1);
                let shift = (noise::hash2(i, 0, seed as u32) - 0.5) * 0.2 * s;
                geo::transform(&mut b, [1.0; 3], [shift, y, 0.0]);
                geo::merge(&mut m, &b);
                y += s * 0.5;
                s *= 0.8;
            }
            Output::Mesh(m)
        }
    }
}
