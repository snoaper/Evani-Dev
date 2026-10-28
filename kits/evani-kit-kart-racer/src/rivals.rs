//! Rival karts built from detailed parts (Stage 4, S4-1b; needs the `models`
//! module).
//!
//! Each rival is a recipe in `data/rivals.json`: a chassis model plus named
//! slots (wheels, seat, spoiler, nose, decals) that hide, recolour or attach
//! other models. The recipe is turned into the engine's part variant
//! (`evani_render::model::Variant`, S3-11: `draw_with` hides and recolours
//! the base's named nodes, `anchor_frame` stands each part at an
//! `anchor:<name>` node of the chassis) with the rival's roster colours
//! filled in, so every rival looks its own without a model file of its own.
//!
//! This is drawing only. Sizes, the collision radius and where the driver
//! sits are the kit's own data (`kart_radius`, `draw_kart_parts`); nothing
//! here reads a model back into the simulation, and the same recipe with the
//! same race draws the same picture.
//!
//! A rival is drawn from its parts only when everything it needs has
//! streamed in and the camera is not fading it; until then (and for the
//! players, and with `kit.kart_racer.rival_parts` off) the primitive kart is
//! drawn, so a slow or failed download never leaves a hole. The driver, the
//! steering wheel and column stay primitives; so do the wheels unless the
//! recipe names a wheel model, which then rolls and steers on the chassis's
//! `wheel_hub.*` anchors like the primitive wheels do.
//!
//! The recipe format is described on [`Recipe`]; `docs/CONTENT.md` has the
//! variant rules it builds on.

use std::cell::RefMut;
use std::collections::BTreeMap;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_render::model::{Frame, ModelHandle, ModelId, ModelInstance, Models, Variant};
use evani_render::math::{quat_axis_angle, quat_mul};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::ai::KartSpec;
use crate::draw::{KartParts, RivalKart, kart_pose, yaw_q};

/// A colour in a recipe: linear `[r, g, b]` or `[r, g, b, a]`, or the name of
/// a colour from the rival's roster entry (see [`Tone::NAMES`]).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Tone {
    Rgb([f32; 3]),
    Rgba([f32; 4]),
    Name(String),
}

impl Tone {
    /// The names a recipe may use for a colour.
    pub const NAMES: [&'static str; 7] = ["kart", "kart_light", "kart_deep", "suit", "suit_deep", "white", "steel"];

    /// The colour for `spec`: the kart's roster colour (`karts.json` `color`)
    /// and its lighter and deeper shades as the primitive kart paints them,
    /// the driver's suit, white, or a cool steel grey.
    pub fn resolve(&self, spec: &KartSpec) -> [f32; 4] {
        let c = spec.color;
        let rgb = match self {
            Tone::Rgb([r, g, b]) => [*r, *g, *b],
            Tone::Rgba([r, g, b, a]) => return [*r, *g, *b, *a],
            Tone::Name(n) => match n.as_str() {
                "kart" => c,
                "kart_light" => [0.35 + c[0] * 0.65, 0.35 + c[1] * 0.65, 0.35 + c[2] * 0.65],
                "kart_deep" => [c[0] * 0.72, c[1] * 0.72, c[2] * 0.72],
                "suit" => suit(spec),
                "suit_deep" => suit(spec).map(|x| x * 0.8),
                "steel" => [0.55, 0.55, 0.62],
                _ => [1.0; 3],
            },
        };
        [rgb[0], rgb[1], rgb[2], 1.0]
    }

    fn check(&self, what: &str) -> Result<(), String> {
        match self {
            Tone::Name(n) if !Self::NAMES.contains(&n.as_str()) => Err(format!("rivals.json: {what}: unknown colour \"{n}\" (use {})", Self::NAMES.join(", "))),
            _ => Ok(()),
        }
    }
}

/// The driver's suit colour as the primitive driver wears it.
fn suit(spec: &KartSpec) -> [f32; 3] {
    let c = spec.color;
    spec.suit.unwrap_or([0.25 + c[0] * 0.5, 0.25 + c[1] * 0.5, 0.25 + c[2] * 0.5])
}

/// The chassis: the base model of the variant.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chassis {
    /// Model id (files `models/<id>[.<tier>].glb`).
    pub model: String,
    /// Nodes of the chassis not drawn.
    #[serde(default)]
    pub hide: Vec<String>,
    /// Node name to colour: multiplies the colour of the parts under it.
    #[serde(default)]
    pub paint: BTreeMap<String, Tone>,
}

/// A slot (seat, spoiler, nose, a decal): it may hide or recolour nodes of
/// the chassis and attach a model at one of its anchors.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// A model to attach (needs `anchor`).
    #[serde(default)]
    pub model: Option<String>,
    /// The chassis's `anchor:<name>` node the model stands at.
    #[serde(default)]
    pub anchor: Option<String>,
    /// Colour multiplied into the attached model (default white).
    #[serde(default)]
    pub colour: Option<Tone>,
    /// Chassis nodes this slot's part replaces (hidden).
    #[serde(default)]
    pub hide: Vec<String>,
    /// Chassis nodes this slot recolours.
    #[serde(default)]
    pub paint: BTreeMap<String, Tone>,
}

/// The wheel slot. Without a `model` the chassis's own wheels stay and
/// nothing else is read. With one, the chassis's `hide` nodes go and the
/// model stands at each of `anchors` (the chassis's `wheel_hub.*` nodes),
/// rolling with the kart's distance and, on `.fl` and `.fr`, steering.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wheels {
    #[serde(default)]
    pub model: Option<String>,
    /// The chassis's own wheel nodes the model replaces.
    #[serde(default)]
    pub hide: Vec<String>,
    /// Anchors the wheels stand at; the hub's height is the wheel's radius.
    #[serde(default)]
    pub anchors: Vec<String>,
    #[serde(default)]
    pub colour: Option<Tone>,
    /// Size of the model on the front and rear hubs (1 = as built).
    #[serde(default = "one")]
    pub front_scale: f32,
    #[serde(default = "one")]
    pub rear_scale: f32,
}

fn one() -> f32 {
    1.0
}

/// One rival's build.
///
/// ```json
/// { "chassis": { "model": "kart", "hide": [], "paint": { "body": "kart" } },
///   "wheels":  { "model": "wheel-sport", "hide": ["wheel_fl", "wheel_fr", "wheel_rl", "wheel_rr"],
///                "anchors": ["wheel_hub.fl", "wheel_hub.fr", "wheel_hub.rl", "wheel_hub.rr"] },
///   "seat":    { "model": "seat-race", "anchor": "driver_seat", "colour": "suit_deep" },
///   "spoiler": { "model": "wing-tall", "anchor": "item_slot", "colour": "kart_deep" },
///   "nose":    { "model": "nose-needle", "anchor": "steer_pivot", "colour": "kart_light" },
///   "decals":  [ { "model": "decal-stripe", "anchor": "exhaust.l", "colour": [1, 1, 1] } ] }
/// ```
///
/// Colours are `[r, g, b]` (linear, multiplied into the part) or a name in
/// [`Tone::NAMES`], resolved against the rival's roster entry, so one recipe
/// serves any kart colour. Unknown keys are errors.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub chassis: Chassis,
    #[serde(default)]
    pub wheels: Wheels,
    #[serde(default)]
    pub seat: Option<Slot>,
    #[serde(default)]
    pub spoiler: Option<Slot>,
    #[serde(default)]
    pub nose: Option<Slot>,
    #[serde(default)]
    pub decals: Vec<Slot>,
}

impl Recipe {
    /// The slots that attach or change something on the chassis, in the order
    /// they are drawn.
    fn slots(&self) -> impl Iterator<Item = &Slot> {
        [&self.seat, &self.spoiler, &self.nose].into_iter().flatten().chain(self.decals.iter())
    }

    fn check(&self, name: &str) -> Result<(), String> {
        let at = |s: &str| format!("recipe \"{name}\" {s}");
        for (n, t) in &self.chassis.paint {
            t.check(&at(&format!("chassis.paint.{n}")))?;
        }
        for (i, s) in self.slots().enumerate() {
            if s.model.is_some() != s.anchor.is_some() {
                return Err(format!("rivals.json: {}: a slot needs both \"model\" and \"anchor\", or neither", at(&format!("slot {i}"))));
            }
            if let Some(t) = &s.colour {
                t.check(&at(&format!("slot {i} colour")))?;
            }
            for (n, t) in &s.paint {
                t.check(&at(&format!("slot {i} paint.{n}")))?;
            }
        }
        if self.wheels.model.is_some() && self.wheels.anchors.is_empty() {
            return Err(format!("rivals.json: {}: a wheel model needs \"anchors\"", at("wheels")));
        }
        if let Some(t) = &self.wheels.colour {
            t.check(&at("wheels colour"))?;
        }
        Ok(())
    }

    /// The engine's variant recipe (`Variant::from_json`) for `spec`.
    pub fn variant_json(&self, spec: &KartSpec) -> Value {
        let mut hide: Vec<&str> = self.chassis.hide.iter().map(String::as_str).collect();
        let mut paint = Map::new();
        let mut parts = Vec::new();
        for (n, t) in &self.chassis.paint {
            paint.insert(n.clone(), json!(t.resolve(spec)));
        }
        for s in self.slots() {
            hide.extend(s.hide.iter().map(String::as_str));
            for (n, t) in &s.paint {
                paint.insert(n.clone(), json!(t.resolve(spec)));
            }
            if let (Some(model), Some(anchor)) = (&s.model, &s.anchor) {
                let colour = s.colour.as_ref().map_or([1.0; 4], |t| t.resolve(spec));
                parts.push(json!({ "model": model, "anchor": anchor, "colour": colour }));
            }
        }
        if self.wheels.model.is_some() {
            hide.extend(self.wheels.hide.iter().map(String::as_str));
        }
        json!({ "base": self.chassis.model, "hide": hide, "paint": paint, "parts": parts })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    /// Format version (1).
    version: u32,
    /// The recipe of a rival `karts` does not name.
    #[serde(default)]
    fallback: Option<String>,
    recipes: BTreeMap<String, Recipe>,
    /// Roster name (`karts.json`) to recipe name.
    #[serde(default)]
    karts: BTreeMap<String, String>,
}

/// Most different models all the recipes together may use: the one shared
/// part pack (chassis, wheels, seats, spoilers, noses, decals). Rivals differ
/// by which parts they combine and by colour, not by files, so the download
/// does not grow with the field.
pub const MAX_PACK_MODELS: usize = 12;

impl File {
    /// The distinct model ids every recipe uses.
    fn models(&self) -> std::collections::BTreeSet<&str> {
        let mut out = std::collections::BTreeSet::new();
        for r in self.recipes.values() {
            out.insert(r.chassis.model.as_str());
            out.extend(r.wheels.model.as_deref());
            out.extend(r.slots().filter_map(|s| s.model.as_deref()));
        }
        out
    }
}

/// The parsed `rivals.json`.
#[derive(Debug, Clone)]
pub struct Recipes {
    file: File,
}

impl Recipes {
    /// The distinct model ids the recipes use: the part pack to download.
    pub fn pack(&self) -> Vec<String> {
        self.file.models().into_iter().map(str::to_owned).collect()
    }
}

impl Recipes {
    /// Parse and check `rivals.json`: unknown keys, colour names, slots
    /// missing an anchor, and recipe names that do not exist are errors.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| format!("rivals.json: {e}"))?;
        if file.version != 1 {
            return Err(format!("rivals.json: version {} is not supported (1)", file.version));
        }
        for (name, r) in &file.recipes {
            r.check(name)?;
        }
        for r in file.karts.values().chain(file.fallback.iter()) {
            if !file.recipes.contains_key(r) {
                return Err(format!("rivals.json: no recipe called \"{r}\""));
            }
        }
        // One shared part pack, recoloured and recombined: a rival never
        // brings a model of its own (Chris, 2026-10-20: games stay light).
        let pack = file.models();
        if pack.len() > MAX_PACK_MODELS {
            return Err(format!("rivals.json: {} different models; the shared part pack is at most {MAX_PACK_MODELS} (reuse parts across recipes and recolour them)", pack.len()));
        }
        Ok(Self { file })
    }

    /// Every kart `karts` names is on the roster (a typo would silently leave
    /// that rival on the fallback).
    pub fn check_roster(&self, roster: &[KartSpec]) -> Result<(), String> {
        for name in self.file.karts.keys() {
            if !roster.iter().any(|s| &s.name == name) {
                return Err(format!("rivals.json: \"{name}\" is not on the roster (karts.json)"));
            }
        }
        Ok(())
    }

    /// The recipe for the roster entry `name`: its own, else the fallback.
    pub fn for_kart(&self, name: &str) -> Option<&Recipe> {
        let r = self.file.karts.get(name).or(self.file.fallback.as_ref())?;
        self.file.recipes.get(r)
    }
}

/// The wheel model of a built rival.
#[derive(Debug)]
struct WheelSet {
    model: ModelHandle,
    anchors: Vec<String>,
    colour: [f32; 4],
    front_scale: f32,
    rear_scale: f32,
    cache: Option<WheelCache>,
}

/// The hubs resolved against one file of the chassis.
#[derive(Debug)]
struct WheelCache {
    base: ModelId,
    hubs: Vec<Hub>,
}

#[derive(Debug, Clone, Copy)]
struct Hub {
    anchor: u16,
    front: bool,
    /// The hub's height over the road in the chassis's own units: the wheel's
    /// radius, which sets how fast it turns.
    radius: f32,
}

/// One rival built from its recipe.
#[derive(Debug)]
struct Built {
    variant: Variant,
    base: ModelHandle,
    wheels: Option<WheelSet>,
    /// The recipe's missing names were logged.
    logged: bool,
}

#[derive(Debug, Default)]
enum Entry {
    /// Not looked at yet.
    #[default]
    Unbuilt,
    /// No recipe, or a bad one (logged): the primitive kart.
    Primitive,
    Built(Box<Built>),
}

/// The rivals' recipes and the parts built from them (a resource).
#[derive(Debug)]
pub struct Rivals {
    recipes: Recipes,
    /// By roster index.
    built: Vec<Entry>,
    /// Problems to log (drained into the error log by [`flush_errors`]).
    errors: Vec<String>,
}

impl Rivals {
    pub fn new(recipes: Recipes) -> Self {
        Self { recipes, built: Vec::new(), errors: Vec::new() }
    }

    fn build(&mut self, models: &mut Models, spec: &KartSpec) -> Entry {
        let Some(recipe) = self.recipes.for_kart(&spec.name) else { return Entry::Primitive };
        let variant = match Variant::from_json(models, &recipe.variant_json(spec)) {
            Ok(v) => v,
            Err(e) => {
                self.errors.push(format!("kart racer: rival {}: {e}", spec.name));
                return Entry::Primitive;
            }
        };
        let w = &recipe.wheels;
        let wheels = w.model.as_deref().map(|m| WheelSet {
            model: models.handle(m),
            anchors: w.anchors.clone(),
            colour: w.colour.as_ref().map_or([1.0; 4], |t| t.resolve(spec)),
            front_scale: w.front_scale,
            rear_scale: w.rear_scale,
            cache: None,
        });
        let base = models.handle(&recipe.chassis.model);
        Entry::Built(Box::new(Built { variant, base, wheels, logged: false }))
    }
}

/// Put the error log lines [`Rivals`] collected into the error log.
pub fn flush_errors(world: &World) {
    let Some(mut r) = world.try_res_mut::<Rivals>() else { return };
    if r.errors.is_empty() {
        return;
    }
    let mut log = world.res_mut::<ErrorLog>();
    for e in r.errors.drain(..) {
        log.error(e);
    }
}

/// What the kart draw needs while it draws the karts of one frame: the model
/// pipeline and the rivals' parts.
pub struct RivalDraw<'a> {
    models: RefMut<'a, Models>,
    rivals: RefMut<'a, Rivals>,
    scale: f32,
}

impl<'a> RivalDraw<'a> {
    /// `None` (every kart is primitive) when parts are switched off
    /// (`kit.kart_racer.rival_parts`) or the game has no model pipeline.
    pub(crate) fn acquire(world: &'a World, on: bool, scale: f32) -> Option<Self> {
        if !on {
            return None;
        }
        Some(Self { models: world.try_res_mut::<Models>()?, rivals: world.try_res_mut::<Rivals>()?, scale })
    }

    /// Draw `kart` from its recipe's parts if it can be drawn whole. Returns
    /// the primitive groups still to draw (`None`: the whole primitive kart).
    pub(crate) fn kart(&mut self, kart: &RivalKart<'_>) -> Option<KartParts> {
        // The players' karts are the hero asset's (S4-1a). The models do not
        // take the chase camera's screen-door fade, so a rival being faded is
        // drawn from primitives, which do.
        if kart.k.player.is_some() || kart.fade < 0.999 {
            return None;
        }
        let models: &mut Models = &mut self.models;
        let rivals: &mut Rivals = &mut self.rivals;
        let scale = self.scale;
        let idx = kart.k.spec;
        if rivals.built.len() <= idx {
            rivals.built.resize_with(idx + 1, Entry::default);
        }
        if matches!(rivals.built[idx], Entry::Unbuilt) {
            let e = rivals.build(models, kart.spec);
            rivals.built[idx] = e;
        }
        let Entry::Built(b) = &mut rivals.built[idx] else { return None };
        let errors = &mut rivals.errors;

        // The wheel model has to be ready before the chassis shows with its
        // own wheels hidden.
        let wheel_id = match &b.wheels {
            Some(w) => Some(models.resolve(w.model)?),
            None => None,
        };
        let (q, squash) = kart_pose(kart.k, kart.vyaw, kart.max_speed);
        let at = Frame::new(kart.pos).rotation(q).scale([squash[0] * scale, squash[1] * scale, squash[2] * scale]);
        if !b.variant.draw(models, at) {
            return None;
        }
        if !b.logged {
            b.logged = true;
            if !b.variant.unresolved.is_empty() {
                errors.push(format!("kart racer: rival {}: the chassis lacks {}", kart.spec.name, b.variant.unresolved.join(", ")));
            }
        }
        let (Some(w), Some(wheel_id)) = (&mut b.wheels, wheel_id) else { return Some(KartParts::DRIVER_WHEELS) };
        let Some(base) = models.resolve(b.base) else { return Some(KartParts::DRIVER) };
        if w.cache.as_ref().is_none_or(|c| c.base != base) {
            let mut hubs = Vec::new();
            for name in &w.anchors {
                match models.anchor(base, name).and_then(|a| Some((a, models.anchor_frame(base, a, &Frame::new([0.0; 3]))?))) {
                    Some((anchor, home)) => hubs.push(Hub { anchor, front: name.ends_with(".fl") || name.ends_with(".fr"), radius: home.position[1].max(0.05) }),
                    None => errors.push(format!("kart racer: rival {}: the chassis lacks anchor {name}", kart.spec.name)),
                }
            }
            w.cache = Some(WheelCache { base, hubs });
        }
        let Some(cache) = &w.cache else { return Some(KartParts::DRIVER) };
        for hub in &cache.hubs {
            let Some(f) = models.anchor_frame(base, hub.anchor, &at) else { continue };
            // Steer about the hub's up, then roll about its axle, in the
            // chassis's own turn (the same order the primitive wheels use).
            let steer = if hub.front { yaw_q(-kart.k.steer_vis * 0.5) } else { [0.0, 0.0, 0.0, 1.0] };
            let roll = quat_axis_angle([1.0, 0.0, 0.0], kart.k.wheel_roll * crate::kart::WHEEL_RADIUS / hub.radius);
            let s = if hub.front { w.front_scale } else { w.rear_scale };
            let wheel = Frame { rotation: quat_mul(f.rotation, quat_mul(steer, roll)), scale: [f.scale[0] * s, f.scale[1] * s, f.scale[2] * s], ..f };
            models.draw(ModelInstance::at(wheel_id, &wheel).tint(w.colour));
        }
        Some(KartParts::DRIVER)
    }
}
