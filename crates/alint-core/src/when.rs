//! The `when` expression language — bounded DSL for gating rules on facts.
//!
//! Grammar (hand-written recursive-descent; no parser combinator):
//!
//! ```text
//! expr       := or_expr
//! or_expr    := and_expr ('or' and_expr)*
//! and_expr   := not_expr ('and' not_expr)*
//! not_expr   := ['not'] cmp_expr
//! cmp_expr   := primary [cmp_op primary]
//! cmp_op     := '==' | '!=' | '<' | '<=' | '>' | '>=' | 'in' | 'matches'
//! primary    := literal | ident_or_call | '(' expr ')'
//! literal    := STRING | INT | BOOL | 'null' | list
//! list       := '[' [expr (',' expr)*] ']'
//! ident_or_call := NS '.' NAME ['(' [expr (',' expr)*] ')']
//! NS         := 'facts' | 'vars' | 'iter'
//! ```
//!
//! Design choices (all load-bearing):
//!
//! - **No arithmetic.** Only comparison.
//! - **Function calls limited to a fixed set on the `iter` namespace.**
//!   `iter.has_file("Cargo.toml")` is supported; arbitrary user-defined
//!   calls are not. Use declared `facts:` for repo-level computation.
//! - **`iter.*` is only meaningful in iteration contexts** (per-iteration
//!   `when_iter:` on `for_each_*`, and nested rules' `when:`). Outside
//!   those, `iter.X` evaluates to `null` and `iter.has_file(_)` to `false`.
//! - **`matches` RHS must be a string literal.** This lets us compile the
//!   regex at parse time; dynamic patterns stay out of the hot path.
//! - **Short-circuit `and` / `or`.** Unevaluated branches don't even touch
//!   their subtree.
//! - **Type coercion is explicit, not silent.** Comparing `Int` to `String`
//!   is an error, not `false`.

use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use thiserror::Error;

use crate::facts::{FactValue, FactValues};
use crate::scope::Scope;
use crate::walker::FileIndex;

// ─── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum WhenError {
    #[error("when parse error at column {pos}: {message}")]
    Parse { pos: usize, message: String },
    #[error("when evaluation error: {0}")]
    Eval(String),
    #[error("invalid regex in `matches`: {0}")]
    Regex(String),
}

// ─── Value (evaluation-time) ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    String(String),
    List(Vec<Value>),
    Null,
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::String(s) => !s.is_empty(),
            Self::List(v) => !v.is_empty(),
            Self::Null => false,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::String(_) => "string",
            Self::List(_) => "list",
            Self::Null => "null",
        }
    }
}

impl From<&FactValue> for Value {
    fn from(f: &FactValue) -> Self {
        match f {
            FactValue::Bool(b) => Self::Bool(*b),
            FactValue::Int(n) => Self::Int(*n),
            FactValue::String(s) => Self::String(s.clone()),
        }
    }
}

// ─── AST ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Facts,
    Vars,
    /// Per-iteration context. Available only when an `IterEnv`
    /// is threaded into the evaluator (via
    /// [`WhenEnv::with_iter`]). Outside those, `iter.X`
    /// evaluates to `null` and `iter.has_file(_)` to `false` —
    /// matching the "missing fact is falsy" rule.
    Iter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
}

#[derive(Debug, Clone)]
pub enum WhenExpr {
    Literal(Value),
    Ident {
        ns: Namespace,
        name: String,
    },
    /// `<ns>.<method>(args...)`. Currently only the `iter`
    /// namespace exposes callable methods; an unknown
    /// (namespace, method) pair is rejected at parse time so
    /// typos don't silently coerce to `null` like value-style
    /// idents do.
    Call {
        ns: Namespace,
        method: String,
        args: Vec<WhenExpr>,
    },
    Not(Box<WhenExpr>),
    And(Box<WhenExpr>, Box<WhenExpr>),
    Or(Box<WhenExpr>, Box<WhenExpr>),
    Cmp {
        left: Box<WhenExpr>,
        op: CmpOp,
        right: Box<WhenExpr>,
    },
    /// `left matches <compiled regex>` — RHS is compiled at parse time.
    Matches {
        left: Box<WhenExpr>,
        pattern: Regex,
    },
    List(Vec<WhenExpr>),
}

// ─── Evaluation environment ──────────────────────────────────────────

#[derive(Debug)]
pub struct WhenEnv<'a> {
    pub facts: &'a FactValues,
    pub vars: &'a HashMap<String, String>,
    /// Per-iteration context, populated when this `WhenEnv`
    /// gates an iterated rule (`for_each_dir` /
    /// `for_each_file` / `every_matching_has`). `None` for
    /// top-level rule gating, where `iter.*` references
    /// resolve to falsy / null per the "unknown fact is
    /// falsy" convention.
    pub iter: Option<IterEnv<'a>>,
}

impl<'a> WhenEnv<'a> {
    /// Construct a `WhenEnv` without iteration context — the
    /// shape every existing call site uses. `iter.*` references
    /// in the expression resolve to null / false.
    #[must_use]
    pub fn new(facts: &'a FactValues, vars: &'a HashMap<String, String>) -> Self {
        Self {
            facts,
            vars,
            iter: None,
        }
    }

    /// Attach an iteration context. The same `WhenEnv` shape can
    /// then evaluate `iter.path`, `iter.basename`, and
    /// `iter.has_file(...)` against the supplied path + index.
    #[must_use]
    pub fn with_iter(mut self, iter: IterEnv<'a>) -> Self {
        self.iter = Some(iter);
        self
    }
}

/// Iteration context exposed to `when:` expressions through the
/// `iter.*` namespace. Built once per iterated entry by
/// `for_each_*` rules and threaded into both the outer
/// `when_iter:` filter and any nested rule's `when:`.
#[derive(Debug, Clone, Copy)]
pub struct IterEnv<'a> {
    /// Relative path of the iterated entry (as walker reported).
    pub path: &'a Path,
    /// Whether the iterated entry is a directory. `iter.has_file`
    /// only does meaningful work when this is `true`; for files
    /// it returns `false`.
    pub is_dir: bool,
    /// File index, used by `iter.has_file(pattern)` to look up
    /// children of the iterated path.
    pub index: &'a FileIndex,
}

// ─── Public entry points ─────────────────────────────────────────────

pub fn parse(src: &str) -> Result<WhenExpr, WhenError> {
    let tokens = lex(src)?;
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.parse_expr()?;
    p.expect_eof()?;
    Ok(expr)
}

impl WhenExpr {
    pub fn evaluate(&self, env: &WhenEnv<'_>) -> Result<bool, WhenError> {
        let v = eval(self, env)?;
        Ok(v.truthy())
    }
}

// ─── Lexer ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Tok {
    Bool(bool),
    Null,
    Int(i64),
    Str(String),
    Ident(String),
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eq2,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    KwAnd,
    KwOr,
    KwNot,
    KwIn,
    KwMatches,
}

#[allow(clippy::too_many_lines)]
fn lex(src: &str) -> Result<Vec<(Tok, usize)>, WhenError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // whitespace
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            i += 1;
            continue;
        }
        let start = i;
        match c {
            b'.' => {
                out.push((Tok::Dot, start));
                i += 1;
            }
            b'(' => {
                out.push((Tok::LParen, start));
                i += 1;
            }
            b')' => {
                out.push((Tok::RParen, start));
                i += 1;
            }
            b'[' => {
                out.push((Tok::LBracket, start));
                i += 1;
            }
            b']' => {
                out.push((Tok::RBracket, start));
                i += 1;
            }
            b',' => {
                out.push((Tok::Comma, start));
                i += 1;
            }
            b'=' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Eq2, start));
                    i += 2;
                } else {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "expected '==' (bare '=' is not an operator)".into(),
                    });
                }
            }
            b'!' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Ne, start));
                    i += 2;
                } else {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "expected '!=' (use 'not' for logical negation)".into(),
                    });
                }
            }
            b'<' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Le, start));
                    i += 2;
                } else {
                    out.push((Tok::Lt, start));
                    i += 1;
                }
            }
            b'>' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Ge, start));
                    i += 2;
                } else {
                    out.push((Tok::Gt, start));
                    i += 1;
                }
            }
            b'"' | b'\'' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        let esc = bytes[i + 1];
                        let ch = match esc {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'\\' => '\\',
                            b'"' => '"',
                            b'\'' => '\'',
                            _ => {
                                return Err(WhenError::Parse {
                                    pos: i,
                                    message: format!(
                                        "unknown escape \\{} in string literal",
                                        esc as char,
                                    ),
                                });
                            }
                        };
                        s.push(ch);
                        i += 2;
                    } else {
                        s.push(bytes[i] as char);
                        i += 1;
                    }
                }
                if i >= bytes.len() {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "unterminated string literal".into(),
                    });
                }
                i += 1;
                out.push((Tok::Str(s), start));
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let num = std::str::from_utf8(&bytes[i..j])
                    .unwrap()
                    .parse::<i64>()
                    .map_err(|e| WhenError::Parse {
                        pos: start,
                        message: format!("invalid integer: {e}"),
                    })?;
                out.push((Tok::Int(num), start));
                i = j;
            }
            c if is_ident_start(c) => {
                let mut j = i;
                while j < bytes.len() && is_ident_cont(bytes[j]) {
                    j += 1;
                }
                let word = &src[i..j];
                let tok = match word {
                    "true" => Tok::Bool(true),
                    "false" => Tok::Bool(false),
                    "null" => Tok::Null,
                    "and" => Tok::KwAnd,
                    "or" => Tok::KwOr,
                    "not" => Tok::KwNot,
                    "in" => Tok::KwIn,
                    "matches" => Tok::KwMatches,
                    _ => Tok::Ident(word.to_string()),
                };
                out.push((tok, start));
                i = j;
            }
            _ => {
                return Err(WhenError::Parse {
                    pos: start,
                    message: format!("unexpected character {:?}", c as char),
                });
            }
        }
    }
    Ok(out)
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Closed list of methods callable on `iter`. Adding new ones is
/// a deliberate API extension — typos in user configs surface as
/// "unknown iter method" rather than silently coercing to false.
fn is_known_iter_method(name: &str) -> bool {
    matches!(name, "has_file")
}

// ─── Parser ──────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<(Tok, usize)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn advance(&mut self) -> Option<&(Tok, usize)> {
        let p = self.pos;
        self.pos += 1;
        self.tokens.get(p)
    }

    fn pos_here(&self) -> usize {
        self.tokens.get(self.pos).map_or_else(
            || self.tokens.last().map_or(0, |(_, p)| *p + 1),
            |(_, p)| *p,
        )
    }

    fn err(&self, message: impl Into<String>) -> WhenError {
        WhenError::Parse {
            pos: self.pos_here(),
            message: message.into(),
        }
    }

    fn expect_eof(&mut self) -> Result<(), WhenError> {
        if self.peek().is_some() {
            Err(self.err("unexpected trailing token"))
        } else {
            Ok(())
        }
    }

    fn parse_expr(&mut self) -> Result<WhenExpr, WhenError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<WhenExpr, WhenError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Tok::KwOr)) {
            self.advance();
            let right = self.parse_and()?;
            left = WhenExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<WhenExpr, WhenError> {
        let mut left = self.parse_not()?;
        while matches!(self.peek(), Some(Tok::KwAnd)) {
            self.advance();
            let right = self.parse_not()?;
            left = WhenExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<WhenExpr, WhenError> {
        if matches!(self.peek(), Some(Tok::KwNot)) {
            self.advance();
            let inner = self.parse_cmp()?;
            return Ok(WhenExpr::Not(Box::new(inner)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<WhenExpr, WhenError> {
        let left = self.parse_primary()?;
        let op = match self.peek() {
            Some(Tok::Eq2) => Some(CmpOp::Eq),
            Some(Tok::Ne) => Some(CmpOp::Ne),
            Some(Tok::Lt) => Some(CmpOp::Lt),
            Some(Tok::Le) => Some(CmpOp::Le),
            Some(Tok::Gt) => Some(CmpOp::Gt),
            Some(Tok::Ge) => Some(CmpOp::Ge),
            Some(Tok::KwIn) => Some(CmpOp::In),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_primary()?;
            return Ok(WhenExpr::Cmp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });
        }
        if matches!(self.peek(), Some(Tok::KwMatches)) {
            self.advance();
            let pos = self.pos_here();
            match self.advance() {
                Some((Tok::Str(s), _)) => {
                    let pattern = Regex::new(s)
                        .map_err(|e| WhenError::Regex(format!("{e} (at column {pos})")))?;
                    return Ok(WhenExpr::Matches {
                        left: Box::new(left),
                        pattern,
                    });
                }
                _ => {
                    return Err(WhenError::Parse {
                        pos,
                        message: "`matches` right-hand side must be a string literal".into(),
                    });
                }
            }
        }
        Ok(left)
    }

    #[allow(clippy::too_many_lines)] // Single match per primary form keeps the dispatch obvious; splitting it costs more than it saves.
    fn parse_primary(&mut self) -> Result<WhenExpr, WhenError> {
        let pos = self.pos_here();
        match self.advance() {
            Some((Tok::Bool(b), _)) => Ok(WhenExpr::Literal(Value::Bool(*b))),
            Some((Tok::Null, _)) => Ok(WhenExpr::Literal(Value::Null)),
            Some((Tok::Int(n), _)) => Ok(WhenExpr::Literal(Value::Int(*n))),
            Some((Tok::Str(s), _)) => Ok(WhenExpr::Literal(Value::String(s.clone()))),
            Some((Tok::LParen, _)) => {
                let inner = self.parse_expr()?;
                match self.advance() {
                    Some((Tok::RParen, _)) => Ok(inner),
                    _ => Err(WhenError::Parse {
                        pos,
                        message: "expected ')'".into(),
                    }),
                }
            }
            Some((Tok::LBracket, _)) => {
                let mut items = Vec::new();
                if !matches!(self.peek(), Some(Tok::RBracket)) {
                    items.push(self.parse_expr()?);
                    while matches!(self.peek(), Some(Tok::Comma)) {
                        self.advance();
                        items.push(self.parse_expr()?);
                    }
                }
                match self.advance() {
                    Some((Tok::RBracket, _)) => Ok(WhenExpr::List(items)),
                    _ => Err(WhenError::Parse {
                        pos,
                        message: "expected ']'".into(),
                    }),
                }
            }
            Some((Tok::Ident(name), _)) => {
                let name_owned = name.clone();
                let ns = match name_owned.as_str() {
                    "facts" => Namespace::Facts,
                    "vars" => Namespace::Vars,
                    "iter" => Namespace::Iter,
                    other => {
                        return Err(WhenError::Parse {
                            pos,
                            message: format!(
                                "unknown identifier {other:?}; only `facts.NAME`, \
                                 `vars.NAME`, and `iter.NAME` are allowed"
                            ),
                        });
                    }
                };
                if !matches!(self.advance(), Some((Tok::Dot, _))) {
                    return Err(WhenError::Parse {
                        pos,
                        message: format!("expected '.' after {name_owned:?}"),
                    });
                }
                let field_pos = self.pos_here();
                let field = match self.advance() {
                    Some((Tok::Ident(f), _)) => f.clone(),
                    _ => {
                        return Err(WhenError::Parse {
                            pos: field_pos,
                            message: "expected identifier after '.'".into(),
                        });
                    }
                };
                // Optional `(args...)` — function-call syntax.
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.advance(); // consume '('
                    if ns != Namespace::Iter {
                        return Err(WhenError::Parse {
                            pos: field_pos,
                            message: format!(
                                "function-call syntax is only available on `iter` \
                                 (got `{name_owned}.{field}(...)`)"
                            ),
                        });
                    }
                    if !is_known_iter_method(&field) {
                        return Err(WhenError::Parse {
                            pos: field_pos,
                            message: format!(
                                "unknown iter method {field:?}; the only callable \
                                 method on `iter` is `has_file`"
                            ),
                        });
                    }
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek(), Some(Tok::Comma)) {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    match self.advance() {
                        Some((Tok::RParen, _)) => {}
                        _ => {
                            return Err(WhenError::Parse {
                                pos: field_pos,
                                message: "expected ')'".into(),
                            });
                        }
                    }
                    return Ok(WhenExpr::Call {
                        ns,
                        method: field,
                        args,
                    });
                }
                Ok(WhenExpr::Ident { ns, name: field })
            }
            _ => Err(WhenError::Parse {
                pos,
                message: "expected literal, identifier, '(' or '['".into(),
            }),
        }
    }
}

// ─── Evaluator ───────────────────────────────────────────────────────

fn eval(e: &WhenExpr, env: &WhenEnv<'_>) -> Result<Value, WhenError> {
    match e {
        WhenExpr::Literal(v) => Ok(v.clone()),
        WhenExpr::Ident { ns, name } => match ns {
            Namespace::Facts => match env.facts.get(name) {
                Some(f) => Ok(Value::from(f)),
                None => Ok(Value::Null),
            },
            Namespace::Vars => match env.vars.get(name) {
                Some(v) => Ok(Value::String(v.clone())),
                None => Ok(Value::Null),
            },
            Namespace::Iter => Ok(eval_iter_value(name, env.iter.as_ref())),
        },
        WhenExpr::Call { ns, method, args } => match ns {
            Namespace::Iter => eval_iter_call(method, args, env),
            // Parser rejects calls on non-iter namespaces, but be
            // defensive in case the AST is hand-built somewhere.
            _ => Err(WhenError::Eval(format!(
                "function-call evaluation not supported on namespace {ns:?}"
            ))),
        },
        WhenExpr::Not(inner) => Ok(Value::Bool(!eval(inner, env)?.truthy())),
        WhenExpr::And(l, r) => {
            let lv = eval(l, env)?;
            if !lv.truthy() {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(eval(r, env)?.truthy()))
        }
        WhenExpr::Or(l, r) => {
            let lv = eval(l, env)?;
            if lv.truthy() {
                return Ok(Value::Bool(true));
            }
            Ok(Value::Bool(eval(r, env)?.truthy()))
        }
        WhenExpr::Cmp { left, op, right } => {
            let lv = eval(left, env)?;
            let rv = eval(right, env)?;
            Ok(Value::Bool(apply_cmp(&lv, *op, &rv)?))
        }
        WhenExpr::Matches { left, pattern } => {
            let lv = eval(left, env)?;
            match lv {
                Value::String(s) => Ok(Value::Bool(pattern.is_match(&s))),
                other => Err(WhenError::Eval(format!(
                    "`matches` left-hand side must be a string; got {}",
                    other.type_name()
                ))),
            }
        }
        WhenExpr::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval(item, env)?);
            }
            Ok(Value::List(out))
        }
    }
}

/// Resolve an `iter.<name>` value-style reference. Returns
/// `Null` when no iteration context is attached or the name is
/// unrecognised — matching the "missing is falsy" convention so
/// that a stray `iter.X` outside an iteration doesn't error.
fn eval_iter_value(name: &str, iter: Option<&IterEnv<'_>>) -> Value {
    let Some(iter) = iter else {
        return Value::Null;
    };
    match name {
        "path" => Value::String(iter.path.to_string_lossy().into_owned()),
        "basename" => match iter.path.file_name().and_then(|s| s.to_str()) {
            Some(s) => Value::String(s.to_string()),
            None => Value::Null,
        },
        "parent_name" => iter
            .path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .map_or(Value::Null, |s| Value::String(s.to_string())),
        "stem" => iter
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .map_or(Value::Null, |s| Value::String(s.to_string())),
        "ext" => iter
            .path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(Value::Null, |s| Value::String(s.to_string())),
        "is_dir" => Value::Bool(iter.is_dir),
        _ => Value::Null,
    }
}

/// Resolve an `iter.<method>(args...)` call. The parser
/// guarantees `method` is one of the known callables (currently
/// just `has_file`); arity / arg-type errors surface as
/// [`WhenError::Eval`] at evaluation time so a parse-clean
/// expression with bad args still reports clearly.
fn eval_iter_call(method: &str, args: &[WhenExpr], env: &WhenEnv<'_>) -> Result<Value, WhenError> {
    match method {
        "has_file" => {
            if args.len() != 1 {
                return Err(WhenError::Eval(format!(
                    "iter.has_file expects exactly 1 argument; got {}",
                    args.len()
                )));
            }
            let pattern = match eval(&args[0], env)? {
                Value::String(s) => s,
                other => {
                    return Err(WhenError::Eval(format!(
                        "iter.has_file argument must be a string; got {}",
                        other.type_name()
                    )));
                }
            };
            Ok(Value::Bool(iter_has_file(env.iter.as_ref(), &pattern)?))
        }
        _ => Err(WhenError::Eval(format!(
            "unknown iter method {method:?} (parser should have caught this)"
        ))),
    }
}

/// Implementation of `iter.has_file(pattern)`. `pattern` is a
/// Git-style glob evaluated relative to the iterated path —
/// `iter.has_file("Cargo.toml")` matches any tracked file at
/// `<iter.path>/Cargo.toml`; `iter.has_file("**/*.bzl")` matches
/// any `.bzl` under the iterated dir at any depth. Returns
/// `false` when the iteration context is absent or the iterated
/// entry isn't a directory (files don't "contain" anything).
///
/// When `pattern` is a literal filename (no glob metacharacters)
/// the fast path consults the index's hash-set directly — O(1)
/// per call. The slow path falls back to a scope match against
/// every file in the index. At 1M files in a 5,000-package
/// monorepo, `for_each_dir` rules with
/// `when_iter: 'iter.has_file("Cargo.toml")'` would otherwise
/// be O(D × N); the fast path collapses them to O(D).
fn iter_has_file(iter: Option<&IterEnv<'_>>, pattern: &str) -> Result<bool, WhenError> {
    let Some(iter) = iter else {
        return Ok(false);
    };
    if !iter.is_dir {
        return Ok(false);
    }
    if !pattern
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
        && !pattern.starts_with('!')
    {
        let candidate = iter.path.join(pattern);
        return Ok(iter.index.contains_file(&candidate));
    }
    let combined = format!("{}/{}", iter.path.to_string_lossy(), pattern);
    let scope = Scope::from_patterns(std::slice::from_ref(&combined))
        .map_err(|e| WhenError::Eval(format!("iter.has_file: invalid glob: {e}")))?;
    Ok(iter
        .index
        .files()
        .any(|e| scope.matches(&e.path, iter.index)))
}

fn apply_cmp(l: &Value, op: CmpOp, r: &Value) -> Result<bool, WhenError> {
    use Value::{Bool, Int, List, Null, String as S};
    match op {
        CmpOp::Eq => Ok(values_equal(l, r)),
        CmpOp::Ne => Ok(!values_equal(l, r)),
        CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => match (l, r) {
            (Int(a), Int(b)) => Ok(cmp_ord(a, b, op)),
            (S(a), S(b)) => Ok(cmp_ord(&a.as_str(), &b.as_str(), op)),
            _ => Err(WhenError::Eval(format!(
                "cannot compare {} with {}",
                l.type_name(),
                r.type_name(),
            ))),
        },
        CmpOp::In => match r {
            List(items) => Ok(items.iter().any(|x| values_equal(l, x))),
            S(haystack) => match l {
                S(needle) => Ok(haystack.contains(needle.as_str())),
                _ => Err(WhenError::Eval(format!(
                    "`in` with a string right-hand side requires a string left; got {}",
                    l.type_name()
                ))),
            },
            _ => {
                let _ = (Bool(false), Null);
                Err(WhenError::Eval(format!(
                    "`in` right-hand side must be a list or string; got {}",
                    r.type_name()
                )))
            }
        },
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_equal(a, b))
        }
        _ => false,
    }
}

fn cmp_ord<T: PartialOrd>(a: &T, b: &T, op: CmpOp) -> bool {
    match op {
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
        _ => unreachable!(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> (FactValues, HashMap<String, String>) {
        let mut f = FactValues::new();
        f.insert("is_rust".into(), FactValue::Bool(true));
        f.insert("is_node".into(), FactValue::Bool(false));
        f.insert("n_files".into(), FactValue::Int(42));
        f.insert("primary".into(), FactValue::String("Rust".into()));
        let mut v = HashMap::new();
        v.insert("org".into(), "Acme Corp".into());
        v.insert("year".into(), "2026".into());
        (f, v)
    }

    fn check(src: &str) -> bool {
        let (facts, vars) = env();
        let expr = parse(src).unwrap();
        expr.evaluate(&WhenEnv {
            facts: &facts,
            vars: &vars,
            iter: None,
        })
        .unwrap()
    }

    #[test]
    fn simple_facts() {
        assert!(check("facts.is_rust"));
        assert!(!check("facts.is_node"));
        assert!(check("not facts.is_node"));
    }

    #[test]
    fn integer_comparison() {
        assert!(check("facts.n_files > 0"));
        assert!(check("facts.n_files == 42"));
        assert!(!check("facts.n_files < 10"));
        assert!(check("facts.n_files >= 42"));
    }

    #[test]
    fn string_equality() {
        assert!(check("facts.primary == \"Rust\""));
        assert!(!check("facts.primary == \"Go\""));
    }

    #[test]
    fn logical_ops_short_circuit() {
        assert!(check("facts.is_rust and facts.n_files > 0"));
        assert!(check("facts.is_node or facts.is_rust"));
        assert!(!check("facts.is_node and facts.nonexistent == 5"));
    }

    #[test]
    fn in_list() {
        assert!(check("facts.primary in [\"Rust\", \"Go\"]"));
        assert!(!check("facts.primary in [\"Python\", \"Java\"]"));
    }

    #[test]
    fn in_string_is_substring() {
        assert!(check("\"cme\" in vars.org"));
        assert!(!check("\"Xyz\" in vars.org"));
    }

    #[test]
    fn matches_regex() {
        assert!(check("vars.org matches \"^Acme\""));
        assert!(check("vars.year matches \"^\\\\d{4}$\""));
        assert!(!check("vars.org matches \"^Xyz\""));
    }

    #[test]
    fn parentheses_override_precedence() {
        assert!(check(
            "(facts.is_node or facts.is_rust) and facts.n_files > 0"
        ));
        assert!(!check("facts.is_node or facts.is_rust and facts.is_node"));
        // Precedence: and binds tighter than or, so this is
        // `is_node or (is_rust and is_node)` == false or (true and false) == false.
    }

    #[test]
    fn unknown_facts_are_null_and_falsy() {
        assert!(!check("facts.nonexistent"));
        assert!(check("not facts.nonexistent"));
    }

    #[test]
    fn unknown_vars_are_null() {
        assert!(!check("vars.not_set"));
    }

    #[test]
    fn null_equals_null() {
        assert!(check("facts.nonexistent == null"));
    }

    #[test]
    fn parse_rejects_bare_equals() {
        let e = parse("facts.x = 1").unwrap_err();
        matches!(e, WhenError::Parse { .. });
    }

    #[test]
    fn parse_rejects_bang_alone() {
        let e = parse("!facts.x").unwrap_err();
        matches!(e, WhenError::Parse { .. });
    }

    #[test]
    fn parse_rejects_invalid_identifier_namespace() {
        let e = parse("ctx.x").unwrap_err();
        let WhenError::Parse { message, .. } = e else {
            panic!();
        };
        assert!(message.contains("facts.NAME"));
    }

    #[test]
    fn parse_rejects_matches_with_non_literal_rhs() {
        let e = parse("vars.org matches vars.pattern").unwrap_err();
        let WhenError::Parse { message, .. } = e else {
            panic!();
        };
        assert!(message.contains("string literal"));
    }

    #[test]
    fn parse_rejects_invalid_regex() {
        let e = parse("vars.org matches \"[unclosed\"").unwrap_err();
        matches!(e, WhenError::Regex(_));
    }

    #[test]
    fn evaluate_rejects_ordering_mixed_types() {
        let (facts, vars) = env();
        let expr = parse("facts.primary > facts.n_files").unwrap();
        let result = expr.evaluate(&WhenEnv {
            facts: &facts,
            vars: &vars,
            iter: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn string_escapes() {
        let (facts, vars) = env();
        let expr = parse("vars.org == \"Acme Corp\"").unwrap();
        assert!(
            expr.evaluate(&WhenEnv {
                facts: &facts,
                vars: &vars,
                iter: None,
            })
            .unwrap()
        );
    }

    #[test]
    fn nested_not_and_or() {
        assert!(check(
            "not (facts.is_node or (facts.n_files == 0 and facts.is_rust))"
        ));
    }

    // ─── iter namespace ──────────────────────────────────────────

    use crate::walker::{FileEntry, FileIndex};
    use std::path::Path;

    fn idx(paths: &[(&str, bool)]) -> FileIndex {
        FileIndex::from_entries(
            paths
                .iter()
                .map(|(p, is_dir)| FileEntry {
                    path: Path::new(p).into(),
                    is_dir: *is_dir,
                    size: 1,
                })
                .collect(),
        )
    }

    fn check_iter(src: &str, iter_path: &Path, is_dir: bool, index: &FileIndex) -> bool {
        let (facts, vars) = env();
        let expr = parse(src).unwrap();
        expr.evaluate(&WhenEnv {
            facts: &facts,
            vars: &vars,
            iter: Some(IterEnv {
                path: iter_path,
                is_dir,
                index,
            }),
        })
        .unwrap()
    }

    #[test]
    fn iter_namespace_parses_and_resolves_value_fields() {
        let index = idx(&[("crates/alint-core", true)]);
        assert!(check_iter(
            "iter.path == \"crates/alint-core\"",
            Path::new("crates/alint-core"),
            true,
            &index,
        ));
        assert!(check_iter(
            "iter.basename == \"alint-core\"",
            Path::new("crates/alint-core"),
            true,
            &index,
        ));
        assert!(check_iter(
            "iter.parent_name == \"crates\"",
            Path::new("crates/alint-core"),
            true,
            &index,
        ));
        assert!(check_iter(
            "iter.is_dir",
            Path::new("crates/alint-core"),
            true,
            &index,
        ));
    }

    #[test]
    fn iter_has_file_matches_literal_child() {
        let index = idx(&[
            ("crates/alint-core", true),
            ("crates/alint-core/Cargo.toml", false),
            ("crates/alint-core/src", true),
            ("crates/alint-core/src/lib.rs", false),
            ("crates/other", true),
            ("crates/other/Cargo.toml", false),
        ]);
        assert!(check_iter(
            "iter.has_file(\"Cargo.toml\")",
            Path::new("crates/alint-core"),
            true,
            &index,
        ));
        assert!(!check_iter(
            "iter.has_file(\"package.json\")",
            Path::new("crates/alint-core"),
            true,
            &index,
        ));
    }

    #[test]
    fn iter_has_file_supports_recursive_glob() {
        let index = idx(&[
            ("pkg", true),
            ("pkg/src", true),
            ("pkg/src/main.rs", false),
            ("pkg/src/inner", true),
            ("pkg/src/inner/lib.rs", false),
        ]);
        assert!(check_iter(
            "iter.has_file(\"**/*.rs\")",
            Path::new("pkg"),
            true,
            &index,
        ));
        assert!(!check_iter(
            "iter.has_file(\"**/*.py\")",
            Path::new("pkg"),
            true,
            &index,
        ));
    }

    #[test]
    fn iter_has_file_returns_false_for_file_iteration() {
        let index = idx(&[("a.rs", false)]);
        assert!(!check_iter(
            "iter.has_file(\"x\")",
            Path::new("a.rs"),
            false,
            &index,
        ));
    }

    #[test]
    fn iter_references_outside_iter_context_are_falsy() {
        // Outside an iteration, `iter.X` resolves to null and
        // `iter.has_file(...)` to false — same "missing fact"
        // convention that `facts.unknown` already follows.
        assert!(!check("iter.path"));
        assert!(check("iter.path == null"));
        assert!(!check("iter.has_file(\"X\")"));
    }

    #[test]
    fn iter_has_file_can_compose_with_boolean_logic() {
        let index = idx(&[("pkg", true), ("pkg/Cargo.toml", false), ("other", true)]);
        assert!(check_iter(
            "iter.has_file(\"Cargo.toml\") and iter.is_dir",
            Path::new("pkg"),
            true,
            &index,
        ));
        assert!(!check_iter(
            "iter.has_file(\"BUILD\") or iter.has_file(\"BUILD.bazel\")",
            Path::new("pkg"),
            true,
            &index,
        ));
    }

    #[test]
    fn parse_rejects_call_on_non_iter_namespace() {
        let e = parse("facts.something(\"x\")").unwrap_err();
        let WhenError::Parse { message, .. } = e else {
            panic!("expected parse error, got {e:?}");
        };
        assert!(
            message.contains("only available on `iter`"),
            "msg: {message}"
        );
    }

    #[test]
    fn parse_rejects_unknown_iter_method() {
        let e = parse("iter.bogus(\"x\")").unwrap_err();
        let WhenError::Parse { message, .. } = e else {
            panic!("expected parse error, got {e:?}");
        };
        assert!(message.contains("unknown iter method"), "msg: {message}");
    }

    #[test]
    fn evaluate_rejects_has_file_with_non_string_arg() {
        let (facts, vars) = env();
        let index = FileIndex::default();
        let expr = parse("iter.has_file(42)").unwrap();
        let err = expr
            .evaluate(&WhenEnv {
                facts: &facts,
                vars: &vars,
                iter: Some(IterEnv {
                    path: Path::new("p"),
                    is_dir: true,
                    index: &index,
                }),
            })
            .unwrap_err();
        let WhenError::Eval(msg) = err else {
            panic!("expected eval error");
        };
        assert!(msg.contains("must be a string"), "msg: {msg}");
    }
}
