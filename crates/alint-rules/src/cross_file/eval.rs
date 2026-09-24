//! `cross_file` runtime evaluation: the per-relation checks
//! (`equals` / `subset` / `superset` / `set_equals` / `identical` / `resolves`)
//! on [`CrossFileRule`](super::CrossFileRule), plus the confined-read helpers.
//! The config schema lives in [`super::spec`]; assembly + `build` in
//! [`super`](super).

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use alint_core::template::{PathTokens, render_path};
use alint_core::{Context, Extract, Result, Violation, extract_values, is_non_literal};

use super::CrossFileRule;
use super::spec::{Relation, Targets, apply_normalize};

/// Per-target callback for `each_target`: receives the target's
/// path, its raw literal values, and the violation sink.
type TargetFn<'a> = dyn FnMut(&Path, &[String], &mut Vec<Violation>) + 'a;

impl CrossFileRule {
    /// Dispatch to the per-relation check. Called by the `Rule::evaluate` shim in
    /// [`super`](super); the rest of the impl is private to this module. Infallible
    /// (read/parse failures are pushed as violations, never `Err`), so the shim
    /// wraps the returned vec in `Ok`.
    pub(super) fn evaluate_impl(&self, ctx: &Context<'_>) -> Vec<Violation> {
        let mut out = Vec::new();
        match self.relation {
            Relation::Equals => {
                if let Some(source_values) = self.source_values(ctx, &mut out) {
                    self.check_equals(ctx, &source_values, &mut out);
                }
            }
            Relation::Subset | Relation::Superset | Relation::SetEquals => {
                if let Some(source_values) = self.source_values(ctx, &mut out) {
                    let source_set: BTreeSet<String> = source_values
                        .iter()
                        .map(|v| apply_normalize(&self.normalize, v))
                        .collect();
                    self.check_set(ctx, &source_set, &mut out);
                }
            }
            Relation::Identical => self.check_identical(ctx, &mut out),
            Relation::Resolves => self.check_resolves(ctx, &mut out),
            Relation::Registered => self.check_registered(ctx, &mut out),
        }
        out
    }

    /// `relation: registered` - every SOURCE member (a filesystem path from
    /// `source.file` / `source.files`, mapped through `register_as`) must be an
    /// element of every target's list (the structured array the target `extract`
    /// locates). Two independent conditions per member: it must EXIST (a named
    /// source only - a glob only matches paths that exist) and it must be
    /// REGISTERED. The fix (`create_and_register`) repairs whichever is unmet;
    /// this check reports them.
    fn check_registered(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) {
        let members = self.registered_members(ctx);
        // A glob source that matched NOTHING is almost always a typo; fire like the
        // set relations do (`targets matched no files`), so a broken rule is not a
        // silent green (audit A#7). Suppressed by `allow_missing_target`.
        if self.source_glob.is_some() && members.is_empty() && !self.allow_missing {
            out.push(Self::violation(
                Path::new(&self.source_file),
                "`source.files` glob matched no files",
            ));
            return;
        }
        // Existence (named source only). A missing named member is reported here;
        // creating it is the `create_and_register` fix's job (Phase 2). Keyed
        // uniquely so it never collides with a registration violation on a shared
        // path (the F4 unique-key rule, [[project_alint-autofix-located-fixer-correlation]]).
        if self.source_glob.is_none() {
            for (path, _) in &members {
                if !member_exists(ctx, path) {
                    out.push(
                        Self::violation(path, "member file does not exist").with_baseline_key(
                            format!("registered\u{0}exists\u{0}{}", crate::slash(path)),
                        ),
                    );
                }
            }
        }
        // Registration: each existing member's value must appear in every target
        // list. `normalize` is rejected on `registered` (build), so the member value
        // and target elements are compared VERBATIM -- the fixer then appends the
        // member's REAL path, not a normalized form (audit A#3/B#2).
        let existing: BTreeSet<String> = members
            .into_iter()
            .filter(|(path, _)| self.source_glob.is_some() || member_exists(ctx, path))
            .map(|(_, value)| value)
            .collect();
        if existing.is_empty() {
            return;
        }
        self.each_target(ctx, out, &mut |target, values, out| {
            let target_set: BTreeSet<&str> = values.iter().map(String::as_str).collect();
            for member in &existing {
                if target_set.contains(member.as_str()) {
                    continue;
                }
                // ONE violation PER (target, missing member). The `baseline_key`
                // carries the single member and doubles as the fix channel: the
                // fixer appends exactly this member (no re-glob, so it never diverges
                // from the check's gitignore-aware member set). A per-member key (not
                // a per-target list of all missing) keeps each member's baseline
                // fingerprint STABLE, so adding/registering one member does not
                // un-grandfather the others (audit A#4). Members are paths, so they
                // never contain the `\0` separator.
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!("{} is missing member {member:?}", crate::slash(target))
                });
                out.push(
                    Violation::new(msg)
                        .with_path(target.to_path_buf())
                        .with_baseline_key(format!(
                            "registered\u{0}member\u{0}{}\u{0}{member}",
                            crate::slash(target)
                        )),
                );
            }
        });
    }

    /// Enumerate the source members as `(path, register_value)` pairs: every match
    /// of the `source.files` glob (files AND directories - a member can be either),
    /// or the single `source.file`. `register_value` = `register_as` (default
    /// `{path}`) rendered over the slash-normalized member path, so it matches the
    /// forward-slash spelling manifests use on every platform.
    fn registered_members(&self, ctx: &Context<'_>) -> Vec<(std::path::PathBuf, String)> {
        let render_value = |p: &Path| -> String {
            let slashed = crate::slash(p);
            let tokens = PathTokens::from_path(Path::new(&slashed));
            render_path(self.register_as.as_deref().unwrap_or("{path}"), &tokens)
        };
        let Some(scope) = &self.source_glob else {
            // A single named member (`source.file`). Normalize the config-verbatim
            // path (strip `./`, resolve `..` lexically) so `member_exists` matches
            // the canonical index paths -- otherwise a `./crates/x/Cargo.toml` that
            // exists reads as missing (audit A#6). A lexical escape keeps the raw
            // path (then reads as missing, honestly).
            let raw = std::path::Path::new(&self.source_file);
            let p = crate::pathsafe::normalize_confined(raw).unwrap_or_else(|| raw.to_path_buf());
            let value = render_value(&p);
            return vec![(p, value)];
        };
        // A `source.files` glob: every matching file OR directory is a member.
        let mut members = Vec::new();
        for e in ctx.index.files() {
            if scope.matches(&e.path, ctx.index) {
                members.push((e.path.to_path_buf(), render_value(&e.path)));
            }
        }
        for e in ctx.index.dirs() {
            if scope.matches(&e.path, ctx.index) {
                members.push((e.path.to_path_buf(), render_value(&e.path)));
            }
        }
        members
    }

    /// Read + extract the source file's literal values (raw, not
    /// normalised - callers normalise as the relation needs).
    /// `None` (with a violation pushed) when the source can't be
    /// read or parsed. Only the value relations + `resolves` call
    /// this; `build` guarantees `source_extract` is `Some` for them.
    fn source_values(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) -> Option<Vec<String>> {
        let extract = self.source_extract.as_ref()?;
        if let Some(scope) = &self.source_glob {
            // Glob-union source: extract from every matching file and
            // union the values into one set (the set relations only).
            let mut all = Vec::new();
            let mut matched = 0usize;
            for entry in ctx.index.files() {
                if scope.matches(&entry.path, ctx.index) {
                    matched += 1;
                    if let Some(vals) = Self::source_values_from(ctx, &entry.path, extract, out) {
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
        Self::source_values_from(ctx, Path::new(&self.source_file), extract, out)
    }

    /// Read one source file + extract its literal values - the
    /// per-file body shared by the single-`file` and `files:`
    /// glob-union forms.
    fn source_values_from(
        ctx: &Context<'_>,
        src: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, src) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(Self::violation(
                    src,
                    &format!(
                        "source file is too large to analyze ({})",
                        crate::io::over_cap(n)
                    ),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(Self::violation(
                    src,
                    &format!("source file is unreadable: {e}"),
                ));
                return None;
            }
        };
        let values = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(src, &format!("source extract failed: {e}")));
                return None;
            }
        };
        // Whole-file content is compared verbatim — the non-literal
        // skip (for interpolated *paths*) does not apply.
        if matches!(extract, Extract::WholeFile) {
            return Some(values);
        }
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                src,
                &format!("skipped non-literal source value {v:?}"),
            ));
        }
        Some(literal)
    }

    /// `relation: equals` - the released `cross_file_value_equals`
    /// behaviour: the source must resolve to exactly one value, and
    /// every target value must equal it after normalize.
    fn check_equals(&self, ctx: &Context<'_>, source_values: &[String], out: &mut Vec<Violation>) {
        let source = match source_values {
            [one] => one.clone(),
            [] => {
                out.push(Self::violation(
                    Path::new(&self.source_file),
                    "canonical value not found (the source query matched no literal value)",
                ));
                return;
            }
            _ => {
                out.push(Self::violation(
                    Path::new(&self.source_file),
                    "source must resolve to exactly one value (the query matched several); \
                     use a set relation (subset/superset/set_equals) for multi-value sources",
                ));
                return;
            }
        };
        let source_norm = apply_normalize(&self.normalize, &source);
        self.each_target(ctx, out, &mut |target, values, out| {
            if values.is_empty() {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "no literal value to compare (the target query matched nothing)",
                    ));
                }
                return;
            }
            for value in values {
                if apply_normalize(&self.normalize, value) != source_norm {
                    out.push(self.mismatch(target, &source, value));
                }
            }
        });
    }

    /// The set relations - compare the source set `S` to each
    /// target's extracted (normalised) set `T`.
    fn check_set(
        &self,
        ctx: &Context<'_>,
        source_set: &BTreeSet<String>,
        out: &mut Vec<Violation>,
    ) {
        self.each_target(ctx, out, &mut |target, values, out| {
            let target_set: BTreeSet<String> = values
                .iter()
                .map(|v| apply_normalize(&self.normalize, v))
                .collect();
            if let Some(v) = self.set_violation(target, source_set, &target_set) {
                out.push(v);
            }
        });
    }

    fn set_violation(
        &self,
        target: &Path,
        source: &BTreeSet<String>,
        actual: &BTreeSet<String>,
    ) -> Option<Violation> {
        let missing: BTreeSet<&String> = source.difference(actual).collect();
        let extra: BTreeSet<&String> = actual.difference(source).collect();
        let reason = match self.relation {
            Relation::Subset if !missing.is_empty() => Some(format!(
                "is missing value(s) required by {}: {}",
                self.source_file,
                render(&missing)
            )),
            Relation::Superset if !extra.is_empty() => Some(format!(
                "has value(s) not present in {}: {}",
                self.source_file,
                render(&extra)
            )),
            Relation::SetEquals if !missing.is_empty() || !extra.is_empty() => Some(format!(
                "set differs from {} (missing: {}; extra: {})",
                self.source_file,
                render(&missing),
                render(&extra),
            )),
            _ => None,
        }?;
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{} {reason}", crate::slash(target)));
        Some(Violation::new(msg).with_path(target.to_path_buf()))
    }

    /// `relation: identical` - every target file's bytes (after
    /// dropping `skip_header_lines` leading lines) must equal the
    /// source file's. Binary-accurate; `normalize` does not apply.
    fn check_identical(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) {
        let Some(src) = confined_rel(ctx, Path::new(&self.source_file)) else {
            out.push(Self::violation(
                Path::new(&self.source_file),
                "source file escapes the repo root",
            ));
            return;
        };
        let src = src.as_path();
        let src_bytes = match crate::io::read_capped(&ctx.root.join(src)) {
            Ok(b) => b,
            Err(e) => {
                out.push(Self::violation(src, &read_cap_reason("source file", &e)));
                return;
            }
        };
        let src_cmp = skip_header(&src_bytes, self.skip_header_lines);

        let paths = self.identical_target_paths(ctx);
        if paths.is_empty() {
            if !self.allow_missing {
                out.push(Self::violation(src, "targets matched no files"));
            }
            return;
        }
        for target in &paths {
            let Some(target) = confined_rel(ctx, target) else {
                out.push(Self::violation(target, "target file escapes the repo root"));
                continue;
            };
            let tgt_bytes = match crate::io::read_capped(&ctx.root.join(&target)) {
                Ok(b) => b,
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    out.push(Self::violation(
                        &target,
                        &format!(
                            "target file is too large to analyze ({})",
                            crate::io::over_cap(n)
                        ),
                    ));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(_)) => {
                    if !self.allow_missing {
                        out.push(Self::violation(
                            &target,
                            "target file is missing or unreadable",
                        ));
                    }
                    continue;
                }
            };
            if skip_header(&tgt_bytes, self.skip_header_lines) != src_cmp {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "{} is not byte-identical to {}",
                        crate::slash(&target),
                        self.source_file,
                    )
                });
                out.push(Violation::new(msg).with_path(target.clone()));
            }
        }
    }

    /// The target paths for `identical` (glob expansion or the
    /// explicit list), ignoring `extract` (which is absent).
    fn identical_target_paths(&self, ctx: &Context<'_>) -> Vec<PathBuf> {
        match &self.targets {
            Some(Targets::Glob { scope, .. }) => ctx
                .index
                .files()
                .filter(|e| scope.matches(&e.path, ctx.index))
                .map(|e| e.path.to_path_buf())
                .collect(),
            Some(Targets::List(list)) => list.iter().map(|(f, _)| PathBuf::from(f)).collect(),
            None => Vec::new(),
        }
    }

    /// `relation: resolves` - each path the source extracts must
    /// exist on disk (file or dir), resolved relative to the source
    /// file's directory. The 1-level forward half of
    /// `registry_paths_resolve`.
    fn check_resolves(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) {
        let Some(paths) = self.source_values(ctx, out) else {
            return;
        };
        let src = Path::new(&self.source_file);
        let base = src.parent().map(Path::to_path_buf).unwrap_or_default();
        let dirs: HashSet<&Path> = ctx.index.dirs().map(|e| &*e.path).collect();
        for entry in &paths {
            // A confined-out (absolute / root-escaping) declared path
            // can never resolve to an in-tree file → treated as unresolved.
            let exists = crate::pathsafe::normalize_confined(&base.join(entry))
                .is_some_and(|r| ctx.index.contains_file(&r) || dirs.contains(r.as_path()));
            if !exists {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "{}: declared path {entry:?} does not resolve to a file or directory",
                        crate::slash(src),
                    )
                });
                out.push(
                    // One source can declare many unresolved paths → key on
                    // the source and the specific declared entry.
                    Violation::new(msg)
                        .with_path(src.to_path_buf())
                        .with_baseline_key(format!(
                            "resolves\u{0}{}\u{0}{entry}",
                            crate::slash(src)
                        )),
                );
            }
        }
    }

    /// Iterate the value-relation targets, calling
    /// `f(target_path, raw_literal_values, out)` for each readable
    /// target. Read/extract errors and a zero-match glob are
    /// reported here, so `f` sees only resolvable targets. `build`
    /// guarantees `targets` is `Some` and every `extract` is `Some`
    /// for the value relations that call this.
    fn each_target(&self, ctx: &Context<'_>, out: &mut Vec<Violation>, f: &mut TargetFn<'_>) {
        match &self.targets {
            Some(Targets::Glob {
                scope,
                extract: Some(extract),
            }) => {
                let mut matched = 0usize;
                for e in ctx.index.files() {
                    if !scope.matches(&e.path, ctx.index) {
                        continue;
                    }
                    matched += 1;
                    if let Some(values) = self.target_values(ctx, &e.path, extract, out) {
                        f(&e.path, &values, out);
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
                for (file, extract) in list {
                    let Some(extract) = extract else { continue };
                    // `file` is already normalized at resolution (`resolve_targets`),
                    // so the violation path matches git's canonical diff spelling and
                    // the value fixer's stored target path (audit F2).
                    let target = Path::new(file);
                    if let Some(values) = self.target_values(ctx, target, extract, out) {
                        f(target, &values, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// Read + extract one target's raw literal values. `None`
    /// (with a violation pushed, unless `allow_missing` for the
    /// missing-file case) when the target can't be read or parsed.
    fn target_values(
        &self,
        ctx: &Context<'_>,
        target: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, target) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                // A too-large target is always a violation — never
                // suppressed by `allow_missing` (it is present,
                // just unanalysable).
                out.push(Self::violation(
                    target,
                    &format!(
                        "target file is too large to analyze ({})",
                        crate::io::over_cap(n)
                    ),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(_)) => {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "target file is missing or unreadable",
                    ));
                }
                return None;
            }
        };
        let values = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(
                    target,
                    &format!("target extract failed: {e}"),
                ));
                return None;
            }
        };
        // Whole-file content is compared verbatim — the non-literal
        // skip (for interpolated *paths*) does not apply.
        if matches!(extract, Extract::WholeFile) {
            return Some(values);
        }
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                target,
                &format!("skipped non-literal target value {v:?}"),
            ));
        }
        Some(literal)
    }

    fn violation(path: &Path, reason: &str) -> Violation {
        Violation::new(format!("{}: {reason}", crate::slash(path))).with_path(path.to_path_buf())
    }

    /// An informational note (non-violation finding) - e.g. a
    /// non-literal value the rule skipped rather than compared.
    ///
    /// Carries a `reason`-discriminated `baseline_key`: a `cross_file` target can
    /// emit SEVERAL notes on one path (one per skipped non-literal value), and a
    /// keyless finding's `violation_key` collapses to `(rule_id, path)`. Once the
    /// rule is FIXABLE (`sync_from`), the fixpoint's per-violation merge requires
    /// distinct keys, so two `${A}`/`${B}` notes -- or a note beside the keyless
    /// "no literal value" violation -- would otherwise collide (the F4 tripwire
    /// panics in debug; a release build silently drops one). Keying the notes makes
    /// every finding on a path unique (the violations are one-per-path or already
    /// carry a per-value key).
    fn note(path: &Path, reason: &str) -> Violation {
        Self::violation(path, reason)
            .as_note()
            .with_baseline_key(format!("note\u{0}{}\u{0}{reason}", crate::slash(path)))
    }

    fn mismatch(&self, target: &Path, source: &str, target_value: &str) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} value {target_value:?} != {} value {source:?}",
                crate::slash(target),
                self.source_file,
            )
        });
        Violation::new(msg)
            .with_path(target.to_path_buf())
            // One query can match several target values → key on the target
            // and the specific failing value so they don't collapse to one
            // fingerprint (which would mask a genuinely new mismatch).
            .with_baseline_key(format!(
                "equals\u{0}{}\u{0}{target_value}",
                crate::slash(target)
            ))
    }
}

/// Render a sorted value set for a violation message.
/// Whether a `registered` member path is present in the walked tree (as a file OR
/// a directory - a member can be either). Used only for a NAMED source's existence
/// check; a glob source only ever yields paths the index already holds.
fn member_exists(ctx: &Context<'_>, path: &Path) -> bool {
    ctx.index.files().any(|e| e.path.as_ref() == path)
        || ctx.index.dirs().any(|e| e.path.as_ref() == path)
}

fn render(set: &BTreeSet<&String>) -> String {
    if set.is_empty() {
        // `set_equals` renders both sides; an empty one reads `none`
        // rather than a dangling `(missing: ; extra: "x")`. The
        // subset/superset paths only call this on a non-empty side.
        return "none".to_string();
    }
    set.iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Drop the first `n` newline-delimited lines of `bytes` (for
/// `identical`'s `skip_header_lines`). Fewer than `n` lines ⇒ the
/// whole file is header, so the comparison is over the empty slice.
fn skip_header(bytes: &[u8], n: usize) -> &[u8] {
    if n == 0 {
        return bytes;
    }
    let mut seen = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == n {
                return &bytes[i + 1..];
            }
        }
    }
    &[]
}

/// The user-facing reason fragment for a capped-read failure.
fn read_cap_reason(what: &str, e: &crate::io::ReadCapError) -> String {
    match e {
        crate::io::ReadCapError::TooLarge(n) => {
            format!(
                "{what} is too large to analyze ({})",
                crate::io::over_cap(*n)
            )
        }
        crate::io::ReadCapError::Io(e) => format!("{what} is unreadable: {e}"),
    }
}

/// Lexically confine `rel`, then verify it doesn't escape the root through
/// an in-repo symlink once joined - `normalize_confined` is symlink-blind,
/// so `link/secret` (with `link -> /etc`) would otherwise read out of the
/// tree. `None` on either escape; the caller reports "escapes the repo
/// root". Used by every cross-file *read* path.
fn confined_rel(ctx: &Context<'_>, rel: &Path) -> Option<PathBuf> {
    let p = crate::pathsafe::normalize_confined(rel)?;
    crate::pathsafe::resolved_within_root(&ctx.root.join(&p), ctx.root).then_some(p)
}

/// Read a tree-relative path as text (the index stores paths, not
/// contents, so the cross-file rules read the file themselves).
fn read_rel(ctx: &Context<'_>, rel: &Path) -> Result<String, crate::io::ReadCapError> {
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

#[cfg(test)]
mod tests {
    use super::super::spec::{Normalize, NormalizeSpec};
    use super::*;
    use alint_core::{FileEntry, FileIndex, Format, Level, Rule, Scope};

    fn index(files: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            files
                .iter()
                .map(|p| FileEntry {
                    path: Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        )
    }

    fn value_rule(
        source_file: &str,
        source: Extract,
        targets: Targets,
        relation: Relation,
        normalize: Normalize,
    ) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source_file.into(),
            source_glob: None,
            source_extract: Some(source),
            targets: Some(targets),
            relation,
            normalize: NormalizeSpec::One(normalize).into_list(),
            allow_missing: false,
            skip_header_lines: 0,
            register_as: None,
            fixer: None,
        }
    }

    fn eval(r: &CrossFileRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
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

    // ─── equals (the migrated cross_file_value_equals path) ──────

    #[test]
    fn equals_glob_targets_pass_and_fail_on_version_lockstep() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.4.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/a")).unwrap();
        std::fs::create_dir_all(root.join("crates/b")).unwrap();
        std::fs::write(
            root.join("crates/a/Cargo.toml"),
            "[package]\nversion = \"1.4.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/b/Cargo.toml"),
            "[package]\nversion = \"1.3.0\"\n",
        )
        .unwrap();
        let idx = index(&["Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"]);
        let r = value_rule(
            "Cargo.toml",
            Extract::Structured(Format::Toml, "$.workspace.package.version".into()),
            Targets::Glob {
                scope: Scope::from_patterns(&["crates/*/Cargo.toml".to_string()]).unwrap(),
                extract: Some(Extract::Structured(
                    Format::Toml,
                    "$.package.version".into(),
                )),
            },
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/Cargo.toml"));
        assert!(v[0].message.contains("1.3.0"));
    }

    #[test]
    fn equals_target_query_matching_nothing_fires_unless_allow_missing() {
        // A target file that resolves no value (the query names an
        // absent key) is a drift by default, but `allow_missing_target`
        // makes it a tolerated absence.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("source.toml"), "v = \"1.0\"\n").unwrap();
        std::fs::write(root.join("target.toml"), "other = \"x\"\n").unwrap();
        let idx = index(&["source.toml", "target.toml"]);
        let make = || {
            value_rule(
                "source.toml",
                Extract::Structured(Format::Toml, "$.v".into()),
                Targets::Glob {
                    scope: Scope::from_patterns(&["target.toml".to_string()]).unwrap(),
                    extract: Some(Extract::Structured(Format::Toml, "$.missing".into())),
                },
                Relation::Equals,
                Normalize::None,
            )
        };
        // Default (`allow_missing_target: false`): the empty target
        // query fires.
        let strict = make();
        let v = eval(&strict, root, &idx);
        assert_eq!(v.len(), 1, "missing target value fires: {v:?}");
        assert!(v[0].message.contains("matched nothing"), "{}", v[0].message);
        // `allow_missing_target: true`: the absence is tolerated.
        let mut lax = make();
        lax.allow_missing = true;
        assert!(
            eval(&lax, root, &idx).is_empty(),
            "allow_missing silences the empty target"
        );
    }

    #[test]
    fn whole_file_equals_compares_verbatim_despite_interpolation_markers() {
        // The body carries `${...}`/`{{...}}` — markers the non-literal
        // skip would normally drop. whole_file must compare verbatim.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let body = "Copyright ${YEAR} Acme\nAll rights {{ reserved }}.\n";
        std::fs::write(root.join("LICENSE"), body).unwrap();
        std::fs::write(root.join("LICENSE-MIT"), body).unwrap();
        std::fs::write(root.join("LICENSE-APACHE"), "different text\n").unwrap();
        let idx = index(&["LICENSE", "LICENSE-MIT", "LICENSE-APACHE"]);
        let r = value_rule(
            "LICENSE",
            Extract::WholeFile,
            Targets::Glob {
                scope: Scope::from_patterns(&["LICENSE-*".to_string()]).unwrap(),
                extract: Some(Extract::WholeFile),
            },
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only LICENSE-APACHE drifts: {v:?}");
        assert!(v[0].message.contains("LICENSE-APACHE"));
    }

    #[test]
    fn equals_multi_value_source_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("m.json"), "{\"v\":[\"1\",\"2\"]}").unwrap();
        let idx = index(&["m.json"]);
        let r = value_rule(
            "m.json",
            Extract::Structured(Format::Json, "$.v[*]".into()),
            Targets::List(vec![(
                "m.json".into(),
                Some(Extract::Structured(Format::Json, "$.v[0]".into())),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exactly one value"));
    }

    #[test]
    fn equals_semver_major_normalize_allows_band() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("global.json"),
            "{\"sdk\":{\"version\":\"8.0.402\"}}",
        )
        .unwrap();
        std::fs::write(root.join("Directory.Build.props"), "8.0.100\n").unwrap();
        let idx = index(&["global.json", "Directory.Build.props"]);
        let r = value_rule(
            "global.json",
            Extract::Structured(Format::Json, "$.sdk.version".into()),
            Targets::List(vec![(
                "Directory.Build.props".into(),
                Some(Extract::Lines(alint_core::LinesOpts::default())),
            )]),
            Relation::Equals,
            Normalize::SemverMajor,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn equals_semver_minor_reconciles_dev_and_patch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // protobuf: `4.36-dev` (version.json) vs `4.36.0` (the .bzl).
        std::fs::write(root.join("version.json"), "{\"v\":\"4.36-dev\"}").unwrap();
        std::fs::write(root.join("protobuf_version.bzl"), "4.36.0\n").unwrap();
        let idx = index(&["version.json", "protobuf_version.bzl"]);
        let r = value_rule(
            "version.json",
            Extract::Structured(Format::Json, "$.v".into()),
            Targets::List(vec![(
                "protobuf_version.bzl".into(),
                Some(Extract::Lines(alint_core::LinesOpts::default())),
            )]),
            Relation::Equals,
            Normalize::SemverMinor,
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "{:?}",
            eval(&r, root, &idx)
        );
    }

    // ─── set relations ──────────────────────────────────────────

    fn set_rule(source: Extract, targets: Targets, relation: Relation) -> CrossFileRule {
        value_rule("src.json", source, targets, relation, Normalize::None)
    }

    fn write_sets(root: &Path, source: &str, target: &str) {
        std::fs::write(root.join("src.json"), source).unwrap();
        std::fs::write(root.join("tgt.json"), target).unwrap();
    }

    fn set_targets() -> Targets {
        Targets::List(vec![(
            "tgt.json".into(),
            Some(Extract::Structured(Format::Json, "$.have[*]".into())),
        )])
    }

    #[test]
    fn subset_fires_when_a_source_value_is_missing_from_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a} -> b is missing.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"c\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("missing"));
        assert!(v[0].message.contains("\"b\""));
        // `c` is extra in the target but `subset` does not care.
        assert!(!v[0].message.contains("\"c\""));
    }

    #[test]
    fn subset_silent_when_source_is_contained() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(
            root,
            "{\"need\":[\"a\",\"b\"]}",
            "{\"have\":[\"a\",\"b\",\"c\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn superset_fires_on_a_target_value_not_in_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a, z} -> z is not covered by the source.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"z\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Superset,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("not present"));
        assert!(v[0].message.contains("\"z\""));
    }

    #[test]
    fn set_equals_reports_both_missing_and_extra() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a, z} -> missing b, extra z.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"z\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::SetEquals,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("missing"));
        assert!(v[0].message.contains("\"b\""));
        assert!(v[0].message.contains("extra"));
        assert!(v[0].message.contains("\"z\""));
    }

    #[test]
    fn set_equals_silent_on_matching_sets_regardless_of_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(root, "{\"need\":[\"b\",\"a\"]}", "{\"have\":[\"a\",\"b\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::SetEquals,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn glob_union_source_set_equals_unions_across_the_glob() {
        // The vim hlgroups shape: the union of `*hl-X*` across every
        // doc file must equal the `default link X` set in one file.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("doc")).unwrap();
        std::fs::write(root.join("doc/a.txt"), "see *hl-Comment* and *hl-String*\n").unwrap();
        std::fs::write(root.join("doc/b.txt"), "also *hl-Number*\n").unwrap();
        std::fs::write(
            root.join("highlight.c"),
            "default link Comment\ndefault link String\ndefault link Number\n",
        )
        .unwrap();
        let idx = index(&["doc/a.txt", "doc/b.txt", "highlight.c"]);
        let r = CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "doc/*.txt".into(),
            source_glob: Some(Scope::from_patterns(&["doc/*.txt".to_string()]).unwrap()),
            source_extract: Some(Extract::Regex(r"\*hl-(\w+)\*".into())),
            targets: Some(Targets::List(vec![(
                "highlight.c".into(),
                Some(Extract::Regex(r"default link (\w+)".into())),
            )])),
            relation: Relation::SetEquals,
            normalize: vec![],
            allow_missing: false,
            skip_header_lines: 0,
            register_as: None,
            fixer: None,
        };
        // union {Comment, String, Number} == highlight.c set → silent.
        assert!(eval(&r, root, &idx).is_empty(), "matched union should pass");
        // A doc declares an extra group the code lacks → set_equals fires.
        std::fs::write(root.join("doc/b.txt"), "also *hl-Number* and *hl-Extra*\n").unwrap();
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("Extra"), "{}", v[0].message);
    }

    #[test]
    fn glob_union_source_matching_no_files_fires() {
        // C2: a `source.files` glob that matches nothing is a
        // misconfiguration — it fires, not a silent `subset` pass.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("highlight.c"), "default link Comment\n").unwrap();
        let idx = index(&["highlight.c"]);
        let r = CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "doc/*.txt".into(),
            source_glob: Some(Scope::from_patterns(&["doc/*.txt".to_string()]).unwrap()),
            source_extract: Some(Extract::Regex(r"\*hl-(\w+)\*".into())),
            targets: Some(Targets::List(vec![(
                "highlight.c".into(),
                Some(Extract::Regex(r"default link (\w+)".into())),
            )])),
            relation: Relation::Subset,
            normalize: vec![],
            allow_missing: false,
            skip_header_lines: 0,
            register_as: None,
            fixer: None,
        };
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("matched no files"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn subset_singleton_is_a_membership_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {needle}; member -> silent.
        write_sets(
            root,
            "{\"need\":[\"needle\"]}",
            "{\"have\":[\"hay\",\"needle\",\"straw\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Structured(Format::Json, "$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    fn identical_rule(targets: Targets, skip_header_lines: usize) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "README.md".into(),
            source_glob: None,
            source_extract: None,
            targets: Some(targets),
            relation: Relation::Identical,
            normalize: Vec::new(),
            allow_missing: false,
            skip_header_lines,
            register_as: None,
            fixer: None,
        }
    }

    #[test]
    fn identical_fires_on_byte_difference_silent_on_match() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), "# Project\n\nHello.\n").unwrap();
        std::fs::create_dir_all(root.join("crates/a")).unwrap();
        std::fs::create_dir_all(root.join("crates/b")).unwrap();
        // a mirrors exactly; b drifts by a byte.
        std::fs::write(root.join("crates/a/README.md"), "# Project\n\nHello.\n").unwrap();
        std::fs::write(root.join("crates/b/README.md"), "# Project\n\nHello!\n").unwrap();
        let idx = index(&["README.md", "crates/a/README.md", "crates/b/README.md"]);
        let r = identical_rule(
            Targets::Glob {
                scope: Scope::from_patterns(&["crates/*/README.md".to_string()]).unwrap(),
                extract: None,
            },
            0,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/README.md"));
        assert!(v[0].message.contains("not byte-identical"));
    }

    #[test]
    fn identical_root_escape_target_fires_without_reading() {
        // Security regression (v0.12 path-confinement): an absolute
        // targets[].file must produce an "escapes the repo root"
        // violation, never read an out-of-tree file.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), "# Project\n").unwrap();
        let idx = index(&["README.md"]);
        let r = identical_rule(Targets::List(vec![("/etc/hostname".into(), None)]), 0);
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("escapes the repo root"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn identical_skip_header_lines_ignores_a_differing_header() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Two leading lines differ; the body is identical.
        std::fs::write(root.join("README.md"), "// 2024 Acme\n// gen\nBODY\n").unwrap();
        std::fs::write(root.join("mirror.md"), "// 2025 Acme\n// gen2\nBODY\n").unwrap();
        let idx = index(&["README.md", "mirror.md"]);
        let mk = |skip| identical_rule(Targets::List(vec![("mirror.md".into(), None)]), skip);
        // skip 2 -> bodies match.
        assert!(eval(&mk(2), root, &idx).is_empty());
        // skip 0 -> headers differ -> fires.
        assert_eq!(eval(&mk(0), root, &idx).len(), 1);
    }

    // ─── resolves ───────────────────────────────────────────────

    fn resolves_rule(source_file: &str, extract: Extract) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source_file.into(),
            source_glob: None,
            source_extract: Some(extract),
            targets: None,
            relation: Relation::Resolves,
            normalize: Vec::new(),
            allow_missing: false,
            skip_header_lines: 0,
            register_as: None,
            fixer: None,
        }
    }

    fn index_with_dirs(files: &[&str], dirs: &[&str]) -> FileIndex {
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

    #[test]
    fn resolves_fires_on_a_declared_path_that_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\", \"crates/gone\"]\n",
        )
        .unwrap();
        // crates/a exists (a dir); crates/gone does not.
        let idx = index_with_dirs(&["Cargo.toml"], &["crates/a"]);
        let r = resolves_rule(
            "Cargo.toml",
            Extract::Structured(Format::Toml, "$.workspace.members[*]".into()),
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("crates/gone"));
        assert!(v[0].message.contains("does not resolve"));
    }

    #[test]
    fn resolves_silent_when_every_path_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("manifest.txt"), "src/a.rs\nsrc/b.rs\n").unwrap();
        let idx = index(&["manifest.txt", "src/a.rs", "src/b.rs"]);
        let r = resolves_rule(
            "manifest.txt",
            Extract::Lines(alint_core::LinesOpts::default()),
        );
        assert!(eval(&r, root, &idx).is_empty());
    }
}
