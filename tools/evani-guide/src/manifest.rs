//! A light reader of a game's `evani.toml` (the module part) and the check
//! that the game crate really has what it lists. xtask's full reader (with
//! `[review]`) uses the same crate check.

use std::collections::BTreeSet;

/// The module part of `apps/<id>/evani.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Modules {
    pub name: String,
    pub kit: String,
    pub look: String,
    pub effects: Vec<String>,
    /// `simple`, `rigid` or `None`.
    pub physics: Option<String>,
    pub net: bool,
    pub xr: bool,
    pub live: bool,
    /// Stage 3 modules; `anim` implies `models` (the parser sets both).
    pub models: bool,
    pub anim: bool,
    pub ui: bool,
    /// `nature = ["ocean", ..]`: evani-material-nature with these of its
    /// parts (`None`: not listed, so not built). See [`MATERIAL_MODULES`].
    pub nature: Option<Vec<String>>,
    /// `proc = ["sky", ..]`: evani-material-proc with these of its parts.
    pub proc: Option<Vec<String>>,
}

/// The material modules of `[modules]`: key, engine crate. `key = [parts]`
/// turns on the app features `<key>` (the crate) and `<key>-<part>` for each
/// part (one of the crate's Cargo features); `key = []` is the crate alone.
pub const MATERIAL_MODULES: &[(&str, &str)] = &[("nature", "evani-material-nature"), ("proc", "evani-material-proc")];

/// A material module's parts: `None` when the key is absent, else a list of
/// distinct part names (lowercase letters, digits and `-`).
pub fn parts(m: Option<&toml::Table>, key: &str) -> Result<Option<Vec<String>>, String> {
    let Some(v) = m.and_then(|m| m.get(key)) else { return Ok(None) };
    let bad = || format!("evani.toml: `modules.{key}` must be a list of part names, e.g. `{key} = [\"{}\"]` (or `[]` for the crate alone)", if key == "nature" { "ocean" } else { "sky" });
    let list = v.as_array().ok_or_else(bad)?;
    let mut out: Vec<String> = Vec::new();
    for p in list {
        let p = p.as_str().filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).ok_or_else(bad)?;
        if out.iter().any(|x| x == p) {
            return Err(format!("evani.toml: `modules.{key}` lists `{p}` twice"));
        }
        out.push(p.to_string());
    }
    Ok(Some(out))
}

/// The app features of the material modules: `<key>` and `<key>-<part>`.
pub fn material_features(nature: Option<&[String]>, proc: Option<&[String]>) -> Vec<String> {
    let mut f = Vec::new();
    for (key, parts) in [("nature", nature), ("proc", proc)] {
        if let Some(parts) = parts {
            f.push(key.to_string());
            f.extend(parts.iter().map(|p| format!("{key}-{p}")));
        }
    }
    f
}

/// The optional engine crates; a game may only carry them as optional
/// dependencies behind features.
pub const OPTIONAL_CRATES: &[&str] = &["evani-fx", "evani-physics", "evani-net", "evani-xr", "evani-live", "evani-gltf", "evani-anim", "evani-ui", "evani-material-proc", "evani-material-layers", "evani-material-nature"];

impl Modules {
    pub fn parse(text: &str) -> Result<Self, String> {
        let t: toml::Table = text.parse().map_err(|e| format!("evani.toml: {e}"))?;
        let s = |k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_string).ok_or_else(|| format!("evani.toml: `{k}` must be a string"));
        let effects = match t.get("effects") {
            None => Vec::new(),
            Some(v) => v
                .as_array()
                .ok_or("evani.toml: `effects` must be a list of names")?
                .iter()
                .map(|e| e.as_str().map(str::to_string).ok_or_else(|| "evani.toml: every effect must be a string".to_string()))
                .collect::<Result<_, _>>()?,
        };
        let m = t.get("modules").and_then(toml::Value::as_table);
        let physics = match m.and_then(|m| m.get("physics")).map(toml::Value::as_str) {
            None | Some(Some("none")) => None,
            Some(Some(p @ ("simple" | "rigid"))) => Some(p.to_string()),
            _ => return Err("evani.toml: `modules.physics` must be \"simple\", \"rigid\" or \"none\"".into()),
        };
        let b = |k: &str| m.and_then(|m| m.get(k)).and_then(toml::Value::as_bool).unwrap_or(false);
        let anim = b("anim");
        let (nature, proc) = (parts(m, "nature")?, parts(m, "proc")?);
        Ok(Self { name: s("name")?, kit: s("kit")?, look: s("look")?, effects, physics, net: b("net"), xr: b("xr"), live: b("live"), models: b("models") || anim, anim, ui: b("ui"), nature, proc })
    }

    /// The game crate features this list turns on (the build always passes
    /// `--no-default-features`, so this is the complete set).
    pub fn features(&self) -> Vec<String> {
        let mut f = Vec::new();
        if !self.effects.is_empty() {
            f.push("fx".to_string());
        }
        f.extend(self.effects.iter().cloned());
        if let Some(p) = &self.physics {
            f.push(format!("physics-{p}"));
        }
        for (on, name) in [(self.net, "net"), (self.xr, "xr"), (self.live, "live"), (self.models, "models"), (self.anim, "anim"), (self.ui, "ui")] {
            if on {
                f.push(name.to_string());
            }
        }
        f.extend(material_features(self.nature.as_deref(), self.proc.as_deref()));
        f
    }

    /// The listed material modules: `(key, engine crate, parts)`.
    pub fn materials(&self) -> Vec<(&'static str, &'static str, &[String])> {
        MATERIAL_MODULES
            .iter()
            .zip([&self.nature, &self.proc])
            .filter_map(|(&(key, krate), parts)| parts.as_deref().map(|p| (key, krate, p)))
            .collect()
    }
}

/// Every way the game crate's `Cargo.toml` fails the list: a feature the
/// list asks for that the crate lacks, the kit or look crate not a
/// dependency.
pub fn crate_problems(features: &[String], kit: &str, look: &str, cargo_toml: &str) -> Vec<String> {
    let t: toml::Table = match cargo_toml.parse() {
        Ok(t) => t,
        Err(e) => return vec![format!("Cargo.toml: {e}")],
    };
    let feats = t.get("features").and_then(toml::Value::as_table);
    let mut out: Vec<String> = features
        .iter()
        .filter(|f| !feats.is_some_and(|x| x.contains_key(f.as_str())))
        .map(|f| format!("evani.toml asks for `{f}` but the app crate has no such feature"))
        .collect();
    let deps = t.get("dependencies").and_then(toml::Value::as_table);
    for dep in [format!("evani-kit-{kit}"), format!("evani-look-{look}")] {
        if !deps.is_some_and(|d| d.contains_key(&dep)) {
            out.push(format!("evani.toml names `{dep}` but the app crate doesn't depend on it"));
        }
    }
    out
}

/// Optional engine crates the game crate depends on without `optional =
/// true` (they would be in every build, listed or not).
pub fn not_optional(cargo_toml: &str) -> Vec<String> {
    let Ok(t) = cargo_toml.parse::<toml::Table>() else { return Vec::new() };
    let mut tables: Vec<&toml::Table> = t.get("dependencies").and_then(toml::Value::as_table).into_iter().collect();
    for target in t.get("target").and_then(toml::Value::as_table).into_iter().flat_map(|x| x.values()) {
        tables.extend(target.get("dependencies").and_then(toml::Value::as_table));
    }
    let mut out = Vec::new();
    for deps in tables {
        for c in OPTIONAL_CRATES {
            if let Some(d) = deps.get(*c)
                && !d.get("optional").and_then(toml::Value::as_bool).unwrap_or(false)
            {
                out.push((*c).to_string());
            }
        }
    }
    out
}

/// The crate's `default` features when they don't mirror the list (`fx` is
/// implied by any effect, so it doesn't count): `(missing, extra)`.
pub fn default_mismatch(features: &[String], cargo_toml: &str) -> Option<(Vec<String>, Vec<String>)> {
    let t: toml::Table = cargo_toml.parse().ok()?;
    let default: BTreeSet<String> = t
        .get("features")
        .and_then(|f| f.get("default"))
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let want: BTreeSet<String> = features.iter().cloned().collect();
    let strip = |s: &BTreeSet<String>| s.iter().filter(|f| *f != "fx").cloned().collect::<BTreeSet<_>>();
    let (default, want) = (strip(&default), strip(&want));
    if default == want {
        return None;
    }
    Some((want.difference(&default).cloned().collect(), default.difference(&want).cloned().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO: &str = "[features]\ndefault = [\"bloom\", \"live\"]\nfx = []\nbloom = [\"fx\"]\nlive = [\"dep:evani-live\"]\n[dependencies]\nevani-kit-demo.workspace = true\nevani-look-basic.workspace = true\nevani-live = { workspace = true, optional = true }\nevani-fx.workspace = true\n";

    #[test]
    fn reads_the_module_list() {
        let m = Modules::parse("name = \"d\"\nkit = \"demo\"\nlook = \"basic\"\neffects = [\"bloom\"]\n[modules]\nphysics = \"none\"\nlive = true\n").unwrap();
        assert_eq!(m.features(), ["fx", "bloom", "live"]);
        assert!(Modules::parse("name = \"d\"\nkit = \"demo\"\nlook = \"basic\"\n[modules]\nphysics = \"soft\"\n").is_err());
    }

    #[test]
    fn material_modules_turn_on_the_crate_and_each_part() {
        let m = Modules::parse("name = \"d\"\nkit = \"demo\"\nlook = \"basic\"\n[modules]\nnature = [\"ocean\"]\nproc = [\"sky\", \"relief\"]\n").unwrap();
        assert_eq!(m.features(), ["nature", "nature-ocean", "proc", "proc-sky", "proc-relief"]);
        assert_eq!(m.materials(), [("nature", "evani-material-nature", &["ocean".to_string()][..]), ("proc", "evani-material-proc", &["sky".to_string(), "relief".to_string()][..])]);
        // `[]` is the crate alone; not listed is nothing at all.
        let alone = Modules::parse("name = \"d\"\nkit = \"demo\"\nlook = \"basic\"\n[modules]\nnature = []\n").unwrap();
        assert_eq!(alone.features(), ["nature"]);
        let none = Modules::parse("name = \"d\"\nkit = \"demo\"\nlook = \"basic\"\n").unwrap();
        assert!(none.features().is_empty() && none.nature.is_none() && none.proc.is_none());
        // Only a list of part names, each once.
        for bad in ["nature = true", "nature = \"ocean\"", "nature = [\"Ocean\"]", "nature = [1]", "proc = [\"sky\", \"sky\"]"] {
            assert!(Modules::parse(&format!("name = \"d\"\nkit = \"demo\"\nlook = \"basic\"\n[modules]\n{bad}\n")).is_err(), "{bad}");
        }
    }

    #[test]
    fn crate_checks() {
        let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(crate_problems(&f(&["fx", "bloom", "live"]), "demo", "basic", CARGO).is_empty());
        let p = crate_problems(&f(&["fx", "ssao"]), "kart", "basic", CARGO);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("`ssao`") && p[1].contains("evani-kit-kart"));
        assert_eq!(not_optional(CARGO), ["evani-fx"]);
        assert_eq!(default_mismatch(&f(&["fx", "bloom", "live"]), CARGO), None);
        assert_eq!(default_mismatch(&f(&["fx", "bloom"]), CARGO), Some((vec![], f(&["live"]))));
    }
}
