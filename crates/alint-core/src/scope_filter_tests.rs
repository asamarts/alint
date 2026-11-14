use super::*;
use crate::walker::{FileEntry, FileIndex};
use std::path::Path;
use std::sync::Arc;

fn idx(paths: &[&str]) -> FileIndex {
    FileIndex::from_entries(
        paths
            .iter()
            .map(|p| FileEntry {
                path: Arc::<Path>::from(Path::new(p)),
                is_dir: false,
                size: 0,
            })
            .collect(),
    )
}

fn filter(names: Vec<&str>) -> ScopeFilter {
    ScopeFilter::has_ancestor_unchecked(names)
}

#[test]
fn root_manifest_matches_root_file() {
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&["Cargo.toml", "lib.rs"]);
    assert!(f.matches(Path::new("lib.rs"), &i));
}

#[test]
fn root_manifest_matches_nested_file() {
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&["Cargo.toml", "src/lib.rs"]);
    assert!(f.matches(Path::new("src/lib.rs"), &i));
}

#[test]
fn nested_manifest_matches_own_dir() {
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&["crates/api/Cargo.toml", "crates/api/src/main.rs"]);
    // Manifest at crates/api/ — main.rs's ancestor.
    assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
}

#[test]
fn manifest_at_files_own_dir_matches_the_manifest_itself() {
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&["crates/api/Cargo.toml"]);
    // `Cargo.toml` is in the file's own dir → match.
    assert!(f.matches(Path::new("crates/api/Cargo.toml"), &i));
}

#[test]
fn root_cargo_toml_matches_itself() {
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&["Cargo.toml"]);
    assert!(f.matches(Path::new("Cargo.toml"), &i));
}

#[test]
fn no_manifest_in_any_ancestor_returns_false() {
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&["src/lib.rs"]);
    assert!(!f.matches(Path::new("src/lib.rs"), &i));
}

#[test]
fn sibling_manifest_does_not_match() {
    let f = filter(vec!["Cargo.toml"]);
    // Sibling has Cargo.toml, but our file is in a different subtree.
    let i = idx(&["crates/api/Cargo.toml", "services/web/src/index.ts"]);
    assert!(!f.matches(Path::new("services/web/src/index.ts"), &i));
}

#[test]
fn two_name_list_matches_if_either_found() {
    let f = filter(vec!["pyproject.toml", "setup.py"]);
    let i = idx(&["app/setup.py", "app/main.py"]);
    assert!(f.matches(Path::new("app/main.py"), &i));
}

#[test]
fn closest_ancestor_among_multiple() {
    // Both root and crates/api have Cargo.toml. Either match.
    let f = filter(vec!["Cargo.toml"]);
    let i = idx(&[
        "Cargo.toml",
        "crates/api/Cargo.toml",
        "crates/api/src/main.rs",
    ]);
    assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
}

// ── from_spec validation ──────────────────────────────────

#[test]
fn from_spec_rejects_empty_list() {
    let err = ScopeFilter::from_spec(
        "r",
        ScopeFilterSpec {
            has_ancestor: Some(vec![]),
            changed_since: None,
            include_manifest_paths: None,
            exclude_manifest_paths: None,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("non-empty"), "msg: {err}");
}

#[test]
fn from_spec_rejects_empty_string() {
    let err = ScopeFilter::from_spec(
        "r",
        ScopeFilterSpec {
            has_ancestor: Some(vec![String::new()]),
            changed_since: None,
            include_manifest_paths: None,
            exclude_manifest_paths: None,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not be empty"), "msg: {err}");
}

#[test]
fn from_spec_rejects_path_separator() {
    let err = ScopeFilter::from_spec(
        "r",
        ScopeFilterSpec {
            has_ancestor: Some(vec!["foo/bar".into()]),
            changed_since: None,
            include_manifest_paths: None,
            exclude_manifest_paths: None,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("path separators"), "msg: {err}");
}

#[test]
fn from_spec_rejects_glob_metacharacters() {
    for bad in &["*.toml", "Cargo?", "[abc]", "{a,b}", "!Cargo"] {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec![(*bad).into()]),
                changed_since: None,
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("glob"), "msg for {bad:?}: {err}");
    }
}

#[test]
fn from_spec_accepts_canonical_manifests() {
    for good in &[
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "setup.py",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
    ] {
        ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec![(*good).into()]),
                changed_since: None,
                include_manifest_paths: None,
                exclude_manifest_paths: None,
            },
        )
        .unwrap_or_else(|e| panic!("{good:?} should be valid; got {e}"));
    }
}

// ── deserialise OneOrMany ─────────────────────────────────

#[test]
fn deserialize_single_string_form() {
    let yaml = "has_ancestor: Cargo.toml\n";
    let spec: ScopeFilterSpec = ayaml::compat::from_str(yaml).unwrap();
    assert_eq!(spec.has_ancestor, Some(vec!["Cargo.toml".to_string()]));
    assert_eq!(spec.changed_since, None);
}

#[test]
fn deserialize_changed_since_only_form() {
    let yaml = "changed_since: origin/main\n";
    let spec: ScopeFilterSpec = ayaml::compat::from_str(yaml).unwrap();
    assert_eq!(spec.has_ancestor, None);
    assert_eq!(spec.changed_since.as_deref(), Some("origin/main"));
}

#[test]
fn deserialize_list_form() {
    let yaml = "has_ancestor:\n  - pom.xml\n  - build.gradle\n";
    let spec: ScopeFilterSpec = ayaml::compat::from_str(yaml).unwrap();
    assert_eq!(
        spec.has_ancestor,
        Some(vec!["pom.xml".to_string(), "build.gradle".to_string()]),
    );
}

#[test]
fn deserialize_rejects_unknown_field() {
    let yaml = "has_ancestor: Cargo.toml\nunknown: x\n";
    assert!(ayaml::compat::from_str::<ScopeFilterSpec>(yaml).is_err());
}

// ── changed_since ─────────────────────────────────────────

fn idx_with_diff(paths: &[&str], since: &str, diff: &[&str]) -> FileIndex {
    let i = idx(paths);
    let mut map = std::collections::HashMap::new();
    map.insert(since.to_string(), diff.iter().map(PathBuf::from).collect());
    i.set_changed_paths(map);
    i
}

#[test]
fn changed_since_matches_only_files_in_diff() {
    let f = ScopeFilter::changed_since_unchecked("origin/main");
    let i = idx_with_diff(&["src/a.rs", "src/b.rs"], "origin/main", &["src/a.rs"]);
    assert!(f.matches(Path::new("src/a.rs"), &i), "in-diff file matches");
    assert!(
        !f.matches(Path::new("src/b.rs"), &i),
        "out-of-diff file skipped"
    );
}

#[test]
fn changed_since_with_unpopulated_cache_matches_nothing() {
    // A missing cache defensively matches nothing. The engine resolves it
    // before dispatch and separately gates repository unavailability.
    let f = ScopeFilter::changed_since_unchecked("origin/main");
    let i = idx(&["src/a.rs"]);
    assert!(!f.matches(Path::new("src/a.rs"), &i));
}

#[test]
fn changed_since_and_composes_with_has_ancestor() {
    // Both gates must hold. a.rs is in the diff AND under a
    // Cargo.toml; b.rs is in the diff but has no ancestor manifest.
    let f = ScopeFilter {
        has_ancestor: vec![PathBuf::from("Cargo.toml")],
        changed_since: Some("origin/main".to_string()),
        manifest_predicates: Vec::new(),
    };
    let i = idx_with_diff(
        &["crates/x/Cargo.toml", "crates/x/a.rs", "loose/b.rs"],
        "origin/main",
        &["crates/x/a.rs", "loose/b.rs"],
    );
    assert!(f.matches(Path::new("crates/x/a.rs"), &i));
    assert!(
        !f.matches(Path::new("loose/b.rs"), &i),
        "no ancestor manifest"
    );
}

#[test]
fn from_spec_rejects_neither_field() {
    let err = ScopeFilter::from_spec(
        "r",
        ScopeFilterSpec {
            has_ancestor: None,
            changed_since: None,
            include_manifest_paths: None,
            exclude_manifest_paths: None,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("at least one"), "msg: {err}");
}

#[test]
fn from_spec_accepts_changed_since_only() {
    let f = ScopeFilter::from_spec(
        "r",
        ScopeFilterSpec {
            has_ancestor: None,
            changed_since: Some("origin/main".into()),
            include_manifest_paths: None,
            exclude_manifest_paths: None,
        },
    )
    .unwrap();
    assert_eq!(f.changed_since(), Some("origin/main"));
    assert_eq!(f.has_ancestor_names(), &[] as &[PathBuf]);
}

// ── manifest-derived path scope (ADR-0010) ────────────────

/// Build a filter from `scope_filter:` YAML (the real deserialise + validate
/// path), so the manifest predicate is constructed exactly as production.
fn manifest_filter(yaml: &str) -> ScopeFilter {
    let spec: ScopeFilterSpec = ayaml::compat::from_str(yaml).expect("scope_filter parses");
    ScopeFilter::from_spec("r", spec).expect("from_spec ok")
}

fn idx_with_manifest(paths: &[&str], key: &str, set: &[&str]) -> FileIndex {
    let i = idx(paths);
    let mut map = std::collections::HashMap::new();
    map.insert(
        key.to_string(),
        ManifestSet::from_paths(set.iter().map(PathBuf::from).collect::<Vec<_>>()),
    );
    i.set_manifest_paths(map, HashMap::new());
    i
}

#[test]
fn exclude_manifest_paths_drops_files_in_set() {
    let f = manifest_filter(
        "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
    );
    let key = f.manifest_predicates()[0].cache_key();
    let i = idx_with_manifest(&["src/cli.ts", "src/lib.ts"], key, &["src/cli.ts"]);
    assert!(
        !f.matches(Path::new("src/cli.ts"), &i),
        "in-set file dropped"
    );
    assert!(
        f.matches(Path::new("src/lib.ts"), &i),
        "out-of-set file kept"
    );
}

#[test]
fn include_manifest_paths_keeps_only_files_in_set() {
    let f = manifest_filter(
        "include_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
    );
    let key = f.manifest_predicates()[0].cache_key();
    let i = idx_with_manifest(
        &["crates/a/lib.rs", "vendor/x.rs"],
        key,
        &["crates/a/lib.rs"],
    );
    assert!(
        f.matches(Path::new("crates/a/lib.rs"), &i),
        "in-set file kept"
    );
    assert!(
        !f.matches(Path::new("vendor/x.rs"), &i),
        "out-of-set file dropped"
    );
}

#[test]
fn include_manifest_paths_expands_glob_members() {
    // `members = ["crates/*"]` (the common Cargo/npm workspace form) is a
    // GLOB: a file under any directory matching `crates/*` is in scope; a
    // sibling outside is not. Regression guard for the silent-no-op footgun
    // where a glob member was matched literally and scoped nothing.
    let f = manifest_filter(
        "include_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
    );
    let key = f.manifest_predicates()[0].cache_key();
    let i = idx_with_manifest(
        &[
            "crates/app/bad.rs",
            "crates/lib/ok.rs",
            "vendor/third.rs",
            "cratesx/y.rs",
        ],
        key,
        &["crates/*"],
    );
    assert!(
        f.matches(Path::new("crates/app/bad.rs"), &i),
        "file under a `crates/*` member is in scope"
    );
    assert!(
        f.matches(Path::new("crates/lib/ok.rs"), &i),
        "another `crates/*` member subtree is in scope"
    );
    assert!(
        !f.matches(Path::new("vendor/third.rs"), &i),
        "a sibling outside `crates/*` is dropped"
    );
    assert!(
        !f.matches(Path::new("cratesx/y.rs"), &i),
        "`crates/*` respects component boundaries (does not match `cratesx/`)"
    );
}

#[test]
fn exclude_manifest_paths_expands_glob_members() {
    // The `exclude` complement: files under a `crates/*` member are dropped,
    // everything else kept.
    let f = manifest_filter(
        "exclude_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
    );
    let key = f.manifest_predicates()[0].cache_key();
    let i = idx_with_manifest(&["crates/app/x.rs", "tools/y.rs"], key, &["crates/*"]);
    assert!(
        !f.matches(Path::new("crates/app/x.rs"), &i),
        "file under an excluded `crates/*` member is dropped"
    );
    assert!(
        f.matches(Path::new("tools/y.rs"), &i),
        "file outside the excluded glob is kept"
    );
}

#[test]
fn include_manifest_paths_expands_brace_glob_members() {
    // `members = ["crates/{app,lib}"]` (npm/pnpm brace alternation, which
    // globset expands) must be recognized as a glob, not matched literally.
    // Regression guard: the detection set once omitted `{`, so a brace member
    // silently scoped nothing.
    let f = manifest_filter(
        "include_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
    );
    let key = f.manifest_predicates()[0].cache_key();
    let i = idx_with_manifest(
        &["crates/app/x.rs", "crates/lib/y.rs", "crates/skip/z.rs"],
        key,
        &["crates/{app,lib}"],
    );
    assert!(
        f.matches(Path::new("crates/app/x.rs"), &i),
        "brace alternation scopes crates/app"
    );
    assert!(
        f.matches(Path::new("crates/lib/y.rs"), &i),
        "brace alternation scopes crates/lib"
    );
    assert!(
        !f.matches(Path::new("crates/skip/z.rs"), &i),
        "brace alternation excludes an unlisted member"
    );
}

#[test]
fn manifest_membership_is_directory_aware_prefix() {
    // A declared DIRECTORY (`workspace.members` -> `crates/a`) matches files
    // UNDER it; a shared-prefix sibling (`crates/ab`) does NOT match, because
    // membership is component-wise, not string-prefix.
    let f = manifest_filter(
        "include_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.workspace.members[*]\" }",
    );
    let key = f.manifest_predicates()[0].cache_key();
    let i = idx_with_manifest(
        &["crates/a/lib.rs", "crates/ab/x.rs", "vendor/y.rs"],
        key,
        &["crates/a"],
    );
    assert!(
        f.matches(Path::new("crates/a/lib.rs"), &i),
        "file under declared dir kept"
    );
    assert!(
        !f.matches(Path::new("crates/ab/x.rs"), &i),
        "shared-prefix sibling dir not matched (component boundary)"
    );
    assert!(
        !f.matches(Path::new("vendor/y.rs"), &i),
        "file outside members dropped"
    );
}

#[test]
fn manifest_absent_include_scopes_nothing_exclude_full() {
    // No cache entry (manifest unresolved) → include drops all, exclude keeps
    // all — consistent with has_ancestor's silent behaviour.
    let inc = manifest_filter(
        "include_manifest_paths:\n  source: p.json\n  extract: { json: \"$.x[*]\" }",
    );
    let exc = manifest_filter(
        "exclude_manifest_paths:\n  source: p.json\n  extract: { json: \"$.x[*]\" }",
    );
    let i = idx(&["a.rs"]); // no set_manifest_paths
    assert!(
        !inc.matches(Path::new("a.rs"), &i),
        "include + absent → nothing"
    );
    assert!(
        exc.matches(Path::new("a.rs"), &i),
        "exclude + absent → full scope"
    );
}

#[test]
fn manifest_and_composes_with_has_ancestor() {
    let f = manifest_filter(
        "has_ancestor: Cargo.toml\nexclude_manifest_paths:\n  source: Cargo.toml\n  extract: { toml: \"$.bin[*].path\" }",
    );
    let key = f.manifest_predicates()[0].cache_key().to_string();
    let i = idx(&[
        "crates/x/Cargo.toml",
        "crates/x/a.rs",
        "crates/x/b.rs",
        "loose/c.rs",
    ]);
    let mut map = std::collections::HashMap::new();
    map.insert(
        key,
        ManifestSet::from_paths([PathBuf::from("crates/x/b.rs")]),
    );
    i.set_manifest_paths(map, HashMap::new());
    assert!(
        f.matches(Path::new("crates/x/a.rs"), &i),
        "under manifest, not excluded"
    );
    assert!(
        !f.matches(Path::new("crates/x/b.rs"), &i),
        "excluded by manifest"
    );
    assert!(
        !f.matches(Path::new("loose/c.rs"), &i),
        "no ancestor manifest"
    );
}

#[test]
fn resolve_set_extracts_maps_and_confines() {
    let f = manifest_filter(
        "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }\n  derive_target: { from: '^dist/(.*)\\.js$', to: 'src/$1.ts' }",
    );
    let pred = &f.manifest_predicates()[0];
    let text = r#"{ "bin": { "cli": "dist/cli.js", "helper": "dist/sub/helper.js" } }"#;
    let set = pred.resolve_set(text).unwrap();
    assert!(
        set.contains(&PathBuf::from("src/cli.ts")),
        "mapped bin → src: {set:?}"
    );
    assert!(
        set.contains(&PathBuf::from("src/sub/helper.ts")),
        "nested mapped: {set:?}"
    );
}

#[test]
fn resolve_set_drops_non_matching_derive_target_entries() {
    let f = manifest_filter(
        "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }\n  derive_target: { from: '^dist/(.*)\\.js$', to: 'src/$1.ts' }",
    );
    let text = r#"{ "bin": { "sh": "scripts/run.sh" } }"#;
    assert!(
        f.manifest_predicates()[0]
            .resolve_set(text)
            .unwrap()
            .is_empty(),
        "an entry that doesn't match `from` is dropped"
    );
}

#[test]
fn resolve_set_rejects_non_string_members() {
    let f = manifest_filter(
        "include_manifest_paths:\n  source: package.json\n  extract: { json: \"$.files[*]\" }",
    );
    let failure = f.manifest_predicates()[0]
        .resolve_set(r#"{ "files": ["src", true, 7, null] }"#)
        .unwrap_err();
    assert_eq!(failure.kind, AnalysisFailureKind::ResourceParse);
    for kind in ["boolean", "number", "null"] {
        assert!(failure.message.contains(kind), "{}", failure.message);
    }
}

#[test]
fn resolve_set_is_relative_to_manifest_dir() {
    let f = manifest_filter(
        "include_manifest_paths:\n  source: packages/foo/package.json\n  extract: { json: \"$.files[*]\" }",
    );
    let text = r#"{ "files": ["src/index.ts"] }"#;
    let set = f.manifest_predicates()[0].resolve_set(text).unwrap();
    assert!(
        set.contains(&PathBuf::from("packages/foo/src/index.ts")),
        "declared path resolves under the manifest's own dir: {set:?}"
    );
}

#[test]
fn resolve_set_confines_escaping_declared_paths() {
    let f = manifest_filter(
        "exclude_manifest_paths:\n  source: packages/foo/package.json\n  extract: { json: \"$.files[*]\" }",
    );
    // `../../etc/x` escapes the repo root once resolved under packages/foo/.
    let text = r#"{ "files": ["../../etc/x", "src/ok.ts"] }"#;
    let set = f.manifest_predicates()[0].resolve_set(text).unwrap();
    assert!(set.contains(&PathBuf::from("packages/foo/src/ok.ts")));
    assert!(
        !set.iter().any(|p| p.starts_with("..")),
        "root-escaping declared path dropped: {set:?}"
    );
}

#[test]
fn resolve_set_keeps_negated_entries_in_declared_order() {
    // Audit 2026-10 finding 8: `!packages/internal` was resolved as the
    // literal path `packages/foo/!packages/internal`-style junk that matched
    // nothing. It must resolve (relative to the manifest dir) and keep its
    // `!` and its position.
    let f = manifest_filter(
        "include_manifest_paths:\n  source: web/package.json\n  extract: { json: \"$.workspaces[*]\" }",
    );
    let text = r#"{ "workspaces": ["packages/*", "!packages/internal", "packages/app"] }"#;
    let set = f.manifest_predicates()[0].resolve_set(text).unwrap();
    assert_eq!(
        set,
        vec![
            PathBuf::from("web/packages/*"),
            PathBuf::from("!web/packages/internal"),
            PathBuf::from("web/packages/app"),
        ],
        "declared order, `!` kept"
    );
}

#[test]
fn resolve_set_dedup_keeps_the_last_occurrence_so_a_re_include_wins() {
    // A repeated positive entry after a negation is a re-include under
    // last-match-wins, exactly like a distinct one (`packages/int*`). The
    // first-occurrence dedup dropped it and excluded `internal`.
    let f = manifest_filter(
        "include_manifest_paths:\n  source: package.json\n  extract: { json: \"$.workspaces[*]\" }",
    );
    let resolve = |text: &str| {
        let set = f.manifest_predicates()[0].resolve_set(text).unwrap();
        ManifestSet::from_paths(set)
    };
    let dup = resolve(r#"{ "workspaces": ["packages/*", "!packages/internal", "packages/*"] }"#);
    let distinct =
        resolve(r#"{ "workspaces": ["packages/*", "!packages/internal", "packages/int*"] }"#);
    for set in [&dup, &distinct] {
        assert!(set.contains_file(Path::new("packages/internal/a.txt")));
        assert!(set.contains_file(Path::new("packages/app/a.txt")));
    }
    // A plain duplicate collapses to its last position.
    let set = f.manifest_predicates()[0]
        .resolve_set(r#"{ "workspaces": ["a", "!b", "a", "!b"] }"#)
        .unwrap();
    assert_eq!(set, vec![PathBuf::from("a"), PathBuf::from("!b")]);
    // ...and the exclusion still holds when nothing re-includes it.
    let set = resolve(r#"{ "workspaces": ["packages/*", "packages/*", "!packages/internal"] }"#);
    assert!(!set.contains_file(Path::new("packages/internal/a.txt")));
}

/// Naive last-match-wins oracle over `(negated, pattern)` members.
fn oracle(members: &[String], file: &Path) -> bool {
    let mut inside = false;
    for m in members {
        let (negated, body) = match m.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, m.as_str()),
        };
        let hit = if is_glob_member(body) {
            let g = compile_member(body).unwrap().compile_matcher();
            let mut prefix = PathBuf::new();
            file.components().any(|c| {
                prefix.push(c);
                g.is_match(&prefix)
            })
        } else {
            file.starts_with(body)
        };
        if hit {
            inside = !negated;
        }
    }
    inside
}

#[test]
fn large_ordered_manifest_set_matches_the_last_match_oracle() {
    // The indexed (> SMALL_ORDERED members) and the short reverse-scan forms
    // must both agree with plain last-match-wins.
    let large: Vec<String> = (0..40)
        .map(|i| format!("packages/p{i}"))
        .chain([
            "!packages/p3".into(),
            "!packages/p1*".into(),
            "packages/p12".into(),
            "libs/*".into(),
            "!libs/b/gen".into(),
            "packages/p5".into(),
            "!packages/p5/test/**".into(),
        ])
        .collect();
    let small: Vec<String> = ["packages/*", "!packages/p1*", "packages/p12", "!**/test/**"]
        .into_iter()
        .map(String::from)
        .collect();
    let files = [
        "packages/p0/a.rs",
        "packages/p3/a.rs",
        "packages/p10/a.rs",
        "packages/p12/a.rs",
        "packages/p12/test/a.rs",
        "packages/p5/src/a.rs",
        "packages/p5/test/a.rs",
        "packages/p99/a.rs",
        "libs/a/x.rs",
        "libs/b/gen/x.rs",
        "libs/b/src/x.rs",
        "other/x.rs",
    ];
    for members in [&large, &small] {
        let set = ManifestSet::from_paths(members.iter().map(PathBuf::from));
        for f in files {
            assert_eq!(
                set.contains_file(Path::new(f)),
                oracle(members, Path::new(f)),
                "{f} with {} members",
                members.len()
            );
        }
    }
}

#[test]
fn large_ordered_manifest_set_is_not_files_times_members() {
    // A 5,000-entry workspace list with one `!` scanned every member for
    // every file: ~27 s for 100k files in release. Indexed, this debug-build
    // run of 40k files takes well under a second; the bound is generous.
    let members = (0..5000)
        .map(|i| PathBuf::from(format!("packages/p{i}")))
        .chain([PathBuf::from("!packages/p17")]);
    let set = ManifestSet::from_paths(members);
    let start = std::time::Instant::now();
    let n = (0..40_000)
        .filter(|i| {
            set.contains_file(Path::new(&format!(
                "packages/p{}/src/m{}/f.rs",
                i % 5000,
                i % 7
            )))
        })
        .count();
    assert_eq!(n, 40_000 - 8);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn manifest_set_negation_is_ordered_last_match_wins() {
    let set = ManifestSet::from_paths(
        ["packages/*", "!packages/internal", "packages/internal/keep"]
            .into_iter()
            .map(PathBuf::from),
    );
    assert!(!set.is_empty());
    assert!(set.contains_file(Path::new("packages/app/src/a.ts")));
    assert!(!set.contains_file(Path::new("packages/internal/src/a.ts")));
    assert!(set.contains_file(Path::new("packages/internal/keep/a.ts")));
    assert!(!set.contains_file(Path::new("vendor/a.ts")));
    // A negated glob excludes across members.
    let set = ManifestSet::from_paths(["packages/*", "!**/test/**"].into_iter().map(PathBuf::from));
    assert!(set.contains_file(Path::new("packages/a/src/x.ts")));
    assert!(!set.contains_file(Path::new("packages/a/test/x.ts")));
    // Order matters: a negation BEFORE the positive is overridden by it.
    let set = ManifestSet::from_paths(
        ["!packages/internal", "packages/*"]
            .into_iter()
            .map(PathBuf::from),
    );
    assert!(set.contains_file(Path::new("packages/internal/a.ts")));
    // Only negations include nothing (and count as empty for the warning).
    let set = ManifestSet::from_paths(["!packages/a"].into_iter().map(PathBuf::from));
    assert!(set.is_empty());
    assert!(!set.contains_file(Path::new("packages/b/x.ts")));
}

#[test]
fn from_spec_accepts_manifest_only() {
    let f = manifest_filter(
        "include_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
    );
    assert_eq!(f.manifest_predicates().len(), 1);
}

#[test]
fn from_spec_rejects_source_escaping_root() {
    let spec: ScopeFilterSpec = ayaml::compat::from_str(
        "exclude_manifest_paths:\n  source: ../outside.json\n  extract: { json: \"$.x[*]\" }",
    )
    .unwrap();
    let err = ScopeFilter::from_spec("r", spec).unwrap_err();
    assert!(err.to_string().contains("repo-root-relative"), "msg: {err}");
}

#[test]
fn shared_config_produces_one_cache_key() {
    // Two predicates with an identical (source, extract) config share a key,
    // so the engine resolves the manifest once.
    let a = manifest_filter(
        "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
    );
    let b = manifest_filter(
        "exclude_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }",
    );
    assert_eq!(
        a.manifest_predicates()[0].cache_key(),
        b.manifest_predicates()[0].cache_key()
    );
}

#[test]
fn distinct_derive_targets_do_not_share_a_cache_key() {
    // Regression: the `derive_target` component of the cache key must be
    // injective. A plain `=>` join let `{from:"a", to:"b=>c"}` and
    // `{from:"a=>b", to:"c"}` forge one key (identical source+extract) and
    // silently share a wrong resolved set. NUL-delimiting keeps them distinct.
    let a = manifest_filter(
        "include_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }\n  derive_target: { from: \"a\", to: \"b=>c\" }",
    );
    let b = manifest_filter(
        "include_manifest_paths:\n  source: package.json\n  extract: { json: \"$.bin.*\" }\n  derive_target: { from: \"a=>b\", to: \"c\" }",
    );
    assert_ne!(
        a.manifest_predicates()[0].cache_key(),
        b.manifest_predicates()[0].cache_key(),
        "distinct derive_targets must not forge one cache key"
    );
}

#[test]
fn from_spec_rejects_nul_in_derive_target() {
    // A double-quoted YAML `"a\0b"` decodes to a literal NUL, which would
    // break the NUL-delimited cache key's injectivity -> rejected at build.
    let spec: ScopeFilterSpec = ayaml::compat::from_str(
            "include_manifest_paths:\n  source: m.json\n  extract: { json: \"$.x[*]\" }\n  derive_target: { from: \"x\", to: \"a\\0b\" }",
        )
        .expect("scope_filter parses");
    let err = ScopeFilter::from_spec("r", spec).unwrap_err();
    assert!(
        err.to_string().contains("NUL"),
        "expected a NUL rejection, got: {err}"
    );
}

#[test]
fn resolve_set_drops_vacuous_nested_entry() {
    // A nested manifest's `.` / `` / self-canceling entry must be dropped, not
    // resolved to the manifest's own directory (`base.join(".")` == base),
    // which would silently scope the manifest's whole subtree. A real child is
    // still kept, resolved under the manifest dir.
    let f = manifest_filter(
        "include_manifest_paths:\n  source: pkg/tsconfig.json\n  extract: { json: \"$.include[*]\" }",
    );
    let set = f.manifest_predicates()[0]
        .resolve_set(r#"{"include": [".", "", "a/..", "src"]}"#)
        .unwrap();
    assert!(
        !set.contains(&PathBuf::from("pkg")),
        "a vacuous entry must not scope the whole subtree: {set:?}"
    );
    assert!(
        set.contains(&PathBuf::from("pkg/src")),
        "a real child resolves under the manifest dir: {set:?}"
    );
    assert_eq!(set.len(), 1, "only the real child survives: {set:?}");
}

#[test]
fn resolve_set_keeps_nested_sibling_reference() {
    // Only an entry collapsing to the manifest's OWN dir is dropped; an
    // in-root `../sibling` reference (a nested `tsconfig` pointing at a sibling
    // package, a documented supported case) resolves elsewhere and is KEPT.
    // (Regression: an over-broad standalone-`..` drop silently unscoped it.)
    let f = manifest_filter(
        "include_manifest_paths:\n  source: packages/web/tsconfig.json\n  extract: { json: \"$.include[*]\" }",
    );
    let set = f.manifest_predicates()[0]
        .resolve_set(r#"{"include": ["src", "../shared/src", "."]}"#)
        .unwrap();
    assert!(
        set.contains(&PathBuf::from("packages/web/src")),
        "the child `src` is kept: {set:?}"
    );
    assert!(
        set.contains(&PathBuf::from("packages/shared/src")),
        "the in-root sibling `../shared/src` is kept: {set:?}"
    );
    assert!(
        !set.contains(&PathBuf::from("packages/web")),
        "the vacuous `.` is dropped (not the whole subtree): {set:?}"
    );
    assert_eq!(set.len(), 2, "src + shared/src, not `.`: {set:?}");
}

#[test]
fn read_manifest_confined_reads_in_root_and_refuses_escape() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path();
    std::fs::write(root.join("m.json"), r#"{"x":1}"#).unwrap();
    // A regular in-root manifest reads.
    assert_eq!(
        read_manifest_confined(root, Path::new("m.json")).unwrap(),
        r#"{"x":1}"#
    );
    let failure = read_manifest_confined(root, Path::new("missing.json")).unwrap_err();
    assert_eq!(failure.kind, AnalysisFailureKind::ResourceRead);
    assert!(failure.message.contains("missing"), "{}", failure.message);
}

#[test]
fn include_manifest_rejects_invalid_utf8_without_disclosing_bytes_or_values() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path();
    let filter = manifest_filter(
        "include_manifest_paths:\n  source: manifest.json\n  extract: { json: \"$.paths[*]\" }",
    );
    let predicate = &filter.manifest_predicates()[0];
    std::fs::write(
        root.join(predicate.source()),
        b"{\"paths\":[\"\xff-secret\"]}",
    )
    .unwrap();

    let failure = read_manifest_confined(root, predicate.source()).expect_err("invalid UTF-8");
    assert_eq!(failure.kind, AnalysisFailureKind::ResourceParse);
    assert_eq!(failure.message, "scope manifest is not valid UTF-8");
    assert!(!failure.message.contains('\u{fffd}'));
    assert!(!failure.message.contains("secret"));
}

#[cfg(unix)]
#[test]
fn read_manifest_confined_refuses_escaping_symlink() {
    let root_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap(); // a different tree, not under root
    let root = root_dir.path();
    std::fs::write(outside_dir.path().join("secret.json"), "SECRET").unwrap();
    std::os::unix::fs::symlink(
        outside_dir.path().join("secret.json"),
        root.join("link.json"),
    )
    .unwrap();
    let failure = read_manifest_confined(root, Path::new("link.json")).unwrap_err();
    assert_eq!(failure.kind, AnalysisFailureKind::ResourceRead);
    assert!(failure.message.contains("outside"), "{}", failure.message);
}

#[test]
fn manifest_set_copies_onto_the_filtered_changed_index() {
    // The `--changed` fix: the engine resolves manifests against the full
    // index (where the manifest is reachable), then copies the resolved map
    // onto the fresh filtered index that per-file `matches` reads. Guards
    // that copy so an `include`/`exclude` predicate isn't silently empty
    // under `--changed`.
    let full = idx_with_manifest(&["a.rs"], "KEY", &["crates/a"]);
    let filtered = idx(&["a.rs"]); // a fresh filtered index, empty cache
    assert!(
        filtered.manifest_paths("KEY").is_none(),
        "filtered index starts with no manifest cache"
    );
    filtered.set_manifest_paths(full.manifest_paths_map().unwrap().clone(), HashMap::new());
    assert!(
        filtered
            .manifest_paths("KEY")
            .unwrap()
            .contains_file(Path::new("crates/a")),
        "the resolved set is visible on the filtered index after the copy"
    );
}

#[test]
fn changed_since_diff_copies_onto_the_filtered_changed_index() {
    // Sibling of the manifest copy: a `scope_filter.changed_since:` diff is
    // resolved on the FULL index and must be copied onto the fresh `--changed`
    // filtered index too, else a `changed_since` rule silently matches nothing
    // under `--changed` (the diff cache would be unpopulated on the filtered
    // index).
    let full = idx(&["a.rs"]);
    let mut m = std::collections::HashMap::new();
    m.insert(
        "REF".to_string(),
        [PathBuf::from("a.rs")].into_iter().collect(),
    );
    full.set_changed_paths(m);
    let filtered = idx(&["a.rs"]); // fresh filtered index, empty cache
    assert!(
        filtered.changed_paths("REF").is_none(),
        "filtered index starts with no changed-paths cache"
    );
    filtered.set_changed_paths(full.changed_paths_map().unwrap().clone());
    assert!(
        filtered
            .changed_paths("REF")
            .unwrap()
            .contains(Path::new("a.rs")),
        "the changed_since diff is visible on the filtered index after the copy"
    );
}
