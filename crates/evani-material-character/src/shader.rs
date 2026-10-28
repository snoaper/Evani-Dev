//! `shaders/character.wgsl` and the patch that wires it into the lit
//! fragment stage (`model_lit_fs`, evani-render `look-surface`).
//!
//! [`patch_lit`] takes the forward module the model pipelines compile (the
//! engine's `FORWARD.source`, comments already stripped) and rewrites eight
//! lines of `model_lit_fs`, by exact text, so it calls `char_frame` (every
//! screen derivative, before the discard), `char_shade` (per material kind)
//! and `char_out` (the coverage mask). If the lit pass ever changes so that
//! a line is missing, the patch returns `None` and the engine compiles the
//! unpatched pass: no character shading, never a broken pipeline. The
//! `tests` below and evani-render's `char_wgsl` test hold the lines.

use evani_core::slots::SlotSignature;

/// The live character constants by index (see [`crate::params::entries`]):
/// 0 on/quality/MSAA, 1 to 5 eye, 6 to 8 skin, 9 to 12 hair, 13 to 28 the
/// fitted skin lookup. Zero by default: no character shading.
pub const CHAR_PARAMS: SlotSignature = SlotSignature {
    name: "char_params",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Character material constants by index: 0 on/quality/MSAA, 1-5 eye, 6-8 skin, 9-12 hair, 13-28 skin lookup fit",
    default_body: "    return vec4<f32>(0.0);",
};

/// The character WGSL, with comments.
pub const WGSL: &str = include_str!("../shaders/character.wgsl");

/// The lines of `model_lit_fs` the patch rewrites: (exact text, what it
/// becomes). Each is replaced once, in order, after `fn model_lit_fs(`.
pub const EDITS: [(&str, &str); 8] = [
    ("-> @location(0) vec4<f32> {", "-> CharOut {"),
    (
        "let occ_tex = textureSample(occlusion_map, map_sampler, uv).r;",
        "let occ_tex = textureSample(occlusion_map, map_sampler, uv).r; let cf = char_frame(in.world, uv, normalize(in.normal));",
    ),
    ("let base = tex * model_mat.base", "var base = tex * model_mat.base"),
    (
        "let l = frame.light_dir.xyz;",
        "let l = frame.light_dir.xyz; let chs = char_shade(kind, base.rgb, n, g, v, l, uv, cf); n = chs.n_spec; rough = select(rough, chs.rough, chs.rough >= 0.0); base = vec4<f32>(chs.albedo, base.a);",
    ),
    ("let sun_rgb = frame.light_color.rgb;", "lit3 = mix(lit3, chs.lit3, chs.own_lit); let sun_rgb = frame.light_color.rgb;"),
    ("let spec = slot_specular(", "let spec = chs.spec_k * slot_specular("),
    (
        "+ vec3<f32>(lit_catch(n, v, wet, occ));",
        "+ vec3<f32>(lit_catch(n, v, wet, occ)) + chs.extra * sun_rgb * (vis * direct_occ) + vec3<f32>(chs.catch_l * lit_sun() * ao);",
    ),
    (
        "return vec4<f32>(mix(c, frame.fog_color.rgb, f), 1.0);",
        "return char_out(vec4<f32>(mix(c, frame.fog_color.rgb, f), 1.0), chs.alpha, in.pos.xy);",
    ),
];

/// `s` without `//` comments, keeping `//@` slot markers and `// evani.`
/// names (as the engine strips its own WGSL; WGSL has no strings).
pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        match line.find("//") {
            Some(i) if !line[i..].starts_with("//@") && !line[i..].starts_with("// evani.") => {
                out.push_str(line[..i].trim_end());
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ => out.push_str(line),
        }
    }
    out
}

/// The forward module `template` with `model_lit_fs` wired to the
/// character materials, for a pipeline with `msaa` samples (1 or 4), or
/// `None` when a line to patch is missing (no lit pass, or it changed).
pub fn patch_lit(template: &str, msaa: u32) -> Option<String> {
    patch_entry(template, "fn model_lit_fs(", &EDITS, msaa)
}

/// The lines of the layered stage `model_layered_fs` (evani-render
/// `material-layers`, `shaders/model_lit_layers.wgsl`) the patch rewrites
/// when a game turns on both families. The same eight edits as [`EDITS`]
/// (that stage already has `var base` and `var spec`), plus one: a material
/// with a character kind (`surface.kind.z` above 0.5, the code `char_shade`
/// picks its shading by) reads as having no layer flag, so its layer record
/// is ignored and it draws exactly as the character pass on the plain lit
/// stage. Untagged materials keep every layer.
pub const LAYERED_EDITS: [(&str, &str); 9] = [
    EDITS[0],
    EDITS[1],
    ("let flags = u32(ly_mat.iri.w + 0.5);", "let flags = select(u32(ly_mat.iri.w + 0.5), 0u, surface.kind.z > 0.5);"),
    ("var base = tex * model_mat.base", "var base = tex * model_mat.base"),
    EDITS[3],
    EDITS[4],
    ("var spec = slot_specular(", "var spec = chs.spec_k * slot_specular("),
    EDITS[6],
    EDITS[7],
];

/// [`patch_lit`] for a module whose model pipelines draw the layered stage
/// `model_layered_fs` (evani-render `material-layers` on): its body gets
/// [`LAYERED_EDITS`]; `model_lit_fs` stays in the module unpatched and
/// unused. `None` when a line is missing, as for [`patch_lit`].
pub fn patch_layered(template: &str, msaa: u32) -> Option<String> {
    patch_entry(template, "fn model_layered_fs(", &LAYERED_EDITS, msaa)
}

/// `edits`, in order, after `entry` in `template`, then the character WGSL.
fn patch_entry(template: &str, entry: &str, edits: &[(&str, &str)], msaa: u32) -> Option<String> {
    let at = template.find(entry)?;
    let (head, tail) = template.split_at(at);
    let mut tail = tail.to_string();
    let mut from = 0;
    for &(find, with) in edits {
        let i = from + tail[from..].find(find)?;
        tail.replace_range(i..i + find.len(), with);
        from = i + with.len();
    }
    Some(format!("{head}{tail}\nconst CHAR_MSAA: u32 = {}u;\n{}", msaa.max(1), strip(WGSL)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIT: &str = include_str!("../../evani-render/shaders/model_lit.wgsl");

    #[test]
    fn every_line_is_in_the_lit_pass_in_order() {
        let p = patch_lit(LIT, 4).expect("all lines found");
        assert!(p.contains("-> CharOut {"));
        assert!(p.contains("const CHAR_MSAA: u32 = 4u;"));
        assert!(p.contains("//@slot char_params"), "the slot marker survives the strip");
        assert!(p.contains("// evani.shader.character"));
        assert!(!p.contains("tentabrobpy"), "comments are stripped");
        assert_eq!(p.matches("char_shade(").count(), 2, "one call, one definition");
        assert!(patch_lit(LIT, 1).is_some_and(|s| s.contains("const CHAR_MSAA: u32 = 1u;")));
    }

    #[test]
    fn a_missing_line_means_no_patch() {
        assert!(patch_lit("fn model_fs() {}", 1).is_none());
        let broken = LIT.replace("let spec = slot_specular(", "let spec = other(");
        assert!(patch_lit(&broken, 1).is_none());
    }

    /// Both families on: the layered stage is the one patched, the plain
    /// stage stays as it was, and character kinds drop their layer flags.
    #[test]
    fn the_layered_stage_patches_when_both_are_on() {
        const LAYERED: &str = include_str!("../../evani-render/shaders/model_lit_layers.wgsl");
        let both = format!("{LIT}\n{LAYERED}");
        let p = patch_layered(&both, 4).expect("all lines found in model_layered_fs");
        let (plain, layered) = p.split_at(p.find("fn model_layered_fs(").expect("layered stage"));
        assert!(!plain.contains("char_shade("), "model_lit_fs is not patched");
        assert!(layered.contains("fn model_layered_fs(in: ModelOut, @builtin(front_facing) facing: bool) -> CharOut {"));
        assert!(layered.contains("let flags = select(u32(ly_mat.iri.w + 0.5), 0u, surface.kind.z > 0.5);"));
        assert!(layered.contains("var spec = chs.spec_k * slot_specular("));
        assert_eq!(layered.matches("char_shade(").count(), 2, "one call, one definition");
        assert!(patch_layered(LIT, 1).is_none(), "no layered stage: nothing to patch");
    }

    #[test]
    fn strip_keeps_markers_and_code() {
        assert_eq!(strip("a // x\n//@slot t\n// evani.k\nb\n// gone\n"), "a\n//@slot t\n// evani.k\nb\n\n");
    }
}
