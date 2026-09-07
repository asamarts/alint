//! `registry_paths_resolve` — a manifest file enumerates
//! path-like entries; each must resolve to an on-disk artefact.
//! Optional reverse "orphan" check: on-disk artefacts in a
//! declared space that no entry references.
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
//!   orphans: { space: "crates/*", unreferenced: warn }
//!   level: error
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use alint_core::{
    Context, Error, Extract, ExtractSpec, Format, Level, Result, Rule, RuleSpec, Scope, Violation,
    extract_values, is_non_literal,
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

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Severity {
    #[default]
    Warn,
    Error,
    Off,
}

/// Enable the reverse-completeness check: on-disk artefacts under the `space`
/// glob that no entry references (the "new crate not wired into the workspace"
/// detector).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct OrphansSpec {
    /// Glob of on-disk artefacts that should each be referenced.
    space: String,
    /// Severity when an on-disk artefact is unreferenced: `warn` (default),
    /// `error`, or `off`.
    #[serde(default)]
    unreferenced: Severity,
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
    /// Constrain the kind each entry must resolve to on disk: `any` (default),
    /// `file`, or `dir`.
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
    /// Enable the reverse-completeness check (see `OrphansSpec`).
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
    exclude_query: Option<String>,
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
                Err(e) => {
                    let why = match e {
                        crate::io::ReadCapError::TooLarge(n) => {
                            format!("is too large to analyze ({})", crate::io::over_cap(n))
                        }
                        crate::io::ReadCapError::Io(e) => {
                            format!("could not be read: {e}")
                        }
                    };
                    violations.push(
                        Violation::new(format!("registry file {} {why}", registry_rel.display()))
                            .with_path(registry_rel.clone()),
                    );
                    continue;
                }
            };

            let (entries, skipped) = match self.extract_entries(&text) {
                Ok(v) => v,
                Err(e) => {
                    violations.push(
                        Violation::new(format!(
                            "registry file {} could not be parsed for `extract`: {e}",
                            registry_rel.display()
                        ))
                        .with_path(registry_rel.clone()),
                    );
                    continue;
                }
            };
            // Non-literal (computed/interpolated) entries are
            // intentionally skipped, not failed — surfaced as notes.
            Self::note_skipped(&registry_rel, &skipped, &mut violations);

            let excluded = self.excluded_entries(&text);
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
                    violations.push(self.violation(&registry_rel, entry, "escapes the repo root"));
                    continue;
                };
                if self.entries_are_globs {
                    let matches = Self::glob_matches(ctx, &resolved);
                    if matches.is_empty() {
                        violations.push(self.violation(
                            &registry_rel,
                            entry,
                            "matched no path on disk",
                        ));
                    } else {
                        covered.extend(matches);
                    }
                    continue;
                }
                covered.push(resolved.clone());
                if let Some(reason) = self.existence_problem(ctx, &resolved, &dir_set) {
                    violations.push(self.violation(&registry_rel, entry, &reason));
                }
            }

            // Globbed entries still need existence/kind checks on
            // each expansion (a `crates/*` match must satisfy
            // `must_contain`, etc.).
            if self.entries_are_globs {
                for p in &covered {
                    if let Some(reason) = self.existence_problem(ctx, p, &dir_set) {
                        violations.push(self.violation(
                            &registry_rel,
                            &p.display().to_string(),
                            &reason,
                        ));
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

    fn extract_entries(
        &self,
        text: &str,
    ) -> std::result::Result<(Vec<String>, Vec<String>), String> {
        let raw = extract_values(&self.extract, text)?;
        // Non-literal (computed/interpolated) entries can't be
        // statically resolved; they're surfaced as notes, not failed.
        let (skipped, kept): (Vec<String>, Vec<String>) =
            raw.into_iter().partition(|e| is_non_literal(e));
        Ok((kept, skipped))
    }

    fn excluded_entries(&self, text: &str) -> HashSet<String> {
        let Some(q) = &self.exclude_query else {
            return HashSet::new();
        };
        // exclude_query is a structured query; reuse the registry's own
        // structured format so a JSON / YAML / XML / ... registry excludes
        // against the right parse. Line / regex / whole_file registries carry
        // no structured format, so fall back to a TOML read (a misconfig
        // surfaces as an empty set, not a panic).
        let ex = match &self.extract {
            Extract::Structured(fmt, _) => Extract::Structured(*fmt, q.clone()),
            _ => Extract::Structured(Format::Toml, q.clone()),
        };
        extract_values(&ex, text)
            .map(|v| v.into_iter().collect())
            .unwrap_or_default()
    }

    /// Reverse-completeness: on-disk artefacts under `orphans.space`
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
        if orph.unreferenced == Severity::Off {
            return;
        }
        let covered_set: HashSet<&Path> = covered.iter().map(PathBuf::as_path).collect();
        let Ok(space) = Scope::from_patterns(std::slice::from_ref(&orph.space)) else {
            return;
        };
        for e in ctx.index.files() {
            if space.matches(&e.path, ctx.index) && !covered_set.contains(&*e.path) {
                out.push(
                    Violation::new(format!(
                        "{} is under `{}` but no entry in {} references it",
                        e.path.display(),
                        orph.space,
                        registry_rel.display(),
                    ))
                    .with_path(e.path.clone()),
                );
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

    /// `None` => the resolved path is fine. `Some(reason)` => a
    /// violation message fragment.
    fn existence_problem(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        dir_set: &HashSet<&Path>,
    ) -> Option<String> {
        let is_file = ctx.index.contains_file(path);
        let is_dir = dir_set.contains(path);
        match self.expect {
            Expect::File => {
                if !is_file {
                    return Some("does not resolve to a file on disk".into());
                }
            }
            Expect::Dir => {
                if !is_dir {
                    return Some("does not resolve to a directory on disk".into());
                }
            }
            Expect::Any => {
                if !is_file && !is_dir {
                    return Some("does not resolve to any path on disk".into());
                }
            }
        }
        if let Some(mc) = &self.must_contain {
            // Only meaningful when the entry is a directory.
            if is_dir && !ctx.index.contains_file(&path.join(mc)) {
                return Some(format!("resolves to a directory missing `{mc}`"));
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
    fn note_skipped(registry: &Path, skipped: &[String], out: &mut Vec<Violation>) {
        for entry in skipped {
            out.push(
                Violation::new(format!(
                    "registry {}: skipped non-literal entry {entry:?} \
                     (cannot statically resolve an interpolated/computed path)",
                    registry.display()
                ))
                .with_path(registry.to_path_buf())
                .as_note(),
            );
        }
    }

    fn violation(&self, registry: &Path, entry: &str, reason: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{}: entry {entry:?} {reason}", registry.display()));
        // One manifest can enumerate many failing entries → key on the
        // registry, the offending entry, and the machine reason so a new
        // broken entry isn't masked by an existing one.
        Violation::new(msg)
            .with_path(registry.to_path_buf())
            .with_baseline_key(format!(
                "entry\u{0}{}\u{0}{entry}\u{0}{reason}",
                crate::slash(registry)
            ))
    }
}

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
    // is read directly. `is_glob` mirrors the structured-path /
    // file_exists literal test.
    let is_glob = opts
        .source
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'));
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

    Ok(Box::new(RegistryPathsResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        source: opts.source,
        registry_scope,
        extract,
        base: Base::parse(opts.base.as_deref()),
        entries_are_globs: opts.entries_are_globs,
        expect: opts.expect,
        must_contain: opts.must_contain,
        exclude_query: opts.exclude_query,
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
        RegistryPathsResolveRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source: opts.source,
            registry_scope: None,
            extract: opts.extract.resolve().expect("test extract valid"),
            base: Base::parse(opts.base.as_deref()),
            entries_are_globs: opts.entries_are_globs,
            expect: opts.expect,
            must_contain: opts.must_contain,
            exclude_query: opts.exclude_query,
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
        };
        r.evaluate(&ctx).unwrap()
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
            v.iter().all(|x| x.is_note),
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
            Extract::Structured(Format::Toml, "$.workspace.members[*]".into()),
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
        let real: Vec<_> = v.iter().filter(|x| !x.is_note).collect();
        let notes: Vec<_> = v.iter().filter(|x| x.is_note).collect();
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
            Extract::Structured(Format::Toml, "$.workspace.members[*]".into()),
        );
        o.entries_are_globs = true;
        let r = rule(o);
        // No crates/* on disk -> the glob matched nothing.
        let v = eval(&r, dir.path(), &index(&["Cargo.toml"], &[]));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("no path"));
    }

    #[test]
    fn orphans_flags_unreferenced_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\"]\n",
        )
        .unwrap();
        let mut o = opts(
            "Cargo.toml",
            Extract::Structured(Format::Toml, "$.workspace.members[*]".into()),
        );
        o.orphans = Some(OrphansSpec {
            space: "crates/*/Cargo.toml".into(),
            unreferenced: Severity::Error,
        });
        let r = rule(o);
        // crates/b exists on disk but isn't a member -> orphan.
        let idx = index(
            &["crates/a/Cargo.toml", "crates/b/Cargo.toml", "Cargo.toml"],
            &["crates/a", "crates/b"],
        );
        let v = eval(&r, dir.path(), &idx);
        assert!(
            v.iter().any(|x| x.message.contains("crates/b/Cargo.toml")),
            "expected crates/b flagged as orphan, got {v:?}"
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
            Extract::Structured(Format::Toml, "$.workspace.members[*]".into()),
        );
        o.exclude_query = Some("$.workspace.exclude[*]".into());
        o.expect = Expect::Dir;
        let r = rule(o);
        // `b` is excluded, so its absence must not fail; `a` exists.
        let idx = index(&["Cargo.toml"], &["a"]);
        assert!(eval(&r, dir.path(), &idx).is_empty());
    }
}
