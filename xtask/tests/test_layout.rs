//! Guards the one-test-binary-per-crate layout
//! (`docs/design/build-dev-loop.md`, section 2).
//!
//! Every crate with integration tests sets `autotests = false` and builds them
//! as one `it` target rooted at `tests/main.rs`, which declares each test file
//! as a module. Without Cargo's discovery a new `tests/*.rs` file is silently
//! never compiled, and a crate that drops `autotests = false` goes back to one
//! binary per file; this test catches both through `cargo metadata`, and
//! checks every module directory's `mod.rs` the same way.
//!
//! Runs in plain `cargo test --workspace`, with no network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde_json::Value;

/// Test targets that must be binaries of their own, as `(package, target,
/// why)`. Each is declared with [[test]] and lives in a directory of `tests/`
/// without a `mod.rs`, which the module scan skips.
const OWN_BINARIES: &[(&str, &str, &str)] = &[
    (
        "alint-core",
        "yaml_memory",
        "it installs a counting global allocator, which in `it` would count every other test's \
         allocations (ADR-0042)",
    ),
    (
        "alint-core",
        "relation_memory",
        "it installs a counting global allocator, which in `it` would count every other test's \
         allocations",
    ),
    (
        "alint-rules",
        "key_parity_memory",
        "it installs a counting global allocator, which in `it` would count every other test's \
         allocations",
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a workspace parent")
        .to_path_buf()
}

fn workspace_packages(root: &Path) -> Vec<Value> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("metadata is JSON");
    metadata["packages"]
        .as_array()
        .expect("metadata lists packages")
        .clone()
}

/// The modules a module directory must declare, and files that cannot be one.
#[derive(Default)]
struct ModuleDir {
    /// Module name to its file, for every `*.rs` file other than the root
    /// (`main.rs` or `mod.rs`) and every subdirectory with a `mod.rs`.
    modules: BTreeMap<String, PathBuf>,
    /// Files that can never be a module of the root as they stand.
    problems: Vec<String>,
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Scan `dir`, whose root file is `root` (`main.rs` or `mod.rs`). `shown` is
/// how the directory is named in messages.
fn scan_module_dir(dir: &Path, root: &str, shown: &str) -> ModuleDir {
    let mut found = ModuleDir::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return found,
        Err(error) => {
            found
                .problems
                .push(format!("cannot read {}: {error}", dir.display()));
            return found;
        }
    };
    for entry in entries {
        let path = entry.expect("read tests dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let module = if path.is_dir() {
            if path.join("main.rs").is_file() {
                found.problems.push(format!(
                    "{shown}/{name}/main.rs is a separate test binary, which `autotests = false` \
                     never builds; rename it to {shown}/{name}/mod.rs and declare `mod {name};`"
                ));
            }
            if !path.join("mod.rs").is_file() {
                continue;
            }
            (name.clone(), path.join("mod.rs"))
        } else if let Some(stem) = name.strip_suffix(".rs")
            && name != root
        {
            (stem.to_string(), path.clone())
        } else {
            continue;
        };
        if is_identifier(&module.0) {
            found.modules.insert(module.0, module.1);
        } else {
            found.problems.push(format!(
                "{shown}/{} cannot be a module; rename it to a valid Rust identifier",
                path.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    found
}

/// `src` with comments removed (block comments nest), string literals reduced
/// to their alphanumeric and simple punctuation characters (so `"linux"` stays
/// readable but a quoted `mod x;`, `*/` or `]` cannot be mistaken for code),
/// and everything inside braces blanked, leaving only top-level items.
fn top_level_code(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut depth = 0usize;
    let mut i = 0;
    let push = |out: &mut String, depth: usize, c: char| {
        out.push(if depth == 0 || c == '\n' { c } else { ' ' });
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            let mut nesting = 0usize;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    nesting += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    nesting -= 1;
                    i += 2;
                    if nesting == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        // Raw strings: r"..", r#".."#, br#".."#.
        let raw_start = if c == 'r' {
            Some(i + 1)
        } else if c == 'b' && next == Some('r') {
            Some(i + 2)
        } else {
            None
        };
        if let Some(mut j) = raw_start {
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            let ident_before = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
            if chars.get(j) == Some(&'"') && !ident_before {
                let mut k = j + 1;
                let body_start = k;
                while k < chars.len() {
                    if chars[k] == '"' && (0..hashes).all(|h| chars.get(k + 1 + h) == Some(&'#')) {
                        break;
                    }
                    k += 1;
                }
                push_string(&mut out, depth, &chars[body_start..k.min(chars.len())]);
                i = k + 1 + hashes;
                continue;
            }
        }
        if c == '"' {
            let mut k = i + 1;
            let body_start = k;
            while k < chars.len() && chars[k] != '"' {
                k += if chars[k] == '\\' { 2 } else { 1 };
            }
            push_string(&mut out, depth, &chars[body_start..k.min(chars.len())]);
            i = k + 1;
            continue;
        }
        // A character literal such as '"' or '{' (a lifetime has no closing quote).
        if c == '\'' {
            let len = if next == Some('\\') { 4 } else { 3 };
            if chars.get(i + len - 1) == Some(&'\'') {
                i += len;
                continue;
            }
        }
        match c {
            '{' => {
                push(&mut out, depth, ' ');
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                push(&mut out, depth, ' ');
            }
            _ => push(&mut out, depth, c),
        }
        i += 1;
    }
    out
}

fn push_string(out: &mut String, depth: usize, body: &[char]) {
    if depth > 0 {
        out.push_str("\"\"");
        return;
    }
    out.push('"');
    out.extend(
        body.iter()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
    );
    out.push('"');
}

/// How one `mod name;` declaration is written.
#[derive(Debug, PartialEq, Eq)]
struct Declaration {
    name: String,
    /// Why the declaration does not reliably compile the file named after it.
    problem: Option<String>,
}

/// The `cfg` predicates a test module may sit behind: platforms CI runs.
const ALLOWED_CFGS: &[&str] = &[
    "unix",
    "windows",
    "not(unix)",
    "not(windows)",
    "target_os=\"linux\"",
    "target_os=\"macos\"",
    "target_os=\"windows\"",
];

/// The top-level `mod name;` declarations in a module root, any visibility,
/// with attributes on the same or earlier lines.
fn declarations(src: &str) -> Vec<Declaration> {
    let declaration = Regex::new(
        r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\s*\([^)]*\))?\s+)?\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*;",
    )
    .unwrap();
    let attribute = Regex::new(r"#\[\s*([A-Za-z_]+)\s*(.*?)\]\s*$").unwrap();
    let path_argument = Regex::new(r"\bpath\s*=").unwrap();
    let mut found = Vec::new();
    for capture in declaration.captures_iter(&top_level_code(src)) {
        let name = capture[2].to_string();
        let mut problem = None;
        for attr in capture[1].split("#[").filter(|a| !a.trim().is_empty()) {
            let attr = format!("#[{attr}");
            let Some(parts) = attribute.captures(attr.trim()) else {
                continue;
            };
            let args: String = parts[2].chars().filter(|c| !c.is_whitespace()).collect();
            match &parts[1] {
                "cfg" => {
                    let predicate = args.trim_start_matches('(').trim_end_matches(')');
                    if !ALLOWED_CFGS.contains(&predicate) {
                        problem = Some(format!(
                            "is behind #[cfg({predicate})], which CI may never enable; gate the \
                             file's contents with an inner #![cfg] on a platform instead"
                        ));
                    }
                }
                "cfg_attr" | "path" => {
                    problem = Some(format!(
                        "uses #[{}], which can compile a file other than the one the module is \
                         named after",
                        &parts[1]
                    ));
                }
                _ if path_argument.is_match(&parts[2]) => {
                    problem = Some("sets a module path".to_string());
                }
                _ => {}
            }
        }
        found.push(Declaration { name, problem });
    }
    found
}

/// Check one module root (`main.rs` or `mod.rs`) and, recursively, every
/// directory module it declares.
fn check_module_root(
    package: &str,
    root_file: &Path,
    dir: &Path,
    shown: &str,
    problems: &mut Vec<String>,
) {
    let root_name = root_file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let found = scan_module_dir(dir, &root_name, shown);
    problems.extend(found.problems.iter().map(|p| format!("{package}: {p}")));
    let source = match std::fs::read_to_string(root_file) {
        Ok(source) => source,
        Err(error) => {
            problems.push(format!(
                "{package}: cannot read {}: {error}",
                root_file.display()
            ));
            return;
        }
    };
    let declared = declarations(&source);
    let mut names = BTreeSet::new();
    for declaration in &declared {
        if !names.insert(declaration.name.as_str()) {
            problems.push(format!(
                "{package}: {shown}/{root_name} declares `mod {};` twice",
                declaration.name
            ));
        }
        if let Some(problem) = &declaration.problem {
            problems.push(format!(
                "{package}: `mod {};` in {shown}/{root_name} {problem}",
                declaration.name
            ));
        }
    }
    for (module, file) in &found.modules {
        if !names.contains(module.as_str()) {
            problems.push(format!(
                "{package}: {} is never compiled; add `mod {module};` to {shown}/{root_name}",
                file.strip_prefix(dir.parent().unwrap_or(dir))
                    .unwrap_or(file)
                    .display()
            ));
        } else if file.file_name().is_some_and(|f| f == "mod.rs") {
            let child = file.parent().unwrap();
            check_module_root(package, file, child, &format!("{shown}/{module}"), problems);
        }
    }
}

#[test]
fn every_crate_builds_its_integration_tests_as_one_binary() {
    let root = workspace_root();
    let mut problems = Vec::new();
    let mut checked = 0;
    let mut own_binaries = BTreeSet::new();
    for package in workspace_packages(&root) {
        let name = package["name"].as_str().unwrap();
        let manifest = PathBuf::from(package["manifest_path"].as_str().unwrap());
        let crate_dir = manifest.parent().unwrap();
        let tests_dir = crate_dir.join("tests");
        let top = scan_module_dir(&tests_dir, "main.rs", "tests");
        let test_targets: Vec<&Value> = package["targets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|target| {
                target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
            })
            .filter(|target| {
                let target_name = target["name"].as_str().unwrap();
                let own = OWN_BINARIES.iter().any(|&(own_package, own_target, _)| {
                    own_package == name && own_target == target_name
                });
                if own {
                    own_binaries.insert((name.to_string(), target_name.to_string()));
                }
                !own
            })
            .collect();
        if top.modules.is_empty() && top.problems.is_empty() && test_targets.is_empty() {
            continue;
        }
        checked += 1;

        let main_rs = tests_dir.join("main.rs");
        let shape_ok = matches!(
            test_targets.as_slice(),
            [target] if target["name"] == "it"
                && Path::new(target["src_path"].as_str().unwrap()) == main_rs
        );
        if !shape_ok {
            let found: Vec<_> = test_targets
                .iter()
                .map(|target| target["name"].as_str().unwrap())
                .collect();
            problems.push(format!(
                "{name}: expected one test target `it` at tests/main.rs, found {found:?}; \
                 set `autotests = false` and declare it with [[test]] in {}",
                manifest.display()
            ));
            continue;
        }
        let target = test_targets[0];
        if target["required-features"]
            .as_array()
            .is_some_and(|features| !features.is_empty())
        {
            problems.push(format!(
                "{name}: the `it` target has required-features, so a plain test run skips it"
            ));
        }
        if target["test"] == Value::Bool(false) {
            problems.push(format!("{name}: the `it` target sets `test = false`"));
        }
        check_module_root(name, &main_rs, &tests_dir, "tests", &mut problems);
    }
    for &(package, target, _) in OWN_BINARIES {
        if !own_binaries.contains(&(package.to_string(), target.to_string())) {
            problems.push(format!(
                "OWN_BINARIES names {package}'s test target `{target}`, which does not exist"
            ));
        }
    }
    assert!(
        checked >= 8,
        "only {checked} crates with integration tests were found"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn declarations_read_every_form_and_flag_unreliable_ones() {
    let src = r##"//! doc mentioning mod fake;
#[cfg(unix)]
mod lsp_common;
pub mod cli;
mod a; // note
pub(crate) mod b;
#[cfg(unix)] mod c;
mod	d ;
mod r#e;
/* mod hidden; /* nested */ mod also_hidden; */
#[path = "other.rs"]
mod f;
#[cfg(any())]
mod g;
#[cfg_attr(unix, path = "o.rs")]
mod h;
#[cfg(target_os = "linux")]
mod i;
const S: &str = "mod ghost; */ #[";
const R: &str = r#"mod raw_ghost;"#;
mod outer { mod inner; }
#[doc = "see https://example.com/x]"]
mod j;
"##;
    let found = declarations(src);
    let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "lsp_common",
            "cli",
            "a",
            "b",
            "c",
            "d",
            "e",
            "f",
            "g",
            "h",
            "i",
            "j"
        ]
    );
    let flagged: Vec<&str> = found
        .iter()
        .filter(|d| d.problem.is_some())
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(flagged, ["f", "g", "h"]);
}

#[test]
fn scan_flags_files_that_cannot_be_modules() {
    let dir = tempfile::tempdir().unwrap();
    let tests = dir.path();
    std::fs::write(tests.join("main.rs"), "").unwrap();
    std::fs::write(tests.join("ok_file.rs"), "").unwrap();
    std::fs::write(tests.join("bad-name.rs"), "").unwrap();
    std::fs::create_dir_all(tests.join("helper")).unwrap();
    std::fs::write(tests.join("helper/mod.rs"), "").unwrap();
    std::fs::create_dir_all(tests.join("multi")).unwrap();
    std::fs::write(tests.join("multi/main.rs"), "").unwrap();
    std::fs::create_dir_all(tests.join("fixtures/src")).unwrap();
    std::fs::write(tests.join("fixtures/src/lib.rs"), "").unwrap();
    let found = scan_module_dir(tests, "main.rs", "tests");
    assert_eq!(
        found.modules.keys().map(String::as_str).collect::<Vec<_>>(),
        ["helper", "ok_file"]
    );
    assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
    assert!(found.problems.iter().any(|p| p.contains("bad-name.rs")));
    assert!(found.problems.iter().any(|p| p.contains("multi/main.rs")));
    assert_eq!(
        scan_module_dir(&tests.join("absent"), "main.rs", "tests").problems,
        Vec::<String>::new()
    );
}

#[test]
fn nested_module_directories_are_checked_against_their_mod_rs() {
    let dir = tempfile::tempdir().unwrap();
    let tests = dir.path();
    std::fs::write(tests.join("main.rs"), "mod helper;\n").unwrap();
    std::fs::create_dir_all(tests.join("helper")).unwrap();
    std::fs::write(tests.join("helper/mod.rs"), "mod declared;\n").unwrap();
    std::fs::write(tests.join("helper/declared.rs"), "").unwrap();
    std::fs::write(tests.join("helper/forgotten.rs"), "").unwrap();
    let mut problems = Vec::new();
    check_module_root("pkg", &tests.join("main.rs"), tests, "tests", &mut problems);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("forgotten.rs is never compiled"));
    assert!(problems[0].contains("tests/helper/mod.rs"));
}
