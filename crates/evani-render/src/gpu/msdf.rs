//! The GPU side of MSDF text (feature `msdf`): fetch each requested font's
//! `fonts/<name>.evf` table and `fonts/<name>.png` atlas, decode the PNG in
//! the browser (`createImageBitmap`, no premultiply, no colour conversion:
//! the channels are distances, not colours), stack every font's atlas in
//! one `Rgba8Unorm` texture and tell [`crate::msdf::Fonts`] where each
//! landed. The sprite pass binds it as group 1.
//!
//! Before any font arrives the texture is a 1 x 1 placeholder. A lost
//! device builds a new `GpuMsdf`, which fetches the fonts again (the
//! browser cache answers).

use std::cell::RefCell;
use std::rc::Rc;

use evani_core::ecs::World;
use evani_core::overlay::ErrorLog;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::msdf::{MAX_FONTS, MsdfFont};
use crate::world::RenderWorld;

/// Group 1 of the sprite pipeline: the font atlas and its sampler.
pub struct MsdfLayouts {
    pub bgl: wgpu::BindGroupLayout,
    /// The sprite pipeline's layout: group 0 (layout A), group 1.
    pub pl_sprite: wgpu::PipelineLayout,
}

impl MsdfLayouts {
    pub fn new(device: &wgpu::Device, a: &wgpu::BindGroupLayout) -> Self {
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evani.layout.msdf"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl_sprite = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("evani.layout.sprite_msdf"),
            bind_group_layouts: &[Some(a), Some(&bgl)],
            immediate_size: 0,
        });
        Self { bgl, pl_sprite }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Load {
    Idle,
    Fetching,
    Done,
}

type Inbox = Rc<RefCell<Vec<(usize, Result<(MsdfFont, web_sys::ImageBitmap), String>)>>>;

pub struct GpuMsdf {
    pub bg: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    load: [Load; MAX_FONTS],
    /// Decoded atlases by slot, kept to rebuild the stacked texture.
    bitmaps: Vec<(usize, web_sys::ImageBitmap)>,
    inbox: Inbox,
}

fn bind(device: &wgpu::Device, layouts: &MsdfLayouts, tex: &wgpu::Texture, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("evani.msdf"),
        layout: &layouts.bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("evani.msdf.atlas"),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Distances, not colours: never sRGB.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

/// Fetch one file; a failed request or an HTTP error status is an error
/// (the status code as the value).
async fn get(win: &web_sys::Window, url: &str) -> Result<web_sys::Response, JsValue> {
    let resp: web_sys::Response = JsFuture::from(win.fetch_with_str(url)).await?.unchecked_into();
    if resp.ok() { Ok(resp) } else { Err(resp.status().into()) }
}

/// Fetch the table and the decoded atlas of one font. Kept small (one
/// error type, no formatting) because every `ui` game downloads it.
async fn fetch_raw(name: &str) -> Result<(Vec<u8>, web_sys::ImageBitmap), JsValue> {
    let win = web_sys::window().ok_or(JsValue::NULL)?;
    let base = ["fonts/", name].concat();
    let evf = get(&win, &[&base, ".evf"].concat()).await?;
    let evf = js_sys::Uint8Array::new(&JsFuture::from(evf.array_buffer()?).await?).to_vec();
    let png = get(&win, &[&base, ".png"].concat()).await?;
    let blob: web_sys::Blob = JsFuture::from(png.blob()?).await?.unchecked_into();
    let opts = web_sys::ImageBitmapOptions::new();
    opts.set_premultiply_alpha(web_sys::PremultiplyAlpha::None);
    opts.set_color_space_conversion(web_sys::ColorSpaceConversion::None);
    let bmp = JsFuture::from(win.create_image_bitmap_with_blob_and_image_bitmap_options(&blob, &opts)?).await?;
    Ok((evf, bmp.unchecked_into()))
}

/// Fetch, check and decode one font.
async fn fetch_font(name: String) -> Result<(MsdfFont, web_sys::ImageBitmap), String> {
    let (evf, bmp) = fetch_raw(&name).await.map_err(evani_core::web::js_err)?;
    let font = MsdfFont::parse(&evf)?;
    if [bmp.width(), bmp.height()] != font.atlas {
        return Err(crate::msdf::evf_err!("10", "the atlas PNG's size differs from its table").into());
    }
    Ok((font, bmp))
}

impl GpuMsdf {
    pub fn new(device: &wgpu::Device, layouts: &MsdfLayouts) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("evani.msdf.linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bg = bind(device, layouts, &texture(device, 1, 1), &sampler);
        Self { bg, sampler, load: [Load::Idle; MAX_FONTS], bitmaps: Vec::new(), inbox: Rc::default() }
    }

    /// Start fetches for newly requested fonts, take in arrived ones and
    /// rebuild the stacked atlas when one did.
    pub fn frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layouts: &MsdfLayouts, rw: &mut RenderWorld, world: &World) {
        for (i, slot) in rw.fonts.slots_mut().iter_mut().enumerate() {
            if let Some(state) = self.load.get_mut(i)
                && *state == Load::Idle
            {
                *state = Load::Fetching;
                // Not on this device yet (a new one after a loss, too).
                slot.placement = None;
                let (name, inbox) = (slot.name.clone(), self.inbox.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    let got = fetch_font(name).await;
                    inbox.borrow_mut().push((i, got));
                });
            }
        }
        let arrived: Vec<_> = self.inbox.borrow_mut().drain(..).collect();
        if arrived.is_empty() {
            return;
        }
        for (i, got) in arrived {
            if let Some(state) = self.load.get_mut(i) {
                *state = Load::Done;
            }
            let Some(slot) = rw.fonts.slots_mut().get_mut(i) else { continue };
            match got {
                Ok((font, bmp)) => {
                    slot.font = Some(font);
                    self.bitmaps.push((i, bmp));
                }
                Err(e) => world.res_mut::<ErrorLog>().error(["font ", &slot.name, ": ", &e].concat()),
            }
        }
        // Stack every atlas top to bottom in one texture.
        let w = self.bitmaps.iter().map(|(_, b)| b.width()).max().unwrap_or(1);
        let h: u32 = self.bitmaps.iter().map(|(_, b)| b.height()).sum();
        let tex = texture(device, w, h);
        let (fw, fh) = (w.max(1) as f32, h.max(1) as f32);
        let mut y = 0;
        for (i, bmp) in &self.bitmaps {
            let size = wgpu::Extent3d { width: bmp.width(), height: bmp.height(), depth_or_array_layers: 1 };
            queue.copy_external_image_to_texture(
                &wgpu::CopyExternalImageSourceInfo {
                    source: wgpu::ExternalImageSource::ImageBitmap(bmp.clone()),
                    origin: wgpu::Origin2d::ZERO,
                    flip_y: false,
                },
                wgpu::CopyExternalImageDestInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                    color_space: wgpu::PredefinedColorSpace::Srgb,
                    premultiplied_alpha: false,
                },
                size,
            );
            if let Some(slot) = rw.fonts.slots_mut().get_mut(*i) {
                slot.placement = Some([0.0, y as f32 / fh, 1.0 / fw, 1.0 / fh]);
            }
            y += bmp.height();
        }
        self.bg = bind(device, layouts, &tex, &self.sampler);
    }
}
