//! Which Evani.Dev a game was built from: the engine version and git commit,
//! and the version of every Evani.Dev part compiled into this game (core,
//! each engine module that is present, the kit and the look).
//!
//! Everything is baked at build time: each part's version is the
//! `CARGO_PKG_VERSION` of its own crate, registered when the part is added
//! to the [`crate::App`] (see [`crate::plugin::EngineModule::version`],
//! [`crate::plugin::KitPlugin::version`] and
//! [`crate::plugin::LookPlugin::version`]), so a part that is not compiled
//! in can never be listed. The list lives in [`crate::plugin::BuildInfo`]
//! `parts`; the overlay's first line and evani-live's `session.status`
//! (`versions`) show it.

use core::fmt::Write;

/// The engine (workspace) version: evani-core's crate version.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The engine's git commit (short hash) at build time, or `unknown` when
/// it was built without git (see evani-core's `build.rs`).
pub const ENGINE_COMMIT: &str = env!("EVANI_GIT_COMMIT");

/// What kind of part a [`PartVersion`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartKind {
    Core,
    Module,
    Kit,
    Look,
}

impl PartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Module => "module",
            Self::Kit => "kit",
            Self::Look => "look",
        }
    }
}

/// One Evani.Dev part compiled into this game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartVersion {
    pub kind: PartKind,
    /// `core`, the module's label (`render`, `fx`...), or the kit's or
    /// look's name (`kart-racer`, `toon`).
    pub name: &'static str,
    /// The part's crate version; `None` for a kit or look that does not
    /// report one.
    pub version: Option<&'static str>,
    /// What the part compiled in, when it varies per game (fx: its effects).
    pub features: Vec<&'static str>,
    /// The wire protocol the part speaks (live: `evani-live/1`).
    pub protocol: Option<&'static str>,
}

impl PartVersion {
    /// evani-core itself (always present).
    pub fn core() -> Self {
        Self {
            kind: PartKind::Core,
            name: "core",
            version: Some(ENGINE_VERSION),
            features: Vec::new(),
            protocol: None,
        }
    }

    pub fn new(kind: PartKind, name: &'static str, version: Option<&'static str>) -> Self {
        Self {
            kind,
            name,
            version,
            features: Vec::new(),
            protocol: None,
        }
    }
}

/// Insert `part` after the parts of its kind or an earlier one, so the list
/// reads core, modules (in the order they were added), kit, look. A kit or
/// look replaces the previous one (a game has one of each).
pub fn insert_part(parts: &mut Vec<PartVersion>, part: PartVersion) {
    if matches!(part.kind, PartKind::Kit | PartKind::Look) {
        parts.retain(|p| p.kind != part.kind);
    }
    let at = parts.iter().rposition(|p| p.kind <= part.kind).map_or(0, |i| i + 1);
    parts.insert(at, part);
}

/// The versions line, e.g. `evani 0.1.0 @03979fc · core 0.1.0 · render
/// 0.1.0 · fx 0.1.0 [bloom outline] · live 0.1.0 (evani-live/1) · kit
/// kart-racer 0.1.0 · look toon 0.1.0` (no newline). Allocation-free once
/// `out` has grown.
pub fn write_versions(out: &mut String, parts: &[PartVersion]) {
    let _ = write!(out, "evani {ENGINE_VERSION} @{ENGINE_COMMIT}");
    for p in parts {
        out.push_str(" · ");
        match p.kind {
            PartKind::Kit | PartKind::Look => {
                out.push_str(p.kind.as_str());
                out.push(' ');
            }
            PartKind::Core | PartKind::Module => {}
        }
        out.push_str(p.name);
        if let Some(v) = p.version {
            out.push(' ');
            out.push_str(v);
        }
        if !p.features.is_empty() {
            out.push_str(" [");
            for (i, f) in p.features.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(f);
            }
            out.push(']');
        }
        if let Some(proto) = p.protocol {
            let _ = write!(out, " ({proto})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_is_baked() {
        assert!(!ENGINE_COMMIT.is_empty());
        assert!(
            ENGINE_COMMIT == "unknown" || ENGINE_COMMIT.chars().all(|c| c.is_ascii_hexdigit()),
            "{ENGINE_COMMIT}"
        );
    }

    #[test]
    fn line_lists_parts_in_kind_order() {
        let mut parts = vec![PartVersion::core()];
        insert_part(&mut parts, PartVersion::new(PartKind::Look, "toon", Some("0.1.0")));
        insert_part(&mut parts, PartVersion::new(PartKind::Kit, "kart-racer", Some("0.2.0")));
        insert_part(&mut parts, PartVersion::new(PartKind::Module, "render", Some("0.1.0")));
        let mut fx = PartVersion::new(PartKind::Module, "fx", Some("0.1.0"));
        fx.features = vec!["bloom", "outline"];
        insert_part(&mut parts, fx);
        let mut live = PartVersion::new(PartKind::Module, "live", Some("0.1.0"));
        live.protocol = Some("evani-live/1");
        insert_part(&mut parts, live);
        // A second kit replaces the first.
        insert_part(&mut parts, PartVersion::new(PartKind::Kit, "kart-racer", Some("0.1.0")));
        let mut out = String::new();
        write_versions(&mut out, &parts);
        assert_eq!(
            out,
            format!(
                "evani {ENGINE_VERSION} @{ENGINE_COMMIT} · core {ENGINE_VERSION} · render 0.1.0 · \
                 fx 0.1.0 [bloom outline] · live 0.1.0 (evani-live/1) · kit kart-racer 0.1.0 · \
                 look toon 0.1.0"
            )
        );
    }
}
