//! `cross_file` runtime evaluation: the per-relation checks
//! (`equals` / `subset` / `superset` / `set_equals` / `identical` / `resolves`)
//! on [`CrossFileRule`](super::CrossFileRule), plus the confined-read helpers.
//! The config schema lives in [`super::spec`]; assembly + `build` in
//! [`super`](super).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use alint_core::template::{PathTokens, render_path};
use alint_core::{
    AnalysisFailureKind, Context, Extract, ExtractedValues, RelatedLocation, Result, Violation,
    extract_values_bounded, is_non_literal, unreadable_file_violation,
};

use super::CrossFileRule;
use super::spec::{Relation, Targets, apply_normalize};

/// Per-target callback for `each_target`: receives the target path, extractor,
/// raw literal values, and the violation sink.
type TargetFn<'a> = dyn FnMut(&Path, &Extract, &[String], &mut Vec<Violation>) + 'a;

impl CrossFileRule {
    /// Dispatch to the per-relation check. Called by the `Rule::evaluate` shim in
    /// [`super`](super); the rest of the impl is private to this module. Infallible:
    /// continued-scan read/parse errors use a structural finding carrier, so the
    /// shim wraps the returned vec in `Ok`.
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
        // the `create_and_register` fix CREATES it (from `content`/`content_from`) --
        // one of the two independent postconditions. Keyed uniquely so it never
        // collides with a registration finding on a shared path (the F4 unique-key
        // rule, [[project_alint-autofix-located-fixer-correlation]]).
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
        // Registration: each EXISTING member's value must appear in every target
        // list. Existence is a PREREQUISITE for registration -- registering a member
        // whose file is missing would write a PHANTOM entry that breaks the consumer
        // (a `workspace.members` path with no crate makes cargo fail), so a MISSING
        // NAMED member is registered only AFTER its create-half fix makes it exist:
        // the fixpoint re-walks and this pass then fires the registration. If the
        // create is skipped (no `content`, an unreadable `content_from`, a root
        // escape, a `--changed` skew), the member stays missing and is NEVER
        // registered -- no phantom (round-2 audit HIGH: the uncoordinated
        // two-postcondition model appended a phantom when the create was withheld;
        // this also drops a root-escaping named source, which never resolves in the
        // index). A glob member always exists (a glob matches only existing paths).
        // `normalize` is rejected on `registered` (build), so member and target
        // elements compare VERBATIM (audit A#3/B#2).
        let mut member_sources: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for (path, value) in members
            .into_iter()
            .filter(|(path, _)| self.source_glob.is_some() || member_exists(ctx, path))
        {
            member_sources.entry(value).or_default().push(path);
        }
        if member_sources.is_empty() {
            return;
        }
        self.each_target(ctx, out, &mut |target, target_extract, values, out| {
            let target_set: BTreeSet<&str> = values.iter().map(String::as_str).collect();
            for (member, source_paths) in &member_sources {
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
                let mut finding = Violation::new(msg)
                    .with_path(target.to_path_buf())
                    .with_baseline_key(format!(
                        "registered\u{0}member\u{0}{}\u{0}{member}",
                        crate::slash(target)
                    ));
                finding.set_related_locations(
                    source_paths
                        .iter()
                        .map(|path| RelatedLocation::new(path, "source member")),
                );
                // Membership absence is still a verdict derived from the target
                // extraction. Mark a secret-bearing target even though no existing
                // target value is rendered in the message.
                self.redact_extracted(
                    &mut finding,
                    "target",
                    target,
                    target_extract,
                    "registered",
                    &[],
                );
                out.push(finding);
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
    fn source_values_from(
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

    /// `relation: equals` - the released `cross_file_value_equals`
    /// behaviour: the source must resolve to exactly one value, and
    /// every target value must equal it after normalize.
    fn check_equals(&self, ctx: &Context<'_>, source_values: &[String], out: &mut Vec<Violation>) {
        let source_extract = self
            .source_extract
            .as_ref()
            .expect("equals has a source extract");
        let source = match source_values {
            [one] => one.clone(),
            [] => {
                let mut finding = Self::violation(
                    Path::new(&self.source_file),
                    "canonical value not found (the source query matched no literal value)",
                );
                self.redact_extracted(
                    &mut finding,
                    "source",
                    Path::new(&self.source_file),
                    source_extract,
                    "missing",
                    &[],
                );
                out.push(finding);
                return;
            }
            _ => {
                let mut finding = Self::violation(
                    Path::new(&self.source_file),
                    "source must resolve to exactly one value (the query matched several); \
                     use a set relation (subset/superset/set_equals) for multi-value sources",
                );
                self.redact_extracted(
                    &mut finding,
                    "source",
                    Path::new(&self.source_file),
                    source_extract,
                    "cardinality",
                    &[],
                );
                out.push(finding);
                return;
            }
        };
        let source_norm = apply_normalize(&self.normalize, &source);
        self.each_target(ctx, out, &mut |target, target_extract, values, out| {
            if values.is_empty() {
                if !self.allow_missing {
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
                }
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

    /// The set relations - compare the source set `S` to each
    /// target's extracted (normalised) set `T`.
    fn check_set(
        &self,
        ctx: &Context<'_>,
        source_set: &BTreeSet<String>,
        out: &mut Vec<Violation>,
    ) {
        let source_extract = self
            .source_extract
            .as_ref()
            .expect("set relation has a source extract");
        self.each_target(ctx, out, &mut |target, target_extract, values, out| {
            let target_set: BTreeSet<String> = values
                .iter()
                .map(|v| apply_normalize(&self.normalize, v))
                .collect();
            if let Some(v) = self.set_violation(
                target,
                ctx,
                source_extract,
                target_extract,
                source_set,
                &target_set,
            ) {
                out.push(v);
            }
        });
    }

    fn set_violation(
        &self,
        target: &Path,
        ctx: &Context<'_>,
        source_extract: &Extract,
        target_extract: &Extract,
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
        let mut finding = Violation::new(msg).with_path(target.to_path_buf());
        self.add_source_locations(ctx, &mut finding, "comparison source");
        let source_values: Vec<&str> = source.iter().map(String::as_str).collect();
        let target_values: Vec<&str> = actual.iter().map(String::as_str).collect();
        self.redact_extracted(
            &mut finding,
            "source",
            Path::new(&self.source_file),
            source_extract,
            "set",
            &source_values,
        );
        self.redact_extracted(
            &mut finding,
            "target",
            target,
            target_extract,
            "set",
            &target_values,
        );
        Some(finding)
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
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(alint_core::too_large_file_violation(src, n));
                return;
            }
            Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push(Self::violation(
                    src,
                    &format!("source file is missing: {e}"),
                ));
                return;
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(unreadable_file_violation(src, &e));
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
                    out.push(alint_core::too_large_file_violation(&target, n));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !self.allow_missing {
                        out.push(Self::violation(&target, "target file is missing"));
                    }
                    continue;
                }
                Err(crate::io::ReadCapError::Io(e)) => {
                    out.push(unreadable_file_violation(&target, &e));
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
                out.push(
                    Violation::new(msg)
                        .with_path(target.clone())
                        .with_related_location(RelatedLocation::new(src, "canonical source")),
                );
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
            Some(Targets::List(list)) => list
                .iter()
                .map(|target| PathBuf::from(&target.file))
                .collect(),
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
                let mut finding = Violation::new(msg)
                    .with_path(src.to_path_buf())
                    .with_baseline_key(format!("resolves\u{0}{}\u{0}{entry}", crate::slash(src)));
                if let Some(resolved) = crate::pathsafe::normalize_confined(&base.join(entry)) {
                    finding.set_related_locations(std::iter::once(RelatedLocation::new(
                        resolved,
                        "declared target",
                    )));
                }
                if let Some(extract) = self.source_extract.as_ref() {
                    self.redact_extracted(
                        &mut finding,
                        "source",
                        src,
                        extract,
                        "resolves",
                        &[entry],
                    );
                }
                out.push(finding);
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

    pub(super) fn add_source_locations(
        &self,
        ctx: &Context<'_>,
        finding: &mut Violation,
        label: &'static str,
    ) {
        if let Some(scope) = &self.source_glob {
            finding.set_related_locations(
                ctx.index
                    .files()
                    .filter(|entry| scope.matches(&entry.path, ctx.index))
                    .map(|entry| RelatedLocation::new(entry.path.as_ref(), label)),
            );
        } else {
            finding.set_related_locations(std::iter::once(RelatedLocation::new(
                Path::new(&self.source_file),
                label,
            )));
        }
    }

    pub(super) fn violation(path: &Path, reason: &str) -> Violation {
        Violation::new(format!("{}: {reason}", crate::slash(path))).with_path(path.to_path_buf())
    }

    fn analysis_failure(path: &Path, kind: AnalysisFailureKind, reason: &str) -> Violation {
        Self::violation(path, reason).as_analysis_failure(kind)
    }

    /// An informational note (non-violation finding), such as a non-literal
    /// value the rule skipped rather than compared. The engine partitions notes
    /// before baseline and fixpoint identity processing, so they need no key.
    pub(super) fn note(path: &Path, reason: &str) -> Violation {
        Self::violation(path, reason).as_note()
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

    pub(super) fn redact_extracted(
        &self,
        finding: &mut Violation,
        role: &str,
        path: &Path,
        extract: &Extract,
        category: &str,
        evidence: &[&str],
    ) {
        if !extract.redacts_by_default() {
            return;
        }
        let locator = format!("{role}\0{}\0{extract:?}\0{category}", crate::slash(path));
        finding.redact_evidence(&self.id, &locator, evidence.iter().copied());
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
    use super::super::spec::{Normalize, NormalizeSpec, Target};
    use super::*;
    use alint_core::{FileEntry, FileIndex, Format, Level, Rule, Scope};

    fn target(file: impl Into<String>, extract: Option<Extract>) -> Target {
        Target {
            file: file.into(),
            extract,
            matches: None,
        }
    }

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
            analysis_time: None,
            process_environment: None,
            trace: None,
        };
        r.evaluate(&ctx).unwrap()
    }

    #[test]
    fn sensitive_equals_redacts_source_and_target_values_selectively() {
        const SECRET: &str = "TOKEN_VALUE_DO_NOT_REPORT";

        let source_dir = tempfile::tempdir().unwrap();
        std::fs::write(source_dir.path().join(".env"), format!("TOKEN={SECRET}\n")).unwrap();
        std::fs::write(
            source_dir.path().join("target.json"),
            r#"{"v":"visible-target"}"#,
        )
        .unwrap();
        let source_rule = value_rule(
            ".env",
            Extract::structured(Format::Dotenv, "$.TOKEN").unwrap(),
            Targets::List(vec![target(
                "target.json",
                Some(Extract::structured(Format::Json, "$.v").unwrap()),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let source_findings = eval(
            &source_rule,
            source_dir.path(),
            &index(&[".env", "target.json"]),
        );
        assert_eq!(source_findings.len(), 1);
        assert!(source_findings[0].is_sensitive());
        assert!(!source_findings[0].message.contains(SECRET));
        assert!(source_findings[0].message.contains("visible-target"));

        let target_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            target_dir.path().join("source.json"),
            r#"{"v":"visible-source"}"#,
        )
        .unwrap();
        std::fs::write(
            target_dir.path().join("target.properties"),
            format!("TOKEN={SECRET}\n"),
        )
        .unwrap();
        let target_rule = value_rule(
            "source.json",
            Extract::structured(Format::Json, "$.v").unwrap(),
            Targets::List(vec![target(
                "target.properties",
                Some(Extract::structured(Format::Properties, "$.TOKEN").unwrap()),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let target_findings = eval(
            &target_rule,
            target_dir.path(),
            &index(&["source.json", "target.properties"]),
        );
        assert_eq!(target_findings.len(), 1);
        assert!(target_findings[0].is_sensitive());
        assert!(!target_findings[0].message.contains(SECRET));

        assert!(target_findings[0].message.contains("visible-source"));
    }
    #[test]
    fn mixed_structured_matches_report_types_and_compare_strings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("source.json"), r#"["expected",7]"#).unwrap();
        std::fs::write(root.join("target.json"), r#"["actual",false]"#).unwrap();
        let idx = index(&["source.json", "target.json"]);
        let all_values = || Extract::structured(Format::Json, "$[*]").unwrap();

        let rule = value_rule(
            "source.json",
            all_values(),
            Targets::List(vec![target("target.json", Some(all_values()))]),
            Relation::Equals,
            Normalize::None,
        );
        let findings = eval(&rule, root, &idx);

        assert_eq!(findings.len(), 3, "{findings:?}");
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains("number")),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains("boolean")),
            "{findings:?}"
        );
        assert!(
            findings.iter().any(|finding| {
                finding.message.contains("expected") && finding.message.contains("actual")
            }),
            "{findings:?}"
        );
    }

    #[test]
    fn sensitive_set_and_skipped_note_do_not_publish_values() {
        const SECRET: &str = "TOKEN_VALUE_DO_NOT_REPORT";
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".env"),
            format!("VALUES={SECRET}\nSKIP=${{{SECRET}}}\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("target.properties"),
            format!("VALUES=other\nSKIP=${{{SECRET}}}\n"),
        )
        .unwrap();
        let idx = index(&[".env", "target.properties"]);

        let set_rule = value_rule(
            ".env",
            Extract::structured(Format::Dotenv, "$.VALUES").unwrap(),
            Targets::List(vec![target(
                "target.properties",
                Some(Extract::structured(Format::Properties, "$.VALUES").unwrap()),
            )]),
            Relation::SetEquals,
            Normalize::None,
        );
        let set_findings = eval(&set_rule, root, &idx);
        assert_eq!(set_findings.len(), 1);
        assert!(set_findings[0].is_sensitive());
        assert!(!set_findings[0].message.contains(SECRET));

        let skipped_rule = value_rule(
            ".env",
            Extract::structured(Format::Dotenv, "$.SKIP").unwrap(),
            Targets::List(vec![target(
                "target.properties",
                Some(Extract::structured(Format::Properties, "$.SKIP").unwrap()),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let skipped = eval(&skipped_rule, root, &idx);
        assert!(!skipped.is_empty());
        let skipped_notes: Vec<&Violation> = skipped
            .iter()
            .filter(|finding| finding.message.contains("skipped"))
            .collect();
        assert!(!skipped_notes.is_empty(), "{skipped:?}");
        assert!(skipped_notes.iter().all(|finding| finding.is_sensitive()));
        assert!(
            skipped_notes
                .iter()
                .all(|finding| !finding.message.contains(SECRET))
        );
    }

    #[test]
    fn sensitive_equals_marks_empty_query_verdicts_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".env"), "TOKEN=source-secret\n").unwrap();
        std::fs::write(root.join("target.properties"), "TOKEN=target-secret\n").unwrap();
        let idx = index(&[".env", "target.properties"]);

        let source_missing = value_rule(
            ".env",
            Extract::structured(Format::Dotenv, "$.MISSING").unwrap(),
            Targets::List(vec![target(
                "target.properties",
                Some(Extract::structured(Format::Properties, "$.TOKEN").unwrap()),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let findings = eval(&source_missing, root, &idx);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].is_sensitive(), "{findings:?}");

        let target_missing = value_rule(
            ".env",
            Extract::structured(Format::Dotenv, "$.TOKEN").unwrap(),
            Targets::List(vec![target(
                "target.properties",
                Some(Extract::structured(Format::Properties, "$.MISSING").unwrap()),
            )]),
            Relation::Equals,
            Normalize::None,
        );
        let findings = eval(&target_missing, root, &idx);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].is_sensitive(), "{findings:?}");
    }

    #[test]
    fn sensitive_source_and_target_parse_failures_are_value_free() {
        for (format, file, malformed) in [
            (
                Format::Dotenv,
                ".env",
                "TOKEN=\"TOKEN_VALUE_DO_NOT_REPORT\n",
            ),
            (
                Format::Properties,
                "bad.properties",
                "TOKEN=\\uZZZZ_TOKEN_VALUE_DO_NOT_REPORT\n",
            ),
        ] {
            let source_dir = tempfile::tempdir().unwrap();
            std::fs::write(source_dir.path().join(file), malformed).unwrap();
            std::fs::write(source_dir.path().join("target.json"), r#"{"v":"x"}"#).unwrap();
            let source_rule = value_rule(
                file,
                Extract::structured(format, "$.TOKEN").unwrap(),
                Targets::List(vec![target(
                    "target.json",
                    Some(Extract::structured(Format::Json, "$.v").unwrap()),
                )]),
                Relation::Equals,
                Normalize::None,
            );
            let source_findings = eval(
                &source_rule,
                source_dir.path(),
                &index(&[file, "target.json"]),
            );
            assert_eq!(source_findings.len(), 1);
            assert!(source_findings[0].is_sensitive());
            assert!(
                !source_findings[0]
                    .message
                    .contains("TOKEN_VALUE_DO_NOT_REPORT")
            );

            let target_dir = tempfile::tempdir().unwrap();
            std::fs::write(target_dir.path().join("source.json"), r#"{"v":"x"}"#).unwrap();
            std::fs::write(target_dir.path().join(file), malformed).unwrap();
            let target_rule = value_rule(
                "source.json",
                Extract::structured(Format::Json, "$.v").unwrap(),
                Targets::List(vec![target(
                    file,
                    Some(Extract::structured(format, "$.TOKEN").unwrap()),
                )]),
                Relation::Equals,
                Normalize::None,
            );
            let target_findings = eval(
                &target_rule,
                target_dir.path(),
                &index(&["source.json", file]),
            );
            assert_eq!(target_findings.len(), 1);
            assert!(target_findings[0].is_sensitive());
            assert!(
                !target_findings[0]
                    .message
                    .contains("TOKEN_VALUE_DO_NOT_REPORT")
            );
        }
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
            Extract::structured(Format::Toml, "$.workspace.package.version").unwrap(),
            Targets::Glob {
                scope: Scope::from_patterns(&["crates/*/Cargo.toml".to_string()]).unwrap(),
                extract: Some(Extract::structured(Format::Toml, "$.package.version").unwrap()),
                matches: None,
            },
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/Cargo.toml"));
        assert!(v[0].message.contains("1.3.0"));
        assert_eq!(v[0].path.as_deref(), Some(Path::new("crates/b/Cargo.toml")));
        assert_eq!(v[0].related_locations.len(), 1);
        assert_eq!(v[0].related_locations[0].path, Path::new("Cargo.toml"));
        assert_eq!(v[0].related_locations[0].label, "canonical source");
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
                Extract::structured(Format::Toml, "$.v").unwrap(),
                Targets::Glob {
                    scope: Scope::from_patterns(&["target.toml".to_string()]).unwrap(),
                    extract: Some(Extract::structured(Format::Toml, "$.missing").unwrap()),
                    matches: None,
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
                matches: None,
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
            Extract::structured(Format::Json, "$.v[*]").unwrap(),
            Targets::List(vec![target(
                "m.json",
                Some(Extract::structured(Format::Json, "$.v[0]").unwrap()),
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
            Extract::structured(Format::Json, "$.sdk.version").unwrap(),
            Targets::List(vec![target(
                "Directory.Build.props",
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
            Extract::structured(Format::Json, "$.v").unwrap(),
            Targets::List(vec![target(
                "protobuf_version.bzl",
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
        Targets::List(vec![target(
            "tgt.json",
            Some(Extract::structured(Format::Json, "$.have[*]").unwrap()),
        )])
    }

    #[test]
    fn optional_targets_filtered_to_empty_are_absent_for_set_relations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(
            root,
            "{\"need\":[\"22.14.0\"]}",
            "{\"have\":[\">=20 <23\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let exact = regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").unwrap();

        for relation in [Relation::Subset, Relation::SetEquals] {
            for targets in [
                Targets::List(vec![Target {
                    file: "tgt.json".into(),
                    extract: Some(Extract::structured(Format::Json, "$.have[*]").unwrap()),
                    matches: Some(exact.clone()),
                }]),
                Targets::Glob {
                    scope: Scope::from_patterns(&["tgt.json".to_string()]).unwrap(),
                    extract: Some(Extract::structured(Format::Json, "$.have[*]").unwrap()),
                    matches: Some(exact.clone()),
                },
            ] {
                let mut rule = set_rule(
                    Extract::structured(Format::Json, "$.need[*]").unwrap(),
                    targets,
                    relation,
                );
                rule.allow_missing = true;
                assert!(
                    eval(&rule, root, &idx).is_empty(),
                    "{relation:?} must treat a fully filtered optional target as absent"
                );
            }
        }
    }

    #[test]
    fn subset_fires_when_a_source_value_is_missing_from_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a} -> b is missing.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"c\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::structured(Format::Json, "$.need[*]").unwrap(),
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
            Extract::structured(Format::Json, "$.need[*]").unwrap(),
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
            Extract::structured(Format::Json, "$.need[*]").unwrap(),
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
            Extract::structured(Format::Json, "$.need[*]").unwrap(),
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
            Extract::structured(Format::Json, "$.need[*]").unwrap(),
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
            targets: Some(Targets::List(vec![target(
                "highlight.c",
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
            targets: Some(Targets::List(vec![target(
                "highlight.c",
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
            Extract::structured(Format::Json, "$.need[*]").unwrap(),
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
                matches: None,
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
        let r = identical_rule(Targets::List(vec![target("/etc/hostname", None)]), 0);
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
        let mk = |skip| identical_rule(Targets::List(vec![target("mirror.md", None)]), skip);
        // skip 2 -> bodies match.
        assert!(eval(&mk(2), root, &idx).is_empty());
        // skip 0 -> headers differ -> fires.
        assert_eq!(eval(&mk(0), root, &idx).len(), 1);
    }

    #[test]
    fn registered_relates_each_missing_value_only_to_its_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"unrelated\"]\n",
        )
        .unwrap();
        let member_paths: Vec<String> = (0..22)
            .map(|index| format!("crates/c{index:02}/Cargo.toml"))
            .collect();
        let mut indexed: Vec<&str> = member_paths.iter().map(String::as_str).collect();
        indexed.push("Cargo.toml");
        let idx = index(&indexed);
        let rule = CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: "crates/*/Cargo.toml".into(),
            source_glob: Some(Scope::from_patterns(&["crates/*/Cargo.toml".to_string()]).unwrap()),
            source_extract: None,
            targets: Some(Targets::List(vec![target(
                "Cargo.toml",
                Some(Extract::structured(Format::Toml, "$.workspace.members[*]").unwrap()),
            )])),
            relation: Relation::Registered,
            normalize: Vec::new(),
            allow_missing: false,
            skip_header_lines: 0,
            register_as: Some("{dir}".into()),
            fixer: None,
        };

        let findings = eval(&rule, root, &idx);
        assert_eq!(findings.len(), 22);
        for finding in findings {
            assert_eq!(finding.related_locations.len(), 1);
            assert_eq!(finding.related_locations_omitted, 0);
            let source = &finding.related_locations[0].path;
            let registered = source.parent().unwrap().to_string_lossy();
            assert!(finding.message.contains(registered.as_ref()));
        }
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
            Extract::structured(Format::Toml, "$.workspace.members[*]").unwrap(),
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
