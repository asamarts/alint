//! Phase 5 Batch 5b evaluation tests (ADR-0050 D8 / #592):
//! - `records:` array-of-objects extraction across JSON, YAML, and TOML with
//!   exact source spans and preserved numeric lexemes;
//! - parameterized string transforms (`split`, `strip_prefix`, `strip_suffix`,
//!   `prefix`, `suffix`) including negative indices, optional vs required
//!   out-of-range splits, and `BytesPerRun` charging;
//! - mixed-reader `union:` with per-part `fields:` and `keys_sensitive`.

#![allow(clippy::too_many_lines)]

use super::*;
use crate::relation::plan::{Capabilities, Records};

fn records_part(paths: &str, format: EntriesFormat, at: &[&str]) -> Part {
    Part {
        from: from(paths),
        reader: Reader::Records(Records {
            format,
            at: at.iter().map(|s| (*s).to_string()).collect(),
        }),
        fields: None,
    }
}

#[test]
fn records_extracts_json_yaml_and_toml_with_spans_and_number_lexemes() {
    let files = tree(&[
        (
            "a.json",
            "{\n  \"items\": [\n    {\"name\": \"api\", \"port\": 8080, \"ratio\": 1.00},\n    \
             {\"name\": \"web\", \"port\": 3000}\n  ]\n}\n",
        ),
        (
            "b.yaml",
            "spec:\n  containers:\n    - name: worker\n      image: ghcr.io/org/app:1.2.3\n      \
             replicas: 2.0\n    - name: sidecar\n      image: nginx\n",
        ),
        (
            "c.toml",
            "[[services]]\nname = \"db\"\nport = 5432\n\n[[services]]\nname = \"cache\"\n",
        ),
    ]);

    let json_rel = named(
        "json_rec",
        Rows::Part(records_part("a.json", EntriesFormat::Json, &["$.items[*]"])),
        &[
            (
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            ),
            (
                "ratio",
                FieldSource::Record {
                    query: "$.ratio".into(),
                    optional: true,
                },
            ),
        ],
    );
    let yaml_rel = named(
        "yaml_rec",
        Rows::Part(records_part(
            "b.yaml",
            EntriesFormat::Yaml,
            &["$.spec.containers[*]"],
        )),
        &[
            (
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            ),
            (
                "replicas",
                FieldSource::Record {
                    query: "$.replicas".into(),
                    optional: true,
                },
            ),
        ],
    );
    let toml_rel = named(
        "toml_rec",
        Rows::Part(records_part(
            "c.toml",
            EntriesFormat::Toml,
            &["$.services[*]"],
        )),
        &[
            (
                "name",
                FieldSource::Record {
                    query: "$.name".into(),
                    optional: false,
                },
            ),
            (
                "port",
                FieldSource::Record {
                    query: "$.port".into(),
                    optional: true,
                },
            ),
        ],
    );
    let plan = plan(vec![json_rel, yaml_rel, toml_rel]);

    // JSON: preserved lexeme `1.00` (not collapsed to `1`) + missing optional cell + spans.
    let jr = run(&plan, 0, &files, Limits::default()).unwrap();
    assert_eq!(jr.rows.len(), 2);
    assert_eq!(jr.rows[0].cells[0].value, Some(Value::String("api".into())));
    assert!(jr.rows[0].cells[0].span.is_some());
    assert_eq!(
        jr.rows[0].cells[1].value,
        Some(Value::Number(
            crate::relation::Number::from_lexeme("1.00").unwrap()
        ))
    );
    assert!(jr.rows[0].cells[1].span.is_some());
    assert_eq!(jr.rows[1].cells[0].value, Some(Value::String("web".into())));
    assert_eq!(jr.rows[1].cells[1].value, None);

    // YAML: preserved lexeme `2.0` + exact spans + missing optional cell.
    let yr = run(&plan, 1, &files, Limits::default()).unwrap();
    assert_eq!(yr.rows.len(), 2);
    assert_eq!(
        yr.rows[0].cells[0].value,
        Some(Value::String("worker".into()))
    );
    assert!(yr.rows[0].cells[0].span.is_some());
    assert_eq!(
        yr.rows[0].cells[1].value,
        Some(Value::Number(
            crate::relation::Number::from_lexeme("2.0").unwrap()
        ))
    );
    assert!(yr.rows[0].cells[1].span.is_some());
    assert_eq!(yr.rows[1].cells[1].value, None);

    // TOML: array-of-tables extraction + exact spans + missing optional cell.
    let tr = run(&plan, 2, &files, Limits::default()).unwrap();
    assert_eq!(tr.rows.len(), 2);
    assert_eq!(tr.rows[0].cells[0].value, Some(Value::String("db".into())));
    assert!(tr.rows[0].cells[0].span.is_some());
    assert_eq!(
        tr.rows[0].cells[1].value,
        Some(Value::Number(
            crate::relation::Number::from_lexeme("5432").unwrap()
        ))
    );
    assert!(tr.rows[0].cells[1].span.is_some());
    assert_eq!(tr.rows[1].cells[1].value, None);
}

#[test]
fn records_fails_loud_on_non_object_missing_required_multi_match_and_non_scalar() {
    let files = tree(&[
        ("non_obj.json", r#"{"items": ["not-an-object"]}"#),
        ("missing_field.json", r#"{"items": [{"name": "a"}]}"#),
        ("multi.json", r#"{"items": [{"tags": ["x", "y"]}]}"#),
        ("non_scalar.json", r#"{"items": [{"meta": {"a": 1}}]}"#),
        ("absent_at.json", r#"{"other": []}"#),
    ]);

    let non_obj = named(
        "non_obj",
        Rows::Part(records_part(
            "non_obj.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "name",
            FieldSource::Record {
                query: "$.name".into(),
                optional: false,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![non_obj]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("selects a string at `items[0]`"),
        "expected non-object failure: {errs:?}"
    );

    let missing_req = named(
        "missing_req",
        Rows::Part(records_part(
            "missing_field.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "image",
            FieldSource::Record {
                query: "$.image".into(),
                optional: false,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![missing_req]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("matches nothing"),
        "expected missing required record field failure: {errs:?}"
    );

    let multi = named(
        "multi",
        Rows::Part(records_part(
            "multi.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "tag",
            FieldSource::Record {
                query: "$.tags[*]".into(),
                optional: true,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![multi]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("matches more than once"),
        "expected multi-match failure: {errs:?}"
    );

    let non_scalar = named(
        "non_scalar",
        Rows::Part(records_part(
            "non_scalar.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "meta",
            FieldSource::Record {
                query: "$.meta".into(),
                optional: true,
            },
        )],
    );
    let errs = messages(&run(&plan(vec![non_scalar]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("a field holds a scalar"),
        "expected non-scalar failure: {errs:?}"
    );

    // Absent `at:` yields 0 rows cleanly.
    let absent = named(
        "absent",
        Rows::Part(records_part(
            "absent_at.json",
            EntriesFormat::Json,
            &["$.items[*]"],
        )),
        &[(
            "name",
            FieldSource::Record {
                query: "$.name".into(),
                optional: false,
            },
        )],
    );
    let rows = run(&plan(vec![absent]), 0, &files, Limits::default()).unwrap();
    assert_eq!(rows.rows.len(), 0);
}

#[test]
fn parameterized_transforms_handle_split_affixes_missing_required_and_bytes_budget() {
    let files = tree(&[(
        "imgs.txt",
        "ghcr.io/org/app:1.2.3\nnginx\n$FOO.plural\n$BAR.singular\n",
    )]);

    // `split` with negative index `-1`, `part: 1` (optional -> Missing on "nginx"),
    // `strip_prefix`, `strip_suffix` (list, first matching once), `prefix`, `suffix`.
    let mut rel = named(
        "imgs",
        Rows::Part(lines("imgs.txt")),
        &[
            ("last", FieldSource::Value),
            ("tag", FieldSource::Value),
            ("clean", FieldSource::Value),
        ],
    );
    rel.relation.transforms = vec![
        (
            "last".into(),
            vec![Transform::Split {
                on: ":".into(),
                part: -1,
                required: false,
            }],
        ),
        (
            "tag".into(),
            vec![Transform::Split {
                on: ":".into(),
                part: 1,
                required: false,
            }],
        ),
        (
            "clean".into(),
            vec![
                Transform::StripPrefix("$".into()),
                Transform::StripSuffix(vec![".singular".into(), ".plural".into()]),
                Transform::Prefix("p-".into()),
                Transform::Suffix("-s".into()),
            ],
        ),
    ];
    let rows = run(&plan(vec![rel]), 0, &files, Limits::default()).unwrap();
    let vals = |col: usize| -> Vec<Option<String>> {
        rows.rows
            .iter()
            .map(|r| match &r.cells[col].value {
                Some(Value::String(s)) => Some(s.to_string()),
                None => None,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        vals(0),
        vec![
            Some("1.2.3".into()),
            Some("nginx".into()),
            Some("$FOO.plural".into()),
            Some("$BAR.singular".into()),
        ]
    );
    assert_eq!(vals(1), vec![Some("1.2.3".into()), None, None, None]);
    assert_eq!(
        vals(2),
        vec![
            Some("p-ghcr.io/org/app:1.2.3-s".into()),
            Some("p-nginx-s".into()),
            Some("p-FOO-s".into()),
            Some("p-BAR-s".into()),
        ]
    );

    // `split` with `required: true` fails loud when out of range.
    let mut req = named(
        "req",
        Rows::Part(lines("imgs.txt")),
        &[("tag", FieldSource::Value)],
    );
    req.relation.transforms = vec![(
        "tag".into(),
        vec![Transform::Split {
            on: ":".into(),
            part: 1,
            required: true,
        }],
    )];
    let errs = messages(&run(&plan(vec![req]), 0, &files, Limits::default()));
    assert!(
        errs[0].contains("part 1 is out of range"),
        "expected required split failure: {errs:?}"
    );

    // `prefix` expanding strings charges `BytesPerRun`.
    let mut big = named(
        "big",
        Rows::Part(lines("imgs.txt")),
        &[("tag", FieldSource::Value)],
    );
    big.relation.transforms = vec![("tag".into(), vec![Transform::Prefix("x".repeat(4096))])];
    let errs = messages(&run(
        &plan(vec![big]),
        0,
        &files,
        limit(LimitKind::BytesPerRun, 2048),
    ));
    assert!(
        errs[0].contains("`bytes_per_run`"),
        "expected bytes budget failure: {errs:?}"
    );
}

#[test]
fn mixed_reader_union_per_part_fields_align_columns_and_track_keys_sensitive() {
    let files = tree(&[
        ("a.json", r#"{"commands": {"help": "Show help"}}"#),
        ("b.json", r#"{"aliases": ["h", "?"]}"#),
        ("c.yaml", "items:\n  - name: status\n"),
    ]);
    let field_def = |source: FieldSource, sensitive: bool| FieldDef { source, sensitive };
    let union_rel = NamedRelation {
        name: "names".into(),
        source: SourceIndex(0),
        relation: RelationDef {
            rows: Rows::Union(vec![
                Part {
                    from: from("a.json"),
                    reader: Reader::Entries(Entries {
                        format: EntriesFormat::Json,
                        at: vec!["$.commands".into()],
                        leaves: false,
                    }),
                    fields: Some(vec![
                        ("name".into(), field_def(FieldSource::EntryKey, false)),
                        (
                            "file".into(),
                            field_def(FieldSource::Path(PathToken::Path), false),
                        ),
                    ]),
                },
                Part {
                    from: from("b.json"),
                    reader: Reader::Read {
                        read: Read::Json("$.aliases[*]".into()),
                        optional: false,
                    },
                    fields: Some(vec![
                        ("name".into(), field_def(FieldSource::Value, false)),
                        (
                            "file".into(),
                            field_def(FieldSource::Path(PathToken::Path), false),
                        ),
                    ]),
                },
                Part {
                    from: from("c.yaml"),
                    reader: Reader::Records(Records {
                        format: EntriesFormat::Yaml,
                        at: vec!["$.items[*]".into()],
                    }),
                    fields: Some(vec![
                        (
                            "name".into(),
                            field_def(
                                FieldSource::Record {
                                    query: "$.name".into(),
                                    optional: false,
                                },
                                false,
                            ),
                        ),
                        (
                            "file".into(),
                            field_def(FieldSource::Path(PathToken::Path), false),
                        ),
                    ]),
                },
            ]),
            fields: vec![
                ("name".into(), field_def(FieldSource::EntryKey, false)),
                (
                    "file".into(),
                    field_def(FieldSource::Path(PathToken::Path), false),
                ),
            ],
            transforms: Vec::new(),
            filters: Vec::new(),
        },
        capabilities: Capabilities::default(),
        reads_environment: false,
        definition_reads_environment: false,
    };

    let plan_val = plan(vec![union_rel.clone()]);
    let rows = run(&plan_val, 0, &files, Limits::default()).unwrap();
    let pairs: Vec<(String, String)> = rows
        .rows
        .iter()
        .map(|r| (text(&r.cells[0]).to_string(), text(&r.cells[1]).to_string()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("help".into(), "a.json".into()),
            ("h".into(), "b.json".into()),
            ("?".into(), "b.json".into()),
            ("status".into(), "c.yaml".into()),
        ]
    );
    assert!(!super::super::keys_sensitive(&plan_val, RelationId(0)));

    // Marking `name` sensitive propagates `keys_sensitive` when part 0 binds `entry.key`.
    let mut sens_rel = union_rel;
    sens_rel.relation.fields[0].1.sensitive = true;
    if let Rows::Union(parts) = &mut sens_rel.relation.rows {
        for part in parts {
            if let Some(fields) = &mut part.fields {
                fields[0].1.sensitive = true;
            }
        }
    }
    let sens_plan = plan(vec![sens_rel]);
    assert!(super::super::keys_sensitive(&sens_plan, RelationId(0)));
}
