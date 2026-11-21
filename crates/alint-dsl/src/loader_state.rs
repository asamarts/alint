//! What one load's `extends:` resolution keeps from entry to entry: the files
//! it loaded, the sources it parsed, the environment's values it recorded and
//! the budgets its entries are charged against. Pulled out of `loader.rs` to
//! keep that file to the resolution itself.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use alint_core::{ConfigSourceClass, Error, Result, RunningVersion};

use super::SourceContext;
use crate::RawConfig;

/// Mutable state shared by one `load_with` call's whole extends resolution.
pub(crate) struct LoadState {
    /// Ancestors on the current DFS path (cycle detection + depth bound).
    pub(crate) visiting: HashSet<PathBuf>,
    /// Completed non-top-level local loads, reused across a diamond chain: two
    /// entries that both extend the same file at every level would otherwise
    /// reload it once per path through the DAG (2^depth loads, an effective
    /// hang from a hostile repo). `is_top` loads read their own trust and
    /// confinement settings and are never memoized.
    /// `None` marks a key seen once: only a key loaded a SECOND time (a real
    /// diamond) is cloned into the memo, so a linear chain never pays for it while
    /// a diamond still loads each shared file at most twice.
    memo: HashMap<MemoKey, Option<RawConfig>, MemoHasher>,
    pub(crate) running_version: RunningVersion,
    /// Every [`RawConfig::environment_values`] parsed so far, so an error the
    /// resolution returns before the configs merge (an unreadable `extends:`
    /// target named by the environment) can still be scrubbed of them.
    pub(crate) environment_values: EnvironmentValues,
    /// Every local file loaded so far, resolved: the top-level config, its
    /// drop-ins and each local `extends:` target, once each.
    pub(crate) config_files: Vec<PathBuf>,
    /// Each of `config_files` with its index there.
    config_file_indices: HashMap<PathBuf, usize>,
    /// The `extends:` sources the rules' field-source markers name.
    pub(crate) field_source_names: crate::trust::FieldSourceNames,
    /// The work of every `extends:` merge so far, in entries
    /// ([`LoadState::charge_extends`]).
    pub(crate) extends_entries: usize,
    /// The size of every source an `extends:` entry merged so far, in bytes
    /// ([`LoadState::charge_extends`]).
    pub(crate) extends_bytes: usize,
    /// The top-level config's directory, resolved, which a local config is
    /// named relative to ([`super::local_source_display`]), also where
    /// `allow_out_of_root: true` lifts the confinement to it.
    pub(crate) display_root: Option<PathBuf>,
    /// The `trusted_extends:` lists handed out so far ([`TrustedExtends`]).
    trusted_lists: usize,
    /// Each remote body parsed so far, by its hash, and each bundled ruleset,
    /// by its name: as parsed, before an entry checks, filters or caps it. An
    /// entry that names one again (another query, another `only:`) copies it
    /// instead of reading, hashing and parsing it again.
    remote_sources: HashMap<String, RawConfig>,
    bundled_sources: HashMap<String, RawConfig>,
    /// The sources whose top-level settings a warning has named, with the
    /// settings: one warning each, however many entries name the source.
    warned_settings: HashSet<(String, Vec<&'static str>)>,
}

impl Default for LoadState {
    fn default() -> Self {
        Self {
            visiting: HashSet::new(),
            memo: HashMap::default(),
            running_version: RunningVersion::current(),
            environment_values: EnvironmentValues::default(),
            config_files: Vec::new(),
            config_file_indices: HashMap::new(),
            field_source_names: crate::trust::FieldSourceNames::default(),
            extends_entries: 0,
            extends_bytes: 0,
            display_root: None,
            trusted_lists: 0,
            remote_sources: HashMap::new(),
            bundled_sources: HashMap::new(),
            warned_settings: HashSet::new(),
        }
    }
}

/// The values the environment gave one load, in the order recorded, each
/// once: a set beside the list makes recording one a lookup, not a scan of
/// all of them.
#[derive(Default)]
pub(crate) struct EnvironmentValues {
    values: Vec<String>,
    held: HashSet<String>,
}

impl EnvironmentValues {
    /// Record `value`, unless it is recorded already.
    pub(crate) fn add(&mut self, value: &str) {
        if !self.held.contains(value) {
            self.held.insert(value.to_owned());
            self.values.push(value.to_owned());
        }
    }

    /// Record each of `values`.
    pub(crate) fn extend(&mut self, values: &[String]) {
        for value in values {
            self.add(value);
        }
    }

    pub(crate) fn as_slice(&self) -> &[String] {
        &self.values
    }
}

/// A top-level config's `trusted_extends:` list (or a drop-in's, for its own
/// chain), which each remote source loaded below it is checked against.
#[derive(Default)]
pub(crate) struct TrustedExtends {
    /// Which list this is, for the memo: a load below one top-level config is
    /// not one below another. Keying the memo by the list itself copied and
    /// hashed all of it for every local load (2,000 entries with 20,000
    /// trusted URLs took 23 seconds). `0` is the empty list a top-level load
    /// is handed and does not read.
    id: usize,
    urls: HashSet<String>,
}

impl TrustedExtends {
    /// Whether the list names `url`.
    pub(crate) fn contains(&self, url: &str) -> bool {
        self.urls.contains(url)
    }
}

#[derive(PartialEq, Eq, Hash)]
pub(crate) struct MemoKey {
    path: PathBuf,
    confine: Option<PathBuf>,
    trusted: usize,
    consumer_version: Option<u32>,
    source_class: ConfigSourceClass,
    /// A source loaded below an entry read from the environment records how
    /// its own `extends:` targets are named, which another load of it skips.
    environment_chosen: bool,
}

impl MemoKey {
    pub(crate) fn new(
        path: &Path,
        confine: Option<&Path>,
        trusted: &TrustedExtends,
        context: &SourceContext<'_>,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            confine: confine.map(Path::to_path_buf),
            trusted: trusted.id,
            consumer_version: context.consumer_version,
            source_class: context.source_class,
            environment_chosen: context.environment_chosen,
        }
    }
}

/// The entries one load's `extends:` entries (and `.alint.d/` drop-ins) may
/// load and merge, in all ([`LoadState::charge_extends`],
/// [`LoadState::charge_extends_load`], [`LoadState::charge_extends_merged_onto`]):
/// four merged configs of the largest size. An entry is charged its source
/// twice, to load it and to merge it, so a flat list of distinct sources that
/// override none of each other's rules or templates, and repeat no id within
/// one, stays within it while they hold at most half of it in all (an empty
/// source counting as one entry), and a source one level down, loaded and
/// merged again with what it extends, the rest. A rule or template that
/// overrides an earlier one of its id, in its own source too, is charged once
/// more, but for one an earlier entry naming the same source wrote last, which
/// is not merged again ([`crate::merge::Merger::push_repeated`]). Sources that
/// hold the same entries are charged for each: they can pass it though they
/// merge to fewer than [`crate::MAX_MERGED_ENTRIES`].
pub(crate) const MAX_EXTENDS_MERGE_WORK: usize = 4 * crate::MAX_MERGED_ENTRIES;

/// The bytes of source one load's `extends:` entries may merge, in all
/// ([`LoadState::charge_extends`]): twice the largest source. Entries are
/// charged in entries above, which a source of few, large rules (a rule of
/// 50,000 keys) or of long text (a 15 MiB comment) makes cheap: what an entry
/// copies, checks and merges grows with the source's size.
pub(crate) const MAX_EXTENDS_SOURCE_BYTES: usize = 2 * alint_core::MAX_STRUCTURED_BYTES;

impl LoadState {
    /// Note `canonical` among the files the configuration came from: its
    /// index there.
    pub(crate) fn record_config_file(&mut self, canonical: &Path) -> usize {
        if let Some(&index) = self.config_file_indices.get(canonical) {
            return index;
        }
        self.config_files.push(canonical.to_path_buf());
        let index = self.config_files.len() - 1;
        self.config_file_indices
            .insert(canonical.to_path_buf(), index);
        index
    }

    /// `urls`, a top-level config's `trusted_extends:`, as the loads below it
    /// read it.
    pub(crate) fn trusted_extends(&mut self, urls: Vec<String>) -> TrustedExtends {
        self.trusted_lists += 1;
        TrustedExtends {
            id: self.trusted_lists,
            urls: urls.into_iter().collect(),
        }
    }

    /// Charge an `extends:` merge of `source` onto what its list merged so far,
    /// or of a drop-in onto the config, with its work: what it brings in
    /// ([`RawConfig::merge_size`], and at least one entry), as a merge onto
    /// what was merged handles only that ([`crate::merge::Merger`]), and the
    /// bytes of `source` ([`MAX_EXTENDS_SOURCE_BYTES`]). One load's entries
    /// may do [`MAX_EXTENDS_MERGE_WORK`] in all, so they are bounded however
    /// its lists repeat a source (`[a, b, a, b, ...]`).
    pub(crate) fn charge_extends(&mut self, source: &RawConfig) -> Result<()> {
        self.charge_extends_work(source.merge_size().max(1))?;
        self.extends_bytes = self.extends_bytes.saturating_add(source.source_size);
        if self.extends_bytes > MAX_EXTENDS_SOURCE_BYTES {
            return Err(Error::Other(format!(
                "the `extends:` entries of this config merge more than \
                 {MAX_EXTENDS_SOURCE_BYTES} bytes of sources in all (each source counted for \
                 every entry that merges it, as its text, or as its YAML aliases expand if that \
                 is larger, and a source that extends others with what it merged); list each \
                 source once, or extend fewer and smaller sources"
            )));
        }
        Ok(())
    }

    /// Charge an `extends:` merge, or a drop-in's, with the rules and
    /// templates it field-merged onto one with their id
    /// ([`crate::merge::Merger::take_merged_onto`]), once more: each is about as
    /// much work again as one it brings in. A list whose sources share their
    /// ids (`[a, b, a, b, ...]`, `b` overriding each rule of `a`) does that for
    /// every rule at every entry; the merge that rebuilt what was merged so far
    /// was charged for that too. Charged after the merge, which is bounded by
    /// what it brings in.
    pub(crate) fn charge_extends_merged_onto(&mut self, merged_onto: usize) -> Result<()> {
        self.charge_extends_work(merged_onto)
    }

    /// Charge an `extends:` entry with loading `source` (read, or copied from
    /// an earlier load), gating it and marking its rules: its entries before
    /// `only:` or `except:` keeps some of them, and at least one. So entries
    /// that name one source in other words, or keep other rules of it, cost
    /// what they load, not what they merge.
    pub(crate) fn charge_extends_load(&mut self, source: &RawConfig) -> Result<()> {
        self.charge_extends_work(source.merge_size().max(1))
    }

    /// Charge a repeated `extends:` entry that merges no new source (consecutive
    /// duplicate) so an unbounded list of repeats still hits the merge budget.
    pub(crate) fn charge_extends_entry(&mut self) -> Result<()> {
        self.charge_extends_work(1)
    }

    fn charge_extends_work(&mut self, work: usize) -> Result<()> {
        self.extends_entries = self.extends_entries.saturating_add(work);
        if self.extends_entries > MAX_EXTENDS_MERGE_WORK {
            return Err(Error::Other(format!(
                "the `extends:` entries and `.alint.d/` drop-ins of this config load and merge \
                 more than {MAX_EXTENDS_MERGE_WORK} entries in all (rules, templates, facts, vars, \
                 `ignore:` patterns, the `vars.` reads in an untrusted remote's `when:`, and each \
                 version 2 source, selector and relation, counted for each source an entry loads, \
                 again for what each entry or drop-in brings in to a merge, and again for each \
                 rule or template a merge combines with one of its id, but not one an earlier \
                 entry naming the same source, with the same filters, wrote last); list each \
                 source once, remove repeated rule and template ids within a source, extend \
                 fewer sources, use fewer drop-ins, or flatten the `extends:` chain"
            )));
        }
        Ok(())
    }

    pub(crate) fn remember(&mut self, key: MemoKey, config: &RawConfig) {
        match self.memo.entry(key) {
            std::collections::hash_map::Entry::Vacant(seen_once) => {
                seen_once.insert(None);
            }
            std::collections::hash_map::Entry::Occupied(mut seen_twice) => {
                *seen_twice.get_mut() = Some(config.clone());
            }
        }
    }

    pub(crate) fn cached(&self, key: Option<&MemoKey>) -> Option<RawConfig> {
        key.and_then(|k| self.memo.get(k)).and_then(Clone::clone)
    }

    /// The remote body pinned by the hash `sri`, as `parse` reads it the
    /// first time.
    pub(crate) fn remote_source(
        &mut self,
        sri: &str,
        parse: impl FnOnce(&mut Self) -> Result<RawConfig>,
    ) -> Result<RawConfig> {
        if let Some(parsed) = self.remote_sources.get(sri) {
            return Ok(parsed.clone());
        }
        let parsed = parse(self)?;
        self.remote_sources.insert(sri.to_owned(), parsed.clone());
        Ok(parsed)
    }

    /// The bundled ruleset `spec`, as `decode` reads it the first time.
    pub(crate) fn bundled_source(
        &mut self,
        spec: &str,
        decode: impl FnOnce() -> Result<RawConfig>,
    ) -> Result<RawConfig> {
        if let Some(decoded) = self.bundled_sources.get(spec) {
            return Ok(decoded.clone());
        }
        let decoded = decode()?;
        self.bundled_sources
            .insert(spec.to_owned(), decoded.clone());
        Ok(decoded)
    }

    /// Warn that the `extends:` source `key` (its resolved file, or how it is
    /// named) sets the top-level settings `declared`, which are ignored, once
    /// per source. `display` names it: as written, without the environment's
    /// values, or `<redacted>` when the environment chose it (`chosen`).
    pub(crate) fn warn_ignored_settings(
        &mut self,
        key: &str,
        display: &str,
        chosen: bool,
        declared: Vec<&'static str>,
    ) {
        let key = if chosen {
            alint_core::CONFIGURATION_MASK
        } else {
            key
        };
        let settings = declared.join(", ");
        if !self.warned_settings.insert((key.to_owned(), declared)) {
            return;
        }
        let name = if chosen {
            std::borrow::Cow::Borrowed(alint_core::CONFIGURATION_MASK)
        } else {
            self.redact(display)
        };
        tracing::warn!(
            "extended config {name} sets {settings}; top-level settings are only read from \
             your own config and its `.alint.d/` drop-ins, so the value is ignored"
        );
    }

    /// `text` without the environment's values recorded so far.
    pub(crate) fn redact<'t>(&self, text: &'t str) -> std::borrow::Cow<'t, str> {
        #[cfg(test)]
        REDACTIONS.with(|redactions| redactions.set(redactions.get() + 1));
        alint_core::redact_environment_values(text, self.environment_values.as_slice())
    }
}

#[cfg(test)]
thread_local! {
    /// How many texts [`LoadState::redact`] has scrubbed on this thread.
    pub(crate) static REDACTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many bytes the memo's keys have hashed on this thread.
    pub(crate) static MEMO_HASHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The memo's hasher: the standard library's, which in the tests also counts
/// the bytes it hashes ([`MEMO_HASHED`]).
#[derive(Default, Clone)]
struct MemoHasher(std::collections::hash_map::RandomState);

impl std::hash::BuildHasher for MemoHasher {
    type Hasher = MemoHashing;

    fn build_hasher(&self) -> MemoHashing {
        MemoHashing(self.0.build_hasher())
    }
}

struct MemoHashing(std::collections::hash_map::DefaultHasher);

impl std::hash::Hasher for MemoHashing {
    fn finish(&self) -> u64 {
        self.0.finish()
    }

    fn write(&mut self, bytes: &[u8]) {
        #[cfg(test)]
        MEMO_HASHED.with(|hashed| hashed.set(hashed.get() + bytes.len()));
        self.0.write(bytes);
    }
}
