//! Tiered streaming (feature `models`, S3-4): the asset manifest v2 rules
//! the model path follows, and the cache policy every streamed file goes
//! through.
//!
//! * The manifest is `models/stream.json` (`cargo xtask manifest <id>`
//!   writes it; `cargo xtask check` errors when it no longer matches the
//!   files). Its URLs are relative to its own folder.
//! * A tier downloads only its own files: an entry with a `tier` is fetched
//!   only on that tier; an entry without one is shared by every tier.
//! * `boot` entries are fetched at once; the title screen waits for them
//!   only. Later groups are fetched in the background by `priority` (higher
//!   first) once boot is done and the title is up.
//! * [`get`]: Cache Storage keyed by content hash for large files (re-fetched
//!   when the browser evicted it, or when the cached bytes no longer match
//!   the hash), a `Range` request for an entry packed inside a bigger file,
//!   and the hash checked on every download.
//!
//! The browser side is the `stream` shim (`js/stream.js`, a wasm-bindgen
//! snippet, so only games with `models` get it); native tests use a fake
//! [`Backend`]. Streaming is render only: the simulation never reads it.

use core::future::Future;
use core::pin::Pin;

use evani_core::assets::{AssetEntry, AssetManifest, content_hash_hex};

/// Where the manifest is, relative to the page.
pub const MANIFEST_URL: &str = "models/stream.json";
/// Folder the manifest's URLs are relative to.
pub const FOLDER: &str = "models/";
/// Files at least this big go into Cache Storage (smaller ones rely on the
/// HTTP cache, which keeps them well).
pub const CACHE_MIN: u64 = 64 * 1024;

/// A boxed local future.
pub type Fut<T> = Pin<Box<dyn Future<Output = T>>>;

/// What the browser does for the stream: the shim's functions, or a fake.
pub trait Backend {
    /// The bytes stored under `key`, if the browser still has them.
    fn cache_get(&self, key: &str) -> Fut<Option<Vec<u8>>>;
    /// Store `bytes` under `key` (best effort, not awaited).
    fn cache_put(&self, key: &str, bytes: &[u8]);
    /// Download `url` (a byte `range` `(offset, length)` of it when given);
    /// `low` asks for a low fetch priority (background prefetch).
    fn fetch(&self, url: &str, range: Option<(u64, u64)>, low: bool) -> Fut<Result<Vec<u8>, String>>;
}

/// Read `models/stream.json` (manifest v1 or v2) through `serde_json::Value`,
/// which the model reader already links, rather than a second derived
/// deserialiser (it saves several KB of wasm). Missing v2 fields take their
/// v1 defaults; `name`, `hash`, `size` and `url` are required.
pub fn manifest_from_text(text: &str) -> Result<AssetManifest, String> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let s = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
    let mut assets = Vec::new();
    for e in v.get("assets").and_then(Value::as_array).ok_or("stream.json: no assets list")? {
        let need = |k: &str| s(e, k).ok_or_else(|| format!("stream.json: an entry has no {k}"));
        assets.push(AssetEntry {
            name: need("name")?,
            hash: need("hash")?,
            size: e.get("size").and_then(Value::as_u64).ok_or("stream.json: an entry has no size")?,
            url: need("url")?,
            kind: s(e, "kind").unwrap_or_default(),
            tier: s(e, "tier"),
            priority: e.get("priority").and_then(Value::as_i64).unwrap_or(0) as i32,
            group: s(e, "group").unwrap_or_else(|| "boot".to_string()),
            offset: e.get("offset").and_then(Value::as_u64),
        });
    }
    Ok(AssetManifest { version: v.get("version").and_then(Value::as_u64).unwrap_or(1) as u32, assets })
}

/// Is `e` fetched on `tier`?
pub fn on_tier(e: &AssetEntry, tier: &str) -> bool {
    e.tier.as_deref().is_none_or(|t| t == tier)
}

/// A shared entry `tier` has its own copy of (`hero.glb` when
/// `hero.low.glb` is listed, `hero.base.jpg` when `hero.low.base.jpg` is):
/// that tier draws its own and never downloads the shared one.
pub fn superseded(m: &AssetManifest, e: &AssetEntry, tier: &str) -> bool {
    if e.tier.is_some() {
        return false;
    }
    // `<dir/><id>.<tier>.<rest>` for shared `<dir/><id>.<rest>`, compared
    // piece by piece (no formatting in shipped code).
    let start = e.name.rfind('/').map_or(0, |i| i + 1);
    let Some(dot) = e.name[start..].find('.').map(|d| start + d) else { return false };
    let (head, rest) = e.name.split_at(dot + 1);
    m.assets.iter().any(|o| {
        o.name.strip_prefix(head).and_then(|r| r.strip_prefix(tier)).and_then(|r| r.strip_prefix('.')) == Some(rest)
    })
}

/// The URL of `e` as the page fetches it.
pub fn url_of(e: &AssetEntry) -> String {
    crate::model::resolve_uri(FOLDER, &e.url)
}

/// The file a model handle draws on `tier`: `<id>.<tier>.glb`, else the
/// shared `<id>.glb`. `None` when the manifest has neither.
pub fn model_file<'a>(m: &'a AssetManifest, id: &str, tier: &str) -> Option<&'a AssetEntry> {
    let own = m.assets.iter().find(|e| e.name.strip_prefix(id).and_then(|r| r.strip_prefix('.')).and_then(|r| r.strip_suffix(".glb")) == Some(tier));
    own.or_else(|| m.assets.iter().find(|e| e.name.strip_prefix(id) == Some(".glb")))
}

/// The entry fetched from page URL `url`, if the manifest lists it.
pub fn entry_for_url<'a>(m: &'a AssetManifest, url: &str) -> Option<&'a AssetEntry> {
    m.assets.iter().find(|e| url_of(e) == url)
}

/// Indices of the entries in `group` (`boot`, or any later group when
/// `boot` is false) for `tier`, highest priority first (manifest order
/// among equals). The background plan leaves out `texture` entries: the
/// model that samples one fetches it, in the one format the device can
/// sample (a prefetch would download every format's set).
pub fn plan(m: &AssetManifest, tier: &str, boot: bool) -> Vec<usize> {
    let mut v: Vec<usize> = (0..m.assets.len())
        .filter(|&i| {
            let e = &m.assets[i];
            on_tier(e, tier) && !superseded(m, e, tier) && (e.group == "boot") == boot && (boot || e.kind != "texture")
        })
        .collect();
    // A plain stable insertion sort: a manifest lists a handful of files,
    // and the standard library's sort costs about 4 KB of wasm here.
    for k in 1..v.len() {
        let mut j = k;
        while j > 0 && m.assets[v[j - 1]].priority < m.assets[v[j]].priority {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
    v
}

/// One file through the cache: the cached copy when it is still there and
/// still matches the hash, else a download (checked against the hash, then
/// cached when large).
pub async fn get(b: &dyn Backend, e: &AssetEntry, low: bool) -> Result<Vec<u8>, String> {
    let cached = e.size >= CACHE_MIN;
    if cached
        && let Some(bytes) = b.cache_get(&e.hash).await
        && content_hash_hex(&bytes) == e.hash
    {
        return Ok(bytes);
    }
    let bytes = b.fetch(&url_of(e), e.offset.map(|o| (o, e.size)), low).await?;
    if content_hash_hex(&bytes) != e.hash {
        return Err(format!("{}: hash mismatch", e.name));
    }
    if cached {
        b.cache_put(&e.hash, &bytes);
    }
    Ok(bytes)
}

/// The browser [`Backend`]: the `stream` shim.
#[cfg(target_arch = "wasm32")]
pub mod web {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    use super::{Backend, Fut};

    #[wasm_bindgen(module = "/js/stream.js")]
    extern "C" {
        fn cache_get(key: &str) -> js_sys::Promise;
        fn cache_put(key: &str, bytes: &[u8]);
        fn get(url: &str, offset: f64, len: f64, low: bool) -> js_sys::Promise;
    }

    /// The shim as a [`Backend`].
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Shim;

    fn bytes(v: JsValue) -> Option<Vec<u8>> {
        v.dyn_into::<js_sys::Uint8Array>().ok().map(|a| a.to_vec())
    }

    impl Backend for Shim {
        fn cache_get(&self, key: &str) -> Fut<Option<Vec<u8>>> {
            let p = cache_get(key);
            Box::pin(async move { JsFuture::from(p).await.ok().and_then(bytes) })
        }

        fn cache_put(&self, key: &str, b: &[u8]) {
            cache_put(key, b);
        }

        fn fetch(&self, url: &str, range: Option<(u64, u64)>, low: bool) -> Fut<Result<Vec<u8>, String>> {
            let (o, n) = range.map_or((-1.0, 0.0), |(o, n)| (o as f64, n as f64));
            let p = get(url, o, n, low);
            Box::pin(async move {
                match JsFuture::from(p).await {
                    Ok(v) => bytes(v).ok_or_else(|| "not bytes".to_string()),
                    Err(e) => Err(e.as_string().unwrap_or_else(|| "a fetch error".to_string())),
                }
            })
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::task::{Context, Poll, Waker};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    /// Poll a future that never waits on anything real (every fake here
    /// resolves at once).
    pub fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = core::pin::pin!(f);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// A fake browser: files by URL, a Cache Storage that can be evicted,
    /// and a log of every download.
    #[derive(Default)]
    pub struct Fake {
        pub files: BTreeMap<String, Vec<u8>>,
        pub cache: RefCell<BTreeMap<String, Vec<u8>>>,
        pub log: Rc<RefCell<Vec<Download>>>,
    }

    /// One logged download: the url, its byte range and whether it came from the cache.
    pub type Download = (String, Option<(u64, u64)>, bool);

    impl Backend for Fake {
        fn cache_get(&self, key: &str) -> Fut<Option<Vec<u8>>> {
            let v = self.cache.borrow().get(key).cloned();
            Box::pin(async move { v })
        }

        fn cache_put(&self, key: &str, bytes: &[u8]) {
            self.cache.borrow_mut().insert(key.to_string(), bytes.to_vec());
        }

        fn fetch(&self, url: &str, range: Option<(u64, u64)>, low: bool) -> Fut<Result<Vec<u8>, String>> {
            self.log.borrow_mut().push((url.to_string(), range, low));
            let r = self.files.get(url).cloned().ok_or_else(|| format!("{url}: HTTP 404")).map(|b| match range {
                Some((o, n)) => b[o as usize..(o + n) as usize].to_vec(),
                None => b,
            });
            Box::pin(async move { r })
        }
    }

    pub fn entry(name: &str, bytes: &[u8], tier: Option<&str>, group: &str, priority: i32) -> AssetEntry {
        AssetEntry {
            name: name.to_string(),
            hash: content_hash_hex(bytes),
            size: bytes.len() as u64,
            url: name.to_string(),
            kind: String::new(),
            tier: tier.map(str::to_string),
            priority,
            group: group.to_string(),
            offset: None,
        }
    }

    /// The cache serves a large file without a download; once the browser
    /// evicts it (or it no longer matches its hash) the next read downloads
    /// it again and caches it again. Small files never enter the cache.
    #[test]
    fn evicted_cache_refetches() {
        let big = vec![7u8; CACHE_MIN as usize + 10];
        let small = vec![1u8; 100];
        let mut f = Fake::default();
        f.files.insert("models/big.glb".into(), big.clone());
        f.files.insert("models/small.glb".into(), small.clone());
        let (eb, es) = (entry("big.glb", &big, None, "later", 0), entry("small.glb", &small, None, "later", 0));
        assert_eq!(block_on(get(&f, &eb, false)).unwrap(), big);
        assert_eq!(block_on(get(&f, &eb, false)).unwrap(), big);
        assert_eq!(f.log.borrow().len(), 1, "the second read came from the cache");
        assert!(f.cache.borrow().contains_key(&eb.hash), "keyed by content hash");
        f.cache.borrow_mut().clear(); // the browser evicted it
        assert_eq!(block_on(get(&f, &eb, false)).unwrap(), big);
        assert_eq!(f.log.borrow().len(), 2, "evicted: downloaded again");
        f.cache.borrow_mut().insert(eb.hash.clone(), vec![0u8; 5]); // damaged
        assert_eq!(block_on(get(&f, &eb, false)).unwrap(), big);
        assert_eq!(f.log.borrow().len(), 3, "a cached copy that fails the hash is downloaded again");
        assert_eq!(f.cache.borrow().get(&eb.hash), Some(&big));
        block_on(get(&f, &es, false)).unwrap();
        block_on(get(&f, &es, false)).unwrap();
        assert_eq!(f.log.borrow().len(), 5, "small files are not put in Cache Storage");
        assert!(!f.cache.borrow().contains_key(&es.hash));
        // A wrong download is an error, never drawn.
        f.files.insert("models/small.glb".into(), vec![2u8; 100]);
        assert!(block_on(get(&f, &es, false)).unwrap_err().contains("hash mismatch"));
    }

    /// An entry packed inside a bigger file is fetched with a byte range.
    #[test]
    fn packed_entries_use_range_requests() {
        let pack: Vec<u8> = (0..=255u8).cycle().take(4000).collect();
        let mut f = Fake::default();
        f.files.insert("models/pack.bin".into(), pack.clone());
        let mut e = entry("b.ktx2", &pack[1000..1500], None, "later", 0);
        e.url = "pack.bin".into();
        e.offset = Some(1000);
        assert_eq!(block_on(get(&f, &e, true)).unwrap(), &pack[1000..1500]);
        assert_eq!(f.log.borrow()[0], ("models/pack.bin".to_string(), Some((1000, 500)), true));
    }

    #[test]
    fn plan_orders_by_priority_and_keeps_to_the_tier() {
        let m = AssetManifest {
            version: 2,
            assets: vec![
                entry("a.low.glb", b"a", Some("low"), "later", 1),
                entry("a.ultra.glb", b"b", Some("ultra"), "later", 9),
                entry("t.png", b"c", None, "later", 5),
                entry("logo.png", b"d", None, "boot", 0),
                AssetEntry { kind: "texture".into(), ..entry("a.low.base.bc7.ktx2", b"e", Some("low"), "later", 9) },
            ],
        };
        assert_eq!(plan(&m, "low", false), [2, 0], "textures wait for the model that samples them");
        assert_eq!(plan(&m, "ultra", false), [1, 2]);
        assert_eq!(plan(&m, "low", true), [3]);
        assert_eq!(model_file(&m, "a", "low").map(|e| e.name.as_str()), Some("a.low.glb"));
        assert!(model_file(&m, "a", "medium").is_none());
        assert!(model_file(&m, "t", "low").is_none());
    }
}
