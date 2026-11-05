//! `extends:` resolution — recursive loader for the YAML
//! composition chain. Pulled out of `lib.rs` to keep that file
//! focused on the public surface (discover / load / parse) and
//! the typed config shape.

use std::path::{Path, PathBuf};

use alint_core::{
    AllowOutOfRoot, ConfigCompatibility, ConfigSourceClass, Error, Result, RunningVersion,
};
use yaml_serde::Mapping;

use crate::extends;
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
}

impl Default for LoadState {
    fn default() -> Self {
        Self {
            visiting: std::collections::HashSet::new(),
            memo: std::collections::HashMap::new(),
            running_version: RunningVersion::current(),
            environment_values: Vec::new(),
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

pub(crate) fn reject_nonliteral_requirement(contents: &str, source: &str) -> Result<()> {
    let value: yaml_serde::Value = yaml_serde::from_str(contents)?;
    reject_nonliteral_requirement_in_value(contents, source, &value)
}

fn reject_nonliteral_requirement_in_value(
    contents: &str,
    source: &str,
    value: &yaml_serde::Value,
) -> Result<()> {
    let Some(requirements) = value.get("requires") else {
        return Ok(());
    };
    let requirement = requirements.get("alint");
    let interpolated = requirement
        .and_then(yaml_serde::Value::as_str)
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

fn yaml_value_contains_tag(value: &yaml_serde::Value) -> bool {
    match value {
        yaml_serde::Value::Tagged(_) => true,
        yaml_serde::Value::Sequence(values) => values.iter().any(yaml_value_contains_tag),
        yaml_serde::Value::Mapping(values) => values
            .iter()
            .any(|(key, value)| yaml_value_contains_tag(key) || yaml_value_contains_tag(value)),
        _ => false,
    }
}

fn deserialize_raw_config(contents: &str, source: &str) -> Result<RawConfig> {
    match yaml_serde::from_str(contents) {
        Ok(config) => {
            let config: RawConfig = config;
            if config.requires.is_some() {
                reject_nonliteral_decoded_requirement(contents, source)?;
            }
            Ok(config)
        }
        Err(parse_error) => {
            if let Err(error @ Error::Other(_)) = reject_nonliteral_requirement(contents, source) {
                return Err(error);
            }
            Err(Error::Yaml(parse_error))
        }
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

    // on it super-linearly — a DoS reachable through an `extends:`'d ruleset.
    let yaml_guards = alint_core::yaml_depth::scan_guards(contents);
    if !yaml_guards.flow_within_limit() {
        return Err(Error::Other(format!(
            "{}: YAML flow nesting exceeds the maximum supported depth ({})",
            source.display(),
            alint_core::yaml_depth::MAX_YAML_FLOW_DEPTH
        )));
    }
    // Reject an alias-expansion bomb (a single anchor referenced many times) the
    // same way -- yaml_serde's own limits don't catch it.
    if !yaml_guards.expansion_within_limit(contents) {
        return Err(Error::Other(format!(
            "{}: YAML alias expansion exceeds the maximum supported size",
            source.display(),
        )));
    }
    let source = display_path(source);
    let config: RawConfig = if contents.contains("{{") {
        let mut value: yaml_serde::Value = yaml_serde::from_str(contents)?;
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
        let mut config: RawConfig = yaml_serde::from_value(value).map_err(|error| {
            let reproduced = masked.is_none_or(|mut masked| {
                let mask = |_: &str| Some(alint_core::CONFIGURATION_MASK.to_string());
                crate::interp::interpolate_value(&mut masked, &mask).is_ok()
                    && yaml_serde::from_value::<RawConfig>(masked)
                        .err()
                        .is_some_and(|masked_error| masked_error.to_string() == error.to_string())
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
fn env_derived_vars(vars: Option<&yaml_serde::Value>) -> std::collections::HashMap<String, String> {
    vars.and_then(yaml_serde::Value::as_mapping)
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let (name, raw) = (k.as_str()?, v.as_str()?);
            crate::interp::reads_env(raw).then(|| (name.to_string(), raw.to_string()))
        })
        .collect()
}
fn value_reads_environment(value: &yaml_serde::Value) -> bool {
    match value {
        yaml_serde::Value::String(text) => crate::interp::reads_env(text),
        yaml_serde::Value::Sequence(values) => values.iter().any(value_reads_environment),
        yaml_serde::Value::Mapping(mapping) => mapping.values().any(value_reads_environment),
        _ => false,
    }
}

/// Whether each entry of a raw (pre-interpolation) sequence reads the
/// environment (`extends:`, so the loader knows which entries came from it).
fn entries_reading_environment(entries: Option<&yaml_serde::Value>) -> Vec<bool> {
    entries
        .and_then(yaml_serde::Value::as_sequence)
        .map(|entries| entries.iter().map(value_reads_environment).collect())
        .unwrap_or_default()
}

/// Each raw rule's [`env_derived_vars`], as the mapping its marker holds.
fn rule_env_derived_vars(rules: Option<&yaml_serde::Value>) -> Vec<Mapping> {
    rules
        .and_then(yaml_serde::Value::as_sequence)
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
fn top_level_environment_names(document: &yaml_serde::Value) -> Vec<String> {
    fn collect(value: &yaml_serde::Value, names: &mut Vec<String>) {
        match value {
            yaml_serde::Value::String(text) => names.extend(crate::interp::env_names(text)),
            yaml_serde::Value::Sequence(values) => {
                for value in values {
                    collect(value, names);
                }
            }
            yaml_serde::Value::Mapping(mapping) => {
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
fn environment_sensitive_fields(entries: Option<&yaml_serde::Value>) -> Vec<Mapping> {
    fn reads_environment(value: &yaml_serde::Value) -> bool {
        value_reads_environment(value)
    }

    entries
        .and_then(yaml_serde::Value::as_sequence)
        .into_iter()
        .flatten()
        .map(|entry| {
            entry
                .as_mapping()
                .into_iter()
                .flat_map(|mapping| mapping.iter())
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
        .map_err(|source| Error::Io {
            path: canonical.clone(),
            source,
        })?;
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
        merged = merge(merged, parent);
    }
    merged = merge(merged, config);
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
    let source_display = if url.starts_with("http://") || url.starts_with("https://") {
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
    let mut parent = if url.starts_with("http://") {
        return Err(Error::Other(format!(
            "plain http:// is not allowed in `extends:` (entry {source_display:?}); \
             use https:// with an SRI hash instead"
        )));
    } else if url.starts_with("https://") {
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
    } else if let Some(spec) = url.strip_prefix("alint://bundled/") {
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
    mark_extended_provenance(&mut parent, &source_display);
    parent.drop_top_level_settings(&alint_core::redact_environment_values(
        &source_display,
        &state.environment_values,
    ));
    parent.rules = apply_rule_filter(parent.rules, entry, &source_display)?;
    if url.starts_with("https://") {
        let base = url.split('#').next().unwrap_or(url);
        if !trusted.iter().any(|entry| entry == base || entry == url) {
            cap_untrusted_remote(&mut parent, &source_display, opts.kind_catalog)?;
        }
    }
    Ok(parent)
}

/// Record that the `extends:` source `url` shaped `parent`'s rules / templates,
/// so `finalize` can judge the EFFECTIVE rule after the id-based field-merge has
/// blurred where each field came from (e.g. refuse any contribution to a
/// spawning rule, or a remote-assembled `${` env reference).
fn mark_extended_provenance(parent: &mut RawConfig, url: &str) {
    let classes: &[crate::SourceClass] = if url.starts_with("https://") {
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
    // A remote body is untrusted input (SRI pins WHICH bytes, not that they are
    // benign), so it needs the same flow-depth guard as a local config -- otherwise
    // `yaml_serde`/libyaml-rs chews super-linearly on a deep-flow bomb and hangs
    // the run. The local/interpolated path guards in `parse_config_interpolated`;
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
    // checks uniform with the remote/local paths means no `yaml_serde::from_str`
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
mod tests {
    use super::*;

    #[test]
    fn display_path_strips_the_windows_verbatim_prefix() {
        // `canonicalize` on Windows yields `\\?\C:\...`; error messages must
        // show the path the way the user would write it.
        assert_eq!(
            display_path(Path::new(r"\\?\C:\repo\.alint.yml")),
            r"C:\repo\.alint.yml"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\.alint.yml")),
            r"\\server\share\.alint.yml"
        );
        assert_eq!(
            display_path(Path::new("/repo/.alint.yml")),
            "/repo/.alint.yml"
        );
    }

    #[test]
    fn remote_source_display_removes_credentials_query_and_fragment() {
        assert_eq!(
            remote_source_display(
                "https://user:secret@example.test/rules.yml?token=secret#sha256-deadbeef"
            ),
            "https://example.test/rules.yml"
        );
    }

    #[test]
    fn requirement_must_be_direct_literal_metadata() {
        for body in [
            "version: 1\nrequires:\n  alint: '{{env.ALINT_REQUIRED}}'\nrules: []\n",
            "version: 1\nvars: { minimum: &v '0.19.0' }\nrequires:\n  alint: *v\nrules: []\n",
            "version: 1\nrequires:\n  alint: &v '0.19.0'\nrules: []\n",
            "version: 1\nrequires:\n  alint: !release '0.19.0'\nrules: []\n",
            "version: 1\nrequires:\n  <<: { alint: '0.19.0' }\nrules: []\n",
            "version: 1\nvars: { requirement_key: &k requires }\n? *k\n:\n  alint: '0.19.0'\nrules: []\n",
            "version: 1\nvars: { minimum: &v '0.19.0' }\n\"requi\\x72es\":\n  alint: *v\nrules: []\n",
        ] {
            let error = parse_config_interpolated_inner(body, Path::new(".alint.yml"))
                .unwrap_err()
                .to_string();
            assert!(error.contains("direct literal stable version"), "{error}");
        }
    }

    #[test]
    fn unrelated_yaml_metadata_does_not_taint_a_literal_requirement() {
        let config = parse_config_interpolated_inner(
            "version: 1\nvars: { reusable: &value literal }\nrequires:\n  alint: '0.19.0'\nrules: []\n",
            Path::new(".alint.yml"),
        )
        .unwrap();
        assert_eq!(
            config.requires.unwrap().alint,
            alint_core::StableVersion::new(0, 19, 0)
        );
    }
    #[test]
    fn every_complete_source_class_requires_an_explicit_version() {
        let running = RunningVersion::new(alint_core::StableVersion::new(0, 19, 0), false);
        for source_class in [
            ConfigSourceClass::Root,
            ConfigSourceClass::LocalExtend,
            ConfigSourceClass::Bundled,
            ConfigSourceClass::Remote,
            ConfigSourceClass::Nested,
            ConfigSourceClass::Inline,
        ] {
            let mut config = RawConfig::default();
            let error = validate_source_compatibility(
                &mut config,
                "source.yml",
                source_class,
                None,
                running,
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("missing required config `version`"),
                "{source_class:?}: {error}"
            );
        }
    }

    #[test]
    fn drop_in_inherits_omitted_version_and_records_compatibility() {
        let mut config: RawConfig = yaml_serde::from_str("rules: []").unwrap();
        validate_source_compatibility(
            &mut config,
            ".alint.d/10-team.yml",
            ConfigSourceClass::DropIn,
            Some(1),
            RunningVersion::new(alint_core::StableVersion::new(0, 19, 0), false),
        )
        .unwrap();
        assert_eq!(config.version, Some(1));
        assert_eq!(config.compatibility.len(), 1);
        assert_eq!(config.compatibility[0].consumer_version, Some(1));
    }

    #[test]
    fn drop_in_explicit_version_must_equal_root() {
        let running = RunningVersion::new(alint_core::StableVersion::new(0, 19, 0), false);
        let mut matching: RawConfig = yaml_serde::from_str("version: 1\nrules: []").unwrap();
        validate_source_compatibility(
            &mut matching,
            ".alint.d/10-team.yml",
            ConfigSourceClass::DropIn,
            Some(1),
            running,
        )
        .unwrap();
        assert_eq!(matching.compatibility[0].consumer_version, Some(1));

        let mut mismatching: RawConfig = yaml_serde::from_str("version: 2\nrules: []").unwrap();
        let error = validate_source_compatibility(
            &mut mismatching,
            ".alint.d/10-team.yml",
            ConfigSourceClass::DropIn,
            Some(1),
            running,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("drop-in config version 2 must equal root config version 1"),
            "{error}"
        );
    }

    #[test]
    fn local_source_requirement_fails_before_child_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".alint.yml");
        std::fs::write(
            &path,
            "version: 1\nrequires:\n  alint: '999.0.0'\nextends:\n  - https://invalid.invalid/rules.yml#sha256-deadbeef\nrules:\n  - id: forbidden-command\n    kind: command\n    command: [sh, -c, 'exit 0']\n    level: error\n",
        )
        .unwrap();
        let error = crate::load(&path).unwrap_err().to_string();
        assert!(error.contains("requires alint 999.0.0 or newer"), "{error}");
        assert!(!error.contains("fetch"), "{error}");
        assert!(!error.contains("forbidden-command"), "{error}");
    }

    #[test]
    fn source_version_direction_is_not_symmetric() {
        for (consumer, source, allowed) in [(1, 1, true), (1, 2, false), (2, 1, true), (2, 2, true)]
        {
            assert_eq!(
                source_version_allowed(consumer, source),
                allowed,
                "{consumer} -> {source}"
            );
        }
        assert!(
            source_version_allowed(2, 1) && !source_version_allowed(1, 2),
            "a v2 -> v1 -> v2 chain must fail at its second edge"
        );
    }

    #[test]
    fn memo_clones_a_config_only_once_it_is_seen_twice() {
        let key = || {
            MemoKey::new(
                Path::new("/repo/base.yml"),
                Some(Path::new("/repo")),
                &[],
                Some(1),
                ConfigSourceClass::LocalExtend,
            )
        };
        let config = RawConfig {
            version: Some(1),
            ..RawConfig::default()
        };
        let mut state = LoadState::default();
        // A linear chain loads each file once: nothing is cloned or served.
        state.remember(key(), &config);
        assert!(state.cached(Some(&key())).is_none());
        // A second load of the same file (a diamond) stores it; later loads hit.
        state.remember(key(), &config);
        assert_eq!(state.cached(Some(&key())).map(|c| c.version), Some(Some(1)));
    }

    #[test]
    fn memoization_separates_consumer_versions() {
        let path = Path::new("/repo/base.yml");
        let v1 = MemoKey::new(
            path,
            Some(Path::new("/repo")),
            &[],
            Some(1),
            ConfigSourceClass::LocalExtend,
        );
        let v2 = MemoKey::new(
            path,
            Some(Path::new("/repo")),
            &[],
            Some(2),
            ConfigSourceClass::LocalExtend,
        );
        assert!(v1 != v2);
    }

    #[test]
    fn confine_none_disables_the_check() {
        // A programmatic caller (or `allow_out_of_root: true`) hands `None`:
        // even a blatant escape target is permitted.
        assert!(
            confine_extends_target(
                Path::new("/etc/shadow"),
                "/etc/shadow",
                None,
                &crate::FilesystemConfigSource,
            )
            .is_ok()
        );
    }

    #[test]
    fn confine_missing_target_is_not_an_escape() {
        // A non-existent target can't be canonicalized; it is left for the
        // caller's not-found path, NOT reported as out-of-root (nothing to read).
        let tmp = tempfile::tempdir().unwrap();
        let res = confine_extends_target(
            &tmp.path().join("nope.yml"),
            "../nope.yml",
            Some(tmp.path()),
            &crate::FilesystemConfigSource,
        );
        assert!(
            res.is_ok(),
            "missing target must defer to the not-found path"
        );
    }

    #[test]
    fn confine_rejects_target_outside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let outside = tmp.path().join("outside.yml");
        std::fs::write(&outside, "x").unwrap();
        let err = confine_extends_target(
            &outside,
            "../outside.yml",
            Some(&root),
            &crate::FilesystemConfigSource,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("outside the lint root"), "{err}");
    }

    #[test]
    fn confine_allows_target_inside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let inside = tmp.path().join("base.yml");
        std::fs::write(&inside, "x").unwrap();
        assert!(
            confine_extends_target(
                &inside,
                "./base.yml",
                Some(tmp.path()),
                &crate::FilesystemConfigSource,
            )
            .is_ok()
        );
    }

    #[test]
    fn extends_chain_depth_is_capped() {
        // L5: an acyclic chain deeper than the cap is rejected (not a stack
        // overflow). c0 -> c1 -> ... all within one dir (so confinement passes).
        let tmp = tempfile::tempdir().unwrap();
        let n = MAX_EXTENDS_DEPTH + 5;
        for i in 0..n {
            let body = if i + 1 < n {
                format!("version: 1\nextends: [./c{}.yml]\nrules: []\n", i + 1)
            } else {
                "version: 1\nrules: []\n".to_string()
            };
            std::fs::write(tmp.path().join(format!("c{i}.yml")), body).unwrap();
        }
        let err = crate::load(&tmp.path().join("c0.yml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("maximum depth"), "{err}");
    }

    #[test]
    fn extends_chain_within_depth_cap_loads() {
        // A short chain (well under the cap) still composes fine.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("base.yml"), "version: 1\nrules: []\n").unwrap();
        std::fs::write(
            tmp.path().join(".alint.yml"),
            "version: 1\nextends: [./base.yml]\nrules: []\n",
        )
        .unwrap();
        assert!(crate::load(&tmp.path().join(".alint.yml")).is_ok());
    }

    #[test]
    fn diamond_extends_chain_loads_each_file_once() {
        // Audit 2026-10: `cN` extends `[./cN+1.yml, ./cN+1.yml]` at every level,
        // so without memoization the bottom file loads 2^depth times (depth 18
        // measured at 18s; depth 20 is minutes). Each level also contributes a
        // rule so the composed result is checked, not just the runtime.
        let tmp = tempfile::tempdir().unwrap();
        let depth = 20;
        for i in 0..=depth {
            let extends = if i < depth {
                format!("extends: [./c{n}.yml, ./c{n}.yml]\n", n = i + 1)
            } else {
                String::new()
            };
            let body = format!(
                "version: 1\n{extends}rules:\n  - id: r{i}\n    kind: file_exists\n    \
                 paths: README.md\n    level: warning\n"
            );
            std::fs::write(tmp.path().join(format!("c{i}.yml")), body).unwrap();
        }
        let started = std::time::Instant::now();
        let registry = alint_rules::builtin_registry();
        let cfg = crate::load_with(
            &tmp.path().join("c0.yml"),
            &crate::LoadOptions::with_catalog(registry.catalog()),
        )
        .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "diamond chain took {:?}",
            started.elapsed()
        );
        assert_eq!(cfg.rules.len(), depth + 1);
        assert_eq!(
            cfg.compatibility.len(),
            depth + 1,
            "diamond references must retain one compatibility record per canonical source/context"
        );
    }
    #[derive(Debug)]
    struct SensitiveFieldEnvironment;

    impl crate::EnvironmentProvider for SensitiveFieldEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            (name == "SECRET").then(|| "canary-secret".to_string())
        }
    }
    /// Values interpolation re-types as numbers, which an error quotes as one.
    #[derive(Debug)]
    struct NumericEnvironment;
    impl crate::EnvironmentProvider for NumericEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            match name {
                "SECRET" => Some("123456789012".to_string()),
                "HEX" => Some("0xDEADBEEF".to_string()),
                _ => None,
            }
        }
    }
    #[derive(Debug)]
    struct InvalidGlobEnvironment;
    impl crate::EnvironmentProvider for InvalidGlobEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            (name == "SECRET").then(|| "canary-secret[".to_string())
        }
    }
    /// A distinct value per name, so a test can tell which were recorded.
    #[derive(Debug)]
    struct NamedEnvironment;
    impl crate::EnvironmentProvider for NamedEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            Some(format!("value-of-{}", name.to_lowercase()))
        }
    }
    #[derive(Debug)]
    struct InvalidRegexEnvironment;
    impl crate::EnvironmentProvider for InvalidRegexEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            (name == "SECRET").then(|| "[builder-canary".to_string())
        }
    }

    #[test]
    fn environment_provenance_survives_templates_and_last_wins_overrides() {
        let raw = parse_config_interpolated_inner_with_environment(
            r#"version: 1
templates:
  - id: scan
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: "{{vars.needle}}"
    level: warning
rules:
  - id: direct
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: "{{env.SECRET}}"
    message: safe prose
    level: warning
  - id: through-template
    extends_template: scan
    vars: { needle: "{{env.SECRET}}" }
  - id: literal-override
    extends_template: scan
    vars: { needle: "{{env.SECRET}}" }
    pattern: literal
  - id: condition
    kind: file_exists
    paths: README.md
    when: "'{{env.SECRET}}' == 'canary-secret'"
    level: warning
  - id: url-only
    kind: file_exists
    paths: README.md
    policy_url: "https://example.test/{{env.SECRET}}"
    level: warning
"#,
            Path::new(".alint.yml"),
            &SensitiveFieldEnvironment,
        )
        .unwrap();
        let registry = alint_rules::builtin_registry();
        let config = raw.finalize(Some(registry.catalog())).unwrap();
        let rule = |id: &str| config.rules.iter().find(|rule| rule.id == id).unwrap();

        assert!(rule("direct").field_is_sensitive("pattern"));
        assert!(!rule("direct").field_is_sensitive("message"));
        assert!(rule("through-template").field_is_sensitive("pattern"));
        assert!(!rule("literal-override").field_is_sensitive("pattern"));
        assert!(rule("condition").field_is_sensitive("when"));
        assert!(rule("url-only").field_is_sensitive("policy_url"));
        assert!(!rule("url-only").has_sensitive_evidence());
    }
    #[test]
    fn sensitive_typed_and_builder_errors_never_echo_interpolated_values() {
        let typed = parse_config_interpolated_inner_with_environment(
            "version: 1\nrules:\n  - id: typed\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: x\n    expect_matches: '{{env.SECRET}}'\n    level: warning\n",
            Path::new(".alint.yml"),
            &SensitiveFieldEnvironment,
        )
        .unwrap();
        let registry = alint_rules::builtin_registry();
        let error = typed
            .finalize(Some(registry.catalog()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("canary-secret"), "{error}");

        let raw = parse_config_interpolated_inner_with_environment(
            "version: 1\nrules:\n  - id: builder\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: '{{env.SECRET}}'\n    level: warning\n",
            Path::new(".alint.yml"),
            &InvalidRegexEnvironment,
        )
        .unwrap();
        let config = raw.finalize(Some(registry.catalog())).unwrap();
        let error = registry.build(&config.rules[0]).unwrap_err().to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("builder-canary"), "{error}");
    }

    /// The first error loading `yaml` (read with `environment`), finalizing it,
    /// and building each rule with its nested rules, as `alint check` does.
    fn first_configuration_error(
        yaml: &str,
        environment: &dyn crate::EnvironmentProvider,
    ) -> String {
        let registry = alint_rules::builtin_registry();
        parse_config_interpolated_inner_with_environment(yaml, Path::new(".alint.yml"), environment)
            .and_then(|raw| raw.finalize(Some(registry.catalog())))
            .and_then(|config| {
                for spec in &config.rules {
                    let rule = registry.build(spec)?;
                    registry.validate_nested(spec, rule.as_ref())?;
                }
                Ok(())
            })
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn structural_errors_stay_readable_beside_a_secret() {
        // Each error names only fields, kinds or option keys, never a value, so
        // a rule that also reads the environment or sets `sensitive: true` must
        // not hide it.
        for (yaml, expected) in [
            (
                // The facebook/react shape: an unsupported `scope_filter`
                // predicate on a rule whose message reads the environment.
                "version: 1\nrules:\n  - id: fixups\n    kind: git_commit_no_fixup\n    level: error\n    message: 'fixup ({{env.SECRET}})'\n    scope_filter: {changed_since: main}\n",
                "`scope_filter.changed_since` is not supported by `git_commit_no_fixup`",
            ),
            (
                "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: README.md\n    level: error\n    sensitive: true\n    expect_matches: true\n",
                "`expect_matches` is not supported by `file_exists`",
            ),
            (
                // An unknown option, caught by the rule builder.
                "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: '{{env.SECRET}}'\n    colour: red\n    level: warning\n",
                "unknown field `colour`",
            ),
            (
                // A nested rule's unknown option, beside a secret in the parent.
                "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    message: '{{env.SECRET}}'\n    level: error\n    require:\n      - kind: file_exists\n        paths: '{path}/Cargo.toml'\n        colour: red\n",
                "unknown field `colour`",
            ),
            (
                // A top-level key error in a file that reads the environment.
                "version: 1\nrulez: []\nrules:\n  - id: r\n    kind: file_exists\n    paths: '{{env.SECRET}}'\n    level: error\n",
                "unknown field `rulez`",
            ),
            (
                // A nested rule's unknown option under a parent marked
                // sensitive: masking keeps the nested `kind`, so the rerun
                // reaches the same error.
                "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    sensitive: true\n    require:\n      - kind: file_exists\n        paths: '{path}/Cargo.toml'\n        colour: red\n",
                "unknown field `colour`",
            ),
            (
                // The same, with the secret in the nested rule itself.
                "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    require:\n      - kind: file_exists\n        paths: '{path}/{{env.SECRET}}'\n        colour: red\n",
                "unknown field `colour`",
            ),
            (
                // A nested rule's unknown kind under a parent marked sensitive.
                "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    sensitive: true\n    require:\n      - kind: file_exits\n        paths: '{path}/Cargo.toml'\n",
                "file_exits",
            ),
        ] {
            let error = first_configuration_error(yaml, &SensitiveFieldEnvironment);
            assert!(error.contains(expected), "{yaml}\n=> {error}");
            assert!(!error.contains("<redacted>"), "{error}");
            assert!(!error.contains("canary-secret"), "{error}");
        }
    }

    #[test]
    fn errors_that_quote_a_secret_stay_redacted() {
        for (yaml, environment, secret) in [
            (
                // A literal regex in a rule marked sensitive.
                "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: '[literal-canary'\n    sensitive: true\n    level: warning\n",
                &SensitiveFieldEnvironment as &dyn crate::EnvironmentProvider,
                "literal-canary",
            ),
            (
                // A nested rule's invalid regex read from the environment.
                "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    require:\n      - kind: file_content_forbidden\n        paths: '{path}/*.txt'\n        pattern: '{{env.SECRET}}'\n",
                &InvalidRegexEnvironment,
                "builder-canary",
            ),
            (
                // A typed value read from the environment, beside a key error
                // the masked copy would reach first.
                "version: '{{env.SECRET}}'\nrulez: []\n",
                &SensitiveFieldEnvironment,
                "canary-secret",
            ),
            (
                // A single `paths:` glob read from the environment.
                "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: '{{env.SECRET}}'\n    level: error\n",
                &InvalidGlobEnvironment,
                "canary-secret",
            ),
        ] {
            let error = first_configuration_error(yaml, environment);
            assert!(error.contains("<redacted>"), "{yaml}\n=> {error}");
            assert!(!error.contains(secret), "{error}");
        }
    }

    #[test]
    fn a_numeric_value_never_reaches_an_error() {
        // Interpolation re-types a numeric value, so masking only strings would
        // let the masked copy fail with the same error, quoting the number.
        for yaml in [
            // Rejected by the rule builder.
            "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*'\n    pattern: '{{env.SECRET}}'\n    level: error\n",
            // Rejected when the merged rule is deserialized.
            "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: README.md\n    message: '{{env.SECRET}}'\n    level: error\n",
            // A bool field given a number.
            "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*'\n    pattern: x\n    expect_matches: '{{env.SECRET}}'\n    level: error\n",
            // A hex value, which an error prints in decimal.
            "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: README.md\n    message: '{{env.HEX}}'\n    level: error\n",
            // A literal number in a rule marked sensitive.
            "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*'\n    pattern: 123456789012\n    sensitive: true\n    level: error\n",
        ] {
            let error = first_configuration_error(yaml, &NumericEnvironment);
            assert!(error.contains("<redacted>"), "{yaml}\n=> {error}");
            for secret in ["123456789012", "3735928559", "DEADBEEF"] {
                assert!(!error.contains(secret), "{yaml}\n=> {error}");
            }
        }
    }

    #[test]
    fn top_level_environment_values_are_recorded_and_scrubbed_from_load_errors() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".alint.d")).unwrap();
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nignore: ['{{env.IGNORED}}']\nfacts:\n  - id: f\n    any_file_exists: ['{{env.FACT}}']\nvars:\n  v: '{{env.VAR}}'\nrules:\n  - id: r\n    kind: file_exists\n    paths: '{{env.RULE}}'\n    level: error\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join(".alint.d/10-extra.yml"),
            "version: 1\nbaseline: '{{env.BASELINE}}'\n",
        )
        .unwrap();
        let registry = alint_rules::builtin_registry();
        let mut options = crate::LoadOptions::with_catalog(registry.catalog());
        options.environment = std::sync::Arc::new(NamedEnvironment);
        let config = crate::load_with(&root.path().join(".alint.yml"), &options).unwrap();
        // Rules, templates and vars are redacted per rule, so only the other
        // top-level fields are recorded, including a drop-in's.
        assert_eq!(
            config.environment_values,
            ["value-of-ignored", "value-of-fact", "value-of-baseline"]
        );
        assert_eq!(
            config.redact_environment_values("bad glob value-of-fact"),
            "bad glob <redacted>"
        );

        std::fs::remove_dir_all(root.path().join(".alint.d")).unwrap();

        // An error met while resolving `extends:` (before the configs merge) is
        // scrubbed too.
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nextends: ['./{{env.MISSING}}.yml']\nrules: []\n",
        )
        .unwrap();
        let error = crate::load_with(&root.path().join(".alint.yml"), &options)
            .unwrap_err()
            .to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("value-of-missing"), "{error}");

        // So is one met before the merge that is not about `extends:` (a
        // version check runs per source)...
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: '{{env.VALUE}}'\nrules: []\n",
        )
        .unwrap();
        let error = crate::load_with(&root.path().join(".alint.yml"), &value_options("987654"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unsupported config version <redacted>"),
            "{error}"
        );
        assert!(!error.contains("987654"), "{error}");

        // ...and one met after it: nested-config discovery walks the tree with
        // the top-level `ignore:` patterns.
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nnested_configs: true\nignore: ['{{env.VALUE}}']\nrules: []\n",
        )
        .unwrap();
        let error = crate::load_with(
            &root.path().join(".alint.yml"),
            &value_options("canary-secret["),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("canary-secret"), "{error}");

        // An `extends:` entry read from the environment is scrubbed in the form
        // the loader normalizes it to, which does not contain it as written:
        // a parse error names the extended file by its canonical path...
        std::fs::create_dir(root.path().join("sub")).unwrap();
        std::fs::write(
            root.path().join("canary-secret.yml"),
            "version: 1\nrules: [\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nextends: ['{{env.VALUE}}']\nrules: []\n",
        )
        .unwrap();
        let error = crate::load_with(
            &root.path().join(".alint.yml"),
            &value_options("sub/../canary-secret.yml"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("canary-secret"), "{error}");

        // ...while a literal entry beside a value read from the environment
        // stays readable.
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nignore: ['{{env.VALUE}}']\nextends: ['./plain-missing.yml']\nrules: []\n",
        )
        .unwrap();
        let error = crate::load_with(
            &root.path().join(".alint.yml"),
            &value_options("canary-ignored"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("plain-missing.yml"), "{error}");
        assert!(!error.contains("<redacted>"), "{error}");
    }

    #[test]
    fn an_extends_entry_read_from_the_environment_is_scrubbed_in_the_forms_it_is_printed_in() {
        let root = tempfile::tempdir().unwrap();
        for name in ["value-of-shared.yml", "value-of-ignored-rules.yml"] {
            std::fs::write(root.path().join(name), "version: 1\nrules: []\n").unwrap();
        }
        // `ignore:` reads a value the literal entry happens to contain.
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nignore: ['{{env.IGNORED}}']\n\
             extends: ['./{{env.SHARED}}.yml', './value-of-ignored-rules.yml']\nrules: []\n",
        )
        .unwrap();
        let options = crate::LoadOptions {
            environment: std::sync::Arc::new(NamedEnvironment),
            ..Default::default()
        };
        let config = crate::load_with(&root.path().join(".alint.yml"), &options).unwrap();
        let forms = |file: &str| {
            config
                .environment_values
                .iter()
                .filter(|value| value.ends_with(file))
                .count()
        };
        // The joined and canonical paths of the entry read from the
        // environment; none of the literal one.
        assert!(
            forms("value-of-shared.yml") >= 2,
            "{:?}",
            config.environment_values
        );
        assert_eq!(
            forms("value-of-ignored-rules.yml"),
            0,
            "{:?}",
            config.environment_values
        );

        // A remote entry is printed without its credentials, query and
        // fragment, so not as the value reads.
        for policy in [crate::RemotePolicy::Deny, crate::RemotePolicy::Allow] {
            std::fs::write(
                root.path().join(".alint.yml"),
                "version: 1\nextends: ['{{env.VALUE}}']\nrules: []\n",
            )
            .unwrap();
            let mut options =
                value_options("https://token@example.invalid/canary-path.yml?q=1#sha256-zz");
            options.remote_policy = policy;
            let error = crate::load_with(&root.path().join(".alint.yml"), &options)
                .unwrap_err()
                .to_string();
            assert!(error.contains("<redacted>"), "{error}");
            assert!(!error.contains("canary-path"), "{error}");
        }
    }

    /// Options whose environment gives `VALUE` the value `value`.
    fn value_options(value: &'static str) -> crate::LoadOptions<'static> {
        #[derive(Debug)]
        struct ValueEnvironment(&'static str);
        impl crate::EnvironmentProvider for ValueEnvironment {
            fn get(&self, name: &str) -> Option<String> {
                (name == "VALUE").then(|| self.0.to_string())
            }
        }
        crate::LoadOptions {
            environment: std::sync::Arc::new(ValueEnvironment(value)),
            ..Default::default()
        }
    }

    #[test]
    fn raw_config_type_errors_never_echo_interpolated_values() {
        let error = parse_config_interpolated_inner_with_environment(
            "version: '{{env.SECRET}}'\nrules: []\n",
            Path::new(".alint.yml"),
            &SensitiveFieldEnvironment,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("canary-secret"), "{error}");
    }
    #[test]
    fn merge_sensitivity_follows_the_winning_field() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("base.yml"),
            r#"version: 1
rules:
  - id: cleared
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: "{{env.SECRET}}"
    level: warning
  - id: tainted
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: literal
    level: warning
"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join(".alint.yml"),
            r#"version: 1
extends: [./base.yml]
rules:
  - id: cleared
    pattern: literal
  - id: tainted
    pattern: "{{env.SECRET}}"
"#,
        )
        .unwrap();
        let registry = alint_rules::builtin_registry();
        let mut options = crate::LoadOptions::with_catalog(registry.catalog());
        options.environment = std::sync::Arc::new(SensitiveFieldEnvironment);
        let config = crate::load_with(&root.path().join(".alint.yml"), &options).unwrap();
        let rule = |id: &str| config.rules.iter().find(|rule| rule.id == id).unwrap();
        assert!(!rule("cleared").field_is_sensitive("pattern"));
        assert!(rule("tainted").field_is_sensitive("pattern"));
    }
}
