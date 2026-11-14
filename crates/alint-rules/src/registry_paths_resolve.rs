//! `registry_paths_resolve` — a manifest file enumerates
//! path-like entries; each must resolve to a path in the walked tree.
//! Optional reverse "orphan" check: walked paths in a declared
//! space that no entry references.
//!
//! Cross-file: reads one manifest and resolves its entries
//! against the engine `FileIndex` (O(1) per entry via the lazy
//! path-set). Design + rationale + open-question resolutions:
//! `docs/design/v0.10/registry_paths_resolve.md`.
//!
//! ```yaml
//! - id: cargo-workspace-members-resolve
//!   kind: registry_paths_resolve
//!   source: Cargo.toml
//!   extract: { toml: "$.workspace.members[*]" }
//!   base: registry_dir          # registry_dir (default) | lint_root | "<path>"
//!   entries_are_globs: true
//!   expect: dir                 # any (default) | file | dir
//!   must_contain: Cargo.toml
//!   exclude_query: "$.workspace.exclude[*]"
//!   orphans: { space: "crates/*" }
//!   level: error
//! ```

use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use alint_core::{
    AnalysisFailureKind, Context, Error, Extract, ExtractSpec, Format, Level, RelatedLocation,
    Result, Rule, RuleSpec, Scope, Violation, extract_values, is_non_literal,
    unreadable_file_violation,
};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Expect {
    #[default]
    Any,
    File,
    Dir,
}

/// Enable the reverse-completeness check: paths in the walked tree under the
/// `space` glob that no entry references. Findings use the rule's top-level `level`;
/// omit `orphans` to disable the reverse check.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[schemars(rename = "RegistryOrphansSpec")]
#[serde(deny_unknown_fields)]
struct OrphansSpec {
    /// Glob of walked paths that should each be referenced.
    space: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The manifest/registry file (path, or a glob to run once per matching
    /// manifest) that enumerates the path entries.
    source: String,
    // A registry entry is a PATH; `whole_file` (the entire manifest as one
    // value) is nonsensical here, and the hand-written schema excluded it.
    // schemars cannot drop a field from the shared extract_spec, so exclude it
    // schema-side with a `not: { required: [whole_file] }` guard. The loader is
    // unchanged (it was already lenient here, as it is for the other kinds).
    #[schemars(extend("not" = {"required": ["whole_file"]}))]
    extract: ExtractSpec,
    /// Resolve entries relative to: `registry_dir` (default), `lint_root`, or
    /// an explicit path.
    #[serde(default)]
    base: Option<String>,
    /// Expand each extracted entry as a glob rather than treating it as a
    /// single literal path; a glob that matches nothing is a violation.
    #[serde(default)]
    entries_are_globs: bool,
    /// Constrain the kind each entry must resolve to: `any` (default), `file`,
    /// or `dir`. Entries resolve against the walked tree, so a path an ignore
    /// file or `ignore:` excludes does not resolve.
    #[serde(default)]
    expect: Expect,
    /// When an entry resolves to a directory, that directory must contain this
    /// named child (e.g. `Cargo.toml`), else the entry is a violation.
    #[serde(default)]
    must_contain: Option<String>,
    /// A structured query selecting entries to subtract from the extracted list
    /// before resolution is checked.
    #[serde(default)]
    exclude_query: Option<String>,
    /// Enable the reverse-completeness check. Findings use the rule's
    /// top-level `level`; omit this option to disable the check.
    #[serde(default)]
    orphans: Option<OrphansSpec>,
}

crate::options_schema_for!(Options);

/// Resolution base for entries.
#[derive(Debug, Clone)]
enum Base {
    /// Directory containing the registry file (default; matches
    /// Cargo / npm semantics + alint's nested-manifest model).
    RegistryDir,
    /// The lint root.
    LintRoot,
    /// An explicit path, relative to the lint root.
    Explicit(PathBuf),
}

impl Base {
    fn parse(raw: Option<&str>) -> Self {
        match raw {
            None | Some("registry_dir") => Self::RegistryDir,
            Some("lint_root") => Self::LintRoot,
            Some(p) => Self::Explicit(PathBuf::from(p)),
        }
    }
}

type RegistryEntries = (
    Vec<String>,
    Vec<String>,
    Vec<alint_core::NonStringValueKind>,
);

#[derive(Debug)]
pub struct RegistryPathsResolveRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    source: String,
    registry_scope: Option<Scope>,
    extract: Extract,
    base: Base,
    entries_are_globs: bool,
    expect: Expect,
    must_contain: Option<String>,
    exclude_extract: Option<Extract>,
    orphans: Option<OrphansSpec>,
    /// Permit reading a `source:` registry file that escapes the repo
    /// root - set post-build from the top-level `allow_out_of_root:`
    /// policy. (The declared *entries* stay confined regardless.)
    allow_out_of_root: bool,
}

impl Rule for RegistryPathsResolveRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: an entry's verdict depends on whether its
        // target exists anywhere in the tree, and the orphan
        // check needs the whole index — never `--changed`-scoped.
        true
    }

    fn set_allow_out_of_root(&mut self, allow: bool) {
        self.allow_out_of_root = allow;
    }

    fn visit_sensitive_paths(
        &self,
        explicit: bool,
        ctx: &Context<'_>,
        visit: &mut dyn FnMut(&Path),
    ) {
        let manifest_sensitive = self
            .registry_scope
            .as_ref()
            .is_some_and(|scope| scope.sensitive_manifest_sources().next().is_some());
        if let Some(scope) = &self.registry_scope {
            for path in scope.sensitive_manifest_sources() {
                visit(path);
            }
        }
        if !explicit
            && !manifest_sensitive
            && !self.extract.redacts_by_default()
            && !self
                .exclude_extract
                .as_ref()
                .is_some_and(Extract::redacts_by_default)
        {
            return;
        }
        for path in self.registry_files(ctx) {
            visit(&path);
        }
    }

    // One pass preserves registry-local parse, exclusion, and orphan ownership.
    #[allow(clippy::too_many_lines)]
    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        // Directory existence: build the dir path-set once per
        // eval (O(D)); per-entry lookups are then O(1), matching
        // `contains_file`'s scaling so the rule stays index-fast.
        let dir_set: HashSet<&Path> = if self.expect == Expect::Dir
            || self.expect == Expect::Any
            || self.must_contain.is_some()
        {
            ctx.index.dirs().map(|e| &*e.path).collect()
        } else {
            HashSet::new()
        };

        for registry_rel in self.registry_files(ctx) {
            // Confine the (config-author-controlled) registry path before
            // reading it (the glob-source arm yields in-tree index paths,
            // for which this is a no-op).
            let Some(registry_rel) = self.confine_source(registry_rel, ctx.root, &mut violations)
            else {
                continue;
            };
            let abs = ctx.root.join(&registry_rel);
            let text = match crate::io::read_capped(&abs) {
                Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    violations.push(alint_core::too_large_file_violation(&registry_rel, n));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    violations.push(alint_core::missing_file_violation(&registry_rel));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) => {
                    violations.push(unreadable_file_violation(&registry_rel, &e));
                    continue;
                }
            };

            let (entries, skipped, non_strings) = match self.extract_entries(&text) {
                Ok(v) => v,
                Err(e) => {
                    let mut finding = Violation::new(format!(
                        "registry file {} could not be parsed for `extract`: {e}",
                        registry_rel.display()
                    ))
                    .with_path(registry_rel.clone())
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse);
                    if self.extract.redacts_by_default() {
                        finding.redact_message(&self.id);
                    }
                    violations.push(finding);
                    continue;
                }
            };
            if let Some(mut violation) =
                crate::non_string_extract_violation(&registry_rel, "registry extract", &non_strings)
            {
                self.redact_extracted(
                    &mut violation,
                    &registry_rel,
                    &self.extract,
                    "non_string",
                    std::iter::empty(),
                );
                violations.push(violation);
            }
            self.note_skipped(&registry_rel, &skipped, &mut violations);

            let excluded = match self.excluded_entries(&text) {
                Ok(values) => values,
                Err(e) => {
                    let mut finding = Violation::new(format!(
                        "registry file {} could not be parsed for `exclude_query`: {e}",
                        registry_rel.display()
                    ))
                    .with_path(registry_rel.clone())
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse);
                    if self
                        .exclude_extract
                        .as_ref()
                        .is_some_and(Extract::redacts_by_default)
                    {
                        finding.redact_message(&self.id);
                    }
                    violations.push(finding);
                    continue;
                }
            };
            if let Some(mut violation) = crate::non_string_extract_violation(
                &registry_rel,
                "exclude query",
                &excluded.non_strings,
            ) {
                if let Some(extract) = &self.exclude_extract {
                    self.redact_extracted(
                        &mut violation,
                        &registry_rel,
                        extract,
                        "exclude_non_string",
                        std::iter::empty(),
                    );
                }
                violations.push(violation);
            }
            let excluded: HashSet<String> = excluded.strings.into_iter().collect();
            let base_dir = self.base_dir(&registry_rel);

            let mut covered: Vec<PathBuf> = Vec::new();
            for entry in &entries {
                if excluded.contains(entry) {
                    continue;
                }
                let Some(resolved) = crate::pathsafe::normalize_confined(&base_dir.join(entry))
                else {
                    // An absolute / root-escaping declared path can never
                    // resolve to an in-tree path.
                    violations.push(self.violation(
                        &registry_rel,
                        entry,
                        &Reason::new("escapes the repo root"),
                    ));
                    continue;
                };
                if self.entries_are_globs {
                    let matches = Self::glob_matches(ctx, &resolved);
                    if matches.is_empty() {
                        // The declaration is a pattern, not a concrete path.
                        // Keep the violation anchored on the registry only.
                        violations.push(self.violation(
                            &registry_rel,
                            entry,
                            &Reason::reworded(
                                "matched no path (missing or ignored)",
                                "matched no path on disk",
                            ),
                        ));
                    } else {
                        covered.extend(matches);
                    }
                    continue;
                }
                covered.push(resolved.clone());
                if let Some(reason) = self.existence_problem(ctx, &resolved, &dir_set) {
                    violations.push(
                        self.violation(&registry_rel, entry, &reason)
                            .with_related_location(RelatedLocation::new(
                                &resolved,
                                "declared target",
                            )),
                    );
                }
            }

            // Globbed entries still need existence/kind checks on
            // each expansion (a `crates/*` match must satisfy
            // `must_contain`, etc.).
            if self.entries_are_globs {
                for p in &covered {
                    if let Some(reason) = self.existence_problem(ctx, p, &dir_set) {
                        violations.push(
                            self.violation(&registry_rel, &p.display().to_string(), &reason)
                                .with_related_location(RelatedLocation::new(p, "resolved target")),
                        );
                    }
                }
            }

            self.check_orphans(ctx, &registry_rel, &covered, &mut violations);
        }

        Ok(violations)
    }
}

impl RegistryPathsResolveRule {
    /// The registry file(s): a literal path, or every index path
    /// matching the glob.
    fn registry_files(&self, ctx: &Context<'_>) -> Vec<PathBuf> {
        match &self.registry_scope {
            None => vec![PathBuf::from(&self.source)],
            Some(scope) => ctx
                .index
                .files()
                .filter(|e| scope.matches(&e.path, ctx.index))
                .map(|e| e.path.to_path_buf())
                .collect(),
        }
    }

    fn base_dir(&self, registry_rel: &Path) -> PathBuf {
        match &self.base {
            Base::RegistryDir => registry_rel
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            Base::LintRoot => PathBuf::new(),
            Base::Explicit(p) => p.clone(),
        }
    }

    fn extract_entries(&self, text: &str) -> std::result::Result<RegistryEntries, String> {
        let raw = extract_values(&self.extract, text)?;
        // Non-literal (computed/interpolated) entries can't be
        // statically resolved; they're surfaced as notes, not failed.
        let (skipped, kept): (Vec<String>, Vec<String>) =
            raw.strings.into_iter().partition(|e| is_non_literal(e));
        Ok((kept, skipped, raw.non_strings))
    }

    fn excluded_entries(
        &self,
        text: &str,
    ) -> std::result::Result<alint_core::ExtractedValues, String> {
        let Some(extract) = &self.exclude_extract else {
            return Ok(alint_core::ExtractedValues::default());
        };
        extract_values(extract, text)
    }

    /// Reverse-completeness: walked paths under `orphans.space`
    /// that no (post-expansion) entry covered.
    fn check_orphans(
        &self,
        ctx: &Context<'_>,
        registry_rel: &Path,
        covered: &[PathBuf],
        out: &mut Vec<Violation>,
    ) {
        let Some(orph) = &self.orphans else {
            return;
        };
        let Ok(space) = Scope::from_patterns(std::slice::from_ref(&orph.space)) else {
            return;
        };
        // `space` describes artefacts, not only files. Cargo workspaces, for
        // example, naturally use `space: "crates/*"`, whose candidates are
        // directories. A manifest entry covers both the exact artefact and any
        // file underneath a referenced directory (`crates/a` covers
        // `crates/a/Cargo.toml`).
        // Ordered collection keeps findings deterministic even though the
        // file index's internal representation is not part of this contract.
        let mut candidates: BTreeSet<PathBuf> = BTreeSet::new();
        for path in ctx
            .index
            .files()
            .map(|entry| &entry.path)
            .chain(ctx.index.dirs().map(|entry| &entry.path))
        {
            if space.matches(path, ctx.index) {
                candidates.insert(path.to_path_buf());
            }
        }
        for candidate in candidates {
            let is_covered = covered
                .iter()
                .any(|entry| candidate == *entry || candidate.starts_with(entry));
            if !is_covered {
                let mut finding = Violation::new(format!(
                    "{} is under `{}` but no entry in {} references it",
                    candidate.display(),
                    orph.space,
                    registry_rel.display(),
                ))
                .with_path(candidate);
                self.redact_extracted(
                    &mut finding,
                    registry_rel,
                    &self.extract,
                    "orphan",
                    std::iter::empty(),
                );
                if let Some(extract) = &self.exclude_extract {
                    self.redact_extracted(
                        &mut finding,
                        registry_rel,
                        extract,
                        "orphan_exclusion",
                        std::iter::empty(),
                    );
                }
                finding.set_related_locations(std::iter::once(RelatedLocation::new(
                    registry_rel,
                    "registry source",
                )));
                out.push(finding);
            }
        }
    }

    fn glob_matches(ctx: &Context<'_>, pattern: &Path) -> Vec<PathBuf> {
        let pat = pattern.to_string_lossy().into_owned();
        let Ok(scope) = Scope::from_patterns(&[pat]) else {
            return Vec::new();
        };
        ctx.index
            .files()
            .filter(|e| scope.matches(&e.path, ctx.index))
            .map(|e| e.path.to_path_buf())
            .chain(
                ctx.index
                    .dirs()
                    .filter(|e| scope.matches(&e.path, ctx.index))
                    .map(|e| e.path.to_path_buf()),
            )
            .collect()
    }

    /// `None` => the resolved path is fine. `Some(reason)` => why the
    /// entry is a violation.
    fn existence_problem(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        dir_set: &HashSet<&Path>,
    ) -> Option<Reason> {
        let is_file = ctx.index.contains_file(path);
        let is_dir = dir_set.contains(path);
        match self.expect {
            Expect::File => {
                if !is_file {
                    // `dir_set` is empty for this kind: a path that is not a
                    // file but is indexed is a directory.
                    return Some(Reason::reworded(
                        if ctx.index.contains_path(path) {
                            "resolves to a directory, not a file"
                        } else {
                            "does not resolve to a file (missing or ignored)"
                        },
                        "does not resolve to a file on disk",
                    ));
                }
            }
            Expect::Dir => {
                if !is_dir {
                    return Some(Reason::reworded(
                        if is_file {
                            "resolves to a file, not a directory"
                        } else {
                            "does not resolve to a directory (missing or ignored)"
                        },
                        "does not resolve to a directory on disk",
                    ));
                }
            }
            Expect::Any => {
                if !is_file && !is_dir {
                    return Some(Reason::reworded(
                        "does not resolve to any path (missing or ignored)",
                        "does not resolve to any path on disk",
                    ));
                }
            }
        }
        if let Some(mc) = &self.must_contain {
            // Only meaningful when the entry is a directory.
            if is_dir && !ctx.index.contains_file(&path.join(mc)) {
                return Some(Reason {
                    shown: Cow::Owned(format!(
                        "resolves to a directory without `{mc}` (missing or ignored)"
                    )),
                    key: Cow::Owned(format!("resolves to a directory missing `{mc}`")),
                });
            }
        }
        None
    }

    /// Confine the registry `source:` path, honoring `allow_out_of_root`.
    /// Returns the path to read; `None` (with a violation or note already
    /// pushed to `out`) when the source escapes the root and isn't
    /// permitted. The declared *entries* stay confined regardless.
    fn confine_source(
        &self,
        rel: PathBuf,
        root: &Path,
        out: &mut Vec<Violation>,
    ) -> Option<PathBuf> {
        match crate::pathsafe::confine_read(&rel, root, self.allow_out_of_root) {
            crate::pathsafe::Confined::In(p) => Some(p),
            crate::pathsafe::Confined::AllowedEscape(p) => {
                out.push(
                    Violation::new(crate::pathsafe::out_of_root_note(&rel))
                        .as_note()
                        .with_path(rel),
                );
                Some(p)
            }
            crate::pathsafe::Confined::Denied => {
                out.push(
                    Violation::new(format!(
                        "registry source {} escapes the repo root",
                        rel.display()
                    ))
                    .with_path(rel),
                );
                None
            }
        }
    }

    /// Surface each non-literal (interpolated/computed) entry as an
    /// informational note rather than a silent skip (v0.11; see
    /// `docs/design/v0.11/informational_findings.md`).
    fn note_skipped(&self, registry: &Path, skipped: &[String], out: &mut Vec<Violation>) {
        for entry in skipped {
            let mut finding = Violation::new(format!(
                "registry {}: skipped non-literal entry {entry:?} \
                 (cannot statically resolve an interpolated/computed path)",
                registry.display()
            ))
            .with_path(registry.to_path_buf())
            .as_note();
            self.redact_extracted(
                &mut finding,
                registry,
                &self.extract,
                "skipped",
                std::iter::once(entry.as_str()),
            );
            out.push(finding);
        }
    }

    fn violation(&self, registry: &Path, entry: &str, reason: &Reason) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{}: entry {entry:?} {}", registry.display(), reason.shown));
        let mut finding = Violation::new(msg)
            .with_path(registry.to_path_buf())
            .with_baseline_key(format!(
                "entry\u{0}{}\u{0}{entry}\u{0}{}",
                crate::slash(registry),
                reason.key
            ));
        self.redact_extracted(
            &mut finding,
            registry,
            &self.extract,
            &reason.key,
            std::iter::once(entry),
        );
        finding
    }

    fn redact_extracted<'a>(
        &self,
        finding: &mut Violation,
        source: &Path,
        extract: &Extract,
        category: &str,
        evidence: impl IntoIterator<Item = &'a str>,
    ) {
        if !extract.redacts_by_default() {
            return;
        }
        let locator = format!(
            "registry\0{}\0{extract:?}\0{category}",
            crate::slash(source)
        );
        finding.redact_evidence(&self.id, &locator, evidence);
    }
}

/// Why a declared entry is a violation.
struct Reason {
    /// The wording in the message.
    shown: Cow<'static, str>,
    /// The wording the baseline key and the redaction locator use.
    key: Cow<'static, str>,
}

impl Reason {
    fn new(text: impl Into<Cow<'static, str>>) -> Self {
        let text = text.into();
        Self {
            shown: text.clone(),
            key: text,
        }
    }

    /// A reason whose message wording changed after alint 0.22. `key` is the
    /// old wording, so findings accepted into a baseline before keep their
    /// keys. (The old "on disk" was wrong: the walk leaves out what an ignore
    /// file or `ignore:` excludes.)
    const fn reworded(shown: &'static str, key: &'static str) -> Self {
        Self {
            shown: Cow::Borrowed(shown),
            key: Cow::Borrowed(key),
        }
    }
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::FullIndex { path_scope: false },
    paths: alint_core::PathsSupport::Forbidden,
    scope_filter: alint_core::ScopeFilterSupport::Rejected {
        reason: std::borrow::Cow::Borrowed("scope_filter is not supported by this kind"),
    },
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("source"),
            selector_type: alint_core::SelectorType::Glob,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("extract"),
            selector_type: alint_core::SelectorType::Extract,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("base"),
            selector_type: alint_core::SelectorType::LiteralPath,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("orphans.space"),
            selector_type: alint_core::SelectorType::Glob,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("exclude_query"),
            selector_type: alint_core::SelectorType::Extract,
            rerootable: false,
        },
        alint_core::SelectorMeta {
            name: std::borrow::Cow::Borrowed("must_contain"),
            selector_type: alint_core::SelectorType::LiteralPath,
            rerootable: false,
        },
    ]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[alint_core::CapabilityMeta {
        capability: alint_core::Capability::OutOfRootRead,
        condition: alint_core::CapabilityCondition::CommonFieldSet {
            field: std::borrow::Cow::Borrowed("allow_out_of_root"),
        },
    }]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(alint_core::Format::ALL),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: false,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "registry_paths_resolve")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    if opts.source.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "registry_paths_resolve `source` must not be empty",
        ));
    }
    // A glob source is resolved against the index; a literal one
    // is read directly. The literal test mirrors the structured-path /
    // file_exists literal test.
    // `literal_glob_path` resolves escapes, so a `source` rendered from a
    // directory literally named `a\b` (`a\\b`) reads that file.
    let literal_source = alint_core::template::literal_glob_path(&opts.source);
    let is_glob = literal_source.is_none();
    let registry_scope = if is_glob {
        Some(
            Scope::from_patterns(std::slice::from_ref(&opts.source))
                .map_err(|e| Error::rule_config(&spec.id, format!("invalid `source` glob: {e}")))?,
        )
    } else {
        None
    };
    let extract = opts
        .extract
        .resolve()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `extract`: {e}")))?;
    if let Extract::Regex(p) = &extract {
        Regex::new(p)
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid `extract.regex`: {e}")))?;
    }
    let exclude_extract = opts
        .exclude_query
        .map(|query| {
            let format = match &extract {
                Extract::Structured(extract) => extract.format,
                _ => Format::Toml,
            };
            Extract::structured(format, query)
                .map_err(|e| Error::rule_config(&spec.id, format!("invalid `exclude_query`: {e}")))
        })
        .transpose()?;

    Ok(Box::new(RegistryPathsResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        source: literal_source.unwrap_or(opts.source),
        registry_scope,
        extract,
        base: Base::parse(opts.base.as_deref()),
        entries_are_globs: opts.entries_are_globs,
        expect: opts.expect,
        must_contain: opts.must_contain,
        exclude_extract,
        orphans: opts.orphans,
        allow_out_of_root: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::LinesOpts;
    use alint_core::{FileEntry, FileIndex};

    fn index(files: &[&str], dirs: &[&str]) -> FileIndex {
        let mut e: Vec<FileEntry> = files
            .iter()
            .map(|p| FileEntry {
                path: Path::new(p).into(),
                is_dir: false,
                size: 1,
            })
            .collect();
        e.extend(dirs.iter().map(|p| FileEntry {
            path: Path::new(p).into(),
            is_dir: true,
            size: 0,
        }));
        FileIndex::from_entries(e)
    }

    fn rule(opts: Options) -> RegistryPathsResolveRule {
        let extract = opts.extract.resolve().expect("test extract valid");
        let exclude_extract = opts.exclude_query.map(|query| {
            let format = match &extract {
                Extract::Structured(extract) => extract.format,
                _ => Format::Toml,
            };
            Extract::structured(format, query).expect("test exclude query valid")
        });
        RegistryPathsResolveRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source: opts.source,
            registry_scope: None,
            extract,
            base: Base::parse(opts.base.as_deref()),
            entries_are_globs: opts.entries_are_globs,
            expect: opts.expect,
            must_contain: opts.must_contain,
            exclude_extract,
            orphans: opts.orphans,
            allow_out_of_root: false,
        }
    }

    fn opts(source: &str, extract: Extract) -> Options {
        Options {
            source: source.into(),
            extract: extract.into(),
            base: None,
            entries_are_globs: false,
            expect: Expect::Any,
            must_contain: None,
            exclude_query: None,
            orphans: None,
        }
    }

    fn eval(r: &RegistryPathsResolveRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
        let ctx = Context {
            root,
            index: idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
            analysis_time: None,
            process_environment: None,
            trace: None,
        };
        r.evaluate(&ctx).unwrap()
    }

    #[test]
    fn dotenv_and_properties_registry_entries_are_redacted() {
        const SECRET: &str = "TOKEN_VALUE_DO_NOT_REPORT";
        for (format, file, body) in [
            (Format::Dotenv, ".env", format!("PATH={SECRET}\n")),
            (
                Format::Properties,
                "registry.properties",
                format!("PATH={SECRET}\n"),
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(file), body).unwrap();
            let entry_rule = rule(opts(file, Extract::structured(format, "$.PATH").unwrap()));
            let findings = eval(&entry_rule, dir.path(), &index(&[file], &[]));
            assert_eq!(findings.len(), 1);
            assert!(findings[0].is_sensitive());
            assert!(!findings[0].message.contains(SECRET), "{findings:?}");
            assert!(findings[0].message.contains("<redacted:"), "{findings:?}");
            assert!(findings[0].message.contains(file), "{findings:?}");

            let mut options = opts(file, Extract::structured(format, "$.PATH").unwrap());
            options.orphans = Some(OrphansSpec {
                space: "artifacts/*".into(),
            });
            let rule = rule(options);
            let findings = eval(
                &rule,
                dir.path(),
                &index(&[file, SECRET], &["artifacts/orphan"]),
            );
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert!(findings[0].is_sensitive(), "{findings:?}");
        }
    }

    #[test]
    fn lines_entries_resolve_pass_and_fail() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("MANIFEST"),
            "src/a.rs\nsrc/b.rs\n# a comment\n",
        )
        .unwrap();
        let r = rule(opts("MANIFEST", Extract::Lines(LinesOpts::default())));
        // Both present -> pass.
        let v = eval(
            &r,
            dir.path(),
            &index(&["src/a.rs", "src/b.rs", "MANIFEST"], &[]),
        );
        assert!(v.is_empty(), "{v:?}");
        // b.rs missing -> one violation.
        let v = eval(&r, dir.path(), &index(&["src/a.rs", "MANIFEST"], &[]));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("src/b.rs"));
        assert_eq!(v[0].path.as_deref(), Some(Path::new("MANIFEST")));
        assert_eq!(v[0].related_locations.len(), 1);
        assert_eq!(v[0].related_locations[0].path, Path::new("src/b.rs"));
        assert_eq!(v[0].related_locations[0].label, "declared target");
    }

    #[test]
    fn unmatched_glob_entry_has_no_bogus_file_location() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MANIFEST"), "src/*.rs\n").unwrap();
        let mut options = opts("MANIFEST", Extract::Lines(LinesOpts::default()));
        options.entries_are_globs = true;
        let findings = eval(&rule(options), dir.path(), &index(&["MANIFEST"], &[]));
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("matched no path"));
        assert_eq!(findings[0].related_locations, Vec::new());
        assert_eq!(findings[0].related_locations_omitted, 0);
    }

    #[test]
    fn reworded_reasons_keep_their_0_22_baseline_keys() {
        // The walk leaves out ignored paths, so the message no longer says
        // "on disk"; the baseline key keeps that wording so that findings
        // accepted under 0.22 still match.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MANIFEST"), "src/b.rs\n").unwrap();
        let idx = index(&["MANIFEST"], &[]);
        for (expect, kind) in [
            (Expect::Any, "any path"),
            (Expect::File, "a file"),
            (Expect::Dir, "a directory"),
        ] {
            let mut options = opts("MANIFEST", Extract::Lines(LinesOpts::default()));
            options.expect = expect;
            let findings = eval(&rule(options), dir.path(), &idx);
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(
                findings[0].message,
                format!(
                    "MANIFEST: entry \"src/b.rs\" does not resolve to {kind} (missing or ignored)"
                )
            );
            assert_eq!(
                findings[0].baseline_key.as_deref(),
                Some(
                    format!("entry\0MANIFEST\0src/b.rs\0does not resolve to {kind} on disk")
                        .as_str()
                )
            );
        }
        let mut options = opts("MANIFEST", Extract::Lines(LinesOpts::default()));
        options.entries_are_globs = true;
        let findings = eval(&rule(options), dir.path(), &idx);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].message,
            "MANIFEST: entry \"src/b.rs\" matched no path (missing or ignored)"
        );
        assert_eq!(
            findings[0].baseline_key.as_deref(),
            Some("entry\0MANIFEST\0src/b.rs\0matched no path on disk")
        );

        // An entry of the other kind says so; its key is the 0.22 one.
        std::fs::write(dir.path().join("MANIFEST"), "src/lib.rs\nsrc\n").unwrap();
        let idx = index(&["MANIFEST", "src/lib.rs"], &["src"]);
        for (expect, entry, shown, key) in [
            (
                Expect::Dir,
                "src/lib.rs",
                "resolves to a file, not a directory",
                "does not resolve to a directory on disk",
            ),
            (
                Expect::File,
                "src",
                "resolves to a directory, not a file",
                "does not resolve to a file on disk",
            ),
        ] {
            let mut options = opts("MANIFEST", Extract::Lines(LinesOpts::default()));
            options.expect = expect;
            let findings = eval(&rule(options), dir.path(), &idx);
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(
                findings[0].message,
                format!("MANIFEST: entry \"{entry}\" {shown}")
            );
            assert_eq!(
                findings[0].baseline_key.as_deref(),
                Some(format!("entry\0MANIFEST\0{entry}\0{key}").as_str())
            );
        }
        std::fs::write(dir.path().join("MANIFEST"), "src\n").unwrap();
        let mut options = opts("MANIFEST", Extract::Lines(LinesOpts::default()));
        options.must_contain = Some("Cargo.toml".to_string());
        let findings = eval(&rule(options), dir.path(), &idx);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].message,
            "MANIFEST: entry \"src\" resolves to a directory without `Cargo.toml` (missing or \
             ignored)"
        );
        assert_eq!(
            findings[0].baseline_key.as_deref(),
            Some("entry\0MANIFEST\0src\0resolves to a directory missing `Cargo.toml`")
        );
    }

    #[test]
    fn source_escape_fires_without_reading() {
        // Security regression (v0.12 path-confinement): an absolute literal
        // `source:` registry path must produce an "escapes the repo root"
        // violation, never read an out-of-tree file.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("MANIFEST"), "src/a.rs\n").unwrap();
        let r = rule(opts("/etc/hostname", Extract::Lines(LinesOpts::default())));
        let v = eval(&r, root, &index(&["src/a.rs", "MANIFEST"], &[]));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("escapes the repo root"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn source_out_of_root_read_when_allowed() {
        // With `allow_out_of_root`, an absolute out-of-tree `source:`
        // manifest is read; its entries still resolve against the lint
        // root (base: lint_root) and a note records the escape.
        let ext = tempfile::tempdir().unwrap();
        std::fs::write(ext.path().join("MANIFEST"), "src/a.rs\n").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut o = opts(
            ext.path().join("MANIFEST").to_str().unwrap(),
            Extract::Lines(LinesOpts::default()),
        );
        o.base = Some("lint_root".into());
        let mut r = rule(o);
        r.set_allow_out_of_root(true);
        let v = eval(&r, root, &index(&["src/a.rs"], &[]));
        assert!(
            v.iter().all(|x| x.note_kind().is_some()),
            "only an out-of-root note: {v:?}"
        );
        assert!(
            v.iter().any(|x| x.message.contains("allow_out_of_root")),
            "{v:?}"
        );
    }

    #[test]
    fn toml_workspace_members_expect_dir_must_contain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        )
        .unwrap();
        let mut o = opts(
            "Cargo.toml",
            Extract::structured(Format::Toml, "$.workspace.members[*]").unwrap(),
        );
        o.expect = Expect::Dir;
        o.must_contain = Some("Cargo.toml".into());
        let r = rule(o);
        // Both crate dirs exist and contain Cargo.toml -> pass.
        let idx = index(
            &[
                "crates/core/Cargo.toml",
                "crates/cli/Cargo.toml",
                "Cargo.toml",
            ],
            &["crates/core", "crates/cli"],
        );
        assert!(eval(&r, dir.path(), &idx).is_empty());
        // cli dir missing its Cargo.toml -> must_contain violation.
        let idx = index(
            &["crates/core/Cargo.toml", "Cargo.toml"],
            &["crates/core", "crates/cli"],
        );
        let v = eval(&r, dir.path(), &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("crates/cli"));
    }

    #[test]
    fn non_literal_entries_are_skipped_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pkgs.nix"),
            "callPackage ./pkgs/real {}\ncallPackage ${pkgs.x}/lib {}\n",
        )
        .unwrap();
        let r = rule(opts(
            "pkgs.nix",
            Extract::Regex(r"callPackage\s+(\S+)".into()),
        ));
        // Only the literal `./pkgs/real` is checked; the
        // genuinely interpolated `${pkgs.x}/lib` entry is
        // skipped (not a violation). Narrowed is_non_literal:
        // the captured token must carry a real `${`/`$(`/`{{`/
        // `+ ` marker — a bare `(.`/`$` no longer over-skips a
        // real literal path (v0.10 post-audit P2).
        let idx = index(&["pkgs.nix"], &["pkgs/real"]);
        let v = eval(&r, dir.path(), &idx);
        // The non-literal entry is skipped (not a violation) but
        // surfaces as an informational note (v0.11).
        let real: Vec<_> = v.iter().filter(|x| x.note_kind().is_none()).collect();
        let notes: Vec<_> = v.iter().filter(|x| x.note_kind().is_some()).collect();
        assert!(
            real.is_empty(),
            "non-literal must not be a violation, got {real:?}"
        );
        assert_eq!(notes.len(), 1, "skipped entry surfaces as one note");
        assert!(
            notes[0].message.contains("skipped non-literal"),
            "note message: {:?}",
            notes[0].message
        );
    }

    #[test]
    fn entries_are_globs_zero_match_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        let mut o = opts(
            "Cargo.toml",
            Extract::structured(Format::Toml, "$.workspace.members[*]").unwrap(),
        );
        o.entries_are_globs = true;
        let r = rule(o);
        // No crates/* on disk -> the glob matched nothing.
        let v = eval(&r, dir.path(), &index(&["Cargo.toml"], &[]));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("no path"));
    }

    #[test]
    fn orphans_flags_unreferenced_dir_without_flagging_a_member() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\"]\n",
        )
        .unwrap();
        let mut o = opts(
            "Cargo.toml",
            Extract::structured(Format::Toml, "$.workspace.members[*]").unwrap(),
        );
        o.orphans = Some(OrphansSpec {
            space: "crates/*".into(),
        });
        let r = rule(o);
        // crates/b exists on disk but isn't a member -> orphan.
        let idx = index(
            &["crates/a/Cargo.toml", "crates/b/Cargo.toml", "Cargo.toml"],
            &["crates/a", "crates/b"],
        );
        let v = eval(&r, dir.path(), &idx);
        assert!(
            v.iter().any(|x| x.message.contains("crates/b")),
            "expected crates/b flagged as orphan, got {v:?}"
        );
        assert!(
            v.iter().all(|x| !x.message.contains("crates/a")),
            "referenced member crates/a must not be flagged, got {v:?}"
        );
    }

    #[test]
    fn exclude_query_subtracts_before_checking() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"b\"]\n",
        )
        .unwrap();
        let mut o = opts(
            "Cargo.toml",
            Extract::structured(Format::Toml, "$.workspace.members[*]").unwrap(),
        );
        o.exclude_query = Some("$.workspace.exclude[*]".into());
        o.expect = Expect::Dir;
        let r = rule(o);
        // `b` is excluded, so its absence must not fail; `a` exists.
        let idx = index(&["Cargo.toml"], &["a"]);
        assert!(eval(&r, dir.path(), &idx).is_empty());
    }
}
