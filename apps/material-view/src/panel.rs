//! The page's markup, built from the data and the registered params (no
//! browser calls here, so it is tested natively; `web.rs` puts it on the
//! page and wires the events).
//!
//! The tuning panel is generated from the registered parameter list, an idea
//! borrowed from Filament's material sandbox (ideas only, no code; see
//! DECISIONS.md): every row is a param of the open material's family, shown
//! by its registered name (the same name the Look Lab and AI live edits
//! use), with an input that fits its type:
//!
//! * f32 / i32: a range slider (at least 44 px tall, index.html);
//! * bool: a toggle; enum: chips; colour: a colour input; vec3: three
//!   sliders.
//!
//! Rows sit in the Base, Finish and Detail groups (each with a reset back to
//! the saved values, A), and the family's quality params go to the one
//! Quality section with the tier chips and the frame-time readout (Filament's
//! grouped quality toggles).

// The markup is used by the wasm page only; native builds compile it for
// the tests.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use evani_core::params::{ParamDef, ParamType, ParamValue};
use evani_kit_material_view::Viewer;
use evani_kit_material_view::library::{Data, Family, PanelRow, under};

/// The catalogue's library tab and the open row's panel.
#[cfg(feature = "material-catalogue")]
#[path = "panel_catalogue.rs"]
pub mod catalogue;

/// The tier chips the Quality section offers (when the build has them).
pub const TIER_CHIPS: &[&str] = &["low", "medium", "high"];

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The HUD (top left): the material's name and a "family · scene · light"
/// line. A part that repeats the one before it (the art styles family shows
/// in the "Art styles" scene) or is empty is left out. Each line carries its
/// whole text as a tooltip, since the HUD shortens long ones with "...".
pub fn hud_html(material: &str, parts: &[&str], paused: bool) -> String {
    let mut line: Vec<&str> = Vec::new();
    for p in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        if !line.last().is_some_and(|l| l.eq_ignore_ascii_case(p)) {
            line.push(p);
        }
    }
    if paused {
        line.push("paused");
    }
    let small = escape(&line.join(" \u{b7} "));
    let name = escape(material);
    format!("Evani.Materials <b title=\"{name}\">{name}</b><small title=\"{small}\">{small}</small>")
}

/// The row label: the name below the family (or material) prefix, dots as
/// " · " and underscores as spaces.
pub fn label(name: &str, prefixes: &[String]) -> String {
    let rest = prefixes
        .iter()
        .filter(|p| under(name, p) && name.len() > p.len())
        .max_by_key(|p| p.len())
        .map_or(name, |p| &name[p.len() + 1..]);
    rest.replace('.', " · ").replace('_', " ")
}

/// A param's own bounds are "unbounded" when the def kept the type's limits.
fn bounded(x: f32) -> bool {
    x.is_finite() && x.abs() < 1.0e6
}

/// The slider range and step for a numeric param (or one vec3 component
/// whose default is `d`). Unbounded params get a range around the default.
pub fn slider_range(def: &ParamDef, d: f32) -> (f32, f32, f32) {
    let lo = if bounded(def.min) {
        def.min
    } else if d >= 0.0 {
        0.0_f32.min(d)
    } else {
        d * 3.0
    };
    let hi = if bounded(def.max) {
        def.max
    } else if d > 0.0 {
        (d * 3.0).max(1.0)
    } else {
        d.abs().max(1.0)
    };
    let (lo, hi) = if hi > lo { (lo, hi) } else { (lo, lo + 1.0) };
    let step = match def.ty {
        ParamType::I32 => 1.0,
        _ => def.step.filter(|s| *s > 0.0).unwrap_or((hi - lo) / 200.0),
    };
    (lo, hi, step)
}

/// Unbounded vec3 components range around the largest component.
pub fn vec3_range(def: &ParamDef, d: [f32; 3]) -> (f32, f32, f32) {
    if bounded(def.min) && bounded(def.max) {
        return slider_range(def, d[0]);
    }
    let m = d.iter().fold(1.0_f32, |a, x| a.max(x.abs())) * 2.0;
    let lo = if bounded(def.min) { def.min } else { -m };
    let hi = if bounded(def.max) { def.max } else { m };
    let step = def.step.filter(|s| *s > 0.0).unwrap_or((hi - lo) / 200.0);
    (lo, hi, step)
}

pub fn fmt_num(x: f32, span: f32) -> String {
    if span >= 50.0 {
        format!("{x:.0}")
    } else if span >= 5.0 {
        format!("{x:.1}")
    } else if span >= 0.5 {
        format!("{x:.2}")
    } else {
        format!("{x:.3}")
    }
}

/// The value text a row shows.
pub fn fmt_value(def: &ParamDef, v: &ParamValue) -> String {
    match *v {
        ParamValue::F32(x) => {
            let (lo, hi, _) = slider_range(def, x);
            let unit = if def.unit.is_empty() { String::new() } else { format!(" {}", def.unit) };
            format!("{}{unit}", fmt_num(x, hi - lo))
        }
        ParamValue::I32(x) => x.to_string(),
        ParamValue::Bool(b) => if b { "on" } else { "off" }.to_string(),
        ParamValue::Vec3(c) => format!("{:.2}, {:.2}, {:.2}", c[0], c[1], c[2]),
        ParamValue::Color(c) => linear_to_hex(c),
        ParamValue::Enum(i) => def.options.get(i as usize).cloned().unwrap_or_else(|| i.to_string()),
    }
}

fn to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

fn to_linear(x: f32) -> f32 {
    if x <= 0.040_45 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

/// Linear RGB (clamped to 0..1) as the `#rrggbb` a colour input shows.
pub fn linear_to_hex(c: [f32; 3]) -> String {
    let b = |x: f32| (to_srgb(x) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// A colour input's `#rrggbb` as linear RGB in 0..1.
pub fn hex_to_linear(hex: &str) -> Option<[f32; 3]> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|b| to_linear(f32::from(b) / 255.0));
    Some([c(0)?, c(2)?, c(4)?])
}

/// The prefixes a material's labels are cut from.
pub fn label_prefixes(family: &Family, panel: &[String]) -> Vec<String> {
    family.prefixes.iter().chain(panel.iter()).cloned().collect()
}

/// One row's markup. Ids: `mvp-<i>` (the input or toggle), `mvp-<i>-<k>`
/// (a vec3 component or an enum chip), `mvv-<i>` (the value text).
pub fn row_html(i: usize, row: &PanelRow, value: &ParamValue, prefixes: &[String]) -> String {
    let def = &row.def;
    let name = escape(&def.name);
    let title = if def.description.is_empty() { name.clone() } else { format!("{name}: {}", escape(&def.description)) };
    let head = format!(
        "<div class=\"row\" title=\"{title}\"><div class=\"lab\"><span>{}</span><b id=\"mvv-{i}\">{}</b></div><small>{name}</small>",
        escape(&label(&def.name, prefixes)),
        escape(&fmt_value(def, value)),
    );
    let input = match *value {
        ParamValue::F32(x) => {
            let (lo, hi, step) = slider_range(def, match def.default {
                ParamValue::F32(d) => d,
                _ => x,
            });
            format!("<input type=\"range\" id=\"mvp-{i}\" data-p=\"{name}\" min=\"{lo}\" max=\"{hi}\" step=\"{step}\" value=\"{x}\">")
        }
        ParamValue::I32(x) => {
            let (lo, hi, step) = slider_range(def, match def.default {
                ParamValue::I32(d) => d as f32,
                _ => x as f32,
            });
            format!("<input type=\"range\" id=\"mvp-{i}\" data-p=\"{name}\" min=\"{lo}\" max=\"{hi}\" step=\"{step}\" value=\"{x}\">")
        }
        ParamValue::Bool(b) => {
            let on = if b { " on" } else { "" };
            format!("<button class=\"tog{on}\" id=\"mvp-{i}\" data-bool=\"{name}\">{}</button>", if b { "On" } else { "Off" })
        }
        ParamValue::Enum(sel) => {
            let chips: String = def
                .options
                .iter()
                .enumerate()
                .map(|(k, o)| {
                    let on = if k as u32 == sel { " class=\"on\"" } else { "" };
                    format!("<button id=\"mvp-{i}-{k}\" data-enum=\"{name}\" data-i=\"{k}\"{on}>{}</button>", escape(o))
                })
                .collect();
            format!("<div class=\"chips\">{chips}</div>")
        }
        ParamValue::Color(c) => {
            format!("<input type=\"color\" id=\"mvp-{i}\" data-p=\"{name}\" value=\"{}\">", linear_to_hex(c))
        }
        ParamValue::Vec3(c) => {
            let d = match def.default {
                ParamValue::Vec3(d) => d,
                _ => c,
            };
            let (lo, hi, step) = vec3_range(def, d);
            (0..3)
                .map(|k| {
                    format!(
                        "<label class=\"axis\"><i>{}</i><input type=\"range\" id=\"mvp-{i}-{k}\" data-p=\"{name}\" data-c=\"{k}\" min=\"{lo}\" max=\"{hi}\" step=\"{step}\" value=\"{}\"></label>",
                        ["x", "y", "z"][k],
                        c[k]
                    )
                })
                .collect()
        }
    };
    format!("{head}{input}</div>")
}

/// The whole tuning panel: the groups in the data's order (rows a group
/// lacks are left out), then the Quality section. `values` is in row order.
/// `style_reset` (the art styles family): one Reset (the selected
/// material's style default) and Reset all instead of the groups' resets to
/// the saved values.
pub fn panel_html(
    data: &Data,
    family: &Family,
    rows: &[PanelRow],
    values: &[ParamValue],
    prefixes: &[String],
    tiers: &[(String, String)],
    style_reset: bool,
) -> String {
    let groups = &data.library.groups;
    let mut out = String::new();
    #[cfg(feature = "material-styles")]
    if style_reset {
        out.push_str(styles::RESET_HTML);
    }
    for g in &groups.order {
        let mine: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].group == g.as_str()).collect();
        if mine.is_empty() {
            continue;
        }
        let gname = groups.names.get(g).map_or(g.as_str(), String::as_str);
        if style_reset {
            out.push_str(&format!("<section><h3>{}</h3>", escape(gname)));
        } else {
            out.push_str(&format!(
                "<section><h3>{}<button class=\"rst\" data-reset=\"{}\" title=\"Put the {} rows back to the saved values (A)\">Reset</button></h3>",
                escape(gname),
                escape(g),
                escape(gname)
            ));
        }
        for i in mine {
            out.push_str(&row_html(i, &rows[i], &values[i], prefixes));
        }
        out.push_str("</section>");
    }
    if rows.iter().all(|r| r.group == "quality") {
        out.push_str(&format!("<p class=\"none\">{} has no tuning params in this build.</p>", escape(&family.name)));
    }
    let qname = groups.names.get("quality").map_or("Quality", String::as_str);
    out.push_str(&format!(
        "<section class=\"quality\"><h3>{}<button class=\"rst\" data-reset=\"quality\" title=\"Put the quality rows back to the saved values (A)\">Reset</button></h3>",
        escape(qname)
    ));
    let chips: String = tiers
        .iter()
        .map(|(id, label)| format!("<button id=\"mv-tier-{0}\" data-tier=\"{0}\">{1}</button>", escape(id), escape(label)))
        .collect();
    out.push_str(&format!("<div class=\"chips tiers\">{chips}</div><div class=\"ft\">Frame time <b id=\"mv-ft\">&hellip;</b> <span id=\"mv-tier-now\"></span></div>"));
    for (i, r) in rows.iter().enumerate() {
        if r.group == "quality" {
            out.push_str(&row_html(i, r, &values[i], prefixes));
        }
    }
    out.push_str("</section>");
    out
}

/// The scene row along the bottom: one big icon per scene, greyed when its
/// family is not in this build.
pub fn scenes_html(data: &Data, v: &Viewer) -> String {
    data.library
        .scenes
        .iter()
        .map(|s| {
            let built = v.has(&s.family);
            let cls = match (built, s.id == v.scene) {
                (false, _) => " class=\"off\" disabled",
                (true, true) => " class=\"on\"",
                _ => "",
            };
            format!(
                "<button data-scene=\"{}\"{cls} title=\"{}\"><span class=\"i\">{}</span><span>{}</span></button>",
                escape(&s.id),
                escape(&s.name),
                escape(&s.icon),
                escape(&s.name)
            )
        })
        .collect()
}

/// The light preset chips.
pub fn lights_html(data: &Data, v: &Viewer) -> String {
    data.lights
        .presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let on = if i == v.light { " class=\"on\"" } else { "" };
            format!("<button data-light=\"{i}\"{on}>{}</button>", escape(&p.name))
        })
        .collect()
}

/// Browser tab 1: the families and their materials. A family this build
/// lacks shows greyed with how to add it.
pub fn families_html(data: &Data, v: &Viewer) -> String {
    let mut out = String::new();
    for f in &data.library.families {
        let built = v.has(&f.id);
        out.push_str(&format!(
            "<div class=\"fam{}\"><h3>{}</h3><p>{}</p>",
            if built { "" } else { " off" },
            escape(&f.name),
            escape(&f.about)
        ));
        if !built {
            out.push_str(&format!(
                "<p class=\"why\">Not in this build: add \"{}\" to the materials list in evani.toml.</p>",
                escape(&f.id)
            ));
        }
        out.push_str("<div class=\"mats\">");
        for m in &f.materials {
            let cls = match (built, m.id == v.material) {
                (false, _) => " class=\"off\" disabled",
                (true, true) => " class=\"on\"",
                _ => "",
            };
            out.push_str(&format!("<button data-mat=\"{}\"{cls}>{}</button>", escape(&m.id), escape(&m.name)));
        }
        out.push_str("</div></div>");
    }
    out
}

/// Browser tab 2: Evani.Materials' world rooms by Meridian Vale zone. A
/// filled slot opens its material; an empty slot shows greyed with "try a
/// material here" and opens its family's default; a GAP slot (no family yet)
/// is greyed and does nothing.
pub fn rooms_html(data: &Data, v: &Viewer) -> String {
    let mut rooms: Vec<_> = data.rooms.rooms.iter().collect();
    rooms.sort_by(|a, b| a.zone.cmp(&b.zone));
    let mut out = String::new();
    for r in rooms {
        out.push_str(&format!(
            "<div class=\"room\"><h3><i>{}</i> {}</h3><p>{}</p>",
            escape(&r.zone),
            escape(&r.name),
            escape(&r.description)
        ));
        for s in &r.slots {
            let fam = data.family_for_rooms(&s.family);
            let fam_name = fam.map_or("No family yet", |f| f.name.as_str());
            // A slot filled from another family (the sea water opens the
            // studio's spectral ocean) needs that family built in too.
            let target = data.slot_target(s).filter(|t| fam.is_some_and(|f| v.has(&f.id)) && data.material(t).is_none_or(|(f, _)| v.has(&f.id)));
            let open = target.map(|t| format!(" data-mat=\"{}\"", escape(t))).unwrap_or_default();
            let filled = s.material.is_some();
            let gap = s.family == "GAP";
            let cls = if filled { "slot" } else { "slot empty" };
            let dis = if target.is_none() { " disabled" } else { "" };
            let on = if filled && s.material.as_deref() == Some(v.material.as_str()) { " on" } else { "" };
            let tail = if filled {
                String::new()
            } else if gap {
                format!(
                    "<em>try a material here</em> <span class=\"gap\">no family covers it yet{}</span>",
                    s.note.as_deref().map(|n| format!(" ({})", escape(n))).unwrap_or_default()
                )
            } else {
                "<em>try a material here</em>".to_string()
            };
            out.push_str(&format!(
                "<button class=\"{cls}{on}\"{open}{dis} title=\"{}\"><b>{}</b><span class=\"tag\">{}</span>{tail}</button>",
                escape(&s.id),
                escape(&s.look),
                escape(fam_name)
            ));
        }
        out.push_str("</div>");
    }
    out
}

/// The art styles' page parts (only with the `material-styles` feature): the
/// style bar, the style header, the material chips, the shape picker, the
/// reference panel and their CSS. A build without the family has none of it.
#[cfg(feature = "material-styles")]
pub mod styles {
    use evani_kit_material_view::styles::{Fidelity, Finish, Status, Style, Styles};

    use super::{escape, linear_to_hex};
    use crate::shapes::Shape;

    /// One material's finish now, as `styles::finish` gives it.
    pub type MatFinish = Option<(Finish, bool, Option<String>)>;

    /// The docked reference panel's narrowest width, in CSS px.
    pub const REFS_MIN_W: f64 = 220.0;
    /// The docked reference panel's widest share of the window.
    pub const REFS_MAX_SHARE: f64 = 0.6;
    /// Room the docked panel always leaves for the viewer (CSS px), so the
    /// desktop layout beside it never overlaps; the window's phone width.
    pub const REFS_KEEP_VIEW: f64 = 760.0;
    /// Where the docked panel's width is remembered.
    pub const REFS_W_KEY: &str = "evani.material_view.refs_w";
    pub const REFS_MISSING: &str = "Reference pictures aren't available here: they're only on the dev server.";

    /// The styles family's panel top: one reset for the selected material,
    /// one for every edit.
    pub const RESET_HTML: &str = "<section class=\"sreset\"><h3>Selected material<span>\
        <button class=\"rst\" data-sreset=\"one\" title=\"Put the selected material back to its style's starting values\">Reset</button> \
        <button class=\"rst\" data-sreset=\"all\" title=\"Drop every edit in every style\">Reset all</button></span></h3></section>";

    /// The test shapes' credits, under the panel.
    pub const ABOUT_HTML: &str = "<p class=\"about\">Monkey head: IridescenceSuzanne, Khronos glTF Sample Assets, CC0. \
        Teapot: Utah Teapot, University of Utah.</p>";

    /// The dock's fixed parts; web.rs fills the strips.
    pub const DOCK_HTML: &str = "<div class=\"srow st\"><button class=\"sarr l\" data-cmd=\"st-prev\" title=\"Earlier styles\" aria-label=\"Earlier styles\">&#8249;</button>\
        <div id=\"mv-stylebar\" class=\"strip\" aria-label=\"Art styles\"></div>\
        <button class=\"sarr r\" data-cmd=\"st-next\" title=\"More styles\" aria-label=\"More styles\">&#8250;</button>\
        <button data-cmd=\"refs\" id=\"mv-refs-btn\" title=\"Show the style's reference pictures\">References</button></div>\
        <div id=\"mv-stylehead\"></div>\
        <div class=\"srow m\"><div id=\"mv-smats\" class=\"strip\" aria-label=\"Materials\"></div></div>\
        <div class=\"srow m\"><div id=\"mv-shapes\" class=\"strip\" aria-label=\"Test shape\"></div></div>";

    /// The reference panel's fixed parts.
    pub const REFS_HTML: &str = "<div id=\"mv-refs-grip\" title=\"Drag to resize\"></div><div id=\"mv-refs-body\"></div>";

    /// The status a style shows.
    pub fn status(s: &Style) -> (&'static str, String) {
        if s.status == Status::Missing || s.materials.is_empty() {
            return ("missing", "Not made yet".to_string());
        }
        match s.status {
            Status::Look => ("look", format!("Look: {}", s.look.as_deref().unwrap_or("?"))),
            _ => ("draft", "Draft".to_string()),
        }
    }

    /// The bar: one numbered button per style, in data order.
    pub fn bar_html(styles: &Styles, active: usize) -> String {
        styles
            .styles
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let on = if i == active { " class=\"on\"" } else { "" };
                let badge = match status(s).0 {
                    "draft" => "<i class=\"badge\">Draft</i>",
                    "missing" => "<i class=\"badge miss\">Not made</i>",
                    _ => "",
                };
                format!(
                    "<button id=\"mv-st-{i}\" data-style=\"{i}\"{on} title=\"{}: {}\"><b>{}</b>{}{badge}</button>",
                    escape(&s.name),
                    escape(&s.mood),
                    s.num,
                    escape(&s.short)
                )
            })
            .collect()
    }

    /// The header: name, the style in a few words, status, note, and every
    /// material's warning.
    pub fn head_html(s: &Style, finishes: &[MatFinish]) -> String {
        let (cls, label) = status(s);
        let mood = escape(&s.mood);
        let mut out = format!(
            "<div class=\"sh1\"><b>{}</b> <span class=\"mood\" title=\"{mood}\">{mood}</span> <span class=\"status {cls}\">{}</span></div><div class=\"note\">{}</div>",
            escape(&s.name),
            escape(&label),
            escape(&s.note)
        );
        // What the shapes show for this style, and whether it is the real look.
        if let Some(r) = s.render.as_ref().filter(|r| !r.label.is_empty() && s.status != Status::Missing) {
            let (cls, tag) = match r.fidelity {
                Fidelity::True => ("true", "True look"),
                Fidelity::Approx => ("approx", "Close match"),
            };
            out.push_str(&format!(
                "<div class=\"look\">Shape look: {} <span class=\"fid {cls}\">{tag}</span></div>",
                escape(&r.label)
            ));
        }
        for (m, f) in s.materials.iter().zip(finishes) {
            if let Some((_, _, Some(w))) = f {
                out.push_str(&format!("<div class=\"warn\">&#9888; {}: {}</div>", escape(&m.name), escape(w)));
            }
        }
        out
    }

    /// The material chips: swatch and name, a dot when edited.
    pub fn mats_html(s: &Style, finishes: &[MatFinish], selected: usize) -> String {
        if s.materials.is_empty() {
            return "<span class=\"none\">No materials: not made yet</span>".to_string();
        }
        s.materials
            .iter()
            .zip(finishes)
            .enumerate()
            .map(|(i, (m, f))| match f {
                Some((fin, edited, _)) => format!(
                    "<button class=\"chip{}\" data-smat=\"{i}\"><i class=\"sw\" style=\"background:{}\"></i>{}{}</button>",
                    if i == selected { " on" } else { "" },
                    linear_to_hex(fin.tint),
                    escape(&m.name),
                    if *edited { "<i class=\"dot\" title=\"Edited\"></i>" } else { "" }
                ),
                None => format!("<button class=\"chip\" disabled><i class=\"sw\"></i>{} <em>not made yet</em></button>", escape(&m.name)),
            })
            .collect()
    }

    /// The shape picker and the show-all switch.
    pub fn shapes_html(current: &str, show_all: bool) -> String {
        let mut out: String = Shape::ALL
            .iter()
            .map(|s| {
                let on = if s.id() == current { " class=\"on\"" } else { "" };
                format!("<button data-shape=\"{}\"{on}>{}</button>", s.id(), s.label())
            })
            .collect();
        let on = |b: bool| if b { " class=\"on\"" } else { "" };
        out.push_str(&format!(
            "<span class=\"sep\"></span><button data-showall=\"1\"{}>Show all</button><button data-showall=\"0\"{}>One at a time</button>",
            on(show_all),
            on(!show_all)
        ));
        out
    }

    /// The reference panel's content: exactly the style's pictures (dev
    /// server only), and the message shown when they can't load.
    pub fn refs_html(styles: &Styles, s: &Style) -> String {
        let imgs: String = (1..=styles.refs_count)
            .map(|n| {
                format!(
                    "<img loading=\"lazy\" decoding=\"async\" src=\"{}{}/{n}.jpg\" alt=\"{} reference {n}\">",
                    escape(&styles.refs_url),
                    escape(&s.refs),
                    escape(&s.name)
                )
            })
            .collect();
        format!(
            "<h3>References<span class=\"hint\">Tap anywhere to close</span><button class=\"rst\" data-cmd=\"refs-close\">Close</button></h3>\
             <p class=\"rt\"><b>{} {}</b><span class=\"mood\">{}</span></p>\
             <div class=\"refs-list\">{imgs}</div><p class=\"refs-miss\">{REFS_MISSING}</p>\
             <button class=\"refs-close\" data-cmd=\"refs-close\">Close</button>",
            s.num,
            escape(&s.name),
            escape(&s.mood)
        )
    }

    /// The styles' CSS, added to the page only by builds with the family.
    /// `--dock-h` (the dock's height) and `--refs-w` (the docked reference
    /// panel's width) are set by web.rs.
    pub const CSS: &str = r#"
body.styles-on { --dock-top: 8px; --under-dock: calc(var(--dock-top) + var(--dock-h, 0px) + 8px); }
#mv-styles { display: none; }
body.styles-on #mv-styles { display: flex; flex-direction: column; gap: 4px; position: fixed; z-index: 60; top: var(--dock-top);
  left: 12px; right: calc(12px + var(--refs-w, 0px)); padding: 4px; box-sizing: border-box;
  background: var(--bg); border: 1px solid var(--line); border-radius: 12px; }
#mv-styles .srow { display: flex; gap: 6px; align-items: center; min-width: 0; }
#mv-styles .strip { display: flex; gap: 6px; align-items: center; overflow-x: auto; scrollbar-width: none; flex: 1; min-width: 0; }
#mv-styles .strip > * { flex: none; }
#mv-styles button { min-height: 38px; padding: 0 10px; white-space: nowrap; }
#mv-stylebar b { color: var(--accent); margin-right: 5px; font-weight: 650; }
#mv-stylebar { scroll-behavior: smooth; }
#mv-styles .sarr { flex: none; min-width: 34px; padding: 0; font-size: 22px; line-height: 1; }
#mv-styles .srow.st:not(.fl):not(.fr) .sarr { display: none; }
#mv-styles .srow.st:not(.fl) .sarr.l, #mv-styles .srow.st:not(.fr) .sarr.r { opacity: 0.3; pointer-events: none; }
#mv-styles .srow.fr > .strip { -webkit-mask-image: linear-gradient(to right, #000 calc(100% - 36px), transparent);
  mask-image: linear-gradient(to right, #000 calc(100% - 36px), transparent); }
#mv-styles .srow.fl > .strip { -webkit-mask-image: linear-gradient(to left, #000 calc(100% - 36px), transparent);
  mask-image: linear-gradient(to left, #000 calc(100% - 36px), transparent); }
#mv-styles .srow.fl.fr > .strip { -webkit-mask-image: linear-gradient(to right, transparent, #000 36px, #000 calc(100% - 36px), transparent);
  mask-image: linear-gradient(to right, transparent, #000 36px, #000 calc(100% - 36px), transparent); }
@media (max-width: 759px) { #mv-styles .sarr { display: none !important; } }
#mv-styles .badge { font-size: 10px; font-style: normal; padding: 1px 4px; margin-left: 6px; border-radius: 5px;
  background: rgba(255, 255, 255, 0.1); color: var(--dim); vertical-align: 1px; }
#mv-styles .badge.miss { color: #ffb347; }
#mv-styles .sep { width: 1px; height: 26px; background: var(--line); margin: 0 2px; }
body:not(.on-styles) #mv-stylehead, body:not(.on-styles) #mv-styles .srow.m { display: none; }
#mv-stylehead { padding: 2px 6px; font-size: 13px; }
#mv-stylehead .sh1 { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; }
#mv-stylehead .sh1 b { font-size: 15px; }
#mv-styles .mood { color: var(--dim); min-width: 0; overflow: hidden; text-overflow: ellipsis; }
.status { font-size: 12px; font-weight: 650; padding: 1px 7px; border-radius: 6px; }
.status.draft { background: rgba(255, 255, 255, 0.12); }
.status.look { background: rgba(110, 200, 140, 0.25); color: #bdf0cc; }
.status.missing { background: rgba(255, 179, 71, 0.2); color: #ffb347; }
#mv-stylehead .note { color: var(--dim); font-size: 12px; }
#mv-stylehead .warn { color: #ffb347; font-size: 12px; font-weight: 600; }
#mv-stylehead .look { font-size: 12px; }
#mv-stylehead .fid { font-size: 11px; font-weight: 650; padding: 0 6px; border-radius: 6px; margin-left: 4px; white-space: nowrap; }
#mv-stylehead .fid.true { background: rgba(110, 200, 140, 0.25); color: #bdf0cc; }
#mv-stylehead .fid.approx { background: rgba(255, 255, 255, 0.12); color: var(--dim); }
#mv-smats .sw { display: inline-block; width: 16px; height: 16px; border-radius: 50%; margin-right: 7px; vertical-align: -3px;
  border: 1px solid rgba(255, 255, 255, 0.35); background: #333; }
#mv-smats .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--accent); margin-left: 6px; vertical-align: 2px; }
#mv-smats em { font-size: 11px; color: var(--dim); }
#mv-panel .sreset h3 span { display: flex; gap: 6px; }
#mv-panel .about { margin: 6px 0 0; font-size: 11px; color: var(--dim); }
body.styles-on #mv-hud { top: var(--under-dock); }
body.styles-on #mv-drawer { top: calc(var(--under-dock) + 76px); }
body.styles-on #mv-panel { top: var(--under-dock); }
body.styles-on #mv-rows { scrollbar-width: thin; scrollbar-color: rgba(255, 255, 255, 0.45) transparent; }
body.styles-on #mv-panel.more-down #mv-rows { -webkit-mask-image: linear-gradient(to bottom, #000 calc(100% - 32px), transparent);
  mask-image: linear-gradient(to bottom, #000 calc(100% - 32px), transparent); }
body.styles-on #mv-panel.more-up #mv-rows { -webkit-mask-image: linear-gradient(to top, #000 calc(100% - 28px), transparent);
  mask-image: linear-gradient(to top, #000 calc(100% - 28px), transparent); }
body.styles-on #mv-panel.more-up.more-down #mv-rows { -webkit-mask-image: linear-gradient(to bottom, transparent, #000 28px, #000 calc(100% - 32px), transparent);
  mask-image: linear-gradient(to bottom, transparent, #000 28px, #000 calc(100% - 32px), transparent); }
body.styles-on #mv-panel.more-down::after { content: "\25BE  More below: scroll"; flex: none; display: block; text-align: center;
  padding: 5px 0 6px; border-top: 1px solid var(--line); font-size: 11px; white-space: nowrap; pointer-events: none; color: var(--accent); }
#mv-scenes.fr { -webkit-mask-image: linear-gradient(to right, #000 calc(100% - 32px), transparent);
  mask-image: linear-gradient(to right, #000 calc(100% - 32px), transparent); }
#mv-scenes.fl { -webkit-mask-image: linear-gradient(to left, #000 calc(100% - 32px), transparent);
  mask-image: linear-gradient(to left, #000 calc(100% - 32px), transparent); }
#mv-scenes.fl.fr { -webkit-mask-image: linear-gradient(to right, transparent, #000 32px, #000 calc(100% - 32px), transparent);
  mask-image: linear-gradient(to right, transparent, #000 32px, #000 calc(100% - 32px), transparent); }
body.refs-open #mv-refs-btn { border-color: var(--accent); background: rgba(224, 178, 90, 0.22); }

#mv-refs { display: none; }
body.refs-dock #mv-refs { display: flex; position: fixed; top: 0; right: 0; bottom: 0; width: var(--refs-w, 0px); z-index: 70;
  background: #12141b; border-left: 1px solid var(--line); }
#mv-refs-grip { flex: none; width: 22px; margin-left: -8px; cursor: ew-resize; touch-action: none; user-select: none; position: relative; }
#mv-refs-grip::before { content: ""; position: absolute; left: 12px; top: 50%; width: 4px; height: 56px; margin-top: -28px;
  border-radius: 2px; background: var(--dim); opacity: 0.6; }
#mv-refs-grip:hover::before { background: var(--accent); opacity: 1; }
#mv-refs-body { flex: 1; overflow-y: auto; padding: 10px 12px 14px 0; overscroll-behavior: contain; }
#mv-refs .rt { margin: 0 0 8px; } #mv-refs .rt .mood { display: block; color: var(--dim); font-size: 12px; }
#mv-refs .refs-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
#mv-refs .refs-list img { display: block; width: 100%; height: 130px; object-fit: cover; border-radius: 8px; background: rgba(255, 255, 255, 0.05); }
#mv-refs .refs-list img.bad { display: none; }
#mv-refs .refs-miss { display: none; color: var(--dim); }
#mv-refs-body.no-refs .refs-miss { display: block; } #mv-refs-body.no-refs .refs-list { display: none; }
#mv-refs .refs-close { display: none; }
#mv-refs h3 .hint { display: none; }
body.refs-sheet #mv-refs { display: flex; position: fixed; inset: 0; z-index: 5000; background: #0a0c10; }
body.refs-sheet #mv-refs-grip { display: none; }
body.refs-sheet #mv-refs-body { padding: 0 14px 24px; }
body.refs-sheet #mv-refs h3 { position: sticky; top: 0; z-index: 2; display: flex; align-items: center; gap: 10px;
  margin: 0 -14px 10px; padding: 10px 14px; background: #0a0c10; border-bottom: 1px solid var(--line); }
body.refs-sheet #mv-refs h3 .hint { display: block; flex: 1; text-align: right; font-size: 12px; font-weight: 400;
  text-transform: none; letter-spacing: 0; color: var(--dim); }
body.refs-sheet #mv-refs h3 .rst { min-height: 44px; padding: 0 18px; font-size: 15px; }
body.refs-sheet #mv-refs .refs-list { grid-template-columns: 1fr; }
body.refs-sheet #mv-refs .refs-list img { height: auto; max-height: 46vh; object-fit: contain; }
body.refs-sheet #mv-refs .refs-close { display: block; width: 100%; min-height: 56px; margin-top: 12px; font-size: 16px; }

@media (max-width: 759px), (pointer: coarse) {
  body.styles-on { --dock-top: var(--top-clear); }
  body.styles-on #mv-hud { top: 12px; }
  body.styles-on #mv-drawer { top: var(--under-dock); }
}
@media (max-width: 759px) {
  body.styles-on #mv-styles { left: 6px; right: 6px; }
  #mv-styles button { min-height: 40px; padding: 0 9px; }
  #mv-stylehead .note { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  #mv-stylehead .sh1 { flex-wrap: nowrap; min-width: 0; }
  #mv-stylehead .sh1 b { white-space: nowrap; }
  #mv-stylehead .sh1 .mood { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  #mv-stylehead .look { display: none; }
  body.styles-on #mv-drawer, body.styles-on #mv-panel { top: auto;
    max-height: min(48vh, calc(100vh - var(--bottom) - var(--under-dock) - 4px)); }
}
/* Short screens (a phone held sideways): the dock goes to the top edge,
   left of the round touch buttons, with a one-line header and the
   materials and shapes rows side by side, so the view keeps its middle. */
@media (max-height: 520px) {
  body.styles-on { --dock-top: 6px; }
  body.styles-on #mv-styles { flex-direction: row; flex-wrap: wrap; gap: 3px; padding: 3px;
    right: calc(var(--tb-r, 12px) + 3 * var(--tb, 48px) + 24px); }
  #mv-styles .srow.st, #mv-stylehead { flex: 1 1 100%; min-width: 0; }
  /* The materials are few and short and must each read by name; the shapes
     scroll (critic rounds 11, 12: Fabric read only "F..."). */
  #mv-styles .srow.m { flex: 2 1 0; min-width: 0; }
  #mv-styles .srow.m + .srow.m { flex: 1 1 0; }
  /* Each half scrolls within its own width: the material chips ran on under
     the shape chips ("Fabric" cut under "Sphere", critic round 8). */
  #mv-styles .srow.m { overflow: hidden; position: relative; box-sizing: border-box; }
  #mv-styles .srow.m > .strip { min-width: 0; overflow-x: auto; scrollbar-width: none; -webkit-mask-image: none; mask-image: none; }
  /* Round 10 ("Fabr" still read as cut under "Sphere"): the two halves are
     parted by a rule, and a half with chips off an edge ends in a solid fade
     and an ellipsis there, so no chip ever looks covered by the next half. */
  #mv-styles .srow.m + .srow.m { border-left: 1px solid var(--line); padding-left: 3px; }
  #mv-styles .srow.m.fr::after, #mv-styles .srow.m.fl::before { position: absolute; top: 0; bottom: 0; width: 34px; display: flex;
    align-items: center; pointer-events: none; color: var(--accent); font-weight: 700; font-size: 16px; content: "\2026"; z-index: 1; }
  #mv-styles .srow.m.fr::after { right: 0; justify-content: flex-end; padding-right: 3px;
    background: linear-gradient(to right, rgba(24, 27, 34, 0), rgba(24, 27, 34, 0.97) 55%); }
  #mv-styles .srow.m.fl::before { left: 0; padding-left: 3px; background: linear-gradient(to left, rgba(24, 27, 34, 0), rgba(24, 27, 34, 0.97) 55%); }
  #mv-styles button { min-height: 32px; padding: 0 8px; font-size: 13px; }
  #mv-styles .badge { display: none; }
  #mv-stylehead { padding: 0 4px; }
  #mv-stylehead .sh1 { flex-wrap: nowrap; min-width: 0; }
  #mv-stylehead .sh1 b { font-size: 13px; white-space: nowrap; }
  #mv-stylehead .sh1 .mood { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; font-size: 12px; }
  #mv-stylehead .note, #mv-stylehead .look { display: none; }
  body.styles-on #mv-hud { top: var(--under-dock); }
  body.styles-on #mv-drawer { top: calc(var(--under-dock) + 52px); }
}
"#;
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_kit_material_view::library::Groups;

    fn data() -> Data {
        Data::shipped().unwrap()
    }

    #[test]
    fn labels_drop_the_family_prefix() {
        let p = vec!["material.nature".to_string()];
        assert_eq!(label("material.nature.water.clarity", &p), "water · clarity");
        assert_eq!(label("material.nature.max_surfaces", &p), "max surfaces");
        assert_eq!(label("other.thing", &p), "other · thing");
    }

    #[test]
    fn colours_round_trip_through_the_colour_input() {
        for c in [[0.0, 0.5, 1.0], [0.2, 0.04, 0.9]] {
            let back = hex_to_linear(&linear_to_hex(c)).unwrap();
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 0.01, "{c:?} -> {back:?}");
            }
        }
        // HDR values clamp to the input's 0..1.
        assert_eq!(linear_to_hex([2.0, -1.0, 1.0]), "#ff00ff");
        assert_eq!(hex_to_linear("nope"), None);
    }

    #[test]
    fn unbounded_params_get_a_range_around_the_default() {
        let d = ParamDef::f32("a.b", 2.0);
        assert_eq!(slider_range(&d, 2.0).0, 0.0);
        assert_eq!(slider_range(&d, 2.0).1, 6.0);
        let r = ParamDef::f32("a.c", 0.5).range(0.0, 1.0).step(0.05);
        assert_eq!(slider_range(&r, 0.5), (0.0, 1.0, 0.05));
        let i = ParamDef::i32("a.d", 4).range(1.0, 8.0);
        assert_eq!(slider_range(&i, 4.0).2, 1.0);
        let v = ParamDef::vec3("a.e", [0.0, -3.0, 1.0]);
        assert_eq!(vec3_range(&v, [0.0, -3.0, 1.0]).0, -6.0);
    }

    fn row(def: ParamDef, group: &'static str, i: u32) -> PanelRow {
        PanelRow { group, id: evani_core::params::ParamId(i), def }
    }

    /// Every param type gets its own input, the reset buttons and the
    /// Quality section are there, and names are escaped.
    #[test]
    fn the_panel_has_an_input_for_every_type() {
        let d = data();
        let fam = d.family("nature").unwrap();
        let rows = vec![
            row(ParamDef::f32("material.nature.a", 0.5).range(0.0, 1.0), "base", 0),
            row(ParamDef::i32("material.nature.b", 3).range(0.0, 9.0), "detail", 1),
            row(ParamDef::bool("material.nature.c", true), "finish", 2),
            row(ParamDef::enumeration("material.nature.d", &["one", "<two>"], 1), "finish", 3),
            row(ParamDef::color("material.nature.e", [1.0, 0.0, 0.0]), "base", 4),
            row(ParamDef::vec3("material.nature.f", [0.0, 1.0, 0.0]), "detail", 5),
            row(ParamDef::bool("material.nature.enabled", true), "quality", 6),
        ];
        let values: Vec<_> = rows.iter().map(|r| r.def.default).collect();
        let tiers = vec![("low".to_string(), "Low".to_string()), ("high".to_string(), "High".to_string())];
        let html = panel_html(&d, fam, &rows, &values, &fam.prefixes, &tiers, false);
        assert!(html.contains("type=\"range\" id=\"mvp-0\""));
        assert!(html.contains("step=\"1\""), "i32 steps by one");
        assert!(html.contains("class=\"tog on\" id=\"mvp-2\""));
        assert!(html.contains("data-enum=\"material.nature.d\" data-i=\"1\" class=\"on\">&lt;two&gt;"));
        assert!(html.contains("type=\"color\" id=\"mvp-4\" data-p=\"material.nature.e\" value=\"#ff0000\""));
        assert!(html.contains("id=\"mvp-5-2\""), "vec3 has three sliders");
        for g in ["base", "finish", "detail", "quality"] {
            assert!(html.contains(&format!("data-reset=\"{g}\"")), "{g} reset");
        }
        assert!(html.contains("data-tier=\"low\"") && html.contains("id=\"mv-ft\""));
        // Base before Finish before Detail, Quality last.
        let at = |s: &str| html.find(s).unwrap();
        assert!(at("data-reset=\"base\"") < at("data-reset=\"finish\""));
        assert!(at("data-reset=\"finish\"") < at("data-reset=\"detail\""));
        assert!(at("data-reset=\"detail\"") < at("data-reset=\"quality\""));
        assert!(at("data-reset=\"quality\"") < at("id=\"mvp-6\""));
    }

    #[test]
    fn the_browser_greys_families_and_slots_this_build_lacks() {
        let d = data();
        let mut v = Viewer::default();
        v.families_on = vec!["nature".to_string()];
        let fams = families_html(&d, &v);
        assert!(fams.contains("data-mat=\"nature.water\">"));
        assert!(fams.contains("data-mat=\"layers.clearcoat\" class=\"off\" disabled"));
        assert!(fams.contains("Not in this build"));
        let rooms = rooms_html(&d, &v);
        assert_eq!(rooms.matches("class=\"room\"").count(), d.rooms.rooms.len());
        assert!(rooms.contains("try a material here"));
        assert!(rooms.contains("no family covers it yet"));
        // An empty nature slot opens the nature default; a layered one is
        // greyed (its family is not built).
        assert!(rooms.contains("data-mat=\"nature.water\""));
        let scenes = scenes_html(&d, &v);
        assert!(scenes.contains("data-scene=\"pool\""));
        assert!(scenes.contains("data-scene=\"ball\" class=\"off\" disabled"));
    }

    /// The HUD line drops a part that repeats the one before it (critic
    /// round 1: "Art styles · Art styles · Studio") and empty ones, and every
    /// line has its whole text as a tooltip.
    #[test]
    fn the_hud_line_has_no_repeated_part() {
        let h = hud_html("Red <plastic>", &["Art styles", "Art styles", "Studio"], false);
        assert!(h.contains("<small title=\"Art styles \u{b7} Studio\">Art styles \u{b7} Studio</small>"), "{h}");
        assert!(h.contains("<b title=\"Red &lt;plastic&gt;\">"), "{h}");
        let h = hud_html("Water", &["Nature", "", "Pool", "Night lamp"], true);
        assert!(h.contains(">Nature \u{b7} Pool \u{b7} Night lamp \u{b7} paused</small>"), "{h}");
    }

    #[test]
    fn groups_come_from_the_data() {
        let g: &Groups = &data().library.groups;
        assert_eq!(g.order, ["base", "finish", "detail"]);
    }

    /// The style bar, header, chips, shapes and references come from the
    /// shipped styles; the styles panel swaps the saved-value resets for
    /// Reset and Reset all.
    #[cfg(feature = "material-styles")]
    #[test]
    fn the_style_parts_follow_the_data() {
        use evani_kit_material_view::styles::{Finish, Styles};
        let s = Styles::shipped().unwrap();
        let bar = styles::bar_html(&s, 4);
        assert_eq!(bar.matches("data-style=").count(), 24);
        assert!(bar.contains("data-style=\"0\" title=\"Bright Toon: Glossy candy colours, chunky round shapes and a hard sparkle highlight\"><b>1</b>Toon<i class=\"badge\">Draft</i>"));
        assert!(bar.contains("id=\"mv-st-4\" data-style=\"4\" class=\"on\""));
        let st = &s.styles[4];
        let f = Finish { tint: [1.0, 0.0, 0.0], roughness: 0.5, metallic: 0.0, bump: 0.0, glow: 0.0 };
        let mut fins: Vec<styles::MatFinish> = st.materials.iter().map(|_| Some((f, false, None))).collect();
        fins[1] = Some((f, true, Some("Look material `x` isn't in this build".into())));
        let head = styles::head_html(st, &fins);
        assert!(head.contains("Look: stylizedpbr") && head.contains("class=\"warn\"") && head.contains("`x`"));
        assert!(head.contains("<div class=\"look\">Shape look: ") && head.contains("<span class=\"fid true\">True look</span>"));
        // A stand-in look says so.
        let pixel = styles::head_html(&s.styles[8], &fins);
        assert!(pixel.contains("Chunky blocks") && pixel.contains("<span class=\"fid approx\">Close match</span>"));
        let mats = styles::mats_html(st, &fins, 1);
        assert!(mats.contains("class=\"chip on\" data-smat=\"1\""));
        assert_eq!(mats.matches("class=\"dot\"").count(), 1);
        assert!(mats.contains("background:#ff0000"));
        fins[2] = None;
        assert!(styles::mats_html(st, &fins, 0).contains("<em>not made yet</em>"));
        let shapes = styles::shapes_html("teapot", false);
        assert_eq!(shapes.matches("data-shape=").count(), 8);
        assert!(shapes.contains("data-shape=\"teapot\" class=\"on\"") && shapes.contains("data-showall=\"0\" class=\"on\""));
        let refs = styles::refs_html(&s, st);
        assert_eq!(refs.matches("<img loading=\"lazy\" decoding=\"async\"").count(), 5);
        assert!(refs.contains("src=\"dev-refs/WK05-painterly-3d/1.jpg\"") && refs.contains(styles::REFS_MISSING));
        let d = data();
        let fam = d.family("styles").unwrap();
        let rows = vec![row(ParamDef::f32("kit.material_view.style.roughness", 0.5).range(0.0, 1.0), "finish", 0)];
        let html = panel_html(&d, fam, &rows, &[ParamValue::F32(0.5)], &fam.prefixes, &[], true);
        assert!(html.contains("data-sreset=\"one\"") && html.contains("data-sreset=\"all\""));
        assert!(!html.contains("data-reset=\"finish\""));
    }
}
