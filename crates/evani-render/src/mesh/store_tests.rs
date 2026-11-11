//! `MeshStore` against a byte-for-byte model of the two GPU buffers: every
//! sync is applied the way `gpu/frame.rs` applies it (grow by one copy, then
//! `write_buffer`s), and the result is checked mesh by mesh.

use super::*;
use crate::mesh::{MAX_MESHES, MeshLimitError, cube, mesh_buffers};

/// The two GPU buffers, as bytes.
#[derive(Default)]
struct Gpu {
    vb: Vec<u8>,
    ib: Vec<u8>,
    /// Bytes written with `write_buffer` over the run.
    uploaded: u64,
    /// Growth copies done.
    copies: u32,
}

impl Gpu {
    fn apply(&mut self, s: &MeshSync) {
        for (buf, size, copy) in [(&mut self.vb, s.vb_size, s.vb_copy), (&mut self.ib, s.ib_size, s.ib_copy)] {
            if buf.len() as u64 != size {
                // A new buffer: zeroed, then the old contents copied in once.
                assert!(copy == 0 || copy == buf.len() as u64, "copy covers the old buffer");
                let mut new = vec![0u8; size as usize];
                new[..copy as usize].copy_from_slice(&buf[..copy as usize]);
                *buf = new;
                if copy > 0 {
                    self.copies += 1;
                }
            } else {
                assert_eq!(copy, 0, "no copy without a resize");
            }
        }
        for w in &s.writes {
            let buf = if w.buf == MeshBuf::Vertex { &mut self.vb } else { &mut self.ib };
            let at = w.offset as usize;
            buf[at..at + w.bytes.len()].copy_from_slice(&w.bytes);
            self.uploaded += w.bytes.len() as u64;
        }
    }

    fn sync(&mut self, store: &mut MeshStore, lib: &MeshLibrary) -> MeshSync {
        let s = store.sync(Some(lib));
        self.apply(&s);
        s
    }

    /// Every drawable mesh's index range, read back from the buffers, gives
    /// exactly its own vertices.
    fn check(&self, store: &MeshStore, lib: &MeshLibrary) {
        let mut spans: Vec<(bool, u32, u32)> = Vec::new();
        let one = |slot: usize, m: &Mesh, spans: &mut Vec<(bool, u32, u32)>| {
            let (first, count, base) = store.ranges()[slot];
            if !drawable(m) {
                assert_eq!((first, count, base, store.tris()[slot]), (0, 0, 0, 0), "slot {slot} draws nothing");
                return;
            }
            assert_eq!(count as usize, m.indices.len(), "slot {slot} index count");
            assert_eq!(store.tris()[slot], m.triangles(), "slot {slot} triangles");
            for (k, &want) in m.indices.iter().enumerate() {
                let at = (first as usize + k) * INDEX_BYTES as usize;
                let got = u32::from_le_bytes(self.ib[at..at + 4].try_into().unwrap());
                assert_eq!(got, want, "slot {slot} index {k}");
                let vat = (base as usize + got as usize) * VERTEX_BYTES as usize;
                let v: Vec<f32> = self.vb[vat..vat + 24].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
                assert_eq!(v, m.vertices[got as usize].to_vec(), "slot {slot} vertex of index {k}");
            }
            spans.push((true, base as u32, m.vertices.len() as u32));
            spans.push((false, first, count));
        };
        for i in 0..BUILTIN_MESH_COUNT {
            one(i, &builtin(MeshId(i as u16)).unwrap(), &mut spans);
        }
        for (j, m) in lib.meshes().iter().enumerate() {
            one(BUILTIN_MESH_COUNT + j, m, &mut spans);
        }
        // No two live meshes share a vertex or index.
        for is_v in [true, false] {
            let kind = if is_v { "v" } else { "i" };
            let mut r: Vec<(u32, u32)> = spans.iter().filter(|s| s.0 == is_v).map(|s| (s.1, s.2)).collect();
            r.sort_unstable();
            for w in r.windows(2) {
                assert!(w[0].0 + w[0].1 <= w[1].0, "{kind} ranges overlap: {w:?}");
            }
            let used: u32 = r.iter().map(|x| x.1).sum();
            let (vh, ih) = store.heaps();
            let heap = if is_v { vh } else { ih };
            assert!(r.last().is_none_or(|l| l.0 + l.1 <= heap.cap), "{kind} range past the buffer");
            // Used + free = the whole buffer, and the free list is sorted and coalesced.
            let free: u32 = heap.free.iter().map(|f| f.1).sum();
            assert_eq!(used + free, heap.cap, "{kind}: used + free = capacity");
            for w in heap.free.windows(2) {
                assert!(w[0].0 + w[0].1 < w[1].0, "{kind} free list sorted and coalesced: {:?}", heap.free);
            }
            for f in &heap.free {
                assert!(r.iter().all(|u| u.0 + u.1 <= f.0 || f.0 + f.1 <= u.0), "{kind} free block overlaps a mesh");
            }
        }
    }
}

/// Small deterministic generator (splitmix64): the tests need no `rand`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A mesh of `verts` vertices and `tris` triangles with values nothing else
/// shares (`tag`), so a misplaced range cannot pass by coincidence.
fn blob(tag: u32, verts: u32, tris: u32, rng: &mut Rng) -> Mesh {
    let f = |k: u32| tag as f32 * 1000.0 + k as f32;
    Mesh {
        vertices: (0..verts).map(|v| [f(v), f(v) + 0.25, f(v) + 0.5, 0.0, 1.0, tag as f32]).collect(),
        indices: (0..tris * 3).map(|_| rng.below(u64::from(verts)) as u32).collect(),
    }
}

fn mesh_bytes(m: &Mesh) -> u64 {
    m.vertices.len() as u64 * VERTEX_BYTES + m.indices.len() as u64 * INDEX_BYTES
}

/// What the old code uploaded for any change: every mesh, both buffers
/// (the built-ins from `mesh_buffers`, which only includes the library with
/// feature `meshes`, so the library is added here whatever the features).
fn full_upload_bytes(lib: &MeshLibrary) -> u64 {
    let b = mesh_buffers(None);
    (b.vb.len() + b.ib.len()) as u64 + lib.meshes().iter().filter(|m| drawable(m)).map(mesh_bytes).sum::<u64>()
}

#[test]
fn first_sync_uploads_builtins_and_library_once() {
    let mut lib = MeshLibrary::default();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    let a = lib.add(cube()).unwrap();
    let s = gpu.sync(&mut store, &lib);
    assert_eq!((s.vb_copy, s.ib_copy), (0, 0), "created, not grown");
    assert!(gpu.vb.len() as u64 >= 4096 * VERTEX_BYTES && gpu.ib.len() as u64 >= 16384 * INDEX_BYTES);
    // The oldest layout (back to back) holds the same bytes.
    assert_eq!(s.bytes_uploaded(), full_upload_bytes(&lib));
    assert_eq!(store.ranges()[usize::from(a.0)].1, 36);
    gpu.check(&store, &lib);
}

#[test]
fn an_add_uploads_only_its_own_ranges() {
    let mut rng = Rng(1);
    let mut lib = MeshLibrary::default();
    for t in 0..40 {
        lib.add(blob(t, 60, 40, &mut rng)).unwrap();
    }
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let m = blob(99, 60, 40, &mut rng);
    let want = mesh_bytes(&m);
    let id = lib.add(m).unwrap();
    let before = full_upload_bytes(&lib);
    let s = gpu.sync(&mut store, &lib);
    assert_eq!((s.vb_copy, s.ib_copy), (0, 0), "room left: no growth");
    assert_eq!(s.writes.len(), 2);
    assert_eq!(s.bytes_uploaded(), want);
    assert!(before > 30 * s.bytes_uploaded(), "{before} B before vs {} B after", s.bytes_uploaded());
    eprintln!("add: {before} B uploaded before, {} B after ({:.0}x less)", s.bytes_uploaded(), before as f64 / s.bytes_uploaded() as f64);
    assert!(lib.is_live(id));
    gpu.check(&store, &lib);
}

#[test]
fn a_sync_with_no_change_uploads_nothing() {
    let mut rng = Rng(2);
    let mut lib = MeshLibrary::default();
    lib.add(blob(1, 30, 20, &mut rng)).unwrap();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    for _ in 0..3 {
        let s = gpu.sync(&mut store, &lib);
        assert!(s.is_empty() && s.bytes_uploaded() == 0);
        assert_eq!(s.vb_size, gpu.vb.len() as u64);
    }
    // No library at all: built-ins only, then nothing.
    let mut bare = MeshStore::new();
    assert!(!bare.sync(None).is_empty());
    assert!(bare.sync(None).is_empty());
}

#[test]
fn a_replace_that_is_not_bigger_writes_in_place() {
    let mut rng = Rng(3);
    let mut lib = MeshLibrary::default();
    let ids: Vec<MeshId> = (0..5).map(|t| lib.add(blob(t, 100, 60, &mut rng)).unwrap()).collect();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let at = usize::from(ids[2].0);
    let (first, _, base) = store.ranges()[at];
    // Same size.
    let same = blob(50, 100, 60, &mut rng);
    let want = mesh_bytes(&same);
    let before = full_upload_bytes(&lib);
    assert!(lib.replace(ids[2], same));
    let s = gpu.sync(&mut store, &lib);
    assert_eq!(s.bytes_uploaded(), want);
    assert_eq!((store.ranges()[at].0, store.ranges()[at].2), (first, base), "same place");
    assert!(s.writes.iter().all(|w| (w.buf == MeshBuf::Vertex && w.offset == base as u64 * 24) || (w.buf == MeshBuf::Index && w.offset == first as u64 * 4)));
    eprintln!("replace (same size): {before} B uploaded before, {} B after", s.bytes_uploaded());
    gpu.check(&store, &lib);
    // Smaller: still in place, and the tail is free for the next add.
    let small = blob(51, 40, 20, &mut rng);
    assert!(lib.replace(ids[2], small));
    let s = gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[at], (first, 60, base));
    assert_eq!(s.bytes_uploaded(), 40 * 24 + 60 * 4);
    gpu.check(&store, &lib);
    let fits = lib.add(blob(52, 60, 20, &mut rng)).unwrap();
    gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(fits.0)].2, base + 40, "took the freed tail");
    gpu.check(&store, &lib);
}

#[test]
fn a_bigger_replace_moves_and_frees_the_old_range() {
    let mut rng = Rng(4);
    let mut lib = MeshLibrary::default();
    let ids: Vec<MeshId> = (0..4).map(|t| lib.add(blob(t, 50, 30, &mut rng)).unwrap()).collect();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let at = usize::from(ids[1].0);
    let old = store.ranges()[at];
    let big = blob(60, 200, 120, &mut rng);
    let want = mesh_bytes(&big);
    assert!(lib.replace(ids[1], big));
    let s = gpu.sync(&mut store, &lib);
    assert_eq!(s.bytes_uploaded(), want, "only the new mesh is written");
    assert_ne!(store.ranges()[at].2, old.2, "moved");
    gpu.check(&store, &lib);
    // The old block is reusable: a mesh of the same size lands there.
    let again = lib.add(blob(61, 50, 30, &mut rng)).unwrap();
    gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(again.0)], (old.0, old.1, old.2));
    gpu.check(&store, &lib);
}

#[test]
fn a_remove_frees_its_slot_and_its_range() {
    let mut rng = Rng(5);
    let mut lib = MeshLibrary::default();
    let a = lib.add(blob(1, 30, 10, &mut rng)).unwrap();
    let b = lib.add(blob(2, 30, 10, &mut rng)).unwrap();
    let c = lib.add(blob(3, 30, 10, &mut rng)).unwrap();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let b_range = store.ranges()[usize::from(b.0)];
    let a_range = store.ranges()[usize::from(a.0)];
    let rev = lib.revision();
    assert!(lib.remove(b));
    assert!(lib.revision() > rev);
    assert!(!lib.remove(b), "already removed");
    assert!(!lib.remove(MeshId::CUBE) && !lib.remove(MeshId(900)));
    assert!(lib.get(b).is_none() && !lib.is_live(b) && lib.live_count() == 2);
    assert!(!lib.replace(b, cube()), "a removed id cannot be replaced");
    let s = gpu.sync(&mut store, &lib);
    assert_eq!(s.bytes_uploaded(), 0, "a remove writes nothing");
    assert_eq!(store.ranges()[usize::from(b.0)], (0, 0, 0));
    assert_eq!(store.tris()[usize::from(b.0)], 0);
    gpu.check(&store, &lib);
    // The next add reuses the slot id and the range.
    let d = lib.add(blob(4, 30, 10, &mut rng)).unwrap();
    assert_eq!(d, b, "lowest free slot");
    let s = gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(d.0)], b_range);
    assert_eq!(s.writes.len(), 2);
    gpu.check(&store, &lib);
    assert!(lib.is_live(a) && lib.is_live(c));
    // Neighbouring free blocks coalesce: remove two, then fit one big mesh in the gap.
    lib.remove(a);
    lib.remove(d);
    gpu.sync(&mut store, &lib);
    let big = lib.add(blob(5, 60, 20, &mut rng)).unwrap();
    gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(big.0)], (a_range.0, 60, a_range.2), "the two freed blocks merged into one gap");
    gpu.check(&store, &lib);
}

#[test]
fn a_full_buffer_doubles_with_one_copy_and_no_reupload() {
    let mut rng = Rng(6);
    let mut lib = MeshLibrary::default();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let (v0, i0) = (gpu.vb.len() as u64, gpu.ib.len() as u64);
    let mut grown = None;
    for t in 0..60 {
        let m = blob(t, 200, 120, &mut rng);
        let want = mesh_bytes(&m);
        lib.add(m).unwrap();
        let s = gpu.sync(&mut store, &lib);
        assert_eq!(s.bytes_uploaded(), want, "an add uploads its mesh, growth or not (add {t})");
        if s.vb_copy > 0 || s.ib_copy > 0 {
            grown = grown.or(Some(s));
        }
        gpu.check(&store, &lib);
    }
    let s = grown.expect("60 meshes of 200 vertices outgrow the first buffer");
    assert!(gpu.vb.len() as u64 >= v0 * 2 || gpu.ib.len() as u64 >= i0 * 2);
    assert!(gpu.copies >= 1);
    // The copy moved the old bytes on the GPU; the CPU wrote only new meshes.
    assert!(s.vb_copy == 0 || s.vb_size == s.vb_copy * 2, "doubled");
    let total_mesh_bytes: u64 = lib.meshes().iter().map(mesh_bytes).sum();
    let builtin_bytes: u64 = (0..BUILTIN_MESH_COUNT).map(|i| mesh_bytes(&builtin(MeshId(i as u16)).unwrap())).sum();
    assert_eq!(gpu.uploaded, total_mesh_bytes + builtin_bytes, "every byte uploaded exactly once");
    eprintln!("growth: {} copies, {} B uploaded in total = exactly the meshes' own bytes", gpu.copies, gpu.uploaded);
}

#[test]
fn growth_by_a_mesh_bigger_than_the_buffer_still_fits() {
    let mut rng = Rng(7);
    let mut lib = MeshLibrary::default();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    // Far past double the starting capacity.
    lib.add(blob(1, 30_000, 20_000, &mut rng)).unwrap();
    let s = gpu.sync(&mut store, &lib);
    assert!(s.vb_copy > 0 && s.ib_copy > 0);
    gpu.check(&store, &lib);
}

#[test]
fn the_full_library_errors_then_recovers_after_a_remove() {
    let mut rng = Rng(8);
    let mut lib = MeshLibrary::default();
    let ids: Vec<MeshId> = (0..MAX_CUSTOM_MESHES as u32).map(|t| lib.try_add(blob(t, 8, 4, &mut rng)).unwrap()).collect();
    assert_eq!(BUILTIN_MESH_COUNT + lib.meshes().len(), MAX_MESHES);
    assert_eq!(lib.try_add(cube()), Err(MeshLimitError));
    assert!(lib.add(cube()).is_none());
    assert!(MeshLimitError.to_string().contains("mesh library full"));
    let rev = lib.revision();
    assert_eq!(lib.try_add(cube()), Err(MeshLimitError));
    assert_eq!(lib.revision(), rev, "a refused add changes nothing");
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    gpu.check(&store, &lib);
    // Removing one makes room for exactly one.
    assert!(lib.remove(ids[100]));
    let again = lib.try_add(blob(999, 8, 4, &mut rng)).unwrap();
    assert_eq!(again, ids[100]);
    assert_eq!(lib.try_add(cube()), Err(MeshLimitError));
    let s = gpu.sync(&mut store, &lib);
    assert_eq!(s.bytes_uploaded(), 8 * 24 + 12 * 4);
    gpu.check(&store, &lib);
}

#[test]
fn a_mesh_with_a_bad_index_draws_nothing_and_a_fix_draws_it() {
    let mut lib = MeshLibrary::default();
    let bad = lib.add(Mesh { vertices: vec![[0.0; 6]; 2], indices: vec![0, 1, 7] }).unwrap();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(bad.0)], (0, 0, 0));
    gpu.check(&store, &lib);
    assert!(lib.replace(bad, cube()));
    gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(bad.0)].1, 36);
    gpu.check(&store, &lib);
    assert!(lib.replace(bad, Mesh::default()));
    gpu.sync(&mut store, &lib);
    assert_eq!(store.ranges()[usize::from(bad.0)], (0, 0, 0));
    gpu.check(&store, &lib);
}

#[test]
fn builtins_never_move() {
    let mut rng = Rng(9);
    let mut lib = MeshLibrary::default();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let first: Vec<_> = store.ranges()[..BUILTIN_MESH_COUNT].to_vec();
    assert_eq!(first, mesh_buffers(None).ranges[..BUILTIN_MESH_COUNT].to_vec(), "same layout as the full build");
    for t in 0..80 {
        lib.add(blob(t, 150, 100, &mut rng)).unwrap();
        gpu.sync(&mut store, &lib);
    }
    assert_eq!(store.ranges()[..BUILTIN_MESH_COUNT], first[..]);
    gpu.check(&store, &lib);
}

/// One churn run: `ops` random adds, replaces and removes; after each one
/// the buffers are checked. Returns the sequence of syncs for comparison.
fn churn(seed: u64, ops: usize) -> (Vec<MeshSync>, Vec<(u32, u32, i32)>) {
    let mut rng = Rng(seed);
    let mut lib = MeshLibrary::default();
    let (mut store, mut gpu) = (MeshStore::new(), Gpu::default());
    gpu.sync(&mut store, &lib);
    let mut live: Vec<MeshId> = Vec::new();
    let mut syncs = Vec::new();
    let mut tag = 0;
    for op in 0..ops {
        tag += 1;
        let (verts, tris) = (3 + rng.below(220) as u32, 1 + rng.below(160) as u32);
        let mesh = blob(tag, verts, tris, &mut rng);
        let bytes = mesh_bytes(&mesh);
        let mut expect = Some(bytes);
        match (rng.below(10), live.is_empty()) {
            (0..=4, _) | (_, true) => match lib.try_add(mesh) {
                Ok(id) => live.push(id),
                Err(_) => expect = Some(0),
            },
            (5..=7, false) => {
                let id = live[rng.below(live.len() as u64) as usize];
                assert!(lib.replace(id, mesh));
            }
            (_, false) => {
                let id = live.swap_remove(rng.below(live.len() as u64) as usize);
                assert!(lib.remove(id));
                expect = Some(0);
            }
        }
        let s = gpu.sync(&mut store, &lib);
        assert_eq!(s.bytes_uploaded(), expect.unwrap(), "op {op}: only the touched mesh is uploaded");
        gpu.check(&store, &lib);
        assert_eq!(lib.live_count(), live.len());
        syncs.push(s);
    }
    (syncs, store.ranges().to_vec())
}

#[test]
fn a_thousand_random_changes_keep_every_range_pointing_at_its_own_data() {
    let (syncs, _) = churn(0xE1A_0001, 1000);
    let copies = syncs.iter().filter(|s| s.vb_copy > 0 || s.ib_copy > 0).count();
    eprintln!("churn: 1000 ops, {copies} growth copies, final buffers {} + {} B", syncs.last().unwrap().vb_size, syncs.last().unwrap().ib_size);
}

#[test]
fn the_layout_is_deterministic() {
    let (a, ra) = churn(77, 300);
    let (b, rb) = churn(77, 300);
    assert_eq!(a, b, "same changes, same writes");
    assert_eq!(ra, rb);
}

#[test]
fn heap_blocks_coalesce_and_first_fit_picks_the_lowest() {
    let mut h = Heap::new(100);
    let (a, b, c) = (h.alloc(10).unwrap(), h.alloc(20).unwrap(), h.alloc(30).unwrap());
    assert_eq!((a, b, c), (0, 10, 30));
    h.release(a, 10);
    h.release(c, 30);
    assert_eq!(h.free, vec![(0, 10), (30, 70)]);
    h.release(b, 20);
    assert_eq!(h.free, vec![(0, 100)], "all merged");
    // Lowest hole that fits.
    let (x, y, _z) = (h.alloc(10).unwrap(), h.alloc(40).unwrap(), h.alloc(10).unwrap());
    h.release(x, 10);
    h.release(y + 5, 20);
    assert_eq!(h.alloc(8), Some(0), "first fit at the lowest offset");
    assert_eq!(h.alloc(45), None);
    assert!(h.grow_for(45));
    assert_eq!(h.cap, 200, "doubled");
    assert!(h.alloc(45).is_some());
}
