//! [`RenderWorld`]: the one narrow API kits use to put things on screen.
//! Each frame a kit's `Stage::Frame` system sets the camera and pushes mesh
//! instances and sprites; the renderer draws them and clears the lists
//! (capacity is kept, so a steady scene allocates nothing per frame).

use evani_core::camera::CameraPose;
use evani_core::capture::CanvasHud;
use evani_core::num::clamp;
use evani_core::plugin::LookData;

use crate::mesh::MeshId;

/// Sprite atlas grid: `ATLAS_GRID x ATLAS_GRID` cells. Cell 0 is the look's
/// `checker` texture; the others are built-in shapes (white, tint with
/// [`Sprite::color`]).
pub const ATLAS_GRID: u32 = 4;

/// One instanced mesh draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshInstance {
    pub mesh: MeshId,
    pub position: [f32; 3],
    /// Unit quaternion `[x, y, z, w]` (see [`crate::math::quat_axis_angle`]).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Index into the look's materials ([`LookData::materials`]).
    pub material: u32,
    /// Linear RGBA multiplied with the material's base colour.
    pub color: [f32; 4],
}

impl MeshInstance {
    pub fn new(mesh: MeshId, position: [f32; 3]) -> Self {
        Self {
            mesh,
            position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            material: 0,
            color: [1.0; 4],
        }
    }
}

/// One screen-space sprite, drawn over the 3D scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    /// Centre in CSS pixels from the top-left of the canvas.
    pub pos: [f32; 2],
    /// Size in CSS pixels.
    pub size: [f32; 2],
    /// Radians, clockwise on screen.
    pub rotation: f32,
    /// Atlas cell, `0..ATLAS_GRID * ATLAS_GRID`.
    pub cell: u16,
    /// Linear RGBA tint (alpha blends).
    pub color: [f32; 4],
    /// Lower layers draw first.
    pub layer: i16,
}

/// First sprite cell of a kit's [`SpriteSheet`]: cell `SHEET_CELL0 + i`
/// draws sheet frame `i`. Cells below it are the built-in atlas.
pub const SHEET_CELL0: u16 = (ATLAS_GRID * ATLAS_GRID) as u16;

/// A kit's own sprite sheet (optional resource). When present, the renderer
/// puts it into the sprite atlas texture below the built-in cells, and sprite
/// cell [`SHEET_CELL0`] `+ i` draws `frames[i]`. Pixels are RGBA8 with
/// sRGB-encoded colour, like the built-in atlas. The renderer re-uploads the
/// sheet when `revision` changes, so a kit bumps it after editing pixels
/// (palette cycling, a look change).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpriteSheet {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, rows top to bottom.
    pub rgba8: Vec<u8>,
    /// Pixel rectangles `[x, y, w, h]` inside the sheet.
    pub frames: Vec<[u32; 4]>,
    pub revision: u64,
}

impl SpriteSheet {
    /// The sprite cell that draws frame `i`.
    pub fn cell(i: usize) -> u16 {
        SHEET_CELL0.saturating_add(i.min(u16::MAX as usize) as u16)
    }

    /// True when the pixel buffer matches the size and every frame lies
    /// inside the sheet.
    pub fn is_valid(&self) -> bool {
        self.rgba8.len() == (self.width as usize) * (self.height as usize) * 4
            && self.frames.iter().all(|f| {
                f[2] > 0 && f[3] > 0 && f[0].saturating_add(f[2]) <= self.width && f[1].saturating_add(f[3]) <= self.height
            })
    }
}

/// One extra camera view drawn into part of the canvas (split-screen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub camera: CameraPose,
    /// Normalised `[x, y, width, height]` of the canvas, from the top-left
    /// (`[0, 0, 1, 0.5]` is the top half).
    pub rect: [f32; 4],
}

impl View {
    pub fn new(camera: CameraPose, rect: [f32; 4]) -> Self {
        Self { camera, rect }
    }

    /// Pixel viewport `[x, y, w, h]` of this view in a `size` target (at
    /// least one pixel wide and high, clamped inside the target).
    pub fn pixels(&self, size: [u32; 2]) -> [f32; 4] {
        let (w, h) = (size[0] as f32, size[1] as f32);
        let x = (self.rect[0].clamp(0.0, 1.0) * w).floor().min((w - 1.0).max(0.0));
        let y = (self.rect[1].clamp(0.0, 1.0) * h).floor().min((h - 1.0).max(0.0));
        // num::clamp: the compiler can't see these bounds are ordered, and
        // f32::clamp's panic message would link float formatting code.
        let vw = clamp((self.rect[2].clamp(0.0, 1.0) * w).round(), 1.0, (w - x).max(1.0));
        let vh = clamp((self.rect[3].clamp(0.0, 1.0) * h).round(), 1.0, (h - y).max(1.0));
        [x, y, vw, vh]
    }
}

/// World resource (inserted by the render module).
#[derive(Debug, Clone)]
pub struct RenderWorld {
    pub camera: CameraPose,
    /// Split-screen views. Empty (the default): one full-canvas view through
    /// [`RenderWorld::camera`]. Otherwise the forward pass draws every mesh
    /// instance once per view, each through its own camera into its own
    /// rectangle. Cleared with the draws each frame, so a kit sets them
    /// every frame it wants them.
    pub views: Vec<View>,
    /// Canvas size in CSS pixels (set by the renderer; sprites use it).
    pub viewport_css: [f32; 2],
    /// Render target size in device pixels (set by the renderer next to
    /// `viewport_css`; `[0, 0]` before the first GPU frame and natively). A
    /// 2D kit uses it to draw each art pixel as a whole number of device
    /// pixels.
    pub target_px: [u32; 2],
    pub meshes: Vec<MeshInstance>,
    /// Decals: flat marks lying on a surface (road lines, paint, scorch
    /// marks). Drawn like meshes but pulled toward the camera by
    /// [`crate::shaders::DECAL_DEPTH_OFFSET`], so they never z-fight the
    /// surface under them. Push them with [`RenderWorld::draw_decal`].
    pub decals: Vec<MeshInstance>,
    pub sprites: Vec<Sprite>, #[cfg(feature = "msdf")] pub fonts: crate::msdf::Fonts,
    /// Blob (contact) shadows `[x, y, z, radius]`: a soft dark disc on the
    /// ground at `(x, y, z)`. Drawn when the shadow map is off (Low, or a
    /// game without the `shadows` effect); the first
    /// [`crate::shaders::MAX_BLOBS`] count, so push the important ones first.
    pub blobs: Vec<[f32; 4]>, #[cfg(feature = "point-lights")] pub lights: Vec<crate::light::Light>, #[cfg(feature = "view-model")] pub view_model: crate::view_model::ViewModel,
    /// How dark a blob's centre is (0..1). Kits set it with their look.
    pub blob_strength: f32,
    /// The sprite layers (inclusive range) a kit draws its HUD on, when it
    /// draws the HUD inside the canvas instead of in the page. Set once, kept
    /// across frames; only live captures read it (the review checks a canvas
    /// HUD from it, see [`RenderWorld::canvas_hud`]).
    pub hud_layers: Option<[i16; 2]>,
}

impl Default for RenderWorld {
    fn default() -> Self {
        Self {
            camera: CameraPose::default(),
            views: Vec::new(),
            viewport_css: [0.0; 2],
            target_px: [0; 2],
            meshes: Vec::new(),
            decals: Vec::new(),
            sprites: Vec::new(), #[cfg(feature = "msdf")] fonts: Default::default(),
            blobs: Vec::new(), #[cfg(feature = "point-lights")] lights: Vec::new(), #[cfg(feature = "view-model")] view_model: Default::default(),
            blob_strength: DEFAULT_BLOB_STRENGTH,
            hud_layers: None,
        }
    }
}

/// Default darkness of a blob shadow's centre.
pub const DEFAULT_BLOB_STRENGTH: f32 = 0.7;

impl RenderWorld {
    pub fn draw_mesh(&mut self, m: MeshInstance) {
        self.meshes.push(m);
    }

    /// A decal: a thin mesh (usually a flat CUBE or PLANE) lying on a
    /// surface. It may sit exactly on the surface; the depth offset keeps it
    /// visible without lifting it.
    pub fn draw_decal(&mut self, m: MeshInstance) {
        self.decals.push(m);
    }

    pub fn draw_sprite(&mut self, s: Sprite) {
        self.sprites.push(s);
    }

    /// A blob shadow of `radius` metres on the ground at `ground`.
    pub fn draw_blob(&mut self, ground: [f32; 3], radius: f32) {
        self.blobs.push([ground[0], ground[1], ground[2], radius]);
    }

    /// This frame's canvas HUD: the sprites on [`RenderWorld::hud_layers`]
    /// with a visible tint, and their bounding box clipped to the canvas
    /// (CSS px). `None` when the kit declares no HUD layers.
    pub fn canvas_hud(&self) -> Option<CanvasHud> {
        let [lo, hi] = self.hud_layers?;
        let [vw, vh] = self.viewport_css;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut sprites = 0u32;
        for s in self.sprites.iter().filter(|s| s.layer >= lo && s.layer <= hi && s.color[3] > 0.05) {
            let (hw, hh) = (s.size[0].abs() * 0.5, s.size[1].abs() * 0.5);
            let (a, b) = ((s.pos[0] - hw).max(0.0), (s.pos[1] - hh).max(0.0));
            let (c, d) = ((s.pos[0] + hw).min(vw), (s.pos[1] + hh).min(vh));
            if c <= a || d <= b {
                continue;
            }
            sprites += 1;
            (x0, y0, x1, y1) = (x0.min(a), y0.min(b), x1.max(c), y1.max(d));
        }
        let rect_css = if sprites > 0 { [x0, y0, x1 - x0, y1 - y0] } else { [0.0; 4] };
        Some(CanvasHud { sprites, rect_css, viewport_css: self.viewport_css })
    }

    /// Drop this frame's draws (keeps capacity).
    pub fn clear(&mut self) {
        self.meshes.clear();
        self.decals.clear();
        self.sprites.clear();
        self.blobs.clear(); #[cfg(feature = "point-lights")] self.lights.clear(); #[cfg(feature = "view-model")] self.view_model.meshes.clear();
        self.views.clear();
    }

    /// How many times the forward pass draws the instances (views, or 1).
    pub fn view_count(&self) -> usize {
        self.views.len().max(1)
    }

    /// Index of the look material `name` (0 if the look has none).
    pub fn material(look: &LookData, name: &str) -> u32 {
        look.materials
            .iter()
            .position(|m| m.name == name)
            .unwrap_or(0) as u32
    }

    /// Sort sprites by layer, keeping submission order within a layer. One
    /// O(n) pass checks the order first, so a list already in layer order
    /// (the usual case) costs a single compare per sprite and nothing else;
    /// only an unsorted list pays for a stable sort.
    pub fn sort_sprites(&mut self) {
        let s = &mut self.sprites;
        if s.windows(2).all(|w| w[0].layer <= w[1].layer) {
            return;
        }
        s.sort_by_key(|sp| sp.layer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::plugin::MaterialDesc;

    fn sprite(layer: i16, cell: u16) -> Sprite {
        Sprite {
            pos: [0.0; 2],
            size: [1.0; 2],
            rotation: 0.0,
            cell,
            color: [1.0; 4],
            layer,
        }
    }

    #[test]
    fn sort_is_stable_and_skips_sorted_lists() {
        let mut w = RenderWorld::default();
        // Already in layer order: left untouched.
        for i in 0..100u16 {
            w.draw_sprite(sprite((i / 10) as i16, i));
        }
        let before = w.sprites.clone();
        w.sort_sprites();
        assert_eq!(w.sprites, before);
        // Unsorted and larger than the small-sort cutoff: stable by layer.
        w.clear();
        for i in 0..200u16 {
            w.draw_sprite(sprite(((i * 7) % 5) as i16, i));
        }
        w.sort_sprites();
        assert!(w.sprites.windows(2).all(|p| p[0].layer < p[1].layer || (p[0].layer == p[1].layer && p[0].cell < p[1].cell)));
    }

    #[test]
    fn canvas_hud_counts_visible_hud_sprites_on_the_canvas() {
        let mut w = RenderWorld { viewport_css: [640.0, 360.0], ..RenderWorld::default() };
        let at = |layer: i16, pos: [f32; 2], alpha: f32| Sprite { pos, size: [10.0, 8.0], color: [1.0, 1.0, 1.0, alpha], ..sprite(layer, 0) };
        w.draw_sprite(at(200, [20.0, 10.0], 1.0));
        w.draw_sprite(at(200, [100.0, 14.0], 1.0));
        w.draw_sprite(at(200, [300.0, 300.0], 0.0)); // invisible
        w.draw_sprite(at(200, [-50.0, 10.0], 1.0)); // off the canvas
        w.draw_sprite(at(90, [400.0, 200.0], 1.0)); // not a HUD layer
        assert_eq!(w.canvas_hud(), None, "no HUD layers declared");
        w.hud_layers = Some([200, 200]);
        let h = w.canvas_hud().unwrap_or_default();
        assert_eq!(h.sprites, 2);
        assert_eq!(h.rect_css, [15.0, 6.0, 90.0, 12.0]);
        assert_eq!(h.viewport_css, [640.0, 360.0]);
        w.clear();
        assert_eq!(w.hud_layers, Some([200, 200]), "kept across frames");
        assert_eq!(w.canvas_hud().map(|h| h.sprites), Some(0));
    }

    #[test]
    fn views_map_to_pixels_and_clear() {
        let mut w = RenderWorld::default();
        assert_eq!(w.view_count(), 1);
        let cam = CameraPose::default();
        w.views.push(View::new(cam, [0.0, 0.0, 1.0, 0.5]));
        w.views.push(View::new(cam, [0.0, 0.5, 1.0, 0.5]));
        assert_eq!(w.view_count(), 2);
        assert_eq!(w.views[0].pixels([800, 601]), [0.0, 0.0, 800.0, 301.0]);
        assert_eq!(w.views[1].pixels([800, 601]), [0.0, 300.0, 800.0, 301.0]);
        // Out-of-range rects stay inside the target and at least 1 px.
        assert_eq!(View::new(cam, [0.9, 2.0, 5.0, 0.0]).pixels([100, 100]), [90.0, 99.0, 10.0, 1.0]);
        w.clear();
        assert!(w.views.is_empty());
    }

    #[test]
    fn draws_sort_and_clear() {
        let mut w = RenderWorld::default();
        w.draw_mesh(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
        for (l, c) in [(2, 0), (1, 1), (2, 2), (0, 3), (1, 4)] {
            w.draw_sprite(sprite(l, c));
        }
        w.sort_sprites();
        let order: Vec<u16> = w.sprites.iter().map(|s| s.cell).collect();
        assert_eq!(order, [3, 1, 4, 0, 2]);
        let cap = w.sprites.capacity();
        w.clear();
        assert!(w.meshes.is_empty() && w.sprites.is_empty());
        w.draw_decal(MeshInstance::new(MeshId::PLANE, [0.0; 3]));
        assert_eq!(w.decals.len(), 1);
        w.clear();
        assert!(w.decals.is_empty());
        assert_eq!(w.sprites.capacity(), cap);
        let look = LookData {
            materials: vec![MaterialDesc::new("a", [1.0; 4]), MaterialDesc::new("glow", [1.0; 4])],
            ..LookData::default()
        };
        assert_eq!(RenderWorld::material(&look, "glow"), 1);
        assert_eq!(RenderWorld::material(&look, "nope"), 0);
    }
}
