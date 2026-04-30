//! `final_newline` — every non-empty file in scope must end
//! with a newline byte.
//!
//! Rationale: POSIX defines a text file as one whose lines end
//! in `\n`. Many tools (`git diff`, `cat`, `less`, every C-family
//! toolchain) handle trailing-newline-less files awkwardly. The
//! rule is check-only by default; wire `fix: file_append_final_newline`
//! to auto-append.
//!
//! Empty files (zero bytes) are treated as fine — there's no
//! missing trailing newline on a file with no content.

use std::path::Path;

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation,
};

use crate::fixers::FileAppendFinalNewlineFixer;

#[derive(Debug)]
pub struct FinalNewlineRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    fixer: Option<FileAppendFinalNewlineFixer>,
}

impl Rule for FinalNewlineRule {
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
            let full = ctx.root.join(&entry.path);
            let Ok(bytes) = std::fs::read(&full) else {
                continue;
            };
            violations.extend(self.evaluate_file(ctx, &entry.path, &bytes)?);
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FinalNewlineRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        // Empty files (zero bytes) are fine — no missing trailing
        // newline on a file with no content.
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        if bytes.last().copied() == Some(b'\n') {
            return Ok(Vec::new());
        }
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| "file does not end with a newline".to_string());
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }

    fn max_bytes_needed(&self) -> Option<usize> {
        // Only the last byte matters; engine reads whole file
        // today but a future bounded-read pass could honour this.
        Some(1)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "final_newline requires a `paths` field"))?;
    let scope = Scope::from_paths_spec(paths)?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileAppendFinalNewline { .. }) => Some(FileAppendFinalNewlineFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "fix.{} is not compatible with final_newline",
                    other.op_name()
                ),
            ));
        }
        None => None,
    };
    Ok(Box::new(FinalNewlineRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope,
        fixer,
    }))
}

#[cfg(test)]
mod tests {
    fn lacks_final_newline(bytes: &[u8]) -> bool {
        !bytes.is_empty() && bytes.last().copied() != Some(b'\n')
    }

    #[test]
    fn file_ending_in_lf_is_fine() {
        assert!(!lacks_final_newline(b"hello\n"));
    }

    #[test]
    fn file_ending_in_crlf_is_fine() {
        // CRLF ends in \n too — final_newline doesn't care about
        // line-ending style; that's line_endings's job.
        assert!(!lacks_final_newline(b"hello\r\n"));
    }

    #[test]
    fn file_missing_trailing_newline_flagged() {
        assert!(lacks_final_newline(b"hello"));
    }

    #[test]
    fn empty_file_is_ok() {
        assert!(!lacks_final_newline(b""));
    }

    #[test]
    fn single_newline_is_ok() {
        assert!(!lacks_final_newline(b"\n"));
    }
}
