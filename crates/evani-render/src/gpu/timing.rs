//! Per-pass GPU timing with timestamp queries: a begin/end pair on every
//! render pass, resolved at the end of the frame and copied into one of
//! six readback buffers, mapped asynchronously and summed per graph pass
//! a frame or two later. Without `timestamp-query` it falls back to an
//! estimate: the time from submit to `onSubmittedWorkDone`, one frame in
//! flight at a time, marked `gpu_estimated` (shown as `~` on the overlay).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use evani_core::overlay::{PassTiming, RenderStats};

/// Query pairs per frame (render passes timed).
const MAX_PAIRS: u32 = 64;
/// Readback buffers (1 KB each). Six, so a GPU-bound frame rate (the GPU a
/// few frames behind) still times nearly every frame; with three, about
/// two frames in five went untimed.
const READBACKS: usize = 6;

const FREE: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;

struct Readback {
    buf: wgpu::Buffer,
    state: Rc<Cell<u8>>,
    names: Vec<&'static str>,
}

pub struct Timer {
    set: Option<wgpu::QuerySet>,
    resolve: Option<wgpu::Buffer>,
    readbacks: Vec<Readback>,
    /// Graph pass of each query pair recorded this frame.
    names: Vec<&'static str>,
    /// Readback chosen for this frame (when one is free).
    target: Option<usize>,
    period_ns: f32,
    pub last_gpu_ms: Option<f32>,
    /// Fallback estimate (no timestamps): submit time of the frame in
    /// flight, `None` when none is.
    est_start: Option<f64>,
    /// Bits of the `f64` done time the work-done callback writes (`NO_DONE`
    /// until it fires). Atomic because wgpu wants a `Send` callback.
    est_done: Arc<AtomicU64>,
}

/// `est_done` before the callback fires.
const NO_DONE: u64 = u64::MAX;

impl Timer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, enabled: bool) -> Self {
        if !enabled {
            return Self {
                set: None,
                resolve: None,
                readbacks: Vec::new(),
                names: Vec::new(),
                target: None,
                period_ns: 1.0,
                last_gpu_ms: None,
                est_start: None,
                est_done: Arc::new(AtomicU64::new(NO_DONE)),
            };
        }
        let bytes = u64::from(MAX_PAIRS) * 16;
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("evani.timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_PAIRS * 2,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("evani.timestamps.resolve"),
            size: bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readbacks = (0..READBACKS)
            .map(|_| Readback {
                buf: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("evani.timestamps.read"),
                    size: bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Rc::new(Cell::new(FREE)),
                names: Vec::with_capacity(MAX_PAIRS as usize),
            })
            .collect();
        Self {
            set: Some(set),
            resolve: Some(resolve),
            readbacks,
            names: Vec::with_capacity(MAX_PAIRS as usize),
            target: None,
            period_ns: queue.get_timestamp_period(),
            last_gpu_ms: None,
            est_start: None,
            est_done: Arc::new(AtomicU64::new(NO_DONE)),
        }
    }

    pub fn supported(&self) -> bool {
        self.set.is_some()
    }

    /// Start a frame: pick a free readback (no timing this frame if none).
    pub fn begin(&mut self) {
        self.names.clear();
        self.target = self.readbacks.iter().position(|r| r.state.get() == FREE);
    }

    /// Timestamp writes for the next render pass of graph pass `pass`.
    pub fn pass(&mut self, pass: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.target?;
        let set = self.set.as_ref()?;
        let i = self.names.len() as u32;
        if i >= MAX_PAIRS {
            return None;
        }
        self.names.push(pass);
        Some(wgpu::RenderPassTimestampWrites {
            query_set: set,
            beginning_of_pass_write_index: Some(i * 2),
            end_of_pass_write_index: Some(i * 2 + 1),
        })
    }

    /// Timestamp writes for a compute pass of graph pass `pass`.
    #[cfg(any(feature = "particles", feature = "clustered"))]
    pub fn compute_pass(&mut self, pass: &'static str) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        let w = self.pass(pass)?;
        Some(wgpu::ComputePassTimestampWrites {
            query_set: w.query_set,
            beginning_of_pass_write_index: w.beginning_of_pass_write_index,
            end_of_pass_write_index: w.end_of_pass_write_index,
        })
    }

    /// Resolve this frame's queries into the chosen readback (before finish).
    pub fn resolve(&mut self, enc: &mut wgpu::CommandEncoder) {
        let (Some(set), Some(resolve), Some(t)) = (&self.set, &self.resolve, self.target) else {
            return;
        };
        let n = self.names.len() as u32;
        if n == 0 {
            self.target = None;
            return;
        }
        let Some(rb) = self.readbacks.get_mut(t) else {
            return;
        };
        enc.resolve_query_set(set, 0..n * 2, resolve, 0);
        enc.copy_buffer_to_buffer(resolve, 0, &rb.buf, 0, u64::from(n) * 16);
        rb.names.clear();
        rb.names.extend_from_slice(&self.names);
    }

    /// After submit: map the readback written this frame or, without
    /// timestamps, start a submit-to-done estimate when none is in flight.
    pub fn after_submit(&mut self, queue: &wgpu::Queue) {
        if self.set.is_none() {
            if self.est_start.is_none() {
                self.est_done.store(NO_DONE, Ordering::Relaxed);
                self.est_start = Some(evani_core::time::now_ms());
                let done = self.est_done.clone();
                queue.on_submitted_work_done(move || {
                    done.store(evani_core::time::now_ms().to_bits(), Ordering::Relaxed);
                });
            }
            return;
        }
        let Some(t) = self.target.take() else {
            return;
        };
        let Some(rb) = self.readbacks.get(t) else {
            return;
        };
        let n = rb.names.len() as u64;
        let state = rb.state.clone();
        state.set(MAPPING);
        rb.buf.map_async(wgpu::MapMode::Read, 0..n * 16, move |r| {
            state.set(if r.is_ok() { READY } else { FAILED });
        });
    }

    /// Read finished readbacks into `stats` (per-pass ms and the total).
    pub fn collect(&mut self, stats: &mut RenderStats) {
        if self.set.is_none() {
            stats.passes.clear();
            stats.gpu_estimated = true;
            stats.gpu_note = "timestamp-query unsupported";
            let bits = self.est_done.load(Ordering::Relaxed);
            if let Some(start) = self.est_start
                && bits != NO_DONE
            {
                let ms = (f64::from_bits(bits) - start).max(0.0) as f32;
                self.est_start = None;
                stats.gpu_ms = Some(ms);
                stats.gpu_samples += 1;
                self.last_gpu_ms = Some(ms);
            }
            return;
        }
        stats.gpu_estimated = false;
        for rb in &mut self.readbacks {
            match rb.state.get() {
                READY => {}
                FAILED => {
                    rb.buf.unmap();
                    rb.state.set(FREE);
                    continue;
                }
                _ => continue,
            }
            let n = rb.names.len() as u64;
            let mut total = 0.0f32;
            stats.passes.clear();
            if let Ok(view) = rb.buf.get_mapped_range(0..n * 16) {
                for (i, name) in rb.names.iter().enumerate() {
                    let at = i * 16;
                    let rd = |o: usize| {
                        let mut b = [0u8; 8];
                        if let Some(s) = view.get(o..o + 8) {
                            b.copy_from_slice(s);
                        }
                        u64::from_le_bytes(b)
                    };
                    let (t0, t1) = (rd(at), rd(at + 8));
                    let ms = if t1 > t0 {
                        (t1 - t0) as f32 * self.period_ns / 1.0e6
                    } else {
                        0.0
                    };
                    total += ms;
                    match stats.passes.iter_mut().find(|p| p.name == *name) {
                        Some(p) => p.gpu_ms += ms,
                        None => stats.passes.push(PassTiming { name, gpu_ms: ms }),
                    }
                }
            }
            rb.buf.unmap();
            rb.state.set(FREE);
            stats.gpu_ms = Some(total);
            stats.gpu_samples += 1;
            stats.gpu_note = "";
            self.last_gpu_ms = Some(total);
        }
    }
}
