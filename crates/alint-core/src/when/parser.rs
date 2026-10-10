use super::lexer::{Tok, is_known_iter_method};
use super::{CmpOp, Namespace, Value, WhenError, WhenExpr};

use regex::Regex;

// ─── Parser ──────────────────────────────────────────────────────────

/// Maximum `(`/`[`/call-args nesting depth the parser will descend.
/// `parse_expr` is mutually recursive through `parse_primary`, so nesting
/// depth == parse recursion depth; an adversarial `when:` from an
/// untrusted `extends:` ruleset (e.g. `"(".repeat(1_000_000)`) would
/// otherwise overflow the parser stack — an uncatchable abort, the
/// strongest determinism violation. The cap is deliberately conservative:
/// one nesting level spans six mutually-recursive frames (the large
/// `parse_primary` among them), and a *debug* build on a small (~2 MiB)
/// test / rayon-worker stack overflows well before a few hundred levels —
/// so 64 (still orders of magnitude beyond any real expression) is the safe
/// ceiling, not a higher round number. The evaluator carries a matching
/// `MAX_EVAL_DEPTH`.
const MAX_DEPTH: usize = 64;

pub(super) struct Parser {
    tokens: Vec<(Tok, usize)>,
    pos: usize,
    depth: usize,
}

impl Parser {
    pub(super) fn new(tokens: Vec<(Tok, usize)>) -> Self {
        Self {
            tokens,
            pos: 0,
            depth: 0,
        }
    }
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

    pub(super) fn expect_eof(&mut self) -> Result<(), WhenError> {
        if self.peek().is_some() {
            Err(self.err("unexpected trailing token"))
        } else {
            Ok(())
        }
    }

    /// Deepen the tracked expression depth by one and fail loudly past
    /// `MAX_DEPTH`. The chain loops (`parse_or`/`parse_and`) call this per
    /// operator so a long *flat* chain is bounded too — they're iterative and
    /// never re-enter `parse_expr`, where the paren/bracket re-entry guard
    /// lives (H5 flat-chain gap).
    fn bump_depth(&mut self) -> Result<(), WhenError> {
        self.bump_depth_for("expression nests too deeply (max depth 64)")
    }

    /// [`bump_depth`](Self::bump_depth) for an operator chain, whose limit is
    /// hit by a long FLAT `a or b or …` list rather than visible nesting; say
    /// so, or the "nests too deeply" message points at the wrong cause.
    fn bump_chain_depth(&mut self) -> Result<(), WhenError> {
        self.bump_depth_for(
            "expression is too complex: more than 64 combined levels of nesting and \
             chained `and` / `or` / `not` terms; split it with a fact",
        )
    }

    fn bump_depth_for(&mut self, message: &str) -> Result<(), WhenError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(self.err(message));
        }
        Ok(())
    }

    pub(super) fn parse_expr(&mut self) -> Result<WhenExpr, WhenError> {
        // Bound recursion before descending: `parse_primary` re-enters
        // `parse_expr` for every `(`, `[` and call-arg list, so this is the
        // single re-entry point. Deep input fails loudly here instead of
        // overflowing the stack. Decrement on the way out so sibling
        // sub-expressions (list items, call args) don't accumulate depth.
        self.bump_depth()?;
        let result = self.parse_or();
        self.depth -= 1;
        result
    }

    fn parse_or(&mut self) -> Result<WhenExpr, WhenError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Tok::KwOr)) {
            self.advance();
            // Each `or` node deepens the built AST's left spine by one. Bump per
            // operator and DO NOT restore: `depth` must bound the CUMULATIVE
            // structural depth of the AST (a `(…) or a or a …` construction nests
            // parens AND chains at each level), so a later recursive Drop / eval
            // of a tall tree can't overflow the stack. `parse_expr` only unwinds
            // its own single nesting bump, so chain depth correctly accumulates
            // along the spine (H5 flat-chain gap). Restoring it here would let a
            // ~MAX_DEPTH² tree parse and abort on Drop — DO NOT.
            self.bump_chain_depth()?;
            let right = self.parse_and()?;
            left = WhenExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<WhenExpr, WhenError> {
        let mut left = self.parse_not()?;
        while matches!(self.peek(), Some(Tok::KwAnd)) {
            self.advance();
            // See `parse_or`: bump per `and` node and never restore, so `depth`
            // bounds the AST's cumulative structural depth (Drop/eval-safe).
            self.bump_chain_depth()?;
            let right = self.parse_not()?;
            left = WhenExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<WhenExpr, WhenError> {
        if matches!(self.peek(), Some(Tok::KwNot)) {
            self.advance();
            // Recurse so `not not x` parses. Each `not` nests the AST (and this
            // call stack) by one, so it counts against the depth bound while its
            // operand is parsed: a `not not not …` run is bounded like nesting.
            // Restore it afterwards, unlike a chain operator: a `not` is a unary
            // node over its OWN operand and does not deepen the surrounding
            // chain's spine, so `not a and not b and …` must cost one level per
            // `and`, not two (33 such terms used to hit the 64 limit).
            self.bump_chain_depth()?;
            let inner = self.parse_not();
            self.depth -= 1;
            return Ok(WhenExpr::Not(Box::new(inner?)));
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
                    "env" => Namespace::Env,
                    other => {
                        return Err(WhenError::Parse {
                            pos,
                            message: format!(
                                "unknown identifier {other:?}; only `facts.NAME`, \
                                 `vars.NAME`, `iter.NAME`, and `env.NAME` are allowed"
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

#[cfg(test)]
mod tests {
    use super::super::{WhenEnv, WhenError, parse};
    use crate::facts::{FactValue, FactValues};
    use std::collections::HashMap;

    /// `not facts.f1 and not facts.f2 and …` with `n` terms.
    fn negated_conjunction(n: usize) -> String {
        (1..=n)
            .map(|i| format!("not facts.f{i}"))
            .collect::<Vec<_>>()
            .join(" and ")
    }

    #[test]
    fn a_long_conjunction_of_negations_parses_and_evaluates() {
        // Each `not` bumped the cumulative depth without restoring it, so 33
        // `not x and` terms (2 levels each) hit the 64 limit although v0.17
        // parsed them. A `not` only nests its own operand.
        let expr = parse(&negated_conjunction(60)).expect("60 negated terms parse");
        let mut facts = FactValues::new();
        for i in 1..=60 {
            facts.insert(format!("f{i}"), FactValue::Bool(false));
        }
        let vars = HashMap::new();
        assert!(expr.evaluate(&WhenEnv::new(&facts, &vars)).unwrap());
    }

    #[test]
    fn a_deep_not_run_is_still_bounded() {
        let err = parse(&format!("{}facts.x", "not ".repeat(65))).unwrap_err();
        assert!(
            matches!(&err, WhenError::Parse { message, .. } if message.contains("too complex")),
            "{err:?}"
        );
        assert!(parse(&format!("{}facts.x", "not ".repeat(60))).is_ok());
        // A `not` over a parenthesised chain keeps that chain's depth: nesting
        // the 60-term chain under `not (…) and …` stays rejected.
        let mut e = negated_conjunction(60);
        for _ in 0..10 {
            e = format!("not ({e}) and facts.y");
        }
        assert!(parse(&e).is_err());
    }
}
