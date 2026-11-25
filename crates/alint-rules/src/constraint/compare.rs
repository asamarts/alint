//! `compare` assertion (ADR-0050 D4): pairwise row comparison (`eq`, `ne`,
//! `lt`, `le`, `gt`, `ge`) under `exact`, `lexical`, or `numeric` ordering,
//! failing loud on `Null` or `Missing` operands and non-conforming types.

use std::cmp::Ordering;

use super::{Related, Run, Spot, type_of};
use crate::constraint::values::{Key, Scalar};
use crate::constraint::{CompareAssertion, CompareLiteral, CompareOp, CompareOrder, CompareTarget};

impl<'r> Run<'r> {
    pub(in crate::constraint) fn compare(&mut self, compare: &CompareAssertion) {
        let Some(field_at) = self.position(&compare.field) else {
            return;
        };
        match &compare.target {
            CompareTarget::Field(other) => {
                let Some(other_at) = self.position(other) else {
                    return;
                };
                self.compare_to_field(compare, field_at, other_at, other);
            }
            CompareTarget::Value(literal) => {
                self.compare_to_value(compare, field_at, literal);
            }
            CompareTarget::Row(literals) => {
                self.compare_to_row(compare, field_at, literals);
            }
        }
    }

    fn compare_to_field(
        &mut self,
        compare: &CompareAssertion,
        field_at: usize,
        other_at: usize,
        other: &str,
    ) {
        let pair_sensitive =
            self.redact_all || self.sensitive(field_at) || self.sensitive(other_at);
        for row in 0..self.rows.rows.len() {
            let (Some(lhs), Some(rhs)) = (
                self.scalar(row, field_at, "assert.compare.field"),
                self.scalar(row, other_at, "assert.compare.to.field"),
            ) else {
                return;
            };
            let Some(passed) = self.eval_compare_pair(
                compare.order,
                compare.op,
                (&lhs, row, field_at, "assert.compare.field"),
                (&rhs, Some((row, other_at)), "assert.compare.to.field"),
            ) else {
                return;
            };
            if passed {
                continue;
            }
            if self.at_finding_cap() {
                self.suppress_findings(1, false);
                continue;
            }
            let typed = compare.order == CompareOrder::Exact
                && lhs.kind().is_some()
                && rhs.kind().is_some()
                && lhs.kind() != rhs.kind();
            let message = format!(
                "`{}` is {} and `{other}` is {} in this row; expected `{}` {} `{other}`",
                compare.field,
                show_operand(&lhs, pair_sensitive, typed),
                show_operand(&rhs, pair_sensitive, typed),
                compare.field,
                compare.op.symbol(),
            );
            let mut key = Key::new("compare");
            key.part(compare.op.key())
                .row(self.own(row), self.hide_keys);
            let other_cell = self.cell(row, other_at);
            let label = format!("`{other}` in this row");
            let other_place = Spot::Cell(other_cell, pair_sensitive || self.point(other_at));
            let related = self.related_at(vec![(other_place, label.as_str())], 0);
            let at = self.spot_point(row, field_at, pair_sensitive);
            self.report(at, message, key, related, Some(row), None);
        }
    }

    fn compare_to_value(
        &mut self,
        compare: &CompareAssertion,
        field_at: usize,
        literal: &CompareLiteral,
    ) {
        let hide = self.redact_all || self.sensitive(field_at) || self.rule.hide_literals;
        let rhs_scalar = match literal {
            CompareLiteral::Scalar(v) => {
                Scalar::of_value(v).expect("builder validated scalar literal")
            }
            CompareLiteral::Number(raw) => Scalar::Number(raw.as_str().into()),
        };
        for row in 0..self.rows.rows.len() {
            let Some(lhs) = self.scalar(row, field_at, "assert.compare.field") else {
                return;
            };
            let Some(passed) = self.eval_compare_pair(
                compare.order,
                compare.op,
                (&lhs, row, field_at, "assert.compare.field"),
                (&rhs_scalar, None, "assert.compare.to.value"),
            ) else {
                return;
            };
            if passed {
                continue;
            }
            if self.at_finding_cap() {
                self.suppress_findings(1, false);
                continue;
            }
            let typed = compare.order == CompareOrder::Exact
                && lhs.kind().is_some()
                && rhs_scalar.kind().is_some()
                && lhs.kind() != rhs_scalar.kind();
            let message = format!(
                "`{}` is {} here; expected {} {}",
                compare.field,
                show_operand(&lhs, hide, typed),
                compare.op.symbol(),
                show_operand(&rhs_scalar, hide, typed),
            );
            let mut key = Key::new("compare");
            key.part(compare.op.key())
                .row(self.own(row), self.hide_keys);
            let at = self.spot_point(row, field_at, hide);
            self.report(at, message, key, Related::empty(), Some(row), None);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn compare_to_row(
        &mut self,
        compare: &CompareAssertion,
        field_at: usize,
        literals: &[(String, alint_core::relation::Value)],
    ) {
        let mut wanted = Vec::with_capacity(literals.len());
        for (name, literal) in literals {
            let Some(position) = self.position(name) else {
                return;
            };
            let literal = Scalar::of_value(literal).expect("builder validated scalar literal");
            wanted.push((position, literal));
        }
        let positions: Vec<usize> = wanted.iter().map(|(pos, _)| *pos).collect();
        let literal_values: Vec<Scalar> = wanted.iter().map(|(_, lit)| lit.clone()).collect();
        let source_desc = if self.redact_all || self.rule.hide_literals {
            "the literals `to.row` names".to_string()
        } else {
            self.describe(&positions, &literal_values)
        };
        let Some(source) =
            self.source_row_at(field_at, &wanted, &source_desc, "assert.compare.to.row")
        else {
            return;
        };
        let Some(expected) = self.scalar(source, field_at, "assert.compare.field") else {
            return;
        };
        let hide = self.redact_all || self.sensitive(field_at) || self.rule.hide_literals;
        let source_cell = self.cell(source, field_at);
        let source_at = self.spot_point(source, field_at, hide);
        let source_shown = self.located_at(source_at);
        if expected.is_null() {
            let message = format!(
                "`assert.compare.to.row`: the source row ({source_desc}, at {source_shown}) holds \
                 no value of `{}`",
                compare.field
            );
            self.fail(Some(source_cell.at.resource.path().as_path()), message);
            return;
        }
        // Validate the source row's operand type even on 1-row relations where
        // the comparison loop below has zero non-source rows.
        match compare.order {
            CompareOrder::Exact => {}
            CompareOrder::Lexical => {
                if !matches!(expected, Scalar::String(_)) {
                    let message = format!(
                        "`assert.compare.to.row`: `{}` holds {} at {}, but `order: lexical` \
                         compares strings",
                        compare.field,
                        type_of(source_cell),
                        self.located(source_cell)
                    );
                    self.fail(Some(source_cell.at.resource.path().as_path()), message);
                    return;
                }
            }
            CompareOrder::Numeric => {
                if parse_finite_number(&expected).is_none() {
                    let message = format!(
                        "`assert.compare.to.row`: `{}` holds {} at {}, but `order: numeric` \
                         compares finite numbers",
                        compare.field,
                        type_of(source_cell),
                        self.located(source_cell)
                    );
                    self.fail(Some(source_cell.at.resource.path().as_path()), message);
                    return;
                }
            }
        }
        for row in 0..self.rows.rows.len() {
            if row == source {
                continue;
            }
            let Some(lhs) = self.scalar(row, field_at, "assert.compare.field") else {
                return;
            };
            let Some(passed) = self.eval_compare_pair(
                compare.order,
                compare.op,
                (&lhs, row, field_at, "assert.compare.field"),
                (&expected, Some((source, field_at)), "assert.compare.to.row"),
            ) else {
                return;
            };
            if passed {
                continue;
            }
            if self.at_finding_cap() {
                self.suppress_findings(1, false);
                continue;
            }
            let typed = compare.order == CompareOrder::Exact
                && lhs.kind().is_some()
                && expected.kind().is_some()
                && lhs.kind() != expected.kind();
            let message = format!(
                "`{}` is {} here; expected {} {} (from the source row {source_desc}, at \
                 {source_shown})",
                compare.field,
                show_operand(&lhs, hide, typed),
                compare.op.symbol(),
                show_operand(&expected, hide, typed),
            );
            let mut key = Key::new("compare");
            key.part(compare.op.key())
                .row(self.own(row), self.hide_keys);
            let related = self.related_at(vec![(source_at, "the source row")], 0);
            let at = self.spot_point(row, field_at, hide);
            self.report(at, message, key, related, Some(row), None);
        }
    }

    /// Evaluate one pair of operands under `(order, op)`, failing loud on
    /// `Null`, `Missing`, or non-conforming types (ADR-0050 D4).
    fn eval_compare_pair(
        &mut self,
        order: CompareOrder,
        op: CompareOp,
        (lhs, lhs_row, lhs_pos, lhs_opt): (&Scalar, usize, usize, &str),
        (rhs, rhs_site, rhs_opt): (&Scalar, Option<(usize, usize)>, &str),
    ) -> Option<bool> {
        if lhs.is_null() {
            let cell = self.cell(lhs_row, lhs_pos);
            let message = format!(
                "`{lhs_opt}`: `{}` is {} at {}; `compare` requires non-null present values \
                 (filter nullable/optional rows with `where:`)",
                self.name(lhs_pos),
                if lhs.is_missing() { "missing" } else { "null" },
                self.located(cell)
            );
            self.fail(Some(cell.at.resource.path().as_path()), message);
            return None;
        }
        if rhs.is_null() {
            let (path, where_desc, field_desc) = match rhs_site {
                Some((r, p)) => {
                    let cell = self.cell(r, p);
                    (
                        Some(cell.at.resource.path().as_path()),
                        format!(" at {}", self.located(cell)),
                        format!("`{}`", self.name(p)),
                    )
                }
                None => (None, String::new(), "target literal".to_string()),
            };
            let message = format!(
                "`{rhs_opt}`: {field_desc} is {}{where_desc}; `compare` requires non-null \
                 present values (filter nullable/optional rows with `where:`)",
                if rhs.is_missing() { "missing" } else { "null" }
            );
            self.fail(path, message);
            return None;
        }
        match order {
            CompareOrder::Exact => Some(match op {
                CompareOp::Eq => lhs == rhs,
                CompareOp::Ne => lhs != rhs,
                _ => unreachable!("builder refuses ordered ops with order: exact"),
            }),
            CompareOrder::Lexical => {
                let Scalar::String(a) = lhs else {
                    let cell = self.cell(lhs_row, lhs_pos);
                    let message = format!(
                        "`{lhs_opt}`: `{}` holds {} at {}, but `order: lexical` compares strings",
                        self.name(lhs_pos),
                        type_of(cell),
                        self.located(cell)
                    );
                    self.fail(Some(cell.at.resource.path().as_path()), message);
                    return None;
                };
                let Scalar::String(b) = rhs else {
                    let (path, desc) = self.rhs_type_desc(rhs_site);
                    let message =
                        format!("`{rhs_opt}`: {desc}, but `order: lexical` compares strings");
                    self.fail(path, message);
                    return None;
                };
                Some(op.matches(a.cmp(b)))
            }
            CompareOrder::Numeric => {
                let Some(a) = parse_finite_number(lhs) else {
                    let cell = self.cell(lhs_row, lhs_pos);
                    let message = format!(
                        "`{lhs_opt}`: `{}` holds {} at {}, but `order: numeric` compares finite \
                         numbers",
                        self.name(lhs_pos),
                        type_of(cell),
                        self.located(cell)
                    );
                    self.fail(Some(cell.at.resource.path().as_path()), message);
                    return None;
                };
                let Some(b) = parse_finite_number(rhs) else {
                    let (path, desc) = self.rhs_type_desc(rhs_site);
                    let message = format!(
                        "`{rhs_opt}`: {desc}, but `order: numeric` compares finite numbers"
                    );
                    self.fail(path, message);
                    return None;
                };
                let ord = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
                Some(op.matches(ord))
            }
        }
    }

    fn rhs_type_desc(
        &self,
        rhs_site: Option<(usize, usize)>,
    ) -> (Option<&'r std::path::Path>, String) {
        match rhs_site {
            Some((r, p)) => {
                let cell = self.cell(r, p);
                (
                    Some(cell.at.resource.path().as_path()),
                    format!(
                        "`{}` holds {} at {}",
                        self.name(p),
                        type_of(cell),
                        self.located(cell)
                    ),
                )
            }
            None => (
                None,
                "target literal is not of the expected type".to_string(),
            ),
        }
    }
}

/// Parse a JSON/YAML/TOML numeric lexeme into a finite `f64`, supporting TOML
/// underscore separators (`1_000`, `1_000.5`) and TOML/YAML radix integers
/// (`0x10`, `0o755`, `0b1010`), while rejecting non-finite values (`inf`, `NaN`).
fn parse_finite_number(scalar: &Scalar) -> Option<f64> {
    let Scalar::Number(lexeme) = scalar else {
        return None;
    };
    let clean: std::borrow::Cow<'_, str> = if lexeme.contains('_') {
        std::borrow::Cow::Owned(lexeme.replace('_', ""))
    } else {
        std::borrow::Cow::Borrowed(lexeme.as_ref())
    };
    let (negative, rest) = match clean.as_bytes().first() {
        Some(b'-') => (true, &clean[1..]),
        Some(b'+') => (false, &clean[1..]),
        _ => (false, clean.as_ref()),
    };
    if let Some((radix, digits)) = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .map(|d| (16, d))
        .or_else(|| {
            rest.strip_prefix("0o")
                .or_else(|| rest.strip_prefix("0O"))
                .map(|d| (8, d))
        })
        .or_else(|| {
            rest.strip_prefix("0b")
                .or_else(|| rest.strip_prefix("0B"))
                .map(|d| (2, d))
        })
    {
        let unsigned = u128::from_str_radix(digits, radix).ok()?;
        #[allow(clippy::cast_precision_loss)]
        let mut value = unsigned as f64;
        if negative {
            value = -value;
        }
        return value.is_finite().then_some(value);
    }
    let parsed: f64 = clean.parse().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn show_operand(scalar: &Scalar, sensitive: bool, typed: bool) -> String {
    if typed {
        scalar.show_typed(sensitive)
    } else {
        scalar.show(sensitive)
    }
}
