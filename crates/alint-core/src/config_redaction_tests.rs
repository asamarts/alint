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
fn masking_replaces_every_string_but_keeps_names_and_other_scalars() {
    let mut value: yaml_serde::Value =
        yaml_serde::from_str("a: secret\nb: [one, {c: two, d: 3, e: true}]\nf: ~\n").unwrap();
    mask_configuration_strings(&mut value);
    let expected: yaml_serde::Value = yaml_serde::from_str(
        "a: <redacted>\nb: [<redacted>, {c: <redacted>, d: 3, e: true}]\nf: ~\n",
    )
    .unwrap();
    assert_eq!(value, expected);
}

#[test]
fn masked_configuration_masks_each_environment_field_and_nothing_else() {
    let mut rule = spec(
        "id: r\nkind: file_content_forbidden\nlevel: error\npaths: ['src/**', '!src/gen/**']\n\
         message: token {{vars.x}}\nwhen: env.CI == \"yes\"\npattern: secret\nlimit: 3\n\
         ignore: plain\nscope_filter: {has_ancestor: Cargo.toml, changed_since: main}\n",
    );
    rule.set_sensitive_fields(environment_fields(&[
        "paths",
        "message",
        "when",
        "pattern",
        "limit",
        "scope_filter",
    ]));
    let masked = rule.masked_configuration().unwrap();
    assert_eq!(masked.id, "r");
    assert_eq!(masked.kind, "file_content_forbidden");
    assert!(matches!(
        masked.paths,
        Some(PathsSpec::Many(ref patterns)) if patterns == &[CONFIGURATION_MASK, CONFIGURATION_MASK]
    ));
    assert_eq!(masked.message.as_deref(), Some(CONFIGURATION_MASK));
    assert_eq!(masked.when.as_deref(), Some(CONFIGURATION_MASK));
    assert_eq!(masked.extra["pattern"], CONFIGURATION_MASK);
    // A number cannot quote text, and a field not read from the environment stays.
    assert_eq!(masked.extra["limit"], 3);
    assert_eq!(masked.extra["ignore"], "plain");
    let scope = masked.scope_filter.unwrap();
    assert_eq!(
        scope.has_ancestor,
        Some(vec![CONFIGURATION_MASK.to_string()])
    );
    assert_eq!(scope.changed_since.as_deref(), Some(CONFIGURATION_MASK));
}

#[test]
fn a_sensitive_rule_masks_every_value_but_its_identity() {
    let masked = spec(
        "id: r\nkind: file_content_forbidden\nlevel: warning\nsensitive: true\n\
         paths: {include: a, exclude: b}\npattern: secret\npolicy_url: https://x\n",
    )
    .masked_configuration()
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
}

#[test]
fn a_secret_in_a_typed_field_this_cannot_mask_fails_closed() {
    let mut fix = spec(
        "id: r\nkind: file_exists\nlevel: error\npaths: LICENSE\n\
         fix: {file_create: {content: secret}}\n",
    );
    assert!(
        fix.masked_configuration().is_some(),
        "no secret, nothing to refuse"
    );
    fix.set_sensitive_fields(environment_fields(&["fix"]));
    assert!(fix.masked_configuration().is_none());

    let mut manifest = spec(
        "id: r\nkind: file_content_forbidden\nlevel: error\npaths: '**'\npattern: x\n\
         scope_filter: {include_manifest_paths: {source: Cargo.toml, extract: {toml: '$.a'}}}\n",
    );
    manifest.set_sensitive_fields(environment_fields(&["scope_filter"]));
    assert!(manifest.masked_configuration().is_none());
}

#[test]
fn an_error_stays_readable_only_when_the_masked_copy_reproduces_it() {
    let mut rule = spec("id: r\nkind: k\nlevel: error\npattern: secret\n");
    let structural = || crate::Error::rule_config("r", "unknown field `patern`");
    // Without a secret every error is kept, and the rerun is not consulted.
    let kept = rule.redact_configuration_error(structural(), |_| unreachable!());
    assert_eq!(kept.to_string(), structural().to_string());

    rule.set_sensitive_fields(environment_fields(&["pattern"]));
    let kept = rule.redact_configuration_error(structural(), |_| Err(structural()));
    assert_eq!(kept.to_string(), structural().to_string());

    let quoting = || crate::Error::rule_config("r", "invalid regex `secret`");
    for rerun_result in [
        Err(crate::Error::rule_config("r", "invalid regex `<redacted>`")),
        Ok(()),
    ] {
        let redacted = rule.redact_configuration_error(quoting(), |masked| {
            assert_eq!(masked.extra["pattern"], CONFIGURATION_MASK);
            rerun_result
        });
        let message = redacted.to_string();
        assert!(message.contains(REDACTED_CONFIGURATION_ERROR), "{message}");
        assert!(!message.contains("secret`"), "{message}");
        assert!(message.contains("\"r\""), "the rule's id stays: {message}");
    }

    // An internal error keeps its kind when it is redacted.
    let internal = rule.redact_configuration_error(crate::Error::internal("secret"), |_| Ok(()));
    assert!(internal.is_internal());
    assert!(!internal.to_string().contains("secret"), "{internal}");
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
