//! The eight checks. Each is a pure function `check(&Ctx) -> Vec<Issue>`.

pub mod bot;
pub mod camera;
pub mod grounding;
pub mod joins;
pub mod overlaps;
pub mod pixels;
pub mod popups;
pub mod seams;

/// Joins sorted integers into runs: `[1,2,3,7]` -> `[(1,3),(7,7)]`.
pub fn runs(v: &[i32]) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for &x in v {
        match out.last_mut() {
            Some(r) if x == r.1 + 1 => r.1 = x,
            _ => out.push((x, x)),
        }
    }
    out
}

pub fn fmt_px(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

pub fn fmt_rect(r: crate::scene::Rect) -> String {
    format!("x {} y {} ({} x {} px)", fmt_px(r[0]), fmt_px(r[1]), fmt_px(r[2]), fmt_px(r[3]))
}

#[cfg(test)]
pub mod testkit {
    //! Tiny synthetic scenes for the unit tests.
    use crate::scene::{Group, Kind, Object, Rect, Scene, Shape, TileDef};

    /// A `w` x `h` map; rows from `ground` down are earth (with the edges
    /// an autotiler would draw).
    pub fn ground(w: i32, h: i32, ground: i32) -> Scene {
        let mut s = Scene { tile_size: 16.0, map: [w as u32, h as u32], pixel_scale: 4.0, ..Scene::default() };
        s.groups.insert("earth".into(), Group { caps: "tlr".into(), meets: vec![] });
        for y in ground..h {
            for x in 0..w {
                s.tiles.push(earth(x, y, if y == ground { "t" } else { "" }));
            }
        }
        s
    }

    pub fn earth(x: i32, y: i32, edges: &str) -> TileDef {
        TileDef { x, y, id: "#".into(), shape: Shape::Full, group: Some("earth".into()), edges: edges.into(), layer: 60, ..TileDef::default() }
    }

    pub fn remove_tile(s: &mut Scene, x: i32, y: i32) {
        s.tiles.retain(|t| !(t.x == x && t.y == y));
    }

    pub fn obj(name: &str, kind: Kind, rect: Rect) -> Object {
        Object { name: name.into(), kind, rect, layer: 50, ..Object::default() }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn runs_join() {
        assert_eq!(super::runs(&[1, 2, 3, 7, 9, 10]), vec![(1, 3), (7, 7), (9, 10)]);
    }
}
