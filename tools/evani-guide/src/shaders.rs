//! The material shader budget (`check`, docs/materials/SHADER_BUDGET.md):
//! every catalogue material draws with its own small shader, one per
//! (recipe, character kind, pass), so the browser compiles only what a
//! material uses (an all-in-one module with every recipe took 21-28 s to
//! compile on an RTX 4060 Ti; one material's takes 0.6-3.7 s). Two rules
//! keep it that way:
//!
//! - **Size.** Each material's module stays under 60 KB
//!   ([`MODULE_BUDGET`]). The check sizes each recipe stage exactly as the
//!   game builds it (its recipe alone, the game's condition layers, the
//!   cut-out, the see-through pass and the style step when listed, cut to
//!   what the lit stage calls) against the stage's share ([`STAGE_BUDGET`]);
//!   the rest of the module (the forward module, lights, sky) is held under
//!   [`TEMPLATE_ALLOWANCE`] ([`CHAR_ALLOWANCE`] with a character kind's
//!   shading, whose stage share is [`CHAR_STAGE_BUDGET`]) by evani-render's
//!   recipe shader test, which builds every real module.
//! - **Count.** A game lists at most [`GAME_PERMUTATIONS`] distinct material
//!   shaders (each is built once or twice, at 1x and 4x MSAA, in the
//!   background); a tool app (`kind = "tool"`, the material viewer, which
//!   draws the whole catalogue) at most [`TOOL_PERMUTATIONS`].
//!
//! Native and text-only (the recipe crate's shader text, no engine, no wgpu).

use evani_material_recipes::shader::{ALL_LAYERS, ALL_RECIPES, LayerSrc, ORGANIC_PLAIN, RecipeSrc, WOOD_PLANKS};
use evani_material_recipes::split::{self, BLEND, CHAR_ALLOWANCE, CHAR_STAGE_BUDGET, GLASS, GAME_PERMUTATIONS, MODULE_BUDGET, OPAQUE, STAGE_BUDGET, TEMPLATE_ALLOWANCE, TOOL_PERMUTATIONS};

use crate::check::Severity;
use crate::materials::{self, CONDITIONS, Listed, Route};

/// The recipes that have see-through rows (evani-material-recipes
/// `blend::mode_of`: glass, fire's flame, smoke and steam, glow's hologram,
/// a clear gem or organic), and the one that refracts (frosted, wavy and
/// block glass).
const SEE_THROUGH: [&str; 5] = ["glass", "fire", "glow", "gem", "organic"];
const REFRACTING: [&str; 1] = ["glass"];

/// One material shader the game can build: recipe (`None`: no recipe, the
/// plain materials and every code without a stage case), character kind
/// name (`None`: plain) and pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permutation {
    pub recipe: Option<String>,
    pub kind: Option<&'static str>,
    pub see: u8,
}

impl Permutation {
    /// This shader's stage share and the allowance for the rest of its
    /// module: a character kind carries its shading, so its stage gets less.
    pub fn budget(&self) -> (usize, usize) {
        if self.kind.is_some() { (CHAR_STAGE_BUDGET, CHAR_ALLOWANCE) } else { (STAGE_BUDGET, TEMPLATE_ALLOWANCE) }
    }

    fn label(&self) -> String {
        let what = match (&self.recipe, self.kind) {
            (Some(r), _) => format!("`{r}`"),
            (None, Some(k)) => format!("the {k} shading"),
            (None, None) => "the no-recipe material".to_string(),
        };
        let pass = match self.see {
            OPAQUE => "",
            BLEND => " (see-through)",
            _ => " (refracting glass)",
        };
        format!("{what}{pass}")
    }
}

/// Every material shader `listed` lets the game build (empty when it lists
/// no recipe stage).
pub fn permutations(listed: &Listed) -> Vec<Permutation> {
    let f = listed.features();
    if !f.iter().any(|x| x == materials::FEATURE_RECIPES) {
        return Vec::new();
    }
    let stage: Vec<String> = materials::RECIPES
        .iter()
        .filter(|(n, r)| r.recipe_stage() && listed.has_recipe(n))
        .map(|(n, _)| (*n).to_string())
        .collect();
    let blend = f.iter().any(|x| x == materials::FEATURE_BLEND);
    let refract = f.iter().any(|x| x == materials::FEATURE_REFRACT);
    let mut out = vec![Permutation { recipe: None, kind: None, see: OPAQUE }];
    out.extend(stage.iter().map(|r| Permutation { recipe: Some(r.clone()), kind: None, see: OPAQUE }));
    for (n, r) in materials::RECIPES {
        if r == Route::Character && listed.has_recipe(n) {
            out.push(Permutation { recipe: None, kind: Some(n), see: OPAQUE });
        }
    }
    if blend {
        out.push(Permutation { recipe: None, kind: None, see: BLEND });
        out.extend(stage.iter().filter(|r| SEE_THROUGH.contains(&r.as_str())).map(|r| Permutation { recipe: Some(r.clone()), kind: None, see: BLEND }));
    }
    if refract {
        out.extend(stage.iter().filter(|r| REFRACTING.contains(&r.as_str())).map(|r| Permutation { recipe: Some(r.clone()), kind: None, see: GLASS }));
    }
    out
}

/// The recipe text the game compiles for `name` (wood without its layouts
/// part draws planks, organic without its food part plain flesh).
fn recipe_src(name: &str, listed: &Listed) -> Option<RecipeSrc> {
    let code = materials::RECIPES.iter().position(|(n, _)| *n == name)? + 1;
    let code = u8::try_from(code).ok()?;
    if name == "wood" && !listed.recipes.iter().any(|r| r == "wood-layouts") {
        return Some(WOOD_PLANKS);
    }
    if name == "organic" && !listed.recipes.iter().any(|r| r == "organic-food") {
        return Some(ORGANIC_PLAIN);
    }
    ALL_RECIPES.iter().find(|r| r.code == code).copied()
}

/// The recipe-stage bytes of one permutation, as the game builds it.
pub fn stage_bytes(p: &Permutation, listed: &Listed, style: bool) -> usize {
    let layers: Vec<LayerSrc> = CONDITIONS
        .iter()
        .zip(ALL_LAYERS.iter())
        .filter(|(c, _)| listed.conditions.iter().any(|x| x == *c))
        .map(|(_, l)| *l)
        .collect();
    let cut = listed.recipes.iter().any(|r| r == materials::CUTOUT);
    let src = p.recipe.as_deref().and_then(|r| recipe_src(r, listed));
    split::stage_for(src.as_ref(), &layers, cut, style, p.see).len()
}

/// `(severity, what, fix)` for each shader over budget and for too many
/// shaders. `tool`: the app is a tool app (its higher cap); `style`: the app
/// builds the recipe stage's style detail step (`material-recipe-style`).
pub fn problems(app: &str, listed: &Listed, tool: bool, style: bool) -> Vec<(Severity, String, String)> {
    let perms = permutations(listed);
    let mut out = Vec::new();
    for p in &perms {
        out.extend(size_problem(app, p, stage_bytes(p, listed, style)));
    }
    let cap = if tool { TOOL_PERMUTATIONS } else { GAME_PERMUTATIONS };
    if perms.len() > cap {
        out.push((
            Severity::Error,
            format!(
                "`[materials]` makes {} distinct material shaders (each recipe, character kind and see-through pass is its own, compiled by the browser at start-up), over the {} cap for {}",
                perms.len(),
                cap,
                if tool { "a tool app" } else { "a game" }
            ),
            format!(
                "List only the recipes the game draws in `[materials] recipes` of apps/{app}/evani.toml (drop `blend` or `refraction` if nothing is see-through), so it builds at most {cap} (docs/materials/SHADER_BUDGET.md)."
            ),
        ));
    }
    out
}

/// The size error for permutation `p` whose recipe stage is `n` bytes, with
/// its fix (`None` within budget).
fn size_problem(app: &str, p: &Permutation, n: usize) -> Option<(Severity, String, String)> {
    let (stage_budget, allowance) = p.budget();
    if n <= stage_budget {
        return None;
    }
    Some((
        Severity::Error,
        format!(
            "the material shader for {} would be about {:.1} KB: its recipe stage is {:.1} KB, over its {:.0} KB share of the {:.0} KB budget per material shader",
            p.label(),
            (n + allowance) as f64 / 1024.0,
            n as f64 / 1024.0,
            stage_budget as f64 / 1024.0,
            MODULE_BUDGET as f64 / 1024.0
        ),
        format!(
            "Make that recipe's WGSL smaller (crates/evani-material-recipes/shaders/rc_<name>.wgsl): move rarely used variants into an optional part listed on its own (like `wood-layouts`), share helpers through rc_shared.wgsl, or list fewer condition layers in apps/{app}/evani.toml; then rerun `cargo xtask check` (docs/materials/SHADER_BUDGET.md)."
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(recipes: &[&str], conditions: &[&str]) -> Listed {
        Listed { recipes: recipes.iter().map(|s| (*s).to_string()).collect(), conditions: conditions.iter().map(|s| (*s).to_string()).collect() }
    }

    #[test]
    fn no_recipes_no_shaders() {
        assert!(permutations(&Listed::default()).is_empty());
        assert!(problems("g", &Listed::default(), false, false).is_empty());
    }

    #[test]
    fn counts_recipes_kinds_and_passes() {
        let l = listed(&["wood", "metal", "skin", "eye", "glass", "blend", "refraction"], &["wet"]);
        let p = permutations(&l);
        // no recipe, wood, metal, glass; skin, eye; blend: none, glass; glass pass.
        assert_eq!(p.len(), 4 + 2 + 2 + 1, "{p:?}");
        assert!(p.iter().any(|x| x.see == GLASS && x.recipe.as_deref() == Some("glass")));
        // A part is not a shader of its own.
        assert_eq!(permutations(&listed(&["wood", "wood-layouts"], &[])).len(), 2);
    }

    #[test]
    fn every_recipe_stage_is_under_its_share() {
        let all: Vec<&str> = materials::RECIPES.iter().map(|(n, _)| *n).chain(["wood-layouts", "organic-food", "cutout", "blend", "refraction"]).collect();
        let l = listed(&all, &CONDITIONS);
        for p in permutations(&l) {
            let n = stage_bytes(&p, &l, true);
            assert!(n <= p.budget().0, "{}: {n} bytes", p.label());
        }
    }

    #[test]
    fn an_oversized_shader_is_an_error_with_a_fix() {
        let plain = Permutation { recipe: Some("wood".to_string()), kind: None, see: OPAQUE };
        assert!(size_problem("g", &plain, STAGE_BUDGET).is_none(), "at the budget is fine");
        let (s, what, fix) = size_problem("g", &plain, STAGE_BUDGET + 1).expect("one byte over is an error");
        assert!(s == Severity::Error);
        assert!(what.contains("`wood`") && what.contains("60 KB budget"), "{what}");
        assert!(fix.contains("apps/g/evani.toml") && fix.contains("SHADER_BUDGET.md"), "{fix}");
        // A character kind's stage has the smaller share.
        let skin = Permutation { recipe: None, kind: Some("skin"), see: OPAQUE };
        assert!(size_problem("g", &skin, CHAR_STAGE_BUDGET + 1).is_some());
        const { assert!(CHAR_STAGE_BUDGET < STAGE_BUDGET && STAGE_BUDGET + TEMPLATE_ALLOWANCE == MODULE_BUDGET && CHAR_STAGE_BUDGET + CHAR_ALLOWANCE == MODULE_BUDGET) };
    }

    #[test]
    fn the_cap_is_an_error_with_a_fix() {
        let many: Vec<&str> = materials::RECIPES.iter().map(|(n, _)| *n).collect();
        let l = listed(&many, &[]);
        let game = problems("g", &l, false, false);
        assert!(game.iter().any(|(s, what, fix)| *s == Severity::Error && what.contains("distinct material shaders") && fix.contains("apps/g/evani.toml")));
        assert!(!problems("v", &l, true, false).iter().any(|(_, what, _)| what.contains("distinct")), "a tool app may draw the catalogue");
    }
}
