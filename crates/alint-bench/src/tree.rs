//! Deterministic synthetic repository-tree generator.
//!
//! The generator is seeded so every `(files, depth, seed)` triple produces
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
}
