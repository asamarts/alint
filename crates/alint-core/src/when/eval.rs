use super::{CmpOp, IterEnv, Namespace, Value, WhenEnv, WhenError, WhenExpr};

use crate::scope::Scope;

// ─── Evaluator ───────────────────────────────────────────────────────

/// Maximum evaluator recursion depth. A `when:` AST can be arbitrarily
/// tall — deep parens, or a long left-nested `and`/`or` chain (built
/// iteratively by the parser but walked recursively here on tree height)
/// — so a crafted expression from an untrusted `extends:` ruleset would
/// overflow the stack (small on a rayon worker) and abort the process.
/// Bail loudly past this. 64 is far beyond any real expression and stays
/// safe even for a debug build on a small (~2 MiB) stack. Mirrors the
/// parser's `MAX_DEPTH`.
const MAX_EVAL_DEPTH: usize = 64;

pub(super) fn eval(e: &WhenExpr, env: &WhenEnv<'_>) -> Result<Value, WhenError> {
    eval_at(e, env, 0)
}

fn eval_at(e: &WhenExpr, env: &WhenEnv<'_>, depth: usize) -> Result<Value, WhenError> {
    if depth > MAX_EVAL_DEPTH {
        return Err(WhenError::Eval(
            "expression nests too deeply to evaluate (max depth 64)".into(),
        ));
    }
    let depth = depth + 1;
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
            // `env.X`: an injected map (tests) takes precedence;
            // otherwise read the live process environment. Env is
            // constant during a run, so the eval-time read is
            // equivalent to a load-time snapshot. Unset → null
            // (falsy), matching the "missing fact is falsy" rule.
            Namespace::Env => {
                let value = match env.env {
                    Some(map) => map.get(name).cloned(),
                    None => std::env::var(name).ok(),
                };
                Ok(value.map_or(Value::Null, Value::String))
            }
        },
        WhenExpr::Call { ns, method, args } => match ns {
            Namespace::Iter => eval_iter_call(method, args, env, depth),
            // Parser rejects calls on non-iter namespaces, but be
            // defensive in case the AST is hand-built somewhere.
            _ => Err(WhenError::Eval(format!(
                "function-call evaluation not supported on namespace {ns:?}"
            ))),
        },
        WhenExpr::Not(inner) => Ok(Value::Bool(!eval_at(inner, env, depth)?.truthy())),
        WhenExpr::And(l, r) => {
            let lv = eval_at(l, env, depth)?;
            if !lv.truthy() {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(eval_at(r, env, depth)?.truthy()))
        }
        WhenExpr::Or(l, r) => {
            let lv = eval_at(l, env, depth)?;
            if lv.truthy() {
                return Ok(Value::Bool(true));
            }
            Ok(Value::Bool(eval_at(r, env, depth)?.truthy()))
        }
        WhenExpr::Cmp { left, op, right } => {
            let lv = eval_at(left, env, depth)?;
            let rv = eval_at(right, env, depth)?;
            Ok(Value::Bool(apply_cmp(&lv, *op, &rv)?))
        }
        WhenExpr::Matches { left, pattern } => {
            let lv = eval_at(left, env, depth)?;
            match lv {
                Value::String(s) => Ok(Value::Bool(pattern.is_match(&s))),
                // A missing fact (Null) matches nothing → falsy, mirroring how
                // `null == "x"` is falsy (not an error). A non-string *value*
                // (bool/int/list) is still a config-type error (L10).
                Value::Null => Ok(Value::Bool(false)),
                other => Err(WhenError::Eval(format!(
                    "`matches` left-hand side must be a string; got {}",
                    other.type_name()
                ))),
            }
        }
        WhenExpr::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval_at(item, env, depth)?);
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
fn eval_iter_call(
    method: &str,
    args: &[WhenExpr],
    env: &WhenEnv<'_>,
    depth: usize,
) -> Result<Value, WhenError> {
    match method {
        "has_file" => {
            if args.len() != 1 {
                return Err(WhenError::Eval(format!(
                    "iter.has_file expects exactly 1 argument; got {}",
                    args.len()
                )));
            }
            let pattern = match eval_at(&args[0], env, depth)? {
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
        // Only the four ordering ops are ever routed here; equality and
        // membership ops are handled by the caller before this point.
        other => unreachable!("cmp_ord called with non-ordering op: {other:?}"),
    }
}
