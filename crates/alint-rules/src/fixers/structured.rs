//! The `set_value` / `remove_value` located structured fixers (Phase 2).
//!
//! Both re-run the host rule's `JSONPath` over the parsed document, map each
//! concrete match location to a byte range via a span-resolving parser
//! (alint-core `structured_fix`), and emit a [`FixEdit::ReplaceRange`] carrying a
//! [`EditVerifier::Structured`] re-parse obligation. The engine batches,
//! overlap-skips, splices, then re-parses and re-runs the query, demoting any
//! edit whose result does not verify to a Suggestion instead of writing bad
//! bytes (auto-fix.md 5.3/5.4/5.8).

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
            // A removal has no statically-unsatisfiable case: whether a matched
            // node can be deleted is document-dependent (the resolver declines a
            // repeated-block / array-parent node, or the document root -- deleting
            // the root would empty the file). Those declines surface at fix time as
            // a `skipped`/`suggested`, never a corruption. So `check` reports
            // "fixable" and a later `fix` may `skip` it: a KNOWN residual
            // over-promise in the safe direction (check never claims LESS than fix
            // resolves; the reverse -- claiming fixable then skipping -- is the
            // tolerated gap, and is exercised by `remove_value_xml_root_is_declined`).
            StructuredOp::Remove => true,
        }
    }

    fn collects_located_edits(&self) -> bool {
        true
    }

    fn collect_edits(
        &self,
        _violations: &[Violation],
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
        // Emit ONE ReplaceRange over the entire file. (Two such rules on one file
        // emit overlapping whole-file edits; the engine applies one and the others
        // re-apply on the next fixpoint re-walk -- correct, just multi-pass.)
        if structured_fix::uses_document_rewrite(self.format) {
            let whole = 0..bytes.len();
            let one = |content: Vec<u8>, expect: ExpectedValue| {
                vec![CollectedEdit {
                    edit: FixEdit::ReplaceRange {
                        path: file.to_path_buf(),
                        range: whole.clone(),
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
            };
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
        // remove_value's declines are document-dependent -> reported fixable.
        assert!(
            StructuredFixer::remove(Format::Xml, jp("$.a"), "$.a".into(), Applicability::Unsafe)
                .can_fix(&v)
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
    fn toml_set_emits_a_whole_document_rewrite() {
        // A round-trip-CST format emits ONE ReplaceRange over the WHOLE file (the
        // mutated document), not a span splice.
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
        assert_eq!((start, end), (0, src.len())); // whole-file range
        assert_eq!(content, "[s]\nport = 9090\n"); // typed, decor preserved
    }

    #[test]
    fn toml_remove_emits_a_whole_document_rewrite() {
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
        assert_eq!((start, end), (0, src.len()));
        assert_eq!(content, "[s]\nkeep = 1\n");
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
}
