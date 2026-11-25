//! `reference` assertion (ADR-0050 D1): single-key and composite-key foreign
//! keys checking that every row's key tuple exists in a target relation.

use std::collections::{BTreeMap, HashSet};

use alint_core::relation::LimitKind;
use alint_core::relation::eval::{Cell, RelationRows};

use super::{Related, Run, type_of};
use crate::constraint::ReferenceAssertion;
use crate::constraint::values::{Key, Kind, Scalar};

type TargetSet = (HashSet<Vec<Scalar>>, Vec<Option<(Kind, usize)>>);

impl<'r> Run<'r> {
    #[allow(clippy::too_many_lines)]
    pub(in crate::constraint) fn reference(
        &mut self,
        reference: &ReferenceAssertion,
        target_rows: &'r RelationRows,
    ) {
        let Some(src_at) = self.positions(&reference.fields) else {
            return;
        };
        let Some(dst_at) = self.target_positions(target_rows, reference) else {
            return;
        };
        let Some((target_set, dst_kinds)) =
            self.build_reference_target(reference, target_rows, &dst_at)
        else {
            return;
        };
        let Some(src_cols) =
            self.reference_source_columns(&src_at, reference, target_rows, &dst_at, &dst_kinds)
        else {
            return;
        };
        let hide_cols: Vec<bool> = src_at
            .iter()
            .zip(&dst_at)
            .map(|(&s, &d)| self.redact_all || self.sensitive(s) || target_rows.fields[d].sensitive)
            .collect();
        let target_label = if reference.target_fields.len() == 1 {
            format!(
                "`{}` of relation `{}`",
                reference.target_fields[0], reference.written
            )
        } else {
            format!(
                "({}) of relation `{}`",
                reference
                    .target_fields
                    .iter()
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                reference.written
            )
        };
        for row in 0..self.rows.rows.len() {
            let tuple: Vec<Scalar> = src_cols.iter().map(|col| col[row].clone()).collect();
            // Explicit `Null` is NEVER skipped by `skip_missing: true`
            // (ADR-0050 D1); check `Null` first before `Missing`.
            let unresolved_idx = tuple
                .iter()
                .position(|v| matches!(v, Scalar::Null))
                .or_else(|| {
                    if reference.skip_missing {
                        None
                    } else {
                        tuple.iter().position(Scalar::is_missing)
                    }
                });
            if let Some(missing_idx) = unresolved_idx {
                if self.at_finding_cap() {
                    self.suppress_findings(1, false);
                    continue;
                }
                let missing_pos = src_at[missing_idx];
                let field_name = self.name(missing_pos).to_string();
                let missing_note = if tuple[missing_idx].is_missing() {
                    self.read_nothing(row, missing_pos)
                } else {
                    String::new()
                };
                let message = format!(
                    "`{field_name}` is {} here{missing_note}, so it cannot reference \
                     {target_label}",
                    tuple[missing_idx].show(hide_cols[missing_idx])
                );
                let mut key = Key::new("reference");
                for (val, &hide) in tuple.iter().zip(&hide_cols) {
                    key.value(val, hide);
                }
                let at = if self.cell(row, missing_pos).span.is_some() {
                    self.spot(row, missing_pos)
                } else {
                    self.row_spot(row)
                };
                let placeholder =
                    format_reference_placeholder(&tuple, &hide_cols, self.rule.hide_literals);
                self.report_with_value(
                    at,
                    message,
                    key,
                    Related::empty(),
                    Some(row),
                    Some(placeholder.as_str()),
                );
                continue;
            }
            if tuple.iter().any(Scalar::is_missing) {
                // `skip_missing: true` with no explicit `Null`.
                continue;
            }
            if !target_set.contains(&tuple) {
                if self.at_finding_cap() {
                    self.suppress_findings(1, false);
                    continue;
                }
                let shown = format_reference_shown(&src_at, &tuple, &hide_cols, |p| self.name(p));
                let message = format!("{shown} has no matching row in {target_label}");
                let mut key = Key::new("reference");
                for (val, &hide) in tuple.iter().zip(&hide_cols) {
                    key.value(val, hide);
                }
                let at = src_at
                    .iter()
                    .copied()
                    .find(|&pos| self.cell(row, pos).span.is_some())
                    .map_or_else(|| self.row_spot(row), |pos| self.spot(row, pos));
                let placeholder =
                    format_reference_placeholder(&tuple, &hide_cols, self.rule.hide_literals);
                self.report_with_value(
                    at,
                    message,
                    key,
                    Related::empty(),
                    Some(row),
                    Some(placeholder.as_str()),
                );
            }
        }
    }

    fn target_positions(
        &mut self,
        target_rows: &RelationRows,
        reference: &ReferenceAssertion,
    ) -> Option<Vec<usize>> {
        let mut out = Vec::with_capacity(reference.target_fields.len());
        for field in &reference.target_fields {
            let Some(pos) = target_rows.field(field) else {
                self.fail(
                    None,
                    format!(
                        "`assert.reference.to`: the rows of `{}` have no field `{field}`, which \
                         the plan declares; this is a bug in alint, so please report it",
                        reference.written
                    ),
                );
                return None;
            };
            out.push(pos);
        }
        Some(out)
    }

    fn build_reference_target(
        &mut self,
        reference: &ReferenceAssertion,
        target_rows: &RelationRows,
        dst_at: &[usize],
    ) -> Option<TargetSet> {
        let k = dst_at.len();
        let mut col_kinds: Vec<BTreeMap<Kind, usize>> = vec![BTreeMap::new(); k];
        let mut set: HashSet<Vec<Scalar>> = HashSet::new();
        let limits = self.limits;
        let tuple_overhead = std::mem::size_of::<Vec<Scalar>>()
            .saturating_add(k.saturating_mul(std::mem::size_of::<Scalar>()))
            .saturating_add(16);
        let mut held_bytes = 0usize;
        for (row_idx, row) in target_rows.rows.iter().enumerate() {
            let mut tuple = Vec::with_capacity(k);
            let mut skip = false;
            for (j, &col) in dst_at.iter().enumerate() {
                let cell = &row.cells[col];
                let scalar = match Scalar::of(cell) {
                    Ok(v) => v,
                    Err(held) => {
                        let message = format!(
                            "`assert.reference.to`: `{}` of relation `{}` holds {held} at {}; a \
                             constraint compares strings, numbers, booleans and null",
                            reference.target_fields[j],
                            reference.written,
                            self.located(cell)
                        );
                        self.fail(Some(cell.at.resource.path().as_path()), message);
                        return None;
                    }
                };
                if let Some(kind) = scalar.kind() {
                    col_kinds[j].entry(kind).or_insert(row_idx);
                } else {
                    skip = true;
                }
                tuple.push(scalar);
            }
            if !skip && set.insert(tuple) {
                held_bytes = held_bytes.saturating_add(tuple_overhead);
                if let Err(limit) = limits
                    .check(LimitKind::DistinctValues, set.len())
                    .and_then(|()| self.may_hold(held_bytes))
                {
                    self.fail(
                        None,
                        format!(
                            "`assert.reference.to`: reads `{}`: {limit}, so the assertion has no \
                             verdict rather than one over part of the target set; narrow `{}`",
                            reference.written, reference.written
                        ),
                    );
                    return None;
                }
            }
        }
        let mut resolved_kinds = Vec::with_capacity(k);
        for (j, kinds) in col_kinds.into_iter().enumerate() {
            if kinds.len() > 1 {
                let mut rows: Vec<usize> = kinds.values().copied().collect();
                rows.sort_unstable();
                let (one, two) = (
                    &target_rows.rows[rows[0]].cells[dst_at[j]],
                    &target_rows.rows[rows[1]].cells[dst_at[j]],
                );
                let message = format!(
                    "`assert.reference.to`: `{}` of relation `{}` holds {} at {} and {} at {}; \
                     the comparison would cross types, so the assertion has no verdict",
                    reference.target_fields[j],
                    reference.written,
                    type_of(one),
                    self.located(one),
                    type_of(two),
                    self.located(two)
                );
                self.fail(Some(one.at.resource.path().as_path()), message);
                return None;
            }
            resolved_kinds.push(kinds.into_iter().next());
        }
        Some((set, resolved_kinds))
    }

    fn reference_source_columns(
        &mut self,
        src_at: &[usize],
        reference: &ReferenceAssertion,
        target_rows: &RelationRows,
        dst_at: &[usize],
        dst_kinds: &[Option<(Kind, usize)>],
    ) -> Option<Vec<Vec<Scalar>>> {
        let mut cols = Vec::with_capacity(src_at.len());
        let option = if src_at.len() == 1 {
            "assert.reference.field"
        } else {
            "assert.reference.fields"
        };
        for (j, &pos) in src_at.iter().enumerate() {
            let values = self.column(pos, option)?;
            let all: Vec<usize> = (0..values.len()).collect();
            self.one_type(&all, pos, &values, option);
            if !self.failures.is_empty() {
                return None;
            }
            if let Some((dst_kind, dst_row)) = dst_kinds[j]
                && let Some(src_row) = values
                    .iter()
                    .position(|v| v.kind().is_some_and(|k| k != dst_kind))
            {
                let src_cell = self.cell(src_row, pos);
                let dst_cell: &Cell = &target_rows.rows[dst_row].cells[dst_at[j]];
                let message = format!(
                    "`{option}`: `{}` holds {} at {} and `{}` of relation `{}` holds {} at {}; \
                     the comparison would cross types, so the assertion has no verdict",
                    self.name(pos),
                    type_of(src_cell),
                    self.located(src_cell),
                    reference.target_fields[j],
                    reference.written,
                    type_of(dst_cell),
                    self.located(dst_cell)
                );
                self.fail(Some(src_cell.at.resource.path().as_path()), message);
                return None;
            }
            cols.push(values);
        }
        Some(cols)
    }
}

fn format_reference_shown<'a>(
    positions: &[usize],
    values: &[Scalar],
    hide_cols: &[bool],
    name_of: impl Fn(usize) -> &'a str,
) -> String {
    positions
        .iter()
        .zip(values)
        .zip(hide_cols)
        .map(|((&pos, val), &hide)| format!("`{}` {}", name_of(pos), val.show(hide)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_reference_placeholder(
    values: &[Scalar],
    hide_cols: &[bool],
    hide_literals: bool,
) -> String {
    values
        .iter()
        .zip(hide_cols)
        .map(|(val, &hide)| {
            if hide || hide_literals {
                "<redacted>".to_string()
            } else if val.is_missing() {
                "<missing>".to_string()
            } else if matches!(val, Scalar::Null) {
                "null".to_string()
            } else {
                val.text_shown()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
