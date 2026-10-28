//! Evani metadata (docs/MODEL_FORMAT.md): anchors from `anchor:<name>` nodes
//! and the root `extras.evani` (format version, part tree, LOD thresholds,
//! texture sets). Unknown fields are kept in [`Evani::raw`] and ignored.

use crate::doc::{idx, list, num, text};
use crate::{Error, Gltf, Node};
use serde_json::Value;

/// The highest `extras.evani.format` this reader understands
/// (docs/MODEL_FORMAT.md). A newer format is refused, an older one reads.
pub const EVANI_FORMAT: u64 = 1;

/// A gameplay anchor: an empty node named `anchor:<name>`.
#[derive(Clone, Debug)]
pub struct Anchor {
    /// The name after `anchor:`.
    pub name: String,
    pub node: usize,
    /// The node's world matrix (column-major), file units.
    pub world: [f32; 16],
}

/// One part of the part tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub role: String,
    /// The parent part's name (`None` for a root part).
    pub parent: Option<String>,
    /// The mirror or repeat group the part belongs to.
    pub group: Option<String>,
    /// The mesh node (solid) that carries the part's geometry.
    pub solid: Option<String>,
    /// The part was dropped from this tier (too small to see).
    pub dropped: bool,
}

/// One level of detail inside a tier file.
#[derive(Clone, Debug, PartialEq)]
pub struct Lod {
    /// The root node of this level's subtree, by name.
    pub node: String,
    /// Its index, when the file has a node of that name.
    pub node_index: Option<usize>,
    /// Use this level while the model's projected height is at least this
    /// fraction of the screen height (levels run from most detailed down).
    pub min_screen: f32,
}

/// The per-device texture files for one glTF texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSet {
    /// Index into the file's `textures`.
    pub texture: usize,
    /// A readable name ("base", "normal").
    pub name: String,
    /// `(format, uri)`: format is `astc`, `bc7`, `etc2` or `rgba`.
    pub sets: Vec<(String, String)>,
}

/// The root `extras.evani`.
#[derive(Clone, Debug)]
pub struct Evani {
    pub format: u64,
    pub class: Option<String>,
    pub tier: Option<String>,
    pub parts: Vec<Part>,
    pub lods: Vec<Lod>,
    pub textures: Vec<TextureSet>,
    /// The whole object as written (provenance, musts, notes, later fields).
    pub raw: Value,
}

impl Evani {
    /// A part by name.
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.name == name)
    }

    /// The children of a part (by name), in file order.
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Part> + 'a {
        self.parts.iter().filter(move |p| p.parent.as_deref() == Some(name))
    }
}

pub(crate) fn anchors(nodes: &[Node], world: &[[f32; 16]]) -> Vec<Anchor> {
    nodes
        .iter()
        .zip(world)
        .enumerate()
        .filter_map(|(i, (n, w))| n.name.strip_prefix("anchor:").map(|a| Anchor { name: a.to_owned(), node: i, world: *w }))
        .collect()
}

pub(crate) fn parse(e: &Value, g: &Gltf) -> Result<Evani, Error> {
    if !e.is_object() {
        return Err(err!("E1", "evani: extras.evani is not an object"));
    }
    let format = e.get("format").and_then(Value::as_u64).ok_or(err!("E2", "evani: extras.evani has no format number"))?;
    if format == 0 || format > EVANI_FORMAT {
        return Err(err!("E3", "evani: extras.evani format is newer than this engine reads"));
    }
    let mut parts = Vec::new();
    for p in list(e, "parts")? {
        parts.push(Part {
            name: text(p, "name").ok_or(err!("E4", "evani: part without a name"))?,
            role: text(p, "role").unwrap_or_default(),
            parent: text(p, "parent"),
            group: text(p, "group"),
            solid: text(p, "solid"),
            dropped: p.get("dropped").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    let mut lods = Vec::new();
    for l in list(e, "lods")? {
        let node = text(l, "node").ok_or(err!("E5", "evani: LOD without a node name"))?;
        lods.push(Lod { node_index: g.node_by_name(&node), node, min_screen: num(l, "min_screen", 0.0) });
    }
    let mut textures = Vec::new();
    for t in list(e, "textures")? {
        let texture = idx(t, "texture").filter(|&i| i < g.textures.len()).ok_or(err!("E6", "evani: texture set for a missing texture"))?;
        let mut sets = Vec::new();
        if let Some(Value::Object(s)) = t.get("sets") {
            for (fmt, v) in s {
                let uri = v.as_str().map(str::to_owned).or_else(|| text(v, "uri")).ok_or(err!("E7", "evani: texture set without a file"))?;
                sets.push((fmt.clone(), uri));
            }
        }
        textures.push(TextureSet { texture, name: text(t, "name").unwrap_or_default(), sets });
    }
    Ok(Evani { format, class: text(e, "class"), tier: text(e, "tier"), parts, lods, textures, raw: e.clone() })
}
