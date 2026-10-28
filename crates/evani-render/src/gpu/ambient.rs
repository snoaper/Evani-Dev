//! Sky ambient on the GPU (features `ibl` and `gtao`, S5-R5; the data side
//! is `crate::ambient`).
//!
//! * The forward frame group (group 0) grows from the shadow layout's three
//!   bindings to: 0 frame uniform, 1 shadow map, 2 comparison sampler, 3 the
//!   ambient uniform (its own buffer, one for every view), 4 the sky cube
//!   (`ibl`), 5 a linear mip sampler and 6 the AO texture (`gtao`). Groups 1
//!   to 3 are all taken, so the frame group is the one that grows.
//! * The ambient term is spliced into the forward module at pipeline build
//!   ([`patch`], after the point-light patch): every lit shading line
//!   (`fs_main`, `model_fs`, `model_relief_fs`) swaps the flat
//!   `frame.ambient` for `amb_diffuse` (SH9 sky irradiance with `ibl`, times
//!   the AO with `gtao`) and, with `ibl`, adds `amb_specular` outside the
//!   base colour (a dielectric's sky reflection). Direct light never sees
//!   the AO. The WGSL files are untouched; games without the features are
//!   unchanged.
//! * Specular, split sum without a lookup table or an HDR file: the
//!   environment BRDF is the analytic mobile fit from Brian Karis, "Physically
//!   Based Shading on Mobile" (Epic Games, 2014), written here from the
//!   published formula. The environment is the analytic sky (a blurred sun
//!   lobe over the sky tint, eased into the SH9 as roughness grows) on tiers
//!   with `ibl_cube: 0`, or a small sky cube (`ibl_cube` px faces, 5 mips,
//!   Rgba16Float) prefiltered at run time with 64 GGX samples per texel
//!   (N = V = R) whenever the sky changes, on the others.

use crate::ambient::{AO_AT, AmbientParams, WORDS};
#[cfg(feature = "ibl")]
use crate::ambient::{CUBE_MIPS, Sky};
use crate::shaders::Lighting;
use crate::splice::Spliced;
use evani_core::params::ParamRegistry;

/// The flat ambient of every lit shading line, and what it becomes.
const DIFFUSE_HOOK: &str = "frame.ambient.rgb + ";
const DIFFUSE_HOOKED: &str = "amb_diffuse(n, in.pos) + ";
/// The end of the lit term (after the point lights), where the sky
/// reflection joins outside the base colour.
#[cfg(feature = "ibl")]
const SPECULAR_HOOK: &str = "frame.light_color.rgb * diffuse)";
#[cfg(feature = "ibl")]
const SPECULAR_HOOKED: &str = "frame.light_color.rgb * diffuse) + amb_specular(n, in.world, in.pos)";

/// The ambient uniform as the shaders see it (`crate::ambient::WORDS`).
const AMBIENT_WGSL: &str = r"
struct Ambient {
    sh: array<vec4<f32>, 9>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    ao: vec4<f32>,
};
";

/// The sky: tint by height (already scaled to render.ambient), and the sun
/// glow.
#[cfg(feature = "ibl")]
const SKY_WGSL: &str = r"
fn amb_sky(d: vec3<f32>) -> vec3<f32> {
    let up = clamp(d.y, -1.0, 1.0);
    let k = sqrt(abs(up));
    return select(mix(amb.horizon.rgb, amb.ground.rgb, k), mix(amb.horizon.rgb, amb.zenith.rgb, k), up >= 0.0);
}
fn amb_glow(d: vec3<f32>) -> vec3<f32> {
    return amb.sun_color.rgb * pow(max(dot(d, amb.sun.xyz), 0.0), 32.0);
}
";

/// Forward module tail: bindings shared by both features.
const FORWARD_HEAD: &str = r"
@group(0) @binding(3) var<uniform> amb: Ambient;
@group(0) @binding(5) var amb_samp: sampler;
";

#[cfg(feature = "gtao")]
const AO_WGSL: &str = r"
@group(0) @binding(6) var amb_ao: texture_2d<f32>;
fn amb_occlusion(frag: vec4<f32>) -> f32 {
    let a = textureSampleLevel(amb_ao, amb_samp, frag.xy * amb.ao.zw, 0.0).r;
    return select(1.0, mix(1.0, a, clamp(amb.ao.y, 0.0, 1.0)), amb.ao.x > 0.5);
}
";
#[cfg(not(feature = "gtao"))]
const AO_WGSL: &str = r"
fn amb_occlusion(frag: vec4<f32>) -> f32 {
    return 1.0;
}
";

#[cfg(feature = "ibl")]
const IBL_WGSL: &str = r"
@group(0) @binding(4) var amb_cube: texture_cube<f32>;
fn amb_sh(n: vec3<f32>) -> vec3<f32> {
    let r = amb.sh[0].rgb + amb.sh[1].rgb * n.y + amb.sh[2].rgb * n.z + amb.sh[3].rgb * n.x
        + amb.sh[4].rgb * (n.x * n.y) + amb.sh[5].rgb * (n.y * n.z) + amb.sh[6].rgb * (3.0 * n.z * n.z - 1.0)
        + amb.sh[7].rgb * (n.x * n.z) + amb.sh[8].rgb * (n.x * n.x - n.y * n.y);
    return max(r, vec3<f32>(0.0));
}
fn amb_diffuse(n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    return mix(frame.ambient.rgb, amb_sh(n), clamp(amb.sun_color.w, 0.0, 1.0)) * amb_occlusion(frag);
}
// Sky radiance round reflection direction r at roughness rough: the cube's
// mip, or the analytic sky with the sun lobe widened by the specular lobe
// (Phong-equivalent exponents combined, peak scaled to keep its energy),
// eased into the SH9 as the lobe approaches a cosine.
fn amb_env(r: vec3<f32>, rough: f32) -> vec3<f32> {
    if (amb.ground.w > 0.5) {
        return textureSampleLevel(amb_cube, amb_samp, r, rough * (amb.ground.w - 1.0)).rgb;
    }
    let a = max(rough * rough, 1e-3);
    let e = max(2.0 / (a * a) - 2.0, 0.0);
    let ec = 32.0 * e / (32.0 + e);
    let sun = amb.sun_color.rgb * (pow(max(dot(r, amb.sun.xyz), 0.0), max(ec, 1e-3)) * (ec + 1.0) / 33.0);
    return mix(amb_sky(r) + sun, amb_sh(r), rough);
}
// Split-sum environment BRDF, analytic fit: Brian Karis, Physically Based
// Shading on Mobile (Epic Games, 2014).
fn amb_env_brdf(f0: f32, rough: f32, ndv: f32) -> f32 {
    let r = rough * vec4<f32>(-1.0, -0.0275, -0.572, 0.022) + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}
fn amb_specular(n: vec3<f32>, world: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    let d = frame.eye.xyz - world;
    let v = d * inverseSqrt(max(dot(d, d), 1e-8));
    let ndv = clamp(dot(n, v), 1e-4, 1.0);
    let r = reflect(-v, n);
    let rough = clamp(amb.horizon.w, 0.02, 1.0);
    let k = amb_env_brdf(amb.zenith.w, rough, ndv) * max(amb.sun_color.w, 0.0) * amb_occlusion(frag);
    return amb_env(r, rough) * k;
}
";

#[cfg(not(feature = "ibl"))]
const IBL_WGSL: &str = r"
fn amb_diffuse(n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    return frame.ambient.rgb * amb_occlusion(frag);
}
";

/// The sky cube: one fullscreen triangle per face and mip (the instance is
/// face plus 6 times mip). Mip 0 is the sky itself; mip m is GGX-prefiltered at roughness
/// m / (mips - 1) from the analytic sky (64 Hammersley samples, N = V = R),
/// so no mip reads another.
#[cfg(feature = "ibl")]
const CUBE_WGSL: &str = r"
struct CubeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) k: u32,
};
@vertex
fn cube_vs(@builtin(vertex_index) i: u32, @builtin(instance_index) k: u32) -> CubeOut {
    var o: CubeOut;
    o.pos = vec4<f32>(f32((i << 1u) & 2u) * 2.0 - 1.0, f32(i & 2u) * 2.0 - 1.0, 0.0, 1.0);
    o.k = k;
    return o;
}
fn cube_dir(face: u32, s: f32, t: f32) -> vec3<f32> {
    switch face {
        case 0u: { return vec3<f32>(1.0, -t, -s); }
        case 1u: { return vec3<f32>(-1.0, -t, s); }
        case 2u: { return vec3<f32>(s, 1.0, t); }
        case 3u: { return vec3<f32>(s, -1.0, -t); }
        case 4u: { return vec3<f32>(s, -t, 1.0); }
        default: { return vec3<f32>(-s, -t, -1.0); }
    }
}
@fragment
fn cube_fs(in: CubeOut) -> @location(0) vec4<f32> {
    let face = in.k % 6u;
    let mip = in.k / 6u;
    let size = max(f32(u32(amb.sun.w) >> mip), 1.0);
    let uv = in.pos.xy / size * 2.0 - 1.0;
    let n = normalize(cube_dir(face, uv.x, uv.y));
    if (mip == 0u) {
        return vec4<f32>(amb_sky(n) + amb_glow(n), 1.0);
    }
    let rough = f32(mip) / max(amb.ground.w - 1.0, 1.0);
    let a = rough * rough;
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    var sum = vec3<f32>(0.0);
    var w = 0.0;
    for (var i = 0u; i < 64u; i++) {
        let xi = vec2<f32>(f32(i) / 64.0, f32(reverseBits(i)) * 2.3283064365386963e-10);
        let phi = 6.2831853 * xi.x;
        let ct = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
        let st = sqrt(max(1.0 - ct * ct, 0.0));
        let h = tx * (st * cos(phi)) + ty * (st * sin(phi)) + n * ct;
        let l = 2.0 * dot(n, h) * h - n;
        let ndl = dot(n, l);
        if (ndl > 0.0) {
            sum += (amb_sky(l) + amb_glow(l)) * ndl;
            w += ndl;
        }
    }
    return vec4<f32>(sum / max(w, 1e-4), 1.0);
}
";

/// Splice the ambient term into a forward-module source (a no-op for every
/// other shader). Runs after the point-light patch.
pub fn patch(mut sp: Spliced) -> Spliced {
    if sp.source.contains(DIFFUSE_HOOK) {
        sp.source = sp.source.replace(DIFFUSE_HOOK, DIFFUSE_HOOKED);
        #[cfg(feature = "ibl")]
        {
            sp.source = sp.source.replace(SPECULAR_HOOK, SPECULAR_HOOKED);
        }
        sp.source.push('\n');
        for part in [AMBIENT_WGSL, FORWARD_HEAD, AO_WGSL, IBL_WGSL] {
            sp.source.push_str(part);
        }
        #[cfg(feature = "ibl")]
        sp.source.push_str(SKY_WGSL);
    }
    sp
}

/// The forward frame group's layout (see the module header).
pub fn frame_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
    let fs = wgpu::ShaderStages::FRAGMENT;
    #[cfg(any(feature = "ibl", feature = "gtao"))]
    let tex = |view_dimension| wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension,
        multisampled: false,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("evani.layout.fwd_amb"),
        entries: &[
            entry(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            ),
            entry(
                1,
                fs,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(2, fs, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison)),
            entry(3, fs, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            #[cfg(feature = "ibl")]
            entry(4, fs, tex(wgpu::TextureViewDimension::Cube)),
            entry(5, fs, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            #[cfg(feature = "gtao")]
            entry(6, fs, tex(wgpu::TextureViewDimension::D2)),
        ],
    })
}

/// What the frame group is built from; it is rebuilt when any changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindKey {
    /// Shadow map key (0 = the 1x1 stand-in).
    pub shadow: u32,
    /// Sky cube generation (`ibl`).
    pub cube: u32,
    /// AO target generation (`gtao`).
    pub ao: u32,
}

/// The views the frame group binds besides the ring, the shadow sampler and
/// the ambient uniform.
pub struct BindViews<'a> {
    pub ring: &'a wgpu::Buffer,
    pub ring_size: u64,
    pub shadow: &'a wgpu::TextureView,
    pub cmp: &'a wgpu::Sampler,
}

/// The frame's ambient: the uniform, the sampler, the forward frame group
/// and (with `ibl`) the sky and its cube.
pub struct AmbientGpu {
    buf: wgpu::Buffer,
    words: Box<[f32; WORDS]>,
    sampler: wgpu::Sampler,
    /// The forward frame group (layout `fwd_amb`), once built.
    pub bg: Option<wgpu::BindGroup>,
    key: Option<BindKey>,
    #[cfg(feature = "ibl")]
    sky: Sky,
    #[cfg(feature = "ibl")]
    cube: Cube,
    #[cfg(feature = "gtao")]
    pub gtao: super::gtao::Gtao,
}

impl AmbientGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.ambient"),
            size: (WORDS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("evani.ambient_samp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            #[cfg(feature = "ibl")]
            cube: Cube::new(device, &buf),
            #[cfg(feature = "gtao")]
            gtao: super::gtao::Gtao::new(device),
            buf,
            words: Box::new([0.0; WORDS]),
            sampler,
            bg: None,
            key: None,
            #[cfg(feature = "ibl")]
            sky: Sky::default(),
        }
    }

    /// This frame's uniform: the sky (SH9, tints, sun; recomputed only when
    /// they change), the cube at `cube` px faces (0 = analytic) and the AO
    /// vec4.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        reg: &ParamRegistry,
        p: &AmbientParams,
        l: &Lighting,
        cube: u32,
        ao: [f32; 4],
    ) {
        #[cfg(feature = "ibl")]
        {
            self.sky.update(reg, p, l);
            self.cube.ensure(device, cube);
            self.sky.write(&mut self.words, reg, p, self.cube.size());
        }
        #[cfg(not(feature = "ibl"))]
        let _ = (device, reg, p, l, cube);
        for (d, v) in self.words[AO_AT..].iter_mut().zip(ao) {
            *d = v;
        }
        queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(&self.words[..]));
    }

    /// Redraw the sky cube if the sky changed since it was drawn. Returns
    /// the draw count.
    #[cfg(feature = "ibl")]
    pub fn draw_cube(&mut self, enc: &mut wgpu::CommandEncoder) -> u32 {
        self.cube.draw(enc, self.sky.generation)
    }

    /// (Re)build the forward frame group when the shadow map, cube or AO
    /// target changed.
    pub fn bind(&mut self, device: &wgpu::Device, layout: &wgpu::BindGroupLayout, key: BindKey, v: &BindViews<'_>) {
        #[cfg(feature = "ibl")]
        let key = BindKey { cube: self.cube.generation, ..key };
        #[cfg(feature = "gtao")]
        let key = BindKey { ao: self.gtao.generation(), ..key };
        if self.bg.is_some() && self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.bg = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.forward_ambient"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: v.ring,
                        offset: 0,
                        size: wgpu::BufferSize::new(v.ring_size),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v.shadow) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(v.cmp) },
                wgpu::BindGroupEntry { binding: 3, resource: self.buf.as_entire_binding() },
                #[cfg(feature = "ibl")]
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(self.cube.view()) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                #[cfg(feature = "gtao")]
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(self.gtao.view()) },
            ],
        }));
    }
}

/// The sky cube: its prefilter pipeline, the cube (when the tier has one)
/// or a 1x1 stand-in, and which sky generation it holds.
#[cfg(feature = "ibl")]
struct Cube {
    pipe: wgpu::RenderPipeline,
    bg: wgpu::BindGroup,
    _dummy: wgpu::Texture,
    dummy_view: wgpu::TextureView,
    tex: Option<CubeTex>,
    /// Sky generation drawn into `tex`.
    done: u32,
    /// Bumped whenever the bound view changes.
    generation: u32,
}

#[cfg(feature = "ibl")]
struct CubeTex {
    size: u32,
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    /// One render view per mip and face (index face + 6 mip).
    faces: Vec<wgpu::TextureView>,
}

#[cfg(feature = "ibl")]
fn cube_texture(device: &wgpu::Device, size: u32, mips: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("evani.sky_cube"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

#[cfg(feature = "ibl")]
fn cube_view(tex: &wgpu::Texture) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::Cube), ..Default::default() })
}

#[cfg(feature = "ibl")]
impl Cube {
    fn new(device: &wgpu::Device, buf: &wgpu::Buffer) -> Self {
        let source = format!("{AMBIENT_WGSL}\n@group(0) @binding(0) var<uniform> amb: Ambient;\n{SKY_WGSL}{CUBE_WGSL}");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("evani.sky_cube"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.sky_cube"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("evani.sky_cube"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("cube_vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("cube_fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.sky_cube"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
        });
        let dummy = cube_texture(device, 1, 1);
        let dummy_view = cube_view(&dummy);
        Self { pipe, bg, _dummy: dummy, dummy_view, tex: None, done: 0, generation: 1 }
    }

    /// The cube at `size` px faces (0 = none: the analytic sky).
    fn ensure(&mut self, device: &wgpu::Device, size: u32) {
        let size = if size == 0 { 0 } else { size.clamp(8, 512) };
        if self.size() == size {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.done = 0;
        self.tex = (size > 0).then(|| {
            let tex = cube_texture(device, size, CUBE_MIPS);
            let faces = (0..CUBE_MIPS * 6)
                .map(|k| {
                    tex.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_mip_level: k / 6,
                        mip_level_count: Some(1),
                        base_array_layer: k % 6,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect();
            CubeTex { size, view: cube_view(&tex), _tex: tex, faces }
        });
    }

    fn size(&self) -> u32 {
        self.tex.as_ref().map_or(0, |t| t.size)
    }

    fn view(&self) -> &wgpu::TextureView {
        self.tex.as_ref().map_or(&self.dummy_view, |t| &t.view)
    }

    fn draw(&mut self, enc: &mut wgpu::CommandEncoder, sky: u32) -> u32 {
        let Some(t) = self.tex.as_ref() else {
            return 0;
        };
        if self.done == sky {
            return 0;
        }
        self.done = sky;
        let mut draws = 0;
        for (k, face) in (0u32..).zip(&t.faces) {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky_cube"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: face,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipe);
            rp.set_bind_group(0, &self.bg, &[]);
            rp.draw(0..3, k..k + 1);
            draws += 1;
        }
        draws
    }
}
