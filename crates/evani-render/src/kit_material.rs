//! Kit material (feature `kit-material`, S5-E3): a kit's own per-fragment
//! code and data on the forward mesh pass, for effects such as hole
//! cut-outs (K6c's tactical-fps look).
//!
//! * The kit sets WGSL defining
//!   `fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32>`
//!   (world position, unit normal, the instance's material slot). Its rgb
//!   multiplies the mesh's base colour; an alpha below 0.5 discards the
//!   fragment, in colour, depth and the shadow map alike.
//! * The code may read `kit_data`, a read-only storage array of `vec4<f32>`
//!   the kit fills through [`KitMaterial::data_mut`] (for example a hole
//!   table bucketed by position). It must use nothing else from the pass
//!   (no `frame`, no slots): the shadow pass compiles it on its own.
//! * Without kit code the default returns `vec4(1.0)`: nothing changes.
//! * Meshes only: models and skinned models do not run it.
//!
//! The code is spliced into the forward module at pipeline build (one line
//! of `fs_main` changes, the rest is appended), so the WGSL files are
//! untouched and games without the feature are byte for byte unchanged.

/// The default kit code: keep every fragment, colour unchanged.
pub const DEFAULT_WGSL: &str = "fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> { return vec4<f32>(1.0); }";

/// The line of the forward fragment stage the kit term goes into.
pub const HOOK: &str = "let base = in.color.rgb * m.base.rgb;";
/// What it becomes: the kit's colour and cut-out, before any lighting.
pub const HOOKED: &str = "let kit = kit_material(in.world, normalize(in.normal), in.material); if (kit.a < 0.5) { discard; } let base = in.color.rgb * m.base.rgb * kit.rgb;";

/// The kit data binding (group 1, binding 15: clear of the model
/// material's bindings in group 1).
pub const BINDING_WGSL: &str = "@group(1) @binding(15) var<storage, read> kit_data: array<vec4<f32>>;\n";

/// The shadow-map variant with a fragment stage that runs the kit's
/// cut-out (used only while the kit has code of its own).
pub const SHADOWS_WGSL: &str = include_str!("../shaders/kit_shadows.wgsl");

/// Most `vec4` records uploaded (4 MB); the rest are ignored.
pub const MAX_DATA: usize = 1 << 18;

/// Splice the kit term into a forward-module source. Sources without the
/// hook line (models, sprites, effects) come back unchanged.
pub fn patch_forward(source: &str, kit_wgsl: &str) -> Option<String> {
    if !source.contains(HOOK) {
        return None;
    }
    let mut s = source.replacen(HOOK, HOOKED, 1);
    s.push('\n');
    s.push_str(BINDING_WGSL);
    s.push_str(if kit_wgsl.trim().is_empty() { DEFAULT_WGSL } else { kit_wgsl });
    s.push('\n');
    Some(s)
}

/// The full shadow-map module for this kit code.
pub fn shadows_source(kit_wgsl: &str) -> String {
    let mut s = String::with_capacity(SHADOWS_WGSL.len() + kit_wgsl.len() + 128);
    s.push_str(SHADOWS_WGSL);
    s.push('\n');
    s.push_str(BINDING_WGSL);
    s.push_str(if kit_wgsl.trim().is_empty() { DEFAULT_WGSL } else { kit_wgsl });
    s.push('\n');
    s
}

/// The kit's material code and data (a world resource). Changing the code
/// rebuilds the forward pipelines (a few frames); changing the data only
/// uploads it.
#[derive(Debug, Clone, Default)]
pub struct KitMaterial {
    wgsl: String,
    data: Vec<[f32; 4]>,
    wgsl_gen: u64,
    data_gen: u64,
}

impl KitMaterial {
    /// Set the kit code (see the module docs for the function it defines).
    /// An empty string restores the default.
    pub fn set_wgsl(&mut self, wgsl: impl Into<String>) {
        self.wgsl = wgsl.into();
        self.wgsl_gen += 1;
    }

    /// The kit code ("" while the kit has set none).
    pub fn wgsl(&self) -> &str {
        &self.wgsl
    }

    /// Whether the kit has code of its own.
    pub fn has_code(&self) -> bool {
        !self.wgsl.trim().is_empty()
    }

    /// The data records the code reads as `kit_data`.
    pub fn data(&self) -> &[[f32; 4]] {
        &self.data
    }

    /// Change the data records; they are uploaded before the next frame.
    pub fn data_mut(&mut self) -> &mut Vec<[f32; 4]> {
        self.data_gen += 1;
        &mut self.data
    }

    /// Code and data generations (each change bumps one).
    pub fn generations(&self) -> (u64, u64) {
        (self.wgsl_gen, self.data_gen)
    }
}
