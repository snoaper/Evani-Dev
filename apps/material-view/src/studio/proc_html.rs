//! The studio page's procedural mode, as markup (pure, so it is tested
//! natively; `proc_page.rs` puts it on the page).
//!
//! The mode switch ("Materials" | "Procedural") sits at the top of the page.
//! In procedural mode the asset browser, the style row, the materials bar
//! and its categories, the inspector, the side column (combination,
//! presets), the variations strip and the narrow dock's tabs are hidden
//! ([`panels`]); shown instead are a column of effect cards on the left
//! (each with a picture taken from the live view), the big live view in the
//! middle and a narrow panel on the right: the item's headline sliders, its
//! presets, an Advanced fold with every other param, a "changed" marker and
//! reset on each edited slider, and a "Use in a game" fold with the exact
//! evani.toml module line and the param names.

use std::fmt::Write as _;

use evani_core::params::{ParamRegistry, ParamValue};

use super::page_view::esc;
use super::proc_data::{self as pd, Item, ProcLib};
use super::proc_view::ProcState;

/// The page parts and whether each shows in materials or procedural mode.
pub const MATERIALS_ONLY: [&str; 9] = ["st-styles", "st-mats", "st-left", "st-insp", "st-side", "st-vars", "st-tabs", "st-note", "st-morepop"];
pub const PROCEDURAL_ONLY: [&str; 2] = ["st-plist", "st-ppanel"];

/// Each part's id and whether it shows in this mode (`hidden` is set from
/// it; the mode switch always shows). `st-morepop` and `st-note` are only
/// ever hidden here: their own code shows them when they have something.
pub fn panels(proc: bool) -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = MATERIALS_ONLY.iter().map(|id| (*id, !proc)).collect();
    out.extend(PROCEDURAL_ONLY.iter().map(|id| (*id, proc)));
    out
}

/// The mode switch. `lib`: the Library section (the approved assets from
/// evani.app) is open, which draws in the procedural scene too.
pub fn mode_html(proc: bool, lib: bool) -> String {
    let b = |v: &str, label: &str, tip: &str, on: bool| {
        format!(r#"<button class="st-modeb{}" role="tab" aria-selected="{on}" data-act="mode" data-v="{v}" title="{tip}">{label}</button>"#, if on { " on" } else { "" })
    };
    let lib = proc && lib;
    format!(
        r#"{}{}{}<span class="st-mode-tx">{}</span>"#,
        b("mat", "Materials", "Assets, art styles and catalogue materials", !proc),
        b("proc", "Procedural", "Effects drawn from code: water, glass, sky", proc && !lib),
        b("lib", "Library", "The Evani Library: the approved assets from evani.app", lib),
        if lib {
            "Approved assets from evani.app, kept on this computer"
        } else if proc {
            "Effects the engine draws from code, no textures to download"
        } else {
            ""
        }
    )
}

/// The effect cards: the built items, then the planned ones greyed as
/// "coming soon". `pic(id)` is a picture taken from the live view, if any.
pub fn list_html(lib: &ProcLib, st: &ProcState, pic: &dyn Fn(&str) -> Option<String>) -> String {
    list_html_with(lib, st, pic, None)
}

/// [`list_html`] with each library item's switch read from `reg` (on when
/// no registry is given).
pub fn list_html_with(lib: &ProcLib, st: &ProcState, pic: &dyn Fn(&str) -> Option<String>, reg: Option<&ParamRegistry>) -> String {
    let card = |h: &mut String, it: &Item| {
        let on = it.id == st.item;
        let preset = st.preset.get(&it.id).and_then(|p| it.preset(p)).map_or("", |p| p.name.as_str());
        let picture = pic(&it.id).map_or_else(|| format!(r#"<span class="pc-ph pc-{0}">{1}</span>"#, esc(&it.stage_as), esc(&it.icon)), |src| format!(r#"<img alt="" src="{}">"#, esc(&src)));
        // A stand-in says so on its picture (its switch still reads On).
        let picture = if super::proc_data::is_standin(it) { format!(r#"{picture}<span class="pc-stub" title="A simple shape from its name until its generator is built">Stand-in, not built yet</span>"#) } else { picture };
        let card = format!(
            r#"<button class="pc-card{}" data-act="proc-item" data-v="{}" aria-pressed="{on}" title="{}"><span class="pc-pic">{picture}</span><span class="pc-tx"><b class="nm">{}</b><span class="nm">{}</span></span></button>"#,
            if on { " on" } else { "" },
            esc(&it.id),
            esc(&it.about),
            esc(&it.name),
            esc(preset)
        );
        match &it.on_param {
            Some(p) => {
                let live = reg.is_none_or(|r| super::proc_view::is_on(r, Some(p)));
                let _ = write!(
                    h,
                    r#"<div class="pc-it{}">{card}<button class="pc-sw" role="switch" data-act="proc-on" data-v="{}" aria-checked="{live}" title="{} ({p})">{}</button></div>"#,
                    if live { "" } else { " off" },
                    esc(&it.id),
                    if live { "On: shown. Turn it off to remove it" } else { "Off: removed. Turn it back on" },
                    if live { "On" } else { "Off" }
                );
            }
            None => h.push_str(&card),
        }
    };
    let mut h = String::from(r#"<h3>Procedural</h3>"#);
    if lib.categories.is_empty() {
        h.push_str(r#"<div class="pc-cards">"#);
        for it in lib.ready() {
            card(&mut h, it);
        }
        h.push_str("</div>");
    } else {
        // The library's categories, each a fold with its count; the open
        // item's category open.
        let open_cat = st.open_item().map(|i| i.category.clone()).unwrap_or_default();
        for (cid, name) in &lib.categories {
            let items: Vec<&Item> = lib.in_category(cid).collect();
            if items.is_empty() {
                continue;
            }
            let built = items.iter().filter(|i| i.icon != "◇").count();
            let _ = write!(
                h,
                r#"<details class="pc-cat" data-cat="{0}"{1}><summary>{2}<small>{3} · {4} built</small></summary><div class="pc-cards">"#,
                esc(cid),
                if *cid == open_cat { " open" } else { "" },
                esc(name),
                items.len(),
                built
            );
            for it in items {
                card(&mut h, it);
            }
            h.push_str("</div></details>");
        }
        let rest: Vec<&Item> = lib.ready().filter(|i| i.category.is_empty()).collect();
        if !rest.is_empty() {
            h.push_str(r#"<details class="pc-cat" data-cat="other" open><summary>Other effects</summary><div class="pc-cards">"#);
            for it in rest {
                card(&mut h, it);
            }
            h.push_str("</div></details>");
        }
    }
    // Planned effects the library does not already list.
    let listed = |a: &Option<String>| a.as_ref().is_some_and(|a| lib.ready().any(|i| i.asset.as_ref() == Some(a)));
    let coming: Vec<&Item> = lib.items.iter().filter(|i| !i.ready && !listed(&i.asset)).collect();
    if !coming.is_empty() {
        h.push_str(r#"<h4 class="pc-soon-h">Coming soon</h4><div class="pc-soon">"#);
        for it in coming {
            let _ = write!(h, r#"<span class="pc-soon-i" title="{}"><i>{}</i><span class="nm">{}</span></span>"#, esc(&it.about), esc(&it.icon), esc(&it.name));
        }
        h.push_str("</div>");
    }
    h
}

/// One slider of the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The live param's name (`data-v`).
    pub name: String,
    pub label: String,
    pub tip: String,
    pub kind: RowKind,
    /// Differs from the open preset's value.
    pub changed: bool,
    /// The lowest quality level it shows on ("High and Ultra"), when not
    /// every level.
    pub level: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Num { min: f32, max: f32, step: f32, value: f32, unit: String },
    Colour { hex: String },
}

/// "wave_strength" → "Wave strength", "deep_color" → "Deep colour".
pub fn label(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    let mut words = last.replace('_', " ").replace("color", "colour");
    if name.starts_with("material.nature.sky.") {
        words = format!("reflected sky {words}");
    }
    let mut c = words.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// The rows for `names` at their current values, marked against `base`.
pub fn rows(reg: &ParamRegistry, names: &[String], base: &[(String, ParamValue)]) -> Vec<Row> {
    names
        .iter()
        .filter_map(|n| {
            let id = reg.id(n)?;
            let def = reg.def(id)?;
            let now = reg.value(id)?;
            let kind = match now {
                ParamValue::F32(v) => RowKind::Num { min: def.min, max: def.max, step: def.step.unwrap_or((def.max - def.min) / 100.0).max(1e-6), value: v, unit: def.unit.clone() },
                ParamValue::Color(c) => RowKind::Colour { hex: pd::to_hex(c) },
                _ => return None,
            };
            let changed = base.iter().find(|(k, _)| k == n).is_some_and(|(_, b)| pd::changed(def.ty, def.step, now, *b));
            Some(Row { name: n.clone(), label: label(n), tip: format!("{} ({n})", def.description), kind, changed, level: None })
        })
        .collect()
}

/// [`rows`] with each row's quality note from the item ("High and Ultra").
pub fn rows_for(reg: &ParamRegistry, item: &Item, names: &[String], base: &[(String, ParamValue)]) -> Vec<Row> {
    let mut out = rows(reg, names, base);
    for r in &mut out {
        r.level = item.level_note(&r.name);
    }
    out
}

/// The item's quality setting now (the switch's value), if it has one.
pub fn quality_now(item: &Item, reg: &ParamRegistry) -> Option<f32> {
    let n = item.quality.as_deref()?;
    match reg.id(n).and_then(|id| reg.value(id)) {
        Some(ParamValue::F32(x)) => Some(x),
        _ => None,
    }
}

/// The Ultra / High / Medium / Low switch (and Auto: by the device).
fn quality_html(h: &mut String, item: &Item, reg: &ParamRegistry) {
    let Some(now) = quality_now(item, reg) else { return };
    h.push_str(r#"<section class="st-sec"><h3>Quality</h3><div class="pc-quality" role="group" aria-label="Quality level">"#);
    for (name, v) in item.quality_buttons() {
        let on = (now - v).abs() < 0.5;
        let tip = if v < 0.0 { "Auto: by the device (Lite and Low show Low, Medium shows Medium, High shows High, Ultra shows Ultra)".to_string() } else { format!("Preview the {name} level") };
        let name = esc(&name);
        let _ = write!(h, r#"<button class="pc-q{}" data-act="proc-quality" data-v="{v}" aria-pressed="{on}" title="{tip}">{name}</button>"#, if on { " on" } else { "" });
    }
    h.push_str(r#"</div><p class="pc-note">Settings marked with a level show only on that level and up.</p></section>"#);
}

/// A number as the slider's readout.
pub fn readout(v: f32, step: f32) -> String {
    let places = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else if step >= 0.01 {
        2
    } else if step >= 0.001 {
        3
    } else {
        5
    };
    format!("{v:.places$}")
}

fn row_html(h: &mut String, r: &Row) {
    let reset = format!(r#"<button class="pc-rst" data-act="proc-reset" data-v="{0}" title="Back to the preset's value" aria-label="Reset {1}">↺</button>"#, esc(&r.name), esc(&r.label));
    let chg = if r.changed { " chg" } else { "" };
    let lv = r.level.map_or_else(String::new, |l| format!(r#" <em class="pc-lv">{l}</em>"#));
    match &r.kind {
        RowKind::Num { min, max, step, value, unit } => {
            let _ = write!(
                h,
                r#"<div class="st-row pc-row{chg}" data-row="{0}"><div class="lab"><span title="{1}">{2}{lv}</span><i class="pc-dot" title="Changed from the preset"></i>{reset}<b data-out="{0}">{3}{4}</b></div><input type="range" data-act="proc-num" data-v="{0}" min="{min}" max="{max}" step="{step}" value="{value}" aria-label="{2}"></div>"#,
                esc(&r.name),
                esc(&r.tip),
                esc(&r.label),
                readout(*value, *step),
                if unit.is_empty() || unit == "x" { String::new() } else { format!(" {}", esc(unit)) }
            );
        }
        RowKind::Colour { hex } => {
            let _ = write!(
                h,
                r#"<div class="st-row pc-row{chg}" data-row="{0}"><div class="lab"><span title="{1}">{2}{lv}</span><i class="pc-dot" title="Changed from the preset"></i>{reset}<b data-out="{0}">{3}</b></div><input type="color" class="pc-col" data-act="proc-col" data-v="{0}" value="{3}" aria-label="{2}"></div>"#,
                esc(&r.name),
                esc(&r.tip),
                esc(&r.label),
                esc(hex)
            );
        }
    }
}

/// What a game writes to use the item as it is now: the module line and
/// each param with its value.
pub fn game_text(item: &Item, reg: &ParamRegistry) -> String {
    let mut t = format!("# evani.toml\n[modules]\n{}\n\n# Live params ({}; set them in a kit's data or with param_set)\n", item.module, item.crate_name);
    for (n, v) in pd::read(reg, &item.params()) {
        let _ = writeln!(t, "{n} = {}", pd::toml_value(v));
    }
    let _ = write!(t, "\n# Surface: {}", item.surface);
    if let Some(q) = &item.quality {
        let fixed: Vec<String> = item.quality_buttons().into_iter().filter(|(_, v)| *v >= 0.0).map(|(n, v)| format!("{v} {n}")).collect();
        let _ = write!(t, "\n# Quality: Ultra, High, Medium or Low by the device's tier ({q} = -1), or fixed: {}", fixed.join(", "));
        for n in item.params() {
            if let Some(l) = item.level_note(&n) {
                let _ = write!(t, "\n#   {n}: {l}");
            }
        }
    }
    t
}

/// The page's own state for the panel's folds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcUi {
    pub advanced: bool,
    pub game: bool,
}

/// The right panel for the open item.
pub fn panel_html(st: &ProcState, reg: &ParamRegistry, ui: &ProcUi) -> String {
    let Some(it) = st.open_item() else { return r#"<p class="st-none">No effect is open.</p>"#.into() };
    let base = st.baseline(reg);
    let head = rows_for(reg, it, &it.headline, &base);
    let adv = rows_for(reg, it, &it.advanced, &base);
    let mut h = format!(
        r#"<header class="pc-head"><span class="pc-ic">{}</span><span class="pc-hx"><b>{}</b><span>{}</span></span></header><section class="st-sec"><h3>Settings<button class="st-small" data-act="proc-reset-all" title="Every setting back to the preset">Reset all</button></h3>"#,
        esc(&it.icon),
        esc(&it.name),
        esc(&it.about)
    );
    if let Some(r) = it.asset.as_deref().and_then(pd::reference_of) {
        reference_html(&mut h, &r);
    }
    if super::proc_data::is_standin(it) {
        h.push_str(r#"<p class="pc-stubrow"><span class="pc-stub">Stand-in, not built yet</span> A simple shape from its name, until its generator is built.</p>"#);
    }
    if let Some(p) = &it.on_param {
        let live = super::proc_view::is_on(reg, Some(p));
        let _ = write!(
            h,
            r#"<div class="st-row pc-onrow"><div class="lab"><span title="{p}">Shown in the view</span><button class="pc-sw" role="switch" data-act="proc-on" data-v="{}" aria-checked="{live}">{}</button></div><p class="pc-note">Art style: Cinematic Realism (the other styles fall back to it for now).</p></div>"#,
            esc(&it.id),
            if live { "On" } else { "Off" }
        );
    }
    for r in &head {
        row_html(&mut h, r);
    }
    h.push_str("</section>");
    quality_html(&mut h, it, reg);
    h.push_str(r#"<section class="st-sec"><h3>Presets</h3><div class="pc-presets">"#);
    let open = st.open_preset();
    for p in &it.presets {
        let on = open == Some(p.id.as_str());
        let _ = write!(h, r#"<button class="pc-preset{}" data-act="proc-preset" data-v="{}" aria-pressed="{on}">{}</button>"#, if on { " on" } else { "" }, esc(&p.id), esc(&p.name));
    }
    h.push_str("</div></section>");
    let _ = write!(h, r#"<details class="pc-fold"{}><summary data-act="proc-adv">Advanced <small>{} more</small></summary>"#, if ui.advanced { " open" } else { "" }, adv.len());
    for r in &adv {
        row_html(&mut h, r);
    }
    h.push_str("</details>");
    let _ = write!(
        h,
        r#"<details class="pc-fold"{}><summary data-act="proc-game">Use in a game</summary><p class="pc-note">Add the module line to the game's evani.toml; the params are live, so a kit's data, the Look Lab or an AI agent set them by these names.</p><pre class="pc-code" data-game="1">{}</pre><button class="st-small pc-copy" data-act="proc-copy">Copy</button></details>"#,
        if ui.game { " open" } else { "" },
        esc(&game_text(it, reg))
    );
    h
}

/// The realism reference: the target picture the asset is tuned towards
/// (next to its live render in the view), its score and note.
fn reference_html(h: &mut String, r: &pd::Reference) {
    // Half stars as written (4.5/5); `{:.0}` rounded them half to even (4.5 read 4/5).
    let score = r.score.map_or_else(|| "not scored yet".to_string(), |s| format!("{s}/5"));
    let _ = write!(
        h,
        r#"<figure class="pc-ref"><img alt="Reference target picture" loading="lazy" src="{}"><figcaption><b>Reference target</b><span class="pc-score">{} · {}</span>{}</figcaption></figure>"#,
        esc(&r.image),
        esc(&score),
        esc(&r.status),
        if r.note.is_empty() { String::new() } else { format!("<span>{}</span>", esc(&r.note)) }
    );
}

/// The page's procedural styles (added after page.rs's own).
pub const CSS: &str = r#"
#studio-page [hidden]{display:none!important}
#st-mode{display:flex;align-items:center;gap:4px;padding:4px;pointer-events:auto;align-self:flex-start;max-width:100%}
.st-modeb{min-height:30px!important;padding:0 14px!important;border-radius:8px!important;font-weight:600}
.st-mode-tx{color:var(--dim);font-size:12px;padding:0 8px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
#studio-page.proc{grid-template-columns:clamp(200px,16vw,380px) minmax(0,1fr) clamp(260px,20vw,440px);grid-template-rows:auto minmax(0,1fr);
 grid-template-areas:"top top top" "plist view ppanel"}
/* The notice line (a copy's message) floats over the top of the view. */
#studio-page.proc #st-note{grid-area:view;align-self:start;z-index:2;margin-top:6px}
#st-plist{grid-area:plist;overflow-y:auto;overscroll-behavior:contain;padding:10px;container-type:inline-size}
#st-ppanel{grid-area:ppanel;overflow-y:auto;overscroll-behavior:contain;padding:10px 12px;container-type:inline-size;scrollbar-width:thin}
.pc-cards{display:flex;flex-direction:column;gap:8px}
.pc-card{display:flex!important;flex-direction:column;align-items:stretch;gap:6px;padding:6px!important;text-align:left;min-width:0}
.pc-pic{display:block;width:100%;aspect-ratio:16/10;border-radius:8px;overflow:hidden;background:#222}
.pc-pic img{display:block;width:100%;height:100%;object-fit:cover}
.pc-pic{position:relative}
.pc-stub{display:inline-block;padding:2px 7px;border-radius:9px;background:rgba(240,170,60,.92);color:#1a1004;font-size:10.5px;font-weight:700;line-height:15px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:calc(100% - 12px)}
.pc-pic>.pc-stub{position:absolute;left:6px;bottom:6px}
.pc-stubrow{margin:0 0 8px;font-size:12px;color:var(--dim);line-height:1.5}
.pc-stubrow .pc-stub{margin-right:4px}
.pc-ph{display:flex;width:100%;height:100%;align-items:center;justify-content:center;font-size:30px;color:rgba(255,255,255,.85);text-shadow:0 1px 3px rgba(0,0,0,.5)}
.pc-ocean{background:linear-gradient(#7fb0dc 0,#c9e0f0 46%,#1f5d74 50%,#0d3a4c 100%)}
.pc-water{background:linear-gradient(#9cc4e2 0,#d7e8f2 40%,#4c7a3a 42%,#3b6b5c 56%,#22463e 100%)}
.pc-glass{background:repeating-conic-gradient(#2a2826 0 25%,#9c9890 0 50%) 50%/24px 24px}
.pc-sky{background:linear-gradient(#3d78c4 0,#8fbbe6 55%,#dbe9f4 80%,#4f7a32 81%,#3a5d25 100%)}
.pc-sea{background:linear-gradient(#6d9fd6 0,#c4dcef 44%,#2c6e88 47%,#123f5a 70%,#0a2c40 100%)}
.pc-quality{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:4px}
@container (max-width:330px){.pc-quality{grid-template-columns:repeat(3,minmax(0,1fr))}}
.pc-q{min-height:30px!important;padding:0 2px!important;font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
/* A level note sits in the label's line (the page's em is a tile badge, absolute). */
#studio-page em.pc-lv{position:static;display:inline;font-style:normal;font-size:10.5px;line-height:inherit;background:none;color:var(--dim);border:1px solid var(--ln);border-radius:6px;padding:0 4px;margin-left:4px;white-space:nowrap;overflow:visible}
.pc-tx{display:flex;flex-direction:column;gap:1px;min-width:0;padding:0 2px 2px}
.pc-tx b{font-size:13.5px}
.pc-tx span{color:var(--dim);font-size:11.5px;min-height:1.3em}
.pc-soon-h{margin:14px 0 6px;font-size:11px;font-weight:650;letter-spacing:.06em;text-transform:uppercase;color:var(--dim)}
.pc-soon{display:flex;flex-direction:column;gap:3px}
.pc-soon-i{display:flex;align-items:center;gap:8px;padding:4px 6px;border-radius:8px;color:var(--dim);font-size:12px;opacity:.75;min-width:0}
.pc-soon-i i{flex:none;width:20px;text-align:center;font-style:normal}
.pc-head{display:flex;gap:10px;align-items:flex-start;padding-bottom:10px;margin-bottom:10px;border-bottom:1px solid var(--ln)}
.pc-ic{flex:none;width:36px;height:36px;border-radius:10px;background:rgba(233,185,73,.16);color:var(--acc);display:flex;align-items:center;justify-content:center;font-size:20px}
.pc-hx{display:flex;flex-direction:column;gap:2px;min-width:0}
.pc-hx b{font-size:16px}
.pc-hx span{color:var(--dim);font-size:12px;line-height:1.35}
.pc-ref{margin:0 0 10px;padding-bottom:10px;border-bottom:1px solid var(--ln)}
.pc-ref img{display:block;width:100%;aspect-ratio:16/10;object-fit:cover;border-radius:8px;background:#222}
.pc-ref figcaption{display:flex;flex-wrap:wrap;align-items:baseline;gap:2px 8px;margin-top:6px;font-size:12px;color:var(--dim);line-height:1.4;min-width:0}
.pc-ref figcaption b{color:inherit;font-size:11.5px;font-weight:650;letter-spacing:.06em;text-transform:uppercase}
.pc-ref .pc-score{color:var(--acc);font-weight:600}
.pc-ref figcaption span:last-child:not(.pc-score){flex-basis:100%}
.pc-row .lab{align-items:center}
.pc-row .lab span{flex:1;min-width:0}
.pc-dot{flex:none;width:7px;height:7px;border-radius:50%;background:var(--acc);visibility:hidden}
.pc-rst{flex:none;min-height:22px!important;height:22px;width:24px;padding:0!important;font-size:13px;visibility:hidden}
.pc-row.chg .pc-dot,.pc-row.chg .pc-rst{visibility:visible}
.pc-row.chg .lab span{color:var(--acc)}
.pc-col{width:100%;height:30px;padding:0 2px;border:1px solid var(--ln);border-radius:8px;background:rgba(255,255,255,.06);margin-top:4px}
.pc-presets{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:6px}
@container (min-width:380px){.pc-presets{grid-template-columns:repeat(3,minmax(0,1fr))}}
.pc-preset{min-height:34px!important;font-size:12.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.pc-fold{border-top:1px solid var(--ln);padding:8px 0}
.pc-fold summary{cursor:pointer;font-size:11.5px;font-weight:650;letter-spacing:.06em;text-transform:uppercase;padding:4px 0;list-style-position:inside}
.pc-fold summary small{color:var(--dim);font-weight:500;letter-spacing:0;text-transform:none;margin-left:6px}
.pc-note{color:var(--dim);font-size:12px;margin:6px 0}
.pc-code{margin:0 0 8px;padding:8px;border-radius:8px;background:rgba(0,0,0,.35);border:1px solid var(--ln);font:11.5px/1.45 ui-monospace,Consolas,monospace;white-space:pre-wrap;word-break:break-all;max-height:260px;overflow:auto}
/* Mid: the same three columns, narrower. */
#studio-page.proc.mid{grid-template-columns:clamp(180px,20vw,240px) minmax(0,1fr) clamp(250px,30vw,320px);grid-template-areas:"top top top" "plist view ppanel"}
/* Narrow: the view on top, the cards in a row, the panel under them. */
#studio-page.proc.narrow{grid-template-columns:minmax(0,1fr);grid-template-rows:auto minmax(170px,1fr) auto minmax(0,46%);grid-template-areas:"top" "view" "plist" "ppanel"}
#studio-page.proc.narrow #st-plist{overflow-x:auto;overflow-y:hidden;padding:6px;scrollbar-width:thin}
#studio-page.proc.narrow #st-plist h3,#studio-page.proc.narrow .pc-soon-h,#studio-page.proc.narrow .pc-soon{display:none}
#studio-page.proc.narrow .pc-cards{flex-direction:row}
#studio-page.proc.narrow .pc-card{flex:0 0 132px}
#studio-page.proc.narrow .st-mode-tx{display:none}
#studio-page.proc.short .pc-pic{aspect-ratio:16/7}
.pc-cat{border-top:1px solid var(--ln);padding:4px 0}
.pc-cat>summary{cursor:pointer;display:flex;align-items:baseline;gap:6px;padding:6px 2px;font-size:12.5px;font-weight:650;list-style-position:inside;min-width:0}
.pc-cat>summary small{margin-left:auto;color:var(--dim);font-weight:500;font-size:11px;white-space:nowrap}
.pc-cat .pc-cards{padding:2px 0 6px}
.pc-it{position:relative;display:flex;flex-direction:column}
.pc-it.off .pc-card{opacity:.5}
.pc-it>.pc-sw{position:absolute;top:10px;right:10px}
.pc-sw{min-height:22px!important;height:22px;padding:0 8px!important;border-radius:11px!important;font-size:11px;font-weight:650;background:rgba(80,200,120,.85)!important;color:#08140c!important;border:none!important}
.pc-sw[aria-checked="false"]{background:rgba(255,255,255,.16)!important;color:var(--dim)!important}
.pc-onrow .lab{display:flex;align-items:center;gap:8px}
.pc-onrow .lab span{flex:1}
.pc-library{background:linear-gradient(#4a4a4e 0,#2b2b2e 60%,#5c5a56 61%,#46443f 100%)}
.pc-grass{background:linear-gradient(#8fbbe6 0,#cfe3f2 45%,#5d8a2c 46%,#3b6020 100%)}
#studio-page.proc.narrow .pc-cat{display:contents}
#studio-page.proc.narrow .pc-cat>summary{display:none}
#studio-page.proc.narrow .pc-it{flex:0 0 132px}
#studio-page.proc.narrow .pc-pic>.pc-stub{white-space:normal;line-height:12px;padding:3px 7px;right:6px;max-width:none}
@media (min-width:1900px){.pc-tx b{font-size:15px}.pc-hx b{font-size:18px}}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::proc_view::{PROC_SCENE, ProcCommand};
    use evani_kit_material_view::{Command, Viewer};

    const DT: f64 = 1000.0 / 60.0;

    /// Switching mode hides the asset browser, the style row, the material
    /// bars, the inspector, the side column, the variations and the dock's
    /// tabs, and shows the effect cards and the effect panel (and back).
    #[test]
    fn switching_mode_hides_and_shows_the_right_panels() {
        let mat = panels(false);
        let proc = panels(true);
        for id in ["st-styles", "st-mats", "st-left", "st-insp", "st-side", "st-vars", "st-tabs"] {
            assert_eq!(mat.iter().find(|(i, _)| *i == id), Some(&(id, true)), "{id} shows with materials");
            assert_eq!(proc.iter().find(|(i, _)| *i == id), Some(&(id, false)), "{id} hides in procedural");
        }
        for id in ["st-plist", "st-ppanel"] {
            assert_eq!(mat.iter().find(|(i, _)| *i == id), Some(&(id, false)));
            assert_eq!(proc.iter().find(|(i, _)| *i == id), Some(&(id, true)));
        }
        // The switch says which mode is on, and goes both ways.
        assert!(mode_html(true, false).contains(r#"aria-selected="true" data-act="mode" data-v="proc""#));
        assert!(mode_html(false, false).contains(r#"aria-selected="true" data-act="mode" data-v="mat""#));
        assert!(mode_html(false, true).contains(r#"aria-selected="true" data-act="mode" data-v="mat""#), "the library draws in the procedural scene only");
        let lib = mode_html(true, true);
        assert!(lib.contains(r#"aria-selected="true" data-act="mode" data-v="lib""#) && lib.contains(r#"aria-selected="false" data-act="mode" data-v="proc""#), "{lib}");
        // The page's actions for it.
        use super::super::page_view::action;
        assert!(action("mode", "proc", "").is_none(), "the mode switch is the procedural page's own action");
        // The procedural grid has no place for the materials parts, and the
        // hidden attribute wins over each part's own display.
        assert!(CSS.contains("#studio-page [hidden]{display:none!important}"));
        assert!(CSS.contains(r#"grid-template-areas:"top top top" "plist view ppanel""#));
    }

    /// The panel's sliders are the item's live params: each slider names a
    /// registered param, moves with it, and the changed marker and the
    /// "Use in a game" text follow.
    #[test]
    fn the_panel_shows_live_params() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        app.world.res_mut::<Viewer>().push(Command::Scene(PROC_SCENE.into()));
        app.run_headless(2, DT);
        let lib = app.world.res::<ProcState>().lib.clone();
        // Every staged item and three library assets (one staging, one panel).
        for it in lib.ready().filter(|i| i.stage_as != "library").chain(lib.ready().filter(|i| i.stage_as == "library").take(3)) {
            app.world.res_mut::<ProcState>().push(ProcCommand::Open(it.id.clone()));
            app.run_headless(2, DT);
            let st = app.world.res::<ProcState>().clone();
            let reg = app.world.res::<ParamRegistry>();
            let html = panel_html(&st, &reg, &ProcUi::default());
            for n in it.headline.iter().chain(&it.advanced) {
                assert!(reg.id(n).is_some(), "{n}");
                assert!(html.contains(&format!(r#"data-v="{n}""#)), "{}: no slider for {n}", it.id);
            }
            assert!(html.contains(&esc(&it.module)), "{}: the module line", it.id);
            assert!(rows(&reg, &it.headline, &st.baseline(&reg)).iter().all(|r| !r.changed), "{}: fresh preset, nothing changed", it.id);
            for p in &it.presets {
                assert!(html.contains(&format!(r#"data-act="proc-preset" data-v="{}""#, p.id)));
            }
            drop(reg);
            // Move the first headline slider: it reads as changed.
            let n = it.headline[0].clone();
            let id = app.world.res::<ParamRegistry>().id(&n).unwrap();
            let def = app.world.res::<ParamRegistry>().def(id).unwrap().clone();
            let v = match def.default {
                ParamValue::F32(_) => ParamValue::F32(def.max),
                _ => ParamValue::Color([1.0, 0.0, 1.0]),
            };
            app.world.res_mut::<ParamRegistry>().set(id, v).unwrap();
            let reg = app.world.res::<ParamRegistry>();
            let r = rows(&reg, std::slice::from_ref(&n), &st.baseline(&reg));
            assert!(r[0].changed, "{n} moved");
            assert!(game_text(it, &reg).contains(&format!("{n} = {}", pd::toml_value(v))));
        }
        let html = list_html(&lib, &app.world.res::<ProcState>(), &|_| None);
        assert!(html.contains("Coming soon") && html.contains("Lava"), "the planned items show as coming soon");
        assert_eq!(html.matches(r#"data-act="proc-item""#).count(), lib.ready().count());
        // The library's categories as folds, each asset with its switch.
        assert_eq!(html.matches(r#"<details class="pc-cat""#).count(), 24);
        assert!(html.contains("Terrain and landforms") && html.contains(r#"data-act="proc-on" data-v="terrain-rolling-hills""#));
        // Review round 3: a stand-in reads On like a built asset; each now
        // carries its "not built yet" badge, and no built asset does.
        // Batch C built the last stand-ins: no card carries the badge.
        let standins = lib.ready().filter(|i| pd::is_standin(i)).count();
        assert_eq!(standins, 0, "no stand-in is left");
        assert_eq!(html.matches(r#"<span class="pc-stub""#).count(), standins, "a badge on every stand-in's card, none on a built one");
    }

    #[test]
    fn a_reference_shows_its_picture_score_and_note() {
        let mut h = String::new();
        let r = pd::Reference { image: "proc-refs/geometry-cube-cuboid.jpg".into(), score: Some(4.0), status: "pass".into(), note: "cast concrete".into() };
        reference_html(&mut h, &r);
        assert!(h.contains(r#"src="proc-refs/geometry-cube-cuboid.jpg""#) && h.contains("Reference target") && h.contains("4/5 · pass") && h.contains("cast concrete"));
        for (s, shown) in [(4.5, "4.5/5"), (2.5, "2.5/5"), (3.0, "3/5")] {
            let mut h = String::new();
            reference_html(&mut h, &pd::Reference { score: Some(s), ..r.clone() });
            assert!(h.contains(shown), "half stars show as written: {shown}");
        }
        let mut h = String::new();
        reference_html(&mut h, &pd::Reference { score: None, note: String::new(), ..r });
        assert!(h.contains("not scored yet"));
        // An asset with no picture yet shows none (the references file lists
        // only assets that have one).
        assert!(pd::reference_of("geometry-cube-cuboid").is_none_or(|r| r.image.starts_with("proc-refs/")));
    }

    #[test]
    fn labels_read_as_words() {
        assert_eq!(label("material.nature.water.wave_strength"), "Wave strength");
        assert_eq!(label("material.nature.water.deep_color"), "Deep colour");
        assert_eq!(label("material.nature.sky.horizon"), "Reflected sky horizon");
        assert_eq!(label("sky.cloud_coverage"), "Cloud coverage");
        assert_eq!(readout(0.12345, 0.005), "0.123");
        assert_eq!(readout(22.0, 0.5), "22.0");
    }

    /// The ocean's panel (the spectral sea) has the Ultra / High / Medium /
    /// Low switch (and Auto), shows which is on, and labels each setting
    /// that doesn't show on every level; "Use in a game" says the same.
    #[test]
    fn the_ocean_panel_has_quality_levels_and_labels() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        app.world.res_mut::<Viewer>().push(Command::Scene(PROC_SCENE.into()));
        app.run_headless(2, DT);
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("ocean".into()));
        app.run_headless(2, DT);
        let st = app.world.res::<ProcState>().clone();
        let reg = app.world.res::<ParamRegistry>();
        let it = st.open_item().unwrap();
        let html = panel_html(&st, &reg, &ProcUi::default());
        for (name, v) in pd::QUALITY_LEVELS {
            assert!(html.contains(&format!(r#"data-act="proc-quality" data-v="{v}""#)), "{name}");
            assert!(html.contains(&format!(">{name}</button>")), "{name}");
        }
        assert!(html.contains(r#"class="pc-q on" data-act="proc-quality" data-v="-1""#), "Auto is on");
        assert!(html.contains(r#"<em class="pc-lv">High and Ultra</em>"#) && html.contains(r#"<em class="pc-lv">Ultra only</em>"#) && html.contains(r#"<em class="pc-lv">Medium and up</em>"#));
        // The labels are the crate's own.
        for (n, l) in &it.levels {
            let want = match l.as_str() {
                "ultra" => "Ultra only",
                "high" => "High and Ultra",
                _ => "Medium and up",
            };
            assert_eq!(evani_material_nature::sea::shows_on(n), want, "{n}");
        }
        for (n, _) in evani_material_nature::sea::SHOWS_FROM {
            assert!(it.level_note(n).is_some(), "{n} is labelled");
        }
        let game = game_text(it, &reg);
        assert!(game.contains("nature = [\"spectral-sea\"]") && game.contains("Quality: Ultra, High, Medium or Low") && game.contains("fixed: 3 Ultra, 2 High, 1 Medium, 0 Low") && game.contains("material.nature.water.spray: Ultra only"));
        drop(reg);
        // The lake and river switch the nature pass's own quality: one top
        // step for High and Ultra (refraction; both buttons light), Medium
        // (waves, foam, glow), Low (calm water), each setting labelled by the
        // lowest it shows on.
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("water".into()));
        app.run_headless(2, DT);
        let st = app.world.res::<ProcState>().clone();
        let reg = app.world.res::<ParamRegistry>();
        let it = st.open_item().unwrap();
        assert_eq!(it.quality.as_deref(), Some("material.nature.quality"));
        let html = panel_html(&st, &reg, &ProcUi::default());
        for (name, v) in [("Auto", -1.0), ("Ultra", 3.0), ("High", 3.0), ("Medium", 2.0), ("Low", 1.0)] {
            assert!(html.contains(&format!(r#"data-act="proc-quality" data-v="{v}" aria-pressed="false" title="Preview the {name} level">{name}</button>"#)) || name == "Auto", "{name}");
        }
        assert!(html.contains(r#"class="pc-q on" data-act="proc-quality" data-v="-1""#), "Auto is on");
        drop(reg);
        // High: the one top step, so Ultra and High light together.
        app.world.res_mut::<ParamRegistry>().set_by_name("material.nature.quality", ParamValue::F32(3.0)).unwrap();
        let reg = app.world.res::<ParamRegistry>();
        let html = panel_html(&st, &reg, &ProcUi::default());
        assert_eq!(html.matches(r#"class="pc-q on""#).count(), 2, "Ultra and High");
        for n in ["flow_speed", "wave_strength", "sss_strength", "whitecap_amount", "foam"] {
            assert_eq!(it.level_note(&format!("material.nature.water.{n}")), Some("Medium and up"), "{n}");
        }
        assert_eq!(it.level_note("material.nature.water.refraction"), Some("High and Ultra"));
        assert_eq!(it.level_note("material.nature.water.deep_color"), None, "colour shows on every level");
        // The levels match the nature pass: 1 draws flat, 2 split, 3 refracts.
        use evani_material_nature::effect::Mode;
        assert_eq!((Mode::for_quality(1, true), Mode::for_quality(2, true), Mode::for_quality(3, true)), (Mode::Flat, Mode::Split, Mode::Refract));
        drop(reg);
        // Glass and sky have no switch.
        for id in ["glass", "sky"] {
            app.world.res_mut::<ProcState>().push(ProcCommand::Open(id.into()));
            app.run_headless(2, DT);
            let st = app.world.res::<ProcState>().clone();
            let html = panel_html(&st, &app.world.res::<ParamRegistry>(), &ProcUi::default());
            assert!(!html.contains("proc-quality"), "{id}");
        }
    }
}
