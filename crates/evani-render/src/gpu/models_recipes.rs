//! GPU side of catalogue material recipes (feature `material-recipes`
//! without `relief`; the CPU side is `crate::model::recipes` and the crate
//! `evani-material-recipes`): the material group's two extra bindings (12 the
//! material's 32-byte block, 13 the shared 128-byte per-frame recipe uniform:
//! condition amounts, the eight quality levels, sim time, the off bits, the
//! extras deny bits and the condition layers' look numbers) and the buffers
//! behind them. Live switches move only binding 13, and a kit's
//! [`BlockEdits`] rewrite one binding 12, so a slider never rebuilds a
//! pipeline. The condition on switches and quality limit rewrite each block's
//! condition mask byte, only when they change (`conditions::apply`), so the
//! shader never sorts. Kept in its own file so the model pipeline's
//! file only gains same-line, cfg'd additions.

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use evani_core::params::ParamRegistry;
use evani_core::tiers::Tiers;
use evani_core::time::Time;
use evani_material_recipes::conditions::{ACTIVE, MaskRule, apply};
use evani_material_recipes::params::{RecipeParams, default_frame, frame_words, mask_rule};
use evani_material_recipes::shader::FRAME_BYTES;

use crate::model::ModelId;
use crate::model::recipes::{BLOCK_WORDS, BlockEdits, stage_state};

/// A material's block buffer and the block it was given.
type KeptBlock = (wgpu::Buffer, [u32; BLOCK_WORDS]);

/// What the recipe stage keeps beside the model pipeline: the frame uniform
/// and each model's block buffers (for live edits and condition masks).
pub(super) struct RecipesGpu {
    pub(super) frame: wgpu::Buffer,
    /// By model id (ids are small and dense), then material index: the
    /// buffer and the block as the model (or the last live edit) gave it.
    blocks: Vec<Vec<Option<KeptBlock>>>,
    /// The condition mask rule the buffers were last written with.
    rule: Option<MaskRule>,
    /// A block was added since the buffers were last rewritten.
    dirty: bool,
    logged: bool,
    /// By model id, then material index: the character kind (with the
    /// recipe code, which material pipeline it draws with).
    #[cfg(feature = "material-split")]
    kinds: Vec<Vec<u8>>,
}

fn uniform_buffer(dev: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
    dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl RecipesGpu {
    pub(super) fn new(dev: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let frame = uniform_buffer(dev, "evani.model.recipes_frame", FRAME_BYTES as u64);
        queue.write_buffer(
            &frame,
            0,
            bytemuck::cast_slice(&default_frame("medium", 0.0)),
        );
        Self {
            frame,
            blocks: Vec::new(),
            rule: None,
            dirty: false,
            logged: false,
            #[cfg(feature = "material-split")]
            kinds: Vec::new(),
        }
    }

    /// Remember material `material`'s character kind of model `model`.
    #[cfg(feature = "material-split")]
    pub(super) fn keep_kind(&mut self, model: ModelId, material: usize, kind: u8) {
        let m = model.0 as usize;
        if self.kinds.len() <= m {
            self.kinds.resize_with(m + 1, Vec::new);
        }
        if let Some(v) = self.kinds.get_mut(m) {
            if material == 0 {
                v.clear();
            }
            if v.len() <= material {
                v.resize(material + 1, 0);
            }
            if let Some(k) = v.get_mut(material) {
                *k = kind;
            }
        }
    }

    /// The (recipe code, character kind) material `material` of model
    /// `model` draws with now (its block as the model or the last live edit
    /// gave it; a material with no block is recipe 0).
    #[cfg(feature = "material-split")]
    pub(super) fn split_of(&self, model: usize, material: usize) -> (u8, u8) {
        let code = self.blocks.get(model).and_then(|v| v.get(material)).and_then(Option::as_ref).map_or(0, |b| evani_material_recipes::split::key_code((b.1[0] & 0xff) as u8));
        let kind = self.kinds.get(model).and_then(|v| v.get(material)).copied().unwrap_or(0);
        (code, kind)
    }

    /// This frame's recipe uniform (the live switches on the active tier, sim
    /// time from the fixed step), the queued block edits, the condition masks
    /// when their rule moved, and a one-time note if the stage fell back.
    /// The frame holds a shared borrow of the param registry while models
    /// prepare, so this only reads it (the family quality mirror, which
    /// writes, is the crate's own Update system).
    pub(super) fn write_frame(&mut self, queue: &wgpu::Queue, world: &World) {
        let tier = world
            .try_res::<Tiers>()
            .map_or_else(|| "medium".to_string(), |t| t.active().id.clone());
        let sim = world.try_res::<Time>().map_or(0.0, |t| t.sim_seconds);
        let (words, rule) = match (
            world.try_res::<ParamRegistry>(),
            world.try_res::<RecipeParams>(),
        ) {
            (Some(reg), Some(p)) => {
                (frame_words(&reg, &p, &tier, sim), ACTIVE.then(|| mask_rule(&reg, &p, &tier)))
            }
            _ => (default_frame(&tier, sim), None),
        };
        // The style detail step's word (kd.w, reserved in the plain stage).
        #[cfg(feature = "material-recipe-style")]
        let words = {
            use evani_material_recipes::style::{RecipeStyle, STYLE_WORD};
            let mut w = words;
            w[STYLE_WORD] = world.try_res::<RecipeStyle>().map_or(0, |s| s.0);
            w
        };
        queue.write_buffer(&self.frame, 0, bytemuck::cast_slice(&words));
        if let Some(mut edits) = world.try_res_mut::<BlockEdits>() {
            for (model, material, w) in edits.take() {
                if let Some((b, orig)) = self
                    .blocks
                    .get_mut(model.0 as usize)
                    .and_then(|v| v.get_mut(material))
                    .and_then(Option::as_mut)
                {
                    *orig = w;
                    queue.write_buffer(b, 0, bytemuck::cast_slice(&effective(&w, rule.as_ref())));
                }
            }
        }
        if rule.is_some() && (self.dirty || rule != self.rule) {
            for (b, orig) in self.blocks.iter().flatten().flatten() {
                queue.write_buffer(b, 0, bytemuck::cast_slice(&effective(orig, rule.as_ref())));
            }
            self.rule = rule;
            self.dirty = false;
        }
        if !self.logged && stage_state() == Some(false) {
            self.logged = true;
            if let Some(mut log) = world.try_res_mut::<ErrorLog>() {
                log.note("material recipes: patch line missing; plain PBR");
            }
        }
    }

    /// Remember material `material`'s block buffer of model `model` and its
    /// block `words` (live edits and condition masks find them here; a reload
    /// replaces them).
    pub(super) fn keep(&mut self, model: ModelId, material: usize, b: &wgpu::Buffer, words: &[u32; BLOCK_WORDS]) {
        let m = model.0 as usize;
        if self.blocks.len() <= m {
            self.blocks.resize_with(m + 1, Vec::new);
        }
        let Some(v) = self.blocks.get_mut(m) else {
            return;
        };
        if v.len() <= material {
            v.resize(material + 1, None);
        }
        if let Some(slot) = v.get_mut(material) {
            *slot = Some((b.clone(), *words));
            self.dirty = true;
        }
    }

    /// A material's block (binding 12), its condition mask under the last
    /// rule (the next frame rewrites it if the rule moved).
    pub(super) fn material_buffer(
        &self,
        dev: &wgpu::Device,
        queue: &wgpu::Queue,
        words: &[u32; BLOCK_WORDS],
    ) -> wgpu::Buffer {
        let b = uniform_buffer(dev, "evani.model.recipe", (BLOCK_WORDS * 4) as u64);
        queue.write_buffer(&b, 0, bytemuck::cast_slice(&effective(words, self.rule.as_ref())));
        b
    }
}

/// `words` with the condition mask `rule` gives it (as they are without one,
/// or with no condition layer compiled in).
fn effective(words: &[u32; BLOCK_WORDS], rule: Option<&MaskRule>) -> [u32; BLOCK_WORDS] {
    match rule {
        Some(r) if ACTIVE => apply(words, r),
        _ => *words,
    }
}

/// The cut-out's depth pipeline (batch 3 B3-P2, `material-recipes-cut` with
/// `shadows`): the shadow map, its cascades and the AO depth prepass draw an
/// alpha-masked material's parts through `model_cut_shadows.wgsl`, which drops
/// what the colour pass drops (the texture's alpha under the cutoff, a cut-out
/// block's holes), so light comes through them. Every other part keeps the
/// depth-only pipeline.
#[cfg(all(feature = "shadows", any(feature = "material-recipes-cut", feature = "material-recipes-blend")))]
pub(super) struct CutShadows {
    pub(super) pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// By model id, then material index: a masked material's group 1 (its
    /// material uniform, base map, sampler, block and the frame uniform),
    /// `None` for the rest.
    groups: Vec<Vec<Option<wgpu::BindGroup>>>,
}

#[cfg(all(feature = "shadows", any(feature = "material-recipes-cut", feature = "material-recipes-blend")))]
impl CutShadows {
    pub(super) fn new(dev: &wgpu::Device, shadow_frame: &wgpu::BindGroupLayout) -> Self {
        use super::models_surface::uniform;
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.model_cut_shadows"),
            entries: &[
                uniform(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                uniform(12),
                uniform(13),
            ],
        });
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("evani.model_cut_shadows"),
            bind_group_layouts: &[Some(shadow_frame), Some(&layout)],
            immediate_size: 0,
        });
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("evani.model_cut_shadows"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::model_cut_shadows().into()),
        });
        // The model vertex: the position (offset 0) and uv (offset 16) of
        // its four attributes; the instance: rows, position scale and offset
        // (4 to 8) and the uv scale and offset (9).
        const VERTEX: [wgpu::VertexAttribute; 2] = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm16x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm16x2, offset: 16, shader_location: 3 },
        ];
        const INSTANCE: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4];
        let buffers = [
            Some(wgpu::VertexBufferLayout { array_stride: crate::model::VERTEX_BYTES as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &VERTEX }),
            Some(wgpu::VertexBufferLayout { array_stride: (crate::model::INSTANCE_WORDS * 4) as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &INSTANCE }),
        ];
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("evani.model_cut_shadows"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("model_cut_shadow_vs"), compilation_options: Default::default(), buffers: &buffers },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("model_cut_shadow_fs"), compilation_options: Default::default(), targets: &[] }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, layout, groups: Vec::new() }
    }

    /// Material `material` of model `model` as uploaded: its group 1 when it
    /// is alpha-masked (`mask`: glTF alphaMode MASK, which every cut-out row
    /// is; material 0 starts the model afresh, so a reloaded model never keeps
    /// an old one).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn keep(&mut self, dev: &wgpu::Device, frame: &wgpu::Buffer, model: ModelId, material: usize, mask: bool, ub: &wgpu::Buffer, base: &wgpu::TextureView, sampler: &wgpu::Sampler, block: &wgpu::Buffer) {
        let m = model.0 as usize;
        if self.groups.len() <= m {
            self.groups.resize_with(m + 1, Vec::new);
        }
        let Some(v) = self.groups.get_mut(m) else {
            return;
        };
        if material == 0 {
            v.clear();
        }
        if v.len() <= material {
            v.resize(material + 1, None);
        }
        let g = mask.then(|| {
            dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("evani.model.cut"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(base) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 12, resource: block.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 13, resource: frame.as_entire_binding() },
                ],
            })
        });
        if let Some(slot) = v.get_mut(material) {
            *slot = g;
        }
    }

    /// The cut group of material `material` of model `model`, if it cuts.
    pub(super) fn group(&self, model: usize, material: usize) -> Option<&wgpu::BindGroup> {
        self.groups.get(model).and_then(|v| v.get(material)).and_then(Option::as_ref)
    }
}
