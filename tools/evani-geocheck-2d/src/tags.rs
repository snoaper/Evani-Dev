//! Intentional cases. A tag names a check (its slug such as `seams`, its
//! number `3`, or one issue type such as `seam`) and either an object
//! (exact name, or a prefix ending in `*`) or a place (`at` in tiles,
//! `radius` in tiles, default 1). `reason` is required. Tagged issues are
//! still reported, as skipped, and never fail the run.
//!
//! Tags come from the scene's `"geocheck"` array (a kit copies it from its
//! level data) and from a `geocheck.toml` next to the scene:
//!
//! ```toml
//! [[tag]]
//! check = "seams"
//! at = [108, 7]
//! radius = 8
//! reason = "the stone wall is the castle's outer keep, drawn without grass on purpose"
//! ```

use crate::Issue;
use crate::geom::dist_point_rect;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Tag {
    pub check: String,
    pub object: Option<String>,
    pub at: Option<[f32; 2]>,
    pub radius: Option<f32>,
    pub reason: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TomlFile {
    tag: Vec<Tag>,
}

/// Parse a `geocheck.toml`.
pub fn parse_toml(src: &str) -> Result<Vec<Tag>, String> {
    let f: TomlFile = toml::from_str(src).map_err(|e| format!("geocheck.toml: {e}"))?;
    Ok(f.tag)
}

/// Every tag needs a check, a reason, and an object or a place.
pub fn validate(tags: &[Tag]) -> Result<(), String> {
    for (i, t) in tags.iter().enumerate() {
        if t.check.trim().is_empty() {
            return Err(format!("geocheck tag {i}: `check` is required"));
        }
        if t.reason.trim().is_empty() {
            return Err(format!("geocheck tag {i} ({}): `reason` is required", t.check));
        }
        if t.object.is_none() && t.at.is_none() {
            return Err(format!("geocheck tag {i} ({}): give `object` or `at`", t.check));
        }
    }
    Ok(())
}

fn name_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

/// Does `tag` cover `issue`?
pub fn matches(tag: &Tag, issue: &Issue, tile_size: f32) -> bool {
    let c = tag.check.trim();
    let check_ok = c == issue.check.slug() || c == issue.check.number().to_string() || c == issue.kind;
    if !check_ok {
        return false;
    }
    if let Some(o) = &tag.object {
        return issue.object.as_deref().is_some_and(|n| name_matches(o, n));
    }
    if let Some(at) = tag.at {
        let p = [(at[0] + 0.5) * tile_size, (at[1] + 0.5) * tile_size];
        let r = tag.radius.unwrap_or(1.0).max(0.0) * tile_size;
        return dist_point_rect(p, issue.px) <= r;
    }
    false
}

/// Mark tagged issues with the first matching tag's reason.
pub fn apply(issues: &mut [Issue], tags: &[Tag], tile_size: f32) {
    for i in issues.iter_mut() {
        if let Some(t) = tags.iter().find(|t| matches(t, i, tile_size)) {
            i.tagged = Some(t.reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Check;

    fn issue() -> Issue {
        Issue::new(Check::Seams, "seam", [160.0, 112.0, 16.0, 16.0]).object("seam earth|stone")
    }

    #[test]
    fn tags_match_by_place_object_and_check() {
        let at = Tag { check: "seams".into(), at: Some([10.0, 7.0]), radius: Some(1.0), reason: "keep".into(), ..Tag::default() };
        assert!(matches(&at, &issue(), 16.0));
        let far = Tag { at: Some([30.0, 7.0]), ..at.clone() };
        assert!(!matches(&far, &issue(), 16.0));
        let wrong_check = Tag { check: "pixels".into(), ..at.clone() };
        assert!(!matches(&wrong_check, &issue(), 16.0));
        let by_type = Tag { check: "seam".into(), object: Some("seam earth*".into()), at: None, ..at.clone() };
        assert!(matches(&by_type, &issue(), 16.0));
        let by_number = Tag { check: "3".into(), ..at };
        assert!(matches(&by_number, &issue(), 16.0));
    }

    #[test]
    fn a_reason_is_required() {
        let t = Tag { check: "seams".into(), at: Some([1.0, 1.0]), ..Tag::default() };
        assert!(validate(&[t]).is_err());
        let toml = "[[tag]]\ncheck = \"seams\"\nat = [108, 7]\nradius = 8\nreason = \"castle keep\"\n";
        let tags = parse_toml(toml).unwrap();
        assert_eq!(tags.len(), 1);
        assert!(validate(&tags).is_ok());
        let mut v = vec![issue()];
        apply(&mut v, &[Tag { check: "seams".into(), object: Some("seam earth|stone".into()), reason: "ok".into(), ..Tag::default() }], 16.0);
        assert_eq!(v[0].tagged.as_deref(), Some("ok"));
    }
}
