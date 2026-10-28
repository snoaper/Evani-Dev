//! Concrete holes cut by the kit material (evani-render `kit-material`):
//! every concrete fragment inside a hole of [`crate::holes::Holes`] is
//! discarded, in colour, depth and the shadow map alike, so a hole is a real
//! hole you see and light through. Hexagon's `keep()` (shaders.ts) is the
//! model; the code here is this kit's own and simpler (one jagged sphere per
//! hole, a darkened rim, no Voronoi chunks or crack network).
//!
//! * The look's `concrete` material is cut; its `concrete_dark` material is
//!   the crater wall: an inside-out sphere per drawn hole (see
//!   [`WALL_SCALE`]) kept only inside solid concrete and outside every other
//!   hole, so a hole has a broken inside instead of showing the backs of
//!   faces. Every other material is untouched.
//! * Without a `concrete` material nothing is cut (the drawing falls back to
//!   dark hole balls).
//!
//! `kit_data` layout (`vec4<f32>` records):
//!
//! | record | contents |
//! |---|---|
//! | 0 | ring cos, ring sin, concrete slot, wall slot (-1: none) |
//! | 1 | floor heights `fy[0..3]`, slab depth |
//! | 2 | `a_in`, `a_out`, `floor_slant`, 0 |
//! | [`BUCKET0`].. | the bucket table: per bucket of `holes.rs` (level, x, z) a (first entry, count) pair, two per record |
//! | [`COVER0`].. | standing cover cells: one bit per grid slot (`(L * 6 + k) * PER + (iu * NV + iv) * NY + iy`), 24 per float (stored as whole numbers, never bit-cast) |
//! | [`ENTRY0`].. | entries: hole centre (level frame) and radius |
//!
//! A bucket lists every hole whose rim reaches it ([`reach`]), newest first;
//! the shader reads at most [`LOOP`] of them. When the entries would not fit
//! in [`MAX_DATA`] records the oldest holes are left out of the drawing
//! (play still has them).

use evani_render::kit_material::{KitMaterial, MAX_DATA};

use crate::arena::{Arena, NU, NV, NY, PER, S60, TAN30};
use crate::holes::{HN, hbi};

/// First bucket-table record.
pub const BUCKET0: usize = 3;
/// Buckets (three levels of `HN * HN`).
pub const BUCKETS: usize = 3 * HN * HN;
/// First cover-bit record.
pub const COVER0: usize = BUCKET0 + BUCKETS.div_ceil(2);
/// Cover bits per float (exact in an `f32`).
pub const BITS: usize = 24;
/// Cover-bit records.
pub const COVER_RECS: usize = (18 * PER).div_ceil(4 * BITS);
/// First entry record.
pub const ENTRY0: usize = COVER0 + COVER_RECS;
/// Most holes one fragment tests.
pub const LOOP: usize = 256;
/// How far the broken outline reaches past the hole's sphere (fraction of its radius).
pub const JAG: f32 = 0.12;
/// Where the darkened rim ends (multiple of the broken radius).
pub const RIM: f32 = 1.6;
/// Crater wall sphere diameter per hole radius: just outside the broken outline.
pub const WALL_SCALE: f32 = 2.0 * (1.0 + JAG) * 1.04;

/// How far a hole of radius `r` reaches (the bucket cover).
pub fn reach(r: f32) -> f32 {
    r * (1.0 + JAG) * RIM
}

/// The kit material code (WGSL), constants filled in.
pub fn wgsl() -> String {
    let f = |v: f32| format!("{v:?}");
    let pairs: [(&str, String); 14] = [
        ("$HB", f(crate::holes::HB)),
        ("$HN", HN.to_string()),
        ("$NU", NU.to_string()),
        ("$NV", NV.to_string()),
        ("$NY", NY.to_string()),
        ("$PER", PER.to_string()),
        ("$BUCKET0", BUCKET0.to_string()),
        ("$COVER0", COVER0.to_string()),
        ("$ENTRY0", ENTRY0.to_string()),
        ("$LOOP", LOOP.to_string()),
        ("$TAN30", f(TAN30)),
        ("$S60", f(S60)),
        ("$JAG", f(JAG)),
        ("$RIM", f(RIM)),
    ];
    let mut s = include_str!("cut.wgsl").to_string();
    for (k, v) in pairs {
        s = s.replace(&format!("{k};"), &format!("{v};"));
    }
    s
}

/// What the uploaded data was built from (a change rebuilds it).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    seed: u64,
    version: u64,
    len: usize,
    newest: [u32; 4],
    gone: usize,
    slots: [i32; 2],
}

/// The cut-out's state (a world resource): its code, and what the uploaded
/// data was built from.
#[derive(Debug, Clone, Default)]
pub struct CutState {
    code: String,
    key: Option<Key>,
    ring: Option<[u32; 2]>,
    counts: Vec<u32>,
    starts: Vec<u32>,
    bits: Vec<u32>,
}

impl CutState {
    /// The state, its code built.
    pub fn new() -> Self {
        CutState { code: wgsl(), ..CutState::default() }
    }

    /// Bring the kit material up to date with the arena: the code once, the
    /// data when the holes, fallen cells or slots change (the ring angle
    /// alone patches the first record). `concrete` and `wall` are the
    /// look's material slots (`None`: nothing is cut, and the kit's code is
    /// taken out again).
    pub fn sync(&mut self, km: &mut KitMaterial, arena: &Arena, concrete: Option<u32>, wall: Option<u32>) {
        if self.code.is_empty() {
            self.code = wgsl();
        }
        let Some(conc) = concrete else {
            if km.wgsl() == self.code {
                km.set_wgsl("");
            }
            self.key = None;
            return;
        };
        if km.wgsl() != self.code {
            km.set_wgsl(self.code.clone());
            self.key = None;
        }
        let wall = wall.filter(|&w| w != conc);
        let slot = |s: Option<u32>| s.map_or(-1, |s| s as i32);
        let newest = arena.holes.list.last().map_or([0; 4], |h| [h.p[0].to_bits(), h.p[1].to_bits(), h.p[2].to_bits(), h.r.to_bits()]);
        let key = Key {
            seed: arena.seed,
            version: arena.holes.version,
            len: arena.holes.list.len(),
            newest,
            gone: arena.gone.iter().filter(|&&g| g).count(),
            slots: [slot(Some(conc)), slot(wall)],
        };
        let ring = [arena.ca.to_bits(), arena.sa.to_bits()];
        let head = [arena.ca, arena.sa, key.slots[0] as f32, key.slots[1] as f32];
        if self.key != Some(key) || km.data().len() < ENTRY0 {
            self.rebuild(km, arena, head);
            self.key = Some(key);
            self.ring = Some(ring);
        } else if self.ring != Some(ring) {
            if let Some(r) = km.data_mut().first_mut() {
                *r = head;
            }
            self.ring = Some(ring);
        }
    }

    /// Rebuild every record (a counting sort of holes into buckets).
    fn rebuild(&mut self, km: &mut KitMaterial, arena: &Arena, head: [f32; 4]) {
        let d = &arena.def;
        let list = &arena.holes.list;
        let range = |h: &crate::holes::Hole| {
            let e = reach(h.r);
            (hbi(h.p[0] - e), hbi(h.p[0] + e), hbi(h.p[2] - e), hbi(h.p[2] + e))
        };
        // The newest holes that fit.
        let budget = MAX_DATA.saturating_sub(ENTRY0);
        let mut total = 0usize;
        let mut take = 0usize;
        for h in list.iter().rev() {
            if h.level >= 3 {
                take += 1;
                continue;
            }
            let (i0, i1, k0, k1) = range(h);
            let n = (i1 - i0 + 1) * (k1 - k0 + 1);
            if total + n > budget {
                break;
            }
            total += n;
            take += 1;
        }
        let chosen = || list.iter().rev().take(take).filter(|h| h.level < 3);
        // Count pass.
        self.counts.clear();
        self.counts.resize(BUCKETS, 0);
        for h in chosen() {
            let (i0, i1, k0, k1) = range(h);
            for a in i0..=i1 {
                for b in k0..=k1 {
                    if let Some(c) = self.counts.get_mut((h.level * HN + a) * HN + b) {
                        *c += 1;
                    }
                }
            }
        }
        self.starts.clear();
        let mut at = 0u32;
        for &c in &self.counts {
            self.starts.push(at);
            at += c;
        }
        // Standing cover cells.
        self.bits.clear();
        self.bits.resize(COVER_RECS * 4, 0);
        for (i, c) in arena.cells.iter().enumerate() {
            if arena.gone.get(i).copied().unwrap_or(true) {
                continue;
            }
            let g = (c.level * 6 + c.sector) * PER + (usize::from(c.iu) * NV + usize::from(c.iv)) * NY + usize::from(c.iy);
            if let Some(w) = self.bits.get_mut(g / BITS) {
                *w |= 1 << (g % BITS);
            }
        }
        let data = km.data_mut();
        data.clear();
        data.reserve(ENTRY0 + at as usize);
        data.push(head);
        data.push([d.fy[0], d.fy[1], d.fy[2], d.slab]);
        data.push([d.a_in, d.a_out, d.floor_slant, 0.0]);
        for pair in self.counts.chunks(2).zip(self.starts.chunks(2)) {
            let (c, s) = pair;
            let get = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0) as f32;
            data.push([get(s, 0), get(c, 0), get(s, 1), get(c, 1)]);
        }
        for w in self.bits.chunks(4) {
            let get = |i: usize| w.get(i).copied().unwrap_or(0) as f32;
            data.push([get(0), get(1), get(2), get(3)]);
        }
        // Fill pass (the counts become each bucket's cursor).
        data.resize(ENTRY0 + at as usize, [0.0; 4]);
        self.counts.iter_mut().for_each(|c| *c = 0);
        for h in chosen() {
            let (i0, i1, k0, k1) = range(h);
            for a in i0..=i1 {
                for b in k0..=k1 {
                    let bk = (h.level * HN + a) * HN + b;
                    let (Some(c), Some(&s)) = (self.counts.get_mut(bk), self.starts.get(bk)) else {
                        continue;
                    };
                    if let Some(r) = data.get_mut(ENTRY0 + (s + *c) as usize) {
                        *r = [h.p[0], h.p[1], h.p[2], h.r];
                    }
                    *c += 1;
                }
            }
        }
    }
}
