//! The `set_value` / `remove_value` located structured fixers (Phase 2).
//!
//! Both re-run the host rule's `JSONPath` over the parsed document, map each
//! concrete match location to a byte range via a span-resolving parser
//! (alint-core `structured_fix`), and emit a [`FixEdit::ReplaceRange`] carrying a
//! [`EditVerifier::Structured`] re-parse obligation. The engine batches,
//! overlap-skips, splices, then re-parses and re-runs the query, demoting any
//! edit whose result does not verify to a Suggestion instead of writing bad
//! bytes (auto-fix.md 5.3/5.4/5.8).

use std::collections::HashMap;
use std::path::Path;

use alint_core::structured_fix::{self, PathSeg};
use alint_core::{
    Applicability, CollectedEdit, EditVerifier, ExpectedValue, FixContext, FixEdit, FixOutcome,
    Fixer, Format, Result, Violation,
};
use serde_json::Value;
use serde_json_path::{JsonPath, NormalizedPath, PathElement};

/// The operation a [`StructuredFixer`] performs.
#[derive(Debug)]
enum StructuredOp {
    /// `set_value`: overwrite the single scalar node the query selects with the
    /// host rule's `equals` value.
    Set(Value),
    /// `remove_value`: delete every node the query selects, plus its
    /// format-specific separator.
    Remove,
    /// `replace` on `*_path_matches`: apply the `search` -> `replacement` regex to
    /// the STRING value each node selects, rewriting it to satisfy the rule's
    /// `matches` pattern (kept for the post-edit re-verify).
    Replace {
        search: regex::Regex,
        replacement: String,
        matches: String,
    },
}

/// A located fixer for the structured-query kinds. Owns a clone of the rule's
/// compiled `JSONPath` (for the `query_located` re-query) and its source string
/// (for the post-edit verifier). `set_value` hosts on `*_path_equals`,
/// `remove_value` on `*_path_absent`.
#[derive(Debug)]
pub struct StructuredFixer {
    format: Format,
    path_expr: JsonPath,
    path_src: String,
    op: StructuredOp,
    applicability: Applicability,
}

impl StructuredFixer {
    /// `set_value` (host `*_path_equals`): overwrite the node at `path_src` with
    /// the rule's `equals` value. Defaults to `Safe` (only a scalar replacing an
    /// existing scalar is ever emitted -- see `collect_edits`).
    #[must_use]
    pub fn set(
        format: Format,
        path_expr: JsonPath,
        path_src: String,
        value: Value,
        applicability: Applicability,
    ) -> Self {
        Self {
            format,
            path_expr,
            path_src,
            op: StructuredOp::Set(value),
            applicability,
        }
    }

    /// `remove_value` (host `*_path_absent`): delete every node the query
    /// selects. `Unsafe` by default (a deletion is not behavior-preserving).
    #[must_use]
    pub fn remove(
        format: Format,
        path_expr: JsonPath,
        path_src: String,
        applicability: Applicability,
    ) -> Self {
        Self {
            format,
            path_expr,
            path_src,
            op: StructuredOp::Remove,
            applicability,
        }
    }

    /// `replace` (host `*_path_matches`): rewrite the STRING value at `path_src`
    /// by applying `search` -> `replacement`, so it satisfies the host rule's
    /// `matches` pattern (the re-verify target). `Unsafe` by default (a regex
    /// rewrite is not behavior-preserving).
    #[must_use]
    pub fn replace(
        format: Format,
        path_expr: JsonPath,
        path_src: String,
        search: regex::Regex,
        replacement: String,
        matches: String,
        applicability: Applicability,
    ) -> Self {
        Self {
            format,
            path_expr,
            path_src,
            op: StructuredOp::Replace {
                search,
                replacement,
                matches,
            },
            applicability,
        }
    }

    /// Build the `Structured` re-parse verifier for this fixer's op.
    fn verifier(&self, expect: ExpectedValue) -> EditVerifier {
        EditVerifier::Structured {
            format: self.format,
            query: self.path_src.clone(),
            expect,
        }
    }

    /// The STATIC (config-only, no-document, no-I/O) preconditions for a
    /// `set_value` to be able to apply, all pure functions of the rule's `equals`
    /// value and this format:
    /// - it is a SCALAR (an object/array is never overwritten at tier);
    /// - it can MATCH the parsed leaf -- a non-string `equals` against a
    ///   string-typed format (XML / dotenv / properties / INI parse every leaf as
    ///   a string) is unsatisfiable, so `equals: 8080` there can never match; and
    /// - it can be SERIALIZED to this format -- dotenv, for one, declines a value
    ///   carrying a control char it cannot escape.
    ///
    /// Consulted by BOTH `can_fix` (so `check` does not advertise an auto-fix)
    /// AND `collect_edits` (so `fix` emits nothing rather than a suggestion that
    /// re-verify would demote anyway), so the two CANNOT disagree about a
    /// config-only decline. Document-dependent preconditions (a single scalar
    /// match, a resolvable span) stay fix-time-only.
    fn set_value_is_statically_applicable(&self, want: &Value) -> bool {
        if !is_scalar(want) {
            return false;
        }
        // A non-string `equals` can never match a string-leaf format.
        let string_leaf_type_mismatch =
            structured_fix::format_leaves_are_strings(self.format) && !want.is_string();
        if string_leaf_type_mismatch {
            return false;
        }
        // The value must be representable in this format's syntax.
        structured_fix::serialize_scalar(self.format, want).is_some()
    }
}

/// Convert a resolved `JSONPath` location into the crate-neutral [`PathSeg`]
/// list the span resolver navigates.
fn to_segs(loc: &NormalizedPath<'_>) -> Vec<PathSeg> {
    loc.iter()
        .map(|el| match el {
            PathElement::Name(n) => PathSeg::Key((*n).to_string()),
            PathElement::Index(i) => PathSeg::Index(*i),
        })
        .collect()
}

/// Render located segments as a VALID single-node `JSONPath` for the post-edit
/// re-query verify. `serde_json_path`'s `NormalizedPath` `Display` emits the RAW
/// key (`['it's']`), which is INVALID `JSONPath` for a key containing `'`, `\`, or a
/// control character -- `JsonPath::parse` then fails and the safe fix is wrongly
/// demoted to a Suggestion that never applies (audit 2026-09-20). Escape each name
/// per RFC 9535's single-quoted string rules so the query round-trips.
fn segs_to_query(segs: &[PathSeg]) -> String {
    use std::fmt::Write as _;
    let mut q = String::from("$");
    for seg in segs {
        match seg {
            PathSeg::Key(k) => {
                q.push_str("['");
                for c in k.chars() {
                    match c {
                        '\'' => q.push_str("\\'"),
                        '\\' => q.push_str("\\\\"),
                        '\u{08}' => q.push_str("\\b"),
                        '\t' => q.push_str("\\t"),
                        '\n' => q.push_str("\\n"),
                        '\u{0C}' => q.push_str("\\f"),
                        '\r' => q.push_str("\\r"),
                        c if (c as u32) < 0x20 => {
                            let _ = write!(q, "\\u{:04x}", c as u32);
                        }
                        c => q.push(c),
                    }
                }
                q.push_str("']");
            }
            PathSeg::Index(i) => {
                let _ = write!(q, "[{i}]");
            }
        }
    }
    q
}

/// Whether `value` is a scalar (the only shape `set_value` overwrites at tier).
fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
    )
}

impl Fixer for StructuredFixer {
    fn describe(&self) -> String {
        match &self.op {
            StructuredOp::Set(_) => format!("set the value at `{}`", self.path_src),
            StructuredOp::Remove => format!("remove the node at `{}`", self.path_src),
            StructuredOp::Replace { .. } => {
                format!("rewrite the value at `{}` to match", self.path_src)
            }
        }
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn can_fix(&self, _violation: &Violation) -> bool {
        // A PURE (no-I/O) convertibility test so `check`'s "auto-fixable" claim
        // matches what `fix` does (mark_fixability / fixable-accuracy contract).
        // Only STATICALLY-knowable never-appliable cases report unfixable here;
        // document-dependent declines (a non-scalar TARGET node, a repeated-block
        // / array-parent removal) are fix-time concerns `check` cannot predict.
        match &self.op {
            // `set_value` is advertised fixable exactly when its value clears the
            // config-only static preconditions (scalar + can-match + can-serialize);
            // `collect_edits` gates on the SAME predicate, so check and fix agree.
            StructuredOp::Set(want) => self.set_value_is_statically_applicable(want),
            // `remove_value` is advertised fixable only for a format that HAS a
            // removal resolver at all. JSON/YAML defer object-member removal
            // (comma surgery), so their removal declines on EVERY document -- a
            // statically-knowable never-appliable case, so `check` must not
            // promise it (else it advertises a fix `fix` can never apply, every
            // time). For a format that DOES resolve removals, whether a given
            // matched node can be deleted stays document-dependent (the resolver
            // declines a repeated-block / array-parent node, or the XML document
            // root -- deleting it would empty the file); those surface at fix time
            // as `skipped`/`suggested`, never a corruption. So `check` reports
            // "fixable" and a later `fix` may `skip` it: the tolerated over-promise
            // in the safe direction, exercised by `remove_value_xml_root_is_declined`.
            StructuredOp::Remove => structured_fix::format_supports_removal(self.format),
            // `replace` is advertised fixable for a SPAN format (all but TOML): it
            // rewrites the value at a resolved span. TOML is document-rewrite with
            // no value span, so its `replace` declines on EVERY document (a
            // statically-knowable never-appliable case -> `check` must not promise
            // it, mirroring the `remove_value` / `format_supports_removal` gate).
            // Whether a span-format node is a STRING the search rewrites stays
            // document-dependent -> a fix-time skip (tolerated, safe direction).
            StructuredOp::Replace { .. } => !structured_fix::uses_document_rewrite(self.format),
        }
    }

    fn collects_located_edits(&self) -> bool {
        true
    }

    // A cohesive dispatcher over three edit regimes (whole-document rewrite for
    // TOML; per-op document removal for JSON; span splice for the rest) x two ops
    // (set/remove), all emitting `CollectedEdit`s from the shared `located` query
    // result. Splitting it would fragment that dispatch and fight `located`'s
    // borrow lifetime for marginal benefit (cf. `Engine::fix`).
    #[allow(clippy::too_many_lines)]
    fn collect_edits(
        &self,
        violations: &[Violation],
        file: &Path,
        bytes: &[u8],
        _root: &Path,
    ) -> Vec<CollectedEdit> {
        // Parse with the SAME lossy decode the host rule uses, so the query sees
        // the same tree. A parse failure means the rule already reported a
        // parse-error violation; emit no edits.
        let text = String::from_utf8_lossy(bytes);
        let Ok(value) = self.format.parse(&text) else {
            return Vec::new();
        };
        let located = self.path_expr.query_located(&value);

        // A round-trip-CST format (TOML / toml_edit) rewrites the WHOLE document
        // rather than splicing a span: toml_edit despans on parse, and owns the
        // formatting / comment / removal surgery, round-tripping byte-identically.
        // The rewrite is reduced to its MINIMAL changed span (`minimal_replace`),
        // NOT emitted as a `0..len` whole-file edit: independent whole-doc rules on
        // one file then produce DISJOINT edits that co-apply in ONE fixpoint pass
        // (no false exit-2 at >10 rules, and `--diff` shows every change). Two rules
        // touching the SAME span still overlap -> the engine serializes them (a
        // genuinely conflicting config still oscillates to a loud exit 2).
        if structured_fix::uses_document_rewrite(self.format) {
            let one = |doc: Vec<u8>, expect: ExpectedValue| {
                let (range, content) = structured_fix::minimal_replace(bytes, &doc);
                if range.is_empty() && content.is_empty() {
                    return Vec::new(); // reserialize was byte-identical: no-op
                }
                vec![CollectedEdit {
                    edit: FixEdit::ReplaceRange {
                        path: file.to_path_buf(),
                        range,
                        content,
                    },
                    applicability: self.applicability,
                    verify: self.verifier(expect),
                    isolation_group: None,
                }]
            };
            return match &self.op {
                StructuredOp::Set(want) => {
                    if located.len() != 1 {
                        return Vec::new();
                    }
                    let Some(node) = located.iter().next() else {
                        return Vec::new();
                    };
                    // Same gates as the span path: single scalar match + the shared
                    // config-only predicate (so check and fix agree). TOML is TYPED,
                    // so a numeric/bool `equals` clears the predicate.
                    if !is_scalar(node.node()) || !self.set_value_is_statically_applicable(want) {
                        return Vec::new();
                    }
                    let segs = to_segs(node.location());
                    match structured_fix::document_set(self.format, bytes, &segs, want) {
                        Some(doc) => one(doc, ExpectedValue::Scalar(want.clone())),
                        None => Vec::new(),
                    }
                }
                StructuredOp::Remove => {
                    // Remove every matched key in ONE rewrite; the shared `Absent`
                    // verifier re-checks the whole file, so a partial removal (some
                    // path unresolved) demotes -- all-or-nothing, as with the span
                    // path. `document_remove` returns None when NOTHING was removed.
                    let paths: Vec<Vec<PathSeg>> =
                        located.iter().map(|n| to_segs(n.location())).collect();
                    match structured_fix::document_remove(self.format, bytes, &paths) {
                        Some(doc) => one(doc, ExpectedValue::Absent),
                        None => Vec::new(),
                    }
                }
                // `replace` on a document-rewrite format (TOML) is DEFERRED: it has
                // no value span to splice, and a document-level string rewrite is a
                // separate increment. Declines (skipped); the other 7 formats do
                // `toml_path_matches`-style replace via the span path below.
                StructuredOp::Replace { .. } => Vec::new(),
            };
        }

        // JSON `remove_value` is a WHOLE-DOCUMENT CST rewrite (the `jsonc-parser`
        // editable CST owns the comma / comment surgery, so a deletion never
        // over-deletes a sibling -- the trap a hand-rolled span splice falls into,
        // invisible to the `Absent` verifier). JSON `set_value` stays a span splice,
        // so this is a per-OP document path, handled before the span path below.
        // Reduced to its MINIMAL changed span (`minimal_replace`) like the TOML
        // whole-doc branch, so multiple removals on one file co-apply in one pass;
        // the `Absent` verifier re-checks the whole post-edit file, so a partial
        // removal (an unresolved path) demotes.
        if matches!(self.op, StructuredOp::Remove)
            && structured_fix::removal_uses_document(self.format)
        {
            let paths: Vec<Vec<PathSeg>> = located.iter().map(|n| to_segs(n.location())).collect();
            let Some(doc) = structured_fix::document_remove(self.format, bytes, &paths) else {
                return Vec::new();
            };
            let (range, content) = structured_fix::minimal_replace(bytes, &doc);
            if range.is_empty() && content.is_empty() {
                return Vec::new();
            }
            return vec![CollectedEdit {
                edit: FixEdit::ReplaceRange {
                    path: file.to_path_buf(),
                    range,
                    content,
                },
                applicability: self.applicability,
                verify: self.verifier(ExpectedValue::Absent),
                isolation_group: None,
            }];
        }

        match &self.op {
            StructuredOp::Set(want) => {
                // The Safe case ONLY: exactly one match, the existing node AND the
                // wanted value both scalar, and the span resolvable. Everything
                // else declines (no edit; the violation honestly stands) rather
                // than guess -- object/array values, zero-match insertion, and
                // nested creation are deferred to a later Suggestion path
                // (auto-fix.md 5.4/5.8), and an ambiguous CST mapping resolves to
                // `None`.
                if located.len() != 1 {
                    return Vec::new();
                }
                let Some(node) = located.iter().next() else {
                    return Vec::new();
                };
                // The existing node must be a scalar to overwrite (document-dependent).
                if !is_scalar(node.node()) {
                    return Vec::new();
                }
                // Gate on the SAME config-only predicate `can_fix` uses, so a value
                // check advertised as fixable is emitted here, and one check declined
                // (non-matching type, or un-serializable) emits nothing -- never a
                // suggestion re-verify would only demote. Keeps check and fix honest.
                if !self.set_value_is_statically_applicable(want) {
                    return Vec::new();
                }
                let Some(content) = structured_fix::serialize_scalar(self.format, want) else {
                    return Vec::new(); // unreachable: the predicate proved serialization succeeds
                };
                let segs = to_segs(node.location());
                let Some(range) = structured_fix::resolve_value_span(self.format, bytes, &segs)
                else {
                    return Vec::new();
                };
                vec![CollectedEdit {
                    edit: FixEdit::ReplaceRange {
                        path: file.to_path_buf(),
                        range,
                        content,
                    },
                    applicability: self.applicability,
                    verify: self.verifier(ExpectedValue::Scalar(want.clone())),
                    isolation_group: None,
                }]
            }
            StructuredOp::Remove => {
                // One removal edit per matched node. The batch's disjointness is
                // enforced by the engine's overlap-skip; the `Absent` verifier
                // re-runs the query on the whole post-edit file, so a PARTIAL
                // removal (some node's span unresolved) fails verification and is
                // demoted rather than written -- the removal is all-or-nothing.
                located
                    .iter()
                    .filter_map(|node| {
                        let segs = to_segs(node.location());
                        let range =
                            structured_fix::resolve_removal_span(self.format, bytes, &segs)?;
                        Some(CollectedEdit {
                            edit: FixEdit::ReplaceRange {
                                path: file.to_path_buf(),
                                range,
                                content: Vec::new(),
                            },
                            applicability: self.applicability,
                            verify: self.verifier(ExpectedValue::Absent),
                            isolation_group: None,
                        })
                    })
                    .collect()
            }
            StructuredOp::Replace {
                search,
                replacement,
                matches,
            } => {
                // One rewrite edit per matched STRING node whose value the search
                // actually changes. Decode the value (the check-side string), apply
                // `search` -> `replacement`, re-serialize as a format token, and
                // splice it over the value span (reusing the `set_value` machinery).
                // A non-string node, an un-serializable result, an unresolvable span,
                // or a search that matched nothing emits NO edit -- the `Matches`
                // verifier would demote a no-op anyway, and this keeps it honest.
                // Rewrite ONLY the VIOLATING nodes: skip any that already satisfy
                // the rule's `matches:` (a wildcard `path:` selects compliant AND
                // non-compliant nodes; rewriting a compliant one would clobber it --
                // e.g. `^`->`v` on an already-`v`-prefixed value). Compiled once here.
                let compliant = regex::Regex::new(matches).ok();
                // W4 (`fix --baseline`): rewrite only the nodes the engine handed us
                // as LIVE violations. The baseline filter (Engine::fix_run) drops the
                // grandfathered nodes upstream, but this fixer re-derives every
                // failing node from the file, so without this it would rewrite
                // accepted, reviewed debt (an under-suppression trust bug: `check
                // --baseline` reports only the new node, but `fix` would mutate the
                // grandfathered ones). Correlate each candidate back to a handed
                // violation by the SAME key the host recorded it under
                // (`matches_baseline_key`, the single shared definition), COUNT-aware
                // so N identical values with M live get exactly M edits -- the
                // check-side budget semantics. Under a plain `fix` (no baseline) the
                // engine passes ALL failing nodes, so every key has budget and every
                // node is fixed: this is transparent to the non-baseline path.
                let mut live: HashMap<&str, usize> = HashMap::new();
                for v in violations {
                    if let Some(k) = v.baseline_key.as_deref() {
                        *live.entry(k).or_default() += 1;
                    }
                }
                // Correlation is active only when the caller handed us KEYED
                // violations (the fix pass, `check --baseline`, and
                // `attach_proposed_edits` all pass the host's keyed violations).
                // The LSP `code_action` synthesizes a positional violation with NO
                // baseline_key to offer a "fix every occurrence" action; there the
                // budget is empty, so fall back to the legacy file-scoped behavior
                // (rewrite every non-compliant node). This never re-opens the
                // grandfather bug: a baseline run always carries keys, so `live` is
                // non-empty and the filter engages.
                let correlate = !live.is_empty();
                located
                    .iter()
                    .filter_map(|node| {
                        let current = node.node().as_str()?;
                        if compliant.as_ref().is_some_and(|re| re.is_match(current)) {
                            return None; // already matches `matches:` -- leave it
                        }
                        let rewritten = search.replace_all(current, replacement.as_str());
                        if rewritten == current {
                            return None;
                        }
                        // Grandfathered (key absent from the live set, or its budget
                        // already spent by an earlier identical node) -> leave it.
                        if correlate {
                            let key = crate::structured_path::matches_baseline_key(
                                &self.path_src,
                                matches,
                                node.node(),
                            );
                            match live.get_mut(key.as_str()) {
                                Some(n) if *n > 0 => *n -= 1,
                                _ => return None,
                            }
                        }
                        let content = structured_fix::serialize_scalar(
                            self.format,
                            &Value::String(rewritten.into_owned()),
                        )?;
                        let segs = to_segs(node.location());
                        let range = structured_fix::resolve_value_span(self.format, bytes, &segs)?;
                        Some(CollectedEdit {
                            edit: FixEdit::ReplaceRange {
                                path: file.to_path_buf(),
                                range,
                                content,
                            },
                            applicability: self.applicability,
                            // Verify THIS node (its own path), not the whole
                            // `matches:` query. A `fix --baseline` deliberately leaves
                            // grandfathered siblings non-matching, so an
                            // all-selected-nodes-match verify (over `self.path_src`)
                            // would demote this legitimate new-node edit to a
                            // Suggestion. Per-node scoping also isolates one bad
                            // rewrite to its own edit under a plain `fix`. Build the
                            // query with `segs_to_query` (RFC 9535 escaping), NOT
                            // `NormalizedPath::to_string()` -- the latter emits raw
                            // keys and fails to parse for keys with `'`/`\`/controls,
                            // which would silently demote the fix. The splice changes
                            // only the scalar value, so the node stays at this path.
                            verify: EditVerifier::Structured {
                                format: self.format,
                                query: segs_to_query(&segs),
                                expect: ExpectedValue::Matches(matches.clone()),
                            },
                            isolation_group: None,
                        })
                    })
                    .collect()
            }
        }
    }

    // A located fixer is routed through `collect_edits`; `apply` is never reached.
    fn apply(&self, _violation: &Violation, _ctx: &FixContext<'_>) -> Result<FixOutcome> {
        Ok(FixOutcome::Skipped(
            "set_value/remove_value are applied via the located-edit path".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn jp(src: &str) -> JsonPath {
        JsonPath::parse(src).unwrap()
    }

    fn edit_of(e: &CollectedEdit) -> (usize, usize, String, Applicability) {
        match &e.edit {
            FixEdit::ReplaceRange { range, content, .. } => (
                range.start,
                range.end,
                String::from_utf8_lossy(content).into_owned(),
                e.applicability,
            ),
            other => panic!("expected ReplaceRange, got {other:?}"),
        }
    }

    #[test]
    fn set_value_emits_a_scalar_replace_over_the_value_span() {
        let src = "region = \"us-east-1\"\n";
        let f = StructuredFixer::set(
            Format::Hcl,
            jp("$.region"),
            "$.region".into(),
            json!("us-west-2"),
            Applicability::Safe,
        );
        let edits = f.collect_edits(&[], Path::new("a.hcl"), src.as_bytes(), Path::new("/r"));
        assert_eq!(edits.len(), 1);
        let (start, end, content, app) = edit_of(&edits[0]);
        assert_eq!(&src[start..end], "\"us-east-1\"");
        assert_eq!(content, "\"us-west-2\"");
        assert_eq!(app, Applicability::Safe);
        assert!(matches!(
            edits[0].verify,
            EditVerifier::Structured {
                expect: ExpectedValue::Scalar(_),
                ..
            }
        ));
        // Splicing yields the corrected document.
        let mut out = src.to_string();
        out.replace_range(start..end, &content);
        assert_eq!(out, "region = \"us-west-2\"\n");
    }

    #[test]
    fn set_value_declines_a_non_scalar_existing_node() {
        // `$.tags` selects a list -> not a scalar replace -> no edit.
        let src = "tags = [\"a\", \"b\"]\n";
        let f = StructuredFixer::set(
            Format::Hcl,
            jp("$.tags"),
            "$.tags".into(),
            json!("x"),
            Applicability::Safe,
        );
        assert!(
            f.collect_edits(&[], Path::new("a.hcl"), src.as_bytes(), Path::new("/r"))
                .is_empty()
        );
    }

    #[test]
    fn set_value_declines_zero_and_multi_match() {
        let f = StructuredFixer::set(
            Format::Hcl,
            jp("$.missing"),
            "$.missing".into(),
            json!("x"),
            Applicability::Safe,
        );
        assert!(
            f.collect_edits(
                &[],
                Path::new("a.hcl"),
                b"region = \"x\"\n",
                Path::new("/r")
            )
            .is_empty()
        );
    }

    #[test]
    fn remove_value_deletes_the_node_line() {
        let src = "keep = 1\ndrop = 2\n";
        let f = StructuredFixer::remove(
            Format::Hcl,
            jp("$.drop"),
            "$.drop".into(),
            Applicability::Unsafe,
        );
        let edits = f.collect_edits(&[], Path::new("a.hcl"), src.as_bytes(), Path::new("/r"));
        assert_eq!(edits.len(), 1);
        let (start, end, content, app) = edit_of(&edits[0]);
        assert_eq!(&src[start..end], "drop = 2\n");
        assert_eq!(content, "");
        assert_eq!(app, Applicability::Unsafe);
        assert!(matches!(
            edits[0].verify,
            EditVerifier::Structured {
                expect: ExpectedValue::Absent,
                ..
            }
        ));
    }

    #[test]
    fn remove_value_emits_one_edit_per_matched_node() {
        // A query matching several nodes yields one removal edit each (the engine
        // splices them back-to-front; the shared `Absent` verifier makes the
        // batch all-or-nothing). Splicing all removals clears the block body.
        let src = "locals {\n  a = 1\n  b = 2\n}\n";
        let f = StructuredFixer::remove(
            Format::Hcl,
            jp("$.locals.*"),
            "$.locals.*".into(),
            Applicability::Unsafe,
        );
        let mut edits = f.collect_edits(&[], Path::new("a.tf"), src.as_bytes(), Path::new("/r"));
        assert_eq!(edits.len(), 2, "one removal edit per matched node");
        // Apply back-to-front (highest start first) so earlier offsets stay valid.
        edits.sort_by_key(|e| match &e.edit {
            FixEdit::ReplaceRange { range, .. } => std::cmp::Reverse(range.start),
            _ => unreachable!(),
        });
        let mut out = src.to_string();
        for e in &edits {
            let (start, end, content, _) = edit_of(e);
            assert_eq!(content, "");
            out.replace_range(start..end, "");
        }
        assert_eq!(out, "locals {\n}\n", "both members removed, block kept");
    }

    #[test]
    fn is_located_and_declines_a_parse_failure() {
        let f = StructuredFixer::set(
            Format::Hcl,
            jp("$.a"),
            "$.a".into(),
            json!("x"),
            Applicability::Safe,
        );
        assert!(f.collects_located_edits());
        // A non-parsing document emits nothing (the rule reported the parse error).
        assert!(
            f.collect_edits(&[], Path::new("a.hcl"), b"= = =\n", Path::new("/r"))
                .is_empty()
        );
    }

    #[test]
    fn set_value_preserves_crlf_and_neighbors() {
        // Byte-locality: only the value span changes -- CRLF endings and the
        // sibling line survive verbatim (the splice must not normalize EOLs).
        let src = "region = \"a\"\r\nzone   = \"b\"\r\n";
        let f = StructuredFixer::set(
            Format::Hcl,
            jp("$.region"),
            "$.region".into(),
            json!("c"),
            Applicability::Safe,
        );
        let edits = f.collect_edits(&[], Path::new("a.hcl"), src.as_bytes(), Path::new("/r"));
        let (start, end, content, _) = edit_of(&edits[0]);
        let mut out = src.to_string();
        out.replace_range(start..end, &content);
        assert_eq!(out, "region = \"c\"\r\nzone   = \"b\"\r\n");
    }

    #[test]
    fn set_value_serializes_typed_values_faithfully() {
        // A numeric `equals` becomes an UNQUOTED HCL number, not a `"8080"`
        // string; a bool becomes a bare `true` (type fidelity, auto-fix.md 5.4).
        for (want, expected) in [(json!(8080), "8080"), (json!(true), "true")] {
            let f = StructuredFixer::set(
                Format::Hcl,
                jp("$.port"),
                "$.port".into(),
                want,
                Applicability::Safe,
            );
            let edits = f.collect_edits(
                &[],
                Path::new("a.hcl"),
                b"port = \"old\"\n",
                Path::new("/r"),
            );
            assert_eq!(edit_of(&edits[0]).2, expected);
        }
    }

    #[test]
    fn can_fix_reflects_static_applicability() {
        let v = Violation::new("x");
        // set_value on a STRING-typed format (XML: every leaf is a string) with a
        // NON-string `equals` is unsatisfiable -> report unfixable, so `check`
        // does not promise an auto-fix that always declines.
        assert!(
            !StructuredFixer::set(
                Format::Xml,
                jp("$.a"),
                "$.a".into(),
                json!(8080),
                Applicability::Safe
            )
            .can_fix(&v)
        );
        // A STRING equals on XML is fixable; a numeric equals on a TYPED format
        // (HCL) is fixable.
        assert!(
            StructuredFixer::set(
                Format::Xml,
                jp("$.a"),
                "$.a".into(),
                json!("8080"),
                Applicability::Safe
            )
            .can_fix(&v)
        );
        assert!(
            StructuredFixer::set(
                Format::Hcl,
                jp("$.a"),
                "$.a".into(),
                json!(8080),
                Applicability::Safe
            )
            .can_fix(&v)
        );
        // A non-scalar `equals` is never applied -> unfixable on any format.
        assert!(
            !StructuredFixer::set(
                Format::Hcl,
                jp("$.a"),
                "$.a".into(),
                json!({"k": 1}),
                Applicability::Safe
            )
            .can_fix(&v)
        );
        // remove_value on a format WITH a removal resolver is reported fixable
        // (its declines are document-dependent -- e.g. the XML root).
        assert!(
            StructuredFixer::remove(Format::Xml, jp("$.a"), "$.a".into(), Applicability::Unsafe)
                .can_fix(&v)
        );
        // JSON removal is now supported (via the jsonc-parser editable CST), so
        // `check` advertises it (its declines are document-dependent, like XML).
        assert!(
            StructuredFixer::remove(Format::Json, jp("$.a"), "$.a".into(), Applicability::Unsafe)
                .can_fix(&v),
            "JSON remove_value is supported (CST) -> check advertises it"
        );
        // YAML removal is now supported too (a conservative single-line block-entry
        // line scan), so `check` advertises it (declines -- block scalar, flow member,
        // multi-line value -- are document-dependent). ALL formats now support removal.
        assert!(
            StructuredFixer::remove(Format::Yaml, jp("$.a"), "$.a".into(), Applicability::Unsafe)
                .can_fix(&v),
            "YAML remove_value is supported (line scan) -> check advertises it"
        );
    }

    #[test]
    fn set_value_un_serializable_value_declines_check_and_fix_together() {
        // Regression (dotenv audit): a string `equals` carrying an un-escapable
        // control char (vertical tab -- dotenv escapes only \n \r \t) is
        // STATICALLY un-serializable, so `check` must NOT advertise it fixable and
        // `fix` must emit nothing -- the two agree via the shared static predicate.
        let v = Violation::new("x");
        let vtab = json!("a\u{0b}b");
        let denv = StructuredFixer::set(
            Format::Dotenv,
            jp("$.MSG"),
            "$.MSG".into(),
            vtab.clone(),
            Applicability::Safe,
        );
        assert!(
            !denv.can_fix(&v),
            "check must not advertise an un-serializable dotenv value"
        );
        assert!(
            denv.collect_edits(&[], Path::new("a.env"), b"MSG=old\n", Path::new("/r"))
                .is_empty(),
            "fix must emit nothing for the same value (agree with can_fix)"
        );
        // The SAME value on HCL IS fixable: HCL escapes control chars (`\u000B`)
        // rather than declining, so check AND fix both accept it.
        let hcl = StructuredFixer::set(
            Format::Hcl,
            jp("$.msg"),
            "$.msg".into(),
            vtab,
            Applicability::Safe,
        );
        assert!(hcl.can_fix(&v), "HCL escapes controls, so it stays fixable");
        assert_eq!(
            hcl.collect_edits(&[], Path::new("a.hcl"), b"msg = \"old\"\n", Path::new("/r"))
                .len(),
            1,
            "HCL emits an edit for the same value"
        );
        // dotenv CAN escape \n, so a newline value stays fixable (the boundary).
        assert!(
            StructuredFixer::set(
                Format::Dotenv,
                jp("$.MSG"),
                "$.MSG".into(),
                json!("a\nb"),
                Applicability::Safe
            )
            .can_fix(&v),
            "dotenv escapes \\n, so a newline value stays fixable"
        );
        // INI also declines an un-escapable control char (INI has no escaping), and
        // the SAME shared predicate keeps can_fix and collect_edits in agreement.
        let ini = StructuredFixer::set(
            Format::Ini,
            jp("$['s']['k']"),
            "$['s']['k']".into(),
            json!("a\u{0b}b"),
            Applicability::Safe,
        );
        assert!(
            !ini.can_fix(&v),
            "INI can_fix must decline a raw control char"
        );
        assert!(
            ini.collect_edits(&[], Path::new("a.ini"), b"[s]\nk = old\n", Path::new("/r"))
                .is_empty(),
            "INI collect_edits must emit nothing for the same value (agree with can_fix)"
        );
    }

    #[test]
    fn xml_set_value_emits_a_scalar_replace_over_the_text() {
        let src = "<config><version>1.0</version></config>";
        let f = StructuredFixer::set(
            Format::Xml,
            jp("$.config.version"),
            "$.config.version".into(),
            json!("2.0"),
            Applicability::Safe,
        );
        let edits = f.collect_edits(&[], Path::new("a.xml"), src.as_bytes(), Path::new("/r"));
        assert_eq!(edits.len(), 1);
        let (start, end, content, _) = edit_of(&edits[0]);
        assert_eq!(&src[start..end], "1.0");
        assert_eq!(content, "2.0");
    }

    #[test]
    fn xml_set_value_declines_a_non_scalar_element() {
        // `$.config` has a child element -> object -> not a scalar -> no edit.
        let src = "<config><a>1</a></config>";
        let f = StructuredFixer::set(
            Format::Xml,
            jp("$.config"),
            "$.config".into(),
            json!("x"),
            Applicability::Safe,
        );
        assert!(
            f.collect_edits(&[], Path::new("a.xml"), src.as_bytes(), Path::new("/r"))
                .is_empty()
        );
    }

    #[test]
    fn xml_set_value_declines_a_statically_unsatisfiable_number() {
        // A numeric `equals` on XML (string leaves) can never match, so
        // `collect_edits` must emit NOTHING -- consistent with `can_fix` -- rather
        // than a suggestion that re-verify would demote anyway.
        let src = "<port>9090</port>";
        let num = StructuredFixer::set(
            Format::Xml,
            jp("$.port"),
            "$.port".into(),
            json!(8080),
            Applicability::Safe,
        );
        assert!(
            num.collect_edits(&[], Path::new("a.xml"), src.as_bytes(), Path::new("/r"))
                .is_empty()
        );
        // The STRING analog IS satisfiable and emits exactly one edit.
        let string = StructuredFixer::set(
            Format::Xml,
            jp("$.port"),
            "$.port".into(),
            json!("8080"),
            Applicability::Safe,
        );
        assert_eq!(
            string
                .collect_edits(&[], Path::new("a.xml"), src.as_bytes(), Path::new("/r"))
                .len(),
            1
        );
    }

    #[test]
    fn xml_remove_value_deletes_the_element_line() {
        let src = "<c>\n  <drop>x</drop>\n  <keep>1</keep>\n</c>\n";
        let f = StructuredFixer::remove(
            Format::Xml,
            jp("$.c.drop"),
            "$.c.drop".into(),
            Applicability::Unsafe,
        );
        let edits = f.collect_edits(&[], Path::new("a.xml"), src.as_bytes(), Path::new("/r"));
        assert_eq!(edits.len(), 1);
        let (start, end, content, _) = edit_of(&edits[0]);
        assert_eq!(&src[start..end], "  <drop>x</drop>\n");
        assert_eq!(content, "");
    }

    #[test]
    fn toml_set_emits_a_minimal_document_edit() {
        // A round-trip-CST format reserializes the whole document, but the fixer
        // reduces it to its MINIMAL changed span (follow-up 2) so independent rules
        // on one file stay disjoint. The edit is NOT whole-file, and applying it
        // reconstructs the mutated (typed, decor-preserved) document.
        let src = b"[s]\nport = 8080\n";
        let f = StructuredFixer::set(
            Format::Toml,
            jp("$['s']['port']"),
            "$['s']['port']".into(),
            json!(9090),
            Applicability::Safe,
        );
        let edits = f.collect_edits(&[], Path::new("a.toml"), src, Path::new("/r"));
        assert_eq!(edits.len(), 1);
        let (start, end, content, _) = edit_of(&edits[0]);
        assert_ne!(
            (start, end),
            (0, src.len()),
            "the edit is minimal, not whole-file"
        );
        let mut out = src.to_vec();
        out.splice(start..end, content.bytes());
        assert_eq!(out, b"[s]\nport = 9090\n");
    }

    #[test]
    fn toml_remove_emits_a_minimal_document_edit() {
        let src = b"[s]\nkeep = 1\ndrop = 2\n";
        let f = StructuredFixer::remove(
            Format::Toml,
            jp("$['s']['drop']"),
            "$['s']['drop']".into(),
            Applicability::Unsafe,
        );
        let edits = f.collect_edits(&[], Path::new("a.toml"), src, Path::new("/r"));
        assert_eq!(edits.len(), 1);
        let (start, end, content, _) = edit_of(&edits[0]);
        assert_ne!(
            (start, end),
            (0, src.len()),
            "the edit is minimal, not whole-file"
        );
        let mut out = src.to_vec();
        out.splice(start..end, content.bytes());
        assert_eq!(out, b"[s]\nkeep = 1\n");
    }

    #[test]
    fn toml_numeric_equals_is_fixable_unlike_string_leaf_formats() {
        let v = Violation::new("x");
        // TOML is TYPED, so a numeric `equals` clears can_fix (contrast the
        // dotenv/XML string-leaf decline of a non-string value).
        assert!(
            StructuredFixer::set(
                Format::Toml,
                jp("$.x"),
                "$.x".into(),
                json!(8080),
                Applicability::Safe
            )
            .can_fix(&v)
        );
        // Null is unrepresentable in TOML -> not fixable.
        assert!(
            !StructuredFixer::set(
                Format::Toml,
                jp("$.x"),
                "$.x".into(),
                json!(null),
                Applicability::Safe
            )
            .can_fix(&v)
        );
    }

    #[test]
    fn replace_gates_toml_and_rewrites_only_violating_nodes() {
        // Follow-up-1 audit MED fixes. Build a `*_path_matches` + replace fixer:
        // search `^` -> `v`, verify the value matches `^v`.
        let mk = |fmt| {
            StructuredFixer::replace(
                fmt,
                jp("$.deps.*"),
                "$.deps.*".into(),
                regex::Regex::new("^").unwrap(),
                "v".into(),
                "^v".into(),
                Applicability::Unsafe,
            )
        };
        let v = Violation::new("x");
        // TOML replace is document-rewrite (no value span) -> declines on EVERY
        // document, so `check` must NOT advertise it; a span format (JSON) IS.
        assert!(!mk(Format::Toml).can_fix(&v));
        assert!(mk(Format::Json).can_fix(&v));
        // A wildcard match hits a COMPLIANT node (`ok: "v9"`, already `^v`) and a
        // VIOLATING one (`bad: "2.0"`). Only the violating node is rewritten -- the
        // compliant one is left alone (rewriting it would clobber `v9` -> `vv9`).
        // The engine hands the fixer the LIVE violations (it never calls a located
        // fixer with none -- the `is_empty` guard fires first); each carries the
        // value-keyed baseline id the fixer correlates on, so `fix --baseline` never
        // rewrites a grandfathered node. Here only `bad` is live.
        let src = b"{\"deps\": {\"ok\": \"v9\", \"bad\": \"2.0\"}}";
        let bad = Violation::new("bad").with_baseline_key(
            crate::structured_path::matches_baseline_key("$.deps.*", "^v", &json!("2.0")),
        );
        let edits =
            mk(Format::Json).collect_edits(&[bad], Path::new("a.json"), src, Path::new("/r"));
        assert_eq!(edits.len(), 1, "only the non-compliant node is rewritten");
        let (start, end, content, _) = edit_of(&edits[0]);
        assert_eq!(content, "\"v2.0\"");
        assert_eq!(&String::from_utf8_lossy(src)[start..end], "\"2.0\"");
    }

    #[test]
    fn replace_rewrites_only_the_live_violations_it_is_handed() {
        // W4 CRITICAL (audit 2026-09-20): two non-compliant nodes, but the engine
        // hands the fixer only ONE as live -- the other is baseline-grandfathered
        // and filtered out upstream. The fixer re-derives BOTH from the file, so it
        // must correlate against the handed set and rewrite only the live node;
        // re-deriving both would mutate accepted debt (under-suppression).
        let f = StructuredFixer::replace(
            Format::Json,
            jp("$.deps.*"),
            "$.deps.*".into(),
            regex::Regex::new("^").unwrap(),
            "v".into(),
            "^v".into(),
            Applicability::Unsafe,
        );
        let src = b"{\"deps\": {\"old\": \"1.0\", \"new\": \"2.0\"}}";
        // Only `new` (value "2.0") is live; `old` (value "1.0") is grandfathered.
        let live = Violation::new("new").with_baseline_key(
            crate::structured_path::matches_baseline_key("$.deps.*", "^v", &json!("2.0")),
        );
        let edits = f.collect_edits(&[live], Path::new("a.json"), src, Path::new("/r"));
        assert_eq!(edits.len(), 1, "only the live node is rewritten");
        let (start, end, content, _) = edit_of(&edits[0]);
        assert_eq!(content, "\"v2.0\"");
        assert_eq!(
            &String::from_utf8_lossy(src)[start..end],
            "\"2.0\"",
            "the rewritten span is the LIVE node, not the grandfathered one"
        );
    }

    #[test]
    fn replace_without_keyed_violations_fixes_all_nodes() {
        // The LSP `code_action` synthesizes a positional violation with NO
        // baseline_key to offer a "fix every occurrence" action. With no keyed
        // violation to correlate against, the fixer falls back to rewriting every
        // non-compliant node (the legacy file-scoped behavior) -- otherwise the LSP
        // would offer no `*_path_matches` fix at all (audit 2026-09-20).
        let f = StructuredFixer::replace(
            Format::Json,
            jp("$.deps.*"),
            "$.deps.*".into(),
            regex::Regex::new("^").unwrap(),
            "v".into(),
            "^v".into(),
            Applicability::Unsafe,
        );
        let src = b"{\"deps\": {\"a\": \"1.0\", \"b\": \"2.0\"}}";
        // A keyless violation, exactly as the LSP builds it.
        let keyless = Violation::new("forbidden");
        let edits = f.collect_edits(&[keyless], Path::new("a.json"), src, Path::new("/r"));
        assert_eq!(
            edits.len(),
            2,
            "both non-compliant nodes are rewritten (fix-all fallback)"
        );
    }

    #[test]
    fn segs_to_query_round_trips_special_keys() {
        // The per-node verify re-queries this path; a key with `'`, `\`, a control
        // char, a quote, unicode, or empty must produce a PARSEABLE JSONPath that
        // re-resolves to the node -- else `JsonPath::parse` fails and the safe fix
        // is silently demoted (audit 2026-09-20).
        for key in [
            "normal",
            "it's",
            "back\\slash",
            "tab\tkey",
            "quote\"dq",
            "café",
            "",
            "$.weird[0]",
        ] {
            let mut m = serde_json::Map::new();
            m.insert(key.to_string(), Value::String("x".into()));
            let doc = Value::Object(m);
            let q = segs_to_query(&[PathSeg::Key(key.to_string())]);
            let jp = JsonPath::parse(&q)
                .unwrap_or_else(|e| panic!("query {q:?} for key {key:?} must parse: {e}"));
            let nodes = jp.query(&doc);
            assert_eq!(
                nodes.iter().count(),
                1,
                "query {q:?} must resolve key {key:?} to exactly one node"
            );
            assert_eq!(nodes.iter().next().and_then(|v| v.as_str()), Some("x"));
        }
    }
}
