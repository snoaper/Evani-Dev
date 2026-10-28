//! What the look draws on each tier (catalog `tiers.json`: High, Medium,
//! Low, Lite). The shading itself (smooth wrapped light, specular,
//! occlusion map, rim, hemisphere ambient) is the same WGSL on every tier;
//! only the extras scale, and each one that drops out has a fallback that
//! keeps the look:
//!
//! | Tier   | Shadows               | Contact AO          | Fur                     |
//! |--------|-----------------------|---------------------|-------------------------|
//! | High   | PCSS, 16 taps (2048)  | GTAO 3 slices x 4   | 8 shells + fins         |
//! | Medium | PCSS, 8 taps (1024)   | GTAO 2 slices x 3   | 6 shells + fins         |
//! | Low    | blob shadows          | baked occlusion map | clump map, no shells    |
//! | Lite   | blob shadows          | baked occlusion map | clump map, no shells    |
//!
//! Biplanar detail is one extra texture read per pixel on flagged
//! materials, so every tier keeps it.

/// The four tiers, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    High,
    Medium,
    Low,
    Lite,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::High, Tier::Medium, Tier::Low, Tier::Lite];

    /// The catalog's lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            Tier::High => "high",
            Tier::Medium => "medium",
            Tier::Low => "low",
            Tier::Lite => "lite",
        }
    }

    pub fn from_name(name: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name))
    }

    /// The look's plan for this tier.
    pub fn plan(self) -> TierPlan {
        match self {
            Tier::High => TierPlan {
                tier: self,
                shadow_map: Some(2048),
                pcf_radius: 1,
                pcss: true,
                gtao: Some(GtaoPlan { slices: 3, steps: 4 }),
                fur_shells: 8,
                fur_fins: true,
                fur_clump_fallback: false,
                biplanar: true,
                occlusion_map: true,
                render_scale: 1.0,
            },
            Tier::Medium => TierPlan {
                tier: self,
                shadow_map: Some(1024),
                pcf_radius: 0,
                pcss: true,
                gtao: Some(GtaoPlan { slices: 2, steps: 3 }),
                fur_shells: 6,
                fur_fins: true,
                fur_clump_fallback: false,
                biplanar: true,
                occlusion_map: true,
                render_scale: 1.0,
            },
            Tier::Low => TierPlan {
                tier: self,
                shadow_map: None,
                pcf_radius: 0,
                pcss: false,
                gtao: None,
                fur_shells: 0,
                fur_fins: false,
                fur_clump_fallback: true,
                biplanar: true,
                occlusion_map: true,
                render_scale: 0.75,
            },
            Tier::Lite => TierPlan {
                tier: self,
                shadow_map: None,
                pcf_radius: 0,
                pcss: false,
                gtao: None,
                fur_shells: 0,
                fur_fins: false,
                fur_clump_fallback: true,
                biplanar: true,
                occlusion_map: true,
                render_scale: 0.65,
            },
        }
    }
}

/// GTAO quality on a tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GtaoPlan {
    /// Directions searched per pixel.
    pub slices: u32,
    /// Steps per side of each direction.
    pub steps: u32,
}

/// What the look draws on one tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierPlan {
    pub tier: Tier,
    /// Shadow-map size (None = blob shadows only).
    pub shadow_map: Option<u32>,
    /// The engine's PCF radius (PCSS uses 16 taps when it is 1 or more).
    pub pcf_radius: u32,
    /// Soft shadows (needs feature `pcss` and a shadow map).
    pub pcss: bool,
    /// GTAO (needs the evani-fx-gtao effect).
    pub gtao: Option<GtaoPlan>,
    /// Most shells per fur material (also capped by the material and by
    /// `look.stylizedpbr.fur_shells_max`).
    pub fur_shells: u32,
    pub fur_fins: bool,
    /// Fur shades from the clump map alone (no shells).
    pub fur_clump_fallback: bool,
    pub biplanar: bool,
    /// The baked occlusion map is read (always: it is the cheap AO).
    pub occlusion_map: bool,
    pub render_scale: f32,
}

impl TierPlan {
    /// Shells to draw for a material asking for `wanted`, with the look's
    /// `fur_shells_max` param.
    pub fn shells_for(&self, wanted: u32, max_param: f32) -> u32 {
        let cap = if max_param.is_finite() { max_param.clamp(0.0, 8.0) as u32 } else { 0 };
        wanted.min(self.fur_shells).min(cap)
    }

    /// Is some contact darkening always present (GTAO or the baked map)?
    pub fn keeps_contact_shading(&self) -> bool {
        self.gtao.is_some() || self.occlusion_map
    }

    /// Is fur always visible as fur (shells or the clump fallback)?
    pub fn keeps_fur(&self) -> bool {
        self.fur_shells > 0 || self.fur_clump_fallback
    }

    /// Is something always grounding objects (a shadow map or blobs)?
    pub fn keeps_grounding(&self) -> bool {
        // Without a shadow map the engine's blob shadows are on.
        true
    }
}
