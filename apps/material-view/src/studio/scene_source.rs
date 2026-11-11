//! WP4: where the studio scene gets its shapes. Two seams, so the packages
//! that own the real work slot in without touching the scene:
//!
//! - [`StudioSources`]: each asset's one source model as a [`SourceMesh`],
//!   as WP2's loader (`loader.rs`, resource [`StudioSource`]) publishes it.
//!   The scene reads no files itself: every frame [`StudioSources::sync`]
//!   keeps what the loader has made (code shapes and model files alike), so
//!   a comparison view can draw an asset the record showed earlier while the
//!   loader follows the current one. Nothing here ever invents a shape: no
//!   source, nothing drawn.
//! - [`StyledCache`]: the geometry stage's output by [`GeometryKey`]. Stage
//!   `none` is the contract's `Passthrough`, made at once (the plain asset
//!   shows the same frame); any other stage is a job on WP3's queue
//!   (`evani_style_geo::cache::Variants`, one job a frame, `variants.rs`):
//!   [`Styled::Making`] meanwhile (the queue's "Making voxel horse..."),
//!   while the scene draws the same asset plain, then the made shape. A
//!   shape that can't be made answers with its reason, and the scene keeps
//!   the plain asset, marked.

use std::collections::HashMap;
use std::rc::Rc;

use evani_kit_material_view::studio::evani_studio::assets::AssetRow;
use evani_kit_material_view::studio::evani_studio::geo::{SourceMesh, StyledMesh};
use evani_kit_material_view::studio::evani_studio::hash::GeometryKey;
use evani_kit_material_view::studio::evani_studio::styles::Geometry;
use evani_style_geo::cache::{Job, Status, Variants};

use super::loader::{SourceState, StudioSource};

/// An asset's source, as far as it has got.
#[derive(Debug, Clone)]
pub enum Source {
    /// The loader is on it (or hasn't reached this asset yet).
    Loading,
    Ready(Rc<SourceMesh>),
    /// It can't be read: the reason (shown, never replaced by a shape).
    Failed(String),
}

/// The sources the scene has been given (a world resource).
#[derive(Debug, Default)]
pub struct StudioSources {
    done: HashMap<String, Source>,
}

impl StudioSources {
    /// A source made elsewhere (tests).
    #[cfg_attr(not(test), allow(dead_code, reason = "the loader fills it through sync"))]
    pub fn insert(&mut self, asset: &str, src: SourceMesh) {
        self.done.insert(asset.to_string(), Source::Ready(Rc::new(src)));
    }

    /// Keep what the loader has published, once per asset (the loader keeps
    /// a read source for good, so a later answer never differs). A source
    /// that breaks the contract (a region out of range) is refused here.
    pub fn sync(&mut self, from: &StudioSource, row: Option<&AssetRow>) {
        if from.asset.is_empty() || self.done.contains_key(&from.asset) {
            return;
        }
        let s = match &from.state {
            SourceState::Ready { mesh, .. } => match row.map(|r| mesh.validate(r.regions.len())) {
                Some(Err(e)) => Source::Failed(format!("{}: {e}", from.asset)),
                _ => Source::Ready(mesh.clone()),
            },
            SourceState::Failed(e) => Source::Failed(e.clone()),
            SourceState::BeingBuilt { .. } | SourceState::Loading => return,
        };
        self.done.insert(from.asset.clone(), s);
    }

    /// What is known of `asset`'s source now.
    pub fn get(&self, asset: &AssetRow) -> Source {
        self.done.get(&asset.id).cloned().unwrap_or(Source::Loading)
    }
}

/// The geometry stage's result for one key.
#[derive(Debug, Clone)]
pub enum Styled {
    Ready(Rc<StyledMesh>),
    /// Being made on WP3's queue (the page shows its `progress()` line,
    /// "Making voxel horse...").
    Making,
    /// The generator can't make it: the reason (the plain asset is drawn).
    NotBuilt(String),
}

/// How many styled meshes are kept (the variants a user flips between).
pub const KEEP: usize = 8;

/// The geometry stage's results by key (a world resource).
#[derive(Debug, Default)]
pub struct StyledCache {
    map: HashMap<GeometryKey, (Styled, u64)>,
    tick: u64,
    /// Generator runs so far (tests: a light change runs none).
    pub runs: u64,
}

impl StyledCache {
    /// The styled mesh for `key`: the plain stage made now, any other
    /// asked of WP3's queue (`variants`; none installed: not built).
    pub fn get(&mut self, key: GeometryKey, src: &Rc<SourceMesh>, asset: &AssetRow, geometry: &Geometry, variants: Option<&mut Variants>) -> Styled {
        self.tick += 1;
        let tick = self.tick;
        if let Some((s, used)) = self.map.get_mut(&key) {
            *used = tick;
            return s.clone();
        }
        let checked = |m: Rc<StyledMesh>| match m.validate(asset.regions.len()) {
            Ok(()) => Styled::Ready(m),
            Err(e) => Styled::NotBuilt(e),
        };
        let s = if *geometry == Geometry::None {
            self.runs += 1;
            match evani_style_geo::generator(geometry.kind()).generate(src, asset, geometry) {
                Ok(m) => checked(Rc::new(m)),
                Err(e) => Styled::NotBuilt(e),
            }
        } else {
            let Some(variants) = variants else { return Styled::NotBuilt("the shape queue isn't installed".into()) };
            match variants.request(Job { key, asset: asset.clone(), source: src.clone(), geometry: geometry.clone() }) {
                // Still on the queue: asked again next frame, not kept.
                Status::Making(_) => {
                    self.runs += 1;
                    return Styled::Making;
                }
                Status::Ready(m) => checked(m),
                Status::Failed(e) => Styled::NotBuilt(e),
            }
        };
        if self.map.len() >= KEEP
            && let Some(old) = self.map.iter().min_by_key(|(_, (_, u))| *u).map(|(k, _)| *k)
        {
            self.map.remove(&old);
        }
        self.map.insert(key, (s.clone(), tick));
        s
    }

    /// A result made elsewhere (WP3's queue, tests).
    #[cfg_attr(not(test), allow(dead_code, reason = "WP3's off-frame queue (variants.rs) fills it"))]
    pub fn insert(&mut self, key: GeometryKey, s: Styled) {
        self.tick += 1;
        self.map.insert(key, (s, self.tick));
    }
}
