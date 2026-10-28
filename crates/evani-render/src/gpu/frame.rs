//! One live GPU device and everything built on it: surface, layouts,
//! uniform ring, meshes, atlas, graph targets, pipelines and timing, plus
//! the per-frame graph walk.

use bytemuck::Pod;
use evani_core::ecs::World;
use evani_core::overlay::{ErrorLog, RenderStats};
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::quality::QualityState;
use evani_core::tiers::Tiers;

use super::pipelines::{self, Formats, Layouts, Pipelines};
use super::timing::Timer;
use super::Init;
use crate::atlas::{SHEET_ORIGIN, atlas_size, build_atlas};
use crate::control::{QualityDriver, render_size, render_size_exact};
use crate::graph::{RenderGraph, TargetKind};
#[cfg(feature = "forward")]
use crate::mesh::{self, MESH_SLOTS};
use crate::post::{EffectShader, PostEffects, PostRecorder};
use crate::shaders::{self, FRAME_WORDS, SpriteRaw};
#[cfg(feature = "forward")]
use crate::shaders::{InstanceRaw, Shading};
#[cfg(feature = "forward")]
use crate::shadow::{self, ShadowParams};
use crate::world::{RenderWorld, SpriteSheet};
use crate::RenderParams;

/// Uniform ring: three regions (one per frame in flight), 256-byte slots.
const REGION: usize = 64 * 1024;
const REGIONS: usize = 3;
const ALIGN: usize = 256;
/// Binding window of the post/engine uniforms (`MAX_UNIFORM_WORDS` floats).
const SMALL_BINDING: u64 = 256;
const FRAME_BINDING: u64 = (FRAME_WORDS * 4) as u64;

struct Ring {
    buf: wgpu::Buffer,
    staging: Vec<u8>,
    region: usize,
    overflowed: bool,
}

impl Ring {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("evani.uniforms"),
                size: (REGION * REGIONS) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            staging: Vec::with_capacity(REGION),
            region: 0,
            overflowed: false,
        }
    }

    fn begin(&mut self) {
        self.region = (self.region + 1) % REGIONS;
        self.staging.clear();
    }

    /// Append one uniform struct; returns its dynamic offset.
    fn push(&mut self, words: &[f32]) -> u32 {
        let start = self.staging.len();
        let need = (words.len() * 4).div_ceil(ALIGN).max(1) * ALIGN;
        // Room for the largest binding window after the last slot.
        if start + need + FRAME_BINDING as usize > REGION {
            self.overflowed = true;
            return (self.region * REGION) as u32;
        }
        self.staging.extend_from_slice(bytemuck::cast_slice(words));
        self.staging.resize(start + need, 0);
        (self.region * REGION + start) as u32
    }

    fn upload(&self, queue: &wgpu::Queue) {
        if !self.staging.is_empty() {
            queue.write_buffer(&self.buf, (self.region * REGION) as u64, &self.staging);
        }
    }
}

/// A growable vertex buffer filled from a reused CPU staging vector of `Pod`
/// instances.
struct Stream<T> {
    buf: wgpu::Buffer,
    cap: u64,
    /// Instances of the current frame; reused, so a steady scene allocates
    /// nothing.
    staging: Vec<T>,
    label: &'static str,
}

impl<T: Pod> Stream<T> {
    fn new(device: &wgpu::Device, label: &'static str, cap: u64) -> Self {
        Self {
            buf: Self::make(device, label, cap),
            cap,
            staging: Vec::with_capacity(cap as usize / size_of::<T>()),
            label,
        }
    }

    fn make(device: &wgpu::Device, label: &'static str, cap: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// One `write_buffer` of the whole staging vector as bytes; the buffer
    /// grows geometrically (next power of two) when the frame needs more.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let bytes: &[u8] = bytemuck::cast_slice(&self.staging);
        let n = bytes.len() as u64;
        if n == 0 {
            return;
        }
        if n > self.cap {
            self.cap = n.next_power_of_two();
            self.buf = Self::make(device, self.label, self.cap);
        }
        queue.write_buffer(&self.buf, 0, bytes);
    }
}

/// One graph target's texture.
struct Target {
    name: &'static str,
    transient: bool,
    key: PoolKey,
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    /// Layout-A bind group sampling it (colour targets only).
    bg: Option<wgpu::BindGroup>,
    /// Layout-AD bind group: the same plus the scene depth (colour targets
    /// only, when a depth target exists).
    bg_depth: Option<wgpu::BindGroup>,
}

/// 4x MSAA forward targets at the `hdr` size: colour (resolved into
/// `hdr`) and depth (resolved into `depth` only when an effect reads it).
#[cfg(feature = "forward")]
struct Msaa {
    size: [u32; 2],
    _color: wgpu::Texture,
    color: wgpu::TextureView,
    _depth: wgpu::Texture,
    depth: wgpu::TextureView,
    /// The 4x depth for the depth-resolve pass (layout DMS).
    depth_bg: wgpu::BindGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PoolKey {
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
}

pub struct Gpu {
    _instance: wgpu::Instance,
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pub adapter_name: String,
    max_dim: u32,
    encode_srgb: bool,
    formats: Formats,
    layouts: Layouts,
    sampler: wgpu::Sampler,
    ring: Ring,
    /// Forward frame uniform (layout B, 656-byte window).
    #[cfg(feature = "forward")]
    #[cfg_attr(feature = "shadows", allow(dead_code))]
    frame_bg: wgpu::BindGroup,
    /// Uniform-only effects (layout B, 256-byte window).
    small_bg: wgpu::BindGroup,
    /// Uniform plus scene depth (layout BD), rebuilt with the targets.
    small_depth_bg: Option<wgpu::BindGroup>,
    // The 3D path (feature `forward`): views, meshes, instances, MSAA,
    // the frame uniform and the per-mesh counts.
    /// This frame's forward views: frame-uniform offset and pixel viewport.
    #[cfg(feature = "forward")]
    views: Vec<(u32, [f32; 4])>,
    #[cfg(feature = "forward")]
    mesh_vb: wgpu::Buffer,
    #[cfg(feature = "forward")]
    mesh_ib: wgpu::Buffer,
    /// (first index, index count, base vertex) per mesh slot.
    #[cfg(feature = "forward")]
    mesh_ranges: [(u32, u32, i32); MESH_SLOTS],
    #[cfg(feature = "forward")]
    mesh_tris: [u32; MESH_SLOTS],
    /// `MeshLibrary` revision the mesh buffers were built from.
    #[cfg(feature = "meshes")]
    mesh_rev: u64,
    #[cfg(feature = "forward")]
    instances: Stream<InstanceRaw>,
    sprites: Stream<SpriteRaw>, #[cfg(feature = "msdf")] msdf: super::msdf::GpuMsdf, #[cfg(all(feature = "lights2d", not(feature = "msdf")))] lights2d: super::lights2d::GpuLights2d, #[cfg(feature = "pseudo3d")] road: super::pseudo3d::GpuRoad,
    nearest: wgpu::Sampler,
    atlas: Atlas,
    targets: Vec<Target>,
    /// Present while the active tier asks for MSAA and its pipelines are
    /// built; the forward pass then draws 4x and resolves into `hdr`.
    #[cfg(feature = "forward")]
    msaa: Option<Msaa>,
    pool: Vec<(PoolKey, wgpu::Texture)>,
    size: [u32; 2],
    pub pipes: Pipelines,
    timer: Timer,
    #[cfg(feature = "forward")]
    frame_words: Box<[f32; FRAME_WORDS]>,
    /// Instances per mesh this frame, and where each run starts.
    #[cfg(feature = "forward")]
    counts: [u32; MESH_SLOTS],
    pub last_size: [u32; 2],
    ring_warned: bool, #[cfg(feature = "models")] models: super::models::ModelGpu, #[cfg(feature = "point-lights")] lights: super::lights::LightGpu, #[cfg(feature = "kit-material")] kit: pipelines::kit_material::KitGpu, #[cfg(feature = "view-model")] vm: view_model::VmGpu, #[cfg(feature = "ambient")] ambient: super::ambient::AmbientGpu,
    /// Live capture readbacks (feature `capture`, live builds only).
    #[cfg(feature = "capture")]
    pub readbacks: super::capture::Readbacks,
    /// The canvas hold during live trial runs (feature `capture`).
    #[cfg(feature = "capture")]
    hold: super::capture::CanvasHold,
    /// `fx.shadows.*` handles (absent params use the defaults).
    #[cfg(feature = "forward")]
    shadow_params: ShadowParams,
    /// Shadow map (feature `shadows`).
    #[cfg(feature = "shadows")]
    shadows: Shadows,
    /// The shared particle system's buffers and pipelines (feature
    /// `particles`).
    #[cfg(feature = "particles")]
    particles: super::particles::GpuParticles,
}

/// The shadow map's GPU side: depth pipeline, the map (made at the tier's
/// size on first use), a 1x1 stand-in and the forward bind group that
/// samples whichever is current.
#[cfg(feature = "shadows")]
struct Shadows {
    pipe: wgpu::RenderPipeline,
    cmp: wgpu::Sampler,
    _dummy: wgpu::Texture,
    dummy_view: wgpu::TextureView,
    map: Option<(u32, wgpu::Texture, wgpu::TextureView)>,
    /// Forward bind group (layout `fwd`): frame uniform, map, sampler.
    fwd_bg: wgpu::BindGroup,
    words: Box<[f32; shaders::SHADOW_WORDS]>, #[cfg(feature = "shadow-cascades")] near: Option<u32>,
}

#[cfg(feature = "shadows")]
impl Shadows {
    fn new(device: &wgpu::Device, layouts: &Layouts, ring: &wgpu::Buffer) -> Self {
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("evani.shadow_cmp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let dummy = depth_texture(device, 1, "evani.shadow_dummy");
        let dummy_view = dummy.create_view(&wgpu::TextureViewDescriptor::default());
        let fwd_bg = fwd_group(device, layouts, ring, &dummy_view, &cmp);
        Self {
            pipe: super::pipelines::shadow_pipeline(device, layouts),
            cmp,
            _dummy: dummy,
            dummy_view,
            map: None,
            fwd_bg,
            words: Box::new([0.0; shaders::SHADOW_WORDS]), #[cfg(feature = "shadow-cascades")] near: None,
        }
    }

    /// The map at `res` texels (made or remade as needed).
    fn ensure(&mut self, device: &wgpu::Device, layouts: &Layouts, ring: &wgpu::Buffer, res: u32) -> &wgpu::TextureView {
        if self.map.as_ref().is_none_or(|m| m.0 != res) {
            let tex = depth_texture(device, res, "evani.shadow_map");
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.fwd_bg = fwd_group(device, layouts, ring, &view, &self.cmp);
            self.map = Some((res, tex, view));
        }
        match &self.map {
            Some((_, _, v)) => v,
            None => &self.dummy_view,
        }
    }
}

#[cfg(feature = "shadows")]
fn depth_texture(device: &wgpu::Device, size: u32, label: &'static str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            #[cfg(not(feature = "shadow-cascades"))] width: size, #[cfg(feature = "shadow-cascades")] width: cascade::width(size),
            #[cfg(not(feature = "shadow-cascades"))] height: size, #[cfg(feature = "shadow-cascades")] height: cascade::height(size),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

#[cfg(feature = "shadows")]
fn fwd_group(
    device: &wgpu::Device,
    layouts: &Layouts,
    ring: &wgpu::Buffer,
    map: &wgpu::TextureView,
    cmp: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("evani.forward_shadows"),
        layout: &layouts.fwd,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: ring,
                    offset: 0,
                    size: wgpu::BufferSize::new(FRAME_BINDING),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(map),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(cmp),
            },
        ],
    })
}

impl Gpu {
    pub fn new(init: Init, world: &World, epoch: u32) -> Self {
        let Init {
            instance,
            surface,
            adapter,
            device,
            queue,
            timestamps,
            adapter_label,
        } = init;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let max_dim = device.limits().max_texture_dimension_2d;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };
        let formats = Formats { surface: format };
        let layouts = Layouts::new(&device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("evani.linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            // Sprite art builds a mip chain: blend between levels. Every
            // other texture this sampler reads has one level.
            mipmap_filter: if cfg!(feature = "sprite-art") {
                wgpu::MipmapFilterMode::Linear
            } else {
                wgpu::MipmapFilterMode::Nearest
            },
            ..Default::default()
        });
        let ring = Ring::new(&device);
        let ring_bg = |size: u64| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layouts.b,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &ring.buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(size),
                    }),
                }],
            })
        };
        #[cfg(feature = "forward")]
        let frame_bg = ring_bg(FRAME_BINDING);
        let small_bg = ring_bg(SMALL_BINDING);

        // Built-in meshes (and custom ones, feature `meshes`), concatenated.
        #[cfg(feature = "meshes")]
        let (bufs, mesh_rev) = {
            let lib = world.try_res::<mesh::MeshLibrary>();
            (mesh::mesh_buffers(lib.as_deref()), lib.map_or(0, |l| l.revision()))
        };
        #[cfg(all(feature = "forward", not(feature = "meshes")))]
        let bufs = mesh::mesh_buffers(None);
        #[cfg(feature = "forward")]
        let (mesh_vb, mesh_ib) = mesh_gpu_buffers(&device, &queue, &bufs);
        #[cfg(feature = "forward")]
        let (mesh_ranges, mesh_tris) = (bufs.ranges, bufs.tris);

        // Sprite atlas (cell 0 = the look's checker; a kit's sheet below).
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("evani.nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        #[cfg(not(feature = "sprite-art"))]
        let atlas = {
            let look = world.res::<LookData>();
            let sheet = world.try_res::<SpriteSheet>();
            Atlas::build(&device, &queue, &layouts, [&sampler, &nearest], &ring.buf, &look, sheet.as_deref())
        };
        #[cfg(feature = "sprite-art")]
        let atlas = Atlas::build_art(&device, &queue, &layouts, [&sampler, &nearest], &ring.buf, world, max_dim);

        let timer = Timer::new(&device, &queue, timestamps);
        #[cfg(feature = "shadows")]
        let shadows = Shadows::new(&device, &layouts, &ring.buf);
        #[cfg(feature = "forward")]
        let shadow_params = ShadowParams::lookup(&world.res::<ParamRegistry>());
        let mut pipes = Pipelines::new(epoch); #[cfg(feature = "kit-material")] pipelines::kit_material::sync_wgsl(world);
        pipes.launch_all(&device, &layouts, formats, world);

        Self {
            _instance: instance,
            #[cfg(feature = "forward")]
            instances: Stream::new(&device, "evani.instances", 64 * 1024),
            sprites: Stream::new(&device, "evani.sprites", 32 * 1024), #[cfg(all(feature = "lights2d", not(feature = "msdf")))] lights2d: Default::default(), #[cfg(feature = "msdf")] msdf: super::msdf::GpuMsdf::new(&device, &layouts.msdf), #[cfg(feature = "models")] models: super::models::ModelGpu::new(&device, &queue, &layouts), #[cfg(feature = "point-lights")] lights: super::lights::LightGpu::new(&device, &layouts), #[cfg(feature = "kit-material")] kit: pipelines::kit_material::KitGpu::new(&device, &layouts, world), #[cfg(feature = "view-model")] vm: view_model::VmGpu::new(), #[cfg(feature = "ambient")] ambient: super::ambient::AmbientGpu::new(&device), #[cfg(feature = "pseudo3d")] road: Default::default(),
            device,
            queue,
            surface,
            config,
            adapter_name: adapter_label,
            max_dim,
            encode_srgb: !format.is_srgb(),
            formats,
            layouts,
            sampler,
            ring,
            #[cfg(feature = "forward")]
            frame_bg,
            small_bg,
            small_depth_bg: None,
            #[cfg(feature = "forward")]
            views: Vec::with_capacity(4),
            #[cfg(feature = "forward")]
            mesh_vb,
            #[cfg(feature = "forward")]
            mesh_ib,
            #[cfg(feature = "forward")]
            mesh_ranges,
            #[cfg(feature = "forward")]
            mesh_tris,
            #[cfg(feature = "meshes")]
            mesh_rev,
            nearest,
            atlas,
            targets: Vec::new(),
            #[cfg(feature = "forward")]
            msaa: None,
            pool: Vec::new(),
            size: [0, 0],
            pipes,
            timer,
            #[cfg(feature = "forward")]
            frame_words: Box::new([0.0; FRAME_WORDS]),
            #[cfg(feature = "forward")]
            counts: [0; MESH_SLOTS],
            last_size: [0, 0],
            ring_warned: false,
            #[cfg(feature = "capture")]
            readbacks: Default::default(),
            #[cfg(feature = "capture")]
            hold: Default::default(),
            #[cfg(feature = "particles")]
            particles: Default::default(),
            #[cfg(feature = "forward")]
            shadow_params,
            #[cfg(feature = "shadows")]
            shadows,
        }
    }

    pub fn max_dim(&self) -> u32 {
        self.max_dim
    }

    pub fn timestamps(&self) -> bool {
        self.timer.supported()
    }

    /// Follow the kit's sprite sheet (rebuild the atlas texture when its
    /// revision changes or it appears/disappears) and the pixel-art filter
    /// (rebuild the uv table only). Two compares per frame otherwise.
    #[cfg(feature = "sprite-art")]
    fn sync_atlas(&mut self, world: &World, nearest: bool) {
        let rev = world.try_res::<SpriteSheet>().filter(|s| s.is_valid()).map(|s| s.revision);
        let art_rev = world.try_res::<crate::art::SpriteArt>().map_or(0, |a| a.revision());
        if rev != self.atlas.sheet_rev || art_rev != self.atlas.art_rev {
            self.atlas = Atlas::build_art(
                &self.device,
                &self.queue,
                &self.layouts,
                [&self.sampler, &self.nearest],
                &self.ring.buf,
                world,
                self.max_dim,
            );
        }
        if nearest != self.atlas.uv_nearest {
            if let Some((packed, pixel)) = &self.atlas.layout {
                crate::art::uv_table(packed, pixel, nearest, &mut self.atlas.uvs);
            } else {
                let sheet = world.try_res::<SpriteSheet>();
                let frames = sheet.as_ref().filter(|s| s.is_valid()).map_or(&[][..], |s| &s.frames[..]);
                shaders::sprite_uv_table(self.atlas.size, SHEET_ORIGIN, frames, nearest, &mut self.atlas.uvs);
            }
            self.atlas.uv_nearest = nearest;
        }
    }

    #[cfg(not(feature = "sprite-art"))]
    fn sync_atlas(&mut self, world: &World, nearest: bool) {
        let sheet = world.try_res::<SpriteSheet>();
        let rev = sheet.as_ref().filter(|s| s.is_valid()).map(|s| s.revision);
        if rev != self.atlas.sheet_rev {
            let look = world.res::<LookData>();
            self.atlas = Atlas::build(
                &self.device,
                &self.queue,
                &self.layouts,
                [&self.sampler, &self.nearest],
                &self.ring.buf,
                &look,
                sheet.as_deref(),
            );
        }
        if nearest != self.atlas.uv_nearest {
            let frames = sheet.as_ref().filter(|s| s.is_valid()).map_or(&[][..], |s| &s.frames[..]);
            shaders::sprite_uv_table(self.atlas.size, SHEET_ORIGIN, frames, nearest, &mut self.atlas.uvs);
            self.atlas.uv_nearest = nearest;
        }
    }

    /// (Re)configure the surface and rebuild every graph target at `size`.
    fn resize(&mut self, size: [u32; 2], graph: &RenderGraph) {
        self.size = size;
        self.config.width = size[0];
        self.config.height = size[1];
        // Configuring sets the canvas's size, which clears it: while the
        // canvas is held, that waits for the hold to end.
        #[cfg(feature = "capture")]
        if self.hold.active {
            self.hold.surface_stale = true;
        } else {
            self.surface.configure(&self.device, &self.config);
        }
        #[cfg(not(feature = "capture"))]
        self.surface.configure(&self.device, &self.config);
        // Transient textures go back to the pool; matching keys are reused.
        for t in self.targets.drain(..) {
            if t.transient {
                self.pool.push((t.key, t.tex));
            }
        }
        for desc in graph.targets() {
            if !desc.kind.is_texture() {
                continue;
            }
            let w = ((size[0] as f32 * desc.scale).round() as u32).max(1);
            let h = ((size[1] as f32 * desc.scale).round() as u32).max(1);
            let format = pipelines::target_format(desc.kind, self.formats);
            // Every target can be sampled (depth too: effects may read it).
            let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            // Live capture copies the final colour target into a readback buffer.
            #[cfg(feature = "capture")]
            let usage = if desc.kind == TargetKind::Ldr {
                usage | wgpu::TextureUsages::COPY_SRC
            } else {
                usage
            };
            let key = PoolKey { w, h, format, usage };
            let pooled = if desc.transient {
                self.pool.iter().position(|(k, _)| *k == key).map(|i| self.pool.swap_remove(i).1)
            } else {
                None
            };
            let tex = pooled.unwrap_or_else(|| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(desc.name),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let bg = (desc.kind != TargetKind::Depth)
                .then(|| a_group(&self.device, &self.layouts, &view, &self.sampler, &self.ring.buf));
            self.targets.push(Target {
                name: desc.name,
                transient: desc.transient,
                key,
                tex,
                view,
                size: [w, h],
                bg,
                bg_depth: None,
            });
        }
        // Depth-reading effects: one group per colour target plus a
        // uniform-only one, all bound to the scene depth.
        self.small_depth_bg = None;
        if let Some(di) = self.targets.iter().position(|t| t.name == "depth") {
            let depth_view = self.targets[di].view.clone();
            let ring = wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &self.ring.buf,
                offset: 0,
                size: wgpu::BufferSize::new(SMALL_BINDING),
            });
            self.small_depth_bg = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layouts.bd,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ring },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&depth_view),
                    },
                ],
            }));
            for t in self.targets.iter_mut().filter(|t| t.bg.is_some()) {
                t.bg_depth = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.layouts.ad,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&t.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.ring.buf,
                                offset: 0,
                                size: wgpu::BufferSize::new(SMALL_BINDING),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&depth_view),
                        },
                    ],
                }));
            }
        }
        // Whatever the new size didn't reuse is freed.
        self.pool.clear();
    }


    /// Samples the forward pass draws with this frame (1, or 4 with MSAA;
    /// always 1 without the 3D path).
    pub(crate) fn msaa_samples(&self) -> u32 {
        #[cfg(feature = "forward")]
        if self.msaa.is_some() {
            return pipelines::MSAA_SAMPLES;
        }
        1
    }

    /// Make the 4x forward targets match `want` (and the `hdr` size).
    #[cfg(feature = "forward")]
    fn ensure_msaa(&mut self, want: bool) {
        let size = find_target(&self.targets, "hdr").map(|t| t.size);
        let (true, Some(size)) = (want, size) else {
            self.msaa = None;
            return;
        };
        if self.msaa.as_ref().is_some_and(|m| m.size == size) {
            return;
        }
        let make = |label, format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: pipelines::MSAA_SAMPLES,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color_tex = make(
            "evani.msaa.color",
            pipelines::target_format(TargetKind::HdrColor, self.formats),
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth_tex = make(
            "evani.msaa.depth",
            pipelines::target_format(TargetKind::Depth, self.formats),
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let color = color_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.msaa.depth"),
            layout: &self.layouts.dms,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&depth),
            }],
        });
        self.msaa = Some(Msaa {
            size,
            _color: color_tex,
            color,
            _depth: depth_tex,
            depth,
            depth_bg,
        });
    }

    /// One frame: install finished pipelines, run the tier/quality driver,
    /// follow the size, then walk the graph.
    pub fn frame(&mut self, world: &mut World, p: &RenderParams, driver: &mut QualityDriver, css: [f32; 2], dpr: f32) {
        let finished = self.pipes.poll(&self.device, &self.layouts, self.formats, world);
        self.pipes
            .take_slot_requests(&self.device, &self.layouts, self.formats, world);
        // Adaptive quality does not judge frames while shaders compile or
        // pipelines swap in (nor aq.grace_ms after).
        if finished || self.pipes.in_flight() {
            world.res_mut::<QualityState>().disturb();
        }
        self.timer.collect(&mut world.res_mut::<RenderStats>());
        #[cfg(feature = "capture")]
        self.readbacks.poll(world);
        #[cfg(feature = "capture")]
        self.hold.update(world);
        // Upload this frame's particle records even if nothing is drawn, so
        // the GPU ring never misses one.
        #[cfg(feature = "particles")]
        if let Some(mut ps) = world.try_res_mut::<crate::particles::ParticleSystem>() {
            // Particle shapes are atlas cells, looked up like sprite cells
            // (named frames from `art::ART_CELL0` with sprite art).
            let uvs = &self.atlas.uvs;
            #[cfg(feature = "sprite-art")]
            let base = self.atlas.art_base;
            #[cfg(feature = "sprite-art")]
            let uv_of = |cell: u16| crate::art::cell_uv(cell, uvs, base);
            #[cfg(not(feature = "sprite-art"))]
            let uv_of = |cell: u16| {
                let i = usize::from(cell);
                uvs.get(i)
                    .or_else(|| uvs.get(i % shaders::ATLAS_CELLS))
                    .copied()
                    .unwrap_or([0.0; 4])
            };
            self.particles
                .prepare(&self.device, &self.queue, &self.layouts, &mut ps, uv_of);
        }

        if !self.pipes.engine_ready() {
            world.res_mut::<RenderWorld>().clear();
            return;
        } #[cfg(feature = "prewarm")] if self.prewarm_hold(world) { world.res_mut::<RenderWorld>().clear(); return; }
        #[cfg(feature = "meshes")]
        self.sync_meshes(world);
        if !driver.started() {
            driver.begin(world, self.max_dim);
        }
        driver.frame(world, self.timer.last_gpu_ms);

        let want = if world.res::<ParamRegistry>().bool(p.pixel_art) {
            // Pixel art keeps whole device pixels on every tier (see
            // `render_size_exact`).
            render_size_exact(css, dpr, self.max_dim)
        } else {
            let tiers = world.res::<Tiers>();
            let q = world.res::<QualityState>().scale;
            render_size(css, dpr, tiers.active(), q, self.max_dim)
        };
        let graph_targets = world.res::<RenderGraph>().targets().iter().filter(|t| t.kind.is_texture()).count();
        if want != self.size || self.targets.len() != graph_targets {
            let graph = world.res::<RenderGraph>();
            self.resize(want, &graph);
        }
        // An enabled effect pass reads the scene depth: it must outlive the
        // forward pass (and, with MSAA, be resolved to one sample).
        #[cfg(feature = "forward")]
        let depth_read = world
            .res::<RenderGraph>()
            .ordered()
            .any(|p| p.enabled && p.owner != "render" && p.inputs.contains(&"depth"));
        // 4x MSAA on the forward pass when the tier asks for it, once its
        // pipelines are built (until then the pass draws one sample).
        #[cfg(feature = "forward")]
        {
            let want_msaa = world.res::<Tiers>().active().msaa >= pipelines::MSAA_SAMPLES
                && self.pipes.engine_ms(pipelines::FORWARD).is_some()
                && (!depth_read || self.pipes.depth_resolve().is_some());
            self.ensure_msaa(want_msaa);
        }
        self.last_size = self.size;

        // Take the canvas texture: the same code with or without `capture`,
        // so a ship build compiles exactly what it did before the hold.
        macro_rules! acquire {
            () => {
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                    wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                        world.res_mut::<RenderWorld>().clear();
                        return;
                    }
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        self.surface.configure(&self.device, &self.config);
                        world.res_mut::<RenderWorld>().clear();
                        return;
                    }
                    wgpu::CurrentSurfaceTexture::Validation => {
                        world.res_mut::<ErrorLog>().error("surface texture: validation error");
                        world.res_mut::<RenderWorld>().clear();
                        return;
                    }
                }
            };
        }
        // A held canvas (live builds): the present pass draws off-screen and
        // the canvas keeps its last frame (no surface texture is taken or
        // presented).
        #[cfg(feature = "capture")]
        let (frame, swap_view) = {
            if !self.hold.active && self.hold.surface_stale {
                self.hold.surface_stale = false;
                self.surface.configure(&self.device, &self.config);
            }
            if let Some(v) = self.hold.view(&self.device, self.config.format, self.size) {
                (None, v)
            } else {
                let frame = acquire!();
                let swap_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(frame), swap_view)
            }
        };
        #[cfg(not(feature = "capture"))]
        let frame = acquire!();
        #[cfg(not(feature = "capture"))]
        let swap_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        #[cfg(feature = "capture")]
        if let Some(pose) = self.readbacks.begin(world) {
            // A capture shoots one full-frame view from its own pose.
            let mut rw = world.res_mut::<RenderWorld>();
            rw.camera = pose;
            rw.views.clear();
        }

        self.ring.begin();
        self.timer.begin();
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let mut draws = 0u32;
        // Only the forward pass draws triangles.
        #[cfg_attr(not(feature = "forward"), allow(unused_mut))]
        let mut tris = 0u32;
        let sprite_count; #[cfg(feature = "msdf")] let ui_from: u32; // first HUD sprite (msdf::HUD_LAYER), drawn by the `ui` pass

        // ---- scene data -> uniforms and instance streams ----
        let (exposure, saturation, clear, pixel_art) = {
            let reg = world.res::<ParamRegistry>();
            (reg.f32(p.exposure), reg.f32(p.saturation), reg.vec3(p.clear_color), reg.bool(p.pixel_art))
        };
        // Live geocheck overrides: a magenta sky (the pass clear only; fog
        // keeps the real colour) and a sub-pixel projection jitter.
        // The geometry debug view also asks for plain materials, so its
        // instance colours are not multiplied by the look's material colours.
        #[cfg(feature = "capture")]
        let (clear, jitter, plain) = {
            let q = world.try_res::<evani_core::capture::CaptureQueue>();
            let q = q.as_deref();
            (
                q.and_then(|q| q.clear_override).unwrap_or(clear),
                q.and_then(|q| q.jitter_px),
                q.is_some_and(|q| q.plain_materials),
            )
        };
        #[cfg(not(feature = "capture"))]
        let plain = false;
        // Jitter and plain materials touch only the 3D path.
        #[cfg(all(feature = "capture", not(feature = "forward")))]
        let _ = jitter;
        #[cfg(not(feature = "forward"))]
        let _ = plain;
        self.sync_atlas(world, pixel_art);
        let sprite_off;
        #[cfg(feature = "shadows")]
        let shadow_draw: Option<(u32, u32)>;
        // Particle groups with live particles this frame (`prepare` decided
        // whether the pass and the draws run).
        #[cfg(feature = "particles")]
        let (particles_world, particles_screen): (bool, Vec<u32>) =
            match world.try_res::<crate::particles::ParticleSystem>() {
                Some(ps) if self.particles.run => (
                    ps.groups().iter().any(|g| g.key == crate::particles::KEY_WORLD),
                    ps.groups().iter().map(|g| g.key).filter(|&k| k != crate::particles::KEY_WORLD).collect(),
                ),
                _ => (false, Vec::new()),
            };
        // The world group draws in the forward pass (3D path only; without
        // it `RenderModule` reports the dropped draw).
        #[cfg(all(feature = "particles", not(feature = "forward")))]
        let _ = particles_world;
        // One frame uniform per view (split-screen), or one for the camera.
        #[cfg(feature = "forward")]
        self.views.clear();
        {
            #[cfg(feature = "forward")]
            let reg = world.res::<ParamRegistry>();
            #[cfg(feature = "forward")]
            let look = world.res::<LookData>();
            // No materials: the frame uniform falls back to plain white, lit.
            #[cfg(feature = "forward")]
            let materials: &[evani_core::plugin::MaterialDesc] = if plain { &[] } else { &look.materials };
            let mut rw = world.res_mut::<RenderWorld>();
            rw.viewport_css = css;
            rw.target_px = self.size;
            // ---- the 3D path: lighting, shading, view uniforms, instances ----
            #[cfg(feature = "forward")]
            {
                let lighting = p.lighting(&reg); #[cfg(feature = "camera-range")] crate::camera_range::sync(&reg); #[cfg(feature = "fog-color")] let lighting = crate::color_ext::apply(&reg, lighting);
                // Shadow map or blob shadows: the shading tail every view shares.
                #[cfg(feature = "shadows")]
                let plan = {
                    let fx = world.try_res::<evani_core::effects::EffectsStatus>();
                    let tiers = world.res::<Tiers>();
                    shadow::plan(&reg, &self.shadow_params, fx.as_deref(), &tiers, &rw, lighting.light_dir, self.max_dim)
                };
                #[cfg(not(feature = "shadows"))]
                let plan: Option<shadow::ShadowPlan> = {
                    let _ = &self.shadow_params;
                    None
                }; #[cfg(feature = "frustum-cull")] crate::cull::run(world, &mut rw, self.size, plan.as_ref().map(|sp| &sp.light_vp));
                // Plain materials (the geometry debug view) shade shadows grey: a
                // look's coloured shadow tint (the toon look's violet) would show
                // on the grey scene as patches that look like issues.
                let (tint, tint_strength) = (reg.vec3(p.shadow_tint), if plain { 0.0 } else { reg.f32(p.shadow_tint_strength) });
                let shading = match &plan {
                    Some(sp) => Shading {
                        light_vp: sp.light_vp,
                        shadow_map: true,
                        strength: sp.strength,
                        texel: 1.0 / sp.res as f32,
                        pcf_radius: sp.pcf_radius,
                        normal_offset: sp.normal_offset,
                        bias: sp.bias,
                        blobs: &[],
                        blob_strength: rw.blob_strength,
                        tint,
                        tint_strength,
                    },
                    None => Shading {
                        blobs: &rw.blobs,
                        blob_strength: rw.blob_strength,
                        tint,
                        tint_strength,
                        ..Shading::default()
                    },
                };
                shaders::write_shading(&mut self.frame_words, &shading); #[cfg(feature = "models")] let cones_read = plan.is_none(); #[cfg(feature = "shadow-cascades")] let (plan, near) = crate::shadow_ext::split(&reg, &world.res::<Tiers>(), &rw, lighting.light_dir, plan, self.max_dim, &mut self.frame_words); // no shadow map this frame: models read their baked sun cones
                #[cfg(feature = "shadows")]
                {
                    shadow_draw = plan.map(|sp| {
                        shaders::write_shadow_uniform(&mut self.shadows.words, &sp.light_vp, materials);
                        (self.ring.push(&self.shadows.words[..]), sp.res)
                    }); #[cfg(feature = "shadow-cascades")] { let n = near.map(|vp| { shaders::write_shadow_uniform(&mut self.shadows.words, &vp, materials); self.ring.push(&self.shadows.words[..]) }); self.shadows.near = n; }
                }
                let full = [0.0, 0.0, self.size[0] as f32, self.size[1].max(1) as f32];
                if rw.views.is_empty() {
                    let aspect = full[2] / full[3];
                    let vp = shaders::view_proj(&rw.camera, aspect);
                    #[cfg(feature = "capture")]
                    let vp = jitter.map_or(vp, |j| shaders::jitter_view_proj(&vp, j, self.size));
                    shaders::write_frame(&mut self.frame_words, &vp, rw.camera.eye, &lighting, materials);
                    self.views.push((self.ring.push(&self.frame_words[..]), full));
                    #[cfg(feature = "particles")]
                    if particles_world {
                        let words = crate::particles::draw_words(&vp, rw.camera.eye, crate::particles::KEY_WORLD, css);
                        self.particles.world.push(self.ring.push(&words));
                    }
                } else {
                    for v in &rw.views {
                        let px = v.pixels(self.size);
                        let vp = shaders::view_proj(&v.camera, px[2] / px[3].max(1.0));
                        #[cfg(feature = "capture")]
                        let vp = jitter.map_or(vp, |j| shaders::jitter_view_proj(&vp, j, [px[2] as u32, px[3] as u32]));
                        shaders::write_frame(&mut self.frame_words, &vp, v.camera.eye, &lighting, materials);
                        self.views.push((self.ring.push(&self.frame_words[..]), px));
                        #[cfg(feature = "particles")]
                        if particles_world {
                            let words = crate::particles::draw_words(&vp, v.camera.eye, crate::particles::KEY_WORLD, css);
                            self.particles.world.push(self.ring.push(&words));
                        }
                    }
                }

                shaders::pack_instances_with_decals(&rw.meshes, &rw.decals, &mut self.instances.staging, &mut self.counts); #[cfg(feature = "models")] self.models.prepare(&self.device, &self.queue, world, &rw, driver.started() && !driver.warming_up(), cones_read); #[cfg(feature = "point-lights")] self.lights.prepare(&self.queue, world, &rw, &self.views); #[cfg(feature = "kit-material")] if self.kit.prepare(&self.device, &self.queue, &self.layouts, world) { self.pipes.relaunch_kit(&self.device, &self.layouts, self.formats, world); } #[cfg(feature = "view-model")] self.vm_prepare(&rw, &lighting, materials); #[cfg(feature = "ambient")] self.ambient_prepare(world, &rw, &lighting, materials);
            }
            rw.sort_sprites(); #[cfg(feature = "msdf")] self.msdf.frame(&self.device, &self.queue, &self.layouts.msdf, &mut rw, world);
            #[cfg(all(not(feature = "sprite-art"), not(feature = "msdf")))]
            shaders::pack_sprites_uv(&rw.sprites, &self.atlas.uvs, &mut self.sprites.staging);
            #[cfg(all(feature = "sprite-art", not(feature = "msdf")))]
            crate::art::pack_sprites(&rw.sprites, &self.atlas.uvs, self.atlas.art_base, &mut self.sprites.staging); #[cfg(feature = "msdf")] { #[cfg(feature = "sprite-art")] let art_base = self.atlas.art_base; #[cfg(not(feature = "sprite-art"))] let art_base = 0; crate::msdf::pack_sprites(&rw.sprites, &rw.fonts, &self.atlas.uvs, art_base, &mut self.sprites.staging); }
            sprite_count = rw.sprites.len() as u32; #[cfg(feature = "msdf")] { ui_from = crate::msdf::hud_from(&rw.sprites); }
            #[cfg(not(all(feature = "lights2d", not(feature = "msdf"))))] { sprite_off = self.ring.push(&[css[0], css[1], 0.0, 0.0]); } #[cfg(all(feature = "lights2d", not(feature = "msdf")))] { let w = self.lights2d.prepare(world, &rw, &mut self.sprites.staging, &self.atlas.uvs, self.atlas.art_base, css); sprite_off = self.ring.push(&w); } #[cfg(feature = "pseudo3d")] { let uvs = &self.atlas.uvs; #[cfg(feature = "sprite-art")] let base = self.atlas.art_base; #[cfg(feature = "sprite-art")] let uv_of = |c: u16| crate::art::cell_uv(c, uvs, base); #[cfg(not(feature = "sprite-art"))] let uv_of = |c: u16| { let i = usize::from(c); uvs.get(i).or_else(|| uvs.get(i % shaders::ATLAS_CELLS)).copied().unwrap_or([0.0; 4]) }; self.road.prepare(&self.device, &self.queue, &self.layouts, world, css, uv_of); } // 2D lights: mark the lit sprites; the light buffer is the uniform
            // Screen particle groups draw between the sprite layers: each
            // after the sprites of its layer and below.
            #[cfg(feature = "particles")]
            for &key in &particles_screen {
                let Some(layer) = crate::particles::layer_of(key) else {
                    continue;
                };
                let split = rw.sprites.partition_point(|s| s.layer <= layer) as u32; #[cfg(feature = "msdf")] let split = split.min(ui_from); // screen particles stay in the scene
                let words = crate::particles::draw_words(&[0.0; 16], [0.0; 3], key, css);
                self.particles.screen.push((split, self.ring.push(&words)));
            }
            #[cfg(feature = "capture")]
            self.readbacks.note_hud(&rw);
            rw.clear();
        }
        #[cfg(feature = "forward")]
        self.instances.upload(&self.device, &self.queue);
        self.sprites.upload(&self.device, &self.queue);
        let tone_off = self.ring.push(&[exposure, saturation, 0.0, 0.0]);
        let present_off = self.ring.push(&[if self.encode_srgb { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]); #[cfg(feature = "clustered")] { let ts = self.timer.compute_pass("lights.bin"); self.lights.dispatch(&mut enc, ts); }

        // ---- shadow map: depth from the light, before the graph ----
        #[cfg(feature = "shadows")]
        if let Some((off, res)) = shadow_draw {
            let view = self.shadows.ensure(&self.device, &self.layouts, &self.ring.buf, res).clone();
            let ts = self.timer.pass("shadows");
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadows"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &view,
                    // The shadow map keeps conventional depth (1 = far;
                    // see crate::depth), unlike the camera passes.
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: ts,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.shadows.pipe); #[cfg(feature = "kit-material")] self.kit.bind_shadow(&mut rp); #[cfg(feature = "shadow-cascades")] cascade::viewport(&mut rp, res, 0);
            rp.set_bind_group(0, &self.small_bg, &[off]);
            rp.set_vertex_buffer(0, self.mesh_vb.slice(..));
            rp.set_vertex_buffer(1, self.instances.buf.slice(..));
            rp.set_index_buffer(self.mesh_ib.slice(..), wgpu::IndexFormat::Uint32);
            let mut first = 0u32;
            for (i, &n) in self.counts.iter().enumerate() {
                // Ground planes receive shadows but never cast them.
                if n > 0 && i != usize::from(mesh::MeshId::PLANE.0) {
                    let (fi, ic, bv) = self.mesh_ranges[i];
                    rp.draw_indexed(fi..fi + ic, bv, first..first + n);
                    draws += 1;
                }
                first += n;
            } #[cfg(feature = "models")] { draws += self.models.draw_shadows(&mut rp, &self.small_bg, off); } #[cfg(feature = "shadow-cascades")] { draws += self.draw_near_cascade(&mut rp, res); }
        } #[cfg(feature = "ambient")] self.ambient_frame(&mut enc, &mut draws);

        // ---- graph walk ----
        let graph = world.res::<RenderGraph>();
        let fx = world.res::<PostEffects>();
        let reg = world.res::<ParamRegistry>();
        // Keep the depth target past the forward pass only if an enabled
        // effect pass reads it.
        #[cfg(feature = "forward")]
        let depth_store = if depth_read {
            wgpu::StoreOp::Store
        } else {
            wgpu::StoreOp::Discard
        };
        for pass in graph.ordered().filter(|p| p.enabled) {
            #[cfg(feature = "compute")]
            if pass.kind == crate::graph::PassKind::Compute {
                #[cfg(feature = "particles")]
                if pass.name == crate::particles::SIM_PASS && self.particles.run {
                    let ts = self.timer.compute_pass(pass.name);
                    self.particles.dispatch(&mut enc, ts);
                }
                continue;
            }
            match (pass.owner, pass.name) {
                #[cfg(feature = "forward")]
                ("render", "forward") => {
                    let msaa = self.msaa.as_ref();
                    let pipe = match msaa {
                        Some(_) => self.pipes.engine_ms(pipelines::FORWARD),
                        None => self.pipes.engine(pipelines::FORWARD),
                    };
                    let (Some(pipe), Some(hdr), Some(depth)) =
                        (pipe, find_target(&self.targets, "hdr"), find_target(&self.targets, "depth"))
                    else {
                        continue;
                    };
                    // With MSAA: draw 4x, resolve colour into hdr, and keep
                    // the 4x depth only for the depth resolve below.
                    let (color_view, resolve, color_store, depth_view) = match msaa {
                        Some(m) => (&m.color, Some(&hdr.view), wgpu::StoreOp::Discard, &m.depth),
                        None => (&hdr.view, None, wgpu::StoreOp::Store, &depth.view),
                    };
                    let ts = self.timer.pass(pass.name);
                    let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("forward"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: color_view,
                            depth_slice: None,
                            resolve_target: resolve,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: f64::from(clear[0]),
                                    g: f64::from(clear[1]),
                                    b: f64::from(clear[2]),
                                    a: 1.0,
                                }),
                                store: color_store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(crate::depth::CLEAR),
                                store: depth_store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: ts,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    rp.set_pipeline(pipe);
                    rp.set_vertex_buffer(0, self.mesh_vb.slice(..));
                    rp.set_vertex_buffer(1, self.instances.buf.slice(..));
                    rp.set_index_buffer(self.mesh_ib.slice(..), wgpu::IndexFormat::Uint32); #[cfg(feature = "point-lights")] self.lights.bind(&mut rp); #[cfg(feature = "kit-material")] self.kit.bind(&mut rp);
                    #[cfg(feature = "shadows")]
                    let frame_bg = &self.shadows.fwd_bg; #[cfg(feature = "ambient")] let frame_bg = self.ambient.bg.as_ref().unwrap_or(frame_bg);
                    #[cfg(not(feature = "shadows"))]
                    let frame_bg = &self.frame_bg;
                    for &(off, px) in &self.views {
                        rp.set_bind_group(0, frame_bg, &[off]);
                        rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
                        let mut first = 0u32;
                        for (i, &n) in self.counts.iter().enumerate() {
                            if n == 0 {
                                continue;
                            }
                            let (fi, ic, bv) = self.mesh_ranges[i];
                            rp.draw_indexed(fi..fi + ic, bv, first..first + n);
                            first += n;
                            draws += 1;
                            tris = tris.saturating_add(n.saturating_mul(self.mesh_tris[i]));
                        }
                    } #[cfg(feature = "models")] { draws += self.models.draw(&mut rp, frame_bg, &self.views, msaa.is_some(), &self.pipes, &mut tris); }
                    // World particles after every opaque mesh, view by view.
                    #[cfg(feature = "particles")]
                    for (&off, &(_, px)) in self.particles.world.iter().zip(&self.views) {
                        rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
                        let atlas_bg = if pixel_art { &self.atlas.bg[1] } else { &self.atlas.bg[0] };
                        draws += self.particles.draw_world(&mut rp, atlas_bg, off, msaa.is_some());
                    }
                    drop(rp); #[cfg(feature = "view-model")] { draws += self.vm_draw(&mut enc, hdr, &mut tris); }
                    // Effects read one-sample depth: resolve the 4x depth
                    // (nearest sample) into the depth target, only when one
                    // does. Timed as part of the forward pass.
                    if let (Some(m), Some(resolve_pipe), true) = (msaa, self.pipes.depth_resolve(), depth_read) {
                        let ts = self.timer.pass(pass.name);
                        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("depth_resolve"),
                            color_attachments: &[],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &depth.view,
                                depth_ops: Some(wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(crate::depth::CLEAR),
                                    store: wgpu::StoreOp::Store,
                                }),
                                stencil_ops: None,
                            }),
                            timestamp_writes: ts,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        rp.set_pipeline(resolve_pipe);
                        rp.set_bind_group(0, &m.depth_bg, &[]);
                        rp.draw(0..3, 0..1);
                        draws += 1;
                    }
                }
                ("render", "sprites") => {
                    #[cfg(feature = "particles")]
                    let screen_groups = self.particles.screen.as_slice();
                    #[cfg(not(feature = "particles"))]
                    let screen_groups: &[(u32, u32)] = &[];
                    #[cfg(feature = "msdf")] let sprite_count = ui_from; // With the 3D path the forward pass cleared `hdr` and an
                    // empty sprites pass is skipped; without it this pass is
                    // the first writer and always runs, to clear.
                    #[cfg(feature = "forward")] #[cfg(not(feature = "pseudo3d"))]
                    if sprite_count == 0 && screen_groups.is_empty() {
                        continue;
                    } #[cfg(all(feature = "forward", feature = "pseudo3d"))] if sprite_count == 0 && screen_groups.is_empty() && !self.road.drawing() { continue; }
                    let (Some(pipe), Some(hdr)) = (self.pipes.engine(pipelines::SPRITE), find_target(&self.targets, "hdr")) else {
                        continue;
                    };
                    let ts = self.timer.pass(pass.name);
                    #[cfg(feature = "forward")]
                    let mut rp = begin_load(&mut enc, &hdr.view, "sprites", ts);
                    #[cfg(not(feature = "forward"))]
                    let mut rp = begin_clear(&mut enc, &hdr.view, "sprites", ts, clear);
                    let atlas_bg = if pixel_art { &self.atlas.bg[1] } else { &self.atlas.bg[0] }; #[cfg(feature = "pseudo3d")] { draws += self.road.draw(&mut rp, atlas_bg, sprite_off); }
                    // The sprites in runs between the screen particle groups
                    // (without groups: one draw of every sprite, as before).
                    let mut drawn = 0u32;
                    let groups = screen_groups.iter().map(|&(split, off)| (split, Some(off)));
                    for (split, _group) in groups.chain(core::iter::once((sprite_count, None))) {
                        if split > drawn {
                            rp.set_pipeline(pipe);
                            rp.set_bind_group(0, atlas_bg, &[sprite_off]); #[cfg(feature = "msdf")] rp.set_bind_group(1, &self.msdf.bg, &[]);
                            rp.set_vertex_buffer(0, self.sprites.buf.slice(..));
                            rp.draw(0..6, drawn..split);
                            draws += 1;
                            drawn = split;
                        }
                        #[cfg(feature = "particles")]
                        if let Some(off) = _group {
                            draws += self.particles.draw_screen(&mut rp, atlas_bg, off);
                        }
                    }
                } #[cfg(feature = "msdf")] ("render", "ui") => draws += self.draw_ui(&mut enc, pass.name, ui_from, sprite_count, sprite_off, pixel_art),
                ("render", "tonemap") | ("render", "present") => {
                    let (idx, src, off) = if pass.name == "tonemap" {
                        (pipelines::TONEMAP, "hdr", tone_off)
                    } else {
                        (pipelines::PRESENT, "ldr", present_off)
                    };
                    let dst_view = if pass.name == "present" {
                        Some(&swap_view)
                    } else {
                        find_target(&self.targets, "ldr").map(|t| &t.view)
                    };
                    let (Some(pipe), Some(src), Some(dst)) =
                        (self.pipes.engine(idx), find_target(&self.targets, src).and_then(|t| t.bg.as_ref()), dst_view)
                    else {
                        continue;
                    };
                    let ts = self.timer.pass(pass.name);
                    let mut rp = begin_load(&mut enc, dst, pass.name, ts);
                    rp.set_pipeline(pipe);
                    rp.set_bind_group(0, src, &[off]);
                    rp.draw(0..3, 0..1);
                    draws += 1;
                }
                _ => {
                    let Some((ei, effect)) = fx.for_pass(pass.name) else {
                        continue;
                    };
                    let mut rec = Recorder {
                        gpu: GpuRefs {
                            targets: &self.targets,
                            pipes: &self.pipes,
                            small_bg: &self.small_bg,
                            small_depth_bg: self.small_depth_bg.as_ref(),
                        },
                        enc: &mut enc,
                        ring: &mut self.ring,
                        timer: &mut self.timer,
                        effect: ei,
                        shaders: effect.shaders(),
                        pass: pass.name,
                        draws: &mut draws,
                    };
                    effect.record(pass.name, &reg, &mut rec);
                }
            }
        }
        drop((graph, fx, reg));
        #[cfg(feature = "capture")]
        {
            let ldr = find_target(&self.targets, "ldr").map(|t| (&t.tex, t.size));
            self.readbacks.record(world, &self.device, &mut enc, ldr);
        }

        self.timer.resolve(&mut enc);
        self.ring.upload(&self.queue);
        if self.ring.overflowed && !self.ring_warned {
            self.ring_warned = true;
            world.res_mut::<ErrorLog>().error("uniform ring full: some draws reused a slot");
        }
        self.queue.submit([enc.finish()]);
        #[cfg(feature = "capture")]
        if let Some(frame) = frame {
            self.queue.present(frame);
        }
        #[cfg(not(feature = "capture"))]
        self.queue.present(frame);
        self.timer.after_submit(&self.queue);

        let mut st = world.res_mut::<RenderStats>();
        st.draw_calls = draws;
        st.triangles = tris;
        st.sprites = sprite_count;
        #[cfg(feature = "particles")]
        {
            st.particles = self.particles.live;
        }
        #[cfg(feature = "capture")]
        self.readbacks.after_submit(&st);
    }

    /// Re-upload the mesh buffers when the `MeshLibrary` changed (a game
    /// builds its spline roads once, so this runs on the first frames only).
    #[cfg(feature = "meshes")]
    fn sync_meshes(&mut self, world: &World) {
        let Some(lib) = world.try_res::<mesh::MeshLibrary>() else {
            return;
        };
        if lib.revision() == self.mesh_rev {
            return;
        }
        let bufs = mesh::mesh_buffers(Some(&lib));
        let (vb, ib) = mesh_gpu_buffers(&self.device, &self.queue, &bufs);
        self.mesh_vb = vb;
        self.mesh_ib = ib;
        self.mesh_ranges = bufs.ranges;
        self.mesh_tris = bufs.tris;
        self.mesh_rev = lib.revision();
    }
}

/// Vertex and index buffers holding `b`.
#[cfg(feature = "forward")]
fn mesh_gpu_buffers(device: &wgpu::Device, queue: &wgpu::Queue, b: &mesh::MeshBuffers) -> (wgpu::Buffer, wgpu::Buffer) {
    let buffer = |label, bytes: &[u8], usage| {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (bytes.len() as u64).max(4).next_multiple_of(4),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buf, 0, bytes);
        buf
    };
    (
        buffer("evani.meshes.vb", &b.vb, wgpu::BufferUsages::VERTEX),
        buffer("evani.meshes.ib", &b.ib, wgpu::BufferUsages::INDEX),
    )
}

/// The sprite atlas texture: built-in cells on top, a kit's sheet below,
/// one bind group per filter (`[linear, nearest]`) and the uv table.
struct Atlas {
    _tex: wgpu::Texture,
    bg: [wgpu::BindGroup; 2],
    size: [u32; 2],
    /// Revision of the sheet in the texture (`None`: no sheet).
    sheet_rev: Option<u64>,
    /// `SpriteArt` revision in the texture.
    #[cfg(feature = "sprite-art")]
    art_rev: u64,
    /// uv table index of named cell `ART_CELL0` (after the built-in cells
    /// and the kit's sheet frames).
    #[cfg(feature = "sprite-art")]
    art_base: usize,
    /// The packed layout and each frame's pixel-art flag (uv rebuilds).
    #[cfg(feature = "sprite-art")]
    layout: Option<(crate::art::Packed, Vec<bool>)>,
    uvs: Vec<[f32; 4]>,
    uv_nearest: bool,
}

impl Atlas {
    fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &Layouts,
        samplers: [&wgpu::Sampler; 2],
        ring: &wgpu::Buffer,
        look: &LookData,
        sheet: Option<&SpriteSheet>,
    ) -> Self {
        let sheet = sheet.filter(|s| s.is_valid() && s.width > 0 && s.height > 0);
        let size = atlas_size(sheet);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("evani.atlas"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let write = |px: &[u8], origin: [u32; 2], w: u32, h: u32| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: origin[0],
                        y: origin[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                px,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        };
        write(&build_atlas(look), [0, 0], crate::atlas::ATLAS_PX, crate::atlas::ATLAS_PX);
        if let Some(s) = sheet {
            write(&s.rgba8, SHEET_ORIGIN, s.width, s.height);
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bg = [
            a_group(device, layouts, &view, samplers[0], ring),
            a_group(device, layouts, &view, samplers[1], ring),
        ];
        let mut uvs = Vec::new();
        let frames = sheet.map_or(&[][..], |s| &s.frames[..]);
        shaders::sprite_uv_table(size, SHEET_ORIGIN, frames, false, &mut uvs);
        Self {
            _tex: tex,
            bg,
            size,
            sheet_rev: sheet.map(|s| s.revision),
            #[cfg(feature = "sprite-art")]
            art_rev: 0,
            #[cfg(feature = "sprite-art")]
            art_base: uvs.len(),
            #[cfg(feature = "sprite-art")]
            layout: None,
            uvs,
            uv_nearest: false,
        }
    }

    /// Sprite art: the built-in cells, the kit's sheet and every named
    /// frame packed with gutters into one atlas with a mip chain. A set that
    /// doesn't fit the device is logged and dropped (named art first, then
    /// the kit's sheet); the plain atlas is the last resort.
    #[cfg(feature = "sprite-art")]
    fn build_art(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &Layouts,
        samplers: [&wgpu::Sampler; 2],
        ring: &wgpu::Buffer,
        world: &World,
        max_dim: u32,
    ) -> Self {
        use crate::art::{self, ArtSource, FrameRef};
        use crate::atlas::{ATLAS_CELL_PX, ATLAS_PX};
        use crate::world::ATLAS_GRID;
        let look = world.res::<LookData>();
        let sheet = world.try_res::<SpriteSheet>();
        let sheet = sheet.as_deref().filter(|s| s.is_valid() && s.width > 0 && s.height > 0);
        let named = world.try_res::<art::SpriteArt>();
        let art_rev = named.as_ref().map_or(0, |a| a.revision());
        let builtin = build_atlas(&look);
        let mut frames: Vec<FrameRef> = (0..ATLAS_GRID * ATLAS_GRID)
            .map(|c| FrameRef {
                name: "",
                source: ArtSource::Engine,
                sheet: &builtin,
                sheet_w: ATLAS_PX,
                rect: [c % ATLAS_GRID * ATLAS_CELL_PX, c / ATLAS_GRID * ATLAS_CELL_PX, ATLAS_CELL_PX, ATLAS_CELL_PX],
                pixel_art: false,
            })
            .collect();
        let engine = frames.len();
        if let Some(s) = sheet {
            frames.extend(s.frames.iter().map(|r| FrameRef {
                name: "",
                source: ArtSource::Kit,
                sheet: &s.rgba8,
                sheet_w: s.width,
                rect: *r,
                pixel_art: false,
            }));
        }
        let base = frames.len();
        if let Some(a) = named.as_deref() {
            frames.extend(a.frames());
        }
        let mut fitted = None;
        let mut tried = usize::MAX;
        for keep in [frames.len(), base, engine] {
            if keep == tried {
                continue;
            }
            tried = keep;
            match art::pack(&frames[..keep], max_dim) {
                Ok(p) => {
                    fitted = Some((keep, p));
                    break;
                }
                Err(e) => world.res_mut::<ErrorLog>().error(format!("{e} ({keep} frames); dropping the rest")),
            }
        }
        let Some((keep, packed)) = fitted else {
            let mut a = Self::build(device, queue, layouts, samplers, ring, &look, sheet);
            a.art_rev = art_rev;
            return a;
        };
        let frames = &frames[..keep];
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("evani.atlas"),
            size: wgpu::Extent3d {
                width: packed.size[0],
                height: packed.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: packed.levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (k, px) in art::build_levels(frames, &packed).iter().enumerate() {
            let (w, h) = (packed.size[0] >> k, packed.size[1] >> k);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: k as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                px,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        // `render.pixel_art` keeps level 0: the nearest group sees no mips.
        let level0 = tex.create_view(&wgpu::TextureViewDescriptor {
            mip_level_count: Some(1),
            ..Default::default()
        });
        let bg = [
            a_group(device, layouts, &view, samplers[0], ring),
            a_group(device, layouts, &level0, samplers[1], ring),
        ];
        let pixel: Vec<bool> = frames.iter().map(|f| f.pixel_art).collect();
        let mut uvs = Vec::new();
        art::uv_table(&packed, &pixel, false, &mut uvs);
        Self {
            _tex: tex,
            bg,
            size: packed.size,
            sheet_rev: sheet.map(|s| s.revision),
            art_rev,
            art_base: base,
            layout: Some((packed, pixel)),
            uvs,
            uv_nearest: false,
        }
    }
}

fn a_group(
    device: &wgpu::Device,
    layouts: &Layouts,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    ring: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layouts.a,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: ring,
                    offset: 0,
                    size: wgpu::BufferSize::new(SMALL_BINDING),
                }),
            },
        ],
    })
}

fn begin_load<'e>(
    enc: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &'static str,
    ts: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: ts,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// The sprites pass as the first writer of `hdr` (no 3D path): clear to
/// `clear` (the live geocheck's magenta sky included), then draw.
#[cfg(not(feature = "forward"))]
fn begin_clear<'e>(
    enc: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &'static str,
    ts: Option<wgpu::RenderPassTimestampWrites<'_>>,
    clear: [f32; 3],
) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(clear[0]),
                    g: f64::from(clear[1]),
                    b: f64::from(clear[2]),
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: ts,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

struct GpuRefs<'a> {
    targets: &'a [Target],
    pipes: &'a Pipelines,
    small_bg: &'a wgpu::BindGroup,
    small_depth_bg: Option<&'a wgpu::BindGroup>,
}

/// The GPU [`PostRecorder`]: one render pass per effect draw.
struct Recorder<'a> {
    gpu: GpuRefs<'a>,
    enc: &'a mut wgpu::CommandEncoder,
    ring: &'a mut Ring,
    timer: &'a mut Timer,
    effect: usize,
    /// The effect's shaders (which ones read depth).
    shaders: &'static [EffectShader],
    pass: &'static str,
    draws: &'a mut u32,
}

impl PostRecorder for Recorder<'_> {
    fn target_size(&self, target: &str) -> [f32; 2] {
        self.gpu
            .targets
            .iter()
            .find(|t| t.name == target)
            .map_or([0.0; 2], |t| [t.size[0] as f32, t.size[1] as f32])
    }

    fn draw(&mut self, shader: usize, src: Option<&str>, dst: &str, uniforms: &[f32]) {
        let Some(pipe) = self.gpu.pipes.effect(self.effect, shader) else {
            return;
        };
        let Some(dst) = self.gpu.targets.iter().find(|t| t.name == dst) else {
            return;
        };
        let depth = self.shaders.get(shader).is_some_and(|s| s.depth);
        let bg = match src {
            Some(s) => {
                let t = self.gpu.targets.iter().find(|t| t.name == s);
                match t.and_then(|t| if depth { t.bg_depth.as_ref() } else { t.bg.as_ref() }) {
                    Some(bg) => bg,
                    None => return,
                }
            }
            None if depth => match self.gpu.small_depth_bg {
                Some(bg) => bg,
                None => return,
            },
            None => self.gpu.small_bg,
        };
        let off = self.ring.push(uniforms);
        let ts = self.timer.pass(self.pass);
        let mut rp = begin_load(self.enc, &dst.view, self.pass, ts);
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, bg, &[off]);
        rp.draw(0..3, 0..1);
        *self.draws += 1;
    }
}

fn find_target<'a>(targets: &'a [Target], name: &str) -> Option<&'a Target> {
    targets.iter().find(|t| t.name == name)
}

#[cfg(feature = "msdf")]
impl Gpu {
    /// The `ui` pass (S3-13b fix, DECISIONS "The HUD after post"): the HUD
    /// sprites (`msdf::HUD_LAYER` and above, `from..count` of the sorted
    /// list) drawn with the MSDF sprite variant into `ldr` after the tone map
    /// and every post effect, so their colours land as authored. Returns the
    /// draw count.
    fn draw_ui(&mut self, enc: &mut wgpu::CommandEncoder, name: &'static str, from: u32, count: u32, off: u32, pixel_art: bool) -> u32 {
        if from >= count {
            return 0;
        }
        let (Some(pipe), Some(ldr)) = (self.pipes.ui(), find_target(&self.targets, "ldr")) else {
            return 0;
        };
        let ts = self.timer.pass(name);
        let mut rp = begin_load(enc, &ldr.view, name, ts);
        let atlas_bg = if pixel_art { &self.atlas.bg[1] } else { &self.atlas.bg[0] };
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, atlas_bg, &[off]);
        rp.set_bind_group(1, &self.msdf.bg, &[]);
        rp.set_vertex_buffer(0, self.sprites.buf.slice(..));
        rp.draw(0..6, from..count);
        1
    }
}

// The first-person view-model pass (feature `view-model`, S5-E3), at the
// end so no line above moves.
#[cfg(feature = "view-model")]
#[path = "view_model.rs"]
mod view_model;
#[cfg(feature = "shadow-cascades")]
mod cascade;
#[cfg(feature = "ambient")]
mod ambient;
// The shader pre-warm hold (feature `prewarm`, ChimpDuty intake).
#[cfg(feature = "prewarm")]
mod prewarm;
