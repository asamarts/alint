//! `relation: equals` (and its `cross_file_value_equals` alias): the source
//! resolves to exactly one value, and every target value must equal it after
//! normalize. Only `equals` runs the code in this module, and it calls nothing
//! in [`super::eval`] (ADR-0026).

use std::collections::HashMap;
use std::path::Path;

use alint_core::{Context, Extract, Violation};

use super::CrossFileRule;
use super::spec::apply_normalize;

impl CrossFileRule {
    /// `relation: equals` - the released `cross_file_value_equals`
    /// behaviour: the source must resolve to exactly one value, and
    /// every target value must equal it after normalize.
    pub(super) fn check_equals(
        &self,
        ctx: &Context<'_>,
        source_values: &[String],
        out: &mut Vec<Violation>,
    ) {
        let source_extract = self
            .source_extract
            .as_ref()
            .expect("equals has a source extract");
        let source = match source_values {
            [one] => one.clone(),
            values => {
                let (message, reason) = if values.is_empty() {
                    (
                        "canonical value not found (the source query matched no literal value)",
                        "missing",
                    )
                } else {
                    (
                        "source must resolve to exactly one value (the query matched several); \
                         use a set relation (subset/superset/set_equals) for multi-value sources",
                        "cardinality",
                    )
                };
                let source_file = Path::new(&self.source_file);
                let mut finding = Self::violation(source_file, message);
                self.redact_extracted(
                    &mut finding,
                    "source",
                    source_file,
                    source_extract,
                    reason,
                    &[],
                );
                out.push(finding);
                return;
            }
        };
        let source_norm = apply_normalize(&self.normalize, &source);
        // `each_target` passes an empty target only when `allow_missing_target`
        // is off.
        self.each_target(ctx, out, &mut |target, target_extract, values, out| {
            if values.is_empty() {
                let mut finding = Self::violation(
                    target,
                    "no literal value to compare (the target query matched nothing)",
                );
                self.add_source_locations(ctx, &mut finding, "canonical source");
                self.redact_extracted(
                    &mut finding,
                    "source",
                    Path::new(&self.source_file),
                    source_extract,
                    "target_missing",
                    &[&source, &source_norm],
                );
                self.redact_extracted(
                    &mut finding,
                    "target",
                    target,
                    target_extract,
                    "missing",
                    &[],
                );
                out.push(finding);
                return;
            }
            let mut occurrences = HashMap::<&str, usize>::new();
            for value in values {
                let occurrence = occurrences.entry(value.as_str()).or_default();
                let target_norm = apply_normalize(&self.normalize, value);
                if target_norm != source_norm {
                    let mut finding = self
                        .mismatch(
                            target,
                            source_extract,
                            target_extract,
                            &source,
                            &source_norm,
                            value,
                            &target_norm,
                        )
                        .with_fix_key(format!("cross_file_equals\u{0}{occurrence}"));
                    self.add_source_locations(ctx, &mut finding, "canonical source");
                    out.push(finding);
                }
                *occurrence += 1;
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn mismatch(
        &self,
        target: &Path,
        source_extract: &Extract,
        target_extract: &Extract,
        source: &str,
        source_normalized: &str,
        target_value: &str,
        target_normalized: &str,
    ) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} value {target_value:?} != {} value {source:?}",
                crate::slash(target),
                self.source_file,
            )
        });
        let mut finding = Violation::new(msg)
            .with_path(target.to_path_buf())
            .with_baseline_key(format!(
                "equals\u{0}{}\u{0}{target_value}",
                crate::slash(target)
            ));
        self.redact_extracted(
            &mut finding,
            "source",
            Path::new(&self.source_file),
            source_extract,
            "equals",
            &[source, source_normalized],
        );
        self.redact_extracted(
            &mut finding,
            "target",
            target,
            target_extract,
            "equals",
            &[target_value, target_normalized],
        );
        finding
    }
}

#[cfg(test)]
mod tests {
    /// ADR-0025 decision 3, gate 2, counted as ADR-0026 defines: the code only
    /// `equals` runs (this module outside its tests) is at most 134 lines that
    /// are neither blank nor only a comment, from each `fn` line to its closing
    /// brace, and it calls nothing in `cross_file::eval`.
    #[test]
    fn equals_stays_within_the_simplicity_gate() {
        let source = include_str!("equals.rs");
        let code = &source[..source.find("#[cfg(test)]").expect("a test module")];
        let mut counted = 0;
        let mut closing: Option<String> = None;
        for line in code.lines() {
            let trimmed = line.trim_start();
            if closing.is_none() && trimmed.trim_start_matches("pub(super) ").starts_with("fn ") {
                closing = Some(format!("{}}}", &line[..line.len() - trimmed.len()]));
            }
            let Some(end) = &closing else { continue };
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                counted += 1;
            }
            if line == end {
                closing = None;
            }
        }
        assert!(
            counted <= 134,
            "equals-only code is {counted} lines; the bound is 134"
        );

        let code_lines = || {
            code.lines()
                .filter(|line| !line.trim_start().starts_with("//"))
        };
        assert!(
            !code_lines().any(|line| line.contains("eval")),
            "equals names cross_file::eval"
        );
        let eval = include_str!("eval.rs");
        let eval = &eval[..eval.find("#[cfg(test)]").expect("a test module")];
        for name in eval.lines().filter_map(|line| {
            let rest = line.trim_start().strip_prefix("pub(super) fn ")?;
            rest.split(['(', '<']).next()
        }) {
            assert!(
                !code_lines().any(|line| line.contains(&format!(".{name}("))),
                "equals calls `{name}` in cross_file::eval"
            );
        }
    }
}
