//! Kit material on the GPU (feature `kit-material`, S5-E3; the CPU side and
//! the WGSL contract are in [`crate::kit_material`]).
//!
//! * The kit data is one read-only storage buffer (at least 16 `vec4`s,
//!   grown to the next power of two), bound at group 1 of the forward mesh
//!   pass: its pipeline layout is `[frame, kit]`, or `[frame, kit, empty,
//!   lights]` with point lights. Model draws rebind group 1 for their
//!   materials afterwards, as before.
//! * The kit code reaches the forward module through [`patch`] at every
//!   pipeline build (start-up, slot rebuilds, fallbacks) from a copy kept
//!   here; a change of code relaunches the forward pipelines.
//! * Shadows: while the kit has code of its own, the mesh shadow draws use
//!   `shaders/kit_shadows.wgsl` (with a fragment stage that discards the
//!   cut-outs), layout `[shadow uniform, kit]`. Models keep their own.

use std::cell::RefCell;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::slots::ShaderSlots;

use super::{FORWARD, Formats, Key, Layouts, Pipelines, Purpose};
use crate::graph::RenderGraph;
use crate::kit_material::{self as km, KitMaterial};
use crate::post::PostEffects;
use crate::splice::{Bodies, Spliced};

/// Smallest kit data buffer, in `vec4` records.
const MIN_RECORDS: usize = 16;

thread_local! {
    /// The kit code the forward module is built with.
    static WGSL: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Copy the world's kit code for the next pipeline builds.
pub fn sync_wgsl(world: &World) {
    if let Some(k) = world.try_res::<KitMaterial>() {
        WGSL.with_borrow_mut(|w| k.wgsl().clone_into(w));
    }
}

/// Splice the kit term into a forward-module source (a no-op for every
/// other shader).
pub fn patch(mut sp: Spliced) -> Spliced {
    if let Some(s) = WGSL.with_borrow(|w| km::patch_forward(&sp.source, w)) {
        sp.source = s; #[cfg(feature = "kit-surface")] WGSL.with_borrow(|w| crate::kit_surface::patch(&mut sp.source, w));
    }
    sp
}

/// The kit data layout and the pipeline layouts that carry it.
pub struct KitLayouts {
    pub kit: wgpu::BindGroupLayout,
    pub pl_forward: wgpu::PipelineLayout,
    #[cfg(feature = "shadows")]
    pub pl_shadow: wgpu::PipelineLayout,
}

impl KitLayouts {
    /// `frame`: the forward frame layout; `b`: the shadow uniform layout;
    /// `rest`: the forward groups after the kit's (point lights).
    pub fn new(device: &wgpu::Device, frame: &wgpu::BindGroupLayout, b: &wgpu::BindGroupLayout, rest: &[Option<&wgpu::BindGroupLayout>]) -> Self {
        let kit = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.kit"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 15,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let mut groups = vec![Some(frame), Some(&kit)];
        groups.extend_from_slice(rest);
        let pl_forward = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("evani.layout.forward_kit"),
            bind_group_layouts: &groups,
            immediate_size: 0,
        });
        #[cfg(feature = "shadows")]
        let pl_shadow = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("evani.layout.kit_shadows"),
            bind_group_layouts: &[Some(b), Some(&kit)],
            immediate_size: 0,
        });
        #[cfg(not(feature = "shadows"))]
        let _ = b;
        Self {
            kit,
            pl_forward,
            #[cfg(feature = "shadows")]
            pl_shadow,
        }
    }
}

/// The kit data buffer, its bind group and the cut-out shadow pipeline.
pub struct KitGpu {
    buf: wgpu::Buffer,
    records: usize,
    bg: wgpu::BindGroup,
    wgsl_gen: u64,
    data_gen: u64,
    #[cfg(feature = "shadows")]
    shadow: Option<wgpu::RenderPipeline>,
    /// Code generation the shadow pipeline was last tried with.
    #[cfg(feature = "shadows")]
    shadow_tried: Option<u64>,
    /// Set if that build failed validation.
    #[cfg(feature = "shadows")]
    shadow_failed: std::rc::Rc<std::cell::Cell<bool>>,
}

impl KitGpu {
    /// Call after [`sync_wgsl`] (the pipelines are launched with that code).
    pub fn new(device: &wgpu::Device, layouts: &Layouts, world: &World) -> Self {
        let wgsl_gen = world.try_res::<KitMaterial>().map_or(0, |k| k.generations().0);
        let (buf, bg) = buffer(device, layouts, MIN_RECORDS);
        Self {
            buf,
            records: MIN_RECORDS,
            bg,
            wgsl_gen,
            data_gen: 0,
            #[cfg(feature = "shadows")]
            shadow: None,
            #[cfg(feature = "shadows")]
            shadow_tried: None,
            #[cfg(feature = "shadows")]
            shadow_failed: Default::default(),
        }
    }

    /// Upload changed data and pick up changed code. Returns whether the
    /// code changed (the caller relaunches the forward pipelines).
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layouts: &Layouts, world: &World) -> bool {
        let Some(k) = world.try_res::<KitMaterial>() else {
            return false;
        };
        let (wgsl_gen, data_gen) = k.generations();
        if data_gen != self.data_gen {
            self.data_gen = data_gen;
            let data = k.data();
            if data.len() > km::MAX_DATA {
                world.res_mut::<ErrorLog>().note(format!("kit material: {} data records, only the first {} are used", data.len(), km::MAX_DATA));
            }
            let data = &data[..data.len().min(km::MAX_DATA)];
            if data.len() > self.records {
                self.records = data.len().next_power_of_two();
                (self.buf, self.bg) = buffer(device, layouts, self.records);
            }
            if !data.is_empty() {
                queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(data));
            }
        }
        let changed = wgsl_gen != self.wgsl_gen;
        if changed {
            self.wgsl_gen = wgsl_gen;
            WGSL.with_borrow_mut(|w| k.wgsl().clone_into(w));
        }
        #[cfg(feature = "shadows")]
        self.prepare_shadow(device, layouts, &k, world);
        changed
    }

    #[cfg(feature = "shadows")]
    fn prepare_shadow(&mut self, device: &wgpu::Device, layouts: &Layouts, k: &KitMaterial, world: &World) {
        if self.shadow.is_some() && self.shadow_failed.get() {
            self.shadow = None;
            world.res_mut::<ErrorLog>().error("kit material: the shadow cut-out shader failed to build; shadows ignore the cut-outs");
        }
        if !k.has_code() {
            self.shadow = None;
            return;
        }
        if self.shadow_tried == Some(self.wgsl_gen) {
            return;
        }
        self.shadow_tried = Some(self.wgsl_gen);
        let (pipe, failed) = shadow_pipeline(device, layouts, k.wgsl());
        self.shadow = Some(pipe);
        self.shadow_failed = failed;
    }

    /// Bind the kit data (group 1) for the forward mesh draws. Call after
    /// the lights' bind, which sets group 1 to its empty group.
    pub fn bind(&self, rp: &mut wgpu::RenderPass<'_>) {
        rp.set_bind_group(1, &self.bg, &[]);
    }

    /// Switch the shadow pass's mesh draws to the cut-out pipeline while
    /// the kit has code of its own (otherwise nothing changes).
    #[cfg(feature = "shadows")]
    pub fn bind_shadow(&self, rp: &mut wgpu::RenderPass<'_>) {
        if let Some(p) = &self.shadow {
            rp.set_pipeline(p);
            rp.set_bind_group(1, &self.bg, &[]);
        }
    }
}

fn buffer(device: &wgpu::Device, layouts: &Layouts, records: usize) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("evani.kit_data"),
        size: (records * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("evani.kit_data"),
        layout: &layouts.kit.kit,
        entries: &[wgpu::BindGroupEntry { binding: 15, resource: buf.as_entire_binding() }],
    });
    (buf, bg)
}

/// The cut-out shadow pipeline: the mesh shadow pipeline's streams, depth
/// format and bias, plus the kit's fragment discard.
#[cfg(feature = "shadows")]
fn shadow_pipeline(dev: &wgpu::Device, layouts: &Layouts, wgsl: &str) -> (wgpu::RenderPipeline, std::rc::Rc<std::cell::Cell<bool>>) {
    let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
    let source = km::shadows_source(wgsl);
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.kit_shadows"),
        source: wgpu::ShaderSource::Wgsl(source.as_str().into()),
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &super::MESH_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: (crate::shaders::INSTANCE_WORDS * 4) as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &super::INSTANCE_ATTRS,
        }),
    ];
    let pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.kit_shadows"),
        layout: Some(&layouts.kit.pl_shadow),
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
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    });
    let err = scope.pop();
    let failed = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = failed.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if err.await.is_some() {
            flag.set(true);
        }
    });
    (pipe, failed)
}

impl Pipelines {
    /// Rebuild the forward pipelines (1x and 4x) with the current kit code;
    /// the old ones draw until the new ones are installed.
    pub fn relaunch_kit(&mut self, dev: &wgpu::Device, layouts: &Layouts, formats: Formats, world: &World) {
        let fx = world.res::<PostEffects>();
        let graph = world.res::<RenderGraph>();
        let slots = world.res::<ShaderSlots>();
        let mut log = world.res_mut::<ErrorLog>();
        for key in [Key::Engine(FORWARD), Key::EngineMs(FORWARD)] {
            self.launch_spliced(dev, layouts, formats, &graph, &fx, &slots, key, Bodies::Current, Purpose::Initial { fallback: false }, &mut log);
        }
    }
}
