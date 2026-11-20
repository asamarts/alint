//! The heap a YAML document's JSON tree takes to decode through ayaml, against
//! libyaml (`yaml_serde` 0.10.7, the parser v0.22 used), on generated
//! documents of each shape in [`SHAPES`]: OpenAPI-shaped ones of about 37 KB,
//! 1.1 MB and 4.4 MB, and of about 1 MB block and flow mappings, flow
//! sequences of integers and block sequences of strings (ADR-0042).
//!
//! A binary of its own: it installs a counting global allocator, which counts
//! every allocation in the process, and keeps the highest count, which cannot
//! be reset. So its one test measures each parser for each shape in a process
//! of its own: it runs this binary again for libyaml and for ayaml on each
//! shape, each decoding the shape's document on one thread and printing its
//! exact peak heap and kept bytes. Measuring the two parsers in separate
//! processes gives ayaml's exact peak even when it stays below libyaml's. The
//! counts do not depend on timing, and the test needs alint-core's
//! `yaml-reference` feature, which brings the reference parser in. The
//! allocator counts a reallocation by the bytes it adds, as the system
//! allocator may grow a block in place.
//!
//! The ratio of the two peaks must stay within each shape's regression bound,
//! its ratio with ayaml 0.2.1 and about 10%. With
//! `ALINT_YAML_MEMORY_RELEASE=1`, as the release preflight runs it, it must be
//! within [`RELEASE_BOUND`] for every shape: v0.23.0 requires an ayaml release
//! whose peak heap is within 1.2x of libyaml's.

use std::fmt::Write as _;

use alint_core::yaml_reference::{ayaml_json_tree, libyaml_json_tree};

#[global_allocator]
static HEAP: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// The most ayaml's peak heap may be, as a multiple of libyaml's, for every
/// shape when the release preflight runs the test (ADR-0042).
const RELEASE_BOUND: f64 = 1.2;

/// The variable that applies [`RELEASE_BOUND`] when set to `1`.
const RELEASE_VARIABLE: &str = "ALINT_YAML_MEMORY_RELEASE";

/// The variable that names the shape a run of this binary measures; the test
/// sets it for each run it starts.
const SHAPE_VARIABLE: &str = "ALINT_YAML_MEMORY_SHAPE";

/// The variable that picks which parser (`libyaml` or `ayaml`) a child run
/// measures.
const PARSER_VARIABLE: &str = "ALINT_YAML_MEMORY_PARSER";

/// The prefix of the line a measuring run prints its peak on.
const MEASURED: &str = "yaml-memory-measured:";

/// A document shape the test measures.
struct Shape {
    name: &'static str,
    /// The most ayaml's peak heap may be, as a multiple of libyaml's, in
    /// every run: the shape's ratio with ayaml 0.2.1, measured on 2026-11-20,
    /// and about 10%.
    regression_bound: f64,
    document: fn() -> String,
}

/// The shapes, each with its ratio with ayaml 0.2.1 in its bound's comment.
const SHAPES: &[Shape] = &[
    // 0.578
    Shape {
        name: "OpenAPI, 37 KB",
        regression_bound: 0.64,
        document: || openapi(40),
    },
    // 0.583
    Shape {
        name: "OpenAPI, 1.1 MB",
        regression_bound: 0.65,
        document: || openapi(1_200),
    },
    // 0.584
    Shape {
        name: "OpenAPI, 4.4 MB",
        regression_bound: 0.65,
        document: || openapi(4_800),
    },
    // 0.673
    Shape {
        name: "block mapping of `kN: 1`, 1 MB",
        regression_bound: 0.75,
        document: block_mapping,
    },
    // 0.626
    Shape {
        name: "flow mapping of `kN: 1`, 1 MB",
        regression_bound: 0.70,
        document: flow_mapping,
    },
    // 0.517
    Shape {
        name: "flow sequence of integers, 1 MB",
        regression_bound: 0.57,
        document: flow_sequence,
    },
    // 0.719
    Shape {
        name: "flow sequence of `1`, 1 MB",
        regression_bound: 0.80,
        document: flow_ones,
    },
    // 0.731
    Shape {
        name: "block sequence of `value N`, 1 MB",
        regression_bound: 0.81,
        document: block_sequence,
    },
    // 0.965
    Shape {
        name: "block sequence of words, 1 MB",
        regression_bound: 1.07,
        document: block_words,
    },
];

/// The size of the documents of every shape but the OpenAPI-shaped ones.
const MEGABYTE: usize = 1_000_000;

/// A fixed sequence of numbers (xorshift64*), without a dependency.
struct Numbers(u64);

impl Numbers {
    fn below(&mut self, below: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;
        usize::try_from(value).unwrap() % below
    }

    fn word(&mut self) -> &'static str {
        const WORDS: [&str; 10] = [
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        ];
        WORDS[self.below(WORDS.len())]
    }
}

/// An OpenAPI-shaped document with `paths` path items, each with a `get` and a
/// `post` of three parameters (about 920 bytes a path item), the same in
/// every run.
fn openapi(paths: usize) -> String {
    let mut numbers = Numbers(0x0A11_E47A_11E4_7A11);
    let mut out = String::from("openapi: 3.0.0\ninfo:\n  title: big\n  version: 1.0.0\npaths:\n");
    for i in 0..paths {
        writeln!(out, "  /api/v1/{}/{i}:", numbers.word()).unwrap();
        for verb in ["get", "post"] {
            let summary: Vec<&str> = (0..6).map(|_| numbers.word()).collect();
            writeln!(
                out,
                "    {verb}:\n      operationId: op{verb}{i}\n      summary: \"{}\"\n      \
                 tags: [{}, {}]\n      parameters:",
                summary.join(" "),
                numbers.word(),
                numbers.word()
            )
            .unwrap();
            for p in 0..3 {
                writeln!(
                    out,
                    "        - name: p{p}\n          in: query\n          \
                     schema: {{type: string, maxLength: {}}}",
                    1 + numbers.below(99)
                )
                .unwrap();
            }
            out += "      responses:\n        '200':\n          description: ok\n";
        }
    }
    out
}

/// A block mapping of `kN: 1` lines.
fn block_mapping() -> String {
    let mut out = String::new();
    for i in 0.. {
        if out.len() >= MEGABYTE {
            break;
        }
        writeln!(out, "k{i}: 1").unwrap();
    }
    out
}

/// A flow mapping of `kN: 1` entries on one line.
fn flow_mapping() -> String {
    let mut out = String::from("{");
    for i in 0.. {
        if out.len() >= MEGABYTE {
            break;
        }
        write!(out, "k{i}: 1, ").unwrap();
    }
    out.push_str("}\n");
    out
}

/// A flow sequence of integers below 100,000, twenty to a line.
fn flow_sequence() -> String {
    let mut numbers = Numbers(0x5EED_0F1A_7E6E_2500);
    let mut out = String::from("[");
    for i in 0.. {
        if out.len() >= MEGABYTE {
            break;
        }
        let separator = if i % 20 == 0 { "\n  " } else { " " };
        write!(out, "{separator}{},", numbers.below(100_000)).unwrap();
    }
    out.push_str("\n]\n");
    out
}

/// A flow sequence of `1` on one line, the shape whose parse at the input cap
/// peaked highest (ADR-0042).
fn flow_ones() -> String {
    let mut out = "[1".to_string();
    out.push_str(&",1".repeat((MEGABYTE - 3) / 2));
    out.push_str("]\n");
    out
}

/// A block sequence of `value N` lines.
fn block_sequence() -> String {
    let mut out = String::new();
    for i in 0.. {
        if out.len() >= MEGABYTE {
            break;
        }
        writeln!(out, "- value {i}").unwrap();
    }
    out
}

/// A block sequence of one word a line.
fn block_words() -> String {
    let mut numbers = Numbers(0xB10C_5E90_0F57);
    let mut out = String::new();
    while out.len() < MEGABYTE {
        writeln!(out, "- {}", numbers.word()).unwrap();
    }
    out
}

/// The heap `decode` takes at its peak, and what its result keeps.
fn heap_of<T>(decode: impl FnOnce() -> T) -> (Result<usize, usize>, usize, T) {
    let before = HEAP.allocated();
    let highest_before = HEAP.max_allocated();
    let value = decode();
    let highest = HEAP.max_allocated();
    let peak = if highest > highest_before {
        Ok(highest - before)
    } else {
        Err(highest_before - before)
    };
    (peak, HEAP.allocated() - before, value)
}

/// What one child run found for a shape and one parser.
struct SingleMeasure {
    len: usize,
    peak: usize,
    kept: usize,
}

impl SingleMeasure {
    fn line(&self) -> String {
        format!("{MEASURED} {} {} {}", self.len, self.peak, self.kept)
    }

    fn parse(line: &str) -> Option<Self> {
        let (_, fields) = line.split_once(MEASURED)?;
        let fields: Vec<usize> = fields
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let [len, peak, kept] = fields[..] else {
            return None;
        };
        Some(Self { len, peak, kept })
    }
}

/// What the two measuring runs found for a shape.
struct Measure {
    len: usize,
    libyaml: usize,
    ayaml: usize,
    kept: usize,
}

impl Measure {
    #[expect(clippy::cast_precision_loss, reason = "ratios of heap sizes")]
    fn ratio(&self) -> f64 {
        self.ayaml as f64 / self.libyaml as f64
    }

    #[expect(clippy::cast_precision_loss, reason = "ratios of heap sizes")]
    fn per_byte(&self, bytes: usize) -> f64 {
        bytes as f64 / self.len as f64
    }
}

/// Decode the document of the shape named `name` through `parser`, in a
/// process that has decoded nothing else, and print what it took.
fn measure(name: &str, parser: &str) {
    let shape = SHAPES
        .iter()
        .find(|shape| shape.name == name)
        .unwrap_or_else(|| panic!("no shape is named {name:?}"));
    // Whatever either parser allocates once per process, before the counts.
    let warm = "a: [1, {b: c}]\n";
    let (peak, kept) = match parser {
        "libyaml" => {
            let _ = libyaml_json_tree(warm).unwrap();
            let text = (shape.document)();
            let (peak, kept, tree) = heap_of(|| libyaml_json_tree(&text));
            let tree = tree.expect("libyaml decodes the document");
            drop(tree);
            let peak = peak.unwrap_or_else(|at_most| {
                panic!(
                    "for {name}, libyaml's peak is not the highest count yet (at most {at_most})"
                )
            });
            (peak, kept)
        }
        "ayaml" => {
            let _ = ayaml_json_tree(warm).unwrap();
            let text = (shape.document)();
            let (peak, kept, tree) = heap_of(|| ayaml_json_tree(&text));
            let tree = tree.expect("ayaml decodes the document");
            drop(tree);
            let peak = peak.unwrap_or_else(|at_most| {
                panic!("for {name}, ayaml's peak is not the highest count yet (at most {at_most})")
            });
            (peak, kept)
        }
        other => panic!("unknown parser {other:?}"),
    };
    let len = (shape.document)().len();
    let measure = SingleMeasure { len, peak, kept };
    println!("{}", measure.line());
}

/// Run this binary's test again for one shape and one parser, and return what
/// it measured.
fn measure_one(name: &str, parser: &str) -> SingleMeasure {
    let test = "ayaml_takes_at_most_the_bound_times_libyamls_heap";
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(SHAPE_VARIABLE, name)
        .env(PARSER_VARIABLE, parser)
        .output()
        .expect("run the test binary for one shape");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let measure = stdout.lines().find_map(SingleMeasure::parse);
    match (output.status.success(), measure) {
        (true, Some(measure)) => measure,
        _ => panic!(
            "measuring {name} with {parser} failed ({}):\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// Measure `name` through libyaml and ayaml in separate processes, checking
/// that both keep the same tree heap.
fn measure_apart(name: &str) -> Measure {
    let libyaml = measure_one(name, "libyaml");
    let ayaml = measure_one(name, "ayaml");
    assert_eq!(libyaml.len, ayaml.len, "document length differs for {name}");
    assert_eq!(
        ayaml.kept, libyaml.kept,
        "the two trees' heap differs for {name}"
    );
    Measure {
        len: ayaml.len,
        libyaml: libyaml.peak,
        ayaml: ayaml.peak,
        kept: ayaml.kept,
    }
}

#[test]
fn ayaml_takes_at_most_the_bound_times_libyamls_heap() {
    if let Ok(name) = std::env::var(SHAPE_VARIABLE) {
        let parser = std::env::var(PARSER_VARIABLE).expect("parser variable set for child run");
        measure(&name, &parser);
        return;
    }
    let release = std::env::var(RELEASE_VARIABLE).is_ok_and(|value| value == "1");
    let mut report = Vec::new();
    let mut over = Vec::new();
    for shape in SHAPES {
        let measure = measure_apart(shape.name);
        let bound = if release {
            RELEASE_BOUND
        } else {
            shape.regression_bound
        };
        let line = format!(
            "{} ({} bytes): ayaml peak {} B ({:.1}/input byte), libyaml peak {} B \
             ({:.1}/input byte), ratio {:.3} (bound {bound}); the tree keeps {} B \
             ({:.1}/input byte)",
            shape.name,
            measure.len,
            measure.ayaml,
            measure.per_byte(measure.ayaml),
            measure.libyaml,
            measure.per_byte(measure.libyaml),
            measure.ratio(),
            measure.kept,
            measure.per_byte(measure.kept),
        );
        if measure.ratio() > bound {
            over.push(line.clone());
        }
        report.push(line);
    }
    eprintln!("{}", report.join("\n"));
    assert!(
        over.is_empty(),
        "ayaml's peak heap is over the bound times libyaml's ({}):\n{}",
        if release {
            "the release condition of ADR-0042"
        } else {
            "the regression bound"
        },
        over.join("\n")
    );
}
