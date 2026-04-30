//! `file_is_text` — every file in scope must be detected as text (not binary).
//!
//! Detection uses `content_inspector` on the first 8 KiB of each file
//! (magic-byte + heuristic analysis). UTF-8, UTF-16 (with BOM), and plain
//! 7-bit ASCII are treated as text.

use std::path::Path;

use alint_core::{Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation};

use crate::io::{Classification, TEXT_INSPECT_LEN, classify_bytes, read_prefix};

#[derive(Debug)]
pub struct FileIsTextRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for FileIsTextRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        self.level
    }
    fn policy_url(&self) -> Option<&str> {
        self.policy_url.as_deref()
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path) {
                continue;
            }
            if entry.size == 0 {
                // Empty files are text by convention.
                continue;
            }
            // Bounded read: only the first TEXT_INSPECT_LEN
            // bytes feed `content_inspector`. Solo runs read
            // just that prefix; the dispatch-flip path receives
            // the whole file from the engine and inspects only
            // the prefix.
            let full = ctx.root.join(&entry.path);
            let bytes = match read_prefix(&full) {
                Ok(b) => b,
                Err(e) => {
                    violations.push(
                        Violation::new(format!("could not read file: {e}"))
                            .with_path(entry.path.clone()),
                    );
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

impl PerFileRule for FileIsTextRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        // Inspect only the first TEXT_INSPECT_LEN bytes; the
        // engine handed us the full file but the classifier
        // only needs the prefix.
        let sample = &bytes[..bytes.len().min(TEXT_INSPECT_LEN)];
        if classify_bytes(sample) != Classification::Binary {
            return Ok(Vec::new());
        }
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| "file is detected as binary; text is required here".to_string());
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }

    fn max_bytes_needed(&self) -> Option<usize> {
        Some(TEXT_INSPECT_LEN)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_is_text requires a `paths` field",
        ));
    };
    Ok(Box::new(FileIsTextRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
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
             kind: file_is_text\n\
             level: warning\n",
        );
        assert!(build(&spec).is_err());
    }

    #[test]
    fn evaluate_passes_on_utf8_text() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_is_text\n\
             paths: \"**/*.rs\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("a.rs", b"// hello\nfn main() {}\n")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "utf-8 text should pass: {v:?}");
    }

    #[test]
    fn evaluate_fires_on_binary_content() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_is_text\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        // Bytes with NUL + binary tail; content_inspector
        // should classify as Binary.
        let mut binary = vec![0u8; 16];
        binary.extend_from_slice(&[0xff, 0xfe, 0xfd, 0xfc]);
        let (tmp, idx) = tempdir_with_files(&[("img.bin", &binary)]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "binary should fire: {v:?}");
    }

    #[test]
    fn evaluate_silent_on_zero_byte_file() {
        // Empty files are treated as text by convention —
        // no read needed, no violation.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_is_text\n\
             paths: \"**/*\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("empty", b"")]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn evaluate_skips_out_of_scope_files() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_is_text\n\
             paths: \"src/**/*.rs\"\n\
             level: warning\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("img.bin", &[0u8; 64])]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "out-of-scope shouldn't fire: {v:?}");
    }
}
