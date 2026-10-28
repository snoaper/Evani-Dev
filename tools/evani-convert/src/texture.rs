//! Texture size per tier, what each image is for, and the per-device sets
//! (ASTC 4x4 for phones, BC7 for desktops) beside a plain PNG or JPEG core.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// The camera's vertical field of view (evani-core's default camera, degrees).
pub const FOV_Y_DEG: f64 = 50.0;
/// The screen height a tier without `target_resolution` is sized for: the
/// bench's reference window (1920 x 1080, tools/bench-runner).
pub const REFERENCE_HEIGHT: f64 = 1080.0;

/// One tier's screen, from assets/tiers/<tier>.json.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub tier: String,
    /// `target_resolution` height, or the bench's 1080.
    pub height: f64,
    pub height_from: &'static str,
    pub render_scale: f64,
    pub dpr_cap: f64,
    /// Megabytes (`max_texture_memory`).
    pub max_texture_mb: Option<f64>,
    pub max_draw_calls: Option<u64>,
    pub max_triangles: Option<u64>,
}

impl Screen {
    pub fn from_json(tier: &str, j: &Value) -> Screen {
        let res = j.get("target_resolution").and_then(Value::as_array).and_then(|a| a.get(1)).and_then(Value::as_f64);
        Screen {
            tier: tier.to_owned(),
            height: res.unwrap_or(REFERENCE_HEIGHT),
            height_from: if res.is_some() { "target_resolution" } else { "bench reference 1080" },
            render_scale: j.get("render_scale").and_then(Value::as_f64).unwrap_or(1.0),
            dpr_cap: j.get("dpr_cap").and_then(Value::as_f64).unwrap_or(1.0),
            max_texture_mb: j.get("max_texture_memory").and_then(Value::as_f64),
            max_draw_calls: j.get("max_draw_calls").and_then(Value::as_u64),
            max_triangles: j.get("max_triangles").and_then(Value::as_u64),
        }
    }

    pub fn load(root: &Path, tier: &str) -> Result<Screen, String> {
        let p = root.join("assets").join("tiers").join(format!("{tier}.json"));
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e} (tiers come from assets/tiers/*.json)", p.display()))?;
        let j: Value = serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(Screen::from_json(tier, &j))
    }

    /// Rendered pixels from the top of the screen to the bottom.
    pub fn pixels(&self) -> f64 {
        self.height * self.render_scale * self.dpr_cap
    }
}

pub fn pow2_ceil(x: f64) -> u32 {
    let mut p = 1u32;
    while f64::from(p) < x && p < (1 << 15) {
        p <<= 1;
    }
    p
}

/// How a tier's texture size was chosen (for the report).
#[derive(Clone, Debug, PartialEq)]
pub struct SizeChoice {
    pub tier: String,
    pub screen_px: f64,
    /// Share of the screen height the model covers at its closest (1 when unknown).
    pub fraction: f64,
    pub floor: u32,
    pub cap: u32,
    pub size: u32,
}

/// The tier's texture size for a model of `extent` metres (its largest box
/// side) seen from `min_distance` metres at the closest: the on-screen
/// pixels it covers, rounded up to a power of two, between the tier's floor
/// (cap / 16) and cap (the power of two at or above the screen height).
pub fn size_for(screen: &Screen, extent: f64, min_distance: Option<f64>) -> SizeChoice {
    let px = screen.pixels();
    let cap = pow2_ceil(px);
    let floor = (cap / 16).max(1);
    let fraction = match min_distance {
        Some(d) if d > 0.0 && extent > 0.0 => (extent / (2.0 * d * (FOV_Y_DEG.to_radians() / 2.0).tan())).min(1.0),
        _ => 1.0,
    };
    let size = pow2_ceil(fraction * px).clamp(floor, cap);
    SizeChoice { tier: screen.tier.clone(), screen_px: px, fraction, floor, cap, size }
}

/// What an image holds, from the material slot that first uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Base,
    Emissive,
    Normal,
    MetalRough,
    Occlusion,
    /// A relief height map, parked in a free material slot while the file is
    /// cleaned and packed (see `look::park_relief`).
    Height,
    Other,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Base => "base",
            Role::Emissive => "emissive",
            Role::Normal => "normal",
            Role::MetalRough => "metal_rough",
            Role::Occlusion => "occlusion",
            Role::Height => "height",
            Role::Other => "map",
        }
    }
    /// Colour (sRGB) rather than data (linear).
    pub fn srgb(self) -> bool {
        matches!(self, Role::Base | Role::Emissive)
    }
}

/// Per image: its role and whether a material blends or masks with it.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageUse {
    pub image: usize,
    pub role: Role,
    pub alpha: bool,
    /// A short unique name (`base`, `normal`, `base_1` ...).
    pub name: String,
}

impl ImageUse {
    /// PNG for normal maps and blended or masked colour; JPEG for the rest.
    pub fn core_ext(&self) -> &'static str {
        if matches!(self.role, Role::Normal | Role::Height) || self.alpha { "png" } else { "jpg" }
    }
}

/// The role of every image the materials use, in image order.
pub fn image_uses(json: &Value) -> Vec<ImageUse> {
    let textures = json.get("textures").and_then(Value::as_array).cloned().unwrap_or_default();
    let n_images = json.get("images").and_then(Value::as_array).map_or(0, Vec::len);
    let mut role: BTreeMap<usize, (Role, bool)> = BTreeMap::new();
    let slots: [(&str, Role); 5] = [
        ("/pbrMetallicRoughness/baseColorTexture/index", Role::Base),
        ("/emissiveTexture/index", Role::Emissive),
        ("/normalTexture/index", Role::Normal),
        ("/pbrMetallicRoughness/metallicRoughnessTexture/index", Role::MetalRough),
        ("/occlusionTexture/index", Role::Occlusion),
    ];
    for m in json.get("materials").and_then(Value::as_array).into_iter().flatten() {
        let alpha = m.get("alphaMode").and_then(Value::as_str).is_some_and(|a| a != "OPAQUE");
        // The slot holding a parked relief height map (it is a height map, whatever the slot is called).
        let parked = m.pointer("/extras/evani/relief/height/parked").and_then(Value::as_str);
        for (ptr, r) in slots {
            let r = if parked.is_some_and(|p| ptr.strip_suffix("/index") == Some(p)) { Role::Height } else { r };
            let Some(t) = m.pointer(ptr).and_then(Value::as_u64) else { continue };
            let Some(src) = textures.get(t as usize).and_then(|t| t.get("source")).and_then(Value::as_u64) else { continue };
            let e = role.entry(src as usize).or_insert((r, false));
            if r == Role::Base && alpha {
                e.1 = true;
            }
        }
    }
    let mut taken: BTreeMap<&'static str, usize> = BTreeMap::new();
    (0..n_images)
        .map(|i| {
            let (r, alpha) = role.get(&i).copied().unwrap_or((Role::Other, false));
            let n = taken.entry(r.name()).or_insert(0);
            let name = if *n == 0 { r.name().to_owned() } else { format!("{}_{n}", r.name()) };
            *n += 1;
            ImageUse { image: i, role: r, alpha, name }
        })
        .collect()
}

/// `ktx create` arguments for one set (`astc` directly, `uastc` as the
/// step before `ktx transcode --target bc7`). No supercompression: the
/// engine (S3-3) reads plain KTX2 levels.
pub fn ktx_create_args(set: &str, srgb: bool, png: &str, out: &str) -> Vec<String> {
    let tf = if srgb { "srgb" } else { "linear" };
    let mut a: Vec<String> = vec!["create".into()];
    match set {
        "astc" => a.extend(["--format".into(), if srgb { "ASTC_4x4_SRGB_BLOCK" } else { "ASTC_4x4_UNORM_BLOCK" }.into(), "--astc-quality".into(), "medium".into()]),
        _ => a.extend(["--format".into(), if srgb { "R8G8B8A8_SRGB" } else { "R8G8B8A8_UNORM" }.into(), "--encode".into(), "uastc".into(), "--uastc-quality".into(), "2".into()]),
    }
    a.extend(["--assign-tf".into(), tf.into(), "--generate-mipmap".into(), "--threads".into(), "1".into(), png.into(), out.into()]);
    a
}

/// GPU bytes of a KTX2 file's levels (its size less the header; close enough
/// for a budget: the header and index are a few hundred bytes).
pub fn ktx2_level_bytes(bytes: &[u8]) -> u64 {
    // KTX2: 48-byte header and a 32-byte index, then per level
    // { byteOffset, byteLength, uncompressedByteLength } as u64s from byte 80.
    let rd = |o: usize| bytes.get(o..o + 8).map_or(0, |b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]));
    let levels = bytes.get(40..44).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).max(1) as usize;
    (0..levels).map(|l| rd(80 + l * 24 + 8)).sum()
}

/// The engine's KTX2 rules (evani-render, S3-3) and the `vkFormat` reader:
/// one copy, in evani-guide, which `cargo xtask check` uses too.
pub use evani_guide::modelcheck::{ktx2_check, ktx2_vk_format};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roles_and_names_come_from_the_materials() {
        let j = json!({
            "materials": [
                {"normalTexture": {"index": 0}, "pbrMetallicRoughness": {"baseColorTexture": {"index": 1}, "metallicRoughnessTexture": {"index": 2}}},
                {"alphaMode": "BLEND", "pbrMetallicRoughness": {"baseColorTexture": {"index": 3}}}
            ],
            "textures": [{"source": 0}, {"source": 1}, {"source": 2}, {"source": 3}],
            "images": [{}, {}, {}, {}, {}]
        });
        let u = image_uses(&j);
        let names: Vec<&str> = u.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["normal", "base", "metal_rough", "base_1", "map"]);
        let ext: Vec<&str> = u.iter().map(ImageUse::core_ext).collect();
        assert_eq!(ext, ["png", "jpg", "jpg", "png", "jpg"]);
    }

    #[test]
    fn a_picture_shared_with_a_colour_map_keeps_the_colour_encoding() {
        // One picture gets one encoding; park_relief notes the share, and the
        // colour map (which the look draws directly) wins.
        let j = json!({
            "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}, "occlusionTexture": {"index": 1}, "extras": {"evani": {"relief": {"height": {"index": 1, "parked": "/occlusionTexture"}}}}}],
            "textures": [{"source": 0}, {"source": 0}],
            "images": [{}]
        });
        let u = image_uses(&j);
        assert_eq!((u[0].role, u[0].role.srgb()), (Role::Base, true));
    }

    #[test]
    fn a_parked_height_map_is_a_linear_png_called_height() {
        let j = json!({
            "materials": [{"normalTexture": {"index": 0}, "occlusionTexture": {"index": 1}, "extras": {"evani": {"relief": {"height": {"index": 1, "parked": "/occlusionTexture"}}}}}],
            "textures": [{"source": 0}, {"source": 1}],
            "images": [{}, {}]
        });
        let u = image_uses(&j);
        assert_eq!((u[1].name.as_str(), u[1].role.srgb(), u[1].core_ext()), ("height", false, "png"));
    }
}
