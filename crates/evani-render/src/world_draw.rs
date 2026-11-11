//! Module-drawn world geometry (feature `world-draws`, CORE_REQUESTS
//! `world-draws`, asked for by evani-grass).
//!
//! Game passes ([`crate::game_pass`]) draw full-screen triangles and fill
//! textures; a few systems need their own geometry in the 3D scene with the
//! scene's depth and anti-aliasing: GPU-generated grass, later perhaps
//! foliage or crowds. Such a module implements [`WorldDraw`] and puts it in
//! the [`WorldDraws`] world resource (inserting the resource if it is
//! missing). Each frame the renderer:
//!
//! 1. before the graph walk, gives each one a compute pass of its own with
//!    the frame's encoder ([`WorldDraw::prepare`]: uploads, generation,
//!    culling, indirect arguments), timed under [`WorldDraw::name`];
//! 2. inside the forward pass, after the opaque meshes and models and
//!    before world particles and see-through models, calls
//!    [`WorldDraw::draw`] with the open render pass.
//!
//! The module owns its pipelines, buffers and WGSL. Its pipelines must match
//! [`WorldGpu::samples`], [`WorldGpu::color`] and [`WorldGpu::depth`] and
//! use the engine's reversed-Z test ([`crate::depth::COMPARE`]); it sets its
//! own viewport for each of [`WorldGpu::views`]. The engine's draws after it
//! set their own state. Games without the feature compile none of this.

use std::rc::Rc;

/// What a [`WorldDraw`] gets from the renderer each frame (browser builds).
#[cfg(target_arch = "wasm32")]
pub struct WorldGpu<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// Render target size in pixels.
    pub size: [u32; 2],
    /// Samples of the forward pass: 4 with MSAA (Medium and up), else 1.
    pub samples: u32,
    /// The forward pass's colour format (`hdr`).
    pub color: wgpu::TextureFormat,
    /// The forward pass's depth format (reversed Z, see [`crate::depth`]).
    pub depth: wgpu::TextureFormat,
    /// This frame's forward views: the engine's frame-uniform offset and
    /// the pixel viewport `[x, y, w, h]`, in the order of
    /// `RenderWorld::views` (one view of the whole target when that is
    /// empty).
    pub views: &'a [(u32, [f32; 4])],
}

/// A module's own world geometry (browser builds).
#[cfg(target_arch = "wasm32")]
pub trait WorldDraw: 'static {
    /// The name its compute pass is timed under (the overlay's per-pass
    /// GPU times), e.g. `"grass"`.
    fn name(&self) -> &'static str;
    /// Before the graph walk: this frame's uploads and compute work, in one
    /// compute pass begun with `ts` (timestamp writes, `None` when the
    /// device has no timer). A module with nothing to do this frame begins
    /// no pass.
    fn prepare(&self, gpu: &WorldGpu<'_>, enc: &mut wgpu::CommandEncoder, ts: Option<wgpu::ComputePassTimestampWrites<'_>>);
    /// Inside the forward pass: record the draws for every view. Returns
    /// the draw calls made (the overlay's count).
    fn draw(&self, gpu: &WorldGpu<'_>, rp: &mut wgpu::RenderPass<'_>) -> u32;
}

/// World resource: the module draws of this build, in the order they were
/// added. A module inserts it if it is missing, then pushes its own.
#[derive(Default, Clone)]
pub struct WorldDraws {
    #[cfg(target_arch = "wasm32")]
    pub draws: Vec<Rc<dyn WorldDraw>>,
    #[cfg(not(target_arch = "wasm32"))]
    _native: Option<Rc<()>>,
}

impl WorldDraws {
    /// Number of module draws (always 0 in native builds, which draw
    /// nothing).
    pub fn len(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        return self.draws.len();
        #[cfg(not(target_arch = "wasm32"))]
        0
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl core::fmt::Debug for WorldDraws {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(target_arch = "wasm32")]
        return f.debug_list().entries(self.draws.iter().map(|d| d.name())).finish();
        #[cfg(not(target_arch = "wasm32"))]
        f.write_str("[]")
    }
}
