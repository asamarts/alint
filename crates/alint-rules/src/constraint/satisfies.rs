//! `satisfies` assertion (ADR-0050 D5, Gap C4): verifies that each row's
//! version satisfies a semantic version range under `node`, `cargo`, or
//! `pep440-basic` dialect semantics.

use alint_core::semver_eval::{parse_range, parse_version};

use super::{Related, Run, Spot, type_of};
use crate::constraint::values::{Key, Scalar};
use crate::constraint::{SatisfiesAssertion, SatisfiesRange, SatisfiesVersion};

impl Run<'_> {
    #[allow(clippy::too_many_lines)]
    pub(in crate::constraint) fn satisfies(&mut self, satisfies: &SatisfiesAssertion) {
        let version_pos = match &satisfies.version {
            SatisfiesVersion::Field(name) => {
                let Some(pos) = self.position(name) else {
                    return;
                };
                Some((name.as_str(), pos))
            }
            SatisfiesVersion::Literal { .. } => None,
        };
        let range_pos = match &satisfies.range {
            SatisfiesRange::Field(name) => {
                let Some(pos) = self.position(name) else {
                    return;
                };
                Some((name.as_str(), pos))
            }
            SatisfiesRange::Literal { .. } => None,
        };
        let has_literal = version_pos.is_none() || range_pos.is_none();
        let pair_sensitive = self.redact_all
            || version_pos.is_some_and(|(_, p)| self.sensitive(p))
            || range_pos.is_some_and(|(_, p)| self.sensitive(p))
            || (has_literal && self.rule.hide_literals);

        for row in 0..self.rows.rows.len() {
            let (ver_scalar, ver_parsed) = match &satisfies.version {
                SatisfiesVersion::Field(name) => {
                    let pos = version_pos.expect("checked above").1;
                    let Some(scalar) = self.scalar(row, pos, "assert.satisfies.version") else {
                        return;
                    };
                    if scalar.is_null() {
                        let cell = self.cell(row, pos);
                        let message = format!(
                            "`assert.satisfies.version`: `{name}` is {} at {}; `satisfies` \
                             requires non-null string versions (filter nullable/optional rows \
                             with `where:`)",
                            if scalar.is_missing() {
                                "missing"
                            } else {
                                "null"
                            },
                            self.located(cell)
                        );
                        self.fail(Some(cell.at.resource.path().as_path()), message);
                        return;
                    }
                    let Scalar::String(raw) = &scalar else {
                        let cell = self.cell(row, pos);
                        let message = format!(
                            "`assert.satisfies.version`: `{name}` holds {} at {}, but \
                             `satisfies` requires a string version",
                            type_of(cell),
                            self.located(cell)
                        );
                        self.fail(Some(cell.at.resource.path().as_path()), message);
                        return;
                    };
                    let parsed = match parse_version(raw) {
                        Ok(v) => v,
                        Err(err) => {
                            let cell = self.cell(row, pos);
                            let detail = if pair_sensitive {
                                "not a valid semantic version".to_string()
                            } else {
                                format!("not a valid semantic version ({err})")
                            };
                            let message = format!(
                                "`assert.satisfies.version`: `{name}` is {} at {}: {detail}",
                                scalar.show(pair_sensitive),
                                self.located(cell)
                            );
                            self.fail(Some(cell.at.resource.path().as_path()), message);
                            return;
                        }
                    };
                    (scalar, parsed)
                }
                SatisfiesVersion::Literal { raw, parsed } => {
                    (Scalar::String(raw.as_str().into()), parsed.clone())
                }
            };

            let (range_scalar, range_parsed) = match &satisfies.range {
                SatisfiesRange::Field(name) => {
                    let pos = range_pos.expect("checked above").1;
                    let Some(scalar) = self.scalar(row, pos, "assert.satisfies.range") else {
                        return;
                    };
                    if scalar.is_null() {
                        let cell = self.cell(row, pos);
                        let message = format!(
                            "`assert.satisfies.range`: `{name}` is {} at {}; `satisfies` requires \
                             non-null string ranges (filter nullable/optional rows with `where:`)",
                            if scalar.is_missing() {
                                "missing"
                            } else {
                                "null"
                            },
                            self.located(cell)
                        );
                        self.fail(Some(cell.at.resource.path().as_path()), message);
                        return;
                    }
                    let Scalar::String(raw) = &scalar else {
                        let cell = self.cell(row, pos);
                        let message = format!(
                            "`assert.satisfies.range`: `{name}` holds {} at {}, but `satisfies` \
                             requires a string range",
                            type_of(cell),
                            self.located(cell)
                        );
                        self.fail(Some(cell.at.resource.path().as_path()), message);
                        return;
                    };
                    let parsed = match parse_range(raw, satisfies.dialect) {
                        Ok(r) => r,
                        Err(err) => {
                            let cell = self.cell(row, pos);
                            let detail = if pair_sensitive {
                                format!(
                                    "not a valid `{}` version range",
                                    satisfies.dialect.as_str()
                                )
                            } else {
                                format!(
                                    "not a valid `{}` version range ({err})",
                                    satisfies.dialect.as_str()
                                )
                            };
                            let message = format!(
                                "`assert.satisfies.range`: `{name}` is {} at {}: {detail}",
                                scalar.show(pair_sensitive),
                                self.located(cell)
                            );
                            self.fail(Some(cell.at.resource.path().as_path()), message);
                            return;
                        }
                    };
                    (scalar, parsed)
                }
                SatisfiesRange::Literal { raw, parsed } => {
                    (Scalar::String(raw.as_str().into()), parsed.clone())
                }
            };

            if range_parsed.matches(&ver_parsed, satisfies.include_prerelease) {
                continue;
            }
            if self.at_finding_cap() {
                self.suppress_findings(1, false);
                continue;
            }

            let ver_shown = ver_scalar.show(pair_sensitive);
            let range_shown = range_scalar.show(pair_sensitive);
            let message = match (version_pos, range_pos) {
                (Some((v_name, _)), Some((r_name, _))) => format!(
                    "`{v_name}` is {ver_shown} and `{r_name}` is {range_shown} in this row; \
                     version does not satisfy range (dialect `{}`)",
                    satisfies.dialect.as_str()
                ),
                (Some((v_name, _)), None) => format!(
                    "`{v_name}` is {ver_shown} here; expected a version satisfying {range_shown} \
                     (dialect `{}`)",
                    satisfies.dialect.as_str()
                ),
                (None, Some((r_name, _))) => format!(
                    "`{r_name}` is {range_shown} here; does not accept version {ver_shown} \
                     (dialect `{}`)",
                    satisfies.dialect.as_str()
                ),
                (None, None) => {
                    unreachable!("builder refuses both version_literal and range_literal")
                }
            };

            let mut key = Key::new("satisfies");
            key.row(self.own(row), self.hide_keys);

            let (at, related) = match (version_pos, range_pos) {
                (Some((_, v_pos)), Some((r_name, r_pos))) => {
                    let at = self.spot_point(row, v_pos, pair_sensitive);
                    let r_cell = self.cell(row, r_pos);
                    let label = format!("`{r_name}` in this row");
                    let r_spot = Spot::Cell(r_cell, pair_sensitive || self.point(r_pos));
                    let related = self.related_at(vec![(r_spot, label.as_str())], 0);
                    (at, related)
                }
                (Some((_, v_pos)), None) => (
                    self.spot_point(row, v_pos, pair_sensitive),
                    Related::empty(),
                ),
                (None, Some((_, r_pos))) => (
                    self.spot_point(row, r_pos, pair_sensitive),
                    Related::empty(),
                ),
                (None, None) => unreachable!(),
            };

            self.report(at, message, key, related, Some(row), None);
        }
    }
}
