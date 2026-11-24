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
//! that uses `changed_since` (which the loader refuses), takes every change:
//! the rule then runs and reports why it cannot read. No change runs no rule.
//!
//! An ignore file the walk reads ([`IGNORE_FILE_NAMES`]) decides which files
//! below its directory are in the index, so a change to one triggers every
//! selector whose `paths:` can match below it. So does a path Git lists as a
//! directory ([`crate::git::ChangedSet::directories`]), whether or not the
//! walk still holds it: a submodule added, moved or removed, whose files Git
//! does not list, or an untracked repository (`sub/`), whose files it does
//! not list either, on every run until it is committed or ignored.
//!
//! The engine decides once per pass what each rule's scope takes of the
//! changed paths ([`TriggerScope::hits`]), and reads it before and after it
//! resolves the manifests: each selector tests a changed path against its
//! globs at most once, and not at all one outside the directories its
//! `paths:` start with ([`ChangedPaths::candidates`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::scope::ChangedPaths;
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
    /// How many times a selector tested a changed path against its globs.
    /// Test only, shared by the scope's clones.
    #[cfg(test)]
    glob_tests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

/// Which selector of a plan a trigger scope holds, as `explain` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerOrigin {
    /// A named selector.
    Named(SelectorId),
    /// The inline selector of part `part` of relation `relation`: its one
    /// `from:`, or that of the `union:` part at that index.
    Inline { relation: RelationId, part: usize },
}

#[derive(Debug, Clone)]
struct TriggerSelector {
    origin: TriggerOrigin,
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
            #[cfg(test)]
            glob_tests: std::sync::Arc::default(),
        }
    }

    /// The selectors the scope holds, each once, in the order the walk
    /// reached them: the relations in the order given, each relation's
    /// selectors before those of the relations it reads.
    pub fn origins(&self) -> impl Iterator<Item = TriggerOrigin> + '_ {
        self.selectors.iter().map(|selector| selector.origin)
    }

    /// Whether a change to `path` (root-relative, as `--changed` lists it)
    /// makes the rule run, Git having listed it as a file: a path it lists as
    /// a directory runs the rule when the selectors can match below it
    /// ([`crate::Engine::with_changed_set`]).
    #[must_use]
    pub fn matches(&self, path: &Path, index: &FileIndex) -> bool {
        let changed = ChangedPaths::new([path], &std::collections::HashSet::new());
        let mut pending = Vec::new();
        self.hits(&changed, index, &mut pending) || self.pending_runs(&pending, &changed, index)
    }

    /// What the scope takes of `changed`: `true` when a changed path runs
    /// the rule whatever the manifests say. Otherwise each `(selector, path)`
    /// that the selector's globs and other predicates take, for a selector
    /// with manifest-derived predicates, goes to `pending`: it runs the rule
    /// when the manifests keep it too ([`Self::pending_runs`]), and with the
    /// manifests left out, before the engine resolves them, any one does.
    /// Each selector tests a path against its globs at most once, and not
    /// at all one its `paths:` cannot match ([`ChangedPaths::candidates`]).
    pub(crate) fn hits(
        &self,
        changed: &ChangedPaths<'_>,
        index: &FileIndex,
        pending: &mut Vec<(usize, usize)>,
    ) -> bool {
        // No change runs no rule, whatever its selectors.
        if changed.is_empty() {
            return false;
        }
        for (position, selector) in self.selectors.iter().enumerate() {
            // A selector that does not compile, or reads `changed_since:`,
            // cannot say which paths it reads, so any change runs the rule
            // (fail open). The loader rejects both in a version 2 config, so
            // no config reaches this today.
            let Some(scope) = &selector.scope else {
                return true;
            };
            if selector.consulted_changed(scope, changed, index) {
                return true;
            }
            for path in changed.candidates(scope) {
                let changed_path = changed.path(path);
                #[cfg(test)]
                if scope.matches_by_glob() {
                    self.glob_tests
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if !scope.path_matches(changed_path) {
                    continue;
                }
                let Some(filter) = scope.scope_filter() else {
                    return true;
                };
                if !filter.matches_without_manifest(changed_path, index) {
                    continue;
                }
                if filter.manifest_predicates().is_empty() {
                    return true;
                }
                pending.push((position, path));
            }
        }
        false
    }

    /// Whether a `(selector, path)` of `pending` ([`Self::hits`]) runs the
    /// rule with the manifests the engine resolved on `index`. A manifest the
    /// engine has not read, or could not, keeps every path the other
    /// predicates keep: the rule runs and reports the failure.
    pub(crate) fn pending_runs(
        &self,
        pending: &[(usize, usize)],
        changed: &ChangedPaths<'_>,
        index: &FileIndex,
    ) -> bool {
        pending.iter().any(|&(selector, path)| {
            self.selectors[selector].manifests_keep(changed.path(path), index)
        })
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

    /// Whether any selector in this trigger scope filters through a
    /// secret-bearing manifest (such as `.env` or `.properties`).
    #[must_use]
    pub fn has_sensitive_manifest_scope(&self) -> bool {
        self.selectors.iter().any(|selector| {
            selector
                .scope
                .as_ref()
                .is_some_and(|scope| scope.sensitive_manifest_sources().next().is_some())
        })
    }

    /// Visit every repository path matched by any selector in this trigger
    /// scope, plus every manifest path their `scope_filter` predicates consult.
    /// A selector whose scope failed to compile fails closed by visiting every
    /// indexed file.
    pub fn visit_matched_paths(&self, index: &FileIndex, visit: &mut dyn FnMut(&Path)) {
        for selector in &self.selectors {
            for manifest in &selector.manifests {
                visit(manifest);
            }
            match &selector.scope {
                Some(scope) => {
                    for entry in scope.candidates(index) {
                        if scope.matches(&entry.path, index) {
                            visit(&entry.path);
                        }
                    }
                }
                None => {
                    for entry in &index.entries {
                        visit(&entry.path);
                    }
                }
            }
        }
    }

    /// How many times a selector tested a changed path against its globs.
    #[cfg(test)]
    pub(crate) fn glob_tests(&self) -> usize {
        self.glob_tests.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl TriggerSelector {
    /// Whether a changed path changes what the selector reads whatever its
    /// globs say of it: an ignore file, or a directory Git lists whole, where
    /// the selector's paths can match, and what its predicates consult.
    fn consulted_changed(
        &self,
        scope: &Scope,
        changed: &ChangedPaths<'_>,
        index: &FileIndex,
    ) -> bool {
        let at = |path: usize| changed.path(path);
        // An ignore file decides which files below its directory the walk
        // indexes, and so which ones the selector reads there.
        if index.reads_ignore_files()
            && IGNORE_FILE_NAMES.iter().any(|name| {
                changed.named(OsStr::new(name)).iter().any(|&path| {
                    at(path)
                        .parent()
                        .is_some_and(|dir| scope.may_match_below(dir))
                })
            })
        {
            return true;
        }
        // Git lists a submodule, added, moved or removed, or an untracked
        // repository, by its directory alone: what the selector reads below
        // it may have changed.
        if changed
            .directories()
            .iter()
            .any(|&path| scope.may_match_below(at(path)))
        {
            return true;
        }
        let Some(filter) = scope.scope_filter() else {
            return false;
        };
        // What the predicates consult changed: what the selector picks may
        // have changed with it. A marker counts for the files at and below
        // its directory, so only where the selector's paths can match.
        filter.has_ancestor_names().iter().any(|marker| {
            marker.file_name().is_some_and(|name| {
                changed.named(name).iter().any(|&path| {
                    marker_directory(at(path), marker).is_some_and(|dir| scope.may_match_below(dir))
                })
            })
        }) || self.manifests.iter().any(|manifest| {
            manifest.file_name().is_some_and(|name| {
                changed
                    .named(name)
                    .iter()
                    .any(|&path| at(path) == manifest.as_path())
            })
        })
    }

    /// Whether the selector's manifest-derived predicates keep `path`, which
    /// its globs and other predicates take: also when one of its manifests is
    /// not resolved on `index`, or could not be read.
    fn manifests_keep(&self, path: &Path, index: &FileIndex) -> bool {
        let Some(filter) = self.scope.as_ref().and_then(Scope::scope_filter) else {
            return true;
        };
        let resolved = filter.manifest_predicates().iter().all(|predicate| {
            let key = predicate.cache_key();
            index.manifest_paths(key).is_some() && index.manifest_failure(key).is_none()
        });
        !resolved || filter.matches(path, index)
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
            Rows::Part(part) => self.selector(&part.from, id, 0),
            Rows::Union(parts) => {
                for (index, part) in parts.iter().enumerate() {
                    self.selector(&part.from, id, index);
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

    /// The selector `from` of part `part` of relation `relation`.
    fn selector(&mut self, from: &SelectorRef, relation: RelationId, part: usize) {
        let (selector, name, origin) = match from {
            SelectorRef::Named(reached) => {
                let SelectorId(index) = reached.id;
                if std::mem::replace(&mut self.selectors_seen[index], true) {
                    return;
                }
                let named = self.plan.selector(reached.id);
                (
                    &named.selector,
                    format!("selector `{}`", named.name),
                    TriggerOrigin::Named(reached.id),
                )
            }
            SelectorRef::Inline(selector) => (
                selector.as_ref(),
                "the selector".to_string(),
                TriggerOrigin::Inline { relation, part },
            ),
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
        self.selectors.push(TriggerSelector {
            origin,
            scope,
            manifests,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::plan::{
        Capabilities, FieldDef, FieldSource, JoinDef, JoinSide, NamedRelation, NamedSelector, Part,
        PlanSource, Reached, Read, Reader, RelationDef, SelectorDef, SourceIndex,
    };
    use crate::{ConfigSourceClass, PathsSpec};

    fn selector(glob: &str) -> SelectorDef {
        SelectorDef {
            paths: PathsSpec::Single(glob.into()),
            scope_filter: None,
        }
    }

    fn part(from: SelectorRef) -> Part {
        Part {
            from,
            reader: Reader::Read {
                read: Read::WholeFile,
                optional: false,
            },
        }
    }

    fn named(rows: Rows) -> NamedRelation {
        NamedRelation {
            name: "r".into(),
            source: SourceIndex(0),
            relation: RelationDef {
                rows,
                fields: vec![(
                    "f".into(),
                    FieldDef {
                        source: FieldSource::Value,
                        sensitive: false,
                    },
                )],
                transforms: Vec::new(),
                filters: Vec::new(),
            },
            capabilities: Capabilities::default(),
            reads_environment: false,
            definition_reads_environment: false,
        }
    }

    /// The scope names each selector it holds once, in the order the walk
    /// reached it, a union's inline ones by their part.
    #[test]
    fn the_scope_names_its_selectors_in_walk_order() {
        let shared = || {
            SelectorRef::Named(Reached {
                id: SelectorId(0),
                alias: None,
            })
        };
        let side = |id: usize| Reached {
            id: RelationId(id),
            alias: None,
        };
        let mut join = named(Rows::Join(JoinDef {
            left: side(0),
            right: side(1),
            on: vec![("f".into(), "f".into())],
            unique: JoinSide::Right,
        }));
        join.relation.filters = vec![(
            "f".into(),
            vec![Predicate::NotIn(vec![Member::Relation {
                relation: side(3),
                field: "f".into(),
            }])],
        )];
        let plan = Plan {
            sources: vec![PlanSource {
                display: ".alint.yml".into(),
                path: PathBuf::from(".alint.yml"),
                class: ConfigSourceClass::Root,
                aliases: Vec::new(),
            }],
            selectors: vec![NamedSelector {
                name: "s".into(),
                source: SourceIndex(0),
                selector: selector("a/*.json"),
                capabilities: Capabilities::default(),
                reads_environment: false,
            }],
            relations: vec![
                named(Rows::Part(part(shared()))),
                named(Rows::Union(vec![
                    part(SelectorRef::Inline(Box::new(selector("b/*.json")))),
                    part(shared()),
                    part(SelectorRef::Inline(Box::new(selector("c/*.json")))),
                ])),
                join,
                named(Rows::Part(part(SelectorRef::Inline(Box::new(selector(
                    "d.json",
                )))))),
            ],
            rules: Vec::new(),
        };
        let origins: Vec<TriggerOrigin> = TriggerScope::of(&plan, &[RelationId(2)])
            .origins()
            .collect();
        let inline = |relation: usize, part: usize| TriggerOrigin::Inline {
            relation: RelationId(relation),
            part,
        };
        assert_eq!(
            origins,
            [
                TriggerOrigin::Named(SelectorId(0)),
                inline(1, 0),
                inline(1, 2),
                inline(3, 0),
            ]
        );
    }

    /// A selector that does not compile takes every change, since it cannot
    /// say which paths it reads; still, no change runs no rule.
    #[test]
    fn an_undecidable_selector_takes_every_change_and_no_change_runs_nothing() {
        let plan = Plan {
            sources: vec![PlanSource {
                display: ".alint.yml".into(),
                path: PathBuf::from(".alint.yml"),
                class: ConfigSourceClass::Root,
                aliases: Vec::new(),
            }],
            selectors: Vec::new(),
            relations: vec![named(Rows::Part(part(SelectorRef::Inline(Box::new(
                selector("a/[.json"),
            )))))],
            rules: Vec::new(),
        };
        let trigger = TriggerScope::of(&plan, &[RelationId(0)]);
        assert!(trigger.selectors[0].scope.is_none());
        let index = FileIndex::from_entries(Vec::new());
        let none = std::collections::HashSet::new();
        let mut pending = Vec::new();
        let nothing = ChangedPaths::new(std::iter::empty(), &none);
        assert!(!trigger.hits(&nothing, &index, &mut pending));
        let anything = ChangedPaths::new([Path::new("src/main.rs")], &none);
        assert!(trigger.hits(&anything, &index, &mut pending));
        assert_eq!(pending, []);
    }
}
