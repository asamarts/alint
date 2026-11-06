use super::*;

fn spec(yaml: &str) -> RuleSpec {
    yaml_serde::from_str(yaml).unwrap()
}

fn environment_fields(fields: &[&str]) -> std::collections::BTreeSet<String> {
    fields.iter().map(|field| (*field).to_string()).collect()
}

#[test]
fn only_environment_fields_or_a_sensitive_rules_values_may_be_secret() {
    let none = environment_fields(&[]);
    assert!(!configuration_field_may_be_secret(false, &none, "pattern"));
    assert!(configuration_field_may_be_secret(
        false,
        &environment_fields(&["pattern"]),
        "pattern"
    ));
    assert!(configuration_field_may_be_secret(true, &none, "pattern"));
    assert!(configuration_field_may_be_secret(true, &none, "message"));
    for identity in ["id", "kind", "level", "sensitive"] {
        assert!(
            !configuration_field_may_be_secret(true, &none, identity),
            "{identity}"
        );
    }
    // An identity field read from the environment is still a secret.
    assert!(configuration_field_may_be_secret(
        true,
        &environment_fields(&["kind"]),
        "kind"
    ));
}

#[test]
fn masking_changes_every_value_but_keeps_names_types_and_nested_rule_identity() {
    let document = "a: secret\nb: [one, {c: two, d: 123456789012, e: true, g: 0, h: 1.5, j: -7, \
         k: 18446744073709551615}]\nf: ~\n\
         i: <redacted>\nrequire: [{id: n, kind: file_exists, level: error, paths: x}]\n\
         data: {id: literal, level: high}\n";
    let expected = |d: i64, g: u64, h: f64, j: i64| {
        format!(
            "a: <redacted>\nb: [<redacted>, {{c: <redacted>, d: {d}, e: false, g: {g}, h: {h:?}, \
             j: {j}, k: 18446744073709551614}}]\nf: ~\n\
             i: <masked>\nrequire: [{{id: n, kind: file_exists, level: error, paths: <redacted>}}]\n\
             data: {{id: <redacted>, level: <redacted>}}\n"
        )
    };
    // At its type's maximum or minimum a number moves the other way.
    for (numbers, expected) in [
        (
            NumberMask::Up,
            expected(123_456_789_013, 1, 1.5_f64.next_up(), -6),
        ),
        (
            NumberMask::Down,
            expected(123_456_789_011, 1, 1.5_f64.next_down(), -8),
        ),
    ] {
        let mut value: yaml_serde::Value = yaml_serde::from_str(document).unwrap();
        mask_configuration_values(&mut value, numbers);
        let expected: yaml_serde::Value = yaml_serde::from_str(&expected).unwrap();
        assert_eq!(value, expected, "{numbers:?}");
    }
}

#[test]
fn a_masked_number_always_differs_and_stays_next_to_the_value() {
    let moved = |number: yaml_serde::Number, numbers| {
        let mut value = yaml_serde::Value::Number(number);
        mask_configuration_values(&mut value, numbers);
        let yaml_serde::Value::Number(masked) = value else {
            panic!("a number stays a number: {value:?}");
        };
        masked
    };
    for numbers in NumberMask::BOTH {
        for value in [
            0.0,
            0.5,
            1.0,
            -1.0,
            1.0e300,
            9_007_199_254_740_992.0,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let masked = moved(value.into(), numbers).as_f64().unwrap();
            assert_ne!(masked, value, "{value} {numbers:?}");
            assert_ne!(
                format!("{masked}"),
                format!("{value}"),
                "{value} {numbers:?}"
            );
            // One step, so a range check on the value still passes.
            assert!(
                [value.next_up(), value.next_down()]
                    .iter()
                    .any(|next| next.to_bits() == masked.to_bits()),
                "{value} {numbers:?}"
            );
        }
        assert_eq!(moved(f64::NAN.into(), numbers).as_f64(), Some(0.0));
        for value in [0, 7, u64::MAX] {
            let masked = moved(value.into(), numbers).as_u64().unwrap();
            assert_eq!(masked.abs_diff(value), 1, "{value} {numbers:?}");
        }
        for value in [-7, i64::MIN] {
            let masked = moved(value.into(), numbers).as_i64().unwrap();
            assert_eq!(masked.abs_diff(value), 1, "{value} {numbers:?}");
        }
    }
    assert_eq!(
        moved(0.5.into(), NumberMask::Up).as_f64(),
        Some(0.5_f64.next_up())
    );
    assert_eq!(
        moved(0.5.into(), NumberMask::Down).as_f64(),
        Some(0.5_f64.next_down())
    );
}

#[test]
fn environment_values_are_removed_in_every_form_an_error_prints() {
    let values = [
        "canary-secret".to_string(),
        "0xDEADBEEF".to_string(),
        "say \"hi\"".to_string(),
        "true".to_string(),
        "abc".to_string(),
    ];
    let text = "glob \"canary-secret[\", number 3735928559, quoted \"say \\\"hi\\\"\", \
                flag true, short abc";
    let redacted = redact_environment_values(text, &values);
    assert_eq!(
        redacted,
        "glob \"<redacted>[\", number <redacted>, quoted \"<redacted>\", flag true, short abc"
    );
    // A path is printed joined to a directory, without its `./`, and on
    // Windows with backslashes (doubled by `{:?}`).
    let paths = [
        "./canary-secret.yml".to_string(),
        "conf/canary/x.yml".to_string(),
    ];
    assert_eq!(
        redact_environment_values(
            "I/O error at \\\\?\\C:\\T\\canary-secret.yml; \"C:\\\\conf\\\\canary\\\\x.yml\"",
            &paths
        ),
        "I/O error at \\\\?\\C:\\T\\<redacted>; \"C:\\\\<redacted>\""
    );
    // A value that contains another is removed whole.
    let nested = ["secret".to_string(), "secret-token".to_string()];
    assert_eq!(
        redact_environment_values("x secret-token y secret", &nested),
        "x <redacted> y <redacted>"
    );
    // A value inside a longer word takes the word with it, so neither the
    // value nor a fragment of the word is left.
    let word = ["build".to_string(), "canary-secret".to_string()];
    assert_eq!(
        redact_environment_values(
            "build, building rebuild_x; \"xcanary-secret_tmp\" -build- é_build",
            &word
        ),
        "<redacted>, <redacted> <redacted>; \"<redacted>\" -<redacted>- <redacted>"
    );
    // A value written with surrounding whitespace or over several lines is
    // printed trimmed, or (a regex error) one line at a time.
    let spaced = [
        "  spaced-canary \n".to_string(),
        "first-line\n  second-line".to_string(),
    ];
    assert_eq!(
        redact_environment_values("glob spaced-canary; at second-line and first-line", &spaced),
        "glob <redacted>; at <redacted> and <redacted>"
    );
    assert!(matches!(
        redact_environment_values("nothing to hide", &values),
        std::borrow::Cow::Borrowed(_)
    ));
}

#[test]
fn an_error_keeps_its_kind_when_an_environment_value_is_removed() {
    let values = ["canary-secret".to_string()];
    // `Internal` is an alint bug (exit 3), not a configuration error.
    let internal =
        redact_environment_values_in_error(crate::Error::internal("bad canary-secret"), &values);
    assert!(matches!(internal, crate::Error::Internal(ref m) if m == "bad <redacted>"));
    let rule = redact_environment_values_in_error(
        crate::Error::rule_config("canary-secret", "glob canary-secret["),
        &values,
    );
    assert!(matches!(
        rule,
        crate::Error::RuleConfig { ref rule_id, ref message }
            if rule_id == "<redacted>" && message == "glob <redacted>["
    ));
    let other = redact_environment_values_in_error(
        crate::Error::Other("missing canary-secret.yml".into()),
        &values,
    );
    assert!(matches!(other, crate::Error::Other(ref m) if m == "missing <redacted>.yml"));
    // An error that quotes no value, and `FileNotInIndex`, come back as they are.
    let untouched =
        redact_environment_values_in_error(crate::Error::internal("nothing to hide"), &values);
    assert!(matches!(untouched, crate::Error::Internal(ref m) if m == "nothing to hide"));
    let not_in_index = redact_environment_values_in_error(
        crate::Error::file_not_in_index(std::path::Path::new("canary-secret.md")),
        &values,
    );
    assert!(matches!(
        not_in_index,
        crate::Error::FileNotInIndex { ref path } if path.as_os_str() == "canary-secret.md"
    ));
}

#[test]
fn masked_configuration_masks_each_environment_field_and_nothing_else() {
    let mut rule = spec(
        "id: r\nkind: file_content_forbidden\nlevel: error\npaths: ['src/**', '!src/gen/**']\n\
         message: token {{vars.x}}\nwhen: env.CI == \"yes\"\npattern: secret\nlimit: 3\n\
         ignore: plain\npolicy_url: https://docs\n\
         scope_filter: {has_ancestor: Cargo.toml, changed_since: main}\n",
    );
    rule.set_sensitive_fields(environment_fields(&[
        "paths",
        "message",
        "when",
        "pattern",
        "limit",
        "scope_filter",
    ]));
    let masked = rule.masked_configuration(NumberMask::Up).unwrap();
    assert_eq!(masked.id, "r");
    assert_eq!(masked.kind, "file_content_forbidden");
    assert!(matches!(
        masked.paths,
        Some(PathsSpec::Many(ref patterns)) if patterns == &[CONFIGURATION_MASK, CONFIGURATION_MASK]
    ));
    assert_eq!(masked.message.as_deref(), Some(CONFIGURATION_MASK));
    assert_eq!(masked.when.as_deref(), Some(CONFIGURATION_MASK));
    assert_eq!(masked.extra["pattern"], CONFIGURATION_MASK);
    // A number read from the environment can be a secret (an account id) and
    // is masked by the next number; a field not read from the environment stays.
    assert_eq!(masked.extra["limit"], 4);
    assert_eq!(
        rule.masked_configuration(NumberMask::Down).unwrap().extra["limit"],
        2
    );
    assert_eq!(masked.extra["ignore"], "plain");
    assert_eq!(masked.policy_url.as_deref(), Some("https://docs"));
    let scope = masked.scope_filter.unwrap();
    assert_eq!(
        scope.has_ancestor,
        Some(vec![CONFIGURATION_MASK.to_string()])
    );
    assert_eq!(scope.changed_since.as_deref(), Some(CONFIGURATION_MASK));

    let mut single = spec("id: r\nkind: k\nlevel: error\npaths: 'canary-secret['\n");
    single.set_sensitive_fields(environment_fields(&["paths"]));
    assert!(matches!(
        single.masked_configuration(NumberMask::Up).unwrap().paths,
        Some(PathsSpec::Single(ref pattern)) if pattern == CONFIGURATION_MASK
    ));
}

#[test]
fn a_sensitive_rule_masks_every_value_but_its_identity() {
    let masked = spec(
        "id: r\nkind: file_content_forbidden\nlevel: warning\nsensitive: true\n\
         paths: {include: a, exclude: b}\npattern: secret\npolicy_url: https://x\n",
    )
    .masked_configuration(NumberMask::Up)
    .unwrap();
    assert_eq!(
        (masked.id.as_str(), masked.kind.as_str(), masked.level),
        ("r", "file_content_forbidden", Level::Warning)
    );
    assert!(matches!(
        masked.paths,
        Some(PathsSpec::IncludeExclude { ref include, ref exclude })
            if include == &[CONFIGURATION_MASK] && exclude == &[CONFIGURATION_MASK]
    ));
    assert_eq!(masked.extra["pattern"], CONFIGURATION_MASK);
    assert_eq!(masked.policy_url.as_deref(), Some(CONFIGURATION_MASK));
    // A field the rule leaves out stays out: a masked `when:` would gate the
    // copy on a condition the rule does not have.
    assert_eq!((masked.message, masked.when), (None, None));
}

#[test]
fn a_secret_in_a_typed_field_this_cannot_mask_fails_closed() {
    let mut fix = spec(
        "id: r\nkind: file_exists\nlevel: error\npaths: LICENSE\n\
         fix: {file_create: {content: secret}}\n",
    );
    assert!(
        fix.masked_configuration(NumberMask::Up).is_some(),
        "no secret, nothing to refuse"
    );
    fix.set_sensitive_fields(environment_fields(&["fix"]));
    assert!(fix.masked_configuration(NumberMask::Up).is_none());

    let mut manifest = spec(
        "id: r\nkind: file_content_forbidden\nlevel: error\npaths: '**'\npattern: x\n\
         scope_filter: {include_manifest_paths: {source: Cargo.toml, extract: {toml: '$.a'}}}\n",
    );
    manifest.set_sensitive_fields(environment_fields(&["scope_filter"]));
    assert!(manifest.masked_configuration(NumberMask::Up).is_none());

    let mut excluded = spec(
        "id: r\nkind: file_content_forbidden\nlevel: error\npaths: '**'\npattern: x\n\
         scope_filter: {exclude_manifest_paths: {source: Cargo.toml, extract: {toml: '$.a'}}}\n",
    );
    excluded.set_sensitive_fields(environment_fields(&["scope_filter"]));
    assert!(excluded.masked_configuration(NumberMask::Up).is_none());
}

#[test]
fn an_error_stays_readable_only_when_the_masked_copy_reproduces_it() {
    let mut rule = spec("id: r\nkind: k\nlevel: error\npattern: secret\n");
    let structural = || crate::Error::rule_config("r", "unknown field `patern`");
    // Without a secret every error is kept, and the rerun is not consulted.
    let kept = rule.redact_unless_reproduced(structural(), |_| unreachable!());
    assert_eq!(kept.to_string(), structural().to_string());

    rule.set_sensitive_fields(environment_fields(&["pattern"]));
    let kept = rule.redact_unless_reproduced(structural(), |_| Err(structural()));
    assert_eq!(kept.to_string(), structural().to_string());

    let quoting = || crate::Error::rule_config("r", "invalid regex `secret`");
    for fails in [true, false] {
        let mut reruns = 0;
        let redacted = rule.redact_unless_reproduced(quoting(), |masked| {
            assert_eq!(masked.extra["pattern"], CONFIGURATION_MASK);
            reruns += 1;
            if fails {
                Err(crate::Error::rule_config("r", "invalid regex `<redacted>`"))
            } else {
                Ok(())
            }
        });
        // Numbers moved up, then down.
        assert_eq!(reruns, 2);
        let message = redacted.to_string();
        assert!(message.contains(REDACTED_CONFIGURATION_ERROR), "{message}");
        assert!(!message.contains("secret`"), "{message}");
        assert!(message.contains("\"r\""), "the rule's id stays: {message}");
    }

    // A number at the top of its range fails the check moved up, and the
    // unrelated error recurs with it moved down.
    let mut limited = spec("id: r\nkind: k\nlevel: error\nlimit: 100\n");
    limited.set_sensitive_fields(environment_fields(&["limit"]));
    let kept = limited.redact_unless_reproduced(structural(), |masked| {
        if masked.extra["limit"].as_u64() > Some(100) {
            Err(crate::Error::rule_config(
                "r",
                "`limit` must be at most 100",
            ))
        } else {
            Err(structural())
        }
    });
    assert_eq!(kept.to_string(), structural().to_string());

    // An internal error keeps its kind when it is redacted.
    let internal = rule.redact_unless_reproduced(crate::Error::internal("secret"), |_| Ok(()));
    assert!(internal.is_internal());
    assert!(!internal.to_string().contains("secret"), "{internal}");
}

#[test]
#[allow(deprecated)]
fn the_0_21_0_redaction_methods_still_fail_closed() {
    // The published 0.21.0 `alint`, `alint-lsp` and `alint-testkit` call
    // these with the 0.21.0 signatures; a patch release must keep them
    // building, and keep the secret out as they expect.
    let mut rule = spec("id: r\nkind: k\nlevel: error\npattern: secret\n");
    let quoting = || crate::Error::rule_config("r", "invalid regex `secret`");
    assert_eq!(
        rule.redact_configuration_error(quoting()).to_string(),
        quoting().to_string()
    );
    assert_eq!(rule.redact_configuration_message("`secret`"), "`secret`");
    rule.set_sensitive_fields(environment_fields(&["pattern"]));
    let redacted = rule.redact_configuration_error(quoting()).to_string();
    assert!(
        redacted.contains(REDACTED_CONFIGURATION_ERROR),
        "{redacted}"
    );
    assert!(
        redacted.contains("\"r\""),
        "the rule's id stays: {redacted}"
    );
    assert!(
        rule.redact_configuration_error(crate::Error::internal("secret"))
            .is_internal()
    );
    assert_eq!(
        rule.redact_configuration_message("`secret`"),
        REDACTED_CONFIGURATION_ERROR
    );
}

#[test]
fn a_when_error_is_hidden_only_when_the_condition_may_be_secret() {
    let mut rule = spec("id: r\nkind: k\nlevel: error\nwhen: 'env.A =='\nmessage: m\n");
    rule.set_sensitive_fields(environment_fields(&["message"]));
    assert_eq!(
        rule.redact_when_error("unexpected end at `==`"),
        "unexpected end at `==`"
    );
    rule.set_sensitive_fields(environment_fields(&["when"]));
    assert_eq!(
        rule.redact_when_error("unexpected end at `==`"),
        REDACTED_CONFIGURATION_ERROR
    );
    let sensitive = spec("id: r\nkind: k\nlevel: error\nsensitive: true\nwhen: x\n");
    assert_eq!(
        sensitive.redact_when_error("bad"),
        REDACTED_CONFIGURATION_ERROR
    );
}

#[test]
fn a_masked_jsonpath_still_parses_and_differs() {
    for query in ["$.k", "$['a-b'][0]", "$", "$['<redacted>']"] {
        let mut value = yaml_serde::Value::String(query.to_string());
        mask_configuration_values(&mut value, NumberMask::Up);
        let yaml_serde::Value::String(masked) = value else {
            panic!("a string stays a string: {value:?}");
        };
        assert_ne!(masked, query);
        assert!(
            serde_json_path::JsonPath::parse(&masked).is_ok(),
            "{query} -> {masked}"
        );
    }
}

#[test]
fn a_masked_when_iter_still_parses_and_differs() {
    for (when_iter, expected) in [
        ("iter.basename == \"canary\"", "false"),
        ("false", "true"),
        ("true", "false"),
    ] {
        let mut rule = spec(&format!(
            "id: r\nkind: for_each_dir\nlevel: error\nselect: 'a/*'\nwhen_iter: '{when_iter}'\n"
        ));
        rule.set_sensitive_fields(environment_fields(&["when_iter", "select"]));
        let masked = rule.masked_configuration(NumberMask::Up).unwrap();
        assert_eq!(masked.extra["when_iter"], expected, "{when_iter}");
        assert!(crate::when::parse(expected).is_ok());
        // Other fields keep the plain mask.
        assert_eq!(masked.extra["select"], CONFIGURATION_MASK);
    }
}

#[test]
fn a_masked_nested_condition_still_parses() {
    // A nested rule parses its `when:` and `when_iter:` before the rules they
    // guard, so an unrelated error in those rules recurs on the masked copy
    // only when the masked conditions parse.
    let mut value: yaml_serde::Value = yaml_serde::from_str(concat!(
        "- kind: for_each_file\n",
        "  select: '*.md'\n",
        "  when_iter: 'iter.basename != \"x\"'\n",
        "  require:\n",
        "    - kind: file_exists\n",
        "      paths: a\n",
        "      when: 'false'\n",
    ))
    .unwrap();
    mask_configuration_values(&mut value, NumberMask::Up);
    let outer = &value[0];
    assert_eq!(outer["kind"], "for_each_file");
    assert_eq!(outer["when_iter"], "false");
    assert_eq!(outer["select"], CONFIGURATION_MASK);
    let inner = &outer["require"][0];
    assert_eq!(inner["when"], "true");
    assert_eq!(inner["paths"], CONFIGURATION_MASK);
    // Outside a rule, `when` is an ordinary value.
    let mut plain: yaml_serde::Value = yaml_serde::from_str("when: 'false'\n").unwrap();
    mask_configuration_values(&mut plain, NumberMask::Up);
    assert_eq!(plain["when"], CONFIGURATION_MASK);
}
