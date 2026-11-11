//! Kit textures on the GPU (feature `kit-textures`, TopGun W3; the CPU side
//! and the WGSL contract are in [`crate::kit_textures`]).
//!
//! * The kit group (group 1 of the forward mesh pass, the kit shadow pass,
//!   the AO depth and the near cascade) gains bindings 16 to 23: the two
//!   sets' `ah` / `nra` arrays, the macro array, the wrap and clamp
//!   samplers and the ready words. One layout and one bind group for every
//!   pass, so the same kit WGSL compiles in all of them.
//! * Nothing registered: 1 x 1 x 1 zero dummies (and ready 0).
//! * A set loads once the tier has settled, by the plan in
//!   [`kt::SetStream`]: with a boot size, every wanted layer's boot pair
//!   first (bound as it lands), then the tier-size pairs into a second pair
//!   of arrays, bound in one swap once every wanted layer is in (the boot
//!   arrays are then freed). Each layer's ready word fades 0 to 1 over
//!   [`KitTextures::fade`] seconds of render time once its pair is in the
//!   bound arrays. A change of tier, set or format drops the arrays and
//!   loads again.
//! * The macro array is RGBA8 with a full mip chain; each pending write
//!   uploads its rect to level 0, then one small render pass per level
//!   redraws only the rect's parent texels (bilinear at the 2 x 2 corner =
//!   the box filter), all in one submission before the frame.
//!
//! State lives here in a thread local (like the kit code), so the hooks in
//! the shared kit-material file stay one-liners.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::tiers::Tiers;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::kit_textures::{self as kt, Bound, KitTextures, LayerStage, MAX_LAYERS, MAX_SETS, ManifestState, Map, Wave};
use crate::texture::{self, DeviceTex, TexFormat};

thread_local! {
    static TEX: RefCell<Option<TexGpu>> = const { RefCell::new(None) };
    static SETTLED: Cell<bool> = const { Cell::new(false) };
    /// The largest forward module built with kit code (bytes), and whether
    /// it was reported.
    static SOURCE: Cell<(usize, bool)> = const { Cell::new((0, false)) };
}

/// Note a forward module just patched with kit code (its length).
pub fn note_source(len: usize) {
    let (n, said) = SOURCE.get();
    if len > n {
        SOURCE.set((len, said && len <= kt::MAX_SHADER_BYTES));
    }
}

/// The frame's tier state: loads start once the tier has settled.
pub fn set_settled(on: bool) {
    SETTLED.set(on);
}

/// The device: the plain request plus every block-compression family the
/// adapter can sample (the same request as with `models`).
#[cfg(not(feature = "models"))]
pub async fn request_device(adapter: &wgpu::Adapter, timestamps: bool) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let mut features = if timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
    features |= adapter.features() & (wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_BC | wgpu::Features::TEXTURE_COMPRESSION_ETC2);
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

/// The kit group layout with the texture bindings (15 to 23).
pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let e = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
    let arr = wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: wgpu::TextureViewDimension::D2Array,
        multisampled: false,
    };
    let samp = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("evani.layout.kit"),
        entries: &[
            e(15, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
            e(16, arr),
            e(17, arr),
            e(18, arr),
            e(19, arr),
            e(20, arr),
            e(21, samp),
            e(22, samp),
            e(23, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
        ],
    })
}

/// Start (or restart, on a new device) with dummies everywhere.
pub fn init(device: &wgpu::Device) {
    TEX.with_borrow_mut(|t| *t = Some(TexGpu::new(device)));
}

/// The kit group for `buf` (the kit data) with the current textures.
pub fn bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, buf: &wgpu::Buffer) -> wgpu::BindGroup {
    TEX.with_borrow_mut(|t| {
        let t = t.get_or_insert_with(|| TexGpu::new(device));
        let mac = t.macro_.as_ref().map_or(&t.dummy.1, |m| &m.view);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.kit_data"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 15, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 16, resource: wgpu::BindingResource::TextureView(pick(t.sets[0].bound(Map::Ah), &t.dummy.1)) },
                wgpu::BindGroupEntry { binding: 17, resource: wgpu::BindingResource::TextureView(pick(t.sets[0].bound(Map::Nra), &t.dummy.1)) },
                wgpu::BindGroupEntry { binding: 18, resource: wgpu::BindingResource::TextureView(pick(t.sets[1].bound(Map::Ah), &t.dummy.1)) },
                wgpu::BindGroupEntry { binding: 19, resource: wgpu::BindingResource::TextureView(pick(t.sets[1].bound(Map::Nra), &t.dummy.1)) },
                wgpu::BindGroupEntry { binding: 20, resource: wgpu::BindingResource::TextureView(mac) },
                wgpu::BindGroupEntry { binding: 21, resource: wgpu::BindingResource::Sampler(&t.wrap) },
                wgpu::BindGroupEntry { binding: 22, resource: wgpu::BindingResource::Sampler(&t.clamp) },
                wgpu::BindGroupEntry { binding: 23, resource: t.state.as_entire_binding() },
            ],
        })
    })
}

/// A set's array view, or the dummy.
fn pick<'a>(a: Option<&'a Arr>, dummy: &'a wgpu::TextureView) -> &'a wgpu::TextureView {
    a.map_or(dummy, |a| &a.view)
}

/// Load, upload and write what changed. Returns whether a view or sampler
/// changed (the caller rebuilds the kit group).
pub fn prepare(device: &wgpu::Device, queue: &wgpu::Queue, world: &World) -> bool {
    TEX.with_borrow_mut(|t| t.as_mut().is_some_and(|t| t.prepare(device, queue, world)))
}

/// One array (all layers of one map of one set, or the macro).
struct Arr {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    levels: u32,
}

#[derive(Default)]
struct SetGpu {
    /// `(set generation, size, boot size, format)` it loads; `None` before
    /// the first sync.
    key: Option<(u64, u32, u32, TexFormat)>,
    layers: u32,
    /// Its plan.
    stream: kt::SetStream,
    /// Per wave (boot, full), per map (AH, NRA).
    arrs: [[Option<Arr>; 2]; 2],
    /// Paths of its layers.
    paths: Vec<String>,
}

fn wi(w: Wave) -> usize {
    usize::from(w == Wave::Full)
}

fn mi(m: Map) -> usize {
    usize::from(m == Map::Nra)
}

impl SetGpu {
    /// The array a map binds now.
    fn bound(&self, map: Map) -> Option<&Arr> {
        let w = match self.stream.bound() {
            Bound::Boot => Wave::Boot,
            Bound::Full => Wave::Full,
        };
        self.arrs[wi(w)][mi(map)].as_ref()
    }
}

struct MacroGpu {
    generation: u64,
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    size: u32,
    /// Per layer, per level: a one-level, one-layer view (render target and
    /// mip source).
    levels_views: Vec<Vec<wgpu::TextureView>>,
    /// Per layer, per level from 0: the bind group that reads that level.
    src: Vec<Vec<wgpu::BindGroup>>,
}

struct MipGen {
    pipe: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

struct File {
    set: usize,
    key: (u64, u32, u32, TexFormat),
    layer: usize,
    map: Map,
    wave: Wave,
    url: String,
    bytes: Result<Vec<u8>, String>,
}

enum Msg {
    Manifest(Result<Option<String>, String>),
    File(File),
}

struct TexGpu {
    dummy: (wgpu::Texture, wgpu::TextureView),
    sets: [SetGpu; MAX_SETS],
    macro_: Option<MacroGpu>,
    mipgen: Option<MipGen>,
    wrap: wgpu::Sampler,
    clamp: wgpu::Sampler,
    aniso: u16,
    state: wgpu::Buffer,
    state_dirty: bool,
    dev_tex: DeviceTex,
    inbox: Rc<RefCell<Vec<Msg>>>,
}

fn sampler(device: &wgpu::Device, label: &str, mode: wgpu::AddressMode, aniso: u16) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: mode,
        address_mode_v: mode,
        address_mode_w: mode,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: aniso.max(1),
        ..Default::default()
    })
}

fn array_view(tex: &wgpu::Texture) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })
}

/// The wgpu format of the KTX2 formats a set may hold (BC7, ASTC 4x4,
/// RGBA8), in the map's colour space (the blocks are the same in both).
fn wgpu_format(vk: u32, srgb: bool) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as F;
    let astc = |channel| F::Astc { block: wgpu::AstcBlock::B4x4, channel };
    let (u, s) = match vk {
        37 | 43 => (F::Rgba8Unorm, F::Rgba8UnormSrgb),
        145 | 146 => (F::Bc7RgbaUnorm, F::Bc7RgbaUnormSrgb),
        157 | 158 => (astc(wgpu::AstcChannel::Unorm), astc(wgpu::AstcChannel::UnormSrgb)),
        _ => return None,
    };
    Some(if srgb { s } else { u })
}

/// Fetch one file. `Ok(None)` for a 404 (a missing manifest is no error).
async fn fetch(url: &str) -> Result<Option<Vec<u8>>, String> {
    let win = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response = JsFuture::from(win.fetch_with_str(url)).await.map_err(evani_core::web::js_err)?.unchecked_into();
    if resp.status() == 404 {
        return Ok(None);
    }
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(evani_core::web::js_err)?).await.map_err(evani_core::web::js_err)?;
    Ok(Some(js_sys::Uint8Array::new(&buf).to_vec()))
}

async fn read_manifest() -> Result<Option<String>, String> {
    match fetch(kt::MANIFEST_URL).await? {
        // A dev server may answer a missing file with the page itself.
        Some(b) if b.first() == Some(&b'{') => String::from_utf8(b).map(Some).map_err(|_| "not UTF-8".to_string()),
        _ => Ok(None),
    }
}

const MIPGEN_WGSL: &str = "@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    // The corner shared by the 2 x 2 source texels: bilinear = their mean.
    return textureSampleLevel(src, smp, pos.xy * 2.0 / vec2<f32>(textureDimensions(src)), 0.0);
}
";

impl MipGen {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("evani.kit_macro_mips"), source: wgpu::ShaderSource::Wgsl(MIPGEN_WGSL.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.kit_macro_mips"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("evani.kit_macro_mips"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("evani.kit_macro_mips"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipe, layout, sampler: sampler(device, "evani.kit_macro_mips", wgpu::AddressMode::ClampToEdge, 1) }
    }
}

impl TexGpu {
    fn new(device: &wgpu::Device) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("evani.kit_tex.dummy"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = array_view(&tex);
        let f = device.features();
        Self {
            dummy: (tex, view),
            sets: Default::default(),
            macro_: None,
            mipgen: None,
            wrap: sampler(device, "evani.kit_wrap", wgpu::AddressMode::Repeat, 1),
            clamp: sampler(device, "evani.kit_clamp", wgpu::AddressMode::ClampToEdge, 1),
            aniso: 1,
            state: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("evani.kit_tex_state"),
                size: (MAX_SETS * MAX_LAYERS * 4) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state_dirty: true,
            dev_tex: DeviceTex {
                astc: f.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC),
                bc: f.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
                etc2: f.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2),
            },
            inbox: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn post(&self, fut: impl core::future::Future<Output = Msg> + 'static) {
        let inbox = self.inbox.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let m = fut.await;
            inbox.borrow_mut().push(m);
        });
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, world: &World) -> bool {
        let Some(mut kt) = world.try_res_mut::<KitTextures>() else {
            return false;
        };
        let mut errs: Vec<String> = Vec::new();
        let mut changed = false;
        if kt.manifest == ManifestState::Unread {
            kt.manifest = ManifestState::Asked;
            self.post(async { Msg::Manifest(read_manifest().await) });
        }
        let msgs = core::mem::take(&mut *self.inbox.borrow_mut());
        for m in msgs {
            match m {
                Msg::Manifest(r) => {
                    kt.manifest = ManifestState::Done;
                    match r {
                        Ok(Some(text)) => {
                            if let Err(e) = kt.apply_manifest(&text) {
                                errs.push(format!("kit textures: {e}"));
                            }
                        }
                        Ok(None) => {}
                        Err(e) => errs.push(format!("kit textures: {}: {e}", kt::MANIFEST_URL)),
                    }
                }
                Msg::File(f) => changed |= self.arrive(device, queue, &mut kt, f, &mut errs),
            }
        }
        let tier = world.res::<Tiers>().active().id.clone();
        let a = kt.anisotropy(&tier);
        if a != self.aniso {
            self.aniso = a;
            self.wrap = sampler(device, "evani.kit_wrap", wgpu::AddressMode::Repeat, a);
            changed = true;
        }
        if SETTLED.get() && kt.manifest == ManifestState::Done {
            kt.set_started();
            for s in 0..MAX_SETS {
                changed |= self.sync_set(s, &tier, &mut kt);
            }
        }
        let dt = world.try_res::<evani_core::time::Time>().map_or(1.0 / 60.0, |t| t.frame_dt);
        self.state_dirty |= kt.tick(dt);
        changed |= self.sync_macro(device, queue, &mut kt, &mut errs);
        if self.state_dirty {
            self.state_dirty = false;
            queue.write_buffer(&self.state, 0, bytemuck::cast_slice(&kt.state_words()));
        }
        drop(kt);
        let (n, said) = SOURCE.get();
        if n > kt::MAX_SHADER_BYTES && !said {
            SOURCE.set((n, true));
            errs.push(format!("kit textures: the forward shader with the kit code is {} KB, over the {} KB cap; fix: shrink the kit's WGSL (cargo xtask check measures it)", n / 1024, kt::MAX_SHADER_BYTES / 1024));
        }
        if !errs.is_empty() {
            let mut log = world.res_mut::<ErrorLog>();
            for e in errs {
                log.error(e);
            }
        }
        changed
    }

    /// Follow set `s`: (re)plan it when its description, the tier's size or
    /// the format changed, then start the fetches its plan allows now.
    /// Returns whether views were dropped.
    fn sync_set(&mut self, s: usize, tier: &str, kt: &mut KitTextures) -> bool {
        let (generation, desc) = kt.set_desc(s);
        let shipped = if kt.shipped().is_empty() { vec![TexFormat::Astc4x4, TexFormat::Bc7, TexFormat::Rgba] } else { kt.shipped().to_vec() };
        let fmt = kt::pick_format(self.dev_tex, &shipped);
        let size = desc.map_or(0, |d| d.size_for(tier));
        let boot = desc.map_or(0, |d| d.boot_for(tier));
        let key = (generation, size, boot, fmt);
        let mut dropped = false;
        if self.sets[s].key != Some(key) {
            let set = &self.sets[s];
            dropped = set.arrs.iter().flatten().any(Option::is_some);
            let (paths, n) = desc.map_or((Vec::new(), 0), |d| (d.layers.iter().map(|l| l.path.clone()).collect(), d.layers.len()));
            self.sets[s] = SetGpu { key: Some(key), layers: n as u32, stream: kt::SetStream::new(size, boot, n), paths, ..Default::default() };
            for l in 0..MAX_LAYERS {
                kt.set_showing(s, l, false);
                kt.set_stage(s, l, LayerStage::Off);
            }
            self.state_dirty = true;
        }
        let want = kt.wanted(s);
        let fetches = self.sets[s].stream.next(want, kt.held());
        for f in fetches {
            let Some(path) = self.sets[s].paths.get(f.layer) else { continue };
            let url = kt::file_url(path, f.map, f.size, fmt);
            let (layer, map, wave) = (f.layer, f.map, f.wave);
            self.post(async move {
                let bytes = fetch(&url).await.and_then(|b| b.ok_or_else(|| "not found (404)".to_string()));
                Msg::File(File { set: s, key, layer, map, wave, url, bytes })
            });
        }
        let st = &self.sets[s].stream;
        for l in 0..st.layers {
            kt.set_stage(s, l, st.stage(l, want));
        }
        dropped
    }

    /// Upload one arrived file into its array layer and move the set's plan
    /// on. Returns whether a bound view changed (an array was created, or
    /// the set swapped to its tier-size arrays).
    fn arrive(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, kt: &mut KitTextures, f: File, errs: &mut Vec<String>) -> bool {
        if self.sets[f.set].key != Some(f.key) {
            return false; // a stale load (the tier or the set changed meanwhile)
        }
        let want = kt.wanted(f.set);
        let set = &mut self.sets[f.set];
        let (created, swapped) = match upload(device, queue, set, &f) {
            Ok(created) => {
                kt.add_loaded(f.bytes.as_ref().map_or(0, |b| b.len() as u64));
                (created, set.stream.got(f.layer, f.map, f.wave, want))
            }
            Err(e) => {
                errs.push(format!("kit textures: {}: {e}", f.url));
                (false, set.stream.fail(f.layer, want))
            }
        };
        if swapped {
            // Every wanted layer is in at the tier's size: free the boot arrays.
            set.arrs[wi(Wave::Boot)] = [None, None];
        }
        for l in 0..set.stream.layers {
            kt.set_showing(f.set, l, set.stream.showing(l));
            kt.set_stage(f.set, l, set.stream.stage(l, want));
        }
        self.state_dirty = true;
        created || swapped
    }
}

/// Upload one file into its wave's array (made on its first file). Returns
/// whether the array was created.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, set: &mut SetGpu, f: &File) -> Result<bool, String> {
    let bytes = f.bytes.as_ref().map_err(Clone::clone)?;
    let k = texture::parse_ktx2(bytes).map_err(|e| e.to_string())?;
    let (Some(format), Some(block)) = (wgpu_format(k.vk_format, f.map.srgb()), texture::block_of(k.vk_format)) else {
        return Err("not BC7, ASTC 4x4 or RGBA8".into());
    };
    if !device.features().contains(format.required_features()) {
        return Err("the device cannot sample this format".into());
    }
    let levels = k.levels.len() as u32;
    let layers = set.layers.max(1);
    let slot = &mut set.arrs[wi(f.wave)][mi(f.map)];
    let mut created = false;
    if slot.is_none() {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(if f.map == Map::Ah { "evani.kit_tex.ah" } else { "evani.kit_tex.nra" }),
            size: wgpu::Extent3d { width: k.width, height: k.height, depth_or_array_layers: layers },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        *slot = Some(Arr { view: array_view(&tex), _tex: tex, format, size: (k.width, k.height), levels });
        created = true;
    }
    let Some(arr) = slot.as_ref() else {
        return Ok(created);
    };
    if arr.format != format || arr.size != (k.width, k.height) || arr.levels != levels {
        return Err("size, format or mip count differs from the set's other layers (every layer of a set must match)".into());
    }
    let (bw, bb) = block;
    for (lvl, &(off, len)) in k.levels.iter().enumerate() {
        let (lw, lh) = ((k.width >> lvl).max(1), (k.height >> lvl).max(1));
        let Some(data) = bytes.get(off..off + len) else {
            continue;
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &arr._tex, mip_level: lvl as u32, origin: wgpu::Origin3d { x: 0, y: 0, z: f.layer as u32 }, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw.div_ceil(bw) * bb), rows_per_image: Some(lh.div_ceil(bw)) },
            wgpu::Extent3d { width: lw.div_ceil(bw) * bw, height: lh.div_ceil(bw) * bw, depth_or_array_layers: 1 },
        );
    }
    Ok(created)
}

impl TexGpu {
    /// Follow the macro texture: (re)make it, then upload this frame's
    /// writes and redraw their mips. Returns whether its view changed.
    fn sync_macro(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, kt: &mut KitTextures, errs: &mut Vec<String>) -> bool {
        let (generation, desc) = kt.macro_state();
        let desc = desc.map(kt::MacroArray::desc);
        let mut changed = false;
        if self.macro_.as_ref().map(|m| m.generation) != desc.map(|_| generation) {
            changed = true;
            self.macro_ = desc.map(|d| self.make_macro(device, generation, d));
        }
        let Some(writes) = kt.macro_mut().map(kt::MacroArray::take_pending) else {
            return changed;
        };
        if writes.is_empty() {
            return changed;
        }
        let Some(m) = &self.macro_ else {
            return changed;
        };
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("evani.kit_macro") });
        let mipgen = self.mipgen.get_or_insert_with(|| MipGen::new(device));
        for w in &writes {
            let [x, y, ww, hh] = w.rect;
            if w.layer >= m.levels_views.len() as u32 {
                errs.push(format!("kit textures: macro write to layer {} dropped", w.layer));
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &m.tex, mip_level: 0, origin: wgpu::Origin3d { x, y, z: w.layer }, aspect: wgpu::TextureAspect::All },
                &w.data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ww * 4), rows_per_image: Some(hh) },
                wgpu::Extent3d { width: ww, height: hh, depth_or_array_layers: 1 },
            );
            let l = w.layer as usize;
            for (k, [rx, ry, rw, rh]) in kt::mip_rects(w.rect, m.size) {
                let (Some(dst), Some(src)) = (m.levels_views[l].get(k as usize), m.src[l].get(k as usize - 1)) else {
                    continue;
                };
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("evani.kit_macro_mips"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_pipeline(&mipgen.pipe);
                rp.set_bind_group(0, src, &[]);
                rp.set_scissor_rect(rx, ry, rw, rh);
                rp.draw(0..3, 0..1);
            }
        }
        queue.submit([enc.finish()]);
        changed
    }

    fn make_macro(&mut self, device: &wgpu::Device, generation: u64, d: kt::MacroDesc) -> MacroGpu {
        let levels = kt::mip_levels(d.size);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("evani.kit_macro"),
            size: wgpu::Extent3d { width: d.size, height: d.size, depth_or_array_layers: d.layers },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let mipgen = self.mipgen.get_or_insert_with(|| MipGen::new(device));
        let mut levels_views = Vec::with_capacity(d.layers as usize);
        let mut src = Vec::with_capacity(d.layers as usize);
        for layer in 0..d.layers {
            let views: Vec<wgpu::TextureView> = (0..levels)
                .map(|k| {
                    tex.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_mip_level: k,
                        mip_level_count: Some(1),
                        base_array_layer: layer,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect();
            src.push(
                views
                    .iter()
                    .take(levels.saturating_sub(1) as usize)
                    .map(|v| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("evani.kit_macro_mips"),
                            layout: &mipgen.layout,
                            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&mipgen.sampler) }],
                        })
                    })
                    .collect(),
            );
            levels_views.push(views);
        }
        MacroGpu { generation, view: array_view(&tex), tex, size: d.size, levels_views, src }
    }
}
