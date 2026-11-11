//! Kit material on the GPU (feature `kit-material`, S5-E3; the CPU side and
//! the WGSL contract are in [`crate::kit_material`]).
//!
//! * The kit data is one read-only storage buffer (at least 16 `vec4`s,
//!   grown to the next power of two up to 4 MB and in 1 MiB steps above, to
//!   at most `MAX_DATA` = 32 MiB or what the device can bind), bound at group 1 of the forward mesh
//!   pass: its pipeline layout is `[frame, kit]`, or `[frame, kit, empty,
//!   lights]` with point lights. Model draws rebind group 1 for their
//!   materials afterwards, as before.
//! * The kit code reaches the forward module through [`patch`] at every
//!   pipeline build (start-up, slot rebuilds, fallbacks) from a copy kept
//!   here; a change of code relaunches the forward pipelines.
//! * Shadows: while the kit has code of its own, the mesh shadow draws use
//!   `shaders/kit_shadows.wgsl` (with a fragment stage that discards the
//!   cut-outs), layout `[shadow uniform, kit]`. Models keep their own. On the
//!   web it compiles off the main thread (the build queue's warm-up), so no
//!   frame waits for it (61.5 KB in the material viewer: a 1.4-2.2 s frozen
//!   frame when it was built with the blocking call).

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
        sp.source = s; #[cfg(feature = "kit-surface")] WGSL.with_borrow(|w| crate::kit_surface::patch(&mut sp.source, w)); #[cfg(feature = "kit-textures")] super::kit_textures::note_source(sp.source.len());
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
        #[cfg(not(feature = "kit-textures"))] let kit = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        }); #[cfg(feature = "kit-textures")] let kit = super::kit_textures::layout(device);
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
    /// Most records this device can bind ([`km::device_max_records`]).
    cap: usize,
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
    /// A build compiling off the main thread (web): its module and, once the
    /// browser is done, whether it compiled (and its pipeline, kept alive
    /// until the build reuses it).
    #[cfg(feature = "shadows")]
    shadow_warm: Option<ShadowWarm>,
}

/// What the off-thread compile reports: compiled, and the browser's pipeline.
#[cfg(feature = "shadows")]
type WarmDone = std::rc::Rc<std::cell::RefCell<Option<(bool, Option<wasm_bindgen::JsValue>)>>>;

/// The kit shadow pipeline's off-thread compile (`createRenderPipelineAsync`
/// through the build queue's swap, as the other pipelines): until it is in,
/// the shadow pass draws the meshes with the plain shadow pipeline (the
/// previous kit one, after a code change), so no frame waits for it.
#[cfg(feature = "shadows")]
struct ShadowWarm {
    module: wgpu::ShaderModule,
    done: WarmDone,
}

#[cfg(feature = "shadows")]
thread_local! {
    /// Kit shadow builds compiling now (the page's "preparing" count).
    static WARMING: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// Kit shadow builds compiling now.
#[cfg(feature = "shadows")] #[allow(dead_code)] // its one call (pipeline_queue.rs `waiting`) sits after a `//` on that line: DECISIONS "Join (mergeall)"
pub(super) fn warming() -> u32 {
    WARMING.with(core::cell::Cell::get)
}

impl KitGpu {
    /// Call after [`sync_wgsl`] (the pipelines are launched with that code).
    pub fn new(device: &wgpu::Device, layouts: &Layouts, world: &World) -> Self {
        let wgsl_gen = world.try_res::<KitMaterial>().map_or(0, |k| k.generations().0);
        #[cfg(feature = "kit-textures")] super::kit_textures::init(device); let (buf, bg) = buffer(device, layouts, MIN_RECORDS);
        let limits = device.limits();
        Self {
            buf,
            records: MIN_RECORDS,
            cap: km::device_max_records(limits.max_storage_buffer_binding_size, limits.max_buffer_size),
            bg,
            wgsl_gen,
            data_gen: 0,
            #[cfg(feature = "shadows")]
            shadow: None,
            #[cfg(feature = "shadows")]
            shadow_tried: None,
            #[cfg(feature = "shadows")]
            shadow_failed: Default::default(),
            #[cfg(feature = "shadows")]
            shadow_warm: None,
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
            let plan = km::plan_upload(data.len(), self.records, self.cap);
            if let Some(over) = plan.over {
                world.res_mut::<ErrorLog>().error(format!("kit material: {over}; only the first {} are used", plan.used));
            }
            let data = &data[..plan.used];
            if plan.alloc != self.records {
                self.records = plan.alloc;
                (self.buf, self.bg) = buffer(device, layouts, self.records);
            }
            if !data.is_empty() {
                queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(data));
            }
        }
        let changed = wgsl_gen != self.wgsl_gen; #[cfg(feature = "kit-textures")] if super::kit_textures::prepare(device, queue, world) { self.bg = super::kit_textures::bind_group(device, &layouts.kit.kit, &self.buf); }
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
        if let Some(w) = &self.shadow_warm {
            let Some((ok, keep)) = w.done.borrow_mut().take() else {
                return;
            };
            WARMING.with(|c| c.set(c.get().saturating_sub(1)));
            let module = self.shadow_warm.take().map(|w| w.module);
            if let (true, Some(module)) = (ok, module) {
                // The browser answers this call from the compile it just did.
                let (pipe, failed) = shadow_build(device, layouts, &module);
                self.shadow = Some(pipe);
                self.shadow_failed = failed;
            } else {
                self.shadow = None;
                world.res_mut::<ErrorLog>().error("kit material: the shadow cut-out shader failed to build; shadows ignore the cut-outs");
            }
            drop(keep);
        }
        if self.shadow_tried == Some(self.wgsl_gen) {
            return;
        }
        self.shadow_tried = Some(self.wgsl_gen);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let source = km::shadows_source(k.wgsl()); #[cfg(feature = "material-split")] let source = evani_material_recipes::split::shake(&source, &["vs_main", "fs_main"]);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("evani.kit_shadows"),
            source: wgpu::ShaderSource::Wgsl(source.as_str().into()),
        });
        let caught = super::queue::warm_call(device, || shadow_descriptor_build(device, layouts, &module));
        let err = scope.pop();
        match caught {
            super::queue::Caught::Built(pipe) => {
                self.shadow = Some(pipe);
                self.shadow_failed = flag_on_error(err);
            }
            super::queue::Caught::Warming(promise) => {
                let done: WarmDone = Default::default();
                let slot = done.clone();
                WARMING.with(|c| c.set(c.get() + 1));
                wasm_bindgen_futures::spawn_local(async move {
                    let bad = err.await.is_some();
                    let r = wasm_bindgen_futures::JsFuture::from(promise).await;
                    let ok = !bad && r.is_ok();
                    *slot.borrow_mut() = Some((ok, r.ok()));
                });
                self.shadow_warm = Some(ShadowWarm { module, done });
            }
        }
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
    #[cfg(not(feature = "kit-textures"))] let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("evani.kit_data"),
        layout: &layouts.kit.kit,
        entries: &[wgpu::BindGroupEntry { binding: 15, resource: buf.as_entire_binding() }],
    }); #[cfg(feature = "kit-textures")] let bg = super::kit_textures::bind_group(device, &layouts.kit.kit, &buf);
    (buf, bg)
}

/// The cut-out shadow pipeline from `module` (the build after the warm-up),
/// with a flag set if it fails validation.
#[cfg(feature = "shadows")]
fn shadow_build(dev: &wgpu::Device, layouts: &Layouts, module: &wgpu::ShaderModule) -> (wgpu::RenderPipeline, std::rc::Rc<std::cell::Cell<bool>>) {
    let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipe = shadow_descriptor_build(dev, layouts, module);
    (pipe, flag_on_error(scope.pop()))
}

/// A flag set once `err` turns out to be an error.
#[cfg(feature = "shadows")]
fn flag_on_error(err: impl core::future::Future<Output = Option<wgpu::Error>> + 'static) -> std::rc::Rc<std::cell::Cell<bool>> {
    let failed = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = failed.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if err.await.is_some() {
            flag.set(true);
        }
    });
    failed
}

/// The cut-out shadow pipeline: the mesh shadow pipeline's streams, depth
/// format and bias, plus the kit's fragment discard.
#[cfg(feature = "shadows")]
fn shadow_descriptor_build(dev: &wgpu::Device, layouts: &Layouts, module: &wgpu::ShaderModule) -> wgpu::RenderPipeline {
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
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.kit_shadows"),
        layout: Some(&layouts.kit.pl_shadow),
        vertex: wgpu::VertexState {
            module,
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
            module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        multiview_mask: None,
        cache: None,
    })
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
