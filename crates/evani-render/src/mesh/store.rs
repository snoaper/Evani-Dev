//! The CPU side of the shared mesh vertex and index buffers: where every
//! mesh lives, and the writes that bring the GPU in line when the
//! [`MeshLibrary`] changes. No GPU types here, so the layout is tested
//! without a device (`store_tests.rs`).

use super::{BUILTIN_MESH_COUNT, MAX_CUSTOM_MESHES, Mesh, MeshId, MeshLibrary, builtin};

/// Bytes of one vertex in the GPU vertex buffer (position + normal).
pub const VERTEX_BYTES: u64 = 24;
/// Bytes of one index in the GPU index buffer.
pub const INDEX_BYTES: u64 = 4;
/// Smallest capacity of the shared vertex buffer, in vertices (96 KiB).
const MIN_VERTICES: u32 = 4096;
/// Smallest capacity of the shared index buffer, in indices (64 KiB).
const MIN_INDICES: u32 = 16384;

/// Which shared GPU buffer a [`MeshWrite`] targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshBuf {
    Vertex,
    Index,
}

/// One `queue.write_buffer` call: `bytes` at `offset` in the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshWrite {
    pub buf: MeshBuf,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// What the GPU must do to match the [`MeshLibrary`] after
/// [`MeshStore::sync`], in this order: (1) when a buffer's size changed,
/// create the new buffer, copy the first `*_copy` bytes of the old one into
/// it with one `copy_buffer_to_buffer` and submit that; (2) apply `writes`
/// with `queue.write_buffer`. Nothing to do when [`MeshSync::is_empty`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshSync {
    /// Size in bytes of the vertex and index buffer after the sync.
    pub vb_size: u64,
    pub ib_size: u64,
    /// Bytes of the old buffer to copy when it grew (0: it did not, or it
    /// is being created).
    pub vb_copy: u64,
    pub ib_copy: u64,
    pub writes: Vec<MeshWrite>,
}

impl MeshSync {
    /// Bytes the CPU uploads with `write_buffer` (a growth copy runs on the
    /// GPU and uploads nothing).
    pub fn bytes_uploaded(&self) -> u64 {
        self.writes.iter().map(|w| w.bytes.len() as u64).sum()
    }

    /// Nothing changed: no write and no growth.
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.vb_copy == 0 && self.ib_copy == 0
    }
}

/// A heap of `cap` units with a sorted, coalesced free list. First fit at
/// the lowest offset, so the same calls always give the same layout.
#[derive(Debug, Clone, Default)]
struct Heap {
    cap: u32,
    free: Vec<(u32, u32)>,
}

impl Heap {
    fn new(cap: u32) -> Self {
        Heap { cap, free: vec![(0, cap)] }
    }

    fn alloc(&mut self, n: u32) -> Option<u32> {
        let i = self.free.iter().position(|&(_, l)| l >= n)?;
        let (s, l) = self.free[i];
        if l == n {
            self.free.remove(i);
        } else {
            self.free[i] = (s + n, l - n);
        }
        Some(s)
    }

    fn release(&mut self, start: u32, n: u32) {
        if n == 0 {
            return;
        }
        let i = self.free.partition_point(|&(s, _)| s < start);
        self.free.insert(i, (start, n));
        if i + 1 < self.free.len() && self.free[i].0 + self.free[i].1 == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == self.free[i].0 {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
    }

    /// Grow so a block of `n` fits: double, or more when `n` needs it. The
    /// new space joins the free block at the end. False on overflow.
    fn grow_for(&mut self, n: u32) -> bool {
        let tail = self.free.last().filter(|&&(s, l)| s + l == self.cap).map_or(0, |&(_, l)| l);
        let Some(need) = self.cap.checked_add(n.saturating_sub(tail)) else {
            return false;
        };
        let new_cap = self.cap.saturating_mul(2).max(need);
        if new_cap == self.cap {
            return false;
        }
        let old = self.cap;
        self.cap = new_cap;
        self.release(old, new_cap - old);
        true
    }

    /// Give a mesh part `(first, len)` room for `new` units: in place when
    /// it fits (the tail goes back to the free list), else a new block (the
    /// old one is freed first). `None` when the heap cannot grow.
    fn fit(&mut self, first: u32, len: u32, new: u32) -> Option<(u32, u32)> {
        if new == 0 {
            self.release(first, len);
            return Some((0, 0));
        }
        if new <= len {
            self.release(first + new, len - new);
            return Some((first, new));
        }
        self.release(first, len);
        loop {
            if let Some(s) = self.alloc(new) {
                return Some((s, new));
            }
            if !self.grow_for(new) {
                return None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct StoreSlot {
    v_first: u32,
    v_len: u32,
    i_first: u32,
    i_len: u32,
    /// The library slot revision uploaded.
    rev: u64,
}

/// Where every mesh lives in the shared GPU buffers, and what to upload when
/// the [`MeshLibrary`] changes. Meshes are placed by a first-fit free list:
///
/// * an add uploads only that mesh's vertex and index range;
/// * a replace with a mesh that is not bigger writes in place (the unused
///   tail is freed); a bigger one takes a new block and frees the old;
/// * a remove frees its ranges for the next add;
/// * a buffer that runs out of room doubles with one GPU copy (never a
///   re-upload from the CPU);
/// * a sync with no library change returns at once and uploads nothing.
///
/// The built-ins take the first ranges at the first sync and never move.
#[derive(Debug, Clone)]
pub struct MeshStore {
    vh: Heap,
    ih: Heap,
    slots: Vec<StoreSlot>,
    ranges: Vec<(u32, u32, i32)>,
    tris: Vec<u32>,
    ready: bool,
    seen: u64,
}

impl Default for MeshStore {
    fn default() -> Self {
        Self::new()
    }
}

/// A mesh the GPU can draw: has triangles and no index past its vertices.
fn drawable(m: &Mesh) -> bool {
    !m.vertices.is_empty()
        && !m.indices.is_empty()
        && m.vertices.len() <= i32::MAX as usize
        && m.indices.len() <= u32::MAX as usize
        && m.indices.iter().all(|&x| (x as usize) < m.vertices.len())
}

impl MeshStore {
    pub fn new() -> Self {
        let n = BUILTIN_MESH_COUNT + MAX_CUSTOM_MESHES;
        MeshStore {
            vh: Heap::default(),
            ih: Heap::default(),
            slots: vec![StoreSlot::default(); n],
            ranges: vec![(0, 0, 0); n],
            tris: vec![0; n],
            ready: false,
            seen: 0,
        }
    }

    /// `(first index, index count, base vertex)` per mesh slot (built-ins,
    /// then custom slots); `(0, 0, 0)` draws nothing.
    pub fn ranges(&self) -> &[(u32, u32, i32)] {
        &self.ranges
    }

    /// Triangle count per mesh slot.
    pub fn tris(&self) -> &[u32] {
        &self.tris
    }

    /// Current buffer sizes in bytes: `(vertex, index)`.
    pub fn sizes(&self) -> (u64, u64) {
        (u64::from(self.vh.cap) * VERTEX_BYTES, u64::from(self.ih.cap) * INDEX_BYTES)
    }

    /// Bring the layout in line with `lib` and return what to upload. Costs
    /// a revision compare when the library has not changed.
    pub fn sync(&mut self, lib: Option<&MeshLibrary>) -> MeshSync {
        let mut out = MeshSync::default();
        let rev = lib.map_or(0, MeshLibrary::revision);
        if !(self.ready && rev == self.seen) {
            self.seen = rev;
            let (old_v, old_i) = (self.vh.cap, self.ih.cap);
            if !self.ready {
                self.ready = true;
                self.start(lib, &mut out);
            }
            if let Some(lib) = lib {
                for (j, m) in lib.meshes().iter().enumerate().take(MAX_CUSTOM_MESHES) {
                    let r = lib.slot_revision(j);
                    if r != self.slots[BUILTIN_MESH_COUNT + j].rev {
                        self.place(BUILTIN_MESH_COUNT + j, m, r, &mut out);
                    }
                }
            }
            if old_v != 0 && self.vh.cap != old_v {
                out.vb_copy = u64::from(old_v) * VERTEX_BYTES;
            }
            if old_i != 0 && self.ih.cap != old_i {
                out.ib_copy = u64::from(old_i) * INDEX_BYTES;
            }
        }
        (out.vb_size, out.ib_size) = self.sizes();
        out
    }

    /// First sync: size both heaps for the built-ins and the library's
    /// meshes (a power of two over what is needed, so the first meshes
    /// cost no growth copy and a few more fit), and place the built-ins.
    fn start(&mut self, lib: Option<&MeshLibrary>, out: &mut MeshSync) {
        let builtins: Vec<Mesh> = (0..BUILTIN_MESH_COUNT).filter_map(|i| builtin(MeshId(i as u16))).collect();
        let custom = lib.map_or(&[][..], MeshLibrary::meshes);
        let (mut nv, mut ni) = (0u64, 0u64);
        for m in builtins.iter().chain(custom).filter(|m| drawable(m)) {
            nv += m.vertices.len() as u64;
            ni += m.indices.len() as u64;
        }
        let size = |n: u64, min: u32| u32::try_from(n.max(u64::from(min)).next_power_of_two()).unwrap_or(u32::MAX);
        self.vh = Heap::new(size(nv, MIN_VERTICES));
        self.ih = Heap::new(size(ni, MIN_INDICES));
        for (i, m) in builtins.iter().enumerate() {
            self.place(i, m, 0, out);
        }
    }

    /// Put `m` in slot `i` (library revision `rev`): reuse or free its old
    /// ranges, take new ones if needed, and queue the writes. A mesh the GPU
    /// cannot draw leaves the slot empty.
    fn place(&mut self, i: usize, m: &Mesh, rev: u64, out: &mut MeshSync) {
        let s = self.slots[i];
        let (nv, ni) = if drawable(m) { (m.vertices.len() as u32, m.indices.len() as u32) } else { (0, 0) };
        let v = self.vh.fit(s.v_first, s.v_len, nv);
        let ix = self.ih.fit(s.i_first, s.i_len, ni);
        let (Some(v), Some(ix)) = (v, ix) else {
            // Out of address space: draw nothing, keep the heaps consistent.
            for (heap, part) in [(&mut self.vh, v), (&mut self.ih, ix)] {
                if let Some((f, l)) = part {
                    heap.release(f, l);
                }
            }
            self.empty(i, rev);
            return;
        };
        if v.1 == 0 || ix.1 == 0 {
            self.empty(i, rev);
            return;
        }
        self.slots[i] = StoreSlot { v_first: v.0, v_len: v.1, i_first: ix.0, i_len: ix.1, rev };
        self.ranges[i] = (ix.0, ix.1, v.0 as i32);
        self.tris[i] = m.triangles();
        let mut vb = Vec::with_capacity(m.vertices.len() * VERTEX_BYTES as usize);
        for f in m.vertices.iter().flatten() {
            vb.extend_from_slice(&f.to_le_bytes());
        }
        out.writes.push(MeshWrite { buf: MeshBuf::Vertex, offset: u64::from(v.0) * VERTEX_BYTES, bytes: vb });
        let mut ib = Vec::with_capacity(m.indices.len() * INDEX_BYTES as usize);
        for x in &m.indices {
            ib.extend_from_slice(&x.to_le_bytes());
        }
        out.writes.push(MeshWrite { buf: MeshBuf::Index, offset: u64::from(ix.0) * INDEX_BYTES, bytes: ib });
    }

    fn empty(&mut self, i: usize, rev: u64) {
        self.slots[i] = StoreSlot { rev, ..StoreSlot::default() };
        self.ranges[i] = (0, 0, 0);
        self.tris[i] = 0;
    }

    #[cfg(test)]
    fn heaps(&self) -> (&Heap, &Heap) {
        (&self.vh, &self.ih)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
