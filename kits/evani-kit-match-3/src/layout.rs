//! Screen layout from the canvas size (CSS pixels): where the board, the
//! HUD panel and Puff go. Landscape puts the HUD left of the board (Puff
//! under it); portrait puts the HUD above the board (Puff at its right).
//! The DOM HUD is placed from [`Layout::hud`], so canvas and DOM agree.

/// Rectangles are `[x, y, w, h]` in CSS pixels from the top-left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Layout {
    pub viewport: [f32; 2],
    pub portrait: bool,
    /// Cell size in CSS pixels.
    pub cell: f32,
    /// Top-left corner of cell (0, 0).
    pub origin: [f32; 2],
    /// The board's cells (without the frame).
    pub board: [f32; 4],
    /// The HUD panel.
    pub hud: [f32; 4],
    /// The HUD unit (`--u`, CSS px) the page sizes the panel's content by.
    /// The panel is always at least as tall as that content needs at this
    /// unit ([`landscape_hud_h`], [`portrait_hud_h`]).
    pub unit: f32,
    /// Puff: centre x, ground y (feet), size.
    pub mascot: [f32; 3],
    /// Board columns and rows this layout was made for.
    pub dims: [usize; 2],
    /// Landscape: the height of the slot for combo callouts, between the
    /// HUD panel's 8 px shadow and the top of Puff's reach
    /// ([`crate::hud::callout_slot`]); 0 in portrait, where the callout has
    /// the strip above the board.
    pub callout_slot: f32,
}

/// Size limits for a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellLimits {
    pub min_px: f32,
    pub max_px: f32,
}

impl Default for CellLimits {
    fn default() -> Self {
        Self { min_px: 28.0, max_px: 96.0 }
    }
}

/// How high Puff reaches above his ground line, in units of his size: the
/// sprout star at the top of his highest hop (cheering), outline included
/// (`draw::mascot`; a draw test holds it). The landscape HUD panel ends
/// above that.
pub const MASCOT_REACH: f32 = 1.6;

/// The landscape HUD panel's height is `HUD_UNITS` HUD units (`--u`) plus
/// [`HUD_FIXED_PX`]: its content (level name and pause, moves, score and
/// star bar, the goal chips in one row and the banner) with a little air,
/// so the panel is no taller than it needs to be. Measured in the page
/// (level 3, the level with two goal chips, banner showing;
/// `review/shots.mjs --only fit`, `shots.json` `need`): the content needs
/// 454 px at `--u` 30 (1920x1080), 387 at 25.2, 364 at 23.6, 299 at 18.9,
/// 250 at 15 and 229 at 13.3, a line of 13.45 u + 50 px (the 50 px is
/// the ink borders, panel 2 x 4 px and moves box, score box, star bar and
/// goal chip 2 x 3 px each, plus the labels that never go below 10 px).
/// 13.6 u + 50 px is that line with about 4 px of air, so the panel is
/// never shorter than its content; the tests hold the measured points and
/// pin the rule against them.
pub const HUD_UNITS: f32 = 13.6;

/// The part of the landscape panel's content that does not scale with
/// `--u` (see [`HUD_UNITS`]).
pub const HUD_FIXED_PX: f32 = 50.0;

/// The portrait strip's content height is `PORTRAIT_HUD_UNITS` HUD units
/// plus [`PORTRAIT_HUD_FIXED_PX`]: the strip's three rows (level name and
/// pause, MOVES and SCORE side by side, the goal chips) measured in the
/// page at five phone and tablet sizes, goal chips included: 150 px at
/// `--u` 16.4, 160 at 18.4, 196 at 24.4, 206 at 25.8, 213 at 26.9 and 225
/// at 28.7, a line of 6.1 u + 50 px (the banner sits below the strip, out
/// of it). 6.2 u + 50 px is that line with 2 to 3 px of air.
pub const PORTRAIT_HUD_UNITS: f32 = 6.2;

/// The part of the portrait strip's content that does not scale with `--u`.
pub const PORTRAIT_HUD_FIXED_PX: f32 = 50.0;

/// The portrait banner hangs this many HUD units below the strip (its text
/// ends about 1.4 u down; measured at 320x568), and the board starts
/// [`PORTRAIT_BANNER_CLEAR_PX`] further down, so the banner never touches a
/// piece.
pub const PORTRAIT_BANNER_GAP_UNITS: f32 = 1.5;

/// Clear space between the banner and the board's frame (CSS px).
pub const PORTRAIT_BANNER_CLEAR_PX: f32 = 6.0;

/// The smallest HUD unit (`--u`, CSS px) a landscape panel is given room
/// for: below it the panel's small labels get hard to read on a phone held
/// sideways. On a short screen Puff shrinks to make that room
/// ([`layout`]); only a screen too short even then gets a smaller unit.
pub const MIN_LANDSCAPE_UNIT: f32 = 15.0;

/// The smallest Puff (in units of the screen height) a short landscape
/// screen shrinks him to when making room for the panel and the callout
/// slot above him.
const MIN_MASCOT_VH: f32 = 0.05;

/// The smallest Puff (in units of the screen height) that gives up size just
/// to leave the panel its full height (a screen with room for the panel
/// without that keeps him as big as it likes).
const FULL_PANEL_MASCOT_VH: f32 = 0.18;

/// The HUD unit (`--u`, CSS px) of a landscape panel `hud_w` wide.
pub fn landscape_unit(hud_w: f32) -> f32 {
    (hud_w / 13.0).clamp(13.0, 30.0)
}

/// The landscape panel's height for HUD unit `u`.
pub fn landscape_hud_h(u: f32) -> f32 {
    HUD_UNITS * u + HUD_FIXED_PX
}

/// The portrait strip's height for HUD unit `u`.
pub fn portrait_hud_h(u: f32) -> f32 {
    PORTRAIT_HUD_UNITS * u + PORTRAIT_HUD_FIXED_PX
}

/// The HUD unit (`--u`, CSS px) of the portrait strip: the strip's width
/// in 15 units, or smaller on a short screen, at most 34.
pub fn portrait_unit(vw: f32, vh: f32, margin: f32) -> f32 {
    (((vh * 0.23).clamp(130.0, 290.0) - margin) / 7.4).min((vw - 2.0 * margin) / 15.0).clamp(12.0, 34.0)
}

/// Lay out a `bw` x `bh` board in a `vw` x `vh` canvas.
pub fn layout(vw: f32, vh: f32, bw: usize, bh: usize, lim: CellLimits) -> Layout {
    let vw = vw.max(64.0);
    let vh = vh.max(64.0);
    let (bwf, bhf) = (bw.max(1) as f32, bh.max(1) as f32);
    let portrait = vh > vw * 1.05;
    let margin = (vw.min(vh) * 0.03).clamp(8.0, 32.0);
    // Room for the board frame around the cells.
    let frame = |cell: f32| cell * 0.35;
    let (mut hud, area, mut mascot, hud_unit);
    let mut callout_slot = 0.0;
    if portrait {
        // HUD across the top, the board under it, Puff in the strip below.
        // The HUD unit comes first (from the strip's width and a share of
        // the height), then the strip is as tall as its content needs at
        // that unit (`portrait_hud_h`), so the goal chips in its last row
        // never run past its border; the board and Puff take the rest.
        let unit = portrait_unit(vw, vh, margin);
        let hud_h = portrait_hud_h(unit) + margin;
        let m = (vh * 0.14).min(vw * 0.32).min(190.0);
        mascot = [vw * 0.5, vh - margin, m];
        hud = [margin, margin * 0.5, vw - 2.0 * margin, hud_h - margin];
        hud_unit = unit;
        // Between the strip and the board sits the banner ("Few moves
        // left!", `--u` * 0.95 text hanging 1.4 u under the strip, index.html
        // `.banner`): the gap holds it with a few px clear of the board.
        let top = hud_h - margin * 0.5 + margin.max(PORTRAIT_BANNER_GAP_UNITS * unit + PORTRAIT_BANNER_CLEAR_PX);
        area = [margin, top, vw - 2.0 * margin, vh - top - m * 1.15 - 1.5 * margin];
    } else {
        let hud_w = (vw * 0.24).clamp(200.0, 420.0);
        // Puff is drawn from 64 px atlas shapes: kept under 190 px so it stays crisp.
        let m = (hud_w * 0.62).min(vh * 0.24).min(190.0);
        // A compact panel: as tall as its content (HUD_UNITS), and the
        // panel (with its 8 px drop shadow) ends above the callout slot and
        // Puff's reach. Panel, slot and Puff stand as one column, centred
        // on the board's mid line.
        // On a short screen (a phone held sideways) Puff shrinks first, so
        // the panel keeps room for at least MIN_LANDSCAPE_UNIT; only when
        // even a small Puff leaves too little does the panel get shorter,
        // its HUD unit shrinking with it (`Layout::hud_unit`) so the
        // content still fits.
        //
        // Between the panel's shadow and Puff's reach sits the slot for the
        // combo callouts (`hud::callout_slot`): they are Puff's shout, and
        // no callout may cover his sprout star. Puff gives up size for it
        // (first just so the panel keeps its full height, down to
        // FULL_PANEL_MASCOT_VH, then so it keeps MIN_LANDSCAPE_UNIT).
        callout_slot = crate::hud::callout_slot(vw, vh, hud_w);
        let avail = vh - 2.0 * margin - 8.0 - callout_slot;
        let full_h = landscape_hud_h(landscape_unit(hud_w));
        let m = m.min(((avail - full_h) / MASCOT_REACH).max(vh * FULL_PANEL_MASCOT_VH));
        let min_hud_h = landscape_hud_h(landscape_unit(hud_w).min(MIN_LANDSCAPE_UNIT));
        let m_room = (avail - min_hud_h) / MASCOT_REACH;
        let m = m.min(m_room.max(vh * MIN_MASCOT_VH));
        let room = avail - m * MASCOT_REACH;
        let hud_h = room.min(full_h);
        let top = margin + (room - hud_h) * 0.5;
        mascot = [margin + hud_w * 0.5, top + hud_h + 8.0 + callout_slot + m * MASCOT_REACH, m];
        hud = [margin, top, hud_w, hud_h];
        // The width's unit, or smaller when the panel is short of room.
        hud_unit = landscape_unit(hud_w).min(((hud_h - HUD_FIXED_PX) / HUD_UNITS).max(4.0));
        area = [hud_w + 2.0 * margin, margin, vw - hud_w - 3.0 * margin, vh - 2.0 * margin];
    }
    let fit = (area[2] / (bwf + 0.7)).min(area[3] / (bhf + 0.7));
    let cell = fit.min(lim.max_px).max(lim.min_px.min(fit)).max(4.0);
    let (w, h) = (cell * bwf, cell * bhf);
    let mut origin = [
        (area[0] + (area[2] - w) * 0.5).max(area[0] + frame(cell) * 0.5),
        (area[1] + (area[3] - h) * 0.5).max(area[1] + frame(cell) * 0.5),
    ];
    if !portrait {
        // No empty gap: the panel column and the board sit side by side,
        // a fixed gap apart, and the pair is centred on the screen.
        let gap = 2.0 * margin + frame(cell) * 0.5;
        let left = ((vw - (hud[2] + gap + w)) * 0.5).max(margin);
        hud[0] = left;
        mascot[0] = left + hud[2] * 0.5;
        origin[0] = left + hud[2] + gap;
    }
    Layout {
        viewport: [vw, vh],
        portrait,
        cell,
        origin,
        board: [origin[0], origin[1], w, h],
        hud,
        unit: hud_unit,
        mascot,
        dims: [bw, bh],
        callout_slot,
    }
}

impl Layout {
    /// The HUD unit (`--u`, CSS px) the page sizes the panel's text by.
    pub fn hud_unit(&self) -> f32 {
        self.unit
    }

    /// The height the HUD's content needs at its unit: the panel is never
    /// shorter than this (a test holds it against the measured heights).
    pub fn hud_need(&self) -> f32 {
        if self.portrait {
            portrait_hud_h(self.unit)
        } else {
            landscape_hud_h(self.unit)
        }
    }

    /// Centre of cell `(x, y)` (fractional cells allowed).
    pub fn center(&self, x: f32, y: f32) -> [f32; 2] {
        [self.origin[0] + (x + 0.5) * self.cell, self.origin[1] + (y + 0.5) * self.cell]
    }

    /// The cell under a point, if it is on the board.
    pub fn hit(&self, p: [f32; 2]) -> Option<(i32, i32)> {
        if self.cell <= 0.0 {
            return None;
        }
        let x = ((p[0] - self.origin[0]) / self.cell).floor();
        let y = ((p[1] - self.origin[1]) / self.cell).floor();
        let (bw, bh) = (self.dims[0] as f32, self.dims[1] as f32);
        (x >= 0.0 && y >= 0.0 && x < bw && y < bh).then_some((x as i32, y as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(r: [f32; 4], vw: f32, vh: f32) -> bool {
        r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= vw + 0.01 && r[1] + r[3] <= vh + 0.01
    }

    fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
        a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    }

    #[test]
    fn fits_desktop_and_phones_without_overlap() {
        for (vw, vh) in [(1920.0, 1080.0), (1280.0, 720.0), (2560.0, 1440.0), (390.0, 844.0), (360.0, 640.0), (844.0, 390.0), (768.0, 1024.0)] {
            for (bw, bh) in [(8, 8), (9, 9)] {
                let l = layout(vw, vh, bw, bh, CellLimits::default());
                assert!(inside(l.board, vw, vh), "{vw}x{vh}: board {:?}", l.board);
                assert!(inside(l.hud, vw, vh), "{vw}x{vh}: hud {:?}", l.hud);
                assert!(!overlap(l.board, l.hud), "{vw}x{vh}: {:?} {:?}", l.board, l.hud);
                let m = [l.mascot[0] - l.mascot[2] * 0.5, l.mascot[1] - l.mascot[2], l.mascot[2], l.mascot[2]];
                assert!(!overlap(l.board, m), "{vw}x{vh}: mascot {m:?} board {:?}", l.board);
                assert!(l.cell >= 28.0 || vw < 400.0, "{vw}x{vh}: cell {}", l.cell);
                assert_eq!(l.portrait, vh > vw * 1.05);
            }
        }
        // Landscape: the HUD panel and its 8 px shadow end above the
        // callout slot, which ends above Puff's reach (his highest hop with
        // the sprout star).
        for (vw, vh) in [(1920.0, 1080.0), (1280.0, 720.0), (1366.0, 768.0), (2560.0, 1440.0), (844.0, 390.0)] {
            let l = layout(vw, vh, 8, 8, CellLimits::default());
            let [_, gy, m] = l.mascot;
            assert!(l.hud[1] + l.hud[3] + 8.0 + l.callout_slot <= gy - m * MASCOT_REACH + 0.01, "{vw}x{vh}: hud {:?} mascot {:?}", l.hud, l.mascot);
            // The panel is tall enough for its content at its HUD unit,
            // even when the room is short (the unit shrinks instead).
            assert!(l.hud[3] + 0.01 >= landscape_hud_h(l.hud_unit()), "{vw}x{vh}: hud {:?} unit {}", l.hud, l.hud_unit());
        }
        // A phone held sideways: Puff shrinks so the panel's text keeps a
        // readable unit, and he stays under the panel inside the screen.
        for (vw, vh) in [(844.0, 390.0), (740.0, 360.0), (667.0, 375.0), (932.0, 430.0), (640.0, 360.0)] {
            let l = layout(vw, vh, 8, 8, CellLimits::default());
            assert!(l.hud_unit() >= MIN_LANDSCAPE_UNIT - 0.01, "{vw}x{vh}: unit {}", l.hud_unit());
            assert!(l.mascot[2] >= vh * MIN_MASCOT_VH - 0.01 && l.mascot[1] <= vh + 0.01, "{vw}x{vh}: mascot {:?}", l.mascot);
        }
        // Desktop Puff gives up some size for the callout slot above him
        // (so the panel keeps its full height), never below 18 % of the
        // screen's height; at 1080p there is room for all of it.
        let l = layout(1280.0, 720.0, 8, 8, CellLimits::default());
        assert!(l.mascot[2] >= 720.0 * FULL_PANEL_MASCOT_VH - 0.01 && l.mascot[2] <= 720.0 * 0.24, "720p mascot {:?}", l.mascot);
        assert!((l.hud_unit() - landscape_unit(l.hud[2])).abs() < 0.01, "720p keeps the panel's full unit");
        let l = layout(1920.0, 1080.0, 8, 8, CellLimits::default());
        assert!((l.mascot[2] - 190.0).abs() < 0.01, "1080p mascot {:?}", l.mascot);
        assert!(l.callout_slot > 0.0);
        assert_eq!(layout(390.0, 844.0, 8, 8, CellLimits::default()).callout_slot, 0.0, "portrait has no slot");
        // Big screens cap the cell size.
        assert_eq!(layout(3840.0, 2160.0, 8, 8, CellLimits::default()).cell, 96.0);
    }

    /// The content height the page measured for a HUD unit, by straight
    /// lines between the measured points (points are `(--u, px)`, sorted).
    fn measured_need(points: &[(f32, f32)], u: f32) -> f32 {
        let (first, last) = (points[0], points[points.len() - 1]);
        if u <= first.0 {
            return first.1;
        }
        if u >= last.0 {
            return last.1 + (u - last.0) * (last.1 - points[points.len() - 2].1) / (last.0 - points[points.len() - 2].0);
        }
        let i = points.iter().position(|p| p.0 >= u).unwrap_or(1).max(1);
        let (a, b) = (points[i - 1], points[i]);
        a.1 + (u - a.0) * (b.1 - a.1) / (b.0 - a.0)
    }

    /// What the page measured (level 3, two goal chips, banner showing; the
    /// panel's `need` in `review/shots.mjs`'s `shots.json`, and the bottom of
    /// the goal chips plus the strip's padding and border in portrait).
    const MEASURED_LANDSCAPE: [(f32, f32); 7] = [(13.3, 229.0), (15.0, 250.0), (18.9, 299.0), (23.6, 364.0), (25.2, 387.0), (29.5, 446.0), (30.0, 454.0)];
    const MEASURED_PORTRAIT: [(f32, f32); 6] = [(16.4, 150.0), (18.4, 160.0), (24.4, 196.0), (25.8, 206.0), (26.9, 213.0), (28.7, 225.0)];

    /// The rule behind the HUD panel: it is at least as tall as the content
    /// the page measured at its HUD unit, at every size (a short screen
    /// shrinks the unit and Puff instead of cutting the panel).
    #[test]
    fn panel_is_at_least_as_tall_as_its_measured_content() {
        let landscape = [
            (568.0, 320.0), (640.0, 360.0), (667.0, 375.0), (740.0, 360.0), (844.0, 390.0), (932.0, 430.0), (1024.0, 600.0),
            (1280.0, 720.0), (1280.0, 800.0), (1366.0, 768.0), (1600.0, 900.0), (1920.0, 1080.0), (2560.0, 1440.0),
        ];
        let portrait = [(320.0, 568.0), (360.0, 640.0), (390.0, 844.0), (412.0, 915.0), (430.0, 932.0), (768.0, 1024.0)];
        for (sizes, points) in [(&landscape[..], &MEASURED_LANDSCAPE[..]), (&portrait[..], &MEASURED_PORTRAIT[..])] {
            for &(vw, vh) in sizes {
                for (bw, bh) in [(8, 8), (9, 9)] {
                    let l = layout(vw, vh, bw, bh, CellLimits::default());
                    let need = measured_need(points, l.unit);
                    assert!(l.hud[3] + 0.01 >= need, "{vw}x{vh}: panel {:?} is shorter than the {need} px its content needs at --u {}", l.hud, l.unit);
                    assert!(l.hud[3] + 0.01 >= l.hud_need(), "{vw}x{vh}: panel {:?} under the model's {}", l.hud, l.hud_need());
                    // Portrait: the banner under the strip has room before the board.
                    if l.portrait {
                        let gap = l.board[1] - (l.hud[1] + l.hud[3]);
                        assert!(gap + 0.01 >= PORTRAIT_BANNER_GAP_UNITS * l.unit + PORTRAIT_BANNER_CLEAR_PX, "{vw}x{vh}: banner gap {gap} at --u {}", l.unit);
                    }
                    // The text stays readable: a unit of at least 13 px.
                    assert!(l.unit >= 13.0 - 0.01, "{vw}x{vh}: unit {}", l.unit);
                }
            }
        }
        // The model itself is never under a measured point (no air to lose).
        for (u, px) in MEASURED_LANDSCAPE {
            assert!(landscape_hud_h(u) >= px, "landscape model under {u}: {px}");
        }
        for (u, px) in MEASURED_PORTRAIT {
            assert!(portrait_hud_h(u) >= px, "portrait model under {u}: {px}");
        }
    }

    /// Chris's desktop note: the board is the hero, the panel compact and
    /// no big empty gap between them.
    #[test]
    fn desktop_board_is_the_hero_with_a_compact_panel_beside_it() {
        for (vw, vh) in [(1920.0, 1080.0), (1280.0, 720.0), (1366.0, 768.0), (2560.0, 1440.0)] {
            let l = layout(vw, vh, 8, 8, CellLimits::default());
            let margin = (vw.min(vh) * 0.03).clamp(8.0, 32.0);
            // The board takes most of the height (or its crisp-art cap).
            assert!(l.board[3] >= vh * 0.65 || l.cell == 96.0, "{vw}x{vh}: board {:?}", l.board);
            // The gap from the panel to the board's cells is the margin
            // twice plus half the frame, not the leftover width.
            let gap = l.board[0] - (l.hud[0] + l.hud[2]);
            assert!((gap - (2.0 * margin + l.cell * 0.175)).abs() < 0.5, "{vw}x{vh}: gap {gap}");
            // The pair is centred: the same space left and right.
            let right = vw - (l.board[0] + l.board[2]);
            assert!((l.hud[0] - right).abs() < 1.0, "{vw}x{vh}: left {} right {right}", l.hud[0]);
            // The panel is as tall as its content, not the whole column.
            assert!(l.hud[3] <= landscape_hud_h(l.hud_unit()) + 0.01, "{vw}x{vh}: hud {:?}", l.hud);
            // Puff stands under the panel, inside the screen.
            assert!(l.mascot[1] <= vh - margin + 0.01 && (l.mascot[0] - (l.hud[0] + l.hud[2] * 0.5)).abs() < 0.01);
        }
        let l = layout(1920.0, 1080.0, 8, 8, CellLimits::default());
        assert!(l.board[0] - (l.hud[0] + l.hud[2]) < 100.0, "1080p gap");
        assert_eq!(l.hud_unit(), 30.0);
    }

    #[test]
    fn hit_and_center_agree() {
        let l = layout(1280.0, 720.0, 8, 8, CellLimits::default());
        for (x, y) in [(0, 0), (7, 7), (3, 5)] {
            let c = l.center(x as f32, y as f32);
            assert_eq!(l.hit(c), Some((x, y)));
        }
        assert_eq!(l.hit([0.0, 0.0]), None);
        assert_eq!(l.hit([l.board[0] + l.board[2] + 2.0, l.board[1] + 4.0]), None);
    }
}
