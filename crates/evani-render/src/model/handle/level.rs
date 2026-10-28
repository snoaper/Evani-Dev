//! Per-level asset groups (feature `level-groups`, S7-7).
//!
//! A game with several levels lists each level's files under a group
//! `level:<name>` in `models/stream.json` (`cargo xtask manifest` writes it
//! from `[stream.levels]` in evani.toml). Such a group is never part of the
//! background plan: nothing of a level is downloaded until the game asks.
//!
//! * [`Models::prefetch`] downloads a group ahead, at a low fetch priority:
//!   its models, data and the one texture set this device samples (the same
//!   rule the models follow), for the settled tier only. Models whose group
//!   was released come back (their handles load again, from the cache).
//! * [`Models::release`] frees what the group's models hold: their GPU
//!   textures, buffers and bind groups, and their place in the table. A
//!   released model's handle draws the kit's placeholder until the group is
//!   prefetched again; the downloaded files stay in Cache Storage, so coming
//!   back to a level costs no network.
//! * A model that is asked for directly (a handle made when the level
//!   starts) still loads on its own, as before; `prefetch` only makes that
//!   a cache hit.
//!
//! The GPU side (`gpu::models`) reports each upload's texture bytes here
//! ([`Levels::uploaded`]) and, once a frame, drops what [`Levels::take_sweep`]
//! names. The simulation never reads any of it.

use std::rc::Rc;

use evani_core::assets::{AssetEntry, AssetManifest};

use super::super::stream;
use super::super::{ModelId, ModelState, Models};
use super::Streaming;
use crate::texture::{DeviceTex, choose_format, set_name};

/// Every level group's name starts with this.
pub const PREFIX: &str = "level:";

/// Is `group` a level group?
pub fn is_level(group: &str) -> bool {
    group.starts_with(PREFIX)
}

/// What the level groups of one [`Streaming`] hold.
#[derive(Debug, Default)]
pub struct Levels {
    /// Groups the game asked to prefetch, until the settled tier can plan them.
    wanted: Vec<String>,
    /// Page URLs of released groups: a handle on one of them stays idle.
    held_urls: Vec<String>,
    /// Released models; anything that still arrives for them is dropped.
    held: Vec<ModelId>,
    /// GPU texture bytes of each uploaded model.
    bytes: Vec<(ModelId, u64)>,
    /// A held model got something new to drop.
    dirty: bool,
}

impl Levels {
    /// The GPU side uploaded `id`'s textures: `bytes` of texture memory.
    pub fn uploaded(&mut self, id: ModelId, bytes: u64) {
        match self.bytes.iter_mut().find(|b| b.0 == id) {
            Some(b) => b.1 = bytes,
            None => self.bytes.push((id, bytes)),
        }
        self.dirty |= self.is_held(id);
    }

    /// Was `id` released?
    pub fn is_held(&self, id: ModelId) -> bool {
        self.held.contains(&id)
    }

    /// Is `url` a file of a released group?
    pub fn url_held(&self, url: &str) -> bool {
        self.held_urls.iter().any(|u| u == url)
    }

    /// Texture bytes the models that are not released hold on the GPU.
    pub fn resident(&self) -> u64 {
        self.bytes
            .iter()
            .filter(|b| !self.is_held(b.0))
            .map(|b| b.1)
            .sum()
    }

    /// Once per change: the released models whose GPU copies the renderer
    /// drops now, and the texture bytes that frees. Idempotent.
    pub fn take_sweep(&mut self) -> (Vec<ModelId>, u64) {
        if !core::mem::take(&mut self.dirty) {
            return (Vec::new(), 0);
        }
        let held = &self.held;
        let freed = self
            .bytes
            .iter()
            .filter(|b| held.contains(&b.0))
            .map(|b| b.1)
            .sum();
        self.bytes.retain(|b| !held.contains(&b.0));
        (self.held.clone(), freed)
    }
}

/// The manifest without its level groups (what the background plan walks).
pub fn without_levels(m: &Rc<AssetManifest>) -> Rc<AssetManifest> {
    if !m.assets.iter().any(|e| is_level(&e.group)) {
        return m.clone();
    }
    Rc::new(AssetManifest {
        version: m.version,
        assets: m
            .assets
            .iter()
            .filter(|e| !is_level(&e.group))
            .cloned()
            .collect(),
    })
}

/// The texture set a KTX2 entry belongs to: `astc` for `hero.base.astc.ktx2`.
fn set_of(name: &str) -> Option<&str> {
    name.strip_suffix(".ktx2")
        .and_then(|n| n.rsplit('.').next())
}

/// Indices of the entries to download ahead for `group` on `tier`, highest
/// priority first: models and data, and the textures of the one set the
/// device samples (`dev`), as the plain image when it samples none of the
/// listed sets. Entries `tier` does not fetch, or has its own copy of, are
/// left out.
pub fn group_plan(m: &AssetManifest, group: &str, tier: &str, dev: DeviceTex) -> Vec<usize> {
    let sets = m
        .assets
        .iter()
        .filter(|e| e.group == group && e.kind == "texture")
        .filter_map(|e| set_of(&e.name));
    let want = set_name(choose_format(sets, dev));
    let mut v: Vec<usize> = (0..m.assets.len())
        .filter(|&i| {
            let e = &m.assets[i];
            let texture_ok =
                e.kind != "texture" || set_of(&e.name).map_or(want == "rgba", |s| s == want);
            e.group == group
                && stream::on_tier(e, tier)
                && !stream::superseded(m, e, tier)
                && texture_ok
        })
        .collect();
    for k in 1..v.len() {
        let mut j = k;
        while j > 0 && m.assets[v[j - 1]].priority < m.assets[v[j]].priority {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
    v
}

impl Streaming {
    /// The files of the groups the game asked to prefetch, for the settled
    /// `tier` and a device with `dev` (the renderer fetches them in the
    /// background, once). Files already asked for are left out.
    pub fn take_level_prefetch(&mut self, tier: &str, dev: DeviceTex) -> Vec<AssetEntry> {
        let Some(m) = self.shared() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for g in core::mem::take(&mut self.levels.wanted) {
            for i in group_plan(&m, &g, tier, dev) {
                if self.mark(&stream::url_of(&m.assets[i])) {
                    out.push(m.assets[i].clone());
                }
            }
        }
        out
    }
}

impl Models {
    /// Ask for level group `group` (`level:<name>`) to be downloaded ahead,
    /// in the background. A group released before is let back in: its
    /// handles load again, from the cache. Nothing is fetched until the
    /// tier is settled; asking twice costs nothing.
    pub fn prefetch(&mut self, group: &str) {
        if let Some(m) = self.stream.shared() {
            let held = &mut self.stream.levels.held_urls;
            held.retain(|u| {
                !m.assets
                    .iter()
                    .any(|e| e.group == group && stream::url_of(e) == *u)
            });
        }
        let wanted = &mut self.stream.levels.wanted;
        if !wanted.iter().any(|w| w == group) {
            wanted.push(group.to_string());
        }
    }

    /// Free level group `group`: the GPU textures, buffers and bind groups
    /// of its models, and their handles' links to them. Returns the texture
    /// bytes this will free. The handles draw the placeholder (and stay
    /// idle) until [`Models::prefetch`] lets the group back in. The
    /// downloaded files stay in Cache Storage.
    pub fn release(&mut self, group: &str) -> u64 {
        let Some(m) = self.stream.shared() else {
            return 0;
        };
        let urls: Vec<String> = m
            .assets
            .iter()
            .filter(|e| e.group == group)
            .map(stream::url_of)
            .collect();
        let mut freed = 0;
        let mut gone = Vec::new();
        for i in 0..self.entries.len() {
            let id = ModelId(i as u32);
            if !urls.contains(&self.entries[i].url) || self.stream.levels.is_held(id) {
                continue;
            }
            self.entries[i].state = ModelState::Failed("released".to_string());
            freed += self
                .stream
                .levels
                .bytes
                .iter()
                .filter(|b| b.0 == id)
                .map(|b| b.1)
                .sum::<u64>();
            self.stream.levels.held.push(id);
            gone.push(id);
        }
        self.stream.levels.dirty = true;
        self.fetch.retain(|f| !gone.contains(f));
        self.pending.retain(|p| !gone.contains(&p.0));
        for h in &mut self.stream.handles {
            h.files.retain(|f| !gone.contains(&f.1));
            h.current = h.current.filter(|c| !gone.contains(c));
            h.shown = h.shown.filter(|c| !gone.contains(c));
        }
        let s = &mut self.stream;
        s.requested.retain(|r| !urls.contains(r));
        for u in urls {
            if !s.levels.held_urls.contains(&u) {
                s.levels.held_urls.push(u);
            }
        }
        s.levels.wanted.retain(|w| w != group);
        freed
    }

    /// Texture bytes the GPU holds for the models that are not released.
    pub fn texture_bytes(&self) -> u64 {
        self.stream.levels.resident()
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::stream::tests::{Fake, block_on, entry};
    use super::*;

    const BC: DeviceTex = DeviceTex {
        astc: false,
        bc: true,
        etc2: false,
    };

    fn glb() -> Vec<u8> {
        std::fs::read(format!(
            "{}/../../tests/fixtures/models/crate.low.glb",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// A game with a hub (always there) and two levels.
    fn manifest() -> AssetManifest {
        let g = glb();
        let tex = |n: &str, grp: &str, tier: Option<&str>| AssetEntry {
            kind: "texture".into(),
            ..entry(n, n.as_bytes(), tier, grp, 5)
        };
        AssetManifest {
            version: 2,
            assets: vec![
                entry("logo.png", b"logo", None, "boot", 0),
                AssetEntry {
                    kind: "model".into(),
                    ..entry("hub.glb", &g, None, "later", 10)
                },
                AssetEntry {
                    kind: "model".into(),
                    ..entry("tree.glb", &g, None, "level:forest", 10)
                },
                AssetEntry {
                    kind: "model".into(),
                    ..entry("tree.low.glb", &g, Some("low"), "level:forest", 10)
                },
                tex("tree.base.bc7.ktx2", "level:forest", None),
                tex("tree.base.astc.ktx2", "level:forest", None),
                tex("tree.low.base.bc7.ktx2", "level:forest", Some("low")),
                AssetEntry {
                    kind: "model".into(),
                    ..entry("rock.glb", &g, None, "level:cave", 10)
                },
            ],
        }
    }

    fn models() -> Models {
        let mut m = Models::default();
        m.set_manifest(Some(manifest()));
        m
    }

    /// Stand in for the browser: read what was asked for and "upload" it
    /// with `bytes` of texture memory each.
    fn load_all(m: &mut Models, bytes: u64) -> Vec<ModelId> {
        let mut ids = Vec::new();
        for (id, url) in m.take_requests() {
            let file = url.strip_prefix("models/").unwrap().to_string();
            let e = manifest()
                .assets
                .into_iter()
                .find(|e| e.url == file)
                .unwrap();
            let data = if e.kind == "model" {
                glb()
            } else {
                e.name.as_bytes().to_vec()
            };
            m.deliver(id, Ok(&data));
            ids.push(id);
        }
        for (id, _) in core::mem::take(&mut m.pending) {
            m.set_state(id, ModelState::Ready);
            m.stream.levels.uploaded(id, bytes);
        }
        ids
    }

    /// Releasing a level frees the texture memory its models held, and
    /// nothing else: the hub keeps its textures and stays drawn.
    #[test]
    fn release_frees_texture_memory() {
        let mut m = models();
        let (hub, tree, rock) = (m.handle("hub"), m.handle("tree"), m.handle("rock"));
        m.sync_tier("low");
        let ids = load_all(&mut m, 4096);
        assert_eq!(ids.len(), 3);
        assert!(m.resolve(hub).is_some() && m.resolve(tree).is_some() && m.resolve(rock).is_some());
        assert_eq!(m.texture_bytes(), 3 * 4096);

        let freed = m.release("level:forest");
        assert_eq!(freed, 4096, "the forest's textures");
        assert_eq!(m.resolve(tree), None, "the placeholder again");
        assert!(!m.handle_failed(tree), "released is not a failure");
        assert!(
            m.resolve(hub).is_some() && m.resolve(rock).is_some(),
            "other groups are untouched"
        );
        let (drop, bytes) = m.stream.levels.take_sweep();
        assert_eq!(
            (drop.len(), bytes),
            (1, 4096),
            "the renderer drops one model and 4096 bytes"
        );
        assert_eq!(
            m.texture_bytes(),
            2 * 4096,
            "memory went down by the level's share"
        );
        assert_eq!(m.stream.levels.take_sweep(), (Vec::new(), 0), "once");

        // Releasing again frees nothing more.
        assert_eq!(m.release("level:forest"), 0);
        assert_eq!(m.release("level:nothing"), 0);
        m.release("level:cave");
        assert_eq!(m.stream.levels.take_sweep().1, 4096);
        assert_eq!(m.texture_bytes(), 4096, "only the hub is left");
    }

    /// A released group's handles stay idle (no re-download every frame),
    /// something that arrives late for it is dropped, and `prefetch` lets
    /// it back in with a fresh load.
    #[test]
    fn released_groups_stay_released_until_prefetched() {
        let mut m = models();
        let tree = m.handle("tree");
        m.sync_tier("low");
        let first = m.take_requests();
        assert_eq!(first.len(), 1);
        let old = first[0].0;
        m.release("level:forest");
        m.sync_tier("low");
        assert!(m.take_requests().is_empty(), "idle after a release");
        // The file arrives after the release: dropped, never parsed.
        m.deliver(old, Ok(&glb()));
        assert!(m.pending.is_empty() && !m.ready(old));
        m.stream.levels.uploaded(old, 999);
        assert_eq!(m.texture_bytes(), 0, "a late upload is not counted");
        assert_eq!(
            m.stream.levels.take_sweep(),
            (vec![old], 999),
            "and is swept"
        );

        m.prefetch("level:forest");
        m.sync_tier("low");
        let again = m.take_requests();
        assert_eq!(again.len(), 1);
        assert_ne!(again[0].0, old, "a fresh load");
        assert_eq!(again[0].1, "models/tree.low.glb");
        load_all_ready(&mut m, again[0].0);
        assert!(m.resolve(tree).is_some());
    }

    fn load_all_ready(m: &mut Models, id: ModelId) {
        m.deliver(id, Ok(&glb()));
        for (id, _) in core::mem::take(&mut m.pending) {
            m.set_state(id, ModelState::Ready);
        }
    }

    /// A level group is not in the background plan, and `prefetch` lists
    /// only the settled tier's files and the one texture set the device uses.
    #[test]
    fn a_level_downloads_only_when_asked() {
        let mut m = models();
        m.sync_tier("low");
        assert!(m.stream.boot_plan("low").len() == 1);
        m.stream.boot_left = 0;
        m.stream.frame();
        let states = m.states();
        let later: Vec<String> = m
            .stream
            .later_plan("low", &states)
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(
            later,
            ["hub.glb"],
            "no level file in the background plan: {later:?}"
        );

        assert!(
            m.stream.take_level_prefetch("low", BC).is_empty(),
            "nothing before the game asks"
        );
        m.prefetch("level:forest");
        m.prefetch("level:forest");
        let asked: Vec<String> = m
            .stream
            .take_level_prefetch("low", BC)
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(
            asked,
            ["tree.low.glb", "tree.low.base.bc7.ktx2"],
            "Low's model, its own texture, BC7 only: {asked:?}"
        );
        assert!(m.stream.take_level_prefetch("low", BC).is_empty(), "once");
        // ASTC phone: the shared set, the other tier's names stay out.
        m.prefetch("level:cave");
        let cave: Vec<String> = m
            .stream
            .take_level_prefetch(
                "low",
                DeviceTex {
                    astc: true,
                    bc: false,
                    etc2: false,
                },
            )
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(cave, ["rock.glb"]);

        let man = manifest();
        let plan = group_plan(
            &man,
            "level:forest",
            "medium",
            DeviceTex {
                astc: true,
                bc: false,
                etc2: false,
            },
        );
        let names: Vec<&str> = plan.iter().map(|&i| man.assets[i].name.as_str()).collect();
        assert_eq!(
            names,
            ["tree.glb", "tree.base.astc.ktx2"],
            "Medium has no file of its own: the shared model, the ASTC set"
        );
    }

    /// A late prefetch (before the manifest is known) is kept until it can
    /// be planned.
    #[test]
    fn prefetch_before_the_manifest_waits() {
        let mut m = Models::default();
        m.prefetch("level:forest");
        assert!(m.stream.take_level_prefetch("low", BC).is_empty());
        m.set_manifest(Some(manifest()));
        assert_eq!(m.stream.take_level_prefetch("low", BC).len(), 2);
    }

    /// The fake backend still serves a level file through the cache.
    #[test]
    fn a_prefetched_file_is_a_cache_hit_later() {
        let g = vec![9u8; crate::model::stream::CACHE_MIN as usize + 1];
        let mut f = Fake::default();
        f.files.insert("models/tree.glb".into(), g.clone());
        let e = entry("tree.glb", &g, None, "level:forest", 10);
        block_on(stream::get(&f, &e, true)).unwrap();
        block_on(stream::get(&f, &e, false)).unwrap();
        assert_eq!(f.log.borrow().len(), 1, "the prefetch is the only download");
    }
}
