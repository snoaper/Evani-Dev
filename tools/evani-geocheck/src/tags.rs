//! Intentional cases: a rock sunk into the ground, a balloon that flies.
//! Level or kit data tags them with a reason and the checks skip them (the
//! report still lists them, as tagged).
//!
//! Sources (both are read into the same JSON shape):
//! - `geocheck.toml` next to the kit's or app's `Cargo.toml`:
//!   ```toml
//!   [thresholds]
//!   sink = 0.3
//!
//!   [[tag]]
//!   check = "floating"          # or "*" for every check
//!   object = "balloon*"         # glob on the object label (optional)
//!   at = [12.0, 8.0, -40.0]     # with radius (optional)
//!   radius = 3.0
//!   reason = "hot-air balloon: flies on purpose"
//!   ```
//! - a `"geocheck"` array in kit data JSON (same fields per entry).

use serde_json::Value;

use crate::geom::{V3, dist};
use crate::report::{Check, Issue};

#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    /// `None`: every check.
    pub check: Option<Check>,
    /// Glob (`*`, `?`) on any object label of the issue, case-insensitive.
    pub object: Option<String>,
    /// Issues within `radius` of this point.
    pub at: Option<V3>,
    pub radius: f64,
    /// Why it is intentional (required).
    pub reason: String,
}

impl Tag {
    pub fn matches(&self, i: &Issue) -> bool {
        if self.check.is_some_and(|c| c != i.check) {
            return false;
        }
        if let Some(g) = &self.object
            && !i.objects.iter().any(|o| glob(g, o))
        {
            return false;
        }
        if let Some(p) = self.at {
            let near_box = i.aabb.is_some_and(|b| b.contains(p, self.radius));
            if dist(p, i.at) > self.radius && !near_box {
                return false;
            }
        }
        true
    }
}

/// Parse tags from a list of JSON objects (a `geocheck.toml`'s `tag` array
/// converted to JSON, or a kit data `"geocheck"` array).
pub fn parse(list: &Value) -> Result<Vec<Tag>, String> {
    let Some(arr) = list.as_array() else {
        return Err("tags must be a list".into());
    };
    let mut out = Vec::with_capacity(arr.len());
    for (n, t) in arr.iter().enumerate() {
        let ctx = |e: &str| format!("tag {}: {e}", n + 1);
        let reason = t
            .get("reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .ok_or_else(|| ctx("needs a reason"))?;
        let check = match t.get("check").and_then(Value::as_str) {
            None | Some("*") => None,
            Some(c) => Some(Check::parse(c).ok_or_else(|| ctx(&format!("unknown check {c}")))?),
        };
        let object = t.get("object").and_then(Value::as_str).map(str::to_string);
        let at = match t.get("at") {
            None => None,
            Some(v) => {
                let a: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                if a.len() != 3 {
                    return Err(ctx("at must be [x, y, z]"));
                }
                Some([a[0], a[1], a[2]])
            }
        };
        if object.is_none() && at.is_none() && check.is_none() {
            return Err(ctx("needs a check, an object or a place (at): it would hide everything"));
        }
        let radius = t.get("radius").and_then(Value::as_f64).unwrap_or(1.0);
        out.push(Tag {
            check,
            object,
            at,
            radius,
            reason: reason.to_string(),
        });
    }
    Ok(out)
}

/// Mark issues covered by a tag (first matching tag's reason).
pub fn apply(issues: &mut [Issue], tags: &[Tag]) {
    for i in issues {
        if let Some(t) = tags.iter().find(|t| t.matches(i)) {
            i.tagged = Some(t.reason.clone());
        }
    }
}

/// Case-insensitive glob with `*` and `?`.
pub fn glob(pattern: &str, s: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = s.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn globs() {
        assert!(glob("tree*", "Tree (sphere)"));
        assert!(glob("*rail*", "guard rail (cube)"));
        assert!(glob("a?c", "abc"));
        assert!(!glob("rock", "rocks"));
        assert!(glob("*", ""));
    }

    #[test]
    fn tags_need_a_reason_and_match_by_check_object_and_place() {
        assert!(parse(&json!([{"check": "floating", "object": "x"}])).is_err());
        assert!(parse(&json!([{"reason": "everything"}])).is_err());
        assert!(parse(&json!([{"check": "nope", "reason": "r"}])).is_err());
        let tags = parse(&json!([
            {"check": "floating", "object": "balloon*", "reason": "it flies"},
            {"check": "*", "at": [10.0, 0.0, 0.0], "radius": 2.0, "reason": "sunk rock"}
        ]))
        .unwrap();
        let mut a = Issue::new(Check::Floating, "floating", [0.0; 3]);
        a.objects = vec!["Balloon (sphere)".into()];
        let mut b = Issue::new(Check::Overlap, "overlap", [11.0, 0.5, 0.0]);
        b.objects = vec!["rock (sphere)".into(), "ground (cube)".into()];
        let mut c = Issue::new(Check::Overlap, "overlap", [20.0, 0.0, 0.0]);
        c.objects = vec!["balloon (sphere)".into()];
        let mut list = vec![a, b, c];
        apply(&mut list, &tags);
        assert_eq!(list[0].tagged.as_deref(), Some("it flies"));
        assert_eq!(list[1].tagged.as_deref(), Some("sunk rock"));
        assert_eq!(list[2].tagged, None);
    }
}
