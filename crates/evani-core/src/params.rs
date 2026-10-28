//! The parameter registry: every tunable value in the engine.
//!
//! Effects, looks, kits, tier presets, the budget calculator and the
//! adaptive-quality controller register their values here, each with a dotted
//! name (`fx.bloom.threshold`), type, range, default, unit and one-line
//! description. This lives in `evani-core` (not `evani-live`) because shipped
//! games need it: baked values ship as JSON and are applied at startup.
//!
//! * Hot paths read through typed handles ([`F32Param`] ...): an index and a
//!   match, no hashing, no allocation.
//! * Every change bumps a global revision; consumers (e.g. a GPU uniform
//!   writer) remember the revision they last saw and call
//!   [`ParamRegistry::changed_since`], so uniforms update on the next frame.
//! * Values are clamped to their range; NaN/inf are rejected.
//! * [`ParamRegistry::set_batch`] is atomic: all or nothing.

use core::fmt;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    F32,
    I32,
    Bool,
    Vec3,
    /// Linear RGB, components usually 0..1 (HDR colours may use a larger max).
    Color,
    Enum,
}

impl ParamType {
    pub fn as_str(self) -> &'static str {
        match self {
            ParamType::F32 => "f32",
            ParamType::I32 => "i32",
            ParamType::Bool => "bool",
            ParamType::Vec3 => "vec3",
            ParamType::Color => "color",
            ParamType::Enum => "enum",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamValue {
    F32(f32),
    I32(i32),
    Bool(bool),
    Vec3([f32; 3]),
    Color([f32; 3]),
    /// Index into the definition's `options`.
    Enum(u32),
}

impl ParamValue {
    pub fn ty(&self) -> ParamType {
        match self {
            ParamValue::F32(_) => ParamType::F32,
            ParamValue::I32(_) => ParamType::I32,
            ParamValue::Bool(_) => ParamType::Bool,
            ParamValue::Vec3(_) => ParamType::Vec3,
            ParamValue::Color(_) => ParamType::Color,
            ParamValue::Enum(_) => ParamType::Enum,
        }
    }
}

/// Definition of one parameter. Build with the constructors and chain
/// `.range()`, `.unit()`, `.describe()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub name: String,
    pub ty: ParamType,
    pub default: ParamValue,
    /// Inclusive range for numeric types (per component for vec3/color).
    pub min: f32,
    pub max: f32,
    /// UI step hint (sliders).
    pub step: Option<f32>,
    pub unit: String,
    pub description: String,
    /// Option names for enums.
    pub options: Vec<String>,
    /// Who registered it, dotted like param names: `core`, `kit.example`,
    /// `look.basic`, `fx.bloom`, `render`... Filled in by the plugin context
    /// when empty.
    pub owner: String,
    /// Lives in a GPU uniform buffer (UI hint; changes never rebuild shaders).
    pub gpu: bool,
    /// Where `bake.export` writes it.
    pub bake: BakeTarget,
}

/// Where a baked value is written (see docs/LIVE_API.md, `schema.get`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BakeTarget {
    /// Derived from the owner: `kit.*` -> kit, `tier.*` -> tier,
    /// `aq`/`budget` -> policy, `live.*` -> none, everything else -> look.
    #[default]
    Auto,
    Look,
    Kit,
    Tier,
    Policy,
    /// Live-only, never baked.
    None,
}

impl BakeTarget {
    /// Resolve `Auto` from a param's owner.
    pub fn resolve(self, owner: &str) -> BakeTarget {
        if self != BakeTarget::Auto {
            return self;
        }
        match owner.split('.').next().unwrap_or("") {
            "kit" => BakeTarget::Kit,
            "tier" => BakeTarget::Tier,
            "aq" | "budget" => BakeTarget::Policy,
            "live" => BakeTarget::None,
            _ => BakeTarget::Look,
        }
    }
}

impl ParamDef {
    fn base(name: &str, ty: ParamType, default: ParamValue, min: f32, max: f32) -> Self {
        Self {
            name: name.to_string(),
            ty,
            default,
            min,
            max,
            step: None,
            unit: String::new(),
            description: String::new(),
            options: Vec::new(),
            owner: String::new(),
            gpu: false,
            bake: BakeTarget::Auto,
        }
    }

    pub fn f32(name: &str, default: f32) -> Self {
        Self::base(name, ParamType::F32, ParamValue::F32(default), f32::MIN, f32::MAX)
    }

    pub fn i32(name: &str, default: i32) -> Self {
        Self::base(
            name,
            ParamType::I32,
            ParamValue::I32(default),
            i32::MIN as f32,
            i32::MAX as f32,
        )
    }

    pub fn bool(name: &str, default: bool) -> Self {
        Self::base(name, ParamType::Bool, ParamValue::Bool(default), 0.0, 1.0)
    }

    pub fn vec3(name: &str, default: [f32; 3]) -> Self {
        Self::base(name, ParamType::Vec3, ParamValue::Vec3(default), f32::MIN, f32::MAX)
    }

    /// Linear RGB colour, range 0..1 unless changed with `.range()`.
    pub fn color(name: &str, default: [f32; 3]) -> Self {
        Self::base(name, ParamType::Color, ParamValue::Color(default), 0.0, 1.0)
    }

    pub fn enumeration(name: &str, options: &[&str], default: u32) -> Self {
        let mut d = Self::base(name, ParamType::Enum, ParamValue::Enum(default), 0.0, 0.0);
        d.options = options.iter().map(|s| s.to_string()).collect();
        d
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    pub fn unit(mut self, unit: &str) -> Self {
        self.unit = unit.to_string();
        self
    }

    pub fn describe(mut self, description: &str) -> Self {
        self.description = description.to_string();
        self
    }

    pub fn owner(mut self, owner: &str) -> Self {
        self.owner = owner.to_string();
        self
    }

    /// Mark as a GPU uniform value.
    pub fn gpu(mut self) -> Self {
        self.gpu = true;
        self
    }

    pub fn bake(mut self, target: BakeTarget) -> Self {
        self.bake = target;
        self
    }

    /// Clamp `v` into this definition. Returns `(value, was_clamped)`.
    pub fn clamp(&self, v: ParamValue) -> Result<(ParamValue, bool), ParamError> {
        let bad = || ParamError::TypeMismatch {
            name: self.name.clone(),
            expected: self.ty,
        };
        let cf = |x: f32| -> Result<(f32, bool), ParamError> {
            if !x.is_finite() {
                return Err(ParamError::NotFinite(self.name.clone()));
            }
            // Not f32::clamp: its panic message would ship float formatting.
            let c = crate::num::clamp(x, self.min, self.max);
            Ok((c, c != x))
        };
        Ok(match (self.ty, v) {
            (ParamType::F32, ParamValue::F32(x)) => {
                let (c, k) = cf(x)?;
                (ParamValue::F32(c), k)
            }
            (ParamType::F32, ParamValue::I32(x)) => {
                let (c, k) = cf(x as f32)?;
                (ParamValue::F32(c), k)
            }
            (ParamType::I32, ParamValue::I32(x)) => {
                let lo = self.min.max(i32::MIN as f32) as i32;
                let hi = self.max.min(i32::MAX as f32) as i32;
                let c = x.clamp(lo, hi.max(lo));
                (ParamValue::I32(c), c != x)
            }
            (ParamType::Bool, ParamValue::Bool(b)) => (ParamValue::Bool(b), false),
            (ParamType::Vec3, ParamValue::Vec3(a)) | (ParamType::Color, ParamValue::Vec3(a)) => {
                let mut out = [0.0; 3];
                let mut k = false;
                for i in 0..3 {
                    let (c, ki) = cf(a[i])?;
                    out[i] = c;
                    k |= ki;
                }
                let v = if self.ty == ParamType::Color {
                    ParamValue::Color(out)
                } else {
                    ParamValue::Vec3(out)
                };
                (v, k)
            }
            (ParamType::Color, ParamValue::Color(a)) => {
                return self.clamp(ParamValue::Vec3(a));
            }
            (ParamType::Enum, ParamValue::Enum(i)) => {
                if (i as usize) < self.options.len() {
                    (ParamValue::Enum(i), false)
                } else {
                    return Err(ParamError::BadEnum {
                        name: self.name.clone(),
                        value: i.to_string(),
                    });
                }
            }
            _ => return Err(bad()),
        })
    }

    /// Convert JSON (number, bool, `[x,y,z]`, option string or index).
    pub fn value_from_json(&self, v: &Value) -> Result<ParamValue, ParamError> {
        let bad = || ParamError::TypeMismatch {
            name: self.name.clone(),
            expected: self.ty,
        };
        Ok(match self.ty {
            ParamType::F32 => ParamValue::F32(v.as_f64().ok_or_else(bad)? as f32),
            ParamType::I32 => {
                let x = v.as_f64().ok_or_else(bad)?;
                if x.fract() != 0.0 {
                    return Err(bad());
                }
                ParamValue::I32(x.clamp(i32::MIN as f64, i32::MAX as f64) as i32)
            }
            ParamType::Bool => ParamValue::Bool(v.as_bool().ok_or_else(bad)?),
            ParamType::Vec3 | ParamType::Color => {
                let a = v.as_array().ok_or_else(bad)?;
                if a.len() != 3 {
                    return Err(bad());
                }
                let mut out = [0.0f32; 3];
                for (o, x) in out.iter_mut().zip(a) {
                    *o = x.as_f64().ok_or_else(bad)? as f32;
                }
                if self.ty == ParamType::Color {
                    ParamValue::Color(out)
                } else {
                    ParamValue::Vec3(out)
                }
            }
            ParamType::Enum => {
                if let Some(s) = v.as_str() {
                    let i = self.options.iter().position(|o| o == s).ok_or_else(|| {
                        ParamError::BadEnum {
                            name: self.name.clone(),
                            value: s.to_string(),
                        }
                    })?;
                    ParamValue::Enum(i as u32)
                } else {
                    ParamValue::Enum(v.as_u64().ok_or_else(bad)? as u32)
                }
            }
        })
    }

    pub fn value_to_json(&self, v: &ParamValue) -> Value {
        match *v {
            ParamValue::F32(x) => json!(x),
            ParamValue::I32(x) => json!(x),
            ParamValue::Bool(b) => json!(b),
            ParamValue::Vec3(a) | ParamValue::Color(a) => json!(a),
            ParamValue::Enum(i) => match self.options.get(i as usize) {
                Some(s) => json!(s),
                None => json!(i),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamError {
    BadName(String),
    Duplicate(String),
    Unknown(String),
    TypeMismatch { name: String, expected: ParamType },
    NotFinite(String),
    BadEnum { name: String, value: String },
    BadRange(String),
    Json(String),
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::BadName(n) => write!(
                f,
                "bad param name '{n}': use lower-case dotted paths like fx.bloom.threshold"
            ),
            ParamError::Duplicate(n) => write!(f, "param '{n}' is already registered"),
            ParamError::Unknown(n) => write!(f, "unknown param '{n}'"),
            ParamError::TypeMismatch { name, expected } => {
                write!(f, "param '{name}' expects {}", expected.as_str())
            }
            ParamError::NotFinite(n) => write!(f, "param '{n}': value must be finite"),
            ParamError::BadEnum { name, value } => {
                write!(f, "param '{name}': '{value}' is not one of its options")
            }
            ParamError::BadRange(n) => write!(f, "param '{n}': min is greater than max"),
            ParamError::Json(e) => write!(f, "params json: {e}"),
        }
    }
}

/// `true` for names like `fx.bloom.threshold`: two or more dot-separated
/// segments of `[a-z0-9_]`, each starting with a letter, at most 96 bytes.
pub fn is_valid_param_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 96 {
        return false;
    }
    let mut segments = 0;
    for seg in name.split('.') {
        segments += 1;
        let mut chars = seg.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    segments >= 2
}

/// Untyped handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParamId(pub u32);

macro_rules! typed_handle {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub ParamId);
        impl $name {
            pub fn id(self) -> ParamId {
                self.0
            }
        }
    };
}
typed_handle!(
    /// Handle to an `f32` param.
    F32Param
);
typed_handle!(
    /// Handle to an `i32` param.
    I32Param
);
typed_handle!(
    /// Handle to a `bool` param.
    BoolParam
);
typed_handle!(
    /// Handle to a `vec3` or `color` param.
    Vec3Param
);
typed_handle!(
    /// Handle to an enum param (reads the option index).
    EnumParam
);

/// Result of one successful set.
#[derive(Debug, Clone, PartialEq)]
pub struct SetOutcome {
    pub id: ParamId,
    pub previous: ParamValue,
    pub value: ParamValue,
    pub clamped: bool,
}

/// Result of applying a values file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplyReport {
    pub applied: usize,
    /// Names whose values were clamped into range.
    pub clamped: Vec<String>,
    /// Names not registered in this build (e.g. an effect compiled out).
    pub unknown: Vec<String>,
}

/// Serializable description of one param (schema export).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParamSchema {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ParamType,
    pub default: Value,
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f32>,
    pub unit: String,
    /// One-line description (`desc` on the wire, as in docs/LIVE_API.md).
    #[serde(rename = "desc", alias = "description")]
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub options: Vec<String>,
    pub owner: String,
    #[serde(default)]
    pub gpu: bool,
    #[serde(default)]
    pub bake: BakeTarget,
}

/// World resource.
#[derive(Debug, Default, Clone)]
pub struct ParamRegistry {
    defs: Vec<ParamDef>,
    values: Vec<ParamValue>,
    changed_at: Vec<u64>,
    revision: u64,
    by_name: BTreeMap<String, ParamId>,
}

impl ParamRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a param. Its default is clamped into range.
    pub fn register(&mut self, mut def: ParamDef) -> Result<ParamId, ParamError> {
        if !is_valid_param_name(&def.name) {
            return Err(ParamError::BadName(def.name));
        }
        if self.by_name.contains_key(&def.name) {
            return Err(ParamError::Duplicate(def.name));
        }
        if def.min > def.max {
            return Err(ParamError::BadRange(def.name));
        }
        if def.ty == ParamType::Enum && def.options.is_empty() {
            return Err(ParamError::BadEnum {
                name: def.name,
                value: "(no options)".into(),
            });
        }
        let (default, _) = def.clamp(def.default)?;
        def.default = default;
        let id = ParamId(self.defs.len() as u32);
        self.by_name.insert(def.name.clone(), id);
        self.defs.push(def);
        self.values.push(default);
        self.changed_at.push(self.revision);
        Ok(id)
    }

    fn typed(&mut self, def: ParamDef, want: &[ParamType]) -> Result<ParamId, ParamError> {
        if !want.contains(&def.ty) {
            return Err(ParamError::TypeMismatch {
                name: def.name,
                expected: want[0],
            });
        }
        self.register(def)
    }

    pub fn register_f32(&mut self, def: ParamDef) -> Result<F32Param, ParamError> {
        self.typed(def, &[ParamType::F32]).map(F32Param)
    }
    pub fn register_i32(&mut self, def: ParamDef) -> Result<I32Param, ParamError> {
        self.typed(def, &[ParamType::I32]).map(I32Param)
    }
    pub fn register_bool(&mut self, def: ParamDef) -> Result<BoolParam, ParamError> {
        self.typed(def, &[ParamType::Bool]).map(BoolParam)
    }
    pub fn register_vec3(&mut self, def: ParamDef) -> Result<Vec3Param, ParamError> {
        self.typed(def, &[ParamType::Vec3, ParamType::Color])
            .map(Vec3Param)
    }
    pub fn register_enum(&mut self, def: ParamDef) -> Result<EnumParam, ParamError> {
        self.typed(def, &[ParamType::Enum]).map(EnumParam)
    }

    // ---- lookups ----------------------------------------------------------

    pub fn id(&self, name: &str) -> Option<ParamId> {
        self.by_name.get(name).copied()
    }

    pub fn def(&self, id: ParamId) -> Option<&ParamDef> {
        self.defs.get(id.0 as usize)
    }

    pub fn value(&self, id: ParamId) -> Option<ParamValue> {
        self.values.get(id.0 as usize).copied()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Definitions in registration order.
    pub fn defs(&self) -> &[ParamDef] {
        &self.defs
    }

    pub fn f32_handle(&self, name: &str) -> Option<F32Param> {
        let id = self.id(name)?;
        (self.defs[id.0 as usize].ty == ParamType::F32).then_some(F32Param(id))
    }

    // ---- hot-path reads (no allocation) -----------------------------------

    #[inline]
    pub fn f32(&self, h: F32Param) -> f32 {
        match self.values.get(h.0.0 as usize) {
            Some(ParamValue::F32(v)) => *v,
            _ => 0.0,
        }
    }

    #[inline]
    pub fn i32(&self, h: I32Param) -> i32 {
        match self.values.get(h.0.0 as usize) {
            Some(ParamValue::I32(v)) => *v,
            _ => 0,
        }
    }

    #[inline]
    pub fn bool(&self, h: BoolParam) -> bool {
        matches!(self.values.get(h.0.0 as usize), Some(ParamValue::Bool(true)))
    }

    #[inline]
    pub fn vec3(&self, h: Vec3Param) -> [f32; 3] {
        match self.values.get(h.0.0 as usize) {
            Some(ParamValue::Vec3(v)) | Some(ParamValue::Color(v)) => *v,
            _ => [0.0; 3],
        }
    }

    #[inline]
    pub fn enum_index(&self, h: EnumParam) -> u32 {
        match self.values.get(h.0.0 as usize) {
            Some(ParamValue::Enum(v)) => *v,
            _ => 0,
        }
    }

    // ---- change tracking --------------------------------------------------

    /// Global revision; bumped by every effective change.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Did `id` change after revision `since`?
    #[inline]
    pub fn changed_since(&self, id: ParamId, since: u64) -> bool {
        self.changed_at
            .get(id.0 as usize)
            .is_some_and(|&r| r > since)
    }

    /// Did any of `ids` change after `since`? (e.g. all params of one
    /// uniform buffer).
    pub fn any_changed_since(&self, ids: &[ParamId], since: u64) -> bool {
        ids.iter().any(|&id| self.changed_since(id, since))
    }

    // ---- writes -------------------------------------------------------------

    /// Set one value (clamped). No-op changes do not bump the revision.
    pub fn set(&mut self, id: ParamId, value: ParamValue) -> Result<SetOutcome, ParamError> {
        let def = self
            .defs
            .get(id.0 as usize)
            .ok_or_else(|| ParamError::Unknown(format!("#{}", id.0)))?;
        let (v, clamped) = def.clamp(value)?;
        let i = id.0 as usize;
        let previous = self.values[i];
        if previous != v {
            self.revision += 1;
            self.values[i] = v;
            self.changed_at[i] = self.revision;
        }
        Ok(SetOutcome {
            id,
            previous,
            value: v,
            clamped,
        })
    }

    pub fn set_f32(&mut self, h: F32Param, v: f32) -> Result<SetOutcome, ParamError> {
        self.set(h.0, ParamValue::F32(v))
    }

    pub fn set_by_name(&mut self, name: &str, value: ParamValue) -> Result<SetOutcome, ParamError> {
        let id = self
            .id(name)
            .ok_or_else(|| ParamError::Unknown(name.to_string()))?;
        self.set(id, value)
    }

    pub fn set_json(&mut self, name: &str, value: &Value) -> Result<SetOutcome, ParamError> {
        let id = self
            .id(name)
            .ok_or_else(|| ParamError::Unknown(name.to_string()))?;
        let v = self.defs[id.0 as usize].value_from_json(value)?;
        self.set(id, v)
    }

    /// Set several values atomically: every entry is validated first; if any
    /// fails nothing changes.
    pub fn set_batch(&mut self, entries: &[(&str, Value)]) -> Result<Vec<SetOutcome>, ParamError> {
        let mut staged = Vec::with_capacity(entries.len());
        for (name, json) in entries {
            let id = self
                .id(name)
                .ok_or_else(|| ParamError::Unknown(name.to_string()))?;
            let def = &self.defs[id.0 as usize];
            let v = def.value_from_json(json)?;
            def.clamp(v)?;
            staged.push((id, v));
        }
        let mut out = Vec::with_capacity(staged.len());
        for (id, v) in staged {
            out.push(self.set(id, v)?);
        }
        Ok(out)
    }

    pub fn reset(&mut self, id: ParamId) -> Result<SetOutcome, ParamError> {
        let d = self
            .def(id)
            .ok_or_else(|| ParamError::Unknown(format!("#{}", id.0)))?
            .default;
        self.set(id, d)
    }

    pub fn reset_all(&mut self) {
        for i in 0..self.defs.len() {
            let _ = self.reset(ParamId(i as u32));
        }
    }

    // ---- JSON ----------------------------------------------------------------

    /// Descriptions of every param, sorted by name.
    pub fn schema(&self) -> Vec<ParamSchema> {
        self.by_name
            .values()
            .map(|&id| {
                let d = &self.defs[id.0 as usize];
                let numeric = matches!(
                    d.ty,
                    ParamType::F32 | ParamType::I32 | ParamType::Vec3 | ParamType::Color
                );
                let finite = |x: f32| (numeric && x.abs() < f32::MAX).then_some(x);
                ParamSchema {
                    name: d.name.clone(),
                    ty: d.ty,
                    default: d.value_to_json(&d.default),
                    value: d.value_to_json(&self.values[id.0 as usize]),
                    min: finite(d.min).filter(|_| d.ty != ParamType::I32 || d.min > i32::MIN as f32),
                    max: finite(d.max).filter(|_| d.ty != ParamType::I32 || d.max < i32::MAX as f32),
                    step: d.step,
                    unit: d.unit.clone(),
                    description: d.description.clone(),
                    options: d.options.clone(),
                    owner: d.owner.clone(),
                    gpu: d.gpu,
                    bake: d.bake.resolve(&d.owner),
                }
            })
            .collect()
    }

    /// `{"version":1,"params":[...]}` (used by `schema.get`).
    pub fn schema_json(&self) -> String {
        serde_json::to_string_pretty(&json!({ "version": 1, "params": self.schema() }))
            .unwrap_or_default()
    }

    /// A JSON Schema (draft 2020-12) describing a values file, so editors can
    /// validate baked params.
    pub fn values_json_schema(&self) -> String {
        let mut props = serde_json::Map::new();
        for &id in self.by_name.values() {
            let d = &self.defs[id.0 as usize];
            let mut p = serde_json::Map::new();
            p.insert("description".into(), json!(d.description));
            let finite = |x: f32| x.abs() < f32::MAX;
            match d.ty {
                ParamType::F32 | ParamType::I32 => {
                    p.insert(
                        "type".into(),
                        json!(if d.ty == ParamType::F32 { "number" } else { "integer" }),
                    );
                    if finite(d.min) {
                        p.insert("minimum".into(), json!(d.min));
                    }
                    if finite(d.max) {
                        p.insert("maximum".into(), json!(d.max));
                    }
                }
                ParamType::Bool => {
                    p.insert("type".into(), json!("boolean"));
                }
                ParamType::Vec3 | ParamType::Color => {
                    let mut item = json!({ "type": "number" });
                    if finite(d.min) && finite(d.max) {
                        item = json!({ "type": "number", "minimum": d.min, "maximum": d.max });
                    }
                    p.insert("type".into(), json!("array"));
                    p.insert("items".into(), item);
                    p.insert("minItems".into(), json!(3));
                    p.insert("maxItems".into(), json!(3));
                }
                ParamType::Enum => {
                    p.insert("enum".into(), json!(d.options));
                }
            }
            p.insert("default".into(), d.value_to_json(&d.default));
            props.insert(d.name.clone(), Value::Object(p));
        }
        serde_json::to_string_pretty(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Evani.Dev parameter values",
            "type": "object",
            "properties": props,
            "additionalProperties": true
        }))
        .unwrap_or_default()
    }

    /// Current values as `{"name": value}`, sorted by name. With
    /// `only_changed`, params at their default are left out.
    pub fn values_json(&self, only_changed: bool) -> String {
        let mut map = serde_json::Map::new();
        for &id in self.by_name.values() {
            let i = id.0 as usize;
            let d = &self.defs[i];
            if only_changed && self.values[i] == d.default {
                continue;
            }
            map.insert(d.name.clone(), d.value_to_json(&self.values[i]));
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    /// Apply a values file (baked params). Unknown names are reported, not
    /// errors, because they can belong to modules compiled out of this build.
    /// Type errors abort with nothing applied.
    pub fn apply_values_json(&mut self, json: &str) -> Result<ApplyReport, ParamError> {
        let map: serde_json::Map<String, Value> =
            serde_json::from_str(json).map_err(|e| ParamError::Json(e.to_string()))?;
        let mut report = ApplyReport::default();
        let mut known: Vec<(&str, Value)> = Vec::with_capacity(map.len());
        for (k, v) in &map {
            if self.id(k).is_some() {
                known.push((k.as_str(), v.clone()));
            } else {
                report.unknown.push(k.clone());
            }
        }
        for o in self.set_batch(&known)? {
            report.applied += 1;
            if o.clamped {
                report.clamped.push(self.defs[o.id.0 as usize].name.clone());
            }
        }
        Ok(report)
    }
}

impl ParamRegistry {
    /// The current value of F32 param `name`, or `default` when it is not
    /// registered (or not an F32). For draw systems that read a tunable by name.
    pub fn f32_or(&self, name: &str, default: f32) -> f32 {
        self.f32_handle(name).map_or(default, |h| self.f32(h))
    }

    /// True only when `name` is a registered Bool param that is on; false when
    /// it is off, missing or another type.
    pub fn bool_or(&self, name: &str) -> bool {
        matches!(self.id(name).and_then(|id| self.value(id)), Some(ParamValue::Bool(true)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> (ParamRegistry, F32Param, I32Param, BoolParam, Vec3Param, EnumParam) {
        let mut r = ParamRegistry::new();
        let thr = r
            .register_f32(
                ParamDef::f32("fx.bloom.threshold", 1.0)
                    .range(0.0, 10.0)
                    .unit("lum")
                    .describe("Brightness above which pixels bloom"),
            )
            .unwrap();
        let mips = r
            .register_i32(ParamDef::i32("fx.bloom.mips", 5).range(1.0, 8.0))
            .unwrap();
        let on = r.register_bool(ParamDef::bool("fx.vignette.enabled", true)).unwrap();
        let tint = r
            .register_vec3(ParamDef::color("look.basic.tint", [1.0, 0.9, 0.8]))
            .unwrap();
        let tm = r
            .register_enum(ParamDef::enumeration(
                "render.tonemap.mode",
                &["aces", "reinhard", "slot"],
                0,
            ))
            .unwrap();
        (r, thr, mips, on, tint, tm)
    }

    #[test]
    fn names_are_validated() {
        assert!(is_valid_param_name("fx.bloom.threshold"));
        assert!(is_valid_param_name("kit.example.speed_2"));
        for bad in ["", "fx", "Fx.bloom", "fx..bloom", "fx.bloom.", "1x.a", "fx.blo om", "fx.b-l"] {
            assert!(!is_valid_param_name(bad), "{bad}");
        }
        let mut r = ParamRegistry::new();
        assert!(matches!(r.register(ParamDef::f32("bad", 0.0)), Err(ParamError::BadName(_))));
        assert!(matches!(
            r.register(ParamDef::f32("a.b", 0.0).range(2.0, 1.0)),
            Err(ParamError::BadRange(_))
        ));
        r.register(ParamDef::f32("a.b", 0.0)).unwrap();
        assert!(matches!(r.register(ParamDef::f32("a.b", 0.0)), Err(ParamError::Duplicate(_))));
        assert!(r.register_i32(ParamDef::f32("a.c", 0.0)).is_err());
        assert!(r.register(ParamDef::enumeration("a.e", &[], 0)).is_err());
    }

    #[test]
    fn typed_reads_and_clamping() {
        let (mut r, thr, mips, on, tint, tm) = reg();
        assert_eq!(r.f32(thr), 1.0);
        assert_eq!(r.i32(mips), 5);
        assert!(r.bool(on));
        assert_eq!(r.vec3(tint), [1.0, 0.9, 0.8]);
        assert_eq!(r.enum_index(tm), 0);

        let o = r.set_f32(thr, 50.0).unwrap();
        assert!(o.clamped);
        assert_eq!(r.f32(thr), 10.0);
        assert!(r.set_f32(thr, f32::NAN).is_err());
        assert_eq!(r.f32(thr), 10.0);
        r.set(mips.id(), ParamValue::I32(-3)).unwrap();
        assert_eq!(r.i32(mips), 1);
        r.set(tint.id(), ParamValue::Color([2.0, -1.0, 0.5])).unwrap();
        assert_eq!(r.vec3(tint), [1.0, 0.0, 0.5]);
        assert!(r.set(tm.id(), ParamValue::Enum(3)).is_err());
        assert!(r.set(on.id(), ParamValue::F32(1.0)).is_err());
        // Default is clamped on registration.
        let h = r.register_f32(ParamDef::f32("a.b", 99.0).range(0.0, 1.0)).unwrap();
        assert_eq!(r.f32(h), 1.0);
    }

    #[test]
    fn revisions_track_changes() {
        let (mut r, thr, mips, ..) = reg();
        let seen = r.revision();
        assert!(!r.changed_since(thr.id(), seen));
        r.set_f32(thr, 1.0).unwrap(); // same value: no change
        assert_eq!(r.revision(), seen);
        r.set_f32(thr, 2.0).unwrap();
        assert!(r.changed_since(thr.id(), seen));
        assert!(!r.changed_since(mips.id(), seen));
        assert!(r.any_changed_since(&[mips.id(), thr.id()], seen));
        let seen = r.revision();
        assert!(!r.changed_since(thr.id(), seen));
        r.reset(thr.id()).unwrap();
        assert_eq!(r.f32(thr), 1.0);
        assert!(r.changed_since(thr.id(), seen));
    }

    #[test]
    fn batch_is_atomic() {
        let (mut r, thr, mips, ..) = reg();
        let rev = r.revision();
        let err = r.set_batch(&[
            ("fx.bloom.threshold", json!(3.0)),
            ("fx.bloom.mips", json!(2.5)),
        ]);
        assert!(err.is_err());
        assert_eq!(r.f32(thr), 1.0);
        assert_eq!(r.revision(), rev);
        assert!(r.set_batch(&[("nope.x", json!(1))]).is_err());
        let out = r
            .set_batch(&[
                ("fx.bloom.threshold", json!(3.0)),
                ("fx.bloom.mips", json!(2)),
                ("render.tonemap.mode", json!("reinhard")),
                ("look.basic.tint", json!([0.1, 0.2, 0.3])),
            ])
            .unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(r.f32(thr), 3.0);
        assert_eq!(r.i32(mips), 2);
        assert_eq!(out[0].previous, ParamValue::F32(1.0));
    }

    #[test]
    fn values_json_round_trip_and_unknowns() {
        let (mut r, thr, ..) = reg();
        r.set_f32(thr, 4.0).unwrap();
        let changed = r.values_json(true);
        assert!(changed.contains("fx.bloom.threshold"));
        assert!(!changed.contains("fx.bloom.mips"));
        let all = r.values_json(false);

        let (mut r2, thr2, ..) = reg();
        let rep = r2.apply_values_json(&all).unwrap();
        assert_eq!(rep.applied, 5);
        assert_eq!(r2.f32(thr2), 4.0);
        assert_eq!(r2.values_json(false), all);

        let rep = r2
            .apply_values_json(r#"{"fx.bloom.threshold": 99, "fx.godrays.strength": 1}"#)
            .unwrap();
        assert_eq!(rep.unknown, vec!["fx.godrays.strength"]);
        assert_eq!(rep.clamped, vec!["fx.bloom.threshold"]);
        assert!(r2.apply_values_json(r#"{"fx.bloom.mips": "x"}"#).is_err());
        assert!(r2.apply_values_json("not json").is_err());
    }

    #[test]
    fn schema_exports() {
        let (r, ..) = reg();
        let s = r.schema();
        assert_eq!(s.len(), 5);
        assert_eq!(s[0].name, "fx.bloom.mips"); // sorted
        let thr = s.iter().find(|p| p.name == "fx.bloom.threshold").unwrap();
        assert_eq!(thr.min, Some(0.0));
        assert_eq!(thr.unit, "lum");
        let json: Value = serde_json::from_str(&r.schema_json()).unwrap();
        assert_eq!(json["params"].as_array().unwrap().len(), 5);
        let js: Value = serde_json::from_str(&r.values_json_schema()).unwrap();
        assert_eq!(
            js["properties"]["render.tonemap.mode"]["enum"],
            json!(["aces", "reinhard", "slot"])
        );
        assert_eq!(js["properties"]["fx.bloom.threshold"]["maximum"], json!(10.0));
    }
}

/// Changing a registered param's range (ChimpDuty intake, limit 3: the
/// renderer's `hdr-color` feature widens `render.clear_color` past 1). Only
/// code that calls it carries it.
impl ParamRegistry {
    /// Set `id`'s range to `min..=max`. Its default and current value are
    /// clamped into the new range (a narrowed value counts as a change).
    pub fn set_range(&mut self, id: ParamId, min: f32, max: f32) -> Result<(), ParamError> {
        let i = id.0 as usize;
        let Some(def) = self.defs.get_mut(i) else {
            return Err(ParamError::Unknown(String::from("(bad param id)")));
        };
        if min.is_nan() || max.is_nan() || min > max {
            return Err(ParamError::BadRange(def.name.clone()));
        }
        def.min = min;
        def.max = max;
        let (default, _) = def.clamp(def.default)?;
        def.default = default;
        let (value, clamped) = def.clamp(self.values[i])?;
        if clamped {
            self.values[i] = value;
            self.revision += 1;
            self.changed_at[i] = self.revision;
        }
        Ok(())
    }
}

#[cfg(test)]
mod set_range_tests {
    use super::*;

    #[test]
    fn set_range_widens_and_narrows() {
        let mut r = ParamRegistry::new();
        let h = r.register_vec3(ParamDef::color("render.clear_color", [0.5, 0.5, 0.5])).unwrap();
        let id = r.id("render.clear_color").unwrap();
        // 0..1 by default: 4 clamps.
        r.set_by_name("render.clear_color", ParamValue::Vec3([4.0, 0.5, 0.5])).unwrap();
        assert_eq!(r.vec3(h), [1.0, 0.5, 0.5]);
        r.set_range(id, 0.0, 64.0).unwrap();
        r.set_by_name("render.clear_color", ParamValue::Vec3([4.0, 0.5, 0.5])).unwrap();
        assert_eq!(r.vec3(h), [4.0, 0.5, 0.5]);
        assert_eq!(r.def(id).unwrap().max, 64.0);
        // Narrowing clamps the current value and counts as a change.
        let rev = r.revision();
        r.set_range(id, 0.0, 2.0).unwrap();
        assert_eq!(r.vec3(h), [2.0, 0.5, 0.5]);
        assert!(r.revision() > rev);
        assert!(matches!(r.set_range(id, 3.0, 1.0), Err(ParamError::BadRange(_))));
    }
}
