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
        "a: '\u{7f}'\nrun: make && test\n",
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
    assert!(fairness(&case, &broken, &[]).is_none());
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

/// Triggers of the input-wide entries, as lines of a block mapping: among
/// them the review's repros, a NEL rewrite that makes ayaml reject, a
/// `!!str` the core-tag rewrite would drop onto another key, alone and after
/// a `!!null` on an empty node, which the oracle rejects, so that both
/// parsers would reject the rewrite at one place, and properties inside a
/// quoted scalar and a comment. A narrow entry's trigger is not one: it
/// explains a rejection, after which the parser that rejected reads nothing
/// to compare.
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
        "m: [!!str :c]\n",
        "m: [&x 1, &x 2, *x]\n",
    ]
    .map(String::from)
    .to_vec();
    triggers.push(format!("m: {{{}: v}}\n", "k".repeat(1_100)));
    triggers
}

/// No trigger explains a difference it does not cause: with the base's own
/// entry left out, each base with each trigger before or after it, or with
/// a byte-order mark or `...` in front, stays unexplained. This reaches each
/// tier of the search: rewrites a parser newly rejects, differences both
/// parsers agree on, errors that move, a value error in place of a syntax
/// error, and two errors a location class would take for one.
#[test]
fn a_trigger_masks_no_other_difference() {
    for (base, id) in MASKED {
        let (case, _) = compare(base, Entry::Value);
        assert!(case.class.is_difference(), "{case:?}");
        assert_eq!(explanation(&case), Some(vec![*id]), "{case:?}");
        assert_eq!(explanation_without(&case, &[id]), None, "{case:?}");
        let mut inputs: Vec<String> = masking_triggers()
            .iter()
            .flat_map(|trigger| [format!("{trigger}{base}"), format!("{base}{trigger}")])
            .collect();
        inputs.push(format!("\u{feff}{base}"));
        inputs.push(format!("...\n{base}"));
        let mut located = 0;
        for input in &inputs {
            let (case, panicked) = compare(input, Entry::Value);
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
/// where the two agree. The inputs after are cases of the long runs that
/// each rule explains.
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
    assert_eq!(fairness(&both, &agreed, &[]), Some(Fairness::BothAgree));
    // As above, but ayaml's error changed: nothing agrees.
    let changed = case(error("B", 2, scanner), error("Y", 2, None));
    assert_eq!(fairness(&both, &changed, &[]), Some(Fairness::Revealed));
    let read = case(Ok("x".to_string()), error("X", 3, None));
    let alias = case(Ok("x".to_string()), error("unknown anchor", 1, None));
    assert_eq!(fairness(&read, &alias, &[]), Some(Fairness::Kept));
    let other = case(Ok("x".to_string()), error("Y", 1, None));
    assert_eq!(fairness(&read, &other, &[]), Some(Fairness::Revealed));
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

/// Differences that long runs left unexplained before the search and the
/// rewrites were fixed for them, with the entries that explain them now.
const FINDINGS: &[(&str, &[&str])] = &[
    // Both reject the rewrite as a duplicate key, worded differently for a
    // null key.
    (
        "null: -3\r\n!!map null: !!str .nan\r\nb: 's ''q'''\r\n",
        &["core-tag-on-another-kind"],
    ),
    // `!` on an empty flow entry becomes `''`, not nothing.
    ("[1,!  ,]\n", &["non-specific-tag"]),
    // A wide integer as 1 would meet the key 1, an error the rewrite made.
    (
        "? 1\n18446744073709551616: x\n",
        &["integers-beyond-64-bits"],
    ),
    (
        "true: \n&a1 1: 's ''q'''\n118446744073709551616: []\n",
        &["integers-beyond-64-bits"],
    ),
    // Each `...` before any content goes, not only the first.
    ("...\n... #c", &["document-end-marker-alone"]),
    ("...\n...\n  - ---", &["document-end-marker-alone"]),
    // A plain scalar that starts with `:` after a tag, in a flow sequence.
    (
        "- [!!str a/**, !!str :a/**]\n",
        &["flow-colon-scalar-after-properties"],
    ),
    // A `...` below a tag is no node of the tag's.
    (
        "\n\n\n \n\n  \n!!map \n\n...",
        &["core-tag-on-another-kind"],
    ),
    // A flow value that starts with `:` before a quote.
    ("- {key: :'q,}\n", &["yaml-1.2-flow-syntax"]),
    // A `%TAG` prefix with `#` after a `,`.
    (
        "%TAG !e! tag:x,#2000:\n---\n[-2.5,\n  's', 's']\n",
        &["number-sign-in-a-tag"],
    ),
    // A `%TAG` prefix that starts with `#` (seed 19).
    (
        "%TAG !e! #tag:x,2000:\n---\n[[1],x]\n",
        &["number-sign-in-a-tag"],
    ),
    // The node on the line after `- ` continues there, so `'` opens no
    // quote and the `:` line is an empty key.
    ("b:\n- \n  ~,\n  'q]]\n:\n", &["empty-implicit-key"]),
    // A block scalar header after a key with an anchor.
    ("true:\n  &a2 null:\n>\n", &["block-scalar-under-an-anchor"]),
    // ayaml's duplicate key, reported where the mapping starts, and the
    // oracle's empty key, which hides it: without the empty key, both give
    // the duplicate key, at the place where each reports it.
    ("key: {key: x}\n: \n? key\n", &["empty-implicit-key"]),
    // A tab before a comment, or a tab line after a CR, after `...`.
    ("a: 1\n...\n\t# c\n", &["tab-as-white-space"]),
    ("- 2001-12-14\r\n...\r\t\n", &["tab-as-white-space"]),
    // A leading byte-order mark hides ayaml's error on the quoted scalar's
    // continuation line (asamarts/ayaml#137); without it, both reject, at
    // different places, and without the tab, ayaml's error is the
    // continuation line's.
    (
        "\u{feff}-\n  -\t\"d\\\n  \\u00e9\\t\"\n  - \"d\\_\\u00e9\\t\"\n  -\n    - |\n      \
         line 0\u{85}\n    - \n- []\n# end",
        &[
            "yaml-1.2-line-breaks",
            "byte-order-mark-starting-a-line",
            "tab-as-white-space",
            "quoted-scalar-continuation-indentation",
        ],
    ),
];

#[test]
fn long_run_findings_are_explained() {
    for (input, expected) in FINDINGS {
        let (case, panicked) = compare(input, Entry::Value);
        assert!(!panicked && case.class.is_difference(), "{case:?}");
        assert_eq!(explanation(&case).as_deref(), Some(*expected), "{input:?}");
    }
    // A long flow key cut to 1,000 bytes would meet a key of 1,000, an
    // error the rewrite made.
    let input = format!(
        "{{a: 1,{}: ~,{}: }}\n",
        "k".repeat(1_000),
        "k".repeat(1_042)
    );
    let (case, _) = compare(&input, Entry::Value);
    assert_eq!(case.class, Class::OnlyOracleRejects, "{case:?}");
    assert_eq!(
        explanation(&case),
        Some(vec!["long-flow-mapping-key"]),
        "{case:?}"
    );
}

/// Each syntax error message of ayaml 0.1.2, the version the workspace pins,
/// with sample values in place of its format arguments, as compatibility
/// mode reports it.
///
/// The list is every string literal and `format!` template that ayaml's
/// scanner and parser pass as an error message (to `Scanner::error_at`,
/// `Parser::error`, `expect_consumed`, `flow_error`'s `FlowLack` and
/// `non_printable_error`), and the messages `compat::map_error` puts in their
/// place, outside their tests: `src/scanner.rs`, `src/scanner/*.rs`,
/// `src/parser.rs`, `src/parser/*.rs` and `src/lib.rs`. Messages that
/// compatibility mode replaces are left out ("unexpected token in mapping
/// value" becomes "mapping values are not allowed in this context"), and so
/// are internal errors no input reaches ("invalid span position", "invalid
/// character boundary"). Refresh the list the same way when the pin moves.
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
        without_marks(
            "found unexpected ':' at line 1 column 3, while scanning a plain scalar at line 1 \
             column 2"
        ),
        "found unexpected ':', while scanning a plain scalar"
    );
    assert_eq!(
        without_marks("control characters are not allowed at position 4"),
        "control characters are not allowed"
    );
    assert_eq!(without_marks("a at b at line x"), "a at b at line x");
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

#[test]
fn a_size_that_is_not_a_number_fails_the_run() {
    assert_eq!(number_setting("N", None, 7), 7);
    assert_eq!(number_setting("N", Some("300_000"), 7), 300_000);
    for value in ["1e5", "", "seven"] {
        let result = std::panic::catch_unwind(|| number_setting("N", Some(value), 7));
        assert!(result.is_err(), "{value:?}");
    }
}

#[test]
fn the_same_seed_gives_the_same_inputs() {
    let inputs = |seed| {
        let mut rng = Rng::new(seed);
        (0..80)
            .map(|index| generated(&mut rng, index, true))
            .collect::<Vec<_>>()
    };
    assert_eq!(inputs(7), inputs(7));
    assert_ne!(inputs(7), inputs(8));
}

#[test]
fn without_specials_no_input_holds_one() {
    let mut rng = Rng::new(3);
    for index in 0..4_000 {
        let input = generated(&mut rng, index, false);
        assert!(
            !generate::SPECIALS
                .iter()
                .any(|special| input.contains(special)),
            "{input:?}"
        );
    }
}

/// A shape of generated input, and how to tell an input has it.
type Shape = (&'static str, fn(&str) -> bool);

/// The generator writes each shape the harness needs.
#[test]
fn the_generator_writes_every_shape() {
    let mut rng = Rng::new(5);
    let inputs: Vec<String> = (0..8_000)
        .map(|index| generated(&mut rng, index, true))
        .collect();
    let shapes: &[Shape] = &[
        ("a complex key", |input| {
            input.contains("? [") || input.contains("? {")
        }),
        ("CRLF", |input| input.contains("\r\n")),
        ("a tab", |input| {
            input.contains(":\t") || input.contains("-\t")
        }),
        ("a blank line in a block scalar", |input| {
            input.contains("|+\n\n") || input.contains("|\n  \n")
        }),
        ("an anchor on a key", |input| {
            input
                .lines()
                .any(|line| line.trim_start().starts_with("&a") && line.contains(": "))
        }),
        ("an escape", |input| {
            input.contains("\\U0001F600") || input.contains("\\x41")
        }),
        ("a quoted-only character in quotes", |input| {
            input.contains("'s\u{7f}") || input.contains("\"d\u{9f}")
        }),
        ("a C0 control", |input| {
            input.contains('\u{1b}') || input.contains('\u{1}')
        }),
        ("depth beyond 4", |input| input.contains("\n          ")),
        ("a long scalar", |input| {
            input.lines().any(|line| line.len() > 1_000)
        }),
        ("a merge key with aliases", |input| {
            input.contains("<<: [*a")
        }),
        ("aliases that fan out", |input| {
            input.starts_with("- &l0 [x]")
        }),
        ("a configuration", |input| input.contains("rules:\n  -")),
    ];
    for (shape, holds) in shapes {
        assert!(
            inputs.iter().any(|input| holds(input)),
            "no input has {shape}"
        );
    }
}

/// The shares of inputs with specials that `docs/design/yaml-parser.md`
/// gives for seed 1 hold, in per mille: of the generated inputs, over the
/// first 40,000 (200; the long run's 300,000 measured 201), and of the corpus
/// mutations at the long run's 20 per file (19).
#[test]
fn the_documented_shares_of_specials_hold() {
    let holds = |input: &str| {
        generate::SPECIALS
            .iter()
            .any(|special| input.contains(special))
    };
    let mut rng = Rng::new(1);
    let inputs = 40_000;
    let generated = (0..inputs)
        .filter(|index| holds(&generated(&mut rng, *index, true)))
        .count();
    let share = generated * 1_000 / inputs;
    assert!((196..=206).contains(&share), "{share} per mille");
    let (mut mutations, mut with) = (0_usize, 0_usize);
    for_each_corpus_input(1, 20, true, |input, mutation| {
        if mutation {
            mutations += 1;
            with += usize::from(holds(input));
        }
    });
    if let Some(share) = (with * 1_000).checked_div(mutations) {
        assert!((17..=22).contains(&share), "{share} per mille");
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
