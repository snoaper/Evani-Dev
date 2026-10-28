//! Ground-truth ambient occlusion (feature `gtao`, S5-R5). All fragment
//! passes, so no compute or storage limits are involved:
//!
//! 1. Depth prepass at AO resolution (the frame's shadow depth pipeline with
//!    each view's matrix flipped to conventional depth; `frame/ambient.rs`).
//! 2. Linear view depth into an R32Float chain of [`MIPS`] mips; mips 1 to 4
//!    use XeGTAO's weighted 2x2 depth filter.
//! 3. The AO pass: XeGTAO's slice integration (slices x steps by tier), the
//!    normal rebuilt from depth, into R8Unorm.
//! 4. One depth-aware 3x3 denoise into the AO texture the forward pass
//!    samples (`gpu::ambient`, binding 6). AO darkens only the ambient term.
//!
//! Tier ladder (`TierPreset::gtao`): 0 off, 1 half resolution with 1 slice x
//! 2 steps, 2 full resolution 2 x 2, 3 full resolution 3 x 3.
//!
//! The structure follows Bevy's SSAO (bevy_pbr/src/ssao, MIT OR Apache-2.0,
//! the Bevy contributors), itself a port of XeGTAO; the WGSL here is our own
//! fragment-shader version of XeGTAO's maths. XeGTAO's notice, as its licence
//! asks:
//!
//! Copyright (C) 2016-2021, Intel Corporation
//!
//! SPDX-License-Identifier: MIT
//!
//! Permission is hereby granted, free of charge, to any person obtaining a
//! copy of this software and associated documentation files (the
//! "Software"), to deal in the Software without restriction, including
//! without limitation the rights to use, copy, modify, merge, publish,
//! distribute, sublicense, and/or sell copies of the Software, and to permit
//! persons to whom the Software is furnished to do so, subject to the
//! following conditions:
//!
//! The above copyright notice and this permission notice shall be included
//! in all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//! OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
//! NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
//! DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
//! USE OR OTHER DEALINGS IN THE SOFTWARE.

/// Mips of the linear depth chain.
const MIPS: u32 = 5;
/// Views the AO passes cover (the uniform has one slot each).
pub const MAX_VIEWS: usize = 4;
/// Uniform slot stride (the dynamic offset alignment) and the bound window.
const SLOT: usize = 256;
const BOUND: u64 = 64;
/// Floats per slot.
const SLOT_WORDS: usize = SLOT / 4;

/// Shared by every pass: the per-view uniform, then the depth, the linear
/// depth (one mip, or the chain) and an AO texture. Uniform: `rect` the
/// view in AO pixels; `cam` (tan half fov x, y, effect radius in metres,
/// falloff fraction); `q` (slices, steps, A, B with linear depth
/// A / (B - d)); `e` (sky depth, final power).
const COMMON_WGSL: &str = r"
struct G { rect: vec4<f32>, cam: vec4<f32>, q: vec4<f32>, e: vec4<f32>, };
@group(0) @binding(0) var<uniform> g: G;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var lin: texture_2d<f32>;
@group(0) @binding(3) var raw: texture_2d<f32>;
@vertex
fn gtao_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32((i << 1u) & 2u) * 2.0 - 1.0, f32(i & 2u) * 2.0 - 1.0, 0.0, 1.0);
}
fn clamp_px(p: vec2<i32>, dims: vec2<u32>) -> vec2<i32> {
    return clamp(p, vec2<i32>(0), vec2<i32>(dims) - 1);
}
@fragment
fn lin_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, clamp_px(vec2<i32>(frag.xy), textureDimensions(depth)), 0);
    return vec4<f32>(g.q.z / max(g.q.w - d, 1e-7), 0.0, 0.0, 1.0);
}
@fragment
fn mip_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = textureDimensions(lin);
    let b = vec2<i32>(frag.xy) * 2;
    let d = vec4<f32>(
        textureLoad(lin, clamp_px(b, dims), 0).r,
        textureLoad(lin, clamp_px(b + vec2<i32>(1, 0), dims), 0).r,
        textureLoad(lin, clamp_px(b + vec2<i32>(0, 1), dims), 0).r,
        textureLoad(lin, clamp_px(b + vec2<i32>(1, 1), dims), 0).r);
    let mx = max(max(d.x, d.y), max(d.z, d.w));
    let r = 0.75 * g.cam.z;
    let fr = max(g.cam.w * r, 1e-4);
    let w = clamp((vec4<f32>(mx) - d) * (-1.0 / fr) + (r * (1.0 - g.cam.w) / fr + 1.0), vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(dot(w, d) / max(w.x + w.y + w.z + w.w, 1e-4), 0.0, 0.0, 1.0);
}
";

/// The AO pass (XeGTAO's main pass: FastACos, slice horizons with distance
/// falloff, the visibility integral against the projected normal) and the
/// denoise (3x3, tent weights times a depth-difference weight). View space:
/// x right, y up, z forward (XeGTAO's convention), camera at the origin.
const AO_WGSL: &str = r"
const PI: f32 = 3.1415927;
const HALF_PI: f32 = 1.5707964;
fn fast_acos(x: f32) -> f32 {
    let a = abs(x);
    let r = (-0.156583 * a + HALF_PI) * sqrt(max(1.0 - a, 0.0));
    return select(PI - r, r, x >= 0.0);
}
fn view_pos(px: vec2<f32>, z: f32) -> vec3<f32> {
    let uv = (px - g.rect.xy) / g.rect.zw;
    return vec3<f32>((uv.x * 2.0 - 1.0) * g.cam.x * z, (1.0 - uv.y * 2.0) * g.cam.y * z, z);
}
fn lin_at(p: vec2<i32>, mip: i32) -> f32 {
    let dims = vec2<i32>(textureDimensions(lin, mip));
    return textureLoad(lin, clamp(p >> vec2<u32>(u32(mip)), vec2<i32>(0), dims - 1), mip).r;
}
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.982918 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}
@fragment
fn ao_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let lo = vec2<i32>(g.rect.xy);
    let hi = max(vec2<i32>(g.rect.xy + g.rect.zw) - 1, lo);
    let pix = clamp(vec2<i32>(frag.xy), lo, hi);
    let z = lin_at(pix, 0);
    if (z >= g.e.x) {
        return vec4<f32>(1.0);
    }
    let radius = g.cam.z;
    let screen_r = radius * g.rect.z / (2.0 * g.cam.x * z);
    if (screen_r < 1.3) {
        return vec4<f32>(1.0);
    }
    let pos = view_pos(frag.xy, z);
    let v = normalize(-pos);
    let zl = lin_at(clamp(pix - vec2<i32>(1, 0), lo, hi), 0);
    let zr = lin_at(clamp(pix + vec2<i32>(1, 0), lo, hi), 0);
    let zt = lin_at(clamp(pix - vec2<i32>(0, 1), lo, hi), 0);
    let zb = lin_at(clamp(pix + vec2<i32>(0, 1), lo, hi), 0);
    let dx = select(pos - view_pos(frag.xy - vec2<f32>(1.0, 0.0), zl), view_pos(frag.xy + vec2<f32>(1.0, 0.0), zr) - pos, abs(zr - z) < abs(zl - z));
    let dy = select(pos - view_pos(frag.xy - vec2<f32>(0.0, 1.0), zt), view_pos(frag.xy + vec2<f32>(0.0, 1.0), zb) - pos, abs(zb - z) < abs(zt - z));
    let c = cross(dx, dy);
    var n = select(v, c * inverseSqrt(max(dot(c, c), 1e-30)), dot(c, c) > 1e-30);
    n = select(n, -n, dot(n, pos) > 0.0);
    let fr = max(g.cam.w * radius, 1e-4);
    let fall_mul = -1.0 / fr;
    let fall_add = radius * (1.0 - g.cam.w) / fr + 1.0;
    let min_s = 1.3 / screen_r;
    let noise_slice = ign(frag.xy);
    let noise_step = ign(frag.xy + vec2<f32>(5.588238));
    let slices = u32(g.q.x);
    let steps = u32(g.q.y);
    var vis = 0.0;
    for (var s = 0u; s < slices; s++) {
        let phi = (f32(s) + noise_slice) / g.q.x * PI;
        let omega = vec2<f32>(cos(phi), -sin(phi)) * screen_r;
        let dir = vec3<f32>(cos(phi), sin(phi), 0.0);
        let ortho = dir - dot(dir, v) * v;
        let axis = normalize(cross(ortho, v));
        let pn = n - axis * dot(n, axis);
        let pn_len = length(pn);
        let cos_n = clamp(dot(pn, v) / max(pn_len, 1e-6), 0.0, 1.0);
        let na = sign(dot(ortho, pn)) * fast_acos(cos_n);
        let low0 = cos(na + HALF_PI);
        let low1 = cos(na - HALF_PI);
        var hc0 = low0;
        var hc1 = low1;
        for (var t = 0u; t < steps; t++) {
            var st = (f32(t) + fract(noise_step + f32(s + t * steps) * 0.618034)) / g.q.y;
            st = st * st + min_s;
            let o = st * omega;
            let mip = i32(round(clamp(log2(length(o)) - 3.3, 0.0, 4.0)));
            let ro = round(o);
            let s0 = frag.xy + ro;
            let s1 = frag.xy - ro;
            let d0 = view_pos(s0, lin_at(clamp(vec2<i32>(floor(s0)), lo, hi), mip)) - pos;
            let d1 = view_pos(s1, lin_at(clamp(vec2<i32>(floor(s1)), lo, hi), mip)) - pos;
            let l0 = length(d0);
            let l1 = length(d1);
            let w0 = clamp(l0 * fall_mul + fall_add, 0.0, 1.0);
            let w1 = clamp(l1 * fall_mul + fall_add, 0.0, 1.0);
            hc0 = max(hc0, mix(low0, dot(d0, v) / max(l0, 1e-6), w0));
            hc1 = max(hc1, mix(low1, dot(d1, v) / max(l1, 1e-6), w1));
        }
        let h0 = na + max(-fast_acos(hc1) - na, -HALF_PI);
        let h1 = na + min(fast_acos(hc0) - na, HALF_PI);
        let sn = sin(na);
        let a0 = (cos_n + 2.0 * h0 * sn - cos(2.0 * h0 - na)) * 0.25;
        let a1 = (cos_n + 2.0 * h1 * sn - cos(2.0 * h1 - na)) * 0.25;
        vis += mix(pn_len, 1.0, 0.05) * (a0 + a1);
    }
    vis = clamp(vis / g.q.x, 0.0, 1.0);
    return vec4<f32>(max(pow(vis, g.e.y), 0.03), 0.0, 0.0, 1.0);
}
@fragment
fn den_fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = textureDimensions(raw);
    let p = clamp_px(vec2<i32>(frag.xy), dims);
    let zc = textureLoad(lin, p, 0).r;
    var sum = 0.0;
    var wsum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let q = clamp_px(p + vec2<i32>(x, y), dims);
            let dz = abs(textureLoad(lin, q, 0).r - zc);
            let w = f32((2 - abs(x)) * (2 - abs(y))) * max(0.0, 1.0 - dz / (0.05 * zc + 0.01));
            sum += textureLoad(raw, q, 0).r * w;
            wsum += w;
        }
    }
    return vec4<f32>(sum / max(wsum, 1e-4), 0.0, 0.0, 1.0);
}
";

/// The AO targets at one resolution, with their seven bind groups.
struct Targets {
    size: [u32; 2],
    _tex: [wgpu::Texture; 4],
    depth: wgpu::TextureView,
    /// One render view per linear-depth mip.
    lin_mips: Vec<wgpu::TextureView>,
    raw: wgpu::TextureView,
    fin: wgpu::TextureView,
    /// Linearize, mips 1 to 4, AO, denoise.
    bgs: Vec<wgpu::BindGroup>,
}

/// The GTAO passes, their targets (while the tier has AO) and this frame's
/// views.
pub struct Gtao {
    layout: wgpu::BindGroupLayout,
    lin_pipe: wgpu::RenderPipeline,
    mip_pipe: wgpu::RenderPipeline,
    ao_pipe: wgpu::RenderPipeline,
    den_pipe: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    words: Box<[f32; SLOT_WORDS * MAX_VIEWS]>,
    _dummy: wgpu::Texture,
    dummy_view: wgpu::TextureView,
    t: Option<Targets>,
    generation: u32,
    /// This frame's views: the prepass uniform's ring offset and the
    /// viewport in AO pixels. Empty when AO is off.
    pub views: Vec<(u32, [f32; 4])>,
}

fn texture(device: &wgpu::Device, size: [u32; 2], mips: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("evani.gtao"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn mip_view(tex: &wgpu::Texture, mip: u32) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor { base_mip_level: mip, mip_level_count: Some(1), ..Default::default() })
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.gtao"),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("gtao_vs"), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// One fullscreen pass into `target` (cleared to 1 = unoccluded / far).
fn pass<'e>(enc: &'e mut wgpu::CommandEncoder, target: &wgpu::TextureView) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("gtao"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

impl Gtao {
    pub fn new(device: &wgpu::Device) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let unfilterable = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.gtao"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(BOUND),
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(2, unfilterable),
                entry(3, unfilterable),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("evani.gtao"),
            source: wgpu::ShaderSource::Wgsl(format!("{COMMON_WGSL}{AO_WGSL}").into()),
        });
        let r32 = wgpu::TextureFormat::R32Float;
        let r8 = wgpu::TextureFormat::R8Unorm;
        let dummy = texture(device, [1, 1], 1, r8);
        let dummy_view = dummy.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            lin_pipe: pipeline(device, &pl, &module, "lin_fs", r32),
            mip_pipe: pipeline(device, &pl, &module, "mip_fs", r32),
            ao_pipe: pipeline(device, &pl, &module, "ao_fs", r8),
            den_pipe: pipeline(device, &pl, &module, "den_fs", r8),
            layout,
            ubuf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("evani.gtao"),
                size: (SLOT * MAX_VIEWS) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            words: Box::new([0.0; SLOT_WORDS * MAX_VIEWS]),
            _dummy: dummy,
            dummy_view,
            t: None,
            generation: 1,
            views: Vec::with_capacity(MAX_VIEWS),
        }
    }

    /// The AO texture the forward pass samples (a 1x1 stand-in while off).
    pub fn view(&self) -> &wgpu::TextureView {
        self.t.as_ref().map_or(&self.dummy_view, |t| &t.fin)
    }

    /// Bumped whenever [`Self::view`] changes.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The prepass depth target, while AO is on.
    pub fn depth_view(&self) -> Option<&wgpu::TextureView> {
        self.t.as_ref().map(|t| &t.depth)
    }

    /// AO off this frame: drop the targets.
    pub fn off(&mut self) {
        self.views.clear();
        if self.t.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Targets at `size` AO pixels (rebuilt when it changes).
    pub fn ensure(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let size = [size[0].max(1), size[1].max(1)];
        if self.t.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let depth_tex = texture(device, size, 1, wgpu::TextureFormat::Depth32Float);
        // The chain is at least 16 px a side, so all five mips exist; the
        // AO textures match the AO resolution exactly (the forward pass
        // samples them by screen fraction).
        let lin_tex = texture(device, [size[0].max(16), size[1].max(16)], MIPS, wgpu::TextureFormat::R32Float);
        let raw_tex = texture(device, size, 1, wgpu::TextureFormat::R8Unorm);
        let fin_tex = texture(device, size, 1, wgpu::TextureFormat::R8Unorm);
        let depth = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lin_all = lin_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lin_mips: Vec<_> = (0..MIPS).map(|m| mip_view(&lin_tex, m)).collect();
        let raw = raw_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let fin = fin_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = |lin: &wgpu::TextureView, ao: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("evani.gtao"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.ubuf,
                            offset: 0,
                            size: wgpu::BufferSize::new(BOUND),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(lin) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(ao) },
                ],
            })
        };
        // A pass never binds what it draws into: linearize reads nothing of
        // the chain (mip 4 stands in), mip m reads mip m - 1, the AO pass
        // reads the whole chain (the final AO stands in at 3), the denoise
        // reads the raw AO.
        let mut bgs = Vec::with_capacity(MIPS as usize + 2);
        if let Some(last) = lin_mips.last() {
            bgs.push(bind(last, &raw));
        }
        for prev in lin_mips.iter().take(MIPS as usize - 1) {
            bgs.push(bind(prev, &raw));
        }
        bgs.push(bind(&lin_all, &fin));
        bgs.push(bind(&lin_all, &raw));
        self.t = Some(Targets { size, _tex: [depth_tex, lin_tex, raw_tex, fin_tex], depth, lin_mips, raw, fin, bgs });
    }

    /// View `i`'s uniform slot.
    pub fn set_view(&mut self, i: usize, slot: [[f32; 4]; 4]) {
        if let Some(dst) = self.words.get_mut(i * SLOT_WORDS..i * SLOT_WORDS + 16) {
            dst.copy_from_slice(slot.as_flattened());
        }
    }

    pub fn upload(&self, queue: &wgpu::Queue) {
        let n = self.views.len().max(1) * SLOT_WORDS;
        queue.write_buffer(&self.ubuf, 0, bytemuck::cast_slice(&self.words[..n]));
    }

    /// The passes after the depth prepass. Returns the draw count.
    pub fn draw(&self, enc: &mut wgpu::CommandEncoder) -> u32 {
        let Some(t) = self.t.as_ref() else {
            return 0;
        };
        if self.views.is_empty() {
            return 0;
        }
        let mut draws = 0;
        for (m, target) in t.lin_mips.iter().enumerate() {
            let mut rp = pass(enc, target);
            rp.set_pipeline(if m == 0 { &self.lin_pipe } else { &self.mip_pipe });
            if let Some(bg) = t.bgs.get(m) {
                rp.set_bind_group(0, bg, &[0]);
                rp.draw(0..3, 0..1);
                draws += 1;
            }
        }
        if let Some(bg) = t.bgs.get(MIPS as usize) {
            let mut rp = pass(enc, &t.raw);
            rp.set_pipeline(&self.ao_pipe);
            for (i, &(_, r)) in self.views.iter().enumerate() {
                rp.set_viewport(r[0], r[1], r[2], r[3], 0.0, 1.0);
                rp.set_bind_group(0, bg, &[(i * SLOT) as u32]);
                rp.draw(0..3, 0..1);
                draws += 1;
            }
        }
        if let Some(bg) = t.bgs.get(MIPS as usize + 1) {
            let mut rp = pass(enc, &t.fin);
            rp.set_pipeline(&self.den_pipe);
            rp.set_bind_group(0, bg, &[0]);
            rp.draw(0..3, 0..1);
            draws += 1;
        }
        draws
    }
}
