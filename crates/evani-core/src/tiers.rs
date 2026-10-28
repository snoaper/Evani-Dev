//! Quality tiers. All numbers live in `assets/tiers/*.json`; nothing
//! tier-specific is hard-coded in Rust. The JSON files are embedded at compile
//! time (so no extra request at startup) and `bake.export` rewrites them.

use serde::{Deserialize, Serialize};

/// One tier preset, exactly as stored in `assets/tiers/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierPreset {
    pub id: String,
    pub label: String,
    /// Frame-rate target.
    pub fps_target: u32,
    /// Upper end of a target range (High: 60-120, VR: 72-90), if any.
    pub fps_max: Option<u32>,
    /// Lowest acceptable fps before degrading, if the spec gives one.
    pub fps_floor: Option<u32>,
    /// Display refresh range for stereo/VR presets.
    pub refresh_hz: Option<[u32; 2]>,
    /// Resolution the target is quoted at (Ultra: 1440p).
    pub target_resolution: Option<[u32; 2]>,
    /// Internal render scale; `None` means native (1.0).
    pub render_scale: Option<f32>,
    /// Cap on devicePixelRatio; `None` means uncapped (native).
    pub dpr_cap: Option<f32>,
    pub max_draw_calls: u32,
    pub max_triangles: u32,
    pub max_sprites: u32,
    /// Live particles the shared particle system holds at once (the
    /// catalog's `max_particles`; the oldest is recycled at the cap). A
    /// game's `fx.particles.max` may set a lower cap of its own.
    pub max_particles: u32,
    #[cfg(feature = "texture-budget")] #[serde(default)] pub max_texture_memory: u32, #[cfg(feature = "point-lights")] #[serde(default)] pub max_point_lights: u32, #[cfg(feature = "shadow-cascades")] #[serde(default)] pub shadow_cascades: u32, #[cfg(feature = "ibl")] #[serde(default)] pub ibl_cube: u32, #[cfg(feature = "gtao")] #[serde(default)] pub gtao: u32, // MB, 0 = not set; point lights per frame (evani-render `point-lights`), 0 = none; see DECISIONS.md (Stage 3 modules). Lines are kept count-neutral: panic locations are in the wasm.
    pub points: u32, // budget points shared by the look and its effects
    pub stereo: bool,
    /// Considered by the first-run warm-up auto-selection.
    pub auto_select: bool,
    /// Forward-pass MSAA sample count: 1 (off) or 4. Missing in older
    /// files means 1.
    #[serde(default = "default_msaa")]
    pub msaa: u32,
    /// Effects switched on when this tier activates (the budget calculator
    /// may still switch some off).
    pub effects_on: Vec<String>,
    /// Shadow-map size in texels when the `shadows` effect is on (`None`:
    /// no shadow map, kits' blob shadows only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_map: Option<u32>,
    /// Shadow-map PCF radius in texels (0 = one hardware-filtered 2x2 tap,
    /// 1 = 3x3 taps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_pcf: Option<u32>,
}

fn default_msaa() -> u32 {
    1
}

impl TierPreset {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let p: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }

    /// Checks serde cannot express: `msaa` is 1 or 4 (what WebGPU
    /// guarantees for every renderable format).
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.msaa, 1 | 4) {
            return Err(format!("tier {}: msaa must be 1 or 4, not {}", self.id, self.msaa));
        }
        Ok(())
    }

    /// Render scale with the generic default (1.0) applied.
    pub fn render_scale_or_native(&self) -> f32 {
        self.render_scale.unwrap_or(1.0)
    }

    /// Frame-time budget in ms for the target fps.
    pub fn frame_budget_ms(&self) -> f32 {
        1000.0 / self.fps_target.max(1) as f32
    }

    /// Clamp a device pixel ratio to this tier's cap.
    pub fn effective_dpr(&self, device_dpr: f32) -> f32 {
        match self.dpr_cap {
            Some(cap) => device_dpr.min(cap),
            None => device_dpr,
        }
    }
}

/// One built-in preset as compiled data: evani-core's build.rs turns each
/// `assets/tiers/<id>.json` into a row of `BUILTIN_TIERS` at build time, so
/// a game ships the numbers and no tier JSON or parser (README "How data
/// ships"). The JSON stays the source of truth; editing it (or
/// `bake.export` writing it) rebuilds the table.
pub(crate) struct TierRow {
    id: &'static str,
    label: &'static str,
    fps_target: u32,
    fps_max: Option<u32>,
    fps_floor: Option<u32>,
    refresh_hz: Option<[u32; 2]>,
    target_resolution: Option<[u32; 2]>,
    render_scale: Option<f32>,
    dpr_cap: Option<f32>,
    max_draw_calls: u32,
    max_triangles: u32,
    max_sprites: u32,
    max_particles: u32, #[cfg(feature = "texture-budget")] max_texture_memory: u32, #[cfg(feature = "point-lights")] max_point_lights: u32, #[cfg(feature = "shadow-cascades")] shadow_cascades: u32, #[cfg(feature = "ibl")] ibl_cube: u32, #[cfg(feature = "gtao")] gtao: u32,
    points: u32,
    stereo: bool,
    auto_select: bool,
    msaa: u32,
    effects_on: &'static [&'static str],
    shadow_map: Option<u32>,
    shadow_pcf: Option<u32>,
}

impl TierRow {
    fn preset(&self) -> TierPreset {
        TierPreset {
            id: self.id.into(),
            label: self.label.into(),
            fps_target: self.fps_target,
            fps_max: self.fps_max,
            fps_floor: self.fps_floor,
            refresh_hz: self.refresh_hz,
            target_resolution: self.target_resolution,
            render_scale: self.render_scale,
            dpr_cap: self.dpr_cap,
            max_draw_calls: self.max_draw_calls,
            max_triangles: self.max_triangles,
            max_sprites: self.max_sprites,
            max_particles: self.max_particles, #[cfg(feature = "texture-budget")] max_texture_memory: self.max_texture_memory, #[cfg(feature = "point-lights")] max_point_lights: self.max_point_lights, #[cfg(feature = "shadow-cascades")] shadow_cascades: self.shadow_cascades, #[cfg(feature = "ibl")] ibl_cube: self.ibl_cube, #[cfg(feature = "gtao")] gtao: self.gtao,
            points: self.points,
            stereo: self.stereo,
            auto_select: self.auto_select,
            msaa: self.msaa,
            effects_on: self.effects_on.iter().map(|s| (*s).to_string()).collect(),
            shadow_map: self.shadow_map,
            shadow_pcf: self.shadow_pcf,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/builtin_tiers.rs"));

/// The built-in tier files, in ascending order. Lite (budget tablets,
/// Mali-G52 class) sits below Low (DECISIONS.md 2026-10-16). Games read the
/// compiled table ([`Tiers::builtin`]); the text is here for tools and tests
/// and only reaches a download if something calls it.
pub const BUILTIN_TIER_JSON: [(&str, &str); 6] = [
    ("lite", include_str!("../../../assets/tiers/lite.json")),
    ("low", include_str!("../../../assets/tiers/low.json")),
    ("medium", include_str!("../../../assets/tiers/medium.json")),
    ("high", include_str!("../../../assets/tiers/high.json")),
    ("ultra", include_str!("../../../assets/tiers/ultra.json")),
    ("vr", include_str!("../../../assets/tiers/vr.json")),
];

/// Why a tier switch was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierError {
    /// No preset with that id.
    Unknown,
    /// A stereo (VR) preset outside an immersive WebXR session.
    NeedsXrSession,
}

impl core::fmt::Display for TierError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            TierError::Unknown => "no such tier",
            TierError::NeedsXrSession => {
                "the VR tier only runs inside an immersive WebXR session (press Enter VR)"
            }
        })
    }
}

/// World resource: the presets and the active one.
///
/// Stereo (VR) presets are only selectable inside an immersive WebXR
/// session ([`Tiers::set_xr_session`]); in a normal tab the list is Lite to
/// Ultra. The bench's mono VR preset run is the one exception
/// ([`Tiers::set_active_preset_run`]).
#[derive(Debug, Clone)]
pub struct Tiers {
    presets: Vec<TierPreset>,
    active: usize,
    /// An immersive WebXR session is running.
    xr_session: bool,
    /// This browser can start one (WebXR + immersive-vr + XRGPUBinding);
    /// written by the xr module, read by settings UIs for Enter VR.
    xr_available: bool,
    /// The flat tier to return to when the XR session ends.
    flat_before_xr: Option<usize>,
}

impl Tiers {
    /// The built-in presets, compiled from `assets/tiers/*.json` at build
    /// time (a malformed file fails the build, not the game). Equal to
    /// [`Tiers::from_builtin_json`]; a test checks.
    pub fn builtin() -> Result<Self, String> {
        Self::new(BUILTIN_TIERS.iter().map(TierRow::preset).collect())
    }

    /// The built-in presets parsed from their JSON text at run time, the way
    /// [`Tiers::builtin`] read them before the table was compiled. For tools
    /// and tests.
    pub fn from_builtin_json() -> Result<Self, String> {
        let presets = BUILTIN_TIER_JSON
            .iter()
            .map(|(name, json)| TierPreset::from_json(json).map_err(|e| format!("{name}.json: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(presets)
    }

    pub fn new(presets: Vec<TierPreset>) -> Result<Self, String> {
        if presets.is_empty() {
            return Err("no tier presets".into());
        }
        // Start on the middle auto-select tier until warm-up decides.
        let auto: Vec<usize> = presets
            .iter()
            .enumerate()
            .filter(|(_, p)| p.auto_select)
            .map(|(i, _)| i)
            .collect();
        let active = auto.get(auto.len().saturating_sub(1) / 2).copied().unwrap_or(0);
        Ok(Self {
            presets,
            active,
            xr_session: false,
            xr_available: false,
            flat_before_xr: None,
        })
    }

    /// A single-preset table (cannot fail).
    pub fn single(preset: TierPreset) -> Self {
        Self {
            presets: vec![preset],
            active: 0,
            xr_session: false,
            xr_available: false,
            flat_before_xr: None,
        }
    }

    pub fn presets(&self) -> &[TierPreset] {
        &self.presets
    }

    pub fn active(&self) -> &TierPreset {
        &self.presets[self.active]
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn get(&self, id: &str) -> Option<&TierPreset> {
        self.presets.iter().find(|p| p.id == id)
    }

    /// Switch tiers at runtime. Returns false for an unknown id, and for a
    /// stereo (VR) preset outside an immersive WebXR session.
    pub fn set_active(&mut self, id: &str) -> bool {
        self.try_set_active(id).is_ok()
    }

    /// [`Tiers::set_active`] with the reason when it refuses.
    pub fn try_set_active(&mut self, id: &str) -> Result<(), TierError> {
        let i = self
            .presets
            .iter()
            .position(|p| p.id == id)
            .ok_or(TierError::Unknown)?;
        if !self.is_selectable(&self.presets[i]) {
            return Err(TierError::NeedsXrSession);
        }
        self.active = i;
        Ok(())
    }

    /// The bench's explicit exception: run any preset, VR included (as a
    /// mono preset run), without an XR session. Nothing else may call it.
    pub fn set_active_preset_run(&mut self, id: &str) -> bool {
        match self.presets.iter().position(|p| p.id == id) {
            Some(i) => {
                self.active = i;
                true
            }
            None => false,
        }
    }

    /// Can `p` be the active tier now? Stereo presets need an XR session.
    pub fn is_selectable(&self, p: &TierPreset) -> bool {
        !p.stereo || self.xr_session
    }

    /// The presets a settings UI or the live API may offer now, with their
    /// index: Lite to Ultra in a normal tab, plus VR inside an XR session.
    pub fn selectable(&self) -> impl Iterator<Item = (usize, &TierPreset)> {
        self.presets
            .iter()
            .enumerate()
            .filter(|(_, p)| self.is_selectable(p))
    }

    /// Is an immersive WebXR session running?
    pub fn xr_session(&self) -> bool {
        self.xr_session
    }

    /// Can this browser start an immersive WebXR session with WebGPU? The
    /// Enter VR button shows only then.
    pub fn xr_available(&self) -> bool {
        self.xr_available
    }

    /// Written by the xr module after feature detection.
    pub fn set_xr_available(&mut self, available: bool) {
        self.xr_available = available;
    }

    /// The XR module starts or ends an immersive session. Starting it makes
    /// the stereo preset selectable and switches to it; ending it returns to
    /// the flat tier that was active before.
    pub fn set_xr_session(&mut self, on: bool) {
        if on == self.xr_session {
            return;
        }
        self.xr_session = on;
        if on {
            self.flat_before_xr = Some(self.active);
            if let Some(i) = self.presets.iter().position(|p| p.stereo) {
                self.active = i;
            }
        } else {
            let back = self.flat_before_xr.take();
            if self.presets[self.active].stereo {
                self.active = back
                    .filter(|&i| self.presets.get(i).is_some_and(|p| !p.stereo))
                    .or_else(|| self.presets.iter().position(|p| !p.stereo))
                    .unwrap_or(0);
            }
        }
    }

    /// Replace one preset (live tuning, bake). Unknown ids are appended.
    pub fn upsert(&mut self, preset: TierPreset) {
        match self.presets.iter().position(|p| p.id == preset.id) {
            Some(i) => self.presets[i] = preset,
            None => self.presets.push(preset),
        }
    }

    /// Ids of the tiers the warm-up may pick, in ascending order.
    pub fn auto_select_ids(&self) -> impl Iterator<Item = &str> {
        self.presets
            .iter()
            .filter(|p| p.auto_select)
            .map(|p| p.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compiled table (build.rs) and the JSON files say the same, field
    /// for field (floats bit for bit), in the same order.
    #[test]
    fn compiled_table_equals_the_json_files() {
        let compiled = Tiers::builtin().unwrap();
        let parsed = Tiers::from_builtin_json().unwrap();
        assert_eq!(compiled.presets(), parsed.presets());
        assert_eq!(compiled.active_index(), parsed.active_index());
        for (a, b) in compiled.presets().iter().zip(parsed.presets()) {
            assert_eq!(a.render_scale.map(f32::to_bits), b.render_scale.map(f32::to_bits), "{}", a.id);
            assert_eq!(a.dpr_cap.map(f32::to_bits), b.dpr_cap.map(f32::to_bits), "{}", a.id);
            a.validate().unwrap();
        }
    }

    /// The catalog says "96 MB", "1.5 GB (streamed)"; the tier files hold the
    /// plain megabytes (1.5 GB = 1536). With `texture-budget` (a game that
    /// lists `models`) the preset carries it too; the compiled table must
    /// agree with the file.
    #[test]
    fn texture_memory_follows_the_catalog() {
        fn megabytes(text: &str) -> u32 {
            let mut words = text.split_whitespace();
            let n: f64 = words.next().unwrap().parse().unwrap();
            match words.next().unwrap().trim_end_matches(',') {
                "MB" => n as u32,
                "GB" => (n * 1024.0) as u32,
                other => panic!("unit {other}"),
            }
        }
        let catalog: serde_json::Value = serde_json::from_str(include_str!("../../../catalog/tiers.json")).unwrap();
        let mut seen = 0;
        for t in catalog["tiers"].as_array().unwrap() {
            let id = t["id"].as_str().unwrap();
            let want = megabytes(t["max_texture_memory"].as_str().unwrap());
            let file = BUILTIN_TIER_JSON.iter().find(|(i, _)| *i == id).unwrap().1;
            let j: serde_json::Value = serde_json::from_str(file).unwrap();
            assert_eq!(j["max_texture_memory"].as_u64(), Some(u64::from(want)), "{id}.json");
            #[cfg(feature = "texture-budget")]
            {
                assert_eq!(Tiers::builtin().unwrap().get(id).unwrap().max_texture_memory, want, "{id} compiled");
                assert_eq!(TierPreset::from_json(file).unwrap().max_texture_memory, want, "{id} parsed");
            }
            seen += 1;
        }
        assert_eq!(seen, 6);
        assert_eq!(megabytes("1.5 GB (streamed)"), 1536);
    }

    #[test]
    fn msaa_is_4x_from_medium_up_and_validated() {
        let t = Tiers::builtin().unwrap();
        let m: Vec<(&str, u32)> = t.presets().iter().map(|p| (p.id.as_str(), p.msaa)).collect();
        assert_eq!(m, [("lite", 1), ("low", 1), ("medium", 4), ("high", 4), ("ultra", 4), ("vr", 4)]);
        let mut j: serde_json::Value = serde_json::from_str(BUILTIN_TIER_JSON[0].1).unwrap();
        j.as_object_mut().unwrap().remove("msaa");
        assert_eq!(TierPreset::from_json(&j.to_string()).unwrap().msaa, 1, "missing means 1");
        j["msaa"] = serde_json::json!(2);
        assert!(TierPreset::from_json(&j.to_string()).unwrap_err().contains("msaa"));
    }

    #[test]
    fn builtin_presets_match_spec() {
        let t = Tiers::builtin().unwrap();
        let ids: Vec<&str> = t.presets().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["lite", "low", "medium", "high", "ultra", "vr"]);

        // Lite: budget tablets (Mali-G52 class), below Low on every limit.
        let lite = t.get("lite").unwrap();
        assert_eq!((lite.fps_target, lite.fps_floor), (60, Some(30)));
        assert!(lite.auto_select && !lite.stereo && lite.shadow_map.is_none() && lite.msaa == 1);
        // Measured on a Lenovo TB305FU (Mali-G52): at 0.65 x, DPR 1, the bench's
        // Lite phases cost at most about 11 ms of GPU time (0.7 x: 12.1, 0.75 x: 14.2).
        assert_eq!((lite.render_scale, lite.dpr_cap), (Some(0.65), Some(1.0)));
        assert_eq!((lite.max_draw_calls, lite.max_triangles, lite.max_sprites, lite.max_particles, lite.points), (100, 40_000, 3_000, 1_000, 5));
        let low = t.get("low").unwrap();
        assert!(lite.render_scale < low.render_scale && lite.dpr_cap < low.dpr_cap);
        assert!(lite.max_draw_calls < low.max_draw_calls && lite.max_triangles < low.max_triangles);
        assert!(lite.max_sprites < low.max_sprites && lite.max_particles < low.max_particles);
        assert!(lite.points < low.points);
        assert!(lite.effects_on.iter().all(|e| low.effects_on.contains(e)), "Lite starts nothing Low doesn't");
        assert_eq!((low.fps_target, low.fps_floor), (60, Some(30)));
        assert_eq!((low.render_scale, low.dpr_cap), (Some(0.75), Some(1.25)));
        assert_eq!(
            (low.max_draw_calls, low.max_triangles, low.max_sprites, low.points),
            (150, 100_000, 5_000, 7)
        );
        let med = t.get("medium").unwrap();
        assert_eq!((med.render_scale, med.dpr_cap), (Some(1.0), Some(1.5)));
        assert_eq!(
            (med.max_draw_calls, med.max_triangles, med.max_sprites, med.points),
            (300, 400_000, 20_000, 11)
        );
        let high = t.get("high").unwrap();
        assert_eq!((high.fps_target, high.fps_max, high.dpr_cap), (60, Some(120), Some(2.0)));
        assert_eq!(
            (high.max_draw_calls, high.max_triangles, high.max_sprites, high.points),
            (800, 2_000_000, 100_000, 16)
        );
        let ultra = t.get("ultra").unwrap();
        assert_eq!(ultra.target_resolution, Some([2560, 1440]));
        assert_eq!((ultra.render_scale, ultra.dpr_cap), (Some(1.0), None));
        assert_eq!(
            (ultra.max_draw_calls, ultra.max_triangles, ultra.max_sprites, ultra.points),
            (2_500, 6_000_000, 500_000, 24)
        );
        let vr = t.get("vr").unwrap();
        assert_eq!(vr.refresh_hz, Some([72, 90]));
        assert!(vr.stereo && !vr.auto_select);
        assert_eq!(
            (vr.max_draw_calls, vr.max_triangles, vr.max_sprites, vr.points),
            (200, 750_000, 10_000, 9)
        );
        assert_eq!(t.auto_select_ids().count(), 5);
    }

    #[test]
    fn runtime_switching() {
        let mut t = Tiers::builtin().unwrap();
        assert_eq!(t.active().id, "medium");
        assert!(t.set_active("ultra"));
        assert_eq!(t.active().id, "ultra");
        assert!(!t.set_active("nope"));
        assert_eq!(t.active().id, "ultra");
        assert_eq!(t.active().effective_dpr(3.0), 3.0);
        assert_eq!(t.get("low").unwrap().effective_dpr(3.0), 1.25);
        assert!((t.get("vr").unwrap().frame_budget_ms() - 13.888).abs() < 0.01);
    }

    /// The VR tier only runs inside an immersive WebXR session; the bench's
    /// mono preset run is the one exception.
    #[test]
    fn vr_tier_needs_an_xr_session() {
        let mut t = Tiers::builtin().unwrap();
        assert!(t.set_active("high"));
        assert_eq!(t.try_set_active("vr"), Err(TierError::NeedsXrSession));
        assert!(!t.set_active("vr"));
        assert_eq!(t.active().id, "high");
        let flat: Vec<&str> = t.selectable().map(|(_, p)| p.id.as_str()).collect();
        assert_eq!(flat, ["lite", "low", "medium", "high", "ultra"]);
        assert!(!t.xr_available());

        // Entering XR switches to VR; leaving returns to the flat tier.
        t.set_xr_session(true);
        assert_eq!(t.active().id, "vr");
        assert!(t.selectable().any(|(_, p)| p.id == "vr"));
        t.set_xr_session(false);
        assert_eq!(t.active().id, "high");
        assert!(!t.set_active("vr"));

        // Bench: an explicit mono preset run.
        assert!(t.set_active_preset_run("vr"));
        assert_eq!(t.active().id, "vr");
        assert!(!t.set_active_preset_run("nope"));
    }
}
