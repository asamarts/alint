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
//! NS         := 'facts' | 'vars' | 'iter' | 'env'
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
    /// Environment variables. `env.CI`, `env.GITHUB_ACTIONS`, etc.
    /// Resolved at evaluation time (env is constant during a run);
    /// an unset variable evaluates to `null`, matching the
    /// "missing fact is falsy" rule. Value-only — there are no
    /// callable methods on `env`.
    Env,
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
    /// Optional environment-variable snapshot backing the `env.*`
    /// namespace. `None` — the production default — means the
    /// evaluator reads the live process environment via
    /// `std::env::var` (env is constant during a run, so the
    /// eval-time read matches a load-time snapshot). Tests inject
    /// a fake map via [`WhenEnv::with_env`] so they never touch
    /// the real environment (Rust 2024 marks `set_var` unsafe).
    pub env: Option<&'a HashMap<String, String>>,
}

impl<'a> WhenEnv<'a> {
    /// Construct a `WhenEnv` without iteration context — the
    /// shape every existing call site uses. `iter.*` references
    /// in the expression resolve to null / false; `env.*` reads
    /// the live process environment.
    #[must_use]
    pub fn new(facts: &'a FactValues, vars: &'a HashMap<String, String>) -> Self {
        Self {
            facts,
            vars,
            iter: None,
            env: None,
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

    /// Back the `env.*` namespace with an explicit map instead of
    /// the live process environment. Used by tests to resolve
    /// `env.X` hermetically.
    #[must_use]
    pub fn with_env(mut self, env: &'a HashMap<String, String>) -> Self {
        self.env = Some(env);
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
    parse_inner(src).map_err(|e| enrich_diagnostic(src, e))
}

fn parse_inner(src: &str) -> Result<WhenExpr, WhenError> {
    let tokens = lex(src)?;
    let mut p = Parser::new(tokens);
    let expr = p.parse_expr()?;
    p.expect_eof()?;
    Ok(expr)
}

/// Enrich a [`WhenError::Parse`] with domain-specific hints for the
/// pitfalls catalogued in `docs/development/CONFIG-AUTHORING.md` § 12:
///
/// - **#12a** — `&&` / `||` / `!` symbols → suggest `and` / `or` / `not`.
/// - **#12b** — `iter.foo.bar(` method-call shapes → suggest the
///   `matches` operator or the bounded iter accessor set.
///
/// Only applies to `WhenError::Parse`; evaluation errors pass through
/// unchanged. The original message is preserved; hints are appended on
/// new lines so callers that just `Display` the error still get the
/// position info.
fn enrich_diagnostic(src: &str, err: WhenError) -> WhenError {
    let WhenError::Parse { pos, message } = err else {
        // Eval / Regex errors don't have positional context to
        // diagnose; pass them through unchanged.
        return err;
    };
    let hint = symbol_keyword_hint(src, pos).or_else(|| method_call_hint(src, pos));
    match hint {
        Some(h) => WhenError::Parse {
            pos,
            message: format!("{message}\n  hint: {h}"),
        },
        None => WhenError::Parse { pos, message },
    }
}

/// Detect `&&` / `||` / `!` near `pos` and return a keyword
/// suggestion. Pitfall #12a.
fn symbol_keyword_hint(src: &str, pos: usize) -> Option<&'static str> {
    let bytes = src.as_bytes();
    let at = bytes.get(pos).copied();
    let next = bytes.get(pos + 1).copied();
    let prev = pos.checked_sub(1).and_then(|p| bytes.get(p).copied());

    let _ = next; // kept for future second-character refinement
    match at {
        Some(b'&') if prev != Some(b'&') => {
            Some("`&&` is not a `when:` operator. Use the keyword `and` instead.")
        }
        Some(b'|') if prev != Some(b'|') => {
            Some("`||` is not a `when:` operator. Use the keyword `or` instead.")
        }
        Some(b'!') => Some("`!` is not a `when:` operator. Use the keyword `not` instead."),
        _ => None,
    }
}

/// Detect `iter.foo.bar(` method-call shapes anywhere in `src`
/// and return a hint. Pitfall #12b.
///
/// The `iter.*` accessors are a fixed set: `iter.path`,
/// `iter.basename`, `iter.parent_name`, `iter.is_dir`,
/// `iter.has_file(...)`. There are no string method calls; use the
/// `matches` operator for regex matching.
///
/// We use a global regex rather than a position-relative check
/// because the lexer's failure column for `iter.path.contains("foo")`
/// is on the second `.`, not the open paren — the position alone
/// doesn't carry enough context to infer the bad shape.
fn method_call_hint(src: &str, _pos: usize) -> Option<&'static str> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        // `iter.<ident>.<ident>(` — a double-dot chain off iter that
        // ends in a function-call-shaped token. Catches
        // `iter.path.contains(...)`, `iter.basename.starts_with(...)`,
        // `iter.parent_name.ends_with(...)`, etc.
        regex::Regex::new(r"\biter\.\w+\.\w+\s*\(").expect("static regex")
    });
    if re.is_match(src) {
        return Some(
            "`iter.*` accessors are a fixed set; method calls aren't supported. Use the `matches` \
             operator for regex matching, e.g. `iter.path matches \"node_modules\"`. The supported \
             accessors are documented in `docs/development/CONFIG-AUTHORING.md` § 12b.",
        );
    }
    None
}

impl WhenExpr {
    pub fn evaluate(&self, env: &WhenEnv<'_>) -> Result<bool, WhenError> {
        let v = eval(self, env)?;
        Ok(v.truthy())
    }
}

mod eval;
mod lexer;
mod parser;

use eval::eval;
use lexer::lex;
use parser::Parser;

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
            env: None,
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
            env: None,
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
                env: None,
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

    #[test]
    fn deeply_nested_input_is_a_parse_error_not_a_stack_overflow() {
        // Untrusted `extends:` rulesets reach the `when:` parser; deeply
        // nested parens must fail loudly here, never overflow the parser
        // stack (an uncatchable abort).
        let src = format!("{}true{}", "(".repeat(10_000), ")".repeat(10_000));
        let err = parse(&src).unwrap_err();
        assert!(matches!(err, WhenError::Parse { .. }), "{err:?}");
    }

    #[test]
    fn long_and_chain_is_an_eval_error_not_a_stack_overflow() {
        // A flat `a and a and …` chain parses iteratively but builds a tall
        // left-nested tree; `eval` recurses on tree height, so a crafted
        // chain must bail with an error rather than abort the process.
        let chain = vec!["facts.x"; 10_000].join(" and ");
        let expr = parse(&chain).expect("a flat and-chain parses");
        let (facts, vars) = env();
        let err = expr
            .evaluate(&WhenEnv {
                facts: &facts,
                vars: &vars,
                iter: None,
                env: None,
            })
            .unwrap_err();
        assert!(matches!(err, WhenError::Eval(_)), "{err:?}");
    }

    // ─── env namespace ───────────────────────────────────────────

    fn check_env(src: &str, vars_env: &[(&str, &str)]) -> bool {
        let (facts, vars) = env();
        let env_map: HashMap<String, String> = vars_env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let expr = parse(src).unwrap();
        expr.evaluate(&WhenEnv::new(&facts, &vars).with_env(&env_map))
            .unwrap()
    }

    #[test]
    fn env_namespace_resolves_injected_value() {
        assert!(check_env("env.CI == \"true\"", &[("CI", "true")]));
        assert!(check_env(
            "env.GITHUB_ACTIONS == \"true\" or env.CI == \"true\"",
            &[("CI", "true")],
        ));
    }

    #[test]
    fn env_unset_var_is_null_and_falsy() {
        assert!(!check_env("env.NOT_SET", &[]));
        assert!(check_env("env.NOT_SET == null", &[]));
        assert!(!check_env("env.NOT_SET == \"true\"", &[]));
    }

    #[test]
    fn env_composes_with_facts_and_vars() {
        assert!(check_env(
            "facts.is_rust and env.CI == \"true\"",
            &[("CI", "true")],
        ));
        assert!(!check_env(
            "facts.is_node and env.CI == \"true\"",
            &[("CI", "true")],
        ));
    }

    #[test]
    fn env_values_are_always_strings_compare_against_string_literals() {
        // env vars resolve to `String`, never `Int`. Comparing to a
        // bare integer literal is silently false (mixed-type `==`),
        // so users must quote: `env.PORT == "8080"`, not `== 8080`.
        assert!(check_env("env.PORT == \"8080\"", &[("PORT", "8080")]));
        assert!(!check_env("env.PORT == 8080", &[("PORT", "8080")]));
    }

    #[test]
    fn env_matches_and_in_operators() {
        assert!(check_env(
            "env.REF matches \"^refs/tags/\"",
            &[("REF", "refs/tags/v1.0",)]
        ));
        assert!(check_env(
            "\"prod\" in env.ENVIRONMENT",
            &[("ENVIRONMENT", "prod-east",)]
        ));
    }

    #[test]
    fn env_parses_as_valid_namespace() {
        // `env.X` parses cleanly (regression guard for the parser
        // namespace dispatch); a bogus namespace still rejects with
        // the updated allowed-list message.
        assert!(parse("env.CI == \"true\"").is_ok());
        let WhenError::Parse { message, .. } = parse("environ.CI").unwrap_err() else {
            panic!("expected parse error");
        };
        assert!(message.contains("env.NAME"), "msg: {message}");
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
            env: None,
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
                env: None,
            })
            .unwrap_err();
        let WhenError::Eval(msg) = err else {
            panic!("expected eval error");
        };
        assert!(msg.contains("must be a string"), "msg: {msg}");
    }
}
