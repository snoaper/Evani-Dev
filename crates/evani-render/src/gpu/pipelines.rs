//! Pipeline cache and builder. Every build (the first one too) is a "job":
//! push an error scope, create the shader module and the pipeline, pop the
//! scope, then a spawned task awaits the scope result and the compilation
//! info and posts the outcome to an inbox the frame polls. A pipeline is
//! only installed once it is known to be valid, so a broken slot body never
//! replaces a working pipeline.

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::slots::{RebuildHandle, ShaderSlots, SlotCompileResult, SlotId, SlotMessage, SlotRebuilds};

use crate::graph::{RenderGraph, TargetKind};
use crate::post::{Blend, PostEffects};
#[cfg(feature = "forward")]
use crate::shaders::DEPTH_RESOLVE_WGSL;
#[cfg(feature = "forward")]
use crate::shaders::INSTANCE_WORDS;
use crate::shaders::{ENGINE_SHADERS, SPRITE_WORDS};
use crate::splice::{self, Bodies, Spliced};

/// Which pipeline: an engine shader (index into [`ENGINE_SHADERS`]), its
/// 4x MSAA variant, the MSAA depth resolve, or an effect shader (effect
/// index in [`PostEffects`], shader index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Engine(usize),
    /// The 4-sample variant of engine shader `i` (only [`FORWARD`]). Kept
    /// apart from the engine indices so engine shaders can be added freely.
    #[cfg(feature = "forward")]
    EngineMs(usize),
    /// Copies the 4x depth into the single-sample `depth` target when an
    /// effect reads depth.
    #[cfg(feature = "forward")]
    DepthResolve,
    Effect(usize, usize), #[cfg(feature = "models")] Model(bool), #[cfg(feature = "msdf")] Ui, #[cfg(feature = "skinning")] Skin(bool), #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] Blend(bool), #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] Glass,
    /// The plain stand-in for [`Key::Model`] / [`Key::Skin`] (same module,
    /// fragment stage `model_fs`): drawn until the full pipeline is built.
    #[cfg(feature = "models")]
    ModelLite(bool),
    #[cfg(feature = "skinning")]
    SkinLite(bool), #[cfg(feature = "material-split")] ModelSplit(u8, bool, u8, u8),
}

/// Forward-pass MSAA sample count when a tier asks for it.
#[cfg(feature = "forward")]
pub const MSAA_SAMPLES: u32 = 4;
/// Engine shaders with a 4x variant (indexes `Pipelines::engine_ms`).
#[cfg(feature = "forward")]
const MS_ENGINE: [usize; 1] = [FORWARD];

// Indices into `ENGINE_SHADERS`: the forward shader comes first, and only
// with the 3D path (feature `forward`).
#[cfg(feature = "forward")]
pub const FORWARD: usize = 0;
#[cfg(feature = "forward")]
pub const SPRITE: usize = 1;
#[cfg(feature = "forward")]
pub const TONEMAP: usize = 2;
#[cfg(feature = "forward")]
pub const PRESENT: usize = 3;
#[cfg(not(feature = "forward"))]
pub const SPRITE: usize = 0;
#[cfg(not(feature = "forward"))]
pub const TONEMAP: usize = 1;
#[cfg(not(feature = "forward"))]
pub const PRESENT: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// Start-up build; `fallback` once retried with the engine's default
    /// slot bodies.
    Initial { fallback: bool },
    /// Part of slot rebuild batch `id`.
    Slot(u64),
}

struct Outcome {
    epoch: u32,
    key: Key,
    purpose: Purpose,
    pipeline: wgpu::RenderPipeline,
    scope_error: Option<String>,
    /// (severity, text, line, column, length) in the spliced source.
    messages: Vec<(&'static str, String, u32, u32, u32)>,
    spliced: Spliced,
}

impl Outcome {
    fn ok(&self) -> bool {
        self.scope_error.is_none() && !self.messages.iter().any(|m| m.0 == "error")
    }

    /// Messages mapped to slot `focus` (or all slots), plus the scope error
    /// when the compiler said nothing.
    fn slot_messages(&self, focus: Option<SlotId>, out: &mut Vec<SlotMessage>) {
        for (sev, text, line, col, len) in &self.messages {
            out.push(self.spliced.message(focus, sev, text, *line, *col, *len));
        }
        if let Some(e) = &self.scope_error
            && !self.messages.iter().any(|m| m.0 == "error")
        {
            out.push(SlotMessage {
                severity: "error",
                text: e.clone(),
                line: 0,
                column: 0,
                length: 0,
                source_line: None,
            });
        }
    }

    fn summary(&self) -> String {
        let mut v = Vec::new();
        self.slot_messages(None, &mut v);
        match v.iter().find(|m| m.severity == "error") {
            Some(m) if m.line > 0 => format!("line {}:{}: {}", m.line, m.column, m.text),
            Some(m) => m.text.clone(),
            None => "invalid pipeline".to_string(),
        }
    }
}

struct Batch {
    id: u64,
    handle: RebuildHandle,
    slot: SlotId,
    slot_name: &'static str,
    body: String,
    requested_ms: f64,
    expected: usize,
    built: Vec<(Key, wgpu::RenderPipeline)>,
    failed: bool,
    messages: Vec<SlotMessage>,
}

/// Bind group layouts every pipeline is built against.
pub struct Layouts {
    /// Texture + filtering sampler + dynamic uniform.
    pub a: wgpu::BindGroupLayout,
    /// Dynamic uniform only.
    pub b: wgpu::BindGroupLayout,
    /// Layout A plus the scene depth (`texture_depth_2d`) at binding 3.
    pub ad: wgpu::BindGroupLayout,
    /// Layout B plus the scene depth (`texture_depth_2d`) at binding 1.
    pub bd: wgpu::BindGroupLayout,
    /// The 4x scene depth (`texture_depth_multisampled_2d`) at binding 0,
    /// for the depth resolve (3D path only).
    #[cfg(feature = "forward")]
    pub dms: wgpu::BindGroupLayout,
    pub pl_a: wgpu::PipelineLayout,
    pub pl_b: wgpu::PipelineLayout,
    pub pl_ad: wgpu::PipelineLayout,
    pub pl_bd: wgpu::PipelineLayout, #[cfg(feature = "effect-mesh")] pub adf: wgpu::BindGroupLayout, #[cfg(feature = "effect-mesh")] pub pl_adf: wgpu::PipelineLayout, #[cfg(feature = "msdf")] pub msdf: super::msdf::MsdfLayouts, #[cfg(feature = "point-lights")] pub lights: super::lights::LightLayouts, #[cfg(feature = "kit-material")] pub kit: kit_material::KitLayouts,
    #[cfg(feature = "forward")]
    pub pl_dms: wgpu::PipelineLayout,
    /// Forward with shadows (feature `shadows`): dynamic uniform, the
    /// shadow map (`texture_depth_2d`) at 1 and a comparison sampler at 2.
    #[cfg(feature = "shadows")]
    pub fwd: wgpu::BindGroupLayout, #[cfg(feature = "ambient")] pub fwd_amb: wgpu::BindGroupLayout,
    #[cfg(feature = "shadows")] #[cfg_attr(feature = "point-lights", allow(dead_code))]
    pub pl_fwd: wgpu::PipelineLayout,
}

impl Layouts {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let depth = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let a = layout("evani.layout.a", &[texture, sampler, uniform(2)]);
        let b = layout("evani.layout.b", &[uniform(0)]);
        let ad = layout("evani.layout.ad", &[texture, sampler, uniform(2), depth(3)]);
        let bd = layout("evani.layout.bd", &[uniform(0), depth(1)]);
        #[cfg(feature = "forward")]
        let dms = layout(
            "evani.layout.dms",
            &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        );
        let pl = |l: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(l)],
                immediate_size: 0,
            })
        };
        let pl_a = pl(&a);
        let pl_b = pl(&b);
        let pl_ad = pl(&ad);
        let pl_bd = pl(&bd); #[cfg(feature = "effect-mesh")] let adf = layout("evani.layout.adf", &[texture, wgpu::BindGroupLayoutEntry { visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ..sampler }, uniform(2), depth(3), wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ..texture }]); #[cfg(feature = "effect-mesh")] let pl_adf = pl(&adf); #[cfg(feature = "msdf")] let msdf = super::msdf::MsdfLayouts::new(device, &a);
        #[cfg(feature = "forward")]
        let pl_dms = pl(&dms);
        #[cfg(feature = "shadows")]
        let fwd = layout(
            "evani.layout.fwd",
            &[
                uniform(0),
                depth(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        );
        #[cfg(all(feature = "shadows", not(feature = "ambient")))]
        let pl_fwd = pl(&fwd); #[cfg(all(feature = "point-lights", feature = "shadows", not(feature = "ambient")))] let lights = super::lights::LightLayouts::new(device, &fwd); #[cfg(all(feature = "point-lights", not(feature = "shadows")))] let lights = super::lights::LightLayouts::new(device, &b); #[cfg(feature = "ambient")] let fwd_amb = super::ambient::frame_layout(device); #[cfg(feature = "ambient")] let pl_fwd = pl(&fwd_amb); #[cfg(all(feature = "point-lights", feature = "ambient"))] let lights = super::lights::LightLayouts::new(device, &fwd_amb); #[cfg(all(feature = "kit-material", feature = "shadows", not(feature = "ambient")))] let kit_frame = &fwd; #[cfg(all(feature = "kit-material", feature = "ambient"))] let kit_frame = &fwd_amb; #[cfg(all(feature = "kit-material", not(feature = "shadows")))] let kit_frame = &b; #[cfg(all(feature = "kit-material", feature = "point-lights"))] let kit = kit_material::KitLayouts::new(device, kit_frame, &b, &[Some(&lights.empty), Some(&lights.lights)]); #[cfg(all(feature = "kit-material", not(feature = "point-lights")))] let kit = kit_material::KitLayouts::new(device, kit_frame, &b, &[]);
        Self {
            a,
            b,
            ad,
            bd,
            #[cfg(feature = "forward")]
            dms,
            pl_a,
            pl_b,
            pl_ad,
            pl_bd, #[cfg(feature = "effect-mesh")] adf, #[cfg(feature = "effect-mesh")] pl_adf, #[cfg(feature = "msdf")] msdf, #[cfg(feature = "point-lights")] lights, #[cfg(feature = "kit-material")] kit,
            #[cfg(feature = "forward")]
            pl_dms,
            #[cfg(feature = "shadows")]
            fwd, #[cfg(feature = "ambient")] fwd_amb,
            #[cfg(feature = "shadows")]
            pl_fwd,
        }
    }

    /// The forward pass's pipeline layout.
    #[cfg(feature = "forward")] #[cfg_attr(feature = "kit-material", allow(unreachable_code))]
    fn forward(&self) -> &wgpu::PipelineLayout { #[cfg(feature = "kit-material")] return &self.kit.pl_forward;
        #[cfg(all(feature = "shadows", not(feature = "point-lights")))]
        {
            &self.pl_fwd
        }
        #[cfg(not(any(feature = "shadows", feature = "point-lights")))]
        {
            &self.pl_b
        } #[cfg(feature = "point-lights")] { &self.lights.pl_forward }
    }
}

/// The shadow-map depth pipeline (feature `shadows`): the mesh and instance
/// streams of the forward pass, vertex stage only, into Depth32Float with a
/// slope-scaled bias. Static WGSL with no slots (naga-validated in tests),
/// so it is built directly rather than through the slot job queue.
#[cfg(feature = "shadows")]
pub fn shadow_pipeline(dev: &wgpu::Device, layouts: &Layouts) -> wgpu::RenderPipeline {
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.shadows_depth"),
        source: wgpu::ShaderSource::Wgsl(crate::shaders::SHADOWS_DEPTH.source.into()),
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &MESH_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: (INSTANCE_WORDS * 4) as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRS,
        }),
    ];
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.shadows"),
        layout: Some(&layouts.pl_b),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    })
}

/// What the builder needs to know about the frame's formats.
#[derive(Debug, Clone, Copy)]
pub struct Formats {
    pub surface: wgpu::TextureFormat,
}

pub fn target_format(kind: TargetKind, f: Formats) -> wgpu::TextureFormat {
    match kind {
        TargetKind::HdrColor => wgpu::TextureFormat::Rgba16Float,
        TargetKind::Depth => wgpu::TextureFormat::Depth32Float,
        TargetKind::Ldr => wgpu::TextureFormat::Rgba8UnormSrgb,
        TargetKind::Swapchain => f.surface,
        // Compute buffers are not textures; never asked for.
        #[cfg(feature = "compute")]
        TargetKind::Buffer => wgpu::TextureFormat::R32Uint, #[cfg(feature = "game-passes")] TargetKind::Game => wgpu::TextureFormat::Rgba16Float, // game targets carry their own format (game_pass::Format)
    }
}

fn blend_state(b: Blend) -> Option<wgpu::BlendState> {
    let c = |src, dst| wgpu::BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation: wgpu::BlendOperation::Add,
    };
    use wgpu::BlendFactor as F;
    match b {
        Blend::Replace => None,
        Blend::Additive => Some(wgpu::BlendState {
            color: c(F::One, F::One),
            alpha: c(F::One, F::One),
        }),
        Blend::Multiply => Some(wgpu::BlendState {
            color: c(F::Dst, F::Zero),
            alpha: c(F::DstAlpha, F::Zero),
        }),
    }
}

const ALPHA_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

#[cfg(feature = "forward")]
const MESH_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
#[cfg(feature = "forward")]
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4];
const SPRITE_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

/// The installed pipelines plus the job inbox.
pub struct Pipelines {
    engine: [Option<wgpu::RenderPipeline>; ENGINE_SHADERS.len()],
    /// 4x variants, in `MS_ENGINE` order. Not needed to start drawing:
    /// until one is built the forward pass runs without MSAA.
    #[cfg(feature = "forward")]
    engine_ms: [Option<wgpu::RenderPipeline>; MS_ENGINE.len()],
    #[cfg(feature = "forward")]
    depth_resolve: Option<wgpu::RenderPipeline>,
    effects: Vec<Vec<Option<wgpu::RenderPipeline>>>,
    inbox: Rc<RefCell<Vec<Outcome>>>,
    epoch: u32,
    batches: Vec<Batch>,
    next_batch: u64, #[cfg(feature = "models")] model: [Option<wgpu::RenderPipeline>; 2], #[cfg(feature = "msdf")] ui: Option<wgpu::RenderPipeline>, #[cfg(feature = "skinning")] skin: [Option<wgpu::RenderPipeline>; 2], #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] blend: [Option<wgpu::RenderPipeline>; 2], #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] glass: Option<wgpu::RenderPipeline>,
    /// Pipeline jobs launched and not finished yet.
    in_flight: core::cell::Cell<u32>,
    /// Builds waiting, warming up off the main thread, the variant cache.
    q: queue::Queue, #[cfg(feature = "models")] model_lite: [Option<wgpu::RenderPipeline>; 2], #[cfg(feature = "skinning")] skin_lite: [Option<wgpu::RenderPipeline>; 2], #[cfg(feature = "material-split")] split: Vec<(Key, wgpu::RenderPipeline)>, #[cfg(feature = "material-split")] split_asked: RefCell<Vec<Key>>,
}

impl Pipelines {
    pub fn new(epoch: u32) -> Self {
        Self {
            engine: [const { None }; ENGINE_SHADERS.len()],
            #[cfg(feature = "forward")]
            engine_ms: [None],
            #[cfg(feature = "forward")]
            depth_resolve: None,
            effects: Vec::new(),
            inbox: Rc::new(RefCell::new(Vec::new())),
            epoch,
            batches: Vec::new(),
            next_batch: 1, #[cfg(feature = "models")] model: [None, None], #[cfg(feature = "msdf")] ui: None, #[cfg(feature = "skinning")] skin: [None, None], #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] blend: [None, None], #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] glass: None,
            in_flight: core::cell::Cell::new(0),
            q: queue::Queue::default(), #[cfg(feature = "models")] model_lite: [None, None], #[cfg(feature = "skinning")] skin_lite: [None, None], #[cfg(feature = "material-split")] split: Vec::new(), #[cfg(feature = "material-split")] split_asked: RefCell::new(Vec::new()),
        }
    }

    pub fn engine(&self, i: usize) -> Option<&wgpu::RenderPipeline> {
        self.engine.get(i).and_then(Option::as_ref)
    }

    /// The 4x variant of engine shader `i`, once built.
    #[cfg(feature = "forward")]
    pub fn engine_ms(&self, i: usize) -> Option<&wgpu::RenderPipeline> {
        let k = MS_ENGINE.iter().position(|&e| e == i)?;
        let p = self.engine_ms.get(k).and_then(Option::as_ref);
        if p.is_none() {
            self.q.want(Key::EngineMs(i));
        }
        p
    }

    #[cfg(feature = "forward")]
    pub fn depth_resolve(&self) -> Option<&wgpu::RenderPipeline> {
        self.depth_resolve.as_ref()
    }

    pub fn effect(&self, e: usize, s: usize) -> Option<&wgpu::RenderPipeline> {
        self.effects.get(e)?.get(s)?.as_ref()
    }

    /// Shader compiles or pipeline builds still running.
    pub fn in_flight(&self) -> bool {
        self.in_flight.get() > 0 || self.q.busy()
    }

    pub fn engine_ready(&self) -> bool {
        self.engine.iter().all(Option::is_some)
    }

    fn template(key: Key, fx: &PostEffects) -> Option<&'static str> { #[cfg(feature = "material-split")] if let Some(t) = Self::split_template(key) { return t; } let key = key.full(); #[cfg(feature = "char-materials")] if let Some(t) = char_material::template(key) { return Some(t); }
        match key {
            Key::Engine(i) => ENGINE_SHADERS.get(i).map(|s| s.source),
            #[cfg(feature = "forward")]
            Key::EngineMs(i) => ENGINE_SHADERS.get(i).map(|s| s.source),
            #[cfg(feature = "forward")]
            Key::DepthResolve => Some(DEPTH_RESOLVE_WGSL), #[cfg(feature = "models")] Key::Model(_) => Some(crate::shaders::FORWARD.source), #[cfg(feature = "msdf")] Key::Ui => ENGINE_SHADERS.get(SPRITE).map(|s| s.source), #[cfg(feature = "skinning")] Key::Skin(_) => Some(crate::shaders::FORWARD.source), #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] Key::Blend(_) => None, #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] Key::Glass => None,
            // Stand-ins were mapped to their full key above.
            #[cfg(feature = "models")]
            Key::ModelLite(_) => None,
            #[cfg(feature = "skinning")]
            Key::SkinLite(_) => None, #[cfg(feature = "material-split")] Key::ModelSplit(..) => None,
            Key::Effect(e, s) => fx.effects.get(e)?.shaders().get(s).map(|sh| sh.source),
        }
    }

    fn label(key: Key, fx: &PostEffects) -> String {
        match key {
            Key::Engine(i) => format!("engine.{}", ENGINE_SHADERS.get(i).map_or("?", |s| s.name)),
            #[cfg(feature = "forward")]
            Key::EngineMs(i) => format!("engine.{}.msaa4", ENGINE_SHADERS.get(i).map_or("?", |s| s.name)),
            #[cfg(feature = "forward")]
            Key::DepthResolve => "engine.depth_resolve".to_string(), #[cfg(feature = "models")] Key::Model(ms) => (if ms { "engine.model.msaa4" } else { "engine.model" }).to_string(), #[cfg(feature = "msdf")] Key::Ui => "engine.sprite.ui".to_string(), #[cfg(feature = "skinning")] Key::Skin(ms) => (if ms { "engine.model_skin.msaa4" } else { "engine.model_skin" }).to_string(), #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] Key::Blend(ms) => (if ms { "engine.model_blend.msaa4" } else { "engine.model_blend" }).to_string(), #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] Key::Glass => "engine.model_glass".to_string(),
            #[cfg(feature = "models")]
            Key::ModelLite(ms) => (if ms { "engine.model.lite.msaa4" } else { "engine.model.lite" }).to_string(),
            #[cfg(feature = "skinning")]
            Key::SkinLite(ms) => (if ms { "engine.model_skin.lite.msaa4" } else { "engine.model_skin.lite" }).to_string(), #[cfg(feature = "material-split")] Key::ModelSplit(see, ms, c, k) => format!("engine.model.{}.{c}.{k}{}", split::pass_name(see), if ms { ".msaa4" } else { "" }),
            Key::Effect(e, s) => match fx.effects.get(e) {
                Some(eff) => format!(
                    "fx.{}.{}",
                    eff.name(),
                    eff.shaders().get(s).map_or("?", |sh| sh.name)
                ),
                None => "fx.?".to_string(),
            },
        }
    }

    fn all_keys(fx: &PostEffects) -> Vec<Key> {
        let mut keys: Vec<Key> = (0..ENGINE_SHADERS.len()).map(Key::Engine).collect(); #[cfg(feature = "msdf")] keys.push(Key::Ui);
        #[cfg(feature = "forward")]
        {
            keys.extend(MS_ENGINE.iter().map(|&i| Key::EngineMs(i)));
            keys.push(Key::DepthResolve); #[cfg(feature = "models")] keys.extend([Key::Model(false), Key::Model(true)]); #[cfg(feature = "skinning")] keys.extend([Key::Skin(false), Key::Skin(true)]); #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] keys.extend([Key::Blend(false), Key::Blend(true)]); #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] keys.push(Key::Glass);
        }
        for (e, eff) in fx.effects.iter().enumerate() {
            for s in 0..eff.shaders().len() {
                keys.push(Key::Effect(e, s));
            }
        }
        keys
    }

    /// Start building every pipeline (engine and compiled-in effects) with
    /// the current slot bodies.
    pub fn launch_all(&mut self, dev: &wgpu::Device, layouts: &Layouts, formats: Formats, world: &World) {
        let fx = world.res::<PostEffects>();
        self.effects = fx.effects.iter().map(|e| vec![None; e.shaders().len()]).collect();
        let slots = world.res::<ShaderSlots>();
        let graph = world.res::<RenderGraph>();
        for key in Self::all_keys(&fx) {
            self.launch_spliced(dev, layouts, formats, &graph, &fx, &slots, key, Bodies::Current, Purpose::Initial { fallback: false }, &mut world.res_mut::<ErrorLog>());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_spliced(
        &self,
        dev: &wgpu::Device,
        layouts: &Layouts,
        formats: Formats,
        graph: &RenderGraph,
        fx: &PostEffects,
        slots: &ShaderSlots,
        key: Key,
        bodies: Bodies,
        purpose: Purpose,
        log: &mut ErrorLog,
    ) -> bool {
        let current = matches!(bodies, Bodies::Current);
        match Self::splice_key(key, fx, slots, bodies) {
            None => false,
            Some(Ok(sp)) => {
                self.launch(dev, layouts, formats, graph, fx, key, purpose, sp, current);
                true
            }
            Some(Err(e)) => {
                log.error(format!("{}: {e}", Self::label(key, fx)));
                false
            }
        }
    }

    /// The source of `key` with `bodies` spliced in and the engine patches
    /// applied (`None`: no template for this key).
    fn splice_key(key: Key, fx: &PostEffects, slots: &ShaderSlots, bodies: Bodies) -> Option<Result<Spliced, splice::SpliceError>> {
        let template = Self::template(key, fx)?;
        let r = splice::splice(template, slots, bodies).map(engine_patches); #[cfg(feature = "material-split")] let r = r.map(|sp| split::shaken(key, sp)); Some(r)
    }

    /// Queue a build. Nothing is created here: [`Pipelines::pump`] starts
    /// it (off the main thread on the web) once something needs it.
    #[allow(clippy::too_many_arguments)]
    fn launch(
        &self,
        _dev: &wgpu::Device,
        _layouts: &Layouts,
        _formats: Formats,
        _graph: &RenderGraph,
        _fx: &PostEffects,
        key: Key,
        purpose: Purpose,
        spliced: Spliced,
        current: bool,
    ) {
        self.q.push(queue::Job { key, purpose, spliced, current });
    }

    /// The tail of every build: post the outcome once the error scope and
    /// the compilation info are in (the pipeline is installed only then).
    fn finish(
        &self,
        key: Key,
        purpose: Purpose,
        spliced: Spliced,
        module: &wgpu::ShaderModule,
        pipeline: wgpu::RenderPipeline,
        err: impl core::future::Future<Output = Option<wgpu::Error>> + 'static,
    ) {
        let info = module.get_compilation_info();
        let inbox = self.inbox.clone();
        let epoch = self.epoch;
        self.in_flight.set(self.in_flight.get() + 1);
        wasm_bindgen_futures::spawn_local(async move {
            let scope_error = err.await.map(|e| e.to_string());
            let info = info.await;
            let messages = info
                .messages
                .into_iter()
                .map(|m| {
                    let sev = match m.message_type {
                        wgpu::CompilationMessageType::Error => "error",
                        wgpu::CompilationMessageType::Warning => "warning",
                        wgpu::CompilationMessageType::Info => "info",
                    };
                    let (l, c, n) = m
                        .location
                        .map_or((0, 0, 0), |loc| (loc.line_number, loc.line_position, loc.length));
                    (sev, m.message, l, c, n)
                })
                .collect();
            inbox.borrow_mut().push(Outcome {
                epoch,
                key,
                purpose,
                pipeline,
                scope_error,
                messages,
                spliced,
            });
        });
    }

    /// Queue the slot rebuilds requested since the last frame. A slot no
    /// pipeline uses is accepted at once.
    pub fn take_slot_requests(&mut self, dev: &wgpu::Device, layouts: &Layouts, formats: Formats, world: &World) {
        let reqs = world.res_mut::<SlotRebuilds>().take_pending();
        if reqs.is_empty() {
            return;
        }
        let fx = world.res::<PostEffects>();
        let graph = world.res::<RenderGraph>();
        let now = evani_core::time::now_ms();
        for req in reqs {
            // A newer body for a slot supersedes a rebuild of it still in
            // flight: pipeline builds finish in any order, so the older one
            // could otherwise land last and put its stale body back (the
            // grey studio sweep's haze, commons-render, 2026-11-11).
            self.supersede(req.slot, world, now);
            let name = world.res::<ShaderSlots>().get(req.slot).map_or("?", |d| d.signature.name);
            let keys: Vec<Key> = Self::all_keys(&fx)
                .into_iter()
                .filter(|k| Self::template(*k, &fx).is_some_and(|t| splice::uses_slot(t, name)))
                .collect(); #[cfg(feature = "material-split")] let keys = self.with_split_keys(keys, &fx, name);
            // A pipeline that uses the slot is not built yet: it will be built
            // with the new body, so take the body now (no second build of it,
            // startfix); the ones already built are rebuilt in the background
            // and swapped in when ready.
            let pending = keys.iter().any(|k| !self.installed(*k));
            if keys.is_empty() || pending {
                let rebuild: Vec<Key> = keys.iter().copied().filter(|k| self.installed(*k)).collect();
                let r = world.res_mut::<ShaderSlots>().set_body(req.slot, &req.body);
                let result = match r {
                    Ok(_) => SlotCompileResult {
                        ok: true,
                        ms: now - req.requested_ms,
                        kept: None,
                        messages: Vec::new(),
                        pipelines_rebuilt: Vec::new(),
                    },
                    Err(e) => rejected(now - req.requested_ms, e.to_string()),
                };
                let ok = result.ok;
                world.res_mut::<SlotRebuilds>().complete(req.handle, result);
                if ok && !rebuild.is_empty() {
                    let slots = world.res::<ShaderSlots>();
                    let mut log = world.res_mut::<ErrorLog>();
                    for k in rebuild {
                        self.launch_spliced(dev, layouts, formats, &graph, &fx, &slots, k, Bodies::Current, Purpose::Initial { fallback: false }, &mut log);
                    }
                }
                continue;
            }
            let id = self.next_batch;
            self.next_batch += 1;
            let mut launched = 0;
            {
                let slots = world.res::<ShaderSlots>();
                let mut log = world.res_mut::<ErrorLog>();
                for &k in &keys {
                    if self.launch_spliced(dev, layouts, formats, &graph, &fx, &slots, k, Bodies::Override(req.slot, &req.body), Purpose::Slot(id), &mut log) {
                        launched += 1;
                    }
                }
            }
            if launched != keys.len() {
                world
                    .res_mut::<SlotRebuilds>()
                    .complete(req.handle, rejected(now - req.requested_ms, "could not splice the body".into()));
                // Outcomes of the jobs that did launch are dropped (no batch).
                continue;
            }
            self.batches.push(Batch {
                id,
                handle: req.handle,
                slot: req.slot,
                slot_name: name,
                body: req.body,
                requested_ms: req.requested_ms,
                expected: keys.len(),
                built: Vec::new(),
                failed: false,
                messages: Vec::new(),
            });
        }
    }

    /// Drop the in-flight rebuild batches of `slot` (a newer body for it has
    /// come): their pipelines are never installed (their outcomes find no
    /// batch) and their requests complete as superseded.
    fn supersede(&mut self, slot: SlotId, world: &World, now: f64) {
        let mut i = 0;
        while i < self.batches.len() {
            if self.batches[i].slot != slot {
                i += 1;
                continue;
            }
            let b = self.batches.swap_remove(i);
            world.res_mut::<SlotRebuilds>().complete(b.handle, superseded(now - b.requested_ms));
        }
    }

    /// Install finished jobs. Call at the start of a frame (the frame
    /// boundary where pipelines may swap). Returns whether any finished.
    pub fn poll(&mut self, dev: &wgpu::Device, layouts: &Layouts, formats: Formats, world: &World) -> bool {
        let done: Vec<Outcome> = core::mem::take(&mut *self.inbox.borrow_mut());
        if done.is_empty() {
            return false;
        }
        self.in_flight
            .set(self.in_flight.get().saturating_sub(done.len() as u32));
        let fx = world.res::<PostEffects>();
        let graph = world.res::<RenderGraph>();
        for o in done {
            if o.epoch != self.epoch {
                continue;
            }
            if o.ok() {
                self.q.remember(o.key, &o.spliced.source, &o.pipeline);
            }
            match o.purpose {
                Purpose::Initial { fallback } => {
                    if o.ok() {
                        if fallback {
                            world.res_mut::<ErrorLog>().note(format!(
                                "{}: using the engine's default slot bodies",
                                Self::label(o.key, &fx)
                            ));
                        }
                        self.install(o.key, o.pipeline);
                        continue;
                    }
                    let label = Self::label(o.key, &fx);
                    let mut log = world.res_mut::<ErrorLog>();
                    log.error(format!("{label}: {}", o.summary()));
                    if !fallback && !o.spliced.spans.is_empty() {
                        let slots = world.res::<ShaderSlots>();
                        self.launch_spliced(dev, layouts, formats, &graph, &fx, &slots, o.key, Bodies::EngineDefaults, Purpose::Initial { fallback: true }, &mut log);
                    }
                }
                Purpose::Slot(id) => {
                    let Some(bi) = self.batches.iter().position(|b| b.id == id) else {
                        continue;
                    };
                    let b = &mut self.batches[bi];
                    let ok = o.ok();
                    if !ok {
                        b.failed = true;
                    }
                    o.slot_messages(Some(b.slot), &mut b.messages);
                    b.built.push((o.key, o.pipeline));
                    if b.built.len() < b.expected {
                        continue;
                    }
                    let b = self.batches.swap_remove(bi);
                    self.finish_batch(b, &fx, world);
                }
            }
        }
        true
    }

    fn finish_batch(&mut self, b: Batch, fx: &PostEffects, world: &World) {
        let ms = evani_core::time::now_ms() - b.requested_ms;
        let set = if b.failed {
            None
        } else {
            Some(world.res_mut::<ShaderSlots>().set_body(b.slot, &b.body))
        };
        let result = match set {
            Some(Ok(_)) => {
                let mut names = Vec::with_capacity(b.built.len());
                for (k, p) in b.built {
                    names.push(Self::label(k, fx));
                    self.install(k, p);
                }
                world
                    .res_mut::<ErrorLog>()
                    .note(format!(
                        "slot {} rebuilt ({} pipelines, {} ms)",
                        b.slot_name,
                        names.len(),
                        evani_core::num::fixed(ms, 0)
                    ));
                SlotCompileResult {
                    ok: true,
                    ms,
                    kept: None,
                    messages: b.messages,
                    pipelines_rebuilt: names,
                }
            }
            Some(Err(e)) => rejected(ms, e.to_string()),
            None => {
                let first = b
                    .messages
                    .iter()
                    .find(|m| m.severity == "error")
                    .map_or_else(|| "invalid".to_string(), |m| {
                        if m.line > 0 {
                            format!("line {}:{}: {}", m.line, m.column, m.text)
                        } else {
                            m.text.clone()
                        }
                    });
                world.res_mut::<ErrorLog>().note(format!(
                    "slot {} rejected, previous pipelines kept: {first}",
                    b.slot_name
                ));
                SlotCompileResult {
                    ok: false,
                    ms,
                    kept: Some("previous"),
                    messages: b.messages,
                    pipelines_rebuilt: Vec::new(),
                }
            }
        };
        world.res_mut::<SlotRebuilds>().complete(b.handle, result);
    }

    fn install(&mut self, key: Key, p: wgpu::RenderPipeline) {
        match key {
            Key::Engine(i) => {
                if let Some(slot) = self.engine.get_mut(i) {
                    *slot = Some(p);
                }
            }
            #[cfg(feature = "forward")]
            Key::EngineMs(i) => {
                if let Some(slot) = MS_ENGINE.iter().position(|&e| e == i).and_then(|k| self.engine_ms.get_mut(k)) {
                    *slot = Some(p);
                }
            }
            #[cfg(feature = "forward")]
            Key::DepthResolve => self.depth_resolve = Some(p), #[cfg(feature = "models")] Key::Model(ms) => if let Some(s) = self.model.get_mut(usize::from(ms)) { *s = Some(p) }, #[cfg(feature = "msdf")] Key::Ui => self.ui = Some(p), #[cfg(feature = "skinning")] Key::Skin(ms) => if let Some(s) = self.skin.get_mut(usize::from(ms)) { *s = Some(p) }, #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] Key::Blend(ms) => if let Some(s) = self.blend.get_mut(usize::from(ms)) { *s = Some(p) }, #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] Key::Glass => self.glass = Some(p),
            #[cfg(feature = "models")]
            Key::ModelLite(ms) => {
                if let Some(s) = self.model_lite.get_mut(usize::from(ms)) {
                    *s = Some(p);
                }
            }
            #[cfg(feature = "skinning")]
            Key::SkinLite(ms) => {
                if let Some(s) = self.skin_lite.get_mut(usize::from(ms)) {
                    *s = Some(p);
                }
            }
            #[cfg(feature = "material-split")] Key::ModelSplit(..) => { self.split.retain(|e| e.0 != key); self.split.push((key, p)); } Key::Effect(e, s) => {
                if let Some(slot) = self.effects.get_mut(e).and_then(|v| v.get_mut(s)) {
                    *slot = Some(p);
                }
            }
        }
    }

    /// Whether `key` has a pipeline installed (any version).
    fn installed(&self, key: Key) -> bool {
        match key {
            Key::Engine(i) => self.engine(i).is_some(),
            #[cfg(feature = "forward")]
            Key::EngineMs(i) => MS_ENGINE.iter().position(|&e| e == i).and_then(|k| self.engine_ms.get(k)).is_some_and(Option::is_some),
            #[cfg(feature = "forward")]
            Key::DepthResolve => self.depth_resolve.is_some(),
            #[cfg(feature = "models")]
            Key::Model(ms) => self.model.get(usize::from(ms)).is_some_and(Option::is_some),
            #[cfg(feature = "models")]
            Key::ModelLite(ms) => self.model_lite.get(usize::from(ms)).is_some_and(Option::is_some),
            #[cfg(feature = "msdf")]
            Key::Ui => self.ui.is_some(),
            #[cfg(feature = "skinning")]
            Key::Skin(ms) => self.skin.get(usize::from(ms)).is_some_and(Option::is_some),
            #[cfg(feature = "skinning")]
            Key::SkinLite(ms) => self.skin_lite.get(usize::from(ms)).is_some_and(Option::is_some),
            #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))]
            Key::Blend(ms) => self.blend.get(usize::from(ms)).is_some_and(Option::is_some),
            #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))]
            Key::Glass => self.glass.is_some(), #[cfg(feature = "material-split")] Key::ModelSplit(..) => self.split.iter().any(|e| e.0 == key),
            Key::Effect(e, s) => self.effects.get(e).and_then(|v| v.get(s)).is_some_and(Option::is_some),
        }
    }
}

/// The engine's own patches over a spliced source (lights, kit code,
/// ambient, contact shadows: whichever are built in).
fn engine_patches(sp: Spliced) -> Spliced { #[cfg(feature = "point-lights")] let sp = super::lights::patch(sp); #[cfg(feature = "kit-material")] let sp = kit_material::patch(sp); #[cfg(feature = "ambient")] let sp = super::ambient::patch(sp); #[cfg(feature = "contact-shadows")] let sp = crate::shaders::contact_patch(sp);
    sp
}

/// The result of a rebuild a newer body for the same slot replaced.
fn superseded(ms: f64) -> SlotCompileResult {
    SlotCompileResult {
        ok: false,
        ms,
        kept: Some("superseded"),
        messages: Vec::new(),
        pipelines_rebuilt: Vec::new(),
    }
}

fn rejected(ms: f64, text: String) -> SlotCompileResult {
    SlotCompileResult {
        ok: false,
        ms,
        kept: Some("previous"),
        messages: vec![SlotMessage {
            severity: "error",
            text,
            line: 0,
            column: 0,
            length: 0,
            source_line: None,
        }],
        pipelines_rebuilt: Vec::new(),
    }
}

fn create_pipeline(
    dev: &wgpu::Device,
    layouts: &Layouts,
    formats: Formats,
    graph: &RenderGraph,
    fx: &PostEffects,
    key: Key,
    module: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    #[cfg(feature = "forward")]
    if key == Key::DepthResolve {
        return depth_resolve_pipeline(dev, layouts, module);
    } #[cfg(all(feature = "models", not(all(feature = "material-recipes-blend", not(feature = "relief")))))] if let Key::Model(ms) = key { return super::models::pipeline(dev, layouts, module, ms, false); } #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] if let Key::Model(ms) = key { return super::models::pipeline(dev, layouts, module, ms, false, 0); } #[cfg(feature = "skinning")] if let Key::Skin(ms) = key { return super::models::skin::pipeline(dev, layouts, module, ms, false); } #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] if let Key::Blend(ms) = key { return super::models::pipeline(dev, layouts, module, ms, false, 1); } #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] if key == Key::Glass { return super::models::pipeline(dev, layouts, module, false, false, 2); } #[cfg(all(feature = "material-split", feature = "skinning"))] if let Key::ModelSplit(split::SKIN, ms, ..) = key { return super::models::skin::pipeline(dev, layouts, module, ms, false); } #[cfg(all(feature = "material-split", not(feature = "material-recipes-blend")))] if let Key::ModelSplit(_, ms, ..) = key { return super::models::pipeline(dev, layouts, module, ms, false); } #[cfg(all(feature = "material-split", feature = "material-recipes-blend"))] if let Key::ModelSplit(see, ms, ..) = key { return super::models::pipeline(dev, layouts, module, ms, false, see); }
    #[cfg(feature = "models")]
    if let Key::ModelLite(ms) = key {
        return super::models::pipeline_lite(dev, layouts, module, ms);
    }
    #[cfg(feature = "skinning")]
    if let Key::SkinLite(ms) = key {
        return super::models::skin::pipeline(dev, layouts, module, ms, true);
    }
    let hdr = wgpu::TextureFormat::Rgba16Float;
    let fmt_of_pass = |pass: &str| {
        graph
            .pass(pass)
            .and_then(|p| p.outputs.first())
            .and_then(|t| graph.target(t))
            .map_or(hdr, |t| target_format(t.kind, formats))
    };
    #[cfg(feature = "forward")]
    let mesh_layouts = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &MESH_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: (INSTANCE_WORDS * 4) as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRS,
        }),
    ];
    let sprite_layouts = [Some(wgpu::VertexBufferLayout {
        array_stride: (SPRITE_WORDS * 4) as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &SPRITE_ATTRS,
    })];
    #[cfg(feature = "forward")]
    let samples = if matches!(key, Key::EngineMs(_)) { MSAA_SAMPLES } else { 1 };
    #[cfg(not(feature = "forward"))]
    let samples = 1;
    let (layout, buffers, format, blend, depth): (_, &[Option<wgpu::VertexBufferLayout>], _, _, _) = match key {
        #[cfg(feature = "forward")]
        Key::Engine(FORWARD) | Key::EngineMs(FORWARD) => (layouts.forward(), &mesh_layouts, hdr, None, true),
        #[cfg(not(feature = "msdf"))] Key::Engine(SPRITE) => (&layouts.pl_a, &sprite_layouts, hdr, Some(ALPHA_BLEND), false), #[cfg(feature = "msdf")] Key::Engine(SPRITE) => (&layouts.msdf.pl_sprite, &sprite_layouts, hdr, Some(ALPHA_BLEND), false), #[cfg(feature = "msdf")] Key::Ui => (&layouts.msdf.pl_sprite, &sprite_layouts, wgpu::TextureFormat::Rgba8UnormSrgb, Some(ALPHA_BLEND), false),
        Key::Engine(TONEMAP) => (&layouts.pl_a, &[], wgpu::TextureFormat::Rgba8UnormSrgb, None, false),
        #[cfg(feature = "forward")]
        Key::EngineMs(_) | Key::DepthResolve => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(feature = "models")] Key::Model(_) => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(feature = "skinning")] Key::Skin(_) => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(all(feature = "material-recipes-blend", not(feature = "relief")))] Key::Blend(_) => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(all(feature = "material-recipes-refract", not(feature = "relief")))] Key::Glass => (&layouts.pl_a, &[], formats.surface, None, false),
        #[cfg(feature = "models")]
        Key::ModelLite(_) => (&layouts.pl_a, &[], formats.surface, None, false),
        #[cfg(feature = "skinning")]
        Key::SkinLite(_) => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(feature = "material-split")] Key::ModelSplit(..) => (&layouts.pl_a, &[], formats.surface, None, false),
        Key::Engine(_) => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(feature = "effect-mesh")] Key::Effect(e, s) if fx.effects.get(e).and_then(|eff| eff.mesh_shader(s)).is_some() => { let sh = fx.effects.get(e).and_then(|eff| eff.shaders().get(s)); let (blend, pass) = sh.map_or((Blend::Replace, ""), |sh| (sh.blend, sh.pass)); (&layouts.pl_adf, &[], fmt_of_pass(pass), blend_state(blend), true) } // a mesh effect: its own layout and a depth target
        Key::Effect(e, s) => {
            let sh = fx.effects.get(e).and_then(|eff| eff.shaders().get(s));
            let (input, depth, blend, pass) =
                sh.map_or((true, false, Blend::Replace, ""), |sh| (sh.input, sh.depth, sh.blend, sh.pass));
            let l = match (input, depth) {
                (true, false) => &layouts.pl_a,
                (false, false) => &layouts.pl_b,
                (true, true) => &layouts.pl_ad,
                (false, true) => &layouts.pl_bd,
            };
            (l, &[], fmt_of_pass(pass), blend_state(blend), false)
        }
    }; #[cfg(feature = "effect-mesh")] let mesh_write = match key { Key::Effect(e, s) => fx.effects.get(e).and_then(|eff| eff.mesh_shader(s)).is_none_or(|m| m.depth_write), _ => true }; // a mesh effect may only test against its depth
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        // Reversed depth (crate::depth): nearer is greater.
        depth_stencil: depth.then(|| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            #[cfg(not(feature = "effect-mesh"))] depth_write_enabled: Some(true), #[cfg(feature = "effect-mesh")] depth_write_enabled: Some(mesh_write),
            depth_compare: Some(crate::depth::COMPARE),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(feature = "forward")]
fn depth_resolve_pipeline(dev: &wgpu::Device, layouts: &Layouts, module: &wgpu::ShaderModule) -> wgpu::RenderPipeline {
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.depth_resolve"),
        layout: Some(&layouts.pl_dms),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(feature = "models")]
impl Pipelines {
    /// The model pipeline (1x or 4x), once built.
    /// While the full pipeline builds, the plain stand-in (once built).
    pub fn model(&self, ms: bool) -> Option<&wgpu::RenderPipeline> {
        if let Some(p) = self.model.get(usize::from(ms)).and_then(Option::as_ref) {
            return Some(p);
        }
        self.q.want(Key::Model(ms));
        self.q.want(Key::ModelLite(ms));
        self.model_lite.get(usize::from(ms)).and_then(Option::as_ref).inspect(|_| self.q.stand_in())
    }
}

#[cfg(feature = "msdf")]
impl Pipelines {
    /// The `ui` pass pipeline (feature `msdf`): the MSDF sprite variant
    /// drawing into the display-referred `ldr` target, once built.
    pub fn ui(&self) -> Option<&wgpu::RenderPipeline> {
        self.ui.as_ref()
    }
}

#[cfg(feature = "skinning")]
impl Pipelines {
    /// The skinned model pipeline (1x or 4x), once built.
    /// While the full pipeline builds, the plain stand-in (once built).
    pub fn skin(&self, ms: bool) -> Option<&wgpu::RenderPipeline> {
        if let Some(p) = self.skin.get(usize::from(ms)).and_then(Option::as_ref) {
            return Some(p);
        }
        self.q.want(Key::Skin(ms));
        self.q.want(Key::SkinLite(ms));
        self.skin_lite.get(usize::from(ms)).and_then(Option::as_ref).inspect(|_| self.q.stand_in())
    }
}
#[cfg(feature = "kit-material")] #[path = "kit_material.rs"] pub mod kit_material; #[cfg(feature = "kit-textures")] #[path = "kit_textures.rs"] pub mod kit_textures; #[cfg(feature = "char-materials")] #[path = "char_material.rs"] mod char_material;

/// The build queue: lazy, off-main-thread pipeline builds (startfix).
/// Declared last: no line above moves.
#[path = "pipeline_queue.rs"]
mod queue;

/// One small shader per material (`material-split`, on with
/// `material-recipes`). Kept at the end so no shipped line moves.
#[cfg(feature = "material-split")]
#[path = "split.rs"]
mod split;

#[cfg(feature = "material-split")]
impl Pipelines {
    /// The pipeline material (`code`, `kind`) draws with in pass `see`
    /// (`split::SKIN` or evani-material-recipes' `OPAQUE`, `BLEND`,
    /// `GLASS`; glass is always one sample): its own once built. Until then
    /// the opaque and skinned passes draw the plain stand-in and the
    /// see-through passes nothing (the material then draws opaque, as
    /// before); either way the key is asked for, so the queue builds it.
    pub fn material(&self, see: u8, ms: bool, code: u8, kind: u8) -> Option<&wgpu::RenderPipeline> {
        use evani_material_recipes::split::{BLEND, GLASS};
        let key = Key::ModelSplit(see, ms && see != GLASS, code, kind);
        if let Some(p) = self.split.iter().find(|e| e.0 == key) {
            // Asked for when built too: when it was last drawn decides
            // whether a live slot edit rebuilds it now or when next drawn.
            self.q.want(key);
            return Some(&p.1);
        }
        let text = split::has_text(key);
        if text {
            self.q.want(key);
        }
        if see == BLEND || see == GLASS {
            return None;
        }
        self.lite(see, ms, text)
    }

    /// The stand-in of pass `see` (asked for, and the page told a stand-in
    /// draws when `waiting`: a material whose own text failed draws it for
    /// good, and the overlay log says why).
    fn lite(&self, see: u8, ms: bool, waiting: bool) -> Option<&wgpu::RenderPipeline> {
        #[cfg(feature = "skinning")]
        if see == split::SKIN {
            self.q.want(Key::SkinLite(ms));
            return self.skin_lite.get(usize::from(ms)).and_then(Option::as_ref).inspect(|_| if waiting { self.q.stand_in() });
        }
        let _ = see;
        self.q.want(Key::ModelLite(ms));
        self.model_lite.get(usize::from(ms)).and_then(Option::as_ref).inspect(|_| if waiting { self.q.stand_in() })
    }

    /// [`Pipelines::material`] for the opaque pass.
    pub fn model_for(&self, ms: bool, code: u8, kind: u8) -> Option<&wgpu::RenderPipeline> {
        self.material(evani_material_recipes::split::OPAQUE, ms, code, kind)
    }

    /// Whether material pipeline `key` is built (any version).
    #[cfg_attr(not(any(feature = "material-recipes-blend", feature = "swatches")), allow(dead_code))] pub fn material_ready(&self, see: u8, ms: bool, code: u8, kind: u8) -> bool {
        let key = Key::ModelSplit(see, ms && see != evani_material_recipes::split::GLASS, code, kind);
        self.split.iter().any(|e| e.0 == key)
    }

    /// The templates with material pipelines: the material keys from their
    /// own text, the stand-ins from the plain forward module, and none for
    /// the all-in-one model, skinned, blend and glass keys, which are never
    /// built (`None`: not a model key, the usual template applies).
    fn split_template(key: Key) -> Option<Option<&'static str>> {
        match key {
            Key::ModelSplit(see, ms, c, k) => Some(split::template(see, ms, c, k)),
            Key::ModelLite(_) => Some(Some(split::lite_template())),
            #[cfg(feature = "skinning")]
            Key::SkinLite(_) => Some(Some(split::lite_template())),
            Key::Model(_) => Some(None),
            #[cfg(feature = "skinning")]
            Key::Skin(_) => Some(None),
            #[cfg(feature = "material-recipes-blend")]
            Key::Blend(_) => Some(None),
            #[cfg(feature = "material-recipes-refract")]
            Key::Glass => Some(None),
            _ => None,
        }
    }

    /// `keys` (the slot's engine pipelines) plus every installed material
    /// pipeline drawn in the last half second whose text uses slot `name`: a
    /// live edit rebuilds those in the background (from the variant cache
    /// for a body seen before) and swaps them in together. An installed one
    /// not drawn lately is dropped instead and rebuilt with the new body
    /// when next drawn, so an edit costs only what is on screen. Queued ones
    /// are spliced again before they start, so they pick the new body up by
    /// themselves.
    fn with_split_keys(&mut self, mut keys: Vec<Key>, fx: &PostEffects, name: &str) -> Vec<Key> {
        let uses = |k: Key| Self::template(k, fx).is_some_and(|t| splice::uses_slot(t, name));
        let mut gone = Vec::new();
        for k in self.split.iter().map(|e| e.0) {
            if keys.contains(&k) || !uses(k) {
                continue;
            }
            if self.q.drawn_lately(k) {
                keys.push(k);
            } else {
                gone.push(k);
            }
        }
        self.split.retain(|e| !gone.contains(&e.0));
        self.split_asked.borrow_mut().retain(|k| !gone.contains(k));
        keys
    }
}

/// The skinned pass of a material pipeline ([`Pipelines::material`]).
#[cfg(all(feature = "material-split", feature = "skinning"))]
pub(super) const SKIN_PASS: u8 = split::SKIN;

/// The swatch balls (feature `swatches`): whether the opaque material
/// pipeline of `(code, kind)` is settled, built or never to be built (its
/// text is not in this build: the stand-in draws it for good). Asks for it
/// when it is neither.
#[cfg(all(feature = "swatches", feature = "material-split"))]
impl Pipelines {
    pub fn material_settled(&self, ms: bool, code: u8, kind: u8) -> bool {
        let see = evani_material_recipes::split::OPAQUE;
        let _ = self.material(see, ms, code, kind);
        self.material_ready(see, ms, code, kind) || !split::has_text(Key::ModelSplit(see, ms, code, kind))
    }
}
