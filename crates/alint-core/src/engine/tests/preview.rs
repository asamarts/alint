use super::*;

// ---- `fix --dry-run` predicts the creates a real fix demotes ----

/// Requires `checks` and creates `path` (a file with `content`, or a directory
/// when `dir`), as `file_create` and `dir_create` do: under the root joined
/// with the lexically normalized path, and only when nothing is there yet.
// Each flag mirrors one rule option a test turns on.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct CreateRule {
    id: &'static str,
    checks: PathBuf,
    path: PathBuf,
    content: &'static [u8],
    dir: bool,
    /// The rule's `paths:`; without one it looks for `checks` as written, as
    /// `dir_exists` and `cross_file` do.
    scope: Option<Scope>,
    /// Also looks for `checks` on disk, as `file_exists` does for a literal
    /// path with `respect_gitignore: false`.
    past_walk: bool,
    /// Sees only git-tracked files, as with `git_tracked_only`.
    tracked_only: bool,
    /// Creates the file only in a directory that exists, as `file_create`
    /// with `create_parents: false` does.
    needs_directory: bool,
}

fn create_rule(
    id: &'static str,
    checks: &str,
    path: &str,
    content: &'static [u8],
    dir: bool,
) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(checks),
        path: PathBuf::from(path),
        content,
        dir,
        scope: Some(Scope::from_patterns(&[checks.to_string()]).unwrap()),
        past_walk: false,
        tracked_only: false,
        needs_directory: false,
    })
}

/// A `file_create` of `path` for a rule with no `paths:` scope that requires
/// `checks`.
fn scopeless_create_rule(id: &'static str, checks: &str, path: &str) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(checks),
        path: PathBuf::from(path),
        content: b"x",
        dir: false,
        scope: None,
        past_walk: false,
        tracked_only: false,
        needs_directory: false,
    })
}

/// A `file_create` of `path` for a rule that looks past the walk for it, or
/// sees only tracked files.
fn create_rule_seeing(
    id: &'static str,
    path: &str,
    past_walk: bool,
    tracked_only: bool,
) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(path),
        path: PathBuf::from(path),
        content: b"x",
        dir: false,
        scope: Some(Scope::from_patterns(&[path.to_string()]).unwrap()),
        past_walk,
        tracked_only,
        needs_directory: false,
    })
}

/// A `file_create` of `path` with `create_parents: false`.
fn create_rule_into_existing_directory(id: &'static str, path: &str) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(path),
        path: PathBuf::from(path),
        content: b"x",
        dir: false,
        scope: Some(Scope::from_patterns(&[path.to_string()]).unwrap()),
        past_walk: false,
        tracked_only: false,
        needs_directory: true,
    })
}

impl crate::rule::Fixer for CreateRule {
    fn describe(&self) -> String {
        format!("create {}", self.path.display())
    }
    fn apply(&self, _v: &Violation, ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        let target = ctx.root.join(
            crate::pathsafe::normalize_confined(&self.path).unwrap_or_else(|| self.path.clone()),
        );
        if target.exists() {
            return Ok(FixOutcome::Skipped("already there".to_string()));
        }
        let previewing = ctx.dry_run || ctx.stage_ops.is_some();
        if self.needs_directory && !previewing && !target.parent().unwrap().is_dir() {
            return Ok(FixOutcome::Skipped("no directory".to_string()));
        }
        if let Some(sink) = ctx.stage_ops {
            if !self.dir {
                sink.borrow_mut().push(FixEdit::CreateFile {
                    path: self.path.clone(),
                    content: self.content.to_vec(),
                });
            }
        } else if !ctx.dry_run {
            if self.dir {
                std::fs::create_dir_all(&target).unwrap();
            } else {
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(&target, self.content).unwrap();
            }
        }
        Ok(FixOutcome::Applied(format!(
            "create {}",
            self.path.display()
        )))
    }
    // Like `file_create`, no editor form for a target outside the root.
    fn fix_edit(&self, _v: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        crate::pathsafe::normalize_confined(&self.path)?;
        (!self.dir).then(|| FixEdit::CreateFile {
            path: self.path.clone(),
            content: self.content.to_vec(),
        })
    }
    fn created_dir(&self) -> Option<&Path> {
        self.dir.then_some(self.path.as_path())
    }
    fn created_file(
        &self,
        _v: &Violation,
        _root: &Path,
        _allow_out_of_root: bool,
    ) -> Option<(PathBuf, Vec<u8>)> {
        (!self.dir).then(|| (self.path.clone(), self.content.to_vec()))
    }
    fn needs_existing_directory(&self) -> bool {
        self.needs_directory
    }
}

impl Rule for CreateRule {
    fn id(&self) -> &str {
        self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        self.scope.as_ref()
    }
    fn sees_path_past_walk(&self, path: &Path) -> bool {
        self.past_walk && path == self.checks
    }
    fn git_tracked_mode(&self) -> crate::GitTrackedMode {
        if self.tracked_only {
            crate::GitTrackedMode::FileOnly
        } else {
            crate::GitTrackedMode::Off
        }
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let found = ctx.index.contains_path(&self.checks)
            || (self.past_walk && ctx.root.join(&self.checks).is_file());
        Ok(if found {
            Vec::new()
        } else {
            vec![Violation::new("missing")]
        })
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(self)
    }
}

#[test]
fn a_preview_declines_a_create_into_a_missing_directory_only_when_nothing_makes_it() {
    // `fix` creates the file once another create has made its directory, in
    // the same pass or the next, so the dry run and `--diff` report it then,
    // whatever the rules' order; with no such create, every mode declines it.
    type Rules = fn() -> Vec<Box<dyn Rule>>;
    let cases: [(&str, Rules, bool); 5] = [
        (
            "alone",
            || vec![create_rule_into_existing_directory("needs", "docs/i.md")],
            false,
        ),
        (
            "directory first",
            || {
                vec![
                    create_rule("dir", "docs", "docs", b"", true),
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                ]
            },
            true,
        ),
        (
            "directory after",
            || {
                vec![
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                    create_rule("dir", "docs", "docs", b"", true),
                ]
            },
            true,
        ),
        (
            "parents of another file",
            || {
                vec![
                    create_rule("other", "docs/sub/o.md", "docs/sub/o.md", b"x", false),
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                ]
            },
            true,
        ),
        (
            "another directory",
            || {
                vec![
                    create_rule("dir", "other", "other", b"", true),
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                ]
            },
            false,
        ),
    ];
    let opts = crate::WalkOptions::default();
    for (case, rules, expected) in cases {
        let applied = |report: &FixReport| {
            let result = report
                .results
                .iter()
                .find(|r| &*r.rule_id == "needs")
                .unwrap();
            matches!(result.items[0].status, FixStatus::Applied(_))
        };
        for dry_run in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let index = crate::walk(tmp.path(), &opts).unwrap();
            let report = Engine::new(rules(), RuleRegistry::new())
                .fix(tmp.path(), &index, &opts, dry_run, Applicability::Safe)
                .unwrap();
            assert_eq!(
                applied(&report),
                expected,
                "{case}, dry_run={dry_run}: {report:?}"
            );
            assert_eq!(
                tmp.path().join("docs/i.md").is_file(),
                expected && !dry_run,
                "{case}"
            );
        }
        let tmp = tempfile::tempdir().unwrap();
        let index = crate::walk(tmp.path(), &opts).unwrap();
        let (report, staged) = Engine::new(rules(), RuleRegistry::new())
            .stage_fixes(tmp.path(), &index, Applicability::Safe)
            .unwrap();
        assert_eq!(applied(&report), expected, "{case}, --diff");
        assert_eq!(
            staged.iter().any(|fix| fix.path == Path::new("docs/i.md")),
            expected,
            "{case}, --diff: {staged:?}"
        );
    }
}

#[test]
fn dry_run_predicts_each_create_the_real_fix_finds_unresolved() {
    // Each rule's dry-run outcome (applied or not) is the real fix's.
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root.join("cache")).unwrap();
        std::fs::write(root.join(".gitignore"), "build/\nout/\n").unwrap();
        let rules = vec![
            // A created ignore file that leaves itself out.
            create_rule(
                "self-ignoring",
                "cache/.gitignore",
                "cache/.gitignore",
                b"*\n",
                false,
            ),
            // One that does not, and leaves out another create of the pass.
            create_rule("logs-ignore", ".ignore", ".ignore", b"*.log\n", false),
            create_rule("log", "x.log", "x.log", b"x", false),
            // A directory the root `.gitignore` leaves out.
            create_rule("build-dir", "build", "build", b"", true),
            // Written to `README.md`, though `out/` is ignored.
            create_rule("normalized", "README.md", "out/../README.md", b"x", false),
            create_rule("outside-paths", "b.txt", "a.txt", b"x", false),
            // Written outside the root, as `allow_out_of_root` permits.
            create_rule("outside-root", "escaped.txt", "../escaped.txt", b"x", false),
            create_rule("plain", "keep.txt", "keep.txt", b"x", false),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                assert_eq!(result.items.len(), 1, "{result:?}");
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected: Vec<(String, bool)> = [
        ("self-ignoring", false),
        ("logs-ignore", true),
        ("log", false),
        ("build-dir", false),
        ("normalized", true),
        ("outside-paths", false),
        ("outside-root", false),
        ("plain", true),
    ]
    .into_iter()
    .map(|(id, applied)| (id.to_string(), applied))
    .collect();
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

#[test]
fn dry_run_takes_the_first_of_two_creates_of_one_ignore_file() {
    // The real `fix` skips a create whose file is already there, so the first
    // `.gitignore` written is the one the walk reads and `x.txt` stays walked.
    let outcome = |dry_run: bool| -> bool {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root).unwrap();
        let rules = vec![
            create_rule("first", ".gitignore", ".gitignore", b"other\n", false),
            create_rule("second", ".gitignore", ".gitignore", b"x.txt\n", false),
            create_rule("x", "x.txt", "x.txt", b"x", false),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        let x = report
            .results
            .iter()
            .find(|result| &*result.rule_id == "x")
            .unwrap();
        matches!(x.items[0].status, FixStatus::Applied(_))
    };
    assert!(outcome(false), "real fix");
    assert!(outcome(true), "dry run");
}

#[test]
fn dry_run_reads_an_absolute_create_inside_the_root_as_relative() {
    // `allow_out_of_root` lets a fixer take an absolute path, which may still
    // name a file in the tree; the walk indexes it there.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let absolute = root.join("keep.txt").to_string_lossy().into_owned();
    let rules = vec![create_rule("absolute", "keep.txt", &absolute, b"x", false)];
    let opts = crate::WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = crate::walk(&root, &opts).unwrap();
    let report = Engine::new(rules, RuleRegistry::new())
        .fix(&root, &index, &opts, true, Applicability::Safe)
        .unwrap();
    assert!(
        matches!(report.results[0].items[0].status, FixStatus::Applied(_)),
        "{:?}",
        report.results[0].items[0].status
    );
}

#[test]
fn dry_run_trusts_a_rule_that_looks_past_the_walk() {
    // Both files are gitignored. The rule that also looks on disk finds the
    // created one anyway; the one that reads only the walk does not.
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), "seen.txt\nunseen.txt\n").unwrap();
        let rules = vec![
            create_rule_seeing("past-walk", "seen.txt", true, false),
            create_rule_seeing("walk-only", "unseen.txt", false, false),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected = vec![
        ("past-walk".to_string(), true),
        ("walk-only".to_string(), false),
    ];
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

#[test]
fn dry_run_resolves_a_tracked_only_rule_only_with_a_tracked_path() {
    // git still tracks `gone.txt`, deleted from the worktree; `new.txt` was
    // never added. A rule that sees only tracked files is resolved by a create
    // of the first, not of the second; a rule that sees the walk, by either.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(root.join("gone.txt"), "x").unwrap();
    git(&["add", "gone.txt"]);
    git(&["commit", "-qm", "base"]);
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    let rules = vec![
        create_rule_seeing("tracked-gone", "gone.txt", false, true),
        create_rule_seeing("tracked-new", "new.txt", false, true),
        create_rule_seeing("walked-new", "new.txt", false, false),
    ];
    let opts = crate::WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = crate::walk(root, &opts).unwrap();
    let report = Engine::new(rules, RuleRegistry::new())
        .fix(root, &index, &opts, true, Applicability::Safe)
        .unwrap();
    let applied: Vec<(String, bool)> = report
        .results
        .iter()
        .map(|result| {
            let applied = matches!(result.items[0].status, FixStatus::Applied(_));
            (result.rule_id.to_string(), applied)
        })
        .collect();
    assert_eq!(
        applied,
        [
            ("tracked-gone".to_string(), true),
            ("tracked-new".to_string(), false),
            ("walked-new".to_string(), true),
        ]
    );
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn dry_run_judges_a_create_by_where_it_lands() {
    // A create through a symlink, or by an absolute or `..` path, lands where
    // the filesystem resolves it; the walk lists it there, and under the path
    // as written when the walk enters that name.
    use std::os::unix::fs::symlink;
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = &base.join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("real/deep")).unwrap();
        std::fs::create_dir_all(root.join("hidden")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(root.join(".gitignore"), "alias\nhidden\nveiled\n").unwrap();
        // A loop the walk does not enter, but which leads back into the root.
        symlink("..", root.join("sub/up")).unwrap();
        // Ignored names for directories the walk does or does not list.
        symlink("real", root.join("alias")).unwrap();
        symlink("hidden", root.join("veiled")).unwrap();
        // A name for the root from outside it, and a way out of it.
        symlink("root", base.join("root-link")).unwrap();
        symlink("../outside", root.join("out")).unwrap();
        // A `..` after it steps up from `real/deep`, not back to the root.
        symlink("real/deep", root.join("deeplink")).unwrap();
        // Leads nowhere until a create makes `made`.
        symlink("made", root.join("pointer")).unwrap();
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let rules = vec![
            create_rule("loop", "x.txt", "sub/up/x.txt", b"x", false),
            create_rule("ignored-alias", "real/y.txt", "alias/y.txt", b"x", false),
            create_rule("dotted-alias", "real/z.txt", "./alias/z.txt", b"x", false),
            create_rule("ignored-both", "hidden/n.txt", "veiled/n.txt", b"x", false),
            create_rule(
                "absolute-via-link",
                "w.txt",
                &path(&base.join("root-link/w.txt")),
                b"x",
                false,
            ),
            create_rule(
                "absolute-dotdot",
                "v.txt",
                &path(&base.join("root/../root/v.txt")),
                b"x",
                false,
            ),
            create_rule("relative-dotdot", "u.txt", "../root/u.txt", b"x", false),
            create_rule("lands-outside", "out/o.txt", "out/o.txt", b"x", false),
            // Written as given, so the OS resolves `deeplink/..` to `real`.
            create_rule(
                "absolute-link-up",
                "t.txt",
                &path(&root.join("deeplink/../t.txt")),
                b"x",
                false,
            ),
            // Out and back in through a directory the create makes, then a
            // symlink: `new/..` cancels and `alias` still leads to `real`.
            create_rule(
                "back-in-alias",
                "real/s.txt",
                "../root/new/../alias/s.txt",
                b"x",
                false,
            ),
            create_rule(
                "back-in-link-up",
                "r.txt",
                "../root/new/../deeplink/../r.txt",
                b"x",
                false,
            ),
            // Written as given, and the `..` cancels a real directory, so the
            // walk lists the file under the symlink that follows.
            create_rule(
                "absolute-back-in-link",
                "deeplink/q.txt",
                &path(&root.join("sub/../deeplink/q.txt")),
                b"x",
                false,
            ),
            // The create makes `made`, steps back out, and `pointer` now leads
            // into it.
            create_rule(
                "back-in-dangling",
                "made/p.txt",
                "../root/made/../pointer/p.txt",
                b"x",
                false,
            ),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                assert_eq!(result.items.len(), 1, "{result:?}");
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected: Vec<(String, bool)> = [
        ("loop", true),
        ("ignored-alias", true),
        ("dotted-alias", true),
        ("ignored-both", false),
        ("absolute-via-link", true),
        ("absolute-dotdot", true),
        ("relative-dotdot", true),
        ("lands-outside", false),
        ("absolute-link-up", false),
        ("back-in-alias", true),
        ("back-in-link-up", false),
        ("absolute-back-in-link", true),
        ("back-in-dangling", true),
    ]
    .into_iter()
    .map(|(id, applied)| (id.to_string(), applied))
    .collect();
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

#[cfg(unix)]
#[test]
fn dry_run_names_an_absolute_create_through_the_canonical_root() {
    // The root given through a symlink, and an absolute target written from
    // its canonical form through an in-tree symlink the walk follows: the walk
    // lists the file under the symlink's name, which only the canonical form
    // of the written path gives.
    use std::os::unix::fs::symlink;
    let outcome = |dry_run: bool| -> bool {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join("root/real")).unwrap();
        symlink("real", base.join("root/alias")).unwrap();
        symlink("root", base.join("root-link")).unwrap();
        let root = &base.join("root-link");
        let target = base.join("root/alias/x.txt").to_string_lossy().into_owned();
        let rules = vec![create_rule(
            "canonical",
            "alias/x.txt",
            &target,
            b"x",
            false,
        )];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        matches!(report.results[0].items[0].status, FixStatus::Applied(_))
    };
    assert!(outcome(false), "real fix");
    assert!(outcome(true), "dry run");
}

#[test]
#[cfg(unix)]
fn a_create_has_one_name_when_it_lands_where_it_is_written() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("real")).unwrap();
    symlink("real", root.join("alias")).unwrap();
    let names = |path: &str| Engine::created_names(root, Path::new(path));
    assert_eq!(names("real/x.txt"), Some(vec![PathBuf::from("real/x.txt")]));
    assert_eq!(
        names("./real/x.txt"),
        Some(vec![PathBuf::from("real/x.txt")])
    );
    assert_eq!(
        names("alias/x.txt"),
        Some(vec![
            PathBuf::from("alias/x.txt"),
            PathBuf::from("real/x.txt")
        ])
    );
    assert_eq!(names("../outside.txt"), None);
    // An absolute path keeps the name written after its last `..`, even
    // after two in a row.
    std::fs::create_dir_all(root.join("sub/inner")).unwrap();
    let absolute = root.join("sub/inner/../../alias/x.txt");
    assert_eq!(
        Engine::created_names(root, &absolute),
        Some(vec![
            PathBuf::from("alias/x.txt"),
            PathBuf::from("real/x.txt")
        ])
    );
}

#[test]
#[cfg(unix)]
fn a_directory_with_a_dot_dot_is_where_the_os_resolves_it() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for dir in ["sub", "q/r", "z/w", "x"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    // `x/x/..` is `q`, through the link; `q/x` is a different link, so
    // only the place the OS resolves the head to gives `q/d`.
    symlink(root.join("q/r"), root.join("x/x")).unwrap();
    symlink(root.join("z/w"), root.join("q/x")).unwrap();
    assert_eq!(
        Engine::directory_location(&root.join("sub/../d")),
        Some(root.join("d"))
    );
    assert_eq!(
        Engine::directory_location(&root.join("x/x/../d")),
        Some(root.join("q/d"))
    );
    assert_eq!(
        Engine::directory_location(&root.join("missing/../d")),
        None,
        "the write needs the directory before the `..`"
    );
}

#[test]
fn engine_fix_warnings_show_a_hidden_file_as_redacted() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("dir")).unwrap();
    std::fs::write(tmp.path().join("dir/secret.txt"), "x").unwrap();
    std::fs::write(tmp.path().join("plain.txt"), "x").unwrap();
    let hidden: HashSet<PathBuf> = std::iter::once(Engine::normalized_write_target(
        tmp.path(),
        Path::new("dir/secret.txt"),
    ))
    .collect();
    // The same file under another spelling is still hidden.
    for spelling in [
        "dir/secret.txt",
        "./dir/secret.txt",
        "dir/../dir/secret.txt",
    ] {
        assert_eq!(
            Engine::shown_path(tmp.path(), &hidden, Path::new(spelling)),
            crate::CONFIGURATION_MASK,
            "{spelling}"
        );
    }
    assert_eq!(
        Engine::shown_path(tmp.path(), &hidden, Path::new("plain.txt")),
        "plain.txt"
    );
}

#[test]
fn dry_run_judges_a_rule_without_a_paths_scope_by_the_path_as_written() {
    // A rule with no `paths:` finds nothing under another name: a create
    // written out of the root and back in lands on `back.txt` but never
    // matches `../root/back.txt`, while `out/../keep.txt` reads as `keep.txt`.
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root).unwrap();
        let rules = vec![
            scopeless_create_rule("back-in", "../root/back.txt", "../root/back.txt"),
            scopeless_create_rule("normalized", "keep.txt", "out/../keep.txt"),
        ];
        let opts = crate::WalkOptions::default();
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected = vec![
        ("back-in".to_string(), false),
        ("normalized".to_string(), true),
    ];
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

/// Creates `../outside.txt` only when its rule's `allow_out_of_root` lets it
/// leave the root, with no editor form, as `file_create` does there.
#[derive(Debug)]
struct OutOfRootCreate;

impl crate::rule::Fixer for OutOfRootCreate {
    fn describe(&self) -> String {
        "create ../outside.txt".to_string()
    }
    fn apply(&self, _v: &Violation, _ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        Ok(FixOutcome::Applied("create ../outside.txt".to_string()))
    }
    fn created_file(
        &self,
        _v: &Violation,
        _root: &Path,
        allow_out_of_root: bool,
    ) -> Option<(PathBuf, Vec<u8>)> {
        allow_out_of_root.then(|| (PathBuf::from("../outside.txt"), b"x".to_vec()))
    }
}

impl Rule for OutOfRootCreate {
    fn id(&self) -> &'static str {
        "escape"
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        Ok(vec![Violation::new("missing")])
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(self)
    }
}

#[test]
fn diff_hides_a_sensitive_create_outside_the_root() {
    // `--diff` hides every file a sensitive item writes. A create out of the
    // root has no editor form, so the target comes from `created_file` under
    // the rule's own `allow_out_of_root`.
    let tmp = tempfile::tempdir().unwrap();
    let root = &tmp.path().join("root");
    std::fs::create_dir_all(root).unwrap();
    let report = |sensitive: bool| FixReport {
        results: vec![FixRuleResult {
            rule_id: "escape".into(),
            level: Level::Error,
            items: vec![FixItem {
                violation: if sensitive {
                    Violation::new("missing").with_redacted_message("escape")
                } else {
                    Violation::new("missing")
                },
                status: FixStatus::Applied("create ../outside.txt".to_string()),
            }],
            analysis_failures: Vec::new(),
        }],
        non_convergent: false,
    };
    let engine = |allow: bool| {
        Engine::from_entries(
            vec![RuleEntry::new(Box::new(OutOfRootCreate)).with_allow_out_of_root(allow)],
            RuleRegistry::new(),
        )
    };
    let outside = Engine::normalized_write_target(root, Path::new("../outside.txt"));
    assert_eq!(
        engine(true).sensitive_fix_targets(&report(true), root),
        std::iter::once(outside).collect::<HashSet<_>>()
    );
    assert!(
        engine(true)
            .sensitive_fix_targets(&report(false), root)
            .is_empty()
    );
    assert!(
        engine(false)
            .sensitive_fix_targets(&report(true), root)
            .is_empty()
    );
}

static PATH_SELECTION_SELECTORS: &[crate::SelectorMeta] = &[crate::SelectorMeta {
    name: std::borrow::Cow::Borrowed("targets.files"),
    selector_type: crate::SelectorType::Glob,
    rerootable: false,
}];

fn path_selection_meta(
    selectors: &'static [crate::SelectorMeta],
    nested_rules: bool,
) -> crate::KindMeta {
    crate::KindMeta {
        dispatch: crate::DispatchClass::FullIndex { path_scope: false },
        paths: crate::PathsSupport::Optional,
        scope_filter: crate::ScopeFilterSupport::Full,
        git_tracked_only: crate::GitTrackedSupport::None,
        respect_gitignore: false,
        root_only: false,
        selectors: std::borrow::Cow::Borrowed(selectors),
        since: crate::SinceSupport::None,
        capabilities: std::borrow::Cow::Borrowed(&[]),
        content: crate::ContentAccess::PathOnly,
        formats: std::borrow::Cow::Borrowed(&[]),
        dependencies: std::borrow::Cow::Borrowed(&[]),
        fix_ops: std::borrow::Cow::Borrowed(&[]),
        expect_matches: false,
        nested_rules,
        trace: crate::TraceMeta::SELECTION,
    }
}

#[test]
fn a_path_selection_read_from_the_environment_is_sensitive() {
    let mut registry = RuleRegistry::new();
    for (kind, selectors, nested) in [
        ("selecting", PATH_SELECTION_SELECTORS, false),
        ("nesting", &[][..], true),
        ("flat", &[][..], false),
    ] {
        registry.register_owned(
            kind,
            std::sync::Arc::new(path_selection_meta(selectors, nested)),
            |_| unreachable!("never built"),
        );
    }
    let entry = |kind: &str, field: Option<&str>| {
        let mut spec: crate::RuleSpec =
            yaml_serde::from_str(&format!("id: r\nkind: {kind}\nlevel: error\n")).unwrap();
        spec.set_sensitive_fields(field.map(str::to_string).into_iter().collect());
        RuleEntry::new(stub("r", "**/*.txt")).with_spec(std::sync::Arc::new(spec))
    };
    let cases = [
        (entry("flat", None), false),
        (entry("flat", Some("paths")), true),
        (entry("flat", Some("scope_filter")), true),
        (entry("flat", Some("message")), false),
        // `require:` and `when_iter:` choose the paths of a nested kind only.
        (entry("nesting", Some("require")), true),
        (entry("flat", Some("require")), false),
        (entry("nesting", Some("when_iter")), true),
        (entry("flat", Some("when_iter")), false),
        // The environment marks a selector's top-level field.
        (entry("selecting", Some("targets")), true),
        (entry("flat", Some("targets")), false),
        (RuleEntry::new(stub("r", "**/*.txt")), false),
    ];
    let expected: Vec<bool> = cases.iter().map(|(_, sensitive)| *sensitive).collect();
    let engine = Engine::from_entries(cases.into_iter().map(|(e, _)| e).collect(), registry);
    let actual: Vec<bool> = engine
        .entries
        .iter()
        .map(|entry| engine.has_sensitive_path_selection(entry))
        .collect();
    assert_eq!(actual, expected);
}
