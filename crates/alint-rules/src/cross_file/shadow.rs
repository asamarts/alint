//! R4 differential shadow for `cross_file`'s `relation: equals`.
//!
//! The authoritative implementation stays in [`super::eval`]. This module runs
//! the same relation through the internal IR (`ExtractAdapter` + read/decode
//! caches), then compares the complete findings. It is compiled only under the
//! test-only `shadow-ir` feature and never changes the returned verdict.
//!
//! Scope: successful reads of structured/lines/whole-file extracts. Reads or
//! decodes that fail are skipped here because their user-facing diagnostics are
//! produced by the authoritative path; regex extracts are skipped because the
//! target-regex syntax filters are intentionally outside the generic adapter.
//! Those paths remain covered by the existing `cross_file` tests and R5 corpus
//! replay's zero-difference check.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use alint_core::relation::{
    DecodeCache, ExtractAdapter, Limits, ReadCache, RelPath, Resource, ResourceRead,
};
use alint_core::{Context, Extract, Violation};

use super::spec::{Target, apply_normalize};
use super::{CrossFileRule, Relation, Targets};

enum Skip {
    /// The authoritative path will produce a read/decode diagnostic; leave the
    /// comparison to the existing tests rather than inventing a second one.
    Unavailable,
}

/// Run the IR implementation and assert that it produced exactly the same
/// findings as the authoritative one. A difference panics in tests/corpus.
#[allow(clippy::too_many_lines)]
pub(super) fn assert_equals_matches(
    rule: &CrossFileRule,
    ctx: &Context<'_>,
    existing: &[Violation],
) {
    if rule.relation != Relation::Equals || rule.source_glob.is_some() {
        return;
    }
    let Some(source_extract) = rule.source_extract.as_ref() else {
        return;
    };
    if !supported_extract(source_extract) {
        return;
    }
    let Some(targets) = rule.targets.as_ref() else {
        return;
    };
    let supported = match targets {
        Targets::Glob { extract, .. } => extract.as_ref().is_none_or(supported_extract),
        Targets::List(list) => list
            .iter()
            .all(|target| target.extract.as_ref().is_none_or(supported_extract)),
    };
    if !supported {
        return;
    }

    let mut shadow = Vec::new();
    // Prefer the engine's run-scoped caches, so the shadow exercises the same
    // coalescing path as a real relation consumer. Direct rule tests have no
    // engine-owned caches; give them a local pair.
    let local_read = ReadCache::new();
    let local_decode = DecodeCache::new();
    let read_cache = ctx.index.read_cache().unwrap_or(&local_read);
    let decode_cache = ctx.index.decode_cache().unwrap_or(&local_decode);
    let source_values = match values(
        rule,
        ctx,
        Path::new(&rule.source_file),
        source_extract,
        "source",
        &mut shadow,
        read_cache,
        decode_cache,
    ) {
        Ok(values) => values,
        Err(Skip::Unavailable) => return,
    };
    let Some(source_values) = source_values else {
        return;
    };

    let source = match source_values.as_slice() {
        [one] => one.clone(),
        [] => {
            let mut finding = CrossFileRule::violation(
                Path::new(&rule.source_file),
                "canonical value not found (the source query matched no literal value)",
            );
            rule.redact_extracted(
                &mut finding,
                "source",
                Path::new(&rule.source_file),
                source_extract,
                "missing",
                &[],
            );
            shadow.push(finding);
            return compare(existing, &shadow);
        }
        _ => {
            let mut finding = CrossFileRule::violation(
                Path::new(&rule.source_file),
                "source must resolve to exactly one value (the query matched several); \
                 use a set relation (subset/superset/set_equals) for multi-value sources",
            );
            rule.redact_extracted(
                &mut finding,
                "source",
                Path::new(&rule.source_file),
                source_extract,
                "cardinality",
                &[],
            );
            shadow.push(finding);
            return compare(existing, &shadow);
        }
    };
    let source_norm = apply_normalize(&rule.normalize, &source);

    let target_specs = match targets {
        Targets::Glob {
            scope,
            extract,
            matches,
        } => {
            let mut specs = Vec::new();
            for entry in ctx.index.files() {
                if scope.matches(&entry.path, ctx.index) {
                    specs.push((
                        entry.path.to_path_buf(),
                        extract.clone().expect("equals target has an extract"),
                        matches.clone(),
                    ));
                }
            }
            if specs.is_empty() {
                if !rule.allow_missing {
                    shadow.push(CrossFileRule::violation(
                        Path::new(&rule.source_file),
                        "targets glob matched no files",
                    ));
                }
                return compare(existing, &shadow);
            }
            specs
        }
        Targets::List(list) => list
            .iter()
            .map(|target| {
                let Target {
                    file,
                    extract,
                    matches,
                } = target;
                (
                    PathBuf::from(file),
                    extract.clone().expect("equals target has an extract"),
                    matches.clone(),
                )
            })
            .collect(),
    };

    for (target, extract, matches) in target_specs {
        let target_values = match values(
            rule,
            ctx,
            &target,
            &extract,
            "target",
            &mut shadow,
            read_cache,
            decode_cache,
        ) {
            Ok(values) => values,
            Err(Skip::Unavailable) => return,
        };
        let Some(mut target_values) = target_values else {
            continue;
        };
        if let Some(regex) = matches.as_ref() {
            target_values.retain(|value| regex.is_match(value));
        }
        if target_values.is_empty() && rule.allow_missing {
            continue;
        }
        if target_values.is_empty() {
            let mut finding = CrossFileRule::violation(
                &target,
                "no literal value to compare (the target query matched nothing)",
            );
            rule.add_source_locations(ctx, &mut finding, "canonical source");
            rule.redact_extracted(
                &mut finding,
                "source",
                Path::new(&rule.source_file),
                source_extract,
                "target_missing",
                &[&source, &source_norm],
            );
            rule.redact_extracted(&mut finding, "target", &target, &extract, "missing", &[]);
            shadow.push(finding);
            continue;
        }
        let mut occurrences = HashMap::<&str, usize>::new();
        for value in &target_values {
            let occurrence = occurrences.entry(value.as_str()).or_default();
            let target_norm = apply_normalize(&rule.normalize, value);
            if target_norm != source_norm {
                let mut finding = rule
                    .mismatch(
                        &target,
                        source_extract,
                        &extract,
                        &source,
                        &source_norm,
                        value,
                        &target_norm,
                    )
                    .with_fix_key(format!("cross_file_equals\u{0}{occurrence}"));
                rule.add_source_locations(ctx, &mut finding, "canonical source");
                shadow.push(finding);
            }
            *occurrence += 1;
        }
    }

    compare(existing, &shadow);
}

fn supported_extract(extract: &Extract) -> bool {
    !matches!(extract, Extract::Regex(_))
}

#[allow(clippy::too_many_arguments)]
fn values(
    rule: &CrossFileRule,
    ctx: &Context<'_>,
    path: &Path,
    extract: &Extract,
    side: &str,
    out: &mut Vec<Violation>,
    read_cache: &ReadCache,
    decode_cache: &DecodeCache,
) -> Result<Option<Vec<String>>, Skip> {
    let Some(resource) = resource_for(path) else {
        return Err(Skip::Unavailable);
    };
    let read = read_cache.read(ctx.root, &resource, 0);
    let ResourceRead::Present(bytes) = read.as_ref() else {
        return Err(Skip::Unavailable);
    };
    let adapter = ExtractAdapter::new(extract.clone());
    let output = decode_cache
        .decode(&adapter, &resource, bytes, &Limits::default())
        .map_err(|_| Skip::Unavailable)?;
    let has_non_strings = !output.non_strings.is_empty();
    if let Some(mut violation) =
        crate::non_string_extract_violation(path, &format!("{side} extract"), &output.non_strings)
    {
        rule.redact_extracted(&mut violation, side, path, extract, "non_string", &[]);
        out.push(violation);
    }
    for note in &output.notes {
        let alint_core::relation::AdapterNote::NonLiteral { value } = note;
        let mut finding =
            CrossFileRule::note(path, &format!("skipped non-literal {side} value {value:?}"));
        rule.redact_extracted(&mut finding, side, path, extract, "skipped", &[value]);
        out.push(finding);
    }
    if has_non_strings && output.rows.is_empty() {
        return Ok(None);
    }
    let values = output
        .rows
        .iter()
        .filter_map(|row| row.value())
        .filter_map(|value| match value {
            alint_core::relation::Value::String(text) => Some(text.to_string()),
            _ => None,
        })
        .collect();
    Ok(Some(values))
}

fn resource_for(path: &Path) -> Option<Resource> {
    let text = path.to_str()?.replace('\\', "/");
    RelPath::parse(&text).map(Resource::literal)
}

fn compare(existing: &[Violation], shadow: &[Violation]) {
    assert_eq!(
        existing.len(),
        shadow.len(),
        "shadow-ir finding count differs\nauthoritative: {existing:#?}\nshadow: {shadow:#?}"
    );
    for (index, (authoritative, shadowed)) in existing.iter().zip(shadow).enumerate() {
        assert_eq!(
            format!("{authoritative:?}"),
            format!("{shadowed:?}"),
            "shadow-ir finding {index} differs"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::spec::{Normalize, NormalizeSpec, Target};
    use super::*;
    use alint_core::{FileIndex, Format, Level, Rule, Scope};
    use proptest::prelude::*;
    use std::fs;

    fn normalize_strategy() -> impl Strategy<Value = Vec<Normalize>> {
        prop::collection::vec(
            prop_oneof![
                Just(Normalize::Trim),
                Just(Normalize::Lower),
                Just(Normalize::SemverMajor),
                Just(Normalize::SemverMinor),
            ],
            0..4,
        )
    }

    fn rule(source: &str, targets: &[&str], normalize: Vec<Normalize>) -> CrossFileRule {
        let extract = Extract::structured(Format::Json, "$.v").unwrap();
        CrossFileRule {
            id: "shadow-property".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source.into(),
            source_glob: None,
            source_extract: Some(extract.clone()),
            targets: Some(Targets::List(
                targets
                    .iter()
                    .map(|file| Target {
                        file: (*file).into(),
                        extract: Some(extract.clone()),
                        matches: None,
                    })
                    .collect(),
            )),
            relation: Relation::Equals,
            normalize,
            allow_missing: false,
            skip_header_lines: 0,
            register_as: None,
            fixer: None,
        }
    }

    proptest! {
        #[test]
        fn ir_equals_matches_authoritative_on_generated_values(
            source_value in any::<String>(),
            target_values in prop::collection::vec(any::<String>(), 1..5),
            normalize in normalize_strategy(),
        ) {
            let dir = tempfile::tempdir().unwrap();
            for (index, value) in target_values.iter().enumerate() {
                fs::write(
                    dir.path().join(format!("target-{index}.json")),
                    serde_json::to_vec(&serde_json::json!({ "v": value })).unwrap(),
                ).unwrap();
            }
            fs::write(
                dir.path().join("source.json"),
                serde_json::to_vec(&serde_json::json!({ "v": source_value })).unwrap(),
            ).unwrap();
            let target_files: Vec<String> = (0..target_values.len())
                .map(|index| format!("target-{index}.json"))
                .collect();
            let target_refs: Vec<&str> = target_files.iter().map(String::as_str).collect();
            let rule = rule("source.json", &target_refs, normalize);
            let index = FileIndex::from_entries(Vec::new());
            let ctx = Context {
                root: dir.path(),
                index: &index,
                registry: None,
                facts: None,
                vars: None,
                git_tracked: None,
                git_blame: None,
                analysis_time: None,
                process_environment: None,
                trace: None,
            };
            rule.evaluate(&ctx).unwrap();
        }
    }

    #[test]
    fn regex_extracts_are_skipped_by_the_shadow() {
        let extract = Extract::Regex(r"(?m)^v=(.*)$".into());
        assert!(!supported_extract(&extract));
    }

    #[test]
    fn target_glob_with_no_matches_is_compared() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("source.json"), br#"{"v":"1.2.3"}"#).unwrap();
        let extract = Extract::structured(Format::Json, "$.v").unwrap();
        let rule = CrossFileRule {
            id: "shadow-no-targets".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "source.json".into(),
            source_glob: None,
            source_extract: Some(extract.clone()),
            targets: Some(Targets::Glob {
                scope: Scope::from_patterns(&["missing/*.json".into()]).unwrap(),
                extract: Some(extract),
                matches: None,
            }),
            relation: Relation::Equals,
            normalize: NormalizeSpec::default().into_list(),
            allow_missing: false,
            skip_header_lines: 0,
            register_as: None,
            fixer: None,
        };
        let index = FileIndex::from_entries(Vec::new());
        let ctx = Context {
            root: dir.path(),
            index: &index,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
            analysis_time: None,
            process_environment: None,
            trace: None,
        };
        let findings = rule.evaluate(&ctx).unwrap();
        assert_eq!(findings.len(), 1);
    }
}
