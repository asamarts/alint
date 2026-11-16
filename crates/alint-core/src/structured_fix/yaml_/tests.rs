//! Tests for the YAML span resolver's own bounds and its agreement with the
//! resolver it replaced. The behaviour shared with the other formats is tested
//! in `structured_fix/tests.rs`.
use super::*;
use crate::structured_fix::{
    resolve_removal_span, resolve_removal_spans, resolve_value_span, resolve_value_spans,
};
use crate::structured_format::Format;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.to_string())
}

fn idx(i: usize) -> PathSeg {
    PathSeg::Index(i)
}

/// Run `f` on a thread with a 2 MiB stack, the size of the rayon workers and
/// the language server's threads, and return what it returns. A stack overflow
/// aborts the test process.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(f)
        .expect("spawn a 2 MiB thread")
        .join()
        .expect("the resolver must return, not panic")
}

/// ROB-2: three documents that ayaml reads as one plain scalar (so the check
/// side's guards pass them) and saphyr reads as a block sequence 100,000 levels
/// deep. saphyr's recursive loader overflowed the stack on each (`alint fix`
/// aborted at 30,000 levels, the language server at 3,000). The resolver walks
/// the events iteratively and declines past 128 levels, or at the second
/// document.
#[test]
fn documents_saphyr_reads_deeper_than_ayaml_decline_on_a_small_stack() {
    const N: usize = 100_000;
    let deep = "- ".repeat(N);
    let inputs = [
        // ayaml's anchor name stops at `:`, saphyr's does not.
        format!("&ak:---\n{deep}x\n"),
        // saphyr skips a BOM at a later line start and reads `...` as the end
        // of a first document.
        format!("a\n\u{feff}...\n{deep}x\n"),
        format!("\r\n\u{feff}\u{feff}{deep}x\n"),
    ];
    for input in inputs {
        let declined = on_small_stack(move || {
            let bytes = input.as_bytes();
            let deep_path = vec![idx(0); 4];
            [
                resolve_value_span(Format::Yaml, bytes, &[]),
                resolve_value_span(Format::Yaml, bytes, &deep_path),
                resolve_removal_span(Format::Yaml, bytes, &[key("x")]),
                resolve_value_spans(Format::Yaml, bytes, std::slice::from_ref(&deep_path))[0]
                    .clone(),
                resolve_removal_spans(Format::Yaml, bytes, &[vec![key("x")]])[0].clone(),
            ]
            .iter()
            .all(Option::is_none)
        });
        assert!(declined, "a document nested past the limit must decline");
    }
}

/// The depth cap is the check side's: 128 levels of nesting resolve, 129
/// decline, block or flow.
#[test]
fn nesting_resolves_to_the_limit_and_declines_past_it() {
    for (open, close) in [("- ", ""), ("[", "]")] {
        let at_limit = format!("{}x{}\n", open.repeat(MAX_DEPTH), close.repeat(MAX_DEPTH));
        let path = vec![idx(0); MAX_DEPTH];
        let span = resolve_value_span(Format::Yaml, at_limit.as_bytes(), &path)
            .expect("a document at the limit resolves");
        assert_eq!(&at_limit[span], "x");
        let past = format!(
            "{}x{}\n",
            open.repeat(MAX_DEPTH + 1),
            close.repeat(MAX_DEPTH + 1)
        );
        let path = vec![idx(0); MAX_DEPTH + 1];
        assert_eq!(
            resolve_value_span(Format::Yaml, past.as_bytes(), &path),
            None
        );
    }
}

/// The alias bomb of ROB-3: ayaml reads it as one plain scalar, while saphyr's
/// loader copies each anchored subtree per alias. Its alias budget does not
/// count the subtree under a mapping key, so twelve levels of `{? *a}` grow
/// tenfold each: `fix` ran 17 s and aborted at 3.35 GB.
fn alias_bomb() -> String {
    let mut bomb = String::from("&ak:---\n- &a0 [x,x,x,x,x,x,x,x,x,x]\n");
    for level in 0..12 {
        let refs = vec![format!("*b{level}"); 10].join(",");
        writeln!(
            bomb,
            "- &b{level} {{? *a{level}}}\n- &a{} [{refs}]",
            level + 1
        )
        .unwrap();
    }
    bomb
}

/// The bomb declines through the public entry points, with a wide margin on
/// the time a walk of a 1 KB document takes.
#[test]
fn an_alias_bomb_declines_quickly() {
    let bomb = alias_bomb();
    let started = Instant::now();
    let paths = [
        vec![],
        vec![idx(25), idx(0), key("x")],
        vec![idx(25), idx(3)],
    ];
    for path in &paths {
        assert_eq!(
            resolve_value_span(Format::Yaml, bomb.as_bytes(), path),
            None
        );
        assert_eq!(
            resolve_removal_span(Format::Yaml, bomb.as_bytes(), path),
            None
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the bomb took {:?}",
        started.elapsed()
    );
}

/// The deterministic bound behind [`an_alias_bomb_declines_quickly`]: the
/// walk records one node per scalar, alias and collection event, so the tree
/// of the bomb is no larger than its text.
#[test]
fn an_alias_bomb_is_read_in_one_pass_over_its_events() {
    let bomb = alias_bomb();
    let index = SpanIndex::read(&bomb).expect("the bomb is one valid document");
    assert!(
        index.nodes.len() <= bomb.len(),
        "{} nodes for {} bytes: an alias was expanded",
        index.nodes.len(),
        bomb.len()
    );
}

/// YAML-14: saphyr's scalar span starts at the anchor, so a set replaced
/// `&x 1` whole and dropped the anchor, breaking every `*x` after it. The set
/// span starts past the anchor; a core-schema tag goes with the value (the
/// serialized value carries its own type).
#[test]
fn a_set_keeps_the_anchor() {
    let set = |src: &str, path: &[PathSeg], to: &str| -> Option<String> {
        let span = resolve_value_span(Format::Yaml, src.as_bytes(), path)?;
        let mut out = src.to_string();
        out.replace_range(span, to);
        Some(out)
    };
    let src = "a: &x 1\nb: *x\n";
    let out = set(src, &[key("a")], "5").unwrap();
    assert_eq!(out, "a: &x 5\nb: *x\n");
    // The edited file re-parses with the anchor in place: the alias reads the
    // new value.
    let value = Format::Yaml.parse(&out).unwrap();
    assert_eq!(
        (value["a"].clone(), value["b"].clone()),
        (5.into(), 5.into())
    );

    let cases = [
        // A core tag after the anchor is replaced, the anchor kept.
        ("a: &x !!str 5\nb: *x\n", "a: &x 6\nb: *x\n"),
        // A core tag alone is replaced, as before.
        ("a: !!str 5\n", "a: 6\n"),
        // A quoted scalar with an anchor clamps at its closing quote, so the
        // comment stays (the span ran to the end of the line).
        ("a: &x \"q\"  # keep\nb: *x\n", "a: &x 6  # keep\nb: *x\n"),
        ("a: &x 'it''s'  # keep\n", "a: &x 6  # keep\n"),
        // In a flow sequence, and with anchor names saphyr reads past a `:`.
        ("s: [&x 1, *x]\n", "s: [&x 6, *x]\n"),
        // In a BOM file, with tabs after the anchor.
        ("\u{feff}a: &x\t\t1\n", "\u{feff}a: &x\t\t6\n"),
    ];
    for (src, want) in cases {
        let path = if src.starts_with("s:") {
            vec![key("s"), idx(0)]
        } else {
            vec![key("a")]
        };
        assert_eq!(set(src, &path, "6").as_deref(), Some(want), "{src:?}");
        Format::Yaml.parse(want).unwrap();
    }
    // An anchor name saphyr reads past a `:` (ayaml does not, so the check
    // side cannot read this file, but the span is still exact).
    assert_eq!(
        set("a: &x:y 1\n", &[key("a")], "6").as_deref(),
        Some("a: &x:y 6\n")
    );
    // A core tag before the anchor cannot be dropped with the anchor kept;
    // an anchor with no scalar after it has nothing to replace; a scalar
    // under a tag outside the core schema still declines.
    for src in [
        "a: !!str &x 5\n",
        "a: &x\nb: 1\n",
        "a: !Ref foo\n",
        "a: &x !Ref foo\n",
    ] {
        assert_eq!(
            resolve_value_span(Format::Yaml, src.as_bytes(), &[key("a")]),
            None,
            "{src:?}"
        );
    }
}

/// A path through an alias declines. saphyr's loader followed a copy of the
/// anchored node whose spans point into the anchor's definition, so a set or
/// a removal at `a.b` edited `x.b` as well, and the re-verify (of `a.b` only)
/// passed.
#[test]
fn a_path_through_an_alias_declines() {
    let flow = "x: &x {b: 1}\na: *x\n";
    assert_eq!(
        resolve_value_span(Format::Yaml, flow.as_bytes(), &[key("a"), key("b")]),
        None
    );
    let block = "x: &x\n  b: 1\na: *x\n";
    assert_eq!(
        resolve_value_span(Format::Yaml, block.as_bytes(), &[key("a"), key("b")]),
        None
    );
    assert_eq!(
        resolve_removal_span(Format::Yaml, block.as_bytes(), &[key("a"), key("b")]),
        None
    );
    // The anchored node itself stays editable.
    let span = resolve_value_span(Format::Yaml, block.as_bytes(), &[key("x"), key("b")]).unwrap();
    assert_eq!(&block[span], "1");
    // A removal whose value is an alias of a scalar removes the line, as
    // before; a merge key leaves the explicit keys beside it reachable.
    let alias_value = "y: &y 1\nx: 1\na: *y\n";
    let span = resolve_removal_span(Format::Yaml, alias_value.as_bytes(), &[key("a")]).unwrap();
    assert_eq!(&alias_value[span], "a: *y\n");
    let merge = "base: &b {i: 1}\njob:\n  <<: *b\n  i: 2\n";
    let span = resolve_value_span(Format::Yaml, merge.as_bytes(), &[key("job"), key("i")]).unwrap();
    assert_eq!(&merge[span], "2");
    assert_eq!(
        resolve_value_span(
            Format::Yaml,
            merge.as_bytes(),
            &[key("job"), key("<<"), key("i")]
        ),
        None
    );
}

/// A mapping that repeats a key declines the document, as saphyr's loader
/// refused it, with keys compared by value; an alias key counts as the scalar
/// it repeats.
#[test]
fn a_repeated_key_declines_the_document() {
    let alias_key = "k: &n name\n*n : 5\n";
    let span = resolve_value_span(Format::Yaml, alias_key.as_bytes(), &[key("name")]).unwrap();
    assert_eq!(&alias_key[span], "5");
    for src in [
        "a: 1\n\"a\": 2\nb: {c: 3}\n",
        "b: {c: 3}\nx: {1: a, 0x1: b}\n",
        "k: &n name\n*n : 5\nname: 6\nb: {c: 3}\n",
    ] {
        let path = [key("b"), key("c")];
        assert_eq!(
            resolve_value_span(Format::Yaml, src.as_bytes(), &path),
            None,
            "{src:?}"
        );
        assert_eq!(
            resolve_removal_span(Format::Yaml, src.as_bytes(), &path),
            None,
            "{src:?}"
        );
    }
}

/// ROB-7: the batch entry points parse the file once for any number of
/// paths, and agree with the single-path ones.
#[test]
fn many_paths_resolve_from_one_parse() {
    const MATCHES: usize = 20_000;
    let src = "- y: 0\n  x: 1\n".repeat(MATCHES);
    let paths: Vec<Vec<PathSeg>> = (0..MATCHES).map(|i| vec![idx(i), key("x")]).collect();
    let before = reads();
    let removals = resolve_removal_spans(Format::Yaml, src.as_bytes(), &paths);
    let values = resolve_value_spans(Format::Yaml, src.as_bytes(), &paths);
    assert_eq!(reads() - before, 2, "one parse per batch");
    for (i, (removal, value)) in removals.iter().zip(&values).enumerate() {
        let line = 14 * i + 7;
        assert_eq!(removal.clone(), Some(line..line + 7));
        assert_eq!(value.clone(), Some(line + 5..line + 6));
    }
    // The same through the single-path entry points (on a few paths).
    for path in paths.iter().step_by(4_999) {
        assert_eq!(
            resolve_value_span(Format::Yaml, src.as_bytes(), path),
            values[match path[0] {
                PathSeg::Index(i) => i,
                PathSeg::Key(_) => unreachable!(),
            }]
        );
    }
}

/// Character offsets map to bytes at every stride boundary and between them.
#[test]
fn char_offsets_map_to_bytes_around_the_stride() {
    let text = "é".repeat(200) + "x";
    let offsets = CharOffsets::new(&text);
    for index in 0..=201 {
        let byte = text
            .char_indices()
            .nth(index)
            .map_or(text.len(), |(b, _)| b);
        assert_eq!(offsets.byte(&text, index), Some(byte), "char {index}");
    }
    assert_eq!(offsets.byte(&text, 202), None);
    let ascii = CharOffsets::new("abc");
    assert_eq!(
        (ascii.byte("abc", 3), ascii.byte("abc", 4)),
        (Some(3), None)
    );
}

/// The resolver v0.22.1 shipped, over saphyr's recursive loader: the reference
/// of the differential test below, kept here only.
mod reference {
    use super::PathSeg;
    use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
    use std::ops::Range;

    /// What the old resolver returned for a path, and whether the path passed
    /// through an alias on the way (the old loader followed a copy of the
    /// anchored node there; the new resolver declines).
    pub(super) struct Old {
        pub(super) span: Option<Range<usize>>,
        pub(super) through_alias: bool,
    }

    pub(super) fn load(stripped: &str) -> Option<MarkedYaml<'_>> {
        let docs = MarkedYaml::load_from_str(stripped).ok()?;
        let [doc] = <[_; 1]>::try_from(docs).ok()?;
        Some(doc)
    }

    /// Whether `node` is the copy saphyr's loader made for an alias: its span
    /// is the alias token alone.
    fn is_alias(stripped: &str, node: &MarkedYaml<'_>) -> bool {
        char_span_to_byte(stripped, node.span.start.index(), node.span.end.index())
            .and_then(|span| stripped.get(span))
            .is_some_and(|text| {
                text.len() > 1
                    && text.starts_with('*')
                    && !text.contains([' ', '\t', '\n', '\r', ',', '[', ']', '{', '}'])
            })
    }

    fn walk<'a, 'i>(
        stripped: &str,
        doc: &'a MarkedYaml<'i>,
        path: &[PathSeg],
        through_alias: &mut bool,
    ) -> Option<&'a MarkedYaml<'i>> {
        let mut node = doc;
        for seg in path {
            *through_alias |= is_alias(stripped, node);
            node = match seg {
                PathSeg::Key(k) => get_key(node, k)?,
                PathSeg::Index(i) => get_index(node, *i)?,
            };
        }
        Some(node)
    }

    pub(super) fn value_span(text: &str, path: &[PathSeg]) -> Old {
        let mut through_alias = false;
        let span = (|| {
            if !crate::yaml_depth::flow_depth_within_limit(text) {
                return None;
            }
            let stripped = text.trim_start_matches('\u{feff}');
            let bom_len = text.len() - stripped.len();
            let doc = load(stripped)?;
            let node = walk(stripped, &doc, path, &mut through_alias)?;
            if !matches!(node.data, YamlData::Value(_) | YamlData::Representation(..)) {
                return None;
            }
            let raw = char_span_to_byte(stripped, node.span.start.index(), node.span.end.index())?;
            let at = stripped.get(raw.clone())?;
            if at.is_empty() || at.contains('\n') || at.starts_with('*') {
                return None;
            }
            let span = super::clamp_quoted_scalar_span(stripped, raw);
            Some(span.start + bom_len..span.end + bom_len)
        })();
        Old {
            span,
            through_alias,
        }
    }

    pub(super) fn removal_span(text: &str, path: &[PathSeg]) -> Old {
        let mut through_alias = false;
        let span = (|| {
            if !crate::yaml_depth::flow_depth_within_limit(text) {
                return None;
            }
            let stripped = text.trim_start_matches('\u{feff}');
            let bom_len = text.len() - stripped.len();
            let doc = load(stripped)?;
            let (PathSeg::Key(target), parents) = path.split_last()? else {
                return None;
            };
            let node = walk(stripped, &doc, parents, &mut through_alias)?;
            through_alias |= is_alias(stripped, node);
            let YamlData::Mapping(m) = &node.data else {
                return None;
            };
            let (knode, vnode) = m.iter().find(
                |(k, _)| matches!(&k.data, YamlData::Value(Scalar::String(s)) if s == target),
            )?;
            if !matches!(
                vnode.data,
                YamlData::Value(_) | YamlData::Representation(..)
            ) {
                return None;
            }
            let entry =
                char_span_to_byte(stripped, knode.span.start.index(), vnode.span.end.index())?;
            if stripped.get(entry.clone())?.contains('\n') {
                return None;
            }
            let line_start = stripped[..entry.start].rfind('\n').map_or(0, |i| i + 1);
            if !stripped[line_start..entry.start]
                .bytes()
                .all(|b| b == b' ' || b == b'\t')
            {
                return None;
            }
            let line_end = stripped[entry.end..]
                .find('\n')
                .map_or(stripped.len(), |i| entry.end + i + 1);
            let tail = stripped[entry.end..line_end]
                .trim_end_matches(['\r', '\n'])
                .trim_start_matches([' ', '\t']);
            if !(tail.is_empty() || tail.starts_with('#')) {
                return None;
            }
            Some(line_start + bom_len..line_end + bom_len)
        })();
        Old {
            span,
            through_alias,
        }
    }

    pub(super) fn get_key<'a, 'i>(
        node: &'a MarkedYaml<'i>,
        key: &str,
    ) -> Option<&'a MarkedYaml<'i>> {
        let YamlData::Mapping(m) = &node.data else {
            return None;
        };
        m.iter().find_map(|(k, v)| match &k.data {
            YamlData::Value(Scalar::String(s)) if s == key => Some(v),
            _ => None,
        })
    }

    fn get_index<'a, 'i>(node: &'a MarkedYaml<'i>, i: usize) -> Option<&'a MarkedYaml<'i>> {
        let YamlData::Sequence(seq) = &node.data else {
            return None;
        };
        seq.get(i)
    }

    fn char_span_to_byte(text: &str, cs: usize, ce: usize) -> Option<Range<usize>> {
        if cs > ce {
            return None;
        }
        let (mut bs, mut be) = (None, None);
        let mut count = 0usize;
        for (b, _) in text.char_indices() {
            if count == cs {
                bs = Some(b);
            }
            if count == ce {
                be = Some(b);
            }
            count += 1;
        }
        if cs == count {
            bs = Some(text.len());
        }
        if ce == count {
            be = Some(text.len());
        }
        Some(bs?..be?)
    }

    /// Every path of string keys and indices in `doc`, aliases followed (so
    /// paths through them are tried too), up to `limit` paths.
    pub(super) fn paths(doc: &MarkedYaml<'_>, limit: usize) -> Vec<Vec<PathSeg>> {
        let mut out = Vec::new();
        let mut pending = vec![(doc, Vec::new())];
        while let Some((node, path)) = pending.pop() {
            if out.len() >= limit || path.len() > 6 {
                continue;
            }
            match &node.data {
                YamlData::Mapping(m) => {
                    for (k, v) in m {
                        if let YamlData::Value(Scalar::String(s)) = &k.data {
                            let mut next = path.clone();
                            next.push(PathSeg::Key(s.to_string()));
                            pending.push((v, next));
                        }
                    }
                }
                YamlData::Sequence(seq) => {
                    for (i, v) in seq.iter().enumerate() {
                        let mut next = path.clone();
                        next.push(PathSeg::Index(i));
                        pending.push((v, next));
                    }
                }
                _ => {}
            }
            out.push(path);
        }
        out
    }
}

/// The span the new resolver returns where the old one returned `old` for a
/// set: the same, but starting past an anchor, which a set keeps, and with a
/// quoted scalar after properties clamped at its closing quote (the old clamp
/// looked for the quote at the anchor). A core tag before an anchor, and an
/// anchor with no scalar after it, decline.
fn expected_value_span(src: &str, old: Range<usize>) -> Option<Range<usize>> {
    let token_end = |at: usize| {
        src[at..old.end]
            .find([' ', '\t'])
            .map_or(old.end, |n| at + n)
    };
    let skip_blanks = |at: usize| {
        at + src[at..old.end].len() - src[at..old.end].trim_start_matches([' ', '\t']).len()
    };
    let mut start = old.start;
    let mut content = old.start;
    let mut tag_seen = false;
    loop {
        match src.as_bytes().get(content) {
            Some(b'&') => {
                if tag_seen {
                    return None;
                }
                content = skip_blanks(token_end(content));
                start = content;
            }
            Some(b'!') if !tag_seen => {
                tag_seen = true;
                content = skip_blanks(token_end(content));
            }
            _ => break,
        }
    }
    if content >= old.end {
        return None;
    }
    Some(start..clamp_quoted_scalar_span(src, content..old.end).end)
}

/// A small YAML document drawn from `choices`: block and flow collections,
/// plain, quoted, tagged and anchored scalars, aliases (some to anchors not yet
/// defined, which do not parse), alias keys, comments, a BOM, CRLF line ends
/// and a second document.
struct Draw<'a> {
    choices: &'a [u8],
    at: usize,
    anchors: usize,
    key_anchors: usize,
    out: String,
}

impl Draw<'_> {
    fn pick(&mut self, n: usize) -> usize {
        let choice = self.choices.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        usize::from(choice) % n
    }

    fn document(choices: &[u8]) -> String {
        let mut draw = Draw {
            choices,
            at: 0,
            anchors: 0,
            key_anchors: 0,
            out: String::new(),
        };
        let prefix = ["", "", "", "\u{feff}", "---\n", "# c\n"][draw.pick(6)];
        draw.out.push_str(prefix);
        draw.block_mapping(0, 0);
        match draw.pick(8) {
            0 => draw.out.push_str("...\n"),
            1 => draw.out.push_str("---\nz: 1\n"),
            _ => {}
        }
        if draw.pick(6) == 0 {
            draw.out = draw.out.replace('\n', "\r\n");
        }
        draw.out
    }

    fn indent(&mut self, n: usize) {
        self.out.push_str(&" ".repeat(n));
    }

    fn key(&mut self) -> String {
        match self.pick(10) {
            0 => "\"a\"".into(),
            1 => "'b'".into(),
            2 => "!!str c".into(),
            3 => {
                self.key_anchors += 1;
                format!("&k{} k{}", self.key_anchors, self.key_anchors)
            }
            4 if self.key_anchors > 0 => format!("*k{} ", 1 + self.pick(self.key_anchors)),
            5 => "1".into(),
            n => ["a", "b", "c", "d", "e", "f", "g", "h"][(n + self.pick(2) * 4) % 8].into(),
        }
    }

    fn scalar(&mut self, flow: bool) -> String {
        let core = [
            "v",
            "1",
            "true",
            "~",
            "x y",
            "\"q\"",
            "'s'",
            "\"a\\\"b\"",
            "'it''s'",
            "\"has # hash\"",
            "é",
            "\"\"",
            "null",
            "0x1F",
        ];
        let mut out = String::new();
        let props = self.pick(10);
        if props == 0 || props == 3 {
            self.anchors += 1;
            write!(out, "&a{} ", self.anchors).unwrap();
        }
        match props {
            1 | 3 => out.push_str("!!str "),
            2 => out.push_str("!t "),
            4 => {
                self.anchors += 1;
                write!(out, "!!str &a{} ", self.anchors).unwrap();
            }
            _ => {}
        }
        if self.pick(12) == 0 {
            // An alias in place of the scalar, to any anchor so far or the next.
            return format!("*a{}", 1 + self.pick(self.anchors + 1));
        }
        let choice = self.pick(core.len() + 1);
        match core.get(choice) {
            Some(text) => out.push_str(text),
            None if !flow => {} // an empty scalar
            None => out.push('v'),
        }
        out
    }

    fn flow(&mut self, depth: usize) -> String {
        let items = self.pick(3);
        let anchor = if self.pick(5) == 0 {
            self.anchors += 1;
            format!("&a{} ", self.anchors)
        } else {
            String::new()
        };
        let mut parts = Vec::new();
        if self.pick(2) == 0 {
            for _ in 0..items {
                let value = if depth < 3 && self.pick(4) == 0 {
                    self.flow(depth + 1)
                } else {
                    self.scalar(true)
                };
                parts.push(value);
            }
            format!("{anchor}[{}]", parts.join(", "))
        } else {
            for _ in 0..items {
                let key = self.key();
                let value = self.scalar(true);
                parts.push(format!("{key}: {value}"));
            }
            let sep = if self.pick(4) == 0 { ",\n  " } else { ", " };
            format!("{anchor}{{{}}}", parts.join(sep))
        }
    }

    /// A value after `key:` or `-`: inline, or a nested block collection.
    fn value(&mut self, indent: usize, depth: usize) {
        match self.pick(8) {
            0 if depth < 4 => {
                if self.pick(5) == 0 {
                    self.anchors += 1;
                    write!(self.out, " &a{}", self.anchors).unwrap();
                }
                self.out.push('\n');
                self.block_mapping(indent + 2, depth + 1);
            }
            1 if depth < 4 => {
                self.out.push('\n');
                self.block_sequence(indent + 2, depth + 1);
            }
            2 => {
                let flow = self.flow(depth);
                writeln!(self.out, " {flow}").unwrap();
            }
            _ => {
                let scalar = self.scalar(false);
                let comment = if self.pick(4) == 0 { "  # c" } else { "" };
                writeln!(self.out, " {scalar}{comment}").unwrap();
            }
        }
    }

    fn block_mapping(&mut self, indent: usize, depth: usize) {
        for _ in 0..=self.pick(4) {
            self.indent(indent);
            let key = self.key();
            write!(self.out, "{key}:").unwrap();
            self.value(indent, depth);
        }
    }

    fn block_sequence(&mut self, indent: usize, depth: usize) {
        for _ in 0..=self.pick(3) {
            self.indent(indent);
            self.out.push('-');
            if depth < 4 && self.pick(4) == 0 {
                // A compact mapping in the sequence entry.
                let key = self.key();
                write!(self.out, " {key}:").unwrap();
                self.value(indent + 2, depth + 1);
            } else {
                self.value(indent, depth);
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(1024))]

    /// The event-walk resolver agrees with the recursive one it replaced on
    /// every path the old one resolved, but for the changes the fix makes: a
    /// path through an alias declines, and a set keeps a scalar's anchor (see
    /// [`expected_value_span`]). Batch resolution agrees with resolving each
    /// path alone.
    #[test]
    fn the_event_walk_agrees_with_the_recursive_resolver(
        choices in proptest::collection::vec(proptest::num::u8::ANY, 0..96),
    ) {
        let src = Draw::document(&choices);
        let stripped = src.trim_start_matches('\u{feff}');
        let mut paths = reference::load(stripped)
            .map(|doc| reference::paths(&doc, 64))
            .unwrap_or_default();
        paths.extend([vec![key("a"), key("zz")], vec![idx(7)]]);
        let values = resolve_value_spans(Format::Yaml, src.as_bytes(), &paths);
        let removals = resolve_removal_spans(Format::Yaml, src.as_bytes(), &paths);
        for (path, (value, removal)) in paths.iter().zip(values.into_iter().zip(removals)) {
            proptest::prop_assert_eq!(
                &value,
                &resolve_value_span(Format::Yaml, src.as_bytes(), path)
            );
            proptest::prop_assert_eq!(
                &removal,
                &resolve_removal_span(Format::Yaml, src.as_bytes(), path)
            );
            let old = reference::value_span(&src, path);
            if let Some(old_span) = old.span {
                let want = if old.through_alias {
                    None
                } else {
                    expected_value_span(&src, old_span)
                };
                proptest::prop_assert_eq!(&value, &want, "set {:?} in {:?}", path, src);
            }
            let old = reference::removal_span(&src, path);
            if let Some(old_span) = old.span {
                let want = (!old.through_alias).then_some(old_span);
                proptest::prop_assert_eq!(&removal, &want, "remove {:?} in {:?}", path, src);
            }
        }
    }
}
