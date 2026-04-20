//! Facts — cached properties of the repository evaluated once per run and
//! referenced by `when` clauses on rules (shipping in a later commit).
//!
//! Each fact has an `id` and exactly one kind-specific top-level field that
//! names its type. Example:
//!
//! ```yaml
//! facts:
//!   - id: is_rust
//!     any_file_exists: ["Cargo.toml"]
//!   - id: is_monorepo
//!     all_files_exist: ["packages", "pnpm-workspace.yaml"]
//!   - id: n_java_files
//!     count_files: "**/*.java"
//! ```
//!
//! Evaluation is declarative and cheap — facts see the walked `FileIndex`
//! but not arbitrary filesystem state outside the repo root.

use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::scope::Scope;
use crate::walker::FileIndex;

/// A value a fact evaluates to. Keeps the surface small for v0.2; richer
/// types (list, map) arrive with the `when` expression language.
#[derive(Debug, Clone, PartialEq)]
pub enum FactValue {
    Bool(bool),
    Int(i64),
    String(String),
}

impl FactValue {
    /// Boolean coercion — `Bool(b)` → b; `Int(n)` → `n != 0`; `String(s)` →
    /// `!s.is_empty()`. Used by `when` evaluation's truthiness checks.
    pub fn truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::String(s) => !s.is_empty(),
        }
    }
}

/// A string or a list of strings — accepted by fact kinds whose input is
/// glob-shaped.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s.clone()],
            Self::Many(v) => v.clone(),
        }
    }
}

/// YAML-level declaration of a single fact.
#[derive(Debug, Clone, Deserialize)]
pub struct FactSpec {
    pub id: String,
    #[serde(flatten)]
    pub kind: FactKind,
}

/// The closed set of built-in fact kinds. Serde dispatches via `untagged`
/// — the first variant whose required field is present in the YAML wins.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FactKind {
    AnyFileExists {
        any_file_exists: OneOrMany,
    },
    AllFilesExist {
        all_files_exist: OneOrMany,
    },
    CountFiles {
        count_files: String,
    },
    FileContentMatches {
        file_content_matches: FileContentMatchesFact,
    },
    GitBranch {
        git_branch: GitBranchFact,
    },
    Custom {
        custom: CustomFact,
    },
}

/// Fact-kind body for `custom`. Spawns `argv` as a child process
/// rooted at the repo; the process's stdout (trimmed of trailing
/// whitespace) becomes the fact's `String` value. A non-zero
/// exit code resolves to the empty string; timeouts and spawn
/// failures do the same. No shell is invoked — `argv` is passed
/// to `execve` (or the platform equivalent) verbatim.
///
/// Security: `custom` facts are only allowed in the user's own
/// top-level config. Any `extends:` ancestor that declares one
/// is rejected at load time — otherwise a malicious ruleset
/// could execute arbitrary code just by being fetched.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomFact {
    /// Program and arguments. `argv[0]` is looked up through PATH
    /// if it's not an absolute or relative-with-separator path.
    pub argv: Vec<String>,
}

/// Fact-kind body for `file_content_matches`. Fact evaluates
/// truthy when at least one file in `paths` contains a regex
/// match for `pattern`. Files that aren't valid UTF-8 are skipped.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileContentMatchesFact {
    pub paths: OneOrMany,
    pub pattern: String,
}

/// Fact-kind body for `git_branch`. Empty — the body is just
/// `git_branch: {}` in YAML and the discriminator is the key.
///
/// Evaluates to the current branch name by reading `.git/HEAD`
/// directly (no `git` binary required). Returns an empty string
/// when the repo isn't on a named branch (detached HEAD, not a
/// git repo at all, worktree/submodule variants, or any unusual
/// `.git` layout we don't fully resolve). An empty string is
/// falsy under `when:` coercion, so downstream rules naturally
/// no-op in those cases.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct GitBranchFact {}

/// The resolved map from fact id to value, produced once per `Engine::run`.
#[derive(Debug, Default, Clone)]
pub struct FactValues(HashMap<String, FactValue>);

impl FactValues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: String, v: FactValue) {
        self.0.insert(id, v);
    }

    pub fn get(&self, id: &str) -> Option<&FactValue> {
        self.0.get(id)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_map(&self) -> &HashMap<String, FactValue> {
        &self.0
    }
}

/// Evaluate a whole fact list against a prebuilt `FileIndex`. Invoked by
/// `Engine::run` before any rule evaluates.
pub fn evaluate_facts(facts: &[FactSpec], root: &Path, index: &FileIndex) -> Result<FactValues> {
    let mut out = FactValues::new();
    for spec in facts {
        let value = evaluate_one(spec, root, index)?;
        out.insert(spec.id.clone(), value);
    }
    Ok(out)
}

fn evaluate_one(spec: &FactSpec, root: &Path, index: &FileIndex) -> Result<FactValue> {
    match &spec.kind {
        FactKind::AnyFileExists { any_file_exists } => {
            let globs = any_file_exists.to_vec();
            let scope = Scope::from_patterns(&globs)?;
            let found = index.files().any(|e| scope.matches(&e.path));
            Ok(FactValue::Bool(found))
        }
        FactKind::AllFilesExist { all_files_exist } => {
            let globs = all_files_exist.to_vec();
            for glob in &globs {
                let scope = Scope::from_patterns(std::slice::from_ref(glob))?;
                if !index.files().any(|e| scope.matches(&e.path)) {
                    return Ok(FactValue::Bool(false));
                }
            }
            Ok(FactValue::Bool(true))
        }
        FactKind::CountFiles { count_files } => {
            let scope = Scope::from_patterns(std::slice::from_ref(count_files))?;
            let count = index.files().filter(|e| scope.matches(&e.path)).count();
            Ok(FactValue::Int(i64::try_from(count).unwrap_or(i64::MAX)))
        }
        FactKind::FileContentMatches {
            file_content_matches: spec,
        } => {
            let scope = Scope::from_patterns(&spec.paths.to_vec())?;
            let regex = Regex::new(&spec.pattern)
                .map_err(|e| Error::Other(format!("fact pattern /{}/: {e}", spec.pattern)))?;
            let any = index.files().any(|entry| {
                if !scope.matches(&entry.path) {
                    return false;
                }
                let Ok(bytes) = std::fs::read(root.join(&entry.path)) else {
                    return false;
                };
                let Ok(text) = std::str::from_utf8(&bytes) else {
                    return false;
                };
                regex.is_match(text)
            });
            Ok(FactValue::Bool(any))
        }
        FactKind::GitBranch { git_branch: _ } => Ok(FactValue::String(read_git_branch(root))),
        FactKind::Custom { custom } => Ok(FactValue::String(run_custom(custom, root))),
    }
}

/// Best-effort: spawn `argv` at `root`, capture stdout. Non-zero
/// exit / spawn failures / unusable output → empty string.
fn run_custom(spec: &CustomFact, root: &Path) -> String {
    let Some((program, args)) = spec.argv.split_first() else {
        return String::new();
    };
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let Ok(output) = output else {
        return String::new();
    };
    if !output.status.success() {
        return String::new();
    }
    match std::str::from_utf8(&output.stdout) {
        Ok(text) => text.trim_end().to_string(),
        Err(_) => String::new(),
    }
}

/// Reject `custom` facts in `config`. Used by the DSL loader to
/// enforce that only the user's top-level config can spawn
/// processes; extended (local or remote) configs can't.
pub fn reject_custom_facts(config: &crate::config::Config, source: &str) -> Result<()> {
    for f in &config.facts {
        if matches!(f.kind, FactKind::Custom { .. }) {
            return Err(Error::Other(format!(
                "fact {:?}: `custom:` facts are only allowed in the user's top-level \
                 config; declaring one in an extended config ({source}) is refused because \
                 it would let a ruleset spawn arbitrary processes",
                f.id
            )));
        }
    }
    Ok(())
}

/// Best-effort branch resolution: read `<root>/.git/HEAD` and
/// extract the branch from a `ref: refs/heads/<branch>` line.
/// Detached HEADs, bare SHAs, worktree pointers, missing files,
/// non-UTF-8 content — every edge case returns `""`. Downstream
/// `when:` coercion treats that as falsy.
fn read_git_branch(root: &Path) -> String {
    let head = root.join(".git").join("HEAD");
    let Ok(content) = std::fs::read_to_string(&head) else {
        return String::new();
    };
    content
        .trim()
        .strip_prefix("ref: refs/heads/")
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::FileEntry;
    use std::path::PathBuf;

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex {
            entries: paths
                .iter()
                .map(|p| FileEntry {
                    path: PathBuf::from(p),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        }
    }

    fn parse(yaml: &str) -> Vec<FactSpec> {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    #[test]
    fn any_file_exists_true_when_match_found() {
        let facts = parse("- id: is_rust\n  any_file_exists: [Cargo.toml]\n");
        let v =
            evaluate_facts(&facts, Path::new("/"), &idx(&["Cargo.toml", "src/lib.rs"])).unwrap();
        assert_eq!(v.get("is_rust"), Some(&FactValue::Bool(true)));
    }

    #[test]
    fn any_file_exists_false_when_no_match() {
        let facts = parse("- id: is_rust\n  any_file_exists: [Cargo.toml]\n");
        let v = evaluate_facts(&facts, Path::new("/"), &idx(&["src/lib.rs"])).unwrap();
        assert_eq!(v.get("is_rust"), Some(&FactValue::Bool(false)));
    }

    #[test]
    fn any_file_exists_accepts_single_string() {
        let facts = parse("- id: has_readme\n  any_file_exists: README.md\n");
        let v = evaluate_facts(&facts, Path::new("/"), &idx(&["README.md"])).unwrap();
        assert_eq!(v.get("has_readme"), Some(&FactValue::Bool(true)));
    }

    #[test]
    fn all_files_exist_true_when_all_match() {
        let facts = parse("- id: is_monorepo\n  all_files_exist: [Cargo.toml, README.md]\n");
        let v = evaluate_facts(
            &facts,
            Path::new("/"),
            &idx(&["Cargo.toml", "README.md", "src/main.rs"]),
        )
        .unwrap();
        assert_eq!(v.get("is_monorepo"), Some(&FactValue::Bool(true)));
    }

    #[test]
    fn all_files_exist_false_when_any_missing() {
        let facts = parse("- id: is_monorepo\n  all_files_exist: [Cargo.toml, README.md]\n");
        let v = evaluate_facts(&facts, Path::new("/"), &idx(&["Cargo.toml"])).unwrap();
        assert_eq!(v.get("is_monorepo"), Some(&FactValue::Bool(false)));
    }

    #[test]
    fn count_files_returns_integer() {
        let facts = parse("- id: n_rs\n  count_files: \"**/*.rs\"\n");
        let v = evaluate_facts(
            &facts,
            Path::new("/"),
            &idx(&["a.rs", "b.rs", "src/c.rs", "README.md"]),
        )
        .unwrap();
        assert_eq!(v.get("n_rs"), Some(&FactValue::Int(3)));
    }

    #[test]
    fn multiple_facts_all_resolved() {
        let facts = parse(
            r#"
- id: is_rust
  any_file_exists: [Cargo.toml]
- id: n_rs
  count_files: "**/*.rs"
- id: has_readme
  any_file_exists: README.md
"#,
        );
        let v = evaluate_facts(
            &facts,
            Path::new("/"),
            &idx(&["Cargo.toml", "src/lib.rs", "README.md"]),
        )
        .unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v.get("is_rust"), Some(&FactValue::Bool(true)));
        assert_eq!(v.get("n_rs"), Some(&FactValue::Int(1)));
        assert_eq!(v.get("has_readme"), Some(&FactValue::Bool(true)));
    }

    #[test]
    fn file_content_matches_true_when_pattern_appears() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[dependencies]\ntokio = \"1\"\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("README.md"), "hello\n").unwrap();

        let facts = parse(
            "- id: uses_tokio\n  file_content_matches:\n    paths: Cargo.toml\n    pattern: tokio\n",
        );
        let idx = idx(&["Cargo.toml", "README.md"]);
        let v = evaluate_facts(&facts, tmp.path(), &idx).unwrap();
        assert_eq!(v.get("uses_tokio"), Some(&FactValue::Bool(true)));
    }

    #[test]
    fn file_content_matches_false_when_pattern_absent() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[dependencies]\n").unwrap();

        let facts = parse(
            "- id: uses_tokio\n  file_content_matches:\n    paths: Cargo.toml\n    pattern: tokio\n",
        );
        let idx = idx(&["Cargo.toml"]);
        let v = evaluate_facts(&facts, tmp.path(), &idx).unwrap();
        assert_eq!(v.get("uses_tokio"), Some(&FactValue::Bool(false)));
    }

    #[test]
    fn file_content_matches_skips_non_utf8_files() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        // Invalid UTF-8 byte sequence.
        std::fs::write(tmp.path().join("blob.bin"), [0xFFu8, 0xFE, 0x00, 0x01]).unwrap();
        std::fs::write(
            tmp.path().join("text.txt"),
            "SPDX-License-Identifier: MIT\n",
        )
        .unwrap();

        let facts = parse(
            "- id: has_spdx\n  file_content_matches:\n    paths: [\"**/*\"]\n    pattern: SPDX\n",
        );
        let idx = idx(&["blob.bin", "text.txt"]);
        let v = evaluate_facts(&facts, tmp.path(), &idx).unwrap();
        // Non-UTF-8 is silently skipped, so `text.txt` is what matters.
        assert_eq!(v.get("has_spdx"), Some(&FactValue::Bool(true)));
    }

    #[test]
    fn git_branch_reads_refs_heads() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/HEAD"), "ref: refs/heads/feature-x\n").unwrap();

        let facts = parse("- id: branch\n  git_branch: {}\n");
        let v = evaluate_facts(&facts, tmp.path(), &idx(&[])).unwrap();
        assert_eq!(
            v.get("branch"),
            Some(&FactValue::String("feature-x".to_string()))
        );
    }

    #[test]
    fn git_branch_detached_head_is_empty_string() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(
            tmp.path().join(".git/HEAD"),
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n",
        )
        .unwrap();

        let facts = parse("- id: branch\n  git_branch: {}\n");
        let v = evaluate_facts(&facts, tmp.path(), &idx(&[])).unwrap();
        assert_eq!(v.get("branch"), Some(&FactValue::String(String::new())));
    }

    #[test]
    fn git_branch_missing_git_dir_is_empty_string() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let facts = parse("- id: branch\n  git_branch: {}\n");
        let v = evaluate_facts(&facts, tmp.path(), &idx(&[])).unwrap();
        assert_eq!(v.get("branch"), Some(&FactValue::String(String::new())));
    }

    #[cfg(unix)]
    #[test]
    fn custom_captures_stdout_trimmed() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let facts = parse(
            "- id: greeting\n  custom:\n    argv: [\"/bin/sh\", \"-c\", \"printf 'hello world\\n'\"]\n",
        );
        let v = evaluate_facts(&facts, tmp.path(), &idx(&[])).unwrap();
        assert_eq!(
            v.get("greeting"),
            Some(&FactValue::String("hello world".to_string()))
        );
    }

    #[test]
    fn custom_unknown_program_is_empty_string() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        let facts =
            parse("- id: nope\n  custom:\n    argv: [\"no-such-program-alint-test-xyzzy\"]\n");
        let v = evaluate_facts(&facts, tmp.path(), &idx(&[])).unwrap();
        assert_eq!(v.get("nope"), Some(&FactValue::String(String::new())));
    }

    #[cfg(unix)]
    #[test]
    fn custom_nonzero_exit_is_empty_string() {
        use tempfile::tempdir;
        let tmp = tempdir().unwrap();
        // `false` exits 1; we should not see any captured output.
        let facts = parse("- id: bad\n  custom:\n    argv: [\"/bin/false\"]\n");
        let v = evaluate_facts(&facts, tmp.path(), &idx(&[])).unwrap();
        assert_eq!(v.get("bad"), Some(&FactValue::String(String::new())));
    }

    #[test]
    fn reject_custom_facts_flags_custom_but_passes_others() {
        let facts = parse(
            "- id: plain\n  any_file_exists: x\n- id: run\n  custom:\n    argv: [\"echo\"]\n",
        );
        let config = crate::config::Config {
            version: 1,
            extends: Vec::new(),
            ignore: Vec::new(),
            respect_gitignore: true,
            vars: std::collections::HashMap::new(),
            facts,
            rules: Vec::new(),
        };
        let err = reject_custom_facts(&config, "./base.yml").unwrap_err();
        assert!(err.to_string().contains("custom"), "{err}");
        assert!(err.to_string().contains("./base.yml"), "{err}");
    }

    #[test]
    fn reject_custom_facts_ok_when_none_present() {
        let facts = parse("- id: plain\n  any_file_exists: x\n");
        let config = crate::config::Config {
            version: 1,
            extends: Vec::new(),
            ignore: Vec::new(),
            respect_gitignore: true,
            vars: std::collections::HashMap::new(),
            facts,
            rules: Vec::new(),
        };
        assert!(reject_custom_facts(&config, "./base.yml").is_ok());
    }

    #[test]
    fn truthy_coercion() {
        assert!(FactValue::Bool(true).truthy());
        assert!(!FactValue::Bool(false).truthy());
        assert!(FactValue::Int(1).truthy());
        assert!(!FactValue::Int(0).truthy());
        assert!(FactValue::String("x".into()).truthy());
        assert!(!FactValue::String(String::new()).truthy());
    }
}
