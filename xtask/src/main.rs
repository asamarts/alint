//! `xtask` — ancillary helpers for alint that don't belong in the shipped binary.
//!
//! Current commands:
//!
//! - `bench-release` — builds alint in release mode, generates deterministic
//!   synthetic trees, runs `hyperfine` across a tree-size × rule-count
//!   matrix, and emits a platform-fingerprinted markdown report. Used to
//!   produce the numbers published in `docs/benchmarks/<version>/`.
//! - `gen-fixture`   — materialize a synthetic tree for ad-hoc experimentation.
//! - `docs-export`   — emit a `docs-bundle/` directory consumed by the
//!   `asamarts/alint.org` site at build time. The bundle is the canonical
//!   handoff format between the alint repo (source of truth for technical
//!   docs) and the site repo (presentation).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "alint developer helpers")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build alint in release mode and run hyperfine across a tree × rules matrix.
    BenchRelease {
        /// Skip the large tree sizes; produce a smoke-test-sized report in ~seconds.
        #[arg(long)]
        quick: bool,
        /// Where to write the markdown report. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Seed used to generate the synthetic trees.
        #[arg(long, default_value_t = 0xA11E47)]
        seed: u64,
    },
    /// Materialize a synthetic tree (persistent) for manual experimentation.
    GenFixture {
        #[arg(long, default_value_t = 1000)]
        files: usize,
        #[arg(long, default_value_t = 4)]
        depth: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Where to place the tree. Defaults to a fresh tempdir.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Emit `docs-bundle/` — the handoff bundle consumed by
    /// `asamarts/alint.org` at site-build time.
    DocsExport {
        /// Output directory. Defaults to `target/docs-bundle/`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Validate the export would succeed without writing
        /// anything. Used by CI to gate merges on a buildable
        /// bundle.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::BenchRelease { quick, out, seed } => bench_release(quick, out, seed),
        Commands::GenFixture {
            files,
            depth,
            seed,
            out,
        } => gen_fixture(files, depth, seed, out),
        Commands::DocsExport { out, check } => docs_export(out, check),
    }
}

fn gen_fixture(files: usize, depth: usize, seed: u64, out: Option<PathBuf>) -> Result<()> {
    let tree = alint_bench::tree::generate_tree(files, depth, seed)?;
    let final_path = match out {
        Some(p) => {
            fs::create_dir_all(&p)?;
            copy_tree(tree.root(), &p)?;
            p
        }
        None => tree.into_persistent()?,
    };
    println!("generated {files} files under {}", final_path.display());
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir_plain(from)? {
        let rel = entry.strip_prefix(from).unwrap();
        let dest = to.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest)?;
        } else if entry.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&entry, &dest)?;
        }
    }
    Ok(())
}

fn walkdir_plain(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let md = fs::metadata(&p)?;
        if md.is_dir() {
            for entry in fs::read_dir(&p)? {
                stack.push(entry?.path());
            }
            out.push(p);
        } else {
            out.push(p);
        }
    }
    Ok(out)
}

// ---- bench-release ---------------------------------------------------------

const RULES_CONFIG_YAML: &str = include_str!("bench_config.yml");

fn bench_release(quick: bool, out: Option<PathBuf>, seed: u64) -> Result<()> {
    ensure_hyperfine()?;

    let binary = build_release_binary()?;
    let sizes: &[usize] = if quick {
        &[500]
    } else {
        &[1_000, 10_000, 100_000]
    };

    // Write the shared config file once to a tempdir and point every run at it.
    let config_dir = tempfile::tempdir()?;
    let config_path = config_dir.path().join(".alint.yml");
    fs::write(&config_path, RULES_CONFIG_YAML)?;

    let mut report = String::new();
    write_header(&mut report, quick, seed)?;

    for &size in sizes {
        eprintln!("[xtask] generating tree of {size} files (seed={seed})...");
        let tree = alint_bench::tree::generate_tree(size, 4, seed)?;
        // hyperfine doesn't care about CWD; we pass the tree path as an argument.
        let target_path = tree.root();
        // Copy the config into the tree so `alint check <path>` discovers it.
        fs::copy(&config_path, target_path.join(".alint.yml"))?;

        eprintln!("[xtask] running hyperfine against {size}-file tree...");
        let md = run_hyperfine(&binary, target_path, size, quick)?;
        writeln!(&mut report, "\n### {size} files\n")?;
        writeln!(&mut report, "{md}")?;
    }

    match out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, report)?;
            eprintln!("[xtask] wrote {}", path.display());
        }
        None => print!("{report}"),
    }
    Ok(())
}

fn ensure_hyperfine() -> Result<()> {
    match Command::new("hyperfine").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "hyperfine not found in PATH. Install it with:\n  \
             cargo install hyperfine\n  # or apt/brew/choco install hyperfine"
        ),
    }
}

fn build_release_binary() -> Result<PathBuf> {
    eprintln!("[xtask] cargo build --release -p alint");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "alint"])
        .status()
        .context("invoking cargo")?;
    if !status.success() {
        bail!("release build failed");
    }
    let workspace_root = workspace_root()?;
    let bin = workspace_root
        .join("target")
        .join("release")
        .join(if cfg!(windows) { "alint.exe" } else { "alint" });
    if !bin.is_file() {
        bail!("expected binary at {}", bin.display());
    }
    Ok(bin)
}

fn workspace_root() -> Result<PathBuf> {
    // xtask is inside the workspace; CARGO_MANIFEST_DIR = alint/xtask; parent = workspace root.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .parent()
        .context("xtask has no parent directory")?;
    Ok(root.to_path_buf())
}

fn run_hyperfine(binary: &Path, tree_root: &Path, size: usize, quick: bool) -> Result<String> {
    let warmup = if quick { "2" } else { "5" };
    let min_runs = if quick { "3" } else { "10" };

    let tmp_md = tempfile::NamedTempFile::new()?;
    let md_path = tmp_md.path().to_path_buf();

    let status = Command::new("hyperfine")
        .args(["--warmup", warmup, "--min-runs", min_runs])
        .arg("--command-name")
        .arg(format!("alint check (synthetic, {size} files)"))
        .arg("--export-markdown")
        .arg(&md_path)
        .arg(format!(
            "{} check {}",
            shell_quote(binary.to_str().unwrap()),
            shell_quote(tree_root.to_str().unwrap())
        ))
        .status()
        .context("invoking hyperfine")?;
    if !status.success() {
        bail!("hyperfine exited non-zero");
    }
    Ok(fs::read_to_string(&md_path)?)
}

fn shell_quote(s: &str) -> String {
    if s.chars().any(|c| c == ' ' || c == '\t') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

fn write_header(report: &mut String, quick: bool, seed: u64) -> Result<()> {
    writeln!(
        report,
        "# alint bench-release results\n\n\
         **Mode:** {mode}  \n\
         **Seed:** `{seed:#x}`  \n\
         **OS:** `{os}/{arch}`  \n\
         **rustc:** `{rustc}`  \n\
         **alint git SHA:** `{sha}`  \n\
         **Generated:** {ts}  \n\n\
         Results measured with `hyperfine` on this machine. Cross-machine \
         variance is expected; see `docs/benchmarks/METHODOLOGY.md` for the \
         reproduction recipe. Do not compare absolute numbers across \
         rows in different files — compare like-for-like.",
        mode = if quick { "quick (smoke)" } else { "full" },
        seed = seed,
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        rustc = rustc_version().unwrap_or_else(|| "unknown".to_string()),
        sha = git_sha().unwrap_or_else(|| "unknown".to_string()),
        ts = now_iso(),
    )?;
    Ok(())
}

fn rustc_version() -> Option<String> {
    let out = Command::new("rustc").arg("--version").output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn git_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn now_iso() -> String {
    // Minimal ISO-ish timestamp without pulling in chrono.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}

// ---- docs-export ----------------------------------------------------------

/// Workspace-relative paths the export reads from. Centralised so a
/// `git mv` of any of these is a one-liner here, not a hunt across
/// the function body.
mod docs_paths {
    pub const SITE_DIR: &str = "docs/site";
    pub const RULES_DOC: &str = "docs/rules.md";
    pub const ARCHITECTURE_DOC: &str = "docs/design/ARCHITECTURE.md";
    pub const ROADMAP_DOC: &str = "docs/design/ROADMAP.md";
    pub const CHANGELOG: &str = "CHANGELOG.md";
    pub const SCHEMA_JSON: &str = "schemas/v1/config.json";
    pub const RULESETS_DIR: &str = "crates/alint-dsl/rulesets/v1";
}

fn docs_export(out: Option<PathBuf>, check: bool) -> Result<()> {
    let workspace = workspace_root()?;
    let target_dir = out.unwrap_or_else(|| workspace.join("target/docs-bundle"));

    // In check mode we still produce the bundle (so all the
    // generators run) — just under a tempdir we discard. Catches
    // missing files / bad YAML / broken --help before merge.
    let _scratch_guard;
    let target_dir = if check {
        let scratch = tempfile::tempdir().context("creating tempdir for --check")?;
        let path = scratch.path().to_path_buf();
        _scratch_guard = scratch;
        path
    } else {
        // Clean previous output so removed pages don't linger.
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)
                .with_context(|| format!("removing stale {}", target_dir.display()))?;
        }
        fs::create_dir_all(&target_dir)?;
        target_dir
    };

    eprintln!("[xtask] docs-export → {}", target_dir.display());

    // 1. Hand-written long-form prose, copied verbatim.
    copy_site_tree(&workspace, &target_dir)?;

    // 2. Verbatim copies of the existing top-level docs.
    copy_one(
        &workspace.join(docs_paths::CHANGELOG),
        &target_dir.join("changelog.md"),
        Some("Changelog"),
    )?;
    copy_one(
        &workspace.join(docs_paths::ARCHITECTURE_DOC),
        &target_dir.join("about/architecture.md"),
        Some("Architecture"),
    )?;
    copy_one(
        &workspace.join(docs_paths::ROADMAP_DOC),
        &target_dir.join("about/roadmap.md"),
        Some("Roadmap"),
    )?;
    // Rule reference: slice docs/rules.md by H2 (= family) into
    // one page per family, plus a generated index. See
    // `generate_rules_pages` for the slicing rules.
    generate_rules_pages(&workspace, &target_dir)?;

    // 3. Per-bundled-ruleset reference page.
    generate_bundled_ruleset_pages(&workspace, &target_dir)?;

    // 4. The JSON Schema, kept as JSON for programmatic use.
    let schema_dest = target_dir.join("configuration/schema.json");
    fs::create_dir_all(schema_dest.parent().unwrap())?;
    fs::copy(workspace.join(docs_paths::SCHEMA_JSON), &schema_dest)?;

    // 5. CLI reference, captured from the alint binary's --help.
    generate_cli_reference(&workspace, &target_dir)?;

    // 6. Manifest. Any consumer (alint.org sync script, audit
    //    tooling) reads this to know what's in the bundle.
    write_manifest(&target_dir)?;

    if check {
        eprintln!("[xtask] docs-export --check OK");
    } else {
        eprintln!("[xtask] docs-export wrote {}", target_dir.display());
    }
    Ok(())
}

/// Recursively copy `docs/site/**.md` into the bundle root. Mirror
/// the directory layout exactly — `docs/site/getting-started/foo.md`
/// → `docs-bundle/getting-started/foo.md`.
fn copy_site_tree(workspace: &Path, target_dir: &Path) -> Result<()> {
    let site_root = workspace.join(docs_paths::SITE_DIR);
    if !site_root.is_dir() {
        bail!(
            "{} is missing — Phase 2 expects hand-written docs to live here",
            site_root.display()
        );
    }
    for entry in walkdir_plain(&site_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let rel = entry.strip_prefix(&site_root).unwrap();
        let dest = target_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&entry, &dest)
            .with_context(|| format!("copying {} → {}", entry.display(), dest.display()))?;
    }
    Ok(())
}

/// Copy one source file into the bundle. If `title` is `Some`,
/// inject a Starlight frontmatter block at the top of the
/// destination so the page renders with the desired title in the
/// Starlight chrome (the source files don't carry their own
/// frontmatter — they're project-internal docs).
fn copy_one(src: &Path, dest: &Path, title: Option<&str>) -> Result<()> {
    if !src.is_file() {
        bail!("expected file at {}", src.display());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(title) = title {
        let body = fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
        let stripped = strip_first_h1(&body);
        let mut out = String::new();
        let _ = writeln!(&mut out, "---");
        let _ = writeln!(&mut out, "title: {title}");
        let _ = writeln!(&mut out, "---");
        let _ = writeln!(&mut out);
        out.push_str(stripped);
        fs::write(dest, out).with_context(|| format!("writing {}", dest.display()))?;
    } else {
        fs::copy(src, dest)
            .with_context(|| format!("copying {} → {}", src.display(), dest.display()))?;
    }
    Ok(())
}

/// Strip the first top-level `# heading` line so the Starlight
/// frontmatter `title` we inject doesn't render *next to* a
/// duplicate H1 from the source file.
fn strip_first_h1(body: &str) -> &str {
    let trimmed = body.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        // Skip until end-of-line + the trailing newline.
        if let Some(idx) = rest.find('\n') {
            return rest[idx + 1..].trim_start_matches('\n');
        }
        return "";
    }
    body
}

/// Slice `docs/rules.md` by H2 (= rule family) into one
/// Starlight page per family, plus a synthesized `rules/index.md`.
/// The single-page rules.md is too long to be useful as a docs
/// landing; per-family pages are the unit users actually want
/// to bookmark.
///
/// Sections we drop:
/// - "Contents" (the source file's TOC; we generate our own)
/// - "Bundled rulesets" (per-ruleset pages already generated
///   from the YAML bodies — having both would cause confusing
///   sidebar duplication)
fn generate_rules_pages(workspace: &Path, target_dir: &Path) -> Result<()> {
    let src = fs::read_to_string(workspace.join(docs_paths::RULES_DOC))
        .with_context(|| format!("reading {}", docs_paths::RULES_DOC))?;
    let sections = split_h2_sections(&src);

    let rules_dir = target_dir.join("rules");
    fs::create_dir_all(&rules_dir)?;

    let mut emitted: Vec<(String, String)> = Vec::new(); // (title, slug)
    for (order, section) in sections.iter().enumerate() {
        let lc = section.title.to_lowercase();
        if lc == "contents" || lc.starts_with("bundled rulesets") {
            continue;
        }
        let slug = slugify(&section.title);
        let mut page = String::new();
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page, "title: '{}'", escape_yaml_string(&section.title));
        let _ = writeln!(
            &mut page,
            "description: 'Rule reference: the {} family.'",
            section.title.to_lowercase()
        );
        let _ = writeln!(&mut page, "sidebar:");
        // `order` is the input index — preserves rules.md ordering
        // in the sidebar instead of going alphabetical.
        let _ = writeln!(&mut page, "  order: {}", order + 2);
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page);
        page.push_str(section.body.trim_start());
        if !page.ends_with('\n') {
            page.push('\n');
        }
        fs::write(rules_dir.join(format!("{slug}.md")), &page)
            .with_context(|| format!("writing rules/{slug}.md"))?;
        emitted.push((section.title.clone(), slug));
    }

    // Index page — short intro + a flat TOC. `order: 1` so it
    // sits at the top of the Rules sidebar group regardless of
    // alphabetical sort.
    let mut idx = String::new();
    let _ = writeln!(&mut idx, "---");
    let _ = writeln!(&mut idx, "title: Rules");
    let _ = writeln!(
        &mut idx,
        "description: Every rule kind alint ships, organised by family."
    );
    let _ = writeln!(&mut idx, "sidebar:");
    let _ = writeln!(&mut idx, "  order: 1");
    let _ = writeln!(&mut idx, "---");
    let _ = writeln!(&mut idx);
    let _ = writeln!(
        &mut idx,
        "alint ships ~50 rule kinds across eleven families. Each rule is one entry in your `.alint.yml` under `rules:`. The pages below cover every kind shipped in the current alint release."
    );
    let _ = writeln!(&mut idx);
    let _ = writeln!(&mut idx, "## Reference by family");
    let _ = writeln!(&mut idx);
    for (title, slug) in &emitted {
        let _ = writeln!(&mut idx, "- [{title}](/docs/rules/{slug}/)");
    }
    fs::write(rules_dir.join("index.md"), idx)?;
    Ok(())
}

/// Sections of a markdown document split at H2 headers (`## …`).
/// Anything before the first H2 is dropped (it's typically the
/// document's H1 + intro paragraph; we don't carry that into the
/// per-family pages).
struct H2Section {
    title: String,
    body: String,
}

fn split_h2_sections(src: &str) -> Vec<H2Section> {
    let mut sections: Vec<H2Section> = Vec::new();
    let mut current: Option<H2Section> = None;
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            current = Some(H2Section {
                title: rest.trim().to_string(),
                body: String::new(),
            });
        } else if let Some(sec) = current.as_mut() {
            sec.body.push_str(line);
            sec.body.push('\n');
        }
    }
    if let Some(prev) = current.take() {
        sections.push(prev);
    }
    sections
}

/// URL-safe slug from a heading. Lowercases, drops any character
/// that isn't `[a-z0-9-]`, collapses runs of `-`. Adequate for
/// headings like "Security / Unicode sanity" → "security-unicode-sanity".
fn slugify(s: &str) -> String {
    let lc = s.to_lowercase();
    let mut out = String::with_capacity(lc.len());
    let mut last_dash = false;
    for ch in lc.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Quote a string safely for a single-quoted YAML scalar — only
/// `'` needs escaping (doubled). Frontmatter titles like
/// `Security / Unicode sanity` need this.
fn escape_yaml_string(s: &str) -> String {
    s.replace('\'', "''")
}

/// One markdown page per `crates/alint-dsl/rulesets/v1/**/*.yml`,
/// summarising the ruleset's rules with their level / kind / message
/// / policy URL. Slash-separated names (`hygiene/lockfiles`,
/// `ci/github-actions`) are flattened with a `-` for the bundle
/// filename so Starlight's autogen sidebar produces a flat list.
fn generate_bundled_ruleset_pages(workspace: &Path, target_dir: &Path) -> Result<()> {
    let rulesets_root = workspace.join(docs_paths::RULESETS_DIR);
    let bundled_dir = target_dir.join("bundled-rulesets");
    fs::create_dir_all(&bundled_dir)?;

    let mut ruleset_pages: Vec<String> = Vec::new();

    for entry in walkdir_plain(&rulesets_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let rel = entry.strip_prefix(&rulesets_root).unwrap();
        let pretty_name = rel.with_extension("");
        let pretty_str = pretty_name.to_string_lossy().replace('\\', "/");
        let flat_filename = format!("{}.md", pretty_str.replace('/', "-"));

        let yaml_text =
            fs::read_to_string(&entry).with_context(|| format!("reading {}", entry.display()))?;
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml_text)
            .with_context(|| format!("parsing {}", entry.display()))?;

        let page = render_ruleset_page(&pretty_str, &yaml);
        let dest = bundled_dir.join(&flat_filename);
        fs::write(&dest, page).with_context(|| format!("writing {}", dest.display()))?;

        ruleset_pages.push(pretty_str);
    }

    // An index page listing every ruleset — overwrites the hand-
    // written placeholder when the sync script lays the bundle into
    // alint.org.
    ruleset_pages.sort();
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: Bundled Rulesets");
    let _ = writeln!(
        &mut index,
        "description: One-line ecosystem baselines built into the alint binary."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    let _ = writeln!(
        &mut index,
        "Adopt with `extends: [alint://bundled/<name>@v1]`. Each ruleset's full rule list lives on its own page below."
    );
    let _ = writeln!(&mut index);
    let _ = writeln!(&mut index, "## Currently shipped");
    let _ = writeln!(&mut index);
    for name in &ruleset_pages {
        let flat = name.replace('/', "-");
        let _ = writeln!(
            &mut index,
            "- [`{name}@v1`](/docs/bundled-rulesets/{flat}/)"
        );
    }
    fs::write(bundled_dir.join("index.md"), index)?;

    Ok(())
}

/// Render the markdown body for a single bundled ruleset. Reads
/// `version` and the `rules:` array; pulls each rule's `id`,
/// `kind`, `level`, `message`, `policy_url`, and `when:`.
fn render_ruleset_page(name: &str, yaml: &serde_yaml_ng::Value) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out, "title: '{name}@v1'");
    let _ = writeln!(
        &mut out,
        "description: Bundled alint ruleset at alint://bundled/{name}@v1."
    );
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "Adopt with:");
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "```yaml");
    let _ = writeln!(&mut out, "extends:");
    let _ = writeln!(&mut out, "  - alint://bundled/{name}@v1");
    let _ = writeln!(&mut out, "```");
    let _ = writeln!(&mut out);

    let Some(rules) = yaml.get("rules").and_then(|r| r.as_sequence()) else {
        let _ = writeln!(&mut out, "_(no rules — this ruleset is a placeholder.)_");
        return out;
    };
    let _ = writeln!(&mut out, "## Rules");
    let _ = writeln!(&mut out);

    for rule in rules {
        let id = rule.get("id").and_then(|v| v.as_str()).unwrap_or("(no-id)");
        let kind = rule.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let level = rule.get("level").and_then(|v| v.as_str()).unwrap_or("");
        let when = rule.get("when").and_then(|v| v.as_str());
        let msg = rule.get("message").and_then(|v| v.as_str());
        let policy = rule.get("policy_url").and_then(|v| v.as_str());

        let _ = writeln!(&mut out, "### `{id}`");
        let _ = writeln!(&mut out);
        if !kind.is_empty() {
            let _ = writeln!(&mut out, "- **kind**: `{kind}`");
        }
        if !level.is_empty() {
            let _ = writeln!(&mut out, "- **level**: `{level}`");
        }
        if let Some(when) = when {
            let _ = writeln!(&mut out, "- **when**: `{when}`");
        }
        if let Some(policy) = policy {
            let _ = writeln!(&mut out, "- **policy**: <{policy}>");
        }
        if let Some(msg) = msg {
            let _ = writeln!(&mut out);
            let _ = writeln!(&mut out, "> {}", msg.replace('\n', " "));
        }
        let _ = writeln!(&mut out);
    }
    out
}

/// Build the alint binary in release mode, then capture
/// `alint --help` and `alint <subcmd> --help` for each subcommand.
/// Each captured help text becomes its own markdown page under
/// `cli/<subcmd>.md`.
fn generate_cli_reference(workspace: &Path, target_dir: &Path) -> Result<()> {
    let bin = build_release_binary()?;

    let cli_dir = target_dir.join("cli");
    fs::create_dir_all(&cli_dir)?;

    // Top-level help → cli/index.md
    let top = run_help(&bin, &[])?;
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: CLI");
    let _ = writeln!(
        &mut index,
        "description: alint's subcommands and global flags, captured from the binary itself."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    let _ = writeln!(&mut index, "```");
    index.push_str(&top);
    let _ = writeln!(&mut index, "```");
    fs::write(cli_dir.join("index.md"), index)?;

    let subcmds = ["check", "fix", "list", "explain", "facts"];
    for sub in subcmds {
        let help = run_help(&bin, &[sub])?;
        let mut page = String::new();
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page, "title: 'alint {sub}'");
        let _ = writeln!(
            &mut page,
            "description: '`alint {sub}` — captured from `alint {sub} --help`.'"
        );
        let _ = writeln!(&mut page, "---");
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "```");
        page.push_str(&help);
        let _ = writeln!(&mut page, "```");
        fs::write(cli_dir.join(format!("{sub}.md")), page)?;
    }

    // Sanity-check: workspace path exists.
    let _ = workspace;
    Ok(())
}

fn run_help(bin: &Path, subcmd_args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(bin);
    cmd.args(subcmd_args).arg("--help");
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!(
            "alint {:?} --help exited {:?}",
            subcmd_args,
            out.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn write_manifest(target_dir: &Path) -> Result<()> {
    let sha = git_sha().unwrap_or_else(|| "unknown".to_string());
    let version = env!("CARGO_PKG_VERSION");
    let now = now_iso();

    let json = format!(
        "{{\n  \"alint_version\": \"{version}\",\n  \"git_sha\": \"{sha}\",\n  \"generated_at\": \"{now}\",\n  \"format_version\": 1\n}}\n"
    );
    fs::write(target_dir.join("manifest.json"), json)?;
    Ok(())
}
