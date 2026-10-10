//! `file_header` — first N lines of each file in scope must match a pattern.

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};
use regex::Regex;
use serde::Deserialize;

use crate::fixers::{FilePrependFixer, InsertHeaderFixer};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Rust regex. The first `lines` lines of each file in scope must match.
    pattern: String,
    /// Number of leading lines to consider.
    #[serde(default = "default_lines")]
    #[schemars(range(min = 1))]
    lines: usize,
}

fn default_lines() -> usize {
    20
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct FileHeaderRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    pattern_src: String,
    pattern: Regex,
    lines: usize,
    /// The `file_prepend` (blind BOF) or `insert_header` (after BOM/shebang/xml-decl)
    /// fixer; `file_header` declares at most one. `None` for a check-only rule.
    fixer: Option<Box<dyn Fixer>>,
}

impl Rule for FileHeaderRule {
    alint_core::rule_common_impl!();

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_deref()
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let full = ctx.root.join(&entry.path);
            // Cap the read so a multi-GB file matched here can't OOM the run
            // via the `for_each`-nested path (which bypasses the engine's cap).
            // Over-cap → skip, matching the engine's per-file batch so the same
            // rule behaves identically whether top-level or nested (M3-F1).
            // A genuine read error (permission / I/O) fails CLOSED with a
            // "could not read file" finding, exactly as `check`'s file-major
            // dispatch reports it, so `check` and `fix` (this whole-index
            // `evaluate` is the read path `fix` uses) agree; `NotFound` (a
            // mid-walk delete) and over-cap stay skips (audit 2026-10 finding 7).
            let bytes = match crate::io::read_capped(&full) {
                Ok(b) => b,
                Err(e) => {
                    violations.extend(crate::io::read_cap_error_violation(&entry.path, &e));
                    continue;
                }
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileHeaderRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(vec![
                Violation::new("file is not valid UTF-8; cannot match header")
                    .with_path(std::sync::Arc::<Path>::from(path))
                    // No fix resolves this: the fixer would prepend bytes, but the
                    // file would still not be valid UTF-8 (and a UTF-16 / binary
                    // file is refused outright), so `check` must not promise one.
                    .with_not_fixable(),
            ]);
        };
        // Match the content AFTER a leading UTF-8 BOM: the BOM is an encoding
        // signature, not header text, and the `file_prepend` / `insert_header`
        // fixers write the header after it (preserving it). Matching the raw text
        // made an anchored `^…` pattern never match a BOM file, so `check` kept
        // flagging what `fix` reported as already fixed (non-convergent).
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        let header: String = text.split_inclusive('\n').take(self.lines).collect();
        if self.pattern.is_match(&header) {
            return Ok(Vec::new());
        }
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "first {} line(s) do not match required header /{}/",
                self.lines, self.pattern_src
            )
        });
        Ok(vec![
            Violation::new(msg)
                .with_path(std::sync::Arc::<Path>::from(path))
                .with_location(1, 1)
                // The prepend / insert-header fixers refuse a binary-looking file
                // (e.g. valid UTF-8 with a NUL), so don't tag it fixable.
                .with_not_fixable_if(crate::io::looks_binary(bytes)),
        ])
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(_paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_header requires a `paths` field",
        ));
    };
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.lines == 0 {
        return Err(Error::rule_config(
            &spec.id,
            "file_header `lines` must be > 0",
        ));
    }
    let pattern = Regex::new(&opts.pattern)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid pattern: {e}")))?;
    let fixer: Option<Box<dyn Fixer>> = match &spec.fix {
        Some(FixSpec::FilePrepend { file_prepend }) => {
            let source = alint_core::resolve_content_source(
                &spec.id,
                "file_prepend",
                &file_prepend.content,
                &file_prepend.content_from,
            )?;
            Some(Box::new(
                FilePrependFixer::new(source).with_applicability(
                    file_prepend
                        .applicability
                        .unwrap_or(alint_core::Applicability::Safe),
                ),
            ))
        }
        // `insert_header` refines `file_prepend`: same header content, but inserted
        // AFTER a leading BOM / shebang / XML declaration so it never displaces a
        // line that must stay first. Safe by default (the position is the one
        // canonical header spot and the content is inert).
        Some(FixSpec::InsertHeader { insert_header }) => {
            // An empty inline header inserts nothing, so `inserted()` always reports
            // "already present" and the check never clears -- reject it up front with
            // a clear message rather than a misleading skip (audit F2). (`content_from`
            // an empty file is the same shape but only knowable at apply time.)
            if matches!(insert_header.content.as_deref(), Some("")) {
                return Err(Error::rule_config(
                    &spec.id,
                    "insert_header `content` must not be empty (an empty header inserts nothing \
                     and would never satisfy the check)",
                ));
            }
            let source = alint_core::resolve_content_source(
                &spec.id,
                "insert_header",
                &insert_header.content,
                &insert_header.content_from,
            )?;
            Some(Box::new(
                InsertHeaderFixer::new(source).with_applicability(
                    insert_header
                        .applicability
                        .unwrap_or(alint_core::Applicability::Safe),
                ),
            ))
        }
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with file_header", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileHeaderRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        pattern_src: opts.pattern,
        pattern,
        lines: opts.lines,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             pattern: \"^// SPDX\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn build_rejects_zero_lines() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"^// SPDX\"\n\
             lines: 0\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("lines"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"[unterminated\"\n\
             level: error\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_when_header_matches() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"SPDX-License-Identifier: Apache-2.0\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "src/main.rs",
            b"// SPDX-License-Identifier: Apache-2.0\n\nfn main() {}\n",
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "header should match: {v:?}");
    }

    #[test]
    fn header_after_a_utf8_bom_matches_and_the_fix_converges() {
        // Check/fix agreement regression: `file_prepend` / `insert_header` place
        // the header AFTER a leading UTF-8 BOM (preserving it), but the check
        // matched the raw text, BOM included, so an anchored `^// SPDX` never
        // matched: check kept flagging while fix said "already has header".
        // The check now matches the content after the BOM, where the fixers
        // write.
        for fix in [
            "file_prepend: { content: \"// SPDX-License-Identifier: MIT\\n\" }",
            "insert_header: { content: \"// SPDX-License-Identifier: MIT\\n\" }",
        ] {
            let rule = build(&spec_yaml(&format!(
                "id: t\nkind: file_header\npaths: \"**/*.rs\"\n\
                 pattern: \"^// SPDX-License-Identifier:\"\nlevel: error\nfix: {{ {fix} }}\n"
            )))
            .unwrap();
            let body: &[u8] = b"\xEF\xBB\xBFfn main() {}\n";
            let (tmp, idx) = tempdir_with_files(&[("a.rs", body)]);
            let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
            assert_eq!(v.len(), 1, "{fix}: missing header is flagged");
            let Some(alint_core::FixEdit::SetContent { content, .. }) =
                rule.fixer().unwrap().fix_edit(&v[0], body, tmp.path())
            else {
                panic!("{fix}: expected a SetContent edit");
            };
            assert!(
                content.starts_with(b"\xEF\xBB\xBF// SPDX"),
                "{fix}: BOM kept first"
            );
            std::fs::write(tmp.path().join("a.rs"), &content).unwrap();
            let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
            assert!(v.is_empty(), "{fix}: fixed file must pass the check: {v:?}");
        }
    }

    #[test]
    fn evaluate_fires_when_header_missing() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"SPDX-License-Identifier:\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("src/main.rs", b"fn main() {}\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn build_wires_insert_header_at_safe_and_rejects_incompatible_fix() {
        // insert_header is accepted on file_header (default Safe).
        let ok = build(&spec_yaml(
            "id: t\nkind: file_header\npaths: \"**/*.sh\"\npattern: \"SPDX\"\nlevel: error\n\
             fix: { insert_header: { content: \"# SPDX\\n\" } }\n",
        ))
        .unwrap();
        assert_eq!(
            ok.fixer().unwrap().applicability(),
            alint_core::Applicability::Safe
        );
        // A per-rule applicability override wins.
        let unsafe_ = build(&spec_yaml(
            "id: t\nkind: file_header\npaths: \"**/*.sh\"\npattern: \"SPDX\"\nlevel: error\n\
             fix: { insert_header: { content: \"# SPDX\\n\", applicability: unsafe } }\n",
        ))
        .unwrap();
        assert_eq!(
            unsafe_.fixer().unwrap().applicability(),
            alint_core::Applicability::Unsafe
        );
        // insert_header with neither content nor content_from is rejected.
        let no_content = build(&spec_yaml(
            "id: t\nkind: file_header\npaths: \"**/*.sh\"\npattern: \"SPDX\"\nlevel: error\n\
             fix: { insert_header: {} }\n",
        ));
        assert!(no_content.is_err(), "a header needs content/content_from");
        // insert_header with an EMPTY inline content is rejected (audit F2: an empty
        // header inserts nothing and never satisfies the check).
        let empty = build(&spec_yaml(
            "id: t\nkind: file_header\npaths: \"**/*.sh\"\npattern: \"SPDX\"\nlevel: error\n\
             fix: { insert_header: { content: \"\" } }\n",
        ))
        .unwrap_err()
        .to_string();
        assert!(empty.contains("must not be empty"), "{empty}");
        // A fix op that is neither file_prepend nor insert_header is rejected.
        let bad = build(&spec_yaml(
            "id: t\nkind: file_header\npaths: \"**/*.sh\"\npattern: \"SPDX\"\nlevel: error\n\
             fix: { file_remove: {} }\n",
        ))
        .unwrap_err()
        .to_string();
        assert!(bad.contains("not compatible with file_header"), "{bad}");
    }

    #[test]
    fn evaluate_only_inspects_first_n_lines() {
        // Pattern only on line 30, but `lines: 5` only looks at
        // lines 1-5 → rule fires.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_header\n\
             paths: \"src/**/*.rs\"\n\
             pattern: \"NEEDLE\"\n\
             lines: 5\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let mut content = String::new();
        for _ in 0..30 {
            content.push_str("filler\n");
        }
        content.push_str("NEEDLE\n");
        let (tmp, idx) = tempdir_with_files(&[("src/main.rs", content.as_bytes())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn non_utf8_and_binary_findings_are_not_fixable() {
        // The prepend fixer refuses binary / UTF-16 content, and prepending to a
        // non-UTF-8 file can never make it match, so `check` must not tag these
        // findings fixable (fix would never converge).
        let rule = build(&spec_yaml(
            "id: t\nkind: file_header\npaths: \"**/*\"\npattern: \"^// ok\"\n\
             level: error\nfix:\n  file_prepend:\n    content: \"// ok\\n\"\n",
        ))
        .unwrap();
        let (tmp, idx) = tempdir_with_files(&[
            ("utf16.txt", &[0xFF, 0xFE, 0x2D, 0x4E, 0x87, 0x65][..]),
            ("nul.txt", b"code\0more\n"),
            ("plain.txt", b"code\n"),
        ]);
        let mut v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        let flags: Vec<(String, bool)> = v
            .iter()
            .map(|x| (crate::slash(x.path.as_deref().unwrap()), x.not_fixable))
            .collect();
        assert_eq!(
            flags,
            vec![
                ("nul.txt".to_string(), true),
                ("plain.txt".to_string(), false),
                ("utf16.txt".to_string(), true),
            ]
        );
    }
}
