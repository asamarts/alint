//! What a fixer proposes, as data: the outcome of applying a fix, the edit
//! it describes, how safe the edit is to apply automatically, and how the
//! engine verifies it. Split from `rule.rs`; re-exported from there.

use std::path::PathBuf;

/// The result of applying (or simulating) one fix against one violation.
#[derive(Debug, Clone)]
pub enum FixOutcome {
    /// The fix was applied (or would be, under `dry_run`). The string
    /// is a human-readable one-liner — e.g. `"created LICENSE"`,
    /// `"would remove target/debug.log"`.
    Applied(String),
    /// The fixer intentionally did nothing; the string explains why
    /// (e.g. `"already exists"`, `"no path on violation"`). This is
    /// distinct from a hard error returned via `Result::Err`.
    Skipped(String),
}

/// A proposed fix expressed as data, so a caller can turn it into an
/// editor edit (an LSP `WorkspaceEdit`) instead of writing the file.
/// Returned by [`Fixer::fix_edit`](super::Fixer::fix_edit). Paths are relative to the alint
/// root, matching [`Violation::path`](super::Violation::path).
///
/// `fix_edit` is the non-writing sibling of [`Fixer::apply`](super::Fixer::apply): `apply`
/// mutates the filesystem (for `alint fix`); `fix_edit` describes the
/// same change so the editor can apply it to the buffer (with undo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixEdit {
    /// Replace the full contents of an existing file.
    SetContent { path: PathBuf, content: Vec<u8> },
    /// Create a file that doesn't exist yet.
    CreateFile { path: PathBuf, content: Vec<u8> },
    /// Delete a file.
    DeleteFile { path: PathBuf },
    /// Rename a file (same directory or not).
    RenameFile { from: PathBuf, to: PathBuf },
    /// Replace the half-open byte range `[range.start, range.end)` of an
    /// existing file with `content`. The range is in bytes against the
    /// file's current contents. This is the workhorse of *located* edits
    /// (Phase 1+): several disjoint `ReplaceRange`s against one file can
    /// be batched, ordered, and spliced in a single pass. No Phase-0
    /// fixer emits one, but the primitive splice and the batching engine
    /// are built and tested here.
    ReplaceRange {
        path: PathBuf,
        range: std::ops::Range<usize>,
        content: Vec<u8>,
    },
    /// Set the permission bits of a file (a `chmod`). `mode` is the full
    /// mode word (e.g. `0o755`). Applied only on Unix; on other platforms the
    /// host rule never fires, so this is unreachable. Emitted by the `ChmodFixer`
    /// (the `chmod` fix op on `executable_bit` / `shebang_has_executable`); the
    /// fixer performs the `set_permissions` itself and records this in the stage
    /// sink so `fix --diff` renders the mode change.
    SetMode { path: PathBuf, mode: u32 },
}

/// How safe a fix is to apply automatically. A [`CollectedEdit`] declares
/// its tier; `alint fix` filters against the user's chosen threshold
/// (`Safe` by default, `Unsafe` with `--unsafe-fixes`).
///
/// The `Ord` is the application order: `Safe < Unsafe < Suggestion <
/// Never`, so "would this apply at `threshold`?" is `self <= threshold`
/// for the two *applying* tiers. `Suggestion` is shown but never applied;
/// `Never` is collected for provenance only and neither applied nor
/// suggested. See auto-fix.md 5.5 and ADR-0017.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Behavior-preserving; applied by a bare `alint fix`.
    Safe,
    /// May change semantics; applied only with `--unsafe-fixes`.
    Unsafe,
    /// Never applied automatically; surfaced as [`FixStatus::Suggested`](crate::FixStatus).
    Suggestion,
    /// Collected for analysis/provenance only; never applied, never
    /// suggested for automatic application.
    Never,
}

impl Applicability {
    /// Whether an edit at this tier is *applied* when the user opted into
    /// applying up to `threshold` (`Safe` for a bare `alint fix`, `Unsafe`
    /// for `--unsafe-fixes`). `Suggestion` and `Never` never apply.
    #[must_use]
    pub fn applies_at(self, threshold: Applicability) -> bool {
        matches!(self, Applicability::Safe | Applicability::Unsafe) && self <= threshold
    }

    /// Whether an edit at this tier is *suggested* (shown, not applied)
    /// under `threshold`: an `Unsafe` edit the user did not opt into, or a
    /// `Suggestion`. `Never` is never suggested; an applied tier is not
    /// "merely" suggested.
    #[must_use]
    pub fn suggested_at(self, threshold: Applicability) -> bool {
        match self {
            Applicability::Suggestion => true,
            Applicability::Unsafe => !self.applies_at(threshold),
            Applicability::Safe | Applicability::Never => false,
        }
    }
}

/// The value a structured edit's target query must hold *after* the edit
/// applies — the right-hand side of the localized-equivalence (`PutGet`)
/// check the engine runs before committing a located edit.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpectedValue {
    /// The query must resolve to exactly this scalar.
    Scalar(serde_json::Value),
    /// The query must match nothing (used by `*_path_absent` /
    /// `remove_value`: after a batched multi-node removal, array indices
    /// shift, so only re-running the query and asserting zero matches is
    /// correct).
    Absent,
    /// The query must resolve to a STRING that matches this regex (its source).
    /// Used by `*_path_matches` + `replace`: the fixer rewrites the value so it
    /// satisfies the rule's `matches:` pattern, and this re-checks that goal
    /// (stored as the source string, since `regex::Regex` is not `PartialEq`).
    Matches(String),
}

/// An executable post-edit check the engine can run *without knowing the
/// op* that produced the edit. This is the load-bearing verification
/// obligation the design's translation-validation rests on (R-VERIFY): a
/// fixer returns not just an edit but the means to prove the edit did what
/// the rule wanted, so the engine can demote an edit whose result does not
/// verify to a [`FixStatus::Suggested`](crate::FixStatus) instead of writing bad bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum EditVerifier {
    /// No semantic check (whole-file normalizers): the edit is its own
    /// specification. The engine still confirms the write succeeded.
    None,
    /// Re-parse the post-edit bytes in `format` (syntactic validity) and
    /// re-run the `JSONPath` `query` against them, asserting the result
    /// matches `expect`.
    ///
    /// `query` is the *owned* `JSONPath` source string (the rule's
    /// `path_src`), NOT a borrowed `serde_json_path::NormalizedPath`: a
    /// `NormalizedPath` borrows the parsed `Value` that drops when
    /// `collect_edits` returns (a dangling borrow), and it cannot express
    /// `Absent` after index shifts anyway. Re-running the source query is
    /// the only correct check.
    Structured {
        format: crate::structured_format::Format,
        query: String,
        expect: ExpectedValue,
    },
}

/// A tag marking edits that must not co-apply in a single pass even when
/// their byte ranges are disjoint (e.g. two rewrites of the same logical
/// node reached by different queries). The concrete grouping policy is
/// pinned when the first isolation-needing op ships (Phase 2+); Phase 0
/// carries the field and exercises exclusion with a fixture rule.
pub type GroupId = u32;

/// A single proposed edit plus everything the engine needs to decide
/// whether and how to apply it *without knowing the op that produced it*:
/// its tier, its post-edit verification obligation, and its
/// mutual-exclusion group. Returned by [`Fixer::collect_edits`](super::Fixer::collect_edits).
///
/// This refines ADR-0017 decision 1 / auto-fix.md 5.2.1, whose
/// `collect_edits -> Vec<(FixEdit, Applicability)>` cannot carry the
/// verifier the Safe acceptance test needs.
#[derive(Debug, Clone)]
pub struct CollectedEdit {
    pub edit: FixEdit,
    pub applicability: Applicability,
    pub verify: EditVerifier,
    pub isolation_group: Option<GroupId>,
}
