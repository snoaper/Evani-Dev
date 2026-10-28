//! Shader slots: WGSL functions with fixed signatures whose bodies can be
//! replaced (by a look at build time, or live at runtime).
//!
//! The render graph calls slots such as `slot_tonemap(color)`. This module
//! owns the signatures and current bodies; evani-render splices
//! [`ShaderSlots::wgsl_function`] into its pass shaders, validates with GPU
//! error scopes, builds the new pipeline and swaps it at a frame boundary,
//! keeping the old one on failure. Core only runs cheap structural checks.

use std::collections::BTreeMap;

/// A fixed slot signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSignature {
    /// Slot id; the WGSL function is `slot_<name>`.
    pub name: &'static str,
    /// `(param name, WGSL type)`.
    pub params: &'static [(&'static str, &'static str)],
    /// WGSL return type.
    pub ret: &'static str,
    pub description: &'static str,
    /// Engine default body (statements only, without the outer braces).
    pub default_body: &'static str,
}

pub const TONEMAP: SlotSignature = SlotSignature {
    name: "tonemap",
    params: &[("color", "vec3<f32>")],
    ret: "vec3<f32>",
    description: "Map HDR linear colour to display range",
    default_body: "    // ACES filmic fit (Narkowicz 2015)\n    let a = 2.51;\n    let b = 0.03;\n    let c = 2.43;\n    let d = 0.59;\n    let e = 0.14;\n    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));",
};

pub const BLOOM_WEIGHT: SlotSignature = SlotSignature {
    name: "bloom_weight",
    params: &[("mip", "u32")],
    ret: "f32",
    description: "Weight of each bloom mip level when upsampling (mip 0 = half res)",
    default_body: "    return 1.0;",
};

pub const TOON_RAMP: SlotSignature = SlotSignature {
    name: "toon_ramp",
    params: &[("n_dot_l", "f32")],
    ret: "f32",
    description: "Diffuse light ramp from N.L (identity = smooth Lambert)",
    default_body: "    return max(n_dot_l, 0.0);",
};

pub const FOG: SlotSignature = SlotSignature {
    name: "fog",
    params: &[("distance", "f32"), ("height", "f32")],
    ret: "f32",
    description: "Fog amount 0..1 from view distance and world height",
    default_body: "    return 0.0;",
};

pub const EXPOSURE: SlotSignature = SlotSignature {
    name: "exposure",
    params: &[("luminance", "f32")],
    ret: "f32",
    description: "Exposure multiplier from average scene luminance",
    default_body: "    return 1.0;",
};

pub const RIM: SlotSignature = SlotSignature {
    name: "rim",
    params: &[("n_dot_v", "f32"), ("n_dot_l", "f32"), ("distance", "f32"), ("base", "vec3<f32>")],
    ret: "vec3<f32>",
    description: "Rim light added to lit surfaces from N.V (towards the camera), N.L, view distance and the base colour (zero = none)",
    default_body: "    return vec3<f32>(0.0);",
};

/// The engine's built-in slots.
pub const BUILTIN_SLOTS: [SlotSignature; 6] = [TONEMAP, BLOOM_WEIGHT, TOON_RAMP, FOG, EXPOSURE, RIM];

/// Maximum body size accepted.
pub const MAX_BODY_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    Unknown(String),
    TooLong,
    Empty,
    /// The body would close the function early or leave it open.
    Unbalanced {
        line: usize,
        message: String,
    },
}

impl core::fmt::Display for SlotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SlotError::Unknown(n) => write!(f, "unknown slot '{n}'"),
            SlotError::TooLong => write!(f, "slot body longer than {MAX_BODY_LEN} bytes"),
            SlotError::Empty => write!(f, "slot body is empty"),
            SlotError::Unbalanced { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

/// Structural check: brackets balance and never close the enclosing
/// function (comments are skipped). Real WGSL validation happens on the GPU.
pub fn check_body(body: &str) -> Result<(), SlotError> {
    if body.len() > MAX_BODY_LEN {
        return Err(SlotError::TooLong);
    }
    if body.trim().is_empty() {
        return Err(SlotError::Empty);
    }
    let b = body.as_bytes();
    let mut stack: Vec<u8> = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let mut block_comments = 0usize;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == b'\n' {
            line += 1;
        }
        if block_comments > 0 {
            if c == b'*' && next == Some(b'/') {
                block_comments -= 1;
                i += 2;
                continue;
            }
            if c == b'/' && next == Some(b'*') {
                block_comments += 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if c == b'/' && next == Some(b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && next == Some(b'*') {
            block_comments += 1;
            i += 2;
            continue;
        }
        match c {
            b'{' | b'(' | b'[' => stack.push(c),
            b'}' | b')' | b']' => {
                let open = match c {
                    b'}' => b'{',
                    b')' => b'(',
                    _ => b'[',
                };
                if stack.pop() != Some(open) {
                    return Err(SlotError::Unbalanced {
                        line,
                        message: format!("unexpected '{}'", c as char),
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
    if block_comments > 0 {
        return Err(SlotError::Unbalanced {
            line,
            message: "unterminated block comment".into(),
        });
    }
    if let Some(open) = stack.last() {
        return Err(SlotError::Unbalanced {
            line,
            message: format!("unclosed '{}'", *open as char),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlotDef {
    pub signature: SlotSignature,
    /// Default for this build: the engine default, or the look's override.
    pub default_body: String,
    pub body: String,
    /// `core` or the look that provided the body (`look.<name>`).
    pub owner: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(pub u32);

/// Lines the generated function header adds before the body (for mapping
/// compiler messages back to body lines).
pub const SLOT_HEADER_LINES: usize = 1;

/// World resource.
#[derive(Debug, Clone, Default)]
pub struct ShaderSlots {
    slots: Vec<SlotDef>,
    by_name: BTreeMap<&'static str, SlotId>,
    changed_at: Vec<u64>,
    revision: u64,
}

impl ShaderSlots {
    /// Registry pre-filled with [`BUILTIN_SLOTS`].
    pub fn with_builtins() -> Self {
        let mut s = Self::default();
        for sig in BUILTIN_SLOTS {
            s.declare(sig);
        }
        s
    }

    /// Declare a slot (idempotent by name).
    pub fn declare(&mut self, signature: SlotSignature) -> SlotId {
        if let Some(&id) = self.by_name.get(signature.name) {
            return id;
        }
        let id = SlotId(self.slots.len() as u32);
        self.slots.push(SlotDef {
            signature,
            default_body: signature.default_body.to_string(),
            body: signature.default_body.to_string(),
            owner: "core".into(),
        });
        self.changed_at.push(self.revision);
        self.by_name.insert(signature.name, id);
        id
    }

    pub fn id(&self, name: &str) -> Option<SlotId> {
        self.by_name.get(name).copied()
    }

    pub fn get(&self, id: SlotId) -> Option<&SlotDef> {
        self.slots.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = (SlotId, &SlotDef)> {
        self.slots
            .iter()
            .enumerate()
            .map(|(i, d)| (SlotId(i as u32), d))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn changed_since(&self, id: SlotId, since: u64) -> bool {
        self.changed_at
            .get(id.0 as usize)
            .is_some_and(|&r| r > since)
    }

    /// A look replaces the default body for this build.
    pub fn set_default(&mut self, name: &str, body: &str, owner: &str) -> Result<(), SlotError> {
        let id = self
            .id(name)
            .ok_or_else(|| SlotError::Unknown(name.to_string()))?;
        check_body(body)?;
        let s = &mut self.slots[id.0 as usize];
        s.default_body = body.to_string();
        s.body = body.to_string();
        s.owner = owner.to_string();
        self.revision += 1;
        self.changed_at[id.0 as usize] = self.revision;
        Ok(())
    }

    /// Replace the current body (live). Returns the previous body. The
    /// renderer must still compile it; if that fails, call this again with
    /// the previous body to roll back.
    pub fn set_body(&mut self, id: SlotId, body: &str) -> Result<String, SlotError> {
        check_body(body)?;
        let s = self
            .slots
            .get_mut(id.0 as usize)
            .ok_or_else(|| SlotError::Unknown(format!("#{}", id.0)))?;
        let prev = core::mem::replace(&mut s.body, body.to_string());
        self.revision += 1;
        self.changed_at[id.0 as usize] = self.revision;
        Ok(prev)
    }

    /// The full WGSL function for a slot, e.g.
    /// `fn slot_tonemap(color: vec3<f32>) -> vec3<f32> { ... }`.
    pub fn wgsl_function(&self, id: SlotId) -> Option<String> {
        let s = self.get(id)?;
        Some(Self::assemble(&s.signature, &s.body))
    }

    pub fn assemble(sig: &SlotSignature, body: &str) -> String {
        let mut out = String::with_capacity(body.len() + 96);
        out.push_str("fn slot_");
        out.push_str(sig.name);
        out.push('(');
        for (i, (name, ty)) in sig.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(name);
            out.push_str(": ");
            out.push_str(ty);
        }
        out.push_str(") -> ");
        out.push_str(sig.ret);
        out.push_str(" {\n");
        out.push_str(body);
        out.push_str("\n}\n");
        out
    }
}

// ---- runtime slot rebuilds ------------------------------------------------------------

/// Ticket for one requested slot rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct RebuildHandle(pub u64);

/// One compiler message, mapped back to the slot body.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SlotMessage {
    /// `error`, `warning` or `info`.
    pub severity: &'static str,
    pub text: String,
    /// 1-based line in the body (0 = outside the body, e.g. a pass template).
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    pub length: u32,
    /// The body line the message points at.
    pub source_line: Option<String>,
}

/// The outcome of a rebuild, shaped like `slot.set`'s `compile` result in
/// docs/LIVE_API.md.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SlotCompileResult {
    pub ok: bool,
    /// Wall-clock ms from request to result.
    pub ms: f64,
    /// `None` on success; `Some("previous")` when the old pipelines stayed.
    pub kept: Option<&'static str>,
    pub messages: Vec<SlotMessage>,
    /// Pipelines swapped in (`post.tonemap`...).
    pub pipelines_rebuilt: Vec<String>,
}

/// A queued rebuild for the renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotRequest {
    pub handle: RebuildHandle,
    pub slot: SlotId,
    pub body: String,
    pub requested_ms: f64,
}

/// World resource: the small API evani-live (and test hooks) use to change a
/// slot at runtime. [`SlotRebuilds::request`] runs the structural check and
/// queues the body; the renderer takes pending requests, compiles them in a
/// GPU error scope, swaps the pipelines in at a frame boundary on success
/// (and only then updates [`ShaderSlots`]) or keeps the old ones, and posts
/// the result; [`SlotRebuilds::take_result`] hands it out once.
#[derive(Debug, Default)]
pub struct SlotRebuilds {
    next: u64,
    pending: Vec<SlotRequest>,
    results: Vec<(RebuildHandle, SlotCompileResult)>,
    /// Requests nobody waits for: their results are dropped on completion.
    detached: Vec<RebuildHandle>,
}

impl SlotRebuilds {
    pub fn request(
        &mut self,
        slots: &ShaderSlots,
        name: &str,
        body: &str,
        now_ms: f64,
    ) -> Result<RebuildHandle, SlotError> {
        let slot = slots
            .id(name)
            .ok_or_else(|| SlotError::Unknown(name.to_string()))?;
        check_body(body)?;
        self.next += 1;
        let handle = RebuildHandle(self.next);
        self.pending.push(SlotRequest {
            handle,
            slot,
            body: body.to_string(),
            requested_ms: now_ms,
        });
        Ok(handle)
    }

    /// Queue a rebuild nobody waits for (its result is dropped): used when
    /// [`ShaderSlots`] already holds a body that was compiled before (undo,
    /// redo, auto-revert, snapshot restore) so the GPU pipelines follow it.
    pub fn request_detached(&mut self, slots: &ShaderSlots, name: &str, body: &str, now_ms: f64) -> Result<(), SlotError> {
        let handle = self.request(slots, name, body, now_ms)?;
        self.detached.push(handle);
        Ok(())
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// For the renderer: everything queued since the last call.
    pub fn take_pending(&mut self) -> Vec<SlotRequest> {
        core::mem::take(&mut self.pending)
    }

    /// For the renderer: post a finished rebuild.
    pub fn complete(&mut self, handle: RebuildHandle, result: SlotCompileResult) {
        if let Some(i) = self.detached.iter().position(|h| *h == handle) {
            self.detached.swap_remove(i);
            return;
        }
        self.results.push((handle, result));
    }

    /// The result, once (then it is forgotten).
    pub fn take_result(&mut self, handle: RebuildHandle) -> Option<SlotCompileResult> {
        let i = self.results.iter().position(|(h, _)| *h == handle)?;
        Some(self.results.swap_remove(i).1)
    }
}

// Declared after every other item in this file, just above the tests (not in BUILTIN_SLOTS): panic
// locations are in the wasm, so games without `relief` stay byte-identical
// only if no line above moves.

/// Micro-detail on textured models (evani-render `relief`, S3-10): the
/// height gradient `(dh/dx, dh/dy)` of the pattern at `uv`, in pattern
/// periods (one period = 1 in each axis, already rotated). `pattern` is the
/// material's recipe from `extras.evani.relief.micro`: pattern id (1 knurl,
/// 2 ribs, 3 dots, 4 grain), period in UV units, depth 0..1, angle in
/// radians. The renderer scales it by the depth and fades it out where the
/// period is under 1.5 px on screen; a look supplies the patterns (the toon
/// look's `relief` feature). Declared by the renderer and the look only in
/// builds with `relief`.
pub const MICRO_DETAIL: SlotSignature = SlotSignature {
    name: "micro_detail",
    params: &[("uv", "vec2<f32>"), ("pattern", "vec4<f32>")],
    ret: "vec2<f32>",
    description: "Micro-detail height gradient of a model material's pattern at uv (in pattern periods); zero = no micro-detail",
    default_body: "    return vec2<f32>(0.0);",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_rebuilds_are_queued_and_their_results_dropped() {
        let slots = ShaderSlots::with_builtins();
        let mut r = SlotRebuilds::default();
        r.request_detached(&slots, "tonemap", "return color;", 0.0).unwrap();
        let waited = r.request(&slots, "tonemap", "return color * 0.5;", 0.0).unwrap();
        let pending = r.take_pending();
        assert_eq!(pending.len(), 2, "the renderer sees both");
        for p in &pending {
            r.complete(p.handle, SlotCompileResult { ok: true, ms: 1.0, kept: None, messages: Vec::new(), pipelines_rebuilt: Vec::new() });
        }
        assert!(r.take_result(pending[0].handle).is_none(), "detached result dropped");
        assert!(r.take_result(waited).is_some());
        assert!(r.results.is_empty() && r.detached.is_empty(), "nothing left behind");
    }

    #[test]
    fn rebuild_requests_check_and_round_trip() {
        let slots = ShaderSlots::with_builtins();
        let mut r = SlotRebuilds::default();
        assert!(matches!(
            r.request(&slots, "nope", "return 1.0;", 0.0),
            Err(SlotError::Unknown(_))
        ));
        assert!(
            r.request(&slots, "tonemap", "return color; }", 0.0)
                .is_err()
        );
        let h = r.request(&slots, "tonemap", "return color;", 5.0).unwrap();
        let p = r.take_pending();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].slot, slots.id("tonemap").unwrap());
        assert!(!r.has_pending());
        assert_eq!(r.take_result(h), None);
        r.complete(
            h,
            SlotCompileResult {
                ok: false,
                ms: 3.0,
                kept: Some("previous"),
                messages: vec![],
                pipelines_rebuilt: vec![],
            },
        );
        assert_eq!(r.take_result(h).map(|x| x.ok), Some(false));
        assert_eq!(r.take_result(h), None);
    }

    #[test]
    fn builtins_and_assembly() {
        let s = ShaderSlots::with_builtins();
        let id = s.id("fog").unwrap();
        let f = s.wgsl_function(id).unwrap();
        assert!(f.starts_with("fn slot_fog(distance: f32, height: f32) -> f32 {\n"));
        assert!(f.contains("return 0.0;"));
        for sig in BUILTIN_SLOTS {
            check_body(sig.default_body).unwrap();
        }
        assert_eq!(
            f.lines().position(|l| l.contains("return")),
            Some(SLOT_HEADER_LINES)
        );
    }

    #[test]
    fn body_checks_stop_escapes() {
        assert!(check_body("return x;").is_ok());
        assert!(check_body("// } not code\nreturn vec3<f32>(1.0);").is_ok());
        assert!(check_body("/* { */ return 1.0;").is_ok());
        assert!(matches!(
            check_body("return 1.0; } fn evil() {"),
            Err(SlotError::Unbalanced { .. })
        ));
        assert!(check_body("if (x) { return 1.0;").is_err());
        assert!(check_body("return (1.0];").is_err());
        assert!(check_body("/* open").is_err());
        assert_eq!(check_body("   "), Err(SlotError::Empty));
        assert_eq!(
            check_body(&"x".repeat(MAX_BODY_LEN + 1)),
            Err(SlotError::TooLong)
        );
        match check_body("a;\nb;\n}") {
            Err(SlotError::Unbalanced { line, .. }) => assert_eq!(line, 3),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn live_swap_and_look_defaults() {
        let mut s = ShaderSlots::with_builtins();
        let id = s.id("tonemap").unwrap();
        let rev = s.revision();
        s.set_default(
            "tonemap",
            "return color / (color + vec3<f32>(1.0));",
            "look.basic",
        )
        .unwrap();
        assert!(s.changed_since(id, rev));
        assert_eq!(s.get(id).unwrap().owner, "look.basic");
        let prev = s.set_body(id, "return color;").unwrap();
        assert!(prev.contains("color + vec3"));
        assert!(s.set_body(id, "return color; }").is_err());
        assert_eq!(s.get(id).unwrap().body, "return color;");
        assert!(s.set_default("nope", "return 1.0;", "x").is_err());
        assert_eq!(s.declare(TONEMAP), id);
    }
}

/// The engine's default `tonemap` is the ACES filmic fit (Krzysztof
/// Narkowicz 2015, CC0; THIRD_PARTY_NOTICES.md): its constants evaluated at
/// known points.
#[cfg(test)]
mod aces_tests {
    use super::TONEMAP;

    /// The body's five constants, read from the WGSL itself.
    fn constants() -> [f32; 5] {
        let mut out = [f32::NAN; 5];
        for (i, k) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            let tag = format!("let {k} = ");
            let line = TONEMAP.default_body.lines().find_map(|l| l.trim().strip_prefix(tag.as_str())).unwrap();
            out[i] = line.trim_end_matches(';').parse().unwrap();
        }
        out
    }

    fn aces(x: f32) -> f32 {
        let [a, b, c, d, e] = constants();
        ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
    }

    #[test]
    fn default_tonemap_is_the_narkowicz_aces_fit() {
        assert_eq!(constants(), [2.51, 0.03, 2.43, 0.59, 0.14]);
        assert!(TONEMAP.default_body.contains("Narkowicz 2015"));
        assert_eq!(aces(0.0), 0.0);
        assert!((aces(0.18) - 0.266_90).abs() < 1e-4, "{}", aces(0.18));
        assert!((aces(1.0) - 0.803_80).abs() < 1e-4, "{}", aces(1.0));
        assert_eq!(aces(10.0), 1.0);
        assert_eq!(aces(1000.0), 1.0);
        // Monotonic over the display range.
        let mut last = 0.0;
        for i in 1..200 {
            let y = aces(i as f32 * 0.05);
            assert!(y >= last, "not monotonic at {}", i as f32 * 0.05);
            last = y;
        }
    }
}
