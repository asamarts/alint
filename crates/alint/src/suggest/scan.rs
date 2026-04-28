//! `Scan` — cached repo state shared across suggesters.
//!
//! Built once at the start of `suggest::run` so each suggester
//! family doesn't re-walk, re-detect, or re-parse the user's
//! config. The walker pass is the most expensive piece on a big
//! repo; folding it through here means O(1) walks per `suggest`
//! invocation regardless of how many suggesters consume the
//! file index.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use alint_core::{FileIndex, WalkOptions, walk};

use crate::init::Detection;
use crate::progress::Progress;

use super::proposal::{Proposal, ProposalKind};

/// All the repo-state every suggester might want, computed
/// once.
#[derive(Debug)]
pub struct Scan {
    pub root: PathBuf,
    /// Walker output, honoring `.gitignore` and the default
    /// excludes. Suggesters scan the file index in their own
    /// passes (a regex over each file's bytes for the
    /// antipattern suggester, a path-shape match for the
    /// scratch-doc detector, etc.).
    pub index: FileIndex,
    /// Ecosystem + workspace detection lifted from
    /// `alint init`. High-confidence bundled-ruleset
    /// suggestions key off this.
    pub detection: Detection,
    /// Extends URIs already declared in the user's
    /// `.alint.yml` (when present). Used for already-covered
    /// detection.
    extends_uris: Vec<String>,
    /// Whether we're inside a git repo. The stale-TODO
    /// suggester silently no-ops when this is `false`.
    pub has_git: bool,
}

impl Scan {
    /// Build the cached scan. Walks the repo (per a basic
    /// `WalkOptions`), detects ecosystem, parses the user's
    /// existing config (best-effort), and probes git.
    pub fn collect(root: &Path, progress: &Progress) -> Result<Self> {
        let phase = progress.phase("Walking repository", None);
        let index = walk(
            root,
            &WalkOptions {
                respect_gitignore: true,
                extra_ignores: Vec::new(),
            },
        )
        .with_context(|| format!("walking {}", root.display()))?;
        let n = index.entries.len();
        phase.finish(&format!("Walked {n} entries"));

        // Workspace detection is on by default — we want to
        // surface monorepo overlays when applicable, and the
        // detector is cheap.
        let detection = crate::init::detect(root, true);

        let extends_uris = read_existing_extends(root);
        let has_git = alint_core::git::collect_tracked_paths(root).is_some();

        Ok(Self {
            root: root.to_path_buf(),
            index,
            detection,
            extends_uris,
            has_git,
        })
    }

    /// True when the user's existing config already extends a
    /// bundled URI matching this proposal. Currently scoped to
    /// bundled-ruleset proposals — rule-shaped proposals
    /// always pass through.
    pub fn config_already_covers(&self, proposal: &Proposal) -> bool {
        match &proposal.kind {
            ProposalKind::BundledRuleset { uri } => self.has_extends(uri),
            ProposalKind::Rule { .. } => false,
        }
    }

    /// Return whether `uri` appears in the user's existing
    /// `extends:` list. Comparison is exact-match against the
    /// canonical URI (no version-skew tolerance — `@v1` and
    /// `@v2` are different rulesets).
    pub fn has_extends(&self, uri: &str) -> bool {
        self.extends_uris.iter().any(|e| e == uri)
    }

    /// Iterate text-likely files (skips obvious binaries based
    /// on extension). Used by the antipattern suggester to
    /// scan content.
    pub fn text_files(&self) -> impl Iterator<Item = &alint_core::FileEntry> + '_ {
        self.index.files().filter(|e| !is_likely_binary(&e.path))
    }

    /// Construct a hand-crafted Scan for unit tests. Skips the
    /// walk/detect/git probes — callers fill exactly the
    /// fields their suggester reads.
    #[cfg(test)]
    pub fn for_test(detection: Detection, index: FileIndex, extends_uris: Vec<String>) -> Self {
        Self {
            root: PathBuf::from("/fake"),
            index,
            detection,
            extends_uris,
            has_git: false,
        }
    }
}

/// Read the `extends:` list from the user's existing
/// `.alint.yml` (or any of the alternate names the loader
/// recognises). Returns an empty vec when no config exists or
/// the file isn't parseable — degraded gracefully so `suggest`
/// always has something to say.
fn read_existing_extends(root: &Path) -> Vec<String> {
    for name in [".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml"] {
        let path = root.join(name);
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&body) else {
            return Vec::new();
        };
        return collect_extends(&doc);
    }
    Vec::new()
}

/// Walk a parsed YAML doc looking for the top-level `extends:`
/// list. Each entry is either a bare string (`"alint://…"`) or
/// a mapping with a `url:` field. Anything else is skipped
/// silently — the loader will surface its own error if the
/// user's config is malformed; we just need our best-effort
/// already-covered set.
fn collect_extends(doc: &serde_yaml_ng::Value) -> Vec<String> {
    let Some(map) = doc.as_mapping() else {
        return Vec::new();
    };
    let Some(extends) = map.get(serde_yaml_ng::Value::String("extends".into())) else {
        return Vec::new();
    };
    let Some(seq) = extends.as_sequence() else {
        return Vec::new();
    };
    seq.iter()
        .filter_map(|entry| match entry {
            serde_yaml_ng::Value::String(s) => Some(s.clone()),
            serde_yaml_ng::Value::Mapping(m) => m
                .get(serde_yaml_ng::Value::String("url".into()))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            _ => None,
        })
        .collect()
}

/// Heuristic binary-file filter. Conservative — we'd rather
/// scan a few extra files than miss a real antipattern hit.
fn is_likely_binary(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some(
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "ico"
                | "icns"
                | "pdf"
                | "zip"
                | "tar"
                | "gz"
                | "tgz"
                | "bz2"
                | "xz"
                | "7z"
                | "exe"
                | "dll"
                | "so"
                | "dylib"
                | "bin"
                | "wasm"
                | "ttf"
                | "otf"
                | "woff"
                | "woff2"
                | "mp3"
                | "mp4"
                | "wav"
                | "ogg"
                | "mov"
                | "webm"
                | "class"
                | "jar"
                | "lock"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn td() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("alint-suggest-")
            .tempdir()
            .unwrap()
    }

    fn touch(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn empty_repo_scan_walks_zero_entries() {
        let tmp = td();
        let scan = Scan::collect(tmp.path(), &Progress::null()).unwrap();
        assert_eq!(scan.index.entries.len(), 0);
        assert!(!scan.has_git);
        assert!(scan.detection.languages.is_empty());
    }

    #[test]
    fn extends_list_reads_string_entries() {
        let tmp = td();
        touch(
            tmp.path(),
            ".alint.yml",
            "version: 1\nextends:\n  - alint://bundled/oss-baseline@v1\n  - alint://bundled/rust@v1\nrules: []\n",
        );
        let scan = Scan::collect(tmp.path(), &Progress::null()).unwrap();
        assert!(scan.has_extends("alint://bundled/oss-baseline@v1"));
        assert!(scan.has_extends("alint://bundled/rust@v1"));
        assert!(!scan.has_extends("alint://bundled/node@v1"));
    }

    #[test]
    fn extends_list_reads_mapping_entries() {
        let tmp = td();
        touch(
            tmp.path(),
            ".alint.yml",
            "version: 1\nextends:\n  - url: alint://bundled/python@v1\n    only: [python-pyproject-toml]\nrules: []\n",
        );
        let scan = Scan::collect(tmp.path(), &Progress::null()).unwrap();
        assert!(scan.has_extends("alint://bundled/python@v1"));
    }

    #[test]
    fn malformed_config_doesnt_error_scan() {
        let tmp = td();
        touch(tmp.path(), ".alint.yml", "{this is not: valid: yaml::");
        // Scan must still build — we silently degrade to an
        // empty extends list rather than fail the command.
        let scan = Scan::collect(tmp.path(), &Progress::null()).unwrap();
        assert!(scan.extends_uris.is_empty());
    }

    #[test]
    fn binary_filter_skips_common_assets() {
        assert!(is_likely_binary(Path::new("logo.png")));
        assert!(is_likely_binary(Path::new("dist/app.wasm")));
        assert!(!is_likely_binary(Path::new("src/main.rs")));
        assert!(!is_likely_binary(Path::new("README.md")));
    }
}
