//! The box layout proof panel (S3-13b), only in a build with `ui` (the
//! tactical-fps ships without it, so its download is unchanged). A canvas HUD
//! panel built each frame with `evani-ui`: a header with a badge, a
//! progress bar, a paragraph that wraps, a scrolling list and two buttons.
//! `Ui::layout` sizes it from the viewport and the MSDF measure, and
//! [`paint`] draws the rectangles and text runs it returns. Build it with
//! `trunk build --features ui` and `cargo xtask fonts tactical-fps --dist <dist>`
//! (docs/UI_LAYOUT.md). `--features ui,text-proof` shows the S3-13a text
//! proof instead.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::time::Time;
use evani_render::text::{self, Align as TextAlign, FontId, TextStyle};
use evani_render::{RenderWorld, Sprite};
use evani_ui::{Align, Cmd, Rect, Size, Style, Text, Ui};

/// The `Stage::Frame` system that draws the panel (after the scene).
pub fn system() -> SystemDesc {
    SystemDesc::new("tactical-fps.hudpanel", Stage::Frame, draw)
}

/// First sprite layer of the panel; a box at depth `d` draws on `LAYER + 2d`
/// and its text on `LAYER + 2d + 1`, so inner boxes cover outer ones. On
/// `HUD_LAYER` and above, so the panel draws in the `ui` pass after the tone
/// map and post effects (docs/UI_LAYOUT.md "HUD after post").
const LAYER: i16 = evani_render::msdf::HUD_LAYER + 40;

// Linear colours (sprites tint in linear light). The panel draws after the
// tone map, so they land on screen as written: ink 1.0 is 255.
const INK: [f32; 4] = [1.0, 1.0, 0.97, 1.0];
const DIM: [f32; 4] = [0.62, 0.68, 0.80, 1.0];
const PANEL: [f32; 4] = [0.02, 0.025, 0.045, 1.0];
const ROW: [f32; 4] = [0.05, 0.065, 0.11, 1.0];
const TRACK: [f32; 4] = [0.08, 0.10, 0.16, 1.0];
const ACCENT: [f32; 4] = [0.10, 0.62, 0.40, 1.0];
const BUTTON: [f32; 4] = [0.04, 0.09, 0.26, 1.0];
const SWATCH: [[f32; 4]; 6] = [
    [0.90, 0.20, 0.12, 1.0],
    [0.95, 0.60, 0.05, 1.0],
    [0.20, 0.65, 0.20, 1.0],
    [0.10, 0.50, 0.90, 1.0],
    [0.55, 0.25, 0.85, 1.0],
    [0.90, 0.25, 0.55, 1.0],
];
const ITEMS: [(&str, &str); 6] =
    [("Red orbs", "12"), ("Amber orbs", "9"), ("Green orbs", "7"), ("Blue orbs", "4"), ("Violet orbs", "3"), ("Pink orbs", "1")];

const ROW_H: f32 = 30.0;
const ROW_GAP: f32 = 4.0;
const VISIBLE_ROWS: f32 = 4.0;

fn label(s: &str, size: f32, color: [f32; 4]) -> Text<'_> {
    Text::new(s, size, color)
}

/// Build the panel's tree for a `w` px wide viewport; `step` is how many
/// list rows the list is scrolled by.
fn build(ui: &mut Ui<'static>, w: f32, h: f32, step: u32) {
    let margin = 12.0;
    let panel_w = (w - 2.0 * margin).clamp(200.0, 340.0);
    // A short screen (a phone held sideways) shows three list rows and puts
    // the panel top left; otherwise four rows, bottom right. The debug
    // overlay and panel use the top right, the tactical-fps hint the bottom left.
    let short = h < 480.0;
    let rows = if short { 3.0 } else { VISIBLE_ROWS };
    let corner = if short { [Align::Start, Align::Start] } else { [Align::End, Align::End] };
    ui.open(Style { size: [Size::Grow(1.0), Size::Grow(1.0)], pad: [margin; 4], align: corner, ..Style::ROW });
    ui.open(Style { size: [Size::Fixed(panel_w), Size::Fit], pad: [14.0; 4], gap: 10.0, fill: Some(PANEL), ..Style::COLUMN });
    // Header: title, a spacer that takes the room, a badge.
    ui.open(Style { size: [Size::Grow(1.0), Size::Fit], align: [Align::Start, Align::Center], ..Style::ROW });
    ui.text(label("Mission 3", 22.0, INK));
    ui.open(Style { size: [Size::Grow(1.0), Size::Fit], ..Style::ROW });
    ui.close();
    ui.open(Style { pad: [10.0, 4.0, 10.0, 4.0], fill: Some(ACCENT), ..Style::ROW });
    ui.text(label("3 / 5", 16.0, INK));
    ui.close();
    ui.close();
    // Progress bar: three parts filled, two empty (weights share the width).
    ui.open(Style { size: [Size::Grow(1.0), Size::Fixed(8.0)], fill: Some(TRACK), ..Style::ROW });
    ui.open(Style { size: [Size::Grow(3.0), Size::Grow(1.0)], fill: Some(ACCENT), ..Style::ROW });
    ui.close();
    ui.open(Style { size: [Size::Grow(2.0), Size::Grow(1.0)], ..Style::ROW });
    ui.close();
    ui.close();
    // A paragraph: it wraps to the panel's inside.
    ui.text(label("Collect the glowing orbs before the timer runs out. Hold Space to spin faster.", 16.0, DIM));
    // The list: four rows show, the rest scroll (a whole row per step).
    let list_h = rows * ROW_H + (rows - 1.0) * ROW_GAP;
    let scroll = step as f32 * (ROW_H + ROW_GAP);
    ui.open(Style { size: [Size::Grow(1.0), Size::Fixed(list_h)], gap: ROW_GAP, scroll: [None, Some(scroll)], ..Style::COLUMN });
    for (i, (name, count)) in ITEMS.iter().enumerate() {
        let row = Style {
            size: [Size::Grow(1.0), Size::Fixed(ROW_H)],
            pad: [10.0, 0.0, 10.0, 0.0],
            gap: 10.0,
            align: [Align::Start, Align::Center],
            fill: Some(ROW),
            ..Style::ROW
        };
        ui.open(row);
        ui.open(Style { size: [Size::Fixed(14.0), Size::Fixed(14.0)], fill: SWATCH.get(i).copied(), ..Style::ROW });
        ui.close();
        ui.text(label(name, 16.0, INK));
        ui.open(Style { size: [Size::Grow(1.0), Size::Fit], ..Style::ROW });
        ui.close();
        ui.text(label(count, 16.0, DIM));
        ui.close();
    }
    ui.close();
    // Two buttons share the width; each centres its label.
    ui.open(Style { size: [Size::Grow(1.0), Size::Fit], gap: 10.0, ..Style::ROW });
    for name in ["Resume", "Quit"] {
        ui.open(Style {
            size: [Size::Grow(1.0), Size::Fixed(36.0)],
            align: [Align::Center, Align::Center],
            fill: Some(BUTTON),
            ..Style::ROW
        });
        ui.text(label(name, 18.0, INK));
        ui.close();
    }
    ui.close();
    ui.close();
    ui.close();
}

fn style(font: FontId, t: &Text<'_>, max_width: f32) -> TextStyle {
    let align = match t.align {
        Align::Start => TextAlign::Left,
        Align::Center => TextAlign::Center,
        Align::End => TextAlign::Right,
    };
    TextStyle { font, max_width, align, ..TextStyle::new(t.size, t.color) }
}

/// Fill `r` with one hard-edged rectangle: the MSDF sprite variant's
/// `Mode::Solid` (an atlas square would blur at panel size, since its edge
/// fades over a fixed number of texels). The colour may be translucent.
fn fill(rw: &mut RenderWorld, r: Rect, color: [f32; 4], layer: i16) {
    rw.draw_sprite(Sprite {
        pos: [r.x + r.w * 0.5, r.y + r.h * 0.5],
        size: [r.w, r.h],
        rotation: 0.0,
        cell: evani_render::msdf::cell(0, evani_render::msdf::Mode::Solid, 0),
        color,
        layer,
    });
}

/// Draw what the layout returned: each box before the boxes inside it.
fn paint(rw: &mut RenderWorld, ui: &Ui<'_>, font: FontId) {
    for cmd in ui.commands() {
        match cmd {
            Cmd::Rect { rect, color, depth } => fill(rw, rect, color, LAYER + 2 * i16::from(depth)),
            Cmd::Text { text: t, pos, max_width, depth } => {
                text::draw(rw, pos, LAYER + 2 * i16::from(depth) + 1, t.s, &style(font, &t, max_width));
            }
        }
    }
}

fn draw(world: &mut World) {
    // The list scrolls one row every 2.5 s, 0 to 2 and round again.
    let step = (world.res::<Time>().render_seconds() / 2.5) as u32 % 3;
    let mut rw = world.res_mut::<RenderWorld>();
    let font = rw.fonts.request("ui");
    let [w, h] = rw.viewport_css;
    // Wait for the font: measuring before it arrives would lay text out empty.
    if w <= 0.0 || h <= 0.0 || !rw.fonts.ready(font) {
        return;
    }
    let mut ui = Ui::new();
    build(&mut ui, w, h, step);
    let measure = |t: &Text<'_>, max_width: f32| text::measure_in(&rw, t.s, &style(font, t, max_width));
    ui.layout([0.0, 0.0], [w, h], measure);
    paint(&mut rw, &ui, font);
}
