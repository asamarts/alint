//! Merging configs: each `extends:` source onto what its list merged so far,
//! and each `.alint.d/` drop-in onto the top-level config. Pulled out of
//! `lib.rs` to keep that file to the public surface and the config's shape.
//!
//! A list merges into one [`Merger`], updated in place, so each config pushed
//! onto it costs what that config brings in, wherever it falls in the list.
//! The merge it replaced rebuilt what it merged onto for every config, so a
//! list of N sources cost N times what it merged (#580). The tests
//! (`merge_tests.rs`) check the two against each other on generated lists.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::path::PathBuf;

use alint_core::{ConfigCompatibility, ConfigSourceClass, Error, FactSpec, Result, StableVersion};
use ayaml::Mapping;

use crate::{RawConfig, UntrustedWhenVarRead, merge_mapping_fields};

/// The entries of the list `$field` of `$source`, a `&mut Cow<RawConfig>`
/// ([`Entries`]).
macro_rules! entries {
    ($source:expr, $($field:ident).+) => {
        match &mut *$source {
            Cow::Owned(config) => {
                Entries::Owned(std::mem::take(&mut config.$($field).+).into_iter())
            }
            Cow::Borrowed(config) => Entries::Borrowed(config.$($field).+.iter()),
        }
    };
}
/// The most entries (rules, templates, facts, vars, `ignore:` patterns,
/// recorded `when` reads, selectors and relations) a config may hold once its
/// `extends:` chain and drop-ins are merged. Real configs hold hundreds; an
/// `extends:` diamond that lists one source twice per level once doubled the
/// lists at each level.
pub(crate) const MAX_MERGED_ENTRIES: usize = 1 << 20;

impl RawConfig {
    /// The entries [`MAX_MERGED_ENTRIES`] counts.
    fn entry_count(&self) -> usize {
        self.rules.len()
            + self.templates.len()
            + self.facts.len()
            + self.vars.len()
            + self.ignore.len()
            + self.untrusted_when_var_reads.len()
            + self.v2.object_count()
    }

    /// What an `extends:` merge or load handles of the config
    /// ([`crate::loader::LoadState::charge_extends`]): its entries, and each version
    /// 2 source, which a merge looks for among those it holds.
    pub(crate) fn merge_size(&self) -> usize {
        self.entry_count() + self.v2.sources.len()
    }
}

/// `b` merged onto `a` ([`Merger`]).
#[cfg(test)]
pub(crate) fn merge(a: RawConfig, b: RawConfig) -> Result<RawConfig> {
    let mut merger = Merger::new(a);
    merger.push(b)?;
    Ok(merger.finish())
}

/// Configs merged in order, each onto what the ones before it merged, with
/// the later one winning on conflicts:
/// - `rules` dedupe by id; rule mappings are **field-merged**, not replaced:
///   the later one's keys override the earlier one's individually. So a child
///   that specifies `{id: X, level: off}` over a parent `{id: X, kind:
///   file_exists, paths: README.md, level: error}` yields a merged rule with
///   kind + paths still set and level overridden. Ordering: each id where it
///   first appears; then the rules without an id (which `finalize` refuses),
///   each once. `templates` merge the same way.
/// - `facts` dedupe by id; a later fact replaces an earlier one wholesale, in
///   its place (fact kinds are a discriminated union: field-merging
///   `any_file_exists` with `all_files_exist` would produce an invalid fact).
/// - `vars` merged as a map; later values override. A var's env-derived mark
///   follows the value that wins: a later literal clears it, a later
///   interpolated value sets it.
/// - `ignore`, the recorded `when` reads and `trusted_extends` keep each entry
///   once, where it first appears, so a source an `extends:` diamond reaches
///   twice does not double them at every level. The compatibility records,
///   the environment's values and the version 2 sources (by file) keep the
///   first config's list as it is and add each later entry they do not hold.
///   `config_files` keeps every entry.
/// - `version`, `respect_gitignore`, `fix_size_limit`, `nested_configs`,
///   `allow_out_of_root` and `baseline` take the later value when it is set,
///   else keep the earlier one (defaults are applied once, in `finalize`). An
///   `extends:`'d config's values are dropped before the merge
///   ([`RawConfig::drop_top_level_settings`]); `allow_out_of_root`,
///   `baseline` and `trusted_extends` are refused from one before it.
/// - `extends` is always left empty on the merged result; resolved already.
///
/// Each list is kept with what makes adding to it a lookup: rules and
/// templates with each id's position, the deduplicated lists with a set of
/// their entries, the version 2 sources with their files. So a push costs
/// what it brings in, and the merged size is counted as it grows.
#[derive(Default)]
pub(crate) struct Merger {
    /// The config the merge starts from, until another is pushed onto it: a
    /// merge of nothing leaves it as it is, its lists too.
    first: Option<RawConfig>,
    /// What was merged, but for the lists kept below, which [`Self::finish`]
    /// puts back.
    merged: RawConfig,
    rules: ById,
    templates: ById,
    facts: Vec<FactSpec>,
    /// The position of each fact's id in `facts`.
    fact_positions: HashMap<String, usize>,
    ignore: Unique<String>,
    untrusted_when_var_reads: Unique<UntrustedWhenVarRead>,
    trusted_extends: Unique<String>,
    environment_values: Unique<String>,
    /// Each of `merged.compatibility`.
    compatibility: HashSet<CompatibilityKey>,
    /// The file of each of `merged.v2.sources`.
    v2_paths: HashSet<PathBuf>,
    /// The selectors and relations of `merged.v2.sources`.
    v2_objects: usize,
}

impl Merger {
    /// A merge that starts from `first`.
    pub(crate) fn new(first: RawConfig) -> Self {
        Self {
            first: Some(first),
            ..Self::default()
        }
    }

    /// The `version:` merged so far.
    pub(crate) fn version(&self) -> Option<u32> {
        self.first
            .as_ref()
            .map_or(self.merged.version, |first| first.version)
    }

    /// Merge `source` onto what was merged so far, moving its entries in.
    pub(crate) fn push(&mut self, source: RawConfig) -> Result<()> {
        self.push_from(Cow::Owned(source))
    }

    /// Merge `source`, which a later entry of its list merges again, onto
    /// what was merged so far. An entry the merge takes (a rule, a var) is
    /// copied in; one it holds already (an `ignore:` pattern, a version 2
    /// source) is only looked up, so the caller need not copy all of it.
    pub(crate) fn push_held(&mut self, source: &RawConfig) -> Result<()> {
        self.push_from(Cow::Borrowed(source))
    }

    /// What was merged.
    pub(crate) fn finish(self) -> RawConfig {
        let Self {
            first,
            mut merged,
            rules,
            templates,
            facts,
            ignore,
            untrusted_when_var_reads,
            trusted_extends,
            environment_values,
            ..
        } = self;
        if let Some(first) = first {
            return first;
        }
        merged.rules = rules.into_list();
        merged.templates = templates.into_list();
        merged.facts = facts;
        merged.ignore = ignore.entries;
        merged.untrusted_when_var_reads = untrusted_when_var_reads.entries;
        merged.trusted_extends = trusted_extends.entries;
        merged.environment_values = environment_values.entries;
        merged
    }

    /// Take `first` as what a merge merges onto: its settings, its vars and
    /// the lists a merge does not reduce, as they are; the lists it reduces to
    /// each entry once, as a later config's.
    fn start(&mut self, mut first: RawConfig) {
        self.compatibility = first
            .compatibility
            .iter()
            .map(CompatibilityKey::of)
            .collect();
        self.environment_values = Unique::of(std::mem::take(&mut first.environment_values));
        self.v2_paths = first
            .v2
            .sources
            .iter()
            .map(|objects| objects.source.path.clone())
            .collect();
        self.v2_objects = first.v2.object_count();
        #[cfg(test)]
        touch(self.compatibility.len() + self.environment_values.len() + self.v2_paths.len());
        let mut first = Cow::Owned(first);
        self.add_reduced(&mut first);
        self.merged = RawConfig {
            requires: None,
            extends: Vec::new(),
            extends_environment: Vec::new(),
            ..first.into_owned()
        };
    }

    fn push_from(&mut self, mut source: Cow<'_, RawConfig>) -> Result<()> {
        if let Some(first) = self.first.take() {
            self.start(first);
        }
        // Every field, so a new one is merged here, or said not to be.
        let RawConfig {
            version,
            requires: _,
            extends: _,
            ignore: _,
            respect_gitignore,
            vars,
            facts: _,
            templates: _,
            rules: _,
            fix_size_limit,
            nested_configs,
            allow_out_of_root,
            baseline,
            trusted_extends: _,
            env_vars,
            untrusted_when_var_reads: _,
            compatibility: _,
            environment_values: _,
            config_files: _,
            field_source_names: _,
            extends_environment: _,
            source_size,
            surface,
            v2: _,
        } = source.as_ref();
        let merged = &mut self.merged;
        // The language follows the version: the source that sets it decides.
        if version.is_some() {
            merged.surface = *surface;
        }
        merged.version = version.or(merged.version);
        merged.respect_gitignore = respect_gitignore.or(merged.respect_gitignore);
        merged.fix_size_limit = fix_size_limit.or(merged.fix_size_limit);
        merged.nested_configs = nested_configs.or(merged.nested_configs);
        // `allow_out_of_root` and `baseline` are top-level-only (an
        // `extends:`'d value is refused upstream); the later config wins when
        // it sets one.
        if !allow_out_of_root.is_confined() {
            merged.allow_out_of_root = allow_out_of_root.clone();
        }
        if baseline.is_some() {
            merged.baseline.clone_from(baseline);
        }
        merged.source_size = merged.source_size.saturating_add(*source_size);
        // The env-derived mark follows the value that wins: a later literal
        // value clears it, an interpolated one sets it.
        for name in vars.keys() {
            match env_vars.get(name) {
                Some(raw) => merged.env_vars.insert(name.clone(), raw.clone()),
                None => merged.env_vars.remove(name),
            };
        }
        #[cfg(test)]
        touch(vars.len());
        match &mut source {
            Cow::Owned(config) => merged.vars.extend(std::mem::take(&mut config.vars)),
            Cow::Borrowed(config) => merged.vars.extend(
                config
                    .vars
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            ),
        }
        for record in entries!(&mut source, compatibility) {
            if self.compatibility.insert(CompatibilityKey::of(&record)) {
                self.merged.compatibility.push(record.into_owned());
            }
        }
        for value in entries!(&mut source, environment_values) {
            self.environment_values.add(value);
        }
        self.merged
            .config_files
            .extend(entries!(&mut source, config_files).map(Cow::into_owned));
        // Set once, on the top-level result after every merge.
        if self.merged.field_source_names.is_empty() {
            self.merged.field_source_names = entries!(&mut source, field_source_names)
                .map(Cow::into_owned)
                .collect();
        }
        // A file an `extends:` diamond reaches twice declares its objects once.
        for objects in entries!(&mut source, v2.sources) {
            if !self.v2_paths.contains(&objects.source.path) {
                self.v2_paths.insert(objects.source.path.clone());
                self.v2_objects += objects.object_count();
                self.merged.v2.sources.push(objects.into_owned());
            }
        }
        self.add_reduced(&mut source);
        self.check_size()
    }

    /// Add the entries of `source`'s lists that keep each entry once.
    fn add_reduced(&mut self, source: &mut Cow<'_, RawConfig>) {
        for pattern in entries!(source, ignore) {
            self.ignore.add(pattern);
        }
        for read in entries!(source, untrusted_when_var_reads) {
            self.untrusted_when_var_reads.add(read);
        }
        // `trusted_extends:` is top-level-only: consumed at the top before any
        // merge, and refused from an extended or nested source. Kept so a
        // stray value still reaches that refusal.
        for url in entries!(source, trusted_extends) {
            self.trusted_extends.add(url);
        }
        for fact in entries!(source, facts) {
            let fact = fact.into_owned();
            if let Some(&at) = self.fact_positions.get(&fact.id) {
                self.facts[at] = fact;
            } else {
                self.fact_positions
                    .insert(fact.id.clone(), self.facts.len());
                self.facts.push(fact);
            }
        }
        for template in entries!(source, templates) {
            self.templates.add(template);
        }
        for rule in entries!(source, rules) {
            self.rules.add(rule);
        }
    }

    /// The entries [`RawConfig::entry_count`] counts of what was merged.
    fn entry_count(&self) -> usize {
        self.rules.len()
            + self.templates.len()
            + self.facts.len()
            + self.merged.vars.len()
            + self.ignore.len()
            + self.untrusted_when_var_reads.len()
            + self.v2_objects
    }

    /// Refuse what was merged past [`MAX_MERGED_ENTRIES`].
    fn check_size(&self) -> Result<()> {
        let entries = self.entry_count();
        if entries <= MAX_MERGED_ENTRIES {
            return Ok(());
        }
        // A version 1 config's message is unchanged.
        let held = if self.v2_objects == 0 {
            "rules, templates, facts, vars and `ignore:` patterns"
        } else {
            "rules, templates, facts, vars, `ignore:` patterns, selectors and relations"
        };
        Err(Error::Other(format!(
            "the merged config holds {entries} entries ({held}), more than the \
             {MAX_MERGED_ENTRIES} supported; flatten the `extends:` chain or split the ruleset"
        )))
    }
}

/// The entries of one list of a config pushed onto a [`Merger`]: moved out of
/// it, or borrowed from it when it is held for a later entry
/// ([`Merger::push_held`]).
enum Entries<'s, T> {
    Owned(std::vec::IntoIter<T>),
    Borrowed(std::slice::Iter<'s, T>),
}

impl<'s, T: Clone> Iterator for Entries<'s, T> {
    type Item = Cow<'s, T>;

    fn next(&mut self) -> Option<Cow<'s, T>> {
        let entry = match self {
            Self::Owned(entries) => entries.next().map(Cow::Owned),
            Self::Borrowed(entries) => entries.next().map(Cow::Borrowed),
        };
        #[cfg(test)]
        touch(usize::from(entry.is_some()));
        entry
    }
}

/// A list that keeps each entry once, where it first appears, with a set of
/// its entries beside it: adding one is a lookup, not a scan of the list.
struct Unique<T> {
    entries: Vec<T>,
    held: HashSet<T>,
}

impl<T> Default for Unique<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            held: HashSet::new(),
        }
    }
}

impl<T: Clone + Eq + Hash> Unique<T> {
    /// `entries` as they are, repeats too, each held.
    fn of(entries: Vec<T>) -> Self {
        let held = entries.iter().cloned().collect();
        Self { entries, held }
    }

    /// Add `entry`, unless it is held already.
    fn add(&mut self, entry: Cow<'_, T>) {
        if !self.held.contains(entry.as_ref()) {
            let entry = entry.into_owned();
            self.held.insert(entry.clone());
            self.entries.push(entry);
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Rules, or templates, merged by id: a later one field-merges onto the one
/// with its id, in that one's place.
#[derive(Default)]
struct ById {
    /// Those with an id, in the order the ids first appear.
    with_id: Vec<Mapping>,
    /// The position of each id in `with_id`.
    positions: HashMap<String, usize>,
    /// Those without an id, each once, after the others: they merge with
    /// nothing, and `finalize` refuses them.
    orphans: Unique<Mapping>,
}

impl ById {
    fn add(&mut self, mapping: Cow<'_, Mapping>) {
        let Some(id) = mapping.get("id").and_then(ayaml::Value::as_str) else {
            self.orphans.add(mapping);
            return;
        };
        if let Some(&at) = self.positions.get(id) {
            // Nested structures (a `fix:` block, a `paths:` include/exclude
            // pair) are replaced wholesale: overriding
            // `fix.file_create.content` alone would be too surprising.
            merge_mapping_fields(&mut self.with_id[at], mapping.into_owned());
        } else {
            self.positions.insert(id.to_owned(), self.with_id.len());
            self.with_id.push(mapping.into_owned());
        }
    }

    fn len(&self) -> usize {
        self.with_id.len() + self.orphans.len()
    }

    fn into_list(self) -> Vec<Mapping> {
        let mut list = self.with_id;
        list.extend(self.orphans.entries);
        list
    }
}

/// All of a compatibility record, which `==` compares and a set can hold
/// ([`ConfigCompatibility`] is not `Hash`).
#[derive(PartialEq, Eq, Hash)]
struct CompatibilityKey(
    String,
    ConfigSourceClass,
    u32,
    Option<StableVersion>,
    Option<u32>,
);

impl CompatibilityKey {
    fn of(record: &ConfigCompatibility) -> Self {
        // Every field, so a new one is compared here too.
        let ConfigCompatibility {
            source,
            source_class,
            config_version,
            requires_alint,
            consumer_version,
        } = record;
        Self(
            source.clone(),
            *source_class,
            *config_version,
            *requires_alint,
            *consumer_version,
        )
    }
}

/// Add each of `incoming` that `target` does not hold yet.
pub(crate) fn extend_compatibility(
    target: &mut Vec<ConfigCompatibility>,
    incoming: impl IntoIterator<Item = ConfigCompatibility>,
) {
    for record in incoming {
        if !target.contains(&record) {
            target.push(record);
        }
    }
}

#[cfg(test)]
thread_local! {
    /// How many entries the [`Merger`]s on this thread have handled: each of
    /// a pushed config's lists, its vars, and the lists of the config a merge
    /// starts from that it reads.
    pub(crate) static MERGE_TOUCHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn touch(entries: usize) {
    MERGE_TOUCHED.with(|touched| touched.set(touched.get() + entries));
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
