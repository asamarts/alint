//! The harness's inputs: seeded token soup, generated documents, and
//! mutations of a document. The same seed always gives the same inputs.

use std::fmt::Write as _;

/// `SplitMix64`: small, fast and deterministic, which is all an input generator
/// needs.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
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
    pub(super) fn below(&mut self, bound: usize) -> usize {
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
/// flow styles, with anchors, aliases, tags, comments and markers. Most are
/// valid YAML; the specials, when set, land inside scalars and comments.
pub(super) fn document(rng: &mut Rng, specials: bool) -> String {
    let mut writer = Writer {
        rng,
        specials,
        out: String::new(),
        anchors: 0,
    };
    if writer.rng.chance(1, 8) {
        let prefix = writer.rng.pick(&[
            "%YAML 1.2\n---\n",
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
    let depth = writer.rng.below(4);
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
    writer.out
}

struct Writer<'r> {
    rng: &'r mut Rng,
    specials: bool,
    out: String,
    anchors: usize,
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

    /// An optional anchor or tag before a node.
    fn properties(&mut self) {
        if self.rng.chance(1, 8) {
            self.anchors += 1;
            let _ = write!(self.out, "&a{} ", self.anchors);
        }
        if self.rng.chance(1, 12) {
            self.out.push_str(
                self.rng
                    .pick(&["!t ", "!!str ", "!e!z ", "!<tag:x,2000:y> "]),
            );
        }
    }

    fn plain_key(&mut self) -> String {
        let key = self
            .rng
            .pick(&["a", "b", "key", "k1", "1", "true", "null", "a b", "<<"]);
        let special = self.special();
        format!("{key}{special}")
    }

    fn scalar(&mut self, indent: usize) {
        if self.anchors > 0 && self.rng.chance(1, 10) {
            let anchor = 1 + self.rng.below(self.anchors);
            let _ = write!(self.out, "*a{anchor}");
            return;
        }
        self.properties();
        let special = self.special();
        match self.rng.below(8) {
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
                let _ = write!(self.out, "{scalar}{special}");
            }
            3 => {
                let _ = write!(self.out, "'s{special} ''q'''");
            }
            4 => {
                let _ = write!(self.out, "\"d{special}\\n\\u00e9\\t\"");
            }
            5 | 6 => {
                let header = self.rng.pick(&["|", ">", "|-", ">+", "|2", "|+", ">-"]);
                self.out.push_str(header);
                self.out.push('\n');
                for line in 0..=self.rng.below(3) {
                    self.indent(indent + 2);
                    let _ = writeln!(self.out, "line {line}{special}");
                }
                // The block scalar ended its own line.
                self.out.pop();
            }
            // An empty value, which is null.
            _ => {}
        }
    }

    fn comment(&mut self) {
        if self.rng.chance(1, 10) {
            let special = self.special();
            let _ = write!(self.out, " # c{special}");
        }
    }

    fn block_mapping(&mut self, indent: usize, depth: usize) {
        for index in 0..=self.rng.below(3) {
            if index > 0 || indent > 0 {
                self.indent(indent);
            }
            if self.rng.chance(1, 14) {
                self.out.push_str("? ");
                let key = self.plain_key();
                self.out.push_str(&key);
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
            self.out.push(' ');
            self.scalar(indent);
            self.comment();
            self.out.push('\n');
            return;
        }
        match self.rng.below(3) {
            0 => {
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
                self.out.push_str(self.rng.pick(&[", ", ",", ",\n  "]));
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
        let scalar = self.rng.pick(&[
            "x", "1", "-2.5", "true", "null", "~", "'s'", "\"d\"", "a b", "", "é",
        ]);
        let _ = write!(self.out, "{scalar}{special}");
    }
}

/// `text` with one random edit: a line dropped, repeated, re-indented or cut
/// short, a token inserted, or two lines swapped.
pub(super) fn mutate(rng: &mut Rng, text: &str, specials: bool) -> String {
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
