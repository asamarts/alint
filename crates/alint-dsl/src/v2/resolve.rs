//! Per-source namespaces (design section 10, ADR-0034 decision 4): every
//! reference in a version 2 configuration resolves to the object it names,
//! once every source has loaded, into the plan the configuration carries
//! ([`alint_core::relation::plan::Plan`]).
//!
//! - A bare name resolves among the objects the referencing source declares.
//! - `alias.name` resolves among the objects the source that the referencing
//!   source's `extends:` entry `as: alias` names declares itself: there is no
//!   re-export, and no chain (`a.b.c`, refused at decode).
//! - Nothing shadows: an extended source's objects resolve in that source
//!   whatever its consumer declares, and a consumer reaches them only by a
//!   qualified name. Each `.alint.d` fragment is a source of its own.
//! - A rule's reference fields resolve in the source that wrote each field
//!   ([`super::origins`]).
//!
//! An unknown name or alias is a load error naming the referencing source and
//! key path, with the names that source can reach when there are few, or the
//! nearest one. The plan check ([`super::plan_check`]) then runs on the
//! resolved plan, and each object passes its declaring source's trust gates.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use alint_core::relation::plan::{
    self as plan, Capabilities, NamedRelation, NamedSelector, Plan, PlanAlias, PlanSource, Reached,
    RelationId, RuleReference, RuleReferences, SelectorId, SourceIndex,
};
use alint_core::{Capability, ConfigSourceClass, Error, GitOperation, Result};

use super::model::{
    Entries, EntriesFormat, Field, FieldSource, Join, JoinSide, Member, Objects, Part, PathToken,
    Predicate, Read, Reader, Reference, Relation, Rows, Selector, SelectorRef, Transform,
};

/// At most this many names are listed in an error; past it, the nearest one
/// is suggested instead.
pub(super) const LISTED_MAX: usize = 6;

/// The error for a problem at `path` of the source `display`, as the decode
/// writes it.
pub(super) fn located(display: &str, path: &str, message: impl std::fmt::Display) -> Error {
    Error::Other(format!("{display}: `{path}`: {message}"))
}

/// `names` as "`a`, `b`", or the nearest of them to `wanted` when there are
/// more than [`LISTED_MAX`]: `what` introduces a list ("its fields are"),
/// `noun` counts the rest ("fields").
pub(super) fn suggest(wanted: &str, names: &[String], what: &str, noun: &str) -> String {
    if names.len() <= LISTED_MAX {
        let listed: Vec<&str> = names.iter().map(String::as_str).collect();
        return format!("{what} {}", super::decode::listed(&listed));
    }
    match nearest(wanted, names) {
        Some(near) => format!("did you mean `{near}`?"),
        None => format!(
            "there are {} {noun}, none named like `{wanted}`",
            names.len()
        ),
    }
}

/// The name in `names` closest to `wanted`, within two edits.
fn nearest<'n>(wanted: &str, names: &'n [String]) -> Option<&'n str> {
    names
        .iter()
        .map(|name| (crate::interp::levenshtein(wanted, name), name))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name.as_str())
}

/// The kind of object a reference must name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Selector,
    Relation,
}

impl Wanted {
    const fn noun(self) -> &'static str {
        match self {
            Self::Selector => "selector",
            Self::Relation => "relation",
        }
    }
}

/// What a name a source declares is.
#[derive(Debug, Clone, Copy)]
enum Declared {
    Selector(SelectorId),
    Relation(RelationId),
    /// An index into the source's aliases.
    Alias(usize),
}

impl Declared {
    const fn noun(self) -> &'static str {
        match self {
            Self::Selector(_) => "selector",
            Self::Relation(_) => "relation",
            Self::Alias(_) => "`extends:` alias",
        }
    }
}

/// A resolved object of the wanted kind.
#[derive(Debug, Clone, Copy)]
enum Found {
    Selector(SelectorId),
    Relation(RelationId),
}

/// Every source's namespace, and the plan's sources and object ids.
#[derive(Debug)]
pub(crate) struct Namespaces<'o> {
    objects: &'o Objects,
    sources: Vec<PlanSource>,
    /// Each source's declared names.
    names: Vec<HashMap<&'o str, Declared>>,
    /// The version 2 sources each source's `extends:` entries name.
    extended: Vec<Vec<SourceIndex>>,
    /// Each selector id's source and index among that source's selectors.
    selectors: Vec<(SourceIndex, usize)>,
    /// The same for relations.
    relations: Vec<(SourceIndex, usize)>,
}

impl<'o> Namespaces<'o> {
    fn new(objects: &'o Objects) -> Self {
        let index_of = |path: &Path| {
            objects
                .sources
                .iter()
                .position(|source| source.source.path == path)
                .map(SourceIndex)
        };
        let mut namespaces = Self {
            objects,
            sources: Vec::with_capacity(objects.sources.len()),
            names: Vec::with_capacity(objects.sources.len()),
            extended: Vec::with_capacity(objects.sources.len()),
            selectors: Vec::new(),
            relations: Vec::new(),
        };
        for (index, source) in objects.sources.iter().enumerate() {
            let at = SourceIndex(index);
            let mut names = HashMap::new();
            for (local, (name, _)) in source.selectors.iter().enumerate() {
                names.insert(
                    name.as_str(),
                    Declared::Selector(SelectorId(namespaces.selectors.len())),
                );
                namespaces.selectors.push((at, local));
            }
            for (local, (name, _)) in source.relations.iter().enumerate() {
                names.insert(
                    name.as_str(),
                    Declared::Relation(RelationId(namespaces.relations.len())),
                );
                namespaces.relations.push((at, local));
            }
            let mut aliases = Vec::with_capacity(source.aliases.len());
            for (local, (name, alias)) in source.aliases.iter().enumerate() {
                names.insert(name.as_str(), Declared::Alias(local));
                aliases.push(PlanAlias {
                    name: name.clone(),
                    url: alias.url.clone(),
                    target: alias.target.as_deref().and_then(index_of),
                });
            }
            namespaces.names.push(names);
            namespaces
                .extended
                .push(source.extended.iter().filter_map(|p| index_of(p)).collect());
            namespaces.sources.push(PlanSource {
                display: source.source.display.clone(),
                path: source.source.path.clone(),
                class: source.source.class,
                aliases,
            });
        }
        namespaces
    }

    fn display(&self, source: SourceIndex) -> &str {
        &self.sources[source.0].display
    }

    /// The source whose resolved file is `path`, if it is version 2.
    pub(crate) fn source_at(&self, path: &Path) -> Option<SourceIndex> {
        self.sources
            .iter()
            .position(|source| source.path == path)
            .map(SourceIndex)
    }

    /// The names of `wanted`'s kind that `source` declares.
    fn own_names(&self, source: SourceIndex, wanted: Wanted) -> Vec<String> {
        let objects = &self.objects.sources[source.0];
        match wanted {
            Wanted::Selector => objects.selectors.iter().map(|(n, _)| n.clone()).collect(),
            Wanted::Relation => objects.relations.iter().map(|(n, _)| n.clone()).collect(),
        }
    }

    /// Whether `source` declares `name` as an object of `wanted`'s kind.
    fn declares(&self, source: SourceIndex, name: &str, wanted: Wanted) -> bool {
        matches!(
            (self.names[source.0].get(name), wanted),
            (Some(Declared::Selector(_)), Wanted::Selector)
                | (Some(Declared::Relation(_)), Wanted::Relation)
        )
    }

    /// Resolve `reference`, written in `from`, to an object of `wanted`'s
    /// kind. `site` says what the place it is written in takes, for an error
    /// that it names the other kind. The error is the message alone.
    fn find(
        &self,
        from: SourceIndex,
        reference: &Reference,
        wanted: Wanted,
        site: &str,
    ) -> std::result::Result<Found, String> {
        let names = &self.names[from.0];
        let Some(alias) = &reference.alias else {
            let name = &reference.name;
            return match names.get(name.as_str()) {
                Some(&declared) => Self::of_kind(declared, name, wanted, site),
                None => Err(format!(
                    "no {} named `{name}` in this config; {}",
                    wanted.noun(),
                    self.bare_hint(from, name, wanted)
                )),
            };
        };
        let qualified = format!("{alias}.{}", reference.name);
        let index = match names.get(alias.as_str()) {
            Some(Declared::Alias(index)) => *index,
            Some(other) => {
                return Err(format!(
                    "`{alias}` in `{qualified}` is a {} of this config, not an `extends:` alias; \
                     a qualified name is `<alias>.<name>`, with an alias an `extends:` entry \
                     gives with `as:`",
                    other.noun()
                ));
            }
            None => {
                return Err(format!(
                    "no `extends:` alias `{alias}` in this config (in `{qualified}`); {}",
                    self.alias_hint(from, alias, &reference.name)
                ));
            }
        };
        let entry = &self.sources[from.0].aliases[index];
        let Some(target) = entry.target else {
            return Err(format!(
                "`{alias}` names {}, a version 1 config, which declares no selectors or \
                 relations",
                entry.url
            ));
        };
        let name = &reference.name;
        let target_display = self.display(target);
        match self.names[target.0].get(name.as_str()) {
            Some(Declared::Alias(_)) => Err(format!(
                "`{name}` is an `extends:` alias of {target_display} (`{alias}`), not a {}: a \
                 qualified name reaches only the objects the aliased config declares itself, \
                 so there is no chained name",
                wanted.noun()
            )),
            Some(&declared) => Self::of_kind(declared, &qualified, wanted, site),
            None => Err(format!(
                "{target_display} (`{alias}`) declares no {} named `{name}`; {}",
                wanted.noun(),
                self.qualified_hint(target, alias, name, wanted)
            )),
        }
    }

    fn of_kind(
        declared: Declared,
        written: &str,
        wanted: Wanted,
        site: &str,
    ) -> std::result::Result<Found, String> {
        match (declared, wanted) {
            (Declared::Selector(id), Wanted::Selector) => Ok(Found::Selector(id)),
            (Declared::Relation(id), Wanted::Relation) => Ok(Found::Relation(id)),
            (Declared::Alias(_), _) => Err(format!(
                "`{written}` is an `extends:` alias, not a {}; write `{written}.<name>` to \
                 reach a {} the config it names declares",
                wanted.noun(),
                wanted.noun()
            )),
            (other, _) => Err(format!("`{written}` is a {}; {site}", other.noun())),
        }
    }

    /// What to say about a bare `name` that `from` does not declare.
    fn bare_hint(&self, from: SourceIndex, name: &str, wanted: Wanted) -> String {
        // An object of a source this one aliases: name it qualified.
        for alias in &self.sources[from.0].aliases {
            if let Some(target) = alias.target
                && self.declares(target, name, wanted)
            {
                return format!(
                    "{} declares it, which this config extends as `{}`: write `{}.{name}` (an \
                     extended config's objects are reached by a qualified name)",
                    self.display(target),
                    alias.name,
                    alias.name
                );
            }
        }
        // An object of a source this one extends without an alias.
        for &target in &self.extended[from.0] {
            if self.declares(target, name, wanted) {
                return format!(
                    "{} declares it, which this config extends: an extended config's objects \
                     are reached only by a qualified name, so give that `extends:` entry an \
                     alias (`as: <alias>`) and write `<alias>.{name}`",
                    self.display(target)
                );
            }
        }
        // A fragment's root.
        if self.sources[from.0].class == ConfigSourceClass::DropIn {
            let root = (0..self.sources.len()).map(SourceIndex).find(|&index| {
                self.sources[index.0].class == ConfigSourceClass::Root
                    && self.declares(index, name, wanted)
            });
            if let Some(root) = root {
                return format!(
                    "{} declares it, the root config: an `.alint.d` fragment has a namespace \
                     of its own and cannot name its root's selectors and relations, so declare \
                     `{name}` in this fragment",
                    self.display(root)
                );
            }
        }
        let mut reachable = self.own_names(from, wanted);
        for alias in &self.sources[from.0].aliases {
            if let Some(target) = alias.target {
                for held in self.own_names(target, wanted) {
                    reachable.push(format!("{}.{held}", alias.name));
                }
            }
        }
        if reachable.is_empty() {
            return format!(
                "it declares no {}s and reaches none through an `extends:` alias",
                wanted.noun()
            );
        }
        suggest(
            name,
            &reachable,
            &format!("the {}s it can reach are", wanted.noun()),
            &format!("{}s it can reach", wanted.noun()),
        )
    }

    /// What to say about `alias.name` where `from` has no alias `alias`.
    fn alias_hint(&self, from: SourceIndex, alias: &str, name: &str) -> String {
        let aliases: Vec<String> = self.sources[from.0]
            .aliases
            .iter()
            .map(|alias| alias.name.clone())
            .collect();
        if aliases.is_empty() {
            return format!(
                "this config gives no `extends:` entry an alias: write `as: {alias}` on the \
                 entry of the config that declares `{name}`"
            );
        }
        suggest(alias, &aliases, "its aliases are", "aliases")
    }

    /// What to say about `alias.name` where `target` does not declare `name`.
    fn qualified_hint(
        &self,
        target: SourceIndex,
        alias: &str,
        name: &str,
        wanted: Wanted,
    ) -> String {
        for &inner in &self.extended[target.0] {
            if self.declares(inner, name, wanted) {
                return format!(
                    "{} declares it, which {} extends: a qualified name reaches only the \
                     objects the aliased config declares itself (there is no re-export), so \
                     extend {} from this config with an alias of its own and write \
                     `<alias>.{name}`",
                    self.display(inner),
                    self.display(target),
                    self.display(inner)
                );
            }
        }
        let held = self.own_names(target, wanted);
        if held.is_empty() {
            return format!("it declares no {}s", wanted.noun());
        }
        let qualified: Vec<String> = held.iter().map(|n| format!("{alias}.{n}")).collect();
        suggest(
            &format!("{alias}.{name}"),
            &qualified,
            &format!("the {}s it declares are", wanted.noun()),
            &format!("{}s it declares", wanted.noun()),
        )
    }

    fn selector(
        &self,
        from: SourceIndex,
        reference: &Reference,
        site: &str,
    ) -> std::result::Result<Reached<SelectorId>, String> {
        match self.find(from, reference, Wanted::Selector, site)? {
            Found::Selector(id) => Ok(Reached {
                id,
                alias: reference.alias.clone(),
            }),
            Found::Relation(_) => unreachable!("`find` returns the wanted kind"),
        }
    }

    pub(crate) fn relation(
        &self,
        from: SourceIndex,
        reference: &Reference,
        site: &str,
    ) -> std::result::Result<Reached<RelationId>, String> {
        match self.find(from, reference, Wanted::Relation, site)? {
            Found::Relation(id) => Ok(Reached {
                id,
                alias: reference.alias.clone(),
            }),
            Found::Selector(_) => unreachable!("`find` returns the wanted kind"),
        }
    }
}

const FROM_SITE: &str = "`from:` takes a selector name or an inline selector map";
const SIDE_SITE: &str = "a join's `left:` and `right:` name relations";
const MEMBER_SITE: &str = "a membership reference's `relation:` names a relation";

/// A configuration's resolved plan, with the namespaces its rules' reference
/// fields resolve in.
#[derive(Debug)]
pub(crate) struct Resolved<'o> {
    pub(crate) plan: Plan,
    pub(crate) namespaces: Namespaces<'o>,
}

/// Resolve every reference of `objects`, check the plan, and judge each
/// object by its declaring source's trust gates.
pub(crate) fn resolve(objects: &Objects) -> Result<Resolved<'_>> {
    let namespaces = Namespaces::new(objects);
    let mut plan = Plan {
        sources: namespaces.sources.clone(),
        ..Plan::default()
    };
    for &(source, local) in &namespaces.selectors {
        let (name, selector) = &objects.sources[source.0].selectors[local];
        plan.selectors.push(NamedSelector {
            name: name.clone(),
            source,
            selector: selector_def(selector),
            capabilities: selector_capabilities(selector),
            reads_environment: selector.reads_environment,
        });
    }
    for &(source, local) in &namespaces.relations {
        let (name, relation) = &objects.sources[source.0].relations[local];
        let reads_environment = relation.reads_environment;
        let relation = relation_def(&namespaces, source, name, relation)?;
        // A named selector it reads that takes a value from the environment
        // makes its failures show no detail too.
        let selectors_read_environment = parts_of(&relation.rows).iter().any(|part| {
            matches!(&part.from, plan::SelectorRef::Named(reached)
                if plan.selectors[reached.id.0].reads_environment)
        });
        plan.relations.push(NamedRelation {
            name: name.clone(),
            source,
            relation,
            capabilities: Capabilities::default(),
            reads_environment: reads_environment || selectors_read_environment,
        });
    }
    let order = super::plan_check::check(&plan)?;
    add_capabilities(&mut plan, &order);
    gate_objects(&plan)?;
    Ok(Resolved { plan, namespaces })
}

/// The parts a relation reads with: its one part, a union's, or none for a
/// join.
fn parts_of(rows: &plan::Rows) -> &[plan::Part] {
    match rows {
        plan::Rows::Part(part) => std::slice::from_ref(part),
        plan::Rows::Union(parts) => parts,
        plan::Rows::Join(_) => &[],
    }
}

fn selector_def(selector: &Selector) -> plan::SelectorDef {
    plan::SelectorDef {
        paths: selector.paths.clone(),
        scope_filter: selector.scope_filter.clone(),
    }
}

fn relation_def(
    namespaces: &Namespaces<'_>,
    source: SourceIndex,
    name: &str,
    relation: &Relation,
) -> Result<plan::RelationDef> {
    let at = |path: String, message: String| located(namespaces.display(source), &path, message);
    let path = format!("relations.{name}");
    let part = |part: &Part, part_path: &str| -> Result<plan::Part> {
        let from = match &part.from {
            SelectorRef::Named(reference) => plan::SelectorRef::Named(
                namespaces
                    .selector(source, reference, FROM_SITE)
                    .map_err(|message| at(format!("{part_path}.from"), message))?,
            ),
            SelectorRef::Inline(selector) => {
                plan::SelectorRef::Inline(Box::new(selector_def(selector)))
            }
        };
        Ok(plan::Part {
            from,
            reader: reader(&part.reader),
        })
    };
    let rows = match &relation.rows {
        Rows::Part(one) => plan::Rows::Part(part(one, &path)?),
        Rows::Union(parts) => plan::Rows::Union(
            parts
                .iter()
                .enumerate()
                .map(|(index, one)| part(one, &format!("{path}.union[{index}]")))
                .collect::<Result<_>>()?,
        ),
        Rows::Join(join) => plan::Rows::Join(join_def(namespaces, source, &path, join)?),
    };
    let mut filters = Vec::with_capacity(relation.filters.len());
    for (filtered, predicates) in &relation.filters {
        let mut resolved = Vec::with_capacity(predicates.len());
        for predicate in predicates {
            resolved.push(match predicate {
                Predicate::In(members) => plan::Predicate::In(members_def(
                    namespaces,
                    source,
                    &format!("{path}.where.{filtered}.in"),
                    members,
                )?),
                Predicate::NotIn(members) => plan::Predicate::NotIn(members_def(
                    namespaces,
                    source,
                    &format!("{path}.where.{filtered}.not_in"),
                    members,
                )?),
                Predicate::Matches(pattern) => plan::Predicate::Matches(pattern.clone()),
                Predicate::NotMatches(pattern) => plan::Predicate::NotMatches(pattern.clone()),
            });
        }
        filters.push((filtered.clone(), resolved));
    }
    Ok(plan::RelationDef {
        rows,
        fields: relation
            .fields
            .iter()
            .map(|(name, field)| (name.clone(), field_def(field)))
            .collect(),
        transforms: relation
            .transforms
            .iter()
            .map(|(name, list)| (name.clone(), list.iter().copied().map(transform).collect()))
            .collect(),
        filters,
    })
}

/// An `in` or `not_in` list at `path`, its references resolved.
fn members_def(
    namespaces: &Namespaces<'_>,
    source: SourceIndex,
    path: &str,
    members: &[Member],
) -> Result<Vec<plan::Member>> {
    let mut resolved = Vec::with_capacity(members.len());
    for (index, member) in members.iter().enumerate() {
        resolved.push(match member {
            Member::Literal(value) => plan::Member::Literal(literal(value)),
            Member::Relation { relation, field } => plan::Member::Relation {
                relation: namespaces.relation(source, relation, MEMBER_SITE).map_err(
                    |message| {
                        located(
                            namespaces.display(source),
                            &format!("{path}[{index}].relation"),
                            message,
                        )
                    },
                )?,
                field: field.clone(),
            },
        });
    }
    Ok(resolved)
}

/// A scalar literal of an `in`/`not_in` list (the decode admits no other) as
/// a row value. A number keeps the text the YAML reader gives it, which need
/// not be the text written; the plan check refuses one.
fn literal(value: &ayaml::Value) -> alint_core::relation::Value {
    use alint_core::relation::{Number, Value};
    match value {
        ayaml::Value::Bool(flag) => Value::Bool(*flag),
        ayaml::Value::Number(number) => {
            let text = number.to_string();
            Number::from_lexeme(&text).map_or_else(|| Value::String(text.into()), Value::Number)
        }
        ayaml::Value::String(text) => Value::String(text.as_str().into()),
        _ => Value::Null,
    }
}

fn join_def(
    namespaces: &Namespaces<'_>,
    source: SourceIndex,
    path: &str,
    join: &Join,
) -> Result<plan::JoinDef> {
    let side = |reference: &Reference, key: &str| {
        namespaces
            .relation(source, reference, SIDE_SITE)
            .map_err(|message| {
                located(
                    namespaces.display(source),
                    &format!("{path}.join.{key}"),
                    message,
                )
            })
    };
    Ok(plan::JoinDef {
        left: side(&join.left, "left")?,
        right: side(&join.right, "right")?,
        on: join.on.clone(),
        unique: join_side(join.unique),
    })
}

const fn join_side(side: JoinSide) -> plan::JoinSide {
    match side {
        JoinSide::Left => plan::JoinSide::Left,
        JoinSide::Right => plan::JoinSide::Right,
    }
}

fn reader(reader: &Reader) -> plan::Reader {
    match reader {
        Reader::Entries(Entries { format, at, leaves }) => plan::Reader::Entries(plan::Entries {
            format: match format {
                EntriesFormat::Json => plan::EntriesFormat::Json,
                EntriesFormat::Yaml => plan::EntriesFormat::Yaml,
                EntriesFormat::Toml => plan::EntriesFormat::Toml,
            },
            at: at.clone(),
            leaves: *leaves,
        }),
        Reader::Read { read, optional } => plan::Reader::Read {
            read: match read {
                Read::Json(query) => plan::Read::Json(query.clone()),
                Read::Yaml(query) => plan::Read::Yaml(query.clone()),
                Read::Toml(query) => plan::Read::Toml(query.clone()),
                Read::Regex(pattern) => plan::Read::Regex(pattern.clone()),
                Read::Lines { comment } => plan::Read::Lines {
                    comment: comment.clone(),
                },
                Read::WholeFile => plan::Read::WholeFile,
            },
            optional: *optional,
        },
    }
}

fn field_def(field: &Field) -> plan::FieldDef {
    plan::FieldDef {
        source: match &field.source {
            FieldSource::Path(token) => plan::FieldSource::Path(match token {
                PathToken::Path => plan::PathToken::Path,
                PathToken::Dir => plan::PathToken::Dir,
                PathToken::Basename => plan::PathToken::Basename,
                PathToken::Stem => plan::PathToken::Stem,
                PathToken::Ext => plan::PathToken::Ext,
                PathToken::ParentName => plan::PathToken::ParentName,
            }),
            FieldSource::EntryKey => plan::FieldSource::EntryKey,
            FieldSource::EntryValue => plan::FieldSource::EntryValue,
            FieldSource::EntryParent => plan::FieldSource::EntryParent,
            FieldSource::EntryRoot => plan::FieldSource::EntryRoot,
            FieldSource::EntryPath => plan::FieldSource::EntryPath,
            FieldSource::Value => plan::FieldSource::Value,
            FieldSource::Bound(query) => plan::FieldSource::Bound(query.clone()),
            FieldSource::Side(side, name) => {
                plan::FieldSource::Side(join_side(*side), name.clone())
            }
        },
        sensitive: field.sensitive,
    }
}

const fn transform(transform: Transform) -> plan::Transform {
    match transform {
        Transform::Trim => plan::Transform::Trim,
        Transform::Lower => plan::Transform::Lower,
        Transform::SemverMajor => plan::Transform::SemverMajor,
        Transform::SemverMinor => plan::Transform::SemverMinor,
        Transform::StripRoot => plan::Transform::StripRoot,
    }
}

/// What reading the resources a selector picks needs: a manifest-derived
/// predicate reads its manifest, and `changed_since` (refused in a selector
/// in this build) would read Git's diff.
fn selector_capabilities(selector: &Selector) -> Capabilities {
    let mut capabilities = Capabilities::default();
    if let Some(filter) = &selector.scope_filter {
        capabilities.reads_content =
            filter.include_manifest_paths.is_some() || filter.exclude_manifest_paths.is_some();
        if filter.changed_since.is_some() {
            capabilities.host.push(Capability::Git {
                operations: Cow::Borrowed(&[GitOperation::Diff]),
            });
        }
    }
    capabilities
}

/// Give each relation its capabilities: its own reads, and those of every
/// selector and relation it references. `order` lists every relation after
/// the relations it reads (the plan check's topological order).
fn add_capabilities(plan: &mut Plan, order: &[RelationId]) {
    for &id in order {
        let relation = &plan.relations[id.0].relation;
        let mut capabilities = Capabilities::default();
        let mut add_from = |from: &plan::SelectorRef| match from {
            plan::SelectorRef::Named(reached) => {
                capabilities.add(&plan.selectors[reached.id.0].capabilities);
            }
            plan::SelectorRef::Inline(selector) => {
                capabilities.add(&selector_capabilities(&Selector {
                    paths: selector.paths.clone(),
                    scope_filter: selector.scope_filter.clone(),
                    reads_environment: false,
                }));
            }
        };
        let mut read = Vec::new();
        match &relation.rows {
            plan::Rows::Part(part) => {
                add_from(&part.from);
                capabilities.reads_content = true;
            }
            plan::Rows::Union(parts) => {
                for part in parts {
                    add_from(&part.from);
                }
                capabilities.reads_content = true;
            }
            plan::Rows::Join(join) => read.extend([join.left.id, join.right.id]),
        }
        for (_, predicates) in &relation.filters {
            for predicate in predicates {
                if let plan::Predicate::In(members) | plan::Predicate::NotIn(members) = predicate {
                    for member in members {
                        if let plan::Member::Relation { relation, .. } = member {
                            read.push(relation.id);
                        }
                    }
                }
            }
        }
        for other in read {
            capabilities.add(&plan.relations[other.0].capabilities);
        }
        plan.relations[id.0].capabilities = capabilities;
    }
}

/// What an object with `capabilities`, declared in a source of `class`,
/// may not do, named for a message; `None` when the source may declare it.
/// The top-level config and its `.alint.d` fragments are the user's own and
/// may declare anything; every other source is refused what would let it
/// act beyond its rules, as the `extends:` gates refuse an extended source's
/// rules (ADR-0043 decision 5).
pub(crate) fn refused(
    capabilities: &Capabilities,
    class: ConfigSourceClass,
) -> Option<&'static str> {
    if matches!(class, ConfigSourceClass::Root | ConfigSourceClass::DropIn) {
        return None;
    }
    capabilities
        .host
        .iter()
        .find_map(|capability| match capability {
            Capability::SpawnUserCommand => Some("starts a process"),
            Capability::OutOfRootRead => Some("reads outside the repository"),
            Capability::WritesDuringCheck => Some("writes files during a check"),
            _ => None,
        })
}

/// Judge each object by its declaring source's trust gates, with what it
/// references (design section 10).
fn gate_objects(plan: &Plan) -> Result<()> {
    let objects = plan
        .selectors
        .iter()
        .map(|object| {
            (
                "selector",
                "selectors",
                &object.name,
                object.source,
                &object.capabilities,
            )
        })
        .chain(plan.relations.iter().map(|object| {
            (
                "relation",
                "relations",
                &object.name,
                object.source,
                &object.capabilities,
            )
        }));
    for (noun, section, name, source, capabilities) in objects {
        let source = plan.source(source);
        if let Some(what) = refused(capabilities, source.class) {
            return Err(located(
                &source.display,
                &format!("{section}.{name}"),
                format!(
                    "this {noun} {what} (itself or through what it references), which a config \
                     reached through `extends:` may not do; declare it in the top-level config \
                     or an `.alint.d` fragment"
                ),
            ));
        }
    }
    Ok(())
}

impl Resolved<'_> {
    /// Resolve the reference fields of rule `id` (an effective rule mapping
    /// of `kind`), each in the source that wrote it (`origins`), and record
    /// them on the plan. A field no version 2 source wrote names no relation.
    pub(crate) fn resolve_rule(
        &mut self,
        id: &str,
        rule: &ayaml::Mapping,
        origins: &[(String, PathBuf)],
    ) -> Result<()> {
        let kind = rule
            .get("kind")
            .and_then(ayaml::Value::as_str)
            .unwrap_or("");
        let mut references = Vec::new();
        for &field in super::origins::reference_fields(kind) {
            let Some(value) = rule.get(field) else {
                continue;
            };
            let origin = origins
                .iter()
                .find(|(held, _)| held == field)
                .and_then(|(_, path)| self.namespaces.source_at(path));
            let Some(source) = origin else {
                return Err(Error::rule_config(
                    id,
                    format!(
                        "`{field}` names a relation, but a version 1 config wrote it; only a \
                         version 2 config's rules reference selectors and relations"
                    ),
                ));
            };
            let display = self.namespaces.display(source).to_string();
            let fail = |message: String| {
                Error::rule_config(id, format!("`{field}` (written by {display}): {message}"))
            };
            let text = value.as_str().ok_or_else(|| {
                fail(format!(
                    "expected a relation name, found {}",
                    super::decode::describe(value)
                ))
            })?;
            let reference = super::decode::parse_reference(text).map_err(fail)?;
            let relation = self
                .namespaces
                .relation(source, &reference, &format!("`{field}:` names a relation"))
                .map_err(fail)?;
            references.push(RuleReference {
                field: field.to_string(),
                source,
                relation,
            });
        }
        if !references.is_empty() {
            self.plan.rules.push(RuleReferences {
                rule: id.to_string(),
                references,
            });
        }
        Ok(())
    }

    /// Judge rule `id`, which an extended source shaped (`extended` names
    /// it), by the extended sources' gates, with the capabilities of what it
    /// references: a rule's capabilities are its own plus those of
    /// everything it references (design section 10).
    pub(crate) fn judge_rule(&self, id: &str, extended: Option<&str>) -> Result<()> {
        let Some(extended) = extended else {
            return Ok(());
        };
        let capabilities = self.plan.rule_capabilities(id);
        match refused(&capabilities, ConfigSourceClass::LocalExtend) {
            Some(what) => Err(Error::rule_config(
                id,
                format!(
                    "a relation this rule references {what}, and an extended config \
                     ({extended}) shapes the rule; such a rule must be declared entirely in \
                     your own top-level config"
                ),
            )),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
