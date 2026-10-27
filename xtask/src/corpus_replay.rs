//! `corpus-replay` — run `alint check` over the v0.12 case-study corpus at its
//! pinned commits, and optionally diff two binaries' reports.
//!
//! The corpus is 110 real repositories, each with the config a v0.12 analyst
//! wrote for it (`docs/design/v0.12/corpus/`), pinned by full commit SHA in
//! `pins.tsv`. Each repository is fetched once, at depth 1, into the cache and
//! reused by later runs. `--compare` runs a second binary over the same trees
//! and reports every repository whose JSON report (proposed edits included) or
//! baseline fingerprints differ: this is how v0.21's shadow port of
//! `cross_file` `equals` is checked on real configurations (ADR-0023). A
//! config with analysis failures cannot produce a baseline by design; both
//! binaries then compare an empty fingerprint list and the check report still
//! carries the analysis failures.
//!
//! Nothing from a corpus repository is executed. Before a config runs, every
//! rule whose kind spawns a process, reads the host keyring, or is unknown to
//! this build's kind catalog is removed, and so is every `fix:` block with a
//! spawning or unknown operation. A config with `custom:` facts, which run
//! commands, is not run at all. alint runs with a fresh `HOME`, a `PATH` of
//! absolute directories only, and no inherited environment.
//!
//! Outcomes: `replayed`; `skipped` (alint exits 2, usually a config the
//! release no longer loads, or a refused config); `unavailable` (the
//! repository or commit is gone upstream); `crashed` (any other failure,
//! including local fetch errors and timeouts); `differs`. A crash or a
//! difference fails the run, and with `--expect` so does any skipped or
//! unavailable repository that is not listed, or a listed one that now replays.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use alint_core::{Capability, FixTrustClass, KindCatalog};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value as Json;
use yaml_serde::Value as Yaml;

/// The committed pin list, relative to the workspace root.
const PINS: &str = "docs/design/v0.12/corpus/pins.tsv";
/// One `alint check` over the largest corpus trees takes seconds; a run this
/// long is hung, not slow.
const RUN_TIMEOUT: Duration = Duration::from_secs(900);
/// A depth-1 fetch of the largest corpus repository takes about a minute.
const FETCH_TIMEOUT: Duration = Duration::from_secs(1200);
/// What git prints when the repository or the pinned commit no longer exists
/// upstream. Any other fetch failure is local and counts as a crash.
const GONE_UPSTREAM: &[&str] = &[
    "Repository not found",
    "not our ref",
    "couldn't find remote ref",
    "unadvertised object",
    "does not appear to be a git repository",
    "returned error: 404",
];

#[derive(Debug, Clone, clap::Args)]
pub struct Args {
    /// The alint binary to run (relative to the current directory).
    #[arg(long, default_value = "target/release/alint")]
    pub alint: PathBuf,
    /// A second alint binary. Every repository whose report or baseline
    /// fingerprints differ from `--alint`'s is a difference, and any
    /// difference fails the run.
    #[arg(long)]
    pub compare: Option<PathBuf>,
    /// Cache for the repository checkouts, derived configs and run homes.
    #[arg(long, default_value = "target/corpus-replay")]
    pub cache: PathBuf,
    /// Only these repositories (`owner/name`); repeatable.
    #[arg(long = "only")]
    pub only: Vec<String>,
    /// Repositories processed in parallel.
    #[arg(long, default_value_t = 4)]
    pub jobs: usize,
    /// Never fetch; a repository missing from the cache is unavailable.
    #[arg(long)]
    pub offline: bool,
    /// The expected skipped and unavailable repositories (`repo<TAB>verdict`
    /// lines). Any other non-replayed repository, or a listed one that
    /// replays, fails the run.
    #[arg(long)]
    pub expect: Option<PathBuf>,
    /// Write the results as JSON.
    #[arg(long)]
    pub out_json: Option<PathBuf>,
    /// Append the Markdown report here (e.g. `$GITHUB_STEP_SUMMARY`).
    #[arg(long)]
    pub summary: Option<PathBuf>,
}

/// One row of `pins.tsv`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pin {
    repo: String,
    sha: String,
    config: String,
}

impl Pin {
    /// The cache directory name: `owner-name`, as the config files are named.
    fn slug(&self) -> String {
        self.repo.replace('/', "-")
    }
}

fn parse_pins(text: &str) -> Result<Vec<Pin>> {
    let mut pins = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = || format!("{PINS}:{}", index + 1);
        let [repo, sha, config] = line.split('\t').collect::<Vec<_>>()[..] else {
            bail!("{}: expected three tab-separated columns", at());
        };
        let name_ok = |s: &str| {
            !s.is_empty()
                && !s.starts_with('.')
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        let Some((owner, name)) = repo.split_once('/') else {
            bail!("{}: repository {repo:?} is not owner/name", at());
        };
        if !name_ok(owner) || !name_ok(name) {
            bail!("{}: repository {repo:?} has an unexpected character", at());
        }
        if sha.len() != 40 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            bail!("{}: {sha:?} is not a full lowercase commit SHA", at());
        }
        if config != format!("{owner}-{name}.alint.yml") {
            bail!("{}: config {config:?} does not match {repo}", at());
        }
        pins.push(Pin {
            repo: repo.to_string(),
            sha: sha.to_string(),
            config: config.to_string(),
        });
    }
    Ok(pins)
}

/// `repo<TAB>verdict[<TAB>note]` lines: the expected non-replayed repositories.
fn parse_expectations(text: &str) -> Result<BTreeMap<String, String>> {
    let mut expected = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut columns = line.split('\t');
        let (Some(repo), Some(verdict)) = (columns.next(), columns.next()) else {
            bail!("expectations line {}: expected repo<TAB>verdict", index + 1);
        };
        if !matches!(verdict, "skipped" | "unavailable") {
            bail!(
                "expectations line {}: verdict {verdict:?} is not skipped or unavailable",
                index + 1
            );
        }
        if expected
            .insert(repo.to_string(), verdict.to_string())
            .is_some()
        {
            bail!("expectations line {}: {repo} is listed twice", index + 1);
        }
    }
    Ok(expected)
}

/// Whether a rule of `kind` must not run on a corpus repository: it spawns a
/// process, reads the host keyring, or this build's catalog does not know it
/// (so nothing is known about what it does).
fn kind_must_not_run(kind: &str, catalog: KindCatalog<'_>) -> bool {
    catalog.meta(kind).is_none_or(|meta| {
        meta.capabilities.iter().any(|entry| {
            matches!(
                entry.capability,
                Capability::SpawnUserCommand | Capability::HostKeyring
            )
        })
    })
}

/// Whether a rule, or any rule nested in it (`require:`), must not run.
fn rule_must_not_run(value: &Yaml, catalog: KindCatalog<'_>) -> bool {
    match value {
        Yaml::Mapping(map) => map.iter().any(|(key, value)| {
            (key.as_str() == Some("kind")
                && value
                    .as_str()
                    .is_some_and(|k| kind_must_not_run(k, catalog)))
                || rule_must_not_run(value, catalog)
        }),
        Yaml::Sequence(items) => items.iter().any(|item| rule_must_not_run(item, catalog)),
        Yaml::Tagged(tagged) => rule_must_not_run(&tagged.value, catalog),
        _ => false,
    }
}

/// Whether a `fix:` block holds an operation that spawns or that the catalog
/// does not know.
fn fix_must_not_run(fix: &Yaml, catalog: KindCatalog<'_>) -> bool {
    fix.as_mapping().is_some_and(|ops| {
        ops.keys().any(|op| {
            op.as_str()
                .and_then(|name| catalog.fix_op(name))
                .is_none_or(|meta| meta.trust == FixTrustClass::Spawning)
        })
    })
}

/// Rules taking the `cross_file` `equals` path, nested rules included: the
/// alias, or `cross_file` with `relation: equals` or no relation (its default).
fn count_equals_rules(value: &Yaml, catalog: KindCatalog<'_>) -> usize {
    match value {
        Yaml::Mapping(map) => {
            let own = map
                .get("kind")
                .and_then(Yaml::as_str)
                .is_some_and(|kind| catalog.canonical(kind) == "cross_file")
                && map
                    .get("relation")
                    .and_then(Yaml::as_str)
                    .is_none_or(|relation| relation == "equals");
            usize::from(own)
                + map
                    .iter()
                    .filter(|(key, _)| key.as_str() != Some("kind"))
                    .map(|(_, value)| count_equals_rules(value, catalog))
                    .sum::<usize>()
        }
        Yaml::Sequence(items) => items.iter().map(|i| count_equals_rules(i, catalog)).sum(),
        Yaml::Tagged(tagged) => count_equals_rules(&tagged.value, catalog),
        _ => 0,
    }
}

/// A config made safe to run, what was removed from it, and how many
/// remaining rules take the `cross_file` `equals` path.
struct Derived {
    yaml: String,
    dropped: Vec<String>,
    equals_rules: usize,
    /// Set when the config cannot be made safe and must not run at all.
    refusal: Option<String>,
}

fn derive_config(source: &str, catalog: KindCatalog<'_>) -> Result<Derived> {
    let mut doc: Yaml = yaml_serde::from_str(source).context("parse the config")?;
    let custom_facts: Vec<String> = doc
        .get("facts")
        .and_then(Yaml::as_sequence)
        .into_iter()
        .flatten()
        .filter(|fact| fact.get("custom").is_some())
        .map(|fact| {
            fact.get("id")
                .and_then(Yaml::as_str)
                .unwrap_or("<no id>")
                .to_string()
        })
        .collect();
    let refusal = (!custom_facts.is_empty()).then(|| {
        format!(
            "custom facts run commands, so the config is not run: {}",
            custom_facts.join(", ")
        )
    });
    let mut dropped = Vec::new();
    if let Some(rules) = doc.get_mut("rules").and_then(Yaml::as_sequence_mut) {
        rules.retain_mut(|rule| {
            let id = rule
                .get("id")
                .and_then(Yaml::as_str)
                .unwrap_or("<no id>")
                .to_string();
            if rule_must_not_run(rule, catalog) {
                dropped.push(id);
                return false;
            }
            if let Some(map) = rule.as_mapping_mut()
                && map
                    .get("fix")
                    .is_some_and(|fix| fix_must_not_run(fix, catalog))
            {
                map.remove("fix");
                dropped.push(format!("{id} (fix)"));
            }
            true
        });
    }
    let equals_rules = doc
        .get("rules")
        .map_or(0, |rules| count_equals_rules(rules, catalog));
    let yaml = yaml_serde::to_string(&doc).context("serialize the derived config")?;
    Ok(Derived {
        yaml,
        dropped,
        equals_rules,
        refusal,
    })
}

/// What one binary did with one repository.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Outcome {
    /// The check completed (exit 0 or 1) with a parseable report.
    Checked {
        violations: u64,
        analysis_failures: u64,
        #[serde(skip)]
        report: Json,
        /// The baseline file's entries, in compare mode only.
        #[serde(skip)]
        fingerprints: Option<Json>,
    },
    /// alint exited 2: a config this release does not load, or a usage or
    /// runtime error it attributes to the input.
    ConfigError { reason: String },
    /// Exit 3, a panic, a signal, a timeout, or an unparseable report.
    Crashed { reason: String },
}

#[derive(Debug, Clone, Serialize)]
struct RepoResult {
    repo: String,
    sha: String,
    /// `replayed`, `skipped`, `unavailable`, `crashed`, or `differs`.
    verdict: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    dropped_rules: Vec<String>,
    equals_rules: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    alint: Option<Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compare: Option<Outcome>,
    /// Rule ids whose results differ, with both sides' entries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    differences: Vec<Difference>,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct Difference {
    rule: String,
    alint: Option<Json>,
    compare: Option<Json>,
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    alint_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    compare_version: Option<&'a str>,
    results: &'a [RepoResult],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    expectation_failures: Vec<String>,
}

pub fn run(args: &Args) -> Result<()> {
    let root = crate::workspace_root()?;
    let pins_text =
        std::fs::read_to_string(root.join(PINS)).with_context(|| format!("read {PINS}"))?;
    let mut pins = parse_pins(&pins_text)?;
    if !args.only.is_empty() {
        for wanted in &args.only {
            if !pins.iter().any(|pin| &pin.repo == wanted) {
                bail!("--only {wanted}: not in {PINS}");
            }
        }
        pins.retain(|pin| args.only.contains(&pin.repo));
    }
    let expected = args
        .expect
        .as_deref()
        .map(|path| {
            std::fs::read_to_string(path)
                .with_context(|| format!("read {}", path.display()))
                .and_then(|text| parse_expectations(&text))
        })
        .transpose()?;
    let alint = absolute(&args.alint)?;
    let compare = args.compare.as_deref().map(absolute).transpose()?;
    let mut versions = Vec::new();
    for bin in std::iter::once(&alint).chain(compare.as_ref()) {
        if !bin.is_file() {
            bail!(
                "alint binary {} does not exist; build it first",
                bin.display()
            );
        }
        versions.push(version_of(bin)?);
    }
    let cache = absolute(&args.cache)?;
    for sub in ["repos", "configs", "home", "baselines"] {
        std::fs::create_dir_all(cache.join(sub))?;
    }

    let registry = alint_rules::builtin_registry();
    let catalog = registry.catalog();
    let corpus = root.join("docs/design/v0.12/corpus");
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let jobs = args.jobs.clamp(1, 32);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(pin) = pins.get(index) else { break };
                    let result = replay_one(
                        pin,
                        &corpus,
                        &cache,
                        &alint,
                        compare.as_deref(),
                        catalog,
                        args.offline,
                    );
                    eprintln!("corpus-replay: {:<40} {}", pin.repo, result.verdict);
                    results.lock().unwrap().push(result);
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|result| result.repo.to_lowercase());

    let expectation_failures = expected
        .as_ref()
        .map(|expected| check_expectations(&results, expected, &args.only))
        .unwrap_or_default();
    write_reports(args, &results, &versions, &expectation_failures)?;
    let failed: Vec<&str> = results
        .iter()
        .filter(|r| matches!(r.verdict, "crashed" | "differs"))
        .map(|r| r.repo.as_str())
        .collect();
    if !failed.is_empty() {
        bail!(
            "{} repositories crashed or differ: {}",
            failed.len(),
            failed.join(", ")
        );
    }
    if !expectation_failures.is_empty() {
        bail!(
            "the replay outcome does not match the expectations: {}",
            expectation_failures.join("; ")
        );
    }
    Ok(())
}

/// The Markdown report (stdout and `--summary`) and the JSON report.
fn write_reports(
    args: &Args,
    results: &[RepoResult],
    versions: &[String],
    expectation_failures: &[String],
) -> Result<()> {
    let report = render_markdown(
        results,
        args.compare.is_some(),
        versions,
        expectation_failures,
    );
    print!("{report}");
    if let Some(path) = &args.summary {
        let mut existing = std::fs::read_to_string(path).unwrap_or_default();
        existing.push_str(&report);
        std::fs::write(path, existing).with_context(|| format!("write {}", path.display()))?;
    }
    if let Some(path) = &args.out_json {
        let json = Report {
            alint_version: &versions[0],
            compare_version: versions.get(1).map(String::as_str),
            results,
            expectation_failures: expectation_failures.to_vec(),
        };
        std::fs::write(path, serde_json::to_string_pretty(&json)?)
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

/// Every skipped or unavailable repository must be listed with that verdict,
/// and every listed repository (among those run) must still not replay.
fn check_expectations(
    results: &[RepoResult],
    expected: &BTreeMap<String, String>,
    only: &[String],
) -> Vec<String> {
    let mut failures = Vec::new();
    for result in results {
        let listed = expected.get(&result.repo).map(String::as_str);
        match (result.verdict, listed) {
            ("skipped" | "unavailable", Some(want)) if want == result.verdict => {}
            ("skipped" | "unavailable", _) => failures.push(format!(
                "{} is unexpectedly {} ({})",
                result.repo,
                result.verdict,
                result.reason.as_deref().unwrap_or("no reason")
            )),
            ("replayed", Some(want)) => failures.push(format!(
                "{} was expected to be {want} but replayed; update the expectations",
                result.repo
            )),
            _ => {}
        }
    }
    for repo in expected.keys() {
        if (only.is_empty() || only.contains(repo)) && !results.iter().any(|r| &r.repo == repo) {
            failures.push(format!(
                "{repo} is listed in the expectations but not pinned"
            ));
        }
    }
    failures
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

fn version_of(bin: &Path) -> Result<String> {
    let output = Command::new(bin)
        .arg("--version")
        .output()
        .with_context(|| format!("run {} --version", bin.display()))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn replay_one(
    pin: &Pin,
    corpus: &Path,
    cache: &Path,
    alint: &Path,
    compare: Option<&Path>,
    catalog: KindCatalog<'_>,
    offline: bool,
) -> RepoResult {
    let started = Instant::now();
    let mut result = RepoResult {
        repo: pin.repo.clone(),
        sha: pin.sha.clone(),
        verdict: "replayed",
        reason: None,
        dropped_rules: Vec::new(),
        equals_rules: 0,
        alint: None,
        compare: None,
        differences: Vec::new(),
        elapsed_ms: 0,
    };
    let finish = |mut result: RepoResult, verdict, reason: Option<String>| {
        result.verdict = verdict;
        result.reason = reason;
        result.elapsed_ms = started.elapsed().as_millis();
        result
    };

    let derived = match std::fs::read_to_string(corpus.join(&pin.config))
        .map_err(anyhow::Error::from)
        .and_then(|source| derive_config(&source, catalog))
    {
        Ok(derived) => derived,
        Err(error) => {
            return finish(
                result,
                "crashed",
                Some(format!("{}: {error:#}", pin.config)),
            );
        }
    };
    result.dropped_rules.clone_from(&derived.dropped);
    result.equals_rules = derived.equals_rules;
    if let Some(refusal) = derived.refusal {
        return finish(result, "skipped", Some(refusal));
    }
    let config = cache.join("configs").join(&pin.config);
    if let Err(error) = std::fs::write(&config, &derived.yaml) {
        return finish(
            result,
            "crashed",
            Some(format!("write {}: {error}", config.display())),
        );
    }

    let checkout = cache.join("repos").join(pin.slug());
    match ensure_checkout(&checkout, pin, offline) {
        Ok(()) => {}
        Err(Fetch::Gone(reason)) => return finish(result, "unavailable", Some(reason)),
        Err(Fetch::Failed(reason)) => return finish(result, "crashed", Some(reason)),
    }

    let baselines = compare.is_some().then(|| cache.join("baselines"));
    let first = check(
        alint,
        &checkout,
        &config,
        cache,
        &pin.slug(),
        baselines.as_deref(),
    );
    let second = compare.map(|bin| {
        check(
            bin,
            &checkout,
            &config,
            cache,
            &pin.slug(),
            baselines.as_deref(),
        )
    });
    if let Err(reason) = ensure_pristine(&checkout) {
        result.alint = Some(first);
        result.compare = second;
        return finish(
            result,
            "crashed",
            Some(format!("alint changed the checkout: {reason}")),
        );
    }

    let (verdict, reason, differences) = classify(&first, second.as_ref());
    result.differences = differences;
    result.alint = Some(first);
    result.compare = second;
    finish(result, verdict, reason)
}

/// The verdict for one repository from one or two outcomes.
fn classify(
    first: &Outcome,
    second: Option<&Outcome>,
) -> (&'static str, Option<String>, Vec<Difference>) {
    let crash = |outcome: &Outcome| match outcome {
        Outcome::Crashed { reason } => Some(reason.clone()),
        _ => None,
    };
    if let Some(reason) = crash(first).or_else(|| second.and_then(crash)) {
        return ("crashed", Some(reason), Vec::new());
    }
    let Some(second) = second else {
        return match first {
            Outcome::ConfigError { reason } => ("skipped", Some(reason.clone()), Vec::new()),
            _ => ("replayed", None, Vec::new()),
        };
    };
    match (first, second) {
        // Both refused the config: the same outcome, whatever the wording.
        (Outcome::ConfigError { reason }, Outcome::ConfigError { .. }) => {
            ("skipped", Some(reason.clone()), Vec::new())
        }
        (
            Outcome::Checked {
                report: a,
                fingerprints: fa,
                ..
            },
            Outcome::Checked {
                report: b,
                fingerprints: fb,
                ..
            },
        ) => {
            let differences = differing_rules(a, b);
            if !differences.is_empty() {
                let reason = format!("{} rule result(s) differ", differences.len());
                return ("differs", Some(reason), differences);
            }
            if a != b {
                let reason = "the reports differ outside the rule results".to_string();
                return ("differs", Some(reason), Vec::new());
            }
            if fa != fb {
                let reason = "the baseline fingerprints differ".to_string();
                return ("differs", Some(reason), Vec::new());
            }
            ("replayed", None, Vec::new())
        }
        _ => (
            "differs",
            Some("the binaries disagree on whether the config loads".to_string()),
            Vec::new(),
        ),
    }
}

/// Rule results that differ, or appear in only one report, with both sides.
fn differing_rules(a: &Json, b: &Json) -> Vec<Difference> {
    let index = |report: &Json| -> BTreeMap<String, Json> {
        report["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|rule| {
                (
                    rule["id"].as_str().unwrap_or_default().to_string(),
                    rule.clone(),
                )
            })
            .collect()
    };
    let (a, b) = (index(a), index(b));
    let mut ids: Vec<&String> = a.keys().chain(b.keys()).collect();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter(|id| a.get(*id) != b.get(*id))
        .map(|id| Difference {
            rule: id.clone(),
            alint: a.get(id).cloned(),
            compare: b.get(id).cloned(),
        })
        .collect()
}

fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .stdin(Stdio::null());
    command
}

/// Why a checkout could not be made ready.
enum Fetch {
    /// The repository or the pinned commit no longer exists upstream.
    Gone(String),
    /// Anything else: a local or network failure, which must not pass.
    Failed(String),
}

fn git_ok(command: &mut Command, what: &str, timeout: Duration) -> Result<String, Fetch> {
    let output = run_with_timeout(command, timeout)
        .map_err(|reason| Fetch::Failed(format!("{what}: {reason}")))?;
    if output.code == Some(0) {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.trim().lines().last().unwrap_or("failed").to_string();
    if GONE_UPSTREAM.iter().any(|marker| stderr.contains(marker)) {
        Err(Fetch::Gone(format!("{what}: {last}")))
    } else {
        Err(Fetch::Failed(format!("{what}: {last}")))
    }
}

const LOCAL_GIT: Duration = Duration::from_secs(120);

/// Make `dir` a pristine checkout of exactly `pin.sha`, fetching it at depth 1
/// if needed. A checkout left dirty or locked by an earlier run is rebuilt.
fn ensure_checkout(dir: &Path, pin: &Pin, offline: bool) -> Result<(), Fetch> {
    let stale = ["index.lock", "shallow.lock", "HEAD.lock"]
        .iter()
        .any(|lock| dir.join(".git").join(lock).exists());
    if dir.join(".git").is_dir() && (stale || ensure_pristine(dir).is_err()) {
        if offline {
            return Err(Fetch::Failed(
                "cached checkout is dirty or locked (--offline)".to_string(),
            ));
        }
        std::fs::remove_dir_all(dir)
            .map_err(|error| Fetch::Failed(format!("remove {}: {error}", dir.display())))?;
    }
    if !dir.join(".git").is_dir() {
        if offline {
            return Err(Fetch::Gone("not in the cache (--offline)".to_string()));
        }
        std::fs::create_dir_all(dir)
            .map_err(|error| Fetch::Failed(format!("create {}: {error}", dir.display())))?;
        git_ok(git(dir).args(["init", "-q"]), "git init", LOCAL_GIT)?;
        let url = format!("https://github.com/{}.git", pin.repo);
        git_ok(
            git(dir).args(["remote", "add", "origin", &url]),
            "git remote add",
            LOCAL_GIT,
        )?;
    }
    let head = git_ok(
        git(dir).args(["rev-parse", "--verify", "-q", "HEAD"]),
        "git rev-parse",
        LOCAL_GIT,
    );
    if head.as_ref().ok().map(String::as_str) != Some(pin.sha.as_str()) {
        if offline {
            return Err(Fetch::Failed(
                "cached checkout is not at the pin (--offline)".to_string(),
            ));
        }
        git_ok(
            git(dir).args([
                "-c",
                "http.lowSpeedLimit=1000",
                "-c",
                "http.lowSpeedTime=60",
                "fetch",
                "-q",
                "--depth",
                "1",
                "--no-tags",
                "origin",
                &pin.sha,
            ]),
            "git fetch",
            FETCH_TIMEOUT,
        )?;
        git_ok(
            git(dir).args([
                "-c",
                "advice.detachedHead=false",
                "checkout",
                "-q",
                "--force",
                "--detach",
                &pin.sha,
            ]),
            "git checkout",
            LOCAL_GIT,
        )?;
    }
    let head = git_ok(
        git(dir).args(["rev-parse", "HEAD"]),
        "git rev-parse",
        LOCAL_GIT,
    )?;
    if head != pin.sha {
        return Err(Fetch::Failed(format!(
            "checkout is at {head}, not {}",
            pin.sha
        )));
    }
    ensure_pristine(dir).map_err(Fetch::Failed)
}

/// The checkout matches its commit exactly: `alint check` must not write.
fn ensure_pristine(dir: &Path) -> Result<(), String> {
    let status = git_ok(
        git(dir).args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored",
        ]),
        "git status",
        LOCAL_GIT,
    )
    .map_err(|(Fetch::Gone(reason) | Fetch::Failed(reason))| reason)?;
    if status.is_empty() {
        Ok(())
    } else {
        let first = status.lines().next().unwrap_or_default();
        Err(format!("the checkout is not pristine: {first}"))
    }
}

/// `PATH` without empty, relative, or `.` entries, so a tracked executable in
/// the checkout (the working directory) can never shadow a real tool.
fn absolute_path_entries(entries: &std::ffi::OsStr) -> std::ffi::OsString {
    std::env::join_paths(std::env::split_paths(entries).filter(|entry| entry.is_absolute()))
        .unwrap_or_default()
}

/// One alint invocation in a scrubbed environment with its own fresh `HOME`.
fn run_alint(
    bin: &Path,
    args: &[&std::ffi::OsStr],
    checkout: &Path,
    cache: &Path,
    slug: &str,
) -> Result<Output, String> {
    let home = tempfile::Builder::new()
        .prefix(&format!("{slug}-"))
        .tempdir_in(cache.join("home"))
        .map_err(|error| format!("create a run HOME: {error}"))?;
    let mut command = Command::new(bin);
    command
        .args(args)
        .current_dir(checkout)
        .env_clear()
        .env(
            "PATH",
            absolute_path_entries(&std::env::var_os("PATH").unwrap_or_default()),
        )
        .env("HOME", home.path())
        .env("LANG", "C.UTF-8")
        .env("NO_COLOR", "1");
    run_with_timeout(&mut command, RUN_TIMEOUT)
}

/// The line that says why alint refused: its last `alint:` line, else the
/// last non-empty line.
fn refusal_reason(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines
        .iter()
        .rev()
        .find(|line| line.starts_with("alint:"))
        .or_else(|| lines.last())
        .map(|line| (*line).to_string())
        .unwrap_or_default()
}

/// Run `alint check` (proposed edits included) and, in compare mode, `alint
/// baseline` for the fingerprints, then classify the result.
fn check(
    bin: &Path,
    checkout: &Path,
    config: &Path,
    cache: &Path,
    slug: &str,
    baselines: Option<&Path>,
) -> Outcome {
    let args: Vec<&std::ffi::OsStr> = ["check", "--format", "json", "--include-fixes", "--config"]
        .iter()
        .map(std::ffi::OsStr::new)
        .chain([config.as_os_str()])
        .collect();
    let output = match run_alint(bin, &args, checkout, cache, slug) {
        Ok(output) => output,
        Err(reason) => return Outcome::Crashed { reason },
    };
    let mut outcome = match output.code {
        Some(0 | 1) => match serde_json::from_slice::<Json>(&output.stdout) {
            Ok(report) => Outcome::Checked {
                violations: report["summary"]["total_violations"].as_u64().unwrap_or(0),
                analysis_failures: report["summary"]["total_analysis_failures"]
                    .as_u64()
                    .unwrap_or(0),
                report,
                fingerprints: None,
            },
            Err(error) => {
                return Outcome::Crashed {
                    reason: format!("unparseable JSON report: {error}"),
                };
            }
        },
        Some(2) => {
            return Outcome::ConfigError {
                reason: refusal_reason(&output.stderr),
            };
        }
        Some(code) => {
            return Outcome::Crashed {
                reason: format!("exit {code}: {}", refusal_reason(&output.stderr)),
            };
        }
        None => {
            return Outcome::Crashed {
                reason: "killed by a signal".to_string(),
            };
        }
    };
    if let (Some(dir), Outcome::Checked { fingerprints, .. }) = (baselines, &mut outcome) {
        match baseline_entries(bin, checkout, config, cache, slug, dir) {
            Ok(entries) => *fingerprints = Some(entries),
            Err(reason) => return Outcome::Crashed { reason },
        }
    }
    outcome
}

/// The fingerprints `alint baseline` records, without the header's version.
fn baseline_entries(
    bin: &Path,
    checkout: &Path,
    config: &Path,
    cache: &Path,
    slug: &str,
    dir: &Path,
) -> Result<Json, String> {
    let file = tempfile::Builder::new()
        .prefix(&format!("{slug}-"))
        .suffix(".json")
        .tempfile_in(dir)
        .map_err(|error| format!("create a baseline file: {error}"))?;
    let path = file.path().to_path_buf();
    // `alint baseline` refuses to write over an existing file it did not create.
    drop(file);
    let _ = std::fs::remove_file(&path);
    let args: Vec<&std::ffi::OsStr> = vec![
        "baseline".as_ref(),
        "--config".as_ref(),
        config.as_os_str(),
        "--output".as_ref(),
        path.as_os_str(),
    ];
    let output = run_alint(bin, &args, checkout, cache, slug)?;
    let text = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    if output.code != Some(0) {
        let reason = refusal_reason(&output.stderr);
        // A config with analysis failures cannot produce a baseline by design:
        // `alint baseline` refuses to grandfather an incomplete analysis. The
        // check report still carries those failures and is compared; treat both
        // sides' fingerprint list as empty rather than turning this expected
        // outcome into a replay crash.
        if output.code == Some(1)
            && reason.contains("refusing to write baseline")
            && reason.contains("analysis failure")
        {
            return Ok(Json::Array(Vec::new()));
        }
        return Err(format!("alint baseline exited {:?}: {reason}", output.code));
    }
    // JSON Lines: a header (`schema_version`, `alint_version`), then one entry
    // per fingerprint.
    let text = text.map_err(|error| format!("read the baseline: {error}"))?;
    let mut lines = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut value: Json = serde_json::from_str(line)
            .map_err(|error| format!("unparseable baseline line: {error}"))?;
        if let Some(header) = value.as_object_mut() {
            header.remove("alint_version");
        }
        lines.push(value);
    }
    if lines.is_empty() {
        return Err("alint baseline wrote an empty file".to_string());
    }
    Ok(Json::Array(lines))
}

struct Output {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Run `command` to completion, killing it after `timeout`. Both pipes are
/// drained on their own threads so a large report cannot block the child.
fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn: {error}"))?;
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Err(error) => return Err(format!("wait: {error}")),
        }
    };
    Ok(Output {
        code: status.code(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn render_markdown(
    results: &[RepoResult],
    compared: bool,
    versions: &[String],
    expectation_failures: &[String],
) -> String {
    let count = |verdict: &str| results.iter().filter(|r| r.verdict == verdict).count();
    let replayed = count("replayed");
    let with_equals = results
        .iter()
        .filter(|r| r.verdict == "replayed" && r.equals_rules > 0)
        .count();
    let mut out = String::new();
    let _ = writeln!(out, "### Corpus replay\n");
    let _ = writeln!(out, "Binaries: {}.\n", versions.join(" against "));
    let _ = writeln!(
        out,
        "{} {}: {replayed} replayed ({with_equals} with `cross_file` `equals` rules), \
         {} skipped, {} unavailable, {} crashed{}.\n",
        results.len(),
        if results.len() == 1 {
            "repository"
        } else {
            "repositories"
        },
        count("skipped"),
        count("unavailable"),
        count("crashed"),
        if compared {
            format!(", {} differ", count("differs"))
        } else {
            String::new()
        },
    );
    let notable: Vec<&RepoResult> = results.iter().filter(|r| r.verdict != "replayed").collect();
    if !notable.is_empty() {
        let _ = writeln!(out, "| Repository | Verdict | Reason |\n|---|---|---|");
        for result in notable {
            let mut reason = result
                .reason
                .clone()
                .unwrap_or_default()
                .replace('|', "\\|");
            if !result.differences.is_empty() {
                let rules: Vec<&str> = result.differences.iter().map(|d| d.rule.as_str()).collect();
                let _ = write!(reason, " ({})", rules.join(", "));
            }
            let _ = writeln!(out, "| `{}` | {} | {reason} |", result.repo, result.verdict);
        }
        out.push('\n');
    }
    if !expectation_failures.is_empty() {
        let _ = writeln!(out, "**Not as expected:**\n");
        for failure in expectation_failures {
            let _ = writeln!(out, "- {failure}");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_test<T>(test: impl FnOnce(KindCatalog<'_>) -> T) -> T {
        let registry = alint_rules::builtin_registry();
        test(registry.catalog())
    }

    #[test]
    fn committed_pins_parse_and_name_existing_configs() {
        let root = crate::workspace_root().unwrap();
        let pins = parse_pins(&std::fs::read_to_string(root.join(PINS)).unwrap()).unwrap();
        assert_eq!(pins.len(), 110);
        for pin in &pins {
            assert!(
                root.join("docs/design/v0.12/corpus")
                    .join(&pin.config)
                    .is_file(),
                "{} names a missing config",
                pin.repo
            );
        }
        let mut repos: Vec<&str> = pins.iter().map(|pin| pin.repo.as_str()).collect();
        repos.sort_unstable();
        repos.dedup();
        assert_eq!(repos.len(), pins.len(), "a repository is pinned twice");
    }

    #[test]
    fn committed_expectations_parse_and_name_pinned_repositories() {
        let root = crate::workspace_root().unwrap();
        let pins = parse_pins(&std::fs::read_to_string(root.join(PINS)).unwrap()).unwrap();
        let text =
            std::fs::read_to_string(root.join("docs/design/v0.12/corpus/replay-expectations.tsv"))
                .unwrap();
        let expected = parse_expectations(&text).unwrap();
        assert!(!expected.is_empty());
        for repo in expected.keys() {
            assert!(
                pins.iter().any(|pin| &pin.repo == repo),
                "{repo} is not pinned"
            );
        }
    }

    #[test]
    fn pins_reject_malformed_rows() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert!(parse_pins(&format!("a/b\t{sha}\ta-b.alint.yml\r\n# note\n")).is_ok());
        for bad in [
            format!("a/b\t{sha}"),
            format!("a/../b\t{sha}\ta-..-b.alint.yml"),
            format!("a/b\t{}\ta-b.alint.yml", &sha[..10]),
            format!("a/b\t{}\ta-b.alint.yml", sha.to_uppercase()),
            format!("a/b\t{sha}\tother.alint.yml"),
            format!("a b/c\t{sha}\ta b-c.alint.yml"),
        ] {
            assert!(parse_pins(&bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn expectations_reject_unknown_verdicts_and_duplicates() {
        assert!(parse_expectations("a/b\tskipped\tnote\nc/d\tunavailable\n").is_ok());
        assert!(parse_expectations("a/b\tcrashed\n").is_err());
        assert!(parse_expectations("a/b\tskipped\na/b\tskipped\n").is_err());
        assert!(parse_expectations("a/b\n").is_err());
    }

    #[test]
    fn derive_removes_everything_that_could_run_a_command() {
        let source = r#"
version: 1
rules:
  - id: keep
    kind: file_exists
    paths: README.md
  - id: fmt
    kind: command_idempotent
    command: ["cargo", "fmt"]
  - id: nested
    kind: for_each_dir
    select: "crates/*"
    require:
      - kind: command
        command: ["true"]
  - id: signed
    kind: git_commit_gpg_signed
  - id: future
    kind: not_a_kind_this_build_knows
  - id: fixed
    kind: file_exists
    paths: LICENSE
    fix:
      command: ["touch", "LICENSE"]
  - id: safe-fix
    kind: file_exists
    paths: NOTICE
    fix:
      file_create:
        content: "x"
"#;
        let derived = catalog_test(|catalog| derive_config(source, catalog)).unwrap();
        assert_eq!(derived.refusal, None);
        assert_eq!(
            derived.dropped,
            ["fmt", "nested", "signed", "future", "fixed (fix)"]
        );
        let doc: Yaml = yaml_serde::from_str(&derived.yaml).unwrap();
        let rules = doc["rules"].as_sequence().unwrap();
        let ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["keep", "fixed", "safe-fix"]);
        assert!(rules[1].get("fix").is_none());
        assert!(rules[2].get("fix").is_some());
    }

    #[test]
    fn custom_facts_refuse_the_whole_config() {
        let source = "version: 1\nfacts:\n  - id: branch\n    custom:\n      argv: [\"git\", \"branch\"]\nrules: []\n";
        let derived = catalog_test(|catalog| derive_config(source, catalog)).unwrap();
        assert!(derived.refusal.unwrap().contains("branch"));
    }

    #[test]
    fn equals_rules_count_the_alias_the_default_and_nested_rules() {
        let source = r#"
version: 1
rules:
  - id: alias
    kind: cross_file_value_equals
  - id: explicit
    kind: cross_file
    relation: equals
  - id: default
    kind: cross_file
  - id: subset
    kind: cross_file
    relation: subset
  - id: nested
    kind: for_each_dir
    select: "crates/*"
    require:
      - kind: cross_file_value_equals
"#;
        let derived = catalog_test(|catalog| derive_config(source, catalog)).unwrap();
        assert_eq!(derived.equals_rules, 4);
    }

    #[test]
    fn every_spawning_kind_in_the_catalog_is_kept_off() {
        catalog_test(|catalog| {
            for kind in alint_dsl::SPAWNING_RULE_KINDS {
                assert!(kind_must_not_run(kind, catalog), "{kind} would run");
            }
            assert!(kind_must_not_run("git_commit_gpg_signed", catalog));
            assert!(!kind_must_not_run("file_exists", catalog));
            assert!(!kind_must_not_run("cross_file_value_equals", catalog));
        });
    }

    fn checked(results: &Json) -> Outcome {
        Outcome::Checked {
            violations: 0,
            analysis_failures: 0,
            report: serde_json::json!({ "schema_version": 1, "results": results }),
            fingerprints: Some(serde_json::json!({ "entries": [] })),
        }
    }

    #[test]
    fn classify_reports_differences_skips_and_fingerprints() {
        let a = checked(
            &serde_json::json!([{ "id": "r1", "passed": true }, { "id": "r2", "passed": true }]),
        );
        let b = checked(
            &serde_json::json!([{ "id": "r1", "passed": true }, { "id": "r2", "passed": false }]),
        );
        assert_eq!(classify(&a, None).0, "replayed");
        assert_eq!(classify(&a, Some(&a.clone())).0, "replayed");
        let (verdict, _, differences) = classify(&a, Some(&b));
        assert_eq!(verdict, "differs");
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].rule, "r2");
        assert_eq!(differences[0].compare.as_ref().unwrap()["passed"], false);

        let mut other_prints = a.clone();
        if let Outcome::Checked { fingerprints, .. } = &mut other_prints {
            *fingerprints = Some(serde_json::json!({ "entries": [{ "fingerprint": "x" }] }));
        }
        let (verdict, reason, _) = classify(&a, Some(&other_prints));
        assert_eq!(verdict, "differs");
        assert!(reason.unwrap().contains("fingerprints"));

        let config = Outcome::ConfigError {
            reason: "bad".into(),
        };
        let reworded = Outcome::ConfigError {
            reason: "worse".into(),
        };
        assert_eq!(classify(&config, None).0, "skipped");
        assert_eq!(classify(&config, Some(&reworded)).0, "skipped");
        assert_eq!(classify(&a, Some(&config)).0, "differs");
        let crash = Outcome::Crashed {
            reason: "exit 101".into(),
        };
        assert_eq!(classify(&a, Some(&crash)).0, "crashed");
        assert_eq!(classify(&crash, None).0, "crashed");
    }

    #[test]
    fn expectations_fail_on_new_skips_and_on_recoveries() {
        let result = |repo: &str, verdict| RepoResult {
            repo: repo.into(),
            sha: String::new(),
            verdict,
            reason: Some("why".into()),
            dropped_rules: Vec::new(),
            equals_rules: 0,
            alint: None,
            compare: None,
            differences: Vec::new(),
            elapsed_ms: 0,
        };
        let expected =
            parse_expectations("a/known\tskipped\nb/fixed\tskipped\nc/gone\tunavailable\n")
                .unwrap();
        let results = [
            result("a/known", "skipped"),
            result("b/fixed", "replayed"),
            result("c/gone", "unavailable"),
            result("d/new", "skipped"),
            result("e/fine", "replayed"),
        ];
        let failures = check_expectations(&results, &expected, &[]);
        assert_eq!(failures.len(), 2, "{failures:?}");
        assert!(
            failures
                .iter()
                .any(|f| f.starts_with("b/fixed was expected"))
        );
        assert!(
            failures
                .iter()
                .any(|f| f.starts_with("d/new is unexpectedly skipped"))
        );
        assert_eq!(
            check_expectations(&results[..1], &expected, &["a/known".into()]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn refusal_reason_prefers_the_alint_line() {
        assert_eq!(
            refusal_reason(b"warning: x\nalint: rule \"r\": bad\n\n"),
            "alint: rule \"r\": bad"
        );
        assert_eq!(
            refusal_reason(b"error: unexpected argument\n"),
            "error: unexpected argument"
        );
        assert_eq!(refusal_reason(b""), "");
    }

    #[test]
    fn path_keeps_only_absolute_entries() {
        // Absolute on every platform (a drive letter on Windows, `/` elsewhere).
        let absolute = std::env::temp_dir();
        let joined =
            std::env::join_paths([absolute.clone(), "".into(), ".".into(), "rel/bin".into()])
                .unwrap();
        let kept: Vec<PathBuf> = std::env::split_paths(&absolute_path_entries(&joined)).collect();
        assert_eq!(kept, [absolute]);
    }
}
