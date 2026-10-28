//! Reading the evidence other tools leave: geocheck reports
//! (`.vibedeck/geocheck/<app>/report.json`) and review cards
//! (`.vibedeck/review/<app>/card.json`). xtask's review uses the same reader.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GeoIssue {
    /// The check's slug (`floating`, `camera`, `bot`, `overlaps`, ..).
    pub check: String,
    pub tagged: bool,
    pub what: String,
    /// The report's own "how to fix it", when it has one.
    pub fix: String,
}

/// Every issue of a geocheck report: the top-level `issues` (evani-geocheck)
/// and each group's `issues` (evani-geocheck-2d). An issue is tagged when
/// `tagged` is true or a non-empty reason.
pub fn geo_issues(report: &Value) -> Vec<GeoIssue> {
    let one = |x: &Value, check: &str| {
        let tagged = match x.get("tagged") {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => !s.trim().is_empty(),
            _ => false,
        };
        let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let what = [s("message"), s("detail"), s("kind"), s("type"), s("object")].into_iter().find(|t| !t.is_empty()).unwrap_or_default();
        let c = s("check");
        GeoIssue { check: if c.is_empty() { check.to_string() } else { c }, tagged, what, fix: s("fix") }
    };
    let mut out: Vec<GeoIssue> = report.get("issues").and_then(Value::as_array).map(|a| a.iter().map(|x| one(x, "")).collect()).unwrap_or_default();
    for g in report.get("groups").and_then(Value::as_array).into_iter().flatten() {
        let check = g.get("check").and_then(Value::as_str).unwrap_or("");
        out.extend(g.get("issues").and_then(Value::as_array).into_iter().flatten().map(|x| one(x, check)));
    }
    out
}

/// A partial geocheck report (`--sample N`: `"partial": true`), in words
/// ("50 of 700 cameras"); `None` for a full report.
pub fn partial(report: &Value) -> Option<String> {
    if report.get("partial") != Some(&Value::Bool(true)) {
        return None;
    }
    let n = |k: &str| report.get(k).and_then(Value::as_u64);
    Some(match (n("cameras"), n("cameras_total")) {
        (Some(c), Some(t)) => format!("{c} of {t} cameras"),
        _ => "a sample".to_string(),
    })
}

/// One review card checklist line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLine {
    pub n: u64,
    /// `pass`, `fail`, `manual`, `not_run`, `flag` or `not_applicable`.
    pub status: String,
    pub text: String,
    pub evidence: String,
}

pub fn card_lines(card: &Value) -> Vec<CardLine> {
    card.get("checklist")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|x| {
            let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            CardLine { n: x.get("n").and_then(Value::as_u64).unwrap_or(0), status: s("status"), text: s("text"), evidence: s("evidence") }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_both_report_shapes_and_the_card() {
        let r3 = json!({"issues":[{"check":"overlap","detail":"a and b share 2 m3","fix":"trim them","tagged":"on purpose"},{"check":"floating","kind":"floating","tagged":false}]});
        let i = geo_issues(&r3);
        assert_eq!(i.len(), 2);
        assert!(i[0].tagged && i[0].fix == "trim them");
        assert!(!i[1].tagged && i[1].what == "floating");
        let r2 = json!({"groups":[{"check":"overlaps","issues":[{"message":"coin 5 overlaps tree 4","fix":"Move coin 5 left 23 px.","tagged":false}]}]});
        let i = geo_issues(&r2);
        assert_eq!(i[0].check, "overlaps");
        assert_eq!(i[0].fix, "Move coin 5 left 23 px.");
        let card = json!({"checklist":[{"n":1,"status":"pass","text":"a","evidence":"ok"},{"n":2,"status":"fail","text":"b","evidence":"bad"}]});
        let l = card_lines(&card);
        assert_eq!(l[1], CardLine { n: 2, status: "fail".into(), text: "b".into(), evidence: "bad".into() });
    }
}
