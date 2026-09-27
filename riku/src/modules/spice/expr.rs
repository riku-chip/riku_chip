//! Expresiones sobre las señales de un análisis, con la sintaxis de ngspice:
//! `v(out)/v(in)`, `db(v(out))`, `deriv(v(out))`, `integ(i(v1))[-1]`,
//! `gain = v(out)/v(in)`.
//!
//! Una expresión se evalúa punto a punto sobre las variables de **un**
//! análisis (todas comparten el eje X, así que no hace falta interpolar). El
//! resultado es una señal (un valor por punto) o un escalar (`max(v(out))`,
//! `v(out)[0]`, `at(v(out), 1u)`), y se compara entre versiones igual que las
//! señales del archivo.
//!
//! Los análisis complejos (`ac`) se evalúan con los valores complejos: en
//! `v(out)/v(in)` se dividen complejos y se muestra la magnitud en dB.
//!
//! | Qué | Sintaxis |
//! |---|---|
//! | Señales | `v(out)`, `v(a,b)` (= `v(a) − v(b)`), `i(v1)`, `@m1[id]`, `time`, `out` (= `v(out)`) |
//! | Números | `1.5`, `1e-9`, sufijos SPICE `f p n u m k meg g t` (`10u`, `2meg`) |
//! | Operadores | `+ − * / ^`, paréntesis, `−x` |
//! | Por punto | `abs mag real imag ph db sqrt exp ln log log10 sin cos tan atan`, `min(a,b)`, `max(a,b)` |
//! | Cálculo | `deriv(v)` (derivada respecto del eje), `integ(v)` (integral acumulada) |
//! | Escalares | `max(v) min(v) mean(v) rms(v) pp(v) integral(v) length(v)`, `at(v, x)` |
//! | Índices | `v[0]`, `v[-1]` (último), `v[10:20]` (tramo, el resto se ignora), `window(v, x0, x1)` |
//!
//! `log` es base 10 y `ln` natural, como en ngspice; `ph` da grados.

use std::fmt;

use super::raw::{Plot, Variable};

// ─── Números complejos ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    pub const fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    const NAN: C = C::real(f64::NAN);

    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    fn is_real(self) -> bool {
        self.im == 0.0
    }

    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }

    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }

    fn mul(self, o: C) -> C {
        C::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }

    fn div(self, o: C) -> C {
        if self.is_real() && o.is_real() {
            return C::real(self.re / o.re);
        }
        let d = o.re * o.re + o.im * o.im;
        C::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }

    fn scale(self, k: f64) -> C {
        C::new(self.re * k, self.im * k)
    }

    fn exp(self) -> C {
        let m = self.re.exp();
        C::new(m * self.im.cos(), m * self.im.sin())
    }

    fn ln(self) -> C {
        C::new(self.abs().ln(), self.arg())
    }

    fn sqrt(self) -> C {
        if self.is_real() && self.re >= 0.0 {
            return C::real(self.re.sqrt());
        }
        let m = self.abs().sqrt();
        let a = self.arg() / 2.0;
        C::new(m * a.cos(), m * a.sin())
    }

    fn pow(self, e: C) -> C {
        if self.is_real() && e.is_real() && (self.re >= 0.0 || e.re.fract() == 0.0) {
            return C::real(self.re.powf(e.re));
        }
        if self.re == 0.0 && self.im == 0.0 {
            return C::real(0.0);
        }
        e.mul(self.ln()).exp()
    }

    fn sin(self) -> C {
        C::new(self.re.sin() * self.im.cosh(), self.re.cos() * self.im.sinh())
    }

    fn cos(self) -> C {
        C::new(self.re.cos() * self.im.cosh(), -self.re.sin() * self.im.sinh())
    }

    fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

// ─── Errores ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum ExprError {
    /// La expresión está mal escrita (vale para cualquier análisis).
    Syntax(String),
    /// Falta una señal en este análisis: la expresión no se aplica a él.
    Missing(String),
    /// Se pudo leer pero no calcular (índice fuera de rango, tamaños…).
    Eval(String),
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(m) => write!(f, "expresión mal escrita: {m}"),
            Self::Missing(n) => write!(f, "no existe la señal {n}"),
            Self::Eval(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ExprError {}

// ─── Árbol ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Num(f64),
    /// Señal por nombre (`v(out)`, `time`, `@m1[id]`) o nodo a secas (`out`).
    Var(String),
    /// `v(a,b)`.
    VDiff(String, String),
    Neg(Box<Node>),
    Bin(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
    Index(Box<Node>, Box<Node>),
    Slice(Box<Node>, Box<Node>, Box<Node>),
}

/// Una expresión lista para evaluar.
#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    /// Nombre para mostrar: el de `nombre = …` o el texto.
    pub name: String,
    /// Texto original (sin el nombre ni el análisis).
    pub text: String,
    /// Análisis al que se limita (`tran: …`), o `None` = todos donde existan
    /// sus señales.
    pub analysis: Option<&'static str>,
    node: Node,
}

/// Prefijos de análisis (como los de ngspice) y la palabra que los
/// identifica en el `Plotname:` del `.raw`.
const ANALYSES: [(&str, &str); 9] = [
    ("op", "operating point"),
    ("tran", "transient"),
    ("ac", "ac analysis"),
    ("dc", "dc transfer"),
    ("noise", "noise"),
    ("tf", "transfer function"),
    ("sens", "sensitivity"),
    ("pz", "pole-zero"),
    ("disto", "distortion"),
];

impl Expression {
    /// `true` si la expresión se evalúa en el análisis `plot_name`.
    pub fn applies_to(&self, plot_name: &str) -> bool {
        let Some(a) = self.analysis else { return true };
        let key = ANALYSES.iter().find(|(p, _)| *p == a).map_or(a, |(_, k)| *k);
        plot_name.to_ascii_lowercase().contains(key)
    }
}

/// Resultado de evaluar sobre un análisis.
#[derive(Clone, Debug, PartialEq)]
pub enum Evaluated {
    /// Una señal alineada con el eje X del análisis (puntos fuera de un
    /// tramo o no calculables quedan en NaN y no se comparan).
    Signal(Variable),
    /// Un número (con su unidad, vacía si no se conoce).
    Scalar(f64, &'static str),
}

// ─── Lectura ───────────────────────────────────────────────────────────────

pub fn parse(input: &str) -> Result<Expression, ExprError> {
    let input = input.trim();
    // `tran: …`, `ac: …`: limitar a un análisis.
    let (analysis, input) = match input.split_once(':') {
        Some((pre, rest)) => match ANALYSES.iter().find(|(p, _)| pre.trim().eq_ignore_ascii_case(p)) {
            Some((p, _)) => (Some(*p), rest.trim()),
            None => (None, input),
        },
        None => (None, input),
    };
    // `nombre = expresión` (un solo `=`, con un identificador simple a la izquierda).
    let (name, text) = match input.split_once('=') {
        Some((l, r)) if !r.starts_with('=') && is_plain_ident(l.trim()) => (Some(l.trim().to_string()), r.trim()),
        _ => (None, input),
    };
    if text.is_empty() {
        return Err(ExprError::Syntax("vacía".into()));
    }
    let mut p = Parser { s: text.chars().collect(), i: 0 };
    let node = p.expr()?;
    p.ws();
    if p.i < p.s.len() {
        return Err(ExprError::Syntax(format!("sobra «{}»", p.s[p.i..].iter().collect::<String>())));
    }
    Ok(Expression { name: name.unwrap_or_else(|| text.to_string()), text: text.to_string(), analysis, node })
}

fn is_plain_ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

fn syntax<T>(m: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError::Syntax(m.into()))
}

impl Parser {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), ExprError> {
        if self.eat(c) { Ok(()) } else { syntax(format!("falta «{c}»")) }
    }

    fn expr(&mut self) -> Result<Node, ExprError> {
        let mut left = self.term()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            left = Node::Bin(op, Box::new(left), Box::new(self.term()?));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Node, ExprError> {
        let mut left = self.unary()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.i += 1;
            left = Node::Bin(op, Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, ExprError> {
        if self.eat('-') {
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Node, ExprError> {
        let base = self.postfix()?;
        if self.eat('^') {
            // Asociativa a la derecha: 2^3^2 = 2^(3^2).
            return Ok(Node::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Node, ExprError> {
        let mut node = self.primary()?;
        while self.eat('[') {
            let a = self.expr()?;
            if self.eat(':') {
                let b = self.expr()?;
                self.expect(']')?;
                node = Node::Slice(Box::new(node), Box::new(a), Box::new(b));
            } else {
                self.expect(']')?;
                node = Node::Index(Box::new(node), Box::new(a));
            }
        }
        Ok(node)
    }

    fn primary(&mut self) -> Result<Node, ExprError> {
        let Some(c) = self.peek() else { return syntax("termina antes de tiempo") };
        if c.is_ascii_digit() || (c == '.' && self.s.get(self.i + 1).is_some_and(char::is_ascii_digit)) {
            return self.number();
        }
        if c == '(' {
            self.i += 1;
            let n = self.expr()?;
            self.expect(')')?;
            return Ok(n);
        }
        if c == '@' {
            return Ok(Node::Var(self.device_param()));
        }
        if c.is_alphabetic() || c == '_' {
            let ident = self.ident();
            if self.peek() == Some('(') {
                let lower = ident.to_ascii_lowercase();
                if lower == "v" || lower == "i" {
                    return self.probe(&lower);
                }
                self.i += 1;
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(')') {
                            break;
                        }
                        self.expect(',')?;
                    }
                }
                return Ok(Node::Call(lower, args));
            }
            return Ok(match ident.to_ascii_lowercase().as_str() {
                "pi" => Node::Num(std::f64::consts::PI),
                "e" => Node::Num(std::f64::consts::E),
                _ => Node::Var(ident),
            });
        }
        syntax(format!("no se esperaba «{c}»"))
    }

    fn ident(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_alphanumeric() || matches!(self.s[self.i], '_' | '.' | '#' | '$')) {
            self.i += 1;
        }
        self.s[start..self.i].iter().collect()
    }

    /// `@m1[id]`, `@m.xm1.msky130_fd_pr__nfet_01v8[gm]`: hasta un operador
    /// fuera de corchetes.
    fn device_param(&mut self) -> String {
        let start = self.i;
        let mut depth = 0;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            match c {
                '[' => depth += 1,
                ']' if depth > 0 => depth -= 1,
                _ if depth == 0 && (c.is_whitespace() || "+-*/^(),:]".contains(c)) => break,
                _ => {}
            }
            self.i += 1;
            if depth == 0 && c == ']' {
                break;
            }
        }
        self.s[start..self.i].iter().collect()
    }

    /// `v(nodo)`, `v(a, b)`, `i(fuente)`: los argumentos son nombres, no
    /// expresiones (pueden tener `.`, `#`, `[`…).
    fn probe(&mut self, kind: &str) -> Result<Node, ExprError> {
        self.expect('(')?;
        let start = self.i;
        let mut depth = 0;
        while self.i < self.s.len() {
            match self.s[self.i] {
                '(' => depth += 1,
                ')' if depth == 0 => break,
                ')' => depth -= 1,
                _ => {}
            }
            self.i += 1;
        }
        let inner: String = self.s[start..self.i].iter().collect();
        self.expect(')')?;
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        match (kind, parts.as_slice()) {
            (_, [n]) if !n.is_empty() => Ok(Node::Var(format!("{kind}({n})"))),
            ("v", [a, b]) if !a.is_empty() && !b.is_empty() => Ok(Node::VDiff(a.to_string(), b.to_string())),
            _ => syntax(format!("{kind}({inner}) no es una señal")),
        }
    }

    /// Número con exponente y sufijo SPICE (`1.5`, `1e-9`, `10u`, `2meg`,
    /// `100nF`: las letras después del sufijo se ignoran, como en SPICE).
    fn number(&mut self) -> Result<Node, ExprError> {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == '.') {
            self.i += 1;
        }
        if self.i < self.s.len() && matches!(self.s[self.i], 'e' | 'E') {
            let mut j = self.i + 1;
            if j < self.s.len() && matches!(self.s[j], '+' | '-') {
                j += 1;
            }
            if j < self.s.len() && self.s[j].is_ascii_digit() {
                while j < self.s.len() && self.s[j].is_ascii_digit() {
                    j += 1;
                }
                self.i = j;
            }
        }
        let text: String = self.s[start..self.i].iter().collect();
        let mut v: f64 = text.parse().map_err(|_| ExprError::Syntax(format!("número inválido «{text}»")))?;
        let letters_start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_alphabetic()) {
            self.i += 1;
        }
        let letters: String = self.s[letters_start..self.i].iter().collect::<String>().to_lowercase();
        let mult = if letters.starts_with("meg") {
            1e6
        } else if letters.starts_with("mil") {
            25.4e-6
        } else {
            match letters.chars().next() {
                Some('t') => 1e12,
                Some('g') => 1e9,
                Some('k') => 1e3,
                Some('m') => 1e-3,
                Some('u') | Some('µ') => 1e-6,
                Some('n') => 1e-9,
                Some('p') => 1e-12,
                Some('f') => 1e-15,
                _ => 1.0,
            }
        };
        v *= mult;
        Ok(Node::Num(v))
    }
}

// ─── Evaluación ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Val {
    Scalar(C),
    Vector(Vec<C>),
}

struct Ctx<'a> {
    plot: &'a Plot,
    x: Vec<f64>,
}

fn eval_err<T>(m: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError::Eval(m.into()))
}

impl Expression {
    /// Evalúa sobre un análisis. [`ExprError::Missing`] indica que el
    /// análisis no tiene alguna señal de la expresión.
    pub fn eval(&self, plot: &Plot) -> Result<Evaluated, ExprError> {
        let x = plot.x().map(|v| v.values.clone()).unwrap_or_default();
        let ctx = Ctx { plot, x };
        let val = ctx.eval(&self.node)?;
        let kind = kind_of(&self.node, plot);
        match val {
            Val::Scalar(c) => {
                let v = if c.is_real() { c.re } else { c.abs() };
                Ok(Evaluated::Scalar(v, if c.is_real() { kind.map_or("", kind_unit) } else { "" }))
            }
            Val::Vector(vs) => {
                // Complejo de verdad (AC): se muestra en dB, como las señales del archivo.
                let complex = plot.complex && !matches!(kind, Some("db" | "phase")) && vs.iter().any(|c| c.im != 0.0);
                let values = vs
                    .iter()
                    .map(|c| {
                        let c = if c.is_finite() { *c } else { C::NAN };
                        if complex { 20.0 * c.abs().max(1e-300).log10() } else { c.re }
                    })
                    .collect();
                let kind = match kind {
                    Some(k) => k,
                    None if complex => "complex",
                    None => "expression",
                };
                Ok(Evaluated::Signal(Variable {
                    name: self.name.clone(),
                    kind: kind.to_string(),
                    values,
                    complex: complex.then(|| vs.iter().map(|c| (c.re, c.im)).collect()),
                }))
            }
        }
    }
}

/// Tipo (y así la unidad) del resultado cuando se puede deducir: el de la
/// señal referida, conservado por escalas, sumas del mismo tipo, índices y
/// reducciones; `db` y `ph` tienen el suyo. `None` = sin unidad conocida.
fn kind_of(n: &Node, plot: &Plot) -> Option<&'static str> {
    match n {
        Node::Var(name) => resolve(plot, name).and_then(|v| match v.kind.as_str() {
            "voltage" => Some("voltage"),
            "current" => Some("current"),
            "time" => Some("time"),
            "frequency" => Some("frequency"),
            _ => None,
        }),
        Node::VDiff(..) => Some("voltage"),
        Node::Neg(a) | Node::Index(a, _) | Node::Slice(a, _, _) => kind_of(a, plot),
        Node::Bin('+' | '-', a, b) => kind_of(a, plot).filter(|k| Some(*k) == kind_of(b, plot)),
        Node::Bin('*', a, b) => match (a.as_ref(), b.as_ref()) {
            (Node::Num(_), x) | (x, Node::Num(_)) => kind_of(x, plot),
            _ => None,
        },
        Node::Bin('/', a, b) if matches!(b.as_ref(), Node::Num(_)) => kind_of(a, plot),
        Node::Call(f, args) => match f.as_str() {
            "db" => Some("db"),
            "ph" | "phase" => Some("phase"),
            "abs" | "mag" | "real" | "re" | "max" | "min" | "mean" | "avg" | "rms" | "pp" | "at" | "window" => {
                args.first().and_then(|a| kind_of(a, plot))
            }
            _ => None,
        },
        _ => None,
    }
}

fn kind_unit(kind: &str) -> &'static str {
    match kind {
        "voltage" => "V",
        "current" => "A",
        "time" => "s",
        "frequency" => "Hz",
        "db" => "dB",
        "phase" => "°",
        _ => "",
    }
}

/// Busca una señal: nombre exacto (sin distinguir mayúsculas) y, si es un
/// nodo a secas, `v(nodo)`.
fn resolve<'a>(plot: &'a Plot, name: &str) -> Option<&'a Variable> {
    let find = |n: &str| plot.vars.iter().find(|v| v.name.eq_ignore_ascii_case(n));
    find(name).or_else(|| (!name.contains('(') && !name.starts_with('@')).then(|| find(&format!("v({name})"))).flatten())
}

fn var_values(v: &Variable) -> Vec<C> {
    match &v.complex {
        Some(pairs) => pairs.iter().map(|&(re, im)| C::new(re, im)).collect(),
        None => v.values.iter().map(|&r| C::real(r)).collect(),
    }
}

impl Ctx<'_> {
    fn eval(&self, n: &Node) -> Result<Val, ExprError> {
        match n {
            Node::Num(v) => Ok(Val::Scalar(C::real(*v))),
            Node::Var(name) => {
                let v = resolve(self.plot, name).ok_or_else(|| ExprError::Missing(name.clone()))?;
                Ok(Val::Vector(var_values(v)))
            }
            Node::VDiff(a, b) => {
                let a = self.eval(&Node::Var(format!("v({a})")))?;
                let b = self.eval(&Node::Var(format!("v({b})")))?;
                binary('-', a, b)
            }
            Node::Neg(a) => Ok(map(self.eval(a)?, |c| c.scale(-1.0))),
            Node::Bin(op, a, b) => binary(*op, self.eval(a)?, self.eval(b)?),
            Node::Index(v, i) => {
                let i = self.index(i)?;
                match self.eval(v)? {
                    Val::Scalar(c) if i == 0 || i == -1 => Ok(Val::Scalar(c)),
                    Val::Scalar(_) => eval_err("índice sobre un número"),
                    Val::Vector(vs) => {
                        let k = wrap(i, vs.len()).ok_or_else(|| ExprError::Eval(format!("índice {i} fuera de rango (0..{})", vs.len())))?;
                        Ok(Val::Scalar(vs[k]))
                    }
                }
            }
            Node::Slice(v, a, b) => {
                let (a, b) = (self.index(a)?, self.index(b)?);
                let Val::Vector(vs) = self.eval(v)? else { return eval_err("tramo sobre un número") };
                let n = vs.len();
                let (Some(lo), Some(hi)) = (wrap(a, n), wrap(b, n)) else {
                    return eval_err(format!("tramo [{a}:{b}] fuera de rango (0..{n})"));
                };
                Ok(Val::Vector(vs.iter().enumerate().map(|(k, c)| if k >= lo && k <= hi { *c } else { C::NAN }).collect()))
            }
            Node::Call(f, args) => self.call(f, args),
        }
    }

    fn index(&self, n: &Node) -> Result<i64, ExprError> {
        match self.eval(n)? {
            Val::Scalar(c) if c.is_real() && c.re.is_finite() => Ok(c.re.round() as i64),
            _ => eval_err("el índice debe ser un número"),
        }
    }

    fn scalar(&self, n: &Node) -> Result<f64, ExprError> {
        match self.eval(n)? {
            Val::Scalar(c) => Ok(c.re),
            Val::Vector(_) => eval_err("se esperaba un número, no una señal"),
        }
    }

    fn vector(&self, n: &Node, f: &str) -> Result<Vec<C>, ExprError> {
        match self.eval(n)? {
            Val::Vector(v) => Ok(v),
            Val::Scalar(_) => eval_err(format!("{f}() necesita una señal")),
        }
    }

    fn call(&self, f: &str, args: &[Node]) -> Result<Val, ExprError> {
        let arity = |n: usize| -> Result<(), ExprError> {
            if args.len() == n { Ok(()) } else { eval_err(format!("{f}() lleva {n} argumento(s)")) }
        };
        let elementwise = |g: fn(C) -> C| -> Result<Val, ExprError> {
            arity(1)?;
            Ok(map(self.eval(&args[0])?, g))
        };
        match f {
            "abs" | "mag" => elementwise(|c| C::real(c.abs())),
            "real" | "re" => elementwise(|c| C::real(c.re)),
            "imag" | "im" => elementwise(|c| C::real(c.im)),
            "ph" | "phase" => elementwise(|c| C::real(c.arg().to_degrees())),
            "db" => elementwise(|c| C::real(20.0 * c.abs().log10())),
            "sqrt" => elementwise(C::sqrt),
            "exp" => elementwise(C::exp),
            "ln" => elementwise(C::ln),
            "log" | "log10" => elementwise(|c| c.ln().scale(std::f64::consts::LOG10_E)),
            "sin" => elementwise(C::sin),
            "cos" => elementwise(C::cos),
            "tan" => elementwise(|c| c.sin().div(c.cos())),
            "atan" => elementwise(|c| C::real(c.re.atan())),
            "deriv" => {
                arity(1)?;
                Ok(Val::Vector(deriv(&self.x, &self.vector(&args[0], f)?)))
            }
            "integ" => {
                arity(1)?;
                Ok(Val::Vector(integ(&self.x, &self.vector(&args[0], f)?)))
            }
            "integral" => {
                arity(1)?;
                let v = integ(&self.x, &self.vector(&args[0], f)?);
                Ok(Val::Scalar(v.iter().rev().find(|c| c.is_finite()).copied().unwrap_or(C::NAN)))
            }
            "mean" | "avg" => {
                arity(1)?;
                Ok(Val::Scalar(mean(&self.x, &self.vector(&args[0], f)?)))
            }
            "rms" => {
                arity(1)?;
                let sq: Vec<C> = self.vector(&args[0], f)?.iter().map(|c| C::real(c.abs() * c.abs())).collect();
                Ok(Val::Scalar(C::real(mean(&self.x, &sq).re.sqrt())))
            }
            "max" | "min" if args.len() == 2 => {
                let pick_max = f == "max";
                binary_with(self.eval(&args[0])?, self.eval(&args[1])?, move |a, b| {
                    let (ma, mb) = (metric(a), metric(b));
                    if (ma >= mb) == pick_max { a } else { b }
                })
            }
            "max" | "min" | "pp" => {
                arity(1)?;
                let v = self.vector(&args[0], f)?;
                let ms: Vec<f64> = v.iter().map(|c| metric(*c)).filter(|m| m.is_finite()).collect();
                if ms.is_empty() {
                    return eval_err(format!("{f}() sobre una señal sin valores"));
                }
                let hi = ms.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let lo = ms.iter().copied().fold(f64::INFINITY, f64::min);
                Ok(Val::Scalar(C::real(match f {
                    "max" => hi,
                    "min" => lo,
                    _ => hi - lo,
                })))
            }
            "length" => {
                arity(1)?;
                Ok(Val::Scalar(C::real(match self.eval(&args[0])? {
                    Val::Vector(v) => v.iter().filter(|c| c.is_finite()).count() as f64,
                    Val::Scalar(_) => 1.0,
                })))
            }
            "at" => {
                arity(2)?;
                let v = self.vector(&args[0], f)?;
                let x0 = self.scalar(&args[1])?;
                Ok(Val::Scalar(at(&self.x, &v, x0)?))
            }
            "window" => {
                arity(3)?;
                let v = self.vector(&args[0], f)?;
                let (x0, x1) = (self.scalar(&args[1])?, self.scalar(&args[2])?);
                let (lo, hi) = (x0.min(x1), x0.max(x1));
                Ok(Val::Vector(v.iter().zip(&self.x).map(|(c, &x)| if x >= lo && x <= hi { *c } else { C::NAN }).collect()))
            }
            _ => Err(ExprError::Syntax(format!("función desconocida {f}()"))),
        }
    }
}

/// Valor que ordenan `max`/`min`: el real, o la magnitud si es complejo.
fn metric(c: C) -> f64 {
    if c.is_real() { c.re } else { c.abs() }
}

fn wrap(i: i64, n: usize) -> Option<usize> {
    let k = if i < 0 { n as i64 + i } else { i };
    (k >= 0 && (k as usize) < n).then_some(k as usize)
}

fn map(v: Val, g: impl Fn(C) -> C) -> Val {
    match v {
        Val::Scalar(c) => Val::Scalar(g(c)),
        Val::Vector(vs) => Val::Vector(vs.into_iter().map(g).collect()),
    }
}

fn binary(op: char, a: Val, b: Val) -> Result<Val, ExprError> {
    binary_with(a, b, move |x, y| match op {
        '+' => x.add(y),
        '-' => x.sub(y),
        '*' => x.mul(y),
        '/' => x.div(y),
        _ => x.pow(y),
    })
}

fn binary_with(a: Val, b: Val, g: impl Fn(C, C) -> C) -> Result<Val, ExprError> {
    Ok(match (a, b) {
        (Val::Scalar(x), Val::Scalar(y)) => Val::Scalar(g(x, y)),
        (Val::Scalar(x), Val::Vector(ys)) => Val::Vector(ys.into_iter().map(|y| g(x, y)).collect()),
        (Val::Vector(xs), Val::Scalar(y)) => Val::Vector(xs.into_iter().map(|x| g(x, y)).collect()),
        (Val::Vector(xs), Val::Vector(ys)) => {
            if xs.len() != ys.len() {
                return eval_err(format!("señales de distinto largo ({} y {})", xs.len(), ys.len()));
            }
            Val::Vector(xs.into_iter().zip(ys).map(|(x, y)| g(x, y)).collect())
        }
    })
}

/// Derivada respecto del eje: diferencias centradas en el interior (sirve
/// con paso variable) y de un lado en los extremos.
fn deriv(x: &[f64], y: &[C]) -> Vec<C> {
    let n = x.len().min(y.len());
    (0..n)
        .map(|i| {
            if n < 2 {
                return C::NAN;
            }
            let (a, b) = if i == 0 { (0, 1) } else if i == n - 1 { (n - 2, n - 1) } else { (i - 1, i + 1) };
            let dx = x[b] - x[a];
            if dx == 0.0 { C::NAN } else { y[b].sub(y[a]).scale(1.0 / dx) }
        })
        .collect()
}

/// Integral acumulada por trapecios desde el primer punto (los puntos NaN
/// no suman).
fn integ(x: &[f64], y: &[C]) -> Vec<C> {
    let n = x.len().min(y.len());
    let mut acc = C::real(0.0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 && y[i].is_finite() && y[i - 1].is_finite() {
            acc = acc.add(y[i].add(y[i - 1]).scale((x[i] - x[i - 1]) / 2.0));
        }
        out.push(if y[i].is_finite() { acc } else { C::NAN });
    }
    out
}

/// Promedio en el eje (integral / tramo), ignorando puntos NaN.
fn mean(x: &[f64], y: &[C]) -> C {
    let pts: Vec<(f64, C)> = x.iter().copied().zip(y.iter().copied()).filter(|(_, c)| c.is_finite()).collect();
    match pts.len() {
        0 => C::NAN,
        1 => pts[0].1,
        _ => {
            let mut acc = C::real(0.0);
            for w in pts.windows(2) {
                acc = acc.add(w[0].1.add(w[1].1).scale((w[1].0 - w[0].0) / 2.0));
            }
            let span = pts[pts.len() - 1].0 - pts[0].0;
            if span == 0.0 { pts[0].1 } else { acc.scale(1.0 / span) }
        }
    }
}

/// Valor interpolado linealmente en `x0`.
fn at(x: &[f64], y: &[C], x0: f64) -> Result<C, ExprError> {
    let n = x.len().min(y.len());
    if n == 0 || x0 < x[0] || x0 > x[n - 1] {
        return eval_err(format!("at(): {x0:e} está fuera del eje"));
    }
    let i = x[..n].partition_point(|&v| v < x0);
    if i == 0 {
        return Ok(y[0]);
    }
    let (x0a, x1a) = (x[i - 1], x[i]);
    if x1a == x0a {
        return Ok(y[i]);
    }
    let t = (x0 - x0a) / (x1a - x0a);
    Ok(y[i - 1].add(y[i].sub(y[i - 1]).scale(t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str, kind: &str, values: Vec<f64>) -> Variable {
        Variable { name: name.into(), kind: kind.into(), values, complex: None }
    }

    fn tran() -> Plot {
        Plot {
            title: String::new(),
            name: "Transient Analysis".into(),
            command: None,
            complex: false,
            vars: vec![
                var("time", "time", vec![0.0, 1.0, 2.0, 3.0]),
                var("v(in)", "voltage", vec![1.0, 2.0, 4.0, 8.0]),
                var("v(out)", "voltage", vec![0.5, 1.0, 2.0, 4.0]),
                var("i(v1)", "current", vec![0.0, 1.0, 0.0, -1.0]),
            ],
        }
    }

    fn signal(e: &str, p: &Plot) -> Vec<f64> {
        match parse(e).unwrap().eval(p).unwrap() {
            Evaluated::Signal(v) => v.values,
            other => panic!("se esperaba señal: {other:?}"),
        }
    }

    fn scalar(e: &str, p: &Plot) -> f64 {
        match parse(e).unwrap().eval(p).unwrap() {
            Evaluated::Scalar(v, _) => v,
            other => panic!("se esperaba número: {other:?}"),
        }
    }

    #[test]
    fn arithmetic_on_signals() {
        let p = tran();
        assert_eq!(signal("v(out)/v(in)", &p), vec![0.5; 4]);
        assert_eq!(signal("v(in,out)", &p), vec![0.5, 1.0, 2.0, 4.0]);
        assert_eq!(signal("out * 2 + 1", &p), vec![2.0, 3.0, 5.0, 9.0]);
        assert_eq!(signal("-v(out)^2", &p), vec![-0.25, -1.0, -4.0, -16.0]);
        assert_eq!(scalar("2^3^2", &p), 512.0);
    }

    #[test]
    fn spice_suffixes() {
        let p = tran();
        assert!((scalar("10u", &p) - 1e-5).abs() < 1e-20);
        assert_eq!(scalar("2meg", &p), 2e6);
        assert!((scalar("100nF", &p) - 1e-7).abs() < 1e-20);
        assert_eq!(scalar("1e3", &p), 1000.0);
        assert!((scalar("1.5k + 1m", &p) - 1500.001).abs() < 1e-9);
    }

    #[test]
    fn calculus_and_reductions() {
        let p = tran();
        // v(in) = 1,2,4,8 con paso 1: derivadas centradas.
        assert_eq!(signal("deriv(v(in))", &p), vec![1.0, 1.5, 3.0, 4.0]);
        // Trapecios: 1.5, 3, 6 acumulados.
        assert_eq!(signal("integ(v(in))", &p), vec![0.0, 1.5, 4.5, 10.5]);
        assert_eq!(scalar("integ(v(in))[-1]", &p), 10.5);
        assert_eq!(scalar("integral(v(in))", &p), 10.5);
        assert_eq!(scalar("mean(v(in))", &p), 3.5);
        assert_eq!(scalar("max(v(in))", &p), 8.0);
        assert_eq!(scalar("pp(i(v1))", &p), 2.0);
        assert_eq!(scalar("v(out)[0]", &p), 0.5);
        assert_eq!(scalar("at(v(in), 2.5)", &p), 6.0);
        assert_eq!(scalar("length(v(in)[1:2])", &p), 2.0);
        assert_eq!(signal("max(v(in), 3)", &p), vec![3.0, 3.0, 4.0, 8.0]);
    }

    #[test]
    fn windows_leave_the_rest_out() {
        let p = tran();
        let w = signal("window(v(in), 1, 2)", &p);
        assert!(w[0].is_nan() && w[3].is_nan());
        assert_eq!(&w[1..3], &[2.0, 4.0]);
        assert_eq!(scalar("max(v(in)[0:1])", &p), 2.0);
    }

    #[test]
    fn complex_division_in_ac() {
        let freq = Variable { name: "frequency".into(), kind: "frequency".into(), values: vec![1.0, 10.0], complex: Some(vec![(1.0, 0.0), (10.0, 0.0)]) };
        let vin = Variable { name: "v(in)".into(), kind: "voltage".into(), values: vec![0.0, 0.0], complex: Some(vec![(1.0, 0.0), (1.0, 0.0)]) };
        // v(out) = 1/(1+j): |·| = 1/√2 → −3.01 dB, fase −45°.
        let vout = Variable { name: "v(out)".into(), kind: "voltage".into(), values: vec![0.0, 0.0], complex: Some(vec![(0.5, -0.5), (0.1, 0.0)]) };
        let p = Plot { title: String::new(), name: "AC Analysis".into(), command: None, complex: true, vars: vec![freq, vin, vout] };
        let g = signal("v(out)/v(in)", &p);
        assert!((g[0] + 3.0103).abs() < 1e-3, "{g:?}");
        let ph = signal("ph(v(out)/v(in))", &p);
        assert!((ph[0] + 45.0).abs() < 1e-9);
        match parse("gain = db(v(out))").unwrap().eval(&p).unwrap() {
            Evaluated::Signal(v) => assert_eq!((v.name.as_str(), v.unit(true)), ("gain", "dB")),
            _ => panic!(),
        }
    }

    #[test]
    fn names_and_errors() {
        let p = tran();
        let e = parse("gain = v(out)/v(in)").unwrap();
        assert_eq!((e.name.as_str(), e.text.as_str()), ("gain", "v(out)/v(in)"));
        assert_eq!(parse("v(out)/v(in)").unwrap().name, "v(out)/v(in)");
        assert_eq!(parse("v(x)/2").unwrap().eval(&p), Err(ExprError::Missing("v(x)".into())));
        assert!(matches!(parse("v(out) +"), Err(ExprError::Syntax(_))));
        assert!(matches!(parse("foo(v(out))").unwrap().eval(&p), Err(ExprError::Syntax(_))));
        assert!(matches!(parse("v(out)[9]").unwrap().eval(&p), Err(ExprError::Eval(_))));
        assert_eq!(parse("@m.xm1.msky130_fd_pr__nfet_01v8[id]*2").unwrap().eval(&p), Err(ExprError::Missing("@m.xm1.msky130_fd_pr__nfet_01v8[id]".into())));
    }

    #[test]
    fn analysis_prefix_limits_where_it_applies() {
        let e = parse("tran: slew = max(deriv(v(out)))").unwrap();
        assert_eq!((e.analysis, e.name.as_str(), e.text.as_str()), (Some("tran"), "slew", "max(deriv(v(out)))"));
        assert!(e.applies_to("Transient Analysis") && !e.applies_to("AC Analysis"));
        assert!(parse("AC: db(v(out))").unwrap().applies_to("AC Analysis"));
        assert!(parse("v(out)").unwrap().applies_to("Operating Point"));
        // Los `:` de un tramo no son un prefijo.
        assert_eq!(parse("v(out)[0:1]").unwrap().analysis, None);
    }

    #[test]
    fn division_by_zero_is_not_a_crash() {
        let p = tran();
        let v = signal("v(out)/i(v1)", &p);
        assert!(v[0].is_nan() && v[1] == 1.0);
    }
}
