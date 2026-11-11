//! Material swatch balls (feature `swatches`; the Asset & Style Studio's
//! material library only): small offscreen renders of a model through the
//! engine's own material pipeline, read back as pixels for the page.
//!
//! - **What the app does.** It adds its ball models once ([`Swatches::add_model`]:
//!   a swatch model is never drawn in the main view, nor into its shadow map),
//!   draws each one at [`AT`] every frame it waits for a picture (so its
//!   instance is staged), and asks for a picture with [`Swatches::request`].
//!   Finished pictures come back from [`Swatches::take_done`]: `size` x
//!   `size` RGBA8, sRGB, top row first.
//! - **What the renderer does** (`gpu/frame/swatch.rs`, browser only). Each
//!   frame it takes at most [`PER_FRAME`] requests whose model is on the GPU
//!   ([`Swatches::uploads`] reached the request's) with its material
//!   pipelines built (a material still compiling would draw the plain
//!   stand-in, never the real thing), draws each into its cell of a small
//!   offscreen target with the frame's own light (the scene's light preset,
//!   sky and exposure) and the forward pass's sample count, tonemaps it with
//!   the scene's tonemap pass, copies the cells into a mappable buffer and
//!   hands the pixels back a frame or two later. Nothing waits on the GPU.
//! - **Model pictures.** The same pass draws any small model, not only a
//!   ball: [`frame_box`] frames a model's box from a yaw and pitch (the
//!   studio's asset tiles: the asset in a material and style), and a
//!   request may name its own [`SwatchRequest::background`] so the picture
//!   sits on the page's card. A frame takes only requests sharing the
//!   oldest one's background (one clear per pass).
//! - **Cost.** One draw per part of each ball, two passes and a 128 KB copy
//!   on frames that take requests; nothing on the others. The targets are
//!   [`PER_FRAME`] cells of [`MAX_SIZE`] px (about 1.5 MB with 4x MSAA) and
//!   are made on first use.

use evani_core::camera::CameraPose;

use crate::model::ModelId;

/// The largest picture a request may ask for (px, square).
pub const MAX_SIZE: u32 = 128;
/// Requests drawn per frame at most (the cells of the offscreen target).
pub const PER_FRAME: usize = 2;
/// Copies waiting for the GPU at most; new requests wait while they are out.
pub const IN_FLIGHT: usize = 2;
/// Where swatch models stand: far from any scene, so the scene's shadow map,
/// blob shadows and lamps never reach them (the frame's sun, sky and
/// exposure still do).
pub const AT: [f32; 3] = [4000.0, 1.0, 4000.0];
/// The ball's share of its picture's width: the edge keeps a hair of the
/// clear colour so a round crop never cuts the ball.
pub const FILL: f32 = 0.97;
/// The swatch camera's vertical field of view (degrees): long, so the ball
/// is barely distorted.
pub const FOV_Y_DEG: f32 = 22.0;

/// One picture asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct SwatchRequest {
    /// The app's key for the picture (it comes back with it).
    pub key: u64,
    pub model: ModelId,
    /// Draw only once the model's GPU uploads reach this count (the app
    /// notes [`Swatches::uploads`] + 1 when it swaps the model's bytes).
    pub uploads: u32,
    pub camera: CameraPose,
    /// Square size in px (clamped to 8..=[`MAX_SIZE`]).
    pub size: u32,
    /// The colour behind the model (linear, before the tonemap; `None`: the
    /// default dark panel the balls are cut from).
    pub background: Option<[f32; 3]>,
}

/// One finished picture.
#[derive(Debug, Clone, PartialEq)]
pub struct SwatchImage {
    pub key: u64,
    pub size: u32,
    /// `size * size * 4` bytes, RGBA8 sRGB, top row first.
    pub rgba: Vec<u8>,
}

/// The swatch queue (a world resource the app inserts).
#[derive(Debug, Default)]
pub struct Swatches {
    models: Vec<ModelId>,
    uploads: Vec<(ModelId, u32)>,
    queue: Vec<SwatchRequest>,
    done: Vec<SwatchImage>,
    failed: Vec<(u64, String)>,
    /// Pictures drawn so far.
    pub drawn: u64,
    /// Frames that drew at least one.
    pub busy_frames: u64,
}

impl Swatches {
    /// Mark `id` as a swatch model: drawn only into swatch pictures.
    pub fn add_model(&mut self, id: ModelId) {
        if !self.models.contains(&id) {
            self.models.push(id);
        }
    }

    pub fn is_swatch(&self, index: usize) -> bool {
        self.models.iter().any(|m| m.0 as usize == index)
    }

    pub fn models(&self) -> &[ModelId] {
        &self.models
    }

    /// Times the model's bytes reached the GPU.
    pub fn uploads(&self, id: ModelId) -> u32 {
        self.uploads.iter().find(|u| u.0 == id).map_or(0, |u| u.1)
    }

    /// The GPU side put `id`'s bytes on the GPU (a swatch model only).
    pub fn note_upload(&mut self, index: usize) {
        let Some(&id) = self.models.iter().find(|m| m.0 as usize == index) else { return };
        match self.uploads.iter_mut().find(|u| u.0 == id) {
            Some(u) => u.1 += 1,
            None => self.uploads.push((id, 1)),
        }
    }

    /// Ask for a picture (a queued one with the same key is replaced).
    pub fn request(&mut self, mut r: SwatchRequest) {
        r.size = r.size.clamp(8, MAX_SIZE);
        self.queue.retain(|q| q.key != r.key);
        self.queue.push(r);
    }

    /// Drop the queued requests of `model` (the app is about to swap it).
    pub fn cancel(&mut self, model: ModelId) {
        self.queue.retain(|q| q.model != model);
    }

    /// Requests waiting to be drawn.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn take_done(&mut self) -> Vec<SwatchImage> {
        core::mem::take(&mut self.done)
    }

    /// Requests that could not be drawn, and why.
    pub fn take_failed(&mut self) -> Vec<(u64, String)> {
        core::mem::take(&mut self.failed)
    }

    /// The finished pictures whose key `mine` claims; the others stay for
    /// another user of the pass (the balls and the asset tiles share it).
    pub fn take_done_where(&mut self, mine: impl Fn(u64) -> bool) -> Vec<SwatchImage> {
        let (out, rest) = core::mem::take(&mut self.done).into_iter().partition(|i| mine(i.key));
        self.done = rest;
        out
    }

    /// The same for the requests that could not be drawn.
    pub fn take_failed_where(&mut self, mine: impl Fn(u64) -> bool) -> Vec<(u64, String)> {
        let (out, rest) = core::mem::take(&mut self.failed).into_iter().partition(|f| mine(f.0));
        self.failed = rest;
        out
    }

    /// The renderer takes up to `max` requests that `ready` says can be
    /// drawn now, oldest first, all on the first one's background (one clear
    /// per pass); the rest wait.
    pub fn take_ready(&mut self, max: usize, mut ready: impl FnMut(&SwatchRequest, u32) -> bool) -> Vec<SwatchRequest> {
        let mut out: Vec<SwatchRequest> = Vec::new();
        let mut i = 0;
        while i < self.queue.len() && out.len() < max {
            let have = self.uploads(self.queue[i].model);
            let same = out.first().is_none_or(|f| f.background == self.queue[i].background);
            if same && ready(&self.queue[i], have) {
                out.push(self.queue.remove(i));
            } else {
                i += 1;
            }
        }
        out
    }

    pub fn complete(&mut self, img: SwatchImage) {
        self.drawn += 1;
        self.done.push(img);
    }

    pub fn fail(&mut self, key: u64, why: impl Into<String>) {
        self.failed.push((key, why.into()));
    }
}

/// The camera that frames a ball of `radius` standing at [`AT`] so its
/// outline fills [`FILL`] of a square picture, seen from `toward` (a
/// direction from the ball to the eye; a little above the equator reads as
/// a ball on a shelf).
pub fn camera(radius: f32, toward: [f32; 3]) -> CameraPose {
    let half = (FOV_Y_DEG.to_radians() * 0.5).tan();
    // The outline's half-angle a for a ball at distance d: sin a = r / d,
    // and its projected radius tan a / tan(fov / 2) = FILL.
    let d = radius / (FILL * half).atan().sin();
    let n = (toward[0] * toward[0] + toward[1] * toward[1] + toward[2] * toward[2]).sqrt().max(1e-6);
    let eye = [AT[0] + toward[0] / n * d, AT[1] + toward[1] / n * d, AT[2] + toward[2] / n * d];
    CameraPose { eye, target: AT, up: [0.0, 1.0, 0.0], fov_y_deg: FOV_Y_DEG }
}

/// The camera that frames a model's box `b` (model units) standing at
/// [`AT`], seen from `yaw` and `pitch` (degrees, the orbit's convention: yaw
/// 0 looks from +z), as close as the box allows: every corner projects
/// within `fill` of the picture's half-size, and the widest one touches it.
pub fn frame_box(b: [[f32; 3]; 2], yaw: f32, pitch: f32, fill: f32) -> CameraPose {
    let centre: [f32; 3] = core::array::from_fn(|k| AT[k] + (b[0][k] + b[1][k]) * 0.5);
    let half: [f32; 3] = core::array::from_fn(|k| ((b[1][k] - b[0][k]) * 0.5).max(1e-3));
    let (az, el) = (yaw.to_radians(), pitch.to_radians());
    // Toward the eye, the picture's right and up.
    let back = [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()];
    let right = [az.cos(), 0.0, -az.sin()];
    let up = [back[1] * right[2] - back[2] * right[1], back[2] * right[0] - back[0] * right[2], back[0] * right[1] - back[1] * right[0]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let t = (FOV_Y_DEG.to_radians() * 0.5).tan() * fill.clamp(0.1, 1.0);
    let mut d = 0.0f32;
    for i in 0..8 {
        let o = [if i & 1 == 0 { -half[0] } else { half[0] }, if i & 2 == 0 { -half[1] } else { half[1] }, if i & 4 == 0 { -half[2] } else { half[2] }];
        // The corner at depth d - z must show within t of the centre line.
        let z = dot(o, back);
        d = d.max(z + dot(o, right).abs().max(dot(o, up).abs()) / t);
    }
    let eye = core::array::from_fn(|k| centre[k] + back[k] * d);
    CameraPose { eye, target: centre, up: [0.0, 1.0, 0.0], fov_y_deg: FOV_Y_DEG }
}

/// The projected outline radius of a ball of `radius` seen by `cam`, as a
/// share of half the picture (1: it touches the edges).
pub fn outline(radius: f32, cam: &CameraPose) -> f32 {
    let d = (0..3).map(|k| (cam.eye[k] - cam.target[k]).powi(2)).sum::<f32>().sqrt();
    (radius / d).clamp(-1.0, 1.0).asin().tan() / (cam.fov_y_deg.to_radians() * 0.5).tan()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(key: u64, model: u32, uploads: u32) -> SwatchRequest {
        SwatchRequest { key, model: ModelId(model), uploads, camera: camera(1.0, [0.3, 0.4, 1.0]), size: 400, background: None }
    }

    /// The camera puts the ball's outline at FILL of the picture, from the
    /// side asked for.
    #[test]
    fn the_camera_frames_the_ball() {
        for r in [0.5, 1.0, 3.0] {
            let c = camera(r, [0.2, 0.35, 1.0]);
            assert!((outline(r, &c) - FILL).abs() < 1e-4, "{}", outline(r, &c));
            assert_eq!(c.target, AT);
            assert!(c.eye[1] > AT[1] && c.eye[2] > AT[2]);
        }
    }

    /// Requests wait for their model's upload, replace the same key, are
    /// taken a few at a time, oldest first, and sizes stay in range.
    #[test]
    fn requests_wait_for_their_model_and_are_taken_a_few_a_frame() {
        let mut s = Swatches::default();
        s.add_model(ModelId(3));
        s.add_model(ModelId(3));
        s.add_model(ModelId(4));
        assert_eq!(s.models(), &[ModelId(3), ModelId(4)]);
        assert!(s.is_swatch(3) && !s.is_swatch(0));
        s.request(req(1, 3, 1));
        s.request(req(2, 4, 1));
        s.request(req(3, 3, 1));
        s.request(req(1, 3, 1));
        assert_eq!(s.pending(), 3);
        assert_eq!(s.queue.iter().map(|q| q.key).collect::<Vec<_>>(), [2, 3, 1]);
        assert_eq!(s.queue[0].size, MAX_SIZE);
        let ready = |r: &SwatchRequest, have: u32| have >= r.uploads;
        assert!(s.take_ready(PER_FRAME, ready).is_empty(), "nothing is uploaded yet");
        s.note_upload(3);
        s.note_upload(0);
        assert_eq!((s.uploads(ModelId(3)), s.uploads(ModelId(0))), (1, 0));
        let got: Vec<u64> = s.take_ready(1, ready).iter().map(|r| r.key).collect();
        assert_eq!(got, [3]);
        s.note_upload(4);
        let got: Vec<u64> = s.take_ready(PER_FRAME, ready).iter().map(|r| r.key).collect();
        assert_eq!(got, [2, 1]);
        s.request(req(9, 4, 2));
        s.cancel(ModelId(4));
        assert_eq!(s.pending(), 0);
        s.complete(SwatchImage { key: 2, size: 8, rgba: vec![0; 256] });
        s.complete(SwatchImage { key: 5, size: 8, rgba: vec![0; 256] });
        s.fail(6, "x");
        assert_eq!(s.take_done_where(|k| k == 5).iter().map(|i| i.key).collect::<Vec<_>>(), [5]);
        assert!(s.take_failed_where(|k| k == 5).is_empty());
        assert_eq!((s.take_done().len(), s.take_done().len(), s.drawn), (1, 0, 2));
        assert_eq!(s.take_failed().len(), 1);
    }

    /// A frame takes only requests on the oldest one's background; the
    /// others wait for the next frame.
    #[test]
    fn one_background_a_frame() {
        let mut s = Swatches::default();
        s.add_model(ModelId(1));
        s.note_upload(1);
        let card = Some([0.05, 0.05, 0.06]);
        s.request(req(1, 1, 1));
        s.request(SwatchRequest { background: card, ..req(2, 1, 1) });
        s.request(req(3, 1, 1));
        let ready = |r: &SwatchRequest, have: u32| have >= r.uploads;
        assert_eq!(s.take_ready(PER_FRAME, ready).iter().map(|r| r.key).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(s.take_ready(PER_FRAME, ready).iter().map(|r| (r.key, r.background)).collect::<Vec<_>>(), [(2, card)]);
    }

    /// A model's box is framed whole and tight from any side: every corner
    /// projects inside the share asked for and the widest one touches it.
    #[test]
    fn a_box_is_framed_whole_from_any_side() {
        let b = [[-0.4, 0.0, -1.0], [0.4, 1.6, 1.0]];
        for (yaw, pitch) in [(35.0, 12.0), (-90.0, 0.0), (180.0, 40.0), (0.0, 0.0)] {
            let c = frame_box(b, yaw, pitch, 0.9);
            assert_eq!(c.target, [AT[0], AT[1] + 0.8, AT[2]]);
            let vp = crate::shaders::view_proj(&c, 1.0);
            let mut widest = 0.0f32;
            for i in 0..8 {
                let p = [b[i & 1][0] + AT[0], b[(i >> 1) & 1][1] + AT[1], b[(i >> 2) & 1][2] + AT[2]];
                let clip: [f32; 4] = core::array::from_fn(|r| vp[r] * p[0] + vp[4 + r] * p[1] + vp[8 + r] * p[2] + vp[12 + r]);
                widest = widest.max((clip[0] / clip[3]).abs()).max((clip[1] / clip[3]).abs());
            }
            assert!((widest - 0.9).abs() < 1e-3, "yaw {yaw} pitch {pitch}: {widest}");
        }
        let front = frame_box(b, 0.0, 0.0, 0.9);
        assert!(front.eye[2] > AT[2] && (front.eye[0] - AT[0]).abs() < 1e-3, "yaw 0 looks from +z");
    }
}
