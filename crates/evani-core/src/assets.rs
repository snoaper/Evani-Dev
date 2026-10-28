//! Asset manifest keyed by content hash, with an async loader and progress.
//!
//! The manifest (JSON, written by tooling) lists every asset with a logical
//! name, a 64-bit FNV-1a content hash, its byte size and URL. URLs normally
//! embed the hash so browsers can cache them forever. Loaded bytes are
//! verified against the hash.

use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// 64-bit FNV-1a. Used as the content hash (cache key / integrity check, not
/// a cryptographic guarantee).
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Lower-case 16-digit hex form of [`content_hash`].
pub fn content_hash_hex(bytes: &[u8]) -> String {
    format!("{:016x}", content_hash(bytes))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetEntry {
    /// Logical name, e.g. `textures/player.png`.
    pub name: String,
    /// 16 hex digits of the FNV-1a 64 content hash.
    pub hash: String,
    /// Size in bytes (for progress before the download starts).
    pub size: u64,
    /// Fetch URL, relative to the page.
    pub url: String,
    /// Free-form kind: `image`, `audio`, `mesh`, `json`, ...
    #[serde(default)]
    pub kind: String, #[cfg(feature = "stream")] #[doc = "v2: the only tier that fetches it (`None`: every tier)."] #[serde(default)] pub tier: Option<String>, #[cfg(feature = "stream")] #[doc = "v2: background fetch order, higher first."] #[serde(default)] pub priority: i32, #[cfg(feature = "stream")] #[doc = "v2: `boot` (the title waits for it) or a later group; v1 entries are `boot`."] #[serde(default = "boot")] pub group: String, #[cfg(feature = "stream")] #[doc = "v2: byte offset inside `url` for an entry packed in a bigger file (a Range request)."] #[serde(default)] pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AssetManifest {
    #[serde(default = "one")]
    pub version: u32,
    pub assets: Vec<AssetEntry>,
}

fn one() -> u32 {
    1
} #[cfg(feature = "stream")] fn boot() -> String { "boot".to_string() }

impl AssetManifest {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    pub fn total_bytes(&self) -> u64 {
        self.assets.iter().map(|a| a.size).sum()
    }
}

/// Handle to an asset in the store (index into the manifest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum AssetState {
    NotRequested,
    Loading,
    Ready(Rc<[u8]>),
    Failed(String),
}

/// Aggregate loading progress.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoadProgress {
    pub requested: u32,
    pub ready: u32,
    pub failed: u32,
    pub bytes_total: u64,
    pub bytes_done: u64,
}

impl LoadProgress {
    /// 0..1 by bytes (1.0 when nothing was requested).
    pub fn fraction(&self) -> f32 {
        if self.bytes_total == 0 {
            if self.requested == 0 || self.ready + self.failed == self.requested {
                1.0
            } else {
                0.0
            }
        } else {
            (self.bytes_done as f64 / self.bytes_total as f64).min(1.0) as f32
        }
    }

    pub fn finished(&self) -> bool {
        self.ready + self.failed == self.requested
    }
}

/// Where bytes come from: `fetch()` on the web, a map in tests.
pub trait AssetSource {
    fn fetch(&self, url: &str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>>>>;
}

/// The asset table and its states.
#[derive(Debug, Default)]
pub struct AssetStore {
    manifest: AssetManifest,
    by_name: BTreeMap<String, AssetId>,
    states: Vec<AssetState>,
    progress: LoadProgress,
}

impl AssetStore {
    pub fn new(manifest: AssetManifest) -> Self {
        let by_name = manifest
            .assets
            .iter()
            .enumerate()
            .map(|(i, a)| (a.name.clone(), AssetId(i as u32)))
            .collect();
        let states = vec![AssetState::NotRequested; manifest.assets.len()];
        Self {
            manifest,
            by_name,
            states,
            progress: LoadProgress::default(),
        }
    }

    pub fn id(&self, name: &str) -> Option<AssetId> {
        self.by_name.get(name).copied()
    }

    pub fn entry(&self, id: AssetId) -> Option<&AssetEntry> {
        self.manifest.assets.get(id.0 as usize)
    }

    pub fn state(&self, id: AssetId) -> Option<&AssetState> {
        self.states.get(id.0 as usize)
    }

    pub fn bytes(&self, id: AssetId) -> Option<Rc<[u8]>> {
        match self.states.get(id.0 as usize)? {
            AssetState::Ready(b) => Some(b.clone()),
            _ => None,
        }
    }

    pub fn progress(&self) -> LoadProgress {
        self.progress
    }

    /// Mark as loading; returns the URL to fetch, or `None` if unknown or
    /// already requested.
    pub fn begin(&mut self, id: AssetId) -> Option<String> {
        let i = id.0 as usize;
        if !matches!(self.states.get(i)?, AssetState::NotRequested) {
            return None;
        }
        self.states[i] = AssetState::Loading;
        self.progress.requested += 1;
        self.progress.bytes_total += self.manifest.assets[i].size;
        Some(self.manifest.assets[i].url.clone())
    }

    /// Record a finished fetch, verifying size-independent content hash.
    pub fn complete(&mut self, id: AssetId, result: Result<Vec<u8>, String>) {
        let i = id.0 as usize;
        let Some(entry) = self.manifest.assets.get(i) else {
            return;
        };
        if !matches!(self.states[i], AssetState::Loading) {
            return;
        }
        let state = match result {
            Ok(bytes) => {
                let got = content_hash_hex(&bytes);
                if got.eq_ignore_ascii_case(&entry.hash) {
                    self.progress.ready += 1;
                    self.progress.bytes_done += entry.size;
                    AssetState::Ready(Rc::from(bytes))
                } else {
                    self.progress.failed += 1;
                    self.progress.bytes_done += entry.size;
                    AssetState::Failed(format!(
                        "{}: hash mismatch (manifest {}, got {got})",
                        entry.name, entry.hash
                    ))
                }
            }
            Err(e) => {
                self.progress.failed += 1;
                self.progress.bytes_done += entry.size;
                AssetState::Failed(format!("{}: {e}", entry.name))
            }
        };
        self.states[i] = state;
    }

    /// Ids of every manifest entry, in manifest order.
    pub fn all_ids(&self) -> impl Iterator<Item = AssetId> + use<> {
        (0..self.manifest.assets.len() as u32).map(AssetId)
    }

    /// Failure messages (for the overlay).
    pub fn failures(&self) -> impl Iterator<Item = &str> {
        self.states.iter().filter_map(|s| match s {
            AssetState::Failed(m) => Some(m.as_str()),
            _ => None,
        })
    }
}

/// World resource: shared handle to the store, so async loads can write into
/// it while systems read it.
#[derive(Clone, Default)]
pub struct Assets {
    pub store: Rc<RefCell<AssetStore>>,
}

impl Assets {
    pub fn new(manifest: AssetManifest) -> Self {
        Self {
            store: Rc::new(RefCell::new(AssetStore::new(manifest))),
        }
    }

    pub fn progress(&self) -> LoadProgress {
        self.store.borrow().progress()
    }

    pub fn id(&self, name: &str) -> Option<AssetId> {
        self.store.borrow().id(name)
    }

    pub fn bytes(&self, id: AssetId) -> Option<Rc<[u8]>> {
        self.store.borrow().bytes(id)
    }

    /// Load `ids` sequentially from `source`. The future resolves when all
    /// are done; progress is observable throughout. On wasm, run it with
    /// `wasm_bindgen_futures::spawn_local` (see [`Assets::spawn_load`]).
    pub fn load(
        &self,
        source: Rc<dyn AssetSource>,
        ids: Vec<AssetId>,
    ) -> impl Future<Output = LoadProgress> + use<> {
        let store = self.store.clone();
        async move {
            for id in ids {
                let url = store.borrow_mut().begin(id);
                if let Some(url) = url {
                    let result = source.fetch(&url).await;
                    store.borrow_mut().complete(id, result);
                }
            }
            store.borrow().progress()
        }
    }

    /// Start loading every asset in the background (wasm).
    #[cfg(all(target_arch = "wasm32", not(evani_headless)))]
    pub fn spawn_load_all(&self) {
        let ids: Vec<AssetId> = self.store.borrow().all_ids().collect();
        let fut = self.load(Rc::new(crate::web::FetchSource), ids);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = fut.await;
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::task::{Context, Poll, Waker};

    /// Minimal executor for futures that never actually wait.
    pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
        let mut fut = core::pin::pin!(fut);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    struct MapSource(BTreeMap<String, Vec<u8>>);

    impl AssetSource for MapSource {
        fn fetch(&self, url: &str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>>>> {
            let r = self.0.get(url).cloned().ok_or_else(|| "404".to_string());
            Box::pin(async move { r })
        }
    }

    fn manifest() -> AssetManifest {
        let a = b"hello".to_vec();
        let b = b"world!".to_vec();
        AssetManifest::from_json(&format!(
            r#"{{"assets":[
                {{"name":"a.txt","hash":"{}","size":5,"url":"a.{}","kind":"text"}},
                {{"name":"b.txt","hash":"{}","size":6,"url":"b"}},
                {{"name":"bad.txt","hash":"0000000000000000","size":3,"url":"bad"}},
                {{"name":"missing.txt","hash":"0000000000000001","size":4,"url":"missing"}}
            ]}}"#,
            content_hash_hex(&a),
            content_hash_hex(&a),
            content_hash_hex(&b)
        ))
        .unwrap()
    }

    #[test]
    fn fnv1a_reference_values() {
        assert_eq!(content_hash(b""), 0xcbf29ce484222325);
        assert_eq!(content_hash(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(content_hash(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn loads_verifies_and_reports_progress() {
        let m = manifest();
        assert_eq!(m.total_bytes(), 18);
        let a_url = m.assets[0].url.clone();
        let assets = Assets::new(m);
        let mut files = BTreeMap::new();
        files.insert(a_url, b"hello".to_vec());
        files.insert("b".to_string(), b"world!".to_vec());
        files.insert("bad".to_string(), b"xyz".to_vec());
        let src: Rc<dyn AssetSource> = Rc::new(MapSource(files));

        assert_eq!(assets.progress().fraction(), 1.0);
        let ids: Vec<AssetId> = assets.store.borrow().all_ids().collect();
        let p = block_on(assets.load(src, ids));
        assert_eq!(p.requested, 4);
        assert_eq!(p.ready, 2);
        assert_eq!(p.failed, 2);
        assert!(p.finished());
        assert_eq!(p.fraction(), 1.0);

        let a = assets.id("a.txt").unwrap();
        assert_eq!(&*assets.bytes(a).unwrap(), b"hello");
        let store = assets.store.borrow();
        let fails: Vec<&str> = store.failures().collect();
        assert!(fails[0].contains("hash mismatch"));
        assert!(fails[1].contains("404"));
    }

    #[test]
    fn begin_is_idempotent() {
        let mut s = AssetStore::new(manifest());
        let id = s.id("b.txt").unwrap();
        assert!(s.begin(id).is_some());
        assert!(s.begin(id).is_none());
        assert_eq!(s.progress().requested, 1);
        assert_eq!(s.progress().fraction(), 0.0);
        assert!(s.begin(AssetId(99)).is_none());
    }

    /// A v1 manifest (no `tier`, `priority`, `group` or `offset`) reads as
    /// before: every entry is shared by all tiers, in the boot group, at
    /// priority 0, a whole file. v2 fields read when present.
    #[cfg(feature = "stream")]
    #[test]
    fn manifest_v1_still_reads() {
        let m = manifest();
        assert_eq!(m.version, 1);
        for e in &m.assets {
            assert_eq!((e.tier.as_deref(), e.priority, e.group.as_str(), e.offset), (None, 0, "boot", None), "{}", e.name);
        }
        assert_eq!(m.assets[0].kind, "text");
        let v2 = AssetManifest::from_json(
            r#"{"version":2,"assets":[{"name":"hero.low.glb","hash":"00","size":9,"url":"hero.low.glb","tier":"low","priority":10,"group":"level1","offset":64}]}"#,
        )
        .unwrap();
        let e = &v2.assets[0];
        assert_eq!((v2.version, e.tier.as_deref(), e.priority, e.group.as_str(), e.offset), (2, Some("low"), 10, "level1", Some(64)));
        // A v2 manifest round-trips.
        assert_eq!(AssetManifest::from_json(&serde_json::to_string(&v2).unwrap()).unwrap(), v2);
    }
}
