//! Formulas in class templates: numbers, measure names (`chassis.width`),
//! `+ - * /`, parentheses, unary minus and the functions `min`, `max`, `abs`,
//! `sqrt`, `sin`, `cos` (degrees). Evaluated in f64 in source order, so the
//! result is the same on every machine.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
enum T {
    Num(f64),
    Id(String),
    Op(char),
    LP,
    RP,
    Comma,
}

fn lex(s: &str) -> Result<Vec<T>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && i + 1 < c.len() && c[i + 1].is_ascii_digit()) {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == 'e' || ((c[i] == '-' || c[i] == '+') && i > st && c[i - 1] == 'e')) {
                i += 1;
            }
            let raw: String = c[st..i].iter().collect();
            out.push(T::Num(raw.parse().map_err(|_| format!("bad number `{raw}` in `{s}`"))?));
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_' || c[i] == '.') {
                i += 1;
            }
            out.push(T::Id(c[st..i].iter().collect()));
        } else if "+-*/".contains(ch) {
            out.push(T::Op(ch));
            i += 1;
        } else if ch == '(' {
            out.push(T::LP);
            i += 1;
        } else if ch == ')' {
            out.push(T::RP);
            i += 1;
        } else if ch == ',' {
            out.push(T::Comma);
            i += 1;
        } else {
            return Err(format!("unexpected `{ch}` in `{s}`"));
        }
    }
    Ok(out)
}

struct E<'a> {
    t: Vec<T>,
    i: usize,
    vars: &'a BTreeMap<String, f64>,
    src: &'a str,
}

impl E<'_> {
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(T::Op(o)) = self.t.get(self.i).cloned() {
            if o != '+' && o != '-' {
                break;
            }
            self.i += 1;
            let r = self.term()?;
            v = if o == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        while let Some(T::Op(o)) = self.t.get(self.i).cloned() {
            if o != '*' && o != '/' {
                break;
            }
            self.i += 1;
            let r = self.unary()?;
            v = if o == '*' { v * r } else { v / r };
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<f64, String> {
        if self.t.get(self.i) == Some(&T::Op('-')) {
            self.i += 1;
            return Ok(-self.unary()?);
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<f64, String> {
        let tok = self.t.get(self.i).cloned();
        self.i += 1;
        match tok {
            Some(T::Num(n)) => Ok(n),
            Some(T::LP) => {
                let v = self.expr()?;
                if self.t.get(self.i) != Some(&T::RP) {
                    return Err(format!("missing ) in `{}`", self.src));
                }
                self.i += 1;
                Ok(v)
            }
            Some(T::Id(name)) => {
                if self.t.get(self.i) == Some(&T::LP) {
                    self.i += 1;
                    let mut args = vec![self.expr()?];
                    while self.t.get(self.i) == Some(&T::Comma) {
                        self.i += 1;
                        args.push(self.expr()?);
                    }
                    if self.t.get(self.i) != Some(&T::RP) {
                        return Err(format!("missing ) after {name}( in `{}`", self.src));
                    }
                    self.i += 1;
                    return match (name.as_str(), args.as_slice()) {
                        ("min", [a, rest @ ..]) => Ok(rest.iter().fold(*a, |m, x| m.min(*x))),
                        ("max", [a, rest @ ..]) => Ok(rest.iter().fold(*a, |m, x| m.max(*x))),
                        ("abs", [a]) => Ok(a.abs()),
                        ("sqrt", [a]) => Ok(a.sqrt()),
                        ("sin", [a]) => Ok(crate::dmath::sin(crate::dmath::deg(*a))),
                        ("cos", [a]) => Ok(crate::dmath::cos(crate::dmath::deg(*a))),
                        _ => Err(format!("unknown function {name} in `{}`", self.src)),
                    };
                }
                self.vars.get(&name).copied().ok_or_else(|| format!("unknown measure `{name}` in `{}`", self.src))
            }
            _ => Err(format!("cannot read `{}`", self.src)),
        }
    }
}

/// Evaluate `src` with the given measures.
pub fn eval(src: &str, vars: &BTreeMap<String, f64>) -> Result<f64, String> {
    let t = lex(src)?;
    let mut e = E { t, i: 0, vars, src };
    let v = e.expr()?;
    if e.i != e.t.len() {
        return Err(format!("unexpected text in `{src}`"));
    }
    Ok(v)
}

/// A JSON number or formula string.
pub fn eval_json(v: &serde_json::Value, vars: &BTreeMap<String, f64>) -> Result<f64, String> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| "bad number".to_string()),
        serde_json::Value::String(s) => eval(s, vars),
        other => Err(format!("expected a number or formula, got {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas() {
        let mut v = BTreeMap::new();
        v.insert("chassis.width".to_string(), 1.2);
        v.insert("k".to_string(), 2.0);
        assert_eq!(eval("chassis.width / 2 + 0.1", &v).unwrap(), 0.7);
        assert_eq!(eval("-(1 + 2) * k", &v).unwrap(), -6.0);
        assert_eq!(eval("max(1, k, 3) - min(4, k)", &v).unwrap(), 1.0);
        assert!((eval("sin(30)", &v).unwrap() - 0.5).abs() < 1e-15);
        assert!(eval("nope + 1", &v).unwrap_err().contains("unknown measure"));
        assert!(eval("(1 + 2", &v).is_err());
    }
}
