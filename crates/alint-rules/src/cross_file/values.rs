//! Reading the values a `cross_file` relation compares, through the relation
//! layer: each file is a [`Resource`] that the run's [`ReadCache`] reads and an
//! [`ExtractAdapter`] decodes (a target `regex:` goes through the
//! target-syntax filter instead). A file the config names (`source.file`, a
//! `targets` entry) is kept and decoded once per run, for every rule that
//! names it; a file a glob matches is read and decoded without being kept, so
//! the run's memory does not grow with the matches. Every value relation,
//! `resolves` and `registered` read through here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use alint_core::relation::{
    Adapter as _, AdapterError, AdapterNote, DecodeCache, ExtractAdapter, LimitKind, Limits,
    ReadCache, RelPath, Resource, ResourceRead, Value,
};
use alint_core::{AnalysisFailureKind, Context, Extract, NonStringValueKind, Violation};

use super::CrossFileRule;
use super::spec::Targets;

/// Per-target callback for `each_target`: receives the target path, extractor,
/// raw literal values, and the violation sink.
pub(super) type TargetFn<'a> = dyn FnMut(&Path, &Extract, &[String], &mut Vec<Violation>) + 'a;

/// Which side of the relation a file is on, and whether the config names it or
/// a glob matched it.
#[derive(Clone, Copy)]
enum Role {
    NamedSource,
    MatchedSource,
    NamedTarget,
    MatchedTarget,
}

impl Role {
    fn side(self) -> &'static str {
        match self {
            Self::NamedSource | Self::MatchedSource => "source",
            Self::NamedTarget | Self::MatchedTarget => "target",
        }
    }

    fn matched(self) -> bool {
        matches!(self, Self::MatchedSource | Self::MatchedTarget)
    }
}

/// The run's read and decode caches, or fresh ones when the engine did not
/// enable them (a rule evaluated directly, as the unit tests do).
#[derive(Clone, Copy)]
struct Caches<'a> {
    reads: &'a ReadCache,
    decodes: &'a DecodeCache,
}

impl<'a> Caches<'a> {
    fn of(ctx: &'a Context<'_>, local: &'a mut Option<(ReadCache, DecodeCache)>) -> Self {
        if let (Some(reads), Some(decodes)) = (ctx.index.read_cache(), ctx.index.decode_cache()) {
            return Self { reads, decodes };
        }
        let (reads, decodes) = local.insert((ReadCache::new(), DecodeCache::new()));
        Self { reads, decodes }
    }
}

/// One file's decoded values: the literal strings, the kinds of the non-string
/// values the extract selected, and the computed values it skipped.
type Decoded = (Vec<String>, Vec<NonStringValueKind>, Vec<String>);

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
        let adapter = ExtractAdapter::new(extract.clone());
        let mut local = None;
        let caches = Caches::of(ctx, &mut local);
        if let Some(scope) = &self.source_glob {
            // Glob-union source: extract from every matching file and
            // union the values into one set (the set relations only).
            let mut all = Vec::new();
            let mut matched = 0usize;
            for entry in ctx.index.files() {
                if scope.matches(&entry.path, ctx.index) {
                    matched += 1;
                    if let Some(vals) = self.read_values(
                        ctx,
                        caches,
                        Role::MatchedSource,
                        &entry.path,
                        &adapter,
                        out,
                    ) {
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
        let source = Path::new(&self.source_file);
        self.read_values(ctx, caches, Role::NamedSource, source, &adapter, out)
    }

    /// Iterate the value-relation targets, calling
    /// `f(target_path, raw_literal_values, out)` for each readable
    /// target. Read/extract errors and a zero-match glob are
    /// reported here, so `f` sees only resolvable targets, and under
    /// `allow_missing_target` only targets with values. `build`
    /// guarantees `targets` is `Some` and every `extract` is `Some`
    /// for the value relations that call this.
    pub(super) fn each_target(
        &self,
        ctx: &Context<'_>,
        out: &mut Vec<Violation>,
        f: &mut TargetFn<'_>,
    ) {
        let mut local = None;
        let caches = Caches::of(ctx, &mut local);
        match &self.targets {
            Some(Targets::Glob {
                scope,
                extract: Some(extract),
                matches,
            }) => {
                let adapter = ExtractAdapter::new(extract.clone());
                let mut matched = 0usize;
                for e in ctx.index.files() {
                    if !scope.matches(&e.path, ctx.index) {
                        continue;
                    }
                    matched += 1;
                    let role = Role::MatchedTarget;
                    if let Some(mut values) =
                        self.read_values(ctx, caches, role, &e.path, &adapter, out)
                    {
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
                    let adapter = ExtractAdapter::new(extract.clone());
                    let role = Role::NamedTarget;
                    if let Some(mut values) =
                        self.read_values(ctx, caches, role, target, &adapter, out)
                    {
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
    /// Read and decode one file's values (raw, not normalized: callers
    /// normalize as the relation needs). `None`, with a finding pushed, when the
    /// file is missing (no finding for a missing target under
    /// `allow_missing_target`), over the size cap, unreadable, fails to decode,
    /// or selects only non-string values.
    fn read_values(
        &self,
        ctx: &Context<'_>,
        caches: Caches<'_>,
        role: Role,
        path: &Path,
        adapter: &ExtractAdapter,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let side = role.side();
        let extract = adapter.extractor();
        // An absolute or root-escaping path never reads outside the tree: the
        // lexical check is here, and the read rejects an escape through a
        // symlink. A matched file is read as a literal path too, with the same
        // containment and file-type checks.
        let Some(relative) = RelPath::from_os_path(path) else {
            out.push(unreadable(path, "path escapes the repo root"));
            return None;
        };
        let resource = Resource::literal(relative);
        let read = if role.matched() {
            caches.reads.read_unretained(ctx.root, &resource, 0)
        } else {
            caches.reads.read(ctx.root, &resource, 0)
        };
        let bytes = match read.as_ref() {
            ResourceRead::Present(bytes) => bytes,
            ResourceRead::TooLarge(length) => {
                out.push(alint_core::too_large_file_violation(path, *length));
                return None;
            }
            ResourceRead::Missing => {
                self.missing(ctx, role, path, &resource, out);
                return None;
            }
            ResourceRead::Unreadable(message) => {
                out.push(unreadable(path, message));
                return None;
            }
        };
        let decoded = match (role, extract) {
            (Role::NamedTarget | Role::MatchedTarget, Extract::Regex(pattern)) => {
                target_regex_values(path, bytes, pattern)
            }
            _ => decode(caches, role, &resource, bytes, adapter),
        };
        let (values, non_strings, skipped) = match decoded {
            Ok(decoded) => decoded,
            Err(message) => {
                let mut finding = Self::analysis_failure(
                    path,
                    AnalysisFailureKind::ResourceParse,
                    &format!("{side} extract failed: {message}"),
                );
                if extract.redacts_by_default() {
                    finding.redact_message(&self.id);
                }
                out.push(finding);
                return None;
            }
        };
        if let Some(mut violation) =
            crate::non_string_extract_violation(path, &format!("{side} extract"), &non_strings)
        {
            self.redact_extracted(&mut violation, side, path, extract, "non_string", &[]);
            out.push(violation);
        }
        if !non_strings.is_empty() && values.is_empty() && skipped.is_empty() {
            return None;
        }
        for value in &skipped {
            let mut finding =
                Self::note(path, &format!("skipped non-literal {side} value {value:?}"));
            self.redact_extracted(&mut finding, side, path, extract, "skipped", &[value]);
            out.push(finding);
        }
        Some(values)
    }

    /// The finding for a file that is not there.
    fn missing(
        &self,
        ctx: &Context<'_>,
        role: Role,
        path: &Path,
        resource: &Resource,
        out: &mut Vec<Violation>,
    ) {
        match role {
            Role::NamedSource => {
                // The read records only that the file is absent; the finding
                // quotes the operating system's reason, as it always has.
                let message = match std::fs::metadata(ctx.root.join(resource.path().as_path())) {
                    Err(error) => format!("source file is missing: {error}"),
                    Ok(_) => "source file is missing".to_string(),
                };
                out.push(Self::violation(path, &message));
            }
            // The glob matched it, so it vanished after the walk.
            Role::MatchedSource => out.push(alint_core::missing_file_violation(path)),
            Role::NamedTarget | Role::MatchedTarget => {
                if !self.allow_missing {
                    out.push(Self::violation(path, "target file is missing"));
                }
            }
        }
    }
}

/// Decode through the adapter, under the per-file value limit `cross_file`
/// has always applied: a named file once per run, a matched one without
/// keeping the output.
fn decode(
    caches: Caches<'_>,
    role: Role,
    resource: &Resource,
    bytes: &[u8],
    adapter: &ExtractAdapter,
) -> Result<Decoded, String> {
    let limits = Limits::default()
        .with(LimitKind::RowsPerExtractor, super::MAX_EXTRACTED_VALUES)
        .expect("the value limit is below the hard cap");
    let output = if role.matched() {
        adapter.extract(resource, bytes, &limits).map(Arc::new)
    } else {
        caches.decodes.decode(adapter, resource, bytes, &limits)
    };
    let output = output.map_err(|error| match error {
        AdapterError::Parse(message) => message,
        AdapterError::Limit(limit) if limit.kind == LimitKind::RowsPerExtractor => format!(
            "extract selected more than the {}-value safety limit",
            limit.limit
        ),
        AdapterError::Limit(limit) => limit.to_string(),
    })?;
    let values = output
        .rows
        .iter()
        .filter_map(|row| match row.value() {
            Some(Value::String(text)) => Some(text.to_string()),
            _ => None,
        })
        .collect();
    let skipped = output
        .notes
        .iter()
        .map(|note| match note {
            AdapterNote::NonLiteral { value } => value.clone(),
        })
        .collect();
    Ok((values, output.non_strings.clone(), skipped))
}

/// A target `regex:`'s values: capture group 1 of each match the target-syntax
/// filter accepts (Dockerfile `FROM`, `__version__`, and similar).
fn target_regex_values(path: &Path, bytes: &[u8], pattern: &str) -> Result<Decoded, String> {
    let text = String::from_utf8_lossy(bytes);
    let captures = super::target_regex_capture_ranges(path, &text, pattern)?;
    let (skipped, values) = captures
        .ranges
        .into_iter()
        .map(|range| text[range].to_string())
        .partition(|value| alint_core::is_non_literal(value));
    Ok((values, Vec::new(), skipped))
}

/// The analysis failure for a file the read could not open or finish.
fn unreadable(path: &Path, message: &str) -> Violation {
    Violation::new(format!("could not read file: {message}"))
        .with_path(path.to_path_buf())
        .as_analysis_failure(AnalysisFailureKind::ResourceRead)
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

#[cfg(test)]
#[path = "values_tests.rs"]
mod tests;
