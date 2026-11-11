//! Per-material shader composition (feature `split`; evani-render's
//! `material-recipes` turns it on, so every build with catalogue recipes
//! uses it).
//!
//! An all-in-one model module would carry every compiled recipe behind one
//! `switch b.recipe`, so the browser would compile all of them into every
//! model pipeline (measured on an RTX 4060 Ti: about 30 s cold for the
//! viewer's 25 recipes, 237 KB of WGSL). Instead a pipeline is made for one
//! [`SplitKey`]: one recipe, one character kind, one pass ([`OPAQUE`],
//! [`BLEND`] or [`GLASS`]). The condition layers stay the build's whole set
//! behind the runtime mask (`b.cond_mask`), so turning a layer on never
//! needs a new shader and layers never multiply the permutations. Each
//! module must stay under [`MODULE_BUDGET`] and a game under
//! [`GAME_PERMUTATIONS`] (`cargo xtask check`).
//!
//! [`shake`] then keeps only what the pipeline's entry points reach, so the
//! text the browser parses and validates is that material's code only. It is
//! the engine's own WGSL assembly (no new dependency); the approach follows
//! the shader-def + permutation-key idea of naga_oil and Bevy's shader cache
//! (both MIT/Apache-2.0), with no code taken from either.

use crate::shader::{self, COMPILED, LAYERS, RC_LAYOUT};

/// One material's shader permutation: its recipe code (block byte 0; 0 for
/// no recipe), its character kind (`surface.kind.z`: 0 plain, 1 skin,
/// 2 eye, 3 hair) and its pass ([`OPAQUE`], [`BLEND`], [`GLASS`]). The
/// sample count is the pipeline's own (1x and 4x; glass is always 1x).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SplitKey {
    /// The recipe code.
    pub recipe: u8,
    /// The character kind.
    pub kind: u8,
    /// The pass.
    pub see: u8,
}

/// Which model pipeline a material's module is for: the opaque one, the
/// see-through (blend) one, or the refracting glass one.
pub const OPAQUE: u8 = 0;
/// The blend pipeline (`REC_BLEND = 1`, the refraction stub).
pub const BLEND: u8 = 1;
/// The glass pipeline (one sample, the copy's `mat_refract`).
pub const GLASS: u8 = 2;

/// Whether this build draws `see` at all (blend and glass need the
/// see-through pass and a recipe compiled; glass also the refraction).
pub fn see_built(see: u8) -> bool {
    match see {
        OPAQUE => true,
        BLEND => shader::BLEND_BUILT,
        GLASS => shader::BLEND_BUILT && cfg!(feature = "refract"),
        _ => false,
    }
}

/// The recipe code a material's shader is keyed by: its own when this build
/// compiles it as a stage recipe, else 0. A code with no case in the
/// dispatch (a character recipe, one not listed) falls through to the lit
/// stage's own values in either text, so it shares the no-recipe shader.
pub fn key_code(code: u8) -> u8 {
    if COMPILED.iter().any(|r| r.code == code) && shader::stage_recipe(code).is_some() { code } else { 0 }
}

/// The recipe stage for one recipe (`None` if it is not compiled in, or not
/// a stage recipe), with this build's layers and cut-out, for pipeline `see`
/// ([`OPAQUE`], [`BLEND`] or [`GLASS`]). The opaque text has no see-through
/// code: in the all-in-one module it ran with `REC_BLEND = 0`, where every
/// line of it returns its input, so leaving it out draws the same.
pub fn stage_one(code: u8, see: u8) -> Option<String> {
    if !see_built(see) {
        return None;
    }
    if code == 0 {
        return Some(shader::stage_text(&[], LAYERS, RC_LAYOUT, false, see));
    }
    let r = COMPILED.iter().find(|r| r.code == code)?;
    shader::stage_recipe(r.code)?;
    Some(shader::stage_text(core::slice::from_ref(r), LAYERS, RC_LAYOUT, shader::CUT_BUILT, see))
}

/// [`crate::shader::patch_lit`] (`entry` `"fn model_lit_fs("`) or
/// [`crate::shader::patch_layered`] (`"fn model_layered_fs("`) for one
/// recipe and pipeline `see`. Recipe 0 never cuts holes (the cut-out's kinds
/// all name a recipe), so its text has no cut-out.
pub fn patch_one(template: &str, entry: &str, msaa: u32, code: u8, see: u8) -> Option<String> {
    let cut = code != 0 && shader::CUT_BUILT;
    let msaa = if see == GLASS { 1 } else { msaa };
    shader::patch_entry(template, entry, msaa, &stage_one(code, see)?, cut, see != OPAQUE)
}

/// [`patch_one`] on `model_lit_fs`, opaque.
pub fn patch_lit_one(template: &str, msaa: u32, code: u8) -> Option<String> {
    patch_one(template, "fn model_lit_fs(", msaa, code, OPAQUE)
}

/// [`patch_one`] on `model_layered_fs`, opaque.
pub fn patch_layered_one(template: &str, msaa: u32, code: u8) -> Option<String> {
    patch_one(template, "fn model_layered_fs(", msaa, code, OPAQUE)
}

/// The largest module a material may compile to (60 KB): about what the
/// browser parses, validates and compiles in well under a second. `cargo
/// xtask check` errors past it (docs/materials/SHADER_BUDGET.md).
pub const MODULE_BUDGET: usize = 60 * 1024;

/// The most distinct material shaders (recipe, character kind and
/// see-through pass, each once per sample count) a game may list (`cargo
/// xtask check`); a tool app (`kind = "tool"`, the material viewer) may list
/// up to [`TOOL_PERMUTATIONS`].
pub const GAME_PERMUTATIONS: usize = 12;
/// The cap for tool apps (the viewer draws the whole catalogue).
pub const TOOL_PERMUTATIONS: usize = 64;

/// What a plain material's module holds besides its recipe stage (the
/// forward module's model entry points, the lit or layered stage, the
/// shadows, lights and sky, the web build's light and sky patches), at most.
/// Measured in the material viewer, which builds every family (2026-10-29,
/// comments stripped): 23.3 KB, plus up to 3.3 KB of web patches; 28 KB
/// keeps about 1 KB to grow. evani-render's recipe shader test keeps every
/// real module's share under it.
pub const TEMPLATE_ALLOWANCE: usize = 28 * 1024;

/// The same for a character material (skin, eye or hair): its kind's
/// shading is in the module and the recipe stage is the no-recipe one.
/// Measured 30.1 KB plus the web patches; 36 KB.
pub const CHAR_ALLOWANCE: usize = 36 * 1024;

/// A plain material's recipe stage share of the budget (`cargo xtask check`
/// errors past it): 32 KB; the largest stage the viewer builds today is
/// 29.6 KB.
pub const STAGE_BUDGET: usize = MODULE_BUDGET - TEMPLATE_ALLOWANCE;

/// A character material's stage share (the no-recipe stage, 14.5 KB today).
pub const CHAR_STAGE_BUDGET: usize = MODULE_BUDGET - CHAR_ALLOWANCE;

/// The functions the patched lit stage calls: the stage's entry points.
pub fn stage_entries(cut: bool, see: u8) -> Vec<&'static str> {
    let mut e = vec!["mat_frame", "mat_surface", "mat_extra"];
    if cut {
        e.push("mat_cut");
    }
    if see != OPAQUE {
        e.extend(["mat_premul", "mat_blend"]);
    }
    e
}

/// One material's recipe stage as `cargo xtask check` sizes it, for any
/// game whatever this crate's features: recipe `r` (`None`: no recipe) with
/// `layers`, the cut-out when `cut` (recipes only), the style step when
/// `style`, for pass `see`, cut down to what the patched stage calls (its
/// share of the material's module).
pub fn stage_for(r: Option<&shader::RecipeSrc>, layers: &[shader::LayerSrc], cut: bool, style: bool, see: u8) -> String {
    let rs: &[shader::RecipeSrc] = r.map_or(&[], core::slice::from_ref);
    let cut = cut && r.is_some();
    let s = shader::stage_text_style(rs, layers, shader::ALL_LAYOUT, cut, see, style);
    shake(&s, &stage_entries(cut, see))
}

/// The character family's `char_shade` cut down to `kind`'s own branch
/// (plain: `char_none` only), so a plain material carries no skin, eye or
/// hair shading and the skin carries no eye or hair. The source is returned
/// unchanged when `char_shade` or the branch is not found in the expected
/// shape, so the material then draws as on the all-in-one path.
pub fn char_only(src: &str, kind: u8) -> String {
    let Some(at) = src.find("fn char_shade(") else {
        return src.to_string();
    };
    let Some(open) = src[at..].find('{').map(|i| at + i + 1) else {
        return src.to_string();
    };
    let Some(close) = src[open..].find("\n}").map(|i| open + i) else {
        return src.to_string();
    };
    let body = &src[open..close];
    let pick = |k: &str| -> Option<String> {
        // `if (abs(kind - K.0) < 0.5) { return char_x(...); }`
        let i = body.find(&format!("abs(kind - {k}.0) < 0.5"))?;
        let r = i + body[i..].find("return ")?;
        let e = r + body[r..].find(';')? + 1;
        Some(body[r..e].to_string())
    };
    let none = "return char_none(base, n);";
    let branch = match kind {
        0 => Some(none.to_string()),
        1 => pick("1"),
        2 => pick("2"),
        3 => pick("3"),
        _ => None,
    };
    let Some(branch) = branch else {
        return src.to_string();
    };
    let new_body = if kind == 0 {
        format!("\n{none}")
    } else {
        format!("\nlet p0 = char_p(0u);\nif (p0.x < 0.5) {{\n{none}\n}}\n{branch}")
    };
    let out = format!("{}{new_body}{}", &src[..open], &src[close..]);
    if kind == 0 { plain_char_io(out) } else { out }
}

/// A plain material's character input and output, cut to what it uses:
/// `char_none` reads no surface frame and always returns alpha 1, so
/// `char_frame` (six derivatives, about 0.5 KB) builds a frame nothing reads
/// and `char_out` never reaches its alpha-to-coverage mask (about 0.5 KB with
/// its noise). Both get a one-line body that returns exactly what they
/// returned for a plain material (an unread frame; the colour with every
/// sample on), and the shake then drops the noise. Measured on the viewer's
/// plain modules: about 0.9 KB less in each. A body that is not in the
/// expected shape is kept whole.
fn plain_char_io(src: String) -> String {
    let src = replace_body(&src, "fn char_frame(", "-> CharFrame {", "\nvar cf: CharFrame;\nreturn cf;\n").unwrap_or(src);
    replace_body(&src, "fn char_out(c: vec4<f32>, a: f32,", "-> CharOut {", "\nvar o: CharOut;\no.color = c;\no.mask = 0xffffffffu;\nreturn o;\n").unwrap_or(src)
}

/// `src` with the body of the function whose head starts `head` and ends
/// `ret` (its return type and `{`) replaced by `body`, matching braces.
/// `None` when the head or its closing brace is not found.
fn replace_body(src: &str, head: &str, ret: &str, body: &str) -> Option<String> {
    let at = src.find(head)?;
    let line_end = at + src[at..].find('\n')?;
    if !src[at..line_end].trim_end().ends_with(ret) {
        return None;
    }
    let open = at + src[at..].find('{')? + 1;
    let mut depth = 1usize;
    let mut close = None;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    Some(format!("{}{body}{}", &src[..open], &src[close..]))
}

/// A top-level WGSL item: its text (with the comments before it) and its
/// name (empty for directives and anything unnamed).
struct Item<'a> {
    text: &'a str,
    name: &'a str,
}

/// `src` split into its top-level items by brace depth (`//` comments run to
/// the end of the line; WGSL has no strings).
fn items(src: &str) -> Vec<Item<'_>> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let (mut depth, mut braced) = (0i32, false);
        while i < b.len() {
            if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            match b[i] {
                b'{' => {
                    depth += 1;
                    braced = true;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 && braced {
                        i += 1;
                        if b.get(i) == Some(&b';') {
                            i += 1;
                        }
                        break;
                    }
                }
                b';' if depth == 0 => {
                    i += 1;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        let text = &src[start..i];
        let code = strip_comments_head(text);
        if code.is_empty() {
            continue;
        }
        out.push(Item { text, name: head(code) });
    }
    out
}

/// `t` from its first character that is neither blank nor in a comment.
fn strip_comments_head(t: &str) -> &str {
    let mut s = t;
    loop {
        s = s.trim_start();
        if s.starts_with("//") {
            match s.find('\n') {
                Some(n) => s = &s[n + 1..],
                None => return "",
            }
        } else {
            return s;
        }
    }
}

/// The name of an item's code: attributes (`@x` or `@x(..)`)
/// skipped, then `fn`, `struct`, `const`, `var` (with its `<..>`),
/// `override`, `alias`; directives (`enable`, `diagnostic`, `requires`) and
/// anything else have no name.
fn head(code: &str) -> &str {
    let mut s = code;
    while let Some(rest) = s.strip_prefix('@') {
        let w = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
        s = rest[w..].trim_start();
        if s.starts_with('(') {
            let mut d = 0;
            let mut end = s.len();
            for (k, c) in s.char_indices() {
                match c {
                    '(' => d += 1,
                    ')' => {
                        d -= 1;
                        if d == 0 {
                            end = k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            s = s[end..].trim_start();
        }
    }
    let w = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len());
    let kind = &s[..w];
    let mut rest = s[w..].trim_start();
    if kind == "var" && rest.starts_with('<') {
        rest = rest.find('>').map_or("", |e| rest[e + 1..].trim_start());
    }
    match kind {
        "fn" | "struct" | "const" | "var" | "override" | "alias" => {
            let n = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
            &rest[..n]
        }
        _ => "",
    }
}

/// `src` with only the top-level items `entries` reach (by name, through
/// every identifier in their text that is not a `.member`), in their order,
/// without comments or indentation ([`push_code`]). Directives and unnamed
/// items always stay. A name no item declares is ignored, so an entry that
/// is missing just keeps less.
pub fn shake(src: &str, entries: &[&str]) -> String {
    let items = items(src);
    let mut by_name = std::collections::HashMap::new();
    for (k, it) in items.iter().enumerate() {
        if !it.name.is_empty() {
            by_name.insert(it.name, k);
        }
    }
    let mut keep = vec![false; items.len()];
    let mut stack: Vec<usize> = entries.iter().filter_map(|e| by_name.get(e).copied()).collect();
    for (k, it) in items.iter().enumerate() {
        if it.name.is_empty() {
            keep[k] = true;
        }
    }
    while let Some(k) = stack.pop() {
        if keep[k] && !items[k].name.is_empty() {
            continue;
        }
        keep[k] = true;
        for id in idents(items[k].text) {
            if let Some(&d) = by_name.get(id)
                && !keep[d]
            {
                stack.push(d);
            }
        }
    }
    let mut out = String::with_capacity(src.len() / 3);
    for (it, k) in items.iter().zip(keep) {
        if k {
            push_code(&mut out, it.text);
        }
    }
    out
}

/// `text`'s code onto `out`, one line per source line with its `//` comment,
/// indentation and trailing blanks dropped and blank lines skipped: what the
/// browser parses, nothing it ignores (WGSL has no strings, so `//` always
/// starts a comment; the sources hold no `/* */` comments).
fn push_code(out: &mut String, text: &str) {
    for line in text.lines() {
        let code = line.find("//").map_or(line, |c| &line[..c]).trim();
        if !code.is_empty() {
            out.push_str(code);
            out.push('\n');
        }
    }
}

/// The identifiers of `t` outside comments that are not `.member` names.
fn idents(t: &str) -> impl Iterator<Item = &str> {
    let b = t.as_bytes();
    let mut i = 0;
    core::iter::from_fn(move || {
        while i < b.len() {
            if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            let c = b[i];
            if c.is_ascii_alphabetic() || c == b'_' {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                if s > 0 && b[s - 1] == b'.' {
                    continue;
                }
                return Some(&t[s..i]);
            }
            if c.is_ascii_digit() {
                // A number literal (and its suffix): never a name.
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_keeps_what_the_entries_reach() {
        let src = "enable f16;\n// a note\nstruct S {\n a: f32,\n};\n@group(0) @binding(1) var<uniform> u: S;\nconst K: f32 = 2.0;\nfn used(x: f32) -> f32 {\n return x * K + u.a;\n}\nfn unused() -> f32 {\n return 1.0;\n}\n@fragment\nfn fs_main() -> @location(0) vec4<f32> {\n let s = S(1.0);\n return vec4<f32>(used(s.a));\n}\n";
        let out = shake(src, &["fs_main"]);
        assert!(out.contains("enable f16;"));
        assert!(out.contains("struct S") && out.contains("var<uniform> u") && out.contains("const K") && out.contains("fn used("));
        assert!(!out.contains("fn unused("));
        assert!(out.contains("@fragment\nfn fs_main("));
        // Comments and indentation are not sent; the code is.
        let c = shake("// head\nfn f() -> f32 {\n    // why\n    let a = 1.0; // trailing\n\n    return a;\n}\n", &["f"]);
        assert_eq!(c, "fn f() -> f32 {\nlet a = 1.0;\nreturn a;\n}\n");
        // A member named like a function does not keep it.
        let m = shake("fn a() -> f32 {\n return 1.0;\n}\nfn e() {\n let v = x.a;\n}\n", &["e"]);
        assert!(!m.contains("fn a("));
    }

    #[test]
    fn char_only_keeps_one_branch() {
        let src = "fn char_shade(kind: f32, base: vec3<f32>, n: vec3<f32>) -> CharShade {\n    let p0 = char_p(0u);\n    if (p0.x < 0.5) {\n        return char_none(base, n);\n    }\n    if (abs(kind - 2.0) < 0.5) {\n        return char_eye(base, n, p0.y);\n    }\n    if (abs(kind - 1.0) < 0.5) {\n        return char_skin(base, n, p0.y);\n    }\n    return char_none(base, n);\n}\nfn after() {}\n";
        let plain = char_only(src, 0);
        assert!(!plain.contains("char_skin") && !plain.contains("char_eye") && plain.contains("return char_none(base, n);\n}\nfn after()"));
        let skin = char_only(src, 1);
        assert!(skin.contains("return char_skin(base, n, p0.y);") && !skin.contains("char_eye"));
        assert_eq!(char_only(src, 3), src, "no hair branch: unchanged");
        assert_eq!(char_only("fn other() {}", 1), "fn other() {}");
    }

    /// A plain material keeps no character frame or coverage mask (it reads
    /// neither); the other kinds keep both whole.
    #[test]
    fn plain_materials_carry_no_character_frame_or_mask() {
        let src = "fn char_frame(p: vec3<f32>, uv: vec2<f32>, n: vec3<f32>) -> CharFrame {\nlet dp1 = dpdx(p);\nvar cf: CharFrame;\ncf.px = length(dp1);\nreturn cf;\n}\nfn char_out(c: vec4<f32>, a: f32, frag: vec2<f32>) -> CharOut {\nvar o: CharOut;\no.color = c;\no.mask = 0xffffffffu;\nif (a >= 0.999) {\nreturn o;\n}\nlet z = char_ign(frag);\nif (CHAR_MSAA > 1u) {\no.mask = 15u;\n} else {\n_ = z;\n}\nreturn o;\n}\nfn char_shade(kind: f32, base: vec3<f32>, n: vec3<f32>) -> CharShade {\nif (abs(kind - 1.0) < 0.5) {\nreturn char_skin(base, n, p0.y);\n}\nreturn char_none(base, n);\n}\nfn after() {}\n";
        let plain = char_only(src, 0);
        assert!(!plain.contains("dpdx") && !plain.contains("char_ign") && !plain.contains("CHAR_MSAA"), "{plain}");
        assert!(plain.contains("-> CharFrame {\nvar cf: CharFrame;\nreturn cf;\n}\nfn char_out("), "{plain}");
        assert!(plain.contains("-> CharOut {\nvar o: CharOut;\no.color = c;\no.mask = 0xffffffffu;\nreturn o;\n}\nfn char_shade("), "{plain}");
        assert!(plain.ends_with("return char_none(base, n);\n}\nfn after() {}\n"), "{plain}");
        let skin = char_only(src, 1);
        assert!(skin.contains("dpdx(p)") && skin.contains("char_ign(frag)"), "the skin keeps its frame and mask");
        // An unexpected head keeps the body whole.
        assert_eq!(replace_body("fn char_out(c: vec4<f32>, a: f32) -> Other {\nreturn 1;\n}", "fn char_out(c: vec4<f32>, a: f32,", "-> CharOut {", ""), None);
    }

    #[test]
    fn one_recipe_stage_has_only_that_recipe() {
        let all = crate::shader::stage_wgsl();
        let Some(first) = COMPILED.first() else {
            return;
        };
        let one = stage_one(first.code, OPAQUE).unwrap_or_default();
        assert!(one.len() <= all.len());
        assert_eq!(one.matches("case ").count(), 1);
        assert!(stage_one(0, OPAQUE).is_some_and(|s| !s.contains("case ")));
        assert!(!one.contains("fn mat_blend("), "the opaque text has no see-through code");
        if see_built(BLEND) {
            let b = stage_one(first.code, BLEND).unwrap_or_default();
            assert!(b.contains("fn mat_blend(") && b.contains("override REC_BLEND"));
            assert!(stage_one(0, BLEND).is_some_and(|s| s.contains("fn mat_blend(")));
        } else {
            assert!(stage_one(first.code, BLEND).is_none());
        }
    }
}
