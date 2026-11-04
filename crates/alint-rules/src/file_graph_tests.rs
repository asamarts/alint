use super::*;
use alint_core::{FileEntry, FileIndex, Format};

#[test]
fn require_spec_error_names_accepted_forms_not_internal_enum() {
    // A value that is neither a mode name nor a map is rejected with a
    // message naming the accepted forms, not the internal untagged enum.
    let err = ayaml::compat::from_str::<RequireSpec>("42").unwrap_err();
    assert!(
        err.to_string().contains("a mode name"),
        "expected a friendly message, got: {err}"
    );
    assert!(
        !err.to_string().contains("RequireSpec"),
        "message leaks the internal enum name: {err}"
    );
}

fn index(files: &[&str]) -> FileIndex {
    FileIndex::from_entries(
        files
            .iter()
            .map(|p| FileEntry {
                path: Path::new(p).into(),
                is_dir: false,
                size: 1,
            })
            .collect(),
    )
}

fn scope(pat: &str) -> Scope {
    Scope::from_patterns(slice::from_ref(&pat.to_string())).expect("valid glob")
}

fn forbidden(nodes: &str, regex: &str, resolve: Resolve, from: &str, to: &str) -> FileGraphRule {
    FileGraphRule {
        id: "t".into(),
        level: Level::Error,
        policy_url: None,
        message: None,
        nodes: scope(nodes),
        edges: EdgeSource::FromContent {
            extract: Extract::Regex(regex.into()),
            resolve,
        },
        require: Require::ForbiddenEdges(vec![ForbiddenPattern {
            from: scope(from),
            to: scope(to),
            from_glob: from.into(),
            to_glob: to.into(),
        }]),
    }
}

#[test]
fn node_error_and_orphan_on_same_node_dont_collide() {
    // A node that is BOTH errored (unreadable / too-large / out-of-repo
    // target) AND an orphan lands two path-only, line-less findings on the
    // SAME path. Without distinct baseline_keys they share a fingerprint and
    // a baseline silently masks a genuinely-new one (baseline.md §6). Both
    // keys — and therefore both fingerprints — must differ.
    let rule = acyclic(
        "**/*.proto",
        r#"import\s+"([^"]+)""#,
        Resolve::RelativeToRepoRoot,
    );
    let node = Path::new("proto/a.proto");
    let node_err = FileGraphRule::node_violation(node, "could not be read: boom");
    let orphan = rule.orphan_violation(node);
    assert_ne!(
        node_err.baseline_key, orphan.baseline_key,
        "node-error and orphan on the same node must carry distinct baseline_keys"
    );
    let fp = |v: &Violation| alint_core::baseline::fingerprint("file_graph", v, None);
    assert_ne!(
        fp(&node_err),
        fp(&orphan),
        "distinct keys must yield distinct fingerprints (no baseline masking)"
    );
}

fn acyclic(nodes: &str, regex: &str, resolve: Resolve) -> FileGraphRule {
    FileGraphRule {
        id: "t".into(),
        level: Level::Error,
        policy_url: None,
        message: None,
        nodes: scope(nodes),
        edges: EdgeSource::FromContent {
            extract: Extract::Regex(regex.into()),
            resolve,
        },
        require: Require::Acyclic,
    }
}

fn eval(r: &FileGraphRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
    let ctx = Context {
        root,
        index: idx,
        registry: None,
        facts: None,
        vars: None,
        git_tracked: None,
        git_blame: None,
        analysis_time: None,
        process_environment: None,
        trace: None,
    };
    r.evaluate(&ctx).expect("evaluate ok")
}

#[test]
fn forbidden_edge_fires_on_relative_import() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/domain")).unwrap();
    std::fs::create_dir_all(root.join("src/infra")).unwrap();
    // domain reaches into infra — forbidden.
    std::fs::write(
        root.join("src/domain/order.ts"),
        "import { db } from \"../infra/db\";\n",
    )
    .unwrap();
    std::fs::write(root.join("src/infra/db.ts"), "export const db = 1;\n").unwrap();
    let idx = index(&["src/domain/order.ts", "src/infra/db.ts"]);
    let r = forbidden(
        "src/**/*.ts",
        r#"from\s+"(\.[^"]+)""#,
        Resolve::RelativeToFile,
        "src/domain/**",
        "src/infra/**",
    );
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("src/domain/order.ts"));
    assert!(v[0].message.contains("src/infra/db"));
}

#[test]
fn forbidden_edge_silent_when_layering_respected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/domain")).unwrap();
    std::fs::create_dir_all(root.join("src/infra")).unwrap();
    // infra → domain is allowed; domain imports a sibling only.
    std::fs::write(
        root.join("src/domain/order.ts"),
        "import { money } from \"./money\";\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/domain/money.ts"),
        "export const money = 1;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/infra/db.ts"),
        "import { order } from \"../domain/order\";\n",
    )
    .unwrap();
    let idx = index(&[
        "src/domain/order.ts",
        "src/domain/money.ts",
        "src/infra/db.ts",
    ]);
    let r = forbidden(
        "src/**/*.ts",
        r#"from\s+"(\.[^"]+)""#,
        Resolve::RelativeToFile,
        "src/domain/**",
        "src/infra/**",
    );
    assert!(eval(&r, root, &idx).is_empty());
}

#[test]
fn bare_specifier_is_dropped_not_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/domain")).unwrap();
    // A bare module specifier that lexically contains "infra"
    // must NOT be treated as a path edge into src/infra.
    std::fs::write(
        root.join("src/domain/order.ts"),
        "import x from \"@company/infra-sdk\";\n",
    )
    .unwrap();
    let idx = index(&["src/domain/order.ts"]);
    let r = forbidden(
        "src/**/*.ts",
        r#"from\s+"([^"]+)""#,
        Resolve::RelativeToFile,
        "src/domain/**",
        "**/infra*/**",
    );
    assert!(
        eval(&r, root, &idx).is_empty(),
        "bare specifier must not resolve to a path edge",
    );
}

#[test]
fn acyclic_fires_on_two_and_three_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    // a -> b -> c -> a (a 3-cycle).
    std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
    std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
    std::fs::write(root.join("proto/c.proto"), "import \"proto/a.proto\";\n").unwrap();
    let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
    let r = acyclic(
        "proto/**/*.proto",
        r#"import\s+"([^"]+)""#,
        Resolve::RelativeToRepoRoot,
    );
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "one distinct cycle: {v:?}");
    assert!(v[0].message.contains("dependency cycle"));
    // Canonical: starts at the smallest path (proto/a.proto).
    assert!(
        v[0].message
            .contains("proto/a.proto \u{2192} proto/b.proto")
    );
}

#[test]
fn acyclic_reports_every_file_on_a_cycle_once_per_strongly_connected_set() {
    // Regression: edges a->b, a->d, b->c, c->a, d->b. One DFS back-edge
    // per GRAY hit reported only a->b->c->a; d (on d->b->c->a->d) was never
    // reported because b was already BLACK when d was explored. Reporting
    // per strongly connected component names every participating file.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("p")).unwrap();
    let w = |name: &str, deps: &[&str]| {
        let mut body = String::new();
        for d in deps {
            body.push_str("import \"p/");
            body.push_str(d);
            body.push_str("\";\n");
        }
        std::fs::write(root.join("p").join(name), body).unwrap();
    };
    w("a", &["b", "d"]);
    w("b", &["c"]);
    w("c", &["a"]);
    w("d", &["b"]);
    // An unrelated 2-cycle: a separate component, a separate finding.
    w("x", &["y"]);
    w("y", &["x"]);
    let idx = index(&["p/a", "p/b", "p/c", "p/d", "p/x", "p/y"]);
    let r = acyclic("p/*", r#"import\s+"([^"]+)""#, Resolve::RelativeToRepoRoot);
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 2, "one finding per cyclic component: {v:?}");
    assert!(v[0].message.contains("p/d"), "d is on a cycle: {v:?}");
    assert_eq!(
        v[0].baseline_key.as_deref(),
        Some("cycle\u{0}p/a\u{0}p/b\u{0}p/c\u{0}p/d")
    );
    // A component that IS a simple cycle keeps the exact path rendering
    // (and the member-set key) the per-cycle reporting produced.
    assert!(
        v[1].message.contains("p/x \u{2192} p/y \u{2192} p/x"),
        "{v:?}"
    );
    assert_eq!(v[1].baseline_key.as_deref(), Some("cycle\u{0}p/x\u{0}p/y"));
    // Deterministic across runs.
    let again = eval(&r, root, &idx);
    assert_eq!(
        v.iter().map(|x| &x.message).collect::<Vec<_>>(),
        again.iter().map(|x| &x.message).collect::<Vec<_>>()
    );
}

#[test]
fn acyclic_silent_on_a_dag() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    // a -> b -> c, no back-edge.
    std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
    std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
    std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
    let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
    let r = acyclic(
        "proto/**/*.proto",
        r#"import\s+"([^"]+)""#,
        Resolve::RelativeToRepoRoot,
    );
    assert!(eval(&r, root, &idx).is_empty());
}

#[test]
fn self_loop_is_not_a_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    std::fs::write(root.join("proto/a.proto"), "import \"proto/a.proto\";\n").unwrap();
    let idx = index(&["proto/a.proto"]);
    let r = acyclic(
        "proto/**/*.proto",
        r#"import\s+"([^"]+)""#,
        Resolve::RelativeToRepoRoot,
    );
    assert!(eval(&r, root, &idx).is_empty(), "a self-edge is degenerate");
}

#[test]
fn resolve_ref_drops_non_path_references() {
    let f = Path::new("src/a/b.ts");
    // Relative-to-file: bare specifier dropped, relative kept.
    assert_eq!(
        resolve_ref("./c", f, Resolve::RelativeToFile),
        Some(PathBuf::from("src/a/c"))
    );
    assert_eq!(
        resolve_ref("../d/e", f, Resolve::RelativeToFile),
        Some(PathBuf::from("src/d/e"))
    );
    assert_eq!(resolve_ref("react", f, Resolve::RelativeToFile), None);
    // Root-escaping reference is dropped.
    assert_eq!(
        resolve_ref("../../../etc/passwd", f, Resolve::RelativeToFile),
        None
    );
    // Relative-to-root: bare path kept, absolute / URL dropped.
    assert_eq!(
        resolve_ref("a/b.proto", f, Resolve::RelativeToRepoRoot),
        Some(PathBuf::from("a/b.proto"))
    );
    assert_eq!(resolve_ref("/abs", f, Resolve::RelativeToRepoRoot), None);
    assert_eq!(
        resolve_ref("https://x/y", f, Resolve::RelativeToRepoRoot),
        None
    );
}

/// Generic constructor for the require modes the `forbidden` /
/// `acyclic` helpers don't cover.
fn mk(nodes: &str, regex: &str, resolve: Resolve, require: Require) -> FileGraphRule {
    FileGraphRule {
        id: "t".into(),
        level: Level::Error,
        policy_url: None,
        message: None,
        nodes: scope(nodes),
        edges: EdgeSource::FromContent {
            extract: Extract::Regex(regex.into()),
            resolve,
        },
        require,
    }
}

#[test]
fn dotenv_and_properties_graph_edges_redact_derived_targets() {
    const SECRET: &str = "TOKEN_VALUE_DO_NOT_REPORT";
    for (format, file, body) in [
        (Format::Dotenv, ".env", format!("DEP={SECRET}\n")),
        (
            Format::Properties,
            "app.properties",
            format!("DEP={SECRET}\n"),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(file), body).unwrap();
        let mut rule = FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(file),
            edges: EdgeSource::FromContent {
                extract: Extract::structured(format, "$.DEP").unwrap(),
                resolve: Resolve::RelativeToRepoRoot,
            },
            require: Require::NoDangling,
        };
        let findings = eval(&rule, dir.path(), &index(&[file]));
        assert_eq!(findings.len(), 1);
        assert!(findings[0].is_sensitive());
        assert!(!findings[0].message.contains(SECRET), "{findings:?}");
        assert!(findings[0].message.contains("<redacted:"), "{findings:?}");
        assert!(findings[0].message.contains(file), "{findings:?}");

        rule.require = Require::NoOrphans { roots: None };
        let findings = eval(&rule, dir.path(), &index(&[file]));
        assert_eq!(findings.len(), 1);
        assert!(findings[0].is_sensitive(), "{findings:?}");
        assert!(!findings[0].message.contains(SECRET), "{findings:?}");
    }
}

#[test]
fn no_dangling_fires_on_missing_then_silent_when_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/real.md"), "# real\n").unwrap();
    let r = mk(
        "docs/**/*.md",
        r"\]\((\.[^)]+)\)",
        Resolve::RelativeToFile,
        Require::NoDangling,
    );

    // a.md links a sibling that doesn't exist -> dangling.
    std::fs::write(root.join("docs/a.md"), "see [x](./missing.md)\n").unwrap();
    let idx = index(&["docs/a.md", "docs/real.md"]);
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("docs/missing.md"));
    assert!(v[0].message.contains("docs/a.md"));
    assert_eq!(v[0].path.as_deref(), Some(Path::new("docs/a.md")));
    assert_eq!(v[0].related_locations.len(), 1);
    assert_eq!(v[0].related_locations[0].path, Path::new("docs/missing.md"));
    assert_eq!(v[0].related_locations[0].label, "unresolved target");

    // a.md links the existing real.md -> silent.
    std::fs::write(root.join("docs/a.md"), "see [r](./real.md)\n").unwrap();
    assert!(eval(&r, root, &idx).is_empty());
}

#[test]
fn no_dangling_dedups_a_repeated_edge() {
    // A node that references the same missing target twice yields ONE
    // dangling violation, not two (the duplicate edge is deduped).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let r = mk(
        "docs/**/*.md",
        r"\]\((\.[^)]+)\)",
        Resolve::RelativeToFile,
        Require::NoDangling,
    );
    std::fs::write(
        root.join("docs/a.md"),
        "[x](./missing.md) and [y](./missing.md)\n",
    )
    .unwrap();
    let idx = index(&["docs/a.md"]);
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "repeated dangling edge deduped: {v:?}");
    assert!(v[0].message.contains("docs/missing.md"));
}

#[test]
fn no_orphans_fires_on_unreferenced_node() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    // a -> b -> c. Nothing references a, so a is the orphan.
    std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
    std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
    std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
    let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
    let r = mk(
        "proto/**/*.proto",
        r#"import\s+"([^"]+)""#,
        Resolve::RelativeToRepoRoot,
        Require::NoOrphans { roots: None },
    );
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "only proto/a.proto is unreferenced: {v:?}");
    assert!(v[0].message.contains("proto/a.proto"));
    assert!(v[0].message.contains("orphan"));
}

#[test]
fn no_orphans_roots_exempts_entry_point() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
    std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
    std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
    let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
    let r = mk(
        "proto/**/*.proto",
        r#"import\s+"([^"]+)""#,
        Resolve::RelativeToRepoRoot,
        Require::NoOrphans {
            roots: Some(scope("proto/a.proto")),
        },
    );
    assert!(
        eval(&r, root, &idx).is_empty(),
        "the declared root is exempt from the orphan check"
    );
}

#[test]
fn build_accepts_named_and_map_require_forms() {
    use crate::test_support::spec_yaml;
    let base = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  \
                    from_content:\n    extract:\n      regex: 'x'\n";
    for tail in [
        "require: no_dangling\nlevel: error\n",
        "require: no_orphans\nlevel: error\n",
        "require:\n  no_orphans:\n    roots: [\"src/main.rs\"]\nlevel: error\n",
    ] {
        let yaml = format!("{base}{tail}");
        assert!(build(&spec_yaml(&yaml)).is_ok(), "should build: {yaml}");
    }
    // Two map modes at once -> rejected.
    let bad = format!(
        "{base}require:\n  forbidden_edges:\n    - {{from: a, to: b}}\n  \
             no_orphans: {{}}\nlevel: error\n"
    );
    assert!(
        build(&spec_yaml(&bad)).is_err(),
        "setting two map modes must be rejected"
    );
}

fn fresh_rule(nodes: &str, from: &str, to: &str, marker: &str) -> FileGraphRule {
    FileGraphRule {
        id: "t".into(),
        level: Level::Error,
        policy_url: None,
        message: None,
        nodes: scope(nodes),
        edges: EdgeSource::DeriveTarget {
            from: Regex::new(from).expect("valid from regex"),
            to: to.into(),
        },
        require: Require::Fresh {
            algo: Algorithm::Sha256,
            marker: Regex::new(marker).expect("valid marker regex"),
        },
    }
}

#[test]
fn fresh_silent_when_marker_matches_then_stale_when_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    let src = "message A {}\n";
    std::fs::write(root.join("proto/a.proto"), src).unwrap();
    let hash = Algorithm::Sha256.hex(src.as_bytes());
    std::fs::write(
        root.join("proto/a.pb.go"),
        format!("// @generated sha256:{hash}\npackage a\n"),
    )
    .unwrap();
    let idx = index(&["proto/a.proto", "proto/a.pb.go"]);
    let r = fresh_rule(
        "proto/**/*.proto",
        r"(.*)\.proto",
        "$1.pb.go",
        r"sha256:([0-9a-f]{64})",
    );
    assert!(eval(&r, root, &idx).is_empty(), "marker matches -> fresh");

    // The source changes; the generated marker is now stale.
    std::fs::write(root.join("proto/a.proto"), "message A { reserved 1; }\n").unwrap();
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("proto/a.pb.go"));
    assert!(v[0].message.contains("out of date"));
    assert_eq!(v[0].path.as_deref(), Some(Path::new("proto/a.pb.go")));
    assert_eq!(v[0].related_locations.len(), 1);
    assert_eq!(v[0].related_locations[0].path, Path::new("proto/a.proto"));
    assert_eq!(v[0].related_locations[0].label, "freshness source");
}

#[test]
fn fresh_fires_when_derived_target_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    std::fs::write(root.join("proto/a.proto"), "message A {}\n").unwrap();
    let idx = index(&["proto/a.proto"]);
    let r = fresh_rule(
        "proto/**/*.proto",
        r"(.*)\.proto",
        "$1.pb.go",
        r"sha256:([0-9a-f]{64})",
    );
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].path.as_deref(), Some(Path::new("proto/a.pb.go")));
    assert!(v[0].analysis_failure_kind().is_none());
}

#[cfg(unix)]
#[test]
fn fresh_never_reads_a_target_through_an_escaping_symlink() {
    // Confinement regression: the derived target was only LEXICALLY confined
    // (`normalize_confined` is symlink-blind), so an in-repo `out -> /outside`
    // symlink let `require: fresh` read a file outside the tree. It must use
    // the symlink-aware `confine_read` like pair_hash / registry_paths_resolve.
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    let src = "message A {}\n";
    std::fs::write(root.join("proto/a.proto"), src).unwrap();
    let hash = Algorithm::Sha256.hex(src.as_bytes());
    std::fs::write(
        outside.path().join("a.pb.go"),
        format!("// @generated sha256:{hash}\n"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("out")).unwrap();
    let idx = index(&["proto/a.proto"]);
    let r = fresh_rule(
        "proto/**/*.proto",
        r"proto/(.*)\.proto",
        "out/$1.pb.go",
        r"sha256:([0-9a-f]{64})",
    );
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "an escaping target must be flagged: {v:?}");
    assert!(v[0].message.contains("escapes the repo root"), "{v:?}");
}

#[test]
fn fresh_skips_sources_not_matching_from() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    std::fs::write(root.join("proto/notes.txt"), "not generated\n").unwrap();
    let idx = index(&["proto/notes.txt"]);
    // `from` matches only *.proto, so notes.txt is skipped.
    let r = fresh_rule(
        "proto/**/*",
        r"(.*)\.proto",
        "$1.pb.go",
        r"sha256:([0-9a-f]{64})",
    );
    assert!(
        eval(&r, root, &idx).is_empty(),
        "a source not matching `from` is skipped"
    );
}

fn derive_dangling_rule(nodes: &str, from: &str, to: &str) -> FileGraphRule {
    FileGraphRule {
        id: "t".into(),
        level: Level::Error,
        policy_url: None,
        message: None,
        nodes: scope(nodes),
        edges: EdgeSource::DeriveTarget {
            from: Regex::new(from).expect("valid from regex"),
            to: to.into(),
        },
        require: Require::NoDangling,
    }
}

#[test]
fn derive_target_no_dangling_requires_the_derived_sibling() {
    // The elasticsearch shape: every licenses/X-LICENSE.txt must
    // be accompanied by a sibling X-NOTICE.txt. No file read — the
    // edge is derived purely from the node path.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("licenses")).unwrap();
    std::fs::write(root.join("licenses/arrow-LICENSE.txt"), "A\n").unwrap();
    std::fs::write(root.join("licenses/arrow-NOTICE.txt"), "N\n").unwrap();
    let r = derive_dangling_rule("licenses/**", r"(.+)-LICENSE\.txt", "$1-NOTICE.txt");

    // sibling present -> silent.
    let idx = index(&["licenses/arrow-LICENSE.txt", "licenses/arrow-NOTICE.txt"]);
    assert!(eval(&r, root, &idx).is_empty(), "sibling present -> silent");

    // a second license with no NOTICE sibling -> dangling.
    let idx = index(&[
        "licenses/arrow-LICENSE.txt",
        "licenses/arrow-NOTICE.txt",
        "licenses/lucene-LICENSE.txt",
    ]);
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("licenses/lucene-NOTICE.txt"));
    assert!(v[0].message.contains("licenses/lucene-LICENSE.txt"));
}

#[test]
fn derive_target_root_escape_fires_and_is_never_read() {
    // Security regression (v0.12 path-confinement): an absolute or
    // `..`-escaping `to:` must produce an "escapes the repo root"
    // violation, never a filesystem read of the out-of-tree path.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("proto")).unwrap();
    std::fs::write(root.join("proto/a.proto"), "x").unwrap();
    let idx = index(&["proto/a.proto"]);

    // no_dangling + absolute `to:` -> escapes (the read oracle shape).
    let r = derive_dangling_rule("proto/**/*.proto", r"(.*)\.proto", "/etc/passwd");
    let v = eval(&r, root, &idx);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(
        v[0].message.contains("escapes the repo root"),
        "{}",
        v[0].message
    );

    // no_dangling + `..`-escape -> escapes.
    let r2 = derive_dangling_rule("proto/**/*.proto", r"(.*)/(.*)\.proto", "../../$2.out");
    assert!(
        eval(&r2, root, &idx)
            .iter()
            .any(|x| x.message.contains("escapes")),
    );

    // fresh + absolute `to:` -> escapes, WITHOUT reading the abs file.
    let f = fresh_rule(
        "proto/**/*.proto",
        r"(.*)\.proto",
        "/etc/hostname",
        r"sha256:([0-9a-f]{64})",
    );
    let vf = eval(&f, root, &idx);
    assert_eq!(vf.len(), 1, "{vf:?}");
    assert!(
        vf[0].message.contains("escapes the repo root"),
        "{}",
        vf[0].message
    );
}

#[test]
fn derive_target_no_dangling_skips_nodes_not_matching_from() {
    // A node the `from` regex doesn't capture has no derived edge,
    // so it can't dangle (README.md is not an `*-LICENSE.txt`).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("licenses")).unwrap();
    std::fs::write(root.join("licenses/README.md"), "# licenses\n").unwrap();
    let idx = index(&["licenses/README.md"]);
    let r = derive_dangling_rule("licenses/**", r"(.+)-LICENSE\.txt", "$1-NOTICE.txt");
    assert!(
        eval(&r, root, &idx).is_empty(),
        "a node not matching `from` has no edge"
    );
}

#[test]
fn build_fresh_mode_and_edge_coupling() {
    use crate::test_support::spec_yaml;
    let ok = "id: t\nkind: file_graph\nnodes: \"**/*.proto\"\nedges:\n  \
                  derive_target:\n    from: '(.*)\\.proto'\n    to: '$1.pb.go'\nrequire:\n  \
                  fresh:\n    marker: 'sha256:([0-9a-f]{64})'\nlevel: error\n";
    assert!(
        build(&spec_yaml(ok)).is_ok(),
        "derive_target + fresh should build: {ok}"
    );

    // `fresh` with `from_content` -> rejected (wrong edge type).
    let bad_fc = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  from_content:\n    \
                      extract:\n      regex: 'x'\nrequire:\n  fresh:\n    \
                      marker: 'h:([0-9a-f])'\nlevel: error\n";
    assert!(
        build(&spec_yaml(bad_fc)).is_err(),
        "fresh needs derive_target"
    );

    // `derive_target` with a content-graph mode (acyclic) -> rejected.
    let bad_dt = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  derive_target:\n    \
                      from: 'a'\n    to: 'b'\nrequire: acyclic\nlevel: error\n";
    assert!(
        build(&spec_yaml(bad_dt)).is_err(),
        "derive_target rejected for the content-graph modes"
    );

    // `derive_target` with `no_dangling` -> builds (v0.12 decouple:
    // the derived sibling must merely exist).
    let dt_nd = "id: t\nkind: file_graph\nnodes: \"licenses/**\"\nedges:\n  \
                     derive_target:\n    from: '(.+)-LICENSE\\.txt'\n    to: '$1-NOTICE.txt'\n\
                     require: no_dangling\nlevel: error\n";
    assert!(
        build(&spec_yaml(dt_nd)).is_ok(),
        "derive_target + no_dangling should build: {dt_nd}"
    );

    // A marker with no capture group -> rejected.
    let bad_marker = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  \
                          derive_target:\n    from: 'a'\n    to: 'b'\nrequire:\n  fresh:\n    \
                          marker: 'nogroup'\nlevel: error\n";
    assert!(
        build(&spec_yaml(bad_marker)).is_err(),
        "marker needs a capture group"
    );
}
