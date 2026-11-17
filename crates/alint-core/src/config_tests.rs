use super::*;
use crate::template::PathTokens;
use std::path::Path;

#[test]
fn allow_out_of_root_policy_resolves() {
    assert!(!AllowOutOfRoot::Confined.allows("r", "k"));
    assert!(AllowOutOfRoot::All.allows("r", "k"));
    let sel = AllowOutOfRoot::Selective {
        kinds: ["json_schema_passes".to_string()].into_iter().collect(),
        rules: ["my-rule".to_string()].into_iter().collect(),
    };
    assert!(sel.allows("anything", "json_schema_passes"), "by kind");
    assert!(sel.allows("my-rule", "other_kind"), "by id");
    assert!(!sel.allows("nope", "other_kind"), "neither id nor kind");
    assert!(AllowOutOfRoot::Confined.is_confined());
    assert!(!AllowOutOfRoot::All.is_confined());
}

/// The grant covers the user's own rules only: never a rule an extended
/// ruleset or a nested config shaped, by kind, by id, or under `true`.
#[test]
fn allow_out_of_root_never_covers_an_extended_rule() {
    let mut spec: RuleSpec =
        crate::yaml::from_str("id: my-rule\nkind: pair_hash\nlevel: error\n").unwrap();
    let grants = [
        AllowOutOfRoot::All,
        AllowOutOfRoot::Selective {
            kinds: ["pair_hash".to_string()].into_iter().collect(),
            rules: std::collections::HashSet::new(),
        },
        AllowOutOfRoot::Selective {
            kinds: std::collections::HashSet::new(),
            rules: ["my-rule".to_string()].into_iter().collect(),
        },
    ];
    for grant in &grants {
        assert!(grant.allows_rule(&spec), "{grant:?}: the user's own rule");
    }
    spec.set_extended_source(true);
    for grant in &grants {
        assert!(!grant.allows_rule(&spec), "{grant:?}: an extended rule");
    }
    assert!(!AllowOutOfRoot::Confined.allows_rule(&spec));
}

#[test]
fn allow_out_of_root_deserializes_bool_and_map() {
    let t: AllowOutOfRoot = crate::yaml::from_str("true").unwrap();
    assert_eq!(t, AllowOutOfRoot::All);
    let f: AllowOutOfRoot = crate::yaml::from_str("false").unwrap();
    assert_eq!(f, AllowOutOfRoot::Confined);
    let m: AllowOutOfRoot = crate::yaml::from_str("kinds: [pair_hash]\nrules: [x]").unwrap();
    match m {
        AllowOutOfRoot::Selective { kinds, rules } => {
            assert!(kinds.contains("pair_hash"));
            assert!(rules.contains("x"));
        }
        other => panic!("expected Selective, got {other:?}"),
    }
    // an unknown key in the map form is rejected with a message that names
    // the accepted forms, not the internal untagged-enum type.
    let err = crate::yaml::from_str::<AllowOutOfRoot>("bogus: 1").unwrap_err();
    assert!(
        err.to_string().contains("a boolean, or a map"),
        "expected a friendly message, got: {err}"
    );
    assert!(
        !err.to_string().contains("Raw"),
        "message leaks the internal enum name: {err}"
    );
}

#[test]
fn untagged_config_enums_name_accepted_forms_not_internal_type() {
    // A value matching no variant reports the accepted forms (via
    // `#[serde(expecting)]`), never the internal untagged-enum type name.
    let p = crate::yaml::from_str::<PathsSpec>("42")
        .unwrap_err()
        .to_string();
    assert!(
        p.contains("a glob string") && !p.contains("PathsSpec"),
        "PathsSpec: {p}"
    );
    let e = crate::yaml::from_str::<ExtendsEntry>("42")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("a URL or path") && !e.contains("ExtendsEntry"),
        "ExtendsEntry: {e}"
    );
    let f = crate::yaml::from_str::<FixSpec>("42")
        .unwrap_err()
        .to_string();
    assert!(
        f.contains("exactly one fix op") && !f.contains("FixSpec"),
        "FixSpec: {f}"
    );
}

#[test]
fn config_default_respects_gitignore_and_caps_fix_size() {
    // Round-trip the documented defaults through serde to
    // catch silent default drift.
    let cfg: Config = crate::yaml::from_str("version: 1\n").expect("minimal config");
    assert_eq!(cfg.version, 1);
    assert!(cfg.respect_gitignore);
    assert_eq!(cfg.fix_size_limit, Some(1 << 20));
    assert!(!cfg.nested_configs);
    assert!(cfg.extends.is_empty());
    assert!(cfg.rules.is_empty());
}

#[test]
fn config_rejects_unknown_top_level_field() {
    let err = crate::yaml::from_str::<Config>("version: 1\nignored_typo: true\n");
    assert!(err.is_err(), "deny_unknown_fields should reject typos");
}

#[test]
fn config_explicit_null_disables_fix_size_limit() {
    let cfg: Config = crate::yaml::from_str("version: 1\nfix_size_limit: null\n").unwrap();
    assert_eq!(cfg.fix_size_limit, None);
}

#[test]
fn extends_entry_url_form_has_no_filters() {
    let e = ExtendsEntry::Url("alint://bundled/oss-baseline@v1".into());
    assert_eq!(e.url(), "alint://bundled/oss-baseline@v1");
    assert!(e.only().is_none());
    assert!(e.except().is_none());
}

#[test]
fn extends_entry_filtered_form_exposes_only_and_except() {
    let e = ExtendsEntry::Filtered {
        url: "alint://bundled/rust@v1".into(),
        only: Some(vec!["rust-edition".into()]),
        except: None,
    };
    assert_eq!(e.url(), "alint://bundled/rust@v1");
    assert_eq!(e.only(), Some(&["rust-edition".to_string()][..]));
    assert!(e.except().is_none());
}

#[test]
fn extends_entry_filtered_form_supports_except_only() {
    let e = ExtendsEntry::Filtered {
        url: "./team.yml".into(),
        only: None,
        except: Some(vec!["legacy-rule".into()]),
    };
    assert_eq!(e.except(), Some(&["legacy-rule".to_string()][..]));
    assert!(e.only().is_none());
}

#[test]
fn paths_spec_accepts_three_shapes() {
    let single: PathsSpec = crate::yaml::from_str("\"src/**\"").unwrap();
    assert!(matches!(single, PathsSpec::Single(s) if s == "src/**"));

    let many: PathsSpec = crate::yaml::from_str("[\"src/**\", \"!src/vendor/**\"]").unwrap();
    assert!(matches!(many, PathsSpec::Many(v) if v.len() == 2));

    let inc_exc: PathsSpec =
        crate::yaml::from_str("include: src/**\nexclude: src/vendor/**\n").unwrap();
    match inc_exc {
        PathsSpec::IncludeExclude { include, exclude } => {
            assert_eq!(include, vec!["src/**"]);
            assert_eq!(exclude, vec!["src/vendor/**"]);
        }
        _ => panic!("expected include/exclude shape"),
    }
}

#[test]
fn paths_spec_include_accepts_string_or_vec() {
    let from_string: PathsSpec =
        crate::yaml::from_str("include: a\nexclude:\n  - b\n  - c\n").unwrap();
    let PathsSpec::IncludeExclude { include, exclude } = from_string else {
        panic!("expected include/exclude shape");
    };
    assert_eq!(include, vec!["a"]);
    assert_eq!(exclude, vec!["b", "c"]);
}

#[test]
fn paths_spec_include_exclude_classifies_negations() {
    let inc = |s: &str| vec![s.to_string()];

    // A leading `!` marks an exclusion (matching `Scope::from_patterns`),
    // not an include — `explain` must not mislabel it.
    let single = PathsSpec::Single("src/**".to_string());
    assert_eq!(single.include_exclude(), (inc("src/**"), Vec::new()));

    let many = PathsSpec::Many(vec!["src/**".to_string(), "!src/vendor/**".to_string()]);
    assert_eq!(
        many.include_exclude(),
        (inc("src/**"), inc("src/vendor/**"))
    );
    assert_eq!(many.render_scope(), "src/**  (excluding src/vendor/**)");

    // Explicit include/exclude, plus a `!` inside the include list: both
    // land in the exclude bucket (mirroring `from_paths_spec`).
    let inc_exc = PathsSpec::IncludeExclude {
        include: vec!["a/**".to_string(), "!a/gen/**".to_string()],
        exclude: vec!["a/test/**".to_string()],
    };
    assert_eq!(
        inc_exc.include_exclude(),
        (
            inc("a/**"),
            vec!["a/gen/**".to_string(), "a/test/**".to_string()],
        )
    );

    // An exclusion-only Single renders as match-all minus the exclude.
    let excl_only = PathsSpec::Single("!dist/**".to_string());
    assert_eq!(excl_only.include_exclude(), (Vec::new(), inc("dist/**")));
    assert_eq!(excl_only.render_scope(), "**  (excluding dist/**)");
}

#[test]
fn rule_spec_deserialize_options_picks_up_kind_specific_fields() {
    #[derive(Deserialize, Debug)]
    struct PatternOnly {
        pattern: String,
    }
    let spec: RuleSpec = crate::yaml::from_str(
        "id: r\nkind: file_content_matches\nlevel: error\npaths: src/**\npattern: TODO\n",
    )
    .unwrap();
    let opts: PatternOnly = spec.deserialize_options().unwrap();
    assert_eq!(opts.pattern, "TODO");
}

#[test]
fn fix_spec_op_name_covers_every_variant() {
    // Two guards below: (1) round-trip -- every `cases` YAML parses to a
    // FixSpec whose op_name() matches, so each listed op is real and
    // correctly named; (2) set-equality of `cases` names with ALL_OP_NAMES.
    //
    // The compile-time backstop for "every variant is covered" is op_name()
    // itself -- an EXHAUSTIVE match, so a new FixSpec variant is a hard
    // compile error there until an arm is added (right beside ALL_OP_NAMES
    // and its doc, which says to add the name in tandem). Known limitation:
    // both `cases` and ALL_OP_NAMES are hand-maintained, so a variant added
    // to op_name() but omitted from BOTH would slip past this test (and the
    // fix-coverage gate would then not require a scenario for it). A fully
    // type-derived list (e.g. `strum::EnumCount`) would close that, but is
    // not worth a core-crate proc-macro dependency for the current op set.
    let cases = [
        ("file_create:\n  content: x\n", "file_create"),
        ("file_remove: {}", "file_remove"),
        ("file_prepend:\n  content: x\n", "file_prepend"),
        ("file_append:\n  content: x\n", "file_append"),
        ("file_rename: {}", "file_rename"),
        (
            "file_trim_trailing_whitespace: {}",
            "file_trim_trailing_whitespace",
        ),
        ("file_append_final_newline: {}", "file_append_final_newline"),
        (
            "file_normalize_line_endings: {}",
            "file_normalize_line_endings",
        ),
        ("file_strip_bidi: {}", "file_strip_bidi"),
        ("file_strip_zero_width: {}", "file_strip_zero_width"),
        ("file_strip_bom: {}", "file_strip_bom"),
        ("file_collapse_blank_lines: {}", "file_collapse_blank_lines"),
        ("replace:\n  replacement: x\n", "replace"),
        ("set_value: {}", "set_value"),
        ("remove_value: {}", "remove_value"),
        ("chmod: {}", "chmod"),
        ("git_untrack: {}", "git_untrack"),
        ("command:\n  run: [\"x\"]\n", "command"),
        ("dir_create: {}", "dir_create"),
        ("sync_from: {}", "sync_from"),
        ("relocate: {}", "relocate"),
        ("create_and_register: {}", "create_and_register"),
        ("sort: {}", "sort"),
        ("indent_style: {}", "indent_style"),
        ("insert_line: {}", "insert_line"),
        ("insert_header:\n  content: x\n", "insert_header"),
    ];
    for (yaml, expected) in cases {
        let spec: FixSpec = crate::yaml::from_str(yaml).unwrap_or_else(|e| panic!("{yaml}: {e}"));
        assert_eq!(spec.op_name(), expected);
    }
    // The gate's SSOT must list exactly the variants op_name covers.
    let enumerated: std::collections::BTreeSet<&str> = cases.iter().map(|(_, n)| *n).collect();
    let ssot: std::collections::BTreeSet<&str> = FixSpec::ALL_OP_NAMES.iter().copied().collect();
    assert_eq!(
        enumerated, ssot,
        "FixSpec::ALL_OP_NAMES must match the op_name-covered variants"
    );
}

#[test]
fn fix_block_rejects_two_ops() {
    // R-TWOOP: an untagged FixSpec would silently keep the first key and
    // drop the second; the load-time guard must reject it instead.
    #[derive(Debug, Deserialize)]
    struct Holder {
        #[serde(default, deserialize_with = "deserialize_fix_spec")]
        fix: Option<FixSpec>,
    }
    let err = crate::yaml::from_str::<Holder>(
        "fix:\n  file_trim_trailing_whitespace: {}\n  file_append_final_newline: {}\n",
    )
    .expect_err("two op keys must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("exactly one op key"),
        "unexpected error: {msg}"
    );

    // One op still parses, and an absent block stays None.
    let one = crate::yaml::from_str::<Holder>("fix:\n  file_append_final_newline: {}\n")
        .expect("single op parses");
    assert_eq!(
        one.fix.as_ref().map(FixSpec::op_name),
        Some("file_append_final_newline")
    );
    let none = crate::yaml::from_str::<Holder>("version: 1\n").expect("absent parses");
    assert!(none.fix.is_none());
}

#[test]
fn every_fix_op_takes_applicability_only_as_a_tier_name() {
    // A one-entry mapping or a tag is how serde writes an enum variant; an
    // untagged `FixSpec` read the op's options from serde's buffered copy,
    // which took `{safe: ~}` for `safe`, past the checked decoder and the
    // trust gates that compare strings.
    #[derive(Debug, Deserialize)]
    struct Holder {
        #[serde(default, deserialize_with = "deserialize_fix_spec")]
        fix: Option<FixSpec>,
    }
    for op in FixSpec::ALL_OP_NAMES {
        for written in ["{safe: ~}", "{safe: null}", "!safe ~", "[safe]"] {
            let yaml = format!("fix:\n  {op}:\n    applicability: {written}\n");
            let error = crate::yaml::from_str::<Holder>(&yaml)
                .map(|holder| holder.fix.map(|fix| fix.op_name()))
                .expect_err(&yaml)
                .to_string();
            assert!(
                error.contains("applicability") || error.contains("YAML tag `!safe`"),
                "{yaml}: {error}"
            );
        }
        // The name itself still decodes, where the op has the field.
        let yaml = format!("fix:\n  {op}:\n    applicability: never\n");
        let error = crate::yaml::from_str::<Holder>(&yaml)
            .err()
            .map(|e| e.to_string());
        assert!(
            error.as_deref().is_none_or(|e| {
                !e.contains("applicability") || e.contains("unknown field `applicability`")
            }),
            "{yaml}: {error:?}"
        );
    }
}

#[test]
fn a_fix_op_error_names_the_op_and_the_field() {
    let error = crate::yaml::from_str::<FixSpec>("replace:\n  pattern: x\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("`replace`"), "{error}");
    assert!(error.contains("replacement"), "{error}");
    let error = crate::yaml::from_str::<FixSpec>("file_vanish: {}\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("`file_vanish` is not a fix op"), "{error}");
}

#[test]
fn resolve_content_source_inline_only() {
    let s = Some("hello".to_string());
    let resolved = resolve_content_source("r", "file_create", &s, &None).unwrap();
    assert!(matches!(resolved, ContentSourceSpec::Inline(b) if b == "hello"));
}

#[test]
fn resolve_content_source_file_only() {
    let p = Some(Path::new("LICENSE").into());
    let resolved = resolve_content_source("r", "file_create", &None, &p).unwrap();
    assert!(matches!(resolved, ContentSourceSpec::File(p) if p == Path::new("LICENSE")));
}

#[test]
fn resolve_content_source_rejects_both_set() {
    let err = resolve_content_source(
        "r",
        "file_prepend",
        &Some("x".into()),
        &Some(Path::new("y").into()),
    )
    .unwrap_err();
    assert!(err.to_string().contains("mutually exclusive"));
}

#[test]
fn resolve_content_source_rejects_neither_set() {
    let err = resolve_content_source("r", "file_append", &None, &None).unwrap_err();
    assert!(err.to_string().contains("required"));
}

#[test]
fn content_source_spec_from_string_variants() {
    let from_owned: ContentSourceSpec = String::from("hi").into();
    assert!(matches!(from_owned, ContentSourceSpec::Inline(s) if s == "hi"));
    let from_str: ContentSourceSpec = "hi".into();
    assert!(matches!(from_str, ContentSourceSpec::Inline(s) if s == "hi"));
}

#[test]
fn compiled_nested_spec_marks_every_field_of_a_sensitive_require() {
    let nested: NestedRuleSpec = crate::yaml::from_str(
        "kind: git_commit_no_fixup\nsince: main\nscope_filter:\n  changed_since: main\n",
    )
    .unwrap();
    let mut compiled = CompiledNestedSpec::compile(nested, "each", 0).unwrap();
    let tokens = PathTokens::from_path(Path::new("pkg/a"));
    let plain = compiled.instantiate("each", 0, Level::Error, &tokens);
    assert!(plain.sensitive_fields.is_empty());
    assert!(!plain.sensitive);

    compiled.sensitive_configuration = true;
    let spec = compiled.instantiate("each", 0, Level::Error, &tokens);
    for field in ["since", "scope_filter", "paths", "message", "when"] {
        assert!(spec.field_is_sensitive(field), "{field}");
    }
    assert!(!spec.field_is_sensitive("kind"));
    // The flag marks fields, not `sensitive: true` itself; the parent,
    // whose `require` is sensitive, redacts the findings it reports.
    assert!(!spec.sensitive);
}

/// `templates` reaches every string an iteration renders: `paths`,
/// `message`, and the options' strings at any depth, inside sequences,
/// mappings (keys included) and tagged values.
#[test]
fn nested_rule_spec_templates_reach_every_option_string() {
    let nested: NestedRuleSpec = crate::yaml::from_str(
        "kind: x\npaths: \"{dir}/a\"\nmessage: \"in {stem}\"\n\
         items: [\"{path}\", [\"{ext}\"]]\n\
         table: { \"{basename}\": { deep: \"{parent_name}\" } }\n",
    )
    .unwrap();
    // A tagged value cannot arrive through the flattened decode, but
    // `extra` is public, so a spec built in code can hold one.
    let mut nested = nested;
    let tagged: crate::yaml::Value = crate::yaml::from_str("!custom '{dir}/t'").unwrap();
    nested.extra.insert("tagged", tagged);
    let mut templates = nested.templates();
    templates.sort_unstable();
    for expected in [
        "{dir}/a",
        "in {stem}",
        "{path}",
        "{ext}",
        "{basename}",
        "{parent_name}",
        "{dir}/t",
    ] {
        assert!(templates.contains(&expected), "{expected}: {templates:?}");
    }
}

#[test]
fn nested_rule_spec_instantiate_synthesizes_id_and_inherits_level() {
    let nested: NestedRuleSpec = crate::yaml::from_str(
        "kind: file_exists\npaths: \"{path}/README.md\"\nmessage: missing in {path}\n\
         git_tracked_only: true\nroot_only: true\n",
    )
    .unwrap();
    let tokens = PathTokens::from_path(Path::new("packages/foo"));
    let spec = nested.instantiate("every-pkg-has-readme", 0, Level::Error, &tokens);

    assert_eq!(spec.id, "every-pkg-has-readme.require[0]");
    assert_eq!(spec.kind, "file_exists");
    assert_eq!(spec.level, Level::Error);
    // Path template should have been rendered for both
    // `paths:` and `message:` from the iterated tokens.
    match spec.paths {
        Some(PathsSpec::Single(p)) => assert_eq!(p, "packages/foo/README.md"),
        other => panic!("unexpected paths shape: {other:?}"),
    }
    assert_eq!(spec.message.as_deref(), Some("missing in packages/foo"));
    // Nested rules don't propagate git_tracked_only: it is a kind-specific
    // option (ADR-0008) stripped from the nested `extra` via PARENT_FIELDS,
    // so it never reaches the leaf rule's options.
    assert!(
        !spec
            .extra
            .keys()
            .any(|k| k.as_str() == Some("git_tracked_only")),
        "git_tracked_only should be stripped from nested extra"
    );
    // The kind's own options still reach it.
    assert_eq!(spec.extra["root_only"], true);
}
