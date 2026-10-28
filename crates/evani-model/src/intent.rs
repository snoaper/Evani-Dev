//! The intent document (`.evmodel`): our own small text format, its canonical
//! JSON form, the content hash and seed, and vocabulary checks with nearest-
//! word suggestions (docs/MODEL_API.md section 1, docs/model_research/01).
//!
//! Text rules: 2-space indentation (tabs are an error), `key: value` lines, a
//! key with nothing after the colon opens a block, inline maps `{ a: b, c }`
//! where a bare word is a flag, lists `[a, b] m` (a trailing unit applies to
//! every item), quantities `4mm`, `2 cm`, `30 deg`, `25%`, counts `3k`, calls
//! `tread(chevron, depth 4mm)`, words with `/` and `.` for references, and
//! `"quoted"` strings. Canonical JSON: lengths in metres, angles in degrees,
//! percentages as fractions, flags as `true`, a single `like` as a list,
//! calls as objects; keys in the published order at the top and sorted below.

use serde_json::{Map, Value};

/// One diagnostic: a short code, a one-line message and a one-line fix.
#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub code: &'static str,
    pub msg: String,
    pub fix: String,
    /// 1-based line in the text form (0 when not from text).
    pub line: usize,
    pub warning: bool,
}

impl Diag {
    pub fn err(code: &'static str, line: usize, msg: impl Into<String>, fix: impl Into<String>) -> Self {
        Diag { code, msg: msg.into(), fix: fix.into(), line, warning: false }
    }
    pub fn warn(code: &'static str, msg: impl Into<String>, fix: impl Into<String>) -> Self {
        Diag { code, msg: msg.into(), fix: fix.into(), line: 0, warning: true }
    }
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("error".into(), Value::from(self.code));
        let msg = if self.line > 0 { format!("line {}: {}", self.line, self.msg) } else { self.msg.clone() };
        m.insert("msg".into(), Value::from(msg));
        m.insert("fix".into(), Value::from(self.fix.clone()));
        Value::Object(m)
    }
}

/// The published top-level order.
pub const FIELDS: [&str; 12] = ["evmodel", "make", "like", "style", "parts", "relief", "colours", "must", "match", "explore", "return", "seed"];

/// `#rrggbb` (sRGB) to bytes.
pub fn parse_hex_colour(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    LParen,
    RParen,
    Comma,
    Colon,
    Num(f64),
    Word(String),
    Str(String),
    /// A number directly or after a space followed by a unit word.
    Unit(String),
}

const UNITS: [&str; 7] = ["m", "cm", "mm", "deg", "rad", "%", "k"];

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '/' || c == '-'
}

fn tokenize(s: &str, line: usize) -> Result<Vec<Tok>, Diag> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            ' ' => i += 1,
            '\t' => return Err(Diag::err("tab", line, "a tab inside a value", "use spaces")),
            '{' => {
                out.push(Tok::LBrace);
                i += 1
            }
            '}' => {
                out.push(Tok::RBrace);
                i += 1
            }
            '[' => {
                out.push(Tok::LBrack);
                i += 1
            }
            ']' => {
                out.push(Tok::RBrack);
                i += 1
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1
            }
            '%' => {
                out.push(Tok::Unit("%".into()));
                i += 1
            }
            '"' => {
                let mut j = i + 1;
                let mut v = String::new();
                while j < cs.len() && cs[j] != '"' {
                    if cs[j] == '\\' && j + 1 < cs.len() {
                        j += 1;
                    }
                    v.push(cs[j]);
                    j += 1;
                }
                if j >= cs.len() {
                    return Err(Diag::err("syntax", line, "a quoted string is not closed", "end it with \""));
                }
                out.push(Tok::Str(v));
                i = j + 1;
            }
            _ if c.is_ascii_digit() || (c == '-' && i + 1 < cs.len() && cs[i + 1].is_ascii_digit()) => {
                let mut j = i + 1;
                while j < cs.len() && (cs[j].is_ascii_digit() || cs[j] == '.') {
                    j += 1;
                }
                let raw: String = cs[i..j].iter().collect();
                let v: f64 = raw.parse().map_err(|_| Diag::err("syntax", line, format!("`{raw}` is not a number"), "write numbers like 0.5 or 12"))?;
                out.push(Tok::Num(v));
                // A unit glued to the number.
                let mut k = j;
                while k < cs.len() && cs[k].is_ascii_alphabetic() {
                    k += 1;
                }
                if k > j {
                    let u: String = cs[j..k].iter().collect();
                    out.push(Tok::Unit(u));
                }
                i = k;
            }
            _ if is_word_char(c) => {
                let mut j = i;
                while j < cs.len() && is_word_char(cs[j]) {
                    j += 1;
                }
                let w: String = cs[i..j].iter().collect();
                // A unit word after a number (with a space) is a unit.
                if matches!(out.last(), Some(Tok::Num(_)) | Some(Tok::RBrack)) && UNITS.contains(&w.as_str()) {
                    out.push(Tok::Unit(w));
                } else {
                    out.push(Tok::Word(w));
                }
                i = j;
            }
            _ => return Err(Diag::err("syntax", line, format!("unexpected `{c}`"), "words use a-z, 0-9, _, ., / and -")),
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnitKind {
    Length,
    Angle,
    Fraction,
    Count,
}

fn unit_value(v: f64, u: &str, line: usize) -> Result<(f64, UnitKind), Diag> {
    Ok(match u {
        "m" => (v, UnitKind::Length),
        "cm" => (v / 100.0, UnitKind::Length),
        "mm" => (v / 1000.0, UnitKind::Length),
        "deg" => (v, UnitKind::Angle),
        "rad" => (v * (180.0 / core::f64::consts::PI), UnitKind::Angle),
        "%" => (v / 100.0, UnitKind::Fraction),
        "k" => (v * 1000.0, UnitKind::Count),
        _ => {
            let near = nearest(u, UNITS.iter().map(|s| s.to_string()));
            return Err(Diag::err("bad_unit", line, format!("unknown unit `{u}`"), format!("use one of m, cm, mm, deg, rad, % (nearest: {})", near.join(", "))));
        }
    })
}

/// Parsed values keep the unit kind so validation can check it.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Map(Vec<(String, Val, usize)>),
    List(Vec<Val>),
    Word(String),
    Str(String),
    Num(f64, Option<UnitKind>),
    Call(String, Vec<Val>, Vec<(String, Val)>),
    Flag,
}

struct P<'a> {
    t: &'a [Tok],
    i: usize,
    line: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn value(&mut self) -> Result<Val, Diag> {
        match self.next() {
            Some(Tok::LBrace) => {
                let mut entries = Vec::new();
                if self.peek() == Some(&Tok::RBrace) {
                    self.i += 1;
                    return Ok(Val::Map(entries));
                }
                loop {
                    let key = match self.next() {
                        Some(Tok::Word(w)) => w,
                        _ => return Err(Diag::err("syntax", self.line, "expected a name inside { }", "write { key: value, flag }")),
                    };
                    if self.peek() == Some(&Tok::Colon) {
                        self.i += 1;
                        let v = self.value()?;
                        entries.push((key, v, self.line));
                    } else {
                        entries.push((key, Val::Flag, self.line));
                    }
                    match self.next() {
                        Some(Tok::Comma) => continue,
                        Some(Tok::RBrace) => break,
                        _ => return Err(Diag::err("syntax", self.line, "expected , or } in a map", "separate entries with commas and close with }")),
                    }
                }
                Ok(Val::Map(entries))
            }
            Some(Tok::LBrack) => {
                let mut items = Vec::new();
                if self.peek() != Some(&Tok::RBrack) {
                    loop {
                        items.push(self.value()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RBrack) => break,
                            _ => return Err(Diag::err("syntax", self.line, "expected , or ] in a list", "separate items with commas and close with ]")),
                        }
                    }
                } else {
                    self.i += 1;
                }
                // A unit after the list applies to each item.
                if let Some(Tok::Unit(u)) = self.peek().cloned() {
                    self.i += 1;
                    let mut out = Vec::new();
                    for it in items {
                        match it {
                            Val::Num(v, None) => {
                                let (cv, k) = unit_value(v, &u, self.line)?;
                                out.push(Val::Num(cv, Some(k)));
                            }
                            _ => return Err(Diag::err("bad_unit", self.line, format!("the unit `{u}` follows a list that is not all plain numbers"), "put the unit after a list of numbers only")),
                        }
                    }
                    return Ok(Val::List(out));
                }
                Ok(Val::List(items))
            }
            Some(Tok::Num(v)) => {
                if let Some(Tok::Unit(u)) = self.peek().cloned() {
                    self.i += 1;
                    let (cv, k) = unit_value(v, &u, self.line)?;
                    return Ok(Val::Num(cv, Some(k)));
                }
                Ok(Val::Num(v, None))
            }
            Some(Tok::Word(w)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let (mut pos, mut kw) = (Vec::new(), Vec::new());
                    if self.peek() == Some(&Tok::RParen) {
                        self.i += 1;
                        return Ok(Val::Call(w, pos, kw));
                    }
                    loop {
                        // `key value` or a positional value.
                        let is_kw = matches!(self.peek(), Some(Tok::Word(_))) && !matches!(self.t.get(self.i + 1), Some(Tok::Comma) | Some(Tok::RParen) | Some(Tok::LParen) | None);
                        if is_kw {
                            let k = match self.next() {
                                Some(Tok::Word(k)) => k,
                                _ => String::new(),
                            };
                            let v = self.value()?;
                            kw.push((k, v));
                        } else {
                            pos.push(self.value()?);
                        }
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            _ => return Err(Diag::err("syntax", self.line, format!("expected , or ) in `{w}(...)`"), "separate arguments with commas and close with )")),
                        }
                    }
                    return Ok(Val::Call(w, pos, kw));
                }
                Ok(Val::Word(w))
            }
            Some(Tok::Str(s)) => Ok(Val::Str(s)),
            Some(Tok::Unit(u)) => Err(Diag::err("bad_unit", self.line, format!("the unit `{u}` has no number before it"), "write the number first, for example 4mm")),
            _ => Err(Diag::err("syntax", self.line, "expected a value", "write a word, number, [list], { map } or call(...)")),
        }
    }
}

fn parse_inline(s: &str, line: usize) -> Result<Val, Diag> {
    let toks = tokenize(s, line)?;
    let mut p = P { t: &toks, i: 0, line };
    let v = p.value()?;
    if p.i < toks.len() {
        return Err(Diag::err("syntax", line, format!("unexpected text after the value: `{}`", s.trim()), "one value per line; put lists in [ ] and maps in { }"));
    }
    Ok(v)
}

struct Line {
    no: usize,
    indent: usize,
    key: String,
    rest: String,
}

fn split_lines(src: &str) -> Result<Vec<Line>, Diag> {
    let mut out = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let no = i + 1;
        // Strip a comment outside quotes.
        let mut in_q = false;
        let mut cut = raw.len();
        for (bi, c) in raw.char_indices() {
            if c == '"' {
                in_q = !in_q;
            }
            if c == '#' && !in_q {
                cut = bi;
                break;
            }
        }
        let text = raw[..cut].trim_end();
        if text.trim().is_empty() {
            continue;
        }
        let lead: String = text.chars().take_while(|c| c.is_whitespace()).collect();
        if lead.contains('\t') {
            return Err(Diag::err("tab_indent", no, "a tab in the indentation", "indent with 2 spaces per level"));
        }
        let indent = lead.chars().count();
        if !indent.is_multiple_of(2) {
            return Err(Diag::err("bad_indent", no, format!("indentation of {indent} spaces"), "indent with 2 spaces per level"));
        }
        let body = text.trim_start();
        let (key, rest) = match body.find(':') {
            Some(c) if !body[..c].contains(' ') && !body[..c].contains('{') && !body[..c].contains('(') => (body[..c].to_string(), body[c + 1..].trim().to_string()),
            _ => (body.to_string(), String::from("\u{0}flag")),
        };
        out.push(Line { no, indent, key, rest });
    }
    Ok(out)
}

fn parse_block(lines: &[Line], i: &mut usize, indent: usize) -> Result<Vec<(String, Val, usize)>, Diag> {
    let mut out: Vec<(String, Val, usize)> = Vec::new();
    while *i < lines.len() {
        let l = &lines[*i];
        if l.indent < indent {
            break;
        }
        if l.indent > indent {
            return Err(Diag::err("bad_indent", l.no, "unexpected indentation", "indent a block 2 spaces under the key that opens it"));
        }
        *i += 1;
        if out.iter().any(|(k, _, _)| *k == l.key) {
            return Err(Diag::err("duplicate_key", l.no, format!("`{}` appears twice", l.key), "keep one entry per key"));
        }
        let v = if l.rest == "\u{0}flag" {
            if l.key.chars().any(|c| !is_word_char(c)) {
                return Err(Diag::err("syntax", l.no, format!("cannot read `{}`", l.key), "write `key: value`"));
            }
            Val::Flag
        } else if l.rest.is_empty() {
            if *i < lines.len() && lines[*i].indent > indent {
                let inner = lines[*i].indent;
                if inner != indent + 2 {
                    return Err(Diag::err("bad_indent", lines[*i].no, "a block must be indented by 2 spaces", "indent a block 2 spaces under its key"));
                }
                Val::Map(parse_block(lines, i, inner)?)
            } else {
                return Err(Diag::err("syntax", l.no, format!("`{}:` has no value", l.key), "give it a value or an indented block"));
            }
        } else {
            parse_inline(&l.rest, l.no)?
        };
        out.push((l.key.clone(), v, l.no));
    }
    Ok(out)
}

/// What a value should be, for unit checks.
fn expect_kind(path: &[&str]) -> Option<UnitKind> {
    match path {
        ["must", "fit_box"] | ["must", "min_thickness"] => Some(UnitKind::Length),
        ["must", "min_size" | "max_size", _] => Some(UnitKind::Length),
        ["must", "wheels_clear_chassis_at_steer"] => Some(UnitKind::Angle),
        ["must", _, "at_least" | "diameter"] => Some(UnitKind::Length),
        ["must", _, "angle"] => Some(UnitKind::Angle),
        ["must", "tri_budget", _] => Some(UnitKind::Count),
        ["relief", _, "depth" | "pitch" | "width" | "size" | "scale"] => Some(UnitKind::Length),
        ["relief", _, "angle"] => Some(UnitKind::Angle),
        ["relief", _, "density"] => Some(UnitKind::Fraction),
        _ => None,
    }
}

fn to_json(v: &Val, path: &mut Vec<String>, line: usize, diags: &mut Vec<Diag>) -> Value {
    match v {
        Val::Map(entries) => {
            let mut m = Map::new();
            for (k, x, l) in entries {
                path.push(k.clone());
                m.insert(k.clone(), to_json(x, path, *l, diags));
                path.pop();
            }
            Value::Object(m)
        }
        Val::List(items) => Value::Array(items.iter().map(|x| to_json(x, path, line, diags)).collect()),
        Val::Word(w) if w == "true" => Value::Bool(true),
        Val::Word(w) if w == "false" => Value::Bool(false),
        Val::Word(w) => Value::from(w.clone()),
        Val::Str(s) => Value::from(s.clone()),
        Val::Flag => Value::Bool(true),
        Val::Num(n, kind) => {
            let p: Vec<&str> = path.iter().map(String::as_str).collect();
            if let (Some(k), Some(want)) = (kind, expect_kind(&p))
                && *k != want
            {
                diags.push(Diag::err(
                    "bad_unit",
                    line,
                    format!("`{}` needs {} but has {}", p.join("."), kind_name(want), kind_name(*k)),
                    format!("write it {}", kind_example(want)),
                ));
            }
            if (kind.is_none() || kind == &Some(UnitKind::Count))
                && let Some(want) = expect_kind(&p)
                    && want == UnitKind::Count
                    && n.fract() != 0.0
                {
                    diags.push(Diag::err("bad_count", line, format!("`{}` must be a whole count", p.join(".")), "write a count like 3000 or 3k"));
                }
            num(*n)
        }
        Val::Call(name, pos, kw) => {
            let mut m = Map::new();
            m.insert("pattern".into(), Value::from(name.clone()));
            for (i, p) in pos.iter().enumerate() {
                let key = if i == 0 { "style".to_string() } else { format!("arg{i}") };
                m.insert(key, to_json(p, path, line, diags));
            }
            for (k, x) in kw {
                path.push(k.clone());
                m.insert(k.clone(), to_json(x, path, line, diags));
                path.pop();
            }
            Value::Object(m)
        }
    }
}

fn kind_name(k: UnitKind) -> &'static str {
    match k {
        UnitKind::Length => "a length",
        UnitKind::Angle => "an angle",
        UnitKind::Fraction => "a fraction",
        UnitKind::Count => "a count",
    }
}
fn kind_example(k: UnitKind) -> &'static str {
    match k {
        UnitKind::Length => "in m, cm or mm (for example 4mm)",
        UnitKind::Angle => "in deg or rad (for example 30 deg)",
        UnitKind::Fraction => "as a plain number from 0 to 1, or a percentage",
        UnitKind::Count => "as a count (for example 3k)",
    }
}

fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map(Value::Number).unwrap_or(Value::Null)
    }
}

/// Parse `.evmodel` text into canonical JSON (validation is separate).
pub fn parse_text(src: &str) -> Result<Value, Vec<Diag>> {
    let lines = split_lines(src).map_err(|d| vec![d])?;
    let mut i = 0;
    let entries = parse_block(&lines, &mut i, 0).map_err(|d| vec![d])?;
    let mut diags = Vec::new();
    let mut top = Map::new();
    for (k, v, kl) in &entries {
        let mut path = vec![k.clone()];
        let mut j = to_json(v, &mut path, *kl, &mut diags);
        if k == "like"
            && let Value::String(_) = j {
                j = Value::Array(vec![j]);
            }
        if k == "relief"
            && let Value::Object(m) = &mut j {
                for (_, layer) in m.iter_mut() {
                    if layer.is_object() {
                        *layer = Value::Array(vec![layer.clone()]);
                    }
                }
            }
        top.insert(k.clone(), j);
    }
    if diags.is_empty() { Ok(Value::Object(top)) } else { Err(diags) }
}

/// Canonical JSON text: top-level keys in the published order, nested keys
/// sorted, numbers by Rust's shortest round-trip formatting (deterministic).
pub fn canonical(v: &Value) -> String {
    let mut s = String::new();
    write_canon(v, true, &mut s);
    s
}

fn write_canon(v: &Value, top: bool, s: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| {
                if top {
                    let ia = FIELDS.iter().position(|f| f == a).unwrap_or(FIELDS.len());
                    let ib = FIELDS.iter().position(|f| f == b).unwrap_or(FIELDS.len());
                    ia.cmp(&ib).then(a.cmp(b))
                } else {
                    a.cmp(b)
                }
            });
            s.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&Value::from(k.as_str()).to_string());
                s.push(':');
                if let Some(x) = m.get(*k) {
                    write_canon(x, false, s);
                }
            }
            s.push('}');
        }
        Value::Array(a) => {
            s.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_canon(x, false, s);
            }
            s.push(']');
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                s.push_str(&i.to_string());
            } else if let Some(f) = n.as_f64() {
                s.push_str(&fmt_num(f));
            }
        }
        other => s.push_str(&other.to_string()),
    }
}

/// Shortest round-trip decimal for a float, never `-0`.
pub fn fmt_num(f: f64) -> String {
    if f == 0.0 {
        return "0".into();
    }
    if f.fract() == 0.0 && f.abs() < 9.0e15 {
        return format!("{}", f as i64);
    }
    format!("{f}")
}

/// `sha256:<hex>` of the canonical form.
pub fn content_hash(v: &Value) -> String {
    crate::sha256::tag(canonical(v).as_bytes())
}

/// The intent's seed, or one derived from its content hash.
pub fn seed(v: &Value) -> u32 {
    if let Some(s) = v.get("seed").and_then(Value::as_u64) {
        return s as u32;
    }
    // Recolouring keeps the shape: colours stay out of the seed.
    let h = match v.as_object() {
        Some(m) if m.contains_key("colours") => {
            let mut m = m.clone();
            m.remove("colours");
            crate::sha256::sha256(canonical(&Value::Object(m)).as_bytes())
        }
        _ => crate::sha256::sha256(canonical(v).as_bytes()),
    };
    u32::from_be_bytes([h[0], h[1], h[2], h[3]])
}

fn word_safe(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_word_char) && !s.starts_with(|c: char| c.is_ascii_digit() || c == '-')
}

/// Canonical JSON back to text (round trip: parse(to_text(j)) == j).
pub fn to_text(v: &Value) -> String {
    let mut out = String::new();
    let Some(m) = v.as_object() else { return out };
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort_by_key(|k| FIELDS.iter().position(|f| f == *k).unwrap_or(FIELDS.len()));
    for k in keys {
        let x = &m[k];
        match (k.as_str(), x) {
            ("parts" | "relief" | "colours" | "must", Value::Object(inner)) => {
                out.push_str(&format!("{k}:\n"));
                let mut ik: Vec<&String> = inner.keys().collect();
                ik.sort();
                for name in ik {
                    let path = [k.as_str(), name.as_str()];
                    out.push_str(&format!("  {name}: {}\n", inline(&inner[name], &path)));
                }
            }
            _ => out.push_str(&format!("{k}: {}\n", inline(x, &[k.as_str()]))),
        }
    }
    out
}

fn unit_suffix(path: &[&str]) -> &'static str {
    match expect_kind(path) {
        Some(UnitKind::Length) => " m",
        Some(UnitKind::Angle) => " deg",
        _ => "",
    }
}

fn inline(v: &Value, path: &[&str]) -> String {
    match v {
        Value::Object(m) => {
            if let Some(p) = m.get("pattern").and_then(Value::as_str) {
                let mut args = Vec::new();
                if let Some(s) = m.get("style") {
                    args.push(inline(s, path));
                }
                let mut ks: Vec<&String> = m.keys().filter(|k| *k != "pattern" && *k != "style").collect();
                ks.sort();
                for k in ks {
                    let mut pp = path.to_vec();
                    pp.push(k);
                    args.push(format!("{k} {}", inline(&m[k], &pp)));
                }
                return format!("{p}({})", args.join(", "));
            }
            let mut ks: Vec<&String> = m.keys().collect();
            ks.sort();
            let items: Vec<String> = ks
                .iter()
                .map(|k| {
                    if m[*k] == Value::Bool(true) {
                        (*k).clone()
                    } else {
                        let mut pp = path.to_vec();
                        pp.push(k);
                        format!("{k}: {}", inline(&m[*k], &pp))
                    }
                })
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
        Value::Array(a) => {
            let all_num = !a.is_empty() && a.iter().all(Value::is_number);
            let suffix = if all_num { unit_suffix(path) } else { "" };
            let items: Vec<String> = a.iter().map(|x| if x.is_number() { inline_num(x) } else { inline(x, path) }).collect();
            format!("[{}]{suffix}", items.join(", "))
        }
        Value::Number(_) => format!("{}{}", inline_num(v), unit_suffix(path)),
        Value::String(s) => {
            if word_safe(s) {
                s.clone()
            } else {
                Value::from(s.as_str()).to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "\"\"".into(),
    }
}

fn inline_num(v: &Value) -> String {
    match v.as_i64() {
        Some(i) => i.to_string(),
        None => fmt_num(v.as_f64().unwrap_or(0.0)),
    }
}

/// Edit distance (insert, delete, substitute; adjacent swap counts 1).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[a.len()][b.len()]
}

/// The three nearest words (ties by name).
pub fn nearest(w: &str, words: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<(usize, String)> = words.map(|x| (edit_distance(w, &x), x)).collect();
    v.sort();
    v.into_iter().take(3).map(|(_, x)| x).collect()
}

/// The vocabulary data (data/vocab.json).
pub fn vocab() -> Value {
    serde_json::from_str(include_str!("../data/vocab.json")).unwrap_or(Value::Null)
}

fn keys_of(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default()
}

fn unknown(code: &'static str, what: &str, w: &str, voc: &Value, table: &str) -> Diag {
    let near = nearest(w, keys_of(voc, table).into_iter());
    let hints: Vec<String> = near
        .iter()
        .map(|n| {
            let means = voc.get(table).and_then(|t| t.get(n)).map(|m| m.get("means").and_then(Value::as_str).or(m.as_str()).unwrap_or("")).unwrap_or("");
            if means.is_empty() { n.clone() } else { format!("{n} ({means})") }
        })
        .collect();
    Diag::err(code, 0, format!("unknown {what} `{w}`"), format!("did you mean: {}", hints.join("; ")))
}

/// Check the canonical intent against the vocabulary (class templates and
/// style adjectives are checked when the model is built).
pub fn validate(v: &Value) -> Vec<Diag> {
    let voc = vocab();
    let mut out = Vec::new();
    let Some(m) = v.as_object() else {
        return vec![Diag::err("not_a_map", 0, "the intent is not a map", "start with `make: <class>`")];
    };
    for k in m.keys() {
        if !FIELDS.contains(&k.as_str()) {
            out.push(unknown("unknown_field", "field", k, &voc, "fields"));
        }
    }
    match m.get("make").and_then(Value::as_str) {
        None => out.push(Diag::err("missing_make", 0, "no `make:` line", "add `make: <class>`, for example `make: kart`")),
        Some(c) => {
            let base = c.split('/').next().unwrap_or(c);
            if voc.get("classes").and_then(|t| t.get(base)).is_none() {
                out.push(unknown("unknown_class", "class", base, &voc, "classes"));
            }
        }
    }
    if let Some(s) = m.get("style") {
        match s.as_str() {
            Some(s) if voc.get("styles").and_then(|t| t.get(s)).is_some() => {}
            Some(s) => out.push(unknown("unknown_style", "style", s, &voc, "styles")),
            None => out.push(Diag::err("bad_style", 0, "`style` must be a word", "write `style: toon_chunky`")),
        }
    }
    match m.get("colours") {
        None => {}
        Some(Value::Object(cols)) => {
            for (region, c) in cols {
                if c.as_str().and_then(parse_hex_colour).is_none() {
                    out.push(Diag::err("bad_colour", 0, format!("colour for `{region}` is not #rrggbb"), format!("write `{region}: \"#7a5636\"` (quoted: # starts a comment)")));
                }
            }
        }
        Some(_) => out.push(Diag::err("bad_colours", 0, "`colours` must be a map of region to colour", "write `colours:` then lines like `  leather: \"#7a5636\"`")),
    }
    if let Some(Value::Object(rel)) = m.get("relief") {
        for (region, layers) in rel {
            let reg = region.rsplit('.').next().unwrap_or(region);
            if !region.contains('.') {
                out.push(Diag::err("bad_region", 0, format!("relief region `{region}` needs a part"), "write `<part>.<region>`, for example tyres.outer"));
            } else if voc.get("regions").and_then(|t| t.get(reg)).is_none() {
                out.push(unknown("unknown_region", "region", reg, &voc, "regions"));
            }
            for layer in layers.as_array().into_iter().flatten() {
                let Some(p) = layer.get("pattern").and_then(Value::as_str) else {
                    out.push(Diag::err("bad_relief", 0, format!("relief on `{region}` is not a pattern call"), "write pattern(style, key value), for example tread(chevron, depth 4mm)"));
                    continue;
                };
                let Some(pd) = voc.get("patterns").and_then(|t| t.get(p)) else {
                    out.push(unknown("unknown_pattern", "pattern", p, &voc, "patterns"));
                    continue;
                };
                let styles: Vec<String> = pd.get("styles").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                if let Some(st) = layer.get("style").and_then(Value::as_str)
                    && !styles.iter().any(|x| x == st) {
                        let near = nearest(st, styles.iter().cloned());
                        out.push(Diag::err("unknown_pattern_style", 0, format!("`{p}` has no style `{st}`"), format!("use one of: {}", if near.is_empty() { "none (no style word)".to_string() } else { near.join(", ") })));
                    }
                if let Some(o) = layer.as_object() {
                    for k in o.keys() {
                        if k != "pattern" && k != "style" && voc.get("pattern_keys").and_then(|t| t.get(k)).is_none() {
                            out.push(unknown("unknown_pattern_key", "pattern key", k, &voc, "pattern_keys"));
                        }
                    }
                }
            }
        }
    }
    if let Some(must) = m.get("must") {
        match must.as_object() {
            None => out.push(Diag::err("bad_must", 0, "`must` is not a block", "list must tests under `must:`")),
            Some(mm) => {
                for (k, a) in mm {
                    if voc.get("musts").and_then(|t| t.get(k)).is_none() {
                        out.push(unknown("unknown_must", "must kind", k, &voc, "musts"));
                        continue;
                    }
                    if let Some(d) = check_must_arg(k, a) {
                        out.push(d);
                    }
                }
            }
        }
    }
    if let Some(Value::Object(mt)) = m.get("match") {
        let views: Vec<String> = voc.get("views").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        for (k, p) in mt {
            if !views.contains(k) {
                let near = nearest(k, views.iter().cloned());
                out.push(Diag::err("unknown_view", 0, format!("unknown view `{k}`"), format!("did you mean: {}", near.join(", "))));
            } else if k != "weight" {
                let path = p.as_str().unwrap_or("");
                let want = if k == "mesh" { ".glb" } else { ".png" };
                if !path.ends_with(want) {
                    out.push(Diag::err("bad_reference", 0, format!("`match.{k}` should be a {want} file"), format!("point it at a {want} made by the reference tool")));
                }
            }
        }
    }
    if let Some(e) = m.get("explore") {
        let ok = e.as_str() == Some("auto") || e.as_u64().is_some_and(|n| (1..=1024).contains(&n));
        if !ok {
            out.push(Diag::err("bad_explore", 0, "`explore` must be auto or a level up to 1024", "write `explore: auto` or `explore: 256`"));
        }
    }
    if let Some(r) = m.get("return")
        && !r.as_u64().is_some_and(|n| (1..=8).contains(&n)) {
            out.push(Diag::err("bad_return", 0, "`return` must be 1 to 8", "write `return: 3`"));
        }
    if let Some(s) = m.get("seed")
        && !s.as_u64().is_some_and(|n| n <= u32::MAX as u64) {
            out.push(Diag::err("bad_seed", 0, "`seed` must be a whole number from 0 to 4294967295", "write `seed: 201` or leave it out"));
        }
    out
}

fn check_must_arg(k: &str, a: &Value) -> Option<Diag> {
    let bad = |fix: &str| Some(Diag::err("bad_must_arg", 0, format!("`must.{k}` has the wrong form"), fix.to_string()));
    match k {
        "fit_box" => {
            let ok = a.as_array().is_some_and(|l| l.len() == 3 && l.iter().all(|x| x.as_f64().is_some_and(|f| f > 0.0)));
            if !ok {
                return bad("write `fit_box: [length, width, height] m`");
            }
        }
        "min_size" | "max_size" => {
            let ok = a.as_object().is_some_and(|o| o.iter().all(|(kk, v)| ["length", "width", "height"].contains(&kk.as_str()) && v.as_f64().is_some_and(|f| f > 0.0)));
            if !ok {
                return bad("write `{ length: 1m, width: 0.5m, height: 0.4m }` (any of them)");
            }
        }
        "wheels_clear_chassis_at_steer" => {
            if !a.as_f64().is_some_and(|f| f > 0.0 && f < 90.0) {
                return bad("write an angle under 90 deg, for example `30 deg`");
            }
        }
        "tri_budget" => {
            let tiers = ["low", "medium", "high", "ultra", "vr"];
            let ok = a.as_object().is_some_and(|o| o.iter().all(|(kk, v)| tiers.contains(&kk.as_str()) && v.as_u64().is_some_and(|n| n > 0)));
            if !ok {
                return bad("write `{ low: 3k, medium: 8k, high: 20k, ultra: 60k }`");
            }
        }
        "clear" => {
            let ok = a.get("parts").and_then(Value::as_array).is_some_and(|p| p.len() == 2) && a.get("at_least").and_then(Value::as_f64).is_some();
            if !ok {
                return bad("write `{ parts: [a, b], at_least: 2cm }`");
            }
        }
        "swing_clear" => {
            let ok = a.get("part").and_then(Value::as_str).is_some() && a.get("about").and_then(Value::as_str).is_some() && a.get("angle").and_then(Value::as_f64).is_some();
            if !ok {
                return bad("write `{ part: lid, about: base.lid_hinge, angle: 95 deg, at_least: 3mm }`");
            }
        }
        "anchor" => {
            if a.get("name").and_then(Value::as_str).is_none() {
                return bad("write `{ name: grip, at: part.anchor, diameter: 3cm }` (at and diameter optional)");
            }
        }
        "stands" => {
            if !matches!(a.as_str(), Some("ground" | "none")) {
                return bad("write `stands: ground` or `stands: none`");
            }
        }
        "symmetric" => {
            if !matches!(a.as_str(), Some("x" | "y" | "z")) {
                return bad("write `symmetric: x`");
            }
        }
        "min_thickness" => {
            if !a.as_f64().is_some_and(|f| f > 0.0) {
                return bad("write a length, for example `6mm`");
            }
        }
        "seats" => {
            if a.as_str().is_none() {
                return bad("write a body reference, for example `bench/kart_driver`");
            }
        }
        "connected"
            if !a.is_boolean() => {
                return bad("write `connected: true` or `false`");
            }
        _ => {}
    }
    None
}

/// Parse text or JSON (a JSON intent starts with `{`).
pub fn read(src: &str) -> Result<Value, Vec<Diag>> {
    let t = src.trim_start();
    let v = if t.starts_with('{') {
        serde_json::from_str::<Value>(t).map_err(|e| vec![Diag::err("bad_json", e.line(), format!("JSON error: {e}"), "send canonical JSON (docs/MODEL_API.md 1.9) or .evmodel text")])?
    } else {
        parse_text(src)?
    };
    let d: Vec<Diag> = validate(&v).into_iter().filter(|d| !d.warning).collect();
    if d.is_empty() { Ok(v) } else { Err(d) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KART: &str = "make: kart\nlike: library/sugarpeak_kart\nstyle: toon_chunky\nparts:\n  chassis: { stance: low, wide }\n  rear_tyres: { size: oversized }\n  engine_cover: { shape: bubble, exhausts: 2 }\n  seat: { type: bucket }\n  nose: { shape: blunt, rounded }\nrelief:\n  tyres.outer: tread(chevron, depth 4mm, pitch 2cm)\n  chassis.edges: wear(chips, depth 1mm, density 0.3)\nmust:\n  fit_box: [1.3, 0.8, 0.7] m\n  wheels_clear_chassis_at_steer: 30 deg\n  seats: chibi_driver_v2\nmatch: { side: ref/kart_side.png, front: ref/kart_front.png }\nexplore: auto\nreturn: 3\n";

    #[test]
    fn the_briefs_kart_gives_the_published_json() {
        let j = parse_text(KART).unwrap();
        let want: Value = serde_json::from_str(r#"{
          "make": "kart", "like": ["library/sugarpeak_kart"], "style": "toon_chunky",
          "parts": { "chassis": { "stance": "low", "wide": true }, "rear_tyres": { "size": "oversized" },
            "engine_cover": { "shape": "bubble", "exhausts": 2 }, "seat": { "type": "bucket" }, "nose": { "shape": "blunt", "rounded": true } },
          "relief": { "tyres.outer": [{ "pattern": "tread", "style": "chevron", "depth": 0.004, "pitch": 0.02 }],
            "chassis.edges": [{ "pattern": "wear", "style": "chips", "depth": 0.001, "density": 0.3 }] },
          "must": { "fit_box": [1.3, 0.8, 0.7], "wheels_clear_chassis_at_steer": 30, "seats": "chibi_driver_v2" },
          "match": { "side": "ref/kart_side.png", "front": "ref/kart_front.png" }, "explore": "auto", "return": 3 }"#).unwrap();
        assert_eq!(canonical(&j), canonical(&want));
        assert!(validate(&j).is_empty(), "{:?}", validate(&j));
    }

    #[test]
    fn canonical_order_and_hash_ignore_source_order() {
        let a = parse_text("make: crate\nmust:\n  stands: ground\n  symmetric: x\n").unwrap();
        let b = parse_text("must:\n  symmetric: x\n  stands: ground\nmake: crate\n").unwrap();
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(content_hash(&a), content_hash(&b));
        assert!(canonical(&a).starts_with("{\"make\":"));
        assert_eq!(seed(&a), seed(&b));
        let c = parse_text("make: crate\nseed: 7\n").unwrap();
        assert_eq!(seed(&c), 7);
    }

    #[test]
    fn colours_parse_validate_round_trip_and_stay_out_of_the_seed() {
        let src = "make: character\ncolours:\n  leather: \"#7a5636\"  # brown\n  hair: \"#1c1612\"\n";
        let j = parse_text(src).unwrap();
        assert_eq!(j["colours"]["leather"], "#7a5636");
        assert!(validate(&j).is_empty(), "{:?}", validate(&j));
        let text = to_text(&j);
        let j2 = parse_text(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
        assert_eq!(canonical(&j), canonical(&j2), "{text}");
        assert_eq!(to_text(&j2), text);
        let plain = parse_text("make: character\n").unwrap();
        assert_eq!(seed(&j), seed(&plain));
        assert_ne!(content_hash(&j), content_hash(&plain));
        let bad = parse_text("make: character\ncolours:\n  leather: \"#7a56\"\n").unwrap();
        let d = validate(&bad);
        assert!(d.iter().any(|d| d.code == "bad_colour" && d.fix.contains("#7a5636")), "{d:?}");
        assert_eq!(parse_hex_colour("#FFa000"), Some([255, 160, 0]));
        assert_eq!(parse_hex_colour("ffa000"), None);
    }

    #[test]
    fn all_benchmark_intents_parse_validate_and_round_trip() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/model_bench");
        let mut n = 0;
        for dir in ["props", "vehicles", "organic"] {
            let mut files: Vec<_> = std::fs::read_dir(format!("{root}/{dir}")).unwrap().map(|e| e.unwrap().path()).collect();
            files.sort();
            for f in files {
                let src = std::fs::read_to_string(&f).unwrap();
                let j = parse_text(&src).unwrap_or_else(|e| panic!("{f:?}: {e:?}"));
                let d = validate(&j);
                assert!(d.is_empty(), "{f:?}: {d:?}");
                let text = to_text(&j);
                let j2 = parse_text(&text).unwrap_or_else(|e| panic!("{f:?} round trip: {e:?}\n{text}"));
                assert_eq!(canonical(&j), canonical(&j2), "{f:?}\n{text}");
                assert_eq!(to_text(&j2), text);
                n += 1;
            }
        }
        assert_eq!(n, 30);
    }

    #[test]
    fn golden_errors() {
        // Unknown word with suggestions.
        let j = parse_text("make: kart\nstyle: toon_chunkie\n").unwrap();
        let d = validate(&j);
        assert_eq!(d[0].code, "unknown_style");
        assert!(d[0].fix.contains("toon_chunky"), "{:?}", d[0]);
        let d = validate(&parse_text("make: kartt\n").unwrap());
        assert_eq!(d[0].code, "unknown_class");
        assert!(d[0].fix.starts_with("did you mean: kart ("), "{:?}", d[0]);
        let d = validate(&parse_text("make: kart\nrelief:\n  tyres.outer: tred(chevron)\n").unwrap());
        assert_eq!(d[0].code, "unknown_pattern");
        assert!(d[0].fix.contains("tread"));
        // Bad unit: a length given in degrees, and an unknown unit.
        let e = parse_text("make: crate\nmust:\n  fit_box: [1, 1, 1] deg\n").unwrap_err();
        assert_eq!(e[0].code, "bad_unit");
        assert_eq!(e[0].line, 3);
        let e = parse_text("make: crate\nrelief:\n  crate.faces: planks(straight, depth 2mx)\n").unwrap_err();
        assert_eq!(e[0].code, "bad_unit");
        // Tab indentation.
        let e = parse_text("make: crate\nmust:\n\tstands: ground\n").unwrap_err();
        assert_eq!(e[0].code, "tab_indent");
        assert_eq!(e[0].line, 3);
        // Odd indentation and duplicate keys.
        assert_eq!(parse_text("make: crate\nmust:\n   stands: ground\n").unwrap_err()[0].code, "bad_indent");
        assert_eq!(parse_text("make: crate\nmake: kart\n").unwrap_err()[0].code, "duplicate_key");
        // Error JSON has code, message and fix.
        let ej = e_json();
        assert!(ej.get("error").is_some() && ej.get("fix").is_some());
    }

    fn e_json() -> Value {
        parse_text("make: crate\nmust:\n\tstands: ground\n").unwrap_err()[0].to_json()
    }

    #[test]
    fn counts_with_k_and_quantities_with_spaces() {
        let j = parse_text("make: kart\nmust:\n  tri_budget: { low: 3k, medium: 1.5k }\n  min_thickness: 6 mm\n").unwrap();
        assert_eq!(j["must"]["tri_budget"]["low"], 3000);
        assert_eq!(j["must"]["tri_budget"]["medium"], 1500);
        assert_eq!(j["must"]["min_thickness"].as_f64(), Some(0.006));
    }
}
