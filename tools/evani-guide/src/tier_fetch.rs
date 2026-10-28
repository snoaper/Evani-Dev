//! Which streamed files a player's browser fetches on one tier with one
//! texture family (S7-1, S7-2, S7-6), worked out from `stream.json` and the
//! models' own texture lists, the same rules the engine follows
//! (`evani-render` `model::stream` and `texture::choose_format`):
//!
//! * a file with a `tier` is fetched only on that tier; a shared file is
//!   skipped on a tier that has its own copy (`hero.glb` when
//!   `hero.low.glb` is listed);
//! * every `boot` file the tier keeps is fetched at once, and every later
//!   model and data file in the background;
//! * a texture is fetched only by a model that samples it, in the device's
//!   family: the model's `extras.evani.textures[].sets` file for `astc` or
//!   `bc7`, else the image's own PNG or JPEG.

use std::collections::BTreeMap;

use serde_json::Value;

/// The texture families a device ends up with (ASTC on phones, BC7 on
/// desktops, the PNG/JPEG core image everywhere else).
pub const FAMILIES: [&str; 3] = ["astc", "bc7", "rgba"];

/// One `stream.json` entry, as far as fetching goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub group: String,
    pub tier: Option<String>,
    pub kind: String,
    pub size: u64,
}

/// One fetched file and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetch {
    pub name: String,
    pub group: String,
    /// `model`, `data`, `boot` or `texture of <model>`.
    pub why: String,
}

/// The entries of a `stream.json` text.
pub fn entries(manifest: &str) -> Result<Vec<Entry>, String> {
    let v: Value = serde_json::from_str(manifest).map_err(|e| format!("stream.json: {e}"))?;
    let s = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
    Ok(v.get("assets")
        .and_then(Value::as_array)
        .ok_or("stream.json: no assets list")?
        .iter()
        .map(|e| Entry {
            name: s(e, "name").unwrap_or_default(),
            group: s(e, "group").unwrap_or_else(|| "boot".to_string()),
            tier: s(e, "tier"),
            kind: s(e, "kind").unwrap_or_default(),
            size: e.get("size").and_then(Value::as_u64).unwrap_or(0),
        })
        .collect())
}

fn on_tier(e: &Entry, tier: &str) -> bool {
    e.tier.as_deref().is_none_or(|t| t == tier)
}

/// The engine's `superseded`: a shared `<dir/><id>.<rest>` when
/// `<dir/><id>.<tier>.<rest>` is listed.
pub fn superseded(all: &[Entry], e: &Entry, tier: &str) -> bool {
    if e.tier.is_some() {
        return false;
    }
    let start = e.name.rfind('/').map_or(0, |i| i + 1);
    let Some(dot) = e.name[start..].find('.').map(|d| start + d) else {
        return false;
    };
    let (head, rest) = e.name.split_at(dot + 1);
    all.iter().any(|o| {
        o.name
            .strip_prefix(head)
            .and_then(|r| r.strip_prefix(tier))
            .and_then(|r| r.strip_prefix('.'))
            == Some(rest)
    })
}

/// The family a texture file belongs to by its name (`.astc.ktx2`,
/// `.bc7.ktx2`, else `rgba`), or `None` for a file that is no texture.
pub fn family_of(name: &str) -> Option<&'static str> {
    let n = name
        .split(['?', '#'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    if n.ends_with(".astc.ktx2") {
        Some("astc")
    } else if n.ends_with(".bc7.ktx2") {
        Some("bc7")
    } else if [".png", ".jpg", ".jpeg", ".webp", ".ktx2"]
        .iter()
        .any(|x| n.ends_with(x))
    {
        Some("rgba")
    } else {
        None
    }
}

/// `dir/` of a manifest name (empty at the top).
fn dir_of(name: &str) -> &str {
    name.rfind('/').map_or("", |i| &name[..=i])
}

/// The texture files a model's glTF JSON samples on `family`, as manifest
/// names (relative to the models folder).
pub fn model_textures(gltf: &Value, model_name: &str, family: &str) -> Vec<String> {
    let images = gltf
        .get("images")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let textures = gltf
        .get("textures")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // image index -> the set file for this family.
    let mut sets: BTreeMap<usize, String> = BTreeMap::new();
    for t in gltf
        .pointer("/extras/evani/textures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(img) = t
            .get("texture")
            .and_then(Value::as_u64)
            .and_then(|i| textures.get(i as usize))
            .and_then(|x| x.get("source"))
            .and_then(Value::as_u64)
        else {
            continue;
        };
        let set = t.get("sets").and_then(|s| s.get(family));
        let uri = set
            .and_then(Value::as_str)
            .or_else(|| set.and_then(|s| s.get("uri")).and_then(Value::as_str));
        if let Some(u) = uri {
            sets.insert(img as usize, u.to_string());
        }
    }
    // The engine samples only base colour, normal and emissive maps, so only
    // those images are fetched (crates/evani-render/src/model.rs).
    let mut used: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for m in gltf
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for p in [
            "/pbrMetallicRoughness/baseColorTexture/index",
            "/normalTexture/index",
            "/emissiveTexture/index",
        ] {
            if let Some(src) = m
                .pointer(p)
                .and_then(Value::as_u64)
                .and_then(|i| textures.get(i as usize))
                .and_then(|x| x.get("source"))
                .and_then(Value::as_u64)
            {
                used.insert(src as usize);
            }
        }
    }
    let dir = dir_of(model_name);
    let mut out = Vec::new();
    for (i, im) in images.iter().enumerate() {
        if !used.contains(&i) {
            continue;
        }
        let Some(uri) = sets
            .get(&i)
            .cloned()
            .or_else(|| im.get("uri").and_then(Value::as_str).map(str::to_string))
        else {
            continue;
        };
        if uri.starts_with("data:") {
            continue;
        }
        let name = format!("{dir}{uri}");
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The files `tier` fetches with `family`, boot first, in manifest order.
/// `gltf_json(name)` gives a model file's glTF JSON (`None`: not readable,
/// its textures are then left out and named in the second list).
pub fn plan(
    all: &[Entry],
    tier: &str,
    family: &str,
    gltf_json: &mut dyn FnMut(&str) -> Option<Value>,
) -> (Vec<Fetch>, Vec<String>) {
    let mut out: Vec<Fetch> = Vec::new();
    let mut unread = Vec::new();
    let keep: Vec<&Entry> = all
        .iter()
        .filter(|e| on_tier(e, tier) && !superseded(all, e, tier))
        .collect();
    let push = |out: &mut Vec<Fetch>, name: &str, group: &str, why: String| {
        if !out.iter().any(|f| f.name == name) {
            out.push(Fetch {
                name: name.to_string(),
                group: group.to_string(),
                why,
            });
        }
    };
    for boot in [true, false] {
        for e in keep.iter().filter(|e| (e.group == "boot") == boot) {
            if boot {
                // The boot plan fetches every boot file the tier keeps; a
                // boot texture of another family is still fetched.
                push(&mut out, &e.name, &e.group, "boot".to_string());
            } else if e.kind != "texture" {
                push(&mut out, &e.name, &e.group, e.kind.clone());
            }
            if e.kind == "model" {
                match gltf_json(&e.name) {
                    Some(j) => {
                        for t in model_textures(&j, &e.name, family) {
                            let group = all
                                .iter()
                                .find(|x| x.name == t)
                                .map_or_else(|| "later".to_string(), |x| x.group.clone());
                            push(&mut out, &t, &group, format!("texture of {}", e.name));
                        }
                    }
                    None => unread.push(e.name.clone()),
                }
            }
        }
    }
    (out, unread)
}

/// The glTF JSON of a `.glb` or `.gltf` file's bytes.
pub fn gltf_of(name: &str, bytes: &[u8]) -> Option<Value> {
    if name.ends_with(".gltf") {
        return serde_json::from_slice(bytes).ok();
    }
    let (json, _) = evani_gltf::split_glb(bytes).ok()?;
    serde_json::from_slice(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn e(name: &str, group: &str, kind: &str) -> Entry {
        Entry {
            name: name.into(),
            group: group.into(),
            tier: crate::stream::tier_of(name).map(str::to_string),
            kind: kind.into(),
            size: 1,
        }
    }

    fn manifest() -> Vec<Entry> {
        vec![
            e("hero.glb", "later", "model"),
            e("hero.low.glb", "later", "model"),
            e("hero.base.jpg", "later", "texture"),
            e("hero.base.astc.ktx2", "later", "texture"),
            e("hero.base.bc7.ktx2", "later", "texture"),
            e("hero.low.base.jpg", "later", "texture"),
            e("logo.png", "boot", "texture"),
        ]
    }

    fn gltf(name: &str) -> Option<Value> {
        Some(match name {
            "hero.glb" => json!({
                "images": [{ "uri": "hero.base.jpg" }, { "uri": "hero.mr.png" }], "textures": [{ "source": 0 }, { "source": 1 }],
                "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicRoughnessTexture": { "index": 1 } } }],
                "extras": { "evani": { "textures": [{ "texture": 0, "sets": { "astc": "hero.base.astc.ktx2", "bc7": { "uri": "hero.base.bc7.ktx2" } } }] } }
            }),
            "hero.low.glb" => {
                json!({ "images": [{ "uri": "hero.low.base.jpg" }], "textures": [{ "source": 0 }],
                    "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }] })
            }
            _ => return None,
        })
    }

    fn names(tier: &str, family: &str) -> Vec<String> {
        plan(&manifest(), tier, family, &mut gltf)
            .0
            .into_iter()
            .map(|f| f.name)
            .collect()
    }

    #[test]
    fn each_tier_and_family_fetches_only_its_own() {
        assert_eq!(
            names("ultra", "astc"),
            ["logo.png", "hero.glb", "hero.base.astc.ktx2"]
        );
        assert_eq!(
            names("ultra", "bc7"),
            ["logo.png", "hero.glb", "hero.base.bc7.ktx2"]
        );
        assert_eq!(
            names("ultra", "rgba"),
            ["logo.png", "hero.glb", "hero.base.jpg"]
        );
        // Low has its own model, whose texture has no compressed set.
        assert_eq!(
            names("low", "astc"),
            ["logo.png", "hero.low.glb", "hero.low.base.jpg"]
        );
        assert_eq!(family_of("a.base.astc.ktx2"), Some("astc"));
        assert_eq!(family_of("a.base.BC7.ktx2"), Some("bc7"));
        assert_eq!(family_of("a.mr.png"), Some("rgba"));
        assert_eq!(family_of("a.glb"), None);
    }

    #[test]
    fn metallic_roughness_only_images_are_not_fetched() {
        for family in ["astc", "bc7", "rgba"] {
            assert!(!names("ultra", family).iter().any(|n| n.contains(".mr.")));
        }
    }

    #[test]
    fn unreadable_models_are_named() {
        let (_, unread) = plan(&manifest(), "ultra", "bc7", &mut |_| None);
        assert_eq!(unread, ["hero.glb"]);
    }
}
