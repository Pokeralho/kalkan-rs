//! Pequena linguagem de expressões usada em placeholders, métricas,
//! condições de visibilidade e regras de cor.
//!
//! ```text
//! cpu_temp * 1.8 + 32            → Fahrenheit
//! ram_used_gb / ram_total_gb * 100
//! hour >= 19 || hour < 6         → verdadeiro à noite
//! clamp(gpu_usage * 2, 0, 100)
//! sin(t * 2) * 0.5 + 0.5         → oscila no tempo (t = segundos desde o carregamento)
//! ```
//!
//! Operadores: `+ - * / %`, `< <= > >= == !=`, `&& || !`, parênteses.
//! Funções: `min max abs round floor ceil clamp sin cos sqrt pow if`.
//! Verdadeiro = qualquer valor diferente de 0. Métricas indisponíveis
//! tornam a expressão indisponível (`None`), exibida como `--`.

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add, Sub, Mul, Div, Rem, Lt, Le, Gt, Ge, Eq, Ne, And, Or,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    const OPS: &[&str] = &["<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "<", ">", "!"];
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let st = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                i += 1;
            }
            let t: String = b[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("número inválido '{t}'"))?));
        } else if c.is_alphabetic() || c == '_' {
            let st = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(b[st..i].iter().collect()));
        } else if c == '(' {
            out.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            out.push(Tok::RParen);
            i += 1;
        } else if c == ',' {
            out.push(Tok::Comma);
            i += 1;
        } else {
            let rest: String = b[i..b.len().min(i + 2)].iter().collect();
            match OPS.iter().find(|o| rest.starts_with(**o)) {
                Some(o) => {
                    out.push(Tok::Op(o));
                    i += o.len();
                }
                None => return Err(format!("caractere inesperado '{c}'")),
            }
        }
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    p: usize,
}

impl Parser {
    fn peek_op(&self) -> Option<&'static str> {
        match self.t.get(self.p) {
            Some(Tok::Op(o)) => Some(o),
            _ => None,
        }
    }

    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        const LEVELS: &[&[(&str, Op)]] = &[
            &[("||", Op::Or)],
            &[("&&", Op::And)],
            &[("==", Op::Eq), ("!=", Op::Ne)],
            &[("<", Op::Lt), ("<=", Op::Le), (">", Op::Gt), (">=", Op::Ge)],
            &[("+", Op::Add), ("-", Op::Sub)],
            &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        while let Some(op) = self.peek_op().and_then(|o| LEVELS[level].iter().find(|(s, _)| *s == o)) {
            self.p += 1;
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Bin(op.1, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek_op() {
            Some("-") => {
                self.p += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some("!") => {
                self.p += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some("+") => {
                self.p += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<Expr, String> {
        let tok = self.t.get(self.p).cloned().ok_or("expressão incompleta")?;
        self.p += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Ident(name) => {
                if self.t.get(self.p) == Some(&Tok::LParen) {
                    self.p += 1;
                    let mut args = Vec::new();
                    if self.t.get(self.p) != Some(&Tok::RParen) {
                        loop {
                            args.push(self.binary(0)?);
                            match self.t.get(self.p) {
                                Some(Tok::Comma) => self.p += 1,
                                Some(Tok::RParen) => break,
                                _ => return Err("esperado ',' ou ')'".into()),
                            }
                        }
                    }
                    self.p += 1;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(match name.as_str() {
                        "true" => Expr::Num(1.0),
                        "false" => Expr::Num(0.0),
                        "pi" => Expr::Num(std::f64::consts::PI),
                        _ => Expr::Var(name),
                    })
                }
            }
            Tok::LParen => {
                let e = self.binary(0)?;
                if self.t.get(self.p) != Some(&Tok::RParen) {
                    return Err("')' ausente".into());
                }
                self.p += 1;
                Ok(e)
            }
            other => Err(format!("token inesperado {other:?}")),
        }
    }
}

impl Expr {
    pub fn parse(s: &str) -> Result<Expr, String> {
        let mut p = Parser { t: lex(s)?, p: 0 };
        let e = p.binary(0)?;
        if p.p != p.t.len() {
            return Err(format!("sobrou texto após a expressão em '{s}'"));
        }
        Ok(e)
    }

    /// Se a expressão é só um nome de variável.
    pub fn as_var(&self) -> Option<&str> {
        match self {
            Expr::Var(v) => Some(v),
            _ => None,
        }
    }

    pub fn eval(&self, env: &dyn Fn(&str) -> Option<f64>) -> Option<f64> {
        let b = |v: bool| if v { 1.0 } else { 0.0 };
        Some(match self {
            Expr::Num(n) => *n,
            Expr::Var(v) => env(v)?,
            Expr::Neg(e) => -e.eval(env)?,
            Expr::Not(e) => b(e.eval(env)? == 0.0),
            Expr::Bin(op, l, r) => {
                // Curto-circuito lógico tolera métricas ausentes no lado não avaliado.
                match op {
                    Op::And => return Some(b(l.eval(env)? != 0.0 && r.eval(env)? != 0.0)),
                    Op::Or => {
                        if l.eval(env).is_some_and(|v| v != 0.0) {
                            return Some(1.0);
                        }
                        return Some(b(r.eval(env)? != 0.0));
                    }
                    _ => {}
                }
                let (a, c) = (l.eval(env)?, r.eval(env)?);
                match op {
                    Op::Add => a + c,
                    Op::Sub => a - c,
                    Op::Mul => a * c,
                    Op::Div => {
                        if c == 0.0 {
                            return None;
                        }
                        a / c
                    }
                    Op::Rem => {
                        if c == 0.0 {
                            return None;
                        }
                        a.rem_euclid(c)
                    }
                    Op::Lt => b(a < c),
                    Op::Le => b(a <= c),
                    Op::Gt => b(a > c),
                    Op::Ge => b(a >= c),
                    Op::Eq => b((a - c).abs() < 1e-9),
                    Op::Ne => b((a - c).abs() >= 1e-9),
                    Op::And | Op::Or => unreachable!(),
                }
            }
            Expr::Call(f, args) => {
                if f == "if" && args.len() == 3 {
                    return if args[0].eval(env)? != 0.0 { args[1].eval(env) } else { args[2].eval(env) };
                }
                let v: Option<Vec<f64>> = args.iter().map(|a| a.eval(env)).collect();
                let v = v?;
                match (f.as_str(), v.as_slice()) {
                    ("min", [a, rest @ ..]) => rest.iter().fold(*a, |m, x| m.min(*x)),
                    ("max", [a, rest @ ..]) => rest.iter().fold(*a, |m, x| m.max(*x)),
                    ("abs", [a]) => a.abs(),
                    ("round", [a]) => a.round(),
                    ("round", [a, d]) => {
                        let k = 10f64.powi(*d as i32);
                        (a * k).round() / k
                    }
                    ("floor", [a]) => a.floor(),
                    ("ceil", [a]) => a.ceil(),
                    ("sqrt", [a]) => a.sqrt(),
                    ("pow", [a, e]) => a.powf(*e),
                    ("sin", [a]) => a.sin(),
                    ("cos", [a]) => a.cos(),
                    ("clamp", [x, lo, hi]) => x.clamp(*lo, hi.max(*lo)),
                    ("lerp", [a, c, t]) => a + (c - a) * t,
                    _ => return None,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(k: &str) -> Option<f64> {
        match k {
            "cpu_temp" => Some(50.0),
            "hour" => Some(20.0),
            "gpu" => None,
            _ => None,
        }
    }

    #[test]
    fn arithmetic_and_logic() {
        let ev = |s: &str| Expr::parse(s).unwrap().eval(&env);
        assert_eq!(ev("cpu_temp * 1.8 + 32"), Some(122.0));
        assert_eq!(ev("hour >= 19 || hour < 6"), Some(1.0));
        assert_eq!(ev("clamp(cpu_temp * 3, 0, 100)"), Some(100.0));
        assert_eq!(ev("-(2 + 3) * 2"), Some(-10.0));
        assert_eq!(ev("if(cpu_temp > 80, 1, 2)"), Some(2.0));
        assert_eq!(ev("gpu + 1"), None);
        assert_eq!(ev("1 || gpu"), Some(1.0));
        assert_eq!(ev("10 % 4"), Some(2.0));
        assert!(Expr::parse("1 +").is_err());
        assert!(Expr::parse("(1").is_err());
    }
}
