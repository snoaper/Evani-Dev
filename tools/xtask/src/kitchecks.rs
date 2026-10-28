//! The kit-process checks of `cargo xtask review` from docs/KIT_PROCESS.md
//! "Done when" item 16 on: the target pack, the signature-moment captures and
//! video, the visual regression, distinct colours, contact shadows, the
//! playtest bot, the geocheck reports and the 2D-only camera, bot-level and
//! UI-text checks. Like `checks.rs`, every check is a small pure function from
//! what was measured or read to a [`Verdict`]; `review.rs` reads the files and
//! runs the kit's tests.
//!
//! Where a check needs the kit's help it reads a documented convention, never
//! per-kit code here (docs/KIT_PROCESS.md, "Kit cooperation"):
//!
//! - `kits/<id>/target/`: the target pack (`reference-prompt.md`,
//!   `look-bible.md`, `moments.md` with one `## <moment_id>` per moment,
//!   `hud-layout.md`, optional `playbook.md` and `reference.png`).
//! - The kit crate's tests named `playtest_win*` and `playtest_lose*` (the
//!   playtest bot reaches a win and a lose), and a test `review_dump` that
//!   writes `$EVANI_REVIEW_DUMP/review.json`:
//!   `{"colours": {"player": {"name", "hex", "marker"}, "others": [{"name", "hex"}]},
//!     "objects": [{"name", "gap", "contact_shadow", "intentional"?}]}`.
//!   `intentional` (optional) is the reason an object is off the ground on
//!   purpose (a hovering pickup, a cloud): the gap rule skips it, the
//!   contact-shadow rule still applies, and the card lists it as tagged.
//! - `.vibedeck/geocheck/<app>/report.json` from evani-geocheck (3D) or
//!   evani-geocheck-2d: every issue has a `check` and a `tagged` flag.

use serde_json::Value;

use crate::checks::{Rgb, Verdict};

fn join(items: &[String]) -> String {
    items.join("; ")
}

/// At most `n` items, then how many more.
fn some_of(items: &[String], n: usize) -> String {
    if items.len() <= n {
        return join(items);
    }
    format!("{}; and {} more", items[..n].join("; "), items.len() - n)
}

// ---- markdown helpers ---------------------------------------------------------------

/// `(level, text)` of a markdown heading line.
fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end();
    let level = t.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 || !t[level..].starts_with(' ') {
        return None;
    }
    Some((level, t[level..].trim()))
}

/// The lines under the first heading whose text contains `word` (any case),
/// up to the next heading of the same or a higher level.
pub fn section<'a>(md: &'a str, word: &str) -> Option<Vec<&'a str>> {
    let word = word.to_lowercase();
    let mut lines = md.lines();
    let level = loop {
        let l = lines.next()?;
        if let Some((lv, text)) = heading(l)
            && text.to_lowercase().contains(&word)
        {
            break lv;
        }
    };
    Some(lines.take_while(|l| heading(l).is_none_or(|(lv, _)| lv > level)).collect())
}

/// Table data rows plus list items in `lines` (a table's header row and
/// separator do not count).
pub fn item_count(lines: &[&str]) -> usize {
    let is_sep = |l: &str| {
        let t = l.trim();
        t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
    };
    let mut n = 0;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with('|') {
            let header = lines.get(i + 1).is_some_and(|next| is_sep(next));
            if !is_sep(t) && !header {
                n += 1;
            }
        } else if t.starts_with("- ") || t.starts_with("* ") || t.split_once(". ").is_some_and(|(d, _)| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())) {
            n += 1;
        }
    }
    n
}

/// Distinct `#rrggbb` colours in a text, lowercase, in order of first use.
pub fn hex_colours(md: &str) -> Vec<String> {
    let b = md.as_bytes();
    let mut out: Vec<String> = Vec::new();
    for i in 0..b.len() {
        if b[i] != b'#' {
            continue;
        }
        let run = b[i + 1..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
        let next_is_word = b.get(i + 1 + run).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        if run == 6 && !next_is_word {
            let h = md[i..i + 7].to_lowercase();
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}

/// The moment ids of `moments.md`: every `## <id>` heading whose text is an
/// id (a-z, 0-9 and _, backticks allowed). Other level-2 headings (`## Notes`)
/// are not moments.
pub fn moment_ids(md: &str) -> Vec<String> {
    md.lines()
        .filter_map(heading)
        .filter(|(lv, _)| *lv == 2)
        .map(|(_, t)| t.trim_matches('`').to_string())
        .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .collect()
}

// ---- 16. target pack ---------------------------------------------------------------

/// What `kits/<id>/target/` holds (file texts; `None` when missing).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TargetPack {
    pub exists: bool,
    pub reference_prompt: Option<String>,
    /// `reference.png`, `.jpg` or `.webp` is there (optional).
    pub reference_image: bool,
    pub look_bible: Option<String>,
    pub moments: Option<String>,
    pub hud_layout: Option<String>,
    pub playbook: Option<String>,
    /// Every other markdown file of the pack (a playbook section may live there).
    pub others: Vec<String>,
}

pub const MIN_PACK_COLOURS: usize = 5;
pub const MIN_PACK_PROPS: usize = 10;
pub const PACK_MOMENTS: std::ops::RangeInclusive<usize> = 5..=8;

/// The target pack is complete: a reference prompt, a look bible (palette of
/// at least 5 hex colours, shape language, sky or backdrop, at least 10 theme
/// props), 5 to 8 signature moments, a HUD layout for 1080p and phone, and
/// the genre playbook items that apply.
pub fn target_pack(kit: &str, p: &TargetPack) -> Verdict {
    if !p.exists {
        return Verdict::fail(format!(
            "No target pack: kits/{kit}/target/ is missing (reference prompt, look bible, moments, HUD layout, playbook items; step 2)."
        ));
    }
    let mut bad = Vec::new();
    let mut good = Vec::new();
    match &p.reference_prompt {
        Some(t) if !t.trim().is_empty() => good.push(if p.reference_image { "reference prompt and image" } else { "reference prompt" }.to_string()),
        _ => bad.push("reference-prompt.md is missing or empty".to_string()),
    }
    match &p.look_bible {
        None => bad.push("look-bible.md is missing".into()),
        Some(lb) => {
            let colours = hex_colours(lb).len();
            let lower = lb.to_lowercase();
            let has_heading = |w: &str| lb.lines().filter_map(heading).any(|(_, t)| t.to_lowercase().contains(w));
            let props = section(lb, "prop").map_or(0, |s| item_count(&s));
            if colours < MIN_PACK_COLOURS {
                bad.push(format!("the look bible names {colours} hex colours (at least {MIN_PACK_COLOURS})"));
            }
            if !has_heading("shape") {
                bad.push("the look bible has no shape-language heading".into());
            }
            if !(lower.contains("sky") || lower.contains("backdrop")) {
                bad.push("the look bible says nothing about the sky or backdrop".into());
            }
            if props < MIN_PACK_PROPS {
                bad.push(format!("the look bible's props section lists {props} props (at least {MIN_PACK_PROPS})"));
            }
            good.push(format!("look bible ({colours} colours, {props} props)"));
        }
    }
    match &p.moments {
        None => bad.push("moments.md is missing".into()),
        Some(m) => {
            let ids = moment_ids(m);
            if !PACK_MOMENTS.contains(&ids.len()) {
                bad.push(format!("moments.md has {} `## <id>` moments (5 to 8)", ids.len()));
            }
            good.push(format!("{} moments", ids.len()));
        }
    }
    match &p.hud_layout {
        None => bad.push("hud-layout.md is missing".into()),
        Some(h) => {
            let l = h.to_lowercase();
            if !l.contains("1080") || !l.contains("phone") {
                bad.push("hud-layout.md must cover both 1080p and phone".into());
            } else {
                good.push("HUD layout (1080p and phone)".into());
            }
        }
    }
    let playbook_heading = |t: &String| t.lines().filter_map(heading).any(|(_, h)| h.to_lowercase().contains("playbook"));
    let in_pack = [&p.look_bible, &p.moments, &p.hud_layout, &p.reference_prompt].into_iter().flatten().chain(p.others.iter()).any(playbook_heading);
    if p.playbook.as_ref().is_some_and(|t| !t.trim().is_empty()) || in_pack {
        good.push("playbook items".into());
    } else {
        bad.push("no playbook items (target/playbook.md or a \"Playbook\" heading listing the genre playbook items that apply)".into());
    }
    if bad.is_empty() { Verdict::pass(format!("Target pack complete: {}.", good.join(", "))) } else { Verdict::fail(format!("Target pack incomplete: {}.", join(&bad))) }
}

// ---- 17. signature-moment captures and video ---------------------------------------

/// One showcase capture from capture.mjs (`summary.showcase[]`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShowcaseShot {
    pub moment: String,
    pub tier: String,
    pub file: String,
    pub width: u32,
    pub height: u32,
    /// The burned-in label's text and its rectangle `[x, y, w, h]` in pixels.
    pub label: String,
    pub label_rect: Option<[u32; 4]>,
}

/// The run video (`summary.video`).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub file: String,
    pub seconds: f64,
    pub frames: Vec<String>,
}

/// What the showcase phase of capture.mjs returned.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Showcase {
    pub shots: Vec<ShowcaseShot>,
    pub video: Option<VideoInfo>,
    /// Why the showcase or the video failed, as the script said it.
    pub errors: Vec<String>,
}

pub fn showcase_from(v: &Value) -> Showcase {
    let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let name = |f: String| f.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    let shots = v
        .get("showcase")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| ShowcaseShot {
                    moment: s(c, "moment"),
                    tier: s(c, "tier"),
                    file: name(s(c, "file")),
                    width: c.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                    height: c.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                    label: s(c, "label"),
                    label_rect: c.get("label_rect").and_then(Value::as_array).and_then(|r| {
                        let n: Vec<u32> = r.iter().filter_map(Value::as_f64).map(|x| x.max(0.0).round() as u32).collect();
                        (n.len() == 4).then(|| [n[0], n[1], n[2], n[3]])
                    }),
                })
                .collect()
        })
        .unwrap_or_default();
    let video = v.get("video").filter(|x| x.is_object()).map(|x| VideoInfo {
        file: name(s(x, "file")),
        seconds: x.get("seconds").and_then(Value::as_f64).unwrap_or(0.0),
        frames: x.get("frames").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(|f| name(f.to_string())).collect()).unwrap_or_default(),
    });
    let errors = ["showcase_error", "video_error"].iter().filter_map(|k| v.get(*k).and_then(Value::as_str)).map(str::to_string).collect();
    Showcase { shots, video, errors }
}

/// `"320x180"` or the first `NNN×NNN` in a look's render rules when they
/// speak of an internal resolution (`"256×240 internal"`).
pub fn internal_resolution(review_key: Option<&str>, render_rules: &str) -> Option<(u32, u32)> {
    fn pair(t: &str) -> Option<(u32, u32)> {
        let chars: Vec<char> = t.chars().collect();
        for i in 0..chars.len() {
            if !chars[i].is_ascii_digit() || (i > 0 && chars[i - 1].is_ascii_digit()) {
                continue;
            }
            let a: String = chars[i..].iter().take_while(|c| c.is_ascii_digit()).collect();
            let j = i + a.len();
            if !matches!(chars.get(j), Some('x' | 'X' | '×')) {
                continue;
            }
            let b: String = chars[j + 1..].iter().take_while(|c| c.is_ascii_digit()).collect();
            if let (Ok(w), Ok(h)) = (a.parse(), b.parse())
                && w > 0
                && h > 0
            {
                return Some((w, h));
            }
        }
        None
    }
    match review_key {
        Some(k) => pair(k),
        None if render_rules.to_lowercase().contains("internal") => pair(render_rules),
        None => None,
    }
}

pub const SHOWCASE_W: u32 = 1920;
pub const SHOWCASE_H: u32 = 1080;
pub const MIN_VIDEO_S: f64 = 3.0;

/// The showcase size: 1920x1080, or for a pixel look the internal resolution
/// times the smallest whole number that reaches 1080 rows.
pub fn showcase_size(internal: Option<(u32, u32)>) -> (u32, u32) {
    match internal {
        Some((w, h)) => {
            let k = SHOWCASE_H.div_ceil(h).max(1);
            (w * k, h * k)
        }
        None => (SHOWCASE_W, SHOWCASE_H),
    }
}

/// Why a capture's size is wrong, if it is.
pub fn size_problem(w: u32, h: u32, internal: Option<(u32, u32)>) -> Option<String> {
    match internal {
        Some((iw, ih)) => {
            let k = w / iw.max(1);
            if !w.is_multiple_of(iw.max(1)) || h != ih * k || h < SHOWCASE_H {
                Some(format!("{w}x{h} is not a whole multiple (of at least {}) of the internal {iw}x{ih}", SHOWCASE_H.div_ceil(ih.max(1))))
            } else {
                None
            }
        }
        None if w < SHOWCASE_W || h < SHOWCASE_H => Some(format!("{w}x{h} is under 1920x1080")),
        None => None,
    }
}

/// Every signature moment captured on every showcase tier at 1920x1080 or
/// more (pixel looks: integer-scaled), each labelled with its tier and the
/// engine commit, plus a run video of at least 3 seconds.
pub fn capture_set(moments: &[String], tiers: &[&str], sc: &Showcase, engine: &str, internal: Option<(u32, u32)>) -> Verdict {
    if moments.is_empty() {
        return Verdict::not_run("No signature moments to capture: the kit has no target/moments.md and evani.toml no `[review] moments`.");
    }
    if sc.shots.is_empty() {
        let why = if sc.errors.is_empty() { "the capture ran no showcase".to_string() } else { join(&sc.errors) };
        return Verdict::not_run(format!("No showcase captures ({why})."));
    }
    let mut bad = Vec::new();
    let short = &engine[..engine.len().min(7)];
    for m in moments {
        for t in tiers {
            let Some(s) = sc.shots.iter().find(|s| &s.moment == m && s.tier.eq_ignore_ascii_case(t)) else {
                bad.push(format!("{m} on {t} is missing"));
                continue;
            };
            if let Some(p) = size_problem(s.width, s.height, internal) {
                bad.push(format!("{} {p}", s.file));
            }
            let label = s.label.to_lowercase();
            if !label.contains(&t.to_lowercase()) || short.is_empty() || !label.contains(&short.to_lowercase()) {
                bad.push(format!("{} label \"{}\" does not name tier {t} and engine {short}", s.file, s.label));
            }
        }
    }
    match &sc.video {
        None => bad.push(format!("no run video{}", if sc.errors.is_empty() { String::new() } else { format!(" ({})", join(&sc.errors)) })),
        Some(v) if v.file.is_empty() => bad.push(format!("{} video frames but no video file ({})", v.frames.len(), join(&sc.errors))),
        Some(v) if v.seconds < MIN_VIDEO_S => bad.push(format!("the video is {:.1} s (at least {MIN_VIDEO_S} s)", v.seconds)),
        Some(_) => {}
    }
    if bad.is_empty() {
        let (w, h) = (sc.shots[0].width, sc.shots[0].height);
        let video = sc.video.as_ref().map_or(String::new(), |v| format!(", video {:.0} s", v.seconds));
        Verdict::pass(format!("{} moments on {} at {w}x{h}, labelled with tier and engine {short}{video}.", moments.len(), tiers.join(" and ")))
    } else {
        Verdict::fail(format!("Showcase captures: {}.", some_of(&bad, 6)))
    }
}

// ---- 18. visual regression -----------------------------------------------------------

/// A pixel differs when its largest channel difference is over this.
pub const DIFF_CHANNEL: u8 = 32;
/// A capture is flagged when more than this share of its pixels differ.
pub const DIFF_FLAG_SHARE: f64 = 0.10;

/// The share of pixels that differ between two captures of the same size,
/// ignoring the rectangles `[x, y, w, h]` (the burned-in labels).
pub fn diff_share(a: &Rgb, b: &Rgb, ignore: &[[u32; 4]]) -> Result<f64, String> {
    if a.w != b.w || a.h != b.h {
        return Err(format!("size changed from {}x{} to {}x{}", b.w, b.h, a.w, a.h));
    }
    let skip = |x: usize, y: usize| ignore.iter().any(|r| x >= r[0] as usize && x < (r[0] + r[2]) as usize && y >= r[1] as usize && y < (r[1] + r[3]) as usize);
    let (mut diff, mut total) = (0u64, 0u64);
    for y in 0..a.h {
        for x in 0..a.w {
            if skip(x, y) {
                continue;
            }
            let i = (y * a.w + x) * 3;
            let d = (0..3).map(|c| a.px[i + c].abs_diff(b.px[i + c])).max().unwrap_or(0);
            total += 1;
            diff += u64::from(d > DIFF_CHANNEL);
        }
    }
    Ok(if total == 0 { 0.0 } else { diff as f64 / total as f64 })
}

/// Each current capture against its accepted one: `(file, share or why not)`;
/// `None` when that capture has no accepted twin.
pub fn regression(results: &[(String, Option<Result<f64, String>>)], accepted_any: bool) -> Verdict {
    if !accepted_any {
        return Verdict::waiting("Nothing to compare with yet: no accepted captures (after Chris approves, `cargo xtask review <kit> --accept` keeps these as the baseline).");
    }
    if results.is_empty() {
        return Verdict::not_run("No showcase captures to compare with the accepted ones.");
    }
    let mut big = Vec::new();
    let mut new = Vec::new();
    let mut worst = 0.0f64;
    for (f, r) in results {
        match r {
            None => new.push(f.clone()),
            Some(Ok(s)) => {
                worst = worst.max(*s);
                if *s > DIFF_FLAG_SHARE {
                    big.push(format!("{f} {:.0}%", s * 100.0));
                }
            }
            Some(Err(e)) => big.push(format!("{f} {e}")),
        }
    }
    let new_note = if new.is_empty() { String::new() } else { format!(" New, no accepted twin: {}.", new.join(", ")) };
    if big.is_empty() {
        Verdict::pass(format!("Every capture within {:.0}% of the accepted one (largest {:.1}%).{new_note}", DIFF_FLAG_SHARE * 100.0, worst * 100.0))
    } else {
        Verdict::flag(format!("Big changes against the accepted captures for the critic and Chris to look at: {}.{new_note}", some_of(&big, 6)))
    }
}

// ---- the kit's review dump -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Colour {
    pub name: String,
    pub hex: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub player: Colour,
    /// How the player is marked apart from colour ("arrow above the kart").
    pub marker: String,
    pub others: Vec<Colour>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grounding {
    pub name: String,
    /// Distance from the object's lowest point to the ground under it (m in
    /// 3D, px in 2D); negative when sunk.
    pub gap: f64,
    pub contact_shadow: Option<bool>,
    /// Why the object is off the ground on purpose (`None`: it must sit on
    /// the ground). Tagged objects skip the gap rule, not the shadow rule.
    pub intentional: Option<String>,
}

/// `$EVANI_REVIEW_DUMP/review.json` from the kit's `review_dump` test.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewDump {
    pub palette: Option<Palette>,
    pub objects: Option<Vec<Grounding>>,
}

pub fn parse_review_dump(v: &Value) -> Result<ReviewDump, String> {
    let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).map(str::to_string);
    let colour = |x: &Value| -> Result<Colour, String> {
        Ok(Colour { name: s(x, "name").unwrap_or_default(), hex: s(x, "hex").ok_or("a colour without `hex`")? })
    };
    let palette = match v.get("colours") {
        None | Some(Value::Null) => None,
        Some(c) => {
            let p = c.get("player").ok_or("`colours` has no `player`")?;
            Some(Palette {
                player: colour(p)?,
                marker: s(p, "marker").unwrap_or_default(),
                others: c.get("others").and_then(Value::as_array).map(|a| a.iter().map(colour).collect::<Result<Vec<_>, _>>()).transpose()?.unwrap_or_default(),
            })
        }
    };
    let objects = match v.get("objects") {
        None | Some(Value::Null) => None,
        Some(o) => Some(
            o.as_array()
                .ok_or("`objects` must be a list")?
                .iter()
                .map(|x| -> Result<Grounding, String> {
                    Ok(Grounding {
                        name: s(x, "name").unwrap_or_default(),
                        gap: x.get("gap").and_then(Value::as_f64).ok_or("an object without a numeric `gap`")?,
                        contact_shadow: x.get("contact_shadow").and_then(Value::as_bool),
                        intentional: s(x, "intentional").map(|r| r.trim().to_string()).filter(|r| !r.is_empty()),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    Ok(ReviewDump { palette, objects })
}

// ---- 19. contact shadows ---------------------------------------------------------------

pub const GAP_3D_M: f64 = 0.05;
pub const GAP_2D_PX: f64 = 1.0;

/// Every object sits on the ground (gap within 5 cm, or 1 px in 2D) and, in
/// 3D, casts a contact shadow. Objects tagged `intentional` (off the ground
/// on purpose, with a reason) skip the gap rule but still need the shadow;
/// the evidence lists them as tagged.
pub fn grounded(objects: Option<&[Grounding]>, is_2d: bool, why_none: &str) -> Verdict {
    let Some(objs) = objects.filter(|o| !o.is_empty()) else {
        return Verdict::not_run(format!("No grounding data ({why_none})."));
    };
    let (tol, unit) = if is_2d { (GAP_2D_PX, "px") } else { (GAP_3D_M, "m") };
    let mut bad = Vec::new();
    let mut tagged = Vec::new();
    for o in objs {
        if let Some(why) = &o.intentional {
            tagged.push(format!("{} ({why})", o.name));
        } else if o.gap.abs() > tol {
            bad.push(format!("{} {} {:.2} {unit}", o.name, if o.gap > 0.0 { "floats" } else { "is sunk" }, o.gap.abs()));
        }
        if !is_2d && o.contact_shadow != Some(true) {
            bad.push(format!("{} has no contact shadow", o.name));
        }
    }
    let tags = if tagged.is_empty() { String::new() } else { format!(" Tagged off the ground on purpose: {}.", some_of(&tagged, 4)) };
    if bad.is_empty() {
        let on_ground = objs.len() - tagged.len();
        Verdict::pass(format!(
            "{on_ground} objects within {tol} {unit} of the ground{}.{tags}",
            if is_2d { "" } else { ", each with a contact shadow" }
        ))
    } else {
        Verdict::fail(format!("Not grounded: {}.{tags}", some_of(&bad, 6)))
    }
}

// ---- 20. distinct colours ----------------------------------------------------------------

/// `#rrggbb` to CIE L*a*b* (D65).
pub fn hex_to_lab(hex: &str) -> Option<[f64; 3]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ch = |i: usize| -> f64 {
        let c = f64::from(u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0)) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (ch(0), ch(2), ch(4));
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f64| if t > (6.0f64 / 29.0).powi(3) { t.cbrt() } else { t / (3.0 * (6.0f64 / 29.0).powi(2)) + 4.0 / 29.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    Some([116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)])
}

/// CIE76 colour difference between two hex colours.
pub fn delta_e(a: &str, b: &str) -> Option<f64> {
    let (p, q) = (hex_to_lab(a)?, hex_to_lab(b)?);
    Some(((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt())
}

pub const MIN_PAIR_DE: f64 = 20.0;
pub const MIN_PLAYER_DE: f64 = 40.0;

/// Racer or unit colours are distinct (every pair at least ΔE 20 apart), the
/// player's is far from all of them (ΔE 40) and the player is also marked.
pub fn colours(palette: Option<&Palette>, why_none: &str) -> Verdict {
    let Some(p) = palette else {
        return Verdict::not_run(format!("No colour palette ({why_none})."));
    };
    let mut bad = Vec::new();
    let all: Vec<&Colour> = std::iter::once(&p.player).chain(p.others.iter()).collect();
    if let Some(c) = all.iter().find(|c| hex_to_lab(&c.hex).is_none()) {
        return Verdict::fail(format!("`{}` is not a #rrggbb colour ({}).", c.hex, c.name));
    }
    let mut nearest_player = f64::MAX;
    for o in &p.others {
        let d = delta_e(&p.player.hex, &o.hex).unwrap_or(0.0);
        nearest_player = nearest_player.min(d);
        if d < MIN_PLAYER_DE {
            bad.push(format!("{} {} is ΔE {d:.0} from the player's {} (at least {MIN_PLAYER_DE:.0})", o.name, o.hex, p.player.hex));
        }
    }
    let mut nearest_pair = f64::MAX;
    for (i, a) in p.others.iter().enumerate() {
        for b in &p.others[i + 1..] {
            let d = delta_e(&a.hex, &b.hex).unwrap_or(0.0);
            nearest_pair = nearest_pair.min(d);
            if d < MIN_PAIR_DE {
                bad.push(format!("{} and {} are ΔE {d:.0} apart (at least {MIN_PAIR_DE:.0})", a.name, b.name));
            }
        }
    }
    if p.marker.trim().is_empty() {
        bad.push("the player has no marker besides colour".into());
    }
    if bad.is_empty() {
        let pair = if nearest_pair == f64::MAX { "one other colour".to_string() } else { format!("closest pair ΔE {nearest_pair:.0}") };
        let player = if nearest_player == f64::MAX { String::new() } else { format!(", player at least ΔE {nearest_player:.0} from all") };
        Verdict::pass(format!("{} colours, {pair}{player}; player marked by {}.", all.len(), p.marker.trim()))
    } else {
        Verdict::fail(format!("Colours: {}.", some_of(&bad, 6)))
    }
}

// ---- 21. playtest bot -------------------------------------------------------------------

/// Which conventions the kit's source provides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KitHooks {
    pub playtest_win: bool,
    pub playtest_lose: bool,
    pub review_dump: bool,
}

impl KitHooks {
    pub fn any(self) -> bool {
        self.playtest_win || self.playtest_lose || self.review_dump
    }
}

/// Look for `fn playtest_win*`, `fn playtest_lose*` and `fn review_dump` in
/// the kit's source.
pub fn kit_hooks(src: &str) -> KitHooks {
    KitHooks { playtest_win: src.contains("fn playtest_win"), playtest_lose: src.contains("fn playtest_lose"), review_dump: src.contains("fn review_dump") }
}

/// `(test name, passed)` from libtest's output lines
/// (`test path::name ... ok` / `... FAILED`; ignored tests are left out).
pub fn parse_libtest(out: &str) -> Vec<(String, bool)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("test ")?;
            let (name, result) = rest.rsplit_once(" ... ")?;
            match result.trim() {
                "ok" => Some((name.trim().to_string(), true)),
                r if r.starts_with("FAILED") => Some((name.trim().to_string(), false)),
                _ => None,
            }
        })
        .collect()
}

/// Item 29: a playground (`[playground]` in evani.toml) supports free roam:
/// declared there and in the kit, with a free-roam test
/// (`evani_guide::playground`, the rule `check` and `build --ship` apply).
/// `None`: not a playground. The test itself runs with the kit's tests
/// (`cargo xtask check --full`).
pub fn free_roam(problems: Option<&[String]>) -> Verdict {
    match problems {
        None => Verdict::not_applicable("Not a playground (no `[playground]` in evani.toml), so free roam is not required yet."),
        Some([]) => Verdict::pass("Playground with free roam: declared in evani.toml and in the kit, with a free-roam test."),
        Some(p) => Verdict::fail(format!("Playground without working free roam: {}", p.join("; "))),
    }
}

/// The playtest bot reaches a win and a lose and nothing hangs: at least one
/// passing `playtest_win*` and `playtest_lose*` test, no failing playtest.
pub fn playtest(tests: Option<&[(String, bool)]>, why_none: &str) -> Verdict {
    let Some(tests) = tests else {
        return Verdict::not_run(format!("The playtest bot did not run ({why_none})."));
    };
    let short = |n: &str| n.rsplit("::").next().unwrap_or(n).to_string();
    let play: Vec<(String, bool)> = tests.iter().map(|(n, ok)| (short(n), *ok)).filter(|(n, _)| n.starts_with("playtest_")).collect();
    if play.is_empty() {
        return Verdict::not_run("The kit has no playtest_win* or playtest_lose* tests.");
    }
    let failed: Vec<String> = play.iter().filter(|(_, ok)| !ok).map(|(n, _)| n.clone()).collect();
    let win = play.iter().any(|(n, ok)| *ok && n.starts_with("playtest_win"));
    let lose = play.iter().any(|(n, ok)| *ok && n.starts_with("playtest_lose"));
    let mut bad = Vec::new();
    if !failed.is_empty() {
        bad.push(format!("failed (a softlock, timeout or wrong end): {}", failed.join(", ")));
    }
    if !win {
        bad.push("no passing playtest_win* test".into());
    }
    if !lose {
        bad.push("no passing playtest_lose* test".into());
    }
    if bad.is_empty() {
        Verdict::pass(format!("The bot reached win and lose in {} playtests: {}.", play.len(), play.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")))
    } else {
        Verdict::fail(format!("Playtest: {}.", join(&bad)))
    }
}

// ---- 22-25. geocheck reports ------------------------------------------------------------------

// The report reader is shared with `cargo xtask check` (evani-guide).
pub use evani_guide::reports::{GeoIssue, geo_issues};

fn describe(issues: &[&GeoIssue]) -> String {
    some_of(&issues.iter().map(|i| if i.what.is_empty() { i.check.clone() } else { format!("{}: {}", i.check, i.what) }).collect::<Vec<_>>(), 5)
}

/// No untagged issue in `.vibedeck/geocheck/<app>/report.json`.
pub fn geocheck(report: Option<&Value>) -> Verdict {
    let Some(r) = report else {
        return Verdict::not_run("No geocheck report (.vibedeck/geocheck/<app>/report.json): run evani-geocheck (3D) or evani-geocheck-2d on the game first.");
    };
    let all = geo_issues(r);
    let untagged: Vec<&GeoIssue> = all.iter().filter(|i| !i.tagged).collect();
    if untagged.is_empty() {
        Verdict::pass(format!("No untagged geometry issues ({} tagged as intended).", all.len()))
    } else {
        Verdict::fail(format!("{} untagged geometry issues: {}.", untagged.len(), describe(&untagged)))
    }
}

/// A 2D-only check read from the same report, by the issues' `check` slug.
pub fn geocheck_area(report: Option<&Value>, slug: &str, title: &str, is_2d: bool) -> Verdict {
    if !is_2d {
        return Verdict::not_applicable(format!("{title} is a 2D check; this kit's look is 3D."));
    }
    let Some(r) = report else {
        return Verdict::not_run(format!("No geocheck-2d report, so {} was not checked.", title.to_lowercase()));
    };
    let all: Vec<GeoIssue> = geo_issues(r).into_iter().filter(|i| i.check == slug).collect();
    let untagged: Vec<&GeoIssue> = all.iter().filter(|i| !i.tagged).collect();
    if untagged.is_empty() {
        Verdict::pass(format!("{title}: no untagged issues in the geocheck-2d report ({} tagged).", all.len()))
    } else {
        Verdict::fail(format!("{title}: {} untagged issues: {}.", untagged.len(), describe(&untagged)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Kind;
    use serde_json::json;

    const BIBLE: &str = "# Look bible\n\n## (a) Palette\n\n| Role | Hex |\n|---|---|\n| Sky | #4f9bf5 |\n| Road | #6f6c7e |\n| Grass | #7cd161 / #5fbb55 |\n| Ink | #2b1240 |\n\n## (b) Shape language\n\n- Rounded.\n\n## (c) Sky and backdrop\n\n- Dome.\n\n## (d) Theme props\n\n| # | Prop |\n|---|---|\n| 1 | a |\n| 2 | b |\n| 3 | c |\n| 4 | d |\n| 5 | e |\n| 6 | f |\n| 7 | g |\n| 8 | h |\n| 9 | i |\n| 10 | j |\n\nBudget under 70 000 triangles.\n\n## (e) Lighting\n\n- Sun.\n";

    fn moments(n: usize) -> String {
        let mut s = "# Signature moments\n\nIntro.\n\n## Notes\n\n".to_string();
        for i in 0..n {
            s.push_str(&format!("## moment_{i}\n\n- In frame: x.\n\n"));
        }
        s
    }

    fn pack() -> TargetPack {
        TargetPack {
            exists: true,
            reference_prompt: Some("A kart on a candy track.".into()),
            reference_image: false,
            look_bible: Some(BIBLE.into()),
            moments: Some(moments(7)),
            hud_layout: Some("Sizes at 1920x1080 (1080p) and on a phone at 844x390.".into()),
            playbook: Some("- Countdown lights.".into()),
            others: Vec::new(),
        }
    }

    #[test]
    fn markdown_helpers_count_props_colours_and_moments() {
        assert_eq!(item_count(&section(BIBLE, "prop").unwrap()), 10);
        assert_eq!(item_count(&section(BIBLE, "shape").unwrap()), 1);
        assert_eq!(hex_colours(BIBLE), ["#4f9bf5", "#6f6c7e", "#7cd161", "#5fbb55", "#2b1240"]);
        assert!(hex_colours("#12345678 and #abc and #a1b2c3d").is_empty(), "only exact #rrggbb counts");
        assert_eq!(moment_ids(&moments(3)), ["moment_0", "moment_1", "moment_2"], "`## Notes` is not a moment");
        assert_eq!(moment_ids("## `drift`\n### sub\n## Boost Pad\n"), ["drift"]);
        assert!(section(BIBLE, "nothing").is_none());
    }

    #[test]
    fn a_complete_pack_passes_and_each_gap_fails() {
        let v = target_pack("kart-racer", &pack());
        assert!(v.pass, "{}", v.evidence);
        assert!(v.evidence.contains("5 colours, 10 props") && v.evidence.contains("7 moments"), "{}", v.evidence);

        assert!(!target_pack("kart-racer", &TargetPack::default()).pass);
        let mut p = pack();
        p.moments = Some(moments(4));
        assert!(target_pack("k", &p).evidence.contains("4 `## <id>` moments"));
        let mut p = pack();
        p.moments = Some(moments(9));
        assert!(!target_pack("k", &p).pass);
        let mut p = pack();
        p.look_bible = Some(BIBLE.replace("| 10 | j |\n", ""));
        assert!(target_pack("k", &p).evidence.contains("9 props"));
        let mut p = pack();
        p.hud_layout = Some("1080p only".into());
        assert!(target_pack("k", &p).evidence.contains("1080p and phone"));
        let mut p = pack();
        p.playbook = None;
        assert!(target_pack("k", &p).evidence.contains("no playbook items"));
        p.others = vec!["# Readme\n\n## Playbook items that apply\n\n- x\n".into()];
        assert!(target_pack("k", &p).pass, "a Playbook heading anywhere in the pack counts");
    }

    /// The kart pilot's pack (kits/kart-racer/target on the kart pilot branch):
    /// 38 colours, 15 props, 7 moments, but no playbook section yet.
    #[test]
    fn the_kart_pilot_pack_shape() {
        let bible = BIBLE.replace("## (d) Theme props", "## (d) Theme props\n\n| 11 | k |\n| 12 | l |");
        let mut p = pack();
        p.look_bible = Some(bible);
        p.playbook = None;
        let v = target_pack("kart-racer", &p);
        assert!(!v.pass && v.evidence.contains("playbook"), "{}", v.evidence);
    }

    fn shot(m: &str, t: &str, w: u32, h: u32, label: &str) -> ShowcaseShot {
        ShowcaseShot { moment: m.into(), tier: t.into(), file: format!("shot-{m}-{t}.png"), width: w, height: h, label: label.into(), label_rect: Some([1700, 1040, 200, 30]) }
    }

    #[test]
    fn showcase_parse_sizes_and_labels() {
        let v = json!({
            "showcase": [{"moment": "drift", "tier": "low", "file": "C:/x/shot-drift-low.png", "width": 1920, "height": 1080, "label": "Low · engine abc1234 · drift", "label_rect": [1700.4, 1040, 200, 30.2]}],
            "video": {"file": "C:/x/video.webm", "seconds": 10.0, "frames": ["C:/x/video-frame-1.jpg"]},
        });
        let sc = showcase_from(&v);
        assert_eq!(sc.shots[0].file, "shot-drift-low.png");
        assert_eq!(sc.shots[0].label_rect, Some([1700, 1040, 200, 30]));
        assert_eq!(sc.video.as_ref().unwrap().frames, ["video-frame-1.jpg"]);
        assert!(sc.errors.is_empty());
        assert_eq!(showcase_from(&json!({"video_error": "no ffmpeg"})).errors, ["no ffmpeg"]);

        assert_eq!(internal_resolution(Some("320x180"), ""), Some((320, 180)));
        assert_eq!(internal_resolution(None, "256×240 internal, fixed 54-colour palette"), Some((256, 240)));
        assert_eq!(internal_resolution(None, "Render to a low internal resolution (about 400×240), threshold"), Some((400, 240)));
        assert_eq!(internal_resolution(None, "Toon bands and outlines at 1920x1080."), None, "no internal resolution named");
        assert_eq!(showcase_size(Some((320, 180))), (1920, 1080));
        assert_eq!(showcase_size(Some((160, 144))), (1280, 1152));
        assert_eq!(showcase_size(None), (1920, 1080));
        assert!(size_problem(1920, 1080, Some((320, 180))).is_none());
        assert!(size_problem(1920, 1080, Some((256, 240))).is_some(), "1920/256 is not whole");
        assert!(size_problem(1280, 720, None).is_some());
        assert!(size_problem(2560, 1440, None).is_none());
    }

    #[test]
    fn capture_set_needs_every_moment_tier_label_and_video() {
        let ms = vec!["drift".to_string(), "finish".to_string()];
        let tiers = ["low", "ultra"];
        let mut sc = Showcase {
            shots: ms.iter().flat_map(|m| tiers.iter().map(move |t| shot(m, t, 1920, 1080, &format!("{} · engine abc1234 · {m}", t.to_uppercase())))).collect(),
            video: Some(VideoInfo { file: "video.webm".into(), seconds: 10.0, frames: vec![] }),
            errors: vec![],
        };
        let v = capture_set(&ms, &tiers, &sc, "abc1234", None);
        assert!(v.pass, "{}", v.evidence);
        assert!(!capture_set(&ms, &tiers, &sc, "fff9999", None).pass, "label names another engine");
        sc.shots.pop();
        assert!(capture_set(&ms, &tiers, &sc, "abc1234", None).evidence.contains("finish on ultra is missing"));
        sc.video = Some(VideoInfo { file: "v".into(), seconds: 2.0, frames: vec![] });
        assert!(capture_set(&ms, &tiers, &sc, "abc1234", None).evidence.contains("2.0 s"));
        sc.shots[0].width = 640;
        assert!(capture_set(&ms, &tiers, &sc, "abc1234", None).evidence.contains("under 1920x1080"));
        assert_eq!(capture_set(&[], &tiers, &sc, "abc1234", None).kind, Kind::NotRun);
        assert_eq!(capture_set(&ms, &tiers, &Showcase::default(), "abc1234", None).kind, Kind::NotRun);
    }

    fn img(w: usize, h: usize, v: u8) -> Rgb {
        Rgb { w, h, px: vec![v; w * h * 3] }
    }

    #[test]
    fn regression_flags_big_changes_and_ignores_the_label() {
        let a = img(10, 10, 100);
        let mut b = img(10, 10, 100);
        for x in 0..10 {
            let i = (9 * 10 + x) * 3;
            b.px[i] = 200; // the bottom row: 10%
        }
        assert!((diff_share(&a, &b, &[]).unwrap() - 0.1).abs() < 1e-9);
        assert_eq!(diff_share(&a, &b, &[[0, 9, 10, 1]]).unwrap(), 0.0, "the label row is ignored");
        let mut small = img(10, 10, 100);
        small.px[0] = 120; // under the channel threshold
        assert_eq!(diff_share(&a, &small, &[]).unwrap(), 0.0);
        assert!(diff_share(&a, &img(5, 5, 0), &[]).is_err());

        let v = regression(&[("shot-a.png".into(), Some(Ok(0.05))), ("shot-b.png".into(), None)], true);
        assert!(v.pass && v.kind == Kind::Measured && v.evidence.contains("shot-b.png"), "{}", v.evidence);
        let v = regression(&[("shot-a.png".into(), Some(Ok(0.35)))], true);
        assert_eq!((v.label(), v.evidence.contains("35%")), ("FLAG", true));
        let v = regression(&[], false);
        assert_eq!((v.label(), v.pass), ("not run", true), "a first review is not blocked");
    }

    fn dump() -> Value {
        json!({
            "colours": {
                "player": {"name": "player", "hex": "#ff4fa3", "marker": "arrow and P1 tag above the kart"},
                "others": [
                    {"name": "Pip", "hex": "#ff7a1a"}, {"name": "Taffy", "hex": "#ffd400"}, {"name": "Clover", "hex": "#8bd900"},
                    {"name": "Bramble", "hex": "#00b86b"}, {"name": "Nib", "hex": "#00c8e0"}, {"name": "Mo", "hex": "#2a7bff"},
                    {"name": "Rumble", "hex": "#3a2fd0"}, {"name": "Zuzu", "hex": "#a23cff"}
                ]
            },
            "objects": [{"name": "kart0", "gap": 0.01, "contact_shadow": true}, {"name": "tree3", "gap": -0.02, "contact_shadow": true}]
        })
    }

    #[test]
    fn lab_matches_known_values() {
        let w = hex_to_lab("#ffffff").unwrap();
        assert!((w[0] - 100.0).abs() < 0.01 && w[1].abs() < 0.01 && w[2].abs() < 0.01, "{w:?}");
        let r = hex_to_lab("#ff0000").unwrap();
        assert!((r[0] - 53.24).abs() < 0.05 && (r[1] - 80.09).abs() < 0.1 && (r[2] - 67.20).abs() < 0.1, "{r:?}");
        assert!(hex_to_lab("ff0000").is_some() && hex_to_lab("#ff00").is_none());
        assert!(delta_e("#000000", "#ffffff").unwrap() > 99.0);
    }

    /// The kart pilot's palette (look bible, rival colours) passes.
    #[test]
    fn the_kart_palette_passes_and_a_close_rival_fails() {
        let d = parse_review_dump(&dump()).unwrap();
        let v = colours(d.palette.as_ref(), "");
        assert!(v.pass, "{}", v.evidence);
        let mut p = d.palette.clone().unwrap();
        p.others.push(Colour { name: "Rose".into(), hex: "#ff5aa8".into() });
        let v = colours(Some(&p), "");
        assert!(!v.pass && v.evidence.contains("Rose"), "{}", v.evidence);
        let mut p = d.palette.clone().unwrap();
        p.others.push(Colour { name: "Pip2".into(), hex: "#ff7d1f".into() });
        assert!(colours(Some(&p), "").evidence.contains("Pip and Pip2"));
        let mut p = d.palette.unwrap();
        p.marker = " ".into();
        assert!(colours(Some(&p), "").evidence.contains("no marker"));
        assert_eq!(colours(None, "no review_dump test").kind, Kind::NotRun);
    }

    #[test]
    fn grounding_needs_small_gaps_and_contact_shadows() {
        let d = parse_review_dump(&dump()).unwrap();
        assert!(grounded(d.objects.as_deref(), false, "").pass);
        let floating = [
            Grounding { name: "coin".into(), gap: 0.3, contact_shadow: Some(true), intentional: None },
            Grounding { name: "rock".into(), gap: 0.0, contact_shadow: None, intentional: None },
        ];
        let v = grounded(Some(&floating), false, "");
        assert!(v.evidence.contains("coin floats 0.30 m") && v.evidence.contains("rock has no contact shadow"), "{}", v.evidence);
        let v = grounded(Some(&floating[1..]), true, "");
        assert!(v.pass, "2D needs no contact shadow: {}", v.evidence);
        assert_eq!(grounded(Some(&[]), false, "x").kind, Kind::NotRun);
        assert!(parse_review_dump(&json!({"objects": [{"name": "a"}]})).is_err());
        assert_eq!(parse_review_dump(&json!({})).unwrap(), ReviewDump::default());
    }

    /// An object off the ground on purpose, with a reason, skips the gap rule
    /// but still needs its contact shadow, and the evidence lists it.
    #[test]
    fn intentional_objects_skip_the_gap_but_not_the_shadow() {
        let d = parse_review_dump(&json!({"objects": [
            {"name": "kart0", "gap": 0.0, "contact_shadow": true},
            {"name": "item_box0", "gap": 0.95, "contact_shadow": true, "intentional": "hovering pickup"},
            {"name": "cloud", "gap": 300.0, "contact_shadow": true, "intentional": "  "}
        ]}))
        .unwrap();
        let objs = d.objects.unwrap();
        assert_eq!(objs[1].intentional.as_deref(), Some("hovering pickup"));
        assert_eq!(objs[2].intentional, None, "a blank reason is no tag");
        let v = grounded(Some(&objs[..2]), false, "");
        assert!(v.pass, "{}", v.evidence);
        assert!(v.evidence.starts_with("1 objects") && v.evidence.contains("item_box0 (hovering pickup)"), "{}", v.evidence);
        let v = grounded(Some(&objs), false, "");
        assert!(!v.pass && v.evidence.contains("cloud floats"), "{}", v.evidence);
        let mut no_shadow = objs[1].clone();
        no_shadow.contact_shadow = Some(false);
        let v = grounded(Some(&[no_shadow]), false, "");
        assert!(!v.pass && v.evidence.contains("item_box0 has no contact shadow"), "{}", v.evidence);
    }

    const LIBTEST: &str = "running 4 tests\ntest tests::playtest_win_three_laps ... ok\ntest tests::playtest_lose_last_place ... ok\ntest tests::review_dump ... ok\ntest tests::slow ... ignored\n\ntest result: ok. 3 passed\n";

    #[test]
    fn playtest_reads_libtest_lines() {
        let t = parse_libtest(LIBTEST);
        assert_eq!(t.len(), 3);
        assert!(playtest(Some(&t), "").pass);
        let t = parse_libtest(&LIBTEST.replace("playtest_lose_last_place ... ok", "playtest_lose_last_place ... FAILED"));
        let v = playtest(Some(&t), "");
        assert!(!v.pass && v.evidence.contains("playtest_lose_last_place") && v.evidence.contains("no passing playtest_lose"), "{}", v.evidence);
        let t = parse_libtest("test a::playtest_win ... ok\n");
        assert!(playtest(Some(&t), "").evidence.contains("no passing playtest_lose"));
        assert_eq!(playtest(None, "the kit has no playtest tests").kind, Kind::NotRun);
        assert_eq!(playtest(Some(&[]), "").kind, Kind::NotRun);
        let h = kit_hooks("#[test]\nfn playtest_win_race() {}\nfn review_dump() {}");
        assert!(h.playtest_win && !h.playtest_lose && h.review_dump && h.any());
        assert!(!kit_hooks("fn main() {}").any());
    }

    #[test]
    fn geocheck_reads_both_report_formats() {
        let r3 = json!({"issues": [{"check": "floating", "tagged": null}, {"check": "overlap", "tagged": "rock pile, on purpose"}]});
        let v = geocheck(Some(&r3));
        assert!(!v.pass && v.evidence.starts_with("1 untagged") && v.evidence.contains("floating"), "{}", v.evidence);
        let r2 = json!({"format": "evani-geocheck-2d-report/1", "groups": [
            {"check": "camera", "type": "terrain_share", "issues": [{"check": "camera", "kind": "terrain_share", "tagged": false, "message": "terrain fills 48% of the screen"}]},
            {"check": "popups", "type": "covers_player", "issues": [{"check": "popups", "tagged": true}]}
        ]});
        assert!(!geocheck(Some(&r2)).pass);
        let cam = geocheck_area(Some(&r2), "camera", "Camera framing", true);
        assert!(!cam.pass && cam.evidence.contains("48%"), "{}", cam.evidence);
        assert!(geocheck_area(Some(&r2), "popups", "UI text rule", true).pass, "a tagged popup issue does not block");
        assert!(geocheck_area(Some(&r2), "bot", "Bot level checks", true).pass);
        assert_eq!(geocheck_area(Some(&r2), "camera", "Camera framing", false).label(), "n/a");
        assert_eq!(geocheck_area(None, "bot", "Bot level checks", true).kind, Kind::NotRun);
        assert_eq!(geocheck(None).kind, Kind::NotRun);
        assert!(geocheck(Some(&json!({"issues": [], "groups": []}))).pass);
    }

    #[test]
    fn free_roam_applies_to_playgrounds_only() {
        assert_eq!(free_roam(None).kind, Kind::NotApplicable);
        assert!(free_roam(Some(&[])).pass);
        let v = free_roam(Some(&["declares free roam, but has no free-roam test".to_string()]));
        assert!(!v.pass && v.kind == Kind::Measured && v.evidence.contains("no free-roam test"), "{}", v.evidence);
    }
}
