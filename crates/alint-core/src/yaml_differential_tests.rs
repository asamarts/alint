use ayaml::ErrorKind;

use super::*;

/// The bounded run's seed and size; each corpus file is mutated once.
const BOUNDED_SEED: u64 = 1;
const BOUNDED_INPUTS: usize = 6_000;

/// The share of the bounded run's inputs that both parsers read to the same
/// value at the value entry point, and that both reject at one place, in
/// percent. A generator that stopped producing YAML both read, or that both
/// reject alike, would fall below these floors (the run measured 52% and
/// 15%).
const SAME_VALUE_FLOOR: usize = 40;
const SAME_LOCATION_FLOOR: usize = 10;

/// The share of the bounded run's generated configurations, unmutated, that
/// both decode into `Config` to the same value, and of their rules that both
/// decode into `RuleSpec` to the same value, in percent (the run measured 5%
/// of 750 and 44% of 1,868). The floors read the generated configurations
/// alone, as the corpus's own configurations would meet them without any: a
/// generator that stopped writing configurations both read would fall below
/// them.
const CONFIG_FLOOR: usize = 3;
const RULE_FLOOR: usize = 30;

#[test]
fn bounded_run_has_no_unexplained_difference() {
    let report = differential_run(BOUNDED_SEED, BOUNDED_INPUTS, 1, true);
    assert_eq!(
        report.failure_count,
        0,
        "{}",
        report.markdown("bounded run")
    );
    let cases = |entry| -> usize {
        Class::ALL
            .iter()
            .map(|class| report.count(entry, *class))
            .sum()
    };
    assert_eq!(cases(Entry::Value), report.inputs);
    let percent = |count| count * 100 / report.inputs;
    let markdown = report.markdown("bounded run");
    assert!(
        percent(report.count(Entry::Value, Class::SameValue)) >= SAME_VALUE_FLOOR,
        "{markdown}"
    );
    assert!(
        percent(report.count(Entry::Value, Class::RejectedAtSameLocation)) >= SAME_LOCATION_FLOOR,
        "{markdown}"
    );
    let configurations = |entry, class| {
        report
            .configuration_classes
            .get(&(entry, class))
            .copied()
            .unwrap_or(0)
    };
    let rules: usize = Class::ALL
        .iter()
        .map(|class| configurations(Entry::Rule, *class))
        .sum();
    assert_eq!(report.configurations, BOUNDED_INPUTS / 8);
    assert!(rules >= report.configurations, "{rules} rules\n{markdown}");
    let config = configurations(Entry::Config, Class::SameValue) * 100 / report.configurations;
    let rule = configurations(Entry::Rule, Class::SameValue) * 100 / rules;
    assert!(
        config >= CONFIG_FLOOR && rule >= RULE_FLOOR,
        "{config}% and {rule}% decode alike\n{markdown}"
    );
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
            "every other one may hold NEL, LS, PS or BOM"
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
        let (case, panicked) = compare(deviation.example, deviation.example_entry());
        assert!(!panicked, "{}", deviation.id);
        assert!(
            case.class.is_difference(),
            "{}: {:?} is not a difference: {case:?}",
            deviation.id,
            deviation.example
        );
        assert_eq!(explanation(&case), Some(vec![deviation.id]), "{case:?}");
    }
}

#[test]
fn every_location_example_is_explained_by_its_own_class() {
    for location in allowlist::LOCATIONS {
        let (input, entry) = location.example;
        let (case, panicked) = compare(input, entry);
        assert!(!panicked, "{}", location.id);
        assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
        assert_eq!(explanation(&case), Some(vec![location.id]), "{case:?}");
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
        let because = match deviation.trigger {
            Trigger::Narrow { because, .. } => because,
            Trigger::Input { .. } => "input-wide",
        };
        for text in [deviation.ayaml, deviation.oracle, deviation.reason, because] {
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

/// A difference that only `quoted-only-characters` explains: ayaml keeps
/// DEL in a quoted scalar, the oracle rejects it.
const UNEXPLAINED: &str = "a: '\u{7f}'\n";
const UNEXPLAINED_ENTRY: &str = "quoted-only-characters";

/// For each input-wide entry, an input with a difference that entry does not
/// explain, with the entry's trigger added as it comes in real files: in a
/// comment, a string or a shell command, or in front of the document. The
/// third field lists the entries left out so that the difference has no
/// other explanation; most inputs are [`UNEXPLAINED`] with the trigger.
const TRIGGERS: &[(&str, &str, &[&str])] = &[
    (
        "yaml-1.2-line-breaks",
        "a: '\u{7f}'\n# \u{85}\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "byte-order-mark-starting-a-line",
        "\u{feff}a: '\u{7f}'\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "quoted-only-characters",
        "a: !!binary x\nb: '\u{7f}'\n",
        &["global-tags"],
    ),
    (
        "verbatim-non-specific-tag",
        "a: '\u{7f}'\nb: !<!> x\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "empty-flow-implicit-key",
        "a: '\u{7f}'\nb: [: x]\n",
        &[UNEXPLAINED_ENTRY],
    ),
    // The entry covers a case by the oracle's message, which an anchor name
    // with `@` shares.
    (
        "tab-as-white-space",
        "run:\tmake\nb: &a@1 x\n",
        &["anchor-name-characters"],
    ),
    // The entry covers a case by the oracle's message, which a key that an
    // LS ends, for the oracle, shares.
    (
        "empty-implicit-key",
        "a:\n  k\u{2028}: 1\nb:\n  : c\n",
        &["yaml-1.2-line-breaks"],
    ),
    (
        "empty-flow-explicit-key",
        "a: '\u{7f}'\nb: [? ]\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "flow-colon-scalar-after-properties",
        "a: '\u{7f}'\nb: [!!str :c]\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "document-end-marker-alone",
        "...\na: '\u{7f}'\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "anchor-name-characters",
        "a: '\u{7f}'\nb: &ci.env x\nc: *ci.env\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "integers-beyond-64-bits",
        "a: '\u{7f}'\nb: 18446744073709551616\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "redefined-anchor",
        "a: '\u{7f}'\nb: [&x 1, &x 2, *x]\n",
        &[UNEXPLAINED_ENTRY],
    ),
    // A long key, then a plain scalar that starts with `:`.
    ("long-flow-mapping-key", "", &["yaml-1.2-flow-syntax"]),
    // A key of 600 `é`, then a key that an LS ends, for the oracle.
    (
        "implicit-key-length-in-characters",
        "",
        &["yaml-1.2-line-breaks"],
    ),
    (
        "top-level-block-scalar-content",
        "|\nx\u{85}y\n",
        &["yaml-1.2-line-breaks"],
    ),
    (
        "core-tag-on-another-kind",
        "a: '\u{7f}'\nb: !!str x\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "typed-decode-core-tags",
        "version: !!int '1'\nvars:\n  a: '\u{7f}'\nrules: []\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "negative-zero-in-an-unsigned-field",
        "version: 1\nfix_size_limit: -0\nvars:\n  a: '\u{7f}'\nrules: []\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "flow-line-tab-indentation",
        "a: '\u{7f}'\nb: [x,\n\t  y]\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "global-tags",
        "a: '\u{7f}'\nb: '!!'\n# !!x\nc: !!binary aGk=\n",
        &[UNEXPLAINED_ENTRY],
    ),
    (
        "non-specific-tag",
        "a: !!binary x\nb: ! 1\n",
        &["global-tags"],
    ),
];

/// The input of `id`'s row in [`TRIGGERS`]; a long run is built here.
fn triggered(id: &str, input: &str) -> String {
    match id {
        "long-flow-mapping-key" => format!("{{{}: v, x: [:a]}}\n", "k".repeat(1_100)),
        "implicit-key-length-in-characters" => {
            format!("{}: 1\nb\u{2028}c: 1\n", "é".repeat(600))
        }
        _ => input.to_string(),
    }
}

/// The input-wide entries explain a difference only when their trigger
/// causes it: a known unexplained difference with a trigger added stays
/// unexplained, although the entry covers it and finds its trigger.
#[test]
fn an_input_wide_trigger_does_not_explain_another_difference() {
    let (base, _) = compare(UNEXPLAINED, Entry::Value);
    assert_eq!(base.class, Class::OnlyOracleRejects);
    assert_eq!(explanation_without(&base, &[UNEXPLAINED_ENTRY]), None);
    for deviation in allowlist::DEVIATIONS {
        let Trigger::Input { neutralize, .. } = deviation.trigger else {
            continue;
        };
        let (_, input, excluded) = TRIGGERS
            .iter()
            .find(|(id, ..)| *id == deviation.id)
            .unwrap_or_else(|| panic!("{} has no trigger in TRIGGERS", deviation.id));
        let input = triggered(deviation.id, input);
        let (case, panicked) = compare(&input, deviation.example_entry());
        assert!(!panicked, "{}", deviation.id);
        assert!(
            deviation.covers(&case) && !neutralize(&input).is_empty(),
            "{} does not cover {case:?}",
            deviation.id
        );
        assert_eq!(
            explanation_without(&case, excluded),
            None,
            "{} explains {case:?}",
            deviation.id
        );
    }
}

/// A rewrite after which a parser rejects what it read is not used: a NEL
/// turned into a line break at the start of the next line leaves the quoted
/// scalar's second line unindented here, which ayaml rejects. Removing the
/// NEL, or the break with the next line indented, explains the case.
#[test]
fn a_rewrite_must_not_make_a_parser_reject() {
    let (case, _) = compare("a: 'x\u{85}y'\n", Entry::Value);
    assert_eq!(case.class, Class::DifferentValue);
    let rewrites = neutralize::line_breaks(case.input);
    let (broken, _) = compare(&rewrites[2], Entry::Value);
    assert!(broken.ayaml.is_err(), "{broken:?}");
    assert!(fairness(&case, &broken, &Hidden::Nothing).is_none());
    let (indented, _) = compare(&rewrites[1], Entry::Value);
    assert_eq!(indented.class, Class::SameValue, "{indented:?}");
    assert_eq!(explanation(&case), Some(vec!["yaml-1.2-line-breaks"]));
}

/// Differences that only their own entry explains, one of each kind the
/// search tells apart: both parsers read the first, only ayaml rejects the
/// second, only the oracle the third.
const MASKED: &[(&str, &str)] = &[
    ("b: ! 1\n", "non-specific-tag"),
    ("c: !<!> x\n", "verbatim-non-specific-tag"),
    ("d: '\u{7f}'\n", "quoted-only-characters"),
];

/// Triggers of the entries, as lines of a block mapping, which a base can
/// come before or after. The input-wide entries', among them the review's
/// repros, a NEL rewrite that makes ayaml reject, a `!!str` the core-tag
/// rewrite would drop onto another key, alone and after a `!!null` on an
/// empty node, which the oracle rejects, so that both parsers would reject
/// the rewrite at one place, and properties inside a quoted scalar and a
/// comment. Then the narrow entries', which one parser rejects: each
/// explains that parser's error, not a difference in the lines both read
/// before it, or in the lines after it, which the other parser read. And a
/// second document, which the oracle refuses without a place.
fn masking_triggers() -> Vec<String> {
    let mut triggers: Vec<String> = [
        "m: 'x\u{85}y'\n",
        "m: 'x &y'\n# '\n",
        "m: {!!str 1: x, 1: y}\n",
        "l: !!null \nm: {!!str 1: x, 1: y}\n",
        "m: !!map 1\n",
        "m: !<tag:x,2000:y> 1\n",
        "m: 18446744073709551616\n",
        "m: 0x10000000000000000\n",
        "run:\tmake\n",
        "run: make && test\n",
        ": e\n",
        "m: [? ]\n",
        "m: [: c]\n",
        "m: [!!str :c]\n",
        "m: [&x 1, &x 2, *x]\n",
        "m: !<!> x\n",
        "m: [x,\n\t  y]\n",
        "m: [x\n\t]\n",
        // The narrow entries'.
        "m: [:a]\n",
        "m: [-]\n",
        "m: !e:a!z x\n",
        "m: !t# x\n",
        "m: 'x'#c\n",
        "m: 'x\ny'\n",
        "m:\n|2\n  x\n",
        "---\nn: 2\n",
    ]
    .map(String::from)
    .to_vec();
    triggers.push(format!("m: {{{}: v}}\n", "k".repeat(1_100)));
    triggers.push(format!("{}: v\n", "é".repeat(600)));
    triggers
}

/// Triggers of the location classes, which both parsers reject at different
/// places, for after a base only: neither parser reads past one, so a base
/// after it shows nothing. A location class explains the two errors as one,
/// not a difference in the lines before them.
const REJECTING_TRIGGERS: &[&str] = &["m: & x\n", "m: &a [*a]\n"];

/// No trigger explains a difference it does not cause: with the base's own
/// entry left out, each base with each trigger before or after it, or with
/// a byte-order mark or `...` in front, stays unexplained. This reaches each
/// tier of the search: rewrites a parser newly rejects, differences both
/// parsers agree on, errors that move, a value error in place of a syntax
/// error, two errors a location class would take for one, and a narrow
/// entry's error with a difference before or after it.
#[test]
fn a_trigger_masks_no_other_difference() {
    for ((base, id), entry) in MASKED
        .iter()
        .flat_map(|masked| [(masked, Entry::Value), (masked, Entry::Json)])
    {
        let (case, _) = compare(base, entry);
        assert!(case.class.is_difference(), "{case:?}");
        assert_eq!(explanation(&case), Some(vec![*id]), "{case:?}");
        assert_eq!(explanation_without(&case, &[id]), None, "{case:?}");
        // A wide integer is no JSON number, which both reject alike.
        let mut inputs: Vec<String> = masking_triggers()
            .iter()
            .filter(|trigger| {
                entry == Entry::Value
                    || !["18446744073709551616", "0x10000000000000000"]
                        .iter()
                        .any(|wide| trigger.contains(wide))
            })
            .flat_map(|trigger| [format!("{trigger}{base}"), format!("{base}{trigger}")])
            .collect();
        inputs.extend(
            REJECTING_TRIGGERS
                .iter()
                .map(|trigger| format!("{base}{trigger}")),
        );
        inputs.push(format!("\u{feff}{base}"));
        inputs.push(format!("...\n{base}"));
        let mut located = 0;
        for input in &inputs {
            let (case, panicked) = compare(input, entry);
            assert!(!panicked && case.class.is_difference(), "{case:?}");
            located += usize::from(case.class == Class::RejectedAtDifferentLocation);
            assert_eq!(explanation_without(&case, &[id]), None, "{id}: {case:?}");
        }
        // The base only ayaml rejects meets the triggers the oracle rejects.
        assert!(
            *id != "verbatim-non-specific-tag" || located >= 8,
            "{located}"
        );
    }
}

/// The typed decodes' input-wide entries, beside each other's construct in a
/// configuration, before and after it, explain only their own difference:
/// a core tag that both read alike or that only ayaml refuses, beside `-0`,
/// and `-0` beside a core tag.
#[test]
fn a_typed_decode_trigger_masks_no_other_difference() {
    for (base, id, triggers) in [
        (
            "version: 1\nfix_size_limit: -0\n",
            "negative-zero-in-an-unsigned-field",
            &[
                "nested_configs: !!bool false\n",
                "respect_gitignore: !!str true\n",
            ][..],
        ),
        (
            "version: !!str 1\n",
            "typed-decode-core-tags",
            &["fix_size_limit: -0\n", "vars:\n  a: -0\n"],
        ),
    ] {
        let (case, _) = compare(base, Entry::Config);
        assert_eq!(explanation(&case), Some(vec![id]), "{case:?}");
        for trigger in triggers {
            for input in [format!("{base}{trigger}"), format!("{trigger}{base}")] {
                let (case, panicked) = compare(&input, Entry::Config);
                assert!(!panicked && case.class.is_difference(), "{case:?}");
                assert_eq!(explanation_without(&case, &[id]), None, "{id}: {case:?}");
            }
        }
    }
}

/// Whether `input` holds text that no rewrite may change: a tab in a
/// scalar's content, or an anchor, alias or tag sigil in a scalar's text or
/// a comment.
fn holds_text_no_rewrite_touches(input: &str) -> bool {
    let places = neutralize::places(input);
    input.char_indices().any(|(at, ch)| match ch {
        '\t' => neutralize::tab_in_content(input, &places, at),
        '&' | '*' | '!' => places[at].literal || places[at].plain,
        _ => false,
    })
}

/// A defect planted in ayaml's value of an input that holds such text.
fn defect_in_text(input: &str, _: Entry, side: Side) -> Side {
    if holds_text_no_rewrite_touches(input) {
        side.map(|value| format!("{value} (planted)"))
    } else {
        side
    }
}

/// A rewrite changes its construct only, so a defect planted in ayaml's
/// reading of an input with a tab or a sigil in a scalar's text, which
/// every rewrite keeps, stays unexplained, with no exception: over the
/// bounded run's generated inputs that both parsers read alike, and the
/// review's repros beside the constructs of the input-wide entries that
/// cover a different value. A rewrite that removed the text would remove
/// the defect, and the search would take it for the entry's.
#[test]
fn a_defect_in_text_no_rewrite_touches_is_never_explained() {
    let mut rng = Rng::new(1);
    let mut inputs: Vec<String> = (0..6_000)
        .map(|index| generated(&mut rng, index, true))
        .collect();
    for text in [
        "s: \"a\tb\"\n",
        "run: make && test\n",
        "a: x !!int y\n",
        "h: hello !\n",
        "q: 'x &y' # *z\n",
        "p: |\n  a\tb\n",
    ] {
        for beside in [
            "",
            "k: &a1 v\nl: *a1\n",
            "t: !!str s\n",
            "n: ! x\n",
            "c: !<tag:x,2000:y> x\n",
            "e: 'x\u{2028}y'\n",
        ] {
            inputs.push(format!("{text}{beside}"));
            inputs.push(format!("{beside}{text}"));
        }
    }
    let mut planted = 0;
    for input in inputs
        .iter()
        .filter(|input| holds_text_no_rewrite_touches(input))
    {
        for entry in [Entry::Value, Entry::Json] {
            let (clean, case) = planted::clean_and_planted(input, entry, defect_in_text);
            if clean.class != Class::SameValue || !case.class.is_difference() {
                continue;
            }
            planted += 1;
            assert_eq!(
                planted::planted_explanation(&case, defect_in_text),
                None,
                "{case:?}"
            );
        }
    }
    assert!(planted >= 2_000, "{planted}");
}

/// A line that starts with `:` holds the value of the `?` key before it: a
/// split there would read `: b` alone, an empty key that only ayaml reads,
/// and take ayaml's one error for two differences.
#[test]
fn a_split_keeps_a_colon_line_with_its_key() {
    let (case, _) = compare("? 'x'#c\n: b\n", Entry::Value);
    assert_eq!(case.class, Class::OnlyAyamlRejects, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["comment-needs-white-space"]),
        "{case:?}"
    );
}

/// ayaml's error at `!<!>` in what YAML 1.2 and the oracle read as a plain
/// scalar's second line: a defect planted where ayaml reads the text as both
/// do.
fn verbatim_tag_in_text(input: &str, _: Entry, side: Side) -> Side {
    if input.contains("\n!<!> a") {
        Err(Rejection {
            message: "a verbatim tag needs a name".to_string(),
            location: Some((2, 1)),
            kind: Some(ErrorKind::Scanner),
        })
    } else {
        side
    }
}

/// The rewrite of `!<!>` follows ayaml into what [`neutralize::places`]
/// reads as a scalar's text only where the oracle rejects the input, so
/// that ayaml reading a tag in a plain scalar that the oracle reads is not
/// taken for the oracle reading the tag as `!`.
#[test]
fn a_tag_ayaml_reads_in_a_plain_scalar_is_not_a_nameless_verbatim_tag() {
    let input = "x\n!<!> a\n";
    let (clean, case) = planted::clean_and_planted(input, Entry::Value, verbatim_tag_in_text);
    assert_eq!(clean.class, Class::SameValue, "{clean:?}");
    assert_eq!(case.class, Class::OnlyAyamlRejects, "{case:?}");
    assert_eq!(
        planted::planted_explanation(&case, verbatim_tag_in_text),
        None,
        "{case:?}"
    );
}

/// The oracle's error about a character it does not allow, which its reader
/// gives before its scanner reads a token, is no location class's but the
/// one about that character: ayaml's unterminated quoted scalar is another
/// error, which the case shows once the character is gone; so is its error
/// about an `x` in a block scalar's header, a character YAML allows.
#[test]
fn a_location_class_does_not_take_the_oracles_reader_error() {
    let (case, _) = compare("a: 'x\n  \u{7f}", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert_eq!(
        explanation_without(&case, &["quoted-only-characters"]),
        None
    );
    assert_eq!(
        explanation(&case),
        Some(vec!["quoted-only-characters", "where-a-construct-opened"])
    );
    let (case, _) = compare("- '\u{7f}'\n- |x\n", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert_eq!(
        explanation_without(&case, &["quoted-only-characters"]),
        None
    );
    assert_eq!(explanation(&case), Some(vec!["quoted-only-characters"]));
}

/// Two errors at two places are two when the prefix through the earlier
/// one's line holds it alone, and each is then explained on its own: a
/// location class does not take ayaml's verbatim-tag error for the oracle's
/// flow error, or ayaml's comment error for the oracle's error at the end.
/// The oracle's "more than one document", which has no place, is never
/// ayaml's error found elsewhere when the oracle read ayaml's error's line.
/// Where the rest of the input is no case of its own, past a construct cut
/// in two, the prefix through the line alone still shows two errors: one
/// that holds ayaml's error as it is explains ayaml's result, and the
/// oracle's error past it is in text ayaml never read; one that holds the
/// oracle's, which ayaml reads, leaves no location class.
#[test]
fn two_errors_at_two_places_are_explained_each_on_its_own() {
    for (input, id, both) in [
        (
            "c: !<!> x\nm: [:a]\n",
            "verbatim-non-specific-tag",
            &["verbatim-non-specific-tag", "yaml-1.2-flow-syntax"][..],
        ),
        (
            "m: [:a]\nc: !<!> x\n",
            "verbatim-non-specific-tag",
            &["yaml-1.2-flow-syntax", "verbatim-non-specific-tag"],
        ),
        (
            "a: 'x'#c\nb",
            "comment-needs-white-space",
            &["comment-needs-white-space"],
        ),
        (
            "c: !<!> x\n---\nb: 2\n",
            "verbatim-non-specific-tag",
            &["verbatim-non-specific-tag"],
        ),
        (
            "'a'#c\n  x: 1\ny\n",
            "comment-needs-white-space",
            &["comment-needs-white-space"],
        ),
        (
            "- 'a'#c\n  x\ny\n",
            "comment-needs-white-space",
            &["comment-needs-white-space"],
        ),
        (
            "|\n  \nx\n",
            "block-scalar-indentation",
            &["block-scalar-indentation"],
        ),
        (
            "- '\u{7f}'\n  x\ny\n",
            "quoted-only-characters",
            &["quoted-only-characters"],
        ),
        (
            "a: |2\n   0\n  - |\n\t\n 1\n",
            "tab-as-white-space",
            &["tab-as-white-space"],
        ),
    ] {
        let (case, _) = compare(input, Entry::Value);
        assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
        assert_eq!(explanation(&case).as_deref(), Some(both), "{input:?}");
        assert_eq!(explanation_without(&case, &[id]), None, "{input:?}");
    }
}

/// The first error of a case is ayaml's where the two are at one place, as
/// ayaml's at a character the oracle's reader refuses and its error at
/// 1:1; the oracle's where it comes first or ayaml read the input; and none
/// where ayaml's has no place, which could be anywhere.
#[test]
fn the_first_error_is_ayaml_s_where_the_two_are_at_one_place() {
    let (case, _) = compare("a: \u{7f}\n", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert_eq!(first_error(&case), Some((3, true)));
    let made_up = |ayaml: Side, oracle: Side| Case {
        input: "a\nb\n",
        entry: Entry::Value,
        rule: 0,
        class: classify(&ayaml, &oracle),
        ayaml,
        oracle,
    };
    let at = |line| Err(rejection("e", Some((line, 1))));
    assert_eq!(first_error(&made_up(at(2), at(1))), Some((0, false)));
    assert_eq!(first_error(&made_up(at(1), at(2))), Some((0, true)));
    assert_eq!(
        first_error(&made_up(Ok("x".to_string()), at(2))),
        Some((2, false))
    );
    assert_eq!(
        first_error(&made_up(Err(rejection("e", None)), at(1))),
        None
    );
}

/// The lines after a prefix read as they would alone where the prefix
/// holds no node, only blank lines and comments, or where no line after it
/// at the top is indented less than the first. Blank lines, comments and
/// lines inside a construct do not count.
#[test]
fn the_rest_stands_alone_unless_a_line_at_the_top_is_indented_less() {
    for (input, start, alone) in [
        ("# c\n\n  - a\nb\n", 5, true),
        ("- [x]\n  - a\nb\n", 6, false),
        ("- [x]\n  - a\n\n  - b\n", 6, true),
        ("- [x]\n  - a\n# c\n  - b\n", 6, true),
        ("- [x]\n  - [a,\n b]\n", 6, true),
    ] {
        let places = neutralize::places(input);
        assert!(at_the_top(input, &places, start), "{input:?}");
        assert_eq!(stands_alone(input, &places, start), alone, "{input:?}");
    }
}

/// Where the oracle's error is in a flow collection that ayaml read, and
/// ayaml's comes on a later line, the input with the collection empty, which
/// keeps its line breaks, stands for the rest: the oracle reads the lines
/// around it as ayaml read them in the case (seeds 4 and 10). A difference
/// after the collection still needs its own entry.
#[test]
fn an_empty_flow_collection_stands_for_one_only_the_oracle_rejects() {
    let search = || Search {
        tried: Vec::new(),
        ids: Vec::new(),
        comparisons: 0,
        complete: false,
    };
    for (input, prefix, rest) in [
        (
            "? {a:}\n%YAML 1.2\n\n.inf--- \n",
            "? {a:}\n",
            "? {}\n%YAML 1.2\n\n.inf--- \n",
        ),
        ("[:a]\n  b\nc\n", "[:a]\n", "[]\n  b\nc\n"),
        (
            "- m: [a,\n  :b]\n  c: !<!> x\n- d\n",
            "- m: [a,\n  :b]\n",
            "- m: []\n\n  c: !<!> x\n- d\n",
        ),
    ] {
        let (case, _) = compare(input, Entry::Value);
        let split = search().split(&case);
        let Split::Two(two, emptied) = &split else {
            panic!("{input:?}: {split:?}");
        };
        assert_eq!((two.input, emptied.as_str()), (prefix, rest), "{input:?}");
    }
    for input in ["? {a:}\n%YAML 1.2\n\n.inf--- \n", "[:a]\n  b\nc\n"] {
        let (case, _) = compare(input, Entry::Value);
        assert_eq!(
            explanation(&case),
            Some(vec!["yaml-1.2-flow-syntax", "after-the-root-node"]),
            "{case:?}"
        );
    }
    // Where ayaml read no collection, as past a NEL, its content to the
    // oracle, the oracle rejects the prefix with it empty too (seed 1).
    let input = "\u{85}  {18446744073709551616]\n    -2\u{2028} #c\n- 😀-a\n-   TRUE";
    let (case, _) = compare(input, Entry::Value);
    let split = search().split(&case);
    assert!(matches!(split, Split::Apart), "{split:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["yaml-1.2-line-breaks", "indentation"]),
        "{case:?}"
    );
    let (case, _) = compare("- m: [:a]\n  c: !<!> x\n- d\n", Entry::Value);
    assert_eq!(
        explanation(&case),
        Some(vec!["yaml-1.2-flow-syntax", "verbatim-non-specific-tag"]),
        "{case:?}"
    );
    assert_eq!(
        explanation_without(&case, &["verbatim-non-specific-tag"]),
        None
    );
    // Where ayaml's error changes with the collection empty, as an alias of
    // an anchor in it, the collection does not stand for it.
    let (case, _) = compare("- m: [&x a,\n  :b]\n  c: *x\n  d: *y\n- e\n", Entry::Value);
    let mut changed = search();
    let split = changed.split(&case);
    assert!(matches!(split, Split::Apart), "{split:?}");
    assert_eq!(changed.comparisons, 3, "{case:?}");
    // The split takes two comparisons after the prefix's, within the budget.
    let (case, _) = compare("[:a]\n  b\nc\n", Entry::Value);
    for (left, two) in [(3, true), (2, false), (1, false)] {
        let mut budget = search();
        budget.comparisons = MAX_COMPARISONS - left;
        let split = budget.split(&case);
        assert_eq!(matches!(split, Split::Two(..)), two, "{left}: {split:?}");
        assert!(budget.comparisons <= MAX_COMPARISONS, "{left}");
    }
}

/// ayaml reports a second document only once the first one's values hold
/// no error, so a rewrite that removes a value error and shows a second
/// document hides nothing, where a syntax error a rewrite makes still
/// hides one (seeds 9 and 12).
#[test]
fn a_second_document_hides_no_value_error() {
    let (case, _) = compare("a: !!int x\n---\nb\n", Entry::Value);
    assert!(
        case.ayaml.as_ref().is_err_and(ayaml_value_error),
        "{case:?}"
    );
    let (next, _) = compare("a: x\n---\nb\n", Entry::Value);
    assert!(
        next.ayaml
            .as_ref()
            .is_err_and(|rejection| rejection.message.contains(AYAML_SECOND_DOCUMENT)),
        "{next:?}"
    );
    assert_eq!(
        fairness(&case, &next, &Hidden::Nothing),
        Some(Fairness::Kept)
    );
    let (broken, _) = compare("a: [x\n---\nb\n", Entry::Value);
    assert!(
        broken.ayaml.as_ref().is_err_and(ayaml_syntax_error),
        "{broken:?}"
    );
    assert_eq!(fairness(&case, &broken, &Hidden::Nothing), None);
}

/// A search counts each comparison out of its budget (see
/// [`MAX_COMPARISONS`]), and a split compares each prefix once and never
/// the whole input, which the search already compared. An error on the last
/// line leaves no prefix; a construct that ends with the error's line
/// leaves one; one that goes on past it, and whose prefix shows nothing,
/// leaves the prefix through the line too; one that goes on to the end of
/// the input leaves only that. A flow collection the oracle's error is in
/// adds the prefix through it and the input with it empty.
#[test]
fn a_search_counts_each_comparison() {
    let search = || Search {
        tried: Vec::new(),
        ids: Vec::new(),
        comparisons: 0,
        complete: false,
    };
    for (input, comparisons, shown) in [
        ("a: !<!> x\n", 0, "One"),
        ("a: |\n x\n\t y\nb\n", 1, "One"),
        ("a: [x, 'y'#c,\n b]\nc: 1\n", 2, "One"),
        ("-\n|2\n  x\n", 1, "Through"),
        ("[:a]\n  b\nc\n", 3, "Two"),
    ] {
        let (case, _) = compare(input, Entry::Value);
        let mut search = search();
        let split = search.split(&case);
        assert!(
            format!("{split:?}").starts_with(shown),
            "{input:?}: {split:?}"
        );
        assert_eq!(search.comparisons, comparisons, "{input:?}");
    }
    // The line before the error, which both read alike.
    let (case, _) = compare("a: 1\nb: !<!> x\n", Entry::Value);
    let mut before = search();
    assert!(before.explain_before(&case));
    assert_eq!(before.comparisons, 1);
    // The rest after a prefix, which both read alike.
    let (case, _) = compare("a: 'x'#c\nb: 1\n", Entry::Value);
    let mut rest = search();
    assert!(rest.explain_rest(&case, &after_prefix(case.input, "a: 'x'#c\n".len())));
    assert_eq!(rest.comparisons, 1);
    // Each rewrite, of which the first explains the case.
    let (case, _) = compare("a: !<!> x\n", Entry::Value);
    let deviation = allowlist::DEVIATIONS
        .iter()
        .find(|deviation| deviation.id == "verbatim-non-specific-tag")
        .unwrap();
    let rewrites = ["a: !<!t> x\n".to_string(), "a: x\n".to_string()];
    let mut rewritten = search();
    assert!(rewritten.explain_rewrites(&case, deviation, &rewrites, &mut false));
    assert_eq!(rewritten.comparisons, 2);
    // A prefix and the rest, each explained, the prefix by a rewrite.
    let (case, _) = compare("c: !<!> x\nm: [:a]\n", Entry::Value);
    let Split::Two(prefix, rest) = search().split(&case) else {
        panic!("{case:?}");
    };
    let mut prefix_alone = search();
    assert!(prefix_alone.explain(&prefix));
    let mut rest_alone = search();
    assert!(rest_alone.explain_rest(&case, &rest));
    let mut both = search();
    assert!(both.explain_both(&case, &prefix, &rest));
    assert!(prefix_alone.comparisons > 0);
    assert_eq!(
        both.comparisons,
        prefix_alone.comparisons + rest_alone.comparisons
    );
}

/// Once a rewrite revealed an error that the case's error hid, only a case
/// both parsers read alike ends the search: a narrow entry explains the
/// case's error, not the text after the revealed one.
#[test]
fn a_narrow_entry_does_not_end_a_search_past_a_revealed_error() {
    let (case, _) = compare("[-]\n", Entry::Value);
    assert_eq!(case.class, Class::OnlyAyamlRejects);
    let explained = |complete| {
        let mut search = Search {
            tried: Vec::new(),
            ids: Vec::new(),
            comparisons: 0,
            complete,
        };
        search.explain(&case).then_some(search.ids)
    };
    assert_eq!(
        explained(false),
        Some(vec!["flow-indicator-needs-white-space"])
    );
    assert_eq!(explained(true), None);
}

/// A rewrite that turns a value error of ayaml's into a scanner error hides
/// the value error, wherever each is (ayaml scans all tokens first), and any
/// syntax error (including a newly introduced parser error or second document)
/// hides a Serde decode error. Document-building errors stream with the parser
/// (ayaml 0.2.2), so uncovering a pre-existing parser error at the same offset
/// is kept while a newly introduced parser error is unfair. Without the
/// core-tag entry, the `!!int` on a key that ayaml refuses does not stand for
/// the line separator, which the line-break rewrite turns into a break that
/// ends the quoted scalar.
#[test]
fn a_rewrite_must_not_trade_a_value_error_for_a_syntax_error() {
    for input in [
        "!!int x: 1\ny: 'd\u{2029}e'\n",
        "!!int x: 1\ny: \"d\u{2029}\\\n  e\"\n",
    ] {
        let (case, _) = compare(input, Entry::Json);
        assert!(case.class.is_difference(), "{case:?}");
        assert!(explanation(&case).is_some(), "{case:?}");
        assert_eq!(
            explanation_without(&case, &["core-tag-on-another-kind"]),
            None,
            "{case:?}"
        );
    }
    // A later parser error or second document hides an earlier Serde decode
    // error, so trading a Serde error for either is unfair.
    let (serde_case, _) = compare("version: bad\nb: 1\n", Entry::Config);
    assert_eq!(
        serde_case.ayaml.as_ref().unwrap_err().kind,
        Some(ErrorKind::Serde),
        "{serde_case:?}"
    );
    let (parser_broken, _) = compare("version: bad\nb: [1, 2}\n", Entry::Config);
    assert_eq!(
        parser_broken.ayaml.as_ref().unwrap_err().kind,
        Some(ErrorKind::Parser),
        "{parser_broken:?}"
    );
    assert_eq!(
        fairness(&serde_case, &parser_broken, &Hidden::Nothing),
        None
    );
    let (second_doc_broken, _) = compare("version: bad\n---\nb: 1\n", Entry::Config);
    assert!(
        second_doc_broken
            .ayaml
            .as_ref()
            .is_err_and(|rejection| rejection.message.contains(AYAML_SECOND_DOCUMENT)),
        "{second_doc_broken:?}"
    );
    assert_eq!(
        fairness(&serde_case, &second_doc_broken, &Hidden::Nothing),
        None
    );
    // Document-building errors stream with the parser (ayaml 0.2.2): removing
    // a resolution error may uncover a pre-existing parser error at its exact
    // offset, while a newly introduced parser error is unfair.
    let (res_with_preexisting, _) = compare("a: !!int x\nb: [1, 2}\n", Entry::Value);
    assert_eq!(
        res_with_preexisting.ayaml.as_ref().unwrap_err().kind,
        Some(ErrorKind::Resolution),
        "{res_with_preexisting:?}"
    );
    let (uncovered_parser, _) = compare("a: x\nb: [1, 2}\n", Entry::Value);
    assert_eq!(
        uncovered_parser.ayaml.as_ref().unwrap_err().kind,
        Some(ErrorKind::Parser),
        "{uncovered_parser:?}"
    );
    assert_eq!(
        fairness(&res_with_preexisting, &uncovered_parser, &Hidden::Nothing),
        Some(Fairness::Kept)
    );
    let (res_clean, _) = compare("a: !!int x\nb: 1\n", Entry::Value);
    assert_eq!(
        fairness(&res_clean, &uncovered_parser, &Hidden::Nothing),
        None
    );
    let (later_res, _) = compare("a: 1\nb: !!int x\n", Entry::Value);
    let (new_earlier_parser, _) = compare("a: [1, 2}\nb: !!int x\n", Entry::Value);
    assert_eq!(
        fairness(&later_res, &new_earlier_parser, &Hidden::Nothing),
        None
    );
    // A multi-document stream is NOT accepted as single-document syntax by
    // ayaml even when `stream_events` parses both documents cleanly.
    let (multi_doc_res, _) = compare("a: !!int x\n---\nb: 1\n", Entry::Value);
    assert_eq!(one_rejects_the_syntax(&multi_doc_res), None);
    // DuplicateKey errors count as value errors and match the oracle's
    // "duplicate entry" errors even when worded "duplicate mapping key"
    // rather than compat's "duplicate entry".
    let (dup, _) = compare("a: 1\na: 2\n", Entry::Value);
    let (Err(ayaml_dup), Err(oracle_dup)) = (&dup.ayaml, &dup.oracle) else {
        panic!("{dup:?}");
    };
    assert!(value_error(ayaml_dup), "{ayaml_dup:?}");
    assert!(same_value_error(ayaml_dup, oracle_dup), "{dup:?}");
    let raw_dup = Rejection {
        message: "duplicate mapping key: \"a\"".to_string(),
        location: Some((1, 1)),
        kind: Some(ErrorKind::DuplicateKey),
    };
    assert!(value_error(&raw_dup), "{raw_dup:?}");
    assert!(same_value_error(&raw_dup, oracle_dup), "{raw_dup:?}");
}

/// The flow-syntax entry covers an error inside a flow collection only at
/// one of its forms: a key that LS breaks, for the oracle, is not one.
#[test]
fn the_flow_entry_needs_its_form() {
    for input in ["[a\u{2028}: b]\n", "{a\u{2028}b: c}\n"] {
        let (case, _) = compare(input, Entry::Value);
        assert_eq!(case.class, Class::OnlyOracleRejects, "{case:?}");
        assert_eq!(
            explanation_without(&case, &["yaml-1.2-line-breaks"]),
            None,
            "{case:?}"
        );
    }
}

/// The core-tag rewrite that replaces only the tags on empty nodes keeps
/// the kinds the other tags give: `!!str` keeps a number a string, which
/// the JSON entry point would reject as out of range in both parsers, an
/// error the rewrite would make.
#[test]
fn a_core_tag_rewrite_keeps_the_kinds_other_tags_give() {
    let (case, _) = compare("- !!null \n- !!str 18446744073709551616\n", Entry::Json);
    assert_eq!(case.class, Class::OnlyOracleRejects, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["core-tag-on-another-kind"]),
        "{case:?}"
    );
}

/// How fair a rewrite is when a parser's error moves, on made-up results:
/// an alias error is kept wherever it moved; an error that moved to an
/// earlier line for another reason was revealed, unless the parser that
/// rejected the case kept its message and both now reject at one place,
/// where the two agree, or the trigger is documented to hide the error now
/// shown. The inputs after are cases of the long runs that each rule
/// explains.
#[test]
fn a_moved_error_is_kept_revealed_or_agreed_on() {
    let error = |message: &str, line, kind| -> Side {
        Err(Rejection {
            message: message.to_string(),
            location: Some((line, 1)),
            kind,
        })
    };
    let case = |ayaml: Side, oracle: Side| Case {
        input: "a\nb\nc\n",
        entry: Entry::Value,
        rule: 0,
        class: classify(&ayaml, &oracle),
        ayaml,
        oracle,
    };
    let scanner = Some(ErrorKind::Scanner);
    let both = case(error("A", 2, scanner), error("X", 3, None));
    // The oracle's error moved up a line, and ayaml kept its own.
    let agreed = case(error("A", 2, scanner), error("Y", 2, None));
    assert_eq!(
        fairness(&both, &agreed, &Hidden::Nothing),
        Some(Fairness::BothAgree)
    );
    // As above, but ayaml's error changed: nothing agrees.
    let changed = case(error("B", 2, scanner), error("Y", 2, None));
    assert_eq!(
        fairness(&both, &changed, &Hidden::Nothing),
        Some(Fairness::Revealed)
    );
    let read = case(Ok("x".to_string()), error("X", 3, None));
    let alias = case(Ok("x".to_string()), error("unknown anchor", 1, None));
    assert_eq!(
        fairness(&read, &alias, &Hidden::Nothing),
        Some(Fairness::Kept)
    );
    let other = case(Ok("x".to_string()), error("Y", 1, None));
    assert_eq!(
        fairness(&read, &other, &Hidden::Nothing),
        Some(Fairness::Revealed)
    );
    // ayaml's scanner error gave way to a parser error two lines up, which
    // only a trigger documented to hide parser errors may have hidden.
    let scanner_error = case(error("A", 3, scanner), error("X", 3, None));
    let parsed = case(error("B", 1, Some(ErrorKind::Parser)), error("X", 3, None));
    assert_eq!(
        fairness(&scanner_error, &parsed, &Hidden::Nothing),
        Some(Fairness::Revealed)
    );
    assert_eq!(
        fairness(&scanner_error, &parsed, &Hidden::ParserErrors),
        Some(Fairness::Kept)
    );
    // A parser error the case gave, or a scanner error now, was not hidden
    // by a scanner error.
    let parser_error = case(error("A", 3, Some(ErrorKind::Parser)), error("X", 3, None));
    let moved_scanner_error = case(error("B", 1, scanner), error("X", 3, None));
    for (before, after) in [
        (&parser_error, &parsed),
        (&scanner_error, &moved_scanner_error),
    ] {
        assert_eq!(
            fairness(before, after, &Hidden::ParserErrors),
            Some(Fairness::Revealed)
        );
    }
    for (input, entry, ids) in [
        (
            "-9223372036854775809 # \n\t",
            Entry::Json,
            &["tab-as-white-space"][..],
        ),
        (
            "&a1 [*a1,\n  *a'1]\n",
            Entry::Value,
            &["anchor-name-characters", "values-after-syntax"],
        ),
    ] {
        let (case, _) = compare(input, entry);
        assert_eq!(explanation(&case), Some(ids.to_vec()), "{case:?}");
    }
}

/// A value error and a syntax error at one place are two errors: ayaml's
/// duplicate key of the root mapping and the oracle's control character
/// are both reported at 1:1, and the control character stays the quoted
/// scalar's difference.
#[test]
fn a_value_error_and_a_syntax_error_at_one_place_differ() {
    for input in ["a: 1\na: 2\nb: '\u{7f}'\n", "b: '\u{7f}'\na: 1\na: 2\n"] {
        let (case, _) = compare(input, Entry::Value);
        assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
        assert_eq!(
            explanation(&case),
            Some(vec![UNEXPLAINED_ENTRY]),
            "{case:?}"
        );
        assert_eq!(explanation_without(&case, &[UNEXPLAINED_ENTRY]), None);
    }
    // ayaml 0.2.2 streams parser events into value construction, so an earlier
    // alias or value error stops before a later parser error that both parsers
    // reject; a control character still needs its own entry.
    let (case, _) = compare("*b\t? {", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["values-after-syntax"]),
        "{case:?}"
    );
    let (case, _) = compare("*b? {'\u{7f}'-.Inf", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["quoted-only-characters", "values-after-syntax"]),
        "{case:?}"
    );
    assert_eq!(
        explanation_without(&case, &["quoted-only-characters"]),
        None
    );
    // Removing a `!!null` tag that caused a streaming resolution error may
    // uncover a pre-existing parser error later in the same document.
    let input = "---\na: \n<<:\n  k1: &a1 0.5\n  null: !!null |2\n  null: !!null |2\n      line 0\n\t\n    line 1\n---\nb: 2\n";
    let (case, _) = compare(input, Entry::Value);
    assert!(explanation(&case).is_some(), "{case:?}");
    assert_eq!(
        explanation_without(&case, &["core-tag-on-another-kind"]),
        None,
        "{case:?}"
    );
    let seed4 = format!(
        "&a1 k1:\n  -\n    <<: &a2 !!null >\n    <<: &a2 !!null >\n      line 0\n\
         \x20 - |\n    line 0\n    line 1\n    line 2\n\n\n  -\n\
         \x20   - \"d\\N\\u00e9\\t\"\n    - true\nkey: {}'q'\n...\n",
        "'q' ".repeat(256)
    );
    let (case, _) = compare(&seed4, Entry::Value);
    assert!(explanation(&case).is_some(), "{case:?}");
    assert_eq!(
        explanation_without(&case, &["core-tag-on-another-kind"]),
        None,
        "{case:?}"
    );
}

/// A lone carriage return ends a line for the predicates as it does for
/// both parsers: after one, a tag with `#` is explained as after a line
/// feed, and the number-sign entry does not explain the block scalar after
/// it that it does not cause.
#[test]
fn a_carriage_return_ends_a_line_for_the_predicates() {
    for input in ["x: a\nb: !t# y\n", "x: a\rb: !t# y\n"] {
        let (case, _) = compare(input, Entry::Value);
        assert!(case.class.is_difference(), "{case:?}");
        assert_eq!(
            explanation(&case),
            Some(vec!["number-sign-in-a-tag"]),
            "{case:?}"
        );
    }
    let (case, _) = compare("x: a\rb: !t# &x\n>\n", Entry::Value);
    assert!(case.class.is_difference(), "{case:?}");
    assert_eq!(
        explanation_without(&case, &["number-sign-in-a-tag"]),
        None,
        "{case:?}"
    );
}

/// A redefined `!!` makes `!!int` a global tag, which the core-tag entries
/// leave alone.
#[test]
fn a_redefined_secondary_handle_is_a_global_tag() {
    let (case, _) = compare("%TAG !! tag:x,2000:\n---\na: !!int 1\n", Entry::Value);
    assert_eq!(case.class, Class::DifferentValue);
    assert_eq!(explanation(&case), Some(vec!["global-tags"]));
    let (case, _) = compare("a: !<!> x\n", Entry::Value);
    assert_eq!(explanation(&case), Some(vec!["verbatim-non-specific-tag"]));
}

/// A core tag in a typed decode is its own entry, which needs no difference
/// in the value trees; the value-tree entry does not cover a typed decode.
#[test]
fn a_core_tag_in_a_typed_decode_is_its_own_entry() {
    for input in [
        "version: !!str 1\n",
        "version: !!float 1\n",
        "version: 1\nrespect_gitignore: !!str true\n",
        "version: !!int '1'\n",
    ] {
        let (value, _) = compare(input, Entry::Value);
        assert_eq!(value.class, Class::SameValue, "{value:?}");
        let (case, _) = compare(input, Entry::Config);
        assert!(case.class.is_difference(), "{case:?}");
        assert_eq!(explanation(&case), Some(vec!["typed-decode-core-tags"]));
        let core = allowlist::DEVIATIONS
            .iter()
            .find(|deviation| deviation.id == "core-tag-on-another-kind")
            .unwrap();
        assert!(!core.covers(&case));
    }
}

/// A line separator, which Rust takes for white space, is three bytes wide:
/// the rewrites and predicates that look back over white space before a
/// property or a flow collection step over all of it. YAML 1.2 reads it as
/// text, so `\u{2028}&a` here is no property.
#[test]
fn white_space_wider_than_a_byte_is_stepped_over() {
    assert_eq!(
        neutralize::flow_colon_scalars("[\u{2028}&a :b]\n"),
        Vec::<String>::new()
    );
    let input = "a: [&b\u{2028}:[c]]\n";
    let at = input.find(":[").unwrap() + 1;
    assert!(!predicates::flow_value_after_a_property_at(input, at));
    assert!(!predicates::flow_value_after_a_property_at(
        "a\u{2028}[b]",
        4
    ));
}

/// ayaml 0.2.2 gives its alias errors the kind Composition, and none the
/// kind Alias: a rewrite may move one to where its node starts, as the
/// oracle's.
#[test]
fn ayaml_s_alias_errors_are_known_by_their_messages() {
    for input in ["a: [&a [*a]]\n", "a: *b\n"] {
        let (case, _) = compare(input, Entry::Value);
        let (Err(ayaml), Err(oracle)) = (&case.ayaml, &case.oracle) else {
            panic!("{case:?}");
        };
        assert_eq!(ayaml.kind, Some(ayaml::ErrorKind::Composition), "{case:?}");
        assert!(alias_error(ayaml) && alias_error(oracle), "{case:?}");
    }
    let (case, _) = compare("a: {b: 1, b: 2}\n", Entry::Value);
    let Err(ayaml) = &case.ayaml else {
        panic!("{case:?}");
    };
    assert!(!alias_error(ayaml), "{case:?}");
}

/// ayaml's errors about a block scalar's indentation are its own, by YAML
/// 1.2, where the oracle reads the scalar otherwise and rejects what
/// follows: the entry explains such a case both reject only when ayaml's
/// error comes first. A document marker after indented empty lines ends a
/// block scalar in ayaml 0.2.2 too (asamarts/ayaml#146).
#[test]
fn a_block_scalar_error_is_ayaml_s_own_only_before_the_oracle_s() {
    let deviation = allowlist::DEVIATIONS
        .iter()
        .find(|deviation| deviation.id == "block-scalar-indentation")
        .unwrap();
    let (case, _) = compare("|\n  \nx\n", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert!(deviation.covers(&case));
    // The oracle's error, at the character it does not allow, is first.
    let (case, _) = compare("a: '\u{7f}'\nb: |\n   \n  x\n", Entry::Value);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert!(!deviation.covers(&case));
    // Both top-level and nested block scalars end at a document marker after
    // indented empty lines.
    for input in ["|\n  \n...\n", "a: |\n   \n...\n"] {
        let (case, _) = compare(input, Entry::Value);
        assert_eq!(case.class, Class::SameValue, "{case:?}");
    }
    for input in ["|\n  \n---\nb\n", "--- |\n  \n--- x\n"] {
        let (case, _) = compare(input, Entry::Value);
        assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
        assert!(!deviation.covers(&case), "{case:?}");
        assert_eq!(
            explanation(&case),
            Some(vec!["after-the-root-node"]),
            "{case:?}"
        );
    }
}

/// Each syntax error message of ayaml 0.2.2, the version the workspace pins,
/// with sample values in place of its format arguments, as compatibility
/// mode reports it.
///
/// The list is every string literal and `format!` template that ayaml's
/// scanner and parser pass as an error message (to `Scanner::error_at`,
/// `Parser::error`, `expect_consumed`, `flow_error`'s `FlowLack`,
/// `compat_indentation_message` and `non_printable_error`), and the messages
/// `compat::map_error` puts in their place, outside their tests:
/// `src/scanner.rs`, `src/scanner/*.rs`, `src/parser.rs`, `src/parser/*.rs`
/// and `src/lib.rs`. Messages that compatibility mode replaces are left out
/// ("unexpected token in mapping value" becomes "mapping values are not
/// allowed in this context"), and so are internal errors no input reaches
/// ("invalid span position", "invalid character boundary"). Refresh the list
/// the same way when the pin moves.
const AYAML_SYNTAX_MESSAGES: &[&str] = &[
    "%TAG requires a handle and a prefix",
    "'@' is a reserved indicator and cannot start a plain scalar",
    "a `%` in a verbatim tag must start a two-digit escape",
    "a block collection cannot start on a `---` line",
    "a block entry is indented less than its parent",
    "a block scalar header comment requires preceding whitespace",
    "a block scalar is not indented past its parent",
    "a byte order mark can only start a document",
    "a comment must be separated from the preceding token",
    "a directive cannot contain a byte order mark",
    "a directive needs a name",
    "a document's first node cannot be indented",
    "a document-start property cannot precede a same-line block mapping key",
    "a flow collection's `?` must be separated from what follows it",
    "a mapping line is missing a `:`",
    "a node cannot have duplicate tag or anchor properties",
    "a tab cannot indent a flow collection's line under a block collection",
    "a tab cannot indent an empty line inside a scalar under a block collection",
    "a tab cannot start a comment after a block scalar",
    "a tag must be followed by whitespace or a line break",
    "a tag's handle needs a suffix after it",
    "a verbatim tag may only hold URI characters; escape others as %XX",
    "a verbatim tag must name a local tag or a tag with a scheme",
    "a verbatim tag needs a name",
    "aliases cannot carry tags or anchors",
    "an anchor name cannot contain a byte order mark",
    "an anchor or alias must be followed by white space",
    "an implicit mapping key may be at most 1024 characters long",
    "anchor/alias name must not be empty",
    "block scalar header must end at a line break",
    "content after document end marker",
    "did not find expected ',' or ']', while parsing a flow sequence",
    "did not find expected node content, while parsing a flow mapping",
    "directive must be followed by a document start",
    "directive without a document",
    "document marker inside quoted scalar",
    "document markers are not allowed inside flow collections",
    "duplicate %TAG directive for !e!",
    "duplicate %YAML directive",
    "escape is not a Unicode scalar value",
    "expected ',' or a closing bracket",
    "expected a flow item",
    "expected a node",
    "expected a token",
    "explicit mapping key must be followed by a ':' line",
    "flow collection entries must be indented",
    "found character that cannot start any token",
    "implicit mapping keys must be on one line",
    "invalid %-escape in tag",
    "invalid %TAG handle e",
    "invalid %TAG prefix x",
    "invalid %YAML directive",
    "invalid hexadecimal escape",
    "leading empty block-scalar lines are more indented than the content",
    "mapping values are not allowed in this context",
    "more than one document in stream",
    "non-printable character U+0001 is not allowed in YAML",
    "properties cannot precede a compact block sequence",
    "properties cannot precede an explicit mapping key",
    "quoted scalar continuation must be indented more than its parent",
    "short hexadecimal escape",
    "tab cannot separate a block structural indicator",
    "tabs are not allowed as leading block-scalar indentation",
    "tabs cannot be the only flow-collection indentation",
    "tabs cannot be the only quoted-scalar indentation",
    "tag %-escapes do not form valid UTF-8",
    "this line is indented differently from the mapping at line 1 column 1",
    "undefined primary tag handle",
    "undefined secondary tag handle",
    "undefined tag handle \"!e!\"",
    "unexpected ] in node position",
    "unexpected content after document",
    "unexpected empty flow mapping entry",
    "unexpected empty flow sequence entry",
    "unexpected '@' in block scalar header",
    "unexpected scalar in node position",
    "unexpected token after node",
    "unexpected token in mapping key",
    "unknown escape \\q",
    "unterminated double-quoted scalar",
    "unterminated escape",
    "unterminated single-quoted scalar",
    "unterminated verbatim tag",
];

/// Every syntax message has a place: a location class's, or, for an error
/// that is ayaml's own wherever the oracle's is, the place of the construct
/// of the input-wide entry that covers it, whose rewrite shows the oracle's
/// error.
#[test]
fn every_ayaml_syntax_message_has_a_location_class() {
    for message in AYAML_SYNTAX_MESSAGES {
        let located = [ErrorKind::Parser, ErrorKind::Scanner, ErrorKind::Tag]
            .into_iter()
            .any(|kind| allowlist::locate(kind, message).is_some());
        let case = Case {
            input: "",
            entry: Entry::Value,
            rule: 0,
            class: Class::RejectedAtDifferentLocation,
            ayaml: Err(rejection(message, Some((1, 1)))),
            oracle: Err(rejection("e", Some((2, 1)))),
        };
        let owned = allowlist::DEVIATIONS.iter().any(|deviation| {
            matches!(deviation.trigger, Trigger::Input { .. }) && deviation.covers(&case)
        });
        assert!(
            located != owned,
            "{message:?}: located {located}, owned {owned}"
        );
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
        rule: 0,
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
        rule: 0,
        class: Class::RejectedAtDifferentLocation,
        ayaml: Err(rejection("a new message", Some((1, 4)))),
        oracle: Err(rejection("e", Some((2, 1)))),
    };
    assert_eq!(explanation(&location), None);
    report.record(&location, false);
    let alike = Case {
        input: "a\n",
        entry: Entry::Value,
        rule: 0,
        class: Class::SameValue,
        ayaml: Ok("a".to_string()),
        oracle: Ok("a".to_string()),
    };
    assert_eq!(explanation(&alike), Some(vec![]));
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
        rule: 0,
        class: Class::RejectedAtDifferentLocation,
        ayaml: Err(rejection("unterminated single-quoted scalar", Some((1, 1)))),
        oracle: Err(rejection("e", Some((2, 1)))),
    };
    assert_eq!(explanation(&case), Some(vec!["where-a-construct-opened"]));
    let mut report = Report::default();
    report.record(&case, false);
    assert_eq!(report.failure_count, 0);
    assert_eq!(
        report
            .explained
            .get(&(Entry::Value, "where-a-construct-opened")),
        Some(&1)
    );
    assert!(
        report
            .markdown("t")
            .contains("`where-a-construct-opened`, Value: `` \"'\\n\" ``")
    );
}

#[test]
fn a_panic_is_caught_and_reported() {
    let (side, panicked): (Side, bool) = guarded(|| panic!("boom"));
    assert!(panicked);
    assert_eq!(side.unwrap_err().message, "panic: boom");
    let (side, panicked) = guarded(|| Ok("a".to_string()));
    assert!(!panicked);
    assert_eq!(side.unwrap(), "a");
}

#[test]
fn the_oracle_message_drops_every_location() {
    let error = yaml_serde::from_str::<yaml_serde::Value>("a: [\n").unwrap_err();
    let rejection = oracle_rejection(&error);
    assert!(rejection.location.is_some());
    assert!(
        !rejection.message.contains(" at line "),
        "{}",
        rejection.message
    );
    assert_eq!(
        crate::yaml_backend::without_marks(
            "found unexpected ':' at line 1 column 3, while scanning a plain scalar at line 1 \
             column 2"
        ),
        "found unexpected ':', while scanning a plain scalar"
    );
    assert_eq!(
        crate::yaml_backend::without_marks("control characters are not allowed at position 4"),
        "control characters are not allowed"
    );
    assert_eq!(
        crate::yaml_backend::without_marks("a at b at line x"),
        "a at b at line x"
    );
    let (case, _) = compare("[:a]\n", Entry::Value);
    assert_eq!(
        describe(&case.oracle),
        "error at 1:2: did not find expected node content, while parsing a flow node"
    );
}

/// An entry lists only the classes its check can cover: the tab entry needs
/// the oracle to reject, so a tab in a case both read is not its to explain.
#[test]
fn the_tab_entry_lists_only_what_it_covers() {
    let tab = allowlist::DEVIATIONS
        .iter()
        .find(|deviation| deviation.id == "tab-as-white-space")
        .unwrap();
    assert_eq!(tab.classes, [Class::OnlyOracleRejects]);
    let (case, _) = compare("- a\n-\tb\n", Entry::Value);
    assert!(case.oracle.is_err() && tab.covers(&case), "{case:?}");
    let (case, _) = compare("a: \"x\ty\"\nb: 'x\ty'\n", Entry::Value);
    assert!(case.oracle.is_ok() && !tab.covers(&case), "{case:?}");
}

/// A place in a rewrite maps back to the input: before, between and after
/// the edits, and inside an edit to where it starts.
#[test]
fn offsets_map_back_through_a_rewrite() {
    let input = "a: x\u{2029}\n|-\u{2029}";
    let rewritten = "a: x\n|-";
    assert_eq!(original_offset(input, rewritten, 2), 2);
    let bar = |text: &str| text.find('|').unwrap();
    assert_eq!(
        original_offset(input, rewritten, bar(rewritten)),
        bar(input)
    );
    assert_eq!(
        original_offset(input, rewritten, rewritten.len()),
        input.len()
    );
    // A tab replaced by two spaces: the second space was the tab.
    assert_eq!(original_offset("a\tb: c", "a  b: c", 2), 1);
    assert_eq!(original_offset("a\tb: c", "a  b: c", 4), 3);
    // An edit before the place moves it back.
    assert_eq!(original_offset("k: x\n- \u{85}y\n", "k: x\n- y\n", 7), 9);
}

/// Each rule is a case of its own, placed at its position in `rules:`.
#[test]
fn rules_decode_one_by_one() {
    let input = "version: 1\nrules:\n  - {id: a, kind: file_exists, level: error}\n  \
                 - {id: b, kind: file_absent}\n";
    assert_eq!(rule_cases(input).len(), 2);
    assert!(rule_cases("a: 1\n").is_empty());
    let (case, _) = compare_at(input, Entry::Rule, 0);
    assert_eq!(case.class, Class::SameValue, "{case:?}");
    let (case, _) = compare_at(input, Entry::Rule, 1);
    assert_eq!(case.class, Class::RejectedAtSameLocation, "{case:?}");
    assert_eq!(case.ayaml.unwrap_err().location, Some((2, 0)));
    let valid = input.replace("file_absent}", "file_absent, level: off}");
    let (case, _) = compare_at(&valid, Entry::Rule, 1);
    assert_eq!(case.class, Class::SameValue, "{case:?}");
    assert!(case.ayaml.unwrap().contains("kind: \"file_absent\""));
    let (case, _) = compare_at(&valid, Entry::Rule, 2);
    assert!(case.ayaml.is_err() && case.oracle.is_err(), "{case:?}");
    // The report counts one case per rule.
    let mut report = Report::default();
    report.add(&valid);
    let rules: usize = Class::ALL
        .iter()
        .map(|class| report.count(Entry::Rule, *class))
        .sum();
    assert_eq!(rules, 2);
}

/// A rule that both parsers reject hides no difference in a later one: the
/// second rule's key `1`, which only the oracle refuses, needs its entry
/// although the first rule's level fails both.
#[test]
fn a_rule_both_reject_hides_no_other() {
    let input = "version: 1\nrules:\n  - {id: z, kind: file_exists, level: Error}\n  \
                 - {id: a, kind: file_exists, level: error, paths: x, 1: y}\n";
    let (first, _) = compare_at(input, Entry::Rule, 0);
    assert_eq!(first.class, Class::RejectedAtSameLocation, "{first:?}");
    let (second, _) = compare_at(input, Entry::Rule, 1);
    assert_eq!(second.class, Class::OnlyOracleRejects, "{second:?}");
    assert_eq!(
        explanation(&second),
        Some(vec!["rule-keys-that-are-not-strings"])
    );
    assert_eq!(
        explanation_without(&second, &["rule-keys-that-are-not-strings"]),
        None
    );
}

/// Two errors for one rule, which both parsers place at the rule's position
/// only, are one only when they agree on the field and on the problem.
#[test]
fn rule_errors_are_one_in_one_field_with_one_problem() {
    // ayaml refuses the message, and the oracle the key `1`, before it
    // reads a field.
    let input = "rules:\n  - {id: a, kind: file_exists, level: error, paths: x, 1: y, \
                 message: [1]}\n";
    let (case, _) = compare_at(input, Entry::Rule, 0);
    assert_eq!(case.class, Class::RejectedAtDifferentLocation, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["rule-keys-that-are-not-strings"])
    );
    assert_eq!(
        explanation_without(&case, &["rule-keys-that-are-not-strings"]),
        None
    );
    // One problem in one field, which the two word differently, wherever
    // the field is in the mapping and whatever fields the rule needs.
    for input in [
        "rules:\n  - {id: a, kind: file_exists, level: error, when: 1}\n",
        "rules:\n  - {id: a, kind: file_exists, level: bad}\n",
        "rules:\n  - {id: [a], kind: 1, level: error}\n",
        "rules:\n  - {kind: file_exists, level: error}\n",
        "rules:\n  - {when: 1, level: error, id: a, kind: k, policy_url: 1}\n",
        "rules:\n  - {id: a, kind: k, level: error, paths: {include: 1}}\n",
        "rules:\n  - 1\n",
        // A mapping where an enum goes, which ayaml read as a variant's name
        // and data, as `level:` with the rule's next lines indented under it.
        "rules:\n  - {id: a, kind: k, level: {error: ~}}\n",
        "version: 1\nrules:\n  - id: a\n    kind: k\n    level:\n      paths: x\n",
        "rules:\n  - {id: a, kind: k, level: 1}\n",
    ] {
        let (case, _) = compare_at(input, Entry::Rule, 0);
        assert_eq!(case.class, Class::RejectedAtSameLocation, "{case:?}");
    }
    // The same problem in another field: ayaml's error, moved to the
    // message, or to `id`, a field the rule needs, which the oracle read.
    let to_message: planted::Plant = |_, _, side| {
        side.map_err(|rejection| Rejection {
            message: rejection.message.replacen("when: ", "message: ", 1),
            ..rejection
        })
    };
    let to_id: planted::Plant = |_, _, side| {
        side.map_err(|rejection| Rejection {
            message: rejection.message.replacen("kind: ", "id: ", 1),
            ..rejection
        })
    };
    for (input, moved, field) in [
        (
            "rules:\n  - {id: a, kind: k, level: error, message: x, when: 1}\n",
            to_message,
            "message: ",
        ),
        (
            "rules:\n  - {id: a, kind: [1], level: error}\n",
            to_id,
            "id: ",
        ),
    ] {
        let (clean, planted) = planted::clean_and_planted(input, Entry::Rule, moved);
        assert_eq!(clean.class, Class::RejectedAtSameLocation, "{clean:?}");
        assert!(
            planted
                .ayaml
                .as_ref()
                .unwrap_err()
                .message
                .starts_with(field)
        );
        assert_eq!(
            planted.class,
            Class::RejectedAtDifferentLocation,
            "{planted:?}"
        );
    }
}

/// A rule's entry explains the two errors it names, not the oracle's alone:
/// with ayaml's error about another field, as a regression would give, the
/// rule's case is left unexplained.
#[test]
fn a_rule_entry_needs_both_errors_it_names() {
    let other: planted::Plant = |_, _, side| {
        side.map_err(|rejection| Rejection {
            message: "kind: invalid type: integer `1`, expected a string".to_string(),
            ..rejection
        })
    };
    let input = "version: 1\nrules:\n  -\n";
    let (clean, planted) = planted::clean_and_planted(input, Entry::Rule, other);
    assert_eq!(
        explanation(&clean),
        Some(vec!["rule-that-is-null"]),
        "{clean:?}"
    );
    assert_eq!(
        planted.class,
        Class::RejectedAtDifferentLocation,
        "{planted:?}"
    );
    assert_eq!(
        planted::planted_explanation(&planted, other),
        None,
        "{planted:?}"
    );
}

#[test]
fn a_size_that_is_not_a_number_fails_the_run() {
    assert_eq!(number_setting("N", None, 7), 7);
    assert_eq!(number_setting("N", Some("300_000"), 7), 300_000);
    for value in ["1e5", "", "seven"] {
        let result = std::panic::catch_unwind(|| number_setting("N", Some(value), 7));
        assert!(result.is_err(), "{value:?}");
    }
}

/// The workflow of the long run watches every input of the run, cannot pass
/// without running it, and keeps a scheduled run apart from a dispatched one,
/// and each dispatched run apart from the others.
#[test]
fn the_long_run_workflow_watches_its_inputs() {
    let workflow =
        std::fs::read_to_string(repository().join(".github/workflows/yaml-differential.yml"))
            .unwrap();
    for path in [
        "\"**/*.yml\"",
        "\"**/*.yaml\"",
        "\"Cargo.toml\"",
        "\"Cargo.lock\"",
        // Every source the run reads: the parsers' glue, the configuration
        // types and what they hold, as facts, levels and compatibility.
        "\"crates/alint-core/src/**\"",
        "\"crates/alint-core/Cargo.toml\"",
    ] {
        assert!(
            workflow.contains(&format!("- {path}")),
            "{path} is not watched"
        );
    }
    let group = workflow
        .lines()
        .find(|line| line.trim_start().starts_with("group:"))
        .expect("a concurrency group");
    assert!(
        group.contains("github.event_name") && group.contains("github.run_id"),
        "the concurrency group: {group}"
    );
    assert!(
        workflow.contains("1 passed"),
        "a run that ran no test passes"
    );
    assert!(workflow.contains("long_run"));
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
    let ayaml = crate::yaml::ayaml_from_str::<crate::yaml::Value>(text).unwrap();
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
    let first = config_form(crate::yaml::ayaml_from_str(text).unwrap());
    let second = config_form(yaml_serde::from_str(text).unwrap());
    assert_eq!(first, second);
    assert!(
        first.contains(r#"vars: {"a": "1", "b": "2", "c": "3"}"#),
        "{first}"
    );
}
