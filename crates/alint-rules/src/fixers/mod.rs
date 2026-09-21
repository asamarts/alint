//! Shared [`Fixer`](alint_core::Fixer) implementations, grouped by family.
//!
//! Each fixer is a small, rule-agnostic helper: rule builders (e.g.
//! `file_exists`, `file_absent`) decide whether the configured `fix:`
//! op makes sense for their kind and, if so, construct one of the
//! fixers here and attach it to the built rule.
//!
//! Families:
//! - [`creators`] — file-creating + content-prepending/appending
//!   (`FileCreateFixer`, `FilePrependFixer`, `FileAppendFixer`).
//! - [`file_ops`] — file CRUD + mode (`FileRemoveFixer`, `FileRenameFixer`,
//!   `ChmodFixer`).
//! - [`git_ops`] — git-index ops (`GitUntrackFixer`); the first *spawning* fixer
//!   (shells out to `git rm --cached`), so it is top-level-only trust-gated.
//! - [`hygiene`] — text-level cleanup (`FileTrimTrailingWhitespaceFixer`,
//!   `FileAppendFinalNewlineFixer`, `FileNormalizeLineEndingsFixer`,
//!   `FileCollapseBlankLinesFixer`).
//! - [`strip`] — byte-stripping (`FileStripBidiFixer`,
//!   `FileStripZeroWidthFixer`, `FileStripBomFixer`).
//! - [`replace`] — the located regex `replace` op (`ReplaceFixer`), emitting
//!   byte-range edits rather than a whole-file rewrite.
//! - [`structured`] — the located structured `set_value` / `remove_value` ops
//!   (`StructuredFixer`), splicing a value/removal span located via a
//!   span-resolving parser (Phase 2).

pub mod creators;
pub mod file_ops;
pub mod git_ops;
pub mod hygiene;
pub mod replace;
pub mod strip;
pub mod structured;

pub use creators::{FileAppendFixer, FileCreateFixer, FilePrependFixer};
pub use file_ops::{ChmodFixer, FileRemoveFixer, FileRenameFixer};
pub use git_ops::GitUntrackFixer;
pub(crate) use hygiene::line_is_blank;
pub use hygiene::{
    FileAppendFinalNewlineFixer, FileCollapseBlankLinesFixer, FileNormalizeLineEndingsFixer,
    FileTrimTrailingWhitespaceFixer, LineEndingTarget,
};
pub use replace::ReplaceFixer;
pub use strip::{FileStripBidiFixer, FileStripBomFixer, FileStripZeroWidthFixer};
pub use structured::StructuredFixer;
