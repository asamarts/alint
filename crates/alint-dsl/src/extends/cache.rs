//! Disk cache for remote `extends:` bodies.
//!
//! Cache path: `<user-cache-dir>/alint/rulesets/<sri>.yml`.
//! The SRI is both the cache key and the integrity guarantee —
//! collisions imply a broken hash, not a data race.
//! Writes are atomic (temp file + rename) so a crashed partial
//! write doesn't leave a corrupt entry behind.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::sri::Sri;

/// Distinguishes concurrent temp files within this process (the PID
/// distinguishes across processes). See [`Cache::put`] (L5).
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn non_empty_abs_path(val: Option<OsString>, is_abs: fn(&Path) -> bool) -> Option<PathBuf> {
    let raw = val?;
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    is_abs(&path).then_some(path)
}

fn resolve_unix_cache_root(
    get_env: impl Fn(&str) -> Option<OsString>,
    is_abs: fn(&Path) -> bool,
) -> Option<PathBuf> {
    // Per the XDG Base Directory Specification, a relative or empty
    // XDG_CACHE_HOME must be ignored in favor of `$HOME/.cache`.
    if let Some(xdg) = non_empty_abs_path(get_env("XDG_CACHE_HOME"), is_abs) {
        return Some(xdg.join("alint").join("rulesets"));
    }
    let home = non_empty_abs_path(get_env("HOME"), is_abs)?;
    Some(home.join(".cache").join("alint").join("rulesets"))
}

fn resolve_macos_cache_root(
    get_env: impl Fn(&str) -> Option<OsString>,
    is_abs: fn(&Path) -> bool,
) -> Option<PathBuf> {
    let home = non_empty_abs_path(get_env("HOME"), is_abs)?;
    Some(
        home.join("Library")
            .join("Caches")
            .join("org.asamarts.alint")
            .join("rulesets"),
    )
}

fn resolve_windows_cache_root(
    get_env: impl Fn(&str) -> Option<OsString>,
    is_abs: fn(&Path) -> bool,
) -> Option<PathBuf> {
    let local_app_data = non_empty_abs_path(get_env("LOCALAPPDATA"), is_abs).or_else(|| {
        non_empty_abs_path(get_env("USERPROFILE"), is_abs)
            .map(|home| home.join("AppData").join("Local"))
    })?;
    Some(
        local_app_data
            .join("asamarts")
            .join("alint")
            .join("cache")
            .join("rulesets"),
    )
}

fn user_cache_root() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        resolve_macos_cache_root(|k| std::env::var_os(k), Path::is_absolute)
    } else if cfg!(target_os = "windows") {
        resolve_windows_cache_root(|k| std::env::var_os(k), Path::is_absolute)
    } else {
        resolve_unix_cache_root(|k| std::env::var_os(k), Path::is_absolute)
    }
}

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
        let root = user_cache_root().ok_or(CacheError::NoCacheDir)?;
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
        // Unique temp name (PID + atomic counter): two concurrent `alint` runs
        // caching the same SRI must not write — and rename — the same fixed
        // `<sri>.yml.tmp` underneath each other (L5). The content is identical
        // (SRI-verified above), so the race was never corrupting, but a unique
        // temp keeps each writer's rename atomic and independent.
        let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp_path = self.root.join(format!(
            "{}.{}.{n}.yml.tmp",
            sri.encoded(),
            std::process::id()
        ));
        if let Err(source) = std::fs::write(&tmp_path, bytes) {
            return Err(CacheError::Io {
                path: tmp_path,
                source,
            });
        }
        if let Err(source) = std::fs::rename(&tmp_path, &final_path) {
            // Don't leave our uniquely-named temp behind on a failed rename.
            let _ = std::fs::remove_file(&tmp_path);
            return Err(CacheError::Io {
                path: final_path,
                source,
            });
        }
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

    fn env_map<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    }

    fn unix_is_abs(p: &Path) -> bool {
        p.to_str().is_some_and(|s| s.starts_with('/'))
    }

    fn windows_is_abs(p: &Path) -> bool {
        let Some(s) = p.to_str() else {
            return false;
        };
        let bytes = s.as_bytes();
        (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
            || s.starts_with(r"\\")
    }

    #[test]
    fn unix_cache_root_follows_xdg_spec_and_falls_back_to_home() {
        assert_eq!(
            resolve_unix_cache_root(
                env_map(&[("XDG_CACHE_HOME", "/var/cache/me"), ("HOME", "/home/alice")]),
                unix_is_abs,
            ),
            Some(
                PathBuf::from("/var/cache/me")
                    .join("alint")
                    .join("rulesets")
            )
        );
        // XDG_CACHE_HOME alone works even when HOME is unset.
        assert_eq!(
            resolve_unix_cache_root(env_map(&[("XDG_CACHE_HOME", "/tmp/cache")]), unix_is_abs),
            Some(PathBuf::from("/tmp/cache").join("alint").join("rulesets"))
        );
        // Relative or empty XDG_CACHE_HOME is ignored per XDG spec.
        for bad_xdg in ["", "relative/cache", "./cache"] {
            assert_eq!(
                resolve_unix_cache_root(
                    env_map(&[("XDG_CACHE_HOME", bad_xdg), ("HOME", "/home/alice")]),
                    unix_is_abs,
                ),
                Some(
                    PathBuf::from("/home/alice")
                        .join(".cache")
                        .join("alint")
                        .join("rulesets")
                ),
                "bad XDG_CACHE_HOME={bad_xdg:?}"
            );
        }
        // Missing or relative HOME yields None.
        assert_eq!(resolve_unix_cache_root(env_map(&[]), unix_is_abs), None);
        assert_eq!(
            resolve_unix_cache_root(env_map(&[("HOME", "relative")]), unix_is_abs),
            None
        );
    }

    #[test]
    fn macos_cache_root_uses_library_caches_bundle_id() {
        assert_eq!(
            resolve_macos_cache_root(env_map(&[("HOME", "/Users/alice")]), unix_is_abs),
            Some(
                PathBuf::from("/Users/alice")
                    .join("Library")
                    .join("Caches")
                    .join("org.asamarts.alint")
                    .join("rulesets")
            )
        );
        assert_eq!(resolve_macos_cache_root(env_map(&[]), unix_is_abs), None);
        assert_eq!(
            resolve_macos_cache_root(env_map(&[("HOME", "")]), unix_is_abs),
            None
        );
        assert_eq!(
            resolve_macos_cache_root(env_map(&[("HOME", "Users/alice")]), unix_is_abs),
            None
        );
    }

    #[test]
    fn windows_cache_root_uses_localappdata_with_userprofile_fallback() {
        let local = r"C:\Users\alice\AppData\Local";
        assert_eq!(
            resolve_windows_cache_root(env_map(&[("LOCALAPPDATA", local)]), windows_is_abs),
            Some(
                PathBuf::from(local)
                    .join("asamarts")
                    .join("alint")
                    .join("cache")
                    .join("rulesets")
            )
        );
        // Relative or empty LOCALAPPDATA falls back to USERPROFILE\AppData\Local.
        let profile = r"C:\Users\alice";
        for bad_local in ["", r"AppData\Local"] {
            assert_eq!(
                resolve_windows_cache_root(
                    env_map(&[("LOCALAPPDATA", bad_local), ("USERPROFILE", profile)]),
                    windows_is_abs,
                ),
                Some(
                    PathBuf::from(profile)
                        .join("AppData")
                        .join("Local")
                        .join("asamarts")
                        .join("alint")
                        .join("cache")
                        .join("rulesets")
                ),
                "bad LOCALAPPDATA={bad_local:?}"
            );
        }
        assert_eq!(
            resolve_windows_cache_root(env_map(&[]), windows_is_abs),
            None
        );
        assert_eq!(
            resolve_windows_cache_root(env_map(&[("USERPROFILE", "relative")]), windows_is_abs),
            None
        );
    }
}
