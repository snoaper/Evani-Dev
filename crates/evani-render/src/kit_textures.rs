//! Kit textures (feature `kit-textures`, TopGun W3): tiling PBR texture
//! arrays and a game-written macro texture for the kit material hooks
//! (`kit_material` / `kit_surface`), so ground and terrain shaders get photo
//! detail up close with hardware filtering, mips and anisotropy.
//!
//! * Two **sets** (0 "ground", 1 "hero") of up to [`MAX_LAYERS`] layers. A
//!   set is two `texture_2d_array`s: `ah` (albedo sRGB + height in A) and
//!   `nra` (normal X, Y in the OpenGL convention, roughness, AO). Every
//!   layer of a set has the same size and format; the array index is the
//!   layer number, picked per pixel by the kit code.
//! * Files are plain KTX2 (no supercompression, full mip chain) in the game
//!   folder: `<path>_ah.<size>.<fmt>.ktx2` and `<path>_nra.<size>.<fmt>.ktx2`,
//!   `<fmt>` = `astc` (ASTC 4x4), `bc7` or `rgba` (RGBA8, half the size, for
//!   devices without block compression). The tier picks `<size>`
//!   ([`TextureSetDesc::size_for`], 0 = the set stays off on that tier); the
//!   device picks `<fmt>` (ASTC, else BC7, else RGBA). Nothing is fetched
//!   before the tier settles, so a tier change at warm-up costs no download.
//! * The sets come from `kit-textures.json` in the game folder (read once at
//!   start; `cargo xtask check` reads the same file) or from code through
//!   [`KitTextures::set`]. Code wins for a set it has set.
//! * The **macro** texture ([`KitTextures::macro_array`]) is a game-written
//!   RGBA8 `texture_2d_array` with engine-made mips. [`MacroArray::write_rect`]
//!   is safe every frame: only the rect is uploaded, and only its parent
//!   texels are rebuilt on each mip level (on the GPU, one small pass per
//!   level).
//! * WGSL: [`BINDING_WGSL`] is appended next to `kit_data` in the forward
//!   module and in the shadow / AO module (same layout, so the same kit code
//!   compiles in both; `KIT_SHADOW_PASS` tells them apart). Unregistered sets
//!   and the macro bind 1 x 1 x 1 zero dummies, so kit code may always sample.
//!
//! Games without the feature compile none of this and their shaders are
//! byte for byte what they were.

use crate::texture::{DeviceTex, TexFormat};

/// Texture sets (0 ground, 1 hero).
pub const MAX_SETS: usize = 2;
/// Layers per set (array layers).
pub const MAX_LAYERS: usize = 16;
/// The manifest in the game folder (served next to `index.html`).
pub const MANIFEST_URL: &str = "kit-textures.json";
/// Largest forward module with kit code and these bindings (bytes of WGSL
/// source). A bigger one is reported in the ErrorLog when it is built, and
/// `cargo xtask check` errors on kit WGSL that can't fit, so a runaway kit
/// shader shows up before it is a slow pipeline build on a phone.
pub const MAX_SHADER_BYTES: usize = 60 * 1024;
/// Largest macro texture side.
pub const MAX_MACRO_SIZE: u32 = 4096;
/// Most macro layers.
pub const MAX_MACRO_LAYERS: u32 = 4;

/// The bindings, appended after `kit_data` (group 1, binding 15).
pub const BINDING_WGSL: &str = "@group(1) @binding(16) var kit_tex0_ah: texture_2d_array<f32>;
@group(1) @binding(17) var kit_tex0_nra: texture_2d_array<f32>;
@group(1) @binding(18) var kit_tex1_ah: texture_2d_array<f32>;
@group(1) @binding(19) var kit_tex1_nra: texture_2d_array<f32>;
@group(1) @binding(20) var kit_macro: texture_2d_array<f32>;
@group(1) @binding(21) var kit_wrap: sampler;
@group(1) @binding(22) var kit_clamp: sampler;
@group(1) @binding(23) var<uniform> kit_tex_state: array<vec4<f32>, 8>;
fn kit_tex_ready(tex_set: u32, layer: u32) -> f32 {
    let i = min(tex_set, 1u) * 16u + min(layer, 15u);
    return kit_tex_state[i >> 2u][i & 3u];
}
";
/// The forward module's pass flag.
pub const FORWARD_FLAG_WGSL: &str = "const KIT_SHADOW_PASS: bool = false;\n";
/// The shadow / AO module's pass flag.
pub const SHADOW_FLAG_WGSL: &str = "const KIT_SHADOW_PASS: bool = true;\n";

/// One of a layer's two maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Map {
    /// Albedo (sRGB) + height (A).
    Ah,
    /// Normal X, Y + roughness + AO (linear).
    Nra,
}

impl Map {
    /// The file-name suffix.
    pub fn suffix(self) -> &'static str {
        match self {
            Map::Ah => "_ah",
            Map::Nra => "_nra",
        }
    }

    /// Whether the map holds sRGB colour.
    pub fn srgb(self) -> bool {
        self == Map::Ah
    }
}

/// The device set name of a format (`astc`, `bc7`, `rgba`).
pub fn format_name(f: TexFormat) -> &'static str {
    match f {
        TexFormat::Astc4x4 => "astc",
        TexFormat::Bc7 => "bc7",
        _ => "rgba",
    }
}

/// The format a device fetches: ASTC 4x4, else BC7, else RGBA8 (the set
/// list in the manifest says which sets were shipped; RGBA is always the
/// last resort).
pub fn pick_format(dev: DeviceTex, shipped: &[TexFormat]) -> TexFormat {
    if dev.astc && shipped.contains(&TexFormat::Astc4x4) {
        TexFormat::Astc4x4
    } else if dev.bc && shipped.contains(&TexFormat::Bc7) {
        TexFormat::Bc7
    } else {
        TexFormat::Rgba
    }
}

/// The URL of one map of one layer at one size and format.
pub fn file_url(path: &str, map: Map, size: u32, f: TexFormat) -> String {
    format!("{path}{}.{size}.{}.ktx2", map.suffix(), format_name(f))
}

/// The anisotropy a tier samples with when the game sets none.
pub fn default_anisotropy(tier: &str) -> u16 {
    match tier {
        "lite" | "low" => 1,
        "medium" => 4,
        "high" | "vr" => 8,
        "ultra" => 16,
        _ => 4,
    }
}

/// One layer: its name (for logs and the check) and its file path without
/// the `_ah` / `_nra` suffix and the size and format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSrc {
    pub name: String,
    pub path: String,
}

impl LayerSrc {
    pub fn new(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self { name: name.into(), path: path.into() }
    }
}

/// A texture set: its size per tier id and its layers (array index =
/// position).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextureSetDesc {
    /// `(tier id, texels per side)`; a tier not listed, or 0, loads nothing.
    pub sizes: Vec<(String, u32)>,
    pub layers: Vec<LayerSrc>,
    /// The boot size (W3c streaming): every wanted layer loads at this size
    /// first, then the whole set swaps to the tier's size once each of them
    /// is in. 0, or not below the tier's size: one step at the tier's size.
    pub boot: u32,
}

impl TextureSetDesc {
    /// Builder: the size on one tier.
    pub fn tier(mut self, tier: &str, size: u32) -> Self {
        self.sizes.retain(|(t, _)| t != tier);
        self.sizes.push((tier.to_owned(), size));
        self
    }

    /// Builder: the boot size (see [`TextureSetDesc::boot`]).
    pub fn boot(mut self, size: u32) -> Self {
        self.boot = size;
        self
    }

    /// The boot size on `tier` (0 = no boot step there).
    pub fn boot_for(&self, tier: &str) -> u32 {
        if self.boot > 0 && self.boot < self.size_for(tier) { self.boot } else { 0 }
    }

    /// Builder: one more layer.
    pub fn layer(mut self, name: &str, path: &str) -> Self {
        self.layers.push(LayerSrc::new(name, path));
        self
    }

    /// The size the set loads at on `tier` (0 = off).
    pub fn size_for(&self, tier: &str) -> u32 {
        self.sizes.iter().find(|(t, _)| t == tier).map_or(0, |(_, s)| *s)
    }

    /// Why the set can't be used, if it can't.
    pub fn validate(&self) -> Result<(), String> {
        if self.layers.is_empty() || self.layers.len() > MAX_LAYERS {
            return Err(format!("a texture set has {} layers (1 to {MAX_LAYERS})", self.layers.len()));
        }
        for (t, s) in &self.sizes {
            if *s != 0 && (!s.is_power_of_two() || *s < 8 || *s > 8192) {
                return Err(format!("set size {s} on tier {t} is not a power of two from 8 to 8192"));
            }
        }
        if self.boot != 0 && (!self.boot.is_power_of_two() || self.boot < 8 || self.boot > 8192) {
            return Err(format!("boot size {} is not a power of two from 8 to 8192", self.boot));
        }
        Ok(())
    }
}

/// Where one layer stands (for status lines, tests and [`KitTextures::boot_done`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LayerStage {
    /// Not loading: the set is off on this tier, the layer isn't wanted, or
    /// the tier hasn't settled yet.
    #[default]
    Off,
    /// Queued: waiting for the boot wave or for [`KitTextures::hold_full`].
    Waiting,
    /// Fetching; nothing of it on the GPU yet.
    Loading,
    /// At its boot size (the set is still bound at the boot size).
    Coarse,
    /// At the tier's size.
    Full,
    /// A file failed (the ErrorLog says which); the layer stays at ready 0.
    Failed,
}

/// The two waves of a set's loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    /// The boot size.
    Boot,
    /// The tier's size.
    Full,
}

/// Which arrays a set binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Boot,
    Full,
}

/// One fetch to start: layer, map, wave and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fetch {
    pub layer: usize,
    pub map: Map,
    pub wave: Wave,
    pub size: u32,
}

/// The loading plan of one set at one tier size (no GPU types, so the rules
/// are tested natively; `gpu::kit_textures` runs it).
///
/// * With a boot size, every wanted layer fetches its two boot maps first;
///   the set binds the boot arrays and each layer fades in as its pair lands.
/// * Once every wanted layer has its boot pair (or failed), and unless
///   [`KitTextures::hold_full`] is on, the tier-size maps follow. When every
///   wanted layer has its tier-size pair, the set swaps to those arrays in
///   one go (so no layer ever drops back) and the boot arrays are freed.
/// * Without a boot size the tier-size maps load at once and each layer
///   shows as its pair lands (W3a's behaviour).
/// * A layer wanted after the swap fetches its tier-size pair only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetStream {
    pub size: u32,
    pub boot: u32,
    pub layers: usize,
    /// Per layer: asked / got for boot AH, boot NRA, full AH, full NRA.
    asked: [[bool; 4]; MAX_LAYERS],
    got: [[bool; 4]; MAX_LAYERS],
    failed: [bool; MAX_LAYERS],
    swapped: bool,
}

fn slot(map: Map, wave: Wave) -> usize {
    usize::from(wave == Wave::Full) * 2 + usize::from(map == Map::Nra)
}

impl SetStream {
    pub fn new(size: u32, boot: u32, layers: usize) -> Self {
        Self { size, boot: if boot < size { boot } else { 0 }, layers: layers.min(MAX_LAYERS), ..Default::default() }
    }

    fn wanted(&self, want: u16) -> impl Iterator<Item = usize> + '_ {
        (0..self.layers).filter(move |l| want & (1 << l) != 0)
    }

    fn pair(&self, l: usize, wave: Wave) -> bool {
        self.got[l][slot(Map::Ah, wave)] && self.got[l][slot(Map::Nra, wave)]
    }

    /// The arrays to bind.
    pub fn bound(&self) -> Bound {
        if self.boot == 0 || self.swapped { Bound::Full } else { Bound::Boot }
    }

    /// Whether the boot wave is over for the `want`ed layers.
    fn boot_wave_done(&self, want: u16) -> bool {
        self.boot == 0 || self.swapped || self.wanted(want).all(|l| self.failed[l] || self.pair(l, Wave::Boot))
    }

    /// The fetches to start now (each is returned once).
    pub fn next(&mut self, want: u16, hold: bool) -> Vec<Fetch> {
        let mut out = Vec::new();
        if self.size == 0 {
            return out;
        }
        let full_ok = self.boot == 0 || self.swapped || (!hold && self.boot_wave_done(want));
        let wanted: Vec<usize> = self.wanted(want).filter(|l| !self.failed[*l]).collect();
        for l in wanted {
            let mut waves = Vec::with_capacity(2);
            if self.boot > 0 && !self.swapped {
                waves.push((Wave::Boot, self.boot));
            }
            if full_ok {
                waves.push((Wave::Full, self.size));
            }
            for (wave, size) in waves {
                for map in [Map::Ah, Map::Nra] {
                    let a = &mut self.asked[l][slot(map, wave)];
                    if !*a {
                        *a = true;
                        out.push(Fetch { layer: l, map, wave, size });
                    }
                }
            }
        }
        out
    }

    /// A file arrived and is uploaded. Returns whether the set just swapped
    /// to its tier-size arrays.
    pub fn got(&mut self, layer: usize, map: Map, wave: Wave, want: u16) -> bool {
        if layer >= self.layers {
            return false;
        }
        self.got[layer][slot(map, wave)] = true;
        self.try_swap(want)
    }

    /// A file failed. Returns whether the set just swapped.
    pub fn fail(&mut self, layer: usize, want: u16) -> bool {
        if layer < self.layers {
            self.failed[layer] = true;
        }
        self.try_swap(want)
    }

    fn try_swap(&mut self, want: u16) -> bool {
        if self.boot == 0 || self.swapped {
            return false;
        }
        let mut any = false;
        for l in self.wanted(want) {
            if self.failed[l] {
                continue;
            }
            if !self.pair(l, Wave::Full) {
                return false;
            }
            any = true;
        }
        self.swapped = any;
        any
    }

    /// Whether the layer shows (both maps in the bound arrays).
    pub fn showing(&self, layer: usize) -> bool {
        layer < self.layers
            && !self.failed[layer]
            && match self.bound() {
                Bound::Boot => self.pair(layer, Wave::Boot),
                Bound::Full => self.pair(layer, Wave::Full),
            }
    }

    /// The layer's stage.
    pub fn stage(&self, layer: usize, want: u16) -> LayerStage {
        if layer >= self.layers || self.size == 0 {
            return LayerStage::Off;
        }
        if self.failed[layer] {
            return LayerStage::Failed;
        }
        let shown = self.showing(layer);
        if shown && self.bound() == Bound::Full {
            return LayerStage::Full;
        }
        if shown {
            return LayerStage::Coarse;
        }
        if want & (1 << layer) == 0 {
            return LayerStage::Off;
        }
        if self.asked[layer].iter().any(|a| *a) { LayerStage::Loading } else { LayerStage::Waiting }
    }
}

/// The macro texture's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroDesc {
    /// Texels per side (a power of two).
    pub size: u32,
    /// Array layers.
    pub layers: u32,
}

/// One pending write: layer, `[x, y, w, h]` at level 0, RGBA8 rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroWrite {
    pub layer: u32,
    pub rect: [u32; 4],
    pub data: Vec<u8>,
}

/// The game-written macro texture (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroArray {
    desc: MacroDesc,
    pending: Vec<MacroWrite>,
}

impl MacroArray {
    pub fn desc(&self) -> MacroDesc {
        self.desc
    }

    /// Write a whole layer (`size * size * 4` bytes, rows top down).
    /// Replaces every earlier pending write to that layer.
    pub fn write(&mut self, layer: u32, rgba: &[u8]) -> Result<(), String> {
        let s = self.desc.size;
        self.write_rect(layer, 0, 0, s, s, rgba)
    }

    /// Write a rect of one layer (`w * h * 4` bytes). Uploaded before the
    /// next frame with its mips rebuilt over the rect only, so it is safe
    /// to call every frame.
    pub fn write_rect(&mut self, layer: u32, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
        let s = self.desc.size;
        if layer >= self.desc.layers {
            return Err(format!("macro write to layer {layer}: the macro has {}", self.desc.layers));
        }
        if w == 0 || h == 0 || x.checked_add(w).is_none_or(|e| e > s) || y.checked_add(h).is_none_or(|e| e > s) {
            return Err(format!("macro write rect {x},{y} {w}x{h} is outside {s}x{s}"));
        }
        if rgba.len() as u64 != u64::from(w) * u64::from(h) * 4 {
            return Err(format!("macro write {w}x{h} needs {} bytes, got {}", u64::from(w) * u64::from(h) * 4, rgba.len()));
        }
        // A write covering an earlier one's rect makes it moot.
        self.pending.retain(|p| !(p.layer == layer && covers([x, y, w, h], p.rect)));
        self.pending.push(MacroWrite { layer, rect: [x, y, w, h], data: rgba.to_vec() });
        Ok(())
    }

    /// The writes since the last call, oldest first (the GPU side takes
    /// them each frame).
    pub fn take_pending(&mut self) -> Vec<MacroWrite> {
        core::mem::take(&mut self.pending)
    }

    /// Writes waiting for the next frame.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

fn covers(a: [u32; 4], b: [u32; 4]) -> bool {
    a[0] <= b[0] && a[1] <= b[1] && a[0] + a[2] >= b[0] + b[2] && a[1] + a[3] >= b[1] + b[3]
}

/// Mip levels of a `size`-texel square, down to 1 x 1.
pub fn mip_levels(size: u32) -> u32 {
    32 - size.max(1).leading_zeros()
}

/// The texels a level-0 rect `[x, y, w, h]` touches on each level from 1
/// (its parent texels): `(level, [x, y, w, h])`, smallest last. These are
/// the only texels the mip rebuild redraws.
pub fn mip_rects(rect: [u32; 4], size: u32) -> Vec<(u32, [u32; 4])> {
    let [x, y, w, h] = rect;
    let (x1, y1) = (x + w, y + h);
    (1..mip_levels(size))
        .map(|k| {
            let side = (size >> k).max(1);
            let (ax, ay) = (x >> k, y >> k);
            let bx = x1.div_ceil(1 << k).min(side);
            let by = y1.div_ceil(1 << k).min(side);
            (k, [ax, ay, (bx - ax).max(1), (by - ay).max(1)])
        })
        .collect()
}

/// GPU bytes of one array layer of `size` with a full mip chain in a format
/// of `block` (see [`crate::texture::block_of`]).
pub fn layer_bytes(size: u32, block: (u32, u32)) -> u64 {
    (0..mip_levels(size)).map(|i| crate::texture::level_bytes((size >> i).max(1), (size >> i).max(1), block)).sum()
}

/// Where the manifest stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ManifestState {
    #[default]
    Unread,
    Asked,
    Done,
}

/// The kit's texture sets and macro texture (a world resource the
/// renderer inserts with the feature).
#[derive(Debug, Clone, Default)]
pub struct KitTextures {
    sets: [Option<TextureSetDesc>; MAX_SETS],
    /// Sets the code set (the manifest leaves them alone).
    by_code: [bool; MAX_SETS],
    set_gen: [u64; MAX_SETS],
    shipped: Vec<TexFormat>,
    anisotropy: Vec<(String, u16)>,
    macro_: Option<MacroArray>,
    macro_gen: u64,
    ready: [[f32; MAX_LAYERS]; MAX_SETS],
    pub manifest: ManifestState,
    /// Layers the game doesn't want, per set (bit = layer; 0 = all wanted).
    skip: [u16; MAX_SETS],
    hold: bool,
    /// Seconds a layer takes to fade in (`None`: [`DEFAULT_FADE_S`]).
    fade: Option<f32>,
    /// Per layer: whether it shows (the fade's goal).
    target: [[bool; MAX_LAYERS]; MAX_SETS],
    stage: [[LayerStage; MAX_LAYERS]; MAX_SETS],
    /// Whether the renderer has started loading at a settled tier.
    started: bool,
    macro_sizes: Vec<(String, u32)>,
    loaded: u64,
}

/// Seconds a layer takes to fade in once its maps are on the GPU.
pub const DEFAULT_FADE_S: f32 = 0.5;

impl KitTextures {
    /// Use `desc` for `set` (0 ground, 1 hero). The renderer reloads that
    /// set at the active tier's size.
    pub fn set(&mut self, set: usize, desc: TextureSetDesc) -> Result<(), String> {
        if set >= MAX_SETS {
            return Err(format!("texture set {set}: there are {MAX_SETS}"));
        }
        desc.validate()?;
        self.by_code[set] = true;
        self.put(set, Some(desc));
        Ok(())
    }

    /// Drop a set (its bindings go back to the dummies).
    pub fn clear(&mut self, set: usize) {
        if set < MAX_SETS {
            self.by_code[set] = true;
            self.put(set, None);
        }
    }

    fn put(&mut self, set: usize, desc: Option<TextureSetDesc>) {
        self.sets[set] = desc;
        self.set_gen[set] += 1;
        self.ready[set] = [0.0; MAX_LAYERS];
        self.target[set] = [false; MAX_LAYERS];
        self.stage[set] = [LayerStage::Off; MAX_LAYERS];
    }

    /// Load only these layers of `set` (W3c: a mission fetches the layers it
    /// uses). Layers not listed aren't fetched; one already on the GPU keeps
    /// showing until the set swaps to its tier size. Default: every layer.
    pub fn want(&mut self, set: usize, layers: &[usize]) {
        if set < MAX_SETS {
            let mask = layers.iter().filter(|l| **l < MAX_LAYERS).fold(0u16, |m, l| m | (1 << l));
            self.skip[set] = !mask;
        }
    }

    /// Load every layer of `set` again (the default).
    pub fn want_all(&mut self, set: usize) {
        if set < MAX_SETS {
            self.skip[set] = 0;
        }
    }

    /// The wanted layers of `set` as a bit mask.
    pub fn wanted(&self, set: usize) -> u16 {
        self.skip.get(set).map_or(0, |s| !s)
    }

    /// Hold the tier-size loads (W3c): sets with a boot size stop at it
    /// until released, so the start of a mission downloads only the boot
    /// sizes (TopGun: at most 4 MB before take-off). Sets without a boot
    /// size load as usual.
    pub fn hold_full(&mut self, on: bool) {
        self.hold = on;
    }

    /// Whether tier-size loads are held.
    pub fn held(&self) -> bool {
        self.hold
    }

    /// Seconds a layer takes to fade in (0 = at once).
    pub fn set_fade(&mut self, seconds: f32) {
        self.fade = Some(seconds.max(0.0));
    }

    /// The fade-in time in seconds.
    pub fn fade(&self) -> f32 {
        self.fade.unwrap_or(DEFAULT_FADE_S)
    }

    /// Set by the renderer: whether a layer shows. Its ready value then
    /// fades towards 1 (or drops to 0) in [`KitTextures::tick`].
    pub fn set_showing(&mut self, set: usize, layer: usize, on: bool) {
        if let Some(t) = self.target.get_mut(set).and_then(|s| s.get_mut(layer)) {
            *t = on;
        }
        if !on {
            self.mark_ready(set, layer, 0.0);
        }
    }

    /// Advance the fades by `dt` seconds (render time). Returns whether any
    /// ready value changed (the renderer then uploads them).
    pub fn tick(&mut self, dt: f32) -> bool {
        let step = if self.fade() <= 0.0 { 1.0 } else { (dt.max(0.0) / self.fade()).max(1e-4) };
        let mut changed = false;
        for (r, t) in self.ready.iter_mut().zip(self.target.iter()) {
            for (v, on) in r.iter_mut().zip(t.iter()) {
                let goal = if *on { 1.0 } else { 0.0 };
                if *v != goal {
                    *v = if *on { (*v + step).min(1.0) } else { 0.0 };
                    changed = true;
                }
            }
        }
        changed
    }

    /// Set by the renderer: a layer's stage.
    pub fn set_stage(&mut self, set: usize, layer: usize, s: LayerStage) {
        if let Some(x) = self.stage.get_mut(set).and_then(|v| v.get_mut(layer)) {
            *x = s;
        }
    }

    /// Where a layer stands.
    pub fn stage(&self, set: usize, layer: usize) -> LayerStage {
        self.stage.get(set).and_then(|s| s.get(layer)).copied().unwrap_or_default()
    }

    /// Set by the renderer once it loads at a settled tier.
    pub fn set_started(&mut self) {
        self.started = true;
    }

    /// Whether the start of the load is over (W3c): the tier has settled and
    /// no wanted layer is still waiting for or fetching its first maps (each
    /// shows at its boot size or its tier size, or failed). A title screen or
    /// a mission start waits on it; the tier sizes go on streaming after.
    pub fn boot_done(&self) -> bool {
        self.started && self.stage.iter().flatten().all(|s| !matches!(s, LayerStage::Waiting | LayerStage::Loading))
    }

    /// Bytes of layer files the renderer has uploaded so far.
    pub fn loaded_bytes(&self) -> u64 {
        self.loaded
    }

    /// Set by the renderer: `n` more bytes uploaded.
    pub fn add_loaded(&mut self, n: u64) {
        self.loaded += n;
    }

    /// The macro size the manifest gives `tier` (`"macro": {"low": 1024}`),
    /// for the game's [`MacroDesc`]; `None` when it gives none.
    pub fn macro_size_for(&self, tier: &str) -> Option<u32> {
        self.macro_sizes.iter().find(|(t, _)| t == tier).map(|(_, s)| *s)
    }

    /// Set the macro size on one tier (the manifest's `"macro"`, from code).
    pub fn set_macro_size(&mut self, tier: &str, size: u32) {
        self.macro_sizes.retain(|(t, _)| t != tier);
        self.macro_sizes.push((tier.to_owned(), size));
    }

    /// A set and its generation (each change bumps it).
    pub fn set_desc(&self, set: usize) -> (u64, Option<&TextureSetDesc>) {
        (self.set_gen.get(set).copied().unwrap_or(0), self.sets.get(set).and_then(Option::as_ref))
    }

    /// The device sets shipped (empty = every one: ASTC, BC7 and RGBA).
    pub fn shipped(&self) -> &[TexFormat] {
        &self.shipped
    }

    /// Say which device sets the game ships (default: all three).
    pub fn set_shipped(&mut self, formats: Vec<TexFormat>) {
        self.shipped = formats;
    }

    /// The anisotropy on `tier` (the game's, else [`default_anisotropy`]),
    /// 1 to 16.
    pub fn anisotropy(&self, tier: &str) -> u16 {
        self.anisotropy.iter().find(|(t, _)| t == tier).map_or_else(|| default_anisotropy(tier), |(_, a)| *a).clamp(1, 16)
    }

    /// Set the anisotropy on one tier.
    pub fn set_anisotropy(&mut self, tier: &str, n: u16) {
        self.anisotropy.retain(|(t, _)| t != tier);
        self.anisotropy.push((tier.to_owned(), n.clamp(1, 16)));
    }

    /// Make (or remake, emptied) the macro texture and return it to write.
    pub fn macro_array(&mut self, desc: MacroDesc) -> Result<&mut MacroArray, String> {
        if !desc.size.is_power_of_two() || desc.size > MAX_MACRO_SIZE || desc.layers == 0 || desc.layers > MAX_MACRO_LAYERS {
            return Err(format!("macro {}x{} with {} layers: the size must be a power of two up to {MAX_MACRO_SIZE}, 1 to {MAX_MACRO_LAYERS} layers", desc.size, desc.size, desc.layers));
        }
        self.macro_gen += 1;
        Ok(self.macro_.insert(MacroArray { desc, pending: Vec::new() }))
    }

    /// The macro texture, to write more.
    pub fn macro_mut(&mut self) -> Option<&mut MacroArray> {
        self.macro_.as_mut()
    }

    /// The macro texture and its generation.
    pub fn macro_state(&self) -> (u64, Option<&MacroArray>) {
        (self.macro_gen, self.macro_.as_ref())
    }

    /// Whether both maps of a layer are on the GPU.
    pub fn layer_ready(&self, set: usize, layer: usize) -> bool {
        self.ready(set, layer) >= 1.0
    }

    /// 0 until a layer is on the GPU, then 1 (the value `kit_tex_ready`
    /// returns in WGSL).
    pub fn ready(&self, set: usize, layer: usize) -> f32 {
        self.ready.get(set).and_then(|s| s.get(layer)).copied().unwrap_or(0.0)
    }

    /// Set by the renderer.
    pub fn mark_ready(&mut self, set: usize, layer: usize, v: f32) {
        if let Some(r) = self.ready.get_mut(set).and_then(|s| s.get_mut(layer)) {
            *r = v;
        }
        if let Some(t) = self.target.get_mut(set).and_then(|s| s.get_mut(layer)) {
            *t = v > 0.0;
        }
    }

    /// Every layer's ready value as the WGSL `kit_tex_state` array.
    pub fn state_words(&self) -> [f32; MAX_SETS * MAX_LAYERS] {
        let mut w = [0.0; MAX_SETS * MAX_LAYERS];
        for (s, r) in self.ready.iter().enumerate() {
            w[s * MAX_LAYERS..(s + 1) * MAX_LAYERS].copy_from_slice(r);
        }
        w
    }

    /// Apply `kit-textures.json` (see the module docs and docs/GROUND_TEXTURES.md):
    /// the sets the code hasn't set, the shipped formats and the anisotropy.
    pub fn apply_manifest(&mut self, text: &str) -> Result<(), String> {
        let m = Manifest::parse(text)?;
        if !m.formats.is_empty() {
            self.shipped = m.formats;
        }
        for (t, a) in m.anisotropy {
            self.set_anisotropy(&t, a);
        }
        // Code set before the manifest arrived wins for the macro sizes and
        // the fade.
        for (t, n) in m.macro_sizes {
            if self.macro_size_for(&t).is_none() {
                self.set_macro_size(&t, n);
            }
        }
        if let (Some(f), None) = (m.fade_s, self.fade) {
            self.fade = Some(f.max(0.0));
        }
        for (i, d) in m.sets.into_iter().enumerate() {
            if let Some(d) = d
                && !self.by_code[i]
            {
                self.put(i, Some(d));
            }
        }
        Ok(())
    }
}

/// `kit-textures.json`, read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest {
    pub sets: [Option<TextureSetDesc>; MAX_SETS],
    pub formats: Vec<TexFormat>,
    pub anisotropy: Vec<(String, u16)>,
    /// Download budget per tier in MB (the check's; the runtime ignores it).
    pub budget_mb: Vec<(String, f64)>,
    /// Macro size per tier (`"macro"`), for [`KitTextures::macro_size_for`].
    pub macro_sizes: Vec<(String, u32)>,
    /// Fade-in seconds (`"fade_s"`).
    pub fade_s: Option<f32>,
}

impl Manifest {
    /// Read the JSON:
    /// `{ "formats": ["astc","bc7","rgba"], "anisotropy": {"high": 8},
    ///    "budget_mb": {"high": 25}, "boot_budget_mb": 4, "macro": {"high": 2048}, "fade_s": 0.5,
    ///    "sets": [{ "set": 0, "size": {"medium": 512, "high": 1024}, "boot": 128,
    ///               "layers": [{"name": "rock", "path": "ground/rock"}] }] }`
    pub fn parse(text: &str) -> Result<Self, String> {
        use serde_json::Value;
        let v: Value = serde_json::from_str(text).map_err(|e| format!("{MANIFEST_URL}: {e}"))?;
        let mut m = Manifest::default();
        for f in v.get("formats").and_then(Value::as_array).into_iter().flatten() {
            m.formats.push(match f.as_str() {
                Some("astc") => TexFormat::Astc4x4,
                Some("bc7") => TexFormat::Bc7,
                Some("rgba") => TexFormat::Rgba,
                _ => return Err(format!("{MANIFEST_URL}: format {f} is not astc, bc7 or rgba")),
            });
        }
        let pairs = |key: &str| -> Vec<(String, f64)> { v.get(key).and_then(Value::as_object).map(|o| o.iter().filter_map(|(k, x)| Some((k.clone(), x.as_f64()?))).collect()).unwrap_or_default() };
        m.anisotropy = pairs("anisotropy").into_iter().map(|(t, a)| (t, a.clamp(1.0, 16.0) as u16)).collect();
        m.budget_mb = pairs("budget_mb");
        m.macro_sizes = pairs("macro").into_iter().map(|(t, n)| (t, n as u32)).collect();
        for (t, n) in &m.macro_sizes {
            if *n != 0 && (!n.is_power_of_two() || *n > MAX_MACRO_SIZE) {
                return Err(format!("{MANIFEST_URL}: macro size {n} on tier {t} is not a power of two up to {MAX_MACRO_SIZE}"));
            }
        }
        m.fade_s = v.get("fade_s").and_then(Value::as_f64).map(|f| f as f32);
        for s in v.get("sets").and_then(Value::as_array).into_iter().flatten() {
            let i = s.get("set").and_then(Value::as_u64).unwrap_or(0) as usize;
            if i >= MAX_SETS {
                return Err(format!("{MANIFEST_URL}: set {i} (there are {MAX_SETS})"));
            }
            let mut d = TextureSetDesc { boot: s.get("boot").and_then(Value::as_u64).unwrap_or(0) as u32, ..Default::default() };
            for (t, n) in s.get("size").and_then(Value::as_object).into_iter().flatten() {
                d.sizes.push((t.clone(), n.as_u64().unwrap_or(0) as u32));
            }
            for l in s.get("layers").and_then(Value::as_array).into_iter().flatten() {
                let name = l.get("name").and_then(Value::as_str).unwrap_or_default();
                let path = l.get("path").and_then(Value::as_str).ok_or_else(|| format!("{MANIFEST_URL}: set {i} layer {name:?} has no path"))?;
                d.layers.push(LayerSrc::new(name, path));
            }
            d.validate().map_err(|e| format!("{MANIFEST_URL}: set {i}: {e}"))?;
            m.sets[i] = Some(d);
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_tier_and_the_device() {
        let d = TextureSetDesc::default().tier("medium", 512).tier("ultra", 2048).layer("rock", "ground/rock");
        assert_eq!((d.size_for("medium"), d.size_for("ultra"), d.size_for("low")), (512, 2048, 0));
        let all = [TexFormat::Astc4x4, TexFormat::Bc7, TexFormat::Rgba];
        let f = pick_format(DeviceTex { astc: true, bc: true, etc2: false }, &all);
        assert_eq!(file_url("ground/rock", Map::Ah, 512, f), "ground/rock_ah.512.astc.ktx2");
        let f = pick_format(DeviceTex { astc: false, bc: true, etc2: false }, &all);
        assert_eq!(file_url("ground/rock", Map::Nra, 1024, f), "ground/rock_nra.1024.bc7.ktx2");
        assert_eq!(pick_format(DeviceTex::default(), &all), TexFormat::Rgba);
        // A device with ASTC fetches BC7 when only BC7 was shipped.
        assert_eq!(pick_format(DeviceTex { astc: true, bc: true, etc2: false }, &[TexFormat::Bc7, TexFormat::Rgba]), TexFormat::Bc7);
    }

    #[test]
    fn sets_validate() {
        assert!(TextureSetDesc::default().validate().is_err());
        assert!(TextureSetDesc::default().tier("high", 1000).layer("a", "a").validate().is_err());
        let mut many = TextureSetDesc::default();
        for i in 0..=MAX_LAYERS {
            many = many.layer(&i.to_string(), "x");
        }
        assert!(many.validate().is_err());
        let mut kt = KitTextures::default();
        assert!(kt.set(2, TextureSetDesc::default().layer("a", "a")).is_err());
        assert!(kt.set(0, TextureSetDesc::default().tier("high", 1024).layer("a", "a")).is_ok());
        assert_eq!(kt.set_desc(0).0, 1);
    }

    #[test]
    fn write_rect_checks_its_rect_and_queues_it() {
        let mut kt = KitTextures::default();
        assert!(kt.macro_array(MacroDesc { size: 1000, layers: 2 }).is_err());
        let m = kt.macro_array(MacroDesc { size: 256, layers: 2 }).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.write_rect(2, 0, 0, 1, 1, &[0; 4]).is_err());
        assert!(m.write_rect(0, 250, 0, 8, 1, &[0; 32]).is_err());
        assert!(m.write_rect(0, 0, 0, 2, 2, &[0; 4]).is_err());
        assert!(m.write_rect(0, 0, 0, 0, 2, &[]).is_err());
        assert!(m.write_rect(0, 8, 8, 4, 4, &[1; 64]).is_ok());
        assert!(m.write_rect(1, 8, 8, 4, 4, &[2; 64]).is_ok());
        assert_eq!(m.pending(), 2);
        let p = m.take_pending();
        assert_eq!((p[0].layer, p[0].rect, p[1].data[0]), (0, [8, 8, 4, 4], 2));
        assert_eq!(m.pending(), 0);
    }

    /// The per-frame pattern (TopGun's field tint): a rect rewritten every
    /// frame never piles up writes, and a whole-layer write drops the
    /// smaller ones it covers.
    #[test]
    fn repeated_write_rect_calls_stay_bounded() {
        let mut kt = KitTextures::default();
        let m = kt.macro_array(MacroDesc { size: 2048, layers: 2 }).unwrap_or_else(|e| panic!("{e}"));
        let tile = vec![7u8; 256 * 256 * 4];
        for frame in 0..100u32 {
            assert!(m.write_rect(1, 512, 768, 256, 256, &tile).is_ok());
            assert_eq!(m.pending(), 1, "frame {frame}");
        }
        assert!(m.write_rect(1, 0, 0, 16, 16, &[0; 1024]).is_ok());
        assert_eq!(m.pending(), 2);
        assert!(m.write(1, &vec![0u8; 2048 * 2048 * 4]).is_ok());
        assert_eq!(m.pending(), 1);
        // Each frame's work is the rect and its parents, never the layer.
        let texels: u32 = mip_rects([512, 768, 256, 256], 2048).iter().map(|(_, r)| r[2] * r[3]).sum();
        assert!(texels < 256 * 256 / 2, "{texels}");
    }

    #[test]
    fn mip_rects_cover_the_parent_texels() {
        assert_eq!(mip_levels(2048), 12);
        let r = mip_rects([3, 5, 2, 1], 16);
        assert_eq!(r[0], (1, [1, 2, 2, 1]));
        assert_eq!(r[1], (2, [0, 1, 2, 1]));
        assert_eq!(r.last(), Some(&(4, [0, 0, 1, 1])));
        // Every level-0 texel's parent on every level is inside its rect.
        for (k, [x, y, w, h]) in mip_rects([3, 5, 2, 1], 16) {
            for (px, py) in [(3u32, 5u32), (4, 5)] {
                let (qx, qy) = (px >> k, py >> k);
                assert!(qx >= x && qx < x + w && qy >= y && qy < y + h, "level {k}");
            }
        }
    }

    #[test]
    fn the_manifest_fills_unset_sets_only() {
        let json = r#"{"formats":["bc7","rgba"],"anisotropy":{"high":4},"budget_mb":{"high":25},
            "sets":[{"set":0,"size":{"medium":512,"high":1024},"layers":[{"name":"rock","path":"g/rock"}]},
                    {"set":1,"size":{"high":2048},"layers":[{"name":"pave","path":"g/pave"}]}]}"#;
        let m = Manifest::parse(json).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.budget_mb, vec![("high".to_string(), 25.0)]);
        let mut kt = KitTextures::default();
        assert!(kt.set(1, TextureSetDesc::default().tier("high", 512).layer("mine", "m")).is_ok());
        assert!(kt.apply_manifest(json).is_ok());
        assert_eq!(kt.set_desc(0).1.map(|d| d.size_for("high")), Some(1024));
        assert_eq!(kt.set_desc(1).1.map(|d| d.layers[0].name.as_str()), Some("mine"));
        assert_eq!(kt.anisotropy("high"), 4);
        assert_eq!(kt.anisotropy("ultra"), 16);
        assert_eq!(kt.shipped(), &[TexFormat::Bc7, TexFormat::Rgba]);
        assert!(Manifest::parse(r#"{"sets":[{"set":3,"layers":[]}]}"#).is_err());
        assert!(Manifest::parse(r#"{"formats":["etc9"]}"#).is_err());
    }

    #[test]
    fn ready_values_reach_the_wgsl_words() {
        let mut kt = KitTextures::default();
        kt.mark_ready(1, 3, 1.0);
        assert!(kt.layer_ready(1, 3) && !kt.layer_ready(0, 3));
        assert_eq!(kt.state_words()[16 + 3], 1.0);
        // Changing a set resets its ready values.
        assert!(kt.set(1, TextureSetDesc::default().layer("a", "a")).is_ok());
        assert!(!kt.layer_ready(1, 3));
    }

    /// The forward source with the builtin slot bodies spliced in (as in
    /// kit_surface's tests).
    fn forward() -> String {
        let mut slots = evani_core::slots::ShaderSlots::with_builtins();
        slots.declare(evani_core::slots::SlotSignature {
            name: "pcss_params",
            params: &[("i", "u32")],
            ret: "vec4<f32>",
            description: "Soft shadows (test stand-in)",
            default_body: "    return vec4<f32>(0.02, 6.0, 1.0, 12.0);",
        });
        #[cfg(feature = "relief")]
        slots.declare(evani_core::slots::MICRO_DETAIL);
        #[cfg(all(feature = "look-surface", not(feature = "relief")))]
        {
            slots.declare(crate::model::surface::SPECULAR);
            slots.declare(crate::model::surface::SURFACE_PARAMS);
        }
        let mut fwd = String::new();
        for line in crate::shaders::FORWARD.source.lines() {
            match line.trim().strip_prefix("//@slot ") {
                Some(name) => match slots.id(name.trim()).and_then(|id| slots.wgsl_function(id)) {
                    Some(f) => fwd.push_str(&f),
                    None => panic!("no slot {name}"),
                },
                None => fwd.push_str(line),
            }
            fwd.push('\n');
        }
        fwd
    }

    fn validate(src: &str) {
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }

    /// Kit code that samples every binding the way a ground shader does:
    /// gradients at the top, then non-uniform branches with
    /// `textureSampleGrad`, the macro with the clamp sampler, and nothing
    /// in the shadow pass.
    const KIT: &str = "fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
    let uv = p.xz * 0.25;
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    if (KIT_SHADOW_PASS) { return vec4<f32>(1.0); }
    var c = vec4<f32>(1.0);
    if (material == 3u) {
        let ah = textureSampleGrad(kit_tex0_ah, kit_wrap, uv, 2u, dx, dy);
        let nra = textureSampleGrad(kit_tex1_nra, kit_wrap, uv, 0u, dx, dy);
        let hero = textureSampleGrad(kit_tex1_ah, kit_wrap, uv, 1u, dx, dy) + textureSampleGrad(kit_tex0_nra, kit_wrap, uv, 1u, dx, dy);
        let m = textureSampleGrad(kit_macro, kit_clamp, uv * 0.01, 1u, dx * 0.01, dy * 0.01);
        c = mix(c, ah * nra.w + hero * 0.0 + m, kit_tex_ready(0u, 2u));
    }
    return vec4<f32>(c.rgb, 1.0);
}";

    #[test]
    fn kit_code_sampling_the_arrays_validates_in_forward_and_shadow() {
        let fwd = crate::kit_material::patch_forward(&forward(), KIT).unwrap_or_else(|| panic!("no hook"));
        assert!(fwd.contains(FORWARD_FLAG_WGSL) && fwd.contains("@binding(23)"));
        validate(&fwd);
        let shadows = crate::kit_material::shadows_source(KIT);
        assert!(shadows.contains(SHADOW_FLAG_WGSL));
        validate(&shadows);
        // The bindings carry over to kit_surface code too.
        #[cfg(feature = "kit-surface")]
        {
            let surf = format!("{KIT}
fn kit_surface(p: vec3<f32>, n: vec3<f32>, material: u32, nr: ptr<function, vec4<f32>>) -> vec4<f32> {{ let t = textureSampleLevel(kit_tex0_nra, kit_wrap, p.xz, 0u, 0.0); *nr = vec4<f32>(normalize(n + vec3<f32>(t.x - 0.5, 0.0, t.y - 0.5)), t.z); return kit_material(p, n, material); }}");
            let mut fwd = crate::kit_material::patch_forward(&forward(), &surf).unwrap_or_else(|| panic!("no hook"));
            assert!(crate::kit_surface::patch(&mut fwd, &surf));
            validate(&fwd);
        }
    }

    /// The ground textures proof's shader (examples/ground-textures): biplanar,
    /// hex-tiling and explicit gradients through `kit_surface`, validated in
    /// the real forward and shadow modules.
    #[cfg(feature = "kit-surface")]
    #[test]
    fn the_proof_kit_shader_validates() {
        let kit = include_str!("../../../examples/ground-textures/kits/evani-kit-ground-textures/shaders/ground.wgsl");
        let mut fwd = crate::kit_material::patch_forward(&forward(), kit).unwrap_or_else(|| panic!("no hook"));
        assert!(crate::kit_surface::patch(&mut fwd, kit));
        validate(&fwd);
        assert!(fwd.len() < MAX_SHADER_BYTES, "{} bytes", fwd.len());
        validate(&crate::kit_material::shadows_source(kit));
    }

    /// evani-guide's `FORWARD_ALLOWANCE` (24 KB): the engine's forward module
    /// with the bindings, before any kit code, fits in it, so the check's
    /// kit share (60 KB less this) is never too generous.
    #[test]
    fn the_forward_module_leaves_the_kit_its_share() {
        let fwd = crate::kit_material::patch_forward(&forward(), "").unwrap_or_else(|| panic!("no hook"));
        assert!(fwd.len() <= 24 * 1024, "{} bytes", fwd.len());
        assert!(fwd.len() + KIT.len() < MAX_SHADER_BYTES);
    }

    /// W3c: the boot wave comes first, the tier-size wave follows only when
    /// every wanted layer has its boot pair, and the set swaps in one go.
    #[test]
    fn a_set_streams_boot_first_then_swaps_whole() {
        let all = 0b111u16;
        let mut s = SetStream::new(1024, 128, 3);
        let f = s.next(all, false);
        assert_eq!(f.len(), 6, "boot maps of 3 layers");
        assert!(f.iter().all(|x| x.wave == Wave::Boot && x.size == 128));
        assert!(s.next(all, false).is_empty(), "each fetch once");
        assert_eq!(s.stage(0, all), LayerStage::Loading);
        // Layer 0's boot pair: it shows at the boot size; no full yet.
        assert!(!s.got(0, Map::Ah, Wave::Boot, all));
        assert!(!s.got(0, Map::Nra, Wave::Boot, all));
        assert!(s.showing(0) && !s.showing(1));
        assert_eq!(s.stage(0, all), LayerStage::Coarse);
        assert!(s.next(all, false).is_empty(), "full waits for the whole boot wave");
        for l in 1..3 {
            s.got(l, Map::Ah, Wave::Boot, all);
            s.got(l, Map::Nra, Wave::Boot, all);
        }
        // Held: nothing more.
        assert!(s.next(all, true).is_empty());
        let f = s.next(all, false);
        assert_eq!(f.len(), 6);
        assert!(f.iter().all(|x| x.wave == Wave::Full && x.size == 1024));
        assert_eq!(s.bound(), Bound::Boot);
        // Two layers in at full: still bound at boot, nobody drops back.
        for l in 0..2 {
            assert!(!s.got(l, Map::Ah, Wave::Full, all));
            assert!(!s.got(l, Map::Nra, Wave::Full, all));
        }
        assert!((0..3).all(|l| s.showing(l)));
        s.got(2, Map::Ah, Wave::Full, all);
        assert!(s.got(2, Map::Nra, Wave::Full, all), "the last pair swaps the set");
        assert_eq!(s.bound(), Bound::Full);
        assert!((0..3).all(|l| s.stage(l, all) == LayerStage::Full));
    }

    #[test]
    fn wanted_layers_failures_and_late_wants() {
        // Only layers 0 and 2 wanted: layer 1 is never fetched.
        let want = 0b101u16;
        let mut s = SetStream::new(512, 64, 3);
        let f = s.next(want, false);
        assert!(f.iter().all(|x| x.layer != 1) && f.len() == 4);
        assert_eq!(s.stage(1, want), LayerStage::Off);
        // Layer 2 fails: the wave goes on without it.
        s.got(0, Map::Ah, Wave::Boot, want);
        s.got(0, Map::Nra, Wave::Boot, want);
        s.fail(2, want);
        assert_eq!(s.stage(2, want), LayerStage::Failed);
        let f = s.next(want, false);
        assert_eq!(f.len(), 2, "full maps of layer 0 only: {f:?}");
        s.got(0, Map::Ah, Wave::Full, want);
        assert!(s.got(0, Map::Nra, Wave::Full, want));
        // Layer 1 wanted after the swap: its tier-size pair only.
        let f = s.next(0b111, false);
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.layer == 1 && x.wave == Wave::Full));
        // No boot size (or one not below the tier's): one wave, W3a's way.
        let mut one = SetStream::new(256, 256, 2);
        assert_eq!(one.bound(), Bound::Full);
        assert!(one.next(0b11, true).iter().all(|x| x.wave == Wave::Full), "hold doesn't hold a set without a boot size");
        one.got(1, Map::Ah, Wave::Full, 0b11);
        one.got(1, Map::Nra, Wave::Full, 0b11);
        assert!(one.showing(1) && !one.showing(0));
        // A set off on the tier asks nothing.
        assert!(SetStream::new(0, 64, 2).next(0b11, false).is_empty());
    }

    #[test]
    fn ready_fades_in_and_boot_done_waits_for_the_first_maps() {
        let mut kt = KitTextures::default();
        assert!(!kt.boot_done(), "not before the tier settles");
        kt.set_started();
        assert!(kt.boot_done());
        kt.set_stage(0, 1, LayerStage::Loading);
        assert!(!kt.boot_done());
        kt.set_stage(0, 1, LayerStage::Coarse);
        assert!(kt.boot_done());
        kt.set_fade(0.5);
        kt.set_showing(0, 1, true);
        assert!(kt.tick(0.25));
        assert!((kt.ready(0, 1) - 0.5).abs() < 1e-5);
        assert!(!kt.layer_ready(0, 1));
        kt.tick(0.25);
        assert!(kt.layer_ready(0, 1));
        assert!(!kt.tick(0.25), "settled: nothing to upload");
        kt.set_showing(0, 1, false);
        assert_eq!(kt.ready(0, 1), 0.0);
        kt.set_fade(0.0);
        kt.set_showing(1, 15, true);
        kt.tick(0.0);
        assert_eq!(kt.state_words()[31], 1.0, "fade 0: at once");
        // Wanted masks.
        kt.want(1, &[0, 3]);
        assert_eq!(kt.wanted(1), 0b1001);
        kt.want_all(1);
        assert_eq!(kt.wanted(1), u16::MAX);
    }

    #[test]
    fn the_manifest_carries_boot_sizes_macro_sizes_and_the_fade() {
        let json = r#"{"macro":{"low":1024,"high":2048},"fade_s":0.25,
            "sets":[{"set":0,"boot":128,"size":{"low":0,"high":1024},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let mut kt = KitTextures::default();
        kt.set_macro_size("high", 4096);
        assert!(kt.apply_manifest(json).is_ok());
        assert_eq!(kt.set_desc(0).1.map(|d| (d.boot_for("high"), d.boot_for("low"))), Some((128, 0)));
        assert_eq!((kt.macro_size_for("low"), kt.macro_size_for("high"), kt.macro_size_for("ultra")), (Some(1024), Some(4096), None), "code wins");
        assert!((kt.fade() - 0.25).abs() < 1e-6);
        assert!(Manifest::parse(r#"{"macro":{"high":1000}}"#).is_err());
        assert!(Manifest::parse(r#"{"sets":[{"set":0,"boot":100,"layers":[{"path":"a"}]}]}"#).is_err());
    }

    #[test]
    fn bc7_layer_sizes() {
        // 2k + mips ~ 5.6 MB per map, so ~11 MB a layer (AH + NRA).
        let b = layer_bytes(2048, (4, 16));
        assert!((5_500_000..5_700_000).contains(&b), "{b}");
        assert_eq!(layer_bytes(4, (4, 16)), 16 * 3);
    }
}
