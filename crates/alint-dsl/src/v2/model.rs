//! The decoded form of a version 2 source's selectors, relations and
//! `extends:` aliases (design `v0.23/selectors-and-constraints.md`, sections 4,
//! 5, 7, 8 and 10).
//!
//! Decoding checks the syntax only: every key is known, every value has its
//! type, and every name is literal and well formed. Nothing here is resolved.
//! A reference keeps the name it was written with, so per-source resolution,
//! the plan check (field existence, cycles, union parts binding every field)
//! and the trust gates can run once every source is loaded
//! ([`super::resolve`]), which turns them into the plan the loaded config
//! carries.
//!
//! Every type is crate-private: version 2 is experimental (ADR-0034 decision
//! 1), so none of this is library API.

use std::path::{Path, PathBuf};

use alint_core::ConfigSourceClass;

/// The version 2 objects of every source a configuration was loaded from, in
/// load order, each with the source that declared it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Objects {
    pub(crate) sources: Vec<SourceObjects>,
}

impl Objects {
    /// `self`'s sources, then those of `other` that `self` does not hold yet:
    /// a file an `extends:` diamond reaches twice declares its objects once.
    pub(crate) fn merged(mut self, other: Self) -> Self {
        if other.sources.is_empty() {
            return self;
        }
        for source in other.sources {
            if !self
                .sources
                .iter()
                .any(|held| held.source.path == source.source.path)
            {
                self.sources.push(source);
            }
        }
        self
    }

    /// Whether any version 2 source is recorded.
    pub(crate) fn has_sources(&self) -> bool {
        !self.sources.is_empty()
    }

    /// Record that the source extends the local file `target`, through
    /// `alias` when its entry has one, once the loader has read it. Called on
    /// one source's objects, before any merge.
    pub(crate) fn record_extended(&mut self, alias: Option<&str>, target: Option<&Path>) {
        for source in &mut self.sources {
            if let Some(target) = target {
                source.extended.push(target.to_path_buf());
            }
            let Some(alias) = alias else {
                continue;
            };
            for (name, held) in &mut source.aliases {
                if name == alias {
                    held.target = target.map(Path::to_path_buf);
                }
            }
        }
    }
}

/// The objects one source declares, under its own namespace (design section
/// 10): a bare name resolves among them, a qualified one through an alias.
#[derive(Debug, Clone)]
pub(crate) struct SourceObjects {
    pub(crate) source: SourceId,
    pub(crate) selectors: Vec<(String, Selector)>,
    pub(crate) relations: Vec<(String, Relation)>,
    pub(crate) aliases: Vec<(String, Alias)>,
    /// The resolved local files its `extends:` entries name, aliased or not,
    /// in the order written: where a hint finds a name the source could reach
    /// through an alias.
    pub(crate) extended: Vec<PathBuf>,
}

impl SourceObjects {
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.selectors.is_empty() && self.relations.is_empty() && self.aliases.is_empty()
    }
}

/// The source that declared an object: the provenance `list` and `explain`
/// show and resolution keys on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceId {
    /// How a message names the source, as the compatibility errors do.
    pub(crate) display: String,
    /// The resolved file, which identifies the source across an `extends:`
    /// diamond.
    pub(crate) path: PathBuf,
    pub(crate) class: ConfigSourceClass,
}

/// An `extends:` entry's `as:`: the name a source gives the source it
/// extends, so it can reach that source's objects as `alias.name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Alias {
    /// The entry's URL, as written.
    pub(crate) url: String,
    /// The extended source's resolved file, once the loader has read it.
    pub(crate) target: Option<PathBuf>,
}

/// A name written where an object is referenced: bare, which resolves in the
/// referencing source, or qualified through an `extends:` alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    pub(crate) alias: Option<String>,
    pub(crate) name: String,
}

/// A selector (section 4): the resources a relation reads, with a rule's
/// `paths:` grammar and its `scope_filter:` predicates but `changed_since`.
#[derive(Debug, Clone)]
pub(crate) struct Selector {
    pub(crate) paths: alint_core::PathsSpec,
    pub(crate) scope_filter: Option<alint_core::ScopeFilterSpec>,
    /// Its definition takes a value from the environment
    /// ([`super::Environment`]).
    pub(crate) reads_environment: bool,
}

/// Where a selector is referenced: a name, or an anonymous inline selector.
#[derive(Debug, Clone)]
pub(crate) enum SelectorRef {
    Named(Reference),
    Inline(Box<Selector>),
}

/// A relation (section 5): a plan that reads rows and names their fields.
#[derive(Debug, Clone)]
pub(crate) struct Relation {
    pub(crate) rows: Rows,
    /// The fields, in the order written.
    pub(crate) fields: Vec<(String, Field)>,
    /// Each field's transforms, applied in order before `filters`.
    pub(crate) transforms: Vec<(String, Vec<Transform>)>,
    /// `where:`: each field's predicates, all of which must hold.
    pub(crate) filters: Vec<(String, Vec<Predicate>)>,
    /// Its definition takes a value from the environment outside its
    /// fields' `where:` and `transform:` entries ([`super::Environment`]).
    pub(crate) reads_environment: bool,
}

/// Where a relation's rows come from: exactly one of `entries:`, `read:`,
/// `union:` and `join:`.
#[derive(Debug, Clone)]
pub(crate) enum Rows {
    Part(Part),
    Union(Vec<Part>),
    Join(Join),
}

/// A selector and how its resources are read: a relation with `entries:` or
/// `read:`, or one part of a `union:`.
#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub(crate) from: SelectorRef,
    pub(crate) reader: Reader,
}

#[derive(Debug, Clone)]
pub(crate) enum Reader {
    Entries(Entries),
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
pub(crate) struct Entries {
    pub(crate) format: EntriesFormat,
    pub(crate) at: Vec<String>,
    pub(crate) leaves: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntriesFormat {
    Json,
    Yaml,
    Toml,
}

/// `read:`: the extract forms `cross_file` has, limited in v0.23 to these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Read {
    Json(String),
    Yaml(String),
    Toml(String),
    Regex(String),
    Lines { comment: Option<String> },
    WholeFile,
}

/// `join:` (section 8): an inner equality join of two relations, one side
/// unique on the join key.
#[derive(Debug, Clone)]
pub(crate) struct Join {
    pub(crate) left: Reference,
    pub(crate) right: Reference,
    /// Each left field with the right field it must equal, in the order
    /// written.
    pub(crate) on: Vec<(String, String)>,
    pub(crate) unique: JoinSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinSide {
    Left,
    Right,
}

/// A field: where its value comes from, and whether it is kept out of
/// messages, the JSON report and baselines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) source: FieldSource,
    pub(crate) sensitive: bool,
}

/// The sources a field may take (section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldSource {
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
    Bound(String),
    /// `left.F` or `right.F` in a join's output.
    Side(JoinSide, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathToken {
    Path,
    Dir,
    Basename,
    Stem,
    Ext,
    ParentName,
}

/// The fixed transforms (ADR-0037 decision 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transform {
    Trim,
    Lower,
    SemverMajor,
    SemverMinor,
    StripRoot,
}

/// A row filter's predicate (section 7).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Predicate {
    In(Vec<Member>),
    NotIn(Vec<Member>),
    Matches(String),
    NotMatches(String),
}

/// An item of an `in` or `not_in` list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Member {
    /// A scalar literal: null, a bool, a number or a string.
    Literal(ayaml::Value),
    /// `{ relation: R, field: F }`: every value of another relation's field.
    Relation { relation: Reference, field: String },
}
