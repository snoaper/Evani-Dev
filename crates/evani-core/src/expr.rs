//! A tiny, safe expression evaluator for CPU-side formulas.
//!
//! Supports numbers, named variables, `+ - * /`, unary minus, parentheses
//! and the functions `min`, `max`, `clamp`, `mix`, `smoothstep`, `pow`,
//! `abs`. No loops, no assignment, no general scripting.
//!
//! Source is parsed once into compact postfix bytecode; [`Expr::eval`] runs it
//! on a fixed-size stack without allocating. Hard limits on source length,
//! nesting depth, op count and stack depth make hostile input fail at compile
//! time instead of exhausting memory or the call stack. Results are always
//! finite (NaN becomes 0, infinities clamp to +/-`f32::MAX`).

use core::fmt;

pub const MAX_SOURCE_LEN: usize = 2048;
pub const MAX_DEPTH: usize = 32;
pub const MAX_OPS: usize = 256;
pub const MAX_STACK: usize = 32;
pub const MAX_VARS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Const(f32),
    Var(u8),
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Min,
    Max,
    Clamp,
    Mix,
    Smoothstep,
    Pow,
    Abs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    /// Byte offset in the source.
    pub pos: usize,
    pub message: String,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at {}: {}", self.pos, self.message)
    }
}

fn err<T>(pos: usize, message: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError {
        pos,
        message: message.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok<'a> {
    Num(f32),
    Ident(&'a str),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
    End,
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<(Tok<'a>, usize), ExprError> {
        let b = self.src.as_bytes();
        while self.pos < b.len() && b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let start = self.pos;
        let Some(&c) = b.get(self.pos) else {
            return Ok((Tok::End, start));
        };
        let single = match c {
            b'+' => Some(Tok::Plus),
            b'-' => Some(Tok::Minus),
            b'*' => Some(Tok::Star),
            b'/' => Some(Tok::Slash),
            b'(' => Some(Tok::LParen),
            b')' => Some(Tok::RParen),
            b',' => Some(Tok::Comma),
            _ => None,
        };
        if let Some(t) = single {
            self.pos += 1;
            return Ok((t, start));
        }
        if c.is_ascii_digit() || c == b'.' {
            let mut end = self.pos;
            while end < b.len() && (b[end].is_ascii_digit() || b[end] == b'.') {
                end += 1;
            }
            if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
                let mut e = end + 1;
                if e < b.len() && (b[e] == b'+' || b[e] == b'-') {
                    e += 1;
                }
                if e < b.len() && b[e].is_ascii_digit() {
                    while e < b.len() && b[e].is_ascii_digit() {
                        e += 1;
                    }
                    end = e;
                }
            }
            let text = &self.src[start..end];
            self.pos = end;
            return match text.parse::<f32>() {
                Ok(v) if v.is_finite() => Ok((Tok::Num(v), start)),
                _ => err(start, format!("bad number '{text}'")),
            };
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let mut end = self.pos;
            while end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'_') {
                end += 1;
            }
            self.pos = end;
            return Ok((Tok::Ident(&self.src[start..end]), start));
        }
        let ch = self.src[start..].chars().next().unwrap_or('?');
        err(start, format!("unexpected character '{ch}'"))
    }
}

struct Parser<'a, 'v> {
    lex: Lexer<'a>,
    tok: Tok<'a>,
    tok_pos: usize,
    vars: &'v [&'v str],
    ops: Vec<Op>,
    depth: usize,
    stack: usize,
    max_stack: usize,
}

impl<'a> Parser<'a, '_> {
    fn bump(&mut self) -> Result<(), ExprError> {
        let (t, p) = self.lex.next()?;
        self.tok = t;
        self.tok_pos = p;
        Ok(())
    }

    fn emit(&mut self, op: Op, pops: usize, pushes: usize) -> Result<(), ExprError> {
        if self.ops.len() >= MAX_OPS {
            return err(self.tok_pos, format!("expression too long (max {MAX_OPS} ops)"));
        }
        self.ops.push(op);
        self.stack = self.stack - pops + pushes;
        if self.stack > MAX_STACK {
            return err(self.tok_pos, "expression too deeply nested");
        }
        self.max_stack = self.max_stack.max(self.stack);
        Ok(())
    }

    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return err(self.tok_pos, format!("nesting deeper than {MAX_DEPTH}"));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<(), ExprError> {
        self.enter()?;
        self.term()?;
        loop {
            let op = match self.tok {
                Tok::Plus => Op::Add,
                Tok::Minus => Op::Sub,
                _ => break,
            };
            self.bump()?;
            self.term()?;
            self.emit(op, 2, 1)?;
        }
        self.depth -= 1;
        Ok(())
    }

    fn term(&mut self) -> Result<(), ExprError> {
        self.unary()?;
        loop {
            let op = match self.tok {
                Tok::Star => Op::Mul,
                Tok::Slash => Op::Div,
                _ => break,
            };
            self.bump()?;
            self.unary()?;
            self.emit(op, 2, 1)?;
        }
        Ok(())
    }

    fn unary(&mut self) -> Result<(), ExprError> {
        match self.tok {
            Tok::Minus => {
                self.enter()?;
                self.bump()?;
                self.unary()?;
                self.emit(Op::Neg, 1, 1)?;
                self.depth -= 1;
                Ok(())
            }
            Tok::Plus => {
                self.enter()?;
                self.bump()?;
                self.unary()?;
                self.depth -= 1;
                Ok(())
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<(), ExprError> {
        let pos = self.tok_pos;
        match self.tok {
            Tok::Num(v) => {
                self.bump()?;
                self.emit(Op::Const(v), 0, 1)
            }
            Tok::LParen => {
                self.bump()?;
                self.expr()?;
                if self.tok != Tok::RParen {
                    return err(self.tok_pos, "expected ')'");
                }
                self.bump()
            }
            Tok::Ident(name) => {
                self.bump()?;
                if self.tok == Tok::LParen {
                    let (op, arity) = match name {
                        "min" => (Op::Min, 2),
                        "max" => (Op::Max, 2),
                        "clamp" => (Op::Clamp, 3),
                        "mix" => (Op::Mix, 3),
                        "smoothstep" => (Op::Smoothstep, 3),
                        "pow" => (Op::Pow, 2),
                        "abs" => (Op::Abs, 1),
                        _ => return err(pos, format!("unknown function '{name}'")),
                    };
                    self.enter()?;
                    self.bump()?;
                    for i in 0..arity {
                        if i > 0 {
                            if self.tok != Tok::Comma {
                                return err(
                                    self.tok_pos,
                                    format!("'{name}' takes {arity} arguments"),
                                );
                            }
                            self.bump()?;
                        }
                        self.expr()?;
                    }
                    if self.tok != Tok::RParen {
                        return err(self.tok_pos, format!("'{name}' takes {arity} arguments"));
                    }
                    self.bump()?;
                    self.depth -= 1;
                    self.emit(op, arity, 1)
                } else {
                    match self.vars.iter().position(|v| *v == name) {
                        Some(i) => self.emit(Op::Var(i as u8), 0, 1),
                        None => err(pos, format!("unknown variable '{name}'")),
                    }
                }
            }
            Tok::End => err(pos, "unexpected end of expression"),
            _ => err(pos, "expected a number, variable, function or '('"),
        }
    }
}

/// A compiled expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    source: String,
    vars: Vec<String>,
    ops: Vec<Op>,
}

impl Expr {
    /// Compile `source`. `vars` names the variables it may use; at eval time
    /// values are passed in the same order.
    pub fn compile(source: &str, vars: &[&str]) -> Result<Self, ExprError> {
        if source.len() > MAX_SOURCE_LEN {
            return err(0, format!("source longer than {MAX_SOURCE_LEN} bytes"));
        }
        if vars.len() > MAX_VARS {
            return err(0, format!("more than {MAX_VARS} variables"));
        }
        let mut p = Parser {
            lex: Lexer { src: source, pos: 0 },
            tok: Tok::End,
            tok_pos: 0,
            vars,
            ops: Vec::new(),
            depth: 0,
            stack: 0,
            max_stack: 0,
        };
        p.bump()?;
        p.expr()?;
        if p.tok != Tok::End {
            return err(p.tok_pos, "unexpected input after expression");
        }
        debug_assert_eq!(p.stack, 1);
        Ok(Self {
            source: source.to_string(),
            vars: vars.iter().map(|s| s.to_string()).collect(),
            ops: p.ops,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn vars(&self) -> &[String] {
        &self.vars
    }

    /// Evaluate with variable values in declaration order (missing ones are
    /// 0). Never allocates; always returns a finite number.
    pub fn eval(&self, vars: &[f32]) -> f32 {
        let mut st = [0.0f32; MAX_STACK];
        let mut sp = 0usize;
        // The compiler guarantees stack discipline; indexing stays checked.
        for op in &self.ops {
            match *op {
                Op::Const(v) => {
                    st[sp] = v;
                    sp += 1;
                }
                Op::Var(i) => {
                    st[sp] = vars.get(i as usize).copied().unwrap_or(0.0);
                    sp += 1;
                }
                Op::Neg => st[sp - 1] = -st[sp - 1],
                Op::Abs => st[sp - 1] = st[sp - 1].abs(),
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Min | Op::Max | Op::Pow => {
                    let b = st[sp - 1];
                    let a = st[sp - 2];
                    sp -= 1;
                    st[sp - 1] = match *op {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        Op::Div => a / b,
                        Op::Min => a.min(b),
                        Op::Max => a.max(b),
                        _ => a.powf(b),
                    };
                }
                Op::Clamp | Op::Mix | Op::Smoothstep => {
                    let c = st[sp - 1];
                    let b = st[sp - 2];
                    let a = st[sp - 3];
                    sp -= 2;
                    st[sp - 1] = match *op {
                        Op::Clamp => a.max(b).min(c),
                        Op::Mix => a + (b - a) * c,
                        _ => smoothstep(a, b, c),
                    };
                }
            }
        }
        let r = if sp > 0 { st[sp - 1] } else { 0.0 };
        if r.is_nan() {
            0.0
        } else {
            r.clamp(-f32::MAX, f32::MAX)
        }
    }
}

/// GLSL/WGSL `smoothstep(edge0, edge1, x)`.
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 == e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str, vars: &[(&str, f32)]) -> f32 {
        let names: Vec<&str> = vars.iter().map(|v| v.0).collect();
        let vals: Vec<f32> = vars.iter().map(|v| v.1).collect();
        Expr::compile(src, &names).unwrap().eval(&vals)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("1 + 2 * 3", &[]), 7.0);
        assert_eq!(ev("(1 + 2) * 3", &[]), 9.0);
        assert_eq!(ev("10 - 4 - 3", &[]), 3.0);
        assert_eq!(ev("8 / 4 / 2", &[]), 1.0);
        assert_eq!(ev("-2 * -3", &[]), 6.0);
        assert_eq!(ev("--2", &[]), 2.0);
        assert_eq!(ev("+2", &[]), 2.0);
        assert_eq!(ev("1.5e2 + .5", &[]), 150.5);
        assert_eq!(ev("2 * -x", &[("x", 4.0)]), -8.0);
    }

    #[test]
    fn functions() {
        assert_eq!(ev("min(3, x)", &[("x", 1.0)]), 1.0);
        assert_eq!(ev("max(3, x)", &[("x", 1.0)]), 3.0);
        assert_eq!(ev("clamp(x, 0, 1)", &[("x", 7.0)]), 1.0);
        assert_eq!(ev("mix(10, 20, 0.25)", &[]), 12.5);
        assert_eq!(ev("smoothstep(0, 1, 0.5)", &[]), 0.5);
        assert_eq!(ev("smoothstep(0, 1, 2)", &[]), 1.0);
        assert_eq!(ev("smoothstep(1, 1, 2)", &[]), 1.0);
        assert_eq!(ev("pow(2, 10)", &[]), 1024.0);
        assert_eq!(ev("abs(-3)", &[]), 3.0);
        assert_eq!(
            ev(
                "mix(near, far, smoothstep(0, 100, dist))",
                &[("near", 1.0), ("far", 3.0), ("dist", 100.0)]
            ),
            3.0
        );
    }

    #[test]
    fn results_are_always_finite() {
        assert_eq!(ev("1 / 0", &[]), f32::MAX);
        assert_eq!(ev("-1 / 0", &[]), -f32::MAX);
        assert_eq!(ev("0 / 0", &[]), 0.0);
        assert_eq!(ev("pow(-1, 0.5)", &[]), 0.0);
        let e = Expr::compile("x + y", &["x", "y"]).unwrap();
        assert_eq!(e.eval(&[1.0]), 1.0, "missing vars are 0");
    }

    #[test]
    fn errors_have_positions() {
        let e = Expr::compile("1 + foo", &[]).unwrap_err();
        assert_eq!(e.pos, 4);
        assert!(e.message.contains("foo"));
        for bad in [
            "", "1 +", "(1", "1)", "min(1)", "min(1,2,3)", "nope(1)", "1 $ 2", "2 3", "1e", "..1",
            "x(1)", "min 1",
        ] {
            assert!(Expr::compile(bad, &["x"]).is_err(), "{bad:?}");
        }
        assert!(Expr::compile("1e", &[]).is_err());
    }

    #[test]
    fn hostile_input_is_bounded() {
        // Deep parentheses.
        let deep = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
        assert!(Expr::compile(&deep, &[]).is_err());
        // Just under the limit still works.
        let ok = format!("{}1{}", "(".repeat(MAX_DEPTH - 1), ")".repeat(MAX_DEPTH - 1));
        assert_eq!(Expr::compile(&ok, &[]).unwrap().eval(&[]), 1.0);
        // Long unary chains.
        assert!(Expr::compile(&"-".repeat(1000), &[]).is_err());
        assert!(Expr::compile(&format!("{}1", "-".repeat(500)), &[]).is_err());
        // Very long sources.
        assert!(Expr::compile(&"1+".repeat(5000), &[]).is_err());
        // Many ops within the length limit.
        let long = format!("1{}", "+1".repeat(600));
        assert!(long.len() < MAX_SOURCE_LEN);
        assert!(Expr::compile(&long, &[]).is_err());
        // Stack depth via right-nested function calls.
        let mut s = String::from("1");
        for _ in 0..40 {
            s = format!("max(1, {s})");
        }
        assert!(Expr::compile(&s, &[]).is_err());
        // Right-leaning a+(b+(c+...)) grows the stack.
        let mut s = String::from("1");
        for _ in 0..40 {
            s = format!("1+({s})");
        }
        assert!(Expr::compile(&s, &[]).is_err());
        // Too many variables, non-ASCII input.
        let names: Vec<String> = (0..17).map(|i| format!("v{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        assert!(Expr::compile("1", &refs).is_err());
        assert!(Expr::compile("1 + é", &[]).is_err());
        assert!(Expr::compile("1e99999", &[]).is_err());
    }

    #[test]
    fn compiled_expr_keeps_source() {
        let e = Expr::compile("x * 2", &["x"]).unwrap();
        assert_eq!(e.source(), "x * 2");
        assert_eq!(e.vars(), &["x".to_string()]);
        assert_eq!(e.eval(&[21.0]), 42.0);
    }
}
