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
    /// Rows entering one equality join. Reserved: joins arrive in Phase 3,
    /// and nothing in v0.21 joins relations.
    JoinInputs,
    /// Rows one equality join produces. Reserved, like `JoinInputs`.
    JoinOutputs,
}

impl LimitKind {
    pub const ALL: [Self; 5] = [
        Self::RowsPerExtractor,
        Self::RowsPerRun,
        Self::DistinctValues,
        Self::JoinInputs,
        Self::JoinOutputs,
    ];

    /// The limits no code enforces yet, because what they bound does not
    /// exist: equality joins arrive in Phase 3.
    pub const RESERVED: [Self; 2] = [Self::JoinInputs, Self::JoinOutputs];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::RowsPerExtractor => "rows_per_extractor",
            Self::RowsPerRun => "rows_per_run",
            Self::DistinctValues => "distinct_values",
            Self::JoinInputs => "join_inputs",
            Self::JoinOutputs => "join_outputs",
        }
    }

    /// The default. Provisional until R2 measures real relations; each is far
    /// above what the v0.12 corpus needs and far below what exhausts memory.
    #[must_use]
    pub fn default_value(self) -> usize {
        match self {
            Self::RowsPerExtractor | Self::JoinInputs | Self::JoinOutputs => 1_000_000,
            Self::RowsPerRun => 5_000_000,
            Self::DistinctValues => 100_000,
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
            Self::RowsPerRun => 50_000_000,
        }
    }

    fn slot(self) -> usize {
        self as usize
    }
}

/// A limit was reached: the analysis is incomplete.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("relation limit `{}` ({limit}) reached: {reached} rows or values", kind.name())]
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

/// The run-wide row budget, shared by every extractor (possibly on several
/// threads).
#[derive(Debug)]
pub struct RunBudget {
    used: AtomicUsize,
    limit: usize,
}

impl RunBudget {
    #[must_use]
    pub fn new(limits: &Limits) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit: limits.get(LimitKind::RowsPerRun),
        }
    }

    /// Take `rows` from the budget, or fail once the run's total would exceed
    /// it. A failed take still counts, so every later take fails too.
    pub fn take(&self, rows: usize) -> Result<(), LimitExceeded> {
        let total = self
            .used
            .fetch_add(rows, Ordering::Relaxed)
            .saturating_add(rows);
        if total > self.limit {
            return Err(LimitExceeded {
                kind: LimitKind::RowsPerRun,
                limit: self.limit,
                reached: total,
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

    /// The join limits stay reserved until Phase 3 adds joins: a use of one
    /// outside this module must come with the join that it bounds, and with
    /// this test's update.
    #[test]
    fn the_join_limits_are_reserved_and_unused() {
        assert_eq!(
            LimitKind::RESERVED,
            [LimitKind::JoinInputs, LimitKind::JoinOutputs]
        );
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
                if path.extension().is_none_or(|ext| ext != "rs")
                    || path.ends_with("relation/limits.rs")
                {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&path).unwrap();
                for kind in LimitKind::RESERVED {
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
            "reserved join limits used:\n{}",
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
}
