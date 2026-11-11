//! The category registry: which categories and assets this build carries.
//!
//! evani-procedural-lib fills it with exactly the assets the game's Cargo
//! features turned on (one feature per asset, one per category), so a
//! registry never names an asset that is not compiled in.

use evani_core::params::{ParamDef, ParamError, ParamRegistry, ParamValue};

use crate::asset::{Path, ProcAsset};
use crate::style::Generated;
use crate::spec::{ParamSpec, Values};

/// One category of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Category {
    /// Its id and Cargo feature (`terrain`).
    pub id: &'static str,
    pub name: &'static str,
}

/// The seed every CPU asset has (`<prefix>.seed`): whole numbers.
pub const SEED: ParamSpec = ParamSpec::num("seed", 1.0, 0.0, 9999.0).step(1.0).describe("Seed: another number gives another variation of the same asset");

/// The categories and assets compiled into this build.
#[derive(Default)]
pub struct Registry {
    categories: Vec<Category>,
    assets: Vec<Box<dyn ProcAsset>>,
    #[cfg(feature = "behaviour")]
    behaviours: crate::behaviour::Behaviours,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry").field("categories", &self.categories).field("assets", &self.features()).finish()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a category (once; a second add of the same id is ignored).
    pub fn add_category(&mut self, c: Category) {
        if !self.categories.iter().any(|x| x.id == c.id) {
            self.categories.push(c);
        }
    }

    /// Add an asset; its category must have been added. Returns false (and
    /// adds nothing) for a duplicate feature or an unknown category.
    pub fn add(&mut self, a: Box<dyn ProcAsset>) -> bool {
        let f = a.info().feature();
        if self.get(&f).is_some() || !self.categories.iter().any(|c| c.id == a.info().category) {
            return false;
        }
        self.assets.push(a);
        true
    }

    /// The behaviour fields this build's assets registered (feature `behaviour`).
    #[cfg(feature = "behaviour")]
    pub fn behaviours(&self) -> &crate::behaviour::Behaviours {
        &self.behaviours
    }

    #[cfg(feature = "behaviour")]
    pub fn behaviours_mut(&mut self) -> &mut crate::behaviour::Behaviours {
        &mut self.behaviours
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    pub fn assets(&self) -> impl Iterator<Item = &dyn ProcAsset> {
        self.assets.iter().map(|a| a.as_ref())
    }

    /// The assets of one category, in the order they were added.
    pub fn in_category<'a>(&'a self, category: &'a str) -> impl Iterator<Item = &'a dyn ProcAsset> + 'a {
        self.assets().filter(move |a| a.info().category == category)
    }

    /// Every asset's feature (`terrain-rolling-hills`).
    pub fn features(&self) -> Vec<String> {
        self.assets.iter().map(|a| a.info().feature()).collect()
    }

    /// The asset by feature (`terrain-rolling-hills`).
    pub fn get(&self, feature: &str) -> Option<&dyn ProcAsset> {
        self.assets().find(|a| a.info().feature() == feature)
    }

    /// The asset's params with the registry's own: `on` for every asset,
    /// `seed` for CPU assets, then the asset's.
    pub fn specs(a: &dyn ProcAsset) -> Vec<ParamSpec> {
        let mut out = Vec::new();
        if a.info().path == Path::Cpu {
            out.push(SEED);
        }
        out.extend_from_slice(a.params());
        out
    }

    /// Every live param name of an asset (its on/off switch first). A
    /// wrapped module's own params are the module's (and not listed).
    pub fn param_names(&self, feature: &str) -> Vec<String> {
        let Some(a) = self.get(feature) else { return Vec::new() };
        let p = a.info().prefix();
        std::iter::once(format!("{p}.on")).chain(Self::specs(a).iter().map(|s| format!("{p}.{}", s.name))).collect()
    }

    /// Register every asset's live params (`procedural.<category>.<asset>.*`).
    pub fn register_params(&self, reg: &mut ParamRegistry) -> Result<(), ParamError> {
        for a in self.assets() {
            let p = a.info().prefix();
            let on = ParamDef::bool(&format!("{p}.on"), true).describe(&format!("{}: on (drawn) or off (removed)", a.info().name)).owner("procedural");
            if reg.id(&on.name).is_none() {
                reg.register(on)?;
            }
            for s in Self::specs(a) {
                let d = s.def(&p);
                if reg.id(&d.name).is_none() {
                    reg.register(d)?;
                }
            }
        }
        Ok(())
    }

    /// Whether the asset is switched on (true when its switch is not
    /// registered yet).
    pub fn is_on(&self, reg: &ParamRegistry, feature: &str) -> bool {
        let Some(a) = self.get(feature) else { return false };
        !matches!(reg.id(&format!("{}.on", a.info().prefix())).and_then(|id| reg.value(id)), Some(ParamValue::Bool(false)))
    }

    /// The asset's live values and seed.
    pub fn values(&self, reg: &ParamRegistry, feature: &str) -> Option<(Values, u64)> {
        let a = self.get(feature)?;
        let v = Values::read(reg, &a.info().prefix(), &Self::specs(a));
        let seed = v.f("seed").max(0.0) as u64;
        Some((v, seed))
    }

    /// Generate the asset from its live values in art style `style`
    /// (`WK01`..`WK24`; a style the asset lacks falls back to Realism).
    pub fn generate(&self, reg: &ParamRegistry, feature: &str, style: &str) -> Option<Generated> {
        self.generate_at(reg, feature, style, 1.0)
    }

    /// [`Self::generate`] at mesh density `density` (the active tier's,
    /// [`crate::detail::Density::for_tier`]).
    pub fn generate_at(&self, reg: &ParamRegistry, feature: &str, style: &str, density: f32) -> Option<Generated> {
        let a = self.get(feature)?;
        let (v, seed) = self.values(reg, feature)?;
        let v = v.with_density(density);
        let (style, fallback) = crate::style::resolve(a.styles(), style);
        let style = crate::style::STYLES.iter().copied().find(|s| *s == style).unwrap_or(crate::style::REALISM);
        Some(Generated { output: a.generate(&v, seed, style), style, fallback })
    }
}
