//! Page hot-replace (docs/LIVE_API.md 14): the bridge watches the game's
//! page files (`apps/<game>/index.html` and the stylesheets it links) and,
//! when one changes, swaps the changed part into the running page without
//! a reload: a stylesheet's text, the `<title>`, or the markup inside an
//! element the game's code doesn't fill. Anything else (ids the code binds
//! to, scripts, build links, markup outside those parts) tells the
//! Evani.dev tab that the page needs a fresh start, through the tab's
//! existing `live_swap_failed` / `page_changed` messages.
//!
//! The bridge also re-serves the page part itself: `/` is composed from the
//! current page source and the game build's own script tags, so page edits
//! never need a wasm rebuild. Dev only: none of this is in a game download.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::html::{self, Doc, Element, Node};
use crate::hub::{Hub, now_ms};

/// How often the page files are checked.
pub const POLL: Duration = Duration::from_millis(250);
/// How long the page gets to report a swap.
pub const PATCH_WAIT: Duration = Duration::from_secs(3);
/// How many past page versions are kept (a page may be behind by this many saves).
const KEEP: usize = 32;

/// The page's own files at one moment: `index.html` and the text of every
/// stylesheet it links, by the `href` written in the page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageSource {
    pub html: String,
    pub sheets: BTreeMap<String, String>,
}

/// What a `<link>` is to the page hot-replace.
#[derive(Debug, Clone, PartialEq)]
enum LinkKind {
    /// A local stylesheet (plain, or Trunk's `css` / `inline type=css`).
    Sheet(String),
    /// A Trunk link the build consumes and leaves nothing in place of
    /// (`rust`, `copy-file`, `copy-dir`).
    Consumed,
    /// Any other Trunk asset: the bridge can't stand in for Trunk here.
    Unsupported,
    Other,
}

/// A page-relative file under the app folder (no `..`, no scheme, no `"`).
fn local_href(href: &str) -> Option<String> {
    let h = href.trim();
    if h.is_empty() || h.contains("://") || h.starts_with("//") || h.starts_with("data:") || h.contains(['"', '<', '>', '\\', '?', '#']) {
        return None;
    }
    let rel = h.trim_start_matches('/');
    let rel = rel.strip_prefix("./").unwrap_or(rel);
    Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| rel.to_string())
}

fn link_kind(e: &Element) -> LinkKind {
    if e.tag != "link" {
        return if e.has_attr("data-trunk") { LinkKind::Unsupported } else { LinkKind::Other };
    }
    let rel = e.attr("rel").unwrap_or("").to_ascii_lowercase();
    let href = e.attr("href").and_then(local_href);
    if e.has_attr("data-trunk") {
        let ty = e.attr("type").unwrap_or("").to_ascii_lowercase();
        return match rel.as_str() {
            "css" => href.map_or(LinkKind::Unsupported, LinkKind::Sheet),
            "inline" if ty == "css" => href.map_or(LinkKind::Unsupported, LinkKind::Sheet),
            "rust" | "copy-file" | "copy-dir" => LinkKind::Consumed,
            _ => LinkKind::Unsupported,
        };
    }
    match href {
        Some(h) if rel.split_ascii_whitespace().any(|r| r == "stylesheet") => LinkKind::Sheet(h),
        _ => LinkKind::Other,
    }
}

/// One `copy-dir` / `copy-file` link: where the build puts it, and where it is
/// in the app folder (both `/`-separated, relative).
#[derive(Debug, Clone, PartialEq)]
pub struct StaticLink {
    pub target: String,
    pub source: String,
    pub dir: bool,
}

/// The existing file under the app folder that a request for `url_path`
/// stands for, if one of `links` covers it.
pub fn resolve_static(links: &[StaticLink], app_dir: &Path, url_path: &str) -> Option<PathBuf> {
    let url = url_path.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
    let url = url.strip_prefix("./").unwrap_or(url);
    if url.is_empty() || !Path::new(url).components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    for link in links {
        let file = if link.dir {
            match url.strip_prefix(link.target.as_str()).and_then(|r| r.strip_prefix('/')) {
                Some(rest) if !rest.is_empty() => app_dir.join(&link.source).join(rest),
                _ => continue,
            }
        } else if url == link.target {
            app_dir.join(&link.source)
        } else {
            continue;
        };
        if file.is_file() {
            return Some(file);
        }
    }
    None
}

impl PageSource {
    /// Read `index.html` and its stylesheets from the app folder.
    pub fn load(app_dir: &Path) -> Result<Self, String> {
        let html = std::fs::read_to_string(app_dir.join("index.html")).map_err(|e| format!("index.html: {e}"))?;
        let doc = html::parse(&html)?;
        let mut sheets = BTreeMap::new();
        for e in doc.elements() {
            if let LinkKind::Sheet(href) = link_kind(e) {
                let text = std::fs::read_to_string(app_dir.join(&href)).map_err(|err| format!("{href}: {err}"))?;
                sheets.insert(href, text);
            }
        }
        Ok(Self { html, sheets })
    }

    /// The files and folders the page asks Trunk to copy into the build.
    pub fn static_links(&self) -> Vec<StaticLink> {
        let Ok(doc) = html::parse(&self.html) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in doc.elements() {
            if e.tag != "link" || !e.has_attr("data-trunk") {
                continue;
            }
            let dir = match e.attr("rel").unwrap_or("").to_ascii_lowercase().as_str() {
                "copy-dir" => true,
                "copy-file" => false,
                _ => continue,
            };
            let Some(source) = e.attr("href").and_then(local_href) else {
                continue;
            };
            let Some(name) = Path::new(&source).file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            // Trunk puts the file or folder, under its own name, inside
            // `data-target-path` (the build root when absent).
            let base = e.attr("data-target-path").and_then(local_href).unwrap_or_default();
            let base = base.trim_end_matches('/');
            let target = if base.is_empty() { name } else { format!("{base}/{name}") };
            out.push(StaticLink { target, source, dir });
        }
        out
    }

    /// The files to watch: `index.html` and every linked stylesheet.
    pub fn files(&self, app_dir: &Path) -> Vec<PathBuf> {
        let mut out = vec![app_dir.join("index.html")];
        out.extend(self.sheets.keys().map(|h| app_dir.join(h)));
        out
    }

    /// Names of the files that differ from `old` (`index.html`, `hud.css`).
    pub fn changed_files(&self, old: &PageSource) -> Vec<String> {
        let mut out = Vec::new();
        if self.html != old.html {
            out.push("index.html".to_string());
        }
        for (h, t) in &self.sheets {
            if old.sheets.get(h) != Some(t) {
                out.push(h.clone());
            }
        }
        out
    }
}

// ---- serving the page part ------------------------------------------------------------

fn find_ci(s: &str, pat: &str) -> Option<usize> {
    s.to_ascii_lowercase().find(&pat.to_ascii_lowercase())
}

/// The game build's own tags in its `index.html`: Trunk's `<script
/// type="module">` that starts the wasm and the preload links after it,
/// up to `</head>`; or, for a dx build (live code swapping, LIVE_API.md 10),
/// dx's self-starting `<script type="module" async src="/./wasm/<pkg>.js">`,
/// which dx puts at the end of `<body>`. That one is given back without
/// `async`, so in `<head>` it still runs only after the page is parsed.
pub fn build_tags(dist_index: &str) -> Option<String> {
    let head_end = find_ci(dist_index, "</head>")?;
    if let Some(start) = find_ci(&dist_index[..head_end], "<script type=\"module\">") {
        let tags = &dist_index[start..head_end];
        return tags.contains("init").then(|| tags.to_string());
    }
    const DX: &str = "<script type=\"module\" async src=\"";
    let start = find_ci(dist_index, DX)?;
    let rest = &dist_index[start + DX.len()..];
    let src = &rest[..rest.find('"')?];
    let ok = src.ends_with(".js") && src.contains("/wasm/") && !src.contains(['<', '>', '\'', ' ']);
    ok.then(|| format!("<script type=\"module\" src=\"{src}\"></script>\n"))
}

/// The page to serve at `/`: the current page source with each `<style>`
/// marked (`data-evani-style="style:N"`), each linked stylesheet inlined as a
/// marked `<style>` (`data-evani-style="<href>"`), Trunk's consumed links
/// dropped, and the build's own tags plus `<meta name="evani-page-rev">`
/// before `</head>`. `None` when the page uses Trunk features the bridge
/// can't stand in for (then the build's own `index.html` is served).
pub fn compose(src: &PageSource, dist_index: &str, rev_tag: &str) -> Option<String> {
    let tags = build_tags(dist_index)?;
    let tags = tags.as_str();
    let doc = html::parse(&src.html).ok()?;
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut n_style = 0;
    for e in doc.elements() {
        if e.tag == "style" {
            let at = e.open_end - 1;
            edits.push((at, at, format!(" data-evani-style=\"style:{n_style}\"")));
            n_style += 1;
            continue;
        }
        match link_kind(e) {
            LinkKind::Sheet(href) => {
                let css = src.sheets.get(&href)?;
                if find_ci(css, "</style").is_some() {
                    return None;
                }
                edits.push((e.start, e.end, format!("<style data-evani-style=\"{href}\">{css}</style>")));
            }
            LinkKind::Consumed => edits.push((e.start, e.end, String::new())),
            LinkKind::Unsupported => return None,
            LinkKind::Other => {}
        }
    }
    let head_end = find_ci(&src.html, "</head>")?;
    edits.push((
        head_end,
        head_end,
        format!("<meta name=\"evani-page-rev\" content=\"{rev_tag}\">\n{tags}"),
    ));
    edits.sort_by_key(|(s, e, _)| (*s, *e));
    let mut out = String::with_capacity(src.html.len() + tags.len() + 64 + src.sheets.values().map(String::len).sum::<usize>());
    let mut at = 0;
    for (s, e, text) in edits {
        if s < at {
            return None;
        }
        out.push_str(&src.html[at..s]);
        out.push_str(&text);
        at = e;
    }
    out.push_str(&src.html[at..]);
    Some(out)
}

// ---- what a change needs ------------------------------------------------------------

/// One in-place change for the page (sent in `page_patch`, 14.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Replace the text of `<style data-evani-style="<key>">`.
    Css { key: String, text: String },
    /// Replace the content of `#id`, if it is still exactly `old`.
    Html { id: String, old: String, html: String },
    /// Replace the `<title>` (its markup, entities decoded by the page).
    Title { html: String },
}

impl Op {
    pub fn to_json(&self) -> Value {
        match self {
            Op::Css { key, text } => json!({ "op": "css", "key": key, "text": text }),
            Op::Html { id, old, html } => json!({ "op": "html", "id": id, "old": old, "html": html }),
            Op::Title { html } => json!({ "op": "title", "html": html }),
        }
    }
}

/// What a change from one page source to another needs: in-place ops, or
/// a fresh start (with the reason in plain words).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub ops: Vec<Op>,
    pub reload: Option<String>,
}

impl Plan {
    fn reload(reason: impl Into<String>) -> Self {
        Self { ops: Vec::new(), reload: Some(reason.into()) }
    }

    /// "2 styles, 1 part" for the tab's toolbar line.
    pub fn summary(&self) -> String {
        let n = |f: fn(&Op) -> bool| self.ops.iter().filter(|o| f(o)).count();
        let (css, html, title) = (n(|o| matches!(o, Op::Css { .. })), n(|o| matches!(o, Op::Html { .. })), n(|o| matches!(o, Op::Title { .. })));
        let mut parts = Vec::new();
        let plural = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
        if css > 0 {
            parts.push(plural(css, "style", "styles"));
        }
        if html > 0 {
            parts.push(plural(html, "part", "parts"));
        }
        if title > 0 {
            parts.push("the title".to_string());
        }
        if parts.is_empty() { "nothing visible".to_string() } else { parts.join(", ") }
    }
}

/// Elements whose content may be swapped: an `id`, no `id` anywhere inside
/// (the game's code finds its parts by id), not raw text, and content in
/// the page file (an empty element is one the code fills).
fn swappable(doc: &Doc, src: &str) -> HashSet<String> {
    doc.elements()
        .into_iter()
        .filter(|e| {
            !matches!(e.tag.as_str(), "html" | "head" | "body" | "script" | "style" | "title" | "textarea" | "template")
                && !e.has_id_below()
                && !e.inner(src).trim().is_empty()
        })
        .filter_map(|e| e.attr("id").map(String::from))
        .collect()
}

/// The page with the swappable parts left out: two sources with the same
/// skeleton differ only in what `page_patch` can swap. Comments are left
/// out, runs of white space count as one space, attributes are compared as
/// written.
fn skeleton_of(e: &Element, frags: &HashSet<String>, out: &mut String) {
    for n in &e.children {
        match n {
            Node::Comment => {}
            Node::Text(t) => {
                // Runs of white space are one space, also across a left-out comment.
                for c in t.chars() {
                    if c.is_ascii_whitespace() {
                        if !out.ends_with(' ') {
                            out.push(' ');
                        }
                    } else {
                        out.push(c);
                    }
                }
            }
            Node::Element(c) => {
                out.push('<');
                out.push_str(&c.tag);
                for (k, v) in &c.attrs {
                    out.push(' ');
                    out.push_str(k);
                    if let Some(v) = v {
                        out.push_str("=\"");
                        out.push_str(v);
                        out.push('"');
                    }
                }
                out.push('>');
                if c.tag == "style" {
                    out.push_str("{style}");
                } else if c.tag == "title" {
                    out.push_str("{title}");
                } else if c.attr("id").is_some_and(|id| frags.contains(id)) {
                    out.push_str("{part}");
                } else {
                    skeleton_of(c, frags, out);
                }
                out.push_str("</");
                out.push_str(&c.tag);
                out.push('>');
            }
        }
    }
}

fn skeleton(e: &Element, frags: &HashSet<String>) -> String {
    let mut s = String::new();
    skeleton_of(e, frags, &mut s);
    s
}

fn ids(doc: &Doc) -> Vec<&str> {
    doc.elements().into_iter().filter_map(|e| e.attr("id")).collect()
}

fn by_id<'a>(doc: &'a Doc, id: &str) -> Option<&'a Element> {
    doc.find(|e| e.attr("id") == Some(id))
}

/// Why two skeletons differ, in plain words.
fn reload_reason(old: &Doc, new: &Doc, frags: &HashSet<String>) -> String {
    let (a, b) = (ids(old), ids(new));
    if a != b {
        let gone: Vec<&str> = a.iter().filter(|i| !b.contains(i)).copied().collect();
        let added: Vec<&str> = b.iter().filter(|i| !a.contains(i)).copied().collect();
        let list = |v: &[&str]| v.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ");
        return match (gone.is_empty(), added.is_empty()) {
            (false, true) => format!("the page lost {} (the game's code finds its parts by id)", list(&gone)),
            (true, false) => format!("the page has new ids {} (the game's code finds its parts by id)", list(&added)),
            (false, false) => format!("ids changed: {} became {} (the game's code finds its parts by id)", list(&gone), list(&added)),
            (true, true) => "the order of the page's ids changed (the game's code finds its parts by id)".to_string(),
        };
    }
    let scripts = |d: &Doc| {
        d.elements()
            .into_iter()
            .filter(|e| e.tag == "script" || e.has_attr("data-trunk"))
            .map(|e| skeleton(&Element { children: Vec::new(), ..e.clone() }, frags) + &format!("{:?}", e.attrs))
            .collect::<Vec<_>>()
    };
    if scripts(old) != scripts(new) {
        return "the page's scripts or build links changed".to_string();
    }
    // The smallest element with an id whose skeleton changed.
    let mut best: Option<(usize, &str)> = None;
    for id in &a {
        if let (Some(x), Some(y)) = (by_id(old, id), by_id(new, id)) {
            let (sx, sy) = (skeleton(x, frags), skeleton(y, frags));
            let open_same = x.attrs == y.attrs && x.tag == y.tag;
            if (sx != sy || !open_same) && best.is_none_or(|(n, _)| sx.len() < n) {
                best = Some((sx.len(), id));
            }
        }
    }
    match best {
        Some((_, id)) if !frags.contains(id) && by_id(old, id).is_some_and(|e| e.children.iter().all(|n| matches!(n, Node::Text(t) if t.trim().is_empty()))) => {
            format!("#{id} is filled by the game's code, so its markup can't be swapped")
        }
        Some((_, id)) => format!("the markup of #{id} changed outside a part that can be swapped"),
        None => "the page's markup changed outside the parts that can be swapped".to_string(),
    }
}

/// What it takes to go from `old` to `new` in a running page.
pub fn plan(old: &PageSource, new: &PageSource) -> Plan {
    let (Ok(od), Ok(nd)) = (html::parse(&old.html), html::parse(&new.html)) else {
        return Plan::reload("the page file couldn't be read as HTML");
    };
    let frags = swappable(&od, &old.html);
    if skeleton(&od.root, &frags) != skeleton(&nd.root, &frags) {
        return Plan::reload(reload_reason(&od, &nd, &frags));
    }
    let mut ops = Vec::new();
    // <style> blocks, in page order (the same count: the skeletons match).
    let styles = |d: &Doc| d.elements().into_iter().filter(|e| e.tag == "style").map(|e| e.span()).collect::<Vec<_>>();
    for (i, (a, b)) in styles(&od).into_iter().zip(styles(&nd)).enumerate() {
        let (ta, tb) = (&old.html[a.0..a.1], &new.html[b.0..b.1]);
        if ta != tb {
            ops.push(Op::Css { key: format!("style:{i}"), text: tb.to_string() });
        }
    }
    // Linked stylesheets.
    for (href, text) in &new.sheets {
        if old.sheets.get(href) != Some(text) {
            ops.push(Op::Css { key: href.clone(), text: text.clone() });
        }
    }
    // The title.
    let title = |d: &Doc, src: &str| d.find(|e| e.tag == "title").map(|e| e.inner(src).to_string());
    if let (Some(a), Some(b)) = (title(&od, &old.html), title(&nd, &new.html))
        && a != b
    {
        ops.push(Op::Title { html: b });
    }
    // Swappable parts, in page order.
    for e in od.elements() {
        let Some(id) = e.attr("id").filter(|id| frags.contains(*id)) else {
            continue;
        };
        let Some(n) = by_id(&nd, id) else { continue };
        let (a, b) = (e.inner(&old.html), n.inner(&new.html));
        if a != b {
            if find_ci(b, "<script").is_some() {
                return Plan::reload(format!("#{id} now has a script, which only runs on a fresh start"));
            }
            ops.push(Op::Html { id: id.to_string(), old: a.to_string(), html: b.to_string() });
        }
    }
    Plan { ops, reload: None }
}

trait Span {
    fn span(&self) -> (usize, usize);
}

impl Span for Element {
    /// The content's byte range.
    fn span(&self) -> (usize, usize) {
        (self.open_end, self.close_start)
    }
}

// ---- the watcher ------------------------------------------------------------------

type Stamp = Vec<(PathBuf, Option<(SystemTime, u64)>)>;

fn stamp_of(files: &[PathBuf]) -> Stamp {
    files
        .iter()
        .map(|f| {
            let m = std::fs::metadata(f).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
            (f.clone(), m)
        })
        .collect()
}

struct HotState {
    rev: u64,
    sources: BTreeMap<u64, PageSource>,
    stamp: Stamp,
    seq: u64,
}

/// The page hot-replace for one game: the page versions seen since the
/// bridge started (rev 0 = the page at start), tagged `<session>:<rev>`.
pub struct PageHot {
    pub app_dir: PathBuf,
    session: String,
    state: Mutex<HotState>,
}

/// What one page change led to (for the log and the tests).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// No page is connected; the next one loads the new files.
    NoPage,
    Swapped { rev: u64, ops: usize },
    NeedsReload { reason: String },
}

impl PageHot {
    /// `None` when the app has no readable `index.html`.
    pub fn new(app_dir: &Path, session: &str) -> Option<Self> {
        let src = PageSource::load(app_dir).ok()?;
        let stamp = stamp_of(&src.files(app_dir));
        Some(Self {
            app_dir: app_dir.to_path_buf(),
            session: session.to_string(),
            state: Mutex::new(HotState { rev: 0, sources: BTreeMap::from([(0, src)]), stamp, seq: 0 }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HotState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn tag(&self, rev: u64) -> String {
        format!("{}:{rev}", self.session)
    }

    /// The rev of a page's tag, if it is one of ours still kept.
    fn rev_of(&self, tag: &str) -> Option<u64> {
        let (s, r) = tag.rsplit_once(':')?;
        let r: u64 = r.parse().ok()?;
        (s == self.session && self.lock().sources.contains_key(&r)).then_some(r)
    }

    #[cfg(test)]
    pub fn rev(&self) -> u64 {
        self.lock().rev
    }

    pub fn source(&self, rev: u64) -> Option<PageSource> {
        self.lock().sources.get(&rev).cloned()
    }

    /// `/`: the current page composed with the build's tags, or the build's
    /// own page if it can't be composed.
    pub fn served(&self, dist_index: &str) -> String {
        let (rev, src) = {
            let g = self.lock();
            (g.rev, g.sources.get(&g.rev).cloned().unwrap_or_default())
        };
        compose(&src, dist_index, &self.tag(rev)).unwrap_or_else(|| dist_index.to_string())
    }

    /// The file on disk for `url_path` (`/models/x.glb`) when the page copies
    /// it into the build with Trunk's `copy-dir` / `copy-file`. dx has no such
    /// links, so its build folder lacks those files; the bridge serves them
    /// from the app folder instead (read fresh on every request).
    pub fn static_file(&self, url_path: &str) -> Option<PathBuf> {
        let src = {
            let g = self.lock();
            g.sources.get(&g.rev).cloned()?
        };
        resolve_static(&src.static_links(), &self.app_dir, url_path)
    }

    /// Look at the page files; a new rev when their content changed.
    pub fn poll(&self) -> Option<u64> {
        let stamp_now = {
            let g = self.lock();
            let files: Vec<PathBuf> = g.stamp.iter().map(|(f, _)| f.clone()).collect();
            let now = stamp_of(&files);
            if now == g.stamp {
                return None;
            }
            now
        };
        // Something was written: let the editor finish, then read it all.
        std::thread::sleep(Duration::from_millis(60));
        let Ok(src) = PageSource::load(&self.app_dir) else {
            // Half-written or missing: try again on the next poll.
            self.lock().stamp = stamp_now;
            return None;
        };
        let mut g = self.lock();
        g.stamp = stamp_of(&src.files(&self.app_dir));
        if g.sources.get(&g.rev) == Some(&src) {
            return None;
        }
        g.rev += 1;
        let rev = g.rev;
        g.sources.insert(rev, src);
        while g.sources.len() > KEEP {
            let Some(first) = g.sources.keys().next().copied() else { break };
            g.sources.remove(&first);
        }
        Some(rev)
    }

    /// Bring the connected page (if any) to `rev`, and tell the tab.
    pub async fn apply(&self, hub: &Hub, rev: u64) -> Outcome {
        let Some(to) = self.source(rev) else {
            return Outcome::NeedsReload { reason: "that page version is gone".into() };
        };
        let prev = self.source(rev.saturating_sub(1)).unwrap_or_default();
        let files = to.changed_files(&prev);
        let label = if files.is_empty() { "page files".to_string() } else { files.join(", ") };
        let Some(page_tag) = hub.page_rev() else {
            hub.dev_news(&json!({ "type": "live_status", "at": now_ms(), "state": "applied", "kind": "page", "files": files,
                "message": format!("Page files saved ({label}); the game page takes them when it opens") }));
            return Outcome::NoPage;
        };
        let base = page_tag.as_deref().and_then(|t| self.rev_of(t));
        let plan = match base.and_then(|b| self.source(b)) {
            Some(from) => plan(&from, &to),
            None => Plan::reload("the open page is from before this live server started"),
        };
        if let Some(reason) = plan.reload {
            self.needs_reload(hub, &files, &label, &reason);
            return Outcome::NeedsReload { reason };
        }
        let seq = {
            let mut g = self.lock();
            g.seq += 1;
            g.seq
        };
        let msg = json!({
            "type": "page_patch", "v": 1, "seq": seq,
            "from": page_tag, "to": self.tag(rev), "files": files,
            "ops": plan.ops.iter().map(Op::to_json).collect::<Vec<_>>(),
        });
        let reply = match hub.send_page_patch(seq, &msg) {
            Some(rx) => tokio::time::timeout(PATCH_WAIT, rx).await.ok().and_then(Result::ok),
            None => None,
        };
        let Some(report) = reply else {
            let reason = "the game page didn't answer the swap".to_string();
            self.needs_reload(hub, &files, &label, &reason);
            return Outcome::NeedsReload { reason };
        };
        if report["ok"] != true {
            let reason = report["error"].as_str().unwrap_or("the page couldn't swap it").to_string();
            self.needs_reload(hub, &files, &label, &reason);
            return Outcome::NeedsReload { reason };
        }
        hub.dev_news(&json!({ "type": "live_status", "at": now_ms(), "state": "applied", "kind": "page", "files": files,
            "message": format!("Swapped in place, no reload: {label} ({})", plan.summary()) }));
        hub.dev_news(&json!({ "type": "live_swapped", "at": now_ms(), "build": 0, "kind": "page", "files": files }));
        Outcome::Swapped { rev, ops: plan.ops.len() }
    }

    /// The tab's existing contract for "can't swap this in place": a
    /// `live_swap_failed` (the toolbar's Reload note) and a `page_changed`
    /// (the tab checks the new page in its hidden copy; also sent on the
    /// page's socket, where the tab's gate holds it until the check passes).
    fn needs_reload(&self, hub: &Hub, files: &[String], label: &str, reason: &str) {
        hub.dev_news(&json!({ "type": "live_swap_failed", "at": now_ms(), "build": 0, "kind": "page", "files": files,
            "message": "needs a fresh start", "reason": reason }));
        let changed = json!({ "type": "live_status", "at": now_ms(), "state": "page_changed", "kind": "page", "files": files,
            "message": format!("Page changed ({label}) but {reason}: reload to see it") });
        hub.dev_news(&changed);
        hub.to_engine(&changed);
    }
}

/// Watch the page files for as long as the bridge runs.
pub async fn run(hot: Arc<PageHot>, hub: Arc<Hub>, quiet: bool) {
    let mut tick = tokio::time::interval(POLL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let h = hot.clone();
        let Ok(Some(rev)) = tokio::task::spawn_blocking(move || h.poll()).await else {
            continue;
        };
        let out = hot.apply(&hub, rev).await;
        if !quiet {
            match out {
                Outcome::NoPage => eprintln!("evani-live-bridge: page files changed (rev {rev}); no page open"),
                Outcome::Swapped { ops, .. } => eprintln!("evani-live-bridge: page rev {rev} swapped in place ({ops} ops)"),
                Outcome::NeedsReload { reason } => eprintln!("evani-live-bridge: page rev {rev} needs a fresh start: {reason}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"<!DOCTYPE html>
<html><head><title>Kart</title>
  <style>body { margin: 0; }</style>
  <link data-trunk rel="inline" type="css" href="hud.css">
  <link data-trunk rel="rust" data-bin="game">
</head>
<body>
  <div id="ui">
    <div id="hud0" class="hud"></div>
    <div id="title" class="screen">
      <h1>Sugarpeak Rally</h1>
      <button data-cmd="start1">1 Player</button>
    </div>
    <div id="results"><h2 id="res-head"></h2><button data-cmd="restart">Race again</button></div>
  </div>
</body></html>"#;

    const DIST: &str = "<html><head><title>Kart</title>\n<script type=\"module\">\nimport init from '/game-1.js';\nawait init();\n</script>\n<link rel=\"modulepreload\" href=\"/game-1.js\"></head><body></body></html>";

    fn src(html: &str, css: &str) -> PageSource {
        PageSource { html: html.to_string(), sheets: BTreeMap::from([("hud.css".to_string(), css.to_string())]) }
    }

    const CSS: &str = ".hud .mid.wrong { color: #ff4d6d; }";

    #[test]
    fn copy_links_are_served_from_the_app_folder() {
        let html = r#"<html><head>
  <link data-trunk rel="copy-dir" href="assets/models">
  <link data-trunk rel="copy-file" href="data/rules.json" data-target-path="extra">
  <link data-trunk rel="copy-dir" href="../outside">
  <link data-trunk rel="rust" data-bin="game">
</head><body></body></html>"#;
        let links = src(html, CSS).static_links();
        assert_eq!(
            links,
            vec![
                StaticLink { target: "models".into(), source: "assets/models".into(), dir: true },
                StaticLink { target: "extra/rules.json".into(), source: "data/rules.json".into(), dir: false },
            ]
        );
        let app = std::env::temp_dir().join(format!("evani-static-{}", std::process::id()));
        std::fs::create_dir_all(app.join("assets/models/sub")).unwrap();
        std::fs::create_dir_all(app.join("data")).unwrap();
        std::fs::write(app.join("assets/models/a.glb"), b"a").unwrap();
        std::fs::write(app.join("assets/models/sub/b.glb"), b"b").unwrap();
        std::fs::write(app.join("data/rules.json"), b"{}").unwrap();
        let at = |u: &str| resolve_static(&links, &app, u);
        assert_eq!(at("/models/a.glb"), Some(app.join("assets/models/a.glb")));
        assert_eq!(at("/models/sub/b.glb?x=1"), Some(app.join("assets/models/sub/b.glb")));
        assert_eq!(at("/extra/rules.json"), Some(app.join("data/rules.json")));
        assert_eq!(at("/models/missing.glb"), None);
        assert_eq!(at("/models"), None);
        assert_eq!(at("/models/"), None);
        assert_eq!(at("/modelsx/a.glb"), None);
        assert_eq!(at("/models/../data/rules.json"), None);
        assert_eq!(at("/rules.json"), None);
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn hrefs_stay_inside_the_app_folder() {
        assert_eq!(local_href("hud.css").as_deref(), Some("hud.css"));
        assert_eq!(local_href("./css/hud.css").as_deref(), Some("css/hud.css"));
        assert_eq!(local_href("/hud.css").as_deref(), Some("hud.css"));
        for bad in ["../x.css", "https://cdn/x.css", "//cdn/x.css", "a/../../b.css", "x.css?v=1", "", "C:\\x.css"] {
            assert_eq!(local_href(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_served_page_inlines_and_marks_styles_and_keeps_the_build_tags() {
        let page = compose(&src(INDEX, CSS), DIST, "s-1:4").unwrap();
        assert!(page.contains("<style data-evani-style=\"style:0\">body { margin: 0; }</style>"), "{page}");
        assert!(page.contains(&format!("<style data-evani-style=\"hud.css\">{CSS}</style>")));
        assert!(!page.contains("data-trunk"), "Trunk's links are consumed: {page}");
        assert!(page.contains("<meta name=\"evani-page-rev\" content=\"s-1:4\">\n<script type=\"module\">\nimport init from '/game-1.js';"));
        assert!(page.contains("<link rel=\"modulepreload\" href=\"/game-1.js\"></head>"));
        assert!(page.contains("<div id=\"title\" class=\"screen\">"));
        // No build tags (a holding page) or an unknown Trunk asset: serve the build's page.
        assert_eq!(compose(&src(INDEX, CSS), "<html><head></head></html>", "t"), None);
        let icon = INDEX.replace("</head>", "<link data-trunk rel=\"icon\" href=\"i.png\"></head>");
        assert_eq!(compose(&src(&icon, CSS), DIST, "t"), None);
    }

    /// A dx build (live code swapping) puts its self-starting script at the
    /// end of `<body>`; the served page gets it in `<head>`, without `async`.
    #[test]
    fn a_dx_build_page_keeps_its_script() {
        let dx = format!(
            "{}\n<script type=\"module\" async src=\"/./wasm/evani-kart-racer.js\"></script>\n</body></html>",
            INDEX.replace("<title>Kart</title>", "<title>Kartdioxus | x</title>").replace("</body></html>", "")
        );
        let tag = "<script type=\"module\" src=\"/./wasm/evani-kart-racer.js\"></script>\n";
        assert_eq!(build_tags(&dx).as_deref(), Some(tag));
        let page = compose(&src(INDEX, CSS), &dx, "s-1:0").unwrap();
        assert!(page.contains(&format!("<meta name=\"evani-page-rev\" content=\"s-1:0\">\n{tag}</head>")), "{page}");
        assert!(page.contains("<title>Kart</title>") && !page.contains("dioxus"));
        assert!(!page.contains("async"));
        let odd = dx.replace("/./wasm/evani-kart-racer.js", "https://x/a.js");
        assert_eq!(build_tags(&odd), None);
    }

    #[test]
    fn a_stylesheet_edit_is_one_css_op() {
        let new_css = CSS.replace("#ff4d6d", "#00e0ff");
        let p = plan(&src(INDEX, CSS), &src(INDEX, &new_css));
        assert_eq!(p.reload, None);
        assert_eq!(p.ops, vec![Op::Css { key: "hud.css".into(), text: new_css }]);
        assert_eq!(p.summary(), "1 style");
    }

    #[test]
    fn inline_styles_title_and_static_text_swap_in_place() {
        let new = INDEX
            .replace("margin: 0;", "margin: 0; background: #123;")
            .replace("<title>Kart</title>", "<title>Kart &amp; co</title>")
            .replace("Sugarpeak Rally</h1>", "Sugarpeak Rally!</h1>");
        let p = plan(&src(INDEX, CSS), &src(&new, CSS));
        assert_eq!(p.reload, None, "{p:?}");
        assert_eq!(p.ops.len(), 3);
        assert_eq!(p.ops[0], Op::Css { key: "style:0".into(), text: "body { margin: 0; background: #123; }".into() });
        assert_eq!(p.ops[1], Op::Title { html: "Kart &amp; co".into() });
        let Op::Html { id, old, html } = &p.ops[2] else { panic!("{:?}", p.ops[2]) };
        assert_eq!(id, "title");
        assert!(old.contains("Sugarpeak Rally</h1>") && html.contains("Sugarpeak Rally!</h1>"));
        assert_eq!(p.summary(), "1 style, 1 part, the title");
        // Comments and re-indenting change nothing that needs a swap or a reload.
        let tidy = INDEX.replace("\n    <div id=\"hud0\"", "\n<!-- HUDs -->\n        <div id=\"hud0\"");
        let p = plan(&src(INDEX, CSS), &src(&tidy, CSS));
        assert_eq!((p.reload, p.ops.len()), (None, 0));
    }

    #[test]
    fn structure_the_code_binds_to_needs_a_fresh_start() {
        let old = src(INDEX, CSS);
        let cases = [
            (INDEX.replace("id=\"hud0\"", "id=\"hud-0\""), "#hud0"),
            (INDEX.replace("<h2 id=\"res-head\"></h2>", ""), "lost #res-head"),
            // Filled by the code (empty in the file).
            (INDEX.replace("<div id=\"hud0\" class=\"hud\"></div>", "<div id=\"hud0\" class=\"hud\"><b>x</b></div>"), "#hud0"),
            // Markup in an element that holds other ids.
            (INDEX.replace("Race again", "Again!"), "#results"),
            // The element's own attributes.
            (INDEX.replace("id=\"title\" class=\"screen\"", "id=\"title\" class=\"screen big\""), "#title"),
            (INDEX.replace("</body>", "<script>alert(1)</script></body>"), "scripts"),
            (INDEX.replace("<h1>Sugarpeak Rally</h1>", "<h1>Hi</h1><script>x()</script>"), "script"),
        ];
        for (new, why) in cases {
            let p = plan(&old, &src(&new, CSS));
            let reason = p.reload.unwrap_or_else(|| panic!("expected a reload for {why}: {:?}", p.ops));
            assert!(reason.contains(why), "{why}: {reason}");
            assert!(p.ops.is_empty());
        }
    }

    /// The watcher: a saved stylesheet is a new rev; the same bytes again aren't.
    #[test]
    fn the_watcher_sees_saved_files_and_ignores_unchanged_ones() {
        let dir = std::env::temp_dir().join(format!("evani-page-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), INDEX).unwrap();
        std::fs::write(dir.join("hud.css"), CSS).unwrap();
        let hot = PageHot::new(&dir, "s-t").unwrap();
        assert_eq!((hot.rev(), hot.poll()), (0, None));
        assert_eq!(hot.tag(0), "s-t:0");
        let served = hot.served(DIST);
        assert!(served.contains("content=\"s-t:0\"") && served.contains(CSS));
        // Same content rewritten (new mtime): no new rev.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("hud.css"), format!("{CSS}\n")).unwrap();
        assert_eq!(hot.poll(), Some(1));
        std::fs::write(dir.join("hud.css"), format!("{CSS}\n")).unwrap();
        let _ = std::fs::OpenOptions::new().append(true).open(dir.join("hud.css"));
        assert_eq!(hot.poll(), None);
        std::fs::write(dir.join("hud.css"), CSS.replace("#ff4d6d", "#0ff")).unwrap();
        assert_eq!(hot.poll(), Some(2));
        assert_eq!(hot.source(2).unwrap().changed_files(&hot.source(1).unwrap()), ["hud.css"]);
        assert!(hot.served(DIST).contains("content=\"s-t:2\"") && hot.served(DIST).contains("#0ff"));
        assert_eq!(hot.rev_of("s-t:1"), Some(1));
        assert_eq!((hot.rev_of("s-x:1"), hot.rev_of("s-t:99"), hot.rev_of("nope")), (None, None, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn hub() -> Arc<Hub> {
        Arc::new(Hub::new(crate::hub::HubInfo { port: 1, root: "r".into(), game: "kart-racer".into(), session: "s".into() }))
    }

    /// The whole loop with a fake page: the patch goes to the page, its
    /// report becomes the tab's news; a structural change asks for a reload.
    #[tokio::test]
    async fn changes_reach_the_page_and_the_tab() {
        let dir = std::env::temp_dir().join(format!("evani-page-apply-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), INDEX).unwrap();
        std::fs::write(dir.join("hud.css"), CSS).unwrap();
        let hot = Arc::new(PageHot::new(&dir, "s-a").unwrap());
        let hub = hub();
        let (dev_tx, mut dev) = tokio::sync::mpsc::unbounded_channel();
        hub.attach_dev(dev_tx);
        let next = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>| serde_json::from_str::<Value>(&rx.try_recv().unwrap()).unwrap();

        // No page yet: news only.
        std::fs::write(dir.join("hud.css"), CSS.replace("#ff4d6d", "#111")).unwrap();
        let rev = hot.poll().unwrap();
        assert_eq!(hot.apply(&hub, rev).await, Outcome::NoPage);
        assert_eq!(next(&mut dev)["state"], "applied");

        // A page at rev 0 connects; the same change goes to it as one op.
        let (eng_tx, mut eng) = tokio::sync::mpsc::unbounded_channel();
        let id = hub.attach_engine(eng_tx);
        hub.on_engine_message(id, r#"{"type":"hello","v":1,"page_rev":"s-a:0"}"#);
        let (h2, hot2) = (hub.clone(), hot.clone());
        let task = tokio::spawn(async move { hot2.apply(&h2, rev).await });
        let msg = loop {
            if let Ok(m) = eng.try_recv() {
                break serde_json::from_str::<Value>(&m).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!((msg["type"].as_str(), msg["from"].as_str(), msg["to"].as_str()), (Some("page_patch"), Some("s-a:0"), Some("s-a:1")));
        assert_eq!(msg["ops"][0], json!({ "op": "css", "key": "hud.css", "text": CSS.replace("#ff4d6d", "#111") }));
        let seq = msg["seq"].as_u64().unwrap();
        hub.on_engine_message(id, &json!({ "type": "event", "v": 1, "event": "page.patched", "data": { "seq": seq, "ok": true, "rev": "s-a:1", "applied": 1 } }).to_string());
        assert_eq!(task.await.unwrap(), Outcome::Swapped { rev: 1, ops: 1 });
        assert_eq!(hub.page_rev(), Some(Some("s-a:1".to_string())));
        let news = next(&mut dev);
        assert_eq!((news["state"].as_str(), news["kind"].as_str()), (Some("applied"), Some("page")));
        assert!(news["message"].as_str().unwrap().contains("no reload: hud.css (1 style)"), "{news}");
        assert_eq!(next(&mut dev)["type"], "live_swapped");

        // A structural change: the tab's Reload note and page_changed (also on the page's socket).
        std::fs::write(dir.join("index.html"), INDEX.replace("id=\"hud0\"", "id=\"hudA\"")).unwrap();
        let rev = hot.poll().unwrap();
        let out = hot.apply(&hub, rev).await;
        assert!(matches!(&out, Outcome::NeedsReload { reason } if reason.contains("#hud0")), "{out:?}");
        let failed = next(&mut dev);
        assert_eq!((failed["type"].as_str(), failed["build"].as_u64(), failed["message"].as_str()), (Some("live_swap_failed"), Some(0), Some("needs a fresh start")));
        assert_eq!(next(&mut dev)["state"], "page_changed");
        let on_page = serde_json::from_str::<Value>(&eng.try_recv().unwrap()).unwrap();
        assert_eq!(on_page["state"], "page_changed");

        // Undo it: from the page's rev 1 the change is nothing at all, swapped (the note clears).
        std::fs::write(dir.join("index.html"), INDEX).unwrap();
        let rev = hot.poll().unwrap();
        let (h2, hot2) = (hub.clone(), hot.clone());
        let task = tokio::spawn(async move { hot2.apply(&h2, rev).await });
        let msg = loop {
            if let Ok(m) = eng.try_recv() {
                break serde_json::from_str::<Value>(&m).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(msg["ops"], json!([]));
        hub.on_engine_message(id, &json!({ "type": "event", "v": 1, "event": "page.patched", "data": { "seq": msg["seq"], "ok": true, "rev": msg["to"] } }).to_string());
        assert_eq!(task.await.unwrap(), Outcome::Swapped { rev: 3, ops: 0 });

        // The page refuses (the code changed that part since it loaded): a reload is needed.
        std::fs::write(dir.join("index.html"), INDEX.replace("1 Player", "One player")).unwrap();
        let rev = hot.poll().unwrap();
        let (h2, hot2) = (hub.clone(), hot.clone());
        let task = tokio::spawn(async move { hot2.apply(&h2, rev).await });
        let msg = loop {
            if let Ok(m) = eng.try_recv() {
                break serde_json::from_str::<Value>(&m).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(msg["ops"][0]["op"], "html");
        hub.on_engine_message(id, &json!({ "type": "event", "v": 1, "event": "page.patched", "data": { "seq": msg["seq"], "ok": false, "error": "#title was changed by the game since the page loaded" } }).to_string());
        assert!(matches!(task.await.unwrap(), Outcome::NeedsReload { reason } if reason.contains("#title")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
