use crate::level::Level;
use crate::rule::RuleResult;

#[derive(Debug, Clone)]
pub struct Report {
    pub results: Vec<RuleResult>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.level == Level::Error && !r.violations.is_empty())
    }

    pub fn has_warnings(&self) -> bool {
        self.results
            .iter()
            .any(|r| r.level == Level::Warning && !r.violations.is_empty())
    }

    pub fn total_violations(&self) -> usize {
        self.results.iter().map(|r| r.violations.len()).sum()
    }

    pub fn failing_rules(&self) -> usize {
        self.results.iter().filter(|r| !r.passed()).count()
    }

    pub fn passing_rules(&self) -> usize {
        self.results.iter().filter(|r| r.passed()).count()
    }
}
