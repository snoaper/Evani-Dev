//! P4 to P9 (and batch 2's B2-P1 foliage) checks on the built recipes (metal, paint, plastic, wood, bark, stone, masonry,
//! plaster, ground, leather, fur, feather, scales, hide, keratin, organic, ceramic, gem, ice, fire, glow): their schema
//! rows equal `catalog/materials/recipes.json`, their shaders keep the recipe
//! contract (plan 2.3 and `src/recipes/mod.rs`), and the compiled set follows
//! the `recipe-<name>` features. The naga validation of every feature mix is
//! in `crates/evani-render/tests/recipe_wgsl.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test code

use evani_material_recipes::recipes::{BUILT, schema};
use evani_material_recipes::shader::{self, ALL_RECIPES, COMPILED, EXTRA_NAMES, RC_SHARED, STAGE, stage_recipe};

/// The recipe's WGSL source file, with comments (for the contract greps).
fn source(name: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
    // A Windows checkout may have CRLF line ends; the strip drops them too.
    let mut s = std::fs::read_to_string(dir.join(format!("rc_{name}.wgsl"))).expect("recipe shader readable").replace("\r\n", "\n");
    // A recipe's parts and their stand-ins (wood's layouts) keep the same contract.
    for part in ["layouts", "planks", "concrete"] {
        if let Ok(t) = std::fs::read_to_string(dir.join(format!("rc_{name}_{part}.wgsl"))) {
            s.push('\n');
            s.push_str(&t.replace("\r\n", "\n"));
        }
    }
    s
}

#[test]
fn built_recipes_match_the_shader_tables() {
    assert_eq!(BUILT.len(), ALL_RECIPES.len());
    for (s, r) in BUILT.iter().zip(ALL_RECIPES) {
        assert_eq!(s.code, r.code, "BUILT and ALL_RECIPES are in the same order");
        let row = stage_recipe(s.code).unwrap_or_else(|| panic!("{} has a STAGE row", s.name));
        assert_eq!(row.name, s.name);
        assert_eq!(usize::from(row.extras), s.extras.len(), "{}", s.name);
        let at = STAGE.iter().position(|x| x.code == s.code).unwrap();
        assert_eq!(EXTRA_NAMES[at], s.extras, "{}: extras names", s.name);
        assert!(s.numbers.len() <= 10 && !s.variants.is_empty(), "{}", s.name);
        assert!(s.numbers.iter().all(|n| n.min < n.max && (n.min..=n.max).contains(&n.default)), "{}", s.name);
        assert_eq!(schema(s.code).map(|x| x.name), Some(s.name));
    }
    assert!(BUILT.windows(2).all(|w| w[0].code < w[1].code), "code order");
    assert_eq!(BUILT[0].number("grain_scale"), Some(3));
}

/// Each built recipe's WGSL defines its `rc_<name>` with the contract's
/// signature, keeps its pattern at Lite (no early Lite return: see
/// `lite_keeps_the_pattern`), keeps its helpers in
/// its own namespace, takes no screen derivative (every one is in
/// `mat_frame`, before the discard), reads no clock, and ships stripped.
#[test]
fn recipe_shaders_keep_the_contract() {
    for (s, r) in BUILT.iter().zip(ALL_RECIPES) {
        let src = source(s.name);
        let sig = format!("fn rc_{}(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {{", s.name);
        assert!(src.contains(&sig), "{}: {sig}", s.name);
        assert!(src.starts_with(&format!("// evani.shader.rc.{}\n", s.name)), "{}: marker line", s.name);
        for line in src.lines().filter(|l| l.starts_with("fn ")) {
            assert!(line.starts_with(&format!("fn rc_{}", s.name)), "{}: helper names start rc_{}: {line}", s.name, s.name);
        }
        for bad in ["dpdx", "dpdy", "fwidth", "textureSample", "model_mat"] {
            assert!(!src.contains(bad), "{}: uses {bad}", s.name);
        }
        let extras = src.matches(&format!("rc_extra(b, RC_X_{}, ", s.name.to_uppercase())).count();
        assert!(extras >= s.extras.len().min(2), "{}: reads its extras", s.name);
        assert_eq!(r.wgsl.matches("//").count(), 1, "{}: only the marker survives the strip", s.name);
        assert!(!r.wgsl.contains("\n ") && !r.wgsl.contains("\n\n"), "{}: stripped tight", s.name);
    }
    for bad in ["dpdx", "dpdy", "fwidth"] {
        assert!(!RC_SHARED.contains(bad), "the shared helpers take no derivative");
        assert!(!shader::ALL_LAYOUT.contains(bad), "the layouts take no derivative");
    }
    for r in ALL_RECIPES {
        let name = stage_recipe(r.code).unwrap().name;
        assert_eq!(shader::uses_layout(&r), ["metal", "paint", "wood", "stone", "masonry", "ceramic", "thin", "cloth", "glass"].contains(&name), "{name}: draws a layout");
    }
}

/// Critic round 1 (Chris's rule: the same look on every device): Lite keeps
/// each recipe's pattern. No recipe may return early at Lite with a flat
/// colour; Lite saves cost by skipping the normal edits, glints, back-light and
/// coat lobes (Medium and up), never by dropping the pattern. An
/// `if (q == 0u ...)` block may only change a value, never `return`.
#[test]
fn lite_keeps_the_pattern() {
    for s in BUILT {
        let src = source(s.name);
        let body = &src[src.find(&format!("fn rc_{}(b: RecBlock", s.name)).unwrap()..];
        let mut from = 0;
        while let Some(at) = body[from..].find("if (q == 0u") {
            let start = from + at;
            let open = start + body[start..].find('{').unwrap();
            let (mut depth, mut end) = (0i32, open);
            for (k, ch) in body[open..].char_indices() {
                depth += match ch {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                };
                if depth == 0 {
                    end = open + k;
                    break;
                }
            }
            assert!(!body[open..end].contains("return"), "{}: Lite returns early and drops the pattern", s.name);
            from = end;
        }
    }
}

/// P6 acceptance: fur is painted, so at every level (Lite included) it is one pass with
/// no extra draws: its WGSL declares no entry point, binding or texture read and no
/// discard, and adds none to the stage. Lite paints the strands and locks as Low does
/// (critic round 1); the fibre normals stay Medium's. The rim is written to `o.sheen`,
/// which `mat_extra` draws.
#[test]
fn fur_is_one_pass_with_no_extra_draws() {
    let fur = ALL_RECIPES.iter().find(|r| r.code == 12).expect("fur is built");
    for bad in ["@vertex", "@fragment", "@compute", "@group", "@binding", "var<", "textureSample", "textureLoad", "discard"] {
        assert!(!fur.wgsl.contains(bad), "fur uses {bad}");
    }
    let count = |s: &str| ["@group(", "@binding(", "@vertex", "@fragment", "var<uniform>"].map(|p| s.matches(p).count());
    let without = shader::stage_wgsl_with(&[], &[]);
    let with = shader::stage_wgsl_with(&[*fur], &[]);
    assert_eq!(count(&without), count(&with), "fur adds no pipeline or binding to the stage");
    let src = source("fur");
    let body = &src[src.find("fn rc_fur(").unwrap()..];
    assert!(body.contains("o.sheen"), "the rim is drawn");
    assert!(body.find("rc_bump(").unwrap() > body.find("if (q >= 2u) {").unwrap(), "fibre normals are Medium's");
    assert!(!body.contains("o.emit"), "the coat is painted, nothing is emitted");
}

/// B5-P1 acceptance: fur has the five new variants after dapple, in this order
/// (the catalogue stores the variant by index, so the old six never move), and
/// `mix` is the last, tenth number. Each new pattern is a function of the
/// pattern position `p` and the block alone (stable along the flow: no
/// screen, world or time input), is faded to its average by the shared
/// anti-aliasing (`rc_fur_avg`, `rc_fur_cell`), and `mix` 0 with pattern
/// contrast 0 paints no new colour: the roan share, the merle dilution and
/// the mottle balance are all multiplied by `mix` or by the contrast, and the
/// crimp gap adds `mix` to the clump.
#[test]
fn fur_new_variants_are_stable_and_fade_to_their_average() {
    let fur = BUILT.iter().find(|r| r.code == 12).expect("fur is built");
    let want = ["solid", "stripes", "spots", "patches", "rosettes", "dapple", "roan", "merle", "mottle", "crimp", "reticulated"];
    assert_eq!(fur.variants, want);
    assert_eq!(fur.numbers.len(), 10);
    assert_eq!(fur.numbers[9].name, "mix");
    assert_eq!(fur.number("mix"), Some(9));
    let src = source("fur");
    let pat = &src[src.find("fn rc_fur_pat(").unwrap()..src.find("fn rc_fur_avg(").unwrap()];
    for v in [6, 7, 8, 9, 10] {
        assert!(pat.contains(&format!("v == {v}u")), "variant {v} has a pattern branch");
    }
    for bad in ["f.", "i.uv", "i.n", "rc_time", "f.world", "rc_near"] {
        assert!(!pat.contains(bad), "the pattern reads only p and the block: {bad}");
    }
    let body = &src[src.find("fn rc_fur(").unwrap()..];
    assert!(body.contains("rc_fade(rc_fur_pat(b, p), rc_fur_avg(b), k)"), "the pattern fades to its average");
    assert!(body.contains("let k = rc_aa(f, rc_fur_cell(b) / sc);"), "and keeps detail only while a cell spans 1.5 pixels");
    // The roan's average is its hairs' cover (round 5: twice the share of
    // the cells hold a hair covering about 0.23 of it), so the far coat is
    // the near coat's mean.
    let avg = &src[src.find("fn rc_fur_avg(").unwrap()..src.find("fn rc_fur_cell(").unwrap()];
    assert!(avg.contains("if (b.variant == 6u) {
        return b.nc.y * 0.45;"), "roan fades to its hairs' cover");
    // mix 0 and contrast 0 add nothing: every new colour is scaled by one of them.
    assert!(body.contains("* b.nb.y;"), "the pattern colour is scaled by the contrast");
    assert!(body.contains("mb = b.nc.y * (1.0 - m);"), "the merle dilution is scaled by mix");
    assert!(body.contains("select(0.0, 0.8 * b.nc.y, b.variant == 9u)"), "the crimp gap adds mix to the clump");
    assert!(pat.contains("b.nc.y * (1.0 +"), "the roan share is scaled by mix");
    // Roan hairs are fine single hairs (studio pass, horse.coat.grey_steel):
    // a grid 3 across and 2 along per cell, so the smallest feature is 0.17.
    assert!(pat.contains("let g = vec2<f32>(p.y * 3.0, p.x * 2.0);"), "roan hairs are half a cell long and a third wide");
    let cell = &src[src.find("fn rc_fur_cell(").unwrap()..src.find("fn rc_fur_h(").unwrap()];
    assert!(cell.contains("select(0.4, 0.17, b.variant == 6u)"), "roan fades below its finer hair width");
    assert!(body.contains("+ (b.nc.y - 0.5) * 0.3"), "the mottle balance moves with mix");
    // The unchanged variants keep their branches and their fade value.
    assert!(pat.contains("if (v == 1u) {") && pat.contains("if (v == 3u) {") && pat.contains("if (v == 2u) {") && pat.contains("if (v == 4u) {"));
    assert!(avg.contains("select(0.25, 0.55,"), "the old variants still fade to 0.25");
}

/// B2-P1 acceptance: foliage is one pass (no binding, entry point, texture
/// read or discard of its own: the card's alpha mask is the model pass's);
/// Lite draws the blades, needles and veins as Low does (critic round 1),
/// the back-light is emitted from Medium only and fades under snow, and the
/// waxy glints are High's.
#[test]
fn foliage_lite_keeps_blades_and_the_back_light_is_medium_up() {
    let fol = ALL_RECIPES.iter().find(|r| r.code == 20).expect("foliage is built");
    for bad in ["@vertex", "@fragment", "@compute", "@group", "@binding", "var<", "textureSample", "textureLoad", "discard"] {
        assert!(!fol.wgsl.contains(bad), "foliage uses {bad}");
    }
    let src = source("foliage");
    let body = &src[src.find("fn rc_foliage(b:").unwrap()..];
    let medium = body.find("if (q >= 2u) {").unwrap();
    assert!(body.find("o.emit").unwrap() > medium, "the back-light is Medium's");
    assert!(body.contains("rc_amount(b, 3u)"), "snow on the leaf stops the back-light");
    assert!(body.find("o.glint").unwrap() > body.find("if (q >= 3u").unwrap(), "glints are High's");
}

/// The compiled set is exactly the recipes whose features are on, in code
/// order, and the stage carries the shared helpers only when one is.
#[test]
fn compiled_recipes_follow_the_features() {
    let want: Vec<u8> = [
        (cfg!(feature = "recipe-metal"), 1u8),
        (cfg!(feature = "recipe-paint"), 2),
        (cfg!(feature = "recipe-plastic"), 3),
        (cfg!(feature = "recipe-wood"), 4),
        (cfg!(feature = "recipe-bark"), 5),
        (cfg!(feature = "recipe-stone"), 6),
        (cfg!(feature = "recipe-masonry"), 7),
        (cfg!(feature = "recipe-plaster"), 8),
        (cfg!(feature = "recipe-ground"), 9),
        (cfg!(feature = "recipe-cloth"), 10),
        (cfg!(feature = "recipe-leather"), 11),
        (cfg!(feature = "recipe-fur"), 12),
        (cfg!(feature = "recipe-feather"), 13),
        (cfg!(feature = "recipe-scales"), 14),
        (cfg!(feature = "recipe-hide"), 15),
        (cfg!(feature = "recipe-keratin"), 19),
        (cfg!(feature = "recipe-foliage"), 20),
        (cfg!(feature = "recipe-thin"), 21),
        (cfg!(feature = "recipe-organic"), 22),
        (cfg!(feature = "recipe-ceramic"), 23),
        (cfg!(feature = "recipe-glass"), 24),
        (cfg!(feature = "recipe-gem"), 25),
        (cfg!(feature = "recipe-ice"), 27),
        (cfg!(feature = "recipe-fire"), 28),
        (cfg!(feature = "recipe-glow"), 29),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|&(_, c)| c)
    .collect();
    assert_eq!(COMPILED.iter().map(|r| r.code).collect::<Vec<_>>(), want);
    let stage = shader::stage_wgsl();
    assert_eq!(stage.contains("fn rc_out("), !want.is_empty(), "shared helpers only with a recipe");
    // B3-P1: the pattern layouts only with a recipe that draws one.
    let layout = want.iter().any(|c| [1, 2, 4, 6, 7, 10, 21, 23, 24].contains(c));
    assert_eq!(stage.contains("fn rc_lay_bond("), layout, "the layouts follow their recipes");
    assert_eq!(cfg!(feature = "layout"), layout, "the recipes that draw a layout turn the feature on");
    for s in BUILT {
        let on = want.contains(&s.code);
        assert_eq!(stage.contains(&format!("fn rc_{}(", s.name)), on, "{}", s.name);
        assert_eq!(stage.contains(&format!("case {}u:", s.code)), on, "{}", s.name);
    }
    // Wood's layouts are their own part: the compiled wood is the full text
    // with `recipe-wood-layouts`, the planks stand-in without it.
    if let Some(w) = COMPILED.iter().find(|r| r.code == 4) {
        let full = ALL_RECIPES.iter().find(|r| r.code == 4).unwrap();
        let want = if cfg!(feature = "recipe-wood-layouts") { full.wgsl } else { shader::WOOD_PLANKS.wgsl };
        assert_eq!(w.wgsl, want, "wood follows its layouts part");
    }
    assert_ne!(shader::WOOD_PLANKS.wgsl, ALL_RECIPES.iter().find(|r| r.code == 4).unwrap().wgsl);
    // Organic's food part the same way (material-view round 6), and its
    // stand-in defines the same two functions.
    if let Some(o) = COMPILED.iter().find(|r| r.code == 22) {
        let full = ALL_RECIPES.iter().find(|r| r.code == 22).unwrap();
        let want = if cfg!(feature = "recipe-organic-food") { full.wgsl } else { shader::ORGANIC_PLAIN.wgsl };
        assert_eq!(o.wgsl, want, "organic follows its food part");
    }
    for f in ["fn rc_organic_f(", "fn rc_organic_fh("] {
        assert_eq!(shader::ORGANIC_PLAIN.wgsl.matches(f).count(), 1, "the organic stand-in has {f}");
        assert_eq!(ALL_RECIPES.iter().find(|r| r.code == 22).unwrap().wgsl.matches(f).count(), 1, "the food part has {f}");
    }
    // P8-4 guard: every function the layouts part defines, the stand-in
    // defines too (marquetry's rc_wood_mq was nearly added to the layouts part
    // alone, which breaks every game whose wood is without layouts), each once.
    let full = ALL_RECIPES.iter().find(|r| r.code == 4).unwrap().wgsl;
    let layouts = &full[full.find("fn rc_wood_u(").unwrap()..];
    let mut n = 0;
    for name in layouts.split("fn rc_wood_").skip(1).filter_map(|t| t.split('(').next()) {
        assert_eq!(shader::WOOD_PLANKS.wgsl.matches(&format!("fn rc_wood_{name}(")).count(), 1, "the stand-in has rc_wood_{name}
fix: add fn rc_wood_{name} to shaders/rc_wood_planks.wgsl (drawing as planks or flat-sawn)");
        n += 1;
    }
    assert!(n >= 4, "layout functions found: {n}");
}

/// The schema rows are the catalogue's recipe table.
#[cfg(feature = "catalog")]
#[test]
fn schema_rows_equal_recipes_json() {
    use evani_material_recipes::catalog::Catalog;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
    let cat = Catalog::load_dir(&dir).expect("catalogue loads");
    for s in BUILT {
        let r = cat.recipes.iter().find(|r| r.code == s.code).unwrap_or_else(|| panic!("{}", s.name));
        assert_eq!(r.id, s.name);
        // Paint's clear coat is the layers family's; its extras are drawn here.
        assert!(r.backend == "recipes" || (r.backend == "layers" && r.also.iter().any(|a| a == "recipes")), "{}", s.name);
        assert_eq!(r.colours.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), s.colours, "{}: colours", s.name);
        assert_eq!(r.variants, s.variants, "{}: variants", s.name);
        assert_eq!(r.light, s.light, "{}: light version", s.name);
        assert_eq!(r.numbers.len(), s.numbers.len(), "{}: numbers", s.name);
        for (a, b) in r.numbers.iter().zip(s.numbers) {
            assert_eq!((a.name.as_str(), a.min, a.max, a.default), (b.name, b.min, b.max, b.default), "{}", s.name);
        }
        let mut extras: Vec<_> = r.extras.iter().collect();
        extras.sort_by_key(|e| e.mask);
        assert_eq!(extras.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), s.extras, "{}: extras", s.name);
        assert!(extras.iter().enumerate().all(|(k, e)| e.mask == 1 << k), "{}: extras bits 1, 2, 4..", s.name);
    }
}

/// Material-view round 4 (merged with waves 1 and 2 on matcat-r4) grew two
/// of the recipes these budgets cover, from the same base (f08e694e, whose
/// thin and organic are 57be02c9's): thin by damask and the petal's depth and
/// sheen, organic by the clear slime. Its growth, by this file's own count
/// (the build's strip, flate2 -9) of 58d77101 against f08e694e, is added to
/// the package budgets as its own measured amount, so each package's plan
/// allowance stays exactly the plan's: thin 1,421 to 1,734 B (+313), organic
/// 697 to 964 B (+267). Merged, thin is 2,188 B and organic 1,121 B, within a
/// few bytes of base + both growths (gzip shares a little across them).
fn round4_growth(name: &str) -> usize {
    match name {
        "thin" => 313,
        "organic" => 267,
        // The oak block's live session (procedural realism, 2026-11-04):
        // solid wood (variants 15, 16) in its own function, rings round a
        // pith with pores, rays, knots and streaks; DECISIONS "Solid oak
        // bytes".
        "wood" => 1020,
        _ => 0,
    }
}

/// P7-2 guard (the thin recipe's first draft came out over its budget): the
/// shipped text of each recipe P7-2 extended, gzip -9, stays inside the
/// plan's byte budget (`docs/materials/BATCHES_7_9_PLAN.md` section 6: the
/// size before plus the package's allowance: feather +400, scales +300,
/// keratin +250, thin +450). A later change that grows one past it fails here
/// with the size. The "before" sizes are by this test's own count (flate2),
/// which is a few bytes off the plan's node count (keratin 1,205 here, 1,198
/// there), so the allowances are the plan's own and there is no extra slack.
/// Package P7-3 adds code to feather and scales by design: it raises their
/// numbers in the same commit, with the plan's new budget beside the change.
#[test]
fn p7_2_recipes_stay_inside_their_byte_budgets() {
    use std::io::Write;
    let gz = |s: &str| {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(s.as_bytes()).unwrap();
        e.finish().unwrap().len()
    };
    // (recipe, size at 57be02c9 by this count, the plan's allowance)
    for (name, before, allow) in [("feather", 1265, 400), ("scales", 1363, 300), ("keratin", 1205, 250), ("thin", 1444, 450)] {
        let code = BUILT.iter().find(|s| s.name == name).unwrap().code;
        let r = ALL_RECIPES.iter().find(|r| r.code == code).unwrap();
        let n = gz(r.wgsl);
        let r4 = round4_growth(name);
        println!("{name}: {n} B gzip, budget {}", before + allow + r4);
        assert!(n <= before + allow + r4, "{name}: {n} B gzip, budget {} B (before {before} + plan allowance {allow} + round 4's measured {r4})
fix: trim rc_{name}.wgsl (shared expressions, fewer branches) or agree a new budget in the plan", before + allow + r4);
    }
}

/// A wing line mask read out of the shader: `[1.0 - ]smoothstep(a, b,
/// [0.5 - ]abs(fract(x) - 0.5))`.
struct LineMask {
    inverted: bool,
    edges: (f32, f32),
    from_half: bool,
}

/// Split `s` at top-level commas (not inside brackets).
fn top_level_args(s: &str) -> Vec<&str> {
    let (mut depth, mut start, mut out) = (0i32, 0, Vec::new());
    for (k, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..k].trim());
                start = k + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out
}

/// Every line mask in a wing branch, parsed from its text. An Err is a form
/// the guard cannot read, which fails too: write the standard form.
fn wing_line_masks(wing: &str) -> Result<Vec<LineMask>, String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = wing[from..].find("smoothstep(") {
        let at = from + k;
        let open = at + "smoothstep".len();
        from = open;
        let mut depth = 0;
        let mut close = None;
        for (j, ch) in wing[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.ok_or("unbalanced smoothstep(")?;
        let inner = &wing[open + 1..close];
        if !inner.contains("abs(fract(") {
            continue;
        }
        let args = top_level_args(inner);
        if args.len() != 3 {
            return Err(format!("smoothstep({inner}) is not smoothstep(a, b, distance)"));
        }
        let edge = |t: &str| t.parse::<f32>().map_err(|_| format!("edge \"{t}\" is not a plain number"));
        let (from_half, rest) = args[2].strip_prefix("0.5 - ").map_or((false, args[2]), |r| (true, r));
        if !(rest.starts_with("abs(fract(") && rest.ends_with(") - 0.5)")) {
            return Err(format!("distance \"{}\" is not [0.5 - ]abs(fract(x) - 0.5)", args[2]));
        }
        out.push(LineMask { inverted: wing[..at].ends_with("1.0 - "), edges: (edge(args[0])?, edge(args[1])?), from_half });
    }
    Ok(out)
}

/// A mask's value where fract(x) = `t`, as the GPU computes it (smoothstep).
fn line_mask_at(m: &LineMask, t: f32) -> f32 {
    let d = (t - 0.5).abs();
    let d = if m.from_half { 0.5 - d } else { d };
    let (a, b) = m.edges;
    let x = ((d - a) / (b - a)).clamp(0.0, 1.0);
    let s = x * x * (3.0 - 2.0 * x);
    if m.inverted { 1.0 - s } else { s }
}

/// The behaviour a wing line must have: about 1 on the vein (fract(x) = 0),
/// about 0 between veins (fract(x) = 0.5), with a rising edge (a < b).
fn check_wing_lines(wing: &str) -> Result<usize, String> {
    let masks = wing_line_masks(wing)?;
    if masks.len() < 2 {
        return Err(format!("the wing has its veins and cross veins: found {} line masks", masks.len()));
    }
    for (k, m) in masks.iter().enumerate() {
        if m.edges.0 >= m.edges.1 {
            return Err(format!("line {k}: smoothstep edges {:?} must rise (a < b)", m.edges));
        }
        let (on, between) = (line_mask_at(m, 0.0), line_mask_at(m, 0.5));
        if on < 0.99 || between > 0.01 {
            return Err(format!("line {k} is inverted or wrong: mask {on:.2} on the vein (want 1), {between:.2} between veins (want 0)"));
        }
    }
    Ok(masks.len())
}

fn wing_branch(src: &str) -> &str {
    let start = src.find("if (v == 8u) {").expect("the wing branch");
    let end = start + src[start..].find("if (v == 9u) {").expect("the segments branch after it");
    &src[start..end]
}

/// P7-2 guard (the wing's first veins were inverted: `1 - smoothstep(0.47,
/// 0.5, abs(fract(a) - 0.5))` is 1 everywhere except on the vein, so the
/// whole wing drew in the vein colour). It reads each line mask out of the
/// wing branch and evaluates it: about 1 on the vein, about 0 between veins.
/// That catches the original form and the other inverted form (dropping the
/// `1.0 -`), whatever the edge numbers are.
#[test]
fn wing_veins_are_thin_lines() {
    let src = source("thin");
    let n = check_wing_lines(wing_branch(&src))
        .unwrap_or_else(|e| panic!("{e}\nfix: write each vein as 1.0 - smoothstep(w0, w1, 0.5 - abs(fract(x) - 0.5)) with w0 < w1: the distance to the vein"));
    assert!(n >= 2);
}

/// The guard itself: it passes the shipped wing and fails on deliberately
/// inverted copies (the original bug, the same line without its `1.0 -`,
/// reversed edges, the wrong distance, a form it cannot read).
#[test]
fn the_wing_vein_guard_fails_on_inverted_copies() {
    let src = source("thin");
    let wing = wing_branch(&src).to_string();
    assert!(check_wing_lines(&wing).is_ok());
    let line = "1.0 - smoothstep(0.02, 0.05, 0.5 - abs(fract(a) - 0.5))";
    assert!(wing.contains(line), "the test's copy of the first vein line is out of date");
    let cases = [
        ("original bug", "1.0 - smoothstep(0.47, 0.5, abs(fract(a) - 0.5))"),
        ("dropped 1.0 -", "smoothstep(0.02, 0.05, 0.5 - abs(fract(a) - 0.5))"),
        ("reversed edges", "1.0 - smoothstep(0.05, 0.02, 0.5 - abs(fract(a) - 0.5))"),
        ("wrong distance", "1.0 - smoothstep(0.02, 0.05, abs(fract(a) - 0.5))"),
        ("unreadable", "1.0 - smoothstep(0.02, 0.05, 0.5 - length(fract(a) - 0.5))"),
    ];
    for (what, bad) in cases {
        let e = check_wing_lines(&wing.replace(line, bad)).expect_err(what);
        println!("{what}: {e}");
    }
    let line2 = "1.0 - smoothstep(0.03, 0.06, 0.5 - abs(fract(x) - 0.5))";
    assert!(wing.contains(line2), "the test's copy of the cross-vein line is out of date");
    assert!(check_wing_lines(&wing.replace(line2, "smoothstep(0.03, 0.06, 0.5 - abs(fract(x) - 0.5))")).is_err());
}

/// P8-4 guard (wood's first draft came out over its budget, +420 B against
/// +350, until charred became a layout that reuses the planks' joints, tones
/// and bevels): the shipped text of each recipe P8-4 extended, gzip -9, stays
/// inside the plan's byte budget (`docs/materials/BATCHES_7_9_PLAN.md`
/// section 6: wood +350 (with its layouts part, as a game with layouts
/// downloads it), ceramic +150, organic +200, glass +120, leather +250, metal
/// +550). The "before" sizes are this test's own count (flate2, same
/// level, of the same stripped text) at 57be02c9, the base of P8-4, so the
/// allowances are the plan's own with no slack.
#[test]
fn p8_4_recipes_stay_inside_their_byte_budgets() {
    use std::io::Write;
    let gz = |s: &str| {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(s.as_bytes()).unwrap();
        e.finish().unwrap().len()
    };
    // (recipe, size at 57be02c9 by this count, the plan's allowance)
    for (name, before, allow) in [("wood", 2288, 350), ("ceramic", 1650, 150), ("organic", 697, 200), ("glass", 1129, 120), ("leather", 1269, 250), ("metal", 1426, 550)] {
        let code = BUILT.iter().find(|s| s.name == name).unwrap().code;
        // Organic's own size is without its food part (material-view round
        // 6), which has its budget below.
        let r = if name == "organic" { &shader::ORGANIC_PLAIN } else { ALL_RECIPES.iter().find(|r| r.code == code).unwrap() };
        let n = gz(r.wgsl);
        let r4 = round4_growth(name);
        println!("{name}: {n} B gzip, budget {}", before + allow + r4);
        assert!(n <= before + allow + r4, "{name}: {n} B gzip, budget {} B (before {before} + plan allowance {allow} + round 4's measured {r4})
fix: trim rc_{name}.wgsl (shared expressions, fewer branches) or agree a new budget in the plan", before + allow + r4);
    }
    // Without the layouts part wood is the planks stand-in: it pays for
    // marquetry's stand-in only.
    println!("wood without layouts: {} B gzip", gz(shader::WOOD_PLANKS.wgsl));
}

/// Material-view round 6 guard: organic's food part (flesh fibres and flakes,
/// corn's kernels, the pie's lattice) moved out of the recipe so the recipe
/// itself shrank (1,158 to about 1,080 B by this count, inside the P8-4
/// budget above); the part adds at most 420 B to a game that lists it, and
/// the stand-in, what every other game downloads, stays a few bytes.
/// Material-view round 10 (mvart, 2026-11-02): raw meat's fat as seams
/// between muscle bundles (`rc_organic_meat`) adds about 430 B to the part,
/// inside the plan's 800 B for that fix (implementation plan M2): 880 B.
#[test]
fn organic_food_part_stays_inside_its_byte_budget() {
    use std::io::Write;
    let gz = |s: &str| {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(s.as_bytes()).unwrap();
        e.finish().unwrap().len()
    };
    let full = gz(ALL_RECIPES.iter().find(|r| r.code == 22).unwrap().wgsl);
    let plain = gz(shader::ORGANIC_PLAIN.wgsl);
    println!("organic: {plain} B gzip without the food part, {full} B with it (+{})", full - plain);
    assert!(full <= plain + 880, "organic's food part adds {} B gzip, budget 880 B
fix: trim shaders/rc_organic_food.wgsl or agree a new budget in DECISIONS", full - plain);
    assert!(plain <= 1164, "organic without its food part: {plain} B gzip, budget 1,164 B");
}

/// P8-4 guard: a recipe's shader reads only the number slots its schema has.
/// Numbers go to the block in schema order (n0-n3 `na`, n4-n7 `nb`, n8-n9
/// `nc`), so a shader that reads a slot past its last number reads a zero.
/// Found in P8-4: leather has 8 numbers but read its craze scale from `nc.x`
/// (n8, always 0) instead of `nb.w` (n7), so every crazed and tufted row
/// drew at scale 1 whatever its number said.
/// The slot errors of one recipe: `wgsl` reads a number slot past its
/// `count` numbers, alone (`b.nc.x`) or with its whole vector (`b.nc.xy`,
/// `b.nc`).
fn slot_errors(name: &str, count: usize, wgsl: &str) -> Vec<String> {
    const SLOTS: [&str; 10] = ["na.x", "na.y", "na.z", "na.w", "nb.x", "nb.y", "nb.z", "nb.w", "nc.x", "nc.y"];
    let mut errors = Vec::new();
    for (k, slot) in SLOTS.iter().enumerate().skip(count) {
        let vector = format!("b.{}", &slot[..2]);
        let whole = k % 4 == 0 && wgsl.match_indices(vector.as_str()).any(|(i, m)| !wgsl[i + m.len()..].starts_with('.') || wgsl[i + m.len()..].starts_with(".xy"));
        if wgsl.contains(&format!("b.{slot}")) || whole {
            errors.push(format!("ERROR {name}: the shader reads b.{slot} (number n{k}) but the recipe has only {count} numbers
  fix: read the number from its own slot (n0-n3 na, n4-n7 nb, n8-n9 nc) or add the number to the schema and recipes.json"));
        }
    }
    errors
}

#[test]
fn recipes_read_only_the_number_slots_they_have() {
    let errors: Vec<String> = BUILT.iter().zip(ALL_RECIPES).flat_map(|(s, r)| slot_errors(s.name, s.numbers.len(), r.wgsl)).collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// The slot guard fails on the leather shader as it was before P8-4 (craze
/// scale read from `nc.x`), and on whole-vector reads past the last number.
#[test]
fn the_slot_guard_fails_on_the_old_leather_read() {
    let leather = ALL_RECIPES.iter().find(|r| r.code == 11).unwrap().wgsl;
    assert!(slot_errors("leather", 8, leather).is_empty());
    assert_eq!(slot_errors("leather", 8, &leather.replace("b.nb.w", "b.nc.x")).len(), 1);
    assert_eq!(slot_errors("x", 4, "let a = b.nb.x;").len(), 1);
    assert_eq!(slot_errors("x", 8, "let a = b.nc;").len(), 1);
    assert_eq!(slot_errors("x", 8, "let a = b.nc.xy;").len(), 1);
    assert!(slot_errors("x", 10, "let a = b.nc.y + b.nc.x;").is_empty());
}

/// Material-view round 5: turf reads `underside` as mown (blades laid one
/// way, 0.5 m stripes). A stored 0 must leave every other turf row exactly as
/// it was, so each new term is written to reduce to the old value at 0: the
/// blade angle is `mix(old, laid, underside)`, the stripe shade is `1 +
/// underside * ...`, the far colour `mix(old, mown, underside)`; and only the
/// lawn sets the number. (A pixel diff could not show this: two shots of the
/// same build differ on the turf tile by about 30% of its pixels.)
#[cfg(feature = "catalog")]
#[test]
fn turf_mowing_is_off_at_zero_and_only_the_lawn_sets_it() {
    use evani_material_recipes::catalog::Catalog;
    let src = source("foliage");
    for (what, line) in [
        ("blade angle: the old random turn at 0", "let a = mix(rc_u01(h) * 6.2832, ln.y + (rc_u01(h) - 0.5) * 1.1, ln.x);"),
        ("laying: underside on turf only (litter 0)", "let ln = vec2<f32>(select(b.nb.w, 0.0, lit), 1.5708 + 3.1416 * ms);"),
        ("stripe shade: 1 at 0, turf only", "return 1.0 + select(0.0, b.nb.w * 0.16 * (2.0 * ms - 1.0), b.variant == 4u);"),
        ("far colour: the old clumps at 0", "mix(0.7 + 0.6 * n0, 0.9 + 0.2 * n0, b.nb.w)"),
    ] {
        assert!(src.contains(line), "{what}");
    }
    // The arithmetic: mix at 0 and a factor of exactly 1 change no bit, in
    // either form a compiler may pick.
    for (x, y) in [(0.173f32, 0.91f32), (5.2, -3.7), (1e-6, 2.5), (0.0, 1.0)] {
        assert_eq!((x + (y - x) * 0.0).to_bits(), x.to_bits());
        assert_eq!((x * (1.0 - 0.0) + y * 0.0).to_bits(), x.to_bits());
        assert_eq!((x * (1.0 + 0.0 * 0.16)).to_bits(), x.to_bits());
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
    let cat = Catalog::load_dir(&dir).expect("catalogue loads");
    let mown: Vec<_> = cat
        .rows
        .iter()
        .filter(|r| r.recipe == "foliage")
        .filter(|r| matches!(r.params.get("variant").and_then(|v| v.as_str()), Some("turf" | "litter")))
        .filter(|r| r.params.get("numbers").and_then(|n| n.get("underside")).and_then(|u| u.as_f64()).unwrap_or(0.0) > 0.0)
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(mown, ["ground.grass.lawn"], "only the lawn is mown");
}
