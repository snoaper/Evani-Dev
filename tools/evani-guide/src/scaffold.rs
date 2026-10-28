//! `new-game <id> [--look <look>] [--genre <genre>] [--effects <list>]`: a new game from the working example.
//!
//! - `kits/evani-kit-<id>/` from `kits/evani-kit-example` (names, parameter
//!   owner `kit.<id>`, system/scene/data names `<id>.*`, `<Id>KitPlugin`).
//! - `apps/<id>/` from `apps/sandbox` (no `dist/`), using that kit and the
//!   look crate `looks/evani-look-<look>`.
//! - Both added to the workspace `members`, the kit to
//!   `[workspace.dependencies]`.
//! - The game's guidance (`apps/<id>/AGENTS.md` and its `CLAUDE.md` pointer).
//! - `[playground] free_roam = true` in its `evani.toml`: new games start as
//!   playgrounds, and the example kit supports free roam with its test.
//!
//! With `--genre` / `--effects` (see [`crate::genre`]) the choices are checked
//! against the catalog first; then the game's `evani.toml` and `Cargo.toml` get
//! the genre's physics and the chosen effects, and `kits/<id>/SPEC.md` is
//! written with the pinned reference image prompt ([`Extras`]).
//!
//! The target pack (`kits/<id>/target/`) is not copied: it is made fresh for
//! each game (docs/KIT_PROCESS.md step 2), and all content is original.

use std::path::Path;

use crate::targets;

const KIT_TEMPLATE: &str = "kits/evani-kit-example";
const APP_TEMPLATE: &str = "apps/sandbox";
/// Text files that get the renames; anything else is copied as is.
const TEXT: &[&str] = &["rs", "toml", "md", "json", "html", "wgsl", "css", "js", "txt"];
/// Added to a new game's `evani.toml`: it is a playground and supports free
/// roam from the start.
const PLAYGROUND: &str = "\n# A playground for the Evani.app library: it must support free roam\n# (docs/PLAYGROUNDS.md). Remove this table if the game isn't for the library.\n[playground]\nfree_roam = true\n";

/// A game id: lowercase letters, digits and single `-`, starting with a
/// letter (parameter names are `kit.<id with _>.<name>`).
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 40
        && b[0].is_ascii_lowercase()
        && !id.ends_with('-')
        && !id.contains("--")
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn camel(id: &str) -> String {
    id.split('-')
        .map(|p| {
            let mut c = p.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

/// The look crate's main `LookPlugin` struct: one with `impl LookPlugin for
/// X` that is defined (`pub struct X`) or re-exported (`pub use ...X`) at the
/// crate root, so the new game's `use evani_look_<look>::X;` resolves. Side
/// looks in sub-modules (photoreal's `daylight::DaylightLook`, stylizedpbr's
/// `film::FilmLook`) also implement `LookPlugin` but are not at the root;
/// `src/lib.rs` is searched first.
fn look_struct(root: &Path, look: &str) -> Option<String> {
    let src = root.join("looks").join(format!("evani-look-{look}")).join("src");
    let lib_rs = src.join("lib.rs");
    let at_root = root_types(&std::fs::read_to_string(&lib_rs).ok()?);
    let mut rest = Vec::new();
    list_files(&src, &mut rest);
    rest.sort();
    let files = std::iter::once(lib_rs.clone()).chain(rest.into_iter().filter(|f| *f != lib_rs));
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        for (i, _) in text.match_indices("impl LookPlugin for ") {
            let name: String = text[i + "impl LookPlugin for ".len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() && at_root.contains(&name) {
                return Some(name);
            }
        }
    }
    None
}

/// Type names a crate's `lib.rs` makes public at its root: `pub struct X` /
/// `pub enum X` at the start of a line (not inside a module), and the last
/// segment (or `as` name) of each `pub use` item.
fn root_types(lib: &str) -> Vec<String> {
    let ident = |s: &str| s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>();
    let mut out = Vec::new();
    for line in lib.lines() {
        for kw in ["pub struct ", "pub enum "] {
            if let Some(r) = line.strip_prefix(kw) {
                out.push(ident(r));
            }
        }
        if let Some(r) = line.strip_prefix("pub use ") {
            let r = r.trim_end().trim_end_matches(';');
            let items = match (r.find('{'), r.rfind('}')) {
                (Some(a), Some(b)) if a < b => r[a + 1..b].split(',').map(str::to_string).collect(),
                _ => vec![r.to_string()],
            };
            for it in items {
                let it = it.trim();
                let name = match it.rsplit_once(" as ") {
                    Some((_, alias)) => alias.trim(),
                    None => it.rsplit("::").next().unwrap_or(it),
                };
                out.push(ident(name));
            }
        }
    }
    out.retain(|n| !n.is_empty());
    out
}

fn list_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            let n = e.file_name();
            if !matches!(n.to_string_lossy().as_ref(), "dist" | "target") {
                list_files(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

/// The sandbox's own proof content (the textured model, S3-3) that a new
/// game must not inherit: these files and folders (prefixes) are not
/// copied, and [`drop_proof`] removes the lines that wire them in.
const PROOF_FILES: &[&str] = &["assets/models/", "src/proof_model.rs", "src/textpanel.rs"];

/// Proof wiring that shares a line with template code (so the sandbox's
/// build without `ui` keeps its line numbers): cut out of the line, not the
/// line.
const PROOF_SNIPPETS: &[&str] = &[" #[cfg(feature = \"ui\")] mod textpanel;", " #[cfg(feature = \"ui\")] game.system(textpanel::system());"];

/// Drop the template's proof lines: every line containing
/// `proof-model:line`, and every block from a line containing
/// `proof-model:begin` to the next line containing `proof-model:end`, and
/// the [`PROOF_SNIPPETS`] inside lines.
fn drop_proof(text: &str) -> String {
    let text = PROOF_SNIPPETS.iter().fold(text.to_string(), |t, s| t.replace(s, ""));
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        if skipping || line.contains("proof-model:begin") {
            skipping = !line.contains("proof-model:end");
        } else if !line.contains("proof-model:line") {
            out.push_str(line);
        }
    }
    out
}

fn copy_tree(root: &Path, from: &str, to: &str, skip: &[&str], subst: &dyn Fn(&str) -> String) -> Result<Vec<String>, String> {
    let src = root.join(from);
    let mut files = Vec::new();
    list_files(&src, &mut files);
    let mut out = Vec::new();
    for f in files {
        let rel = crate::rel(&src, &f);
        if skip.contains(&rel.as_str()) || PROOF_FILES.iter().any(|p| rel.starts_with(p)) {
            continue;
        }
        let dest = root.join(to).join(&rel);
        let is_text = f.extension().and_then(|e| e.to_str()).is_some_and(|e| TEXT.contains(&e));
        if is_text {
            let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            crate::write_file(&dest, &subst(&drop_proof(&text)))?;
        } else {
            if let Some(d) = dest.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&f, &dest).map_err(|e| format!("{}: {e}", f.display()))?;
        }
        out.push(crate::rel(root, &dest));
    }
    Ok(out)
}

/// Insert `line` after the last line of the `members = [` list that starts
/// with `"<prefix>`, and the kit after the last `evani-kit-` dependency.
fn add_to_workspace(text: &str, id: &str) -> Result<String, String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let after_last = |lines: &mut Vec<String>, starts: &str, new: String| -> Result<(), String> {
        let i = lines.iter().rposition(|l| l.trim_start().starts_with(starts)).ok_or_else(|| format!("Cargo.toml: no line starting with {starts} to add {new:?} after"))?;
        let indent: String = lines[i].chars().take_while(|c| c.is_whitespace()).collect();
        lines.insert(i + 1, format!("{indent}{new}"));
        Ok(())
    };
    after_last(&mut lines, "\"kits/", format!("\"kits/evani-kit-{id}\","))?;
    after_last(&mut lines, "\"apps/", format!("\"apps/{id}\","))?;
    after_last(&mut lines, "evani-kit-", format!("evani-kit-{id} = {{ path = \"kits/evani-kit-{id}\" }}"))?;
    let mut s = lines.join(nl);
    if text.ends_with('\n') {
        s.push_str(nl);
    }
    Ok(s)
}

/// The sandbox's test pins the basic look's own values; another look gets
/// the test without them.
fn drop_basic_look_asserts(text: &str) -> String {
    let (Some(a), Some(b)) = (text.find("        // The look overrides the renderer's own saturation default."), text.find("ParamValue::Color([1.0; 3])));")) else {
        return text.to_string();
    };
    if b < a {
        return text.to_string();
    }
    let end = text[b..].find('\n').map_or(text.len(), |n| b + n + 1);
    format!("{}{}", &text[..a], &text[end..])
}

/// The template lists some looks as optional dependencies of its proof
/// features. After the rename, the chosen look is the game's own look (a
/// plain dependency), so its optional line would be a second
/// `evani-look-<look>` key and Cargo refuses the file. Drop that optional
/// line, and drop `"dep:evani-look-<look>"` from the feature lists that
/// turned it on (the dependency is always on now; the feature's other entries,
/// like `evani-look-<look>/film`, stay valid).
fn fold_chosen_look(text: &str, look: &str) -> String {
    let key = format!("evani-look-{look}");
    let dep = format!("\"dep:{key}\"");
    let optional = |line: &str| {
        let t = line.trim_start();
        t.strip_prefix(key.as_str()).is_some_and(|rest| rest.trim_start().starts_with('=')) && line.contains("optional = true")
    };
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if optional(line) {
            continue;
        }
        if line.contains(&dep) {
            out.push_str(&line.replace(&format!(", {dep}"), "").replace(&format!("{dep}, "), "").replace(&dep, ""));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// What `--genre` / `--effects` add (all from [`crate::genre::resolve`]).
#[derive(Debug, Default, Clone)]
pub struct Extras {
    /// `[modules] physics` (the genre's).
    pub physics: Option<String>,
    /// The `effects` list; `Some(vec![])` is none.
    pub effects: Option<Vec<String>>,
    /// The text of `kits/<id>/SPEC.md`.
    pub spec: Option<String>,
}

/// Make the game `id` with look `look`. Returns the files written.
pub fn new_game(root: &Path, id: &str, look: &str) -> Result<Vec<String>, String> {
    new_game_with(root, id, look, &Extras::default())
}

/// Set the module list of the new game: its `physics` and `effects` in
/// `evani.toml`, and the matching features (and `default`) in its `Cargo.toml`.
fn apply_modules(root: &Path, app_dir: &str, extras: &Extras) -> Result<(), String> {
    let manifest = root.join(app_dir).join("evani.toml");
    let mut text = std::fs::read_to_string(&manifest).map_err(|e| format!("{app_dir}/evani.toml: {e}"))?;
    if let Some(list) = &extras.effects {
        let items: Vec<String> = list.iter().map(|e| format!("\"{e}\"")).collect();
        let line = format!("effects = [{}]   # each is an evani-fx feature; [] = no fx module", items.join(", "));
        text = crate::genre::replace_line(&text, "effects = [", &line).ok_or_else(|| format!("{app_dir}/evani.toml: no `effects = [..]` line to set"))?;
    }
    if let Some(p) = &extras.physics {
        let line = format!("physics = \"{p}\"   # the genre's physics (catalog/genres.json): \"simple\" | \"rigid\" | \"none\"");
        text = crate::genre::replace_line(&text, "physics = ", &line).ok_or_else(|| format!("{app_dir}/evani.toml: no `physics = ..` line in [modules] to set"))?;
    }
    crate::write_file(&manifest, &text)?;

    // The crate's features: every chosen effect needs one, and `default` mirrors the list.
    let modules = crate::manifest::Modules::parse(&text)?;
    let cargo_path = root.join(app_dir).join("Cargo.toml");
    let cargo = std::fs::read_to_string(&cargo_path).map_err(|e| format!("{app_dir}/Cargo.toml: {e}"))?;
    let nl = if cargo.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = cargo.lines().map(str::to_string).collect();
    // The new default list replaces the old block (from `default = [` to its closing `]`).
    let start = lines.iter().position(|l| l.trim_start().starts_with("default = [")).ok_or_else(|| format!("{app_dir}/Cargo.toml: no `default = [` feature list"))?;
    let end = (start..lines.len()).find(|&i| lines[i].contains(']')).ok_or_else(|| format!("{app_dir}/Cargo.toml: the `default` list is not closed"))?;
    let want: Vec<String> = modules.features().into_iter().filter(|f| f != "fx").collect();
    let default = format!("default = [{}]", want.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", "));
    lines.splice(start..=end, [default]);
    let fx = lines.iter().position(|l| l.trim_start().starts_with("fx = [")).ok_or_else(|| format!("{app_dir}/Cargo.toml: no `fx = [..]` feature"))?;
    let mut at = fx + 1;
    for e in &modules.effects {
        if !lines.iter().any(|l| l.trim_start().starts_with(&format!("{e} = ["))) {
            lines.insert(at, format!("{e} = [\"fx\", \"evani-fx/{e}\"]"));
            at += 1;
        }
    }
    let mut out = lines.join(nl);
    out.push_str(nl);
    std::fs::write(&cargo_path, out).map_err(|e| e.to_string())
}

/// The templates' Cargo licence line, when a template inherits it from the
/// workspace (the engine is MIT).
const ENGINE_LICENSE_FIELD: &str = "license.workspace = true";
/// A new game's own crates (kit and app) say MIT themselves, so the game keeps
/// its licence when it is copied out of the workspace.
const GAME_LICENSE_FIELD: &str = "license = \"MIT\"";

/// The `LICENSE` a new game starts with: the plain MIT licence for the game's
/// own code and art (this folder and its kit). The game's owner may choose
/// other terms.
pub fn game_licence(id: &str) -> String {
    crate::playground::mit_licence(&format!("the authors of the game `{id}`"))
}

/// [`new_game`] with the catalog choices of `--genre` / `--effects`.
pub fn new_game_with(root: &Path, id: &str, look: &str, extras: &Extras) -> Result<Vec<String>, String> {
    if !valid_id(id) {
        return Err(format!("`{id}` is not a game id: use lowercase letters, digits and single `-`, starting with a letter (like `kart-racer`)"));
    }
    for t in [KIT_TEMPLATE, APP_TEMPLATE] {
        if !root.join(t).is_dir() {
            return Err(format!("{t} (the template) is missing"));
        }
    }
    let kit_dir = format!("kits/evani-kit-{id}");
    let app_dir = format!("apps/{id}");
    for d in [&kit_dir, &app_dir] {
        if root.join(d).exists() {
            return Err(format!("{d} already exists"));
        }
    }
    let spec_path = root.join("kits").join(id).join("SPEC.md");
    if extras.spec.is_some() && spec_path.exists() {
        return Err(format!("kits/{id}/SPEC.md already exists"));
    }
    let look_struct = look_struct(root, look).ok_or_else(|| format!("no look crate looks/evani-look-{look} with `impl LookPlugin for ..` (see looks/)"))?;
    let cargo_path = root.join("Cargo.toml");
    let cargo = std::fs::read_to_string(&cargo_path).map_err(|e| e.to_string())?;
    let new_cargo = add_to_workspace(&cargo, id)?;

    let snake = id.replace('-', "_");
    let kit_struct = format!("{}Kit", camel(id));
    let kit_names = move |t: &str| -> String {
        t.replace("evani-kit-example", &format!("evani-kit-{id}"))
            .replace("evani_kit_example", &format!("evani_kit_{snake}"))
            .replace("ExampleKit", &kit_struct)
            .replace("kit.example", &format!("kit.{snake}"))
            .replace("\"example.", &format!("\"{snake}."))
            .replace("\"example\"", &format!("\"{id}\""))
            .replace("kits/example/", &format!("kits/{id}/"))
            .replace("example kit", &format!("{id} kit"))
            // The game's own crates name their MIT licence themselves rather
            // than inherit it from the workspace (docs/LICENSING.md).
            .replace(ENGINE_LICENSE_FIELD, GAME_LICENSE_FIELD)
    };
    let kit_names2 = kit_names.clone();
    let mut written = copy_tree(root, KIT_TEMPLATE, &kit_dir, &["README.md", "CHANGES.md"], &kit_names)?;
    let readme = format!(
        "# evani-kit-{id}\n\nThe gameplay of the game `{id}` (`apps/{id}`), started from `evani-kit-example` by `cargo xtask new-game`. Replace the example spinners with this game's systems; keep the conventions in kits/AGENTS.md and docs/KIT_PROCESS.md.\n"
    );
    crate::write_file(&root.join(&kit_dir).join("README.md"), &readme)?;
    written.push(format!("{kit_dir}/README.md"));

    let look_snake = look.replace('-', "_");
    let app_names = move |t: &str| -> String {
        let mut s = kit_names2(t).replace("kit = \"example\"", &format!("kit = \"{id}\""));
        if look != "basic" {
            // `BasicLook` is a unit struct; another look may carry settings
            // (photoreal), so the game builds it with `Default` (every look has it).
            s = drop_basic_look_asserts(&s)
                .replace("with_look(BasicLook)", &format!("with_look({look_struct}::default())"))
                .replace("evani-look-basic", &format!("evani-look-{look}"))
                .replace("evani_look_basic", &format!("evani_look_{look_snake}"))
                .replace("BasicLook", &look_struct)
                .replace("look = \"basic\"", &format!("look = \"{look}\""));
        }
        s.replace("evani-sandbox", &format!("evani-{id}")).replace("sandbox", id)
    };
    // The template's own guidance files are not copied: the new game gets fresh ones below.
    written.extend(copy_tree(root, APP_TEMPLATE, &app_dir, &["AGENTS.md", "CLAUDE.md"], &app_names)?);
    if look != "basic" {
        let app_cargo = root.join(&app_dir).join("Cargo.toml");
        let text = std::fs::read_to_string(&app_cargo).map_err(|e| format!("{app_dir}/Cargo.toml: {e}"))?;
        let folded = fold_chosen_look(&text, look);
        if folded != text {
            crate::write_file(&app_cargo, &folded)?;
        }
    }
    // New games start as playgrounds that support free roam (the example
    // kit does, with its test; docs/PLAYGROUNDS.md).
    let manifest = root.join(&app_dir).join("evani.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{app_dir}/evani.toml: {e}"))?;
    if crate::playground::parse(&text)?.is_none() {
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        crate::write_file(&manifest, &format!("{text}{sep}{PLAYGROUND}"))?;
    }

    let licence_path = format!("{app_dir}/LICENSE");
    crate::write_file(&root.join(&licence_path), &game_licence(id))?;
    written.push(licence_path);

    if extras.physics.is_some() || extras.effects.is_some() {
        apply_modules(root, &app_dir, extras)?;
    }
    if let Some(spec) = &extras.spec {
        crate::write_file(&spec_path, spec)?;
        written.push(format!("kits/{id}/SPEC.md"));
    }

    std::fs::write(&cargo_path, new_cargo).map_err(|e| e.to_string())?;
    written.push("Cargo.toml".into());
    for t in targets::targets(root, targets::Options::default()) {
        if t.rel.starts_with(&format!("{app_dir}/")) && t.write(root)? {
            written.push(t.rel.clone());
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names() {
        assert!(valid_id("kart-racer") && valid_id("snake2"));
        assert!(!valid_id("2d-x") && !valid_id("Kart") && !valid_id("a--b") && !valid_id("a-") && !valid_id(""));
        assert_eq!(camel("kart-racer"), "KartRacer");
    }

    #[test]
    fn workspace_lists_get_the_new_crates() {
        let t = "[workspace]\nmembers = [\n    \"kits/evani-kit-example\",\n    \"apps/sandbox\",\n    \"tools/xtask\",\n]\n\n[workspace.dependencies]\nevani-kit-example = { path = \"kits/evani-kit-example\" }\nevani-look-basic = { path = \"looks/evani-look-basic\" }\n";
        let s = add_to_workspace(t, "snake").unwrap();
        assert!(s.contains("    \"kits/evani-kit-example\",\n    \"kits/evani-kit-snake\",\n    \"apps/sandbox\",\n    \"apps/snake\",\n    \"tools/xtask\","), "{s}");
        assert!(s.contains("evani-kit-example = { path = \"kits/evani-kit-example\" }\nevani-kit-snake = { path = \"kits/evani-kit-snake\" }\n"), "{s}");
    }

    #[test]
    fn makes_a_game_from_the_templates() {
        let r = crate::testutil::fake_engine("scaffold");
        let w = |p: &str, s: &str| crate::write_file(&r.join(p), s).unwrap();
        w("Cargo.toml", "[workspace]\nmembers = [\n    \"kits/evani-kit-example\",\n    \"apps/sandbox\",\n]\n[workspace.dependencies]\nevani-kit-example = { path = \"kits/evani-kit-example\" }\n");
        w("kits/evani-kit-example/Cargo.toml", "[package]\nname = \"evani-kit-example\"\nlicense.workspace = true\n[dependencies]\nevani-render = { workspace = true, features = [\"forward\"] }\n");
        w("kits/evani-kit-example/CHANGES.md", "bake notes\n");
        w(
            "kits/evani-kit-example/src/lib.rs",
            "pub struct ExampleKitPlugin;\nfn name() -> &'static str { \"example\" }\nconst P: &str = \"kit.example.spin_speed\";\nconst S: &str = \"example.main\";\n",
        );
        w("apps/sandbox/Cargo.toml", "[package]\nname = \"evani-sandbox\"\nlicense.workspace = true\n[dependencies]\nevani-kit-example.workspace = true\nevani-look-basic.workspace = true\n");
        w("apps/sandbox/evani.toml", "name = \"sandbox\"\nkit = \"example\"\nlook = \"basic\"\nmodels = true # proof-model:line\n");
        w("apps/sandbox/src/main.rs", "use evani_kit_example::ExampleKitPlugin;\nuse evani_look_basic::BasicLook;\nmod panel; #[cfg(feature = \"ui\")] mod textpanel;\nmod proof_model; // proof-model:line\nfn main() { App::new(\"sandbox\"); }\n// proof-model:begin\nfn proof() {}\n// proof-model:end\n");
        w("apps/sandbox/src/proof_model.rs", "pub fn proof() {}\n");
        w("apps/sandbox/src/textpanel.rs", "pub fn system() {}\n");
        w("apps/sandbox/assets/models/m.glb", "glTF");
        w("apps/sandbox/assets/other.txt", "kept\n");
        w("apps/sandbox/dist/junk.wasm", "x");
        w("apps/sandbox/AGENTS.md", "Private notes about the sandbox.\n");
        w("looks/evani-look-toon/src/lib.rs", "pub struct ToonLook;\nimpl LookPlugin for ToonLook {}\n");

        let out = new_game(&r, "space-snake", "toon").unwrap();
        let read = |p: &str| std::fs::read_to_string(r.join(p)).unwrap();
        let lib = read("kits/evani-kit-space-snake/src/lib.rs");
        assert!(lib.contains("SpaceSnakeKitPlugin") && lib.contains("\"space-snake\"") && lib.contains("kit.space_snake.spin_speed") && lib.contains("\"space_snake.main\""), "{lib}");
        // The template kit draws 3D, so the new kit keeps the 3D path on.
        let kit_cargo = read("kits/evani-kit-space-snake/Cargo.toml");
        assert!(kit_cargo.contains("name = \"evani-kit-space-snake\"") && kit_cargo.contains("evani-render = { workspace = true, features = [\"forward\"] }"), "{kit_cargo}");
        assert!(!r.join("kits/evani-kit-space-snake/CHANGES.md").exists());
        assert!(read("kits/evani-kit-space-snake/README.md").contains("`space-snake`"));
        let manifest = read("apps/space-snake/evani.toml");
        assert_eq!(manifest, format!("name = \"space-snake\"\nkit = \"space-snake\"\nlook = \"toon\"\n{PLAYGROUND}"));
        assert_eq!(crate::playground::parse(&manifest).unwrap(), Some(crate::playground::Playground { free_roam: true }));
        let main = read("apps/space-snake/src/main.rs");
        assert!(main.contains("use evani_kit_space_snake::SpaceSnakeKitPlugin;") && main.contains("use evani_look_toon::ToonLook;") && main.contains("App::new(\"space-snake\")"), "{main}");
        // The sandbox's model proof stays in the sandbox.
        assert!(!main.contains("proof") && !manifest.contains("models"), "{main}\n{manifest}");
        assert!(!r.join("apps/space-snake/src/proof_model.rs").exists() && !r.join("apps/space-snake/assets/models").exists());
        // So does its MSDF text panel (S3-13a); the line it shared stays.
        assert!(main.contains("mod panel;\n") && !main.contains("textpanel") && !r.join("apps/space-snake/src/textpanel.rs").exists(), "{main}");
        assert_eq!(read("apps/space-snake/assets/other.txt"), "kept\n");
        assert!(read("apps/space-snake/Cargo.toml").contains("name = \"evani-space-snake\""));
        // The game's own crates name MIT themselves, and its LICENSE is the plain MIT text.
        for c in ["kits/evani-kit-space-snake/Cargo.toml", "apps/space-snake/Cargo.toml"] {
            let t = read(c);
            assert!(t.contains("license = \"MIT\"") && !t.contains("license.workspace"), "{c}: {t}");
        }
        let lic = read("apps/space-snake/LICENSE");
        assert_eq!(lic, crate::playground::mit_licence("the authors of the game `space-snake`"));
        assert!(lic.starts_with("MIT License\n\nCopyright (c) the authors of the game `space-snake`\n") && lic.trim_end().ends_with("SOFTWARE."), "{lic}");
        assert!(out.iter().any(|f| f == "apps/space-snake/LICENSE"), "{out:?}");
        assert!(!r.join("apps/space-snake/dist").exists());
        let ws = read("Cargo.toml");
        assert!(ws.contains("\"kits/evani-kit-space-snake\",") && ws.contains("\"apps/space-snake\",") && ws.contains("evani-kit-space-snake = { path"));
        let agents = read("apps/space-snake/AGENTS.md");
        assert!(agents.contains("apps/space-snake/evani.toml") && !agents.contains("Private notes"), "{agents}");
        assert!(out.contains(&"apps/space-snake/CLAUDE.md".to_string()));
        assert_eq!(out.iter().filter(|f| f.ends_with("/AGENTS.md")).count(), 1, "{out:?}");

        assert!(new_game(&r, "space-snake", "toon").unwrap_err().contains("already exists"));
        assert!(new_game(&r, "other", "nope").unwrap_err().contains("evani-look-nope"));
    }

    #[test]
    fn the_chosen_look_is_not_listed_twice() {
        let t = "[features]\ntoon = [\"dep:evani-look-toon\"]\nrelief = [\"models\", \"toon\", \"evani-look-toon/relief\"]\nlit = [\"models\", \"dep:evani-look-toon\", \"evani-look-toon/film\"]\nend = [\"x\", \"dep:evani-look-toon\"]\n\n[dependencies]\nevani-look-toon.workspace = true\nevani-look-toonish = { workspace = true, optional = true }\nevani-look-toon = { workspace = true, optional = true }\n";
        let s = fold_chosen_look(t, "toon");
        assert_eq!(
            s,
            "[features]\ntoon = []\nrelief = [\"models\", \"toon\", \"evani-look-toon/relief\"]\nlit = [\"models\", \"evani-look-toon/film\"]\nend = [\"x\"]\n\n[dependencies]\nevani-look-toon.workspace = true\nevani-look-toonish = { workspace = true, optional = true }\n"
        );
        // Another look's optional line is untouched.
        assert_eq!(fold_chosen_look(t, "photoreal"), t);
    }

    /// The real templates (`apps/sandbox`, `kits/evani-kit-example`) with
    /// every look the engine ships: the new game's `Cargo.toml` parses (no
    /// duplicate keys), depends on its look as a plain dependency, and every
    /// `dep:` entry names an optional dependency.
    #[test]
    fn every_shipped_look_makes_a_valid_cargo_toml() {
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut looks: Vec<String> = std::fs::read_dir(engine.join("looks"))
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.strip_prefix("evani-look-").map(str::to_string))
            .collect();
        looks.sort();
        assert!(looks.len() >= 3 && looks.iter().any(|l| l == "stylizedpbr"), "{looks:?}");
        for look in &looks {
            let r = crate::testutil::fake_engine(&format!("scaffold-look-{look}"));
            let copy = |p: &str| crate::write_file(&r.join(p), &std::fs::read_to_string(engine.join(p)).unwrap()).unwrap();
            crate::write_file(&r.join("Cargo.toml"), "[workspace]\nmembers = [\n    \"kits/evani-kit-example\",\n    \"apps/sandbox\",\n]\n[workspace.dependencies]\nevani-kit-example = { path = \"kits/evani-kit-example\" }\n").unwrap();
            for p in ["apps/sandbox/Cargo.toml", "apps/sandbox/evani.toml", "apps/sandbox/src/main.rs", "kits/evani-kit-example/Cargo.toml", "kits/evani-kit-example/src/lib.rs"] {
                copy(p);
            }
            for l in &looks {
                let mut files = Vec::new();
                list_files(&engine.join("looks").join(format!("evani-look-{l}")).join("src"), &mut files);
                for f in files {
                    copy(&crate::rel(&engine, &f));
                }
            }
            new_game(&r, "new-one", look).unwrap();
            let text = std::fs::read_to_string(r.join("apps/new-one/Cargo.toml")).unwrap();
            let cargo: toml::Table = text.parse().unwrap_or_else(|e| panic!("look {look}: {e}\n{text}"));
            let deps = cargo["dependencies"].as_table().unwrap();
            let own = deps.get(&format!("evani-look-{look}")).unwrap_or_else(|| panic!("look {look}: no evani-look-{look} dependency"));
            assert!(own.get("optional").is_none(), "look {look}: its own look is optional: {own}");
            let optional = |name: &str| deps.get(name).and_then(|d| d.get("optional")).and_then(toml::Value::as_bool) == Some(true);
            for (feature, list) in cargo["features"].as_table().unwrap() {
                for entry in list.as_array().unwrap().iter().filter_map(toml::Value::as_str) {
                    if let Some(dep) = entry.strip_prefix("dep:") {
                        assert!(optional(dep), "look {look}: feature `{feature}` turns on `{dep}`, which is not an optional dependency");
                    }
                }
            }
            let main = std::fs::read_to_string(r.join("apps/new-one/src/main.rs")).unwrap();
            // The struct the game imports is the look's main struct, at the
            // crate root (checked against the real lib.rs, not through
            // `look_struct`): `pub struct X` there, and `impl LookPlugin for X`.
            let prefix = format!("use evani_look_{}::", look.replace('-', "_"));
            let line = main.lines().find(|l| l.trim_start().starts_with(&prefix)).unwrap_or_else(|| panic!("look {look}: no `{prefix}..` in main.rs"));
            let name = line.trim_start()[prefix.len()..].trim_end_matches(';').trim();
            let lib = std::fs::read_to_string(engine.join("looks").join(format!("evani-look-{look}")).join("src/lib.rs")).unwrap();
            let defined = lib.lines().any(|l| l.strip_prefix("pub struct ").is_some_and(|r| r.starts_with(name) && !r[name.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')));
            let reexported = lib.lines().any(|l| l.starts_with("pub use ") && l.contains(name));
            assert!(defined || reexported, "look {look}: the game imports `{name}`, which lib.rs neither defines nor re-exports at the root");
            assert!(lib.contains(&format!("impl LookPlugin for {name}")) || reexported, "look {look}: `{name}` is not the look's LookPlugin");
        }
    }

    #[test]
    fn the_look_struct_is_the_one_at_the_crate_root() {
        let r = crate::testutil::fake_engine("scaffold-look-struct");
        let w = |p: &str, s: &str| crate::write_file(&r.join(p), s).unwrap();
        // A side look in a sub-module (sorted first) also implements LookPlugin.
        w("looks/evani-look-two/src/a_side.rs", "pub struct SideLook;\nimpl LookPlugin for SideLook {}\n");
        w("looks/evani-look-two/src/lib.rs", "pub mod a_side;\npub struct MainLook {\n    x: u8,\n}\nimpl LookPlugin for MainLook {}\nmod tests {\n    pub struct Inner;\n}\n");
        assert_eq!(look_struct(&r, "two").as_deref(), Some("MainLook"));
        // The main look defined in a sub-module and re-exported at the root.
        w("looks/evani-look-re/src/lib.rs", "mod inner;\npub mod side;\npub use inner::{Settings, ReLook};\n");
        w("looks/evani-look-re/src/inner.rs", "pub struct ReLook;\nimpl LookPlugin for ReLook {}\n");
        w("looks/evani-look-re/src/side.rs", "pub struct Other;\nimpl LookPlugin for Other {}\n");
        assert_eq!(look_struct(&r, "re").as_deref(), Some("ReLook"));
        // Only side looks: none (new-game then says so).
        w("looks/evani-look-side/src/lib.rs", "pub mod a;\n");
        w("looks/evani-look-side/src/a.rs", "pub struct A;\nimpl LookPlugin for A {}\n");
        assert_eq!(look_struct(&r, "side"), None);
        assert_eq!(root_types("pub use a::b::C;\npub use d::{E, F as G};\npub enum H {}\n    pub struct Nested;\n"), ["C", "E", "G", "H"]);
    }

    /// A new game for every look the engine ships, in a copy of the real
    /// templates with the engine's own workspace (dependency paths pointing
    /// into this engine), passes `cargo check`: the look import resolves and
    /// every feature and dependency is valid. Seven native checks (minutes):
    /// `cargo test -p evani-guide --lib -- --ignored every_look_new_game_compiles`
    /// (`CARGO_TARGET_DIR` is used for the checks when set).
    #[test]
    #[ignore = "cargo check of seven generated games (minutes); run by hand"]
    fn every_look_new_game_compiles() {
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf();
        let engine_fwd = engine.to_string_lossy().replace('\\', "/");
        let mut looks: Vec<String> = std::fs::read_dir(engine.join("looks"))
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.strip_prefix("evani-look-").map(str::to_string))
            .collect();
        looks.sort();
        let target = std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()).map_or_else(|| engine.join("target"), std::path::PathBuf::from);
        let mut failed = Vec::new();
        for look in &looks {
            let r = crate::testutil::temp_root(&format!("scaffold-check-{look}"));
            // The engine's workspace file: members are the two templates; every
            // other path dependency points into this engine.
            let mut ws = String::new();
            let mut in_members = false;
            for line in std::fs::read_to_string(engine.join("Cargo.toml")).unwrap().lines() {
                if in_members {
                    in_members = line.trim() != "]";
                    continue;
                }
                if line.starts_with("members = [") {
                    ws.push_str("members = [\n    \"kits/evani-kit-example\",\n    \"apps/sandbox\",\n]\n");
                    in_members = !line.trim_end().ends_with(']');
                    continue;
                }
                if line.starts_with("exclude = ") || line.starts_with("default-members = ") {
                    continue;
                }
                if line.contains("path = \"") && !line.contains("path = \"kits/evani-kit-example\"") {
                    ws.push_str(&line.replace("path = \"", &format!("path = \"{engine_fwd}/")));
                } else {
                    ws.push_str(line);
                }
                ws.push('\n');
            }
            crate::write_file(&r.join("Cargo.toml"), &ws).unwrap();
            let _ = std::fs::copy(engine.join("Cargo.lock"), r.join("Cargo.lock"));
            // The kit template's build script is shared (`build = "../../tools/bake-index.rs"`).
            crate::write_file(&r.join("tools/bake-index.rs"), &std::fs::read_to_string(engine.join("tools/bake-index.rs")).unwrap()).unwrap();
            let mut dirs: Vec<String> = vec!["apps/sandbox".into(), "kits/evani-kit-example".into(), "tests/fixtures/models".into()];
            dirs.extend(looks.iter().map(|l| format!("looks/evani-look-{l}/src")));
            for d in dirs {
                let mut files = Vec::new();
                list_files(&engine.join(&d), &mut files);
                for f in files {
                    let dest = r.join(crate::rel(&engine, &f));
                    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    std::fs::copy(&f, &dest).unwrap();
                }
            }
            new_game(&r, "new-one", look).unwrap_or_else(|e| panic!("look {look}: {e}"));
            let out = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args(["check", "--offline", "-p", "evani-new-one", "--manifest-path"])
                .arg(r.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .unwrap();
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                let tail: Vec<&str> = err.lines().filter(|l| l.starts_with("error") || l.trim_start().starts_with("-->")).take(12).collect();
                failed.push(format!("look {look}:\n{}", tail.join("\n")));
            } else {
                let _ = std::fs::remove_dir_all(&r);
            }
        }
        assert!(failed.is_empty(), "{} of {} looks don't compile:\n{}", failed.len(), looks.len(), failed.join("\n\n"));
    }
}
