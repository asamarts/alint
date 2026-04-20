//! Disk cache for remote `extends:` bodies.
//!
//! Cache path: `<user-cache-dir>/alint/rulesets/<sri>.yml`.
//! The SRI is both the cache key and the integrity guarantee —
//! collisions imply a broken hash, not a data race.
//! Writes are atomic (temp file + rename) so a crashed partial
//! write doesn't leave a corrupt entry behind.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use super::sri::Sri;

/// On-disk cache rooted at `<cache-dir>/alint/rulesets/`.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    /// Cache under the user's platform-specific cache directory.
    /// Linux:   `$XDG_CACHE_HOME/alint/rulesets/` or
    ///          `$HOME/.cache/alint/rulesets/`.
    /// macOS:   `$HOME/Library/Caches/org.asamarts.alint/rulesets/`.
    /// Windows: `%LOCALAPPDATA%\asamarts\alint\cache\rulesets\`.
    pub fn user_default() -> Result<Self, CacheError> {
        let dirs = ProjectDirs::from("org", "asamarts", "alint").ok_or(CacheError::NoCacheDir)?;
        let root = dirs.cache_dir().join("rulesets");
        Ok(Self { root })
    }

    /// Construct with an explicit root (used by tests and by
    /// anyone that wants to pin the cache location).
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path where the body for `sri` is (or will be) stored.
    pub fn entry_path(&self, sri: &Sri) -> PathBuf {
        self.root.join(format!("{}.yml", sri.encoded()))
    }

    /// Read the cached body for `sri`. Returns `Ok(None)` when
    /// the cache miss is clean (file absent). The body is
    /// re-verified against `sri` so a poisoned on-disk entry
    /// fails loudly instead of returning bad content.
    pub fn get(&self, sri: &Sri) -> Result<Option<Vec<u8>>, CacheError> {
        let path = self.entry_path(sri);
        match std::fs::read(&path) {
            Ok(bytes) => {
                sri.verify(&bytes).map_err(|e| CacheError::CorruptEntry {
                    path: path.clone(),
                    reason: e.to_string(),
                })?;
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(CacheError::Io { path, source }),
        }
    }

    /// Write `bytes` under `sri`. Caller must have already
    /// verified `sri.verify(bytes)`; this method double-checks
    /// defensively and refuses to persist a mismatch.
    ///
    /// Write is atomic: bytes land in a sibling temp file first
    /// and are renamed into place.
    pub fn put(&self, sri: &Sri, bytes: &[u8]) -> Result<(), CacheError> {
        sri.verify(bytes).map_err(|e| CacheError::CorruptEntry {
            path: self.entry_path(sri),
            reason: format!("refusing to cache content that fails its own SRI: {e}"),
        })?;
        std::fs::create_dir_all(&self.root).map_err(|source| CacheError::Io {
            path: self.root.clone(),
            source,
        })?;
        let final_path = self.entry_path(sri);
        let tmp_path = final_path.with_extension("yml.tmp");
        std::fs::write(&tmp_path, bytes).map_err(|source| CacheError::Io {
            path: tmp_path.clone(),
            source,
        })?;
        std::fs::rename(&tmp_path, &final_path).map_err(|source| CacheError::Io {
            path: final_path,
            source,
        })?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not resolve a user cache directory on this platform")]
    NoCacheDir,
    #[error("cache I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cached entry at {path} is corrupt: {reason}")]
    CorruptEntry { path: PathBuf, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extends::sri::Sri;
    use tempfile::TempDir;

    const EMPTY_SHA256: &str =
        "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn fresh_cache() -> (TempDir, Cache) {
        let tmp = TempDir::new().unwrap();
        let cache = Cache::at(tmp.path().join("cache"));
        (tmp, cache)
    }

    #[test]
    fn miss_returns_none() {
        let (_tmp, cache) = fresh_cache();
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        assert!(cache.get(&sri).unwrap().is_none());
    }

    #[test]
    fn put_then_get_round_trips() {
        let (_tmp, cache) = fresh_cache();
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        cache.put(&sri, b"").unwrap();
        let got = cache.get(&sri).unwrap().unwrap();
        assert_eq!(got, b"");
    }

    #[test]
    fn put_refuses_content_that_fails_sri() {
        let (_tmp, cache) = fresh_cache();
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        let err = cache.put(&sri, b"not empty").unwrap_err();
        assert!(matches!(err, CacheError::CorruptEntry { .. }), "{err}");
    }

    #[test]
    fn get_detects_poisoned_entry() {
        let (_tmp, cache) = fresh_cache();
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        // Bypass `put` to simulate disk corruption / tampering.
        std::fs::create_dir_all(cache.root()).unwrap();
        std::fs::write(cache.entry_path(&sri), b"tampered").unwrap();
        let err = cache.get(&sri).unwrap_err();
        assert!(matches!(err, CacheError::CorruptEntry { .. }), "{err}");
    }

    #[test]
    fn entry_path_uses_sri_as_filename() {
        let (_tmp, cache) = fresh_cache();
        let sri = Sri::parse(EMPTY_SHA256).unwrap();
        let p = cache.entry_path(&sri);
        assert_eq!(
            p.file_name().and_then(|s| s.to_str()),
            Some("sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.yml")
        );
    }
}
