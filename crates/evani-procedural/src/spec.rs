//! An asset's params: the schema ([`ParamSpec`]) and the values one
//! generation reads ([`Values`]).
//!
//! Every spec becomes a live param `procedural.<category>.<asset>.<name>`
//! (`-` in ids becomes `_`), registered by [`crate::ProcModule`], so the
//! Look Lab, the library viewer and AI agents (`param_set`) move the same
//! values a game's generation reads.

use evani_core::params::{ParamDef, ParamRegistry, ParamValue};

/// What kind of value a param holds, with its default and limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecKind {
    /// A slider.
    Num { default: f32, min: f32, max: f32, step: f32, unit: &'static str },
    /// A linear RGB colour.
    Color([f32; 3]),
}

/// One param of an asset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// The last segment of the live name (`[a-z][a-z0-9_]*`).
    pub name: &'static str,
    pub describe: &'static str,
    pub kind: SpecKind,
}

impl ParamSpec {
    /// A slider from `min` to `max` (step: a hundredth of the range).
    pub const fn num(name: &'static str, default: f32, min: f32, max: f32) -> Self {
        Self { name, describe: "", kind: SpecKind::Num { default, min, max, step: (max - min) / 100.0, unit: "" } }
    }

    /// A colour (linear RGB).
    pub const fn color(name: &'static str, default: [f32; 3]) -> Self {
        Self { name, describe: "", kind: SpecKind::Color(default) }
    }

    pub const fn step(mut self, step: f32) -> Self {
        if let SpecKind::Num { default, min, max, unit, .. } = self.kind {
            self.kind = SpecKind::Num { default, min, max, step, unit };
        }
        self
    }

    pub const fn unit(mut self, unit: &'static str) -> Self {
        if let SpecKind::Num { default, min, max, step, .. } = self.kind {
            self.kind = SpecKind::Num { default, min, max, step, unit };
        }
        self
    }

    pub const fn describe(mut self, describe: &'static str) -> Self {
        self.describe = describe;
        self
    }

    /// The live param definition under `prefix`.
    pub fn def(&self, prefix: &str) -> ParamDef {
        let name = format!("{prefix}.{}", self.name);
        let d = match self.kind {
            SpecKind::Num { default, min, max, step, unit } => ParamDef::f32(&name, default).range(min, max).step(step).unit(unit),
            SpecKind::Color(c) => ParamDef::color(&name, c),
        };
        d.describe(self.describe).owner("procedural")
    }

    pub fn default_value(&self) -> Value {
        match self.kind {
            SpecKind::Num { default, .. } => Value::F32(default),
            SpecKind::Color(c) => Value::Color(c),
        }
    }
}

/// One param's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    F32(f32),
    Color([f32; 3]),
}

/// The values one generation reads, by spec name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Values {
    vals: Vec<(&'static str, Value)>,
    /// The mesh density the active tier asks for (0: not set, read as 1;
    /// [`crate::detail::Density`]).
    density: f32,
}

impl Values {
    /// Every spec at its default.
    pub fn defaults(specs: &[ParamSpec]) -> Self {
        Self { vals: specs.iter().map(|s| (s.name, s.default_value())).collect(), density: 0.0 }
    }

    /// The live values under `prefix` (a spec the registry lacks keeps its
    /// default).
    pub fn read(reg: &ParamRegistry, prefix: &str, specs: &[ParamSpec]) -> Self {
        let mut v = Self::defaults(specs);
        for (name, val) in &mut v.vals {
            let live = reg.id(&format!("{prefix}.{name}")).and_then(|id| reg.value(id));
            match (live, &mut *val) {
                (Some(ParamValue::F32(x)), Value::F32(d)) => *d = x,
                (Some(ParamValue::Color(c)), Value::Color(d)) => *d = c,
                _ => {}
            }
        }
        v
    }

    /// A slider's value (0 when the asset has no such param).
    pub fn f(&self, name: &str) -> f32 {
        match self.get(name) {
            Some(Value::F32(x)) => x,
            _ => 0.0,
        }
    }

    /// A colour's value (mid grey when the asset has no such param).
    pub fn color(&self, name: &str) -> [f32; 3] {
        match self.get(name) {
            Some(Value::Color(c)) => c,
            _ => [0.5; 3],
        }
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.vals.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    /// How finely carved relief is refined: 1 at High and Ultra, under 1 on
    /// the lower tiers (round 9, tiered mesh density; the relief's size and
    /// place never change with it).
    pub fn density(&self) -> f32 {
        if self.density > 0.0 { self.density } else { 1.0 }
    }

    /// The same values at mesh density `d`.
    pub fn with_density(mut self, d: f32) -> Self {
        self.density = d.clamp(0.1, 2.0);
        self
    }

    /// Set one value (tests, previews); unknown names are ignored.
    pub fn set(&mut self, name: &str, value: Value) {
        if let Some((_, v)) = self.vals.iter_mut().find(|(n, _)| *n == name) {
            *v = value;
        }
    }
}
