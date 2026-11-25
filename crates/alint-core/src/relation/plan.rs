//! The resolved form of a version 2 configuration's named selectors and
//! relations (design `v0.23/selectors-and-constraints.md`, sections 4, 5, 8
//! and 10): what the loader builds once every source has loaded and every
//! name has resolved, carried on [`crate::Config::relation_plan`].
//!
//! Every reference here is resolved. A named selector or relation is an index
//! into [`Plan::selectors`] or [`Plan::relations`], with the `extends:` alias
//! a qualified reference went through ([`Reached`]). The loader has checked
//! the plan before handing it over: every field a relation names exists,
//! every part of a union binds every field, `strip-root` applies to an
//! `entry.path` field, and no join or membership filter reads itself through
//! other relations, so a reader may rely on those. Each object keeps the
//! source that declared it, and the capabilities reading it needs.
//!
//! Version 2 is experimental (ADR-0034 decision 1), so this module is part of
//! the hidden relation layer and promises no stability. It is public only so
//! the CLI's `list` can show the objects, the relation engine can execute
//! them, and the `constraint` kind can read its input.

use std::path::PathBuf;

use crate::{Capability, ConfigSourceClass, PathsSpec, ScopeFilterSpec};

use super::Value;

/// A version 2 configuration's named selectors and relations, resolved.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Every version 2 source the configuration was loaded from, in load
    /// order: the sources an `extends:` reaches before the config that
    /// extends them, then the top-level config, then its `.alint.d`
    /// fragments.
    pub sources: Vec<PlanSource>,
    /// Every named selector, source by source in load order, each source's in
    /// the order written.
    pub selectors: Vec<NamedSelector>,
    /// Every named relation, in the same order.
    pub relations: Vec<NamedRelation>,
    /// The references each rule's fields make, resolved in the source that
    /// wrote each field: the `constraint` kind's `input:`.
    pub rules: Vec<RuleReferences>,
}

impl Plan {
    #[must_use]
    pub fn source(&self, index: SourceIndex) -> &PlanSource {
        &self.sources[index.0]
    }

    #[must_use]
    pub fn selector(&self, id: SelectorId) -> &NamedSelector {
        &self.selectors[id.0]
    }

    #[must_use]
    pub fn relation(&self, id: RelationId) -> &NamedRelation {
        &self.relations[id.0]
    }

    /// The aliases other sources give each source (`extends: [{ url, as }]`),
    /// indexed by [`SourceIndex`], each with the source that wrote it, in load
    /// order: the names through which a consumer reaches a source's objects.
    /// Built in one pass over every alias, so a caller that wants the aliases
    /// of every source looks each source's up instead of reading every alias
    /// once per source.
    #[must_use]
    pub fn aliases_by_source(&self) -> Vec<Vec<(SourceIndex, &PlanAlias)>> {
        let mut by_source = vec![Vec::new(); self.sources.len()];
        for (consumer, source) in self.sources.iter().enumerate() {
            for alias in &source.aliases {
                if let Some(aliases) = alias.target.and_then(|target| by_source.get_mut(target.0)) {
                    aliases.push((SourceIndex(consumer), alias));
                }
            }
        }
        by_source
    }

    /// `ids` and every relation they read, transitively, in the order a run
    /// evaluates them when these are the only relations its rules read: `ids`
    /// in plan order, each after the relations it reads
    /// ([`RelationDef::reads`], in that order), each relation once. `explain`
    /// lists a rule's relations in this order.
    #[must_use]
    pub fn evaluation_order(&self, ids: &[RelationId]) -> Vec<RelationId> {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let mut visited = vec![false; self.relations.len()];
        let mut order = Vec::new();
        for id in ids {
            self.post_order(id, &mut visited, &|_| false, &mut order);
        }
        order
    }

    /// Push onto `order` relation `id` and each relation it reads,
    /// transitively, that `visited` does not mark and `known` does not
    /// answer, each after the relations it reads, marking each. The walk
    /// keeps its own stack, so a long chain of joins cannot exhaust the
    /// thread's, and it visits each relation once (the plan check refused
    /// cycles).
    pub(crate) fn post_order(
        &self,
        id: RelationId,
        visited: &mut [bool],
        known: &dyn Fn(RelationId) -> bool,
        order: &mut Vec<RelationId>,
    ) {
        let mut stack = vec![(id, false)];
        while let Some((next, expanded)) = stack.pop() {
            if expanded {
                order.push(next);
                continue;
            }
            if std::mem::replace(&mut visited[next.0], true) || known(next) {
                continue;
            }
            stack.push((next, true));
            for read in self.relation(next).relation.reads().into_iter().rev() {
                if !visited[read.id.0] {
                    stack.push((read.id, false));
                }
            }
        }
    }

    /// The references rule `id` makes, if any.
    #[must_use]
    pub fn rule(&self, id: &str) -> Option<&RuleReferences> {
        self.rules.iter().find(|rule| rule.rule == id)
    }

    /// What reading everything rule `id` references needs: the union of
    /// those objects' capabilities. A rule's own capabilities are its kind's;
    /// the trust gates judge the two together (design section 10).
    #[must_use]
    pub fn rule_capabilities(&self, id: &str) -> Capabilities {
        let mut capabilities = Capabilities::default();
        for reference in self.rule(id).map_or(&[][..], |rule| &rule.references) {
            capabilities.add(&self.relation(reference.relation.id).capabilities);
        }
        capabilities
    }
}

/// A version 2 source, as the objects it declares name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSource {
    /// How a message names the source, as the loader's errors do.
    pub display: String,
    /// The resolved file.
    pub path: PathBuf,
    pub class: ConfigSourceClass,
    /// The aliases it gives the sources it extends, in the order written.
    pub aliases: Vec<PlanAlias>,
}

/// An `extends:` entry's `as:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanAlias {
    pub name: String,
    /// The entry's URL, after `{{env.X}}` interpolation.
    pub url: String,
    /// The source it names, when that source is version 2 (a version 1
    /// source declares no selector or relation).
    pub target: Option<SourceIndex>,
}

/// An index into [`Plan::sources`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceIndex(pub usize);

/// An index into [`Plan::selectors`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SelectorId(pub usize);

/// An index into [`Plan::relations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelationId(pub usize);

/// A resolved reference: the object, and the `extends:` alias a qualified
/// reference (`deps.ranges`) went through (`None` for a bare name, which
/// resolves in the referencing source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reached<Id> {
    pub id: Id,
    pub alias: Option<String>,
}

/// A named selector and the source that declared it.
#[derive(Debug, Clone)]
pub struct NamedSelector {
    pub name: String,
    pub source: SourceIndex,
    pub selector: SelectorDef,
    /// What reading the resources it picks needs.
    pub capabilities: Capabilities,
    /// Whether its definition takes a value from the environment
    /// (`{{env.X}}`): a relation that reads it then shows no detail of its
    /// analysis failures.
    pub reads_environment: bool,
}

/// A named relation and the source that declared it.
#[derive(Debug, Clone)]
pub struct NamedRelation {
    pub name: String,
    pub source: SourceIndex,
    pub relation: RelationDef,
    /// What reading its rows needs: its own reads, and those of every
    /// selector and relation it references, transitively.
    pub capabilities: Capabilities,
    /// Whether its definition, its fields' `where:` and `transform:` entries
    /// included (which make those fields sensitive too), or a named selector
    /// it reads takes a value from the environment (`{{env.X}}`), or a
    /// relation it reads (a join side, a membership reference) does,
    /// transitively. Then the environment may have chosen its rows: no
    /// analysis failure of it shows its detail, which could echo the value (a
    /// key path an `at:` from the environment selected, say), and a rule
    /// that reads it shows no path or value of its findings, since the rows
    /// a filter from the environment keeps say where that value occurs.
    pub reads_environment: bool,
    /// Whether its own definition, outside its fields' `where:` and
    /// `transform:` entries, takes a value from the environment: then
    /// `explain` shows none of the values it holds (an inline selector's
    /// globs, its reader, a `bound:` path, a join's `unique:`), only its
    /// names. [`Self::reads_environment`] is this, a `where:` or `transform:`
    /// entry from the environment, or what it reads doing either.
    pub definition_reads_environment: bool,
}

impl NamedRelation {
    /// Whether this is a rule's inline `input:` relation, which the loader
    /// plans as a named one under where it is written (`rules[3].input`): a
    /// name no config can declare, since every declared name is a name
    /// ([`is_name`]).
    #[must_use]
    pub fn is_inline(&self) -> bool {
        !is_name(&self.name)
    }
}

/// Whether `text` is a name a config can declare a selector or relation
/// under: an ASCII letter or `_`, then ASCII letters, digits, `_` and `-`.
/// A `.` would read as a qualified reference. The loader checks every
/// declared name with this, and a plan tells a rule's inline `input:`
/// relation by its name not being one ([`NamedRelation::is_inline`]), so the
/// two never disagree.
#[must_use]
pub fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What reading an object makes alint do, as the trust gates judge it
/// (design section 10). A rule's capabilities are its own plus those of
/// everything it references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Reads the content of files: a relation reads the resources its
    /// selectors pick, and a manifest-derived `scope_filter:` predicate reads
    /// its manifest.
    pub reads_content: bool,
    /// Host capabilities, as rule kinds declare them. No selector or
    /// relation has one in v0.23 (`changed_since`, which would need Git, is
    /// refused in a selector).
    pub host: Vec<Capability>,
}

impl Capabilities {
    /// Add `other`'s capabilities to these.
    pub fn add(&mut self, other: &Self) {
        self.reads_content |= other.reads_content;
        for capability in &other.host {
            if !self.host.contains(capability) {
                self.host.push(capability.clone());
            }
        }
    }
}

/// A selector (section 4): a rule's `paths:` grammar and its `scope_filter:`
/// predicates but `changed_since`.
#[derive(Debug, Clone)]
pub struct SelectorDef {
    pub paths: PathsSpec,
    pub scope_filter: Option<ScopeFilterSpec>,
}

impl SelectorDef {
    /// The selector's `paths:` and `scope_filter:` as one scope, as a rule's
    /// [`Scope::from_spec`](crate::Scope::from_spec) compiles its own. `name`
    /// is how an error names the selector.
    pub(crate) fn compile(&self, name: &str) -> crate::Result<crate::Scope> {
        let scope = crate::Scope::from_paths_spec(&self.paths)?;
        Ok(match &self.scope_filter {
            Some(spec) => {
                scope.with_scope_filter(crate::ScopeFilter::from_spec(name, spec.clone())?)
            }
            None => scope,
        })
    }
}

/// Where a relation reads: a named selector, or an anonymous inline one.
#[derive(Debug, Clone)]
pub enum SelectorRef {
    Named(Reached<SelectorId>),
    Inline(Box<SelectorDef>),
}

/// A relation (section 5): a plan that reads rows and names their fields.
#[derive(Debug, Clone)]
pub struct RelationDef {
    pub rows: Rows,
    /// The fields, in the order written.
    pub fields: Vec<(String, FieldDef)>,
    /// Each field's transforms, applied in order before `filters`.
    pub transforms: Vec<(String, Vec<Transform>)>,
    /// `where:`: each field's predicates, all of which must hold.
    pub filters: Vec<(String, Vec<Predicate>)>,
}

impl RelationDef {
    /// The field `name`, if the relation declares one.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields
            .iter()
            .find(|(held, _)| held == name)
            .map(|(_, field)| field)
    }

    /// Whether a `union:` relation declares per-part `fields:` instead of one
    /// shared top-level `fields:` map (ADR-0050 D8, #592).
    #[must_use]
    pub fn uses_part_fields(&self) -> bool {
        matches!(
            &self.rows,
            Rows::Union(parts) if parts.iter().any(|part| part.fields.is_some())
        )
    }

    /// The fields active for `part`: per-part `fields:` when present, else the
    /// relation's top-level `fields:`.
    #[must_use]
    pub fn part_fields<'a>(&'a self, part: &'a Part) -> &'a [(String, FieldDef)] {
        part.fields.as_deref().unwrap_or(&self.fields)
    }

    /// Every `FieldDef` declared for `name` across parts (one entry for
    /// shared-fields relations, one per part for per-part `union:`).
    #[must_use]
    pub fn field_sources<'a>(&'a self, name: &str) -> Vec<&'a FieldDef> {
        if let Rows::Union(parts) = &self.rows
            && parts.iter().any(|part| part.fields.is_some())
        {
            return parts
                .iter()
                .filter_map(|part| {
                    part.fields
                        .as_deref()
                        .unwrap_or(&self.fields)
                        .iter()
                        .find(|(held, _)| held == name)
                        .map(|(_, field)| field)
                })
                .collect();
        }
        self.field(name).into_iter().collect()
    }

    /// The transforms of field `name`, in order (none when it has none).
    #[must_use]
    pub fn transforms_of(&self, name: &str) -> &[Transform] {
        self.transforms
            .iter()
            .find(|(held, _)| held == name)
            .map_or(&[], |(_, transforms)| transforms.as_slice())
    }

    /// The relations this one reads, as its definition writes them, in the
    /// order the evaluator reads them: a join's left side, its right side,
    /// then the relation of each `{ relation, field }` item of its `in:` and
    /// `not_in:` lists, in the order written.
    #[must_use]
    pub fn reads(&self) -> Vec<&Reached<RelationId>> {
        let mut reads = Vec::new();
        if let Rows::Join(join) = &self.rows {
            reads.extend([&join.left, &join.right]);
        }
        for (_, predicates) in &self.filters {
            for predicate in predicates {
                if let Predicate::In(members) | Predicate::NotIn(members) = predicate {
                    for member in members {
                        if let Member::Relation { relation, .. } = member {
                            reads.push(relation);
                        }
                    }
                }
            }
        }
        reads
    }
}

/// Where a relation's rows come from: exactly one of `entries:`, `read:`,
/// `union:` and `join:`.
#[derive(Debug, Clone)]
pub enum Rows {
    Part(Part),
    Union(Vec<Part>),
    Join(JoinDef),
}

/// A selector and how its resources are read: a relation with `entries:`,
/// `records:` or `read:`, or one part of a `union:`.
#[derive(Debug, Clone)]
pub struct Part {
    pub from: SelectorRef,
    pub reader: Reader,
    /// Per-part field bindings in a mixed-reader `union:` (ADR-0050 D8, #592),
    /// normalized to the first part's field order; `None` when the relation
    /// declares shared top-level `fields:`.
    pub fields: Option<Vec<(String, FieldDef)>>,
}

#[derive(Debug, Clone)]
pub enum Reader {
    Entries(Entries),
    Records(Records),
    Read {
        read: Read,
        /// A `read:` that matches nothing yields no row instead of a missing
        /// one.
        optional: bool,
    },
}

/// `entries:`: one row per member of each object `at` selects, or per leaf
/// below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entries {
    pub format: EntriesFormat,
    pub at: Vec<String>,
    pub leaves: bool,
    /// When `true` and `format == EntriesFormat::Json`, enforce strict RFC 8259
    /// JSON parsing (ADR-0050 D7).
    pub strict: bool,
}

/// `records:`: one row per object element `at` selects, with arbitrary sibling
/// scalar subfields read via `record: <JSONPath>` (ADR-0050 D8, #592).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Records {
    pub format: EntriesFormat,
    pub at: Vec<String>,
    /// When `true` and `format == EntriesFormat::Json`, enforce strict RFC 8259
    /// JSON parsing (ADR-0050 D7).
    pub strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntriesFormat {
    Json,
    Yaml,
    Toml,
}

/// `read:`: the extract forms `cross_file` has, limited in v0.23 to these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    Json { query: String, strict: bool },
    Yaml(String),
    Toml(String),
    Regex(String),
    Lines { comment: Option<String> },
    WholeFile,
}

/// `join:` (section 8): an inner equality join of two relations, one side
/// unique on the join key.
#[derive(Debug, Clone)]
pub struct JoinDef {
    pub left: Reached<RelationId>,
    pub right: Reached<RelationId>,
    /// Each left field with the right field it must equal, in the order
    /// written.
    pub on: Vec<(String, String)>,
    pub unique: JoinSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinSide {
    Left,
    Right,
}

/// A field: where its value comes from, and whether it is kept out of
/// messages, the JSON report and baselines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub source: FieldSource,
    pub sensitive: bool,
}

/// What an optional `bound:` field does when its `JSONPath` matches nothing in a
/// resource (#593).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoundMissing {
    /// Drop the resource from the relation (emit no rows and exclude it from
    /// `resources`, matching `read: { optional: true }`). Default.
    #[default]
    Skip,
    /// Keep the resource's rows with a `Missing` cell (`Cell { value: None }`).
    Missing,
}

/// The sources a field may take (section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSource {
    /// `path`, or a path token (`dir`, `basename`, `stem`, `ext`,
    /// `parent_name`).
    Path(PathToken),
    EntryKey,
    EntryValue,
    EntryParent,
    EntryRoot,
    EntryPath,
    /// `value`: a `read:` row's value.
    Value,
    /// `bound: <JSONPath>`: a value read once per resource.
    Bound {
        query: String,
        optional: bool,
        on_missing: BoundMissing,
    },
    /// `record: <JSONPath>`: a scalar subfield read from each `records:`
    /// element object (ADR-0050 D8, #592).
    Record {
        query: String,
        optional: bool,
    },
    /// `left.F` or `right.F` in a join's output.
    Side(JoinSide, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathToken {
    Path,
    Dir,
    Basename,
    Stem,
    Ext,
    ParentName,
}

/// Relation transforms (ADR-0037 decision 2, ADR-0050 D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transform {
    Trim,
    /// Trims, then lowercases each character: not Unicode case folding.
    Lower,
    SemverMajor,
    SemverMinor,
    StripRoot,
    /// Split on non-empty separator `on` and select 0-based (or negative from
    /// end) segment `part`; out-of-range emits `Missing` when `!required` or
    /// fails when `required`.
    Split {
        on: String,
        part: i64,
        required: bool,
    },
    StripPrefix(String),
    StripSuffix(Vec<String>),
    Prefix(String),
    Suffix(String),
}

/// A row filter's predicate (section 7).
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    In(Vec<Member>),
    NotIn(Vec<Member>),
    Matches(String),
    NotMatches(String),
}

/// An item of an `in` or `not_in` list.
#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    /// A scalar literal: a bool or a string (the loader refuses a null,
    /// which is in no set, and a number, whose lexeme the configuration's
    /// YAML does not keep).
    Literal(Value),
    /// `{ relation: R, field: F }`: every value of another relation's field.
    Relation {
        relation: Reached<RelationId>,
        field: String,
    },
}

/// The references one rule's fields make (the `constraint` kind's):
/// each field resolved in the source that wrote it, so a field an extended
/// source wrote keeps naming that source's objects when a consumer overrides
/// the rule's other fields by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleReferences {
    pub rule: String,
    pub references: Vec<RuleReference>,
}

/// A rule field that names a relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleReference {
    /// The rule's top-level field (`input`).
    pub field: String,
    /// The source that wrote the field, in whose namespace it resolved.
    pub source: SourceIndex,
    pub relation: Reached<RelationId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(display: &str, aliases: Vec<PlanAlias>) -> PlanSource {
        PlanSource {
            display: display.into(),
            path: PathBuf::from(display),
            class: ConfigSourceClass::Root,
            aliases,
        }
    }

    fn relation(name: &str, capabilities: Capabilities) -> NamedRelation {
        NamedRelation {
            name: name.into(),
            source: SourceIndex(0),
            relation: RelationDef {
                rows: Rows::Part(Part {
                    from: SelectorRef::Inline(Box::new(SelectorDef {
                        paths: PathsSpec::Single("*.json".into()),
                        scope_filter: None,
                    })),
                    reader: Reader::Read {
                        read: Read::WholeFile,
                        optional: false,
                    },
                    fields: None,
                }),
                fields: vec![(
                    "text".into(),
                    FieldDef {
                        source: FieldSource::Value,
                        sensitive: false,
                    },
                )],
                transforms: vec![("text".into(), vec![Transform::Trim])],
                filters: Vec::new(),
            },
            capabilities,
            reads_environment: false,
            definition_reads_environment: false,
        }
    }

    #[test]
    fn aliases_by_source_lists_each_consumer_alias() {
        let alias = |name: &str, target: Option<usize>| PlanAlias {
            name: name.into(),
            url: format!("./{name}.yml"),
            target: target.map(SourceIndex),
        };
        let plan = Plan {
            sources: vec![
                source("base.yml", Vec::new()),
                source(
                    ".alint.yml",
                    vec![alias("base", Some(0)), alias("v1", None)],
                ),
                source(".alint.d/a.yml", vec![alias("b", Some(0))]),
            ],
            ..Plan::default()
        };
        let by_source = plan.aliases_by_source();
        let found: Vec<(&str, &str)> = by_source[0]
            .iter()
            .map(|(consumer, alias)| {
                let consumer = plan.source(*consumer).display.as_str();
                (consumer, alias.name.as_str())
            })
            .collect();
        assert_eq!(found, [(".alint.yml", "base"), (".alint.d/a.yml", "b")]);
        assert!(by_source[1].is_empty() && by_source[2].is_empty());
        assert_eq!(by_source.len(), 3);
    }

    #[test]
    fn a_rule_has_the_capabilities_of_what_it_references() {
        let spawns = Capabilities {
            reads_content: false,
            host: vec![Capability::SpawnUserCommand],
        };
        let reads = Capabilities {
            reads_content: true,
            host: Vec::new(),
        };
        let reference = |field: &str, id: usize| RuleReference {
            field: field.into(),
            source: SourceIndex(0),
            relation: Reached {
                id: RelationId(id),
                alias: None,
            },
        };
        let plan = Plan {
            sources: vec![source(".alint.yml", Vec::new())],
            relations: vec![relation("a", reads.clone()), relation("b", spawns)],
            rules: vec![
                RuleReferences {
                    rule: "both".into(),
                    references: vec![reference("input", 0), reference("other", 1)],
                },
                RuleReferences {
                    rule: "one".into(),
                    references: vec![reference("input", 0)],
                },
            ],
            ..Plan::default()
        };
        assert_eq!(
            plan.rule_capabilities("both"),
            Capabilities {
                reads_content: true,
                host: vec![Capability::SpawnUserCommand],
            }
        );
        assert_eq!(plan.rule_capabilities("one"), reads);
        assert_eq!(plan.rule_capabilities("none"), Capabilities::default());
        assert_eq!(plan.relation(RelationId(1)).name, "b");
        assert_eq!(
            plan.relation(RelationId(0)).relation.transforms_of("text"),
            [Transform::Trim]
        );
        assert!(
            plan.relation(RelationId(0))
                .relation
                .field("nope")
                .is_none()
        );
    }

    /// A join of relations `left` and `right` whose `where:` probes
    /// `probes` through `in:` lists.
    fn join(name: &str, left: usize, right: usize, probes: &[usize]) -> NamedRelation {
        let reached = |id: usize| Reached {
            id: RelationId(id),
            alias: None,
        };
        let mut named = relation(name, Capabilities::default());
        named.relation.rows = Rows::Join(JoinDef {
            left: reached(left),
            right: reached(right),
            on: vec![("text".into(), "text".into())],
            unique: JoinSide::Right,
        });
        named.relation.filters = vec![(
            "text".into(),
            probes
                .iter()
                .map(|&id| {
                    Predicate::In(vec![
                        Member::Literal(Value::String("x".into())),
                        Member::Relation {
                            relation: reached(id),
                            field: "text".into(),
                        },
                    ])
                })
                .collect(),
        )];
        named
    }

    /// A run evaluates each relation after those it reads, a join's left
    /// side, its right side and the relations its lists probe in that order,
    /// and the relations its rules read in plan order, each once.
    #[test]
    fn the_evaluation_order_puts_each_relation_after_those_it_reads() {
        let plan = Plan {
            sources: vec![source(".alint.yml", Vec::new())],
            relations: vec![
                relation("a", Capabilities::default()),
                relation("b", Capabilities::default()),
                join("j", 1, 0, &[4, 3]),
                relation("c", Capabilities::default()),
                relation("d", Capabilities::default()),
                join("k", 2, 3, &[]),
            ],
            ..Plan::default()
        };
        let order = |ids: &[usize]| -> Vec<usize> {
            let ids: Vec<RelationId> = ids.iter().copied().map(RelationId).collect();
            plan.evaluation_order(&ids)
                .into_iter()
                .map(|id| id.0)
                .collect()
        };
        let reads: Vec<usize> = plan
            .relation(RelationId(2))
            .relation
            .reads()
            .into_iter()
            .map(|read| read.id.0)
            .collect();
        assert_eq!(reads, [1, 0, 4, 3]);
        assert_eq!(order(&[2]), [1, 0, 4, 3, 2]);
        assert_eq!(order(&[5]), [1, 0, 4, 3, 2, 5]);
        assert_eq!(order(&[3, 0]), [0, 3]);
        assert_eq!(order(&[4, 2, 4]), [1, 0, 4, 3, 2]);
        assert_eq!(order(&[]), Vec::<usize>::new());
    }

    #[test]
    fn adding_capabilities_keeps_each_once() {
        let mut held = Capabilities {
            reads_content: false,
            host: vec![Capability::EnvRead],
        };
        held.add(&Capabilities {
            reads_content: true,
            host: vec![Capability::EnvRead, Capability::HostKeyring],
        });
        assert_eq!(
            held,
            Capabilities {
                reads_content: true,
                host: vec![Capability::EnvRead, Capability::HostKeyring],
            }
        );
    }
}
