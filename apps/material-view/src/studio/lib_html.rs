//! The studio page's Library section, as markup (pure, so it is tested
//! natively; `proc_page.rs` puts it in the left column). The rows are the
//! approved assets evani.app serves ([`lib_data`]); a row whose asset this
//! window has opens it in the live view, with the item's own panel on the
//! right, the same as a Procedural card.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::lib_data::{self as ld, Carried, Contents, Row, Shown, Source, State};
use super::page_view::esc;
use super::proc_data::ProcLib;

/// A reference picture as the page has it: checked (with its object URL),
/// or what went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pic {
    Good(String),
    Mismatch,
}

/// What the section shows besides the list itself.
#[derive(Debug, Clone, Default)]
pub struct LibView<'a> {
    /// The list hasn't answered yet (the first fetch, or Refresh).
    pub loading: bool,
    /// The open procedural item.
    pub open: &'a str,
    /// Pictures by their SHA-256, as checked so far.
    pub pics: Option<&'a HashMap<String, Pic>>,
    /// The live view's card pictures by item (the placeholder otherwise).
    pub thumbs: Option<&'a HashMap<String, String>>,
}

fn style_name(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect::<String>().replace('-', " "))
}

fn score(x: f64) -> String {
    format!("{x:.1}")
}

/// The line under an asset's name: who improved it and its score.
pub fn by_line(a: &ld::Asset) -> String {
    match (a.state, &a.author, a.server_score) {
        (State::Base, ..) => "The engine's own version".into(),
        (_, Some(who), Some(s)) => format!("Approved version · improved by {who} · {}", score(s)),
        (_, None, Some(s)) => format!("Approved version · {}", score(s)),
        _ => "Approved version".into(),
    }
}

/// The row's state note, if it has one.
pub fn state_note(r: &Row) -> Option<String> {
    let a = &r.asset;
    Some(match a.state {
        State::Arriving => "Update on its way: it joins with the next library build".into(),
        State::HeldBack => format!("Held back: {}", a.why.as_deref().unwrap_or("the library build couldn't take it")),
        State::InLibrary if r.carried == Carried::Other => "This window's build has another version of it".into(),
        State::InLibrary => return a.in_library_since.as_deref().map(|t| format!("In the library since {}", ld::when(t))),
        State::Base => return None,
    })
}

fn categories(c: &[f64; 5]) -> String {
    let mut h = String::from(r#"<span class="lb-cats" aria-label="Scores by category">"#);
    for ((_, label), v) in ld::SCORE_CATEGORIES.iter().zip(c) {
        let _ = write!(
            h,
            r#"<span class="lb-cat" title="{label}: {} of 5"><span class="nm">{label}</span><i><i style="width:{:.0}%"></i></i><b>{}</b></span>"#,
            score(*v),
            v / 5.0 * 100.0,
            score(*v)
        );
    }
    h.push_str("</span>");
    h
}

/// One asset's row.
pub fn row_html(r: &Row, lib: &ProcLib, v: &LibView) -> String {
    let a = &r.asset;
    let icon = r.item.as_deref().and_then(|i| lib.items.iter().find(|x| x.id == i)).map_or("◆", |i| i.icon.as_str());
    let pic = a.reference.as_ref().and_then(|p| v.pics.and_then(|m| m.get(&p.sha256)));
    let thumb = r.item.as_deref().and_then(|i| v.thumbs.and_then(|t| t.get(i)));
    let picture = match (pic, thumb) {
        (Some(Pic::Good(url)), _) => format!(r#"<img alt="" src="{}">"#, esc(url)),
        (_, Some(src)) => format!(r#"<img alt="" src="{}">"#, esc(src)),
        _ => format!(r#"<span class="pc-ph">{}</span>"#, esc(icon)),
    };
    let mut tx = format!(r#"<b class="nm">{}</b><span class="nm">{} · {}</span>"#, esc(&a.name), esc(&style_name(&a.style)), esc(&style_name(&a.category)));
    match a.version_ref() {
        Some(vr) => {
            let _ = write!(tx, r#"<code class="lb-ver" title="This version's name">{}</code>"#, esc(&vr));
        }
        None => {
            let _ = write!(tx, r#"<code class="lb-ver" title="No approved version yet">{}</code>"#, esc(&a.id));
        }
    }
    let _ = write!(tx, r#"<span class="lb-by">{}</span>"#, esc(&by_line(a)));
    if let Some(c) = &a.score_categories {
        tx.push_str(&categories(c));
    }
    if let Some(n) = state_note(r) {
        let _ = write!(tx, r#"<em class="lb-st lb-{}">{}</em>"#, match a.state {
            State::Arriving => "arr",
            State::HeldBack => "held",
            _ => "in",
        }, esc(&n));
    }
    if pic == Some(&Pic::Mismatch) {
        tx.push_str(r#"<em class="lb-st lb-held">Its picture didn't match its check, so it isn't shown</em>"#);
    }
    let title = format!("{} ({})", a.name, a.version_ref().unwrap_or_else(|| a.id.clone()));
    match &r.item {
        Some(item) => format!(
            r#"<button class="lb-row{}" data-act="lib-open" data-v="{}" data-ref="{}" aria-pressed="{}" title="Open {} in the view"><span class="pc-pic lb-pic">{picture}</span><span class="lb-tx">{tx}</span></button>"#,
            if item == v.open { " on" } else { "" },
            esc(item),
            esc(&a.id),
            item == v.open,
            esc(&title)
        ),
        None => format!(
            r#"<div class="lb-row lb-off" data-ref="{}" title="{}"><span class="pc-pic lb-pic">{picture}</span><span class="lb-tx">{tx}<em class="lb-st">Not in this window yet: a newer library build brings it</em></span></div>"#,
            esc(&a.id),
            esc(&title)
        ),
    }
}

/// The whole section: its head (Refresh), where the list comes from, the
/// rows, what was left out and why, and this window's build.
pub fn section_html(shown: Option<&Shown>, contents: Option<&Contents>, lib: &ProcLib, v: &LibView) -> String {
    let mut h = String::from(
        r#"<div class="lb-head"><h3>Evani Library</h3><button class="lb-rf" data-act="lib-refresh" title="Ask Evani.App for the latest library list">Refresh</button></div>"#,
    );
    let Some(shown) = shown.filter(|_| !v.loading) else {
        h.push_str(r#"<p class="lb-note" role="status">Loading the library list…</p>"#);
        return h;
    };
    match shown {
        Shown::Nothing { missing: true, .. } => h.push_str(
            r#"<p class="lb-note" role="status">No library list yet. Evani.App downloads it from evani.app in the background and keeps it on this computer; press Refresh in a moment.</p>"#,
        ),
        Shown::Nothing { why, .. } => {
            let _ = write!(h, r#"<p class="lb-note lb-warn" role="status">The library list couldn't be used: {}. Nothing is shown from it; Refresh tries again.</p>"#, esc(why));
        }
        Shown::List { listing, source, saved_at } => {
            let _ = write!(
                h,
                r#"<p class="lb-meta">{} · rev {} · {}</p>"#,
                if listing.stage == "test" { "Test library" } else { "From evani.app" },
                listing.rev,
                esc(&ld::when(&listing.published_at))
            );
            let from = saved_at.as_deref().map(|t| format!(" (from {})", ld::when(t))).unwrap_or_default();
            match source {
                Source::Fresh => {}
                Source::AppSaved => {
                    let why = match listing.cache.as_ref().map(|c| c.state.as_str()) {
                        Some("offline") => "Evani.App can't reach evani.app",
                        Some("server-error") => "evani.app didn't answer properly",
                        _ => "the latest library couldn't be used",
                    };
                    let _ = write!(h, r#"<p class="lb-note lb-saved" role="status">Saved copy: {why}, so this is the list saved on this computer{from}. Refresh tries again.</p>"#);
                }
                Source::WindowSaved { why } => {
                    let _ = write!(h, r#"<p class="lb-note lb-saved" role="status">Saved copy{from}: the latest list didn't load ({}). Refresh tries again.</p>"#, esc(why));
                }
            }
            let rows = ld::rows(listing, contents, lib);
            if rows.is_empty() {
                h.push_str(r#"<p class="lb-note">evani.app has no approved assets in the library yet.</p>"#);
            } else {
                let here = rows.iter().filter(|r| r.item.is_some()).count();
                let approved = ld::approved_count(&rows);
                let _ = write!(h, r#"<p class="lb-count">{approved} approved asset{}, {here} open here</p><div class="lb-rows">"#, if approved == 1 { "" } else { "s" });
                for r in &rows {
                    h.push_str(&row_html(r, lib, v));
                }
                h.push_str("</div>");
            }
            if !listing.skipped.is_empty() {
                let n = listing.skipped.len();
                let _ = write!(h, r#"<details class="lb-skip"><summary>{n} left out of the list</summary><ul>"#);
                for s in &listing.skipped {
                    let _ = write!(h, r#"<li><b>{}</b>: {}</li>"#, esc(&s.label), esc(&s.why));
                }
                h.push_str("</ul></details>");
            }
        }
    }
    match contents {
        Some(c) => {
            let at = c.built_at.as_deref().map(|t| format!(", {}", ld::when(t))).unwrap_or_default();
            let _ = write!(h, r#"<p class="lb-build">This window: library build {}{}</p>"#, esc(&c.build), esc(&at));
        }
        None => h.push_str(r#"<p class="lb-build">This window is a developer build: it shows its own versions of the assets.</p>"#),
    }
    h
}

/// The section's styles (added with the procedural mode's).
pub const CSS: &str = r#"
.lb-head{display:flex;align-items:center;justify-content:space-between;gap:6px;min-width:0}
.lb-head h3{margin:0;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.lb-rf{flex:none;min-height:30px!important;padding:0 10px!important;font-size:12px}
.lb-meta,.lb-count,.lb-build{margin:6px 0;color:var(--dim);font-size:11.5px;line-height:1.4;overflow-wrap:anywhere}
.lb-note{margin:8px 0;padding:8px 9px;border:1px solid var(--ln);border-radius:8px;font-size:12px;line-height:1.45;overflow-wrap:anywhere}
.lb-saved,.lb-warn{border-color:rgba(233,185,73,.55);background:rgba(233,185,73,.1)}
.lb-rows{display:flex;flex-direction:column;gap:8px}
.lb-row{display:flex!important;flex-direction:column;align-items:stretch;gap:6px;width:100%;padding:6px!important;text-align:left;min-width:0;box-sizing:border-box;border:1px solid var(--ln);border-radius:10px;background:rgba(255,255,255,.04)}
.lb-row.on{border-color:var(--acc);background:rgba(233,185,73,.14)}
.lb-off{opacity:.62}
.lb-tx{display:flex;flex-direction:column;gap:2px;min-width:0;padding:0 2px 2px}
.lb-tx b{font-size:13.5px}
.lb-tx>span.nm{color:var(--dim);font-size:11.5px}
.lb-ver{display:block;font:10.5px/1.35 ui-monospace,Consolas,monospace;color:var(--dim);overflow-wrap:anywhere;word-break:break-all;white-space:normal}
.lb-by{font-size:11.5px;line-height:1.35;overflow-wrap:anywhere}
.lb-cats{display:flex;flex-direction:column;gap:1px;margin-top:3px}
.lb-cat{display:grid;grid-template-columns:minmax(0,1fr) minmax(24px,40%) 2.2em;align-items:center;gap:5px;font-size:10.5px;color:var(--dim)}
.lb-cat>i{display:block;height:5px;border-radius:3px;background:rgba(255,255,255,.12);overflow:hidden}
.lb-cat>i>i{display:block;height:100%;background:var(--acc)}
.lb-cat b{font-weight:600;text-align:right;color:inherit}
#studio-page em.lb-st{position:static;display:block;font-style:normal;font-size:11px;line-height:1.35;padding:2px 0 0;background:none;border-radius:0;color:var(--dim);white-space:normal;overflow:visible;text-overflow:clip}
#studio-page em.lb-arr{color:#9fd3ff}
#studio-page em.lb-held{color:#ffb347}
.lb-skip{margin:10px 0 4px;font-size:12px;color:var(--dim)}
.lb-skip summary{cursor:pointer}
.lb-skip ul{margin:6px 0 0;padding-left:16px;line-height:1.45}
.lb-skip li{overflow-wrap:anywhere}
/* No picture: a short placeholder, not a tall empty box. */
.lb-pic:has(>.pc-ph){aspect-ratio:16/5}
/* A wide column (a phone-width window's bottom sheet): the picture beside
   the words, so several rows fit. */
@container (min-width:420px){.lb-row{flex-direction:row;align-items:flex-start}.lb-row>.lb-pic{flex:none;width:clamp(120px,30%,200px)}.lb-row>.lb-tx{flex:1}}
/* Narrow windows: the Procedural cards scroll sideways in one strip; the
   Library's list scrolls down and keeps its heading. */
#studio-page.proc.narrow #st-plist:has(.lb-head){overflow-x:hidden;overflow-y:auto}
#studio-page.proc.narrow #st-plist .lb-head h3{display:block}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::lib_data::{Fetched, PictureCheck, Saved, read_contents, resolve};

    const VALID: &str = include_str!("fixtures/library/valid.json");
    const BAD_HASH: &str = include_str!("fixtures/library/bad-hash.json");
    const CONTENTS: &str = include_str!("fixtures/library/library-contents.json");

    use crate::studio::lib_data::tests::lib;

    fn page(text: Fetched, saved: Option<&Saved>, contents: Option<&Contents>, v: &LibView) -> String {
        let (shown, _) = resolve(&text, saved);
        section_html(Some(&shown), contents, lib(), v)
    }

    /// Every listed asset shows its name, style, version name and score;
    /// the radar's categories when the website has them; the state words.
    #[test]
    fn the_list_shows_each_assets_facts() {
        let c = read_contents(CONTENTS).unwrap();
        let h = page(Fetched::Text(VALID.into()), None, Some(&c), &LibView { open: "rocks-pebbles", ..Default::default() });
        assert!(h.contains("From evani.app · rev 8 · 9 Nov 2026, 10:42 UTC"), "{h}");
        assert!(h.contains("3 approved assets"), "the base asset isn't counted: {h}");
        for want in [
            "Pebbles",
            "Photoreal · Rocks",
            "rocks-pebbles@photoreal#3f9a0c1e2b4d",
            "Approved version · improved by @pebblemaker · 4.0",
            "In the library since 9 Nov 2026, 10:42 UTC",
            "Update on its way: it joins with the next library build",
            "Held back: its change clashes with Rocks: cobble; the builder will try again",
            "The engine's own version",
            "This window: library build a41c700dcf58, 9 Nov 2026, 10:30 UTC",
        ] {
            assert!(h.contains(&esc(want)), "missing `{want}`:\n{h}");
        }
        for want in [r#"title="Silhouette: 4.0 of 5""#, r#"data-act="lib-refresh""#] {
            assert!(h.contains(want), "missing `{want}`:\n{h}");
        }
        #[cfg(feature = "procedural")]
        {
            assert!(h.contains(r#"<button class="lb-row on" data-act="lib-open" data-v="rocks-pebbles""#), "{h}");
            assert!(h.contains(r#"data-v="terrain-rolling-hills""#));
        }
        // A developer build says so.
        let dev = page(Fetched::Text(VALID.into()), None, None, &LibView::default());
        assert!(dev.contains("This window is a developer build"));
    }

    /// A base asset has no approved version, so the count leaves it out.
    #[test]
    fn the_count_covers_only_approved_assets() {
        let mixed = page(Fetched::Text(include_str!("fixtures/library/mixed.json").into()), None, None, &LibView::default());
        assert!(mixed.contains("1 approved asset, "), "{mixed}");
        assert!(!mixed.contains("2 approved assets"), "{mixed}");
    }

    /// A picture opens only once it matched its hash; one that didn't is
    /// never shown and its row says so.
    #[test]
    fn pictures_show_only_when_they_match() {
        let sha = "5e2b7c0d9a8f6e4d3c2b1a0f9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e0d";
        assert_eq!(ld::check_picture(sha, Some(sha)), PictureCheck::Good);
        let mut pics = HashMap::new();
        pics.insert(sha.to_string(), Pic::Good("blob:pebbles".into()));
        let ok = page(Fetched::Text(VALID.into()), None, None, &LibView { pics: Some(&pics), ..Default::default() });
        assert!(ok.contains(r#"<img alt="" src="blob:pebbles">"#));
        pics.insert(sha.to_string(), Pic::Mismatch);
        let bad = page(Fetched::Text(VALID.into()), None, None, &LibView { pics: Some(&pics), ..Default::default() });
        assert!(!bad.contains("blob:pebbles"));
        assert!(bad.contains("Its picture didn't match its check, so it isn't shown"), "{bad}");
    }

    /// The plain messages: loading, no list yet, a broken list, a saved
    /// copy, nothing approved yet, and what was left out and why.
    #[test]
    fn the_plain_messages() {
        let loading = section_html(None, None, lib(), &LibView::default());
        assert!(loading.contains("Loading the library list…"));
        let none = page(Fetched::Missing, None, None, &LibView::default());
        assert!(none.contains("No library list yet. Evani.App downloads it from evani.app"), "{none}");
        let broken = page(Fetched::Text("{".into()), None, None, &LibView::default());
        assert!(broken.contains("The library list couldn't be used: the list isn&#39;t readable (not JSON)."), "{broken}");
        let saved = Saved { text: VALID.into(), saved_at: "2026-11-09T12:00:00Z".into() };
        let cached = page(Fetched::Failed("the window couldn't reach Evani.App".into()), Some(&saved), None, &LibView::default());
        assert!(cached.contains("Saved copy (from 9 Nov 2026, 12:00 UTC): the latest list didn't load"), "{cached}");
        assert!(cached.contains("Pebbles"), "the saved list still shows");
        let offline = page(Fetched::Text(include_str!("fixtures/library/offline.json").into()), None, None, &LibView::default());
        assert!(offline.contains("Saved copy: Evani.App can't reach evani.app, so this is the list saved on this computer (from 9 Nov 2026, 09:15 UTC)."), "{offline}");
        let empty = page(Fetched::Text(include_str!("fixtures/library/empty.json").into()), None, None, &LibView::default());
        assert!(empty.contains("Test library · rev 1") && empty.contains("evani.app has no approved assets in the library yet."), "{empty}");
        let skipped = page(Fetched::Text(BAD_HASH.into()), None, None, &LibView::default());
        assert!(skipped.contains("4 left out of the list"), "{skipped}");
        assert!(skipped.contains("<b>Short grass</b>: its content hash didn&#39;t pass its check"));
        assert!(skipped.contains("1 approved asset, "));
        let missing = read_listing_with("rocks-unicorn");
        assert!(missing.contains("lb-row lb-off") && missing.contains("Not in this window yet"), "{missing}");
    }

    fn read_listing_with(asset: &str) -> String {
        page(Fetched::Text(VALID.replace("rocks-pebbles", asset)), None, None, &LibView::default())
    }

    /// Long names never push the column wider: the version wraps, the rest
    /// shortens with "..." or wraps.
    #[test]
    fn long_words_wrap_or_shorten() {
        let rule = |sel: &str| -> &str {
            let at = CSS.find(&format!("\n{sel}{{")).unwrap_or_else(|| panic!("no `{sel}` rule"));
            let r = &CSS[at + 1..];
            &r[..r.find('}').unwrap()]
        };
        assert!(rule(".lb-ver").contains("word-break:break-all"));
        assert!(rule(".lb-row").contains("min-width:0"));
        assert!(rule(".lb-note").contains("overflow-wrap:anywhere"));
        assert!(rule(".lb-head h3").contains("text-overflow:ellipsis"));
        // Guard (shot at 700 px: the narrow layout's sideways-only column
        // left every row after the first out of reach): the list scrolls down.
        assert!(rule("#studio-page.proc.narrow #st-plist:has(.lb-head)").contains("overflow-y:auto"));
        assert!(CSS.contains("#studio-page.proc.narrow #st-plist .lb-head h3{display:block}"));
    }
}
