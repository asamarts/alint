//! Load-time hints for `^` and `$` anchors that cannot do what a content
//! pattern's author almost certainly meant.
//!
//! The content kinds search the whole file, or a block of whole lines, with
//! the `regex` crate's defaults. `^` and `$` then anchor to the start and end
//! of that text rather than of each line, and `$` does not match before a
//! final newline. Two shapes are reliably mistakes (issue #343; the design is
//! `docs/design/content-pattern-anchors.md`):
//!
//! - a `$` that can only be reached by text that does not end in a newline,
//!   which is almost no text file;
//! - a forbidden pattern that starts with `^` and is not anchored at the end,
//!   which only ever looks at line 1.
//!
//! The checks read the parsed pattern, so escapes, character classes, `\A`
//! and `\z` are understood exactly. A pattern that turns on multi-line mode
//! anywhere gets no hints: its author is already anchoring to lines. Hints
//! are advisory and never change a finding.

use regex_syntax::ast::{self, AssertionKind, Ast, RepetitionKind, RepetitionRange};

use crate::facts::{FactKind, FactSpec};
use crate::registry::RuleRegistry;
use crate::rule::Rule;

/// What a content pattern is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternTarget {
    /// The whole file, where a match satisfies the rule
    /// (`file_content_matches` and the `file_content_matches` fact).
    File,
    /// The whole file, where a match is a finding (`file_content_forbidden`).
    ForbiddenFile,
    /// The first or last N lines, newlines included (`file_header`,
    /// `file_footer`).
    Lines,
}

/// An anchor that almost certainly does not do what the author meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternHint {
    /// A `$` that only text without a final newline can reach.
    EndBeforeFinalNewline(PatternTarget),
    /// A forbidden pattern that starts with `^` and is open at the end.
    LeadingStart,
}

impl PatternHint {
    /// The explanation and the fix, as one line for a warning.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::EndBeforeFinalNewline(PatternTarget::Lines) => {
                "`$` matches only at the very end of the checked lines, after their final \
                 newline, so this pattern fails whenever the last checked line ends in one; use \
                 `(?m)$` for the end of a line (`(?Rm)$` with CRLF line endings) or `\\s*$` for \
                 the end of the block"
            }
            Self::EndBeforeFinalNewline(_) => {
                "`$` matches only at the very end of the file, after its final newline, so this \
                 pattern fails on any file that ends in one; use `(?m)$` for the end of a line \
                 (`(?Rm)$` with CRLF line endings) or `\\s*$` for the end of the file"
            }
            Self::LeadingStart => {
                "`^` matches only at the start of the file, not at the start of each line; use \
                 `(?m)^` to anchor to lines, or `\\A` if you mean the start of the file"
            }
        }
    }
}

/// A hint and the rule or fact whose pattern raised it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHint {
    /// The rule id, or the fact id when [`Self::fact`] is set.
    pub id: String,
    /// `id` names a fact rather than a rule.
    pub fact: bool,
    pub hint: PatternHint,
}

impl std::fmt::Display for ConfigHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = if self.fact { "fact" } else { "rule" };
        write!(f, "{what} `{}`: {}", self.id, self.hint.message())
    }
}

/// The hints for a built rule and every nested rule it instantiates. A
/// nested rule that cannot be prepared adds none: hints are advisory, and
/// evaluating the rule reports that failure.
#[must_use]
pub fn rule_tree_hints(
    registry: &RuleRegistry,
    rule: &dyn Rule,
    spec: Option<&crate::RuleSpec>,
) -> Vec<ConfigHint> {
    let mut hints = Vec::new();
    let _ =
        crate::engine::visit_rule_tree_with_registry(registry, rule, spec, &mut |rule, spec| {
            // A hint says what a pattern starts or ends with, so a pattern that
            // may be secret (read from the environment, or in a `sensitive: true`
            // rule) raises none.
            if spec.is_some_and(|spec| spec.field_is_sensitive("pattern")) {
                return Ok(());
            }
            hints.extend(rule.pattern_hints().into_iter().map(|hint| ConfigHint {
                id: rule.id().to_string(),
                fact: false,
                hint,
            }));
            Ok(())
        });
    hints
}

/// The hints for the facts' patterns. Only a `file_content_matches` fact
/// without `extract` searches whole files; with `extract`, the pattern meets
/// each extracted value, where `^` and `$` mean what they say.
#[must_use]
pub fn fact_hints(facts: &[FactSpec]) -> Vec<ConfigHint> {
    environment_fact_hints(facts, &[])
}

/// [`fact_hints`], leaving out a pattern that holds one of
/// `environment_values` (a configuration's [`crate::Config::environment_values`])
/// as the output scrubber finds it: a hint says what the pattern starts or
/// ends with.
#[must_use]
pub fn environment_fact_hints(
    facts: &[FactSpec],
    environment_values: &[String],
) -> Vec<ConfigHint> {
    facts
        .iter()
        .filter_map(|fact| match &fact.kind {
            FactKind::FileContentMatches {
                file_content_matches,
            } if file_content_matches.extract.is_none() => {
                Some((fact, &file_content_matches.pattern))
            }
            _ => None,
        })
        .filter(|(_, pattern)| {
            matches!(
                crate::redact_environment_values(pattern, environment_values),
                std::borrow::Cow::Borrowed(_)
            )
        })
        .flat_map(|(fact, pattern)| {
            pattern_hints(pattern, PatternTarget::File)
                .into_iter()
                .map(|hint| ConfigHint {
                    id: fact.id.clone(),
                    fact: true,
                    hint,
                })
        })
        .collect()
}

/// The hints for `pattern` searched as `target`. A pattern that does not
/// parse gets none: building its rule reports the error.
#[must_use]
pub fn pattern_hints(pattern: &str, target: PatternTarget) -> Vec<PatternHint> {
    let Ok(ast) = ast::parse::Parser::new().parse(pattern) else {
        return Vec::new();
    };
    let flags = Flags::scan(&ast);
    if flags.multi_line {
        return Vec::new();
    }
    let reader = Reader { pattern, flags };
    let mut hints = Vec::new();
    if target == PatternTarget::ForbiddenFile
        && starts_with_line_start(&ast)
        && !ends_anchored(&ast)
    {
        hints.push(PatternHint::LeadingStart);
    }
    if reader.has_end_before_final_newline(&ast) {
        hints.push(PatternHint::EndBeforeFinalNewline(target));
    }
    hints
}

/// The flags turned on anywhere in the pattern. Scoping is ignored on
/// purpose: counting a flag as on everywhere can only suppress a hint.
#[derive(Debug, Default, Clone, Copy)]
struct Flags {
    multi_line: bool,
    dot_matches_new_line: bool,
    ignore_whitespace: bool,
}

impl Flags {
    fn scan(ast: &Ast) -> Self {
        let mut flags = Self::default();
        walk(ast, &mut |node| match node {
            Ast::Flags(set) => flags.add(&set.flags),
            Ast::Group(group) => {
                if let ast::GroupKind::NonCapturing(set) = &group.kind {
                    flags.add(set);
                }
            }
            _ => {}
        });
        flags
    }

    fn add(&mut self, set: &ast::Flags) {
        let mut negated = false;
        for item in &set.items {
            match &item.kind {
                ast::FlagsItemKind::Negation => negated = true,
                ast::FlagsItemKind::Flag(flag) if !negated => match flag {
                    ast::Flag::MultiLine => self.multi_line = true,
                    ast::Flag::DotMatchesNewLine => self.dot_matches_new_line = true,
                    ast::Flag::IgnoreWhitespace => self.ignore_whitespace = true,
                    _ => {}
                },
                ast::FlagsItemKind::Flag(_) => {}
            }
        }
    }
}

fn walk(ast: &Ast, visit: &mut dyn FnMut(&Ast)) {
    visit(ast);
    match ast {
        Ast::Repetition(rep) => walk(&rep.ast, visit),
        Ast::Group(group) => walk(&group.ast, visit),
        Ast::Alternation(alt) => alt.asts.iter().for_each(|a| walk(a, visit)),
        Ast::Concat(concat) => concat.asts.iter().for_each(|a| walk(a, visit)),
        _ => {}
    }
}

/// What a sub-pattern can match, as far as a following `$` cares.
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// Some match ends with `\n`.
    ends_in_newline: bool,
    /// It can match the empty string.
    can_be_empty: bool,
}

impl Shape {
    const EMPTY: Self = Self {
        ends_in_newline: false,
        can_be_empty: true,
    };
    /// Matches nothing: the identity for an alternation's branches.
    const NOTHING: Self = Self {
        ends_in_newline: false,
        can_be_empty: false,
    };

    fn char(is_newline: bool) -> Self {
        Self {
            ends_in_newline: is_newline,
            can_be_empty: false,
        }
    }
}

struct Reader<'a> {
    pattern: &'a str,
    flags: Flags,
}

impl Reader<'_> {
    /// Some `$` in a sequence follows only items that cannot end in `\n`.
    fn has_end_before_final_newline(&self, ast: &Ast) -> bool {
        match ast {
            Ast::Concat(concat) => concat.asts.iter().enumerate().any(|(i, item)| {
                (is_end_line(item) && self.reached_without_newline(&concat.asts[..i]))
                    || self.has_end_before_final_newline(item)
            }),
            Ast::Alternation(alt) => alt
                .asts
                .iter()
                .any(|a| self.has_end_before_final_newline(a)),
            Ast::Group(group) => self.has_end_before_final_newline(&group.ast),
            Ast::Repetition(rep) => self.has_end_before_final_newline(&rep.ast),
            _ => false,
        }
    }

    /// Reading back from a `$`, the first item that must consume a character
    /// cannot end in `\n`. Running out of items, as in `^$` or a `$` that opens
    /// its sequence, gives no hint.
    fn reached_without_newline(&self, before: &[Ast]) -> bool {
        for item in before.iter().rev() {
            let shape = self.shape(item);
            if shape.ends_in_newline {
                return false;
            }
            if !shape.can_be_empty {
                return true;
            }
        }
        false
    }

    fn shape(&self, ast: &Ast) -> Shape {
        match ast {
            Ast::Empty(_) | Ast::Flags(_) | Ast::Assertion(_) => Shape::EMPTY,
            Ast::Literal(literal) => Shape::char(literal.c == '\n'),
            Ast::Dot(_) => Shape::char(self.flags.dot_matches_new_line),
            Ast::ClassUnicode(_) | Ast::ClassPerl(_) | Ast::ClassBracketed(_) => {
                Shape::char(self.class_matches_newline(ast))
            }
            Ast::Repetition(rep) => {
                let inner = self.shape(&rep.ast);
                let (min, max) = bounds(&rep.op.kind);
                Shape {
                    ends_in_newline: inner.ends_in_newline && max != Some(0),
                    can_be_empty: inner.can_be_empty || min == 0,
                }
            }
            Ast::Group(group) => self.shape(&group.ast),
            Ast::Alternation(alt) => {
                alt.asts
                    .iter()
                    .map(|a| self.shape(a))
                    .fold(Shape::NOTHING, |acc, shape| Shape {
                        ends_in_newline: acc.ends_in_newline || shape.ends_in_newline,
                        can_be_empty: acc.can_be_empty || shape.can_be_empty,
                    })
            }
            Ast::Concat(concat) => {
                let mut ends_in_newline = false;
                for item in concat.asts.iter().rev() {
                    let shape = self.shape(item);
                    ends_in_newline |= shape.ends_in_newline;
                    if !shape.can_be_empty {
                        return Shape {
                            ends_in_newline,
                            can_be_empty: false,
                        };
                    }
                }
                Shape {
                    ends_in_newline,
                    can_be_empty: true,
                }
            }
        }
    }

    /// Whether a character class matches `\n`, decided by the regex engine on
    /// the class's own text so that every class syntax counts exactly. Under
    /// `(?x)` anywhere the class is also tried verbose; either match counts.
    fn class_matches_newline(&self, class: &Ast) -> bool {
        let span = class.span();
        let text = &self.pattern[span.start.offset..span.end.offset];
        let matches = |prefix: &str| {
            regex::bytes::Regex::new(&format!(r"{prefix}\A(?:{text})\z"))
                .ok()
                .is_none_or(|re| re.is_match(b"\n"))
        };
        matches("") || (self.flags.ignore_whitespace && matches("(?x)"))
    }
}

fn is_end_line(ast: &Ast) -> bool {
    matches!(ast, Ast::Assertion(a) if a.kind == AssertionKind::EndLine)
}

fn bounds(kind: &RepetitionKind) -> (u32, Option<u32>) {
    match kind {
        RepetitionKind::ZeroOrOne => (0, Some(1)),
        RepetitionKind::ZeroOrMore => (0, None),
        RepetitionKind::OneOrMore => (1, None),
        RepetitionKind::Range(RepetitionRange::Exactly(n)) => (*n, Some(*n)),
        RepetitionKind::Range(RepetitionRange::AtLeast(n)) => (*n, None),
        RepetitionKind::Range(RepetitionRange::Bounded(min, max)) => (*min, Some(*max)),
    }
}

/// The pattern opens with `^`, after any flags, directly or in a leading
/// group that is not an alternation.
fn starts_with_line_start(ast: &Ast) -> bool {
    match ast {
        Ast::Assertion(a) => a.kind == AssertionKind::StartLine,
        Ast::Concat(concat) => concat
            .asts
            .iter()
            .find(|item| !matches!(item, Ast::Flags(_) | Ast::Empty(_)))
            .is_some_and(starts_with_line_start),
        Ast::Group(group) => starts_with_line_start(&group.ast),
        _ => false,
    }
}

/// The pattern closes with `$` or `\z`, so a leading `^` is likely part of a
/// whole-file shape such as `^$` or `^exact text$`.
fn ends_anchored(ast: &Ast) -> bool {
    match ast {
        Ast::Assertion(a) => matches!(a.kind, AssertionKind::EndLine | AssertionKind::EndText),
        Ast::Concat(concat) => concat.asts.last().is_some_and(ends_anchored),
        Ast::Group(group) => ends_anchored(&group.ast),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PatternTarget::{File, ForbiddenFile, Lines};

    const END: PatternHint = PatternHint::EndBeforeFinalNewline(File);

    #[test]
    fn a_dollar_that_needs_a_missing_final_newline_is_flagged() {
        // The broken shapes shipped in alint's own docs, examples and corpus
        // (docs/design/content-pattern-anchors.md, section 1).
        for pattern in [
            r"^\d+\.\d+\.\d+$",
            r"^[0-9]+\.[0-9]+\.[0-9]+$",
            r"[ \t]+$",
            r"\r$",
            r";;+$",
            r"^\* -text$",
            r"foo[ \t]*$",
            r"(?:foo|bar)$",
            r"x.*$",
            r"[^\n]+$",
            r"\S$",
            r"x{2}$",
        ] {
            assert_eq!(pattern_hints(pattern, File), vec![END], "{pattern}");
        }
    }

    #[test]
    fn a_dollar_that_a_newline_can_reach_is_not_flagged() {
        for pattern in [
            r"^\d+\.\d+\.\d+\s*$",
            r"^v\d+\.\d+\.\d+\s*$",
            r"foo\n$",
            r"foo\n?$",
            r"foo(?:\r?\n)?$",
            r"(?s).*$",
            r"(?s:.*)$",
            r"[\s\S]*$",
            r"[^x]$",
            r"\D$",
            r"\W+$",
            r"\p{Cc}$",
            r"# type:\s*ignore([^\[]|$)",
            r"# noqa([^:]|$)",
            r"^$",
            r"$",
            // `.*` can match nothing at the very end, so this matches any file.
            r".*$",
            r"foo\z",
            r"(?m)foo$",
            r"(?Rm)foo$",
            r"(?m:foo$)",
            r"(?m)a(?-m)b$",
            // A negated class can take the final newline, so this reaches `$`.
            r"install [a-z]+([^/=<>~!]+)[A-Za-z0-9._\-*+!]*$",
        ] {
            assert_eq!(pattern_hints(pattern, File), Vec::new(), "{pattern}");
        }
    }

    #[test]
    fn a_leading_caret_in_a_forbidden_pattern_is_flagged() {
        for pattern in [
            r"^test",
            r"^\s*from urllib\.parse import .*urlparse",
            r"^\s*\*\s*@author",
            r"^\s*print\s*\(",
            r"(?i)^todo",
            r"(?:^foo)bar",
        ] {
            assert_eq!(
                pattern_hints(pattern, ForbiddenFile),
                vec![PatternHint::LeadingStart],
                "{pattern}"
            );
        }
    }

    #[test]
    fn a_caret_that_is_not_a_line_anchor_attempt_is_not_flagged() {
        for pattern in [
            r"(?m)^test",
            r"(?m:^test)",
            r"\Atest",
            r"(?:^|[\s;{(])console\.log",
            r"(^|[ \t])if\(",
            r"[^a]test",
            r"\^test",
            // Anchored at both ends: a whole-file shape, not a line check.
            r"^$",
            r"^exact contents\s*$",
            r"^\xEF\xBB\xBF\z",
        ] {
            assert_eq!(
                pattern_hints(pattern, ForbiddenFile),
                Vec::new(),
                "{pattern}"
            );
        }
    }

    #[test]
    fn a_leading_caret_is_left_alone_where_it_is_usually_meant() {
        // A required pattern at the file start (a VERSION file, a header block)
        // is the common, correct use of `^`.
        assert_eq!(pattern_hints(r"^[0-9]+\.[0-9]+", File), Vec::new());
        assert_eq!(pattern_hints(r"^// Copyright", Lines), Vec::new());
    }

    #[test]
    fn both_hints_can_fire_together() {
        assert_eq!(
            pattern_hints(r"^TODO:.*\S$", ForbiddenFile),
            vec![PatternHint::EndBeforeFinalNewline(ForbiddenFile)],
            "anchored at both ends, so no leading-caret hint; `\\S$` is the end hint",
        );
        assert_eq!(
            pattern_hints(r"^foo(?:bar$|baz)", ForbiddenFile),
            vec![
                PatternHint::LeadingStart,
                PatternHint::EndBeforeFinalNewline(ForbiddenFile),
            ],
        );
    }

    #[test]
    fn the_lines_target_names_the_block_in_its_message() {
        let [hint] = pattern_hints(r"}$", Lines)[..] else {
            panic!("expected one hint");
        };
        assert!(
            hint.message().contains("checked lines"),
            "{}",
            hint.message()
        );
        assert!(
            END.message().contains("end of the file"),
            "{}",
            END.message()
        );
    }

    #[test]
    fn verbose_mode_classes_are_read_both_ways() {
        // Under `(?x)` whitespace in a class is ignored; `[ \n]` still holds
        // an escaped newline either way, so the `$` is reachable.
        assert_eq!(pattern_hints(r"(?x) foo [ \n] $", File), Vec::new());
        assert_eq!(pattern_hints(r"(?x) foo [ \t] $", File), vec![END]);
    }

    #[test]
    fn facts_get_hints_only_when_they_search_whole_files() {
        let facts: Vec<FactSpec> = ayaml::compat::from_str(
            "- id: whole_file\n  file_content_matches:\n    paths: VERSION\n    pattern: '^[0-9]+[.][0-9]+$'\n\
             - id: extracted\n  file_content_matches:\n    paths: global.json\n    extract: { json: \"$.sdk.version\" }\n    pattern: '^9[.]0[.]306$'\n",
        )
        .unwrap();
        let hints = fact_hints(&facts);
        assert_eq!(
            hints,
            vec![ConfigHint {
                id: "whole_file".to_string(),
                fact: true,
                hint: END,
            }]
        );
        assert!(
            hints[0]
                .to_string()
                .starts_with("fact `whole_file`: `$` matches")
        );
    }

    /// A rule whose pattern always raises [`PatternHint::LeadingStart`].
    #[derive(Debug)]
    struct HintingRule;

    impl Rule for HintingRule {
        fn id(&self) -> &'static str {
            "anchored"
        }

        fn level(&self) -> crate::Level {
            crate::Level::Error
        }

        fn pattern_hints(&self) -> Vec<PatternHint> {
            vec![PatternHint::LeadingStart]
        }

        fn evaluate(
            &self,
            _ctx: &crate::Context<'_>,
        ) -> crate::error::Result<Vec<crate::Violation>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn a_rule_pattern_that_may_be_secret_raises_no_hint() {
        let registry = RuleRegistry::new();
        let hints = |spec: &crate::RuleSpec| rule_tree_hints(&registry, &HintingRule, Some(spec));
        let mut spec: crate::RuleSpec =
            ayaml::compat::from_str("id: anchored\nkind: anchored\nlevel: error\npattern: '^x'\n")
                .unwrap();
        assert_eq!(hints(&spec).len(), 1);
        // Another field read from the environment leaves the hint.
        spec.sensitive_fields.insert("message".to_string());
        assert_eq!(hints(&spec).len(), 1);
        spec.sensitive_fields.insert("pattern".to_string());
        assert_eq!(hints(&spec), Vec::new());
        spec.sensitive_fields.clear();
        spec.sensitive = true;
        assert_eq!(hints(&spec), Vec::new());
    }

    #[test]
    fn a_fact_pattern_holding_an_environment_value_raises_no_hint() {
        let facts: Vec<FactSpec> = ayaml::compat::from_str(
            "- id: secret\n  file_content_matches:\n    paths: VERSION\n    pattern: 'v-canary-value$'\n\
             - id: plain\n  file_content_matches:\n    paths: VERSION\n    pattern: 'plain$'\n",
        )
        .unwrap();
        assert_eq!(fact_hints(&facts).len(), 2);
        let hints = environment_fact_hints(&facts, &["canary-value".to_string()]);
        let ids: Vec<_> = hints.iter().map(|hint| hint.id.as_str()).collect();
        assert_eq!(ids, ["plain"]);
    }

    #[test]
    fn an_unparsable_pattern_gets_no_hints() {
        assert_eq!(pattern_hints(r"[bad", ForbiddenFile), Vec::new());
    }
}
