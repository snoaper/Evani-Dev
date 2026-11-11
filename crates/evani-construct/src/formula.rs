//! Compiled formulas for recipes: the same tokens as [`crate::expr`]
//! (numbers, dotted names, `+ - * /`, parentheses, unary minus, calls), parsed
//! once into a tree and evaluated many times against a slot array, so a
//! surface sampled thousands of times never re-parses its formula.
//!
//! Functions: `min`, `max`, `abs`, `sqrt`, `sin`, `cos`, `tan` (degrees, as in
//! `expr`), `pow`, `exp`, `ln`, `floor`, `clamp(x, lo, hi)`, `lerp(a, b, t)`,
//! `smooth(e0, e1, x)` (smoothstep) and `sign`. `pi` is a constant. All on
//! [`crate::dmath`] and IEEE basic operations, in source order: the same bits
//! on every machine.

use crate::dmath;
use crate::expr::{T, lex};

#[derive(Clone, Copy, Debug, PartialEq)]
enum F {
    Min,
    Max,
    Abs,
    Sqrt,
    Sin,
    Cos,
    Tan,
    Pow,
    Exp,
    Ln,
    Floor,
    Clamp,
    Lerp,
    Smooth,
    Sign,
}

#[derive(Clone, Debug, PartialEq)]
enum N {
    Num(f64),
    Var(usize),
    Neg(Box<N>),
    Bin(char, Box<N>, Box<N>),
    Call(F, Vec<N>),
}

/// A parsed formula over named slots.
#[derive(Clone, Debug, PartialEq)]
pub struct Formula {
    pub src: String,
    root: N,
}

struct P<'a> {
    t: Vec<T>,
    i: usize,
    names: &'a [String],
    src: &'a str,
}

impl P<'_> {
    fn expr(&mut self) -> Result<N, String> {
        let mut v = self.term()?;
        while let Some(T::Op(o)) = self.t.get(self.i).cloned() {
            if o != '+' && o != '-' {
                break;
            }
            self.i += 1;
            v = N::Bin(o, Box::new(v), Box::new(self.term()?));
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<N, String> {
        let mut v = self.unary()?;
        while let Some(T::Op(o)) = self.t.get(self.i).cloned() {
            if o != '*' && o != '/' {
                break;
            }
            self.i += 1;
            v = N::Bin(o, Box::new(v), Box::new(self.unary()?));
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<N, String> {
        if self.t.get(self.i) == Some(&T::Op('-')) {
            self.i += 1;
            return Ok(N::Neg(Box::new(self.unary()?)));
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<N, String> {
        let tok = self.t.get(self.i).cloned();
        self.i += 1;
        match tok {
            Some(T::Num(n)) => Ok(N::Num(n)),
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
                    let (f, arity) = match name.as_str() {
                        "min" => (F::Min, 0),
                        "max" => (F::Max, 0),
                        "abs" => (F::Abs, 1),
                        "sqrt" => (F::Sqrt, 1),
                        "sin" => (F::Sin, 1),
                        "cos" => (F::Cos, 1),
                        "tan" => (F::Tan, 1),
                        "pow" => (F::Pow, 2),
                        "exp" => (F::Exp, 1),
                        "ln" => (F::Ln, 1),
                        "floor" => (F::Floor, 1),
                        "clamp" => (F::Clamp, 3),
                        "lerp" => (F::Lerp, 3),
                        "smooth" => (F::Smooth, 3),
                        "sign" => (F::Sign, 1),
                        _ => return Err(format!("unknown function {name} in `{}`", self.src)),
                    };
                    if arity != 0 && args.len() != arity {
                        return Err(format!("{name} takes {arity} argument(s) in `{}`", self.src));
                    }
                    return Ok(N::Call(f, args));
                }
                if name == "pi" {
                    return Ok(N::Num(core::f64::consts::PI));
                }
                match self.names.iter().position(|n| *n == name) {
                    Some(k) => Ok(N::Var(k)),
                    None => Err(format!("unknown name `{name}` in `{}` (known: {})", self.src, self.names.join(", "))),
                }
            }
            _ => Err(format!("cannot read `{}`", self.src)),
        }
    }
}

fn ev(n: &N, v: &[f64]) -> f64 {
    match n {
        N::Num(x) => *x,
        N::Var(k) => v.get(*k).copied().unwrap_or(0.0),
        N::Neg(a) => -ev(a, v),
        N::Bin(o, a, b) => {
            let (a, b) = (ev(a, v), ev(b, v));
            match o {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                _ => a / b,
            }
        }
        N::Call(f, args) => {
            let a: Vec<f64> = args.iter().map(|x| ev(x, v)).collect();
            let x = a.first().copied().unwrap_or(0.0);
            let y = a.get(1).copied().unwrap_or(0.0);
            let z = a.get(2).copied().unwrap_or(0.0);
            match f {
                F::Min => a.iter().skip(1).fold(x, |m, q| m.min(*q)),
                F::Max => a.iter().skip(1).fold(x, |m, q| m.max(*q)),
                F::Abs => x.abs(),
                F::Sqrt => x.max(0.0).sqrt(),
                F::Sin => dmath::sin(dmath::deg(x)),
                F::Cos => dmath::cos(dmath::deg(x)),
                F::Tan => dmath::tan(dmath::deg(x)),
                F::Pow => {
                    if x <= 0.0 {
                        if x == 0.0 { 0.0 } else { -dmath::pow(-x, y) }
                    } else {
                        dmath::pow(x, y)
                    }
                }
                F::Exp => dmath::exp(x),
                F::Ln => dmath::ln(x.max(1e-300)),
                F::Floor => x.floor(),
                F::Clamp => x.max(y).min(z),
                F::Lerp => x + (y - x) * z,
                F::Smooth => {
                    let t = if (y - x).abs() < 1e-300 { if z < x { 0.0 } else { 1.0 } } else { ((z - x) / (y - x)).clamp(0.0, 1.0) };
                    t * t * (3.0 - 2.0 * t)
                }
                F::Sign => {
                    if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
            }
        }
    }
}

impl Formula {
    /// Parse `src` against the slot names (index = slot).
    pub fn parse(src: &str, names: &[String]) -> Result<Formula, String> {
        let t = lex(src)?;
        let mut p = P { t, i: 0, names, src };
        let root = p.expr()?;
        if p.i != p.t.len() {
            return Err(format!("unexpected text in `{src}`"));
        }
        Ok(Formula { src: src.to_string(), root })
    }
    /// A constant formula.
    pub fn constant(x: f64) -> Formula {
        Formula { src: format!("{x}"), root: N::Num(x) }
    }
    /// Evaluate with the slot values (a missing slot reads 0).
    pub fn eval(&self, vals: &[f64]) -> f64 {
        ev(&self.root, vals)
    }
    /// The value when the formula uses no slots.
    pub fn as_constant(&self) -> Option<f64> {
        fn pure(n: &N) -> bool {
            match n {
                N::Num(_) => true,
                N::Var(_) => false,
                N::Neg(a) => pure(a),
                N::Bin(_, a, b) => pure(a) && pure(b),
                N::Call(_, a) => a.iter().all(pure),
            }
        }
        pure(&self.root).then(|| ev(&self.root, &[]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_formulas_match_expr() {
        let names: Vec<String> = ["a", "hull.beam", "t"].iter().map(|s| s.to_string()).collect();
        let f = Formula::parse("hull.beam / 2 * (1 - pow(abs(t), 2)) + min(a, 3) - sin(30)", &names).unwrap();
        let got = f.eval(&[2.0, 8.0, 0.5]);
        assert!((got - (4.0 * 0.75 + 2.0 - 0.5)).abs() < 1e-12, "{got}");
        let mut vars = std::collections::BTreeMap::new();
        vars.insert("a".to_string(), 2.0);
        vars.insert("k".to_string(), 3.0);
        let e = crate::expr::eval("-(1 + a) * k / 4 + max(1, 2, 3)", &vars).unwrap();
        let names2: Vec<String> = ["a", "k"].iter().map(|s| s.to_string()).collect();
        let c = Formula::parse("-(1 + a) * k / 4 + max(1, 2, 3)", &names2).unwrap();
        assert_eq!(c.eval(&[2.0, 3.0]).to_bits(), e.to_bits());
        assert!(Formula::parse("b + 1", &names).unwrap_err().contains("unknown name"));
        assert!(Formula::parse("clamp(1, 2)", &names).is_err());
        assert_eq!(Formula::parse("2 * pi", &names).unwrap().as_constant(), Some(2.0 * core::f64::consts::PI));
        assert_eq!(Formula::parse("smooth(0, 1, 0.5)", &names).unwrap().as_constant(), Some(0.5));
    }
}
