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
//! - [`file_ops`] — file CRUD (`FileRemoveFixer`, `FileRenameFixer`).
//! - [`hygiene`] — text-level cleanup (`FileTrimTrailingWhitespaceFixer`,
//!   `FileAppendFinalNewlineFixer`, `FileNormalizeLineEndingsFixer`,
//!   `FileCollapseBlankLinesFixer`).
//! - [`strip`] — byte-stripping (`FileStripBidiFixer`,
//!   `FileStripZeroWidthFixer`, `FileStripBomFixer`).
//! - [`replace`] — the located regex `replace` op (`ReplaceFixer`), emitting
//!   byte-range edits rather than a whole-file rewrite.

pub mod creators;
pub mod file_ops;
pub mod hygiene;
pub mod replace;
pub mod strip;

pub use creators::{FileAppendFixer, FileCreateFixer, FilePrependFixer};
pub use file_ops::{FileRemoveFixer, FileRenameFixer};
pub(crate) use hygiene::line_is_blank;
pub use hygiene::{
    FileAppendFinalNewlineFixer, FileCollapseBlankLinesFixer, FileNormalizeLineEndingsFixer,
    FileTrimTrailingWhitespaceFixer, LineEndingTarget,
};
pub use replace::ReplaceFixer;
pub use strip::{FileStripBidiFixer, FileStripBomFixer, FileStripZeroWidthFixer};
