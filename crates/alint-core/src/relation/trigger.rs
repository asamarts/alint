//! Whether a rule that reads relations runs under `--changed` (design
//! `v0.23/selectors-and-constraints.md` section 9): its trigger scope.
//!
//! A `constraint` rule's trigger scope is the union of the selectors of
//! every relation its plan reads: its input, both sides of each join, the
//! relation each membership filter (`in:`, `not_in:`) probes, and the
//! relation its `set` compares against, followed through the plan. Under
//! `--changed`, the rule runs when a changed path, added, modified or
//! deleted, is one a selector reads, and then judges the whole snapshot. The
//! scope decides only whether the rule runs, never what it sees.
//!
//! A selector reads a path that its `paths:` and its `scope_filter:`
//! predicates keep, so the predicates narrow the trigger scope as they narrow
//! the selector. They are judged on the tree after the change, so a change to
//! what they consult also triggers the rule: a file named as a
//! `has_ancestor` marker in a directory where the selector's `paths:` can
//! match (at or below it), and the manifest a manifest-derived predicate
//! reads. With neither changed, a predicate's verdict on a deleted path is
//! the one it had before the change. A selector that does not compile, or
//! that uses `changed_since` (which the loader refuses), takes every path:
//! the rule then runs and reports why it cannot read.
//!
//! An ignore file the walk reads ([`IGNORE_FILE_NAMES`]) decides which files
//! below its directory are in the index, so a change to one triggers every
//! selector whose `paths:` can match there.

use std::path::{Path, PathBuf};

use crate::{FileIndex, Scope, ScopeFilter};

use super::plan::{Member, Plan, Predicate, RelationId, Rows, SelectorId, SelectorRef};

/// The ignore files the walk reads in every directory it lists, with
/// `respect_gitignore` on (`crate::walker_ignore_files`). The others it reads
/// (`.git/info/exclude`, the global excludes file) are never in a diff.
const IGNORE_FILE_NAMES: [&str; 2] = [".gitignore", ".ignore"];

/// The selectors a rule's relations read, compiled, as a trigger under
/// `--changed`.
#[derive(Debug, Clone, Default)]
pub struct TriggerScope {
    selectors: Vec<TriggerSelector>,
}

#[derive(Debug, Clone)]
struct TriggerSelector {
    /// The selector as the relation layer compiles it; `None` when it does
    /// not compile or uses `changed_since`, and then it takes every path.
    scope: Option<Scope>,
    /// The manifests its manifest-derived predicates read, root-relative.
    manifests: Vec<PathBuf>,
}

impl TriggerScope {
    /// The trigger scope of a rule that reads `relations` of `plan`.
    #[must_use]
    pub fn of(plan: &Plan, relations: &[RelationId]) -> Self {
        let mut walk = Walk {
            plan,
            relations_seen: vec![false; plan.relations.len()],
            selectors_seen: vec![false; plan.selectors.len()],
            selectors: Vec::new(),
        };
        for &id in relations {
            walk.relation(id);
        }
        Self {
            selectors: walk.selectors,
        }
    }

    /// Whether a change to `path` (root-relative, as `--changed` lists it)
    /// makes the rule run.
    #[must_use]
    pub fn matches(&self, path: &Path, index: &FileIndex) -> bool {
        self.selectors
            .iter()
            .any(|selector| selector.matches(path, index, true))
    }

    /// [`Self::matches`] with every manifest-derived predicate left out, for
    /// when the engine has not resolved the manifests yet.
    pub(crate) fn matches_without_manifest(&self, path: &Path, index: &FileIndex) -> bool {
        self.selectors
            .iter()
            .any(|selector| selector.matches(path, index, false))
    }

    /// The manifest-derived predicates of every selector, for the engine to
    /// resolve once per run with the rules' own.
    pub(crate) fn manifest_predicates(
        &self,
    ) -> impl Iterator<Item = &crate::scope_filter::ManifestPredicate> {
        self.selectors
            .iter()
            .filter_map(|selector| selector.scope.as_ref()?.scope_filter())
            .flat_map(ScopeFilter::manifest_predicates)
    }
}

impl TriggerSelector {
    fn matches(&self, path: &Path, index: &FileIndex, manifests: bool) -> bool {
        let Some(scope) = &self.scope else {
            return true;
        };
        // An ignore file decides which files below its directory the walk
        // indexes, and so which ones the selector reads there.
        if index.reads_ignore_files()
            && path
                .file_name()
                .is_some_and(|name| IGNORE_FILE_NAMES.iter().any(|ignore| name == *ignore))
            && path.parent().is_some_and(|dir| scope.may_match_below(dir))
        {
            return true;
        }
        let Some(filter) = scope.scope_filter() else {
            return scope.matches(path, index);
        };
        // What the predicates consult changed: what the selector picks may
        // have changed with it. A marker counts for the files at and below
        // its directory, so only where the selector's paths can match.
        if filter.has_ancestor_names().iter().any(|marker| {
            marker_directory(path, marker).is_some_and(|dir| scope.may_match_below(dir))
        }) || self.manifests.iter().any(|manifest| manifest == path)
        {
            return true;
        }
        // A manifest the engine has not read, or could not, keeps every path
        // the other predicates keep: the rule runs and reports the failure.
        let resolved = manifests
            && filter.manifest_predicates().iter().all(|predicate| {
                let key = predicate.cache_key();
                index.manifest_paths(key).is_some() && index.manifest_failure(key).is_none()
            });
        if resolved {
            scope.matches(path, index)
        } else {
            scope.matches_without_manifest(path, index)
        }
    }
}

/// The directory `path` names a `has_ancestor` marker in, when it is one:
/// `crates/api` for `crates/api/Cargo.toml` and the marker `Cargo.toml`.
fn marker_directory<'p>(path: &'p Path, marker: &Path) -> Option<&'p Path> {
    if !path.ends_with(marker) {
        return None;
    }
    path.ancestors().nth(marker.components().count())
}

/// The walk over the plan from a rule's relations to every selector they
/// read, each relation and named selector once.
struct Walk<'p> {
    plan: &'p Plan,
    relations_seen: Vec<bool>,
    selectors_seen: Vec<bool>,
    selectors: Vec<TriggerSelector>,
}

impl Walk<'_> {
    fn relation(&mut self, id: RelationId) {
        if std::mem::replace(&mut self.relations_seen[id.0], true) {
            return;
        }
        let relation = &self.plan.relation(id).relation;
        match &relation.rows {
            Rows::Part(part) => self.selector(&part.from),
            Rows::Union(parts) => {
                for part in parts {
                    self.selector(&part.from);
                }
            }
            Rows::Join(join) => {
                self.relation(join.left.id);
                self.relation(join.right.id);
            }
        }
        for (_, predicates) in &relation.filters {
            for predicate in predicates {
                let (Predicate::In(members) | Predicate::NotIn(members)) = predicate else {
                    continue;
                };
                for member in members {
                    if let Member::Relation { relation, .. } = member {
                        self.relation(relation.id);
                    }
                }
            }
        }
    }

    fn selector(&mut self, from: &SelectorRef) {
        let (selector, name) = match from {
            SelectorRef::Named(reached) => {
                let SelectorId(index) = reached.id;
                if std::mem::replace(&mut self.selectors_seen[index], true) {
                    return;
                }
                let named = self.plan.selector(reached.id);
                (&named.selector, format!("selector `{}`", named.name))
            }
            SelectorRef::Inline(selector) => (selector.as_ref(), "the selector".to_string()),
        };
        let scope = selector.compile(&name).ok().filter(|scope| {
            scope
                .scope_filter()
                .and_then(ScopeFilter::changed_since)
                .is_none()
        });
        let manifests = scope
            .as_ref()
            .and_then(Scope::scope_filter)
            .map_or(&[][..], ScopeFilter::manifest_predicates)
            .iter()
            .filter_map(|predicate| crate::pathsafe::normalize_confined(predicate.source()))
            .collect();
        self.selectors.push(TriggerSelector { scope, manifests });
    }
}
