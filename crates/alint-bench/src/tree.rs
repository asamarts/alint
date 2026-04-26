//! Deterministic synthetic repository-tree generator.
//!
//! Two shapes:
//!
//! - [`generate_tree`] — a flat random tree of `files` files distributed
//!   across directories up to `depth` deep. Used by the v0.1 macro-bench.
//! - [`generate_monorepo`] — a Cargo-workspace-shaped tree with a root
//!   `Cargo.toml`, `crates/<pkg-NNN>/{Cargo.toml,README.md,src/*.rs}`
//!   per package. Used by the v0.5 scale-ceiling bench because the
//!   bundled workspace rulesets need real workspace shape (`[workspace]`,
//!   `[package].name`, members list) to fire meaningfully.
//!
//! The generator is seeded so every `(args..., seed)` triple produces
//! byte-identical names, directory structure, and file content across
//! platforms and invocations. This lets us publish reproducible benchmark
//! numbers without shipping or downloading a real-world corpus.

use std::io;
use std::path::{Path, PathBuf};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use tempfile::TempDir;

const EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "md", "yaml", "yml", "json", "txt", "py"];

/// A generated tree rooted under a tempdir that is cleaned up on drop.
#[derive(Debug)]
pub struct Tree {
    dir: TempDir,
    /// Relative paths of every generated file, in generation order.
    pub files: Vec<PathBuf>,
}

impl Tree {
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn into_persistent(self) -> io::Result<PathBuf> {
        Ok(self.dir.keep())
    }
}

/// Generate `files` files distributed across directories up to `depth` deep,
/// under a fresh tempdir. Filenames, extensions, content, and nesting are
/// all derived from the seeded PRNG.
pub fn generate_tree(files: usize, depth: usize, seed: u64) -> io::Result<Tree> {
    let dir = TempDir::new()?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut file_list = Vec::with_capacity(files);

    for i in 0..files {
        let d = rng.random_range(0..=depth);
        let mut rel = PathBuf::new();
        for level in 0..d {
            let bucket = rng.random_range(0u32..6);
            rel.push(format!("dir_{level}_{bucket}"));
        }
        let ext = EXTENSIONS[rng.random_range(0..EXTENSIONS.len())];
        rel.push(format!("file_{i:06}.{ext}"));

        let abs = dir.path().join(&rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let size = rng.random_range(256usize..4096);
        let content = lorem_bytes(size, &mut rng);
        std::fs::write(&abs, content)?;
        file_list.push(rel);
    }

    Ok(Tree {
        dir,
        files: file_list,
    })
}

/// Cargo-workspace-shaped tree: root `Cargo.toml` declaring a
/// workspace, plus `packages` per-crate directories under
/// `crates/`, each with a `Cargo.toml` (declaring a `[package]`
/// table with a unique `name`), a `README.md`, and
/// `files_per_package` `*.rs` source files under `src/`.
///
/// File counts: the resulting tree contains
/// `1 + packages * (2 + files_per_package)` files total
/// (workspace root + per-package `Cargo.toml` + `README.md` +
/// the source files), which the v0.5 bench harness uses to hit
/// rounded targets like 1k / 10k / 100k.
///
/// Real workspace + package shape is necessary so the
/// `monorepo/cargo-workspace@v1` bundled ruleset's
/// fact-gated rules (`facts.is_cargo_workspace`) fire and the
/// `toml_path_matches` rules see well-formed manifests.
pub fn generate_monorepo(packages: usize, files_per_package: usize, seed: u64) -> io::Result<Tree> {
    let dir = TempDir::new()?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut file_list = Vec::with_capacity(1 + packages * (2 + files_per_package));

    // Root workspace manifest. Members listed by glob so the
    // `cargo-workspace-members-declared` rule (toml path
    // `$.workspace.members[*]`) matches at least one entry.
    let workspace_toml = "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n";
    std::fs::write(dir.path().join("Cargo.toml"), workspace_toml)?;
    file_list.push(PathBuf::from("Cargo.toml"));

    for pkg_idx in 0..packages {
        let pkg_name = format!("pkg-{pkg_idx:06}");
        let pkg_rel = PathBuf::from("crates").join(&pkg_name);
        let pkg_abs = dir.path().join(&pkg_rel);
        std::fs::create_dir_all(&pkg_abs)?;
        std::fs::create_dir_all(pkg_abs.join("src"))?;

        let manifest =
            format!("[package]\nname = \"{pkg_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
        std::fs::write(pkg_abs.join("Cargo.toml"), manifest)?;
        file_list.push(pkg_rel.join("Cargo.toml"));

        std::fs::write(
            pkg_abs.join("README.md"),
            format!("# {pkg_name}\n\nSynthetic crate generated for benchmarking.\n"),
        )?;
        file_list.push(pkg_rel.join("README.md"));

        for file_idx in 0..files_per_package {
            let stem = if file_idx == 0 {
                "lib".to_string()
            } else {
                format!("mod_{file_idx:04}")
            };
            let rel = pkg_rel.join("src").join(format!("{stem}.rs"));
            let size = rng.random_range(256usize..2048);
            let content = lorem_bytes(size, &mut rng);
            std::fs::write(dir.path().join(&rel), content)?;
            file_list.push(rel);
        }
    }

    Ok(Tree {
        dir,
        files: file_list,
    })
}

/// Pick a deterministic subset of a tree's files to "touch" — used
/// by the `--changed`-mode bench harness to produce a working-tree
/// diff of a known size without running git mutations from inside
/// the generator. Caller iterates the returned paths and writes
/// new content to each, then runs `git status` etc. to confirm.
///
/// `fraction` is in `[0.0, 1.0]`; out-of-range values are clamped.
/// `seed` is independent of the tree-generation seed so the same
/// tree can produce 1% / 10% / 100% subsets reproducibly across
/// invocations.
pub fn select_subset(files: &[PathBuf], fraction: f64, seed: u64) -> Vec<&PathBuf> {
    let fraction = fraction.clamp(0.0, 1.0);
    // Use integer permille arithmetic to dodge clippy's f64
    // precision warnings — we don't need decimal accuracy
    // here, only "what fraction of N rounds to which usize."
    // 1_000_000 permille granularity is more than enough for
    // bench fractions 0.0–1.0 at any reasonable tree size.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let permille = (fraction * 1_000_000.0).round() as u64;
    let count_64 = u64::try_from(files.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(permille)
        / 1_000_000;
    let target = usize::try_from(count_64)
        .unwrap_or(files.len())
        .min(files.len());
    if target == 0 {
        return Vec::new();
    }
    // Fisher-Yates partial shuffle: O(target) draws, deterministic
    // given the seed. Avoids materialising and shuffling the whole
    // index when target ≪ files.len().
    let mut indices: Vec<usize> = (0..files.len()).collect();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    for i in 0..target {
        let j = i + rng.random_range(0..(files.len() - i));
        indices.swap(i, j);
    }
    indices.truncate(target);
    indices.sort_unstable();
    indices.into_iter().map(|i| &files[i]).collect()
}

/// Generate `size` bytes of pseudo-English ASCII with newlines every 72
/// columns. UTF-8 valid; ideal for exercising content-regex rules without
/// hitting accidental binary-detection paths.
fn lorem_bytes(size: usize, rng: &mut impl Rng) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut line_len = 0;
    while out.len() < size {
        if line_len >= 72 {
            out.push(b'\n');
            line_len = 0;
            continue;
        }
        let c = rng.random_range(0u8..27);
        if c == 26 {
            out.push(b' ');
        } else {
            out.push(b'a' + c);
        }
        line_len += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn deterministic_for_same_seed() {
        let t1 = generate_tree(50, 3, 42).unwrap();
        let t2 = generate_tree(50, 3, 42).unwrap();
        let files1: BTreeSet<_> = t1.files.iter().collect();
        let files2: BTreeSet<_> = t2.files.iter().collect();
        assert_eq!(files1, files2);
        for rel in &t1.files {
            let a = std::fs::read(t1.root().join(rel)).unwrap();
            let b = std::fs::read(t2.root().join(rel)).unwrap();
            assert_eq!(a, b, "file {rel:?} differs across runs");
        }
    }

    #[test]
    fn produces_requested_file_count() {
        let t = generate_tree(25, 2, 1).unwrap();
        assert_eq!(t.files.len(), 25);
        for rel in &t.files {
            assert!(t.root().join(rel).is_file());
        }
    }

    #[test]
    fn honors_max_depth() {
        let t = generate_tree(100, 3, 7).unwrap();
        for rel in &t.files {
            // Components = directory segments + 1 for the filename.
            assert!(rel.components().count() <= 4, "{rel:?}");
        }
    }

    // ─── monorepo shape ──────────────────────────────────────────

    #[test]
    fn monorepo_has_root_workspace_manifest() {
        let t = generate_monorepo(3, 4, 42).unwrap();
        let manifest =
            std::fs::read_to_string(t.root().join("Cargo.toml")).expect("workspace Cargo.toml");
        assert!(manifest.contains("[workspace]"));
        assert!(manifest.contains("members"));
    }

    #[test]
    fn monorepo_emits_expected_file_count() {
        // Per-package: 1 Cargo.toml + 1 README + N source files.
        // Plus 1 root Cargo.toml.
        let pkgs = 5;
        let files_per = 3;
        let t = generate_monorepo(pkgs, files_per, 1).unwrap();
        let expected = 1 + pkgs * (2 + files_per);
        assert_eq!(t.files.len(), expected);
        for rel in &t.files {
            assert!(t.root().join(rel).is_file(), "{rel:?} missing");
        }
    }

    #[test]
    fn monorepo_per_package_manifest_declares_unique_name() {
        let t = generate_monorepo(4, 1, 7).unwrap();
        let mut names: BTreeSet<String> = BTreeSet::new();
        for pkg_idx in 0..4 {
            let pkg = format!("pkg-{pkg_idx:06}");
            let manifest =
                std::fs::read_to_string(t.root().join("crates").join(&pkg).join("Cargo.toml"))
                    .unwrap();
            assert!(manifest.contains(&format!("name = \"{pkg}\"")));
            names.insert(pkg);
        }
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn monorepo_deterministic_for_same_seed() {
        let a = generate_monorepo(3, 4, 99).unwrap();
        let b = generate_monorepo(3, 4, 99).unwrap();
        let af: BTreeSet<_> = a.files.iter().collect();
        let bf: BTreeSet<_> = b.files.iter().collect();
        assert_eq!(af, bf);
        for rel in &a.files {
            let av = std::fs::read(a.root().join(rel)).unwrap();
            let bv = std::fs::read(b.root().join(rel)).unwrap();
            assert_eq!(av, bv, "{rel:?} differs across runs");
        }
    }

    // ─── select_subset ───────────────────────────────────────────

    #[test]
    fn select_subset_zero_fraction_is_empty() {
        let files: Vec<PathBuf> = (0..100).map(|i| PathBuf::from(format!("f{i}"))).collect();
        assert!(select_subset(&files, 0.0, 1).is_empty());
    }

    #[test]
    fn select_subset_full_fraction_returns_everything() {
        let files: Vec<PathBuf> = (0..100).map(|i| PathBuf::from(format!("f{i}"))).collect();
        let picked = select_subset(&files, 1.0, 1);
        assert_eq!(picked.len(), 100);
    }

    #[test]
    fn select_subset_respects_fraction_size() {
        let files: Vec<PathBuf> = (0..100).map(|i| PathBuf::from(format!("f{i}"))).collect();
        assert_eq!(select_subset(&files, 0.10, 1).len(), 10);
        assert_eq!(select_subset(&files, 0.50, 1).len(), 50);
    }

    #[test]
    fn select_subset_deterministic_for_same_seed() {
        let files: Vec<PathBuf> = (0..200).map(|i| PathBuf::from(format!("f{i}"))).collect();
        let a = select_subset(&files, 0.10, 7);
        let b = select_subset(&files, 0.10, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn select_subset_different_seeds_pick_different_files() {
        let files: Vec<PathBuf> = (0..200).map(|i| PathBuf::from(format!("f{i}"))).collect();
        let a = select_subset(&files, 0.10, 7);
        let b = select_subset(&files, 0.10, 8);
        assert_ne!(a, b);
    }

    #[test]
    fn select_subset_results_are_sorted() {
        let files: Vec<PathBuf> = (0..50).map(|i| PathBuf::from(format!("f{i:03}"))).collect();
        let picked = select_subset(&files, 0.30, 11);
        let owned: Vec<&PathBuf> = picked.clone();
        let mut sorted = owned.clone();
        sorted.sort();
        assert_eq!(picked, sorted);
    }

    #[test]
    fn select_subset_clamps_out_of_range_fractions() {
        let files: Vec<PathBuf> = (0..10).map(|i| PathBuf::from(format!("f{i}"))).collect();
        // > 1.0 clamps to 1.0
        assert_eq!(select_subset(&files, 5.0, 1).len(), 10);
        // < 0.0 clamps to 0.0
        assert!(select_subset(&files, -0.5, 1).is_empty());
    }
}
