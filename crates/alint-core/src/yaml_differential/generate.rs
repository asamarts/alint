//! The harness's inputs: seeded token soup, generated documents, generated
//! configurations, and mutations of a document. The same seed always gives
//! the same inputs. The structured fixes' tests edit the documents too.

use std::fmt::Write as _;

pub(super) use super::configuration::configuration;

/// `SplitMix64`: small, fast and deterministic, which is all an input generator
/// needs.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(super) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`; `bound` must not be zero.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    /// True with probability `numerator / denominator`.
    pub(super) fn chance(&mut self, numerator: usize, denominator: usize) -> bool {
        self.below(denominator) < numerator
    }

    pub(super) fn pick<'a, T: ?Sized>(&mut self, items: &[&'a T]) -> &'a T {
        items[self.below(items.len())]
    }
}

/// The characters libyaml and YAML 1.2 read differently (next line, line
/// separator, paragraph separator), and the byte-order mark.
pub(super) const SPECIALS: &[&str] = &["\u{85}", "\u{2028}", "\u{2029}", "\u{feff}"];

/// Characters YAML allows in a quoted scalar only (DEL, C1 controls,
/// U+FFFE and U+FFFF), and C0 controls, which it allows nowhere.
pub(super) const CONTROLS: &[&str] = &[
    "\u{7f}", "\u{80}", "\u{8a}", "\u{9f}", "\u{fffe}", "\u{ffff}", "\u{1}", "\u{8}", "\u{1b}",
    "\u{1f}",
];

/// The escapes of a double-quoted scalar, valid and not.
const ESCAPES: &[&str] = &[
    "\\n",
    "\\t",
    "\\\t",
    "\\0",
    "\\a",
    "\\b",
    "\\e",
    "\\f",
    "\\r",
    "\\v",
    "\\/",
    "\\\"",
    "\\\\",
    "\\ ",
    "\\N",
    "\\_",
    "\\L",
    "\\P",
    "\\x41",
    "\\u00e9",
    "\\U0001F600",
    "\\x4",
    "\\uD800",
    "\\q",
    "\\\n  ",
];

const INDICATORS: &[&str] = &[
    "-",
    "- ",
    "? ",
    "?",
    ": ",
    ":",
    ",",
    ", ",
    "[",
    "]",
    "{",
    "}",
    "#",
    " #c",
    "&a ",
    "&b ",
    "&a",
    "*a",
    "*b",
    "!t ",
    "!!str ",
    "!!int ",
    "!!map ",
    "!<tag:x,2000:y> ",
    "!e!z ",
    "|",
    ">",
    "|-",
    ">+",
    "|2",
    ">1-",
    "'",
    "\"",
    "%YAML 1.2\n",
    "%TAG !e! tag:x,2000:\n",
    "---",
    "--- ",
    "...",
    "@",
    "`",
    "<<: ",
    "%",
    "? [",
    "? {",
    "\t",
];

const SCALARS: &[&str] = &[
    "a",
    "b c",
    "key",
    "1",
    "-2",
    "007",
    "0x1F",
    "0o17",
    "1_000",
    "1.5",
    "-0.0",
    "1e3",
    ".inf",
    "-.Inf",
    ".nan",
    "true",
    "False",
    "TRUE",
    "null",
    "Null",
    "~",
    "yes",
    "on",
    "2001-12-14",
    "12:30",
    "'q''s'",
    "'a b'",
    "\"e\\n\\u0041\"",
    "\"\\t\"",
    "\"\\x\"",
    "\"\\U0001F600\"",
    "\"unterminated",
    "'open",
    "18446744073709551616",
    "-9223372036854775809",
    "+1",
    "0.",
    "é",
    "😀",
    "a:b",
    "a#b",
    "http://x",
    "-a",
    "?a",
    ":a",
    "'\u{7f}'",
    "\"\u{9f}\"",
    "'\u{ffff}'",
    "x\u{1}y",
];

const SPACE: &[&str] = &[
    " ", "  ", "\t", "\n", "\n", "\r\n", "\n  ", "\n    ", "\n- ", "\n  - ", "\n\n", " \n",
];

/// A run of indicators, scalars and white space, with the specials when
/// `specials` is set.
pub(super) fn token_soup(rng: &mut Rng, specials: bool) -> String {
    let mut out = String::new();
    for _ in 0..=rng.below(24) {
        let token = match rng.below(if specials { 11 } else { 10 }) {
            0..=2 => rng.pick(INDICATORS),
            3..=6 => rng.pick(SCALARS),
            7..=9 => rng.pick(SPACE),
            _ => rng.pick(SPECIALS),
        };
        out.push_str(token);
    }
    out
}

/// A document built from a random tree, written in a random mix of block and
/// flow styles, with anchors, aliases, merge keys, tags, comments, escapes,
/// long and odd scalars and markers, sometimes with CRLF line breaks. Most
/// are valid YAML; the specials, when set, land inside scalars and comments.
/// A few are aliases that fan out, which only the oracle's count of alias
/// uses refuses.
pub(crate) fn document(rng: &mut Rng, specials: bool) -> String {
    if rng.chance(1, 40) {
        return fan_out(rng);
    }
    let mut writer = Writer {
        rng,
        specials,
        out: String::new(),
        anchors: 0,
        mapping_anchors: Vec::new(),
    };
    if writer.rng.chance(1, 8) {
        let prefix = writer.rng.pick(&[
            "%YAML 1.2\n---\n",
            "%YAML 1.3\n---\n",
            "---\n",
            "--- # c\n",
            "\u{feff}",
            "%TAG !e! tag:x,2000:\n---\n",
        ]);
        // The byte-order mark is one of the specials.
        if specials || prefix != "\u{feff}" {
            writer.out.push_str(prefix);
        }
    }
    let depth = if writer.rng.chance(1, 8) {
        4 + writer.rng.below(5)
    } else {
        writer.rng.below(4)
    };
    // The sequence's chance is drawn only when the mapping's fails.
    let mapping = writer.rng.chance(1, 2);
    if mapping {
        writer.block_mapping(0, depth);
    } else if writer.rng.chance(1, 2) {
        writer.block_sequence(0, depth);
    } else {
        writer.flow(depth);
        writer.out.push('\n');
    }
    if writer.rng.chance(1, 10) {
        writer
            .out
            .push_str(writer.rng.pick(&["...\n", "---\nb: 2\n", "# end\n"]));
    }
    if writer.rng.chance(1, 16) {
        writer.out = writer.out.replace('\n', "\r\n");
    }
    writer.out
}

/// A sequence of anchored flow sequences, each holding `width` aliases to the
/// one before, so the last expands to `width` to the power of the levels.
fn fan_out(rng: &mut Rng) -> String {
    let width = 2 + rng.below(9);
    let levels = 2 + rng.below(3);
    let mut out = String::from("- &l0 [x]\n");
    for level in 1..=levels {
        let aliases = vec![format!("*l{}", level - 1); width].join(", ");
        if level == levels {
            let _ = writeln!(out, "- [{aliases}]");
        } else {
            let _ = writeln!(out, "- &l{level} [{aliases}]");
        }
    }
    out
}

struct Writer<'r> {
    rng: &'r mut Rng,
    specials: bool,
    out: String,
    anchors: usize,
    /// The anchors on block mappings, which a merge key can name.
    mapping_anchors: Vec<usize>,
}

impl Writer<'_> {
    fn indent(&mut self, indent: usize) {
        self.out.extend(std::iter::repeat_n(' ', indent));
    }

    fn special(&mut self) -> &'static str {
        if self.specials && self.rng.chance(1, 6) {
            self.rng.pick(SPECIALS)
        } else {
            ""
        }
    }

    /// Now and then, a character YAML allows in a quoted scalar only, or
    /// nowhere.
    fn control(&mut self) -> &'static str {
        if self.rng.chance(1, 40) {
            self.rng.pick(CONTROLS)
        } else {
            ""
        }
    }

    /// Separating white space: a space, sometimes a tab.
    fn separation(&mut self) -> &'static str {
        if self.rng.chance(1, 20) { "\t" } else { " " }
    }

    fn new_anchor(&mut self) -> usize {
        self.anchors += 1;
        self.anchors
    }

    /// An optional anchor or tag before a node.
    fn properties(&mut self) {
        if self.rng.chance(1, 8) {
            let anchor = self.new_anchor();
            let _ = write!(self.out, "&a{anchor} ");
        }
        if self.rng.chance(1, 12) {
            self.out.push_str(self.rng.pick(&[
                "!t ",
                "!!str ",
                "!e!z ",
                "!<tag:x,2000:y> ",
                "!<!> ",
                "!!null ",
                "!!binary ",
            ]));
        }
    }

    fn plain_key(&mut self) -> String {
        if self.rng.chance(1, 60) {
            // Around the 1,024 characters libyaml allows in an implicit key.
            let len = 1_000 + self.rng.below(60);
            return "k".repeat(len);
        }
        let key = self
            .rng
            .pick(&["a", "b", "key", "k1", "1", "true", "null", "a b", "<<"]);
        let special = self.special();
        let anchor = if self.rng.chance(1, 20) {
            format!("&a{} ", self.new_anchor())
        } else {
            String::new()
        };
        format!("{anchor}{key}{special}")
    }

    fn scalar(&mut self, indent: usize) {
        if self.anchors > 0 && self.rng.chance(1, 10) {
            let anchor = 1 + self.rng.below(self.anchors);
            let _ = write!(self.out, "*a{anchor}");
            return;
        }
        self.properties();
        let special = self.special();
        let control = self.control();
        match self.rng.below(9) {
            0..=2 => {
                let scalar = self.rng.pick(&[
                    "x",
                    "y z",
                    "1",
                    "-3",
                    "0.5",
                    "1e3",
                    ".inf",
                    ".nan",
                    "true",
                    "no",
                    "null",
                    "~",
                    "0x10",
                    "0o7",
                    "2001-12-14",
                    "é",
                    "18446744073709551616",
                ]);
                let _ = write!(self.out, "{scalar}{special}{control}");
            }
            3 => {
                let _ = write!(self.out, "'s{special}{control} ''q'''");
            }
            4 => {
                let escape = self.rng.pick(ESCAPES);
                let _ = write!(self.out, "\"d{special}{control}{escape}\\u00e9\\t\"");
            }
            5 | 6 => self.block_scalar(indent, special),
            7 if self.rng.chance(1, 6) => {
                let len = 100 + self.rng.below(2_000);
                let word = self.rng.pick(&["word ", "x", "'q' ", "é"]);
                let text = word.repeat(len / word.len() + 1);
                let _ = write!(self.out, "{}", text.trim_end());
            }
            // An empty value, which is null.
            _ => {}
        }
    }

    /// A block scalar, with blank lines among and after its lines now and
    /// then, which its chomping keeps or strips.
    fn block_scalar(&mut self, indent: usize, special: &str) {
        let header = self
            .rng
            .pick(&["|", ">", "|-", ">+", "|2", "|+", ">-", "|+2", ">2-"]);
        self.out.push_str(header);
        self.out.push('\n');
        for line in 0..=self.rng.below(3) {
            if self.rng.chance(1, 4) {
                let blank = self.rng.pick(&["\n", "  \n", "\t\n"]);
                self.out.push_str(blank);
            }
            self.indent(indent + 2);
            let more = if self.rng.chance(1, 6) { "  " } else { "" };
            let _ = writeln!(self.out, "{more}line {line}{special}");
        }
        if self.rng.chance(1, 4) {
            let trailing = self.rng.pick(&["\n", "\n\n", "  \n"]);
            self.out.push_str(trailing);
        }
        // The block scalar ended its own line.
        self.out.pop();
    }

    fn comment(&mut self) {
        if self.rng.chance(1, 10) {
            let special = self.special();
            let control = self.control();
            let separation = self.separation();
            let _ = write!(self.out, "{separation}# c{special}{control}");
        }
    }

    /// An explicit key: a plain scalar, or a flow collection.
    fn explicit_key(&mut self) {
        self.out.push_str("? ");
        if self.rng.chance(1, 2) {
            let key = self.plain_key();
            self.out.push_str(&key);
        } else {
            self.flow(1);
        }
    }

    fn block_mapping(&mut self, indent: usize, depth: usize) {
        for index in 0..=self.rng.below(3) {
            if index > 0 || indent > 0 {
                self.indent(indent);
            }
            if !self.mapping_anchors.is_empty() && self.rng.chance(1, 8) {
                let named = self.mapping_anchors[self.rng.below(self.mapping_anchors.len())];
                if self.rng.chance(1, 3) {
                    let other = self.mapping_anchors[self.rng.below(self.mapping_anchors.len())];
                    let _ = writeln!(self.out, "<<: [*a{named}, *a{other}]");
                } else {
                    let _ = writeln!(self.out, "<<: *a{named}");
                }
                continue;
            }
            if self.rng.chance(1, 14) {
                self.explicit_key();
                self.out.push('\n');
                self.indent(indent);
                self.out.push(':');
            } else {
                let key = self.plain_key();
                self.out.push_str(&key);
                self.out.push(':');
            }
            self.value(indent, depth);
        }
    }

    fn block_sequence(&mut self, indent: usize, depth: usize) {
        for index in 0..=self.rng.below(3) {
            if index > 0 || indent > 0 {
                self.indent(indent);
            }
            self.out.push('-');
            self.value(indent, depth);
        }
    }

    /// A value after `key:` or `-`, ending its line.
    fn value(&mut self, indent: usize, depth: usize) {
        if depth == 0 || self.rng.chance(1, 2) {
            let separation = self.separation();
            self.out.push_str(separation);
            self.scalar(indent);
            self.comment();
            self.out.push('\n');
            return;
        }
        match self.rng.below(3) {
            0 => {
                if self.rng.chance(1, 6) {
                    let anchor = self.new_anchor();
                    // Now and then a block scalar instead, its header on the
                    // next line and not indented past the key.
                    if self.rng.chance(1, 8) {
                        let _ = writeln!(self.out, " &a{anchor}");
                        self.indent(indent);
                        let special = self.special();
                        self.block_scalar(indent, special);
                        self.out.push('\n');
                        return;
                    }
                    self.mapping_anchors.push(anchor);
                    let _ = write!(self.out, " &a{anchor}");
                }
                self.comment();
                self.out.push('\n');
                let child = indent + 2;
                self.block_mapping(child, depth - 1);
            }
            1 => {
                self.comment();
                self.out.push('\n');
                let child = indent + if self.rng.chance(1, 3) { 0 } else { 2 };
                self.block_sequence(child, depth - 1);
            }
            _ => {
                self.out.push(' ');
                self.flow(depth - 1);
                self.out.push('\n');
            }
        }
    }

    fn flow(&mut self, depth: usize) {
        self.properties();
        let mapping = self.rng.chance(1, 2);
        self.out.push(if mapping { '{' } else { '[' });
        let entries = self.rng.below(4);
        for index in 0..entries {
            if index > 0 {
                self.out
                    .push_str(self.rng.pick(&[", ", ",", ",\n  ", ",\n\t  "]));
            }
            if mapping {
                let key = self.plain_key();
                self.out.push_str(&key);
                self.out.push_str(": ");
            }
            if depth > 0 && self.rng.chance(1, 3) {
                self.flow(depth - 1);
            } else {
                self.flow_scalar();
            }
        }
        if entries > 0 && self.rng.chance(1, 8) {
            self.out.push(',');
        }
        self.out.push(if mapping { '}' } else { ']' });
    }

    fn flow_scalar(&mut self) {
        if self.anchors > 0 && self.rng.chance(1, 10) {
            let anchor = 1 + self.rng.below(self.anchors);
            let _ = write!(self.out, "*a{anchor}");
            return;
        }
        let special = self.special();
        let control = self.control();
        let scalar = self.rng.pick(&[
            "x",
            "1",
            "-2.5",
            "true",
            "null",
            "~",
            "'s'",
            "\"d\"",
            "a b",
            "",
            "é",
            "'q",
            "\"\\x41\"",
        ]);
        let _ = write!(self.out, "{scalar}{special}{control}");
    }
}

/// `text` with one random edit: a line dropped, repeated, re-indented or cut
/// short, a token inserted, or two lines swapped.
pub(crate) fn mutate(rng: &mut Rng, text: &str, specials: bool) -> String {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    if lines.is_empty() {
        return token_soup(rng, specials);
    }
    let at = rng.below(lines.len());
    match rng.below(7) {
        0 => {
            lines.remove(at);
        }
        1 => {
            let line = lines[at].clone();
            lines.insert(at, line);
        }
        2 => {
            let line = &mut lines[at];
            if rng.chance(1, 2) {
                line.insert(0, ' ');
            } else if line.starts_with(' ') {
                line.remove(0);
            } else {
                line.insert_str(0, "  ");
            }
        }
        3 => {
            let line = &mut lines[at];
            let boundaries: Vec<usize> = line.char_indices().map(|(index, _)| index).collect();
            let cut = boundaries[rng.below(boundaries.len())];
            line.truncate(cut);
        }
        4 => {
            let line = &mut lines[at];
            let boundaries: Vec<usize> = line
                .char_indices()
                .map(|(index, _)| index)
                .chain([line.len()])
                .collect();
            let insert_at = boundaries[rng.below(boundaries.len())];
            let token = if specials && rng.chance(1, 4) {
                rng.pick(SPECIALS)
            } else if rng.chance(1, 2) {
                rng.pick(INDICATORS)
            } else {
                rng.pick(SCALARS)
            };
            line.insert_str(insert_at, token);
        }
        5 => {
            let other = rng.below(lines.len());
            lines.swap(at, other);
        }
        _ => {
            lines.truncate(at + 1);
        }
    }
    lines.concat()
}

#[cfg(test)]
mod tests {
    use super::super::{for_each_corpus_input, generated};
    use super::{Rng, SPECIALS};

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
                !SPECIALS.iter().any(|special| input.contains(special)),
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
            // The shapes of entries that long runs once never saw.
            ("another YAML minor version", |input| {
                input.contains("%YAML 1.3")
            }),
            ("the verbatim tag `!<!>`", |input| input.contains("!<!> ")),
            ("a flow line that a tab indents", |input| {
                input.contains(",\n\t  ")
            }),
            ("a rule key that is not a string", |input| {
                input.contains("rules:\n  -")
                    && ["    1: y", "    true: y", "    ~: y", "    [a]: y"]
                        .iter()
                        .any(|line| input.contains(line))
            }),
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
        let holds = |input: &str| SPECIALS.iter().any(|special| input.contains(special));
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
}
