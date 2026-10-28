//! Shader-slot splicing: a pass's WGSL template marks where slot functions
//! go with `//@slot <name>` lines; [`splice`] replaces each marker with the
//! assembled `fn slot_<name>(...) -> ... { body }` and keeps a line table so
//! compiler messages (1-based lines in the spliced source) map back to the
//! line of the slot body the author wrote.

use evani_core::slots::{SLOT_HEADER_LINES, ShaderSlots, SlotId, SlotMessage};

/// The marker prefix, on a line of its own.
pub const MARKER: &str = "//@slot ";

/// Which body each slot gets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bodies<'a> {
    /// The current bodies in [`ShaderSlots`].
    Current,
    /// Current bodies, but this one slot gets a candidate body (live rebuild).
    Override(SlotId, &'a str),
    /// The engine's built-in default bodies (fallback when a look's body
    /// does not compile).
    EngineDefaults,
}

/// Where one slot function landed in the spliced source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSpan {
    pub slot: SlotId,
    pub name: &'static str,
    /// 1-based line of the first body line in the spliced source.
    pub body_first_line: u32,
    pub body_lines: u32,
    /// The body text that was spliced in (for `source_line`).
    pub body: String,
}

/// A spliced shader source plus its line table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spliced {
    pub source: String,
    pub spans: Vec<SlotSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpliceError {
    /// A marker names a slot that isn't declared.
    UnknownSlot { line: u32, name: String },
}

impl core::fmt::Display for SpliceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpliceError::UnknownSlot { line, name } => {
                write!(f, "line {line}: unknown shader slot '{name}'")
            }
        }
    }
}

/// The slot named by a marker line, if `line` is one.
pub fn marker_name(line: &str) -> Option<&str> {
    line.trim_start()
        .strip_prefix(MARKER)
        .map(str::trim)
        .filter(|n| !n.is_empty())
}

/// Slot names a template uses, in marker order.
pub fn slot_names(template: &str) -> impl Iterator<Item = &str> {
    template.lines().filter_map(marker_name)
}

/// Does `template` call slot `name`?
pub fn uses_slot(template: &str, name: &str) -> bool {
    slot_names(template).any(|n| n == name)
}

/// Replace every `//@slot <name>` line of `template` with the slot function.
pub fn splice(template: &str, slots: &ShaderSlots, bodies: Bodies) -> Result<Spliced, SpliceError> {
    let mut source = String::with_capacity(template.len() + 1024);
    let mut spans = Vec::new();
    let mut out_line: u32 = 0; // lines written so far
    for (i, line) in template.lines().enumerate() {
        let Some(name) = marker_name(line) else {
            source.push_str(line);
            source.push('\n');
            out_line += 1;
            continue;
        };
        let (id, def) = slots
            .id(name)
            .and_then(|id| slots.get(id).map(|d| (id, d)))
            .ok_or_else(|| SpliceError::UnknownSlot {
                line: i as u32 + 1,
                name: name.to_string(),
            })?;
        let body: &str = match bodies {
            Bodies::Override(o, b) if o == id => b,
            Bodies::EngineDefaults => def.signature.default_body,
            _ => &def.body,
        };
        let f = ShaderSlots::assemble(&def.signature, body);
        let body_lines = body.lines().count().max(1) as u32;
        spans.push(SlotSpan {
            slot: id,
            name: def.signature.name,
            body_first_line: out_line + SLOT_HEADER_LINES as u32 + 1,
            body_lines,
            body: body.to_string(),
        });
        out_line += f.lines().count() as u32;
        source.push_str(&f);
    }
    Ok(Spliced { source, spans })
}

impl Spliced {
    /// Map a 1-based line of the spliced source to `(slot, 1-based body
    /// line)` when it falls inside a slot body.
    pub fn map_line(&self, line: u32) -> Option<(&SlotSpan, u32)> {
        self.spans.iter().find_map(|s| {
            (line >= s.body_first_line && line < s.body_first_line + s.body_lines)
                .then(|| (s, line - s.body_first_line + 1))
        })
    }

    /// A compiler message mapped to slot `focus`'s body: line/column there
    /// and the body line's text, or line 0 when it points elsewhere (then
    /// the text is prefixed with where it points).
    pub fn message(
        &self,
        focus: Option<SlotId>,
        severity: &'static str,
        text: &str,
        line: u32,
        column: u32,
        length: u32,
    ) -> SlotMessage {
        match self.map_line(line) {
            Some((span, body_line)) if focus.is_none_or(|f| f == span.slot) => SlotMessage {
                severity,
                text: text.to_string(),
                line: body_line,
                column,
                length,
                source_line: span
                    .body
                    .lines()
                    .nth(body_line as usize - 1)
                    .map(str::to_string),
            },
            Some((span, body_line)) => SlotMessage {
                severity,
                text: format!("slot {} line {body_line}: {text}", span.name),
                line: 0,
                column: 0,
                length: 0,
                source_line: None,
            },
            None => SlotMessage {
                severity,
                text: if line > 0 {
                    format!("shader line {line}: {text}")
                } else {
                    text.to_string()
                },
                line: 0,
                column: 0,
                length: 0,
                source_line: self
                    .source
                    .lines()
                    .nth((line as usize).saturating_sub(1))
                    .filter(|_| line > 0)
                    .map(str::to_string),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "// evani.shader.t\nstruct S { x: f32 };\n//@slot tonemap\n  //@slot exposure\n@fragment\nfn fs_main() -> @location(0) vec4<f32> {\n    return vec4<f32>(slot_tonemap(vec3<f32>(slot_exposure(0.18))), 1.0);\n}\n";

    #[test]
    fn splices_markers_and_maps_lines() {
        let slots = ShaderSlots::with_builtins();
        let tm = slots.id("tonemap").unwrap();
        let body = "    let x = color;\n    return x +;";
        let s = splice(T, &slots, Bodies::Override(tm, body)).unwrap();
        assert!(!s.source.contains("//@slot"));
        assert_eq!(slot_names(T).collect::<Vec<_>>(), ["tonemap", "exposure"]);
        assert!(uses_slot(T, "exposure") && !uses_slot(T, "fog"));
        let lines: Vec<&str> = s.source.lines().collect();
        // Find the bad body line in the spliced source and map it back.
        let bad = lines.iter().position(|l| l.contains("x +;")).unwrap() as u32 + 1;
        let (span, body_line) = s.map_line(bad).unwrap();
        assert_eq!(span.name, "tonemap");
        assert_eq!(body_line, 2);
        let m = s.message(Some(tm), "error", "expected expression", bad, 14, 1);
        assert_eq!((m.line, m.column), (2, 14));
        assert_eq!(m.source_line.as_deref(), Some("    return x +;"));
        // The exposure function follows on the right lines.
        let exp = s.spans[1].body_first_line as usize;
        assert!(lines[exp - 1].contains("return 1.0;"), "{}", lines[exp - 1]);
        assert!(lines[exp - 2].starts_with("fn slot_exposure("));
        // Outside any body.
        let m = s.message(Some(tm), "error", "oops", 2, 1, 1);
        assert_eq!(m.line, 0);
        assert!(m.text.starts_with("shader line 2"));
        // In another slot's body.
        let m = s.message(Some(tm), "error", "oops", exp as u32, 1, 1);
        assert!(m.text.starts_with("slot exposure line 1"), "{}", m.text);
    }

    #[test]
    fn engine_defaults_and_unknown_slots() {
        let mut slots = ShaderSlots::with_builtins();
        slots.set_default("tonemap", "return color;", "look.t").unwrap();
        let cur = splice(T, &slots, Bodies::Current).unwrap();
        assert!(cur.source.contains("return color;"));
        let def = splice(T, &slots, Bodies::EngineDefaults).unwrap();
        assert!(def.source.contains("ACES"));
        assert_eq!(
            splice("//@slot nope\n", &slots, Bodies::Current),
            Err(SpliceError::UnknownSlot {
                line: 1,
                name: "nope".into()
            })
        );
    }
}
