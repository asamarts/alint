//! R-KANI regression guard. The weekly Kani job runs `cargo kani -p
//! alint-core`; the original bug (R-KANI) pointed it at `-p alint-rules`, which
//! has zero `#[kani::proof]` harnesses, so the job verified nothing and passed
//! vacuously. The `-p` arg is fixed, but nothing stopped the *proofs* from being
//! deleted and the job silently going vacuous again.
//!
//! This runs on the normal `cargo test` path (it only reads source), so a
//! dropped proof reds PR CI immediately instead of the weekly-only Kani job.
//! It asserts alint-core carries at least the known harnesses; adding more never
//! reds it, deleting one does.

use std::path::{Path, PathBuf};

/// The proof harnesses expected to exist. Deleting one (or renaming it without
/// updating the Kani workflow) must be a conscious change, so it reds here.
const EXPECTED_HARNESSES: &[&str] = &[
    "fn confine_steps_is_sound",
    "fn overlap_skip_accepts_a_pairwise_disjoint_set",
];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The concatenated source of this crate's `src/` tree.
fn all_src() -> String {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn alint_core_carries_kani_proofs() {
    let src = all_src();
    let count = src.matches("#[kani::proof]").count();
    assert!(
        count >= 1,
        "alint-core must carry at least one `#[kani::proof]` harness. The Kani \
         job runs `cargo kani -p alint-core`; with zero proofs it passes \
         vacuously (that was R-KANI). Do not remove the last proof."
    );
    assert!(
        count >= EXPECTED_HARNESSES.len(),
        "expected at least {} proof harnesses, found {count} `#[kani::proof]` \
         attributes in alint-core/src",
        EXPECTED_HARNESSES.len(),
    );
}

#[test]
fn known_kani_harnesses_are_not_deleted() {
    let src = all_src();
    for harness in EXPECTED_HARNESSES {
        assert!(
            src.contains(harness),
            "the Kani harness `{harness}` is missing from alint-core/src. If a \
             proof was intentionally renamed or removed, update the Kani \
             workflow (.github/workflows/kani.yml) and EXPECTED_HARNESSES here."
        );
    }
}
