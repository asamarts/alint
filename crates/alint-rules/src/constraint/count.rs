//! Unified `count` assertion (ADR-0050 D2): row count and distinct-value count
//! bounds (`min`, `max`, `exact`), whole-relation or per multi-field `group_by`,
//! with per-row findings for emptiness checks (`max: 0` / `exact: 0`).

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use alint_core::MAX_RELATED_LOCATIONS;
use alint_core::relation::{LimitKind, RelPath, Resource};

use super::{LISTED_VALUES, Place, Related, Run, rows_word};
use crate::constraint::CountAssertion;
use crate::constraint::values::{Key, Scalar};

struct CountGroup<'r> {
    value: Option<Vec<Scalar>>,
    rows: Vec<usize>,
    first: Option<&'r RelPath>,
    literal: bool,
}

impl<'r> Run<'r> {
    pub(in crate::constraint) fn count(&mut self, count: &CountAssertion, kept: Option<&[bool]>) {
        let group_at = if count.group_by.is_empty() {
            Vec::new()
        } else {
            let Some(positions) = self.positions(&count.group_by) else {
                return;
            };
            positions
        };
        let field_at = match &count.field {
            Some(field) => {
                let Some(pos) = self.position(field) else {
                    return;
                };
                Some(pos)
            }
            None => None,
        };
        let distinct_values = if let Some(pos) = field_at {
            let Some(values) = self.column(pos, "assert.count.field") else {
                return;
            };
            let all: Vec<usize> = (0..values.len()).collect();
            self.one_type(&all, pos, &values, "assert.count.field");
            if !self.failures.is_empty() {
                return;
            }
            Some(values)
        } else {
            None
        };
        let Some(groups) = self.count_groups(count, &group_at, kept) else {
            return;
        };
        let empty_mode = matches!(count.max, Some(0)) || matches!(count.exact, Some(0));
        if empty_mode {
            self.count_empty(&groups, field_at, distinct_values.as_deref());
            return;
        }
        for group in &groups {
            self.count_aggregate(
                count,
                &group_at,
                group,
                field_at,
                distinct_values.as_deref(),
            );
            if !self.failures.is_empty() {
                return;
            }
        }
    }

    fn count_groups(
        &mut self,
        count: &CountAssertion,
        group_at: &[usize],
        kept: Option<&[bool]>,
    ) -> Option<Vec<CountGroup<'r>>> {
        let rows = self.rows;
        let resources = rows
            .resources
            .iter()
            .enumerate()
            .filter(|(index, _)| kept.is_none_or(|kept| kept[*index]))
            .map(|(_, resource)| resource);
        if group_at.is_empty() {
            let paths: BTreeSet<&'r RelPath> = resources.map(Resource::path).collect();
            let first = paths.iter().next().copied();
            return Some(vec![CountGroup {
                value: None,
                rows: (0..rows.rows.len()).collect(),
                first,
                literal: false,
            }]);
        }
        let (grouped, group_cols) = self.groups_by_fields(group_at, "assert.count.group_by")?;
        for (index, tuple) in count.groups.iter().enumerate() {
            if !self.check_group_tuple_types(
                tuple,
                group_at,
                &group_cols,
                &format!("assert.count.groups[{index}]"),
            ) {
                return None;
            }
        }
        let mut out: Vec<CountGroup<'r>> = Vec::with_capacity(grouped.order.len());
        let mut index: HashMap<Vec<Scalar>, usize> = HashMap::with_capacity(grouped.order.len());
        for (value, members) in grouped.iter() {
            let first = members
                .iter()
                .map(|&row| rows.rows[row].at.resource.path())
                .min();
            index.insert(value.clone(), out.len());
            out.push(CountGroup {
                value: Some(value.clone()),
                rows: members.clone(),
                first,
                literal: false,
            });
        }
        if !count.resource_groups.is_empty() {
            for resource in resources {
                let Some(tuple) = count
                    .resource_groups
                    .iter()
                    .map(|rg| rg.group_of(resource).map(|s| Scalar::String(s.into())))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                add_count_resource(&mut out, &mut index, tuple, resource.path());
            }
        }
        if count.stub_root_groups {
            for stub in &rows.stub_rows {
                let Some(tuple) = group_at
                    .iter()
                    .map(|&pos| Scalar::of(&stub.cells[pos]).ok().filter(|v| !v.is_null()))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                add_count_resource(&mut out, &mut index, tuple, stub.at.resource.path());
            }
        }
        for tuple in &count.groups {
            let scalars: Vec<Scalar> = tuple
                .iter()
                .map(|v| Scalar::of_value(v).expect("builder validated scalar literals"))
                .collect();
            if !index.contains_key(&scalars) {
                index.insert(scalars.clone(), out.len());
                out.push(CountGroup {
                    value: Some(scalars),
                    rows: Vec::new(),
                    first: None,
                    literal: true,
                });
            }
        }
        Some(out)
    }

    fn count_empty(
        &mut self,
        groups: &[CountGroup<'r>],
        field_at: Option<usize>,
        distinct_values: Option<&[Scalar]>,
    ) {
        for group in groups {
            for &row in &group.rows {
                if let Some(values) = distinct_values
                    && values[row].is_null()
                {
                    continue;
                }
                if self.at_finding_cap() {
                    self.suppress_findings(1, false);
                    continue;
                }
                let anchor = field_at.unwrap_or(0);
                let at = self.spot(row, anchor);
                let message = match field_at {
                    Some(pos) => format!(
                        "expected no values of `{}`, but found {} here",
                        self.name(pos),
                        distinct_values.expect("distinct values")[row]
                            .show(self.redact_all || self.sensitive(pos))
                    ),
                    None => format!(
                        "expected no rows in {}, but found a row here",
                        self.input_subject()
                    ),
                };
                let mut key = Key::new("count_empty");
                key.row(self.own(row), self.hide_keys);
                self.report(at, message, key, Related::empty(), Some(row), None);
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn count_aggregate(
        &mut self,
        count: &CountAssertion,
        group_at: &[usize],
        group: &CountGroup<'r>,
        field_at: Option<usize>,
        distinct_values: Option<&[Scalar]>,
    ) {
        let (actual, sample_rows, sample_values) = if let (Some(pos), Some(values)) =
            (field_at, distinct_values)
        {
            let mut seen: HashMap<&Scalar, usize> = HashMap::new();
            let mut order: Vec<(usize, &Scalar)> = Vec::new();
            for &row in &group.rows {
                let value = &values[row];
                if value.is_null() {
                    continue;
                }
                if !seen.contains_key(value) {
                    seen.insert(value, row);
                    order.push((row, value));
                    if let Err(limit) = self.limits.check(LimitKind::DistinctValues, seen.len()) {
                        self.fail(
                            None,
                            format!(
                                "`assert.count.distinct`: `{}`: {limit}, so the assertion has no \
                                 verdict; narrow the relation",
                                self.name(pos)
                            ),
                        );
                        return;
                    }
                }
            }
            let sample_rows: Vec<usize> = order.iter().map(|(row, _)| *row).collect();
            let sample_vals: Vec<Scalar> = order
                .iter()
                .take(LISTED_VALUES)
                .map(|(_, v)| (*v).clone())
                .collect();
            (order.len() as u64, sample_rows, sample_vals)
        } else {
            (group.rows.len() as u64, group.rows.clone(), Vec::new())
        };
        let (bound_kind, expectation) = if let Some(exact) = count.exact {
            if actual == exact {
                return;
            }
            ("exact", format!("expected exactly {exact}"))
        } else if let Some(min) = count.min
            && actual < min
        {
            ("min", format!("expected at least {min}"))
        } else if let Some(max) = count.max
            && actual > max
        {
            ("max", format!("expected at most {max}"))
        } else {
            return;
        };
        if self.at_finding_cap() {
            self.suppress_findings(1, false);
            return;
        }
        let subject = self.count_subject(group_at, group);
        let unit = match field_at {
            Some(pos) => format!("distinct value(s) of `{}`", self.name(pos)),
            None => rows_word(usize::try_from(actual).unwrap_or(2)).to_string(),
        };
        let mut message = format!("{subject} has {actual} {unit}; {expectation}");
        if let Some(pos) = field_at
            && !sample_values.is_empty()
        {
            let hide = self.redact_all || self.sensitive(pos);
            let listed: Vec<String> = sample_values.iter().map(|v| v.show(hide)).collect();
            let more = usize::try_from(actual)
                .unwrap_or(usize::MAX)
                .saturating_sub(listed.len());
            message.push_str(" (");
            message.push_str(&listed.join(", "));
            if more > 0 {
                let _ = write!(message, ", and {more} more");
            }
            message.push(')');
        }
        let anchor_col = field_at.unwrap_or(0);
        let (at, reported, related) = if let Some(&first_row) = sample_rows.first() {
            let rest = &sample_rows[1..];
            let spots: Vec<_> = rest
                .iter()
                .take(MAX_RELATED_LOCATIONS)
                .map(|&row| (self.spot(row, anchor_col), "another counted row"))
                .collect();
            let omitted = rest.len() - spots.len();
            (
                Place::At(self.spot(first_row, anchor_col)),
                Some(first_row),
                self.related_at(spots, omitted),
            )
        } else {
            let place = group
                .first
                .map_or(Place::Nowhere, |path| Place::Resource(path.as_path()));
            (place, None, Related::empty())
        };
        let mut key = Key::new("count");
        key.part(bound_kind);
        match &group.value {
            Some(values) => {
                for (&pos, val) in group_at.iter().zip(values) {
                    let hide = self.redact_all
                        || self.sensitive(pos)
                        || (group.literal && self.rule.hide_literals);
                    key.value(val, hide);
                }
            }
            None => {
                key.part("all");
            }
        }
        self.report(at, message, key, related, reported, None);
    }

    fn count_subject(&self, group_at: &[usize], group: &CountGroup<'_>) -> String {
        if let Some(values) = &group.value {
            return group_at
                .iter()
                .zip(values)
                .map(|(&pos, val)| {
                    let hide = self.redact_all
                        || self.sensitive(pos)
                        || (group.literal && self.rule.hide_literals);
                    format!("`{}` {}", self.name(pos), val.show(hide))
                })
                .collect::<Vec<_>>()
                .join(", ");
        }
        self.input_subject()
    }

    fn input_subject(&self) -> String {
        let named = self.rule.plan.relation(self.rule.inputs[0]);
        if named.is_inline() {
            "the inline input".to_string()
        } else {
            format!("relation `{}`", named.name)
        }
    }
}

fn add_count_resource<'r>(
    out: &mut Vec<CountGroup<'r>>,
    index: &mut HashMap<Vec<Scalar>, usize>,
    value: Vec<Scalar>,
    path: &'r RelPath,
) {
    if let Some(&at) = index.get(&value) {
        if out[at].first.is_none_or(|first| path < first) {
            out[at].first = Some(path);
        }
    } else {
        index.insert(value.clone(), out.len());
        out.push(CountGroup {
            value: Some(value),
            rows: Vec::new(),
            first: Some(path),
            literal: false,
        });
    }
}
