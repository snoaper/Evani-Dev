//! The geometry stage's recognisability check: a styled shape must still be
//! its asset. For one asset and one generated shape:
//!
//! - **Silhouette**: the intersection over union of the source's and the
//!   shape's masks from the front, the side and three-quarter (the
//!   source's framing for both) is at least [`MIN_IOU`] in every view
//!   ([`MIN_IOU_VOXEL`] for voxels: conservative cubes grow the outline by
//!   up to a cube all round, by design, and a thin leg's mask is a few
//!   pixels wide, so the same growth costs it more overlap).
//! - **Tagged parts**: every connected piece of every thin region (each
//!   leg, ear, the mane, the tail, a horn) and of every region keeping the
//!   source's colour (each eye) still has triangles of its region where it
//!   was, spanning at least [`MIN_SPAN`] of its length.
//! - **Pieces**: voxels are one piece; other shapes have no more pieces
//!   than the source (nothing breaks off).
//!
//! `cargo xtask style-bake --check` runs it over every asset with a source
//! and every geometry style, and the crate's tests hold the horse to it.

use evani_studio::assets::AssetRow;
use evani_studio::geo::{SourceMesh, StyledMesh};
use evani_studio::styles::GeometryKind;

use crate::math::{V3, add, bounds, dot, scale};
use crate::view::{Frame, Soup, VIEWS, iou, silhouette};
use crate::voxel::tagged_pieces;

/// The least silhouette overlap per view, for shapes that follow the
/// surface (low-poly, papercraft, clay, yarn; the horse measures 0.80 to
/// 0.94).
pub const MIN_IOU: f32 = 0.75;
/// The least for voxels (the horse at its own cube size measures 0.72 to
/// 0.81).
pub const MIN_IOU_VOXEL: f32 = 0.65;
/// A thin piece must keep this much of its length.
pub const MIN_SPAN: f32 = 0.7;
/// The masks' size (the source fills it, with a margin).
const MASK_W: usize = 160;
const MASK_H: usize = 120;

/// One asset x shape result.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeReport {
    pub asset: String,
    pub style: String,
    pub kind: GeometryKind,
    pub triangles: usize,
    /// Silhouette IoU per view (front, side, three-quarter).
    pub iou: [f32; 3],
    /// Tagged pieces checked (thin and kept regions), and the ones that
    /// fail (region id, why).
    pub thin_pieces: usize,
    pub thin_failures: Vec<String>,
    /// Pieces in the source and in the shape.
    pub source_pieces: usize,
    pub shape_pieces: usize,
}

impl ShapeReport {
    /// Every failure as a line (empty: recognisable).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let need = if self.kind == GeometryKind::Voxel { MIN_IOU_VOXEL } else { MIN_IOU };
        for (v, x) in VIEWS.iter().zip(self.iou) {
            if x < need {
                out.push(format!("{} {}: the {} silhouette overlaps the source only {x:.2} (needs {need})", self.asset, self.style, v.name));
            }
        }
        for f in &self.thin_failures {
            out.push(format!("{} {}: {f}", self.asset, self.style));
        }
        let pieces_ok = if self.kind == GeometryKind::Voxel { self.shape_pieces == 1 } else { self.shape_pieces <= self.source_pieces.max(1) && self.shape_pieces >= 1 };
        if !pieces_ok {
            out.push(format!("{} {}: {} pieces from a source of {} (voxels must be one piece; nothing may break off)", self.asset, self.style, self.shape_pieces, self.source_pieces));
        }
        out
    }
}

/// Pieces of a triangle soup, joined through shared (welded) corners.
fn soup_pieces(tris: &[[V3; 3]]) -> usize {
    let mut keys: Vec<([i32; 3], u32)> = Vec::with_capacity(tris.len() * 3);
    for (t, tri) in tris.iter().enumerate() {
        for p in tri {
            keys.push((p.map(|x| (x * 1e4).round() as i32), t as u32));
        }
    }
    keys.sort_unstable();
    let mut parent: Vec<u32> = (0..tris.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    for w in keys.windows(2) {
        if w[0].0 == w[1].0 {
            let (a, b) = (find(&mut parent, w[0].1), find(&mut parent, w[1].1));
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
    }
    (0..tris.len() as u32).filter(|&t| find(&mut parent, t) == t).count()
}

/// Check `shape` (made in `style`) against its source.
pub fn check(src: &SourceMesh, asset: &AssetRow, style: &str, kind: GeometryKind, shape: &StyledMesh) -> ShapeReport {
    let a = Soup::from_source(src);
    let b = Soup::from_styled(shape);
    let mut ious = [0.0; 3];
    for (k, v) in VIEWS.iter().enumerate() {
        let f = Frame::fit(&a, *v, MASK_W, MASK_H, 0.05);
        ious[k] = iou(&silhouette(&a, &f), &silhouette(&b, &f));
    }
    let tris = src.indices.as_chunks::<3>().0;
    let pieces = tagged_pieces(src, asset);
    let mut failures = Vec::new();
    for (region, piece) in &pieces {
        let name = asset.regions.get(usize::from(*region)).map_or("?", |r| r.id.as_str());
        let pts: Vec<V3> = piece.iter().flat_map(|&t| tris[t].map(|v| src.positions[v as usize])).collect();
        let (lo, hi) = bounds(&pts);
        let axis = long_axis(&pts);
        let (s_lo, s_hi) = pts.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(dot(*p, axis)), h.max(dot(*p, axis))));
        // The box round the piece, a little larger (a voxel sticks out by
        // up to a cube).
        let pad = 0.06;
        let inside = |p: V3| (0..3).all(|k| p[k] >= lo[k] - pad && p[k] <= hi[k] + pad);
        let (mut found, mut lo_d, mut hi_d) = (0usize, f32::MAX, f32::MIN);
        for (t, r) in b.tris.iter().zip(&b.region) {
            if r != region {
                continue;
            }
            let c = scale(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
            if !inside(c) {
                continue;
            }
            found += 1;
            for p in t {
                let d = dot(*p, axis);
                lo_d = lo_d.min(d);
                hi_d = hi_d.max(d);
            }
        }
        let want = s_hi - s_lo;
        let got = (hi_d.min(s_hi + pad) - lo_d.max(s_lo - pad)).max(0.0);
        if found == 0 {
            failures.push(format!("the {name} piece near {:?} is gone", centre_of(lo, hi)));
        } else if want > 0.0 && got < MIN_SPAN * want {
            failures.push(format!("the {name} piece near {:?} keeps only {:.0}% of its length", centre_of(lo, hi), 100.0 * got / want));
        }
    }
    let src_tris: Vec<[V3; 3]> = tris.iter().map(|t| t.map(|v| src.positions[v as usize])).collect();
    ShapeReport {
        asset: asset.id.clone(),
        style: style.to_string(),
        kind,
        triangles: shape.triangles(),
        iou: ious,
        thin_pieces: pieces.len(),
        thin_failures: failures,
        source_pieces: soup_pieces(&src_tris),
        shape_pieces: soup_pieces(&b.tris),
    }
}

fn centre_of(lo: V3, hi: V3) -> [f32; 3] {
    scale(add(lo, hi), 0.5).map(|x| (x * 100.0).round() / 100.0)
}

fn long_axis(pts: &[V3]) -> V3 {
    let n = pts.len().max(1) as f64;
    let mean = pts.iter().fold([0.0f64; 3], |m, p| [m[0] + f64::from(p[0]) / n, m[1] + f64::from(p[1]) / n, m[2] + f64::from(p[2]) / n]);
    let mut cov = [[0.0f64; 3]; 3];
    for p in pts {
        let d = [f64::from(p[0]) - mean[0], f64::from(p[1]) - mean[1], f64::from(p[2]) - mean[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j] / n;
            }
        }
    }
    crate::math::eigen_sym(cov).1[0].map(|x| x as f32)
}
