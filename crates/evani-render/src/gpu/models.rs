//! GPU side of textured models (feature `models`, S3-3; the CPU side is
//! `crate::model`, formats and the budget `crate::texture`).
//!
//! * The device asks for ASTC, BC and ETC2 sampling whenever the adapter
//!   has them, so a model's KTX2 set can be chosen per device.
//! * Files and textures are fetched in spawned tasks that post to an inbox
//!   the frame drains. Relative texture URIs resolve against the model's
//!   folder. A model draws only once its file and every texture it samples
//!   are on the GPU.
//! * KTX2 levels go in with `write_texture`; PNG and JPEG are decoded by
//!   the browser (`createImageBitmap`, no colour conversion, no
//!   premultiplication) with a mip chain made by the browser's resize, and
//!   copied with `copy_external_image_to_texture`.
//! * The tier's `max_texture_memory` holds every model's textures: over it,
//!   the largest textures drop their top mip levels (`drop_top_mips`).
//! * One instance buffer a frame (grown as needed); one draw per part per
//!   view in the forward pass, and per part in the shadow pass.
//! * The sun cones (`model::sun`) are baked here, lazily: only on frames
//!   without a shadow map (the only ones that read them), one model at a
//!   time, `sun::FRAME_UNITS` a frame, then written over the model's vertex
//!   buffers. A model that finishes loading on such a frame waits (still
//!   `Loading`) until its cones are in, so it never shows unbaked; one
//!   loaded with a shadow map shows at once and keeps its parts on the CPU
//!   in case the tier drops.

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::tiers::Tiers;
use js_sys::Reflect;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::pipelines::{Layouts, MSAA_SAMPLES, Pipelines};
use crate::model::handle::{self, Manifest};
use crate::model::stream::{self, Backend, web::Shim};
use crate::model::{self, INSTANCE_WORDS, MATERIAL_WORDS, ModelId, ModelState, Models, Part, Prepared, TexSource, VERTEX_BYTES};
use crate::world::RenderWorld;
use evani_core::assets::AssetManifest;
use crate::texture::{self, DeviceTex, TexSize}; #[cfg(feature = "level-groups")] mod level;

const VERTEX_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Unorm16x4, 1 => Snorm8x4, 2 => Snorm8x4, 3 => Unorm16x2];
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
    7 => Float32x4,
    8 => Float32x4,
    9 => Float32x4,
    10 => Float32x4,
];
#[cfg(feature = "shadows")]
const SHADOW_VERTEX_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Unorm16x4];
#[cfg(feature = "shadows")]
const SHADOW_INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
    7 => Float32x4,
    8 => Float32x4,
];

/// The device the renderer runs on: the same request as without models,
/// plus every block-compression family the adapter can sample.
pub async fn request_device(adapter: &wgpu::Adapter, timestamps: bool) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let mut features = if timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
    features |= adapter.features()
        & (wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC | wgpu::Features::TEXTURE_COMPRESSION_ETC2);
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("evani"),
            required_features: features,
            required_limits: adapter.limits(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: Default::default(),
        })
        .await
        .map_err(|e| format!("device: {e}"))
}

/// The material group (group 1 of model.wgsl): uniform, base, normal and
/// emissive maps, sampler. Layouts with the same entries are
/// interchangeable, so the pipeline and the bind groups each make one.
fn material_layout(dev: &wgpu::Device) -> wgpu::BindGroupLayout {
    let tex = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("evani.layout.model"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            tex(1),
            tex(2),
            tex(3),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            }, #[cfg(feature = "relief")] tex(5), #[cfg(feature = "relief")] super::models_relief::uniform(6), #[cfg(feature = "relief")] super::models_relief::uniform(7), #[cfg(all(feature = "look-surface", not(feature = "relief")))] tex(5), #[cfg(all(feature = "look-surface", not(feature = "relief")))] tex(6), #[cfg(all(feature = "look-surface", not(feature = "relief")))] super::models_surface::uniform(8), #[cfg(all(feature = "material-layers", not(feature = "relief")))] tex(9), #[cfg(all(feature = "material-layers", not(feature = "relief")))] super::models_surface::uniform(10), #[cfg(all(feature = "material-layers", not(feature = "relief")))] super::models_surface::uniform(11),
        ],
    })
}

/// The model pipeline (`model_vs` / `model_fs` of the forward module), 1x
/// or 4x to match the forward pass. No culling: the shader discards back
/// faces of single-sided parts.
pub fn pipeline(dev: &wgpu::Device, layouts: &Layouts, module: &wgpu::ShaderModule, ms: bool) -> wgpu::RenderPipeline {
    #[cfg(all(feature = "shadows", not(feature = "ambient")))]
    let frame = &layouts.fwd; #[cfg(feature = "ambient")] let frame = &layouts.fwd_amb;
    #[cfg(not(feature = "shadows"))]
    let frame = &layouts.b;
    let mat = material_layout(dev);
    let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(frame), Some(&mat), #[cfg(feature = "point-lights")] Some(&layouts.lights.empty), #[cfg(feature = "point-lights")] Some(&layouts.lights.lights)],
        immediate_size: 0,
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: (INSTANCE_WORDS * 4) as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRS,
        }),
    ];
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("model_vs"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(crate::depth::COMPARE),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: if ms { MSAA_SAMPLES } else { 1 },
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module, #[cfg(not(any(feature = "relief", feature = "look-surface")))]
            entry_point: Some("model_fs"), #[cfg(feature = "relief")] entry_point: Some("model_relief_fs"), #[cfg(all(feature = "look-surface", not(feature = "relief")))] entry_point: Some(LIT_FS),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba16Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Model depth into the shadow map (static WGSL, built directly).
#[cfg(feature = "shadows")]
fn shadow_pipeline(dev: &wgpu::Device, layouts: &Layouts) -> wgpu::RenderPipeline {
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("evani.model_shadows"),
        source: wgpu::ShaderSource::Wgsl(crate::shaders::MODEL_SHADOWS.into()),
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SHADOW_VERTEX_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: (INSTANCE_WORDS * 4) as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &SHADOW_INSTANCE_ATTRS,
        }),
    ];
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.model_shadows"),
        layout: Some(&layouts.pl_b),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("model_shadow_vs"),
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

/// A texture as it arrived.
enum TexData {
    Ktx2(Vec<u8>),
    /// Mip chain, level 0 first.
    Bitmaps(Vec<web_sys::ImageBitmap>),
    /// No source this device can use: the default texture.
    Missing,
}

enum Msg {
    Glb(ModelId, Result<Vec<u8>, String>),
    Tex(ModelId, usize, Result<TexData, String>),
    /// `models/stream.json` read (`Ok(None)`: the game has none).
    Manifest(Result<Option<AssetManifest>, String>),
    /// One boot-group file arrived (or failed: the reason).
    Boot(Result<(), String>),
}

/// Where a texture comes from, owned for the fetch task (a URL goes
/// through the manifest's cache when it lists it).
enum Source {
    Url(String, bool, Option<Rc<AssetManifest>>),
    Bytes(Vec<u8>),
    Missing,
}

/// A model waiting for its textures.
struct Loading {
    id: ModelId,
    url: String,
    prepared: Prepared,
    tex: Vec<Option<TexData>>,
}

struct GpuPart {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    count: u32, #[cfg(feature = "skinning")] sb: Option<wgpu::Buffer>,
    /// The part without its vertices (what the instance words need).
    part: Part,
}

struct GpuModel {
    parts: Vec<GpuPart>,
    materials: Vec<wgpu::BindGroup>,
    _textures: Vec<wgpu::Texture>,
    _uniforms: Vec<wgpu::Buffer>,
    /// The parts' bytes (as `parts`, in order) until the cones are baked.
    cones: model::sun::Lazy,
    /// Drawn and `Ready`; false while it waits for its cones.
    shown: bool, #[cfg(feature = "skinning")] skin: skin::ModelSkin,
}

/// Instances `first..first + count` of the instance buffer draw part
/// `part` of model `model`.
struct Run {
    model: usize,
    part: usize,
    first: u32,
    count: u32,
}

/// Owned by `Gpu`; `prepare` once a frame before the passes.
pub(super) struct ModelGpu {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 1x1 white (sRGB) and flat normal (linear) stand-ins.
    defaults: [(wgpu::Texture, wgpu::TextureView); 2],
    #[cfg(feature = "shadows")]
    shadow: wgpu::RenderPipeline,
    dev_tex: DeviceTex,
    inbox: Rc<RefCell<Vec<Msg>>>,
    loading: Vec<Loading>,
    ready: Vec<Option<GpuModel>>,
    /// Texture bytes on the GPU (all models).
    resident: u64,
    synced: bool,
    instances: wgpu::Buffer,
    staging: Vec<f32>,
    runs: Vec<Run>, #[cfg(feature = "relief")] relief: wgpu::Buffer, #[cfg(feature = "skinning")] skin: skin::SkinGpu, #[cfg(feature = "view-model-models")] vm_runs: Vec<Run>, #[cfg(all(feature = "material-layers", not(feature = "relief")))] layers: super::models_layers::LayersGpu,
    /// This frame's before/after split (S3-14), copied from `Models.split`.
    #[cfg(feature = "hooks")]
    split: Option<crate::model::Split>,
}

fn pixel(dev: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, rgba: [u8; 4]) -> (wgpu::Texture, wgpu::TextureView) {
    let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
    let t = dev.create_texture(&wgpu::TextureDescriptor {
        label: Some("evani.model.default"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        t.as_image_copy(),
        &rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
        size,
    );
    let v = t.create_view(&wgpu::TextureViewDescriptor::default());
    (t, v)
}

fn instance_buffer(dev: &wgpu::Device, bytes: u64) -> wgpu::Buffer {
    dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("evani.model.instances"),
        size: bytes,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A browser error as text.
fn js(e: JsValue) -> String {
    e.as_string()
        .or_else(|| Reflect::get(&e, &JsValue::from_str("message")).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| "a browser error".to_string())
}

/// `models/stream.json`, if the game has one (a missing file, or a page
/// the server sends instead, means none).
async fn read_manifest() -> Result<Option<AssetManifest>, String> {
    let Ok(b) = Shim.fetch(stream::MANIFEST_URL, None, false).await else {
        return Ok(None);
    };
    if b.first() != Some(&b'{') {
        return Ok(None);
    }
    let text = core::str::from_utf8(&b).map_err(|_| "not UTF-8".to_string())?;
    stream::manifest_from_text(text).map(Some)
}

/// `window.createImageBitmap(source, options)`.
async fn image_bitmap(source: &JsValue, opts: &[(&str, JsValue)]) -> Result<web_sys::ImageBitmap, String> {
    let window: JsValue = web_sys::window().ok_or("no window")?.into();
    let o = js_sys::Object::new();
    for (k, v) in opts {
        Reflect::set(&o, &JsValue::from_str(k), v).map_err(js)?;
    }
    let f: js_sys::Function = Reflect::get(&window, &JsValue::from_str("createImageBitmap")).map_err(js)?.dyn_into().map_err(js)?;
    let p: js_sys::Promise = f.call2(&window, source, &o).map_err(js)?.dyn_into().map_err(js)?;
    JsFuture::from(p).await.map_err(js)?.dyn_into().map_err(js)
}

/// Decode a PNG or JPEG into a full mip chain (each level resized by the
/// browser from level 0). Raw texel values: no colour conversion, no
/// premultiplied alpha, so normal maps stay data.
async fn decode(bytes: Vec<u8>) -> Result<Vec<web_sys::ImageBitmap>, String> {
    let window: JsValue = web_sys::window().ok_or("no window")?.into();
    let arr = js_sys::Uint8Array::from(bytes.as_slice());
    let ctor: js_sys::Function = Reflect::get(&window, &JsValue::from_str("Blob")).map_err(js)?.dyn_into().map_err(js)?;
    // `Reflect.construct(Blob, args)`: `args` is the argument list, so this
    // is `new Blob([arr])` (one part, the bytes). Never drop the outer
    // array: `new Blob(arr)` takes each byte as a part and stringifies it,
    // and every embedded or PNG/JPEG picture decodes as garbage. Guarded in
    // a real browser by evani-convert's `gate_still_runs_after_matching`
    // (its master's pictures are embedded).
    let blob = Reflect::construct(&ctor, &js_sys::Array::of1(&js_sys::Array::of1(&arr))).map_err(js)?;
    let raw = [("premultiplyAlpha", JsValue::from_str("none")), ("colorSpaceConversion", JsValue::from_str("none"))];
    let top = image_bitmap(&blob, &raw).await?;
    let (w, h) = (top.width(), top.height());
    if w == 0 || h == 0 {
        return Err("an empty image".to_string());
    }
    let src: JsValue = top.clone().into();
    let mut out = vec![top];
    for i in 1..texture::full_levels(w, h) {
        let opts = [
            ("premultiplyAlpha", JsValue::from_str("none")),
            ("colorSpaceConversion", JsValue::from_str("none")),
            ("resizeWidth", JsValue::from((w >> i).max(1))),
            ("resizeHeight", JsValue::from((h >> i).max(1))),
            ("resizeQuality", JsValue::from_str("high")),
        ];
        out.push(image_bitmap(&src, &opts).await?);
    }
    Ok(out)
}

async fn load_texture(src: Source) -> Result<TexData, String> {
    match src {
        Source::Url(url, true, m) => handle::fetch_file(&Shim, m, &url).await.map(TexData::Ktx2),
        Source::Url(url, false, m) => decode(handle::fetch_file(&Shim, m, &url).await?).await.map(TexData::Bitmaps),
        Source::Bytes(b) => decode(b).await.map(TexData::Bitmaps),
        Source::Missing => Ok(TexData::Missing),
    }
}

/// The wgpu format of a KTX2 Vulkan format, in the colour space the
/// material needs (the blocks are the same in both).
fn ktx_format(vk: u32, srgb: bool) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as F;
    let astc = |channel| F::Astc { block: wgpu::AstcBlock::B4x4, channel };
    let (u, s) = match vk {
        37 | 43 => (F::Rgba8Unorm, F::Rgba8UnormSrgb),
        131..=134 => (F::Bc1RgbaUnorm, F::Bc1RgbaUnormSrgb),
        137 | 138 => (F::Bc3RgbaUnorm, F::Bc3RgbaUnormSrgb),
        145 | 146 => (F::Bc7RgbaUnorm, F::Bc7RgbaUnormSrgb),
        147 | 148 => (F::Etc2Rgb8Unorm, F::Etc2Rgb8UnormSrgb),
        149 | 150 => (F::Etc2Rgb8A1Unorm, F::Etc2Rgb8A1UnormSrgb),
        151 | 152 => (F::Etc2Rgba8Unorm, F::Etc2Rgba8UnormSrgb),
        157 | 158 => (astc(wgpu::AstcChannel::Unorm), astc(wgpu::AstcChannel::UnormSrgb)),
        _ => return None,
    };
    Some(if srgb { s } else { u })
}

/// A texture ready to upload: its format, size and the levels it keeps.
struct Upload<'a> {
    format: wgpu::TextureFormat,
    size: TexSize,
    data: &'a TexData,
    ktx: Option<texture::Ktx2>,
}

impl ModelGpu {
    pub fn new(dev: &wgpu::Device, queue: &wgpu::Queue, layouts: &Layouts) -> Self {
        #[cfg(not(feature = "shadows"))]
        let _ = layouts;
        let f = dev.features();
        Self {
            layout: material_layout(dev),
            sampler: dev.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("evani.model.sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                anisotropy_clamp: 4,
                ..Default::default()
            }),
            defaults: [
                pixel(dev, queue, wgpu::TextureFormat::Rgba8UnormSrgb, [255; 4]),
                pixel(dev, queue, wgpu::TextureFormat::Rgba8Unorm, [128, 128, 255, 255]),
            ],
            #[cfg(feature = "shadows")]
            shadow: shadow_pipeline(dev, layouts),
            dev_tex: DeviceTex {
                astc: f.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC),
                bc: f.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
                etc2: f.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2),
            },
            inbox: Rc::new(RefCell::new(Vec::new())),
            loading: Vec::new(),
            ready: Vec::new(),
            resident: 0,
            synced: false,
            instances: instance_buffer(dev, (INSTANCE_WORDS * 4 * 64) as u64),
            staging: Vec::new(),
            runs: Vec::new(), #[cfg(feature = "relief")] relief: super::models_relief::frame_buffer(dev), #[cfg(feature = "skinning")] skin: skin::SkinGpu::new(dev, layouts), #[cfg(feature = "view-model-models")] vm_runs: Vec::new(), #[cfg(all(feature = "material-layers", not(feature = "relief")))] layers: super::models_layers::LayersGpu::new(dev, queue),
            #[cfg(feature = "hooks")]
            split: None,
        }
    }

    /// Download `list` one file at a time: the boot group (`low` false; each
    /// arrival is counted, a failure reported) or a later group in the
    /// background (`low` true; best effort, since a failed prefetch is
    /// fetched again, and reported, when a model asks for it). One future
    /// type for both keeps the wasm small.
    fn fetch_group(&self, list: Vec<evani_core::assets::AssetEntry>, low: bool) {
        if list.is_empty() {
            return;
        }
        let inbox = self.inbox.clone();
        wasm_bindgen_futures::spawn_local(async move {
            for e in list {
                let r = stream::get(&Shim, &e, low).await.map(|_| ());
                if !low {
                    inbox.borrow_mut().push(Msg::Boot(r));
                }
            }
        });
    }

    fn post(&self, fut: impl core::future::Future<Output = Msg> + 'static) {
        let inbox = self.inbox.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let m = fut.await;
            inbox.borrow_mut().push(m);
        });
    }

    /// Fetch, upload, bake and pack this frame's model instances. `settled`: the
    /// tier is chosen (after the first-run warm-up), so handles may fetch.
    /// `cones_read`: this frame has no shadow map, so the shader reads the
    /// baked sun cones.
    pub fn prepare(&mut self, dev: &wgpu::Device, queue: &wgpu::Queue, world: &World, rw: &RenderWorld, settled: bool, cones_read: bool) {
        self.runs.clear(); #[cfg(feature = "view-model-models")] self.vm_runs.clear();
        self.staging.clear(); #[cfg(feature = "relief")] super::models_relief::write_frame(queue, &self.relief, world); #[cfg(all(feature = "material-layers", not(feature = "relief")))] self.layers.write_frame(queue, world);
        let Some(mut models) = world.try_res_mut::<Models>() else {
            return;
        };
        #[cfg(feature = "hooks")]
        {
            self.split = models.split;
        }
        if !self.synced {
            // A new device: everything is uploaded again.
            self.synced = true;
            models.reload_all();
        }
        // Handles: read the manifest once (only games that use handles ask),
        // then follow the settled tier: its boot group at once, the handles'
        // own files, then the later groups in the background, one by one.
        if models.stream.manifest == Manifest::Unknown && models.has_handles() {
            models.stream.manifest = Manifest::Asked;
            self.post(async { Msg::Manifest(read_manifest().await) });
        }
        if settled && matches!(models.stream.manifest, Manifest::Ready(_) | Manifest::Missing) {
            let tier = world.res::<Tiers>().active().id.clone();
            models.sync_tier(&tier);
            let boot = models.stream.boot_plan(&tier);
            self.fetch_group(boot, false);
            let states = models.states();
            let later = models.stream.later_plan(&tier, &states);
            self.fetch_group(later, true); #[cfg(feature = "level-groups")] self.fetch_group(models.stream.take_level_prefetch(&tier, self.dev_tex), true);
        }
        let manifest = models.stream.shared();
        for id in core::mem::take(&mut models.fetch) {
            let Some(url) = models.url(id).map(str::to_owned) else {
                continue;
            };
            let m = manifest.clone();
            self.post(async move { Msg::Glb(id, handle::fetch_file(&Shim, m, &url).await) });
        }
        let msgs = core::mem::take(&mut *self.inbox.borrow_mut());
        for m in msgs {
            match m {
                Msg::Manifest(r) => {
                    let r = r.unwrap_or_else(|e| {
                        world.res_mut::<ErrorLog>().error(format!("{}: {e}; models draw their shared files", stream::MANIFEST_URL));
                        None
                    });
                    models.set_manifest(r);
                }
                Msg::Boot(r) => {
                    if let Err(e) = r {
                        world.res_mut::<ErrorLog>().error(format!("boot file: {e}"));
                    }
                    models.stream.boot_left = models.stream.boot_left.saturating_sub(1);
                }
                Msg::Glb(id, Ok(b)) => models.loaded(id, &b),
                Msg::Glb(id, Err(e)) => {
                    world.res_mut::<ErrorLog>().error(format!("model {}: {e}", models.url(id).unwrap_or("?")));
                    models.set_state(id, ModelState::Failed(e));
                }
                Msg::Tex(id, i, r) => {
                    let Some(l) = self.loading.iter_mut().find(|l| l.id == id) else {
                        continue;
                    };
                    let data = r.unwrap_or_else(|e| {
                        world.res_mut::<ErrorLog>().error(format!("model {}: texture {i}: {e}; drawn without it", l.url));
                        TexData::Missing
                    });
                    if let Some(slot) = l.tex.get_mut(i) {
                        *slot = Some(data);
                    }
                }
            }
        }
        for (id, prepared) in core::mem::take(&mut models.pending) {
            let url = models.url(id).unwrap_or("").to_owned();
            self.start(id, url, prepared, &mut models.stream);
        }
        let budget = match world.res::<Tiers>().active().max_texture_memory {
            0 => u64::MAX,
            mb => u64::from(mb) << 20,
        };
        let mut i = 0;
        while i < self.loading.len() {
            if self.loading[i].tex.iter().any(Option::is_none) {
                i += 1;
                continue;
            }
            let l = self.loading.swap_remove(i);
            let id = l.id; #[cfg(feature = "level-groups")] let before = self.resident;
            let mut log = world.res_mut::<ErrorLog>();
            match self.upload(dev, queue, l, budget, &mut log) {
                Ok(mut m) => { #[cfg(feature = "level-groups")] models.stream.levels.uploaded(id, self.resident - before);
                    let at = id.0 as usize;
                    if self.ready.len() <= at {
                        self.ready.resize_with(at + 1, || None);
                    }
                    m.shown = !cones_read || m.cones.baked();
                    if m.shown {
                        models.set_state(id, ModelState::Ready);
                    }
                    self.ready[at] = Some(m);
                }
                Err(e) => {
                    log.error(format!("model {}: {e}", models.url(id).unwrap_or("?")));
                    models.set_state(id, ModelState::Failed(e));
                }
            }
        } #[cfg(feature = "level-groups")] self.sweep_released(&mut models);
        // The sun cones: one model's share a frame where they are read.
        let drawn = |mi: usize| models.draws.iter().any(|d| d.model.0 as usize == mi);
        let next = model::sun::pick(cones_read, self.ready.iter().enumerate().map(|(mi, gm)| gm.as_ref().map_or((true, false, false), |g| (g.cones.baked(), !g.shown, drawn(mi)))));
        if let Some(gm) = next.and_then(|mi| self.ready.get_mut(mi)).and_then(Option::as_mut)
            && let Some(baked) = gm.cones.advance(model::sun::FRAME_UNITS)
        { #[cfg(feature = "skinning")] let baked = model::skin::unproxy(baked, &mut gm.skin.keep, &gm.skin.bind);
            for (gp, p) in gm.parts.iter().zip(&baked) {
                queue.write_buffer(&gp.vb, 0, &p.vertices);
            }
        }
        // A model waiting for its cones shows once they are in, or at once
        // when the frame has a shadow map again (they are not read).
        for (mi, gm) in self.ready.iter_mut().enumerate() {
            if let Some(gm) = gm
                && !gm.shown
                && (gm.cones.baked() || !cones_read)
            {
                gm.shown = true;
                models.set_state(ModelId(mi as u32), ModelState::Ready);
            }
        }
        // This frame's instances, one run per part of each drawn model: a
        // shared part draws every instance, a level's part the instances
        // at that level.
        models.select_lods(rw); #[cfg(feature = "skinning")] self.skin.frame(dev, queue, &self.ready, &mut models, world);
        for (mi, gm) in self.ready.iter().enumerate() {
            let Some(gm) = gm.as_ref().filter(|g| g.shown) else {
                continue;
            };
            if !models.draws.iter().any(|d| d.model.0 as usize == mi) {
                continue;
            }
            for (pi, p) in gm.parts.iter().enumerate() {
                let first = (self.staging.len() / INSTANCE_WORDS) as u32;
                models.stage(mi, &p.part, &mut self.staging);
                let count = (self.staging.len() / INSTANCE_WORDS) as u32 - first;
                if count > 0 {
                    self.runs.push(Run { model: mi, part: pi, first, count });
                }
            }
        } #[cfg(feature = "view-model-models")] self.vm_stage(&mut models, rw);
        models.draws.clear(); #[cfg(feature = "skinning")] models.skin.clear();
        models.stream.frame();
        if self.staging.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&self.staging);
        if bytes.len() as u64 > self.instances.size() {
            self.instances = instance_buffer(dev, (bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.instances, 0, bytes);
    }

    /// Start fetching a parsed model's textures.
    fn start(&mut self, id: ModelId, url: String, prepared: Prepared, s: &mut handle::Streaming) {
        self.loading.retain(|l| l.id != id);
        let folder = url.rfind('/').map_or("", |k| &url[..=k]).to_owned();
        let m = s.shared();
        for (i, need) in prepared.textures.iter().enumerate() {
            let src = match need.source(self.dev_tex) {
                TexSource::Ktx2(u, _) => Source::Url(model::resolve_uri(&folder, u), true, m.clone()),
                TexSource::Embedded(b) => Source::Bytes(b.to_vec()),
                TexSource::Uri(u) => Source::Url(model::resolve_uri(&folder, u), false, m.clone()),
                TexSource::None => Source::Missing,
            };
            if let Source::Url(u, ..) = &src {
                // Asked for now: the background plan leaves it out.
                s.mark(u);
            }
            self.post(async move { Msg::Tex(id, i, load_texture(src).await) });
        }
        let tex = (0..prepared.textures.len()).map(|_| None).collect();
        self.loading.push(Loading { id, url, prepared, tex });
    }

    /// Put a model whose textures all arrived on the GPU.
    fn upload(&mut self, dev: &wgpu::Device, queue: &wgpu::Queue, l: Loading, budget: u64, log: &mut ErrorLog) -> Result<GpuModel, String> {
        let Loading { url, prepared, tex, .. } = l;
        let mut ups: Vec<Option<Upload<'_>>> = Vec::with_capacity(tex.len());
        for (t, need) in tex.iter().zip(&prepared.textures) {
            ups.push(match t {
                Some(d @ TexData::Ktx2(b)) => {
                    let k = texture::parse_ktx2(b)?;
                    let format = ktx_format(k.vk_format, need.srgb).ok_or("ktx2: a format the engine does not upload")?;
                    if !dev.features().contains(format.required_features()) {
                        return Err("ktx2: the device cannot sample this set's format".to_string());
                    }
                    let block = texture::block_of(k.vk_format).ok_or("ktx2: a format the engine does not upload")?;
                    let size = TexSize { width: k.width, height: k.height, levels: k.levels.len() as u32, block };
                    Some(Upload { format, size, data: d, ktx: Some(k) })
                }
                Some(d @ TexData::Bitmaps(v)) => v.first().map(|top| Upload {
                    format: if need.srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
                    size: TexSize { width: top.width(), height: top.height(), levels: v.len() as u32, block: (1, 4) },
                    data: d,
                    ktx: None,
                }),
                _ => None,
            });
        }
        // The tier's budget over every model's textures.
        let sizes: Vec<TexSize> = ups.iter().flatten().map(|u| u.size).collect();
        let mut skips = texture::drop_top_mips(&sizes, budget.saturating_sub(self.resident)).into_iter();
        let mut textures = Vec::with_capacity(ups.len());
        let mut views: Vec<Option<wgpu::TextureView>> = Vec::with_capacity(ups.len());
        let mut dropped = 0;
        for u in &ups {
            let Some(u) = u else {
                views.push(None);
                continue;
            };
            let mut skip = skips.next().unwrap_or(0);
            // A block-compressed top level must be whole blocks.
            let (bw, _) = u.size.block;
            while skip > 0 && !((u.size.width >> skip).is_multiple_of(bw) && (u.size.height >> skip).is_multiple_of(bw)) {
                skip -= 1;
            }
            dropped += u32::from(skip > 0);
            let (t, v) = self.upload_texture(dev, queue, u, skip);
            self.resident += u.size.bytes(skip);
            textures.push(t);
            views.push(Some(v));
        }
        if dropped > 0 {
            log.note(format!("model {url}: {dropped} textures over the tier's texture budget drop their top mip levels"));
        }
        let mut materials = Vec::with_capacity(prepared.materials.len());
        let mut uniforms = Vec::with_capacity(prepared.materials.len());
        for m in &prepared.materials {
            let view = |k: usize, d: usize| m.maps[k].and_then(|i| views.get(i)).and_then(Option::as_ref).unwrap_or(&self.defaults[d].1);
            let mut words: [f32; MATERIAL_WORDS] = m.words;
            if m.maps[1].and_then(|i| views.get(i)).and_then(Option::as_ref).is_none() {
                // No normal map arrived: the geometric normal.
                words[10] = 0.0;
            }
            let ub = dev.create_buffer(&wgpu::BufferDescriptor {
                label: Some("evani.model.material"),
                size: (MATERIAL_WORDS * 4) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&ub, 0, bytemuck::cast_slice(&words)); #[cfg(feature = "relief")] let hv = m.relief.height.and_then(|i| views.get(i)).and_then(Option::as_ref); #[cfg(feature = "relief")] let rub = super::models_relief::material_buffer(dev, queue, &m.relief, hv.is_some()); #[cfg(all(feature = "look-surface", not(feature = "relief")))] let mrv = m.surface.mr.and_then(|i| views.get(i)).and_then(Option::as_ref).unwrap_or(&self.defaults[0].1); #[cfg(all(feature = "look-surface", not(feature = "relief")))] let ocv = m.surface.occlusion.and_then(|i| views.get(i)).and_then(Option::as_ref); #[cfg(all(feature = "look-surface", not(feature = "relief")))] let sub = super::models_surface::material_buffer(dev, queue, &m.surface, ocv.is_some()); #[cfg(all(feature = "material-layers", not(feature = "relief")))] let lub = super::models_layers::material_buffer(dev, queue, &m.layers);
            materials.push(dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("evani.model.material"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view(0, 0)) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(view(1, 1)) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(view(2, 0)) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) }, #[cfg(feature = "relief")] wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(hv.unwrap_or(&self.defaults[0].1)) }, #[cfg(feature = "relief")] wgpu::BindGroupEntry { binding: 6, resource: rub.as_entire_binding() }, #[cfg(feature = "relief")] wgpu::BindGroupEntry { binding: 7, resource: self.relief.as_entire_binding() }, #[cfg(all(feature = "look-surface", not(feature = "relief")))] wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(mrv) }, #[cfg(all(feature = "look-surface", not(feature = "relief")))] wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(ocv.unwrap_or(&self.defaults[0].1)) }, #[cfg(all(feature = "look-surface", not(feature = "relief")))] wgpu::BindGroupEntry { binding: 8, resource: sub.as_entire_binding() }, #[cfg(all(feature = "material-layers", not(feature = "relief")))] wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&self.layers.weave_view) }, #[cfg(all(feature = "material-layers", not(feature = "relief")))] wgpu::BindGroupEntry { binding: 10, resource: lub.as_entire_binding() }, #[cfg(all(feature = "material-layers", not(feature = "relief")))] wgpu::BindGroupEntry { binding: 11, resource: self.layers.frame.as_entire_binding() },
                ],
            }));
            uniforms.push(ub); #[cfg(feature = "relief")] uniforms.push(rub); #[cfg(all(feature = "look-surface", not(feature = "relief")))] uniforms.push(sub); #[cfg(all(feature = "material-layers", not(feature = "relief")))] uniforms.push(lub);
        }
        let mut parts = Vec::with_capacity(prepared.parts.len());
        let mut cpu = Vec::with_capacity(prepared.parts.len());
        for mut p in prepared.parts {
            if p.indices.is_empty() || p.material >= materials.len() {
                continue;
            }
            let buf = |label, bytes: &[u8], usage| {
                let b = dev.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: bytes.len() as u64,
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&b, 0, bytes);
                b
            };
            let vb = buf("evani.model.vertices", &p.vertices, wgpu::BufferUsages::VERTEX);
            let ib = buf("evani.model.indices", bytemuck::cast_slice(&p.indices), wgpu::BufferUsages::INDEX); #[cfg(feature = "skinning")] let sb = (!p.skin.is_empty()).then(|| buf("evani.model.skin", &p.skin, wgpu::BufferUsages::VERTEX));
            let count = p.indices.len() as u32;
            // The drawn copy without its bytes; the full one waits for the
            // cone bake.
            let (vertices, indices) = (core::mem::take(&mut p.vertices), core::mem::take(&mut p.indices));
            parts.push(GpuPart { vb, ib, count, part: p.clone(), #[cfg(feature = "skinning")] sb });
            cpu.push(Part { vertices, indices, ..p });
        } #[cfg(feature = "skinning")] let (cpu, keep) = model::skin::proxies(cpu, &prepared.bind);
        Ok(GpuModel { parts, materials, _textures: textures, _uniforms: uniforms, cones: model::sun::Lazy::new(cpu), shown: false, #[cfg(feature = "skinning")] skin: skin::ModelSkin { bind: prepared.bind, keep } })
    }

    /// One texture with its top `skip` levels dropped.
    fn upload_texture(&self, dev: &wgpu::Device, queue: &wgpu::Queue, u: &Upload<'_>, skip: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let (w, h) = ((u.size.width >> skip).max(1), (u.size.height >> skip).max(1));
        let levels = u.size.levels.max(1) - skip;
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if matches!(u.data, TexData::Bitmaps(_)) {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        let t = dev.create_texture(&wgpu::TextureDescriptor {
            label: Some("evani.model.texture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: u.format,
            usage,
            view_formats: &[],
        });
        match (u.data, &u.ktx) {
            (TexData::Ktx2(b), Some(k)) => {
                let (bw, bytes) = u.size.block;
                for (lvl, &(off, len)) in k.levels.iter().enumerate().skip(skip as usize) {
                    let (lw, lh) = ((u.size.width >> lvl).max(1), (u.size.height >> lvl).max(1));
                    let Some(data) = b.get(off..off + len) else {
                        continue;
                    };
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &t,
                            mip_level: lvl as u32 - skip,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        data,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(lw.div_ceil(bw) * bytes),
                            rows_per_image: Some(lh.div_ceil(bw)),
                        },
                        wgpu::Extent3d { width: lw.div_ceil(bw) * bw, height: lh.div_ceil(bw) * bw, depth_or_array_layers: 1 },
                    );
                }
            }
            (TexData::Bitmaps(v), _) => {
                for (lvl, bm) in v.iter().enumerate() {
                    if lvl >= skip as usize {
                        queue.copy_external_image_to_texture(
                            &wgpu::CopyExternalImageSourceInfo {
                                source: wgpu::ExternalImageSource::ImageBitmap(bm.clone()),
                                origin: wgpu::Origin2d::ZERO,
                                flip_y: false,
                            },
                            wgpu::CopyExternalImageDestInfo {
                                texture: &t,
                                mip_level: lvl as u32 - skip,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                                color_space: wgpu::PredefinedColorSpace::Srgb,
                                premultiplied_alpha: false,
                            },
                            wgpu::Extent3d { width: bm.width(), height: bm.height(), depth_or_array_layers: 1 },
                        );
                    }
                    bm.close();
                }
            }
            _ => {}
        }
        let v = t.create_view(&wgpu::TextureViewDescriptor::default());
        (t, v)
    }

    /// The model draws of the forward pass, view by view. Returns the draw
    /// count and adds the triangles to `tris`.
    pub fn draw(
        &self,
        rp: &mut wgpu::RenderPass<'_>,
        frame_bg: &wgpu::BindGroup,
        views: &[(u32, [f32; 4])],
        ms: bool,
        pipes: &Pipelines,
        tris: &mut u32,
    ) -> u32 {
        let (false, Some(pipe)) = (self.runs.is_empty(), pipes.model(ms)) else {
            return 0;
        };
        rp.set_pipeline(pipe);
        rp.set_vertex_buffer(1, self.instances.slice(..));
        let mut draws = 0;
        for &(off, px) in views {
            rp.set_bind_group(0, frame_bg, &[off]);
            rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
            for r in &self.runs {
                let Some(gm) = self.ready.get(r.model).and_then(Option::as_ref) else {
                    continue;
                };
                let Some(p) = gm.parts.get(r.part) else {
                    continue;
                }; #[cfg(feature = "skinning")] if p.sb.is_some() { continue; }
                let Some(mat) = gm.materials.get(p.part.material) else {
                    continue;
                };
                rp.set_bind_group(1, mat, &[]);
                rp.set_vertex_buffer(0, p.vb.slice(..));
                rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
                #[cfg(feature = "hooks")]
                if !split_scissor(rp, self.split, r.model, px, views.len()) {
                    continue;
                }
                rp.draw_indexed(0..p.count, 0, r.first..r.first + r.count);
                draws += 1;
                *tris = tris.saturating_add((p.count / 3).saturating_mul(r.count));
            }
            #[cfg(feature = "hooks")]
            split_reset(rp, self.split, px, views.len());
        } #[cfg(feature = "skinning")] { draws += self.draw_skinned(rp, frame_bg, views, ms, pipes, tris); }
        draws
    }

    /// The model draws of the shadow pass (shadow uniform at `off`).
    #[cfg(feature = "shadows")]
    pub fn draw_shadows(&self, rp: &mut wgpu::RenderPass<'_>, bg: &wgpu::BindGroup, off: u32) -> u32 {
        if self.runs.is_empty() {
            return 0;
        }
        rp.set_pipeline(&self.shadow);
        rp.set_bind_group(0, bg, &[off]);
        rp.set_vertex_buffer(1, self.instances.slice(..));
        let mut draws = 0;
        for r in &self.runs {
            let Some(p) = self.ready.get(r.model).and_then(Option::as_ref).and_then(|m| m.parts.get(r.part)) else {
                continue;
            }; #[cfg(feature = "skinning")] if p.sb.is_some() { continue; }
            rp.set_vertex_buffer(0, p.vb.slice(..));
            rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..p.count, 0, r.first..r.first + r.count);
            draws += 1;
        } #[cfg(feature = "skinning")] self.skin_shadows(rp, bg, off, &mut draws);
        draws
    }
}

#[cfg(feature = "skinning")]
pub(super) mod skin;

/// The split view's scissor (S3-14, dev builds): with one view, the split's
/// `left` model draws left of the line and `right` right of it. False: the
/// run's side is empty this frame (skip it).
#[cfg(feature = "hooks")]
fn split_scissor(rp: &mut wgpu::RenderPass<'_>, split: Option<crate::model::Split>, model: usize, px: [f32; 4], views: usize) -> bool {
    let Some(s) = split.filter(|_| views == 1) else {
        return true;
    };
    let (x0, y0, w, h) = view_px(px);
    match s.columns(model as u32, x0, w) {
        Some((_, 0)) => false,
        Some((x, cw)) if h > 0 => {
            rp.set_scissor_rect(x, y0, cw, h);
            true
        }
        Some(_) => false,
        None => {
            rp.set_scissor_rect(x0, y0, w.max(1), h.max(1));
            true
        }
    }
}

/// Back to the whole view after the split view's runs.
#[cfg(feature = "hooks")]
fn split_reset(rp: &mut wgpu::RenderPass<'_>, split: Option<crate::model::Split>, px: [f32; 4], views: usize) {
    if split.is_some() && views == 1 {
        let (x0, y0, w, h) = view_px(px);
        rp.set_scissor_rect(x0, y0, w.max(1), h.max(1));
    }
}

/// A viewport as whole pixels inside it.
#[cfg(feature = "hooks")]
fn view_px(px: [f32; 4]) -> (u32, u32, u32, u32) {
    let x0 = px[0].max(0.0).ceil();
    let y0 = px[1].max(0.0).ceil();
    let w = ((px[0] + px[2]).floor() - x0).max(0.0);
    let h = ((px[1] + px[3]).floor() - y0).max(0.0);
    (x0 as u32, y0 as u32, w as u32, h as u32)
}

/// The lit surface pass's fragment stage (feature `look-surface` without
/// `relief`, S5-R5b; `shaders/model_lit.wgsl`), for the static and the
/// skinned model pipelines. Declared last: no line above moves.
#[cfg(all(feature = "look-surface", not(feature = "relief"), not(feature = "material-layers")))]
pub(super) const LIT_FS: &str = "model_lit_fs";

/// First-person models (ChimpDuty intake). Declared last: no line above moves.
#[cfg(feature = "view-model-models")]
mod vm;

/// The layered materials' fragment stage (feature `material-layers` without
/// `relief`; `shaders/model_lit_layers.wgsl`): the lit surface pass with
/// sheen, clear coat, anisotropy and iridescence on top, for the static and
/// the skinned model pipelines. Declared last: no line above moves.
#[cfg(all(feature = "material-layers", not(feature = "relief")))]
pub(super) const LIT_FS: &str = "model_layered_fs";
