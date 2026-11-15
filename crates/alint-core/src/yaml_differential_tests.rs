use ayaml::ErrorKind;

use super::*;

/// The bounded run's seed and size; each corpus file is mutated once.
const BOUNDED_SEED: u64 = 1;
const BOUNDED_INPUTS: usize = 6_000;

/// The share of the bounded run's inputs that both parsers read to the same
/// value at the value entry point, and that both reject at one place, in
/// percent. A generator that stopped producing YAML both read, or that both
/// reject alike, would fall below these floors (the run measured 53% and 17%).
const SAME_VALUE_FLOOR: usize = 40;
const SAME_LOCATION_FLOOR: usize = 12;

#[test]
fn bounded_run_has_no_unexplained_difference() {
    let report = differential_run(BOUNDED_SEED, BOUNDED_INPUTS, 1, true);
    assert_eq!(
        report.failure_count,
        0,
        "{}",
        report.markdown("bounded run")
    );
    let value_cases: usize = Class::ALL
        .iter()
        .map(|class| report.count(Entry::Value, *class))
        .sum();
    assert_eq!(value_cases, report.inputs);
    let percent = |class| report.count(Entry::Value, class) * 100 / report.inputs;
    assert!(
        percent(Class::SameValue) >= SAME_VALUE_FLOOR,
        "{}",
        report.markdown("bounded run")
    );
    assert!(
        percent(Class::RejectedAtSameLocation) >= SAME_LOCATION_FLOOR,
        "{}",
        report.markdown("bounded run")
    );
    assert!(report.count(Entry::Config, Class::SameValue) > 0);
    // Inside a Git checkout the corpus is never empty.
    if repository().join(".git").exists() {
        assert!(
            report.inputs > BOUNDED_INPUTS + 100,
            "the corpus is missing"
        );
    }
}

/// The long run; see the module documentation.
#[test]
#[ignore = "the long run, sized by ALINT_YAML_DIFF_INPUTS"]
fn long_run() {
    let inputs = usize::try_from(env_number("ALINT_YAML_DIFF_INPUTS", 300_000)).unwrap();
    let seed = env_number("ALINT_YAML_DIFF_SEED", 1);
    let mutations = usize::try_from(env_number("ALINT_YAML_DIFF_MUTATIONS", 20)).unwrap();
    let specials = env_number("ALINT_YAML_DIFF_SPECIALS", 1) != 0;
    let report = differential_run(seed, inputs, mutations, specials);
    let title = format!(
        "seed {seed}, {inputs} generated inputs, {mutations} mutations per file, {}",
        if specials {
            "half of them with NEL, LS, PS and BOM"
        } else {
            "none with NEL, LS, PS or BOM"
        }
    );
    if let Ok(path) = std::env::var("ALINT_YAML_DIFF_REPORT") {
        let markdown = report.markdown(&title) + &allowlist_markdown();
        std::fs::write(path, markdown).unwrap();
    }
    assert_eq!(report.failure_count, 0, "{}", report.markdown(&title));
}

#[test]
fn every_deviation_example_is_explained_by_its_own_entry() {
    for deviation in allowlist::DEVIATIONS {
        let (case, panicked) = compare(deviation.example, Entry::Value);
        assert!(!panicked, "{}", deviation.id);
        assert!(
            case.class.is_difference(),
            "{}: {:?} is not a difference: {case:?}",
            deviation.id,
            deviation.example
        );
        assert_eq!(explanation(&case), Some(deviation.id), "{case:?}");
    }
}

#[test]
fn every_location_example_is_explained_by_its_own_class() {
    for location in allowlist::LOCATIONS {
        let (case, panicked) = compare(location.example, Entry::Value);
        assert!(!panicked, "{}", location.id);
        assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
        assert_eq!(explanation(&case), Some(location.id), "{case:?}");
    }
}

#[test]
fn allowlist_ids_are_unique_and_documented() {
    let mut ids = BTreeSet::new();
    for deviation in allowlist::DEVIATIONS {
        assert!(ids.insert(deviation.id), "{} is listed twice", deviation.id);
        assert!(!deviation.classes.is_empty(), "{}", deviation.id);
        assert!(
            deviation.classes.iter().all(|class| class.is_difference()),
            "{}",
            deviation.id
        );
        for text in [deviation.ayaml, deviation.oracle, deviation.reason] {
            assert!(
                !text.is_empty() && !text.contains('\u{2014}'),
                "{}",
                deviation.id
            );
        }
    }
    for location in allowlist::LOCATIONS {
        assert!(ids.insert(location.id), "{} is listed twice", location.id);
        assert!(!location.kinds.is_empty(), "{}", location.id);
        for text in [location.ayaml, location.oracle] {
            assert!(
                !text.is_empty() && !text.contains('\u{2014}'),
                "{}",
                location.id
            );
        }
    }
    let markdown = allowlist_markdown();
    assert!(ids.iter().all(|id| markdown.contains(&format!("`{id}`"))));
}

/// Each syntax error message of ayaml 0.1.2, the version the workspace pins,
/// with sample values in place of its format arguments. Refresh the list from
/// ayaml's scanner and parser when the pin moves.
const AYAML_SYNTAX_MESSAGES: &[&str] = &[
    "%TAG requires a handle and a prefix",
    "a `%` in a verbatim tag must start a two-digit escape",
    "a block collection cannot start on a `---` line",
    "a block scalar header comment requires preceding whitespace",
    "a byte order mark can only start a document",
    "a comment must be separated from the preceding token",
    "a directive cannot contain a byte order mark",
    "a directive needs a name",
    "a document-start property cannot precede a same-line block mapping key",
    "a flow collection's `?` must be separated from what follows it",
    "a tab cannot indent a flow collection's line under a block collection",
    "a tab cannot start a comment after a block scalar",
    "a tag must be followed by whitespace or a line break",
    "a tag's handle needs a suffix after it",
    "a verbatim tag must name a local tag or a tag with a scheme",
    "a verbatim tag needs a name",
    "aliases cannot carry tags or anchors",
    "an anchor name cannot contain a byte order mark",
    "an anchor or alias must be followed by white space",
    "an implicit mapping key may be at most 1024 characters long",
    "anchor/alias name must not be empty",
    "block scalar header must end at a line break",
    "content after document end marker",
    "directive without a document",
    "document marker inside quoted scalar",
    "document markers are not allowed inside flow collections",
    "duplicate %TAG directive for !e!",
    "duplicate %YAML directive",
    "escape is not a Unicode scalar value",
    "expected ',' or a closing bracket",
    "expected a flow item",
    "expected a node",
    "expected a single YAML document, but found trailing documents",
    "expected a token",
    "explicit mapping key must be followed by a ':' line",
    "flow collection entries must be indented",
    "implicit mapping keys must be on one line",
    "invalid %-escape in tag",
    "invalid %TAG handle e",
    "invalid %TAG prefix x",
    "invalid %YAML directive",
    "invalid hexadecimal escape",
    "leading empty block-scalar lines are more indented than the content",
    "malformed flow",
    "properties cannot precede a compact block sequence",
    "properties cannot precede an explicit mapping key",
    "quoted scalar continuation must be indented more than its parent",
    "short hexadecimal escape",
    "tab cannot separate a block structural indicator",
    "tabs are not allowed as leading block-scalar indentation",
    "tabs cannot be the only flow-collection indentation",
    "tabs cannot be the only quoted-scalar indentation",
    "tag %-escapes do not form valid UTF-8",
    "undefined primary tag handle",
    "undefined secondary tag handle",
    "undefined tag handle \"!e!\"",
    "unexpected ] in node position",
    "unexpected empty flow mapping entry",
    "unexpected empty flow sequence entry",
    "unexpected '@' in block scalar header",
    "unknown escape \\q",
    "unterminated double-quoted scalar",
    "unterminated escape",
    "unterminated single-quoted scalar",
    "unterminated verbatim tag",
    "unterminated",
    "'@' is a reserved indicator and cannot start a plain scalar",
];

#[test]
fn every_ayaml_syntax_message_has_a_location_class() {
    for message in AYAML_SYNTAX_MESSAGES {
        let located = [ErrorKind::Parser, ErrorKind::Scanner, ErrorKind::Tag]
            .into_iter()
            .any(|kind| allowlist::locate(kind, message).is_some());
        assert!(located, "no location class covers {message:?}");
    }
}

fn rejection(message: &str, location: Option<(usize, usize)>) -> Rejection {
    Rejection {
        message: message.to_string(),
        location,
        kind: Some(ErrorKind::Parser),
    }
}

#[test]
fn classify_tells_the_classes_apart() {
    let at = |line, column| Err(rejection("e", Some((line, column))));
    let value = |text: &str| Ok(text.to_string());
    assert_eq!(classify(&value("a"), &value("a")), Class::SameValue);
    assert_eq!(classify(&value("a"), &value("b")), Class::DifferentValue);
    assert_eq!(
        classify(&at(1, 2), &at(1, 2)),
        Class::RejectedAtSameLocation
    );
    assert_eq!(
        classify(&at(1, 2), &at(2, 1)),
        Class::RejectedAtDifferentLocation
    );
    let nowhere = || Err(rejection("e", None));
    assert_eq!(
        classify(&nowhere(), &nowhere()),
        Class::RejectedAtDifferentLocation
    );
    assert_eq!(classify(&at(1, 1), &value("a")), Class::OnlyAyamlRejects);
    assert_eq!(classify(&value("a"), &at(1, 1)), Class::OnlyOracleRejects);
}

#[test]
fn an_unexplained_difference_or_a_panic_fails_the_run() {
    let mut report = Report::default();
    let value = Case {
        input: "a: 1\n",
        entry: Entry::Value,
        class: Class::DifferentValue,
        ayaml: Ok("x".to_string()),
        oracle: Ok("y".to_string()),
    };
    assert_eq!(explanation(&value), None);
    report.record(&value, false);
    // An ayaml error message no location class names.
    let location = Case {
        input: "a: [\n",
        entry: Entry::Json,
        class: Class::RejectedAtDifferentLocation,
        ayaml: Err(rejection("a new message", Some((1, 4)))),
        oracle: Err(rejection("e", Some((2, 1)))),
    };
    assert_eq!(explanation(&location), None);
    report.record(&location, false);
    let alike = Case {
        input: "a\n",
        entry: Entry::Value,
        class: Class::SameValue,
        ayaml: Ok("a".to_string()),
        oracle: Ok("a".to_string()),
    };
    assert_eq!(explanation(&alike), Some(""));
    report.record(&alike, true);
    assert_eq!(report.failure_count, 3);
    let markdown = report.markdown("t");
    assert!(
        markdown.contains("3 unexplained differences or panics."),
        "{markdown}"
    );
    assert!(
        markdown.contains("unexplained, Value, DifferentValue:"),
        "{markdown}"
    );
    assert!(markdown.contains("a new message"), "{markdown}");
    assert!(markdown.contains("panic, Value, SameValue:"), "{markdown}");
}

#[test]
fn a_documented_location_explains_a_rejection_at_another_place() {
    let case = Case {
        input: "'\n",
        entry: Entry::Value,
        class: Class::RejectedAtDifferentLocation,
        ayaml: Err(rejection("unterminated single-quoted scalar", Some((1, 1)))),
        oracle: Err(rejection("e", Some((2, 1)))),
    };
    assert_eq!(explanation(&case), Some("where-a-construct-opened"));
    let mut report = Report::default();
    report.record(&case, false);
    assert_eq!(report.failure_count, 0);
    assert_eq!(
        report
            .explained
            .get(&(Entry::Value, "where-a-construct-opened")),
        Some(&1)
    );
}

#[test]
fn a_panic_is_caught_and_reported() {
    let (side, panicked) = guarded(|| panic!("boom"));
    assert!(panicked);
    assert_eq!(side.unwrap_err().message, "panic: boom");
    let (side, panicked) = guarded(|| Ok("a".to_string()));
    assert!(!panicked);
    assert_eq!(side.unwrap(), "a");
}

#[test]
fn the_oracle_message_drops_its_own_location() {
    let error = yaml_serde::from_str::<yaml_serde::Value>("a: [\n").unwrap_err();
    let rejection = oracle_rejection(&error);
    assert!(rejection.location.is_some());
    assert!(
        !rejection.message.contains(" at line "),
        "{}",
        rejection.message
    );
}

#[test]
fn the_same_seed_gives_the_same_inputs() {
    let inputs = |seed| {
        let mut rng = Rng::new(seed);
        (0..60)
            .map(|index| {
                let specials = index % 2 == 1;
                let document = generate::document(&mut rng, specials);
                [
                    generate::token_soup(&mut rng, specials),
                    generate::mutate(&mut rng, &document, specials),
                    document,
                ]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(inputs(7), inputs(7));
    assert_ne!(inputs(7), inputs(8));
}

#[test]
fn without_specials_no_input_holds_one() {
    let mut rng = Rng::new(3);
    for _ in 0..2_000 {
        let document = generate::document(&mut rng, false);
        let inputs = [
            generate::token_soup(&mut rng, false),
            generate::mutate(&mut rng, &document, false),
            document,
        ];
        for input in inputs {
            assert!(
                !generate::SPECIALS
                    .iter()
                    .any(|special| input.contains(special)),
                "{input:?}"
            );
        }
    }
}

#[test]
fn corpus_files_have_their_own_seeds_and_line_breaks() {
    assert_ne!(fnv1a("a.yml"), fnv1a("b.yml"));
    assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
    assert!(corpus().iter().all(|(_, text)| !text.contains("\r\n")));
}

#[test]
fn values_from_both_parsers_meet_in_one_form() {
    let text = "a: [1, -2.5, .nan, true, ~, !t x]\n18446744073709551615: b\n";
    let ayaml = crate::yaml::from_str::<crate::yaml::Value>(text).unwrap();
    let oracle: yaml_serde::Value = yaml_serde::from_str(text).unwrap();
    assert_eq!(from_ayaml(ayaml.parser()), from_oracle(&oracle));
    assert_eq!(
        format!("{:?}", from_ayaml(ayaml.parser())),
        format!(
            "Mapping([(String(\"a\"), Sequence([Integer(\"1\"), Float({}), Float({}), \
             Bool(true), Null, Tagged(\"!t\", String(\"x\"))])), \
             (Integer(\"18446744073709551615\"), String(\"b\"))])",
            (-2.5f64).to_bits(),
            f64::NAN.to_bits()
        )
    );
}

#[test]
fn config_form_orders_hashed_collections() {
    let text = "version: 1\nvars:\n  b: 2\n  a: 1\n  c: 3\n";
    let first = config_form(crate::yaml::from_str(text).unwrap());
    let second = config_form(yaml_serde::from_str(text).unwrap());
    assert_eq!(first, second);
    assert!(
        first.contains(r#"vars: {"a": "1", "b": "2", "c": "3"}"#),
        "{first}"
    );
}
