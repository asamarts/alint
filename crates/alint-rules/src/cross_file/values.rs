//! Reading the values a `cross_file` relation compares: the source's extracted
//! values (one file, or a glob union for the set relations), each target's,
//! and the confined reads behind them. Every value relation and `resolves`
//! read through here.

use std::path::{Path, PathBuf};

use alint_core::{
    AnalysisFailureKind, Context, Extract, ExtractedValues, Violation, extract_values_bounded,
    is_non_literal, unreadable_file_violation,
};

use super::CrossFileRule;
use super::spec::Targets;

/// Per-target callback for `each_target`: receives the target path, extractor,
/// raw literal values, and the violation sink.
pub(super) type TargetFn<'a> = dyn FnMut(&Path, &Extract, &[String], &mut Vec<Violation>) + 'a;

impl CrossFileRule {
    /// Read + extract the source file's literal values (raw, not
    /// normalised - callers normalise as the relation needs).
    /// `None` (with a violation pushed) when the source can't be
    /// read or parsed. Only the value relations + `resolves` call
    /// this; `build` guarantees `source_extract` is `Some` for them.
    pub(super) fn source_values(
        &self,
        ctx: &Context<'_>,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let extract = self.source_extract.as_ref()?;
        if let Some(scope) = &self.source_glob {
            // Glob-union source: extract from every matching file and
            // union the values into one set (the set relations only).
            let mut all = Vec::new();
            let mut matched = 0usize;
            for entry in ctx.index.files() {
                if scope.matches(&entry.path, ctx.index) {
                    matched += 1;
                    if let Some(vals) =
                        self.source_values_from(ctx, &entry.path, extract, out, true)
                    {
                        all.extend(vals);
                    }
                }
            }
            // A glob that matches nothing is a misconfiguration (a
            // typo'd path) — fire, mirroring the target-glob behaviour,
            // rather than silently passing `subset` / yielding a
            // confusing empty-set `set_equals` diff.
            if matched == 0 {
                if !self.allow_missing {
                    out.push(Self::violation(
                        Path::new(&self.source_file),
                        "`source.files` glob matched no files",
                    ));
                }
                return None;
            }
            return Some(all);
        }
        self.source_values_from(ctx, Path::new(&self.source_file), extract, out, false)
    }

    /// Read one source file + extract its literal values - the
    /// per-file body shared by the single-`file` and `files:`
    /// glob-union forms.
    pub(super) fn source_values_from(
        &self,
        ctx: &Context<'_>,
        src: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
        discovered: bool,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, src) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(alint_core::too_large_file_violation(src, n));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e))
                if discovered && e.kind() == std::io::ErrorKind::NotFound =>
            {
                out.push(alint_core::missing_file_violation(src));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push(Self::violation(
                    src,
                    &format!("source file is missing: {e}"),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(unreadable_file_violation(src, &e));
                return None;
            }
        };
        let values = match extract_values_bounded(extract, &text, super::MAX_EXTRACTED_VALUES) {
            Ok(v) => v,
            Err(e) => {
                let mut finding = Self::analysis_failure(
                    src,
                    AnalysisFailureKind::ResourceParse,
                    &format!("source extract failed: {e}"),
                );
                if extract.redacts_by_default() {
                    finding.redact_message(&self.id);
                }
                out.push(finding);
                return None;
            }
        };
        let has_non_strings = !values.non_strings.is_empty();
        if let Some(mut violation) =
            crate::non_string_extract_violation(src, "source extract", &values.non_strings)
        {
            self.redact_extracted(&mut violation, "source", src, extract, "non_string", &[]);
            out.push(violation);
        }
        if has_non_strings && values.strings.is_empty() {
            return None;
        }
        // Whole-file content is compared verbatim — the non-literal
        // skip (for interpolated *paths*) does not apply.
        if matches!(extract, Extract::WholeFile) {
            return Some(values.strings);
        }
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.strings.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            let mut finding = Self::note(src, &format!("skipped non-literal source value {v:?}"));
            self.redact_extracted(&mut finding, "source", src, extract, "skipped", &[v]);
            out.push(finding);
        }
        Some(literal)
    }

    /// Iterate the value-relation targets, calling
    /// `f(target_path, raw_literal_values, out)` for each readable
    /// target. Read/extract errors and a zero-match glob are
    /// reported here, so `f` sees only resolvable targets. `build`
    /// guarantees `targets` is `Some` and every `extract` is `Some`
    /// for the value relations that call this.
    pub(super) fn each_target(
        &self,
        ctx: &Context<'_>,
        out: &mut Vec<Violation>,
        f: &mut TargetFn<'_>,
    ) {
        match &self.targets {
            Some(Targets::Glob {
                scope,
                extract: Some(extract),
                matches,
            }) => {
                let mut matched = 0usize;
                for e in ctx.index.files() {
                    if !scope.matches(&e.path, ctx.index) {
                        continue;
                    }
                    matched += 1;
                    if let Some(mut values) = self.target_values(ctx, &e.path, extract, out) {
                        if let Some(regex) = matches {
                            values.retain(|value| regex.is_match(value));
                        }
                        if values.is_empty() && self.allow_missing {
                            continue;
                        }
                        f(&e.path, extract, &values, out);
                    }
                }
                if matched == 0 && !self.allow_missing {
                    out.push(Self::violation(
                        Path::new(&self.source_file),
                        "targets glob matched no files",
                    ));
                }
            }
            Some(Targets::List(list)) => {
                for target_spec in list {
                    let Some(extract) = &target_spec.extract else {
                        continue;
                    };
                    // `file` is already normalized at resolution (`resolve_targets`),
                    // so the violation path matches git's canonical diff spelling and
                    // the value fixer's stored target path (audit F2).
                    let target = Path::new(&target_spec.file);
                    if let Some(mut values) = self.target_values(ctx, target, extract, out) {
                        if let Some(regex) = &target_spec.matches {
                            values.retain(|value| regex.is_match(value));
                        }
                        if values.is_empty() && self.allow_missing {
                            continue;
                        }
                        f(target, extract, &values, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// Read + extract one target's raw literal values. `None`
    /// (with a violation pushed, unless `allow_missing` for the
    /// missing-file case) when the target can't be read or parsed.
    pub(super) fn target_values(
        &self,
        ctx: &Context<'_>,
        target: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, target) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(alint_core::too_large_file_violation(target, n));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                if !self.allow_missing {
                    out.push(Self::violation(target, "target file is missing"));
                }
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(unreadable_file_violation(target, &e));
                return None;
            }
        };
        let values = match extract {
            Extract::Regex(pattern) => super::target_regex_capture_ranges(target, &text, pattern)
                .map(|captures| ExtractedValues {
                    strings: captures
                        .ranges
                        .into_iter()
                        .map(|range| text[range].to_string())
                        .collect(),
                    non_strings: Vec::new(),
                }),
            _ => extract_values_bounded(extract, &text, super::MAX_EXTRACTED_VALUES),
        };
        let values = match values {
            Ok(values) => values,
            Err(error) => {
                let mut finding = Self::analysis_failure(
                    target,
                    AnalysisFailureKind::ResourceParse,
                    &format!("target extract failed: {error}"),
                );
                if extract.redacts_by_default() {
                    finding.redact_message(&self.id);
                }
                out.push(finding);
                return None;
            }
        };
        let has_non_strings = !values.non_strings.is_empty();
        if let Some(mut violation) =
            crate::non_string_extract_violation(target, "target extract", &values.non_strings)
        {
            self.redact_extracted(&mut violation, "target", target, extract, "non_string", &[]);
            out.push(violation);
        }
        if has_non_strings && values.strings.is_empty() {
            return None;
        }
        // Whole-file content is compared verbatim — the non-literal
        // skip (for interpolated *paths*) does not apply.
        if matches!(extract, Extract::WholeFile) {
            return Some(values.strings);
        }
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.strings.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            let mut finding =
                Self::note(target, &format!("skipped non-literal target value {v:?}"));
            self.redact_extracted(&mut finding, "target", target, extract, "skipped", &[v]);
            out.push(finding);
        }
        Some(literal)
    }
}

/// Lexically confine `rel`, then verify it doesn't escape the root through
/// an in-repo symlink once joined - `normalize_confined` is symlink-blind,
/// so `link/secret` (with `link -> /etc`) would otherwise read out of the
/// tree. `None` on either escape; the caller reports "escapes the repo
/// root". Used by every cross-file *read* path.
pub(super) fn confined_rel(ctx: &Context<'_>, rel: &Path) -> Option<PathBuf> {
    let p = crate::pathsafe::normalize_confined(rel)?;
    crate::pathsafe::resolved_within_root(&ctx.root.join(&p), ctx.root).then_some(p)
}

/// Read a tree-relative path as text (the index stores paths, not
/// contents, so the cross-file rules read the file themselves).
pub(super) fn read_rel(ctx: &Context<'_>, rel: &Path) -> Result<String, crate::io::ReadCapError> {
    // Confine to the repo root before any read — an absolute or
    // root-escaping (lexically or via symlink) `source.file` /
    // `targets[].file` must never read a file outside the tree.
    let Some(rel) = confined_rel(ctx, rel) else {
        return Err(crate::io::ReadCapError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path escapes the repo root",
        )));
    };
    crate::io::read_capped(&ctx.root.join(rel)).map(|b| String::from_utf8_lossy(&b).into_owned())
}
