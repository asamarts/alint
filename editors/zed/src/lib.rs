//! alint Zed extension.
//!
//! Zed has no config-only path for a custom external LSP binary, so the
//! integration is a small wasm extension that tells Zed how to launch
//! `alint lsp`. Diagnostics, hover, and quick-fixes are rendered by Zed
//! from the server's LSP messages — this crate is just the launcher +
//! binary resolution (settings → PATH → versioned cached copy → SHA-256
//! verified GitHub release download matching this extension's version).

use std::io::Read as _;
use std::path::Path;

use flate2::read::GzDecoder;
use zed_extension_api::{
    self as zed,
    http_client::{HttpMethod, HttpRequest, RedirectPolicy},
    settings::{CommandSettings, LspSettings},
    LanguageServerId, Result,
};

const REPO: &str = "asamarts/alint";
const EXTENSION_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_SHA256_BYTES: usize = 4096;

struct AlintExtension {
    cached_binary_path: Option<String>,
}

impl AlintExtension {
    fn binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
        binary_settings: Option<&CommandSettings>,
    ) -> Result<String> {
        // 1. Explicit `binary.path` in the worktree's LSP settings.
        if let Some(path) = binary_settings.and_then(|b| b.path.as_ref()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        // 2. `alint` on PATH.
        if let Some(path) = worktree.which("alint") {
            return Ok(path);
        }

        // 3. In-memory cached copy from this session.
        if let Some(path) = &self.cached_binary_path {
            if is_existing_file(path) {
                return Ok(path.clone());
            }
        }

        // 4. Versioned on-disk cache probe BEFORE any network call (works offline
        //    across Zed restarts without hitting the GitHub API).
        let (platform, arch) = zed::current_platform();
        let target = target_triple(platform, arch)?;
        let tag = format!("v{EXTENSION_VERSION}");
        validate_tag(&tag)?;
        let binary_name = binary_name_for(platform);
        let work_dir = versioned_work_dir(&tag);
        let binary_path = versioned_binary_path(&tag, &target, binary_name);
        if is_existing_file(&binary_path) {
            self.cached_binary_path = Some(binary_path.clone());
            return Ok(binary_path);
        }

        // 5. Download the matching release pinned to `v<CARGO_PKG_VERSION>` and
        //    verify its `.sha256` sidecar before extracting the single binary.
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let result = self.download_verified_release(
            language_server_id,
            &tag,
            &target,
            binary_name,
            &work_dir,
            &binary_path,
        );
        match result {
            Ok(()) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::None,
                );
                prune_stale_version_dirs(Path::new("."), &work_dir);
                self.cached_binary_path = Some(binary_path.clone());
                Ok(binary_path)
            }
            Err(err) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Failed(err.clone()),
                );
                Err(err)
            }
        }
    }

    fn download_verified_release(
        &self,
        language_server_id: &LanguageServerId,
        tag: &str,
        target: &str,
        binary_name: &str,
        work_dir: &str,
        binary_path: &str,
    ) -> Result<()> {
        let release = zed::github_release_by_tag_name(REPO, tag)?;
        let asset_name = format!("alint-{tag}-{target}.tar.gz");
        let sha_name = format!("{asset_name}.sha256");
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| format!("no release asset named {asset_name}"))?;
        let sha_asset = release
            .assets
            .iter()
            .find(|a| a.name == sha_name)
            .ok_or_else(|| format!("no checksum asset named {sha_name}"))?;

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Downloading,
        );

        let sha_bytes = fetch_https(&sha_asset.download_url, MAX_SHA256_BYTES)?;
        let expected_sha = parse_sha256_file(&sha_bytes, &asset_name)?;
        let tar_gz = fetch_https(&asset.download_url, MAX_ARCHIVE_BYTES)?;
        let actual_sha = sha256_hex(&tar_gz);
        if actual_sha != expected_sha {
            return Err(format!(
                "SHA-256 mismatch for {asset_name} (expected {expected_sha}, got {actual_sha})"
            ));
        }

        let expected_entry = format!("alint-{tag}-{target}/{binary_name}");
        let binary_bytes =
            extract_single_file_from_tar_gz(&tar_gz, &expected_entry, MAX_ARCHIVE_BYTES)?;

        let dest = Path::new(binary_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {e}", parent.display()))?;
        }
        let tmp_path = format!("{work_dir}/.{binary_name}.tmp");
        std::fs::write(&tmp_path, &binary_bytes)
            .map_err(|e| format!("failed to write temporary binary {tmp_path}: {e}"))?;
        if let Err(err) = std::fs::rename(&tmp_path, dest) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(format!("failed to install binary to {binary_path}: {err}"));
        }
        zed::make_file_executable(binary_path)?;
        Ok(())
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
        let lsp_settings = LspSettings::for_worktree("alint", worktree).ok();
        let binary_settings = lsp_settings.as_ref().and_then(|s| s.binary.as_ref());
        let path = self.binary_path(language_server_id, worktree, binary_settings)?;
        Ok(build_command(path, binary_settings))
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        let init_opts = LspSettings::for_worktree("alint", worktree)
            .ok()
            .and_then(|s| s.initialization_options);
        Ok(init_opts)
    }
}

fn build_command(path: String, binary_settings: Option<&CommandSettings>) -> zed::Command {
    let args = binary_settings
        .and_then(|b| b.arguments.clone())
        .unwrap_or_else(|| vec!["lsp".to_string()]);
    let mut env: Vec<(String, String)> = binary_settings
        .and_then(|b| b.env.as_ref())
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    env.sort_by(|a, b| a.0.cmp(&b.0));
    zed::Command {
        command: path,
        args,
        env,
    }
}

fn is_existing_file(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|stat| stat.is_file())
}

fn binary_name_for(platform: zed::Os) -> &'static str {
    if platform == zed::Os::Windows {
        "alint.exe"
    } else {
        "alint"
    }
}

fn versioned_work_dir(tag: &str) -> String {
    format!("alint-{tag}")
}

fn versioned_binary_path(tag: &str, target: &str, binary_name: &str) -> String {
    format!("alint-{tag}/alint-{tag}-{target}/{binary_name}")
}

fn validate_tag(tag: &str) -> Result<()> {
    let Some(rest) = tag.strip_prefix('v') else {
        return Err(format!("invalid release tag {tag:?}: must start with 'v'"));
    };
    if rest.is_empty() || rest.contains("..") {
        return Err(format!("invalid release tag {tag:?}"));
    }
    if !rest
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(format!(
            "invalid release tag {tag:?}: contains disallowed characters"
        ));
    }
    Ok(())
}

fn prune_stale_version_dirs(base_dir: &Path, keep_name: &str) {
    let Ok(entries) = std::fs::read_dir(base_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name != keep_name && name.starts_with("alint-v") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn fetch_https(url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    if !url.starts_with("https://") {
        return Err(format!("refusing non-HTTPS download URL: {url}"));
    }
    let req = HttpRequest::builder()
        .method(HttpMethod::Get)
        .url(url)
        .redirect_policy(RedirectPolicy::FollowLimit(5))
        .build()?;
    let resp = req.fetch()?;
    if resp.body.len() > max_bytes {
        return Err(format!(
            "response exceeds {max_bytes} bytes fetching {url}"
        ));
    }
    Ok(resp.body)
}

fn parse_sha256_file(bytes: &[u8], archive_name: &str) -> Result<String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| format!("invalid UTF-8 in checksum file for {archive_name}"))?;
    let token = text.split_whitespace().next().unwrap_or("");
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("invalid SHA-256 checksum file for {archive_name}"));
    }
    Ok(token.to_ascii_lowercase())
}

/// FIPS 180-4 SHA-256 (zero external crate dependencies).
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut hex = String::with_capacity(64);
    for word in h {
        use std::fmt::Write as _;
        let _ = write!(hex, "{word:08x}");
    }
    hex
}

fn normalize_tar_path(raw: &str) -> Result<String> {
    let mut p = raw;
    while let Some(stripped) = p.strip_prefix("./") {
        p = stripped;
    }
    if p.is_empty() || p.starts_with('/') || p.contains('\\') {
        return Err(format!("disallowed tar entry path: {raw:?}"));
    }
    for seg in p.split('/') {
        if seg == ".." || seg == "." || seg.is_empty() {
            return Err(format!("disallowed tar entry path segment: {raw:?}"));
        }
        if seg.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(format!("control character in tar entry path: {raw:?}"));
        }
    }
    Ok(p.to_string())
}

fn read_tar_cstr(slice: &[u8]) -> Result<String> {
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    std::str::from_utf8(&slice[..end])
        .map(str::to_string)
        .map_err(|_| "non-UTF-8 string in tar header".to_string())
}

fn parse_tar_octal(slice: &[u8]) -> Result<u64> {
    if slice.first().is_some_and(|&b| b & 0x80 != 0) {
        if slice[0] != 0x80 {
            return Err("unsupported negative or oversized base-256 tar field".to_string());
        }
        let mut val = 0u64;
        for &b in &slice[1..] {
            val = val
                .checked_shl(8)
                .ok_or_else(|| "tar base-256 field overflow".to_string())?
                | u64::from(b);
        }
        return Ok(val);
    }
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    let text = std::str::from_utf8(&slice[..end])
        .map_err(|_| "invalid tar numeric field".to_string())?
        .trim();
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| format!("invalid tar octal field: {text:?}"))
}

fn verify_tar_checksum(hdr: &[u8]) -> Result<()> {
    let expected = parse_tar_octal(&hdr[148..156])?;
    let mut sum = 0u64;
    for (i, &b) in hdr.iter().enumerate() {
        sum += if (148..156).contains(&i) {
            u64::from(b' ')
        } else {
            u64::from(b)
        };
    }
    if sum != expected {
        return Err(format!(
            "corrupt tar header checksum (expected {expected}, got {sum})"
        ));
    }
    Ok(())
}

fn parse_pax_records(data: &[u8]) -> Result<(Option<String>, Option<u64>)> {
    let mut path_override = None;
    let mut size_override = None;
    let mut pos = 0usize;
    while pos < data.len() {
        let Some(sp) = data[pos..].iter().position(|&b| b == b' ') else {
            return Err("malformed PAX record".to_string());
        };
        let len_str = std::str::from_utf8(&data[pos..pos + sp])
            .map_err(|_| "invalid PAX length".to_string())?;
        let rec_len: usize = len_str
            .parse()
            .map_err(|_| "invalid PAX record length".to_string())?;
        if rec_len == 0 || pos + rec_len > data.len() || data[pos + rec_len - 1] != b'\n' {
            return Err("truncated or invalid PAX record".to_string());
        }
        let body = std::str::from_utf8(&data[pos + sp + 1..pos + rec_len - 1])
            .map_err(|_| "non-UTF-8 PAX record".to_string())?;
        if let Some((k, v)) = body.split_once('=') {
            match k {
                "path" => path_override = Some(v.to_string()),
                "size" => {
                    size_override = Some(
                        v.parse::<u64>()
                            .map_err(|_| format!("invalid PAX size {v:?}"))?,
                    );
                }
                _ => {}
            }
        }
        pos += rec_len;
    }
    Ok((path_override, size_override))
}

/// Decompress `tar_gz` with a strict uncompressed cap and extract ONLY the
/// regular file matching `expected_entry`, rejecting symlinks/hardlinks,
/// traversal, duplicate targets, and corrupt checksums.
fn extract_single_file_from_tar_gz(
    tar_gz: &[u8],
    expected_entry: &str,
    max_uncompressed: usize,
) -> Result<Vec<u8>> {
    let expected = normalize_tar_path(expected_entry)?;
    let mut decoder = GzDecoder::new(tar_gz).take((max_uncompressed + 1) as u64);
    let mut tar = Vec::new();
    decoder
        .read_to_end(&mut tar)
        .map_err(|e| format!("failed to decompress gzip archive: {e}"))?;
    if tar.len() > max_uncompressed {
        return Err(format!(
            "decompressed archive exceeds {max_uncompressed} bytes"
        ));
    }
    if tar.len() % 512 != 0 {
        return Err("truncated tar archive".to_string());
    }

    let mut offset = 0usize;
    let mut next_pax_path: Option<String> = None;
    let mut next_pax_size: Option<u64> = None;
    let mut next_gnu_name: Option<String> = None;
    let mut found: Option<Vec<u8>> = None;

    while offset + 512 <= tar.len() {
        let hdr = &tar[offset..offset + 512];
        if hdr.iter().all(|&b| b == 0) {
            break;
        }
        verify_tar_checksum(hdr)?;
        let typeflag = hdr[156];
        let mut size = parse_tar_octal(&hdr[124..136])?;
        if let Some(ps) = next_pax_size.take() {
            size = ps;
        }
        let size_usize = usize::try_from(size)
            .map_err(|_| "tar entry size exceeds addressable memory".to_string())?;
        let data_start = offset + 512;
        let data_end = data_start
            .checked_add(size_usize)
            .ok_or_else(|| "tar entry size overflow".to_string())?;
        if data_end > tar.len() {
            return Err("truncated tar entry payload".to_string());
        }
        let payload = &tar[data_start..data_end];
        let padded = (size_usize + 511) & !511;
        offset = data_start
            .checked_add(padded)
            .ok_or_else(|| "tar offset overflow".to_string())?;

        match typeflag {
            b'x' => {
                let (p, s) = parse_pax_records(payload)?;
                next_pax_path = p;
                next_pax_size = s;
                continue;
            }
            b'g' => {
                let (p, s) = parse_pax_records(payload)?;
                if p.is_some() || s.is_some() {
                    return Err("unsupported PAX global path/size override".to_string());
                }
                continue;
            }
            b'L' => {
                next_gnu_name = Some(read_tar_cstr(payload)?);
                continue;
            }
            b'1' | b'2' => {
                return Err("tar archive contains unsupported link entry".to_string());
            }
            b'5' => {
                let _ = next_pax_path.take();
                let _ = next_gnu_name.take();
                continue;
            }
            b'0' | 0 => {
                let raw_name = if let Some(p) = next_pax_path.take() {
                    let _ = next_gnu_name.take();
                    p
                } else if let Some(g) = next_gnu_name.take() {
                    g
                } else {
                    let name = read_tar_cstr(&hdr[0..100])?;
                    let is_posix_ustar = &hdr[257..263] == b"ustar\0";
                    let prefix = if is_posix_ustar {
                        read_tar_cstr(&hdr[345..500])?
                    } else {
                        String::new()
                    };
                    if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}/{name}")
                    }
                };
                let normalized = normalize_tar_path(&raw_name)?;
                if normalized.eq_ignore_ascii_case(&expected) {
                    if found.is_some() || normalized != expected {
                        return Err(format!("duplicate or case-colliding entry for {expected}"));
                    }
                    found = Some(payload.to_vec());
                }
            }
            other => {
                return Err(format!("unsupported tar entry typeflag: {other:#x}"));
            }
        }
    }

    found.ok_or_else(|| format!("extracted tarball missing {expected}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::collections::HashMap;
    use std::io::Write as _;

    fn make_header(name: &str, size: usize, typeflag: u8) -> [u8; 512] {
        let mut h = [0u8; 512];
        let nb = name.as_bytes();
        h[..nb.len()].copy_from_slice(nb);
        h[100..108].copy_from_slice(b"0000755\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        let sz = format!("{size:011o}\0");
        h[124..136].copy_from_slice(sz.as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[148..156].fill(b' ');
        h[156] = typeflag;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        let chk = format!("{sum:06o}\0 ");
        h[148..156].copy_from_slice(chk.as_bytes());
        h
    }

    fn make_tar_gz(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let mut tar = Vec::new();
        for &(name, tf, data) in entries {
            tar.extend_from_slice(&make_header(name, data.len(), tf));
            tar.extend_from_slice(data);
            let rem = data.len() % 512;
            if rem != 0 {
                tar.resize(tar.len() + (512 - rem), 0);
            }
        }
        tar.resize(tar.len() + 1024, 0);
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&tar).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn tag_validation_rejects_traversal_and_accepts_semver() {
        assert!(validate_tag("v0.23.0").is_ok());
        assert!(validate_tag("v0.24.0-rc.1").is_ok());
        assert!(validate_tag("0.23.0").is_err());
        assert!(validate_tag("v../escape").is_err());
        assert!(validate_tag("v0.23.0/evil").is_err());
        assert!(validate_tag("v0.23.0\\evil").is_err());
    }

    #[test]
    fn parse_sha256_normalizes_and_rejects_bad_files() {
        let upper = "A".repeat(64);
        assert_eq!(
            parse_sha256_file(format!("{upper}  alint.tar.gz\n").as_bytes(), "a").unwrap(),
            "a".repeat(64)
        );
        assert!(parse_sha256_file(b"short  alint.tar.gz", "a").is_err());
    }

    #[test]
    fn tar_gz_extracts_exact_entry_and_rejects_symlinks_traversal_and_bombs() {
        let target = "alint-v0.23.0-x86_64-unknown-linux-musl/alint";
        let ok = make_tar_gz(&[("./alint-v0.23.0-x86_64-unknown-linux-musl/alint", b'0', b"ELF")]);
        assert_eq!(
            extract_single_file_from_tar_gz(&ok, target, 1024 * 1024).unwrap(),
            b"ELF"
        );

        let symlink = make_tar_gz(&[(target, b'2', b"")]);
        assert!(extract_single_file_from_tar_gz(&symlink, target, 1024 * 1024).is_err());

        let traversal = make_tar_gz(&[("../evil", b'0', b"x")]);
        assert!(extract_single_file_from_tar_gz(&traversal, target, 1024 * 1024).is_err());

        let big = make_tar_gz(&[(target, b'0', &[0x41u8; 8192])]);
        assert!(extract_single_file_from_tar_gz(&big, target, 4096).is_err());
    }

    #[test]
    fn build_command_forwards_arguments_and_env_with_defaults() {
        let default_cmd = build_command("/bin/alint".into(), None);
        assert_eq!(default_cmd.command, "/bin/alint");
        assert_eq!(default_cmd.args, vec!["lsp".to_string()]);
        assert!(default_cmd.env.is_empty());

        let mut env = HashMap::new();
        env.insert("ALINT_LOG".into(), "debug".into());
        let custom = CommandSettings {
            path: Some("/custom/alint".into()),
            arguments: Some(vec!["lsp".into(), "--untrusted".into()]),
            env: Some(env),
        };
        let cmd = build_command("/custom/alint".into(), Some(&custom));
        assert_eq!(cmd.args, vec!["lsp", "--untrusted"]);
        assert_eq!(cmd.env, vec![("ALINT_LOG".into(), "debug".into())]);
    }
}
