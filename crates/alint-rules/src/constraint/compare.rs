//! `compare` assertion (ADR-0050 D4, D5): pairwise row comparison (`eq`, `ne`,
//! `lt`, `le`, `gt`, `ge`) under `exact`, `lexical`, `numeric`, or `semver`
//! ordering, failing loud on `Null` or `Missing` operands and non-conforming
//! types.

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
        let pair_sensitive = self.redact_all
            || self.rule.hide_literals
            || self.sensitive(field_at)
            || self.sensitive(other_at);
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
                pair_sensitive,
                (&lhs, row, field_at, "assert.compare.field"),
                (&rhs, None, Some((row, other_at)), "assert.compare.to.field"),
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
        let (rhs_scalar, rhs_semver) = match literal {
            CompareLiteral::Scalar(v) => (
                Scalar::of_value(v).expect("builder validated scalar literal"),
                None,
            ),
            CompareLiteral::Number(raw) => (Scalar::Number(raw.as_str().into()), None),
            CompareLiteral::Semver { raw, parsed } => {
                (Scalar::String(raw.as_str().into()), Some(parsed))
            }
        };
        for row in 0..self.rows.rows.len() {
            let Some(lhs) = self.scalar(row, field_at, "assert.compare.field") else {
                return;
            };
            let Some(passed) = self.eval_compare_pair(
                compare.order,
                compare.op,
                hide,
                (&lhs, row, field_at, "assert.compare.field"),
                (&rhs_scalar, rhs_semver, None, "assert.compare.to.value"),
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
        let mut expected_semver = None;
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
                if parse_exact_decimal(&expected).is_none() {
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
            CompareOrder::Semver => {
                let Scalar::String(raw) = &expected else {
                    let message = format!(
                        "`assert.compare.to.row`: `{}` holds {} at {}, but `order: semver` \
                         compares semantic version strings",
                        compare.field,
                        type_of(source_cell),
                        self.located(source_cell)
                    );
                    self.fail(Some(source_cell.at.resource.path().as_path()), message);
                    return;
                };
                match alint_core::semver_eval::parse_version(raw) {
                    Ok(v) => expected_semver = Some(v),
                    Err(err) => {
                        let detail = if hide {
                            "not a valid semantic version".to_string()
                        } else {
                            format!("not a valid semantic version ({err})")
                        };
                        let message = format!(
                            "`assert.compare.to.row`: `{}` is {} at {}: {detail}",
                            compare.field,
                            expected.show(hide),
                            self.located(source_cell)
                        );
                        self.fail(Some(source_cell.at.resource.path().as_path()), message);
                        return;
                    }
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
                hide,
                (&lhs, row, field_at, "assert.compare.field"),
                (
                    &expected,
                    expected_semver.as_ref(),
                    Some((source, field_at)),
                    "assert.compare.to.row",
                ),
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
        pair_sensitive: bool,
        (lhs, lhs_row, lhs_pos, lhs_opt): (&Scalar, usize, usize, &str),
        (rhs, rhs_semver, rhs_site, rhs_opt): (
            &Scalar,
            Option<&alint_core::semver_eval::SemverVersion>,
            Option<(usize, usize)>,
            &str,
        ),
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
                let Some(a) = parse_exact_decimal(lhs) else {
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
                let Some(b) = parse_exact_decimal(rhs) else {
                    let (path, desc) = self.rhs_type_desc(rhs_site);
                    let message = format!(
                        "`{rhs_opt}`: {desc}, but `order: numeric` compares finite numbers"
                    );
                    self.fail(path, message);
                    return None;
                };
                Some(op.matches(a.cmp(&b)))
            }
            CompareOrder::Semver => self.eval_compare_semver(
                op,
                pair_sensitive,
                (lhs, lhs_row, lhs_pos, lhs_opt),
                (rhs, rhs_semver, rhs_site, rhs_opt),
            ),
        }
    }

    fn eval_compare_semver(
        &mut self,
        op: CompareOp,
        pair_sensitive: bool,
        (lhs, lhs_row, lhs_pos, lhs_opt): (&Scalar, usize, usize, &str),
        (rhs, rhs_semver, rhs_site, rhs_opt): (
            &Scalar,
            Option<&alint_core::semver_eval::SemverVersion>,
            Option<(usize, usize)>,
            &str,
        ),
    ) -> Option<bool> {
        let hide = pair_sensitive || self.redact_all || self.sensitive(lhs_pos);
        let Scalar::String(a_raw) = lhs else {
            let cell = self.cell(lhs_row, lhs_pos);
            let message = format!(
                "`{lhs_opt}`: `{}` holds {} at {}, but `order: semver` compares semantic \
                 version strings",
                self.name(lhs_pos),
                type_of(cell),
                self.located(cell)
            );
            self.fail(Some(cell.at.resource.path().as_path()), message);
            return None;
        };
        let a = match alint_core::semver_eval::parse_version(a_raw) {
            Ok(v) => v,
            Err(err) => {
                let cell = self.cell(lhs_row, lhs_pos);
                let detail = if hide {
                    "not a valid semantic version".to_string()
                } else {
                    format!("not a valid semantic version ({err})")
                };
                let message = format!(
                    "`{lhs_opt}`: `{}` is {} at {}: {detail}",
                    self.name(lhs_pos),
                    lhs.show(hide),
                    self.located(cell)
                );
                self.fail(Some(cell.at.resource.path().as_path()), message);
                return None;
            }
        };
        if let Some(b) = rhs_semver {
            return Some(op.matches(a.cmp(b)));
        }
        let Scalar::String(b_raw) = rhs else {
            let (path, desc) = self.rhs_type_desc(rhs_site);
            let message = format!(
                "`{rhs_opt}`: {desc}, but `order: semver` compares semantic version strings"
            );
            self.fail(path, message);
            return None;
        };
        let b = match alint_core::semver_eval::parse_version(b_raw) {
            Ok(v) => v,
            Err(err) => {
                let rhs_hide =
                    hide || rhs_site.map_or(self.rule.hide_literals, |(_, p)| self.sensitive(p));
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
                let detail = if rhs_hide {
                    "not a valid semantic version".to_string()
                } else {
                    format!("not a valid semantic version ({err})")
                };
                let message = format!(
                    "`{rhs_opt}`: {field_desc} is {}{where_desc}: {detail}",
                    rhs.show(rhs_hide),
                );
                self.fail(path, message);
                return None;
            }
        };
        Some(op.matches(a.cmp(&b)))
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

/// Canonical exact decimal representation of a finite numeric lexeme.
/// Zero is uniquely represented as `negative: false, digits: "", exp10: 0`.
/// Non-zero numbers store significant base-10 digits (no leading/trailing `'0'`)
/// and `exp10` such that the value equals `(-1)^negative * 0.<digits> * 10^exp10`
/// (i.e. `exp10` is the 1-based scientific order of magnitude: `1` -> `"1", 1`,
/// `10` -> `"1", 2`, `0.5` -> `"5", 0`, `0.05` -> `"5", -1`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExactDecimal {
    negative: bool,
    digits: String,
    exp10: i128,
}

impl ExactDecimal {
    fn from_decimal_parts(negative: bool, sig: &str, exp10: i128) -> Self {
        let trimmed_lead = sig.trim_start_matches('0');
        let leading_zeros = i128::try_from(sig.len() - trimmed_lead.len()).unwrap_or(0);
        let digits = trimmed_lead.trim_end_matches('0');
        if digits.is_empty() {
            Self {
                negative: false,
                digits: String::new(),
                exp10: 0,
            }
        } else {
            Self {
                negative,
                digits: digits.to_owned(),
                exp10: exp10.saturating_sub(leading_zeros),
            }
        }
    }
}

impl Ord for ExactDecimal {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_zero = self.digits.is_empty();
        let other_zero = other.digits.is_empty();
        match (self_zero, other_zero) {
            (true, true) => return Ordering::Equal,
            (true, false) => {
                return if other.negative {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
            }
            (false, true) => {
                return if self.negative {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            (false, false) => {}
        }
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self
                .exp10
                .cmp(&other.exp10)
                .then_with(|| self.digits.cmp(&other.digits)),
            (true, true) => other
                .exp10
                .cmp(&self.exp10)
                .then_with(|| other.digits.cmp(&self.digits)),
        }
    }
}

impl PartialOrd for ExactDecimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Parse a JSON/YAML/TOML numeric lexeme into an exact finite [`ExactDecimal`],
/// supporting TOML underscore separators (`1_000`, `1_000.5`) and TOML/YAML
/// radix integers (`0x10`, `0o755`, `0b1010`), preserving full precision for
/// integers `> 2^53` and finite exponents `>= 1e309` while rejecting non-finite
/// values (`inf`, `NaN`).
fn parse_exact_decimal(scalar: &Scalar) -> Option<ExactDecimal> {
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
        if digits.is_empty() || digits.starts_with('+') || digits.starts_with('-') {
            return None;
        }
        let unsigned = u128::from_str_radix(digits, radix).ok()?;
        let dec = unsigned.to_string();
        let exp10 = i128::try_from(dec.len()).ok()?;
        return Some(ExactDecimal::from_decimal_parts(negative, &dec, exp10));
    }
    let (mantissa, exp_part) = match rest.find(['e', 'E']) {
        Some(idx) => (&rest[..idx], Some(&rest[idx + 1..])),
        None => (rest, None),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(idx) => (&mantissa[..idx], &mantissa[idx + 1..]),
        None => (mantissa, ""),
    };
    if (int_part.is_empty() && frac_part.is_empty())
        || !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let exp_offset: i128 = match exp_part {
        Some(exp) => {
            let (exp_neg, exp_digits) = match exp.as_bytes().first() {
                Some(b'-') => (true, &exp[1..]),
                Some(b'+') => (false, &exp[1..]),
                _ => (false, exp),
            };
            if exp_digits.is_empty() || !exp_digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let mag: i128 = exp_digits.parse().ok()?;
            if exp_neg { mag.checked_neg()? } else { mag }
        }
        None => 0,
    };
    let mut sig = String::with_capacity(int_part.len() + frac_part.len());
    sig.push_str(int_part);
    sig.push_str(frac_part);
    let exp10 = i128::try_from(int_part.len())
        .ok()?
        .checked_add(exp_offset)?;
    Some(ExactDecimal::from_decimal_parts(negative, &sig, exp10))
}

fn show_operand(scalar: &Scalar, sensitive: bool, typed: bool) -> String {
    if typed {
        scalar.show_typed(sensitive)
    } else {
        scalar.show(sensitive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(s: &str) -> ExactDecimal {
        parse_exact_decimal(&Scalar::Number(s.into())).expect("valid finite number")
    }

    #[test]
    fn exact_decimal_preserves_precision_above_f64_and_supports_radixes_and_large_exponents() {
        // Integers > 2^53 (9007199254740992) must not collapse under f64 rounding.
        assert!(num("9007199254740992") < num("9007199254740993"));
        assert_eq!(num("0x20000000000000"), num("9007199254740992"));
        assert_eq!(num("0o755"), num("493"));
        assert_eq!(num("0b1010"), num("10"));
        assert_eq!(num("1_000.5"), num("1000.50"));

        // Huge finite decimals beyond f64::MAX (~1.8e308) compare accurately.
        assert!(num("1e309") < num("2e309"));
        assert!(num("-2e309") < num("-1e309"));
        assert_eq!(num("-0.0"), num("0"));
        assert_eq!(num("+0e100"), num("0.000"));
        assert!(num("-0.001") < num("0"));
        assert!(num("0") < num("0.001"));

        // Non-finite and non-numeric forms are rejected.
        for bad in ["inf", "+inf", "-inf", "NaN", "nan", "0x", "0x-1", "1e", ""] {
            assert!(
                parse_exact_decimal(&Scalar::Number(bad.into())).is_none(),
                "{bad:?} must be rejected"
            );
        }
        assert!(parse_exact_decimal(&Scalar::String("42".into())).is_none());
    }
}
