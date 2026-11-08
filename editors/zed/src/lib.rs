//! alint Zed extension.
//!
//! Zed has no config-only path for a custom external LSP binary, so the
//! integration is a small wasm extension that tells Zed how to launch
//! `alint lsp`. Diagnostics, hover, and quick-fixes are rendered by Zed
//! from the server's LSP messages — this crate is just the launcher +
//! binary resolution (settings → PATH → managed GitHub download,
//! mirroring the other install channels).

use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct AlintExtension {
    cached_binary_path: Option<String>,
}

impl AlintExtension {
    fn binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        // 1. Explicit `binary.path` in the worktree's LSP settings.
        if let Ok(lsp_settings) = LspSettings::for_worktree("alint", worktree) {
            if let Some(binary) = lsp_settings.binary {
                if let Some(path) = binary.path {
                    return Ok(path);
                }
            }
        }

        // 2. `alint` on PATH.
        if let Some(path) = worktree.which("alint") {
            return Ok(path);
        }

        // 3. A copy this extension downloaded earlier (still present).
        if let Some(path) = &self.cached_binary_path {
            if std::fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(path.clone());
            }
        }

        // 4. Download from GitHub. Strategy note (kept consistent across
        // editors): the VS Code / JetBrains plugins download the alint
        // release matching their own (release-stamped) version; a Zed
        // wasm extension can't read its own version at runtime, so it
        // takes the latest release instead.
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            "asamarts/alint",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        let (platform, arch) = zed::current_platform();
        let target = target_triple(platform, arch)?;
        let tag = if release.version.starts_with('v') {
            release.version.clone()
        } else {
            format!("v{}", release.version)
        };
        let asset_name = format!("alint-{tag}-{target}.tar.gz");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("no release asset named {asset_name}"))?;

        // The tarball extracts to `alint-<tag>-<target>/alint(.exe)`.
        let work_dir = format!("alint-{tag}");
        let binary_name = if platform == zed::Os::Windows {
            "alint.exe"
        } else {
            "alint"
        };
        let binary_path = format!("{work_dir}/alint-{tag}-{target}/{binary_name}");

        if !std::fs::metadata(&binary_path).is_ok_and(|stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(
                &asset.download_url,
                &work_dir,
                zed::DownloadedFileType::GzipTar,
            )?;
            zed::make_file_executable(&binary_path)?;
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

impl zed::Extension for AlintExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let path = self.binary_path(language_server_id, worktree)?;
        Ok(zed::Command {
            command: path,
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

/// Map Zed's platform/arch to the alint release-target triple. Mirrors
/// `npm/platforms.json` and the release.yml build matrix.
fn target_triple(platform: zed::Os, arch: zed::Architecture) -> Result<String> {
    let triple = match (platform, arch) {
        (zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-musl",
        (zed::Os::Linux, zed::Architecture::Aarch64) => "aarch64-unknown-linux-musl",
        (zed::Os::Mac, zed::Architecture::X8664) => "x86_64-apple-darwin",
        (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
        (zed::Os::Windows, zed::Architecture::X8664) => "x86_64-pc-windows-msvc",
        (zed::Os::Windows, zed::Architecture::Aarch64) => "aarch64-pc-windows-msvc",
        (os, arch) => return Err(format!("unsupported platform {os:?}/{arch:?}")),
    };
    Ok(triple.to_string())
}

zed::register_extension!(AlintExtension);
