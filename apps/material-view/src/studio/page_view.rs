//! WP6, every target: what the studio page shows and what each of its
//! controls does, as pure functions, so they are tested natively.
//!
//! - The markup of each part of the mockup's layout (`*_html`): the style
//!   bar, the materials bar, the asset browser, the "3D view coming" card,
//!   the inspector (material settings, the style's controls, global
//!   lighting), the side column (current combination, presets), the
//!   variations strip and the one-line notice. Every value comes from the
//!   catalogues and the selection record; nothing names a default shape.
//! - [`action`]: every control's `data-act` (and `data-v`) to what it does:
//!   a [`StudioCommand`] for the kit, a page-only change ([`UiAct`]), a
//!   material switch, export or save. The test lists every control the
//!   markup has and the command each pushes.

use std::fmt::Write as _;

use evani_kit_material_view::catalogue::{self as kcat, Library};
use evani_kit_material_view::studio::evani_studio::Compat;
use evani_kit_material_view::studio::evani_studio::compat::{compat, part_compat};
use evani_kit_material_view::studio::evani_studio::resolve::{BoundBy, part_bindings, resolve};
use evani_kit_material_view::studio::evani_studio::state::Because;
use evani_kit_material_view::studio::variations::Variation;
use evani_kit_material_view::studio::{Catalogs, Notice, Pick, Selection, StudioCommand, StudioState};

/// The asset browser's tabs: id, label, icon, the asset categories it holds.
pub const TABS: [(&str, &str, &str, &[&str]); 6] = [
    ("shapes", "Shapes", "◇", &["shapes"]),
    ("animals", "Animals", "♞", &["animals"]),
    ("props", "Props", "⚱", &["objects", "furniture", "vehicles", "fantasy", "scifi"]),
    ("buildings", "Buildings", "⌂", &["buildings"]),
    ("nature", "Nature", "♣", &["plants", "land"]),
    ("people", "People", "☺", &["people"]),
];

/// The light presets the lighting panel shows, in the mockup's order.
pub const LIGHTS: [&str; 4] = ["studio", "sunset", "daylight", "night"];

/// Materials the chips row holds at most (the rest are under "More").
pub const MAX_CHIPS: usize = 48;

/// The page's own state (what is open, typed, filtered): never part of the
/// record, never in a cache key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageUi {
    /// The asset browser's tab (`all`: every category).
    pub tab: String,
    /// The open asset category (its thumbnails shown).
    pub open: Option<String>,
    pub ready_only: bool,
    pub asset_q: String,
    pub mat_q: String,
    /// The materials dropdown's category (empty: all).
    pub mat_cat: String,
    /// The full material list ("More") is open.
    pub more: bool,
    /// Narrow windows: which panel the dock under the view shows.
    pub dock: String,
    pub export_menu: bool,
}

impl Default for PageUi {
    fn default() -> Self {
        Self {
            tab: String::new(),
            open: None,
            ready_only: false,
            asset_q: String::new(),
            mat_q: String::new(),
            mat_cat: String::new(),
            more: false,
            dock: "assets".into(),
            export_menu: false,
        }
    }
}

/// A page-only change.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAct {
    Tab(String),
    OpenCategory(String),
    ReadyOnly,
    AssetQuery(String),
    MatQuery(String),
    MatCategory(String),
    /// "All": the default chips again (search and category cleared).
    MatAll,
    MatMore,
    /// "+ Add material": the full material list, its search focused.
    AddMaterial,
    /// The style bar's arrows: scroll one page left (-1) or right (1).
    ScrollStyles(i32),
    Dock(String),
    ExportMenu,
}

/// What a control does.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Cmd(StudioCommand),
    Ui(UiAct),
    /// A material switch of the record's material (a recipe number), by
    /// param name: an edit remembered for this combination.
    Switch { name: String, value: f32 },
    /// Export the combination's recipe file (download).
    Export,
    /// Copy the combination's recipe to the clipboard.
    CopyRecipe,
    /// Save the combination as a preset (`StudioCommand::Save` with its name).
    Save,
}

/// Control `kind` (its `data-act`), with `v` (its `data-v`) and `input`
/// (an input's or select's value), to what it does. `None`: not a control.
pub fn action(kind: &str, v: &str, input: &str) -> Option<Act> {
    let num = || input.trim().parse::<f32>().ok().filter(|x| x.is_finite());
    let cmd = |c: StudioCommand| Some(Act::Cmd(c));
    let ui = |u: UiAct| Some(Act::Ui(u));
    match kind {
        "style" => cmd(StudioCommand::Pick(Pick::Style(v.into()))),
        "asset" => cmd(StudioCommand::Pick(Pick::Asset(v.into()))),
        "mat" => cmd(StudioCommand::Pick(Pick::Material(v.into()))),
        "part" => cmd(StudioCommand::Pick(Pick::Part(if v.is_empty() { None } else { Some(v.into()) }))),
        "show-on" => cmd(StudioCommand::Pick(Pick::ShowOn(v.into()))),
        "var" => cmd(StudioCommand::Variation(v.into())),
        "light" => cmd(StudioCommand::Light(v.into())),
        "quality" => cmd(StudioCommand::Quality(input.into())),
        "control" => num().map(|value| Act::Cmd(StudioCommand::Control { id: v.into(), value })),
        "reset" => cmd(StudioCommand::Reset),
        "open" => cmd(StudioCommand::Open(v.into())),
        "switch" => num().map(|value| Act::Switch { name: v.into(), value }),
        "save" => Some(Act::Save),
        "export" => Some(Act::Export),
        "export-copy" => Some(Act::CopyRecipe),
        "export-menu" => ui(UiAct::ExportMenu),
        "tab" => ui(UiAct::Tab(v.into())),
        "cat" => ui(UiAct::OpenCategory(v.into())),
        "ready-only" => ui(UiAct::ReadyOnly),
        "asset-q" => ui(UiAct::AssetQuery(input.into())),
        "mat-q" => ui(UiAct::MatQuery(input.into())),
        "mat-cat" => ui(UiAct::MatCategory(input.into())),
        "mat-all" => ui(UiAct::MatAll),
        "mat-more" => ui(UiAct::MatMore),
        "add-mat" => ui(UiAct::AddMaterial),
        "scroll" => ui(UiAct::ScrollStyles(if v.starts_with('-') { -1 } else { 1 })),
        "dock" => ui(UiAct::Dock(v.into())),
        _ => None,
    }
}

/// Apply a page-only change to the page's state.
pub fn apply_ui(ui: &mut PageUi, a: UiAct) {
    match a {
        UiAct::Tab(t) => {
            ui.open = None;
            ui.tab = t;
        }
        UiAct::OpenCategory(c) => ui.open = if ui.open.as_deref() == Some(c.as_str()) { None } else { Some(c) },
        UiAct::ReadyOnly => ui.ready_only = !ui.ready_only,
        UiAct::AssetQuery(q) => ui.asset_q = q,
        UiAct::MatQuery(q) => ui.mat_q = q,
        UiAct::MatCategory(c) => ui.mat_cat = c,
        UiAct::MatAll => {
            ui.mat_q.clear();
            ui.mat_cat.clear();
            ui.more = false;
        }
        UiAct::MatMore => ui.more = !ui.more,
        UiAct::AddMaterial => ui.more = true,
        UiAct::ScrollStyles(_) => {}
        UiAct::Dock(d) => ui.dock = d,
        UiAct::ExportMenu => ui.export_menu = !ui.export_menu,
    }
}

/// `s` safe inside HTML text and attribute values.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// A style's icon on the style bar (by its short name).
pub fn style_icon(short: &str) -> &'static str {
    match short {
        "Toon" => "◕",
        "Cosy" => "♨",
        "Cel" => "◑",
        "Anime" => "✦",
        "Painterly" => "✎",
        "Low-poly" => "◆",
        "Diorama" => "▲",
        "Voxel" => "▣",
        "Pixel" => "▦",
        "Pixel HD" => "▩",
        "Retro 3D" => "◩",
        "Rubber-hose" => "➰",
        "Storybook" => "❦",
        "Watercolour" => "❀",
        "Ink" => "✒",
        "Gothic" => "✠",
        "Comic" => "✸",
        "Paper" => "✉",
        "Yarn" => "❂",
        "Clay" => "⬮",
        "Dither" => "▒",
        "Silhouette" => "◐",
        "Neon" => "✧",
        "Realism" => "◉",
        _ => "◇",
    }
}

/// An asset category's icon.
pub fn category_icon(cat: &str) -> &'static str {
    match cat {
        "shapes" => "◇",
        "animals" => "♞",
        "people" => "☺",
        "buildings" => "⌂",
        "plants" => "♣",
        "land" => "⛰",
        "vehicles" => "⛟",
        "furniture" => "⑁",
        "objects" => "⚱",
        "fantasy" => "✧",
        "scifi" => "⌬",
        _ => "◇",
    }
}

/// A CSS colour for a material's swatch.
pub fn swatch(c: &Catalogs, material: &str) -> String {
    match c.materials.get(material).and_then(|m| m.colour) {
        Some([r, g, b]) => format!("rgb({r},{g},{b})"),
        None => "#8a8f98".into(),
    }
}

/// `#rrggbb` of a material's colour.
pub fn hex(c: &Catalogs, material: &str) -> Option<String> {
    c.materials.get(material).and_then(|m| m.colour).map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
}

/// An asset's rendered thumbnail, as the page addresses it
/// (`models/thumbs/<id>.png`; `None`: the asset has none yet).
pub fn thumb_url(c: &Catalogs, asset: &str) -> Option<String> {
    c.assets.asset(asset).and_then(|a| a.thumbnail.as_ref()).map(|t| format!("{}{t}", evani_render::model::stream::FOLDER))
}

/// An asset's picture: its rendered thumbnail, tinted to `tint`'s colour
/// when given (the same asset in another material), else in its own
/// colours; the category's icon on the material's swatch when it has no
/// thumbnail yet.
pub fn asset_pic(c: &Catalogs, asset: &str, tint: Option<&str>, swatch_of: &str) -> String {
    match thumb_url(c, asset) {
        Some(url) => {
            let url = esc(&url);
            match tint {
                Some(m) => format!(
                    r#"<span class="pic img tinted" style="--sw:{}"><img src="{url}" alt="" draggable="false"><i class="tint" style="-webkit-mask-image:url('{url}');mask-image:url('{url}')"></i></span>"#,
                    swatch(c, m)
                ),
                None => format!(r#"<span class="pic img"><img src="{url}" alt="" draggable="false"></span>"#),
            }
        }
        None => format!(r#"<span class="pic" style="--sw:{}">{}</span>"#, swatch(c, swatch_of), c.assets.asset(asset).map_or("◇", |a| category_icon(&a.category))),
    }
}

/// A tile's picture: the engine's drawing of the tile's record (the asset
/// in that material and the style; studio/tiles.rs paints `canvas
/// data-tile` once it is in) over the recoloured thumbnail, which shows
/// until then, so a tile is never an empty box. `tile` is the record's
/// page id (`c`, `v:<material>`, `s:<saved name>`).
pub fn tile_pic(c: &Catalogs, asset: &str, material: &str, tile: &str) -> String {
    let pic = asset_pic(c, asset, Some(material), material);
    let canvas = format!(r#"<canvas class="tc" data-tile="{}" width="1" height="1" aria-hidden="true"></canvas></span>"#, esc(tile));
    match pic.strip_suffix("</span>") {
        Some(head) => format!("{head}{canvas}"),
        None => pic,
    }
}

/// Characters a material's name may take on a tile (two short lines) before
/// it is shortened.
pub const TILE_NAME_MAX: usize = 20;
/// The same for a saved preset's longer name ("Toon Horse · Horse coat, bay").
pub const SAVED_NAME_MAX: usize = 30;

/// `name` cut at a word boundary to at most `max` characters including a "…"
/// (never inside a word, never ending on a comma or dash); whole when it
/// fits. A lone first word longer than `max` stays whole.
pub fn shorten(name: &str, max: usize) -> String {
    let name = name.trim();
    if name.chars().count() <= max {
        return name.to_string();
    }
    let mut kept = String::new();
    for w in name.split_whitespace() {
        let next = if kept.is_empty() { w.to_string() } else { format!("{kept} {w}") };
        if next.chars().count() + 1 > max && !kept.is_empty() {
            break;
        }
        kept = next;
    }
    let kept = kept.trim_end_matches([',', ';', ':', '-', '·', ' ']);
    if kept == name { kept.to_string() } else { format!("{kept}…") }
}

/// A material's own catalogue name on a tile ("Horse coat, bay"), shortened
/// at a word boundary when long (the tile's tooltip has the full name).
pub fn tile_material(c: &Catalogs, material: &str) -> String {
    shorten(material_name(c, material), TILE_NAME_MAX)
}

/// Is `material` the asset's own (the tile wears a "default" tag)?
fn is_default(c: &Catalogs, asset: &str, material: &str) -> bool {
    c.assets.asset(asset).is_some_and(|a| a.default_material == material)
}

/// The material ball: the material itself, drawn by the engine (studio/
/// balls.rs: its catalogue row on a sphere through the real material
/// pipeline, in the scene's light and the open style), painted into the
/// canvas once its picture is in. Until then the canvas is empty and the
/// ball's soft shaded colour shows through, as a loading state.
pub fn material_ball(c: &Catalogs, material: &str) -> String {
    let tip = esc(material_name(c, material));
    format!(r#"<div class="st-ball" style="--sw:{}" title="{tip}">{}</div>"#, swatch(c, material), ball_canvas(material, 0))
}

/// The engine-drawn ball's canvas for `material` (`pri` 0 for the
/// inspector and the card, painted first; 1 for the chips).
pub fn ball_canvas(material: &str, pri: u8) -> String {
    format!(r#"<canvas class="bc" data-ball="{}" data-pri="{pri}" width="1" height="1" aria-hidden="true"></canvas>"#, esc(material))
}

/// The ball alone, as the card in the 3D view shows it (the same engine
/// picture as the inspector's).
pub fn ball_layers(_c: &Catalogs, material: &str) -> Option<String> {
    Some(ball_canvas(material, 0))
}

fn style_short<'a>(c: &'a Catalogs, id: &'a str) -> &'a str {
    c.styles.style(id).map_or(id, |s| s.short.as_str())
}

fn asset_name<'a>(c: &'a Catalogs, id: &'a str) -> &'a str {
    c.assets.asset(id).map_or(id, |a| a.name.as_str())
}

fn material_name<'a>(c: &'a Catalogs, id: &'a str) -> &'a str {
    c.materials.get(id).map_or(id, |m| m.name.as_str())
}

/// "Voxel + Bay coat + Horse": the combination's short title.
pub fn title(c: &Catalogs, s: &Selection) -> String {
    let fam = c.materials.get(&s.material).map_or_else(|| material_name(c, &s.material).to_string(), |m| kcat::sub_name(&m.subcategory));
    format!("{} + {} + {}", style_short(c, &s.style), fam, asset_name(c, &s.asset))
}

/// The name a saved combination gets: "Voxel Horse · Horse coat, bay".
pub fn saved_name(c: &Catalogs, s: &Selection) -> String {
    format!("{} {} · {}", style_short(c, &s.style), asset_name(c, &s.asset), material_name(c, &s.material))
}

/// The style bar: every art style, the record's one marked.
pub fn styles_html(c: &Catalogs, current: &str) -> String {
    let mut h = String::from(r#"<button class="st-arrow" data-act="scroll" data-v="-1" title="Scroll the styles left" aria-label="Scroll the styles left">‹</button><div class="st-styles-row" role="tablist" aria-label="Art style">"#);
    for s in &c.styles.styles {
        let on = s.id == current;
        let _ = write!(
            h,
            r#"<button class="st-style{}" role="tab" aria-selected="{on}" data-act="style" data-v="{}" title="{}"><span class="ic">{}</span><span class="tx">{}</span></button>"#,
            if on { " on" } else { "" },
            esc(&s.id),
            esc(&s.name),
            style_icon(&s.short),
            esc(&s.short)
        );
    }
    h.push_str(r#"</div><button class="st-arrow" data-act="scroll" data-v="1" title="Scroll the styles right" aria-label="Scroll the styles right">›</button>"#);
    h
}

/// Why `material` doesn't go on the record's asset (`None`: it does).
fn why_not(c: &Catalogs, asset: &str, material: &str) -> Option<String> {
    let (a, m) = (c.assets.asset(asset)?, c.materials.get(material)?);
    let level = compat(c, a, m);
    evani_kit_material_view::studio::variations::reason(c, &a.class, &level)
}

/// The materials the chips row shows: with a search, its ready hits; else
/// one material per family (the record's own where it is in that family),
/// the families that belong on the record's asset first.
pub fn chip_materials(c: &Catalogs, lib: Option<&Library>, ui: &PageUi, sel: &Selection) -> Vec<String> {
    let ready = |id: &str| c.materials.get(id).is_some_and(|m| m.ready() && m.status != "alias");
    let in_cat = |id: &str| ui.mat_cat.is_empty() || c.materials.get(id).is_some_and(|m| m.category == ui.mat_cat);
    let q = ui.mat_q.trim();
    if !q.is_empty() {
        let Some(lib) = lib else { return Vec::new() };
        return lib
            .search(q, &kcat::Filter::default())
            .iter()
            .filter_map(|h| lib.cat.rows.get(h.row).map(|r| r.id.clone()))
            .filter(|id| ready(id) && in_cat(id))
            .take(MAX_CHIPS)
            .collect();
    }
    let asset = c.assets.asset(&sel.asset);
    let cur_sub = c.materials.get(&sel.material).map(|m| m.subcategory.clone());
    // (family, first material, its rank: 0 native here, 1 goes on it, 2 not, size)
    let mut fams: Vec<(String, String, u8, usize)> = Vec::new();
    for m in c.materials.rows.iter().filter(|m| ready(&m.id) && in_cat(&m.id)) {
        let rank = match asset.map(|a| compat(c, a, m)) {
            Some(Compat::Native) => 0,
            Some(l) if l.applies() => 1,
            _ => 2,
        };
        match fams.iter_mut().find(|f| f.0 == m.subcategory) {
            Some(f) => {
                f.2 = f.2.min(rank);
                f.3 += 1;
            }
            None => fams.push((m.subcategory.clone(), m.id.clone(), rank, 1)),
        }
    }
    if let Some(sub) = cur_sub
        && let Some(f) = fams.iter_mut().find(|f| f.0 == sub)
    {
        f.1 = sel.material.clone();
    }
    fams.sort_by(|a, b| a.2.cmp(&b.2).then(b.3.cmp(&a.3)).then(a.0.cmp(&b.0)));
    fams.into_iter().map(|f| f.1).take(MAX_CHIPS).collect()
}

/// Why `material` doesn't go on part `part` of the record's asset.
fn why_not_on_part(c: &Catalogs, asset: &str, part: &str, material: &str) -> Option<String> {
    let (a, m) = (c.assets.asset(asset)?, c.materials.get(material)?);
    let p = a.part(part)?;
    match part_compat(c, a, p, m) {
        Compat::NoRegion => Some(format!("Can't go on the {}", p.label.to_lowercase())),
        level => evani_kit_material_view::studio::variations::reason(c, &a.class, &level),
    }
}

/// One round material chip (for the selected part when `part` is set).
fn chip(c: &Catalogs, sel: &Selection, part: Option<&str>, id: &str, label: &str) -> String {
    let (on, why) = match part {
        Some(p) => (sel.part_material(c, p).as_deref() == Some(id), why_not_on_part(c, &sel.asset, p, id)),
        None => (id == sel.material, why_not(c, &sel.asset, id)),
    };
    let tip = match &why {
        Some(w) => format!("{} · {w}", material_name(c, id)),
        None => material_name(c, id).to_string(),
    };
    format!(
        r#"<button class="st-chip{}{}" data-act="mat" data-v="{}" title="{}"><i style="--sw:{}">{}</i><span>{}</span></button>"#,
        if on { " on" } else { "" },
        if why.is_some() { " foreign" } else { "" },
        esc(id),
        esc(&tip),
        swatch(c, id),
        ball_canvas(id, 1),
        esc(label)
    )
}

/// The materials bar: the category dropdown, the search, "All", the chips
/// and "More".
#[cfg(test)] pub fn mats_html(c: &Catalogs, ui: &PageUi, sel: &Selection, chips: &[String]) -> String {
    mats_html_for(c, ui, sel, None, chips)
}

/// [`mats_html`] with part `part` selected: its chip is marked, the ones
/// that can't go on it say why.
pub fn mats_html_for(c: &Catalogs, ui: &PageUi, sel: &Selection, part: Option<&str>, chips: &[String]) -> String {
    let mut h = String::from(r#"<label class="st-matsel" title="Material category"><span class="sr">Material category</span><select data-act="mat-cat"><option value="">Materials</option>"#);
    for (id, name) in kcat::CATEGORIES {
        if c.materials.rows.iter().any(|m| m.category == id && m.ready()) {
            let _ = write!(h, r#"<option value="{id}"{}>{}</option>"#, if ui.mat_cat == id { " selected" } else { "" }, esc(name));
        }
    }
    let _ = write!(
        h,
        r#"</select></label><label class="st-search"><span class="ic">⌕</span><input type="search" data-act="mat-q" placeholder="Search materials…" value="{}" aria-label="Search materials"></label><button class="st-all{}" data-act="mat-all" title="Every family again">All</button><div class="st-chips">"#,
        esc(&ui.mat_q),
        if ui.mat_q.is_empty() && ui.mat_cat.is_empty() { " on" } else { "" }
    );
    let searching = !ui.mat_q.trim().is_empty();
    for id in chips {
        let label = match c.materials.get(id) {
            Some(m) if !searching => kcat::sub_name(&m.subcategory),
            _ => material_name(c, id).to_string(),
        };
        h.push_str(&chip(c, sel, part, id, &label));
    }
    if chips.is_empty() {
        h.push_str(r#"<span class="st-none">No ready material matches.</span>"#);
    }
    let _ = write!(h, r#"</div><button class="st-more{}" data-act="mat-more" aria-expanded="{}" title="Every material, by family">More ▾</button>"#, if ui.more { " on" } else { "" }, ui.more);
    h
}

/// The full material list ("More"), by family, in the dropdown's category.
pub fn more_html(c: &Catalogs, ui: &PageUi, sel: &Selection) -> String {
    let mut fams: Vec<(&str, Vec<&str>)> = Vec::new();
    for m in c.materials.rows.iter().filter(|m| m.ready() && m.status != "alias" && (ui.mat_cat.is_empty() || m.category == ui.mat_cat)) {
        match fams.iter_mut().find(|f| f.0 == m.subcategory) {
            Some(f) => f.1.push(&m.id),
            None => fams.push((&m.subcategory, vec![&m.id])),
        }
    }
    let mut h = format!(r#"<div class="st-more-head"><b>Every material{}</b><button data-act="mat-more" title="Close the list">Close</button></div>"#, if ui.mat_cat.is_empty() { String::new() } else { format!(" · {}", esc(&kcat::category_name(&ui.mat_cat))) });
    for (sub, ids) in fams {
        let _ = write!(h, r#"<section><h4>{} <small>{}</small></h4><div class="st-more-row">"#, esc(&kcat::sub_name(sub)), ids.len());
        for id in ids {
            h.push_str(&chip(c, sel, None, id, material_name(c, id)));
        }
        h.push_str("</div></section>");
    }
    h
}

/// Which tab an asset category is under.
pub fn tab_of(cat: &str) -> &'static str {
    TABS.iter().find(|t| t.3.contains(&cat)).map_or("props", |t| t.0)
}

/// The asset browser: tabs, search and filter, the categories with counts,
/// and the open category's thumbnails.
/// `source` is the loader's word on the record's asset ("Loading…"; empty
/// when it is ready), shown on its tile.
pub fn assets_html(c: &Catalogs, ui: &PageUi, sel: &Selection, source: &str) -> String {
    let cur_cat = c.assets.asset(&sel.asset).map_or("animals", |a| a.category.as_str());
    let has_assets = |k: &str| c.assets.assets.iter().any(|a| a.category == k);
    // The open category: the one clicked, else the chosen tab's first one
    // with assets (its first at all), else the record's.
    let tab_first = |t: &str| {
        let cats: Vec<&str> = c.assets.categories.iter().map(|k| k.id.as_str()).filter(|k| TABS.iter().any(|x| x.0 == t && x.3.contains(k))).collect();
        cats.iter().copied().find(|k| has_assets(k)).or_else(|| cats.first().copied())
    };
    let all_open = ui.open.is_none() && ui.tab == "all";
    let open_cat: Option<&str> = match (&ui.open, ui.tab.as_str()) {
        (Some(o), _) => Some(o.as_str()),
        (None, "all") => None,
        (None, "") => Some(cur_cat),
        (None, t) => tab_first(t),
    };
    let tab = if all_open { "all" } else { open_cat.map_or("all", tab_of) };
    let mut h = String::from(r#"<div class="st-tabs" role="tablist" aria-label="Asset kinds">"#);
    for (id, label, icon, _) in TABS {
        let _ = write!(h, r#"<button class="st-tab{}" role="tab" aria-selected="{}" data-act="tab" data-v="{id}" title="{label}"><span class="ic">{icon}</span><span class="tx">{label}</span></button>"#, if tab == id { " on" } else { "" }, tab == id);
    }
    let _ = write!(h, r#"<button class="st-tab st-tab-all{}" data-act="tab" data-v="all" title="Every category open" aria-label="Every category open"><span class="ic">▦</span></button></div>"#, if tab == "all" { " on" } else { "" });
    let _ = write!(
        h,
        r#"<div class="st-find"><label class="st-search"><span class="ic">⌕</span><input type="search" data-act="asset-q" placeholder="Search assets…" value="{}" aria-label="Search assets"></label><button class="st-filter{}" data-act="ready-only" aria-pressed="{}" title="{}">⏚</button></div><div class="st-cats">"#,
        esc(&ui.asset_q),
        if ui.ready_only { " on" } else { "" },
        ui.ready_only,
        if ui.ready_only { "Showing built assets only (click for every asset)" } else { "Show built assets only" }
    );
    let q = kcat::fold(ui.asset_q.trim());
    let words: Vec<&str> = q.split_whitespace().collect();
    let matches = |a: &&evani_kit_material_view::studio::evani_studio::assets::AssetRow| {
        let hay = kcat::fold(&format!("{} {} {} {}", a.name, a.search_words, a.subcategory, a.id));
        words.iter().all(|w| hay.contains(w)) && (!ui.ready_only || a.status == evani_kit_material_view::studio::evani_studio::assets::AssetStatus::Ready)
    };
    // Every category is listed with its count (the mockup's list); the open
    // one shows its thumbnails.
    let mut shown = 0;
    for cat in &c.assets.categories {
        let rows: Vec<_> = c.assets.assets.iter().filter(|a| a.category == cat.id).filter(matches).collect();
        if (!words.is_empty() || ui.ready_only) && rows.is_empty() {
            continue;
        }
        shown += 1;
        let open = !words.is_empty() || (all_open && !rows.is_empty()) || open_cat == Some(cat.id.as_str());
        let _ = write!(
            h,
            r#"<button class="st-cat{}" data-act="cat" data-v="{}" aria-expanded="{open}" title="{}"><span class="ic">{}</span><span class="nm">{}</span><span class="n">{}</span></button>"#,
            if open { " on" } else { "" },
            esc(&cat.id),
            esc(&cat.name),
            category_icon(&cat.id),
            esc(&cat.name),
            rows.len()
        );
        if open {
            if rows.is_empty() {
                h.push_str(r#"<p class="st-none">Nothing here yet: these assets are planned.</p>"#);
            } else {
                h.push_str(r#"<div class="st-grid">"#);
                for a in rows {
                    let on = a.id == sel.asset;
                    use evani_kit_material_view::studio::evani_studio::assets::AssetStatus as S;
                    let (badge, cls) = match a.status {
                        S::Ready if on && !source.is_empty() => (source, ""),
                        S::Ready => ("", ""),
                        S::Building => ("Not built yet", " unbuilt"),
                        S::Planned => ("Planned", " unbuilt"),
                    };
                    let _ = write!(
                        h,
                        r#"<button class="st-thumb{}{cls}" data-act="asset" data-v="{}" title="{}{}">{}<span class="nm">{}</span>{}</button>"#,
                        if on { " on" } else { "" },
                        esc(&a.id),
                        esc(&a.name),
                        match a.status {
                            S::Ready if badge.is_empty() => String::new(),
                            S::Ready => format!(" · {}", esc(badge)),
                            _ => format!(" · {badge}: its model is being made, it shows as soon as it lands"),
                        },
                        asset_pic(c, &a.id, None, &a.default_material),
                        esc(&a.name),
                        if badge.is_empty() { String::new() } else { format!(r#"<em>{}</em>"#, esc(badge)) }
                    );
                }
                h.push_str("</div>");
            }
        }
    }
    if shown == 0 {
        h.push_str(r#"<p class="st-none">No asset matches.</p>"#);
    }
    h.push_str("</div>");
    h
}

/// The viewport's own card while the studio's 3D scene isn't in this build.
pub fn coming_html(c: &Catalogs, sel: &Selection, source: &str) -> String {
    format!(
        r#"<div class="st-coming" role="status"><span class="pic" style="--sw:{}">{}</span><b title="{t}">{t}</b><span>Style: {}</span><span>Material: {}</span><span>Asset: {}{}</span><p>3D view coming: the studio's 3D scene is still being built. Your picks are kept and show here as soon as it lands.</p></div>"#,
        swatch(c, &sel.material),
        c.assets.asset(&sel.asset).map_or("◇", |a| category_icon(&a.category)),
        esc(c.styles.style(&sel.style).map_or(sel.style.as_str(), |s| s.name.as_str())),
        esc(material_name(c, &sel.material)),
        esc(asset_name(c, &sel.asset)),
        if source.is_empty() { String::new() } else { format!(" ({})", esc(source)) },
        t = esc(&title(c, sel)),
    )
}

/// One slider of the inspector.
#[derive(Debug, Clone, PartialEq)]
pub struct Slider {
    /// `data-act` and `data-v`: `switch` + param name, or `control` + id.
    pub act: &'static str,
    pub key: String,
    pub label: String,
    pub tip: String,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub value: f32,
}

/// The record's material's switches (its recipe's numbers), each at
/// `value(param name)` or else the row's own number.
pub fn material_sliders(lib: &Library, material: &str, value: &dyn Fn(&str) -> Option<f32>) -> Vec<Slider> {
    let Some((_, row)) = lib.find(material) else { return Vec::new() };
    let row = kcat::working_row(&lib.cat, row);
    let Ok(recipe) = lib.cat.recipe_for(&row) else { return Vec::new() };
    let own = |n: &str| row.params.get("numbers").and_then(|x| x.get(n)).and_then(serde_json::Value::as_f64).map(|x| x as f32);
    recipe
        .numbers
        .iter()
        .map(|n| {
            let name = kcat::number_param(&n.name);
            let v = value(&name).or_else(|| own(&n.name)).unwrap_or(n.default).clamp(n.min, n.max);
            let label = n.name.replace('_', " ");
            let mut l = label.chars();
            let label = l.next().map_or(String::new(), |f| f.to_uppercase().chain(l).collect());
            Slider { act: "switch", tip: name.clone(), key: name, label, min: n.min, max: n.max, step: ((n.max - n.min) / 100.0).max(0.001), value: v }
        })
        .collect()
}

/// The record's style's controls, at the record's values.
pub fn style_sliders(c: &Catalogs, state: &StudioState) -> Vec<Slider> {
    let Some(s) = c.styles.style(&state.selection.style) else { return Vec::new() };
    let values = state.style_controls();
    s.controls
        .iter()
        .filter_map(|id| {
            let d = evani_kit_material_view::studio::evani_studio::styles::control(id)?;
            let v = values.get(id).copied().or_else(|| s.control_default(id)).unwrap_or(d.min);
            Some(Slider { act: "control", key: id.clone(), label: d.label.into(), tip: d.what.into(), min: d.min, max: d.max, step: d.step, value: v })
        })
        .collect()
}

fn fmt_num(v: f32) -> String {
    format!("{v:.2}")
}

fn slider_html(h: &mut String, s: &Slider) {
    let _ = write!(
        h,
        r#"<div class="st-row"><div class="lab"><span title="{}">{}</span><b data-out="{}">{}</b></div><input type="range" data-act="{}" data-v="{}" min="{}" max="{}" step="{}" value="{}" aria-label="{}"></div>"#,
        esc(&s.tip),
        esc(&s.label),
        esc(&s.key),
        fmt_num(s.value),
        s.act,
        esc(&s.key),
        s.min,
        s.max,
        s.step,
        s.value,
        esc(&s.label)
    );
}

/// What each part of the record's asset wears: (part id, label, material or
/// `None` for the model's own colour, whether the user gave it its own).
pub fn part_list(c: &Catalogs, sel: &Selection) -> Vec<(String, String, Option<String>, bool)> {
    let Ok(r) = resolve(c, sel, &std::collections::BTreeMap::new(), None) else { return Vec::new() };
    part_bindings(c, &r).into_iter().map(|(p, b)| (p.id.clone(), p.label.clone(), b.material.clone(), matches!(b.from, BoundBy::Part(_)))).collect()
}

/// The inspector's parts list: "All parts", then each part with its own
/// material ball; the selected one marked. Picking a material then goes on
/// that part only.
pub fn parts_html(c: &Catalogs, state: &StudioState) -> String {
    let list = part_list(c, &state.selection);
    if list.len() < 2 {
        return String::new();
    }
    let all = state.part.is_none();
    let mut h = format!(
        r#"<section class="st-sec st-parts"><h3>Parts<span class="st-hint">{}</span></h3><div class="st-plist" role="radiogroup" aria-label="Which part a material goes on"><button class="st-part st-part-all{}" data-act="part" data-v="" role="radio" aria-checked="{all}" title="A material goes on every part made for it"><span class="pb all">✱</span><span class="tx">All parts</span></button>"#,
        if all { "Pick a part, then a material" } else { "Materials now go on this part" },
        if all { " on" } else { "" },
    );
    for (id, label, material, own) in &list {
        let on = state.part.as_deref() == Some(id.as_str());
        let (ball, name) = match material {
            Some(m) => (format!(r#"<span class="pb" style="--sw:{}">{}</span>"#, swatch(c, m), ball_canvas(m, 1)), material_name(c, m).to_string()),
            None => (r#"<span class="pb own"></span>"#.to_string(), "Model's own colour".to_string()),
        };
        let _ = write!(
            h,
            r#"<button class="st-part{}" data-act="part" data-v="{}" role="radio" aria-checked="{on}" title="{}: {}{}">{ball}<span class="tx"><b>{}</b><small>{}</small></span></button>"#,
            if on { " on" } else { "" },
            esc(id),
            esc(label),
            esc(&name),
            if *own { " (your pick)" } else { "" },
            esc(label),
            esc(&name),
        );
    }
    h.push_str("</div></section>");
    h
}

/// The inspector: the parts list, material settings (preview ball, base
/// colour, switches, reset), the style's controls, global lighting and
/// quality.
pub fn inspector_html(c: &Catalogs, state: &StudioState, mat: &[Slider], style: &[Slider], lights: &[(String, String)], tiers: &[(String, String)]) -> String {
    let sel = &state.selection;
    let mut h = format!(r#"<div class="st-insp-main">{}"#, parts_html(c, state));
    let _ = write!(
        h,
        r#"<section class="st-sec"><h3>Material settings<button class="st-small" data-act="reset" title="Back to the material's own values and the asset's framing">Reset</button></h3><p class="st-editing" title="These settings change {n}">Editing <b>{n}</b></p>{}<div class="st-row"><div class="lab"><span>Base colour</span></div><div class="st-hex" title="The material's own colour"><i style="background:{}"></i><b>{}</b></div></div>"#,
        material_ball(c, &sel.material),
        swatch(c, &sel.material),
        hex(c, &sel.material).unwrap_or_else(|| "pattern".into()),
        n = esc(material_name(c, &sel.material)),
    );
    if mat.is_empty() {
        h.push_str(r#"<p class="st-none">This material has no switches.</p>"#);
    }
    for s in mat {
        slider_html(&mut h, s);
    }
    h.push_str("</section>");
    if !style.is_empty() {
        let _ = write!(h, r#"<section class="st-sec"><h3>Style: {}</h3>"#, esc(style_short(c, &sel.style)));
        for s in style {
            slider_html(&mut h, s);
        }
        h.push_str("</section>");
    }
    // Global lighting stays in view under the scrolling settings (the mockup's inspector).
    h.push_str(r#"</div><section class="st-sec st-lighting"><h3>Global lighting</h3><div class="st-lights">"#);
    for id in LIGHTS {
        let Some((_, name)) = lights.iter().find(|(i, _)| i == id) else { continue };
        let on = state.view.light == id;
        let short = name.split_whitespace().next().unwrap_or(name);
        let _ = write!(h, r#"<button class="st-light{}" data-act="light" data-v="{id}" aria-pressed="{on}" title="{}"><span class="pic lt-{id}"></span><span class="tx">{}</span></button>"#, if on { " on" } else { "" }, esc(name), esc(short));
    }
    h.push_str("</div>");
    if !tiers.is_empty() {
        h.push_str(r#"<label class="st-quality"><span>Quality</span><select data-act="quality">"#);
        for (id, label) in std::iter::once(&("auto".to_string(), "Auto".to_string())).chain(tiers) {
            let _ = write!(h, r#"<option value="{}"{}>{}</option>"#, esc(id), if state.view.quality == *id { " selected" } else { "" }, esc(label));
        }
        h.push_str("</select></label>");
    }
    h.push_str("</section>");
    h
}

/// The side column: the current combination (thumbnail, name, tags,
/// Export, Save to library) and the presets (saved combinations).
pub fn side_html(c: &Catalogs, state: &StudioState, menu: bool, tiles: &[Variation]) -> String {
    let sel = &state.selection;
    let fam = c.materials.get(&sel.material).map_or_else(String::new, |m| kcat::sub_name(&m.subcategory));
    let name = format!("{} {}", style_short(c, &sel.style), asset_name(c, &sel.asset));
    let mut h = format!(
        r#"<section class="st-sec st-combo"><h3>Current combination</h3><div class="st-cur">{}<div class="st-cur-tx"><b title="{n}">{n}</b><span title="{m}">{m}</span></div><div class="st-tags"><span>{}</span><span title="{f}">{f}</span><span>{}</span></div></div><div class="st-export"><button class="st-primary" data-act="export" title="Download this combination's recipe file (style, material, asset and your settings)">⤓ Export</button><button class="st-primary st-caret" data-act="export-menu" aria-expanded="{menu}" title="More export choices">▾</button></div>"#,
        tile_pic(c, &sel.asset, &sel.material, "c"),
        esc(style_short(c, &sel.style)),
        esc(asset_name(c, &sel.asset)),
        n = esc(&name),
        m = esc(material_name(c, &sel.material)),
        f = esc(&fam),
    );
    // Each part's material (the combination is style + asset + these).
    let parts = part_list(c, sel);
    if parts.len() > 1 {
        h.push_str(r#"<ul class="st-psum" aria-label="Each part's material">"#);
        for (_, label, material, own) in &parts {
            let name = material.as_deref().map_or("Model's own colour", |m| material_name(c, m));
            let _ = write!(
                h,
                r#"<li{} title="{l}: {n}"><i style="--sw:{}"></i><span>{l}</span><b>{n}</b></li>"#,
                if *own { r#" class="own""# } else { "" },
                material.as_deref().map_or_else(|| "#222".to_string(), |m| swatch(c, m)),
                l = esc(label),
                n = esc(name),
            );
        }
        h.push_str("</ul>");
    }
    if menu {
        h.push_str(r#"<div class="st-menu"><button data-act="export" title="Download the recipe file">Recipe file (.json)</button><button data-act="export-copy" title="Copy the recipe to the clipboard">Copy recipe</button></div>"#);
    }
    h.push_str(r#"<button class="st-save" data-act="save" title="Keep this combination and its settings in your presets">⊕ Save to library</button></section><section class="st-sec"><h3>Presets</h3>"#);
    // Your saved combinations first, then (up to six tiles) this asset in
    // the materials that go on it, in this style: one click applies one.
    let suggested: Vec<&Variation> = tiles.iter().filter(|t| t.why.is_none()).take(PRESETS.saturating_sub(state.saved.len())).collect();
    if state.saved.is_empty() && suggested.is_empty() {
        h.push_str(r#"<p class="st-none">Saved combinations show here. Use Save to library.</p>"#);
    } else {
        h.push_str(r#"<div class="st-presets">"#);
        for s in state.saved.iter().rev() {
            let on = s.selection == *sel;
            let _ = write!(
                h,
                r#"<button class="st-preset{}" data-act="open" data-v="{}" title="{}">{}<span class="nm">{}</span></button>"#,
                if on { " on" } else { "" },
                esc(&s.name),
                esc(&s.name),
                tile_pic(c, &s.selection.asset, &s.selection.material, &format!("s:{}", s.name)),
                esc(&shorten(&s.name, SAVED_NAME_MAX))
            );
        }
        for t in suggested {
            let label = tile_material(c, &t.material);
            let _ = write!(
                h,
                r#"<button class="st-preset{}" data-act="var" data-v="{}" title="{} {}: {}">{}<span class="nm">{}</span></button>"#,
                if t.current { " on" } else { "" },
                esc(&t.material),
                esc(style_short(c, &sel.style)),
                esc(asset_name(c, &sel.asset)),
                esc(material_name(c, &t.material)),
                tile_pic(c, &sel.asset, &t.material, &format!("v:{}", t.material)),
                esc(&label)
            );
        }
        h.push_str("</div>");
    }
    h.push_str("</section>");
    h
}

/// Preset tiles the side column shows at most (saved ones first).
pub const PRESETS: usize = 6;

/// The variations strip: the same asset and style in other materials, the
/// current one ticked, the ones that don't go on it greyed with the reason.
pub fn vars_html(c: &Catalogs, sel: &Selection, tiles: &[Variation]) -> String {
    let mut h = String::from(r#"<h3>Variations</h3><div class="st-vars-row">"#);
    for t in tiles {
        let tip = match &t.why {
            Some(w) => format!("{} · {w}", t.name),
            None if t.current => format!("{} (current)", t.name),
            None => t.name.clone(),
        };
        let _ = write!(
            h,
            r#"<button class="st-var{}{}" data-act="var" data-v="{}" title="{}"{}>{}<span class="nm">{}</span>{}{}</button>"#,
            if t.current { " on" } else { "" },
            if t.why.is_some() { " foreign" } else { "" },
            esc(&t.material),
            esc(&tip),
            if t.why.is_some() { r#" aria-disabled="true""# } else { "" },
            tile_pic(c, &sel.asset, &t.material, &format!("v:{}", t.material)),
            esc(&tile_material(c, &t.material)),
            if is_default(c, &sel.asset, &t.material) { r#"<em class="dflt" title="The asset's own material">default</em>"# } else { "" },
            match (&t.why, t.current) {
                (_, true) => r#"<em class="tick">✓</em>"#.to_string(),
                (Some(w), _) => format!(r#"<em>{}</em>"#, esc(w)),
                _ => String::new(),
            }
        );
    }
    h.push_str(r#"<button class="st-var st-add" data-act="add-mat" title="Pick any material from the full list"><span class="pic">＋</span><span class="nm">Add material</span></button></div>"#);
    h
}

/// The notice line: what the last pick did, or why it was refused.
pub fn note_html(c: &Catalogs, state: &StudioState, error: Option<&str>) -> String {
    let label = |asset: &str| c.assets.asset(asset).and_then(|a| c.assets.class_of(a)).map_or_else(|| asset_name(c, asset).to_string(), |k| k.label.clone());
    let plural = |class: &str| c.assets.class(class).map_or(class.to_string(), |k| k.plural.clone());
    let text = match &state.notice {
        Notice::None => String::new(),
        Notice::ShownOn { material, asset } => format!("Showing {} on {}", esc(material_name(c, material)), esc(&label(asset))),
        Notice::NotApplied { material, made_for, offer } => format!(
            r#"{} is made for {}: not applied <button class="st-small" data-act="show-on" data-v="{}">Show on {}</button>"#,
            esc(material_name(c, material)),
            esc(&plural(made_for)),
            esc(offer),
            esc(&label(offer))
        ),
        Notice::NoPlace { material, asset } => format!("{} can't go on {}", esc(material_name(c, material)), esc(&label(asset))),
        Notice::NotOnPart { material, asset, part } => format!(
            "{} can't go on the {}'s {}",
            esc(material_name(c, material)),
            esc(&asset_name(c, asset).to_lowercase()),
            esc(&c.assets.asset(asset).and_then(|a| a.part(part)).map_or(part.clone(), |p| p.label.to_lowercase()))
        ),
        Notice::Substituted { from, to, because } => format!(
            "{} instead of {} ({})",
            esc(material_name(c, to)),
            esc(material_name(c, from)),
            match because {
                Because::Remembered => "the one you last used on it",
                Because::Closest => "the closest colour that fits",
                Because::Default => "the asset's own",
            }
        ),
        Notice::NotBuilt { material } => format!("{} isn't built yet: nothing changed", esc(material_name(c, material))),
    };
    match error {
        Some(e) if text.is_empty() => format!(r#"<span class="err">Couldn't do that: {}</span>"#, esc(e)),
        Some(e) => format!(r#"{text} · <span class="err">{}</span>"#, esc(e)),
        None => text,
    }
}

/// The combination's recipe file (Export): the record, the style's control
/// values, this combination's settings and the cache keys.
pub fn recipe_json(c: &Catalogs, state: &StudioState, keys: Option<[String; 3]>) -> String {
    let sel = &state.selection;
    let v = serde_json::json!({
        "evani_studio_combination": 1,
        "name": saved_name(c, sel),
        "selection": sel,
        "style": { "id": sel.style, "name": c.styles.style(&sel.style).map(|s| s.name.clone()), "controls": state.style_controls() },
        "material": { "id": sel.material, "name": material_name(c, &sel.material), "colour": hex(c, &sel.material) },
        "asset": { "id": sel.asset, "name": asset_name(c, &sel.asset) },
        "settings": state.settings(),
        "keys": keys.map(|[g, v, p]| serde_json::json!({ "geometry": g, "variant": v, "pass": p })),
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// The recipe file's name: `voxel-horse-horse-coat-bay.studio.json`.
pub fn recipe_file_name(c: &Catalogs, sel: &Selection) -> String {
    let raw = format!("{} {} {}", style_short(c, &sel.style), asset_name(c, &sel.asset), material_name(c, &sel.material));
    let mut out = String::new();
    for ch in raw.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    format!("{}.studio.json", out.trim_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_kit_material_view::studio::{catalogs, variations};

    fn every_part(c: &Catalogs, lib: &Library, state: &StudioState, ui: &PageUi) -> String {
        let sel = &state.selection;
        let chips = chip_materials(c, Some(lib), ui, sel);
        let mats = material_sliders(lib, &sel.material, &|_| None);
        let lights: Vec<(String, String)> = LIGHTS.iter().map(|l| (l.to_string(), l.to_string())).collect();
        let tiers = vec![("low".to_string(), "Low".to_string())];
        let tiles = variations(c, sel, 12, 3);
        [
            styles_html(c, &sel.style),
            mats_html(c, ui, sel, &chips),
            more_html(c, ui, sel),
            assets_html(c, ui, sel, ""),
            coming_html(c, sel, "Loading…"),
            inspector_html(c, state, &mats, &style_sliders(c, state), &lights, &tiers),
            side_html(c, state, true, &tiles),
            vars_html(c, sel, &tiles),
            note_html(c, state, Some("x")),
            r#"<button data-act="dock" data-v="assets">"#.into(),
        ]
        .concat()
    }

    fn acts(html: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for part in html.split("data-act=\"").skip(1) {
            let kind = part.split('"').next().unwrap_or_default().to_string();
            let v = part.split("data-v=\"").nth(1).filter(|_| part.find("data-v=\"").is_some_and(|i| i < part.find('>').unwrap_or(0))).and_then(|s| s.split('"').next()).unwrap_or_default().to_string();
            out.push((kind, v));
        }
        out
    }

    /// Every control the page draws does something real: each `data-act` in
    /// every part maps to a studio command, a material switch, export or
    /// save, or a page change; and the table of what each pushes.
    #[test]
    fn every_control_does_something() {
        let c = catalogs().unwrap();
        let lib = Library::bundled().unwrap();
        let mut state = StudioState::new(&c).unwrap();
        state.apply(&c, Pick::Style("WK08".into())).unwrap();
        state.save("Voxel Horse");
        let ui = PageUi { more: true, tab: "all".into(), ..PageUi::default() };
        let html = every_part(&c, &lib, &state, &ui);
        let found = acts(&html);
        assert!(found.len() > 60, "{}", found.len());
        let mut kinds: Vec<&str> = found.iter().map(|(k, _)| k.as_str()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        for (kind, v) in &found {
            assert!(action(kind, v, "0.5").is_some(), "control {kind}={v} does nothing");
        }
        // The table: every kind of control and the command it pushes.
        let table: Vec<(&str, Act)> = kinds.iter().map(|k| (*k, action(k, "x", "0.5").unwrap_or(Act::Save))).collect();
        let expect = [
            ("style", Act::Cmd(StudioCommand::Pick(Pick::Style("x".into())))),
            ("asset", Act::Cmd(StudioCommand::Pick(Pick::Asset("x".into())))),
            ("mat", Act::Cmd(StudioCommand::Pick(Pick::Material("x".into())))),
            ("var", Act::Cmd(StudioCommand::Variation("x".into()))),
            ("light", Act::Cmd(StudioCommand::Light("x".into()))),
            ("quality", Act::Cmd(StudioCommand::Quality("0.5".into()))),
            ("control", Act::Cmd(StudioCommand::Control { id: "x".into(), value: 0.5 })),
            ("reset", Act::Cmd(StudioCommand::Reset)),
            ("open", Act::Cmd(StudioCommand::Open("x".into()))),
            ("switch", Act::Switch { name: "x".into(), value: 0.5 }),
            ("save", Act::Save),
            ("export", Act::Export),
            ("export-copy", Act::CopyRecipe),
            ("part", Act::Cmd(StudioCommand::Pick(Pick::Part(Some("x".into()))))),
        ];
        assert_eq!(action("part", "", ""), Some(Act::Cmd(StudioCommand::Pick(Pick::Part(None)))), "the All parts row");
        for (k, a) in expect {
            assert!(table.contains(&(k, a.clone())), "{k} should push {a:?}; the page has {kinds:?}");
        }
        for k in ["show-on", "tab", "cat", "ready-only", "asset-q", "mat-q", "mat-cat", "mat-all", "mat-more", "add-mat", "scroll", "dock", "export-menu"] {
            assert!(action(k, "x", "y").is_some(), "{k}");
        }
        assert_eq!(action("nothing", "", ""), None);
    }

    /// The roof's parts list: All parts and its four parts, each with its own
    /// material ball and name; picking the gable marks it, its chip state
    /// follows the gable, the card lists every part, and a gable-only pick
    /// shows in the list and the card as the user's own.
    #[test]
    fn the_inspector_lists_the_parts_and_a_part_takes_its_own_material() {
        let c = catalogs().unwrap();
        let mut state = StudioState::new(&c).unwrap();
        state.apply(&c, Pick::Asset("building.roof".into())).unwrap();
        let h = parts_html(&c, &state);
        assert_eq!(h.matches(r#"data-act="part""#).count(), 5, "{h}");
        assert!(h.contains("All parts") && h.contains("Ridge cap") && h.contains("Gable ends") && h.contains("Fascia and barge boards"));
        assert!(h.contains(r#"data-ball="brick.red.common""#), "the gable's ball is the engine-drawn brick");
        state.apply(&c, Pick::Part(Some("gable".into()))).unwrap();
        state.apply(&c, Pick::Material("plaster.white".into())).unwrap();
        let h = parts_html(&c, &state);
        assert!(h.contains(r#"class="st-part on" data-act="part" data-v="gable""#), "{h}");
        assert!(h.contains("Plaster, white"));
        let chips = vec!["plaster.white".to_string(), "horse.coat.bay".to_string()];
        let bar = mats_html_for(&c, &PageUi::default(), &state.selection, state.part.as_deref(), &chips);
        assert!(bar.contains(r#"class="st-chip on" data-act="mat" data-v="plaster.white""#), "{bar}");
        assert!(bar.contains(r#"class="st-chip foreign" data-act="mat" data-v="horse.coat.bay""#), "{bar}");
        let card = side_html(&c, &state, false, &[]);
        assert_eq!(card.matches("<li").count(), 4, "{card}");
        assert!(card.contains(r#"class="own" title="Gable ends: Plaster, white""#), "{card}");
        // A single-part asset shows no list.
        state.apply(&c, Pick::Asset("shape.sphere".into())).unwrap();
        assert!(parts_html(&c, &state).is_empty());
        // The notice for a pick the part can't take.
        let mut s = StudioState::new(&c).unwrap();
        s.apply(&c, Pick::Asset("animal.horse".into())).unwrap();
        s.apply(&c, Pick::PartMaterial { part: "hooves".into(), material: "plaster.white".into() }).unwrap();
        assert!(note_html(&c, &s, None).contains("can't go on the horse's hooves"), "{}", note_html(&c, &s, None));
    }

    /// Every style is on the bar and the record's one is marked; the chips
    /// come from the real catalogue (one per family by default, the record's
    /// material among them); the browser lists the catalogue's assets with
    /// the unbuilt ones marked, never another object.
    #[test]
    fn the_parts_show_the_catalogues() {
        let c = catalogs().unwrap();
        let lib = Library::bundled().unwrap();
        let state = StudioState::new(&c).unwrap();
        let sel = &state.selection;
        let bar = styles_html(&c, &sel.style);
        assert_eq!(bar.matches("data-act=\"style\"").count(), c.styles.styles.len());
        assert_eq!(bar.matches("st-style on").count(), 1);
        let ui = PageUi::default();
        let chips = chip_materials(&c, Some(&lib), &ui, sel);
        assert!(chips.len() > 12 && chips.contains(&sel.material), "{chips:?}");
        let subs: std::collections::BTreeSet<_> = chips.iter().filter_map(|id| c.materials.get(id).map(|m| m.subcategory.clone())).collect();
        assert_eq!(subs.len(), chips.len(), "one chip per family");
        let found = chip_materials(&c, Some(&lib), &PageUi { mat_q: "brick".into(), ..PageUi::default() }, sel);
        let bricks = found.iter().filter(|id| id.starts_with("brick.")).count();
        assert!(found.first().is_some_and(|id| id.starts_with("brick.")) && bricks * 2 > found.len(), "{found:?}");
        let all = assets_html(&c, &PageUi { tab: "all".into(), open: Some("animals".into()), ..PageUi::default() }, sel, "");
        for a in c.assets.assets.iter().filter(|a| a.category == "animals") {
            assert!(all.contains(&format!("data-v=\"{}\"", a.id)), "{}", a.id);
        }
        let unbuilt = c.assets.assets.iter().filter(|a| a.category == "animals" && a.status != evani_kit_material_view::studio::evani_studio::assets::AssetStatus::Ready).count();
        assert_eq!(all.matches("st-thumb on unbuilt").count() + all.matches("st-thumb unbuilt").count(), unbuilt);
        assert!(title(&c, sel).ends_with("+ Horse"), "{}", title(&c, sel));
        assert!(coming_html(&c, sel, "").contains("3D view coming"));
        assert!(assets_html(&c, &PageUi::default(), sel, "Loading…").contains("<em>Loading…</em>"), "the record's tile says it is loading");
    }

    /// The mockup's layout: the browser lists every category with its
    /// count whatever the tab, ready assets show their rendered thumbnail
    /// (never a placeholder icon), and the variation and preset tiles show
    /// the same asset in their material, with short names.
    #[test]
    fn the_page_follows_the_mockup() {
        let c = catalogs().unwrap();
        let state = StudioState::new(&c).unwrap();
        let sel = &state.selection;
        for ui in [PageUi::default(), PageUi { tab: "buildings".into(), ..PageUi::default() }] {
            let html = assets_html(&c, &ui, sel, "");
            for cat in &c.assets.categories {
                assert!(html.contains(&format!("data-act=\"cat\" data-v=\"{}\"", cat.id)), "tab {:?} lists {}", ui.tab, cat.id);
            }
        }
        let html = assets_html(&c, &PageUi::default(), sel, "");
        for a in c.assets.assets.iter().filter(|a| a.category == "animals" && a.thumbnail.is_some()) {
            assert!(html.contains(&format!("src=\"models/thumbs/{}.png\"", a.id)), "{} shows its thumbnail", a.id);
        }
        let tiles = variations(&c, sel, 12, 3);
        let strip = vars_html(&c, sel, &tiles);
        assert!(strip.contains("class=\"dflt\"") && strip.contains("class=\"tint\""), "{strip}");
        let side = side_html(&c, &state, false, &tiles);
        assert_eq!(side.matches("<button class=\"st-preset").count(), PRESETS.min(tiles.iter().filter(|t| t.why.is_none()).count()));
        assert_eq!(tile_material(&c, "horse.coat.bay"), "Horse coat, bay");
        let horse = StudioState::new(&c).unwrap();
        assert_eq!(vars_html(&c, &horse.selection, &variations(&c, &horse.selection, 12, 3)).matches("class=\"dflt\"").count(), 1, "one default tag");
    }

    /// The guard against tiles going back to recoloured thumbnails: every
    /// variation, preset and current-combination tile carries the engine's
    /// picture (`canvas data-tile`, studio/tiles.rs paints it over the
    /// thumbnail once drawn), naming the record it shows, and the page shows
    /// the canvas only once painted (the thumbnail stays until then).
    #[test]
    fn every_tile_is_the_engine_drawn_asset() {
        let c = catalogs().unwrap();
        let mut state = StudioState::new(&c).unwrap();
        state.save("Mine");
        let sel = state.selection.clone();
        let tiles = variations(&c, &sel, 12, 3);
        let strip = vars_html(&c, &sel, &tiles);
        for t in &tiles {
            let canvas = format!(r#"<canvas class="tc" data-tile="v:{}""#, t.material);
            assert!(strip.contains(&canvas), "the {} variation is not engine-drawn: {strip}", t.material);
        }
        assert_eq!(strip.matches("<canvas class=\"tc\"").count(), tiles.len(), "one picture per tile");
        let side = side_html(&c, &state, false, &tiles);
        assert!(side.contains(r#"<canvas class="tc" data-tile="c""#), "the current combination: {side}");
        assert!(side.contains(r#"data-tile="s:Mine""#), "a saved preset: {side}");
        assert_eq!(side.matches("<button class=\"st-preset").count(), side.matches("<canvas class=\"tc\"").count() - 1, "every preset tile has its picture");
        for id in ["c", &format!("v:{}", sel.material), "s:Mine"] {
            assert!(super::super::tiles::Tile::parse(id).is_some(), "{id}");
        }
        // The thumbnail is under the picture until it is in.
        let pic = tile_pic(&c, &sel.asset, &sel.material, "c");
        assert!(pic.find("<img").zip(pic.find("<canvas")).is_some_and(|(i, k)| i < k) && pic.ends_with("</canvas></span>"), "{pic}");
        let css = include_str!("page.rs");
        assert!(css.contains(".pic>.tc{") && css.contains("opacity:0") && css.contains(".pic>.tc[data-k]{opacity:1}"));
    }

    /// The studio page names no default shape: its object comes only from
    /// the record (WP6 guard; the legacy viewer's sphere-first menu, its
    /// `cat.shape` auto ball and `StyleView.shape` stay out of the studio).
    #[test]
    fn the_studio_page_names_no_default_shape() {
        for (file, src) in [("page.rs", include_str!("page.rs")), ("page_view.rs", include_str!("page_view.rs"))] {
            let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            let code = code.split("#[cfg(test)]").next().unwrap_or_default();
            for banned in ["\"sphere\"", "\"ball\"", "shape.sphere", "Shape::ALL", "cat.shape", "kcat::SHAPE", "StyleView"] {
                assert!(!code.contains(banned), "{file} names {banned}");
            }
        }
    }

    #[test]
    fn page_changes_and_the_recipe_file() {
        let mut ui = PageUi::default();
        apply_ui(&mut ui, UiAct::MatQuery("oak".into()));
        apply_ui(&mut ui, UiAct::AddMaterial);
        assert!(ui.more);
        apply_ui(&mut ui, UiAct::MatAll);
        assert!(ui.mat_q.is_empty() && !ui.more);
        apply_ui(&mut ui, UiAct::OpenCategory("animals".into()));
        apply_ui(&mut ui, UiAct::OpenCategory("animals".into()));
        assert_eq!(ui.open, None, "a second click closes the category");
        let c = catalogs().unwrap();
        let state = StudioState::new(&c).unwrap();
        let json: serde_json::Value = serde_json::from_str(&recipe_json(&c, &state, None)).unwrap();
        assert_eq!(json["selection"]["asset"], "animal.horse");
        assert_eq!(recipe_file_name(&c, &state.selection), "toon-horse-horse-coat-bay.studio.json");
        assert_eq!(esc("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }

    /// Shortening happens at a word boundary, with a "…", and never leaves a
    /// dangling comma or cuts a word.
    #[test]
    fn long_names_shorten_at_a_word_boundary() {
        assert_eq!(shorten("Horse coat, bay", TILE_NAME_MAX), "Horse coat, bay");
        assert_eq!(shorten("Standing-seam metal roof", 24), "Standing-seam metal roof");
        assert_eq!(shorten("Hair, tight coils, black", 20), "Hair, tight coils…");
        assert_eq!(shorten("Brick, herringbone path wall", 17), "Brick…");
        assert_eq!(shorten("Unbelievably-long-single-word", 10), "Unbelievably-long-single-word");
        assert_eq!(shorten("Unbelievably-long-first word", 10), "Unbelievably-long-first…");
    }

    /// The (material id, shown name) of every tile button of `act` in `html`.
    fn tile_names(html: &str, class: &str) -> Vec<(String, String)> {
        html.split("<button class=\"").skip(1).filter(|b| b.starts_with(class) && b.contains("data-act=\"var\"")).filter_map(|b| {
            let id = b.split("data-v=\"").nth(1)?.split('"').next()?.to_string();
            let nm = b.split("<span class=\"nm\">").nth(1)?.split("</span>").next()?.to_string();
            Some((id, nm.replace("&amp;", "&").replace("&#39;", "'")))
        }).collect()
    }

    /// Preset and variation names are the material's own catalogue name (the
    /// start of it, at a word boundary, with "…" and the full name in the
    /// tooltip when long): never "Bark on Wall" for "Log wall, bark on",
    /// never a lone "Brown" or a word the material's name doesn't have.
    #[test]
    fn preset_and_variation_names_are_never_misleading() {
        let c = catalogs().unwrap();
        let mut checked = 0;
        for asset in ["building.wall", "building.roof", "land.patch", "animal.horse", "animal.cow", "animal.dog"] {
            for style in ["WK01", "WK13", "WK08"] {
                let mut state = StudioState::new(&c).unwrap();
                state.apply(&c, Pick::Asset(asset.into())).unwrap();
                state.apply(&c, Pick::Style(style.into())).unwrap();
                let sel = state.selection.clone();
                let tiles = variations(&c, &sel, 12, 3);
                let strip = vars_html(&c, &sel, &tiles);
                let side = side_html(&c, &state, false, &tiles);
                for (html, class, what) in [(&strip, "st-var", "variation"), (&side, "st-preset", "preset")] {
                    let names = tile_names(html, class);
                    assert!(!names.is_empty(), "{asset} {style}: no {what} tiles");
                    for (id, shown) in names {
                        let full = material_name(&c, &id);
                        let (stem, cut) = match shown.strip_suffix('…') {
                            Some(s) => (s, true),
                            None => (shown.as_str(), false),
                        };
                        assert!(full.starts_with(stem), "{asset} {style}: {what} {shown:?} is not the start of {full:?}");
                        assert!(stem.len() == full.len() || full[stem.len()..].starts_with([' ', ',']), "{asset} {style}: {what} {shown:?} cuts a word of {full:?}");
                        assert_eq!(cut, stem.len() < full.len(), "{asset} {style}: {what} {shown:?} must end in … exactly when {full:?} was cut");
                        if cut {
                            assert!(html.contains(&esc(full)), "{asset} {style}: the tooltip of {shown:?} has the full name {full:?}");
                        }
                        checked += 1;
                    }
                }
                // A saved preset keeps its whole name in the tooltip too.
                state.save("Voxel Horse · Horse coat, steel grey with a very long name");
                let side = side_html(&c, &state, false, &tiles);
                assert!(side.contains("title=\"Voxel Horse · Horse coat, steel grey with a very long name\"") && side.contains("Voxel Horse · Horse coat…"), "{side}");
            }
        }
        assert!(checked > 100, "{checked}");
        assert_eq!(tile_material(&c, "horse.coat.bay"), "Horse coat, bay");
        let horse = StudioState::new(&c).unwrap();
        assert_eq!(vars_html(&c, &horse.selection, &variations(&c, &horse.selection, 12, 3)).matches("class=\"dflt\"").count(), 1, "one default tag");
    }

    /// Every ball the page shows (the inspector's, the card's, each chip's)
    /// is the engine's own picture of the material (a canvas the swatch pass
    /// paints, studio/balls.rs), never a tinted sphere picture or a colour
    /// disc, and it follows the material. A ball made of a tint again (an
    /// `<img>`, a mask, a gradient-only ball) fails here.
    #[test]
    fn every_ball_is_the_engine_drawn_material_never_a_tint() {
        let c = catalogs().unwrap();
        let mut state = StudioState::new(&c).unwrap();
        let tinted = |h: &str| h.contains("<img") || h.contains("mask-image") || h.contains("st-ball plain") || h.contains("<i style=\"background:");
        let html = inspector_html(&c, &state, &[], &[], &[], &[]);
        assert!(html.contains(&format!(r#"<canvas class="bc" data-ball="{}" data-pri="0""#, state.selection.material)), "{html}");
        assert!(!tinted(&material_ball(&c, &state.selection.material)), "the inspector ball is a tint");
        state.apply(&c, Pick::Material("brick.red.common".into())).unwrap();
        let other = inspector_html(&c, &state, &[], &[], &[], &[]);
        assert!(other.contains(r#"data-ball="brick.red.common""#));
        let ui = PageUi { mat_q: "brick".into(), ..PageUi::default() };
        let chips: Vec<String> = ["brick.red.common", "wood.walnut.stock", "horse.coat.bay", "metal.steel.brushed"].map(String::from).to_vec();
        let bar = mats_html(&c, &ui, &state.selection, &chips);
        for m in &chips {
            assert!(bar.contains(&format!(r#"data-ball="{m}" data-pri="1""#)), "the {m} chip is not engine-drawn: {bar}");
        }
        assert!(!tinted(&bar), "a chip is a tint: {bar}");
        assert!(ball_layers(&c, "brick.red.common").is_some_and(|b| !tinted(&b) && b.contains("data-ball")));
        let card = include_str!("scene_hud.rs");
        assert!(card.contains("ball_layers(&d.0, &v.selection.material)"), "the card in the 3D view uses the same ball");
    }
}
