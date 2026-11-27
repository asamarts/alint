//! Deterministic (Callgrind) library benchmark of the rule engine.
//!
//! Unlike the wall-clock `rule_engine` criterion bench, this measures exact
//! instruction / cache / branch counts via Valgrind — load-immune, so it is the
//! reproducible REGRESSION GATE that runs in CI on every PR (no quiet box
//! needed). Design: `docs/design/deterministic-perf-gating.md`.
//!
//! Gate: `Ir` (instruction count, +2%) and `EstimatedCycles` (net work + cache +
//! branch penalties, +5%) are hard-gated vs the committed baseline. Branch
//! mispredicts (`Bcm`/`Bim`) are diagnostic-only — collected + printed, but not
//! gated (they false-positive on benign branch-pattern shifts; the +2-3% v0.12
//! drift was a benign ~+7-23% mispredict delta at <1% net cycles).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use alint_bench::fact_fixture::{absent_literal_facts, build_entries};
use alint_core::facts::{FactSpec, evaluate_facts};
use alint_core::{Context, Engine, FileEntry, FileIndex, Level, Rule, RuleEntry, Violation};
use gungraun::{
    Callgrind, EventKind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};

const CONFIG_YAML: &str = r#"
version: 1
rules:
  - id: rs-snake
    kind: filename_case
    paths: "**/*.rs"
    case: snake
    level: warning
  - id: tsx-pascal
    kind: filename_case
    paths: "**/*.tsx"
    case: pascal
    level: warning
  - id: no-bak
    kind: file_absent
    paths: "**/*.bak"
    level: error
  - id: readme
    kind: file_exists
    paths: "README.md"
    root_only: true
    level: warning
  - id: md-names
    kind: filename_regex
    paths: "**/*.md"
    pattern: "[a-zA-Z0-9_.-]+"
    level: info
  - id: no-huge
    kind: file_max_size
    paths: "**"
    max_bytes: 10485760
    level: warning
"#;

/// Keep source specs attached: W3 capability routing is metadata-driven, so
/// `Engine::new` would benchmark only its compatibility fallback.
fn build_engine() -> Engine {
    let config = alint_dsl::parse(CONFIG_YAML).expect("bench config parses");
    let registry = alint_rules::builtin_registry();
    let entries = config
        .rules
        .iter()
        .map(|spec| {
            let rule = registry.build(spec).expect("bench rule builds");
            RuleEntry::new(rule).with_spec(Arc::new(spec.clone()))
        })
        .collect();
    Engine::from_entries(entries, registry)
}

fn build_index(n: usize) -> FileIndex {
    let mut entries = Vec::with_capacity(n + 1);
    entries.push(FileEntry {
        path: PathBuf::from("README.md").into(),
        is_dir: false,
        size: 2048,
    });
    for i in 0..n {
        let (path, size) = match i % 5 {
            0 => (format!("src/mod_{}/file_{i}.rs", i % 16), 1024u64),
            1 => (format!("components/Widget{i}.tsx"), 2048),
            2 => (format!("docs/page_{i}.md"), 512),
            3 => (format!("tests/test_{i}.rs"), 800),
            _ => (format!("misc/data_{i}.yaml"), 256),
        };
        entries.push(FileEntry {
            path: PathBuf::from(path).into(),
            is_dir: false,
            size,
        });
    }
    FileIndex::from_entries(entries)
}

#[derive(Debug)]
struct AnalysisTimeRule;

impl Rule for AnalysisTimeRule {
    fn id(&self) -> &'static str {
        "analysis-time-bench"
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn wants_analysis_time(&self) -> bool {
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> alint_core::Result<Vec<Violation>> {
        std::hint::black_box(ctx.analysis_time()?);
        Ok(Vec::new())
    }
}

struct Fixture {
    engine: Engine,
    index: FileIndex,
    root: PathBuf,
}

fn fixture(n: usize) -> Fixture {
    Fixture {
        engine: build_engine(),
        index: build_index(n),
        root: PathBuf::from("/bench/root"),
    }
}

struct FactFixture {
    facts: Vec<FactSpec>,
    index: FileIndex,
}

fn fact_fixture(n: usize) -> FactFixture {
    FactFixture {
        facts: absent_literal_facts(),
        index: FileIndex::from_entries(build_entries(n)),
    }
}

#[library_benchmark(setup = fact_fixture)]
#[bench::n10k(10_000)]
fn absent_literal_fact_run(fix: FactFixture) {
    std::hint::black_box(evaluate_facts(&fix.facts, Path::new("/bench/root"), &fix.index).unwrap());
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
        .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z")
        .output()
        .expect("git is required for the deterministic analysis-time benchmark");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn analysis_time_fixture() -> Fixture {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target_dir = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        }
        _ => workspace_root.join("target"),
    };
    let root = target_dir.join("det-trees/analysis-time");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    run_git(&root, &["init", "-q", "-b", "main"]);
    run_git(&root, &["config", "user.name", "Test"]);
    run_git(&root, &["config", "user.email", "test@example.com"]);
    run_git(&root, &["config", "commit.gpgsign", "false"]);
    run_git(&root, &["commit", "--allow-empty", "-qm", "fixed"]);
    Fixture {
        engine: Engine::new(
            vec![Box::new(AnalysisTimeRule)],
            alint_rules::builtin_registry(),
        ),
        index: FileIndex::from_entries(Vec::new()),
        root,
    }
}

#[library_benchmark(setup = fixture)]
#[bench::n1k(1_000)]
#[bench::n10k(10_000)]
fn engine_run(fix: Fixture) {
    // Destructure (consuming `fix`) so the gungraun-required by-value arg isn't
    // a `needless_pass_by_value` clippy warning.
    let Fixture {
        engine,
        index,
        root,
    } = fix;
    std::hint::black_box(engine.run(&root, &index).unwrap());
}

#[library_benchmark(setup = analysis_time_fixture)]
#[bench::head_commit()]
fn analysis_time_run(fix: Fixture) {
    let Fixture {
        engine,
        index,
        root,
    } = fix;
    std::hint::black_box(engine.run(&root, &index).unwrap());
}

library_benchmark_group!(
    name = engine,
    benchmarks = [engine_run, absent_literal_fact_run, analysis_time_run]
);

main!(
    // `Engine::run` evaluates rules on rayon worker threads (`pool.install`).
    // Callgrind collects only the benchmark function's own thread by default,
    // which left `engine_run` counting ~0.06% of the engine's work (mostly the
    // fixture teardown). One worker thread keeps the count deterministic; the
    // job-execute toggles collect what that worker runs. See
    // docs/benchmarks/investigations/2026-10-v0.20-bench-increases/.
    config = LibraryBenchmarkConfig::default()
        .env("RAYON_NUM_THREADS", "1")
        .tool(
            Callgrind::default()
                .args([
                    "--cache-sim=yes",
                    "--branch-sim=yes",
                    "--toggle-collect=*StackJob*as rayon_core::job::Job*::execute*",
                    "--toggle-collect=*HeapJob*as rayon_core::job::Job*::execute*",
                ])
                // Gate on Ir (work, +2%) and EstimatedCycles (net work + cache +
                // branch penalties, +5%). Branch mispredicts (Bcm/Bim) are
                // diagnostic-only — reported always, but NOT gated: they false-positive
                // on benign branch-pattern shifts (v0.12's walker symlink-security
                // closure moved them +73..217% at <1% net cycles). See the design doc.
                .soft_limits([(EventKind::Ir, 2.0), (EventKind::EstimatedCycles, 5.0)]),
        ),
    library_benchmark_groups = engine
);
