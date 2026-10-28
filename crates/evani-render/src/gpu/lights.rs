//! Point lights on the GPU (features `point-lights` and `clustered`, S5-R3).
//!
//! * One uniform (bind group 3 of the forward, model and skinned pipelines;
//!   groups 1 and 2 get an empty group where a pipeline has none): a header
//!   `[count, binned views, slice scale, near]`, with `clustered` four view
//!   records (pixel rect, eye, right + tan x, up + tan y, forward), then the
//!   lights as `[position, range]`, `[colour x intensity, 0]`.
//! * The light term is spliced into the forward module at pipeline build
//!   ([`patch`]): every lit shading line (`fs_main`, `model_fs`,
//!   `model_relief_fs`) gains `point_lights(...)` beside the ambient, and the
//!   light code is appended at the end, so slot line numbers stay valid and
//!   the WGSL files are untouched (games without the feature are unchanged).
//!   Unlit materials and sprites never reach those lines.
//! * `clustered`: `shaders/cluster_bin.wgsl` sorts the lights into 16 x 9
//!   screen tiles x 16 exponential depth slices per view (up to 4 views, up
//!   to 31 lights a cluster as u8 indices) before the graph runs, and the
//!   fragment reads its cluster's list. A tier capped at 8 or less, a failed
//!   binning pipeline or a pixel outside every view rect takes the plain
//!   path: a loop over the first [`MAX_LINEAR`] lights.

use evani_core::ecs::World;
use evani_core::tiers::Tiers;

use super::pipelines::Layouts;
use crate::light::{self, MAX_LINEAR};
use crate::splice::Spliced;
use crate::world::RenderWorld;

#[cfg(feature = "clustered")]
const MAX_LIGHTS: usize = 256;
#[cfg(not(feature = "clustered"))]
const MAX_LIGHTS: usize = MAX_LINEAR;
#[cfg(feature = "clustered")]
const MAX_VIEWS: usize = 4;
#[cfg(not(feature = "clustered"))]
const MAX_VIEWS: usize = 0;
const VIEW_WORDS: usize = 20;
const LIST: usize = 4 + MAX_VIEWS * VIEW_WORDS;
const WORDS: usize = LIST + MAX_LIGHTS * 8;

/// The lit shading term of every lit fragment stage, and what it becomes.
const HOOK: &str = "frame.ambient.rgb + frame.light_color.rgb * diffuse";
const HOOKED: &str = "frame.ambient.rgb + point_lights(in.world, n, in.pos) + frame.light_color.rgb * diffuse";

/// One light's contribution: a smooth window to zero at the range over an
/// inverse-square core, through the look's toon ramp (lit side only).
const LIGHT_WGSL: &str = r"
struct PointLight { pos_range: vec4<f32>, color: vec4<f32> }
fn point_light(i: u32, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let l = lights.list[i];
    let d = l.pos_range.xyz - p;
    let d2 = dot(d, d);
    let q = d2 / max(l.pos_range.w * l.pos_range.w, 1e-6);
    let fall = clamp(1.0 - q * q, 0.0, 1.0);
    let ndl = dot(n, d * inverseSqrt(max(d2, 1e-8)));
    let ramp = select(0.0, max(slot_toon_ramp(ndl), 0.0), ndl > 0.0);
    return l.color.rgb * (fall * fall / (d2 + 1.0) * ramp);
}
fn point_lights_linear(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = min(u32(lights.info.x), 8u);
    for (var i = 0u; i < count; i++) {
        sum += point_light(i, p, n);
    }
    return sum;
}
";

#[cfg(not(feature = "clustered"))]
const PATH_WGSL: &str = r"
struct Lights { info: vec4<f32>, list: array<PointLight, 8> }
@group(3) @binding(0) var<uniform> lights: Lights;
fn point_lights(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    return point_lights_linear(p, n);
}
";

#[cfg(feature = "clustered")]
const PATH_WGSL: &str = r"
struct LightView { rect: vec4<f32>, eye: vec4<f32>, right: vec4<f32>, up: vec4<f32>, fwd: vec4<f32> }
struct Lights { info: vec4<f32>, views: array<LightView, 4>, list: array<PointLight, 256> }
@group(3) @binding(0) var<uniform> lights: Lights;
@group(3) @binding(1) var<storage, read> clusters: array<u32>;
fn point_lights(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    var vi = -1;
    let views = min(u32(lights.info.y), 4u);
    for (var v = 0u; v < views; v++) {
        let r = lights.views[v].rect;
        if (vi < 0 && all(frag.xy >= r.xy) && all(frag.xy < r.xy + r.zw)) {
            vi = i32(v);
        }
    }
    var sum = vec3<f32>(0.0);
    if (vi < 0) {
        sum = point_lights_linear(p, n);
    } else {
        let vw = lights.views[vi];
        let uv = (frag.xy - vw.rect.xy) / max(vw.rect.zw, vec2<f32>(1.0));
        let tx = min(u32(max(uv.x, 0.0) * 16.0), 15u);
        let ty = min(u32(max(uv.y, 0.0) * 9.0), 8u);
        let z = max(dot(p - vw.eye.xyz, vw.fwd.xyz), 1e-4);
        let tz = u32(clamp(floor(log2(z / lights.info.w) * lights.info.z), 0.0, 15.0));
        let c = (((u32(vi) * 16u + tz) * 9u + ty) * 16u + tx) * 8u;
        let count = clusters[c] & 0xffu;
        for (var k = 1u; k <= count; k++) {
            sum += point_light((clusters[c + k / 4u] >> ((k % 4u) * 8u)) & 0xffu, p, n);
        }
    }
    return sum;
}
";

/// Splice the light term into a forward-module source (a no-op for every
/// other shader).
pub fn patch(mut sp: Spliced) -> Spliced {
    if sp.source.contains(HOOK) {
        sp.source = sp.source.replace(HOOK, HOOKED);
        sp.source.push('\n');
        sp.source.push_str(LIGHT_WGSL);
        sp.source.push_str(PATH_WGSL);
    }
    sp
}

/// The lights bind group layouts and the forward pipeline layout that
/// carries them.
pub struct LightLayouts {
    pub empty: wgpu::BindGroupLayout,
    pub lights: wgpu::BindGroupLayout,
    pub pl_forward: wgpu::PipelineLayout,
}

impl LightLayouts {
    pub fn new(device: &wgpu::Device, frame: &wgpu::BindGroupLayout) -> Self {
        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("evani.layout.empty"), entries: &[] });
        let lights = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.lights"),
            entries: &[
                buffer(0, wgpu::BufferBindingType::Uniform),
                #[cfg(feature = "clustered")]
                buffer(1, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let pl_forward = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(frame), Some(&empty), Some(&empty), Some(&lights)],
            immediate_size: 0,
        });
        Self { empty, lights, pl_forward }
    }
}

/// The frame's lights: the uniform, its bind groups and (with `clustered`)
/// the cluster grid and its binning pipeline.
pub struct LightGpu {
    buf: wgpu::Buffer,
    empty_bg: wgpu::BindGroup,
    bg: wgpu::BindGroup,
    words: Box<[f32; WORDS]>,
    pick: Vec<usize>,
    scores: Vec<f32>,
    eyes: Vec<[f32; 3]>,
    #[cfg(feature = "clustered")]
    bin: Bin,
}

impl LightGpu {
    pub fn new(device: &wgpu::Device, layouts: &Layouts) -> Self {
        let l = &layouts.lights;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.lights"),
            size: (WORDS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(feature = "clustered")]
        let bin = Bin::new(device, &buf);
        let empty_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &l.empty, entries: &[] });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.lights"),
            layout: &l.lights,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                #[cfg(feature = "clustered")]
                wgpu::BindGroupEntry { binding: 1, resource: bin.clusters.as_entire_binding() },
            ],
        });
        Self {
            buf,
            empty_bg,
            bg,
            words: Box::new([0.0; WORDS]),
            pick: Vec::new(),
            scores: Vec::new(),
            eyes: Vec::new(),
            #[cfg(feature = "clustered")]
            bin,
        }
    }

    /// Pick this frame's lights (the tier cap, nearest the cameras first)
    /// and upload them. `views` are the forward pass's views (uniform
    /// offset, pixel rect), in the order of `rw.views` (or the one camera).
    pub fn prepare(&mut self, queue: &wgpu::Queue, world: &World, rw: &RenderWorld, views: &[(u32, [f32; 4])]) {
        let cap = (world.res::<Tiers>().active().max_point_lights as usize).min(MAX_LIGHTS);
        self.eyes.clear();
        if rw.views.is_empty() {
            self.eyes.push(rw.camera.eye);
        } else {
            self.eyes.extend(rw.views.iter().map(|v| v.camera.eye));
        }
        light::nearest(&rw.lights, &self.eyes, cap, &mut self.pick, &mut self.scores);
        let w = &mut self.words;
        w[0] = self.pick.len() as f32;
        for (dst, l) in w[LIST..].as_chunks_mut::<8>().0.iter_mut().zip(self.pick.iter().filter_map(|&i| rw.lights.get(i))) {
            let c = l.color;
            let k = l.intensity;
            *dst = [l.position[0], l.position[1], l.position[2], l.range, c[0] * k, c[1] * k, c[2] * k, 0.0];
        }
        #[cfg(feature = "clustered")]
        self.bin.views(w, rw, views, cap > MAX_LINEAR && !self.pick.is_empty());
        #[cfg(not(feature = "clustered"))]
        let _ = views;
        let used = LIST + self.pick.len() * 8;
        queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(&w[..used.min(WORDS)]));
    }

    /// Bind the lights (group 3) and empty groups 1 and 2 for the forward
    /// pass; the model draws replace group 1 (and skinned ones group 2).
    pub fn bind(&self, rp: &mut wgpu::RenderPass<'_>) {
        rp.set_bind_group(1, &self.empty_bg, &[]);
        rp.set_bind_group(2, &self.empty_bg, &[]);
        rp.set_bind_group(3, &self.bg, &[]);
    }

    /// The `lights.bin` compute pass (clustered, only when binning).
    #[cfg(feature = "clustered")]
    pub fn dispatch(&self, enc: &mut wgpu::CommandEncoder, ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        self.bin.dispatch(enc, ts);
    }
}

/// Clusters per view (16 x 9 tiles x 16 slices), words per cluster, and the
/// depth range the slices span (exponentially; the last slice runs on).
#[cfg(feature = "clustered")]
const CLUSTERS: u32 = 16 * 9 * 16;
#[cfg(feature = "clustered")]
const CLUSTER_WORDS: u64 = 8;
#[cfg(feature = "clustered")]
const ZN: f32 = 0.5;
#[cfg(feature = "clustered")]
const ZF: f32 = 300.0;

#[cfg(feature = "clustered")]
struct Bin {
    clusters: wgpu::Buffer,
    pipe: wgpu::ComputePipeline,
    bg: wgpu::BindGroup,
    /// Set if the binning pipeline failed validation: plain path from then on.
    failed: std::rc::Rc<std::cell::Cell<bool>>,
    /// Views binned this frame (0: plain path).
    binned: u32,
}

#[cfg(feature = "clustered")]
impl Bin {
    fn new(device: &wgpu::Device, lights: &wgpu::Buffer) -> Self {
        let clusters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.lights.clusters"),
            size: u64::from(CLUSTERS) * CLUSTER_WORDS * 4 * MAX_VIEWS as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.lights_bin"),
            entries: &[entry(0, wgpu::BufferBindingType::Uniform), entry(1, wgpu::BufferBindingType::Storage { read_only: false })],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("evani.lights_bin"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/cluster_bin.wgsl").into()),
        });
        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("evani.lights_bin"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("cs_main"),
            compilation_options: Default::default(),
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
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.lights_bin"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: lights.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: clusters.as_entire_binding() },
            ],
        });
        Self { clusters, pipe, bg, failed, binned: 0 }
    }

    /// Fill the header's view records (`want`: the tier allows more than
    /// the plain path's lights).
    fn views(&mut self, w: &mut [f32; WORDS], rw: &RenderWorld, views: &[(u32, [f32; 4])], want: bool) {
        let n = if want && !self.failed.get() { views.len().min(MAX_VIEWS) } else { 0 };
        w[1] = n as f32;
        w[2] = 16.0 / (ZF / ZN).log2();
        w[3] = ZN;
        for (k, (dst, &(_, px))) in w[4..LIST].as_chunks_mut::<VIEW_WORDS>().0.iter_mut().zip(views).take(n).enumerate() {
            let cam = rw.views.get(k).map_or(&rw.camera, |v| &v.camera);
            let f = crate::math::normalize(sub(cam.target, cam.eye));
            let r = crate::math::normalize(cross(f, cam.up));
            let u = cross(r, f);
            let ty = (cam.fov_y_deg.to_radians() * 0.5).tan();
            let tx = ty * px[2] / px[3].max(1.0);
            let e = cam.eye;
            dst.copy_from_slice(&[
                px[0], px[1], px[2], px[3], e[0], e[1], e[2], 0.0, r[0], r[1], r[2], tx, u[0], u[1], u[2], ty, f[0], f[1], f[2], 0.0,
            ]);
        }
        self.binned = n as u32;
    }

    fn dispatch(&self, enc: &mut wgpu::CommandEncoder, ts: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        if self.binned == 0 {
            return;
        }
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("lights.bin"), timestamp_writes: ts });
        cp.set_pipeline(&self.pipe);
        cp.set_bind_group(0, &self.bg, &[]);
        cp.dispatch_workgroups((self.binned * CLUSTERS).div_ceil(64), 1, 1);
    }
}

#[cfg(feature = "clustered")]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[cfg(feature = "clustered")]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
