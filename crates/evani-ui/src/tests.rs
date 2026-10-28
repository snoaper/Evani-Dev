use super::*;

// ---- a fixed-width "font": 8 px per character at size 16 ---------------------

/// Measure `t` wrapped at `max_w` (0: one line): half the line height per
/// character, words wrap greedily at spaces.
fn mono(t: &Text<'_>, max_w: f32) -> [f32; 2] {
    let cw = t.size * 0.5;
    if max_w <= 0.0 {
        return [t.s.chars().count() as f32 * cw, t.size];
    }
    let cap = (max_w / cw).floor().max(1.0) as usize;
    let (mut lines, mut cur, mut widest) = (1usize, 0usize, 0usize);
    for word in t.s.split(' ') {
        let l = word.chars().count();
        let need = if cur == 0 { l } else { cur + 1 + l };
        if cur > 0 && need > cap {
            widest = widest.max(cur);
            lines += 1;
            cur = l;
        } else {
            cur = need;
        }
    }
    widest = widest.max(cur);
    [widest as f32 * cw, lines as f32 * t.size]
}

const INK: [f32; 4] = [1.0; 4];
const PANEL: [f32; 4] = [0.1, 0.1, 0.2, 1.0];
const GEM: &str = "Collect all the gems before time runs out";

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

/// The HUD panel the golden test checks. Returns the ids in a fixed order:
/// root, header, title, spacer, icon, body, left, gem text, right, footer.
fn hud<'a>(ui: &mut Ui<'a>) -> [NodeId; 10] {
    let root = ui.open(Style {
        size: [Size::Fixed(300.0), Size::Fixed(200.0)],
        pad: [10.0; 4],
        gap: 6.0,
        fill: Some(PANEL),
        align: [Align::Center, Align::Start],
        ..Style::COLUMN
    });
    let header = ui.open(Style { size: [Size::Grow(1.0), Size::Fit], gap: 4.0, align: [Align::Start, Align::Center], ..Style::ROW });
    let title = ui.text(Text::new("Score", 16.0, INK));
    let spacer = ui.open(Style { size: [Size::Grow(1.0), Size::Fit], ..Style::ROW });
    ui.close();
    let icon = ui.open(Style { size: [Size::Fixed(20.0), Size::Fixed(20.0)], fill: Some(INK), ..Style::ROW });
    ui.close();
    ui.close();
    let body = ui.open(Style { size: [Size::Grow(1.0), Size::Grow(1.0)], gap: 6.0, ..Style::ROW });
    let left = ui.open(Style { size: [Size::Grow(1.0), Size::Grow(1.0)], pad: [4.0; 4], fill: Some(PANEL), ..Style::COLUMN });
    let gem = ui.text(Text::new(GEM, 16.0, INK));
    ui.close();
    let right = ui.open(Style { size: [Size::Fixed(60.0), Size::Grow(1.0)], fill: Some(INK), ..Style::COLUMN });
    ui.close();
    ui.close();
    let footer = ui.text(Text::new("Press A", 16.0, INK));
    ui.close();
    [root, header, title, spacer, icon, body, left, gem, right, footer]
}

#[test]
fn layout_matches_golden_boxes() {
    let mut ui = Ui::new();
    let [root, header, title, spacer, icon, body, left, gem, right, footer] = hud(&mut ui);
    ui.layout([10.0, 20.0], [300.0, 200.0], mono);
    let golden = [
        (root, rect(10.0, 20.0, 300.0, 200.0)),
        (header, rect(20.0, 30.0, 280.0, 20.0)),
        (title, rect(20.0, 32.0, 40.0, 16.0)),
        (spacer, rect(64.0, 40.0, 212.0, 0.0)),
        (icon, rect(280.0, 30.0, 20.0, 20.0)),
        (body, rect(20.0, 56.0, 280.0, 132.0)),
        // 214 wide: the 336 px of text and padding shrank by the 122 px the row was short.
        (left, rect(20.0, 56.0, 214.0, 132.0)),
        // Wrapped to the 206 px inside the padding: two lines.
        (gem, rect(24.0, 60.0, 206.0, 32.0)),
        (right, rect(240.0, 56.0, 60.0, 132.0)),
        // Centred in the 280 px column.
        (footer, rect(132.0, 194.0, 56.0, 16.0)),
    ];
    for (id, want) in golden {
        assert_eq!(ui.rect(id), Some(want), "box {id}");
    }
    // Paint order: parents first; boxes without a fill draw nothing.
    let cmds: Vec<Cmd<'_>> = ui.commands().collect();
    let kinds: Vec<(bool, u8)> = cmds.iter().map(|c| matches!(c, Cmd::Rect { .. })).zip(cmds.iter().map(depth_of)).collect();
    assert_eq!(kinds, [(true, 0), (false, 2), (true, 2), (true, 2), (false, 3), (true, 2), (false, 1)]);
    // The wrapped run is told the width it was measured at.
    let Some(Cmd::Text { pos, max_width, .. }) = cmds.get(4) else { panic!("gem text") };
    assert_eq!((*pos, *max_width), ([24.0, 60.0], 206.0));
    // A centre-aligned run anchors at the middle of its box.
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(100.0), Size::Fit], ..Style::COLUMN });
    ui.text(Text { align: Align::Center, width: Size::Grow(1.0), ..Text::new("Hi", 16.0, INK) });
    ui.layout([0.0; 2], [100.0, 50.0], mono);
    let Some(Cmd::Text { pos, .. }) = ui.commands().next() else { panic!("hi") };
    assert_eq!(pos, [50.0, 0.0]);
}

fn depth_of(c: &Cmd<'_>) -> u8 {
    match c {
        Cmd::Rect { depth, .. } | Cmd::Text { depth, .. } => *depth,
    }
}

#[test]
fn same_tree_same_boxes() {
    let run = |ui: &mut Ui<'static>| -> Vec<(NodeId, Option<Rect>)> {
        let ids = hud(ui);
        ui.layout([10.0, 20.0], [300.0, 200.0], mono);
        ids.iter().map(|&id| (id, ui.rect(id))).collect()
    };
    let (mut a, mut b) = (Ui::new(), Ui::new());
    let first = run(&mut a);
    assert_eq!(first, run(&mut b), "two fresh trees differ");
    // Laying out again changes nothing (layout is a pure function of the tree).
    a.layout([10.0, 20.0], [300.0, 200.0], mono);
    let again: Vec<_> = first.iter().map(|&(id, _)| (id, a.rect(id))).collect();
    assert_eq!(first, again, "a second layout moved something");
    let (ca, cb): (Vec<_>, Vec<_>) = (a.commands().collect(), b.commands().collect());
    assert_eq!(ca, cb);
}

// ---- no allocation -------------------------------------------------------------

mod counting {
    #![allow(unsafe_code)]
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        pub static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub struct Counting;

    // SAFETY: every call forwards to the system allocator unchanged; the
    // only extra work is bumping a thread-local counter (const-initialised,
    // so touching it never allocates).
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            unsafe { System.realloc(p, l, n) }
        }
    }
}

#[global_allocator]
static ALLOC: counting::Counting = counting::Counting;

#[test]
fn layout_does_not_allocate() {
    let count = || counting::COUNT.with(std::cell::Cell::get);
    // The counter sees allocations at all (so a zero below means something).
    let probe = count();
    drop(std::hint::black_box(Box::new(7u8)));
    assert!(count() > probe, "the counting allocator is not counting");
    // Build, lay out (with wrapping, shrinking and a scroll box) and read
    // the commands, all between two readings of this thread's counter.
    let before = count();
    let mut ui = Ui::new();
    hud(&mut ui);
    ui.open(Style { size: [Size::Fixed(50.0), Size::Fixed(20.0)], scroll: [None, Some(5.0)], ..Style::COLUMN });
    ui.text(Text::new("one two three four five six", 16.0, INK));
    ui.close();
    ui.layout([0.0; 2], [320.0, 240.0], mono);
    let mut n = 0;
    for c in ui.commands() {
        n += usize::from(matches!(c, Cmd::Rect { .. }));
    }
    let after = count();
    assert!(n > 0 && ui.len() > 10, "the tree was empty");
    assert_eq!(after - before, 0, "building and laying out allocated {} times", after - before);
}

// ---- the rules -----------------------------------------------------------------

#[test]
fn grow_shares_free_room_by_weight() {
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(300.0), Size::Fixed(40.0)], gap: 10.0, ..Style::ROW });
    let a = ui.open(Style { size: [Size::Grow(1.0), Size::Grow(1.0)], ..Style::ROW });
    ui.close();
    let b = ui.open(Style { size: [Size::Grow(2.0), Size::Fixed(10.0)], ..Style::ROW });
    ui.close();
    let c = ui.open(Style { size: [Size::Fixed(40.0), Size::Fit], ..Style::ROW });
    ui.close();
    ui.layout([0.0; 2], [0.0; 2], mono);
    // 300 - 40 - 2 gaps of 10 = 240 free: 80 and 160.
    assert_eq!(ui.rect(a), Some(rect(0.0, 0.0, 80.0, 40.0)));
    assert_eq!(ui.rect(b), Some(rect(90.0, 0.0, 160.0, 10.0)));
    assert_eq!(ui.rect(c), Some(rect(260.0, 0.0, 40.0, 0.0)));
}

#[test]
fn text_wraps_in_a_narrow_column_and_never_below_its_longest_word() {
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(100.0), Size::Fit], pad: [5.0; 4], ..Style::COLUMN });
    let t = ui.text(Text::new("aa bb cc dd ee ff", 16.0, INK));
    ui.close();
    ui.layout([0.0; 2], [400.0, 400.0], mono);
    // 90 px inside: 11 characters per line, so "aa bb cc dd" then "ee ff".
    assert_eq!(ui.rect(t), Some(rect(5.0, 5.0, 90.0, 32.0)));
    // A box narrower than the longest word stops shrinking the run at that word.
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(20.0), Size::Fit], ..Style::COLUMN });
    let t = ui.text(Text::new("tiny extraordinary", 16.0, INK));
    ui.close();
    ui.layout([0.0; 2], [400.0, 400.0], mono);
    assert_eq!(ui.rect(t).map(|r| r.w), Some(104.0));
    // In a row, fit text shrinks to the room and its neighbour keeps its size.
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(120.0), Size::Fit], gap: 8.0, ..Style::ROW });
    let a = ui.text(Text::new("alpha beta gamma delta", 16.0, INK));
    let b = ui.open(Style { size: [Size::Fixed(30.0), Size::Fixed(10.0)], ..Style::ROW });
    ui.close();
    ui.close();
    ui.layout([0.0; 2], [400.0, 400.0], mono);
    assert_eq!(ui.rect(a).map(|r| (r.w, r.h)), Some((82.0, 48.0)));
    assert_eq!(ui.rect(b), Some(rect(90.0, 0.0, 30.0, 10.0)));
}

#[test]
fn scroll_offsets_clamp_and_clip() {
    let build = |scroll: f32| {
        let mut ui = Ui::new();
        ui.open(Style { size: [Size::Fixed(100.0), Size::Fixed(50.0)], scroll: [None, Some(scroll)], fill: Some(PANEL), ..Style::COLUMN });
        for _ in 0..6 {
            ui.open(Style { size: [Size::Grow(1.0), Size::Fixed(20.0)], fill: Some(INK), ..Style::ROW });
            ui.close();
        }
        ui.layout([0.0; 2], [100.0, 50.0], mono);
        ui
    };
    // 6 rows of 20 in 50: 70 to scroll.
    let ui = build(0.0);
    assert_eq!(ui.max_scroll(0), [0.0, 70.0]);
    // Top: rows 0 and 1 whole, row 2 cut to the last 10 px, the rest gone.
    let rows: Vec<Rect> = ui.commands().filter_map(|c| if let Cmd::Rect { rect, .. } = c { Some(rect) } else { None }).collect();
    assert_eq!(rows, [rect(0.0, 0.0, 100.0, 50.0), rect(0.0, 0.0, 100.0, 20.0), rect(0.0, 20.0, 100.0, 20.0), rect(0.0, 40.0, 100.0, 10.0)]);
    // Too far down clamps to the end; a negative offset to the start.
    assert_eq!(build(999.0).rect(6), Some(rect(0.0, 50.0 - 20.0, 100.0, 20.0)));
    assert_eq!(build(-5.0).rect(1), Some(rect(0.0, 0.0, 100.0, 20.0)));
    // Half way: row 1 is cut at the top.
    let rows: Vec<Rect> = build(30.0).commands().filter_map(|c| if let Cmd::Rect { rect, .. } = c { Some(rect) } else { None }).collect();
    assert_eq!(rows[1], rect(0.0, 0.0, 100.0, 10.0));
    // A text run that is not wholly inside the box is left out.
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(100.0), Size::Fixed(24.0)], scroll: [None, Some(8.0)], ..Style::COLUMN });
    ui.text(Text::new("first", 16.0, INK));
    ui.text(Text::new("second", 16.0, INK));
    ui.close();
    ui.layout([0.0; 2], [100.0, 24.0], mono);
    // Offset 8: "first" spans -8..8 (cut), "second" 8..24 (inside).
    let shown: Vec<&str> = ui.commands().filter_map(|c| if let Cmd::Text { text, .. } = c { Some(text.s) } else { None }).collect();
    assert_eq!(shown, ["second"]);
}

#[test]
fn alignment_places_the_children_block() {
    let mut ui = Ui::new();
    ui.open(Style { size: [Size::Fixed(100.0), Size::Fixed(60.0)], gap: 4.0, align: [Align::End, Align::Center], ..Style::ROW });
    let a = ui.open(Style { size: [Size::Fixed(20.0), Size::Fixed(10.0)], ..Style::ROW });
    ui.close();
    let b = ui.open(Style { size: [Size::Fixed(20.0), Size::Fixed(30.0)], ..Style::ROW });
    ui.close();
    ui.close();
    ui.layout([0.0; 2], [100.0, 60.0], mono);
    // The block is 44 wide, flush right (x = 56); each child is centred in the 60.
    assert_eq!(ui.rect(a), Some(rect(56.0, 25.0, 20.0, 10.0)));
    assert_eq!(ui.rect(b), Some(rect(80.0, 15.0, 20.0, 30.0)));
}

#[test]
fn a_full_tree_drops_the_extra_boxes_and_stays_paired() {
    let mut ui = Ui::new();
    for _ in 0..CAP + 10 {
        ui.open(Style::ROW);
    }
    for _ in 0..CAP + 10 {
        ui.close();
    }
    assert_eq!((ui.len(), ui.dropped()), (CAP, 10));
    // Still usable: a text run after the closes lands at the top level.
    assert_eq!(ui.text(Text::new("x", 16.0, INK)), NONE);
    ui.layout([0.0; 2], [10.0, 10.0], mono);
    assert_eq!(ui.rect(NONE), None);
}

#[test]
fn unclosed_boxes_are_closed_by_layout_and_bad_numbers_do_not_panic() {
    let mut ui = Ui::new();
    let root = ui.open(Style { size: [Size::Grow(f32::NAN), Size::Fixed(f32::INFINITY)], scroll: [Some(f32::NAN), None], pad: [f32::NAN; 4], ..Style::COLUMN });
    ui.text(Text::new("still here", 16.0, INK));
    ui.layout([0.0; 2], [50.0, 50.0], mono);
    assert!(ui.rect(root).is_some());
    let _ = ui.commands().count();
}
