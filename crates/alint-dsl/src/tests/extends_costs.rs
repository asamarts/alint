//! What one load's `extends:` entries cost, counted: the files the
//! environment chose, the trusted list, sources named again in other words,
//! drop-ins, and the budgets that bound them all.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::loader::{self, LoadState};

fn write(root: &Path, relative: &str, body: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

/// What `line` writes for each of `items`, in order.
fn lines<T>(
    items: impl IntoIterator<Item = T>,
    line: impl Fn(&mut String, T) -> std::fmt::Result,
) -> String {
    let mut out = String::new();
    for item in items {
        line(&mut out, item).unwrap();
    }
    out
}

/// An environment that holds `values` and nothing else.
#[derive(Debug)]
struct Values(&'static [(&'static str, &'static str)]);

impl crate::EnvironmentProvider for Values {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(held, _)| *held == name)
            .map(|(_, value)| (*value).to_string())
    }
}

fn options(values: &'static [(&'static str, &'static str)]) -> LoadOptions<'static> {
    LoadOptions {
        environment: Arc::new(Values(values)),
        ..LoadOptions::with_catalog(test_catalog())
    }
}

/// `count` entries that name `file` each in other words (`./file`,
/// `././file`, ...), short enough for a Windows path.
fn spellings(file: &str, count: usize) -> String {
    lines(1..=count, |out, times| {
        writeln!(out, "  - {}{file}", "./".repeat(times))
    })
}

/// `path` loaded as `load` loads it, before `finalize`, with the state the
/// load kept.
fn resolved(path: &Path, opts: &LoadOptions<'_>) -> (Result<RawConfig>, LoadState) {
    let mut state = LoadState::default();
    let raw = crate::resolve_top_level(path, opts, &mut state);
    (raw, state)
}

/// What `run` counted on `counter`, and its result.
fn counted<R>(
    counter: &'static std::thread::LocalKey<std::cell::Cell<usize>>,
    run: impl FnOnce() -> R,
) -> (R, usize) {
    let before = counter.with(std::cell::Cell::get);
    let result = run();
    (result, counter.with(std::cell::Cell::get) - before)
}

/// Each entry of a file the environment chose recorded the target's path as
/// joined, and named its source in a warning scrubbed of every value recorded
/// so far, so one entry read from the environment made the file it chose
/// load in time quadratic in its entries (1,000 entries naming one empty file
/// took 68 seconds). A file below such an entry is named `<redacted>` with no
/// scrub, each file is recorded once however it is spelled, and a warning
/// about a file's top-level settings is given once.
#[test]
fn a_file_the_environment_chose_costs_each_entry_once() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = 40;
    write(
        tmp.path(),
        "e.yml",
        "version: 1\nrespect_gitignore: false\n",
    );
    write(tmp.path(), "f.yml", "version: 1\nnested_configs: false\n");
    write(
        tmp.path(),
        "p.yml",
        &format!("version: 1\nextends:\n{}", spellings("e.yml", entries)),
    );
    let root = write(
        tmp.path(),
        ".alint.yml",
        &format!(
            "version: 1\nextends:\n  - '{{{{env.POLICY}}}}'\n{}rules: []\n",
            spellings("f.yml", entries)
        ),
    );
    let (loaded, redactions) = counted(&loader::REDACTIONS, || {
        load_with(&root, &options(&[("POLICY", "./p.yml")]))
    });
    let config = loaded.unwrap();
    // `f.yml`, which the environment did not choose, is named once, scrubbed.
    assert_eq!(redactions, 1);
    // The entry as written, then `p.yml` and `e.yml`, each relative to the
    // root and resolved.
    assert_eq!(
        config.environment_values.len(),
        5,
        "{:?}",
        config.environment_values
    );
}

/// A local load below the top-level config is memoized by the
/// `trusted_extends:` list it is checked against, which the memo copied and
/// hashed in full for each load: 2,000 entries with 20,000 trusted URLs took
/// 23 seconds. It is keyed by which list it is, and an entry that loads an
/// empty file costs at least one entry of the budget.
#[test]
fn a_memoized_load_does_not_hash_the_trusted_list() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = 40;
    write(tmp.path(), "e.yml", "version: 1\n");
    let trusted: String = lines(0..2_000, |out, index| {
        writeln!(out, "  - https://example.invalid/r{index}.yml")
    });
    let root = write(
        tmp.path(),
        ".alint.yml",
        &format!(
            "version: 1\ntrusted_extends:\n{trusted}extends:\n{}",
            spellings("e.yml", entries)
        ),
    );
    let ((raw, state), hashed) = counted(&loader::MEMO_HASHED, || resolved(&root, &options(&[])));
    raw.unwrap();
    // A key hashes two paths and a few small fields; the list would add
    // 2,000 URLs to each.
    assert!(hashed < entries * 2_000, "{hashed} bytes hashed");
    assert_eq!(state.extends_entries, entries);
}

/// An entry that names a remote body by another URL (another query) read,
/// hashed and parsed it again: 40 entries of a 15 MiB body took 157 seconds.
/// The body is parsed once, and each entry charged its size.
#[test]
fn a_remote_body_named_by_other_urls_parses_once() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let body = "version: 1\nrules:\n  - {id: r, kind: file_exists, paths: x, level: error}\n";
    let url = seed_remote(&cache, body);
    let (base, sri) = url.split_once('#').unwrap();
    let entries: String = lines(0..5, |out, query| {
        writeln!(out, "  - '{base}?v={query}#{sri}'")
    });
    let root = write(
        tmp.path(),
        ".alint.yml",
        &format!("version: 1\nextends:\n{entries}rules: []\n"),
    );
    let opts = LoadOptions {
        cache: Some(cache),
        ..options(&[])
    };
    let ((raw, state), parses) = loader::parses_during(|| resolved(&root, &opts));
    assert_eq!(raw.unwrap().rules.len(), 1);
    assert_eq!(parses, 2, "the root and the body");
    assert_eq!(state.extends_bytes, 5 * body.len());
}

/// An entry that names a bundled ruleset with another `only:` decoded it
/// again (7 ms), and was charged a few entries: the budget admitted 230,000
/// such entries, 27 minutes. It is decoded once, and each entry charged its
/// size.
#[test]
fn a_bundled_ruleset_named_with_other_filters_decodes_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        "agent-no-console-log",
        "agent-no-debugger-statements",
        "agent-no-model-todos",
    );
    let entries: String = [
        a.to_owned(),
        b.to_owned(),
        c.to_owned(),
        format!("{a}, {b}"),
        format!("{b}, {c}"),
        format!("{a}, {c}"),
    ]
    .iter()
    .fold(String::new(), |mut out, only| {
        writeln!(
            out,
            "  - {{url: alint://bundled/agent-hygiene@v1, only: [{only}]}}"
        )
        .unwrap();
        out
    });
    let root = write(
        tmp.path(),
        ".alint.yml",
        &format!("version: 1\nextends:\n{entries}rules: []\n"),
    );
    let ((raw, state), parses) = loader::parses_during(|| resolved(&root, &options(&[])));
    raw.unwrap();
    assert_eq!(parses, 2, "the root and the ruleset");
    let size = crate::bundled::resolve("agent-hygiene@v1").unwrap().len();
    assert_eq!(state.extends_bytes, 6 * size);
}

/// A source of few, large rules merged in entries cost almost nothing: 60
/// entries naming one file whose one rule has 50,000 keys took 41 seconds,
/// charged 180 entries. Each entry is charged the source's size in bytes, the
/// sources a file merged with it, and a source whose aliases expand past its
/// text what they expand to.
#[test]
fn an_entry_is_charged_the_size_of_its_source() {
    let tmp = tempfile::tempdir().unwrap();
    let big = format!(
        "version: 1\nrules:\n  - id: r\n    kind: file_exists\n    paths: x\n    level: error\n    \
         message: '{}'\n",
        "x".repeat(4_096)
    );
    write(tmp.path(), "big.yml", &big);
    let middle = "version: 1\nextends: [./big.yml]\n";
    write(tmp.path(), "middle.yml", middle);
    let root = write(
        tmp.path(),
        ".alint.yml",
        &format!(
            "version: 1\nextends:\n{}  - ./middle.yml\nrules: []\n",
            spellings("big.yml", 3)
        ),
    );
    let (raw, state) = resolved(&root, &options(&[]));
    raw.unwrap();
    // Three entries of `big.yml`, `middle.yml`'s own, and `middle.yml` with
    // what it merged.
    assert_eq!(
        state.extends_bytes,
        3 * big.len() + big.len() + (big.len() + middle.len())
    );

    // A thousand aliases of one 1 KB string, in a text a tenth that size.
    let aliases: String = lines(0..1_000, |out, index| writeln!(out, "  a{index}: *v"));
    let text = format!(
        "version: 1\nvars:\n  v: &v '{}'\n{aliases}",
        "y".repeat(1_024)
    );
    let parsed = crate::loader::parse_config_interpolated_with_environment(
        &text,
        Path::new("aliased.yml"),
        &crate::HostEnvironment,
        &crate::loader::TEST_ROOT,
    )
    .unwrap();
    assert!(
        parsed.source_size > 1_000 * 1_024 / 16,
        "{} bytes, for a {}-byte text",
        parsed.source_size,
        text.len()
    );
}

/// The bytes the entries of one load merge are held to a budget: a source
/// is refused once what the entries before it merged, and it, come to more.
#[test]
fn the_extends_source_budget_bounds_a_load() {
    let source = |size: usize| RawConfig {
        source_size: size,
        ..RawConfig::default()
    };
    let mut state = LoadState::default();
    let empty = RawConfig::default();
    state
        .charge_extends(&empty, &source(loader::MAX_EXTENDS_SOURCE_BYTES / 2))
        .expect("half the budget");
    state
        .charge_extends(&empty, &source(loader::MAX_EXTENDS_SOURCE_BYTES / 2))
        .expect("all of it");
    assert_eq!(
        state
            .charge_extends(&empty, &source(1))
            .unwrap_err()
            .to_string(),
        format!(
            "the `extends:` entries of this config merge more than {} bytes of sources in all \
             (each source counted for every entry that merges it, as its text, or as its YAML \
             aliases expand if that is larger, and a source that extends others with what it \
             merged); list each source once, or extend fewer and smaller sources",
            loader::MAX_EXTENDS_SOURCE_BYTES
        )
    );
}

/// Each `.alint.d/` drop-in merges onto the whole config, which a merge
/// rebuilds, and was never charged: 200 empty drop-ins on a config of 50,000
/// `ignore:` patterns took ten seconds. Each drop-in's merge is charged as an
/// `extends:` merge is.
#[test]
fn each_drop_in_merge_is_charged() {
    let tmp = tempfile::tempdir().unwrap();
    let patterns: String = lines(0..1_000, |out, index| writeln!(out, "  - 'p{index}/**'"));
    let root = write(
        tmp.path(),
        ".alint.yml",
        &format!("version: 1\nignore:\n{patterns}rules: []\n"),
    );
    let drop_in = "version: 1\n";
    for name in ["10.yml", "20.yml", "30.yml"] {
        write(tmp.path(), &format!(".alint.d/{name}"), drop_in);
    }
    let (raw, state) = resolved(&root, &options(&[]));
    raw.unwrap();
    // Each merge rebuilds the 1,000 patterns and brings in nothing.
    assert_eq!(state.extends_entries, 3 * 1_000);
    assert_eq!(state.extends_bytes, 3 * drop_in.len());
}

/// The budgets' sizes, and where a flat list of rulesets meets the entry
/// budget, as the configuration docs' Limits section and the CHANGELOG give
/// them: about 90 rulesets of 1,000 rules each, or about 2,900 one-rule
/// files. Each entry of such a list is charged the ruleset it loads, and its
/// merge what was merged so far and the ruleset.
#[test]
fn the_extends_budgets_have_their_documented_sizes() {
    assert_eq!(loader::MAX_EXTENDS_MERGE_WORK, 4_194_304);
    assert_eq!(loader::MAX_EXTENDS_SOURCE_BYTES, 64 * 1024 * 1024);
    let entries = |count: usize| RawConfig {
        ignore: vec![String::new(); count],
        ..RawConfig::default()
    };
    // The first entry of a flat list of `rules`-rule rulesets the budget
    // refuses.
    let refused_at = |rules: usize| {
        let mut state = LoadState::default();
        let source = entries(rules);
        (1..=10_000)
            .find(|&entry: &usize| {
                let merged = entries((entry - 1) * rules);
                state
                    .charge_extends_load(&source)
                    .and_then(|()| state.charge_extends(&merged, &source))
                    .is_err()
            })
            .unwrap()
    };
    assert_eq!(refused_at(1_000), 91);
    assert_eq!(refused_at(1), 2_895);
    // Sources of 1 MiB: the 65th passes 64 MiB.
    let mut state = LoadState::default();
    let source = RawConfig {
        source_size: 1 << 20,
        ..RawConfig::default()
    };
    let refused = (1..=65)
        .find(|_| {
            state
                .charge_extends(&RawConfig::default(), &source)
                .is_err()
        })
        .unwrap();
    assert_eq!(refused, 65);
}

/// A file the environment chose, and each it extends, is named `<redacted>`
/// in the plan, and so is the URL of an entry that leads to one, which an
/// error quotes: the text scrub skips a name shorter than four characters
/// (`q`, `../v`).
#[test]
fn a_file_below_an_entry_read_from_the_environment_is_named_redacted() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "sd/p.yml",
        "version: 2\nextends:\n  - url: ../q\n    as: q\nselectors:\n  own: { paths: x }\n",
    );
    write(
        tmp.path(),
        "q",
        "version: 2\nselectors:\n  docs: { paths: README.md }\n",
    );
    let root = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: '{{env.POLICY}}'\n    as: p\nrules: []\n",
    );
    let opts = options(&[("POLICY", "./sd/p.yml")]);
    let plan = load_with(&root, &opts)
        .unwrap()
        .relation_plan
        .expect("a version 2 config has a plan");
    let displays: Vec<&str> = plan
        .sources
        .iter()
        .map(|source| source.display.as_str())
        .collect();
    assert_eq!(displays, ["<redacted>", "<redacted>", ".alint.yml"]);

    // An alias of a version 1 file the environment chose.
    write(tmp.path(), "v", "version: 1\nrules: []\n");
    write(
        tmp.path(),
        "sd/p.yml",
        "version: 2\nextends:\n  - url: ../v\n    as: q\nrelations:\n  r:\n    from: q.docs\n    \
         read: { json: $.a }\n    fields: { a: value }\n",
    );
    let error = load_with(&root, &opts).unwrap_err().to_string();
    assert!(
        error.contains("`q` names <redacted>, a version 1 config") && !error.contains("../v"),
        "{error}"
    );
}
