//! Model handles (feature `models`, S3-4).
//!
//! [`Models::handle`] returns a [`ModelHandle`] at once. The kit draws its
//! own placeholder while [`Models::resolve`] is `None`, then the model it
//! returns. A handle names a model, not a file: on each tier it draws
//! `<id>.<tier>.glb` from the manifest (or the shared `<id>.glb`), so a
//! lower tier gets a lighter file of the same model. Nothing is fetched
//! until the tier is settled (after the first-run warm-up), so a tier never
//! downloads another tier's files. After a tier switch the handle keeps
//! drawing the file it has until the new tier's file is ready.
//!
//! Render only: what gameplay reads (anchors, sizes, collision) comes from
//! the boot group or from kit data written at build time, never from here,
//! so the simulation never waits on the network.

use std::rc::Rc;

use evani_core::assets::AssetManifest;

use super::stream::{self, Backend}; #[cfg(feature = "level-groups")] pub mod level;
use super::{ModelId, ModelState, Models};

/// A model by name, returned at once by [`Models::handle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelHandle(pub u32);

/// What is known about the manifest.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Manifest {
    /// Not asked for yet.
    #[default]
    Unknown,
    /// Being fetched.
    Asked,
    /// Read.
    Ready(Rc<AssetManifest>),
    /// The game has none (or it failed to read): handles draw `<id>.glb`.
    Missing,
}

#[derive(Debug)]
struct Handle {
    id: String,
    /// Files asked for so far (page URL, model).
    files: Vec<(String, ModelId)>,
    /// The file for the settled tier.
    current: Option<ModelId>,
    /// The last file that was ready (drawn until `current` is).
    shown: Option<ModelId>,
}

/// The streaming state inside [`Models`].
#[derive(Debug, Default)]
pub struct Streaming {
    pub manifest: Manifest,
    handles: Vec<Handle>,
    /// Page URLs asked for, in order (foreground and background).
    pub requested: Vec<String>,
    /// The boot group was started for this tier.
    boot_tier: Option<String>,
    /// Boot entries still downloading.
    pub boot_left: u32,
    /// Frames drawn since the boot group finished.
    frames_since_boot: u32,
    /// The later groups were started for this tier.
    later_tier: Option<String>, #[cfg(feature = "level-groups")] pub(crate) levels: level::Levels,
}

impl Streaming {
    fn manifest(&self) -> Option<&AssetManifest> {
        match &self.manifest {
            Manifest::Ready(m) => Some(m),
            _ => None,
        }
    }

    /// The manifest to hand to a fetch task.
    pub fn shared(&self) -> Option<Rc<AssetManifest>> {
        match &self.manifest {
            Manifest::Ready(m) => Some(m.clone()),
            _ => None,
        }
    }

    /// Note `url` as asked for; false when it already was.
    pub(crate) fn mark(&mut self, url: &str) -> bool {
        if self.requested.iter().any(|u| u == url) {
            return false;
        }
        self.requested.push(url.to_string());
        true
    }

    /// The boot group for `tier`, once (marked as requested).
    pub fn boot_plan(&mut self, tier: &str) -> Vec<evani_core::assets::AssetEntry> {
        if self.boot_tier.is_some() {
            return Vec::new();
        }
        let Some(m) = self.shared() else { return Vec::new() };
        self.boot_tier = Some(tier.to_string());
        let v: Vec<_> = stream::plan(&m, tier, true).into_iter().map(|i| m.assets[i].clone()).filter(|e| self.mark(&stream::url_of(e))).collect();
        self.boot_left = v.len() as u32;
        v
    }

    /// The later groups for `tier`, once per tier: only after the boot
    /// group is done and a frame (the title) has been drawn since, and while
    /// no handle is still waiting for its own file (those come first).
    /// Files already asked for are left out.
    pub fn later_plan(&mut self, tier: &str, models: &[ModelState]) -> Vec<evani_core::assets::AssetEntry> {
        if self.boot_tier.is_none() || self.boot_left > 0 || self.frames_since_boot < 1 || self.later_tier.as_deref() == Some(tier) {
            return Vec::new();
        }
        let waiting = self.handles.iter().any(|h| h.current.is_some_and(|c| models.get(c.0 as usize) == Some(&ModelState::Loading)));
        let Some(m) = self.shared() else { return Vec::new() };
        if waiting {
            return Vec::new();
        }
        self.later_tier = Some(tier.to_string()); #[cfg(feature = "level-groups")] let m = level::without_levels(&m);
        stream::plan(&m, tier, false).into_iter().map(|i| m.assets[i].clone()).filter(|e| self.mark(&stream::url_of(e))).collect()
    }

    /// One frame drawn (counts frames since boot).
    pub fn frame(&mut self) {
        if self.boot_tier.is_some() && self.boot_left == 0 {
            self.frames_since_boot = self.frames_since_boot.saturating_add(1);
        }
    }
}

/// One file as the model path fetches it: through the cache when the
/// manifest lists it, a plain download otherwise.
pub async fn fetch_file(b: &dyn Backend, manifest: Option<Rc<AssetManifest>>, url: &str) -> Result<Vec<u8>, String> {
    match manifest.as_deref().and_then(|m| stream::entry_for_url(m, url)) {
        Some(e) => stream::get(b, e, false).await,
        None => b.fetch(url, None, false).await,
    }
}

impl Models { #[cfg(feature = "frustum-cull")] pub(crate) fn retain_draws(&mut self, keep: &mut dyn FnMut(&super::ModelInstance, [f32; 4]) -> bool) -> u32 { let before = self.draws.len(); let entries = &self.entries; self.draws.retain(|d| { let ((x, y, z), r) = entries.get(d.model.0 as usize).map_or(((0.0, 0.0, 0.0), 0.0), |e| ((e.sphere.0[0], e.sphere.0[1], e.sphere.0[2]), e.sphere.1)); keep(d, [x, y, z, r]) }); (before - self.draws.len()) as u32 } // S5-R2 frustum culling: drop draws `keep` refuses (draw, model-space bounding sphere [x, y, z, r], r 0 until read); returns how many; poses follow each draw's own `joints`.
    /// A handle to model `id` (a name such as `proof-figure`, files in
    /// `models/`), at once. The same id gives the same handle.
    pub fn handle(&mut self, id: &str) -> ModelHandle {
        let s = &mut self.stream;
        if let Some(i) = s.handles.iter().position(|h| h.id == id) {
            return ModelHandle(i as u32);
        }
        s.handles.push(Handle { id: id.to_string(), files: Vec::new(), current: None, shown: None });
        ModelHandle(s.handles.len() as u32 - 1)
    }

    /// Has the game asked for any handle (only then is the manifest read)?
    pub fn has_handles(&self) -> bool {
        !self.stream.handles.is_empty()
    }

    /// The model to draw for `h` this frame: the settled tier's file once
    /// it is ready, else the last file that was ready; `None` (draw the
    /// placeholder) before any is.
    pub fn resolve(&self, h: ModelHandle) -> Option<ModelId> {
        let hd = self.stream.handles.get(h.0 as usize)?;
        hd.current.filter(|&c| self.ready(c)).or(hd.shown.filter(|&c| self.ready(c)))
    }

    /// The handle's model failed for good (no file, a bad file): the
    /// placeholder stays. The reason is in the error log.
    pub fn handle_failed(&self, h: ModelHandle) -> bool {
        let Some(hd) = self.stream.handles.get(h.0 as usize) else { return false };
        hd.current.is_some_and(|c| matches!(self.state(c), Some(ModelState::Failed(_)))) && self.resolve(h).is_none()
    }

    /// The boot group is downloaded (a title screen waits for this only).
    pub fn boot_ready(&self) -> bool {
        matches!(self.stream.manifest, Manifest::Missing) || (self.stream.boot_tier.is_some() && self.stream.boot_left == 0)
    }

    /// Give the manifest (the browser reads `models/stream.json` itself;
    /// native runs and tests call this). `None`: there is none.
    pub fn set_manifest(&mut self, m: Option<AssetManifest>) {
        self.stream.manifest = m.map_or(Manifest::Missing, |m| Manifest::Ready(Rc::new(m)));
    }

    /// Point every handle at its file for the settled `tier`, asking for
    /// files not asked for yet. Waits until the manifest is known.
    pub fn sync_tier(&mut self, tier: &str) {
        if matches!(self.stream.manifest, Manifest::Unknown | Manifest::Asked) {
            return;
        }
        for i in 0..self.stream.handles.len() {
            let url = {
                let id = self.stream.handles[i].id.as_str();
                match self.stream.manifest() {
                    Some(m) => stream::model_file(m, id, tier).map(stream::url_of),
                    None => Some(format!("{}{id}.glb", stream::FOLDER)),
                }
            };
            let Some(url) = url else {
                let id = self.stream.handles[i].id.clone();
                if self.stream.handles[i].current.is_none() {
                    // A failed placeholder entry, so the reason shows once.
                    let mid = self.push(&id);
                    self.set_state(mid, ModelState::Failed(format!("{id}: not in {}", stream::MANIFEST_URL)));
                    self.stream.handles[i].current = Some(mid);
                }
                continue;
            }; #[cfg(feature = "level-groups")] if self.stream.levels.url_held(&url) { continue; }
            let known = self.stream.handles[i].files.iter().find(|f| f.0 == url).map(|f| f.1);
            let mid = match known {
                Some(m) => m,
                None => {
                    let m = self.load(&url);
                    self.stream.mark(&url);
                    self.stream.handles[i].files.push((url, m));
                    m
                }
            };
            self.stream.handles[i].current = Some(mid);
        }
        for i in 0..self.stream.handles.len() {
            if let Some(c) = self.stream.handles[i].current.filter(|&c| self.ready(c)) {
                self.stream.handles[i].shown = Some(c);
            }
        }
    }

    /// Every model's state, by id (for [`Streaming::later_plan`]).
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn states(&self) -> Vec<ModelState> {
        self.entries.iter().map(|e| e.state.clone()).collect()
    }

    /// Native runs: the files asked for since the last call (id and page
    /// URL), for the caller to read and pass to [`Models::deliver`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_requests(&mut self) -> Vec<(ModelId, String)> {
        core::mem::take(&mut self.fetch).into_iter().filter_map(|id| self.url(id).map(|u| (id, u.to_string()))).collect()
    }

    /// Native runs: a requested file's bytes (or why it could not be read).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn deliver(&mut self, id: ModelId, bytes: Result<&[u8], String>) {
        match bytes {
            Ok(b) => self.loaded(id, b),
            Err(e) => self.set_state(id, ModelState::Failed(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::stream::tests::{Fake, block_on, entry};
    use super::*;
    use crate::texture::DeviceTex;

    fn bytes(rel: &str) -> Vec<u8> {
        std::fs::read(format!("{}/../../{rel}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    /// The test's stand-in for the browser's model path: every asked-for
    /// file and every texture it samples go through [`fetch_file`], then the
    /// background plan.
    fn pump(models: &mut Models, b: &Fake, tier: &str) {
        for _ in 0..4 {
            models.sync_tier(tier);
            for e in models.stream.boot_plan(tier) {
                block_on(stream::get(b, &e, false)).unwrap();
                models.stream.boot_left -= 1;
            }
            let m = models.stream.shared();
            for (id, url) in models.take_requests() {
                let r = block_on(fetch_file(b, m.clone(), &url));
                models.deliver(id, r.as_deref().map_err(Clone::clone));
            }
            for (id, p) in core::mem::take(&mut models.pending) {
                let url = models.url(id).unwrap_or("").to_string();
                let folder = &url[..=url.rfind('/').unwrap()];
                for t in &p.textures {
                    let u = match t.source(DeviceTex { astc: false, bc: true, etc2: false }) {
                        crate::model::TexSource::Ktx2(u, _) | crate::model::TexSource::Uri(u) => crate::model::resolve_uri(folder, u),
                        _ => continue,
                    };
                    block_on(fetch_file(b, m.clone(), &u)).unwrap();
                }
                models.set_state(id, ModelState::Ready);
            }
            models.stream.frame();
            let states = models.states();
            for e in models.stream.later_plan(tier, &states) {
                block_on(stream::get(b, &e, true)).unwrap();
            }
        }
    }

    fn setup() -> (Models, Fake) {
        let low = bytes("tests/fixtures/models/crate.low.glb");
        let ultra = bytes("apps/sandbox/assets/models/proof-figure.glb");
        let mut b = Fake::default();
        let mut assets = vec![
            entry("hero.low.glb", &low, Some("low"), "later", 10),
            entry("hero.medium.glb", &low, Some("medium"), "later", 10),
            entry("hero.ultra.glb", &ultra, Some("ultra"), "later", 10),
            entry("hero.glb", &low, None, "later", 10),
            entry("shared.glb", &low, None, "later", 5),
            entry("logo.png", b"logo", None, "boot", 0),
            entry("level2.low.glb", &low, Some("low"), "later", 1),
            entry("level2.ultra.glb", &ultra, Some("ultra"), "later", 1),
        ];
        for t in ["base.jpg", "base.bc7.ktx2", "normal.png", "normal.bc7.ktx2"].into_iter().chain(cfg!(all(feature = "look-surface", not(feature = "relief"))).then_some("mr.png")) {
            let name = format!("proof-figure.{t}");
            let data = bytes(&format!("apps/sandbox/assets/models/{name}"));
            assets.push(entry(&name, &data, Some("ultra"), "later", 8));
            b.files.insert(format!("models/{name}"), data);
        }
        for e in &assets {
            b.files.entry(format!("models/{}", e.url)).or_insert_with(|| match e.name.as_str() {
                "logo.png" => b"logo".to_vec(),
                n if n.contains("ultra") => ultra.clone(),
                _ => low.clone(),
            });
        }
        let mut models = Models::default();
        models.set_manifest(Some(AssetManifest { version: 2, assets }));
        (models, b)
    }

    /// On Low, every download (the handle's file, boot and the background
    /// plan) is a Low or a shared file; Ultra's files are never asked for.
    /// After a switch to Ultra the handle keeps drawing Low's file until
    /// Ultra's is ready, and Ultra asks only for Ultra or shared files.
    #[test]
    fn low_requests_only_low_files() {
        let (mut models, b) = setup();
        let h = models.handle("hero");
        assert_eq!(models.handle("hero"), h, "same id, same handle");
        assert_eq!(models.resolve(h), None, "the placeholder first");
        pump(&mut models, &b, "low");
        let low_id = models.resolve(h).expect("Low's file drawn");
        assert_eq!(models.url(low_id), Some("models/hero.low.glb"));
        let log: Vec<String> = b.log.borrow().iter().map(|l| l.0.clone()).collect();
        assert!(!log.is_empty());
        for u in &log {
            assert!(!u.contains("ultra") && !u.contains("medium"), "Low downloaded {u} ({log:?})");
            assert_ne!(u, "models/hero.glb", "Low has its own hero file, so never the shared one");
        }
        for want in ["models/hero.low.glb", "models/logo.png", "models/shared.glb", "models/level2.low.glb"] {
            assert!(log.iter().any(|u| u == want), "{want} not fetched: {log:?}");
        }
        assert!(log.iter().position(|u| u == "models/hero.low.glb") < log.iter().position(|u| u == "models/level2.low.glb"), "the handle's own file before the background plan");
        assert!(b.log.borrow().iter().filter(|l| l.0.contains("level2")).all(|l| l.2), "background fetches ask for low priority");

        // A tier switch: Low's file stays on screen until Ultra's is ready.
        let before = b.log.borrow().len();
        models.sync_tier("ultra");
        assert_eq!(models.resolve(h), Some(low_id), "no pop to the placeholder during a switch");
        pump(&mut models, &b, "ultra");
        let ultra_id = models.resolve(h).unwrap();
        assert_eq!(models.url(ultra_id), Some("models/hero.ultra.glb"));
        let after: Vec<String> = b.log.borrow()[before..].iter().map(|l| l.0.clone()).collect();
        for u in &after {
            assert!(!u.contains(".low.") && !u.contains("medium"), "Ultra downloaded {u}");
        }
        assert!(after.iter().any(|u| u.contains("proof-figure.base")), "Ultra's textures: {after:?}");
    }

    #[test]
    fn a_game_without_a_manifest_draws_the_shared_file() {
        let mut models = Models::default();
        let h = models.handle("proof-figure");
        models.sync_tier("low");
        assert!(models.take_requests().is_empty(), "nothing before the manifest is known");
        models.set_manifest(None);
        assert!(models.boot_ready());
        models.sync_tier("low");
        assert_eq!(models.take_requests().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["models/proof-figure.glb"]);
        let mut m2 = Models::default();
        m2.set_manifest(Some(AssetManifest { version: 2, assets: vec![] }));
        let h2 = m2.handle("ghost");
        m2.sync_tier("low");
        assert!(m2.handle_failed(h2), "a handle the manifest does not list fails once, visibly");
        assert_eq!(models.resolve(h), None);
    }
}
