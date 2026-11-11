//! WP2 (docs/studio/WORK_PACKAGES.md): the asset loader. Owns: the model
//! handle per asset (`models.handle(source.model)`), the studio's source
//! tier (rest pose, unskinned, `_EVANI_PART` per vertex), framing the camera
//! from the asset's bounds and its row's `frame`, and the "being built"
//! state for an asset whose status isn't `ready` (the same asset id, a clear
//! label, never another object).
//!
//! Each frame it looks at the record's asset and publishes [`StudioSource`]:
//! the asset's one source as a [`SourceMesh`] (radius 1, a region per
//! vertex; `evani_style_geo::source`) with its [`Framing`], whatever the
//! asset is made of: a code shape is built here (no download), a model is
//! fetched through its handle (the stream picks the tier; the studio tier
//! is the shared `<model>.glb`, the low-tier conversion every device
//! draws) and read once. Every style package starts from this.
//! Sources stay in memory by asset id (a horse is about 0.3 MB of floats),
//! so going back to an asset reads nothing again.

use std::collections::BTreeMap;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_kit_material_view::studio::evani_studio::assets::{AssetRow, AssetStatus, Frame, SourceKind};
use evani_kit_material_view::studio::{Studio, StudioData};
use evani_render::model::{ModelHandle, Models};
use evani_style_geo::SourceMesh;

/// The camera that frames a source at radius 1 around the origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    /// From the origin, for a vertical field of view of [`FOV_Y_DEG`].
    pub distance: f32,
}

/// The studio camera's vertical field of view.
pub const FOV_Y_DEG: f32 = 35.0;

/// The row's `frame`: the asset's bounds (radius 1) fill `fill` of the
/// view's height.
pub fn framing(f: &Frame) -> Framing {
    let half = (FOV_Y_DEG.to_radians() * 0.5).tan();
    Framing { yaw_deg: f.yaw, pitch_deg: f.pitch, distance: (1.0 / (f.fill * half)).max(1.2) }
}

/// What the record's asset is, for the scene and the page.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceState {
    /// Its source model is still being made: the page shows "Horse is being
    /// built" with the asset's own name; nothing else is drawn in its place.
    BeingBuilt { name: String },
    /// Downloading or reading.
    Loading,
    Ready { mesh: Rc<SourceMesh>, framing: Framing },
    /// For good (the reason is in the error log too).
    Failed(String),
}

/// The loader's output, a resource.
#[derive(Debug, Clone, PartialEq)]
// The scene (WP4) and the page (WP6) read it; until they land only the
// tests do.
#[cfg_attr(not(test), allow(dead_code))]
pub struct StudioSource {
    pub asset: String,
    pub state: SourceState,
}

#[cfg(target_arch = "wasm32")]
type Pending = Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>>;

/// The loader's own memory.
#[derive(Default)]
struct Loader {
    handles: BTreeMap<String, ModelHandle>,
    sources: BTreeMap<String, Result<Rc<SourceMesh>, String>>,
    #[cfg(target_arch = "wasm32")]
    fetching: BTreeMap<String, Pending>,
}

impl Loader {
    fn state(&mut self, row: &AssetRow, world: &World) -> SourceState {
        if row.status != AssetStatus::Ready {
            return SourceState::BeingBuilt { name: row.name.clone() };
        }
        let framing = framing(&row.frame);
        if let Some(s) = self.sources.get(&row.id) {
            return match s {
                Ok(m) => SourceState::Ready { mesh: m.clone(), framing },
                Err(e) => SourceState::Failed(e.clone()),
            };
        }
        let read: Option<Result<SourceMesh, String>> = match row.source.kind {
            SourceKind::Code => Some(row.source.shape.as_deref().and_then(evani_style_geo::code::code_source).ok_or_else(|| format!("{}: no code shape `{}`", row.id, row.source.shape.as_deref().unwrap_or("")))),
            SourceKind::Model | SourceKind::BlenderScript => {
                let model = row.source.model.clone().unwrap_or_default();
                self.model_bytes(&row.id, &model, world).map(|b| b.and_then(|b| evani_style_geo::source::source_from_glb(&b, row)))
            }
        };
        match read {
            None => SourceState::Loading,
            Some(r) => {
                let r = r.map(Rc::new);
                if let Err(e) = &r
                    && let Some(mut log) = world.try_res_mut::<ErrorLog>()
                {
                    log.error(format!("material-view: studio: {e}"));
                }
                self.sources.insert(row.id.clone(), r);
                self.state(row, world)
            }
        }
    }

    /// The asset's model file, once its handle has one (`None` meanwhile).
    fn model_bytes(&mut self, asset: &str, model: &str, world: &World) -> Option<Result<Vec<u8>, String>> {
        let mut models = world.try_res_mut::<Models>()?;
        let h = *self.handles.entry(asset.to_owned()).or_insert_with(|| models.handle(model));
        if models.handle_failed(h) {
            return Some(Err(format!("{asset}: the model `{model}` didn't load (see the log)")));
        }
        let id = models.resolve(h)?;
        let url = models.url(id)?.to_owned();
        drop(models);
        #[cfg(not(target_arch = "wasm32"))]
        {
            const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/");
            let _ = asset;
            let file = url.strip_prefix(evani_render::model::stream::FOLDER).unwrap_or(&url).to_owned();
            Some(std::fs::read(format!("{DIR}{file}")).map_err(|e| format!("{file}: {e}")))
        }
        #[cfg(target_arch = "wasm32")]
        {
            use evani_core::assets::AssetSource;
            if let Some(cell) = self.fetching.get(asset) {
                let got = cell.borrow_mut().take();
                if got.is_some() {
                    self.fetching.remove(asset);
                }
                return got;
            }
            let cell: Pending = Rc::new(std::cell::RefCell::new(None));
            let into = cell.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let r = evani_core::web::FetchSource.fetch(&url).await;
                *into.borrow_mut() = Some(r);
            });
            self.fetching.insert(asset.to_owned(), cell);
            None
        }
    }
}

/// Add the loader (its resource and its frame system).
pub fn install(ctx: &mut evani_core::plugin::PluginContext) {
    ctx.world.insert_resource(StudioSource { asset: String::new(), state: SourceState::Loading });
    let mut loader = Loader::default();
    ctx.system(evani_core::ecs::SystemDesc::new("material-view.studio.loader", evani_core::ecs::Stage::Frame, move |w: &mut World| {
        let (Some(data), Some(asset)) = (w.try_res::<StudioData>().map(|d| d.0.clone()), w.try_res::<Studio>().map(|s| s.state.selection.asset.clone())) else { return };
        let Some(row) = data.assets.asset(&asset) else { return };
        let state = loader.state(row, w);
        if let Some(mut out) = w.try_res_mut::<StudioSource>()
            && (out.asset != asset || out.state != state)
        {
            *out = StudioSource { asset, state };
        }
    }));
}

#[cfg(test)]
mod tests {
    use evani_kit_material_view::studio::{Pick, StudioCommand};

    use super::*;

    const DT: f64 = 1000.0 / 60.0;

    fn source(app: &evani_core::App) -> StudioSource {
        app.world.res::<StudioSource>().clone()
    }

    /// The record's asset reaches the scene as its own source, whatever it
    /// is made of; one not ready yet says "being built" with its own name
    /// and nothing else; a test shape is built at once. The loader keeps up
    /// with picks, and never names another asset.
    #[test]
    fn the_record_asset_is_loaded_or_named_as_being_built() {
        let mut app = crate::build_app();
        let data = app.world.res::<StudioData>().0.clone();
        let pick = |app: &mut evani_core::App, id: &str| {
            app.world.res_mut::<Studio>().push(StudioCommand::Pick(Pick::Asset(id.into())));
            app.run_headless(3, DT);
            source(app)
        };
        for row in &data.assets.assets {
            let s = pick(&mut app, &row.id);
            assert_eq!(s.asset, row.id, "the loader follows the record");
            match (&row.status, &s.state) {
                (AssetStatus::Ready, SourceState::Ready { mesh, framing }) => {
                    mesh.validate(row.regions.len()).unwrap();
                    assert!(framing.distance > 1.0, "{}", row.id);
                }
                // A model streams over a few frames natively too.
                (AssetStatus::Ready, SourceState::Loading) => {}
                (AssetStatus::Ready, other) => panic!("{}: a ready asset is {other:?}", row.id),
                (_, SourceState::BeingBuilt { name }) => assert_eq!(name, &row.name),
                (_, other) => panic!("{}: an asset being built is {other:?}", row.id),
            }
        }
        let cube = pick(&mut app, "shape.cube");
        assert!(matches!(cube.state, SourceState::Ready { .. }), "a code shape needs no download: {cube:?}");
    }

    /// The row's fill is what the camera gives: radius 1 at `distance`
    /// spans `fill` of the view's height.
    #[test]
    fn framing_fills_the_view_as_the_row_says() {
        let f = framing(&Frame { yaw: 30.0, pitch: 10.0, fill: 0.8 });
        let shown = 1.0 / (f.distance * (FOV_Y_DEG.to_radians() * 0.5).tan());
        assert!((shown - 0.8).abs() < 1e-4, "{shown}");
        assert_eq!((f.yaw_deg, f.pitch_deg), (30.0, 10.0));
    }
}
