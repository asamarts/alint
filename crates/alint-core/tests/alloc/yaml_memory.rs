//! The heap a YAML document's JSON tree takes to decode through ayaml, against
//! libyaml (`yaml_serde` 0.10.7, the parser v0.22 used), on generated
//! OpenAPI-shaped documents of about 37 KB and 1.1 MB (ADR-0042).
//!
//! A binary of its own: it installs a counting global allocator, which counts
//! every allocation in the process. Its one test decodes on one thread, so
//! the counts do not depend on timing, and it needs alint-core's
//! `yaml-reference` feature, which brings the reference parser in. The
//! allocator counts a reallocation by the bytes it adds, as the system
//! allocator may grow a block in place.
//!
//! The ratio of the two peaks must stay within [`REGRESSION_BOUND`], today's
//! ratio and about 10%. With `ALINT_YAML_MEMORY_RELEASE=1`, as the release
//! preflight runs it, it must be within [`RELEASE_BOUND`]: v0.23.0 waits for an
//! ayaml release whose peak heap is within 1.2x of libyaml's.

use std::fmt::Write as _;

use alint_core::yaml_reference::{ayaml_json_tree, libyaml_json_tree};

#[global_allocator]
static HEAP: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// The most ayaml's peak heap may be, as a multiple of libyaml's, in every
/// run: the ratios measured on 2026-11-16 with ayaml 0.1.2 (1.78 at 37 KB, 1.73 at
/// 1.1 MB), and about 10%.
const REGRESSION_BOUND: f64 = 1.95;

/// The most it may be when the release preflight runs it (ADR-0042).
const RELEASE_BOUND: f64 = 1.2;

/// The variable that applies [`RELEASE_BOUND`] when set to `1`.
const RELEASE_VARIABLE: &str = "ALINT_YAML_MEMORY_RELEASE";

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

/// The peak heap of a decode, above what was live before it.
#[derive(Clone, Copy, Debug)]
enum Peak {
    /// The decode set the process's highest count, so this is its peak.
    Exactly(usize),
    /// The process had counted more before, so the decode's peak is at most
    /// this: the allocator keeps the highest count, which cannot be reset.
    AtMost(usize),
}

impl Peak {
    fn bytes(self) -> usize {
        match self {
            Self::Exactly(bytes) | Self::AtMost(bytes) => bytes,
        }
    }
}

/// The heap `decode` takes at its peak, and what its result keeps.
fn heap_of<T>(decode: impl FnOnce() -> T) -> (Peak, usize, T) {
    let before = HEAP.allocated();
    let highest_before = HEAP.max_allocated();
    let value = decode();
    let highest = HEAP.max_allocated();
    let peak = if highest > highest_before {
        Peak::Exactly(highest - before)
    } else {
        Peak::AtMost(highest_before - before)
    };
    (peak, HEAP.allocated() - before, value)
}

#[test]
fn ayaml_takes_at_most_the_bound_times_libyamls_heap() {
    let release = std::env::var(RELEASE_VARIABLE).is_ok_and(|value| value == "1");
    let bound = if release {
        RELEASE_BOUND
    } else {
        REGRESSION_BOUND
    };
    // Whatever either parser allocates once per process, before the counts.
    let warm = "a: [1, {b: c}]\n";
    assert_eq!(
        ayaml_json_tree(warm).unwrap(),
        libyaml_json_tree(warm).unwrap()
    );

    let mut report = Vec::new();
    let mut over = Vec::new();
    // Smaller documents first, and libyaml first at each size: libyaml's
    // peak, the measure, is then the highest count yet, and ayaml's is too
    // unless it is at most libyaml's.
    for (size, paths) in [("37 KB", 40), ("1.1 MB", 1200)] {
        let text = openapi(paths);
        let (libyaml, libyaml_kept, libyaml_tree) = heap_of(|| libyaml_json_tree(&text));
        let libyaml_tree = libyaml_tree.expect("libyaml decodes the document");
        let Peak::Exactly(libyaml) = libyaml else {
            panic!("at {size}, libyaml's peak is not the highest count yet: {libyaml:?}");
        };
        drop(libyaml_tree);
        let (ayaml, ayaml_kept, ayaml_tree) = heap_of(|| ayaml_json_tree(&text));
        let ayaml_tree = ayaml_tree.expect("ayaml decodes the document");
        assert_eq!(
            ayaml_kept, libyaml_kept,
            "the two trees' heap differs at {size}"
        );
        drop(ayaml_tree);
        #[expect(clippy::cast_precision_loss, reason = "ratios of heap sizes")]
        let (ratio, per_byte) = (ayaml.bytes() as f64 / libyaml as f64, |bytes: usize| {
            bytes as f64 / text.len() as f64
        });
        let at_most = if matches!(ayaml, Peak::AtMost(_)) {
            "at most "
        } else {
            ""
        };
        let line = format!(
            "{size} ({} bytes): ayaml peak {at_most}{} B ({:.1}/input byte), libyaml peak \
             {libyaml} B ({:.1}/input byte), ratio {at_most}{ratio:.3}; the tree keeps \
             {ayaml_kept} B ({:.1}/input byte)",
            text.len(),
            ayaml.bytes(),
            per_byte(ayaml.bytes()),
            per_byte(libyaml),
            per_byte(ayaml_kept),
        );
        if ratio > bound {
            over.push(line.clone());
        }
        report.push(line);
    }
    eprintln!("{}", report.join("\n"));
    assert!(
        over.is_empty(),
        "ayaml's peak heap is over {bound}x libyaml's ({}):\n{}",
        if release {
            "the release condition of ADR-0042"
        } else {
            "the regression bound"
        },
        over.join("\n")
    );
}
