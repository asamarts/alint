//! `file_absent` — emit a violation for every file matching `paths`.

use alint_core::{
    Context, Error, FixSpec, Fixer, Level, PathsSpec, Result, Rule, RuleSpec, Scope, Violation,
};
use serde::Deserialize;

use crate::fixers::FileRemoveFixer;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// If true, only a file matching `paths` directly at the repository root is
    /// forbidden; a nested match with the same name is allowed.
    #[serde(default)]
    root_only: bool,
    /// Restrict matches to files tracked in git's index: entries present in the
    /// walked tree but not in `git ls-files` are skipped. No effect outside a
    /// git repo. Default `false`.
    #[serde(default)]
    git_tracked_only: bool,
    /// When non-empty, a file matching `paths` is reported only if its raw
    /// content begins with one of these byte signatures, each given as an
    /// even-length hex string (e.g. `"00051607"`). This separates genuine
    /// binary junk from unrelated files that merely share the name pattern:
    /// macOS `AppleDouble` sidecars start with `00 05 16 07` and `.DS_Store`
    /// with `00 00 00 01 "Bud1"`, whereas Hadoop writes `._<name>.crc`
    /// checksum files that begin with `crc\0` and are not macOS junk. A file
    /// that cannot be read, or is shorter than every signature, does not
    /// match. Empty (the default) keeps the historical name-only behaviour.
    #[serde(default)]
    content_prefix_hex: Vec<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct FileAbsentRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    patterns: Vec<String>,
    root_only: bool,
    /// When `true`, only fire on entries that are also tracked
    /// in git's index. Outside a git repo or with no rules
    /// opting in, the tracked-set is `None` and every entry
    /// reads as "untracked," so the rule becomes a no-op -
    /// which is the right default for "don't let X be
    /// committed" semantics.
    git_tracked_only: bool,
    /// Parsed `content_prefix_hex` signatures. Empty ⇒ pure name-based
    /// matching (the historical behaviour); non-empty ⇒ a name match is only
    /// reported when the file's leading bytes equal one of these prefixes.
    content_prefixes: Vec<Vec<u8>>,
    fixer: Option<FileRemoveFixer>,
}

impl Rule for FileAbsentRule {
    alint_core::rule_common_impl!();
    fn git_tracked_mode(&self) -> alint_core::GitTrackedMode {
        if self.git_tracked_only {
            alint_core::GitTrackedMode::FileOnly
        } else {
            alint_core::GitTrackedMode::Off
        }
    }

    fn requires_full_index(&self) -> bool {
        // The verdict on "is X forbidden?" is over the whole tree —
        // an unchanged-but-already-committed `.env` should still
        // be visible. The engine skips this rule entirely when its
        // scope doesn't intersect the diff, which is the usual
        // user expectation in `--changed` mode.
        true
    }

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        // v0.9.11: when `git_tracked_only` is set the engine
        // hands us a pre-filtered `ctx.index` (file_only mode);
        // the per-entry `is_git_tracked` check that lived here
        // is now subsumed by the engine-side narrowing.
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            // `root_only`: only a match directly at the repo root is forbidden;
            // a nested file with the same name is allowed.
            if self.root_only && crate::is_nested(&entry.path) {
                continue;
            }
            // Content-signature gate: when configured, a name match is only a
            // violation if the file's leading bytes match one of the
            // signatures. Keeps look-alikes (e.g. Hadoop's `._*.crc` checksum
            // files) out of a macOS-junk rule that would otherwise fire on the
            // shared `._` prefix alone.
            if !self.content_prefixes.is_empty()
                && !self.content_matches(&ctx.root.join(&entry.path))
            {
                continue;
            }
            let msg = self.message.clone().unwrap_or_else(|| {
                let tracked = if self.git_tracked_only {
                    " and tracked in git"
                } else {
                    ""
                };
                format!(
                    "file is forbidden (matches [{}]{tracked}): {}",
                    self.patterns.join(", "),
                    entry.path.display()
                )
            });
            violations.push(Violation::new(msg).with_path(entry.path.clone()));
        }
        Ok(violations)
    }

    fn fixer(&self) -> Option<&dyn Fixer> {
        self.fixer.as_ref().map(|f| f as &dyn Fixer)
    }
}

impl FileAbsentRule {
    /// Do `full`'s leading bytes match any configured content signature?
    ///
    /// Reads only as many bytes as the longest signature. A file that cannot
    /// be opened or read is treated as a non-match: the rule never flags what
    /// it cannot positively identify as junk (an unreadable `._foo` is far more
    /// likely a permissions quirk than committed `AppleDouble` data).
    fn content_matches(&self, full: &std::path::Path) -> bool {
        let max = self
            .content_prefixes
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        let Ok(head) = crate::io::read_prefix_n(full, max) else {
            return false;
        };
        self.content_prefixes
            .iter()
            .any(|sig| head.starts_with(sig))
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "file_absent")?;
    let Some(paths) = &spec.paths else {
        return Err(Error::rule_config(
            &spec.id,
            "file_absent requires a `paths` field",
        ));
    };
    let opts: Options = spec.deserialize_options()?;
    let content_prefixes = opts
        .content_prefix_hex
        .iter()
        .map(|hex| parse_content_prefix(hex))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|msg| Error::rule_config(&spec.id, msg))?;
    let fixer = match &spec.fix {
        Some(FixSpec::FileRemove { .. }) => Some(FileRemoveFixer),
        Some(other) => {
            return Err(Error::rule_config(
                &spec.id,
                format!("fix.{} is not compatible with file_absent", other.op_name()),
            ));
        }
        None => None,
    };
    Ok(Box::new(FileAbsentRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_paths_spec(paths)?,
        patterns: patterns_of(paths),
        root_only: opts.root_only,
        git_tracked_only: opts.git_tracked_only,
        content_prefixes,
        fixer,
    }))
}

fn patterns_of(spec: &PathsSpec) -> Vec<String> {
    match spec {
        PathsSpec::Single(s) => vec![s.clone()],
        PathsSpec::Many(v) => v.clone(),
        PathsSpec::IncludeExclude { include, .. } => include.clone(),
    }
}

/// Decode one `content_prefix_hex` entry into raw bytes. Rejects empty,
/// odd-length, non-ASCII, and non-hex inputs so a malformed signature surfaces
/// at load time rather than silently never matching (odd/garbage) or always
/// matching (empty ⇒ every file "starts with" it).
fn parse_content_prefix(hex: &str) -> std::result::Result<Vec<u8>, String> {
    if hex.is_empty() {
        return Err("content_prefix_hex entries must not be empty".to_string());
    }
    if !hex.is_ascii() || hex.len() % 2 != 0 {
        return Err(format!(
            "content_prefix_hex entries must be even-length ASCII hex, got {hex:?}"
        ));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| format!("content_prefix_hex has a non-hex digit: {hex:?}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index, spec_yaml, tempdir_with_files};
    use std::path::Path;

    #[test]
    fn build_rejects_missing_paths_field() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             level: error\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("paths"), "unexpected: {err}");
    }

    #[test]
    fn build_rejects_incompatible_fix_op() {
        // file_absent supports `file_remove` only; any other
        // op surfaces a config error so a typo doesn't silently
        // disable the fix path.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             fix:\n  \
               file_create:\n    \
                 content: \"\"\n",
        );
        let err = build(&spec).unwrap_err().to_string();
        assert!(err.contains("file_create"), "unexpected: {err}");
    }

    #[test]
    fn build_accepts_file_remove_fix() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             fix:\n  \
               file_remove: {}\n",
        );
        let rule = build(&spec).expect("valid file_remove fix");
        assert!(rule.fixer().is_some(), "fixer should be present");
    }

    #[test]
    fn evaluate_passes_when_no_match_present() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["src/main.rs", "README.md"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert!(v.is_empty(), "unexpected: {v:?}");
    }

    #[test]
    fn evaluate_fires_one_violation_per_match() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"**/*.bak\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        let idx = index(&["a.bak", "src/b.bak", "ok.txt"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 2, "expected one violation per .bak: {v:?}");
    }

    #[test]
    fn git_tracked_only_advertises_file_only_mode() {
        // v0.9.11: the silent-no-op-outside-git-repo guarantee
        // moved from a per-rule runtime check to an engine-side
        // pre-filtered FileIndex. Calling `evaluate` directly
        // bypasses the engine's filtering, so this unit test
        // can no longer assert the no-op behaviour at the rule
        // level — instead it asserts the rule advertises the
        // correct `GitTrackedMode`, which is what tells the
        // engine to substitute an empty index when the
        // tracked-set is `None`. The end-to-end no-op behaviour
        // is asserted by the e2e scenarios under
        // `crates/alint-e2e/scenarios/check/git/`.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             git_tracked_only: true\n",
        );
        let rule = build(&spec).unwrap();
        assert_eq!(
            rule.git_tracked_mode(),
            alint_core::GitTrackedMode::FileOnly,
            "git_tracked_only on file_absent must advertise FileOnly mode",
        );
    }

    /// ADR-0008: `respect_gitignore` is honoured ONLY by `file_exists` (pitfall
    /// #18), so it lives solely in `file_exists`'s `Options`. A sibling
    /// existence kind like `file_absent` must reject it at load rather than
    /// accept-and-ignore - the subtle case where a user assumes every existence
    /// kind shares the option.
    #[test]
    fn build_rejects_respect_gitignore_sibling_existence_kind() {
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \"*.bak\"\n\
             level: error\n\
             respect_gitignore: false\n",
        );
        assert!(
            build(&spec).is_err(),
            "respect_gitignore must be rejected on file_absent (file_exists-only, ADR-0008)"
        );
    }

    #[test]
    fn rule_advertises_full_index_requirement() {
        // Existence-axis rules opt out of changed-mode
        // filtering — an unchanged-but-already-committed `.env`
        // should still fire.
        let spec = spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: \".env\"\n\
             level: error\n",
        );
        let rule = build(&spec).unwrap();
        assert!(rule.requires_full_index());
    }

    #[test]
    fn build_rejects_scope_filter_on_cross_file_rule() {
        // file_absent is a cross-file rule (requires_full_index =
        // true); scope_filter is per-file-rules-only. The build
        // path must reject it with a clear message pointing at
        // the for_each_dir + when_iter: alternative.
        let yaml = r#"
id: t
kind: file_absent
paths: "*.bak"
level: error
scope_filter:
  has_ancestor: Cargo.toml
"#;
        let spec = spec_yaml(yaml);
        let err = build(&spec).unwrap_err().to_string();
        assert!(
            err.contains("scope_filter is supported on per-file rules only"),
            "expected per-file-only message, got: {err}",
        );
        assert!(
            err.contains("file_absent"),
            "expected message to name the cross-file kind, got: {err}",
        );
    }

    #[test]
    fn root_only_forbids_only_root_level_matches() {
        let rule = build(&spec_yaml(
            "id: t\nkind: file_absent\npaths: \"**/notes.md\"\nlevel: error\nroot_only: true\n",
        ))
        .unwrap();
        let idx = index(&["notes.md", "sub/notes.md"]);
        assert_eq!(
            rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap().len(),
            1,
            "root_only forbids only the root-level notes.md, not the nested one",
        );
        // Without root_only, both matches fire.
        let plain = build(&spec_yaml(
            "id: t\nkind: file_absent\npaths: \"**/notes.md\"\nlevel: error\n",
        ))
        .unwrap();
        assert_eq!(
            plain
                .evaluate(&ctx(Path::new("/fake"), &idx))
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn build_accepts_root_only_and_rejects_unknown_option() {
        assert!(
            build(&spec_yaml(
                "id: t\nkind: file_absent\npaths: [\"x\"]\nlevel: error\nroot_only: true\n",
            ))
            .is_ok()
        );
        assert!(
            build(&spec_yaml(
                "id: t\nkind: file_absent\npaths: [\"x\"]\nlevel: error\nbogus: 1\n",
            ))
            .is_err()
        );
    }

    // --- content_prefix_hex: the macOS-junk signature gate -------------
    //
    // Byte signatures under test:
    //   AppleDouble (`._*`)  : 00 05 16 07            -> real macOS junk
    //   .DS_Store            : 00 00 00 01 "Bud1"     -> real macOS junk
    //   Hadoop `._*.crc`     : "crc\0" (63 72 63 00)  -> NOT junk (the bug)

    /// The canonical macOS-junk spec: both `DS_Store`/`AppleDouble` name patterns,
    /// gated on both signatures. Mirrors `hygiene-no-macos-junk` in the bundled
    /// ruleset, so these tests double as a regression guard for it.
    fn macos_junk_spec() -> RuleSpec {
        spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: [\"**/.DS_Store\", \"**/._*\"]\n\
             level: error\n\
             content_prefix_hex: [\"00051607\", \"0000000142756431\"]\n",
        )
    }

    #[test]
    fn content_gate_flags_real_appledouble() {
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            "src/._foo.txt",
            b"\x00\x05\x16\x07\x00\x02\x00\x00Mac OS X        \x00\x02".as_slice(),
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "real AppleDouble sidecar must fire: {v:?}");
    }

    #[test]
    fn content_gate_ignores_hadoop_crc() {
        // The regression under fix: `._SUCCESS.crc` matches `**/._*` by name
        // but is a Hadoop checksum file ("crc\0…"), not a macOS AppleDouble
        // sidecar, so it must not be flagged as macOS junk.
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, idx) =
            tempdir_with_files(&[("data/._SUCCESS.crc", b"crc\x00\x00\x00\x02\x00".as_slice())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(
            v.is_empty(),
            "Hadoop ._*.crc checksum must NOT be treated as macOS junk: {v:?}"
        );
    }

    #[test]
    fn content_gate_flags_real_ds_store() {
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(
            ".DS_Store",
            b"\x00\x00\x00\x01Bud1\x00\x00\x00\x00".as_slice(),
        )]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "real .DS_Store (Bud1 magic) must fire: {v:?}");
    }

    #[test]
    fn content_gate_ignores_non_junk_dot_underscore() {
        // A user file named `._notes` holding ordinary text is not macOS junk.
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, idx) = tempdir_with_files(&[("._notes", b"just some text\n".as_slice())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "non-junk ._notes must not fire: {v:?}");
    }

    #[test]
    fn content_gate_ignores_empty_and_short_files() {
        // An empty file, and a file shorter than every signature, cannot begin
        // with a 4+ byte magic, so neither fires.
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, idx) = tempdir_with_files(&[
            ("._empty", b"".as_slice()),
            ("._short", b"\x00\x05".as_slice()),
        ]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(v.is_empty(), "empty/short ._ files must not fire: {v:?}");
    }

    #[test]
    fn content_gate_ignores_unverifiable_missing_file() {
        // A path in the index with no file on disk cannot be verified; the
        // conservative gate treats the read error as a non-match.
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, _real) = tempdir_with_files(&[("real.txt", b"x".as_slice())]);
        let idx = index(&["._ghost"]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert!(
            v.is_empty(),
            "unverifiable/missing ._ file must not fire: {v:?}"
        );
    }

    #[test]
    fn content_gate_matches_any_of_multiple_signatures() {
        // Two signatures configured; a file matching the SECOND (Bud1) fires,
        // proving the gate is an OR over signatures, not just the first.
        let rule = build(&macos_junk_spec()).unwrap();
        let (tmp, idx) =
            tempdir_with_files(&[("nested/.DS_Store", b"\x00\x00\x00\x01Bud1----".as_slice())]);
        let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "second signature (Bud1) must also match: {v:?}");
    }

    #[test]
    fn without_content_gate_matches_by_name_only() {
        // Backward-compat: no content_prefix_hex => pure name match, no read.
        // The Hadoop CRC file (a false positive under name-only matching) still
        // fires here — exactly the behaviour the signature gate exists to fix.
        let rule = build(&spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: [\"**/._*\"]\n\
             level: error\n",
        ))
        .unwrap();
        let idx = index(&["data/._SUCCESS.crc", "src/._foo"]);
        let v = rule.evaluate(&ctx(Path::new("/fake"), &idx)).unwrap();
        assert_eq!(v.len(), 2, "name-only matching is unchanged: {v:?}");
    }

    #[test]
    fn build_rejects_malformed_content_prefix_hex() {
        for bad in ["0005160", "zz", "00 05", ""] {
            let spec = spec_yaml(&format!(
                "id: t\n\
                 kind: file_absent\n\
                 paths: [\"**/._*\"]\n\
                 level: error\n\
                 content_prefix_hex: [\"{bad}\"]\n",
            ));
            assert!(
                build(&spec).is_err(),
                "malformed content_prefix_hex {bad:?} must be rejected at build",
            );
        }
    }

    #[test]
    fn content_gate_composes_with_fixer() {
        // The file_remove fixer still attaches; only confirmed-junk files reach
        // it, so `alint fix` deletes real AppleDouble data, never `._*.crc`.
        let rule = build(&spec_yaml(
            "id: t\n\
             kind: file_absent\n\
             paths: [\"**/._*\"]\n\
             level: error\n\
             content_prefix_hex: [\"00051607\"]\n\
             fix:\n  \
               file_remove: {}\n",
        ))
        .unwrap();
        assert!(
            rule.fixer().is_some(),
            "fixer must still attach with the gate"
        );
    }
}
