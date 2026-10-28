//! evani-geocheck-2d: the 2D half of Evani.Dev's geometry integrity checker.
//!
//! A kit's dump harness writes a JSON scene (see [`scene`] and README.md);
//! the eight checks in [`checks`] are pure functions of that scene and
//! return [`Issue`]s; [`tags`] marks intentional cases; [`report`] writes
//! `report.md`, `report.json` and highlighted crops. Dev-time only, never
//! shipped in a game.

pub mod checks;
pub mod geom;
pub mod image;
pub mod render;
pub mod report;
pub mod scene;
pub mod tags;

use geom::{Geo, Mask};
use scene::{Atlas, Rect, Scene};
use serde::Serialize;

/// Chris's seven 2D checks and the draw-join check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Check {
    /// 1. Collectibles, enemies and props vs solids and decor; air around collectibles.
    Overlaps,
    /// 2. No floating decor; enemies spawn on ground.
    Grounding,
    /// 3. Autotile seams and transitions.
    Seams,
    /// 4. One pixel scale, whole-pixel positions.
    Pixels,
    /// 5. Player in the framing zone; terrain share.
    Camera,
    /// 6. Reachability, coin arcs, hidden hazards, completion.
    Bot,
    /// 7. Score/text popups.
    Popups,
    /// 8. Joins between drawn sprites of one layer: no thin gap where
    ///    strips, tiles or bands meet, at any camera position.
    Joins,
}

impl Check {
    pub const ALL: [Check; 8] = [Check::Overlaps, Check::Grounding, Check::Seams, Check::Pixels, Check::Camera, Check::Bot, Check::Popups, Check::Joins];

    pub fn slug(self) -> &'static str {
        match self {
            Check::Overlaps => "overlaps",
            Check::Grounding => "grounding",
            Check::Seams => "seams",
            Check::Pixels => "pixels",
            Check::Camera => "camera",
            Check::Bot => "bot",
            Check::Popups => "popups",
            Check::Joins => "joins",
        }
    }

    pub fn number(self) -> u8 {
        self as u8 + 1
    }

    pub fn title(self) -> &'static str {
        match self {
            Check::Overlaps => "Overlaps and clear air",
            Check::Grounding => "Floating decor and enemy footing",
            Check::Seams => "Autotile seams",
            Check::Pixels => "Pixel integrity",
            Check::Camera => "Camera framing",
            Check::Bot => "Bot level checks",
            Check::Popups => "Text popups",
            Check::Joins => "Draw joins",
        }
    }
}

/// What a crop shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum CropSource {
    /// The static level around a world rect.
    Level(Rect),
    /// A drawn frame (index into `scene.frames`), cropped to a world rect
    /// (the whole view when `None`).
    Frame(usize, Option<Rect>),
    /// A camera-sweep frame (index into `scene.sweep`), as [`CropSource::Frame`].
    Sweep(usize, Option<Rect>),
}

/// One problem found.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub check: Check,
    /// Specific type, e.g. `clear_air`, `seam`, `coin_arc`.
    #[serde(rename = "type")]
    pub kind: String,
    /// 1 (cosmetic) .. 5 (blocks play).
    pub severity: u8,
    /// Size of the problem within its type (px, s, share), for ranking.
    pub amount: f32,
    pub object: Option<String>,
    /// Tile coordinates.
    pub tile: Option<[i32; 2]>,
    /// World art-pixel rect of the problem.
    pub px: Rect,
    pub tick: Option<u64>,
    pub message: String,
    pub fix: String,
    #[serde(skip)]
    pub crop: CropSource,
    /// World rects to highlight in the crop.
    #[serde(skip)]
    pub highlights: Vec<Rect>,
    /// Set by tagging: the tag's reason.
    pub tagged: Option<String>,
}

impl Issue {
    pub fn new(check: Check, kind: &str, px: Rect) -> Self {
        Self {
            check,
            kind: kind.into(),
            severity: 2,
            amount: 0.0,
            object: None,
            tile: None,
            px,
            tick: None,
            message: String::new(),
            fix: String::new(),
            crop: CropSource::Level(px),
            highlights: vec![px],
            tagged: None,
        }
    }

    pub fn sev(mut self, s: u8, amount: f32) -> Self {
        self.severity = s;
        self.amount = amount;
        self
    }

    pub fn object(mut self, o: &str) -> Self {
        self.object = Some(o.into());
        self
    }

    pub fn tile(mut self, t: [i32; 2]) -> Self {
        self.tile = Some(t);
        self
    }

    pub fn tick(mut self, t: u64) -> Self {
        self.tick = Some(t);
        self
    }

    pub fn msg(mut self, m: String, fix: String) -> Self {
        self.message = m;
        self.fix = fix;
        self
    }

    pub fn crop(mut self, c: CropSource, highlights: Vec<Rect>) -> Self {
        self.crop = c;
        self.highlights = highlights;
        self
    }
}

/// Everything a check reads: the scene, its atlas, the tile shapes and
/// each object's opaque mask.
pub struct Ctx<'a> {
    pub scene: &'a Scene,
    pub atlas: Option<&'a Atlas>,
    pub geo: Geo<'a>,
    pub masks: Vec<Mask>,
}

impl<'a> Ctx<'a> {
    pub fn new(scene: &'a Scene, atlas: Option<&'a Atlas>) -> Self {
        let masks = scene.objects.iter().map(|o| geom::object_mask(o, atlas)).collect();
        Self { scene, atlas, geo: Geo::new(scene), masks }
    }

    pub fn tile_of(&self, p: [f32; 2]) -> [i32; 2] {
        [self.geo.cell(p[0]), self.geo.cell(p[1])]
    }

    pub fn layer_name(&self, l: i32) -> String {
        self.scene.layer_names.get(&l.to_string()).cloned().unwrap_or_else(|| format!("layer {l}"))
    }
}

/// Checks that did not run on this scene, with the reason.
pub fn not_run(scene: &Scene) -> Vec<(Check, &'static str)> {
    let mut v = Vec::new();
    if !scene.pixel_art {
        v.push((Check::Pixels, "the scene declares `pixel_art: false` (smooth art drawn at any size; whole-pixel rules do not apply)"));
        v.push((Check::Joins, "the scene declares `pixel_art: false` (draw joins are checked on the art-pixel grid)"));
    } else if !scene.frames.iter().chain(&scene.sweep).any(|f| f.sprites.is_some()) {
        v.push((Check::Joins, "no frame in the scene lists its drawn sprites (`frames[].sprites` or `sweep`)"));
    }
    v
}

/// Run all eight checks (check 4 only for pixel art), then apply tags.
/// Issues come back sorted by check, then worst first.
pub fn run_all(scene: &Scene, atlas: Option<&Atlas>, extra_tags: &[tags::Tag]) -> Result<Vec<Issue>, String> {
    let ctx = Ctx::new(scene, atlas);
    let mut out = Vec::new();
    out.extend(checks::overlaps::check(&ctx));
    out.extend(checks::grounding::check(&ctx));
    out.extend(checks::seams::check(&ctx));
    // Check 4 is about pixel art; a smooth-art scene says so and skips it
    // (the report lists it as not run).
    if scene.pixel_art {
        out.extend(checks::pixels::check(&ctx));
    }
    out.extend(checks::camera::check(&ctx));
    out.extend(checks::bot::check(&ctx));
    out.extend(checks::popups::check(&ctx));
    out.extend(checks::joins::check(&ctx));
    let mut all_tags = scene.geocheck.clone();
    all_tags.extend_from_slice(extra_tags);
    tags::validate(&all_tags)?;
    tags::apply(&mut out, &all_tags, scene.tile_size);
    out.sort_by(|a, b| a.check.cmp(&b.check).then(b.severity.cmp(&a.severity)).then(b.amount.total_cmp(&a.amount)));
    Ok(out)
}
