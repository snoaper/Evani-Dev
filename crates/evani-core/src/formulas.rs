//! Named CPU-side formulas: each is either a piecewise-linear [`Curve`] or an
//! [`Expr`]. Kits and engine systems register them with named input variables
//! (a curve uses the first one as x) and evaluate them through a handle. The
//! live API can swap a formula between curve and expression at runtime; baked
//! formulas ship as JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::curve::Curve;
use crate::expr::{Expr, ExprError};
use crate::params::is_valid_param_name;

#[derive(Debug, Clone, PartialEq)]
pub enum FormulaBody {
    Curve(Curve),
    Expr(Expr),
}

impl FormulaBody {
    #[inline]
    pub fn eval(&self, vars: &[f32]) -> f32 {
        match self {
            FormulaBody::Curve(c) => c.eval(vars.first().copied().unwrap_or(0.0)),
            FormulaBody::Expr(e) => e.eval(vars),
        }
    }
}

/// JSON form: `{"curve": {"points": [[x,y],...]}}` or `{"expr": "..."}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormulaJson {
    Curve(Curve),
    Expr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormulaDef {
    pub name: String,
    pub description: String,
    /// Input variable names; a curve reads `vars[0]`.
    pub vars: Vec<String>,
    pub owner: String,
    /// Allowed x range of a curve body (`None` = unbounded). Live edits clamp
    /// control points into it; evaluation is unaffected.
    pub x_range: Option<[f32; 2]>,
    /// Allowed y range of a curve body (`None` = unbounded).
    pub y_range: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormulaId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum FormulaError {
    BadName(String),
    Duplicate(String),
    Unknown(String),
    Expr(ExprError),
    Curve(String),
    Json(String),
}

impl core::fmt::Display for FormulaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FormulaError::BadName(n) => write!(f, "bad formula name '{n}'"),
            FormulaError::Duplicate(n) => write!(f, "formula '{n}' already registered"),
            FormulaError::Unknown(n) => write!(f, "unknown formula '{n}'"),
            FormulaError::Expr(e) => write!(f, "expression error {e}"),
            FormulaError::Curve(e) => write!(f, "curve error: {e}"),
            FormulaError::Json(e) => write!(f, "formula json: {e}"),
        }
    }
}

/// World resource.
#[derive(Debug, Default, Clone)]
pub struct Formulas {
    defs: Vec<FormulaDef>,
    bodies: Vec<FormulaBody>,
    defaults: Vec<FormulaBody>,
    changed_at: Vec<u64>,
    revision: u64,
    by_name: BTreeMap<String, FormulaId>,
}

impl Formulas {
    pub fn register(
        &mut self,
        name: &str,
        description: &str,
        vars: &[&str],
        body: FormulaJson,
    ) -> Result<FormulaId, FormulaError> {
        if !is_valid_param_name(name) {
            return Err(FormulaError::BadName(name.to_string()));
        }
        if self.by_name.contains_key(name) {
            return Err(FormulaError::Duplicate(name.to_string()));
        }
        let body = Self::build(vars, body)?;
        let id = FormulaId(self.defs.len() as u32);
        self.defs.push(FormulaDef {
            name: name.to_string(),
            description: description.to_string(),
            vars: vars.iter().map(|s| s.to_string()).collect(),
            owner: String::new(),
            x_range: None,
            y_range: None,
        });
        self.defaults.push(body.clone());
        self.bodies.push(body);
        self.changed_at.push(self.revision);
        self.by_name.insert(name.to_string(), id);
        Ok(id)
    }

    fn build(vars: &[&str], body: FormulaJson) -> Result<FormulaBody, FormulaError> {
        Ok(match body {
            FormulaJson::Curve(c) => {
                c.validate()
                    .map_err(|e| FormulaError::Curve(e.to_string()))?;
                FormulaBody::Curve(c)
            }
            FormulaJson::Expr(src) => {
                FormulaBody::Expr(Expr::compile(&src, vars).map_err(FormulaError::Expr)?)
            }
        })
    }

    pub(crate) fn set_owner(&mut self, id: FormulaId, owner: &str) {
        if let Some(d) = self.defs.get_mut(id.0 as usize) {
            d.owner = owner.to_string();
        }
    }

    pub fn id(&self, name: &str) -> Option<FormulaId> {
        self.by_name.get(name).copied()
    }

    pub fn def(&self, id: FormulaId) -> Option<&FormulaDef> {
        self.defs.get(id.0 as usize)
    }

    /// Give a curve formula its axis ranges (the live API clamps control
    /// points into them). A range with min > max or a non-finite end is
    /// ignored.
    pub fn set_ranges(&mut self, id: FormulaId, x: Option<[f32; 2]>, y: Option<[f32; 2]>) {
        let ok = |r: Option<[f32; 2]>| r.filter(|r| r[0].is_finite() && r[1].is_finite() && r[0] <= r[1]);
        if let Some(d) = self.defs.get_mut(id.0 as usize) {
            d.x_range = ok(x);
            d.y_range = ok(y);
        }
    }

    /// Every formula id, sorted by name.
    pub fn ids(&self) -> impl Iterator<Item = FormulaId> + '_ {
        self.by_name.values().copied()
    }

    /// The body registered by the kit or look (before baked files and live
    /// edits).
    pub fn default_body(&self, id: FormulaId) -> Option<&FormulaBody> {
        self.defaults.get(id.0 as usize)
    }

    pub fn body(&self, id: FormulaId) -> Option<&FormulaBody> {
        self.bodies.get(id.0 as usize)
    }

    /// Evaluate (0 for an unknown id). No allocation.
    #[inline]
    pub fn eval(&self, id: FormulaId, vars: &[f32]) -> f32 {
        self.bodies
            .get(id.0 as usize)
            .map(|b| b.eval(vars))
            .unwrap_or(0.0)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn changed_since(&self, id: FormulaId, since: u64) -> bool {
        self.changed_at
            .get(id.0 as usize)
            .is_some_and(|&r| r > since)
    }

    /// Replace a formula's body (curve or expression). On error the old body
    /// stays. Returns the previous body (for undo).
    pub fn set(&mut self, id: FormulaId, body: FormulaJson) -> Result<FormulaBody, FormulaError> {
        let i = id.0 as usize;
        let def = self
            .defs
            .get(i)
            .ok_or_else(|| FormulaError::Unknown(format!("#{}", id.0)))?;
        let vars: Vec<&str> = def.vars.iter().map(String::as_str).collect();
        let new = Self::build(&vars, body)?;
        self.revision += 1;
        self.changed_at[i] = self.revision;
        Ok(core::mem::replace(&mut self.bodies[i], new))
    }

    pub fn set_by_name(&mut self, name: &str, body: FormulaJson) -> Result<FormulaBody, FormulaError> {
        let id = self
            .id(name)
            .ok_or_else(|| FormulaError::Unknown(name.to_string()))?;
        self.set(id, body)
    }

    pub fn reset(&mut self, id: FormulaId) -> Result<(), FormulaError> {
        let i = id.0 as usize;
        let d = self
            .defaults
            .get(i)
            .cloned()
            .ok_or_else(|| FormulaError::Unknown(format!("#{}", id.0)))?;
        self.bodies[i] = d;
        self.revision += 1;
        self.changed_at[i] = self.revision;
        Ok(())
    }

    pub fn to_json_body(&self, id: FormulaId) -> Option<FormulaJson> {
        Some(match self.body(id)? {
            FormulaBody::Curve(c) => FormulaJson::Curve(c.clone()),
            FormulaBody::Expr(e) => FormulaJson::Expr(e.source().to_string()),
        })
    }

    /// Schema entries sorted by name.
    pub fn schema(&self) -> Value {
        let list: Vec<Value> = self
            .by_name
            .values()
            .map(|&id| {
                let d = &self.defs[id.0 as usize];
                json!({
                    "name": d.name,
                    "description": d.description,
                    "vars": d.vars,
                    "owner": d.owner,
                    "body": self.to_json_body(id),
                })
            })
            .collect();
        Value::Array(list)
    }

    /// `{"name": {"curve": ...} | {"expr": "..."}}` sorted by name. With
    /// `only_changed`, formulas equal to their default are left out.
    pub fn values_json(&self, only_changed: bool) -> String {
        let mut map = serde_json::Map::new();
        for &id in self.by_name.values() {
            let i = id.0 as usize;
            if only_changed && self.bodies[i] == self.defaults[i] {
                continue;
            }
            if let Some(b) = self.to_json_body(id) {
                map.insert(
                    self.defs[i].name.clone(),
                    serde_json::to_value(b).unwrap_or(Value::Null),
                );
            }
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    /// Apply a formulas file; unknown names are returned, not errors.
    /// Atomic: on any compile error nothing changes.
    pub fn apply_values_json(&mut self, json: &str) -> Result<Vec<String>, FormulaError> {
        let map: BTreeMap<String, FormulaJson> =
            serde_json::from_str(json).map_err(|e| FormulaError::Json(e.to_string()))?;
        let mut unknown = Vec::new();
        let mut staged = Vec::new();
        for (name, body) in map {
            match self.id(&name) {
                Some(id) => {
                    let d = &self.defs[id.0 as usize];
                    let vars: Vec<&str> = d.vars.iter().map(String::as_str).collect();
                    staged.push((id, Self::build(&vars, body)?));
                }
                None => unknown.push(name),
            }
        }
        for (id, body) in staged {
            let i = id.0 as usize;
            self.bodies[i] = body;
            self.revision += 1;
            self.changed_at[i] = self.revision;
        }
        Ok(unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_and_expr_formulas_swap_at_runtime() {
        let mut f = Formulas::default();
        let lod = f
            .register(
                "render.lod.distance_scale",
                "LOD distance multiplier by fps headroom",
                &["headroom"],
                FormulaJson::Curve(Curve::linear(0.0, 0.5, 1.0, 1.5).unwrap()),
            )
            .unwrap();
        assert_eq!(f.eval(lod, &[0.5]), 1.0);
        let rev = f.revision();
        let old = f
            .set(lod, FormulaJson::Expr("mix(0.5, 1.5, headroom * headroom)".into()))
            .unwrap();
        assert!(matches!(old, FormulaBody::Curve(_)));
        assert!(f.changed_since(lod, rev));
        assert_eq!(f.eval(lod, &[0.5]), 0.75);
        // A bad expression is rejected and the old body keeps running.
        assert!(f.set(lod, FormulaJson::Expr("mix(".into())).is_err());
        assert!(f.set(lod, FormulaJson::Expr("unknown_var".into())).is_err());
        assert_eq!(f.eval(lod, &[0.5]), 0.75);
        f.reset(lod).unwrap();
        assert_eq!(f.eval(lod, &[0.5]), 1.0);
        assert_eq!(f.eval(FormulaId(9), &[0.5]), 0.0);
    }

    #[test]
    fn json_round_trip_and_atomic_apply() {
        let mut f = Formulas::default();
        let a = f
            .register("kit.a.speed", "", &["t"], FormulaJson::Expr("t * 2".into()))
            .unwrap();
        f.register(
            "kit.a.curve",
            "",
            &["x"],
            FormulaJson::Curve(Curve::constant(1.0)),
        )
        .unwrap();
        assert!(f.register("kit.a.speed", "", &[], FormulaJson::Expr("1".into())).is_err());
        assert!(f.register("Bad", "", &[], FormulaJson::Expr("1".into())).is_err());
        assert_eq!(f.values_json(true), "{}");
        f.set(a, FormulaJson::Expr("t * 3".into())).unwrap();
        let changed = f.values_json(true);
        assert!(changed.contains("t * 3") && !changed.contains("kit.a.curve"));

        let mut g = Formulas::default();
        g.register("kit.a.speed", "", &["t"], FormulaJson::Expr("t".into()))
            .unwrap();
        let unknown = g
            .apply_values_json(&f.values_json(false))
            .unwrap();
        assert_eq!(unknown, vec!["kit.a.curve".to_string()]);
        assert_eq!(g.eval(FormulaId(0), &[2.0]), 6.0);
        assert!(
            g.apply_values_json(r#"{"kit.a.speed":{"expr":"t +"}}"#)
                .is_err()
        );
        assert_eq!(g.eval(FormulaId(0), &[2.0]), 6.0);
        assert_eq!(f.schema().as_array().unwrap().len(), 2);
    }
}
