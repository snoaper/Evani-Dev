//! The see-through and back-lit surfaces a game shows: what kind each is
//! and where it sits. They come from kit data ([`surfaces_from_json`]), a
//! glTF model ([`crate::gltf`]) or code ([`NatureSurfaces::push`]).

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::camera::CameraPose;
use serde_json::Value;

/// What a surface is. The shape follows from the kind (see the shader's
/// table in `shaders/nature_common.wgsl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Still water inside an ellipse at the surface height (a pond).
    WaterEllipse,
    /// Still water inside a rectangle (a pool, a trough).
    WaterRect,
    /// A thin box of glass (a window pane).
    GlassPane,
    /// A vertical capsule of glass (a bottle, a jar).
    GlassBottle,
    /// An ellipsoid bush or tree crown that glows when the sun is behind it.
    Foliage,
    /// A box region of grass blades over the ground; the tips glow.
    Grass,
    /// A small solid ellipsoid lit through (an ear, a fruit, wax).
    ThinBlob,
    /// A thin box lit through (paper, a sail, a curtain): the thin colour.
    ThinPatch,
    /// An ellipsoid shade lit from inside (a lampshade, a candle).
    Lamp,
    /// A thin box lit through with the foliage colour (a big leaf, a
    /// frond, a petal).
    Leaf,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::WaterEllipse,
        Kind::WaterRect,
        Kind::GlassPane,
        Kind::GlassBottle,
        Kind::Foliage,
        Kind::Grass,
        Kind::ThinBlob,
        Kind::ThinPatch,
        Kind::Lamp,
        Kind::Leaf,
    ];

    /// The shape code the shader reads (`Surf.c.w`).
    pub fn code(self) -> f32 {
        match self {
            Kind::WaterEllipse => 0.0,
            Kind::WaterRect => 1.0,
            Kind::GlassPane => 2.0,
            Kind::GlassBottle => 3.0,
            Kind::Foliage => 4.0,
            Kind::Grass => 5.0,
            Kind::ThinBlob => 6.0,
            Kind::ThinPatch => 7.0,
            Kind::Lamp => 8.0,
            Kind::Leaf => 9.0,
        }
    }

    /// The `evani_kind` name (glTF material extras, kit data).
    pub fn name(self) -> &'static str {
        match self {
            Kind::WaterEllipse => "water",
            Kind::WaterRect => "water_rect",
            Kind::GlassPane => "glass",
            Kind::GlassBottle => "glass_bottle",
            Kind::Foliage => "foliage",
            Kind::Grass => "grass",
            Kind::ThinBlob => "thin",
            Kind::ThinPatch => "thin_patch",
            Kind::Lamp => "lamp",
            Kind::Leaf => "leaf",
        }
    }

    /// A kind from its `evani_kind` name (and a few everyday synonyms).
    pub fn from_name(name: &str) -> Option<Kind> {
        let n = name.trim().to_ascii_lowercase().replace('-', "_");
        Some(match n.as_str() {
            "water" | "water_ellipse" | "pond" => Kind::WaterEllipse,
            "water_rect" | "pool" => Kind::WaterRect,
            "glass" | "glass_pane" | "window" => Kind::GlassPane,
            "glass_bottle" | "bottle" | "jar" => Kind::GlassBottle,
            "foliage" | "leaves" | "bush" => Kind::Foliage,
            "grass" => Kind::Grass,
            "thin" | "thin_blob" | "ear" | "wax" | "fruit" => Kind::ThinBlob,
            "thin_patch" | "paper" | "sail" | "curtain" => Kind::ThinPatch,
            "leaf" | "frond" | "petal" => Kind::Leaf,
            "lamp" | "lampshade" | "candle" => Kind::Lamp,
            _ => return None,
        })
    }

    /// Water and glass stand in front of what the depth shows: the game
    /// leaves them out of its solid draw. The others light a solid surface.
    pub fn see_through(self) -> bool {
        matches!(
            self,
            Kind::WaterEllipse | Kind::WaterRect | Kind::GlassPane | Kind::GlassBottle
        )
    }
}

/// One surface. Water: `centre.y` is the water level and `half` its x/z
/// reach (`half[1]` unused). A bottle: `half[0]` is its radius and `half[1]`
/// half the height of its straight part. Everything else: the half size of
/// its box or ellipsoid. `yaw` turns it about +y (radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    pub kind: Kind,
    pub centre: [f32; 3],
    pub half: [f32; 3],
    pub yaw: f32,
}

impl Surface {
    pub fn new(kind: Kind, centre: [f32; 3], half: [f32; 3]) -> Self {
        Self {
            kind,
            centre,
            half,
            yaw: 0.0,
        }
    }

    pub fn with_yaw(mut self, yaw: f32) -> Self {
        self.yaw = yaw;
        self
    }

    /// This surface moved by a model instance's placement: turned by `yaw`
    /// about +y, scaled by `scale`, then moved to `at`.
    pub fn placed(self, at: [f32; 3], yaw: f32, scale: f32) -> Self {
        let (s, c) = yaw.sin_cos();
        let p = self.centre.map(|v| v * scale);
        Self {
            centre: [
                at[0] + c * p[0] + s * p[2],
                at[1] + p[1],
                at[2] - s * p[0] + c * p[2],
            ],
            half: self.half.map(|v| v * scale),
            yaw: self.yaw + yaw,
            ..self
        }
    }

    /// Radius of a sphere round everything the surface covers.
    pub fn reach(&self) -> f32 {
        let h = self.half;
        match self.kind {
            Kind::GlassBottle => h[0] + h[1],
            _ => (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt(),
        }
    }

    /// The 8 uniform words the shader reads for it.
    pub fn words(&self) -> [f32; 8] {
        let c = self.centre;
        let h = self.half;
        [c[0], c[1], c[2], self.kind.code(), h[0], h[1], h[2], self.yaw]
    }

    fn valid(&self) -> bool {
        self.centre.iter().chain(&self.half).all(|v| v.is_finite())
            && self.yaw.is_finite()
            && self.half.iter().all(|v| *v >= 0.0)
    }
}

/// Read surfaces from kit data: a JSON list of
/// `{"kind": "water", "centre": [x, y, z], "half": [x, y, z], "yaw": 0}`
/// (`yaw` optional, radians; `center` also accepted).
pub fn surfaces_from_json(text: &str) -> Result<Vec<Surface>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("nature surfaces: {e}"))?;
    let list = v
        .as_array()
        .ok_or("nature surfaces: expected a list of surfaces")?;
    let mut out = Vec::with_capacity(list.len());
    for (i, s) in list.iter().enumerate() {
        let kind_name = s
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("nature surface {i}: no `kind`"))?;
        let kind = Kind::from_name(kind_name)
            .ok_or_else(|| format!("nature surface {i}: unknown kind `{kind_name}`"))?;
        let v3 = |key: &str| -> Result<[f32; 3], String> {
            let a = s
                .get(key)
                .and_then(Value::as_array)
                .filter(|a| a.len() == 3)
                .ok_or_else(|| format!("nature surface {i}: `{key}` must be [x, y, z]"))?;
            let mut o = [0.0f32; 3];
            for (k, x) in a.iter().enumerate() {
                o[k] = x
                    .as_f64()
                    .ok_or_else(|| format!("nature surface {i}: `{key}` must hold numbers"))?
                    as f32;
            }
            Ok(o)
        };
        let centre = v3("centre").or_else(|_| v3("center"))?;
        let half = v3("half")?;
        let yaw = s.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let surf = Surface::new(kind, centre, half).with_yaw(yaw);
        if !surf.valid() {
            return Err(format!("nature surface {i}: sizes must be finite and not negative"));
        }
        out.push(surf);
    }
    Ok(out)
}

pub(crate) struct State {
    pub(crate) surfaces: RefCell<Vec<Surface>>,
    /// This frame's surfaces: capped and sorted far to near.
    pub(crate) drawn: RefCell<Vec<Surface>>,
    pub(crate) camera: Cell<CameraPose>,
    pub(crate) time: Cell<f32>,
    pub(crate) sun: Cell<[f32; 3]>,
    pub(crate) sun_col: Cell<[f32; 3]>,
    /// Per-channel scale of the reflected sky, packed ([`crate::pack_sky_tint`]).
    pub(crate) sky_tint: Cell<f32>,
    pub(crate) mode: Cell<crate::effect::Mode>,
}

/// The surfaces to draw (a world resource; cheap to clone, clones share the
/// list). A game sets them once for a still scene, or each frame for
/// moving ones.
#[derive(Clone)]
pub struct NatureSurfaces(pub(crate) Rc<State>);

impl Default for NatureSurfaces {
    fn default() -> Self {
        Self(Rc::new(State {
            surfaces: RefCell::new(Vec::new()),
            drawn: RefCell::new(Vec::new()),
            camera: Cell::new(CameraPose::default()),
            time: Cell::new(0.0),
            sun: Cell::new([0.0, 1.0, 0.0]),
            sun_col: Cell::new([1.0; 3]),
            sky_tint: Cell::new(crate::pack_sky_tint([1.0; 3])),
            mode: Cell::new(crate::effect::Mode::Off),
        }))
    }
}

impl NatureSurfaces {
    /// Add a surface (one with a non-finite or negative size is ignored).
    pub fn push(&self, s: Surface) {
        if s.valid()
            && let Ok(mut v) = self.0.surfaces.try_borrow_mut()
        {
            v.push(s);
        }
    }

    /// Replace every surface.
    pub fn set(&self, list: &[Surface]) {
        if let Ok(mut v) = self.0.surfaces.try_borrow_mut() {
            v.clear();
            v.extend(list.iter().filter(|s| s.valid()));
        }
    }

    pub fn clear(&self) {
        if let Ok(mut v) = self.0.surfaces.try_borrow_mut() {
            v.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.0.surfaces.try_borrow().map_or(0, |v| v.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copy of the surfaces held.
    pub fn list(&self) -> Vec<Surface> {
        self.0.surfaces.try_borrow().map_or_else(|_| Vec::new(), |v| v.clone())
    }

    /// How the pass draws this frame (set by the module from the tier).
    pub fn mode(&self) -> crate::effect::Mode {
        self.0.mode.get()
    }

    /// Surfaces drawn this frame (after the cap and the view cull).
    pub fn drawn(&self) -> usize {
        self.0.drawn.try_borrow().map_or(0, |v| v.len())
    }

    /// Pick this frame's surfaces: those in front of the camera within its
    /// reach, at most `cap`, nearest kept first, then sorted far to near so
    /// batches combine back to front. `see_through_only` (Lite and Low, whose
    /// flat draws show only water and glass) leaves the rest out, so they
    /// take no batch.
    pub(crate) fn plan(&self, cap: usize, see_through_only: bool) {
        let cam = self.0.camera.get();
        let (Ok(src), Ok(mut out)) = (self.0.surfaces.try_borrow(), self.0.drawn.try_borrow_mut()) else {
            return;
        };
        out.clear();
        let f = crate::effect::norm([
            cam.target[0] - cam.eye[0],
            cam.target[1] - cam.eye[1],
            cam.target[2] - cam.eye[2],
        ]);
        let d2 = |s: &Surface| {
            let d = [
                s.centre[0] - cam.eye[0],
                s.centre[1] - cam.eye[1],
                s.centre[2] - cam.eye[2],
            ];
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        };
        out.extend(src.iter().copied().filter(|s| {
            if see_through_only && !s.kind.see_through() {
                return false;
            }
            let d = [
                s.centre[0] - cam.eye[0],
                s.centre[1] - cam.eye[1],
                s.centre[2] - cam.eye[2],
            ];
            // Behind the camera by more than its own size: not seen.
            d[0] * f[0] + d[1] * f[1] + d[2] * f[2] > -s.reach()
        }));
        out.sort_by(|a, b| d2(a).total_cmp(&d2(b)));
        out.truncate(cap);
        out.reverse();
    }
}
