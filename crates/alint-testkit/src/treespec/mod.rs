//! Declarative filesystem-tree specs: parse, materialize, verify,
//! extract. Deliberately kept free of any alint-specific types so
//! this module can be spun out as a standalone crate without
//! refactoring. Format documented in `TREE_SPEC.md` at the crate root.

mod extract;
mod materialize;
mod spec;
mod verify;

pub use extract::{ExtractOpts, extract};
pub use materialize::materialize;
pub use spec::{TreeNode, TreeSpec, TreeSpecIter};
pub use verify::{Discrepancy, VerifyMode, VerifyReport, verify};
