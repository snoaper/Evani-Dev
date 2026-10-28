//! Issues and the report: every issue with its check, what it touches, where
//! it is, how bad it is and a suggested fix, grouped by type and object
//! class, as markdown (for people) and JSON (for tools and the critic).

use serde_json::{Value, json};

use crate::geom::{Aabb, V3};

/// The eight checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    Overlap,
    Floating,
    Gap,
    ZFight,
    Mesh,
    Moving,
    Collision,
    Camera,
}

impl Check {
    pub const ALL: [Check; 8] = [
        Check::Overlap,
        Check::Floating,
        Check::Gap,
        Check::ZFight,
        Check::Mesh,
        Check::Moving,
        Check::Collision,
        Check::Camera,
    ];

    /// Machine name (tags, JSON, the live command).
    pub fn name(self) -> &'static str {
        match self {
            Check::Overlap => "overlap",
            Check::Floating => "floating",
            Check::Gap => "gap",
            Check::ZFight => "zfight",
            Check::Mesh => "mesh",
            Check::Moving => "moving",
            Check::Collision => "collision",
            Check::Camera => "camera",
        }
    }

    /// Title in the report.
    pub fn title(self) -> &'static str {
        match self {
            Check::Overlap => "Static overlaps",
            Check::Floating => "Floating objects",
            Check::Gap => "Gaps and cracks",
            Check::ZFight => "Z-fighting",
            Check::Mesh => "Mesh validation",
            Check::Moving => "Moving clipping",
            Check::Collision => "Collision mismatch",
            Check::Camera => "Camera clipping",
        }
    }

    pub fn parse(s: &str) -> Option<Check> {
        Check::ALL.into_iter().find(|c| c.name() == s)
    }

    /// Highlight colour in the debug view and the crops (RGB 0..1).
    pub fn color(self) -> [f32; 3] {
        match self {
            Check::Overlap => [1.0, 0.1, 0.1],
            Check::Floating => [1.0, 0.9, 0.1],
            Check::Gap => [1.0, 0.0, 1.0],
            Check::ZFight => [0.1, 0.95, 1.0],
            Check::Mesh => [1.0, 0.55, 0.0],
            Check::Moving => [1.0, 0.4, 0.7],
            Check::Collision => [0.3, 0.5, 1.0],
            Check::Camera => [0.6, 1.0, 0.3],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Visible and certain: blocks hand-over unless tagged.
    Error,
    /// A risk (e.g. z-fighting only far away): still blocks unless tagged.
    Warning,
}

/// One problem found.
#[derive(Debug, Clone)]
pub struct Issue {
    pub check: Check,
    /// Sub-type ("seam overlap", "over nothing", "tunnelling", ...).
    pub kind: String,
    /// Object class labels involved ("rail (cube)", "tree (sphere)").
    pub objects: Vec<String>,
    /// Instance indices in the analysed frame (for the debug tint).
    pub items: Vec<usize>,
    /// World position.
    pub at: V3,
    /// World box of the problem (for the highlight capture).
    pub aabb: Option<Aabb>,
    /// How bad: volume, gap, distance, pixels.
    pub measure: f64,
    pub unit: String,
    pub detail: String,
    pub fix: String,
    pub severity: Severity,
    /// The tag's reason when an intentional-case tag covers it.
    pub tagged: Option<String>,
    /// Highlight crop (path relative to the report).
    pub crop: Option<String>,
    /// Pixel box in a capture (render-based checks): x, y, w, h.
    pub pixels: Option<[u32; 4]>,
    /// Simulation time or frame of a moving issue.
    pub time: Option<f64>,
    /// Check-specific measurements, machine-readable (`null` when none).
    pub data: Value,
}

impl Issue {
    pub fn new(check: Check, kind: &str, at: V3) -> Issue {
        Issue {
            check,
            kind: kind.into(),
            objects: Vec::new(),
            items: Vec::new(),
            at,
            aabb: None,
            measure: 0.0,
            unit: String::new(),
            detail: String::new(),
            fix: String::new(),
            severity: Severity::Error,
            tagged: None,
            crop: None,
            pixels: None,
            time: None,
            data: Value::Null,
        }
    }

    /// Object class of the issue: its labels, sorted, joined.
    pub fn class(&self) -> String {
        let mut o = self.objects.clone();
        o.sort();
        o.dedup();
        o.join(" <-> ")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "check": self.check.name(),
            "kind": self.kind,
            "objects": self.objects,
            "items": self.items,
            "at": round3(self.at),
            "aabb": self.aabb.map(|b| json!({"min": round3(b.min), "max": round3(b.max)})),
            "measure": (self.measure * 1e4).round() / 1e4,
            "unit": self.unit,
            "detail": self.detail,
            "fix": self.fix,
            "severity": match self.severity { Severity::Error => "error", Severity::Warning => "warning" },
            "tagged": self.tagged,
            "crop": self.crop,
            "pixels": self.pixels,
            "time": self.time,
            "data": self.data,
        })
    }
}

fn round3(p: V3) -> [f64; 3] {
    p.map(|x| (x * 1e3).round() / 1e3)
}

fn fmt_p(p: V3) -> String {
    format!("({:.2}, {:.2}, {:.2})", p[0], p[1], p[2])
}

/// Issues of the same check, kind and object class.
#[derive(Debug, Clone)]
pub struct Group<'a> {
    pub check: Check,
    pub kind: &'a str,
    pub class: String,
    pub issues: Vec<&'a Issue>,
}

/// Group issues by check, kind and class, worst first within a group, groups
/// ordered by check and then size.
pub fn group<'a>(issues: impl IntoIterator<Item = &'a Issue>) -> Vec<Group<'a>> {
    let mut groups: Vec<Group> = Vec::new();
    for i in issues {
        let class = i.class();
        match groups.iter_mut().find(|g| g.check == i.check && g.kind == i.kind && g.class == class) {
            Some(g) => g.issues.push(i),
            None => groups.push(Group {
                check: i.check,
                kind: &i.kind,
                class,
                issues: vec![i],
            }),
        }
    }
    for g in &mut groups {
        g.issues.sort_by(|a, b| b.measure.total_cmp(&a.measure));
    }
    groups.sort_by(|a, b| a.check.cmp(&b.check).then(b.issues.len().cmp(&a.issues.len())));
    groups
}

/// The whole report.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub app: String,
    pub issues: Vec<Issue>,
    /// Per check: what ran and how (one line each), or why it was skipped.
    pub ran: Vec<(Check, String)>,
    /// Extra facts (scene size, frames, captures).
    pub notes: Vec<String>,
}

impl Report {
    pub fn untagged(&self) -> usize {
        self.issues.iter().filter(|i| i.tagged.is_none()).count()
    }

    pub fn count(&self, c: Check, tagged: bool) -> usize {
        self.issues.iter().filter(|i| i.check == c && i.tagged.is_some() == tagged).count()
    }

    /// The worst untagged issues: errors first, then by measure relative to
    /// the check (one per group so the list is varied).
    pub fn worst(&self, n: usize) -> Vec<&Issue> {
        let mut heads: Vec<(usize, &Issue)> = group(self.issues.iter().filter(|i| i.tagged.is_none()))
            .into_iter()
            .map(|g| (g.issues.len(), g.issues[0]))
            .collect();
        heads.sort_by(|a, b| a.1.severity.cmp(&b.1.severity).then(b.0.cmp(&a.0)));
        heads.into_iter().take(n).map(|h| h.1).collect()
    }

    pub fn to_json(&self) -> Value {
        let counts: serde_json::Map<String, Value> = Check::ALL
            .iter()
            .map(|c| {
                (
                    c.name().to_string(),
                    json!({"untagged": self.count(*c, false), "tagged": self.count(*c, true)}),
                )
            })
            .collect();
        json!({
            "app": self.app,
            "untagged": self.untagged(),
            "total": self.issues.len(),
            "counts": counts,
            "ran": self.ran.iter().map(|(c, s)| json!({"check": c.name(), "how": s})).collect::<Vec<_>>(),
            "notes": self.notes,
            "issues": self.issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
        })
    }

    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        let un = self.untagged();
        s.push_str(&format!("# Geometry check: {}\n\n", self.app));
        s.push_str(&format!(
            "{} issue(s): {} untagged (block hand-over), {} tagged as intentional.\n\n",
            self.issues.len(),
            un,
            self.issues.len() - un
        ));
        s.push_str("| Check | Untagged | Tagged |\n|---|---:|---:|\n");
        for c in Check::ALL {
            s.push_str(&format!("| {} | {} | {} |\n", c.title(), self.count(c, false), self.count(c, true)));
        }
        if !self.ran.is_empty() {
            s.push_str("\n## How each check ran\n\n");
            for (c, how) in &self.ran {
                s.push_str(&format!("- **{}**: {}\n", c.title(), how));
            }
        }
        if !self.notes.is_empty() {
            s.push_str("\n## Notes\n\n");
            for n in &self.notes {
                s.push_str(&format!("- {n}\n"));
            }
        }
        let worst = self.worst(5);
        if !worst.is_empty() {
            s.push_str("\n## Worst issues\n\n");
            for (k, i) in worst.iter().enumerate() {
                s.push_str(&format!(
                    "{}. **{} / {}**: {} at {}, {:.3} {}. Fix: {}\n",
                    k + 1,
                    i.check.title(),
                    i.kind,
                    i.class(),
                    fmt_p(i.at),
                    i.measure,
                    i.unit,
                    i.fix
                ));
                if let Some(c) = &i.crop {
                    s.push_str(&format!("   ![]({c})\n"));
                }
            }
        }
        for tagged in [false, true] {
            let groups = group(self.issues.iter().filter(|i| i.tagged.is_some() == tagged));
            if groups.is_empty() {
                continue;
            }
            s.push_str(if tagged { "\n## Tagged (intentional, skipped)\n" } else { "\n## Issues by type\n" });
            let mut last: Option<Check> = None;
            for g in groups {
                if last != Some(g.check) {
                    s.push_str(&format!("\n### {}\n", g.check.title()));
                    last = Some(g.check);
                }
                let head = g.issues[0];
                s.push_str(&format!(
                    "\n**{}: {}** ({} found). Fix: {}\n\n",
                    g.kind,
                    if g.class.is_empty() { "-" } else { &g.class },
                    g.issues.len(),
                    head.fix
                ));
                for i in g.issues.iter().take(12) {
                    s.push_str(&format!("- {} {:.3} {}", fmt_p(i.at), i.measure, i.unit));
                    if let Some(t) = i.time {
                        s.push_str(&format!(" at t={t:.2}s"));
                    }
                    if !i.detail.is_empty() {
                        s.push_str(&format!(": {}", i.detail));
                    }
                    if let Some(r) = &i.tagged {
                        s.push_str(&format!(" (tag: {r})"));
                    }
                    if let Some(c) = &i.crop {
                        s.push_str(&format!(" [capture]({c})"));
                    }
                    s.push('\n');
                }
                if g.issues.len() > 12 {
                    s.push_str(&format!("- ... and {} more (see report.json)\n", g.issues.len() - 12));
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(c: Check, kind: &str, obj: &str, m: f64) -> Issue {
        let mut i = Issue::new(c, kind, [1.0, 2.0, 3.0]);
        i.objects = vec![obj.into()];
        i.measure = m;
        i.unit = "m".into();
        i.fix = "move it".into();
        i
    }

    #[test]
    fn groups_and_counts() {
        let mut r = Report {
            app: "t".into(),
            ..Default::default()
        };
        r.issues.push(issue(Check::Overlap, "overlap", "a", 0.1));
        r.issues.push(issue(Check::Overlap, "overlap", "a", 0.3));
        r.issues.push(issue(Check::Floating, "floating", "b", 0.2));
        let mut t = issue(Check::Floating, "floating", "balloon", 3.0);
        t.tagged = Some("it flies".into());
        r.issues.push(t);
        assert_eq!(r.untagged(), 3);
        assert_eq!(r.count(Check::Floating, true), 1);
        let g = group(&r.issues);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].issues.len(), 2);
        assert_eq!(g[0].issues[0].measure, 0.3);
        let w = r.worst(5);
        assert_eq!(w.len(), 2);
        let md = r.to_markdown();
        assert!(md.contains("Static overlaps"));
        assert!(md.contains("it flies"));
        let j = r.to_json();
        assert_eq!(j["untagged"], 3);
        assert_eq!(j["counts"]["floating"]["tagged"], 1);
    }

    #[test]
    fn check_names_round_trip() {
        for c in Check::ALL {
            assert_eq!(Check::parse(c.name()), Some(c));
        }
    }
}
