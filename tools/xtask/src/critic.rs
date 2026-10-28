//! `cargo xtask critic <app-or-kit>`: the independent critic's packet
//! (docs/KIT_PROCESS.md step 8).
//!
//! It writes `.vibedeck/review/<app>/critic/packet.md`: every capture, page
//! screenshot, video frame and geometry debug view of the last review, the
//! target pack, the genre playbook, the quality bar (done-when items 8 to 10),
//! the rubric with its pass rule, the reply schema and the earlier rounds'
//! must-fix lists. The orchestrator then starts the `kit-critic` agent
//! (.claude/agents/kit-critic.md) on that packet, with a fresh context that
//! did not build the kit; it writes `critic-round-N.json` and `.md` next to
//! the packet. `cargo xtask review` reads the newest round for the card.
//!
//! At most three rounds: a fourth is refused and goes to Chris. The verdict
//! is recomputed from the scores here (every applicable item 2 or more, mean
//! 2.3 or more, no must-fix item) and the stricter of that and the critic's
//! own verdict counts.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::checks::Verdict;
use crate::{load_app, workspace_root};

pub const MAX_ROUNDS: u32 = 3;
pub const PASS_MIN_ITEM: u8 = 2;
pub const PASS_MEAN: f64 = 2.3;

pub fn usage() -> &'static str {
    "  cargo xtask critic <app-or-kit> [--extra-round]    write the critic packet for the kit-critic agent (at most 3 rounds; --extra-round: one more after an escalation decided another fix round)"
}

/// One rubric item: scored 0 to 3 with evidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RubricItem {
    pub id: &'static str,
    pub title: &'static str,
    pub what: &'static str,
    /// The item applies from this stage on (STAGES.md).
    pub from_stage: u32,
}

pub const RUBRIC: [RubricItem; 11] = [
    RubricItem { id: "R1", title: "Moments match the target", what: "Each signature moment in target/moments.md is captured and shows what its \"In frame\" and \"Pass\" lines ask for, on Low and Ultra.", from_stage: 0 },
    RubricItem { id: "R2", title: "Look bible", what: "Palette (the hex colours), shape language and sky/backdrop match target/look-bible.md; nothing off-palette or flat where the bible asks for shape.", from_stage: 0 },
    RubricItem { id: "R3", title: "Props and middle distance", what: "The bible's theme props are present and placed as it says; the middle distance is filled (no empty plains between the play area and the backdrop).", from_stage: 0 },
    RubricItem { id: "R4", title: "Grounding and lighting", what: "Every object sits on the ground with a contact shadow; the light direction reads the same in every shot; shadows are never pure black.", from_stage: 0 },
    RubricItem { id: "R5", title: "Look effects", what: "The look's signature effects (outlines, bands, palette lock, bloom ..) show and help reading the game; no effect hides the action.", from_stage: 0 },
    RubricItem { id: "R6", title: "HUD at 1080p and on a phone", what: "Every HUD element of target/hud-layout.md is visible, readable and placed as specified at 1920x1080 and on the phone-size page shots, including during a countdown or popup.", from_stage: 0 },
    RubricItem { id: "R7", title: "Placeholder bar", what: "Done-when item 9: procedural models have distinct, bevelled silhouettes in the look's proportions and animate the genre's key actions.", from_stage: 0 },
    RubricItem { id: "R8", title: "Genre playbook", what: "Every item of docs/playbooks/<genre>.md that applies is met (score the worst missed item).", from_stage: 0 },
    RubricItem { id: "R9", title: "Geometry debug views", what: "The geocheck's debug-view captures show no floating, sunk, overlapping or cut-off geometry that is not tagged as intended.", from_stage: 0 },
    RubricItem { id: "R10", title: "Hero assets and close-up", what: "Done-when item 10: hero assets are detailed models and hold up in the close-up capture.", from_stage: 3 },
    RubricItem { id: "R11", title: "Video", what: "The run video frames show motion that reads (speed, feedback, juice), no flicker, popping or camera jumps.", from_stage: 0 },
];

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub id: String,
    pub score: u8,
    pub applicable: bool,
    pub evidence: String,
}

/// `critic-round-N.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct CriticReport {
    pub kit: String,
    pub round: u32,
    pub scores: Vec<Score>,
    pub must_fix: Vec<String>,
    /// The critic's own verdict: `pass` or `fail`.
    pub verdict: String,
    pub summary: String,
}

pub fn parse_report(v: &Value) -> Result<CriticReport, String> {
    let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let scores = v
        .get("scores")
        .and_then(Value::as_array)
        .ok_or("no `scores` list")?
        .iter()
        .map(|x| -> Result<Score, String> {
            let id = s(x, "id");
            let applicable = x.get("applicable").and_then(Value::as_bool).unwrap_or(true);
            let score = match x.get("score").and_then(Value::as_u64) {
                Some(n @ 0..=3) => n as u8,
                Some(n) => return Err(format!("{id}: score {n} is not 0 to 3")),
                None if !applicable => 0,
                None => return Err(format!("{id}: no score")),
            };
            Ok(Score { id, score, applicable, evidence: s(x, "evidence") })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let must_fix = v
        .get("must_fix")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|m| m.as_str().map(str::to_string).or_else(|| m.get("what").and_then(Value::as_str).map(str::to_string))).collect())
        .unwrap_or_default();
    Ok(CriticReport {
        kit: s(v, "kit"),
        round: v.get("round").and_then(Value::as_u64).unwrap_or(0) as u32,
        scores,
        must_fix,
        verdict: s(v, "verdict").to_lowercase(),
        summary: s(v, "summary"),
    })
}

/// The pass rule applied to a report, at a stage.
#[derive(Debug, Clone, PartialEq)]
pub struct Judgement {
    pub pass: bool,
    pub mean: f64,
    pub reasons: Vec<String>,
}

pub fn judge(r: &CriticReport, stage: u32) -> Judgement {
    let mut reasons = Vec::new();
    let mut applicable = Vec::new();
    for item in RUBRIC {
        let applies = stage >= item.from_stage;
        match r.scores.iter().find(|s| s.id.eq_ignore_ascii_case(item.id)) {
            None if applies => reasons.push(format!("{} not scored", item.id)),
            None => {}
            Some(s) if applies && !s.applicable => reasons.push(format!("{} marked not applicable but it applies", item.id)),
            Some(s) if applies => {
                if s.score < PASS_MIN_ITEM {
                    reasons.push(format!("{} {} scored {}", item.id, item.title, s.score));
                }
                applicable.push(f64::from(s.score));
            }
            Some(_) => {}
        }
    }
    let mean = if applicable.is_empty() { 0.0 } else { applicable.iter().sum::<f64>() / applicable.len() as f64 };
    if mean < PASS_MEAN {
        reasons.push(format!("mean {mean:.2} under {PASS_MEAN}"));
    }
    if !r.must_fix.is_empty() {
        reasons.push(format!("{} must-fix items", r.must_fix.len()));
    }
    if r.verdict != "pass" {
        reasons.push(format!("the critic's verdict is {}", if r.verdict.is_empty() { "missing" } else { &r.verdict }));
    }
    Judgement { pass: reasons.is_empty(), mean, reasons }
}

/// The round numbers of the `critic-round-N.json` files in a folder, sorted.
pub fn rounds_in(names: &[String]) -> Vec<u32> {
    let mut v: Vec<u32> = names.iter().filter_map(|n| n.strip_prefix("critic-round-")?.strip_suffix(".json")?.parse().ok()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The next round to run, or why none may run. `extra`: the escalation
/// after [`MAX_ROUNDS`] decided on another fix round (`--extra-round`).
pub fn next_round(rounds: &[u32], latest_passed: bool, extra: bool) -> Result<u32, String> {
    let last = rounds.last().copied().unwrap_or(0);
    if latest_passed {
        return Err(format!("The critic passed the kit in round {last}; no new round is needed (rerun only after changes, by deleting the passing round's files)."));
    }
    if last >= MAX_ROUNDS && !extra {
        return Err(format!("{MAX_ROUNDS} critic rounds have run and the kit has not passed: escalate to Chris with the round reports (docs/KIT_PROCESS.md step 8)."));
    }
    Ok(last + 1)
}

/// The card's critic line: the newest round's judgement.
pub fn critic_verdict(latest: Option<&Result<CriticReport, String>>, rounds: usize, stage: u32) -> Verdict {
    let Some(r) = latest else {
        return Verdict::not_run("No critic round yet: `cargo xtask critic <kit>`, then the orchestrator starts the kit-critic agent (docs/KIT_PROCESS.md step 8).");
    };
    let r = match r {
        Ok(r) => r,
        Err(e) => return Verdict::fail(format!("The newest critic report cannot be read: {e}.")),
    };
    let j = judge(r, stage);
    let head = format!("Round {} of {MAX_ROUNDS}, mean {:.2}", r.round.max(rounds as u32), j.mean);
    if j.pass {
        return Verdict::pass(format!("{head}, every item {PASS_MIN_ITEM} or more, no must-fix items: the critic passed it."));
    }
    let must = if r.must_fix.is_empty() { String::new() } else { format!(" Must fix: {}.", r.must_fix.join("; ")) };
    let next = if rounds as u32 >= MAX_ROUNDS {
        " Three rounds used: escalate to Chris.".to_string()
    } else {
        format!(" Round {} of {MAX_ROUNDS} is still to run: it judges the builder's fixes for this list.", rounds as u32 + 1)
    };
    Verdict::fail(format!("{head}: {}.{must}{next}", j.reasons.join(", ")))
}

/// Everything the packet lists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PacketInputs {
    pub app: String,
    pub kit: String,
    pub look: String,
    pub round: u32,
    pub stage: u32,
    /// Paths relative to the repo root.
    pub images: Vec<String>,
    pub video: Option<String>,
    pub video_frames: Vec<String>,
    pub geocheck_images: Vec<String>,
    pub target_files: Vec<String>,
    pub playbook: Option<String>,
    pub look_readme: Option<String>,
    pub spec: Option<String>,
    pub card: Option<String>,
    /// Done-when items 8 to 10, as written.
    pub quality_bar: Vec<(u32, String)>,
    /// Earlier rounds: (round, must-fix list).
    pub previous: Vec<(u32, Vec<String>)>,
    pub out_dir: String,
}

fn list(s: &mut String, items: &[String], none: &str) {
    if items.is_empty() {
        let _ = writeln!(s, "- (none: {none})");
    }
    for i in items {
        let _ = writeln!(s, "- `{i}`");
    }
}

pub fn packet_md(p: &PacketInputs) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Critic packet: {} (kit {}, look {}), round {} of {MAX_ROUNDS}", p.app, p.kit, p.look, p.round);
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "You are the independent critic (`.claude/agents/kit-critic.md`). You did not build this kit. Read every file below, score each rubric item 0 to 3 with evidence (the file and what you see in it), list the must-fix items, and write `{}/critic-round-{}.json` and `{}/critic-round-{}.md`. Change nothing else.",
        p.out_dir, p.round, p.out_dir, p.round
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Pictures (open every one)");
    let _ = writeln!(s);
    let _ = writeln!(s, "Showcase captures (`shot-<moment>-<tier>.png`, 1920x1080 or integer-scaled, labelled with tier and engine commit), the review's moment and bench captures, page screenshots (the whole page with its HUD, a DOM HUD over the canvas or one drawn inside it, desktop and phone sizes) and the world-edge clear-colour renders (`geo-*`, black and white background):");
    let _ = writeln!(s);
    list(&mut s, &p.images, "run `cargo xtask review` first");
    let _ = writeln!(s);
    let _ = writeln!(s, "Run video{}; its frames in order:", p.video.as_ref().map_or(" (none)".to_string(), |v| format!(" `{v}`")));
    let _ = writeln!(s);
    list(&mut s, &p.video_frames, "no video was recorded");
    let _ = writeln!(s);
    let _ = writeln!(s, "Geometry debug views (the geocheck's captures and crops):");
    let _ = writeln!(s);
    list(&mut s, &p.geocheck_images, "no geocheck report for this game yet; score R9 from the captures and say so");
    let _ = writeln!(s);
    let _ = writeln!(s, "## What it should look like");
    let _ = writeln!(s);
    let _ = writeln!(s, "Target pack:");
    let _ = writeln!(s);
    list(&mut s, &p.target_files, "the kit has no target pack; score R1 to R3 against the SPEC and the look README, and put \"write the target pack\" first in must_fix");
    let _ = writeln!(s);
    for (label, f) in [("Genre playbook", &p.playbook), ("Look README", &p.look_readme), ("Kit SPEC", &p.spec), ("Review card (automatic checks)", &p.card)] {
        let _ = writeln!(s, "- {label}: {}", f.as_ref().map_or("none".to_string(), |x| format!("`{x}`")));
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "Quality bar (docs/KIT_PROCESS.md, done when):");
    let _ = writeln!(s);
    for (n, t) in &p.quality_bar {
        let _ = writeln!(s, "{n}. {t}");
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "## Rubric (current stage {})", p.stage);
    let _ = writeln!(s);
    let _ = writeln!(s, "Scores: 0 missing or broken; 1 present but clearly below the target; 2 meets the target with small gaps; 3 matches the target. Evidence names the picture and what is in it.");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Id | Item | What to judge | Applies |");
    let _ = writeln!(s, "|---|---|---|---|");
    for r in RUBRIC {
        let applies = if p.stage >= r.from_stage { "yes".to_string() } else { format!("no (from Stage {}): score it `applicable: false`", r.from_stage) };
        let _ = writeln!(s, "| {} | {} | {} | {applies} |", r.id, r.title, r.what);
    }
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Pass rule: every applicable item {PASS_MIN_ITEM} or more, the mean of the applicable items {PASS_MEAN} or more, and no must-fix items. `cargo xtask review` recomputes this from your scores and the stricter of its verdict and yours counts."
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "## Reply: `critic-round-{}.json`", p.round);
    let _ = writeln!(s);
    let _ = writeln!(s, "```json");
    let _ = writeln!(
        s,
        "{{\"kit\": \"{}\", \"round\": {}, \"scores\": [{{\"id\": \"R1\", \"score\": 2, \"applicable\": true, \"evidence\": \"shot-drift-ultra.png: ...\"}}], \"must_fix\": [\"...\"], \"verdict\": \"pass or fail\", \"summary\": \"two or three sentences\"}}",
        p.kit, p.round
    );
    let _ = writeln!(s, "```");
    let _ = writeln!(s);
    let _ = writeln!(s, "One entry per rubric item R1 to R11. A must-fix item is one concrete change with the picture that shows the problem. The `.md` holds the same in prose, for Chris.");
    let _ = writeln!(s);
    let _ = writeln!(s, "## Earlier rounds");
    let _ = writeln!(s);
    if p.previous.is_empty() {
        let _ = writeln!(s, "None: this is the first round.");
    }
    for (n, fixes) in &p.previous {
        let _ = writeln!(s, "Round {n} must-fix (check each is fixed now; say which are not):");
        let _ = writeln!(s);
        if fixes.is_empty() {
            let _ = writeln!(s, "- (none listed)");
        }
        for f in fixes {
            let _ = writeln!(s, "- {f}");
        }
        let _ = writeln!(s);
    }
    s
}

/// "Current stage: N" in STAGES.md.
pub fn current_stage(stages_md: &str) -> u32 {
    stages_md
        .lines()
        .find_map(|l| l.trim().strip_prefix("Current stage:"))
        .and_then(|r| r.trim().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
        .unwrap_or(0)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn images_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for p in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
        if p.is_dir() {
            images_under(&p, out);
        } else if p.extension().is_some_and(|x| x == "png" || x == "jpg" || x == "webp") {
            out.push(p);
        }
    }
}

/// The critic rounds of an app: `(round numbers, newest report)`.
pub fn read_rounds(dir: &Path) -> (Vec<u32>, Option<Result<CriticReport, String>>) {
    let names: Vec<String> = files_in(dir).iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    let rounds = rounds_in(&names);
    let latest = rounds.last().map(|n| {
        let f = dir.join(format!("critic-round-{n}.json"));
        std::fs::read_to_string(&f)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
            .and_then(|v| parse_report(&v))
            .map(|mut r| {
                if r.round == 0 {
                    r.round = *n;
                }
                r
            })
    });
    (rounds, latest)
}

pub fn cmd_critic(args: &[String]) -> Result<(), String> {
    let (name, extra) = match args {
        [n] if !n.starts_with("--") => (n.clone(), false),
        [n, f] if !n.starts_with("--") && f == "--extra-round" => (n.clone(), true),
        _ => return Err(format!("critic takes one app or kit name (and optionally --extra-round)\n{}", usage())),
    };
    let root = workspace_root();
    let app_name = crate::review::resolve_app(&root, &name)?;
    let app = load_app(&app_name)?;
    let place = crate::review::Place::in_engine(&root, &app_name, &app.manifest.kit);
    critic_for(&root, &app, &place, extra)
}

/// `cargo xtask critic --app-dir <folder> [--extra-round]`: the packet for a
/// game outside the engine, from its own review and geocheck folders and its
/// target pack (as `review --app-dir` finds them).
pub(crate) fn cmd_critic_app(app: &crate::AppDir, args: &[String]) -> Result<(), String> {
    let extra = match args {
        [] => false,
        [f] if f == "--extra-round" => true,
        _ => return Err(format!("critic --app-dir takes only --extra-round\n{}", usage())),
    };
    let place = crate::review::Place::outside(app)?;
    critic_for(&workspace_root(), app, &place, extra)
}

fn critic_for(root: &Path, app: &crate::AppDir, place: &crate::review::Place, extra: bool) -> Result<(), String> {
    let root = root.to_path_buf();
    let app_name = app.name.clone();
    let m = &app.manifest;
    let review = place.home.join(".vibedeck").join("review").join(&app_name);
    let dir = review.join("critic");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stage = std::fs::read_to_string(root.join("STAGES.md")).map(|t| current_stage(&t)).unwrap_or(0);

    let (rounds, latest) = read_rounds(&dir);
    let latest_passed = matches!(&latest, Some(Ok(r)) if judge(r, stage).pass);
    let round = next_round(&rounds, latest_passed, extra)?;
    let previous = rounds
        .iter()
        .map(|n| {
            let r = std::fs::read_to_string(dir.join(format!("critic-round-{n}.json"))).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| parse_report(&v).ok());
            (*n, r.map(|r| r.must_fix).unwrap_or_default())
        })
        .collect();

    let review_files = files_in(&review);
    let name_of = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let images: Vec<String> = review_files
        .iter()
        .filter(|p| {
            let n = name_of(p);
            n.ends_with(".png") && ["shot-", "moment-", "page-", "geo-", "capture-"].iter().any(|k| n.starts_with(k))
        })
        .map(|p| rel(&root, p))
        .collect();
    let video_frames: Vec<String> = review_files.iter().filter(|p| name_of(p).starts_with("video-frame-")).map(|p| rel(&root, p)).collect();
    let video = review_files.iter().find(|p| name_of(p).starts_with("video.")).map(|p| rel(&root, p));
    let mut geo = Vec::new();
    images_under(&place.home.join(".vibedeck").join("geocheck").join(&app_name), &mut geo);
    geo.sort();
    let kit_dir = place.kit_pack.clone();
    let target_files = files_in(&kit_dir.join("target")).iter().map(|p| rel(&root, p)).collect();
    let exists = |p: PathBuf| p.exists().then(|| rel(&root, &p));
    let process = std::fs::read_to_string(root.join("docs").join("KIT_PROCESS.md")).unwrap_or_default();
    let quality_bar = crate::review::parse_done_when(&process).into_iter().filter(|(n, _)| (8..=10).contains(n)).collect();

    let p = PacketInputs {
        app: app_name.clone(),
        kit: m.kit.clone(),
        look: m.look.clone(),
        round,
        stage,
        images,
        video,
        video_frames,
        geocheck_images: geo.iter().map(|p| rel(&root, p)).collect(),
        target_files,
        playbook: exists(root.join("docs").join("playbooks").join(format!("{}.md", m.kit))),
        look_readme: exists(root.join("looks").join(format!("evani-look-{}", m.look)).join("README.md")),
        spec: exists(kit_dir.join("SPEC.md")),
        card: exists(review.join("card.md")),
        quality_bar,
        previous,
        out_dir: rel(&root, &dir),
    };
    let md = packet_md(&p);
    let out = dir.join("packet.md");
    std::fs::write(&out, md).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("Critic packet for {app_name}, round {round} of {MAX_ROUNDS}: {}", out.display());
    println!(
        "  {} pictures, {} video frames, {} geometry views, {} target-pack files, playbook {}",
        p.images.len(),
        p.video_frames.len(),
        p.geocheck_images.len(),
        p.target_files.len(),
        p.playbook.as_deref().unwrap_or("none")
    );
    println!("  Next: the orchestrator starts the kit-critic agent on this packet; it writes {}/critic-round-{round}.json and .md.", p.out_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(scores: &[(&str, u64)], must: &[&str], verdict: &str) -> Value {
        json!({
            "kit": "kart-racer", "round": 1,
            "scores": scores.iter().map(|(id, s)| json!({"id": id, "score": s, "applicable": true, "evidence": "shot-drift-low.png"})).collect::<Vec<_>>(),
            "must_fix": must, "verdict": verdict, "summary": "ok",
        })
    }

    fn all(score: u64) -> Vec<(&'static str, u64)> {
        RUBRIC.iter().filter(|r| r.from_stage == 0).map(|r| (r.id, score)).collect()
    }

    #[test]
    fn parse_checks_scores() {
        let r = parse_report(&report(&all(3), &[], "PASS")).unwrap();
        assert_eq!((r.round, r.scores.len(), r.verdict.as_str()), (1, 10, "pass"));
        assert!(parse_report(&report(&[("R1", 4)], &[], "pass")).is_err());
        assert!(parse_report(&json!({"kit": "k"})).is_err());
        let r = parse_report(&json!({"scores": [{"id": "R10", "applicable": false}], "must_fix": [{"what": "lift the coins"}], "verdict": "fail"})).unwrap();
        assert_eq!((r.scores[0].applicable, r.must_fix[0].as_str()), (false, "lift the coins"));
    }

    #[test]
    fn the_pass_rule_and_the_stricter_verdict() {
        let ok = parse_report(&report(&all(3), &[], "pass")).unwrap();
        let j = judge(&ok, 2);
        assert!(j.pass && (j.mean - 3.0).abs() < 1e-9, "{j:?}");
        // Mean 2.2: every item 2, two items 3 would be 2.2; under 2.3 fails.
        let mut s = all(2);
        s[0].1 = 3;
        s[1].1 = 3;
        let j = judge(&parse_report(&report(&s, &[], "pass")).unwrap(), 2);
        assert!(!j.pass && j.reasons.iter().any(|r| r.contains("mean 2.20")), "{j:?}");
        // One item under 2 fails even with a high mean.
        let mut s = all(3);
        s[3].1 = 1;
        assert!(judge(&parse_report(&report(&s, &[], "pass")).unwrap(), 2).reasons.iter().any(|r| r.contains("R4")));
        // A must-fix item, or the critic's own fail, fails.
        assert!(!judge(&parse_report(&report(&all(3), &["lift the coins"], "pass")).unwrap(), 2).pass);
        assert!(!judge(&parse_report(&report(&all(3), &[], "fail")).unwrap(), 2).pass);
        // A missing item fails; R10 only counts from Stage 3.
        assert!(!judge(&parse_report(&report(&all(3)[1..], &[], "pass")).unwrap(), 2).pass);
        assert!(judge(&ok, 3).reasons.iter().any(|r| r.contains("R10 not scored")));
    }

    #[test]
    fn rounds_stop_after_three() {
        let names: Vec<String> = ["critic-round-2.json", "critic-round-1.json", "critic-round-1.md", "packet.md", "critic-round-x.json"].iter().map(|s| s.to_string()).collect();
        assert_eq!(rounds_in(&names), [1, 2]);
        assert_eq!(next_round(&[], false, false), Ok(1));
        assert_eq!(next_round(&[1, 2], false, false), Ok(3));
        assert!(next_round(&[1, 2, 3], false, false).unwrap_err().contains("escalate to Chris"));
        assert!(next_round(&[1], true, false).unwrap_err().contains("passed"));
        // An escalation that decided on another fix round.
        assert_eq!(next_round(&[1, 2, 3], false, true), Ok(4));
        assert!(next_round(&[1, 2, 3, 4], true, true).unwrap_err().contains("passed"));
    }

    #[test]
    fn card_verdict_shows_round_mean_and_must_fix() {
        assert_eq!(critic_verdict(None, 0, 2).label(), "not run");
        let ok: Result<CriticReport, String> = parse_report(&report(&all(3), &[], "pass"));
        let v = critic_verdict(Some(&ok), 1, 2);
        assert!(v.pass && v.evidence.starts_with("Round 1 of 3, mean 3.00"), "{}", v.evidence);
        let mut bad = parse_report(&report(&all(2), &["the boost flame hides the kart (shot-boost-ultra.png)"], "fail")).unwrap();
        bad.round = 3;
        let v = critic_verdict(Some(&Ok(bad)), 3, 2);
        assert!(!v.pass && v.evidence.contains("boost flame") && v.evidence.contains("escalate to Chris"), "{}", v.evidence);
        assert!(!v.evidence.contains("still to run"), "{}", v.evidence);
        let mut two = parse_report(&report(&all(2), &["the podium is backlit (shot-podium-ultra.png)"], "fail")).unwrap();
        two.round = 2;
        let v = critic_verdict(Some(&Ok(two)), 2, 2);
        assert!(!v.pass && v.evidence.starts_with("Round 2 of 3"), "{}", v.evidence);
        assert!(v.evidence.ends_with("Round 3 of 3 is still to run: it judges the builder's fixes for this list."), "{}", v.evidence);
        assert_eq!(critic_verdict(Some(&Err("bad json".into())), 1, 2).label(), "FAIL");
    }

    #[test]
    fn packet_lists_everything_and_the_rubric() {
        let p = PacketInputs {
            app: "kart-racer".into(),
            kit: "kart-racer".into(),
            look: "toon".into(),
            round: 2,
            stage: 2,
            images: vec![".vibedeck/review/kart-racer/shot-drift-low.png".into()],
            video: Some(".vibedeck/review/kart-racer/video.webm".into()),
            video_frames: vec![".vibedeck/review/kart-racer/video-frame-1.jpg".into()],
            target_files: vec!["kits/kart-racer/target/moments.md".into()],
            playbook: Some("docs/playbooks/kart-racer.md".into()),
            quality_bar: vec![(8, "Visual quality bar.".into())],
            previous: vec![(1, vec!["fix the world edge".into()])],
            out_dir: ".vibedeck/review/kart-racer/critic".into(),
            ..PacketInputs::default()
        };
        let md = packet_md(&p);
        for want in [
            "shot-drift-low.png",
            "video-frame-1.jpg",
            "kits/kart-racer/target/moments.md",
            "docs/playbooks/kart-racer.md",
            "8. Visual quality bar.",
            "| R11 | Video |",
            "no (from Stage 3)",
            "critic-round-2.json",
            "Round 1 must-fix",
            "fix the world edge",
            "mean of the applicable items 2.3",
        ] {
            assert!(md.contains(want), "missing {want:?}:\n{md}");
        }
        assert_eq!(current_stage("# Stages\n\nCurrent stage: 2 (First kits). More.\n"), 2);
        assert_eq!(current_stage("nothing"), 0);
    }
}
