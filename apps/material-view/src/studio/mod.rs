//! The Asset & Style Studio's app side (feature `material-studio`;
//! docs/studio/SPEC.md, CONTRACTS.md, WORK_PACKAGES.md). The state and its
//! rules are the kit's `studio` module and evani-studio; this folder turns
//! the resolved record into pixels and the page.
//!
//! One file per work package, so parallel packages never edit the same file:
//! - `loader.rs` (WP2): asset handles, the source tier, framing from bounds,
//!   the "being built" state (the same asset, never a stand-in shape).
//! - `variants.rs` (WP3): the geometry stage at runtime: a job queue
//!   time-sliced at 4 ms a frame (or a worker), the memory LRU and the OPFS
//!   cache keyed by `GeometryKey`, "Making voxel horse..." progress.
//! - `scene.rs` (WP4): draws `Resolved`: region bindings to model parts (at
//!   most 16 slots), the surface stage, one model slot replaced in place.
//! - `pass.rs` (WP5): `Resolved::pass` to the screen pass params.
//! - `pick.rs` (parts): a tap on the view selects the part under it; the
//!   selected part flashes briefly.
//! - `page.rs` (WP6, wasm only): the page: style bar, materials bar, asset
//!   browser, viewport tools, inspector, variations, notices.
//!
//! - `proc_data.rs`, `proc_view.rs`, `proc_page.rs` (the procedural
//!   section, with `material-nature` and `material-playground`): the
//!   library of code-drawn effects (catalog/procedural), their scene and
//!   state, and the page's procedural mode.
//! - `lib_data.rs`, `lib_html.rs` (with the procedural section): the Evani
//!   Library tab, the approved assets from evani.app (Evani.App serves the
//!   list; the rows open their procedural items).
//!
//! The rule all of them keep: draw `Studio::state.resolve(..)` and nothing
//! else; never another object, never a fallback sphere.

mod balls;
mod loader;
pub mod pass;
pub mod pick;
pub mod scene;
mod scene_build;
#[cfg(target_arch = "wasm32")]
mod scene_hud;
mod scene_source;
mod tiles;
mod variants;
#[cfg(all(feature = "material-nature", feature = "material-playground"))]
pub mod proc_data;
#[cfg(all(feature = "material-nature", feature = "material-playground"))]
pub mod proc_view;
#[cfg(all(any(target_arch = "wasm32", test), feature = "material-nature", feature = "material-playground"))]
mod proc_html;
// The Library section: the approved assets evani.app serves (lib_data the
// list and its checks, lib_html its markup; proc_page puts it on the page).
#[cfg(all(any(target_arch = "wasm32", test), feature = "material-nature", feature = "material-playground"))]
mod lib_data;
#[cfg(all(any(target_arch = "wasm32", test), feature = "material-nature", feature = "material-playground"))]
mod lib_html;
#[cfg(all(target_arch = "wasm32", feature = "material-nature", feature = "material-playground"))]
mod proc_page;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(any(target_arch = "wasm32", test))]
mod page_view;

use evani_core::plugin::PluginContext;

/// Add the app's studio parts (after the kit's `studio::install`).
pub fn install(ctx: &mut PluginContext) {
    loader::install(ctx);
    variants::install(ctx);
    pass::install(ctx);
    #[cfg(target_arch = "wasm32")]
    page::install(ctx);
    scene::install(ctx);
    balls::install(ctx);
    tiles::install(ctx);
    #[cfg(all(feature = "material-nature", feature = "material-playground"))]
    proc_view::install(ctx);
}

/// The studio's draw systems, added after the other scenes' (main.rs), so
/// the styles scene's per-frame resets run before the studio draws.
pub fn install_late(ctx: &mut PluginContext) {
    scene::install_system(ctx);
    balls::install_system(ctx);
    tiles::install_system(ctx);
    #[cfg(all(feature = "material-nature", feature = "material-playground"))]
    proc_view::install_system(ctx);
    #[cfg(target_arch = "wasm32")]
    scene_hud::install(ctx);
}

#[cfg(test)]
mod tests {
    use evani_core::overlay::ErrorLog;
    use evani_kit_material_view::studio::{Pick, Selection, Studio, StudioCommand, StudioData};

    const DT: f64 = 1000.0 / 60.0;

    fn push(app: &mut evani_core::App, c: StudioCommand) {
        app.world.res_mut::<Studio>().push(c);
        app.run_headless(1, DT);
    }

    /// The app opens on the catalogue's default asset (a horse, never a test
    /// shape) and the record follows the page's commands through the real
    /// app: the acceptance picks, a light change that changes nothing else.
    #[test]
    fn the_studio_runs_in_the_app() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        let data = app.world.res::<StudioData>().0.clone();
        let sel = |app: &evani_core::App| app.world.res::<Studio>().state.selection.clone();
        assert_eq!(sel(&app), Selection::new("animal.horse", "WK01", "horse.coat.bay"));
        push(&mut app, StudioCommand::Pick(Pick::Style("WK08".into())));
        let voxel = app.world.res::<Studio>().state.resolve(&data).unwrap();
        assert_eq!((voxel.selection.asset.as_str(), voxel.selection.style.as_str()), ("animal.horse", "WK08"));
        push(&mut app, StudioCommand::Light("sunset".into()));
        let lit = app.world.res::<Studio>().state.resolve(&data).unwrap();
        assert_eq!((lit.geometry_key, lit.variant_key), (voxel.geometry_key, voxel.variant_key));
        push(&mut app, StudioCommand::Pick(Pick::Asset("animal.cow".into())));
        assert_eq!(sel(&app).style, "WK08", "the style stays");
        assert_eq!(sel(&app).asset, "animal.cow");
        push(&mut app, StudioCommand::Pick(Pick::Asset("animal.unicorn".into())));
        assert!(app.world.res::<Studio>().error.as_deref().is_some_and(|e| e.contains("unicorn")));
        assert_eq!(sel(&app).asset, "animal.cow", "a refused pick changes nothing");
        let log = app.world.res::<ErrorLog>();
        let studio_errors: Vec<&str> = log.entries().filter(|e| e.message.contains("studio")).map(|e| e.message.as_str()).collect();
        assert!(studio_errors.is_empty(), "{studio_errors:?}");
    }
}
