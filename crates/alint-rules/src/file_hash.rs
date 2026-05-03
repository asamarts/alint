//! `file_hash` — assert a file's SHA-256 equals a declared hex
//! string.
//!
//! Use cases: pin generated files so a re-run would fail if the
//! generator changes; verify that a bundled LICENSE text matches
//! the canonical Apache/MIT hash; lock down "do not edit" fixtures.
//!
//! Check-only. Fix would require knowing what the "right"
//! content is, which is the generator's job, not alint's.

use std::path::Path;

use alint_core::{Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Expected SHA-256 in lowercase hex (64 chars). Accepting
    /// uppercase and the `sha256:` prefix keeps the field forgiving.
    sha256: String,
}

#[derive(Debug)]
pub struct FileHashRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    expected: [u8; 32],
}

impl Rule for FileHashRule {
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
            if !self.scope.matches(&entry.path, ctx.index) {
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

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for FileHashRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let actual: [u8; 32] = hasher.finalize().into();
        if actual == self.expected {
            return Ok(Vec::new());
        }
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "sha256 mismatch: expected {}, got {}",
                encode_hex(&self.expected),
                encode_hex(&actual),
            )
        });
        Ok(vec![
            Violation::new(msg).with_path(std::sync::Arc::<Path>::from(path)),
        ])
    }
}

fn parse_sha256(raw: &str) -> std::result::Result<[u8; 32], String> {
    let trimmed = raw.strip_prefix("sha256:").unwrap_or(raw);
    if trimmed.len() != 64 {
        return Err(format!(
            "sha256 must be 64 hex chars; got {}",
            trimmed.len()
        ));
    }
    let mut out = [0u8; 32];
    for (i, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
        let hi = hex_digit(chunk[0]).ok_or_else(|| "non-hex character".to_string())?;
        let lo = hex_digit(chunk[1]).ok_or_else(|| "non-hex character".to_string())?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + b - b'a'),
        b'A'..=b'F' => Some(10 + b - b'A'),
        _ => None,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let _paths = spec
        .paths
        .as_ref()
        .ok_or_else(|| Error::rule_config(&spec.id, "file_hash requires a `paths` field"))?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let expected = parse_sha256(&opts.sha256)
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid sha256: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "file_hash has no fix op — alint can't synthesize the correct content",
        ));
    }
    Ok(Box::new(FileHashRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        expected,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHA-256 of the empty string.
    const EMPTY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn parses_bare_hex() {
        let bytes = parse_sha256(EMPTY_HASH).unwrap();
        assert_eq!(encode_hex(&bytes), EMPTY_HASH);
    }

    #[test]
    fn parses_sha256_prefix() {
        let bytes = parse_sha256(&format!("sha256:{EMPTY_HASH}")).unwrap();
        assert_eq!(encode_hex(&bytes), EMPTY_HASH);
    }

    #[test]
    fn accepts_uppercase_hex() {
        let upper = EMPTY_HASH.to_ascii_uppercase();
        let bytes = parse_sha256(&upper).unwrap();
        assert_eq!(encode_hex(&bytes), EMPTY_HASH);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(parse_sha256("e3b0c442").is_err());
    }

    #[test]
    fn rejects_non_hex_chars() {
        let bad = format!("zz{}", &EMPTY_HASH[2..]);
        assert!(parse_sha256(&bad).is_err());
    }
}
