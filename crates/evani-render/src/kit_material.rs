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
//!   Size: up to [`MAX_DATA`] records (2^21 = 32 MiB); the GPU buffer is
//!   allocated to what the kit writes, so a kit that writes little pays
//!   little and one that writes nothing pays 256 bytes.
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

/// Bytes in one `kit_data` record (a `vec4<f32>`).
pub const RECORD_BYTES: usize = 16;

/// The largest `kit_data` the engine uploads: 2^21 `vec4` records = 32 MiB
/// (it was 2^18 = 4 MB). The buffer is a read-only storage buffer; WebGPU
/// guarantees `maxStorageBufferBindingSize` of at least 128 MiB, so 32 MiB
/// binds on every adapter. A device that reports less lowers the cap for
/// that device only ([`device_max_records`]).
///
/// The cap costs nothing by itself: the buffer is allocated to what the
/// kit writes ([`alloc_records`]), never to the cap.
pub const MAX_DATA: usize = 1 << 21;

/// The former cap (4 MB). Buffers up to this size still grow by powers of
/// two exactly as before; only larger ones use [`alloc_records`]' steps.
pub const LEGACY_MAX_DATA: usize = 1 << 18;

/// Granularity of the allocation above [`LEGACY_MAX_DATA`]: 2^16 records
/// (1 MiB), so a 5 MB table takes 5 MiB, not the next power of two.
const BIG_STEP: usize = 1 << 16;

/// The `kit_data` record cap for a device: [`MAX_DATA`], lowered to what
/// the device can bind and allocate (`max_storage_buffer_binding_size` and
/// `max_buffer_size`, in bytes).
pub fn device_max_records(max_storage_binding: u64, max_buffer: u64) -> usize {
    let bytes = max_storage_binding.min(max_buffer);
    let records = usize::try_from(bytes / RECORD_BYTES as u64).unwrap_or(usize::MAX);
    records.min(MAX_DATA)
}

/// A kit asked for more `kit_data` than the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KitDataError {
    /// Records the kit asked for.
    pub records: usize,
    /// The cap in force (records).
    pub cap: usize,
}

impl std::fmt::Display for KitDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "kit_data: {} records ({:.1} MiB) is more than the {} records ({:.1} MiB) a kit may use; \
             shrink the table (pack fewer or smaller records) or split it across frames. The engine's cap is {} MiB",
            self.records,
            mib(self.records),
            self.cap,
            mib(self.cap),
            mib(MAX_DATA).round() as usize,
        )
    }
}

impl std::error::Error for KitDataError {}

fn mib(records: usize) -> f64 {
    records as f64 * RECORD_BYTES as f64 / (1024.0 * 1024.0)
}

/// Whether `len` records fit under `cap`.
pub fn check_len(len: usize, cap: usize) -> Result<(), KitDataError> {
    if len > cap { Err(KitDataError { records: len, cap }) } else { Ok(()) }
}

/// The records to allocate for `len` used records, at most `cap`: at least
/// 16, the next power of two up to [`LEGACY_MAX_DATA`] (the old growth), and
/// above it the next whole 1 MiB. Never more than `cap`, never less than
/// `len.min(cap)`.
pub fn alloc_records(len: usize, cap: usize) -> usize {
    let want = if len <= LEGACY_MAX_DATA { len.max(16).next_power_of_two() } else { len.div_ceil(BIG_STEP) * BIG_STEP };
    want.min(cap).max(len.min(cap))
}

/// What one upload does: how many records go up, the buffer size needed,
/// and the error to log when the kit asked for too many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadPlan {
    /// Records uploaded (`len` clamped to the cap).
    pub used: usize,
    /// Records the buffer must hold (unchanged `have` when it is big enough).
    pub alloc: usize,
    /// Set when the kit asked for more than the cap; the rest is dropped.
    pub over: Option<KitDataError>,
}

/// Plan an upload of `len` records into a buffer of `have` records.
pub fn plan_upload(len: usize, have: usize, cap: usize) -> UploadPlan {
    let used = len.min(cap);
    UploadPlan {
        used,
        alloc: if used > have { alloc_records(used, cap) } else { have },
        over: check_len(len, cap).err(),
    }
}

/// Splice the kit term into a forward-module source. Sources without the
/// hook line (models, sprites, effects) come back unchanged.
pub fn patch_forward(source: &str, kit_wgsl: &str) -> Option<String> {
    if !source.contains(HOOK) {
        return None;
    }
    let mut s = source.replacen(HOOK, HOOKED, 1);
    s.push('\n');
    s.push_str(BINDING_WGSL); #[cfg(feature = "kit-textures")] { s.push_str(crate::kit_textures::BINDING_WGSL); s.push_str(crate::kit_textures::FORWARD_FLAG_WGSL); }
    s.push_str(if kit_wgsl.trim().is_empty() { DEFAULT_WGSL } else { kit_wgsl });
    s.push('\n');
    Some(s)
}

/// The full shadow-map module for this kit code.
pub fn shadows_source(kit_wgsl: &str) -> String {
    let mut s = String::with_capacity(SHADOWS_WGSL.len() + kit_wgsl.len() + 128);
    s.push_str(SHADOWS_WGSL);
    s.push('\n');
    s.push_str(BINDING_WGSL); #[cfg(feature = "kit-textures")] { s.push_str(crate::kit_textures::BINDING_WGSL); s.push_str(crate::kit_textures::SHADOW_FLAG_WGSL); }
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

    /// Replace the data records, refusing a table over [`MAX_DATA`] with a
    /// [`KitDataError`] (the old data stays). Use it to find out at once
    /// that a table is too big; [`KitMaterial::data_mut`] still accepts
    /// anything and the renderer logs the error and drops the excess.
    pub fn set_data(&mut self, records: Vec<[f32; 4]>) -> Result<(), KitDataError> {
        check_len(records.len(), MAX_DATA)?;
        self.data_gen += 1;
        self.data = records;
        Ok(())
    }

    /// Change the data records; they are uploaded before the next frame.
    /// Up to [`MAX_DATA`] records (32 MiB) are used.
    pub fn data_mut(&mut self) -> &mut Vec<[f32; 4]> {
        self.data_gen += 1;
        &mut self.data
    }

    /// Code and data generations (each change bumps one).
    pub fn generations(&self) -> (u64, u64) {
        (self.wgsl_gen, self.data_gen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table of `n` records whose every record is recognisable.
    fn table(n: usize) -> Vec<[f32; 4]> {
        (0..n).map(|i| [i as f32, (i % 4096) as f32, 1.0, (n - i) as f32]).collect()
    }

    #[test]
    fn cap_is_32_mib_and_keeps_old_behaviour_below_4_mb() {
        assert_eq!(MAX_DATA * RECORD_BYTES, 32 << 20);
        // Up to the old cap the buffer grows by powers of two, as before.
        for len in [0, 1, 16, 17, 1000, 65_536, 100_000, LEGACY_MAX_DATA] {
            assert_eq!(alloc_records(len, MAX_DATA), len.max(16).next_power_of_two(), "len {len}");
        }
        // A game that writes nothing pays the 16-record minimum, not the cap.
        assert_eq!(alloc_records(0, MAX_DATA) * RECORD_BYTES, 256);
    }

    #[test]
    fn big_tables_allocate_to_what_they_need() {
        // 5 MiB of records takes 5 MiB, not the next power of two (8 MiB).
        let five_mib = 5 * (1 << 20) / RECORD_BYTES;
        assert_eq!(alloc_records(five_mib, MAX_DATA), five_mib);
        let a = alloc_records(five_mib + 1, MAX_DATA);
        assert!(a > five_mib && a - five_mib <= BIG_STEP);
        assert_eq!(alloc_records(MAX_DATA, MAX_DATA), MAX_DATA);
        // Never over the cap, whatever the request.
        assert_eq!(alloc_records(usize::MAX / 2, MAX_DATA), MAX_DATA);
    }

    #[test]
    fn write_and_read_back_near_the_cap() {
        let near = MAX_DATA - 3;
        let want = table(near);
        let mut k = KitMaterial::default();
        k.set_data(want.clone()).expect("a table under the cap is accepted");
        assert_eq!(k.data().len(), near);
        assert_eq!(k.data()[0], want[0]);
        assert_eq!(k.data()[near / 2], want[near / 2]);
        assert_eq!(k.data()[near - 1], want[near - 1]);
        assert_eq!(k.generations().1, 1);

        // The renderer's plan for it: every record goes up, no error, and
        // the buffer is at most the cap.
        let plan = plan_upload(near, 16, MAX_DATA);
        assert_eq!(plan.used, near);
        assert!(plan.over.is_none());
        assert!(plan.alloc >= near && plan.alloc <= MAX_DATA);

        // Exactly the cap is allowed, through the old API too.
        let mut k = KitMaterial::default();
        k.data_mut().extend(table(MAX_DATA));
        let plan = plan_upload(k.data().len(), plan.alloc, MAX_DATA);
        assert_eq!((plan.used, plan.alloc), (MAX_DATA, MAX_DATA));
        assert!(plan.over.is_none());
        assert_eq!(k.data()[MAX_DATA - 1][0], (MAX_DATA - 1) as f32);
    }

    #[test]
    fn over_the_cap_is_a_clear_error() {
        let mut k = KitMaterial::default();
        k.set_data(table(100)).unwrap();
        let gen_before = k.generations().1;
        let err = k.set_data(table(MAX_DATA + 1)).expect_err("one record over the cap is refused");
        assert_eq!(err, KitDataError { records: MAX_DATA + 1, cap: MAX_DATA });
        // The old data and generation are untouched.
        assert_eq!(k.data().len(), 100);
        assert_eq!(k.generations().1, gen_before);
        let msg = err.to_string();
        assert!(msg.contains("kit_data") && msg.contains("32 MiB") && msg.contains(&(MAX_DATA + 1).to_string()), "{msg}");

        // The renderer path (data_mut) keeps working: it uploads the cap and reports the rest.
        let plan = plan_upload(MAX_DATA + 5, 16, MAX_DATA);
        assert_eq!(plan.used, MAX_DATA);
        assert_eq!(plan.over, Some(KitDataError { records: MAX_DATA + 5, cap: MAX_DATA }));
    }

    #[test]
    fn small_devices_lower_the_cap() {
        // The WebGPU minimum (128 MiB) leaves the 32 MiB cap alone.
        assert_eq!(device_max_records(128 << 20, 256 << 20), MAX_DATA);
        // A device that binds only 16 MiB gets 16 MiB, and the error names that cap.
        let small = device_max_records(16 << 20, 256 << 20);
        assert_eq!(small, (16 << 20) / RECORD_BYTES);
        assert_eq!(check_len(small + 1, small).unwrap_err().cap, small);
        // The buffer limit counts too.
        assert_eq!(device_max_records(128 << 20, 8 << 20), (8 << 20) / RECORD_BYTES);
    }
}
