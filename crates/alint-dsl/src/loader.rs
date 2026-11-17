//! `extends:` resolution — recursive loader for the YAML
//! composition chain. Pulled out of `lib.rs` to keep that file
//! focused on the public surface (discover / load / parse) and
//! the typed config shape.

use std::path::{Path, PathBuf};

use alint_core::{
    AllowOutOfRoot, ConfigCompatibility, ConfigSourceClass, Error, Result, RunningVersion,
};
use ayaml::Mapping;

use crate::extends;
use crate::extends::EntryKind as ExtendsEntryKind;
use crate::{
    LoadOptions, RawConfig, apply_rule_filter, merge, reject_allow_out_of_root_in,
    reject_baseline_in, reject_command_rules_in, reject_spawning_templates_in,
};

/// Maximum depth of an `extends:` chain — a recursion-stack guard against a
/// hostile deeply-nested (acyclic) chain. Generous: real compositions are a
/// handful deep. See [`load_recursive`] (L5).
const MAX_EXTENDS_DEPTH: usize = 64;

/// Mutable state shared by one `load_with` call's whole extends resolution.
pub(crate) struct LoadState {
    /// Ancestors on the current DFS path (cycle detection + depth bound).
    pub(crate) visiting: std::collections::HashSet<PathBuf>,
    /// Completed non-top-level local loads, reused across a diamond chain: two
    /// entries that both extend the same file at every level would otherwise
    /// reload it once per path through the DAG (2^depth loads, an effective
    /// hang from a hostile repo). `is_top` loads read their own trust and
    /// confinement settings and are never memoized.
    /// `None` marks a key seen once: only a key loaded a SECOND time (a real
    /// diamond) is cloned into the memo, so a linear chain never pays for it while
    /// a diamond still loads each shared file at most twice.
    memo: std::collections::HashMap<MemoKey, Option<RawConfig>>,
    running_version: RunningVersion,
    /// Every [`RawConfig::environment_values`] parsed so far, so an error the
    /// resolution returns before the configs merge (an unreadable `extends:`
    /// target named by the environment) can still be scrubbed of them.
    pub(crate) environment_values: Vec<String>,
    /// Every local file loaded so far, resolved: the top-level config, its
    /// drop-ins and each local `extends:` target, once each.
    pub(crate) config_files: Vec<PathBuf>,
}

impl Default for LoadState {
    fn default() -> Self {
        Self {
            visiting: std::collections::HashSet::new(),
            memo: std::collections::HashMap::new(),
            running_version: RunningVersion::current(),
            environment_values: Vec::new(),
            config_files: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceContext {
    source_class: ConfigSourceClass,
    consumer_version: Option<u32>,
}

impl SourceContext {
    pub(crate) const fn new(
        source_class: ConfigSourceClass,
        consumer_version: Option<u32>,
    ) -> Self {
        Self {
            source_class,
            consumer_version,
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
struct MemoKey {
    path: PathBuf,
    confine: Option<PathBuf>,
    trusted: Vec<String>,
    consumer_version: Option<u32>,
    source_class: ConfigSourceClass,
}

impl LoadState {
    /// Note `canonical` among the files the configuration came from.
    fn record_config_file(&mut self, canonical: &Path) {
        if !self.config_files.iter().any(|file| file == canonical) {
            self.config_files.push(canonical.to_path_buf());
        }
    }

    fn remember(&mut self, key: MemoKey, config: &RawConfig) {
        match self.memo.entry(key) {
            std::collections::hash_map::Entry::Vacant(seen_once) => {
                seen_once.insert(None);
            }
            std::collections::hash_map::Entry::Occupied(mut seen_twice) => {
                *seen_twice.get_mut() = Some(config.clone());
            }
        }
    }

    fn cached(&self, key: Option<&MemoKey>) -> Option<RawConfig> {
        key.and_then(|k| self.memo.get(k)).and_then(Clone::clone)
    }
}

impl MemoKey {
    fn new(
        path: &Path,
        confine: Option<&Path>,
        trusted: &[String],
        consumer_version: Option<u32>,
        source_class: ConfigSourceClass,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            confine: confine.map(Path::to_path_buf),
            trusted: trusted.to_vec(),
            consumer_version,
            source_class,
        }
    }
}

/// Parse a local config file's `contents` into a [`RawConfig`],
/// resolving `{{env.X}}` interpolation first. Shared by
/// `load_recursive` and nested-config loading so every local config
/// file (top-level, `.alint.d/` drop-ins, local `extends:` targets,
/// and nested configs) gets identical interpolation treatment —
/// bundled and remote `extends:` content is handled elsewhere and
/// deliberately NOT interpolated against the consumer's environment.
///
/// Gated on the presence of any `{{` marker: a config with no
/// interpolation parses straight into `RawConfig`, which keeps the
/// line/column-aware serde error messages (the `Value` round-trip
/// loses span info) AND skips a redundant second parse. An interp
/// failure is reported with the `source` path; a typed/YAML error
/// propagates bare so the existing diagnostics are unchanged.
pub(crate) fn parse_config_interpolated_with_environment(
    contents: &str,
    source: &Path,
    environment: &dyn crate::EnvironmentProvider,
) -> Result<RawConfig> {
    let contents = crate::config_text(contents, &display_path(source))?;
    // Name the file in a YAML/typed error: in an `extends:` chain or a
    // `.alint.d/` drop-in, a bare "at line 2 column 1" reads as if the
    // top-level config were at fault.
    parse_config_interpolated_inner_with_environment(contents, source, environment).map_err(|e| {
        match e {
            Error::Yaml(e) => {
                Error::Other(format!("{}: YAML parse error: {e}", display_path(source)))
            }
            other => other,
        }
    })
}

/// The error for a config at `path` that could not be read: one past the size
/// cap is a config error, in the words [`crate::config_text`] uses.
pub(crate) fn config_read_error(path: &Path, source: std::io::Error) -> Error {
    if source.kind() == std::io::ErrorKind::FileTooLarge {
        return Error::Other(format!("{}: {source}", display_path(path)));
    }
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `path` for a user-facing message, without the `\\?\` verbatim prefix that
/// `canonicalize` adds on Windows (`\\?\UNC\server\share` becomes
/// `\\server\share`). The prefix never occurs on Unix, so this is a no-op there.
fn display_path(path: &Path) -> String {
    let shown = path.display().to_string();
    if let Some(unc) = shown.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = shown.strip_prefix(r"\\?\") {
        local.to_owned()
    } else {
        shown
    }
}

pub(crate) fn validate_source_compatibility(
    config: &mut RawConfig,
    source: &str,
    source_class: ConfigSourceClass,
    consumer_version: Option<u32>,
    running_version: RunningVersion,
) -> Result<()> {
    let version = match config.version {
        Some(version) => version,
        None if source_class == ConfigSourceClass::DropIn => consumer_version.ok_or_else(|| {
            Error::internal("drop-in compatibility validation has no root consumer version")
        })?,
        None => {
            return Err(Error::Other(format!(
                "{source}: missing required config `version` for {source_class:?} source"
            )));
        }
    };
    if source_class == ConfigSourceClass::DropIn
        && let (Some(declared), Some(consumer)) = (config.version, consumer_version)
        && declared != consumer
    {
        return Err(Error::Other(format!(
            "{source}: drop-in config version {declared} must equal root config version {consumer}"
        )));
    }
    if !alint_core::Config::supports_version(version) {
        return Err(Error::Other(format!(
            "{source}: unsupported config version {version} (this build supports {} through {})",
            alint_core::Config::MIN_SUPPORTED_VERSION,
            alint_core::Config::CURRENT_VERSION
        )));
    }
    if let Some(consumer) = consumer_version
        && !source_version_allowed(consumer, version)
    {
        return Err(Error::Other(format!(
            "{source}: config version {consumer} cannot consume config version {version}"
        )));
    }
    if let Some(requirements) = config.requires
        && !running_version.satisfies(requirements.alint)
    {
        return Err(Error::Other(format!(
            "{source}: requires alint {} or newer (running {running_version}; config version {version})",
            requirements.alint
        )));
    }
    config.version = Some(version);
    config.compatibility.push(ConfigCompatibility {
        source: source.to_string(),
        source_class,
        config_version: version,
        requires_alint: config.requires.map(|requirements| requirements.alint),
        consumer_version,
    });
    Ok(())
}

pub(crate) fn source_version_allowed(consumer: u32, source: u32) -> bool {
    source <= consumer
}

fn local_source_display(path: &Path, confine: Option<&Path>) -> String {
    confine
        .and_then(|root| path.strip_prefix(root).ok())
        .filter(|relative| !relative.as_os_str().is_empty())
        .map_or_else(
            || {
                path.file_name().map_or_else(
                    || display_path(path),
                    |name| name.to_string_lossy().into_owned(),
                )
            },
            display_path,
        )
}

fn remote_source_display(entry: &str) -> String {
    let without_fragment = entry.split('#').next().unwrap_or(entry);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let Some((scheme, rest)) = without_query.split_once("://") else {
        return "<remote>".to_string();
    };
    let (authority, suffix) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if suffix.is_empty() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/{suffix}")
    }
}

/// After `contents` failed to decode: the error a non-literal `requires:`
/// earns, which names the cause better than the decode error does. That takes
/// a second parse, so a text that does not spell `requires` (and so has no such
/// key, unless one is spelled with escapes, which then gets the decode error:
/// an error all the same) skips it.
pub(crate) fn nonliteral_requirement_after_error(contents: &str, source: &str) -> Option<Error> {
    if !contents.contains("requires") {
        return None;
    }
    let value: ayaml::Value = alint_core::yaml::from_str(contents).ok()?;
    reject_nonliteral_requirement_in_value(contents, source, &value)
        .err()
        .filter(|error| matches!(error, Error::Other(_)))
}

fn reject_nonliteral_requirement_in_value(
    contents: &str,
    source: &str,
    value: &ayaml::Value,
) -> Result<()> {
    let Some(requirements) = value.get("requires") else {
        return Ok(());
    };
    let requirement = requirements.get("alint");
    let interpolated = requirement
        .and_then(ayaml::Value::as_str)
        .is_some_and(|value| value.contains("{{") || value.contains("${"));
    if interpolated || yaml_value_contains_tag(requirements) {
        return Err(nonliteral_requirement_error(source));
    }
    reject_nonliteral_decoded_requirement(contents, source)
}

pub(crate) fn reject_nonliteral_decoded_requirement(contents: &str, source: &str) -> Result<()> {
    if !alint_core::yaml_depth::mapping_subtree_is_literal(contents, &["requires"])
        .map_err(|error| Error::Other(format!("{source}: YAML parse error: {error}")))?
    {
        return Err(nonliteral_requirement_error(source));
    }
    Ok(())
}

fn nonliteral_requirement_error(source: &str) -> Error {
    Error::Other(format!(
        "{source}: `requires.alint` must be a direct literal stable version without interpolation, aliases, anchors, tags, or merge keys"
    ))
}

fn yaml_value_contains_tag(value: &ayaml::Value) -> bool {
    match value {
        ayaml::Value::Tagged(_) => true,
        ayaml::Value::Sequence(values) => values.iter().any(yaml_value_contains_tag),
        ayaml::Value::Mapping(values) => values
            .iter()
            .any(|(key, value)| yaml_value_contains_tag(key) || yaml_value_contains_tag(value)),
        _ => false,
    }
}

fn deserialize_raw_config(contents: &str, source: &str) -> Result<RawConfig> {
    match alint_core::yaml::from_str(contents) {
        Ok(config) => {
            let config: RawConfig = config;
            if config.requires.is_some() {
                reject_nonliteral_decoded_requirement(contents, source)?;
            }
            Ok(config)
        }
        Err(parse_error) => Err(nonliteral_requirement_after_error(contents, source)
            .unwrap_or(Error::Yaml(parse_error))),
    }
}

#[cfg(test)]
fn parse_config_interpolated_inner(contents: &str, source: &Path) -> Result<RawConfig> {
    parse_config_interpolated_inner_with_environment(contents, source, &crate::HostEnvironment)
}

fn parse_config_interpolated_inner_with_environment(
    contents: &str,
    source: &Path,
    environment: &dyn crate::EnvironmentProvider,
) -> Result<RawConfig> {
    fn redact_interpolated_error(source: &str) -> Error {
        Error::Other(format!(
            "{source}: could not deserialize interpolated config: <redacted> (the error \
             could quote a value read from the environment)"
        ))
    }

    // Reject a deeply-nested-flow config before any parser reads it (the parser
    // v0.22 used hung on one) — a DoS reachable through an `extends:`'d ruleset.
    let yaml_guards = alint_core::yaml_depth::scan_guards(contents);
    if !yaml_guards.flow_within_limit() {
        return Err(Error::Other(format!(
            "{}: YAML flow nesting exceeds the maximum supported depth ({})",
            source.display(),
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    // Reject an alias-expansion bomb (a single anchor referenced many times) the
    // same way. The guard was written for the parser v0.22 used, whose limits
    // missed it; ayaml charges an alias budget of its own, which the guard's
    // is stricter than, and the guard stays as defense in depth (`yaml_depth`).
    if !yaml_guards.expansion_within_limit(contents) {
        return Err(Error::Other(format!(
            "{}: YAML alias expansion exceeds the maximum supported size",
            source.display(),
        )));
    }
    let source = display_path(source);
    let config: RawConfig = if contents.contains("{{") {
        let mut value: ayaml::Value = alint_core::yaml::from_str(contents)?;
        let has_sensitive_interpolation = value_reads_environment(&value);
        reject_nonliteral_requirement_in_value(contents, &source, &value)?;
        // Which vars take their value from the environment, read off the RAW
        // text before interpolation erases the difference.
        let env_vars = env_derived_vars(value.get("vars"));
        let rule_sensitive_fields = environment_sensitive_fields(value.get("rules"));
        let template_sensitive_fields = environment_sensitive_fields(value.get("templates"));
        let extends_environment = entries_reading_environment(value.get("extends"));
        let rule_env_vars = rule_env_derived_vars(value.get("rules"));
        // The names the top-level fields other than rules, templates and vars
        // read (`extends:`, `ignore:`, `facts:`, `baseline:` and the rest). The
        // code that consumes those fields can quote a value in an error, and no
        // rule-level redaction covers it, so their values are recorded below
        // for the embedder to remove from any error it shows.
        let top_level_names = top_level_environment_names(&value);
        // The same document with every environment value masked: an error it
        // reproduces exactly cannot quote one of those values.
        let masked = has_sensitive_interpolation.then(|| value.clone());
        let read = std::cell::RefCell::new(std::collections::HashMap::new());
        crate::interp::interpolate_value(&mut value, &|name| {
            let found = environment.get(name);
            if let Some(found) = &found {
                read.borrow_mut().insert(name.to_string(), found.clone());
            }
            found
        })
        .map_err(|error| Error::Other(format!("{source}: interpolation error: {error}")))?;
        let read = read.into_inner();
        let mut environment_values = Vec::new();
        for name in &top_level_names {
            if let Some(value) = read.get(name) {
                add_environment_values(&mut environment_values, std::slice::from_ref(value));
            }
        }
        let mut config: RawConfig = crate::core_yaml::decode(value).map_err(|error| {
            // Each masked value keeps the type interpolation gave the real one
            // (a 12-digit id is a number), so an error in an unrelated field
            // after it still recurs; numbers move both ways, as a rule's
            // rerun does, for a value at the edge of its range.
            let reproduced = masked.is_none_or(|masked| {
                alint_core::NumberMask::BOTH.into_iter().any(|numbers| {
                    let mut masked = masked.clone();
                    crate::interp::interpolate_masked(
                        &mut masked,
                        &|name| environment.get(name),
                        numbers,
                    )
                    .is_ok()
                        && crate::core_yaml::decode::<RawConfig>(masked)
                            .err()
                            .is_some_and(|masked_error| {
                                masked_error.to_string() == error.to_string()
                            })
                })
            });
            if reproduced {
                error.into()
            } else {
                redact_interpolated_error(&source)
            }
        })?;
        // Reject user-authored private markers before adding the loader's own
        // env-provenance marker below.
        crate::reject_ambiguous_yaml_in(&config, &source)?;
        config.env_vars = env_vars;
        config.environment_values = environment_values;
        config.extends_environment = extends_environment;
        for (rule, marks) in config.rules.iter_mut().zip(rule_env_vars) {
            if !marks.is_empty() {
                rule.insert(crate::ENV_VARS_MARKER.into(), marks.into());
            }
        }
        for (rule, marks) in config.rules.iter_mut().zip(rule_sensitive_fields) {
            if !marks.is_empty() {
                rule.insert(crate::SENSITIVE_FIELDS_MARKER.into(), marks.into());
            }
        }
        for (template, marks) in config.templates.iter_mut().zip(template_sensitive_fields) {
            if !marks.is_empty() {
                template.insert(crate::SENSITIVE_FIELDS_MARKER.into(), marks.into());
            }
        }
        config
    } else {
        let config = deserialize_raw_config(contents, &source)?;
        crate::reject_ambiguous_yaml_in(&config, &source)?;
        config
    };
    Ok(config)
}

/// The entries of a raw (pre-interpolation) `vars:` mapping whose value reads
/// the environment (`{{env.X}}`, any spacing, with or without a default),
/// mapped to that raw text. Only these become secret: a `{{vars.X}}` span in a
/// var value is never resolved against other vars, so it cannot launder one.
fn env_derived_vars(vars: Option<&ayaml::Value>) -> std::collections::HashMap<String, String> {
    vars.and_then(ayaml::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let (name, raw) = (k.as_str()?, v.as_str()?);
            crate::interp::reads_env(raw).then(|| (name.to_string(), raw.to_string()))
        })
        .collect()
}
fn value_reads_environment(value: &ayaml::Value) -> bool {
    match value {
        ayaml::Value::String(text) => crate::interp::reads_env(text),
        ayaml::Value::Sequence(values) => values.iter().any(value_reads_environment),
        ayaml::Value::Mapping(mapping) => mapping.values().any(value_reads_environment),
        _ => false,
    }
}

/// Whether the URL of each entry of a raw (pre-interpolation) `extends:` reads
/// the environment, so the loader knows which sources came from it: the whole
/// entry in the string form, its `url:` in the mapping form. An `only:` or
/// `except:` read from the environment leaves the URL as written; those values
/// are recorded with the other top-level ones.
fn entries_reading_environment(entries: Option<&ayaml::Value>) -> Vec<bool> {
    entries
        .and_then(ayaml::Value::as_sequence)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| value_reads_environment(entry.get("url").unwrap_or(entry)))
                .collect()
        })
        .unwrap_or_default()
}

/// Each raw rule's [`env_derived_vars`], as the mapping its marker holds.
fn rule_env_derived_vars(rules: Option<&ayaml::Value>) -> Vec<Mapping> {
    rules
        .and_then(ayaml::Value::as_sequence)
        .map(|rules| {
            rules
                .iter()
                .map(|r| {
                    env_derived_vars(r.get("vars"))
                        .into_iter()
                        .map(|(k, v)| (k.into(), v.into()))
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Append each of `values` that `into` does not hold yet.
pub(crate) fn add_environment_values(into: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !into.contains(value) {
            into.push(value.clone());
        }
    }
}

/// The environment names read by the top-level fields of a raw
/// (pre-interpolation) document, other than `rules:`, `templates:` and `vars:`.
fn top_level_environment_names(document: &ayaml::Value) -> Vec<String> {
    fn collect(value: &ayaml::Value, names: &mut Vec<String>) {
        match value {
            ayaml::Value::String(text) => names.extend(crate::interp::env_names(text)),
            ayaml::Value::Sequence(values) => {
                for value in values {
                    collect(value, names);
                }
            }
            ayaml::Value::Mapping(mapping) => {
                for value in mapping.values() {
                    collect(value, names);
                }
            }
            // Interpolation leaves a tagged value as written, so it reads no name.
            _ => {}
        }
    }

    let mut names = Vec::new();
    for (key, value) in document.as_mapping().into_iter().flatten() {
        if !matches!(key.as_str(), Some("rules" | "templates" | "vars")) {
            collect(value, &mut names);
        }
    }
    names
}

/// Sparse top-level fields containing at least one environment interpolation.
/// The scan runs on raw YAML because interpolation erases that provenance.
fn environment_sensitive_fields(entries: Option<&ayaml::Value>) -> Vec<Mapping> {
    fn reads_environment(value: &ayaml::Value) -> bool {
        value_reads_environment(value)
    }

    entries
        .and_then(ayaml::Value::as_sequence)
        .into_iter()
        .flatten()
        .map(|entry| {
            entry
                .as_mapping()
                .into_iter()
                .flat_map(ayaml::Mapping::iter)
                .filter(|(field, value)| {
                    !matches!(field.as_str(), Some("id" | "kind" | "level" | "vars"))
                        && reads_environment(value)
                })
                .map(|(field, _)| (field.clone(), true.into()))
                .collect()
        })
        .collect()
}

/// Recursively load `path`, resolving its `extends:` chain
/// left-to-right. Later entries in the chain override earlier
/// ones; the current file's own definitions override everything
/// it extends. Rules are field-merged at the YAML-Mapping layer
/// so children can override individual fields without re-stating
/// the entire rule.
pub(crate) fn load_recursive(
    path: &Path,
    state: &mut LoadState,
    opts: &LoadOptions<'_>,
    confine: Option<&Path>,
    is_top: bool,
    // The top-level `trusted_extends:` allowlist (remote URLs whose content fixers
    // stay auto-applying), threaded down the LOCAL nesting so a remote extended at
    // any depth is checked against the user's list. A non-top-level config may not
    // grant trust, so nested calls receive the top's list; `is_top` calls read
    // their OWN `trusted_extends:` and ignore this argument. (W2, auto-fix.md 5.5.)
    trusted: &[String],
    context: SourceContext,
) -> Result<RawConfig> {
    let canonical = opts
        .config_source
        .canonicalize(path)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    // Diamond chains reload a shared file once per DAG path (2^depth); a
    // non-top-level load is a pure function of its `MemoKey`, so reuse it.
    let memo_key = (!is_top).then(|| {
        MemoKey::new(
            &canonical,
            confine,
            trusted,
            context.consumer_version,
            context.source_class,
        )
    });
    state.record_config_file(&canonical);
    if let Some(cached) = state.cached(memo_key.as_ref()) {
        return Ok(cached);
    }
    let visiting = &mut state.visiting;
    if !visiting.insert(canonical.clone()) {
        return Err(Error::Other(format!(
            "cycle in `extends` chain at {}",
            display_path(&canonical)
        )));
    }
    // Bound the depth of an *acyclic* chain (the cycle guard above only catches
    // repeats): a hostile repo could otherwise nest thousands of local configs
    // each extending the next and overflow the recursion stack (L5). `visiting`
    // holds exactly the ancestors on the current DFS path (balanced insert /
    // remove), so its length is the current depth. The cap is far above any
    // real composition (root → team → org → bundled is ~4).
    if visiting.len() > MAX_EXTENDS_DEPTH {
        return Err(Error::Other(format!(
            "`extends:` chain exceeds the maximum depth of {MAX_EXTENDS_DEPTH} (at {}); \
             flatten the chain or split the ruleset",
            display_path(&canonical),
        )));
    }

    let contents = opts
        .config_source
        .read_to_string(&canonical)
        .map_err(|source| config_read_error(&canonical, source))?;
    let mut config = parse_config_interpolated_with_environment(
        &contents,
        &canonical,
        opts.environment.as_ref(),
    )?;
    add_environment_values(&mut state.environment_values, &config.environment_values);
    validate_source_compatibility(
        &mut config,
        &local_source_display(&canonical, confine),
        context.source_class,
        context.consumer_version,
        state.running_version,
    )?;

    // W2 (auto-fix.md 5.5): `trusted_extends:` is a top-level authority. An is_top
    // config (the user's own `.alint.yml` or a `.alint.d/` drop-in) owns the
    // allowlist and uses it for its own extends chain; a non-top-level config's
    // list is rejected per-source below, so nested loads use the list threaded from
    // the top. Taken (consumed) at is_top -- it gates the demotion here and is not
    // carried onto `Config`.
    let top_trusted: Vec<String>;
    let trusted: &[String] = if is_top {
        top_trusted = std::mem::take(&mut config.trusted_extends);
        &top_trusted
    } else {
        trusted
    };

    let extends = std::mem::take(&mut config.extends);
    let extends_environment = std::mem::take(&mut config.extends_environment);
    if extends.is_empty() {
        visiting.remove(&canonical);
        if let Some(key) = memo_key {
            state.remember(key, &config);
        }
        return Ok(config);
    }

    let source_dir = canonical
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    // Local `extends:` targets stay within the lint tree (the confinement
    // boundary the loader was handed — the top-level config's directory),
    // so a shared ruleset committed to the repo cannot smuggle in a local
    // `extends: ../../../../etc/shadow` to read arbitrary files off the
    // host. The top-level `allow_out_of_root: true` lifts it for the whole
    // chain — the same blanket escape that lifts per-rule read confinement.
    // A `Selective` allowlist names rule kinds/ids and has no meaning for an
    // extends *path*, so only the `All` form opens this gate. Only the USER'S
    // TOP-LEVEL config may open it: a sub-config's flag is rejected by
    // `reject_allow_out_of_root_in` at the parent's loop below, but that fires
    // AFTER this sub-config has already resolved ITS OWN `extends:` — so gating
    // on `is_top` here is what actually prevents an extended ruleset from
    // lifting confinement and reading an out-of-root `extends:` target (a FIFO
    // hangs, a big file is slurped, host paths become an existence oracle)
    // before the rejection can fire. Without `is_top` the reject is one level
    // too late.
    let confine = match (&config.allow_out_of_root, is_top) {
        (AllowOutOfRoot::All, true) => None,
        _ => confine,
    };

    let mut merged = RawConfig {
        version: config.version,
        ..RawConfig::default()
    };
    for (index, entry) in extends.iter().enumerate() {
        let parent = load_extended_source(
            entry,
            extends_environment.get(index).copied().unwrap_or(false),
            &source_dir,
            state,
            opts,
            confine,
            trusted,
            config.version,
        )?;
        merged = merge(merged, parent)?;
    }
    merged = merge(merged, config)?;
    state.visiting.remove(&canonical);
    if let Some(key) = memo_key {
        state.remember(key, &merged);
    }
    Ok(merged)
}

/// `from_environment`: the entry read the environment, so it is also removed
/// from errors in the forms the loader prints it in, which need not contain the
/// value as written (a remote URL without its credentials, query and fragment;
/// a local path joined, canonical, or relative to the root).
#[allow(clippy::too_many_arguments)]
fn load_extended_source(
    entry: &alint_core::ExtendsEntry,
    from_environment: bool,
    source_dir: &Path,
    state: &mut LoadState,
    opts: &LoadOptions<'_>,
    confine: Option<&Path>,
    trusted: &[String],
    consumer_version: Option<u32>,
) -> Result<RawConfig> {
    let url = entry.url();
    let kind = ExtendsEntryKind::of(url);
    let source_display = if kind.is_url() {
        remote_source_display(url)
    } else {
        url.to_string()
    };
    if from_environment {
        add_environment_values(
            &mut state.environment_values,
            std::slice::from_ref(&source_display),
        );
    }
    let mut parent = if kind == ExtendsEntryKind::PlainHttp {
        return Err(Error::Other(format!(
            "plain http:// is not allowed in `extends:` (entry {source_display:?}); \
             use https:// with an SRI hash instead"
        )));
    } else if let ExtendsEntryKind::Unsupported(scheme) = kind {
        return Err(Error::Other(format!(
            "unsupported `extends:` scheme `{}` (entry {source_display:?}); use https://, \
             alint://bundled/ or a local path",
            scheme.to_ascii_lowercase()
        )));
    } else if kind == ExtendsEntryKind::Https {
        if opts.remote_policy == crate::RemotePolicy::Deny {
            return Err(Error::Other(format!(
                "remote `extends` is disabled for this load (entry {source_display:?})"
            )));
        }
        let mut remote = load_remote(url, opts, &mut state.visiting)?;
        validate_source_compatibility(
            &mut remote,
            &source_display,
            ConfigSourceClass::Remote,
            consumer_version,
            state.running_version,
        )?;
        if !remote.extends.is_empty() {
            return Err(Error::Other(format!(
                "remote config at {source_display} contains its own `extends:`; \
                 nested remote extends are not supported in this build"
            )));
        }
        crate::reject_env_expansion_in(&remote.rules, &remote.templates, &source_display)?;
        remote
    } else if let ExtendsEntryKind::Bundled(spec) = kind {
        let mut bundled = load_bundled(spec)?;
        validate_source_compatibility(
            &mut bundled,
            &format!("alint://bundled/{spec}"),
            ConfigSourceClass::Bundled,
            consumer_version,
            state.running_version,
        )?;
        if !bundled.extends.is_empty() {
            return Err(Error::internal(format!(
                "bundled ruleset '{spec}' declares its own `extends:`"
            )));
        }
        bundled
    } else {
        let target = resolve_relative(source_dir, url);
        if from_environment {
            let mut normalized = vec![display_path(&target)];
            if let Ok(canonical) = opts.config_source.canonicalize(&target) {
                normalized.push(local_source_display(&canonical, confine));
                normalized.push(display_path(&canonical));
            }
            add_environment_values(&mut state.environment_values, &normalized);
        }
        confine_extends_target(&target, url, confine, opts.config_source.as_ref())?;
        load_recursive(
            &target,
            state,
            opts,
            confine,
            false,
            trusted,
            SourceContext::new(ConfigSourceClass::LocalExtend, consumer_version),
        )?
    };
    gate_extended_source(&parent, &source_display, opts.kind_catalog)?;
    mark_extended_provenance(
        &mut parent,
        &source_display,
        kind == ExtendsEntryKind::Https,
    );
    parent.drop_top_level_settings(&alint_core::redact_environment_values(
        &source_display,
        &state.environment_values,
    ));
    parent.rules = apply_rule_filter(parent.rules, entry, &source_display)?;
    if kind == ExtendsEntryKind::Https {
        let base = url.split('#').next().unwrap_or(url);
        if !trusted.iter().any(|entry| entry == base || entry == url) {
            cap_untrusted_remote(&mut parent, &source_display, opts.kind_catalog)?;
        }
    }
    Ok(parent)
}

/// Record that the `extends:` source `url` (fetched when `remote`) shaped
/// `parent`'s rules / templates, so `finalize` can judge the EFFECTIVE rule
/// after the id-based field-merge has blurred where each field came from (e.g.
/// refuse any contribution to a spawning rule, or a remote-assembled `${` env
/// reference).
fn mark_extended_provenance(parent: &mut RawConfig, url: &str, remote: bool) {
    let classes: &[crate::SourceClass] = if remote {
        &[crate::SourceClass::Extended, crate::SourceClass::Remote]
    } else {
        &[crate::SourceClass::Extended]
    };
    crate::trust::mark_extended_source_in(&mut parent.rules, url, classes);
    crate::trust::mark_extended_source_in(&mut parent.templates, url, classes);
}

/// Apply the restrictions on a remote `extends:` the user has NOT listed in
/// `trusted_extends:` (the URL matches with or without its `#sha256-` fragment).
fn cap_untrusted_remote(
    parent: &mut RawConfig,
    url: &str,
    catalog: Option<alint_core::KindCatalog<'_>>,
) -> Result<()> {
    // It may not read the consumer's environment through a `when:` (a boolean
    // oracle on each variable), including one an instance it shapes assembles
    // from a template at finalize (hence the provenance mark).
    // A `vars.*` read is recorded here and refused at finalize when the var's
    // effective value came from the environment (only known once every local
    // config has merged).
    parent.untrusted_when_var_reads =
        crate::reject_env_reads_in_when(&parent.rules, &parent.templates, url)?;
    crate::mark_provenance_in(&mut parent.rules, crate::SourceClass::UntrustedRemote, url);
    // ...and a template it shapes may not splice an env-derived instance var
    // (`{{vars.token}}`) into the rule a user's instance expands it into.
    crate::mark_provenance_in(
        &mut parent.templates,
        crate::SourceClass::UntrustedRemote,
        url,
    );
    // It may not hide files from every rule (yours included) with `ignore:`.
    crate::reject_untrusted_ignore_in(&parent.ignore, url)?;
    // W2 content-fixer trust (auto-fix.md 5.5): an untrusted remote may PROPOSE a
    // content edit but never auto-write one -- demote its content-injecting fixers
    // to `suggestion` before the merge. Local / nested targets (the user's own
    // tree) and bundled (first-party) sources are honored at their declared tier.
    // A remote is a leaf (no nested `extends:`), so this caps exactly that
    // source's own rules.
    //
    // BOTH `rules` AND `templates` are demoted: a template's `fix:` block is
    // spliced into its referencing rule at `finalize` (after this per-source
    // gate), so a remote content fixer smuggled through a `templates:` entry
    // would otherwise escape the cap (the template analogue of
    // `reject_fix_promotion_templates_in`).
    //
    // The raw mappings also receive a monotonic provenance marker. It survives
    // id-based field merges and template expansion, then `finalize` demotes the
    // EFFECTIVE fixer. That closes both mixed-source directions: an untrusted
    // rule instantiating a trusted template, and a trusted rule instantiating a
    // template partly defined by an untrusted source.
    crate::demote_content_fixers_in(&mut parent.rules, catalog);
    crate::demote_content_fixers_in(&mut parent.templates, catalog);
    crate::mark_untrusted_fix_sources_in(&mut parent.rules);
    crate::mark_untrusted_fix_sources_in(&mut parent.templates);
    Ok(())
}

fn sanitized_remote_resolution_error(source: &str, error: &extends::ResolveError) -> Error {
    let detail = match error {
        extends::ResolveError::Sri(_) => "integrity verification failed".to_string(),
        extends::ResolveError::Fetch(extends::FetchError::Request { .. }) => {
            "HTTPS request failed".to_string()
        }
        extends::ResolveError::Fetch(extends::FetchError::Status { status, .. }) => {
            format!("server returned HTTP {status}")
        }
        extends::ResolveError::Fetch(extends::FetchError::TooLarge { limit, .. }) => {
            format!("response body exceeds the {limit}-byte ceiling")
        }
        extends::ResolveError::Fetch(extends::FetchError::Io { source, .. }) => {
            format!("response body read failed: {source}")
        }
        extends::ResolveError::Cache(extends::CacheError::NoCacheDir) => {
            "could not resolve a user cache directory".to_string()
        }
        extends::ResolveError::Cache(extends::CacheError::Io { source, .. }) => {
            format!("cache I/O failed: {source}")
        }
        extends::ResolveError::Cache(extends::CacheError::CorruptEntry { .. }) => {
            "cached remote ruleset failed integrity verification".to_string()
        }
    };
    Error::Other(format!("resolving {source}: {detail}"))
}

fn load_remote(
    entry: &str,
    opts: &LoadOptions<'_>,
    visiting: &mut std::collections::HashSet<PathBuf>,
) -> Result<RawConfig> {
    let source = remote_source_display(entry);
    let (url, sri) = extends::split_url_and_sri(entry).map_err(|_| {
        Error::Other(format!(
            "remote `extends` entry {source:?} has invalid integrity metadata; \
             expected `#sha256-<64 lowercase hex characters>`"
        ))
    })?;
    let Some(sri) = sri else {
        return Err(Error::Other(format!(
            "remote `extends` entry {source:?} has no integrity hash; \
             HTTPS extends require `#sha256-<hex>` in the URL fragment"
        )));
    };
    let source = remote_source_display(&url);

    let cache = match opts.cache.clone() {
        Some(c) => c,
        None => extends::Cache::user_default()
            .map_err(|e| Error::Other(format!("could not open cache: {e}")))?,
    };
    let fetcher = opts.fetcher.clone().unwrap_or_default();
    let body = extends::resolve_remote(&url, &sri, &fetcher, &cache)
        .map_err(|error| sanitized_remote_resolution_error(&source, &error))?;

    // Remote entries may themselves extend other things (local
    // paths relative to… what, exactly?). For v0.2 we forbid
    // nested extends in a remote body to dodge that ambiguity.
    // When we lift this restriction, the base for relative
    // resolution needs a deliberate decision.
    let body_str = std::str::from_utf8(&body)
        .map_err(|e| Error::Other(format!("remote body from {source} is not UTF-8: {e}")))?;
    let body_str = crate::config_text(body_str, &format!("remote config at {source}"))?;
    // A remote body is untrusted input (SRI pins WHICH bytes, not that they are
    // benign), so it needs the same guards as a local config (written for the
    // parser v0.22 used, which hung on a deep-flow bomb, and kept as defense in
    // depth). The local/interpolated path guards in `parse_config_interpolated`;
    // this direct `from_str` bypassed it (the guard's docs claim `extends:` bodies
    // are covered -- true for local, and now for remote).
    let yaml_guards = alint_core::yaml_depth::scan_guards(body_str);
    if !yaml_guards.flow_within_limit() {
        return Err(Error::Other(format!(
            "remote config at {source}: YAML flow nesting exceeds the maximum supported depth ({})",
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    if !yaml_guards.expansion_within_limit(body_str) {
        return Err(Error::Other(format!(
            "remote config at {source}: YAML alias expansion exceeds the maximum supported size"
        )));
    }
    let config = deserialize_raw_config(body_str, &source).map_err(|error| match error {
        Error::Yaml(error) => Error::Other(format!(
            "remote config at {source}: YAML parse error: {error}"
        )),
        other => other,
    })?;
    crate::reject_ambiguous_yaml_in(&config, &source)?;
    // Cycle guard token for the URL itself so a self-referencing
    // fetched config can't loop.
    let token = std::path::PathBuf::from(format!("remote://{}", sri.encoded()));
    if !visiting.insert(token.clone()) {
        return Err(Error::Other(format!("cycle on remote extends: {source}")));
    }
    visiting.remove(&token);
    Ok(config)
}

/// Every per-source trust refusal for one `extends:`'d config. Runs before the
/// config merges, so `url` names the offending source in each error.
fn gate_extended_source(
    parent: &RawConfig,
    url: &str,
    catalog: Option<alint_core::KindCatalog<'_>>,
) -> Result<()> {
    // Extended configs cannot introduce `custom:` facts or process-spawning
    // rule kinds. Unknown kinds are spawning for this raw-YAML gate: a plugin
    // catalog omission must deny, never silently bypass the trust boundary.
    alint_core::facts::reject_custom_facts_in(&parent.facts, url)?;
    reject_command_rules_in(&parent.rules, url, catalog)?;
    crate::reject_spawning_fix_ops_in(&parent.rules, url, catalog)?;
    crate::reject_fix_promotion_in(&parent.rules, url)?;
    reject_spawning_templates_in(&parent.templates, url, catalog)?;
    crate::reject_spawning_fix_op_templates_in(&parent.templates, url, catalog)?;
    crate::reject_fix_promotion_templates_in(&parent.templates, url)?;
    reject_allow_out_of_root_in(&parent.allow_out_of_root, url)?;
    reject_baseline_in(&parent.baseline, url)?;
    // A ruleset may not allowlist ITSELF into auto-applying content fixers;
    // only the user's top-level config grants that via `trusted_extends:`.
    crate::reject_trusted_extends_in(&parent.trusted_extends, url)?;
    Ok(())
}

/// Load an `alint://bundled/<name>@<rev>` ruleset from the
/// in-binary registry. Bundled rulesets can't themselves extend
/// anything — they're static, leaf-only fragments.
fn load_bundled(spec: &str) -> Result<RawConfig> {
    let body = crate::bundled::resolve(spec).ok_or_else(|| {
        let shipped: Vec<String> = crate::bundled::catalog()
            .map(|(n, r)| format!("alint://bundled/{n}@{r}"))
            .collect();
        Error::Other(format!(
            "unknown bundled ruleset 'alint://bundled/{spec}'; \
             this build ships: [{}]",
            shipped.join(", "),
        ))
    })?;

    // Bundled rulesets are compiled-in and trusted (byte-identical every build), so
    // these guards are defense-in-depth, not an attack surface -- but keeping the
    // checks uniform with the remote/local paths means no `alint_core::yaml::from_str`
    // in the loader is ever unguarded. A bundled ruleset tripping one is an alint bug.
    let yaml_guards = alint_core::yaml_depth::scan_guards(body);
    if !yaml_guards.flow_within_limit() || !yaml_guards.expansion_within_limit(body) {
        return Err(Error::internal(format!(
            "built-in ruleset '{spec}' exceeds a maximum supported YAML complexity limit"
        )));
    }
    let config =
        deserialize_raw_config(body, &format!("alint://bundled/{spec}")).map_err(|error| {
            match error {
                Error::Yaml(error) => {
                    // A ruleset shipped *inside* the binary failing to parse is an
                    // alint bug, not the user's config — Internal → CLI exit 3 (M11).
                    Error::internal(format!(
                        "built-in ruleset '{spec}' failed to parse: {error}"
                    ))
                }
                other => Error::internal(other.to_string()),
            }
        })?;
    crate::reject_ambiguous_yaml_in(&config, &format!("alint://bundled/{spec}"))
        .map_err(|e| Error::internal(e.to_string()))?;
    Ok(config)
}

/// Reject a local `extends:` target that resolves outside the confinement
/// root. `confine == None` means confinement is disabled — either a
/// programmatic caller that handed the loader no root, or a top-level config
/// that opted out via `allow_out_of_root: true`.
///
/// Both sides are canonicalized, so `..`, `.`, and symlinks are resolved: a
/// symlink that lives inside the tree but points out is caught too. A
/// canonicalize failure (most often a missing target) is deliberately *not*
/// treated as an escape — there is nothing to read, so it is left for
/// [`load_recursive`] to surface with its existing not-found error.
fn confine_extends_target(
    target: &Path,
    entry: &str,
    confine: Option<&Path>,
    source_provider: &dyn crate::ConfigSourceProvider,
) -> Result<()> {
    let Some(root) = confine else { return Ok(()) };
    let (Ok(canon_root), Ok(canon_target)) = (
        source_provider.canonicalize(root),
        source_provider.canonicalize(target),
    ) else {
        return Ok(());
    };
    if !canon_target.starts_with(&canon_root) {
        return Err(Error::Other(format!(
            "`extends:` target {entry:?} resolves to {} which is outside the lint root {}; \
             a local `extends:` chain must stay within the linted tree. Set \
             `allow_out_of_root: true` on the top-level config to override.",
            canon_target.display(),
            canon_root.display(),
        )));
    }
    Ok(())
}

fn resolve_relative(source_dir: &Path, entry: &str) -> PathBuf {
    let candidate = Path::new(entry);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        source_dir.join(candidate)
    }
}

#[cfg(test)]
#[path = "loader_tests.rs"]
mod tests;
