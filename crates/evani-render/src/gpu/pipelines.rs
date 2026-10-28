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
    Effect(usize, usize), #[cfg(feature = "models")] Model(bool), #[cfg(feature = "msdf")] Ui, #[cfg(feature = "skinning")] Skin(bool),
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
    pub pl_bd: wgpu::PipelineLayout, #[cfg(feature = "msdf")] pub msdf: super::msdf::MsdfLayouts, #[cfg(feature = "point-lights")] pub lights: super::lights::LightLayouts, #[cfg(feature = "kit-material")] pub kit: kit_material::KitLayouts,
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
        let pl_bd = pl(&bd); #[cfg(feature = "msdf")] let msdf = super::msdf::MsdfLayouts::new(device, &a);
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
            pl_bd, #[cfg(feature = "msdf")] msdf, #[cfg(feature = "point-lights")] lights, #[cfg(feature = "kit-material")] kit,
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
        TargetKind::Buffer => wgpu::TextureFormat::R32Uint,
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
    next_batch: u64, #[cfg(feature = "models")] model: [Option<wgpu::RenderPipeline>; 2], #[cfg(feature = "msdf")] ui: Option<wgpu::RenderPipeline>, #[cfg(feature = "skinning")] skin: [Option<wgpu::RenderPipeline>; 2],
    /// Pipeline jobs launched and not finished yet.
    in_flight: core::cell::Cell<u32>,
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
            next_batch: 1, #[cfg(feature = "models")] model: [None, None], #[cfg(feature = "msdf")] ui: None, #[cfg(feature = "skinning")] skin: [None, None],
            in_flight: core::cell::Cell::new(0),
        }
    }

    pub fn engine(&self, i: usize) -> Option<&wgpu::RenderPipeline> {
        self.engine.get(i).and_then(Option::as_ref)
    }

    /// The 4x variant of engine shader `i`, once built.
    #[cfg(feature = "forward")]
    pub fn engine_ms(&self, i: usize) -> Option<&wgpu::RenderPipeline> {
        let k = MS_ENGINE.iter().position(|&e| e == i)?;
        self.engine_ms.get(k).and_then(Option::as_ref)
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
        self.in_flight.get() > 0
    }

    pub fn engine_ready(&self) -> bool {
        self.engine.iter().all(Option::is_some)
    }

    fn template(key: Key, fx: &PostEffects) -> Option<&'static str> { #[cfg(feature = "char-materials")] if let Some(t) = char_material::template(key) { return Some(t); }
        match key {
            Key::Engine(i) => ENGINE_SHADERS.get(i).map(|s| s.source),
            #[cfg(feature = "forward")]
            Key::EngineMs(i) => ENGINE_SHADERS.get(i).map(|s| s.source),
            #[cfg(feature = "forward")]
            Key::DepthResolve => Some(DEPTH_RESOLVE_WGSL), #[cfg(feature = "models")] Key::Model(_) => Some(crate::shaders::FORWARD.source), #[cfg(feature = "msdf")] Key::Ui => ENGINE_SHADERS.get(SPRITE).map(|s| s.source), #[cfg(feature = "skinning")] Key::Skin(_) => Some(crate::shaders::FORWARD.source),
            Key::Effect(e, s) => fx.effects.get(e)?.shaders().get(s).map(|sh| sh.source),
        }
    }

    fn label(key: Key, fx: &PostEffects) -> String {
        match key {
            Key::Engine(i) => format!("engine.{}", ENGINE_SHADERS.get(i).map_or("?", |s| s.name)),
            #[cfg(feature = "forward")]
            Key::EngineMs(i) => format!("engine.{}.msaa4", ENGINE_SHADERS.get(i).map_or("?", |s| s.name)),
            #[cfg(feature = "forward")]
            Key::DepthResolve => "engine.depth_resolve".to_string(), #[cfg(feature = "models")] Key::Model(ms) => (if ms { "engine.model.msaa4" } else { "engine.model" }).to_string(), #[cfg(feature = "msdf")] Key::Ui => "engine.sprite.ui".to_string(), #[cfg(feature = "skinning")] Key::Skin(ms) => (if ms { "engine.model_skin.msaa4" } else { "engine.model_skin" }).to_string(),
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
            keys.push(Key::DepthResolve); #[cfg(feature = "models")] keys.extend([Key::Model(false), Key::Model(true)]); #[cfg(feature = "skinning")] keys.extend([Key::Skin(false), Key::Skin(true)]);
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
        let Some(template) = Self::template(key, fx) else {
            return false;
        };
        match splice::splice(template, slots, bodies) {
            Ok(sp) => { #[cfg(feature = "point-lights")] let sp = super::lights::patch(sp); #[cfg(feature = "kit-material")] let sp = kit_material::patch(sp); #[cfg(feature = "ambient")] let sp = super::ambient::patch(sp);
                self.launch(dev, layouts, formats, graph, fx, key, purpose, sp);
                true
            }
            Err(e) => {
                log.error(format!("{}: {e}", Self::label(key, fx)));
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn launch(
        &self,
        dev: &wgpu::Device,
        layouts: &Layouts,
        formats: Formats,
        graph: &RenderGraph,
        fx: &PostEffects,
        key: Key,
        purpose: Purpose,
        spliced: Spliced,
    ) {
        // Strictly LIFO: push, create, pop (before any await).
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(spliced.source.as_str().into()),
        });
        let pipeline = create_pipeline(dev, layouts, formats, graph, fx, key, &module);
        let err = scope.pop();
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
            let name = world.res::<ShaderSlots>().get(req.slot).map_or("?", |d| d.signature.name);
            let keys: Vec<Key> = Self::all_keys(&fx)
                .into_iter()
                .filter(|k| Self::template(*k, &fx).is_some_and(|t| splice::uses_slot(t, name)))
                .collect();
            if keys.is_empty() {
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
                world.res_mut::<SlotRebuilds>().complete(req.handle, result);
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
            Key::DepthResolve => self.depth_resolve = Some(p), #[cfg(feature = "models")] Key::Model(ms) => if let Some(s) = self.model.get_mut(usize::from(ms)) { *s = Some(p) }, #[cfg(feature = "msdf")] Key::Ui => self.ui = Some(p), #[cfg(feature = "skinning")] Key::Skin(ms) => if let Some(s) = self.skin.get_mut(usize::from(ms)) { *s = Some(p) },
            Key::Effect(e, s) => {
                if let Some(slot) = self.effects.get_mut(e).and_then(|v| v.get_mut(s)) {
                    *slot = Some(p);
                }
            }
        }
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
    } #[cfg(feature = "models")] if let Key::Model(ms) = key { return super::models::pipeline(dev, layouts, module, ms); } #[cfg(feature = "skinning")] if let Key::Skin(ms) = key { return super::models::skin::pipeline(dev, layouts, module, ms); }
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
        Key::EngineMs(_) | Key::DepthResolve => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(feature = "models")] Key::Model(_) => (&layouts.pl_a, &[], formats.surface, None, false), #[cfg(feature = "skinning")] Key::Skin(_) => (&layouts.pl_a, &[], formats.surface, None, false),
        Key::Engine(_) => (&layouts.pl_a, &[], formats.surface, None, false),
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
    };
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
            depth_write_enabled: Some(true),
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
    pub fn model(&self, ms: bool) -> Option<&wgpu::RenderPipeline> {
        self.model.get(usize::from(ms)).and_then(Option::as_ref)
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
    pub fn skin(&self, ms: bool) -> Option<&wgpu::RenderPipeline> {
        self.skin.get(usize::from(ms)).and_then(Option::as_ref)
    }
}
#[cfg(feature = "kit-material")] #[path = "kit_material.rs"] pub mod kit_material; #[cfg(feature = "char-materials")] #[path = "char_material.rs"] mod char_material;
