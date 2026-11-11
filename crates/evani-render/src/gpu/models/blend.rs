//! The see-through pass of catalogue materials (batch 3 B3-P3, feature
//! `material-recipes-blend`; the refraction copy with
//! `material-recipes-refract`). Its own file, so the model pipeline's file
//! only gains same-line, cfg'd additions and no line of it moves for games
//! without it.
//!
//! - A material whose glTF alphaMode is BLEND carries its mode in the free
//!   material word 15 (`model::recipes::blend_words`: over, add or refract,
//!   from its block) and its sort priority in word 19 (`extras.evani_sort`).
//! - The opaque pass skips those parts; after the opaque models and the world
//!   particles, the forward pass draws them through the blend pipeline (the
//!   same patched module with `REC_BLEND = 1`: premultiplied alpha, depth
//!   test on, depth writes off, destination alpha kept so the tone map's
//!   screen-door marks survive), one draw per instance, ordered by priority,
//!   then far to near by the instance's centre (per object: enough for panes,
//!   cards and emitters), then by run and instance so ties never flicker.
//! - Refract-mode glass (frosted, wavy, block) is left for the glass pass,
//!   after the forward pass: the opaque colour is copied into a half-size
//!   texture with three mips (two downsample passes more), only on a frame
//!   that has such glass, and the glass pipeline (one sample, its own module
//!   with `mat_refract`, group 2 the copy) draws it over the resolved colour
//!   with the resolved depth. Glass does not see other refracting glass.
//! - Shadows: a blended part goes through the hole-aware depth pipeline (its
//!   stipple, `blend_shadow.wgsl`); the AO depth prepass skips it.

#[cfg(any(feature = "shadows", feature = "material-recipes-refract"))]
use core::cell::Cell;
#[cfg(feature = "material-recipes-refract")]
use core::cell::RefCell;

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_material_recipes::blend::{REFRACT, SortKey, sort_far_to_near};
use evani_material_recipes::params::RecipeParams;
use evani_material_recipes::split::{BLEND, GLASS};

use super::super::pipelines::{Layouts, Pipelines};
use super::ModelGpu;
use crate::model::INSTANCE_WORDS;
use crate::world::RenderWorld;

/// The copy of the opaque colour's format (HDR, so a bright sky refracts
/// bright on every tier).
#[cfg(feature = "material-recipes-refract")]
const COPY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The copy's mips (half size, quarter, eighth).
#[cfg(feature = "material-recipes-refract")]
const COPY_MIPS: u32 = 3;
/// Frames without refracting glass before the copy is freed.
#[cfg(feature = "material-recipes-refract")]
const COPY_IDLE: u32 = 240;

/// One see-through draw this frame: run, instance and mode.
#[derive(Clone, Copy, Debug)]
struct Item {
    run: u32,
    instance: u32,
    mode: u8,
}

/// The copy of the opaque colour and its groups.
#[cfg(feature = "material-recipes-refract")]
struct CopyTex {
    size: [u32; 2],
    _tex: wgpu::Texture,
    /// Each mip as a render target.
    mips: [wgpu::TextureView; COPY_MIPS as usize],
    /// Mip 0 and 1 as the next downsample's source.
    src: [wgpu::BindGroup; 2],
    /// Group 2 of the glass pipeline: every mip and the sampler.
    glass: wgpu::BindGroup,
}

/// What the see-through pass keeps beside the model pipeline.
pub(in crate::gpu) struct BlendGpu {
    /// By model id, then material index: (mode, sort priority).
    modes: Vec<Vec<(u8, i32)>>,
    /// `material.recipe.blend.enabled`: off draws every blended part opaque.
    on: bool,
    /// This frame's see-through draws, in draw order.
    order: Vec<Item>,
    /// This frame has refracting glass and the refraction is on.
    glass: bool,
    /// The AO depth prepass is drawing (it skips blended parts).
    #[cfg(feature = "shadows")]
    pub(super) ao: Cell<bool>,
    #[cfg(feature = "material-recipes-refract")]
    idle: Cell<u32>,
    #[cfg(feature = "material-recipes-refract")]
    copy: RefCell<Option<CopyTex>>,
    #[cfg(feature = "material-recipes-refract")]
    blit: (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler),
}

/// The copy's sampled group (texture, filtering sampler).
#[cfg(feature = "material-recipes-refract")]
fn copy_layout(dev: &wgpu::Device) -> wgpu::BindGroupLayout {
    dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("evani.layout.model_copy"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    })
}

/// The downsample pass: a full-screen triangle, one bilinear tap at the
/// centre of each target texel (the box average of the 2 x 2 below it).
#[cfg(feature = "material-recipes-refract")]
const BLIT_WGSL: &str = "@group(0) @binding(0) var t: texture_2d<f32>;\n@group(0) @binding(1) var s: sampler;\nstruct O { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };\n@vertex fn vs(@builtin(vertex_index) i: u32) -> O { let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)); return O(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv); }\n@fragment fn fs(o: O) -> @location(0) vec4<f32> { return vec4<f32>(min(textureSampleLevel(t, s, o.uv, 0.0).rgb, vec3<f32>(6.0e4)), 1.0); }\n";

impl BlendGpu {
    pub(in crate::gpu) fn new(dev: &wgpu::Device) -> Self {
        #[cfg(not(feature = "material-recipes-refract"))]
        let _ = dev;
        Self {
            modes: Vec::new(),
            on: true,
            order: Vec::new(),
            glass: false,
            #[cfg(feature = "shadows")]
            ao: Cell::new(false),
            #[cfg(feature = "material-recipes-refract")]
            idle: Cell::new(0),
            #[cfg(feature = "material-recipes-refract")]
            copy: RefCell::new(None),
            #[cfg(feature = "material-recipes-refract")]
            blit: blit_pipeline(dev),
        }
    }

    /// Material `material` of model `model` as uploaded: its blend mode and
    /// sort priority (material words 15 and 19; material 0 starts the model
    /// afresh, so a reloaded model keeps nothing old).
    pub(super) fn keep(&mut self, model: crate::model::ModelId, material: usize, mode: f32, priority: f32) {
        let m = model.0 as usize;
        if self.modes.len() <= m {
            self.modes.resize_with(m + 1, Vec::new);
        }
        let Some(v) = self.modes.get_mut(m) else {
            return;
        };
        if material == 0 {
            v.clear();
        }
        if v.len() <= material {
            v.resize(material + 1, (0, 0));
        }
        if let Some(slot) = v.get_mut(material) {
            *slot = ((mode.round().clamp(0.0, 3.0)) as u8, priority.round() as i32);
        }
    }

    /// The blend mode of a material (0: opaque).
    pub(super) fn mode(&self, model: usize, material: usize) -> u8 {
        self.modes.get(model).and_then(|v| v.get(material)).map_or(0, |m| m.0)
    }

}

/// The blend (`see` 1) and glass (`see` 2) pipelines are the model pipeline
/// (`models::pipeline`, `see` 0) with these differences: the module's
/// `REC_BLEND` constant is 1 ([`options`]); the glass pipeline's group 2 is
/// the copy ([`copy_for`], [`groups`]); premultiplied alpha over what is there
/// (add is the same state with alpha 0), the destination's alpha kept
/// ([`state`]); depth tested and never written; the glass pipeline has one
/// sample. No culling either way: the shader drops back faces of single-sided
/// parts.
///
/// The copy's group layout for the glass pipeline (`see` 2), else none.
pub(super) fn copy_for(dev: &wgpu::Device, see: u8) -> Option<wgpu::BindGroupLayout> {
    #[cfg(feature = "material-recipes-refract")]
    if see == 2 {
        return Some(copy_layout(dev));
    }
    let _ = (dev, see);
    None
}

/// The pipeline layout's groups: the model pipeline's, with the copy as
/// group 2 for glass.
pub(super) fn groups<'a>(frame: &'a wgpu::BindGroupLayout, mat: &'a wgpu::BindGroupLayout, copy: &'a Option<wgpu::BindGroupLayout>, layouts: &'a Layouts, see: u8) -> Vec<Option<&'a wgpu::BindGroupLayout>> {
    let mut g = vec![Some(frame), Some(mat)];
    if see == 2 {
        g.push(copy.as_ref());
    }
    #[cfg(feature = "point-lights")]
    {
        if see != 2 {
            g.push(Some(&layouts.lights.empty));
        }
        g.push(Some(&layouts.lights.lights));
    }
    #[cfg(not(feature = "point-lights"))]
    let _ = layouts;
    g
}

/// `REC_BLEND = 1` for the blend and glass pipelines.
pub(super) fn options(see: u8) -> wgpu::PipelineCompilationOptions<'static> {
    wgpu::PipelineCompilationOptions { constants: if see == 0 { &[] } else { &[("REC_BLEND", 1.0)] }, ..Default::default() }
}

/// The blend state of the blend and glass pipelines; the model pipeline's
/// has none.
pub(super) fn state(see: u8) -> Option<wgpu::BlendState> {
    (see != 0).then_some(wgpu::BlendState {
        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
    })
}

#[cfg(feature = "material-recipes-refract")]
fn blit_pipeline(dev: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler) {
    let bgl = copy_layout(dev);
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("evani.model_copy"), source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()) });
    let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("evani.model_copy"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
    let pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("evani.model_copy"),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(COPY_FORMAT.into())] }),
        multiview_mask: None,
        cache: None,
    });
    let sampler = dev.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("evani.model_copy"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    (pipe, bgl, sampler)
}

/// The copy of a `w` x `h` colour target, at half size.
#[cfg(feature = "material-recipes-refract")]
fn make_copy(dev: &wgpu::Device, blit: &(wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler), size: [u32; 2]) -> CopyTex {
    let tex = dev.create_texture(&wgpu::TextureDescriptor {
        label: Some("evani.model_copy"),
        size: wgpu::Extent3d { width: size[0].div_ceil(2).max(1), height: size[1].div_ceil(2).max(1), depth_or_array_layers: 1 },
        mip_level_count: COPY_MIPS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COPY_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mip = |m: u32| tex.create_view(&wgpu::TextureViewDescriptor { base_mip_level: m, mip_level_count: Some(1), ..Default::default() });
    let mips = [mip(0), mip(1), mip(2)];
    let group = |v: &wgpu::TextureView| {
        dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.model_copy"),
            layout: &blit.1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&blit.2) }],
        })
    };
    let all = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let src = [group(&mips[0]), group(&mips[1])];
    let glass = group(&all);
    CopyTex { size, _tex: tex, mips, src, glass }
}

/// A render pass of the glass work: `view` loaded or cleared as `load` says
/// and stored, `depth` (if any) loaded and stored.
#[cfg(feature = "material-recipes-refract")]
fn begin<'e>(enc: &'e mut wgpu::CommandEncoder, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>, depth: Option<&wgpu::TextureView>, ts: Option<wgpu::RenderPassTimestampWrites<'_>>) -> wgpu::RenderPass<'e> {
    let keep = wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store };
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("glass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment { view, depth_ops: Some(keep), stencil_ops: None }),
        timestamp_writes: ts,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

impl ModelGpu {
    /// This frame's see-through order (after the runs are staged): the live
    /// switches, then each blended run's instances by priority, far to near
    /// and by id. Skinned parts keep the opaque path.
    pub(super) fn blend_frame(&mut self, world: &World, rw: &RenderWorld) {
        let mut sort = true;
        #[cfg_attr(not(feature = "material-recipes-refract"), allow(unused_mut))]
        let mut refract = cfg!(feature = "material-recipes-refract");
        if let (Some(reg), Some(p)) = (world.try_res::<ParamRegistry>(), world.try_res::<RecipeParams>()) {
            self.blend.on = reg.bool(p.blend.0);
            sort = reg.bool(p.blend.1);
            #[cfg(feature = "material-recipes-refract")]
            {
                refract = reg.f32(p.refraction) > 0.0;
            }
        }
        let eye = rw.camera.eye;
        let mut keys = Vec::new();
        for (ri, r) in self.runs.iter().enumerate() {
            let Some(p) = self.ready.get(r.model).and_then(Option::as_ref).and_then(|m| m.parts.get(r.part)) else {
                continue;
            }; #[cfg(feature = "skinning")] if p.sb.is_some() { continue; }
            let mode = self.blend.mode(r.model, p.part.material);
            if mode == 0 {
                continue;
            }
            let priority = self.blend.modes.get(r.model).and_then(|v| v.get(p.part.material)).map_or(0, |m| m.1);
            for k in 0..r.count {
                let i = (r.first + k) as usize * INSTANCE_WORDS;
                let Some(w) = self.staging.get(i..i + INSTANCE_WORDS) else {
                    continue;
                };
                // The part's centre in the world: its box's middle through the
                // instance rows.
                let c = [0, 1, 2].map(|a| w[16 + a] + w[12 + a] * 0.5);
                let at = [0, 1, 2].map(|row| w[row * 4] * c[0] + w[row * 4 + 1] * c[1] + w[row * 4 + 2] * c[2] + w[row * 4 + 3]);
                let d2 = (0..3).map(|a| (at[a] - eye[a]) * (at[a] - eye[a])).sum::<f32>();
                // The id is the draw itself (run, instance, mode); runs and instances
                // come in order, so it also breaks ties as they were staged.
                keys.push(SortKey { priority, dist2: if sort { d2 } else { 0.0 }, id: (ri as u32, r.first + k, u32::from(mode)) });
            }
        }
        sort_far_to_near(&mut keys);
        self.blend.order.clear();
        self.blend.order.extend(keys.iter().map(|k| Item { run: k.id.0, instance: k.id.1, mode: k.id.2 as u8 }));
        self.blend.glass = refract && self.blend.order.iter().any(|it| it.mode == REFRACT);
        #[cfg(feature = "material-recipes-refract")]
        {
            let idle = if self.blend.glass { 0 } else { self.blend.idle.get().saturating_add(1) };
            self.blend.idle.set(idle);
            if idle > COPY_IDLE {
                self.blend.copy.replace(None);
            }
        }
    }

    /// The see-through draws of the forward pass, after the opaque models and
    /// the world particles, view by view (refracting glass is left to
    /// [`ModelGpu::glass_pass`] when it runs).
    pub(in crate::gpu) fn draw_blend(&self, rp: &mut wgpu::RenderPass<'_>, frame_bg: &wgpu::BindGroup, views: &[(u32, [f32; 4])], ms: bool, pipes: &Pipelines, tris: &mut u32) -> u32 {
        if !self.blend.on || self.blend.order.is_empty() {
            return 0;
        }
        let glass = self.glass_now(pipes);
        rp.set_vertex_buffer(1, self.instances.slice(..));
        let mut draws = 0;
        for &(off, px) in views {
            rp.set_bind_group(0, frame_bg, &[off]);
            rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
            // Refracting glass whose glass pipeline is in is left to the glass pass.
            draws += self.blend_items(rp, pipes, BLEND, ms, |it, (code, kind)| !(glass && it.mode == REFRACT && pipes.material_ready(GLASS, false, code, kind)), tris);
        }
        draws
    }

    /// Material (recipe code, character kind) of a see-through item.
    fn item_split(&self, it: &Item) -> Option<(u8, u8)> {
        let r = self.runs.get(it.run as usize)?;
        let p = self.ready.get(r.model).and_then(Option::as_ref)?.parts.get(r.part)?;
        Some(self.recipes.split_of(r.model, p.part.material))
    }

    /// Whether the opaque pass leaves this material to its blend pipeline
    /// (see-through, the pass on and that material's blend pipeline built;
    /// until then it draws opaque, as before).
    pub(super) fn blend_skips(&self, model: usize, material: usize, pipes: &Pipelines, ms: bool) -> bool {
        if !self.blend.on || self.blend.mode(model, material) == 0 {
            return false;
        }
        let (code, kind) = self.recipes.split_of(model, material);
        pipes.material(BLEND, ms, code, kind).is_some()
    }

    /// Whether this frame draws the glass pass: refracting glass, the
    /// refraction on, and at least one of its glass pipelines built (each
    /// refracting material asks for its own).
    pub(in crate::gpu) fn glass_now(&self, pipes: &Pipelines) -> bool {
        if !self.blend.on || !self.blend.glass {
            return false;
        }
        let mut any = false;
        for it in self.blend.order.iter().filter(|it| it.mode == REFRACT) {
            if let Some((code, kind)) = self.item_split(it) {
                any |= pipes.material(GLASS, false, code, kind).is_some();
            }
        }
        any
    }

    /// Draw the order's items `keep` lets through, one instance each, each
    /// with its material's pipeline of pass `see` (an item whose pipeline
    /// is not built yet is skipped: it drew opaque).
    #[allow(clippy::too_many_arguments)]
    fn blend_items(&self, rp: &mut wgpu::RenderPass<'_>, pipes: &Pipelines, see: u8, ms: bool, keep: impl Fn(&Item, (u8, u8)) -> bool, tris: &mut u32) -> u32 {
        let mut draws = 0;
        let mut cur: Option<&wgpu::RenderPipeline> = None;
        for it in &self.blend.order {
            let Some(r) = self.runs.get(it.run as usize) else {
                continue;
            };
            let Some(gm) = self.ready.get(r.model).and_then(Option::as_ref) else {
                continue;
            };
            let Some(p) = gm.parts.get(r.part) else {
                continue;
            };
            let Some(mat) = gm.materials.get(p.part.material) else {
                continue;
            };
            let (code, kind) = self.recipes.split_of(r.model, p.part.material);
            if !keep(it, (code, kind)) {
                continue;
            }
            let Some(pipe) = pipes.material(see, ms, code, kind) else {
                continue;
            };
            if !cur.is_some_and(|c| core::ptr::eq(c, pipe)) {
                rp.set_pipeline(pipe);
                cur = Some(pipe);
            }
            rp.set_bind_group(1, mat, &[]);
            rp.set_vertex_buffer(0, p.vb.slice(..));
            rp.set_index_buffer(p.ib.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..p.count, 0, it.instance..it.instance + 1);
            draws += 1;
            *tris = tris.saturating_add(p.count / 3);
        }
        draws
    }

    /// The AO depth prepass is drawing (`true`) or done (`false`): it skips
    /// blended parts, which would darken what is behind them.
    #[cfg(feature = "gtao")]
    pub(in crate::gpu) fn blend_ao(&self, on: bool) {
        self.blend.ao.set(on);
    }

    /// The glass pass (feature `material-recipes-refract`): copy `hdr` (the
    /// resolved opaque colour, `size` pixels) into the half-size copy and its
    /// mips, then draw the refracting glass over `hdr` with the one-sample
    /// `depth`, far to near, view by view. Returns the draws.
    #[cfg(feature = "material-recipes-refract")]
    #[allow(clippy::too_many_arguments)]
    pub(in crate::gpu) fn glass_pass(
        &self,
        dev: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        hdr: &wgpu::TextureView,
        size: [u32; 2],
        depth: &wgpu::TextureView,
        frame_bg: &wgpu::BindGroup,
        views: &[(u32, [f32; 4])],
        pipes: &Pipelines,
        lights: Option<&wgpu::BindGroup>,
        timer: &mut super::super::timing::Timer,
        tris: &mut u32,
    ) -> u32 {
        {
            let mut c = self.blend.copy.borrow_mut();
            if c.as_ref().is_none_or(|c| c.size != size) {
                *c = Some(make_copy(dev, &self.blend.blit, size));
            }
        }
        let copy = self.blend.copy.borrow();
        let Some(copy) = copy.as_ref() else {
            return 0;
        };
        let (blit, bgl, sampler) = &self.blend.blit;
        let from_hdr = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("evani.model_copy.hdr"),
            layout: bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(hdr) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) }],
        });
        let mut draws = 0;
        for m in 0..COPY_MIPS as usize {
            let src = if m == 0 { &from_hdr } else { &copy.src[m - 1] };
            let ts = if m == 0 { timer.pass("glass.copy") } else { None };
            let mut rp = begin(enc, &copy.mips[m], wgpu::LoadOp::Clear(wgpu::Color::BLACK), None, ts);
            rp.set_pipeline(blit);
            rp.set_bind_group(0, src, &[]);
            rp.draw(0..3, 0..1);
            draws += 1;
        }
        let ts = timer.pass("glass");
        let mut rp = begin(enc, hdr, wgpu::LoadOp::Load, Some(depth), ts);
        rp.set_vertex_buffer(1, self.instances.slice(..));
        rp.set_bind_group(2, &copy.glass, &[]);
        if let Some(l) = lights {
            rp.set_bind_group(3, l, &[]);
        }
        for &(off, px) in views {
            rp.set_bind_group(0, frame_bg, &[off]);
            rp.set_viewport(px[0], px[1], px[2], px[3], 0.0, 1.0);
            draws += self.blend_items(&mut rp, pipes, GLASS, false, |it, _| it.mode == REFRACT, tris);
        }
        draws
    }
}

/// The swatch balls (feature `swatches`) draw see-through materials opaque:
/// set the pass on or off and return what it was. Declared last.
#[cfg(feature = "swatches")]
impl ModelGpu {
    pub(super) fn swatch_blend(&mut self, on: bool) -> bool {
        core::mem::replace(&mut self.blend.on, on)
    }
}
