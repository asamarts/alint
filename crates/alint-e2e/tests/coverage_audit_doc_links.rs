//! Repo-side documentation link integrity (external-evaluation §3.1).
//!
//! A reader browsing the docs on GitHub hit two classes of dead link:
//!
//!   1. **Broken relative links** — a markdown link to a repo file that
//!      moved or was misspelt (e.g. `crates/alint-core/src/when.rs`
//!      after the `when` module became a directory).
//!   2. **Root-absolute site links** — `](/docs/concepts/…)`. On GitHub
//!      a `/…` link resolves against `github.com`, not the repo or the
//!      site, so it dead-ends. These belong to the rendered site at
//!      alint.org, not to a repo reader.
//!
//! This gate enforces both on the actively-maintained docs:
//!
//!   * every relative file link resolves to a real path, and
//!   * no doc introduces a root-absolute `/…` link, EXCEPT the few
//!     dual-purpose files that are sliced/copied into the alint.org
//!     site (where `/docs/…` is the correct, resolvable form) and that
//!     carry a "reading this on GitHub? the full reference is at
//!     alint.org" banner for repo readers.
//!
//! Out of scope (different link-resolution semantics, not browsed as
//! repo docs): the site-content tree `docs/site/**`, the benchmark
//! result/▸archive snapshots, and the versioned `docs/design/v*/`
//! design records.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Dual-purpose docs that ARE rendered into the alint.org site, where a
/// root-absolute `/docs/…` cross-link is the correct form. Each must
/// also tell a GitHub reader where those links resolve (asserted
/// below). Everything else is a repo-only doc and may not use `/…`.
const SITE_RENDERED_ALLOWLIST: &[&str] = &["docs/rules.md"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Is `rel` (a `.md` path, already extension-filtered by the caller) an
/// actively-maintained doc this gate governs?
fn is_governed(rel: &str) -> bool {
    // Site content + historical snapshots have their own link semantics.
    if rel.starts_with("docs/site/")
        || rel.starts_with("docs/benchmarks/")
        || rel.contains("/archive/")
    {
        return false;
    }
    // Versioned design records: docs/design/v0.9/, v0.12/, ...
    if rel.starts_with("docs/design/v")
        && rel
            .trim_start_matches("docs/design/v")
            .starts_with(|c: char| c.is_ascii_digit())
    {
        return false;
    }
    true
}

fn walk_md(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_md(base, &p, out);
        } else if p.extension().is_some_and(|x| x == "md")
            && let Ok(rel) = p.strip_prefix(base)
        {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if is_governed(&rel) {
                out.push(rel);
            }
        }
    }
}

fn collect_docs(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Top-level governance/readme docs.
    for top in [
        "README.md",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "GOVERNANCE.md",
        "CODE_OF_CONDUCT.md",
        "RELEASING.md",
    ] {
        if root.join(top).is_file() {
            out.push(top.to_string());
        }
    }
    walk_md(root, &root.join("docs"), &mut out);
    out
}

/// Markdown links `[text](url)`, returning each url. Skips image/inline
/// soup edge cases by taking the simplest paren-balanced form.
fn links(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut urls = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            if let Some(end) = text[i + 2..].find(')') {
                urls.push(text[i + 2..i + 2 + end].trim().to_string());
                i = i + 2 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    urls
}

#[test]
fn repo_doc_links_are_not_dead_on_github() {
    let root = repo_root();
    let docs = collect_docs(&root);
    assert!(!docs.is_empty(), "no governed docs found");

    let mut broken_relative: Vec<String> = Vec::new();
    let mut root_absolute: Vec<String> = Vec::new();
    let mut missing_banner: Vec<String> = Vec::new();

    for rel in &docs {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path).unwrap();
        let allow_site_links = SITE_RENDERED_ALLOWLIST.contains(&rel.as_str());

        if allow_site_links {
            // Must orient a GitHub reader to the rendered site.
            let lc = text.to_lowercase();
            if !(lc.contains("alint.org") && lc.contains("github")) {
                missing_banner.push(format!(
                    "{rel}: carries root-absolute /docs links but no \
                     'reading on GitHub? full reference at alint.org' banner"
                ));
            }
        }

        for url in links(&text) {
            // Anchors, externals, mail/other schemes: not our concern.
            if url.starts_with('#') || url.starts_with("mailto:") {
                continue;
            }
            if url.starts_with("http://") || url.starts_with("https://") {
                continue;
            }
            // Scheme-like (e.g. `alint://…`) — not a path.
            if regexish_scheme(&url) {
                continue;
            }
            if url.starts_with('/') {
                if !allow_site_links {
                    root_absolute.push(format!(
                        "{rel}: ]({url}) — dead on GitHub; use https://alint.org{url}"
                    ));
                }
                continue;
            }
            // Relative link: resolve the path part (drop anchor/query).
            let target = url.split(['#', '?']).next().unwrap_or("");
            if target.is_empty() || target.contains(' ') {
                continue; // prose / placeholder, not a real path
            }
            let resolved = path.parent().unwrap().join(target);
            if !resolved.exists() {
                broken_relative.push(format!("{rel}: ]({url}) — target not found"));
            }
        }
    }

    let mut problems = String::new();
    for v in [&broken_relative, &root_absolute, &missing_banner] {
        for line in v {
            writeln!(problems, "  - {line}").unwrap();
        }
    }
    assert!(
        broken_relative.is_empty() && root_absolute.is_empty() && missing_banner.is_empty(),
        "documentation link problems ({} broken relative, {} dead root-absolute, \
         {} missing banner):\n{problems}",
        broken_relative.len(),
        root_absolute.len(),
        missing_banner.len(),
    );
}

/// Does the url begin with a `scheme:` that isn't a path (e.g.
/// `alint://`, `tel:`)? `http(s)` is handled before this is called.
fn regexish_scheme(url: &str) -> bool {
    if let Some(colon) = url.find(':') {
        let scheme = &url[..colon];
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
            && url[colon..].starts_with(":/")
    } else {
        false
    }
}
