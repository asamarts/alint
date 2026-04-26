use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::error::Result;
use crate::facts::{FactSpec, FactValues, evaluate_facts};
use crate::registry::RuleRegistry;
use crate::report::{FixItem, FixReport, FixRuleResult, FixStatus, Report};
use crate::rule::{Context, FixContext, FixOutcome, Rule, RuleResult, Violation};
use crate::walker::FileIndex;
use crate::when::{WhenEnv, WhenExpr};

/// A rule bundled with an optional `when` expression. Rules with a `when`
/// that evaluates to false at runtime are skipped (no `RuleResult` is
/// produced) — same observable effect as `level: off`, but gated on facts.
#[derive(Debug)]
pub struct RuleEntry {
    pub rule: Box<dyn Rule>,
    pub when: Option<WhenExpr>,
}

impl RuleEntry {
    pub fn new(rule: Box<dyn Rule>) -> Self {
        Self { rule, when: None }
    }

    #[must_use]
    pub fn with_when(mut self, expr: WhenExpr) -> Self {
        self.when = Some(expr);
        self
    }
}

/// Executes a set of rules against a pre-built [`FileIndex`].
///
/// The engine owns a [`RuleRegistry`] so cross-file rules (e.g.
/// `for_each_dir`) can build nested rules on demand during evaluation.
/// Optional `facts` and `vars` (set via the builder chain) are evaluated
/// at run time and threaded into each rule's [`Context`] and into the
/// `when` expression evaluator that gates rules.
#[derive(Debug)]
pub struct Engine {
    entries: Vec<RuleEntry>,
    registry: RuleRegistry,
    facts: Vec<FactSpec>,
    vars: HashMap<String, String>,
    fix_size_limit: Option<u64>,
    /// In `--changed` mode, the set of paths (relative to root)
    /// that the user wants linted. `None` means "full check"; the
    /// engine bypasses every changed-set short-circuit. See
    /// [`Engine::with_changed_paths`] for the contract.
    changed_paths: Option<HashSet<PathBuf>>,
}

impl Engine {
    /// Backward-compatible: wrap each rule in a [`RuleEntry`] with no `when`.
    pub fn new(rules: Vec<Box<dyn Rule>>, registry: RuleRegistry) -> Self {
        let entries = rules.into_iter().map(RuleEntry::new).collect();
        Self {
            entries,
            registry,
            facts: Vec::new(),
            vars: HashMap::new(),
            fix_size_limit: Some(1 << 20),
            changed_paths: None,
        }
    }

    /// Construct from rule entries (each carrying an optional `when`).
    pub fn from_entries(entries: Vec<RuleEntry>, registry: RuleRegistry) -> Self {
        Self {
            entries,
            registry,
            facts: Vec::new(),
            vars: HashMap::new(),
            fix_size_limit: Some(1 << 20),
            changed_paths: None,
        }
    }

    #[must_use]
    pub fn with_fix_size_limit(mut self, limit: Option<u64>) -> Self {
        self.fix_size_limit = limit;
        self
    }

    #[must_use]
    pub fn with_facts(mut self, facts: Vec<FactSpec>) -> Self {
        self.facts = facts;
        self
    }

    #[must_use]
    pub fn with_vars(mut self, vars: HashMap<String, String>) -> Self {
        self.vars = vars;
        self
    }

    /// Restrict evaluation to the given set of paths (relative to
    /// the alint root). Per-file rules see a [`FileIndex`]
    /// filtered to only these paths; rules that override
    /// [`Rule::requires_full_index`] (cross-file + existence
    /// rules) still see the full index but are skipped when
    /// their [`Rule::path_scope`] doesn't intersect the set.
    ///
    /// An empty set short-circuits to a no-op report — there's
    /// nothing to lint. Pass `None` (or omit) to disable
    /// `--changed` semantics entirely.
    #[must_use]
    pub fn with_changed_paths(mut self, set: HashSet<PathBuf>) -> Self {
        self.changed_paths = Some(set);
        self
    }

    pub fn rule_count(&self) -> usize {
        self.entries.len()
    }

    pub fn run(&self, root: &Path, index: &FileIndex) -> Result<Report> {
        // Empty changed-set fast path: nothing to lint, return
        // an empty report rather than walk the entries list at
        // all. Saves the fact-evaluation pass too.
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty) {
            return Ok(Report {
                results: Vec::new(),
            });
        }

        let fact_values = evaluate_facts(&self.facts, root, index)?;
        let git_tracked = self.collect_git_tracked_if_needed(root);
        let filtered_index = self.build_filtered_index(index);
        let full_ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
        };
        let filtered_ctx = filtered_index.as_ref().map(|fi| Context {
            root,
            index: fi,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
        });
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
        };
        let results: Vec<RuleResult> = self
            .entries
            .par_iter()
            .filter_map(|entry| {
                if self.skip_for_changed(entry.rule.as_ref()) {
                    return None;
                }
                let ctx = pick_ctx(entry.rule.as_ref(), &full_ctx, filtered_ctx.as_ref());
                run_entry(entry, ctx, &when_env, &fact_values)
            })
            .collect();
        Ok(Report { results })
    }

    /// Evaluate every rule and apply fixers for their violations.
    /// Fixes run sequentially — rules whose fixers touch the filesystem
    /// must not race. Rules with no fixer contribute
    /// [`FixStatus::Unfixable`] entries so the caller sees them in the
    /// report. Rules that pass (no violations) are omitted from the
    /// result, same as [`Engine::run`]'s usual behaviour.
    pub fn fix(&self, root: &Path, index: &FileIndex, dry_run: bool) -> Result<FixReport> {
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty) {
            return Ok(FixReport {
                results: Vec::new(),
            });
        }

        let fact_values = evaluate_facts(&self.facts, root, index)?;
        let git_tracked = self.collect_git_tracked_if_needed(root);
        let filtered_index = self.build_filtered_index(index);
        let full_ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
        };
        let filtered_ctx = filtered_index.as_ref().map(|fi| Context {
            root,
            index: fi,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
        });
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
        };
        let fix_ctx = FixContext {
            root,
            dry_run,
            fix_size_limit: self.fix_size_limit,
        };

        let mut results: Vec<FixRuleResult> = Vec::new();
        for entry in &self.entries {
            if self.skip_for_changed(entry.rule.as_ref()) {
                continue;
            }
            let ctx = pick_ctx(entry.rule.as_ref(), &full_ctx, filtered_ctx.as_ref());
            if let Some(expr) = &entry.when {
                match expr.evaluate(&when_env) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        results.push(FixRuleResult {
                            rule_id: entry.rule.id().to_string(),
                            level: entry.rule.level(),
                            items: vec![FixItem {
                                violation: Violation::new(format!("when evaluation error: {e}")),
                                status: FixStatus::Unfixable,
                            }],
                        });
                        continue;
                    }
                }
            }
            let violations = match entry.rule.evaluate(ctx) {
                Ok(v) => v,
                Err(e) => vec![Violation::new(format!("rule error: {e}"))],
            };
            if violations.is_empty() {
                continue;
            }
            let fixer = entry.rule.fixer();
            let items: Vec<FixItem> = violations
                .into_iter()
                .map(|v| {
                    let status = match fixer {
                        Some(f) => match f.apply(&v, &fix_ctx) {
                            Ok(FixOutcome::Applied(s)) => FixStatus::Applied(s),
                            Ok(FixOutcome::Skipped(s)) => FixStatus::Skipped(s),
                            Err(e) => FixStatus::Skipped(format!("fix error: {e}")),
                        },
                        None => FixStatus::Unfixable,
                    };
                    FixItem {
                        violation: v,
                        status,
                    }
                })
                .collect();
            results.push(FixRuleResult {
                rule_id: entry.rule.id().to_string(),
                level: entry.rule.level(),
                items,
            });
        }
        Ok(FixReport { results })
    }

    /// Collect git's tracked-paths set, but only if at least one
    /// loaded rule asked for it. Most repos / configs never opt
    /// in, so this returns `None` zero-cost in the common case.
    /// Inside a non-git directory, or when `git` exits non-zero
    /// (corrupt repo, missing binary), the helper also returns
    /// `None` — rules that consult it then treat every entry as
    /// "untracked," which is the right default for absence-style
    /// rules with `git_tracked_only: true`.
    fn collect_git_tracked_if_needed(
        &self,
        root: &Path,
    ) -> Option<std::collections::HashSet<std::path::PathBuf>> {
        let any_wants = self.entries.iter().any(|e| e.rule.wants_git_tracked());
        if !any_wants {
            return None;
        }
        crate::git::collect_tracked_paths(root)
    }

    /// Build a [`FileIndex`] containing only the entries the user
    /// said they care about (the `--changed` set). Returns `None`
    /// when no changed-set is configured — callers fall back to
    /// the full index.
    fn build_filtered_index(&self, full: &FileIndex) -> Option<FileIndex> {
        let set = self.changed_paths.as_ref()?;
        let entries = full
            .entries
            .iter()
            .filter(|e| set.contains(&e.path))
            .cloned()
            .collect();
        Some(FileIndex { entries })
    }

    /// True when `--changed` mode is active AND the rule's
    /// `path_scope` exists AND no path in the changed-set
    /// satisfies it. Cross-file rules return `path_scope = None`
    /// per the roadmap contract — so they always return `false`
    /// here (i.e. never skipped).
    fn skip_for_changed(&self, rule: &dyn Rule) -> bool {
        let Some(set) = &self.changed_paths else {
            return false;
        };
        let Some(scope) = rule.path_scope() else {
            return false;
        };
        !set.iter().any(|p| scope.matches(p))
    }
}

/// Pick the [`Context`] a rule should evaluate against:
/// `full_ctx` if it [`requires_full_index`](Rule::requires_full_index),
/// otherwise the changed-only filtered context (falling back to
/// `full_ctx` when no `--changed` set is configured).
fn pick_ctx<'a>(
    rule: &dyn Rule,
    full_ctx: &'a Context<'a>,
    filtered_ctx: Option<&'a Context<'a>>,
) -> &'a Context<'a> {
    if rule.requires_full_index() {
        full_ctx
    } else {
        filtered_ctx.unwrap_or(full_ctx)
    }
}

fn run_entry(
    entry: &RuleEntry,
    ctx: &Context<'_>,
    when_env: &WhenEnv<'_>,
    _facts: &FactValues,
) -> Option<RuleResult> {
    if let Some(expr) = &entry.when {
        match expr.evaluate(when_env) {
            Ok(true) => {} // proceed
            Ok(false) => return None,
            Err(e) => {
                return Some(RuleResult {
                    rule_id: entry.rule.id().to_string(),
                    level: entry.rule.level(),
                    policy_url: entry.rule.policy_url().map(str::to_string),
                    violations: vec![Violation::new(format!("when evaluation error: {e}"))],
                    is_fixable: entry.rule.fixer().is_some(),
                });
            }
        }
    }
    Some(run_one(entry.rule.as_ref(), ctx))
}

fn run_one(rule: &dyn Rule, ctx: &Context<'_>) -> RuleResult {
    let violations = match rule.evaluate(ctx) {
        Ok(v) => v,
        Err(e) => vec![Violation::new(format!("rule error: {e}"))],
    };
    RuleResult {
        rule_id: rule.id().to_string(),
        level: rule.level(),
        policy_url: rule.policy_url().map(str::to_string),
        violations,
        is_fixable: rule.fixer().is_some(),
    }
}
