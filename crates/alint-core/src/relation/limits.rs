//! Limits on relation work (general-linter section 9.5). Hitting one is an
//! analysis failure (DEC-2), never an empty relation that passes.

use std::sync::atomic::{AtomicUsize, Ordering};

/// One bounded quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LimitKind {
    /// Rows one extractor emits for one resource; every adapter checks it.
    RowsPerExtractor,
    /// Rows all extractors emit in one run; the decode cache's [`RunBudget`]
    /// enforces it.
    RowsPerRun,
    /// Distinct values one relation may hold when converted to a set;
    /// [`Relation::distinct_values`](super::Relation::distinct_values)
    /// enforces it.
    DistinctValues,
    /// Rows entering one equality join, on each side; the relation
    /// evaluator's join enforces it.
    JoinInputs,
    /// Rows one equality join produces; the evaluator's join enforces it.
    JoinOutputs,
    /// Cells the relation evaluator builds in one run: a row of a relation
    /// with F fields holds F cells, and each file a part reads holds one.
    /// The evaluator takes them from the run's count as it builds each row,
    /// whether a part, a `union:`'s parts or a join builds it, so a union of
    /// many parts over one decode fails at the cap rather than after. Every
    /// relation a run evaluates stays in its memo for the run, and a row's
    /// size grows with its fields, so it is the run's cells, not one
    /// relation's rows, that bound what relations hold, with
    /// [`Self::BytesPerRun`] for what a cell holds beyond its size.
    CellsPerRun,
    /// Bytes the relation evaluator holds in one run beyond its cells: each
    /// decode it reads, whole, once (its rows, values and key paths), and
    /// each text it builds (a path token, an `entry.key`, `entry.parent`,
    /// `entry.root` or `entry.path`, a key path a row keeps, a transformed
    /// value, a note, a failure). A value's text is shared, never copied,
    /// so a field or join that copies a value costs a cell and no bytes.
    /// With [`Self::CellsPerRun`] it bounds what a run's relations hold,
    /// whatever the input's shape; the evaluator takes the bytes as it
    /// builds what holds them. Apart from that count, the run's decode cache
    /// keeps a [`RunBudget`] of this many bytes: the key paths every decode
    /// it makes builds take theirs from it as they are built, and
    /// `key_parity` the tables it keeps of its files' keys, so what a run's
    /// decodes hold of key paths is bounded however many files it decodes.
    BytesPerRun,
    /// Bytes of materialized key-path pointers one decode may build
    /// ([`PathBudget`](super::key_paths::PathBudget)). A single deep, wide
    /// file (for example 1 MB of JSON 120 levels deep and 100,000 keys wide)
    /// expands to ~300 MiB of per-row pointers under a 1 GiB `BytesPerRun`
    /// cap; this tighter per-decode cap stops such expansion early while still
    /// allowing large real-world key files.
    KeyPathBytesPerDecode,
    /// Findings one relation-backed rule (`constraint`, `key_parity`) may emit
    /// in a single evaluation. When a rule produces more (for example `G x V`
    /// missing values across empty groups), it stops emitting further findings
    /// at this cap and adds one non-baselinable analysis-failure finding
    /// naming the cap and suppressed count.
    FindingsPerRule,
}

impl LimitKind {
    pub const ALL: [Self; 9] = [
        Self::RowsPerExtractor,
        Self::RowsPerRun,
        Self::DistinctValues,
        Self::JoinInputs,
        Self::JoinOutputs,
        Self::CellsPerRun,
        Self::BytesPerRun,
        Self::KeyPathBytesPerDecode,
        Self::FindingsPerRule,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::RowsPerExtractor => "rows_per_extractor",
            Self::RowsPerRun => "rows_per_run",
            Self::DistinctValues => "distinct_values",
            Self::JoinInputs => "join_inputs",
            Self::JoinOutputs => "join_outputs",
            Self::CellsPerRun => "cells_per_run",
            Self::BytesPerRun => "bytes_per_run",
            Self::KeyPathBytesPerDecode => "key_path_bytes_per_decode",
            Self::FindingsPerRule => "findings_per_rule",
        }
    }

    /// What the limit counts, as [`LimitExceeded`] says it.
    fn unit(self) -> &'static str {
        match self {
            Self::BytesPerRun | Self::KeyPathBytesPerDecode => "bytes",
            Self::CellsPerRun => "cells",
            Self::FindingsPerRule => "findings",
            _ => "rows or values",
        }
    }

    /// The default. Provisional until R2 measures real relations; each is far
    /// above what the v0.12 corpus needs and far below what exhausts memory.
    /// A cell costs about what a decoded row does (100 to 150 bytes, at most
    /// 256 with its share of its row), so a run may hold as many cells as it
    /// may decode rows; with 1 GiB of bytes, the relations of a run at the
    /// defaults hold at most about 2.4 GB.
    #[must_use]
    pub fn default_value(self) -> usize {
        match self {
            Self::RowsPerExtractor | Self::JoinInputs | Self::JoinOutputs => 1_000_000,
            Self::RowsPerRun | Self::CellsPerRun => 5_000_000,
            Self::DistinctValues => 100_000,
            Self::BytesPerRun => 1 << 30,
            Self::KeyPathBytesPerDecode => 32 << 20,
            Self::FindingsPerRule => 10_000,
        }
    }

    /// The largest value a configuration may raise the limit to.
    #[must_use]
    pub fn hard_cap(self) -> usize {
        match self {
            Self::RowsPerExtractor
            | Self::DistinctValues
            | Self::JoinInputs
            | Self::JoinOutputs => 10_000_000,
            Self::RowsPerRun | Self::CellsPerRun => 50_000_000,
            // 16 GiB, or half of what a 32-bit `usize` holds.
            Self::BytesPerRun | Self::KeyPathBytesPerDecode => {
                usize::try_from(16u64 << 30).unwrap_or(usize::MAX >> 1)
            }
            Self::FindingsPerRule => 1_000_000,
        }
    }

    fn slot(self) -> usize {
        self as usize
    }
}

/// A limit was reached: the analysis is incomplete.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("relation limit `{}` ({limit}) reached: {reached} {}", kind.name(), kind.unit())]
pub struct LimitExceeded {
    pub kind: LimitKind,
    pub limit: usize,
    pub reached: usize,
}

/// A configured limit outside `1..=hard_cap`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("relation limit `{}` must be between 1 and {cap}, not {value}", kind.name())]
pub struct LimitConfigError {
    pub kind: LimitKind,
    pub value: usize,
    pub cap: usize,
}

/// The limits one evaluation runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    values: [usize; LimitKind::ALL.len()],
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            values: LimitKind::ALL.map(LimitKind::default_value),
        }
    }
}

impl Limits {
    /// These limits with `kind` set to `value`, refusing zero and anything
    /// above the hard cap.
    pub fn with(mut self, kind: LimitKind, value: usize) -> Result<Self, LimitConfigError> {
        let cap = kind.hard_cap();
        if value == 0 || value > cap {
            return Err(LimitConfigError { kind, value, cap });
        }
        self.values[kind.slot()] = value;
        Ok(self)
    }

    #[must_use]
    pub fn get(&self, kind: LimitKind) -> usize {
        self.values[kind.slot()]
    }

    /// Fail once `count` exceeds the limit for `kind`.
    pub fn check(&self, kind: LimitKind, count: usize) -> Result<(), LimitExceeded> {
        let limit = self.get(kind);
        if count > limit {
            return Err(LimitExceeded {
                kind,
                limit,
                reached: count,
            });
        }
        Ok(())
    }
}

/// A run-wide budget of rows ([`LimitKind::RowsPerRun`]) or bytes
/// ([`LimitKind::BytesPerRun`]), shared by every extractor and consumer that
/// takes from it (possibly on several threads).
#[derive(Debug)]
pub struct RunBudget {
    used: AtomicUsize,
    limit: usize,
    kind: LimitKind,
}

impl RunBudget {
    /// The run's rows: [`LimitKind::RowsPerRun`] in `limits`.
    #[must_use]
    pub fn new(limits: &Limits) -> Self {
        Self::of(limits, LimitKind::RowsPerRun)
    }

    /// The run's bytes: [`LimitKind::BytesPerRun`] in `limits`. The decode
    /// cache keeps one, from which the key paths each decode builds and what
    /// a consumer holds of its decodes take their bytes.
    #[must_use]
    pub fn bytes(limits: &Limits) -> Self {
        Self::of(limits, LimitKind::BytesPerRun)
    }

    fn of(limits: &Limits, kind: LimitKind) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit: limits.get(kind),
            kind,
        }
    }

    /// Take `amount` from the budget, or fail once the run's total would
    /// exceed it. A failed take still counts, so every later take fails too.
    pub fn take(&self, amount: usize) -> Result<(), LimitExceeded> {
        let total = self
            .used
            .fetch_add(amount, Ordering::Relaxed)
            .saturating_add(amount);
        if total > self.limit {
            return Err(LimitExceeded {
                kind: self.kind,
                limit: self.limit,
                reached: total,
            });
        }
        Ok(())
    }

    /// Fail when a take has already exceeded the budget, so every later
    /// take would fail too: a decode then need not be made to fail.
    pub fn spent(&self) -> Result<(), LimitExceeded> {
        let used = self.used();
        if used > self.limit {
            return Err(LimitExceeded {
                kind: self.kind,
                limit: self.limit,
                reached: used,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_limit_has_a_default_within_its_cap_and_a_distinct_name() {
        let limits = Limits::default();
        let mut names = std::collections::BTreeSet::new();
        for kind in LimitKind::ALL {
            assert!(kind.default_value() >= 1);
            assert!(kind.default_value() <= kind.hard_cap(), "{kind:?}");
            assert_eq!(limits.get(kind), kind.default_value());
            assert!(names.insert(kind.name()));
        }
    }

    #[test]
    fn every_limit_fails_one_past_its_value_and_never_below() {
        for kind in LimitKind::ALL {
            let limits = Limits::default().with(kind, 3).unwrap();
            assert_eq!(limits.check(kind, 3), Ok(()));
            assert_eq!(
                limits.check(kind, 4),
                Err(LimitExceeded {
                    kind,
                    limit: 3,
                    reached: 4
                })
            );
            // Other limits keep their defaults.
            for other in LimitKind::ALL.into_iter().filter(|o| *o != kind) {
                assert_eq!(limits.get(other), other.default_value());
            }
        }
    }

    #[test]
    fn a_reached_limit_names_what_it_counts() {
        let reached = |kind| {
            LimitExceeded {
                kind,
                limit: 3,
                reached: 4,
            }
            .to_string()
        };
        assert_eq!(
            reached(LimitKind::BytesPerRun),
            "relation limit `bytes_per_run` (3) reached: 4 bytes"
        );
        assert_eq!(
            reached(LimitKind::KeyPathBytesPerDecode),
            "relation limit `key_path_bytes_per_decode` (3) reached: 4 bytes"
        );
        assert_eq!(
            reached(LimitKind::CellsPerRun),
            "relation limit `cells_per_run` (3) reached: 4 cells"
        );
        assert_eq!(
            reached(LimitKind::FindingsPerRule),
            "relation limit `findings_per_rule` (3) reached: 4 findings"
        );
        for kind in LimitKind::ALL.into_iter().filter(|kind| {
            !matches!(
                kind,
                LimitKind::BytesPerRun
                    | LimitKind::KeyPathBytesPerDecode
                    | LimitKind::CellsPerRun
                    | LimitKind::FindingsPerRule
            )
        }) {
            assert_eq!(
                reached(kind),
                format!(
                    "relation limit `{}` (3) reached: 4 rows or values",
                    kind.name()
                )
            );
        }
    }

    #[test]
    fn configured_limits_stay_within_one_and_the_hard_cap() {
        for kind in LimitKind::ALL {
            let cap = kind.hard_cap();
            assert!(Limits::default().with(kind, cap).is_ok());
            assert_eq!(
                Limits::default().with(kind, cap + 1),
                Err(LimitConfigError {
                    kind,
                    value: cap + 1,
                    cap
                })
            );
            assert!(Limits::default().with(kind, 0).is_err());
        }
    }

    /// The join limits bound the relation evaluator's equality join and
    /// nothing else: a use of one elsewhere must come with the join that it
    /// bounds, and with this test's update.
    #[test]
    fn the_join_limits_bound_only_the_evaluators_join() {
        const JOIN_LIMITS: [LimitKind; 2] = [LimitKind::JoinInputs, LimitKind::JoinOutputs];
        const ENFORCED_IN: [&str; 3] = [
            "relation/limits.rs",
            "relation/eval/ops.rs",
            "relation/eval/tests.rs",
        ];
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace's crates directory");
        let mut pending = vec![crates.to_path_buf()];
        let mut scanned = 0;
        let mut uses = Vec::new();
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|name| name != "target") {
                        pending.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                scanned += 1;
                if ENFORCED_IN.iter().any(|allowed| path.ends_with(allowed)) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for kind in JOIN_LIMITS {
                    if text.contains(&format!("{kind:?}")) {
                        uses.push(format!("{}: {kind:?}", path.display()));
                    }
                }
            }
        }
        assert!(
            scanned > 100,
            "only {scanned} Rust files found under {}",
            crates.display()
        );
        assert!(
            uses.is_empty(),
            "join limits used outside the evaluator's join:\n{}",
            uses.join("\n")
        );
    }

    #[test]
    fn the_run_budget_is_shared_and_stays_exhausted() {
        let limits = Limits::default().with(LimitKind::RowsPerRun, 10).unwrap();
        let budget = RunBudget::new(&limits);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| budget.take(4).unwrap());
            }
        });
        assert_eq!(budget.used(), 8);
        assert!(budget.take(2).is_ok());
        let exceeded = budget.take(1).unwrap_err();
        assert_eq!(exceeded.kind, LimitKind::RowsPerRun);
        assert_eq!(exceeded.reached, 11);
        assert!(
            budget.take(0).is_err(),
            "an exhausted budget stays exhausted"
        );
        assert!(budget.take(usize::MAX).is_err());
    }

    #[test]
    fn a_byte_budget_names_the_byte_limit() {
        let limits = Limits::default().with(LimitKind::BytesPerRun, 10).unwrap();
        let budget = RunBudget::bytes(&limits);
        assert!(budget.take(10).is_ok());
        let exceeded = budget.take(1).unwrap_err();
        assert_eq!(exceeded.kind, LimitKind::BytesPerRun);
        assert_eq!(exceeded.reached, 11);
        assert_eq!(budget.spent().unwrap_err().kind, LimitKind::BytesPerRun);
    }
}
