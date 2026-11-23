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
use crate::merge::Merger;
use crate::{
    LoadOptions, RawConfig, apply_rule_filter, reject_allow_out_of_root_in, reject_baseline_in,
    reject_command_rules_in, reject_spawning_templates_in,
};

/// Maximum depth of an `extends:` chain — a recursion-stack guard against a
/// hostile deeply-nested (acyclic) chain. Generous: real compositions are a
/// handful deep. See [`load_recursive`] (L5).
const MAX_EXTENDS_DEPTH: usize = 64;

#[path = "loader_state.rs"]
mod state;

pub(crate) use state::{LoadState, MemoKey, TrustedExtends};
#[cfg(test)]
pub(crate) use state::{MAX_EXTENDS_MERGE_WORK, MAX_EXTENDS_SOURCE_BYTES, MEMO_HASHED, REDACTIONS};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceContext<'a> {
    source_class: ConfigSourceClass,
    consumer_version: Option<u32>,
    /// How the config this one is read for is named, for an error about the
    /// edge between the two (a version 1 config extending a version 2 one).
    consumer_display: Option<&'a str>,
    /// Whether an `extends:` entry read from the environment chose this
    /// source or one it is read for: where it lies tells what the
    /// environment holds, so every local source below such an entry is
    /// named `<redacted>`, as the entry's own target is.
    environment_chosen: bool,
}

impl<'a> SourceContext<'a> {
    pub(crate) const fn new(
        source_class: ConfigSourceClass,
        consumer_version: Option<u32>,
    ) -> Self {
        Self {
            source_class,
            consumer_version,
            consumer_display: None,
            environment_chosen: false,
        }
    }

    const fn consumed_by(mut self, display: &'a str) -> Self {
        self.consumer_display = Some(display);
        self
    }

    /// This context, chosen by the environment too when `chosen` is.
    const fn chosen_by_environment(mut self, chosen: bool) -> Self {
        self.environment_chosen |= chosen;
        self
    }

    /// The source at `display` read in this context, for the version dispatch.
    fn origin<'d>(&self, display: &'d str) -> crate::v2::Origin<'d>
    where
        'a: 'd,
    {
        crate::v2::Origin::new(self.source_class, display)
            .consumer(self.consumer_version, self.consumer_display)
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
/// The top-level `version:` is read first and decides the language the rest
/// decodes as ([`crate::v2::dispatch`]); `origin` says what may be version 2.
///
/// Gated on the presence of any `{{` marker: a config with no
/// interpolation decodes straight from the text into `RawConfig`, which
/// keeps the line/column-aware serde error messages (the `Value`
/// round-trip loses span info). An interp failure is reported with the
/// `source` path; a typed/YAML error propagates bare so the existing
/// diagnostics are unchanged.
pub(crate) fn parse_config_interpolated_with_environment(
    contents: &str,
    source: &Path,
    environment: &dyn crate::EnvironmentProvider,
    origin: &crate::v2::Origin<'_>,
) -> Result<RawConfig> {
    let contents = crate::config_text(contents, &display_path(source))?;
    // Name the file in a YAML/typed error: in an `extends:` chain or a
    // `.alint.d/` drop-in, a bare "at line 2 column 1" reads as if the
    // top-level config were at fault.
    parse_config_interpolated_inner_with_environment(contents, source, environment, origin).map_err(
        |e| match e {
            Error::Yaml(e) => {
                Error::Other(format!("{}: YAML parse error: {e}", display_path(source)))
            }
            other => other,
        },
    )
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
    // A version 2 read any other way than as the literal `version: 2` (from the
    // environment, say) was decoded as version 1, and is refused as today.
    let experimental = version == crate::v2::VERSION && config.surface == crate::v2::Surface::V2;
    if !alint_core::Config::supports_version(version) && !experimental {
        return Err(Error::Other(format!(
            "{source}: {}",
            crate::v2::unsupported_version(version)
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

/// Refuse a rule or template of `config`, a source whose config `version:`
/// has been checked ([`validate_source_compatibility`]), whose `kind:`, or
/// the kind of a rule under its `require:` at any depth, needs a later
/// config version than the source declares (the `constraint` kind needs 2;
/// [`alint_core::RuleRegistry::min_config_version`]). `source` names the
/// source in the error. Every source declares its version (an `.alint.d/`
/// fragment takes its root's) and no `{{...}}` can supply a kind, so each
/// rule's kind is a literal some source's check saw; without a catalog, the
/// kind's builder refuses it.
pub(crate) fn reject_kinds_newer_than_source(
    config: &RawConfig,
    source: &str,
    catalog: Option<alint_core::KindCatalog<'_>>,
) -> Result<()> {
    let (Some(catalog), Some(version)) = (catalog, config.version) else {
        return Ok(());
    };
    for (section, mappings) in [("rules", &config.rules), ("templates", &config.templates)] {
        for (index, mapping) in mappings.iter().enumerate() {
            if let Some((kind, needs)) = newer_kind(mapping, catalog, version) {
                return Err(Error::Other(format!(
                    "{source}: `{section}[{index}]`: the `{kind}` rule kind needs config \
                     `version: {needs}` (it is experimental), and this config is version \
                     {version}; declare `version: {needs}` to use it"
                )));
            }
        }
    }
    Ok(())
}

/// The first kind in `rule` or under its `require:` that a config of
/// `version` may not use, with the version it needs.
fn newer_kind<'m>(
    rule: &'m Mapping,
    catalog: alint_core::KindCatalog<'_>,
    version: u32,
) -> Option<(&'m str, u32)> {
    if let Some(kind) = rule.get("kind").and_then(ayaml::Value::as_str)
        && let Some(needs) = catalog.min_config_version(kind)
        && needs > version
    {
        return Some((kind, needs));
    }
    rule.get("require")
        .and_then(ayaml::Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(ayaml::Value::as_mapping)
        .find_map(|nested| newer_kind(nested, catalog, version))
}

pub(crate) fn source_version_allowed(consumer: u32, source: u32) -> bool {
    source <= consumer
}

/// How errors and listings name the local config at `path` (resolved): by
/// its path relative to `root` (the top-level config's directory, resolved),
/// `../`-relative when it lies outside (`allow_out_of_root: true` lets an
/// `extends:` chain leave the root). Without a root, or on another Windows
/// drive, by its file name.
fn local_source_display(path: &Path, root: Option<&Path>) -> String {
    root.and_then(|root| relative_to(path, root))
        .filter(|relative| !relative.as_os_str().is_empty())
        .map_or_else(
            || {
                path.file_name().map_or_else(
                    || display_path(path),
                    |name| name.to_string_lossy().into_owned(),
                )
            },
            |relative| display_path(&relative),
        )
}

/// `path` relative to `root`, with a `..` for each of `root`'s components
/// past the two's common prefix; `None` when they share no root.
fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    if let Ok(inside) = path.strip_prefix(root) {
        return Some(inside.to_path_buf());
    }
    let mut path_parts = path.components().peekable();
    let mut root_parts = root.components().peekable();
    let mut shared = 0;
    while let (Some(a), Some(b)) = (path_parts.peek(), root_parts.peek()) {
        if a != b {
            break;
        }
        path_parts.next();
        root_parts.next();
        shared += 1;
    }
    // Two paths with no common prefix, or only a Windows prefix (`C:`) with
    // no root, have no relative path between them.
    let rooted = path
        .components()
        .take(shared)
        .any(|part| matches!(part, std::path::Component::RootDir));
    if !rooted {
        return None;
    }
    let mut relative: PathBuf = root_parts
        .map(|_| std::path::Component::ParentDir)
        .collect();
    relative.extend(path_parts);
    Some(relative)
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
    let value: ayaml::Value = decode_text(contents).ok()?;
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

/// Decode `contents`, a whole YAML document, as `T`. Every parse of a whole
/// config the loader makes goes through here (a typed decode, or a parse to a
/// value for the version dispatch), so the tests can count them: a version 1
/// source parses once.
pub(crate) fn decode_text<'de, T: serde::Deserialize<'de>>(
    contents: &'de str,
) -> std::result::Result<T, alint_core::yaml::Error> {
    #[cfg(test)]
    PARSES.with(|parses| parses.set(parses.get() + 1));
    alint_core::yaml::from_str(contents)
}

#[cfg(test)]
thread_local! {
    /// How many whole documents [`decode_text`] has parsed on this thread.
    pub(crate) static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `f`'s result, and how many whole documents it parsed.
#[cfg(test)]
pub(crate) fn parses_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = PARSES.with(std::cell::Cell::get);
    let result = f();
    (result, PARSES.with(std::cell::Cell::get) - before)
}

fn deserialize_raw_config(contents: &str, source: &str) -> Result<RawConfig> {
    deserialize_document(contents, source, |config: &RawConfig| config)
}

/// Decode `contents` as `T`, whose v1 part `raw` reads: a [`RawConfig`], or a
/// version 2 [`crate::v2::Document`].
fn deserialize_document<T: serde::de::DeserializeOwned>(
    contents: &str,
    source: &str,
    raw: impl Fn(&T) -> &RawConfig,
) -> Result<T> {
    match decode_text(contents) {
        Ok(document) => {
            if raw(&document).requires.is_some() {
                reject_nonliteral_decoded_requirement(contents, source)?;
            }
            Ok(document)
        }
        Err(parse_error) => Err(nonliteral_requirement_after_error(contents, source)
            .unwrap_or(Error::Yaml(parse_error))),
    }
}

/// How the tests read a local config: as `alint_dsl::load` reads the
/// top-level one.
#[cfg(test)]
pub(crate) const TEST_ROOT: crate::v2::Origin<'static> =
    crate::v2::Origin::new(ConfigSourceClass::Root, ".alint.yml");

#[cfg(test)]
fn parse_config_interpolated_inner(contents: &str, source: &Path) -> Result<RawConfig> {
    parse_config_interpolated_inner_with_environment(
        contents,
        source,
        &crate::HostEnvironment,
        &TEST_ROOT,
    )
}

/// The v1 part of a decoded document, and the v2-only keys it took aside.
type Decoded = (RawConfig, Vec<(String, ayaml::Value)>);

/// A text decode made before the version is known: as version 1 reads the
/// text, or as version 2 does.
enum TextDecode {
    AsV1(Result<RawConfig>),
    AsV2(Result<crate::v2::Document>),
}

impl TextDecode {
    /// The decode as `surface` reads the text: kept when it reads it the same
    /// way (version 1's, or version 1's of a source with no v2-only key, which
    /// fails it, as version 2's), and redone as `surface` reads it otherwise.
    fn read_as(self, surface: crate::v2::Surface, contents: &str, source: &str) -> Result<Decoded> {
        let as_v2 = |decoded: Result<crate::v2::Document>| {
            decoded.map(|document| (document.config, document.v2_keys))
        };
        match (surface, self) {
            (crate::v2::Surface::V1, Self::AsV1(decoded)) => Ok((decoded?, Vec::new())),
            (crate::v2::Surface::V1, Self::AsV2(_)) => {
                Ok((deserialize_raw_config(contents, source)?, Vec::new()))
            }
            (crate::v2::Surface::V2, Self::AsV1(Ok(config))) => Ok((config, Vec::new())),
            (crate::v2::Surface::V2, Self::AsV1(Err(_))) => as_v2(deserialize_document(
                contents,
                source,
                |document: &crate::v2::Document| &document.config,
            )),
            (crate::v2::Surface::V2, Self::AsV2(decoded)) => as_v2(decoded),
        }
    }
}

/// Insert `marker` with each entry's `marks` into the entry, where it has any.
fn mark_entries(entries: &mut [Mapping], marker: &str, marks: Vec<Mapping>) {
    for (entry, marks) in entries.iter_mut().zip(marks) {
        if !marks.is_empty() {
            entry.insert(marker.into(), marks.into());
        }
    }
}

/// `contents` parsed for the version dispatch ([`crate::v2::read_document`]).
/// A text that opens with `version: 2` and does not parse fails with its own
/// decode's error (`text_decode`, which reads version 2's keys as version 2
/// does), which says why as version 1's decode would: a key written twice is
/// a `duplicate field`, where the parse alone says `duplicate entry with
/// key`.
fn read_document(
    contents: &str,
    origin: &crate::v2::Origin<'_>,
    text_decode: &mut Option<TextDecode>,
) -> Result<Option<ayaml::Value>> {
    match crate::v2::read_document(contents, origin) {
        Err(Error::Yaml(parse_error)) => match text_decode.take() {
            Some(TextDecode::AsV2(Err(error))) => Err(error),
            _ => Err(Error::Yaml(parse_error)),
        },
        read => read,
    }
}

/// Whether `contents` opens with `version: 2`: the cheap sign of a version 2
/// source, whose text then decodes as version 2 reads it from the start, so a
/// v2-only key does not fail a version 1 decode first. It saves a parse and
/// decides nothing: the dispatch reads the version.
fn opens_with_version_two(contents: &str) -> bool {
    contents
        .strip_prefix("version: 2")
        .is_some_and(|rest| rest.starts_with(['\n', '\r', ' ']) || rest.is_empty())
}

/// Decode an interpolated `value` as `surface` reads it.
fn decode_value(
    value: ayaml::Value,
    surface: crate::v2::Surface,
) -> std::result::Result<Decoded, alint_core::yaml::Error> {
    match surface {
        crate::v2::Surface::V1 => {
            crate::core_yaml::decode::<RawConfig>(value).map(|config| (config, Vec::new()))
        }
        crate::v2::Surface::V2 => crate::core_yaml::decode::<crate::v2::Document>(value)
            .map(|document| (document.config, document.v2_keys)),
    }
}

/// `error`, the decode error of an interpolated config, when the same document
/// with every environment value masked (`masked`, `None` when it reads none)
/// fails with the same message, so it cannot quote one; the redacted error
/// otherwise.
fn reproduce_or_redact(
    error: alint_core::yaml::Error,
    masked: Option<&ayaml::Value>,
    environment: &dyn crate::EnvironmentProvider,
    surface: crate::v2::Surface,
    source: &str,
) -> Error {
    // Each masked value keeps the type interpolation gave the real one (a
    // 12-digit id is a number), so an error in an unrelated field after it
    // still recurs; numbers move both ways, as a rule's rerun does, for a
    // value at the edge of its range.
    let reproduced = masked.is_none_or(|masked| {
        alint_core::NumberMask::BOTH.into_iter().any(|numbers| {
            let mut masked = masked.clone();
            crate::interp::interpolate_masked(
                &mut masked,
                &|name| environment.get(name),
                numbers,
                crate::interp::Scope::Document,
            )
            .is_ok()
                && decode_value(masked, surface)
                    .err()
                    .is_some_and(|masked_error| masked_error.to_string() == error.to_string())
        })
    });
    if reproduced {
        error.into()
    } else {
        redact_interpolated_error(source)
    }
}

fn redact_interpolated_error(source: &str) -> Error {
    Error::Other(format!(
        "{source}: could not deserialize interpolated config: <redacted> (the error could quote \
         a value read from the environment)"
    ))
}

/// What a version 2 source's selectors and relations read from the
/// environment, read off its raw document before interpolation erases the
/// difference ([`crate::v2::Environment`]); `None` for a version 1 source or
/// one that interpolates nothing.
fn v2_environment_of(
    surface: crate::v2::Surface,
    contents: &str,
    document: Option<&ayaml::Value>,
) -> Option<crate::v2::Environment> {
    if surface == crate::v2::Surface::V2 && contents.contains("{{") {
        document.map(crate::v2::Environment::of)
    } else {
        None
    }
}

/// The decode of `contents` made before its version is known, or the config
/// itself when that decode settles it (`Break`). A config that interpolates
/// nothing decodes straight from the text. Most such configs are version 1
/// with nothing of version 2 in them, which that one decode proves: they need
/// no second parse to read their `version:`, and decode once, as before
/// version 2. One that opens with `version: 2` decodes as version 2 reads it,
/// with the v2-only keys taken aside.
fn decode_from_text(
    contents: &str,
    source: &str,
    origin: &crate::v2::Origin<'_>,
) -> std::ops::ControlFlow<Result<RawConfig>, Option<TextDecode>> {
    use std::ops::ControlFlow::{Break, Continue};

    if contents.contains("{{") {
        return Continue(None);
    }
    if opens_with_version_two(contents) {
        return Continue(Some(TextDecode::AsV2(deserialize_document(
            contents,
            source,
            |document: &crate::v2::Document| &document.config,
        ))));
    }
    let decoded = deserialize_raw_config(contents, source);
    if let Ok(config) = &decoded
        && crate::v2::plainly_v1(config, origin)
    {
        return Break(crate::reject_ambiguous_yaml_in(config, source).and(decoded));
    }
    // A version 1 decode error stands as it is, without the parse that reads
    // the version, unless the text could be something else.
    if decoded.is_err() && !crate::v2::may_select_version_two(contents, origin) {
        return Break(decoded);
    }
    Continue(Some(TextDecode::AsV1(decoded)))
}

fn parse_config_interpolated_inner_with_environment(
    contents: &str,
    source: &Path,
    environment: &dyn crate::EnvironmentProvider,
    origin: &crate::v2::Origin<'_>,
) -> Result<RawConfig> {
    let expanded = reject_yaml_bombs(contents, source)?;
    parse_guarded(contents, source, environment, origin).map(|config| RawConfig {
        source_size: source_size(contents, expanded),
        ..config
    })
}

/// [`parse_config_interpolated_inner_with_environment`] past the YAML guards.
fn parse_guarded(
    contents: &str,
    source: &Path,
    environment: &dyn crate::EnvironmentProvider,
    origin: &crate::v2::Origin<'_>,
) -> Result<RawConfig> {
    let source_path = source;
    let source = if origin.display == alint_core::CONFIGURATION_MASK {
        origin.display.to_owned()
    } else {
        display_path(source)
    };
    let text_decode = match decode_from_text(contents, &source, origin) {
        std::ops::ControlFlow::Break(settled) => return settled,
        std::ops::ControlFlow::Continue(text_decode) => text_decode,
    };
    // The version decides the language before any typed decode (design section
    // 3 item 1). A document that does not parse dispatches as version 1, whose
    // decode reports the parse error as it always has, unless it declares
    // version 2.
    let mut text_decode = text_decode;
    let document = read_document(contents, origin, &mut text_decode)?;
    let surface = crate::v2::dispatch(document.as_ref(), contents, origin)?;
    if surface == crate::v2::Surface::V2
        && let Some(document) = &document
    {
        crate::v2::check_document(document, origin.display)?;
    }
    let v2_environment = v2_environment_of(surface, contents, document.as_ref());
    let (mut config, v2_keys, masked) = if let Some(decoded) = text_decode {
        let (config, v2_keys) = decoded.read_as(surface, contents, &source)?;
        crate::reject_ambiguous_yaml_in(&config, &source)?;
        (config, v2_keys, None)
    } else {
        let mut value: ayaml::Value = match document {
            Some(document) => document,
            None => decode_text(contents)?,
        };
        let has_sensitive_interpolation = value_reads_environment(&value);
        reject_nonliteral_requirement_in_value(contents, &source, &value)?;
        // Which vars take their value from the environment, read off the RAW
        // text before interpolation erases the difference.
        let env_vars = env_derived_vars(value.get("vars"));
        let rule_sensitive_fields = environment_sensitive_fields(value.get("rules"));
        let template_sensitive_fields = environment_sensitive_fields(value.get("templates"));
        let extends_environment = entries_reading_environment(value.get("extends"));
        let baseline_from_environment = value.get("baseline").is_some_and(value_reads_environment);
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
        let (mut config, v2_keys) = decode_value(value, surface).map_err(|error| {
            reproduce_or_redact(error, masked.as_ref(), environment, surface, &source)
        })?;
        // Reject user-authored private markers before adding the loader's own
        // env-provenance marker below.
        crate::reject_ambiguous_yaml_in(&config, &source)?;
        config.env_vars = env_vars;
        config.environment_values = environment_values;
        config.extends_environment = extends_environment;
        config.baseline_from_environment = baseline_from_environment;
        mark_entries(&mut config.rules, crate::ENV_VARS_MARKER, rule_env_vars);
        mark_entries(
            &mut config.rules,
            crate::SENSITIVE_FIELDS_MARKER,
            rule_sensitive_fields,
        );
        mark_entries(
            &mut config.templates,
            crate::SENSITIVE_FIELDS_MARKER,
            template_sensitive_fields,
        );
        (config, v2_keys, masked)
    };
    config.surface = surface;
    if surface == crate::v2::Surface::V2 {
        let masked = masked.as_ref().map(|document| Masked {
            document,
            environment,
        });
        attach_v2_objects(
            &mut config,
            &v2_keys,
            origin,
            source_path,
            v2_environment.as_ref(),
            masked.as_ref(),
        )?;
    }
    Ok(config)
}

/// Reject a config whose YAML would cost too much to read, before any parser
/// reads it; the size the alias guard estimated for what its aliases expand
/// into otherwise ([`source_size`]).
fn reject_yaml_bombs(contents: &str, source: &Path) -> Result<usize> {
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
    let (expansion, expanded) = yaml_guards.expansion_and_size(contents);
    if !expansion.within_limit_for_ayaml() {
        return Err(Error::Other(format!(
            "{}: YAML alias expansion exceeds the maximum supported size",
            source.display(),
        )));
    }
    Ok(expanded)
}

/// What the `extends:` budget counts a source of `text` as, in bytes
/// (`MAX_EXTENDS_SOURCE_BYTES`): its length, or the alias guard's estimate of
/// the tree its aliases expand into (`expanded`), scaled to input bytes as the
/// guard scales its budget, if that is larger.
fn source_size(text: &str, expanded: usize) -> usize {
    text.len()
        .max(expanded / alint_core::yaml_depth::YAML_EXPANSION_BYTES_PER_INPUT_BYTE)
}

/// Decode a version 2 source's selectors, relations and aliases onto
/// `config`, with the source that declared them. Every version 2 source is
/// recorded, one that declares nothing too: its rules' references resolve in
/// its (empty) namespace, and an alias may name it.
fn attach_v2_objects(
    config: &mut RawConfig,
    v2_keys: &[(String, ayaml::Value)],
    origin: &crate::v2::Origin<'_>,
    source_path: &Path,
    environment: Option<&crate::v2::Environment>,
    masked: Option<&Masked<'_>>,
) -> Result<()> {
    let source = crate::v2::SourceId {
        display: origin.display.to_string(),
        path: source_path.to_path_buf(),
        class: origin.class,
    };
    // An error may quote a value the environment supplied, or part of one (a
    // JSONPath's hint rewrites the path it quotes). With each value removed
    // as written, it stands only when the same source with every such value
    // in the object it is about masked fails the same way, so it quotes none
    // of them in another form. Only that object's values are masked: one
    // masked elsewhere may be no valid value there (a `format:`) and fail
    // first, where the error it is compared with never reached.
    let mut objects = crate::v2::decode_objects(source.clone(), v2_keys, &config.extends).map_err(
        |undecoded| {
            let shown = alint_core::redact_environment_values_in_error(
                undecoded.error,
                &config.environment_values,
            );
            if masked.is_none_or(|masked| {
                objects_error_recurs(&shown, masked, &source, &undecoded.locus)
            }) {
                shown
            } else {
                Error::Other(format!(
                    "{}: `{}`: <redacted> (the error could quote a value read from the \
                     environment)",
                    origin.display, undecoded.object
                ))
            }
        },
    )?;
    if let Some(environment) = environment {
        environment.mark(&mut objects);
        // A selector's or relation's error in the plan check, once every
        // source has loaded, is rerun on its masked copies.
        if let Some(masked) = masked {
            crate::v2::decode_twins(
                &mut objects,
                |locus| environment.reads(locus),
                |locus, numbers| masked.part(locus, numbers),
            );
        }
    }
    // A rule's or template's inline `input:` relation, which reads nothing
    // from the environment (`{{...}}` is refused in one), joins the source's
    // relations under a name no config can write.
    crate::v2::hoist_inline_inputs(&mut objects, &mut config.rules, &mut config.templates)
        .map_err(|error| {
            alint_core::redact_environment_values_in_error(error, &config.environment_values)
        })?;
    config.v2.sources.push(objects);
    Ok(())
}

/// A source that reads the environment, as written (before interpolation),
/// with the environment it reads: what a decode error or a plan-check error
/// is rerun on, with the values it reads masked.
struct Masked<'a> {
    document: &'a ayaml::Value,
    environment: &'a dyn crate::EnvironmentProvider,
}

impl Masked<'_> {
    /// The source interpolated, with the values under `locus` masked (numbers
    /// moved as `numbers` says) and every other value as the environment
    /// gives it; `None` when `locus` is not in the document or it does not
    /// interpolate.
    fn twin(
        &self,
        locus: &crate::v2::Locus,
        numbers: alint_core::NumberMask,
    ) -> Option<ayaml::Value> {
        let lookup = |name: &str| self.environment.get(name);
        let mut document = self.document.clone();
        let mut part = std::mem::replace(locus.find_mut(&mut document)?, ayaml::Value::Null);
        crate::interp::interpolate_value(&mut document, &lookup).ok()?;
        crate::interp::interpolate_masked(&mut part, &lookup, numbers, locus.scope()).ok()?;
        *locus.find_mut(&mut document)? = part;
        Some(document)
    }

    /// The value under `locus` alone, interpolated with each environment
    /// value masked.
    fn part(
        &self,
        locus: &crate::v2::Locus,
        numbers: alint_core::NumberMask,
    ) -> Option<ayaml::Value> {
        let mut part = locus.find(self.document)?.clone();
        let lookup = |name: &str| self.environment.get(name);
        crate::interp::interpolate_masked(&mut part, &lookup, numbers, locus.scope()).ok()?;
        Some(part)
    }
}

/// Whether `masked`, with every environment value in the object at `locus`
/// masked (numbers moved either way, as [`reproduce_or_redact`] moves them),
/// fails to decode its objects with `error`'s exact message, which therefore
/// quotes none of them: a masked string reads `<redacted>`, as one removed
/// from a message does.
fn objects_error_recurs(
    error: &Error,
    masked: &Masked<'_>,
    source: &crate::v2::SourceId,
    locus: &crate::v2::Locus,
) -> bool {
    let message = error.to_string();
    alint_core::NumberMask::BOTH.into_iter().any(|numbers| {
        masked
            .twin(locus, numbers)
            .and_then(|document| decode_value(document, crate::v2::Surface::V2).ok())
            .and_then(|(config, v2_keys)| {
                crate::v2::decode_objects(source.clone(), &v2_keys, &config.extends).err()
            })
            .is_some_and(|undecoded| undecoded.error.to_string() == message)
    })
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

/// Append each of `values` that `into` does not hold yet: a lookup each, not a
/// scan of `into`.
pub(crate) fn add_environment_values(into: &mut Vec<String>, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let held: std::collections::HashSet<&str> = into.iter().map(String::as_str).collect();
    let mut added = std::collections::HashSet::new();
    let new: Vec<String> = values
        .iter()
        .filter(|value| !held.contains(value.as_str()) && added.insert(value.as_str()))
        .cloned()
        .collect();
    into.extend(new);
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

/// Put `canonical` on the current `extends:` path (`visiting`), refusing a
/// cycle and a chain deeper than [`MAX_EXTENDS_DEPTH`]. Errors name `display`
/// (which is `<redacted>` when the environment chose the file).
fn enter_extends_chain(
    visiting: &mut std::collections::HashSet<PathBuf>,
    canonical: &Path,
    display: &str,
) -> Result<()> {
    if !visiting.insert(canonical.to_path_buf()) {
        return Err(Error::Other(format!(
            "cycle in `extends` chain at {display}"
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
            "`extends:` chain exceeds the maximum depth of {MAX_EXTENDS_DEPTH} (at {display}); \
             flatten the chain or split the ruleset",
        )));
    }
    Ok(())
}

/// How the local config at `canonical` is named: by its path from the root,
/// or, when the environment chose it or a file above it (`chosen`),
/// `<redacted>` wherever its name is shown, as its path, even one too short
/// for the text scrub, tells what the environment holds.
fn local_config_display(
    canonical: &Path,
    confine: Option<&Path>,
    state: &LoadState,
    chosen: bool,
) -> String {
    if chosen {
        alint_core::CONFIGURATION_MASK.to_owned()
    } else {
        local_source_display(canonical, confine.or(state.display_root.as_deref()))
    }
}

/// Recursively load `path`, resolving its `extends:` chain
/// left-to-right. Later entries in the chain override earlier
/// ones; the current file's own definitions override everything
/// it extends. Rules are field-merged at the YAML-Mapping layer
/// so children can override individual fields without re-stating
/// the entire rule.
#[allow(clippy::too_many_lines)]
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
    trusted: &TrustedExtends,
    context: SourceContext<'_>,
) -> Result<RawConfig> {
    let canonical = opts.config_source.canonicalize(path).map_err(|source| {
        // The error names the path as joined, which the environment chose.
        if context.environment_chosen {
            state.environment_values.add(&display_path(path));
        }
        Error::Io {
            path: if context.environment_chosen {
                PathBuf::from(alint_core::CONFIGURATION_MASK)
            } else {
                path.to_path_buf()
            },
            source,
        }
    })?;
    // Diamond chains reload a shared file once per DAG path (2^depth); a
    // non-top-level load is a pure function of its `MemoKey`, so reuse it.
    let memo_key = (!is_top).then(|| MemoKey::new(&canonical, confine, trusted, &context));
    let file = state.record_config_file(&canonical, context.environment_chosen);
    if let Some(cached) = state.cached(memo_key.as_ref()) {
        return Ok(cached);
    }
    let display = local_config_display(&canonical, confine, state, context.environment_chosen);
    let cycle_display = if context.environment_chosen {
        display.as_str()
    } else {
        // Preserve canonical display for non-environment-chosen cycles/depths.
        ""
    };
    let canonical_display;
    let chain_display = if cycle_display.is_empty() {
        canonical_display = display_path(&canonical);
        canonical_display.as_str()
    } else {
        cycle_display
    };
    enter_extends_chain(&mut state.visiting, &canonical, chain_display)?;

    let mut config = read_local_config(&canonical, file, &display, state, opts, context)?;
    refuse_out_of_root(is_top, &config.allow_out_of_root, &display, opts)?;

    // W2 (auto-fix.md 5.5): `trusted_extends:` is a top-level authority. An is_top
    // config (the user's own `.alint.yml` or a `.alint.d/` drop-in) owns the
    // allowlist and uses it for its own extends chain; a non-top-level config's
    // list is rejected per-source below, so nested loads use the list threaded from
    // the top. Taken (consumed) at is_top -- it gates the demotion here and is not
    // carried onto `Config`.
    let top_trusted: TrustedExtends;
    let trusted: &TrustedExtends = if is_top {
        top_trusted = state.trusted_extends(std::mem::take(&mut config.trusted_extends));
        &top_trusted
    } else {
        trusted
    };

    let extends = std::mem::take(&mut config.extends);
    let extends_environment = std::mem::take(&mut config.extends_environment);
    if extends.is_empty() {
        state.visiting.remove(&canonical);
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

    let mut merging = Merger::new(RawConfig {
        version: config.version,
        ..RawConfig::default()
    });
    let keys: Vec<EntryKey<'_>> = extends
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let from_environment = extends_environment.get(index).copied().unwrap_or(false);
            EntryKey::of(&entry.entry, from_environment)
        })
        .collect();
    let mut repeats = Repeats::of(&keys);
    let mut resolved_targets: std::collections::HashMap<&str, Option<PathBuf>> =
        std::collections::HashMap::new();
    for (index, entry) in extends.iter().enumerate() {
        let key = &keys[index];
        let parent = match repeats.take(key) {
            Some(parent) => parent,
            None => load_extended_source(
                &entry.entry,
                key.from_environment,
                &source_dir,
                state,
                opts,
                confine,
                trusted,
                SourceContext::new(ConfigSourceClass::LocalExtend, config.version)
                    .consumed_by(&display)
                    .chosen_by_environment(context.environment_chosen),
            )?,
        };
        if config.v2.has_sources() {
            let chosen = context.environment_chosen || key.from_environment;
            record_extended(
                &mut config.v2,
                index,
                entry,
                &source_dir,
                opts,
                chosen,
                &mut resolved_targets,
            );
        }
        // An entry that repeats the one before it merges the same source onto
        // itself, which changes nothing; still charge one entry so a long run
        // of consecutive repeats cannot bypass the merge budget.
        if index > 0 && keys[index - 1] == *key {
            state.charge_extends_entry()?;
            repeats.hold(key, parent);
            continue;
        }
        state.charge_extends(&parent)?;
        repeats.merge(key, parent, &mut merging)?;
        state.charge_extends_merged_onto(merging.take_merged_onto())?;
    }
    merging.push(config)?;
    let merged = merging.finish();
    state.visiting.remove(&canonical);
    if let Some(key) = memo_key {
        state.remember(key, &merged);
    }
    Ok(merged)
}

/// What makes two entries of one `extends:` list load the same source, gated
/// and filtered the same way: the entry, and whether it read the environment.
#[derive(PartialEq, Eq, Hash)]
struct EntryKey<'e> {
    url: &'e str,
    only: Option<&'e [String]>,
    except: Option<&'e [String]>,
    from_environment: bool,
}

impl<'e> EntryKey<'e> {
    fn of(entry: &'e alint_core::ExtendsEntry, from_environment: bool) -> Self {
        Self {
            url: entry.url(),
            only: entry.only(),
            except: entry.except(),
            from_environment,
        }
    }
}

/// The sources an `extends:` list names more than once: each is loaded, gated
/// and filtered for its first entry and reused by the later ones, and dropped
/// after its last. Two thousand entries of one ruleset of two thousand rules
/// otherwise loaded it two thousand times (152 seconds).
struct Repeats<'k, 'e> {
    /// Each key's entries after the current one.
    left: std::collections::HashMap<&'k EntryKey<'e>, usize>,
    /// The source of each key with entries left.
    held: std::collections::HashMap<&'k EntryKey<'e>, RawConfig>,
    /// The first entry of each key the list names more than once, which
    /// tells its source from the others' ([`Merger::push_repeated`]).
    repeated: std::collections::HashMap<&'k EntryKey<'e>, usize>,
}

impl<'k, 'e> Repeats<'k, 'e> {
    fn of(keys: &'k [EntryKey<'e>]) -> Self {
        let mut left = std::collections::HashMap::new();
        let mut repeated = std::collections::HashMap::new();
        for (index, key) in keys.iter().enumerate() {
            left.entry(key).and_modify(|count| *count += 1).or_insert(0);
            repeated.entry(key).or_insert(index);
        }
        repeated.retain(|key, _| left[key] > 0);
        Self {
            left,
            held: std::collections::HashMap::new(),
            repeated,
        }
    }

    /// The source of `key`'s entry, if an earlier entry loaded it.
    fn take(&mut self, key: &'k EntryKey<'e>) -> Option<RawConfig> {
        self.held.remove(key)
    }

    /// Keep `source` for `key`'s later entries, if it has any.
    fn hold(&mut self, key: &'k EntryKey<'e>, source: RawConfig) {
        if let Some(left) = self.left.get_mut(key)
            && *left > 0
        {
            *left -= 1;
            self.held.insert(key, source);
        }
    }

    /// Merge `source`, `key`'s entry, into `merger`: from where it is kept for
    /// `key`'s later entries, if it has any, so it is not copied for them.
    fn merge(
        &mut self,
        key: &'k EntryKey<'e>,
        source: RawConfig,
        merger: &mut Merger,
    ) -> Result<()> {
        let Some(&repeated) = self.repeated.get(key) else {
            return merger.push(source);
        };
        if self.left.get(key).is_some_and(|left| *left > 0) {
            merger.push_repeated(std::borrow::Cow::Borrowed(&source), repeated)?;
            self.hold(key, source);
            Ok(())
        } else {
            merger.push_repeated(std::borrow::Cow::Owned(source), repeated)
        }
    }
}

/// Record on a version 2 source's `objects` where its `extends:` `entry` (at
/// `index`) leads, for qualified references to resolve, and that the
/// environment chose its target (`chosen`), which is then named `<redacted>`.
/// Only a version 2 source has objects to record it on, so only it pays to
/// resolve a local target (which has just loaded, so it resolves); kept out
/// of line for the same reason.
#[inline(never)]
fn record_extended<'e>(
    objects: &mut crate::v2::Objects,
    index: usize,
    entry: &'e crate::v2::ExtendsEntry,
    source_dir: &Path,
    opts: &LoadOptions<'_>,
    chosen: bool,
    resolved_targets: &mut std::collections::HashMap<&'e str, Option<PathBuf>>,
) {
    let url = entry.entry.url();
    let target = resolved_targets.entry(url).or_insert_with(|| {
        (ExtendsEntryKind::of(url) == ExtendsEntryKind::Local)
            .then(|| resolve_relative(source_dir, url))
            .and_then(|target| opts.config_source.canonicalize(&target).ok())
    });
    objects.record_extended(index, target.as_deref(), chosen);
}

/// Read, parse and check the local config at `canonical`, the configuration's
/// file `file` (its index), which errors name `display`.
fn read_local_config(
    canonical: &Path,
    file: usize,
    display: &str,
    state: &mut LoadState,
    opts: &LoadOptions<'_>,
    context: SourceContext<'_>,
) -> Result<RawConfig> {
    let contents = opts
        .config_source
        .read_to_string(canonical)
        .map_err(|source| {
            let err_path = if context.environment_chosen {
                Path::new(display)
            } else {
                canonical
            };
            config_read_error(err_path, source)
        })?;
    // Checked in place, and returned as parsed: a config is large to move.
    let mut parsed = parse_config_interpolated_with_environment(
        &contents,
        canonical,
        opts.environment.as_ref(),
        &context.origin(display),
    );
    if let Ok(config) = &mut parsed {
        state.environment_values.extend(&config.environment_values);
        validate_source_compatibility(
            config,
            display,
            context.source_class,
            context.consumer_version,
            state.running_version,
        )?;
        reject_kinds_newer_than_source(config, display, opts.kind_catalog)?;
        // Each field of a version 2 source's rules and templates is written
        // by it, for a field that names a relation to resolve there.
        if config.surface == crate::v2::Surface::V2 {
            crate::v2::mark_field_origins(&mut config.rules, file);
            crate::v2::mark_field_origins(&mut config.templates, file);
        }
    }
    parsed
}

/// An untrusted load ([`crate::OutOfRootPolicy::Deny`]) refuses a top-level
/// (`is_top`) config's `allow_out_of_root:`, which would let the config read
/// and write outside the repository.
fn refuse_out_of_root(
    is_top: bool,
    allow: &alint_core::AllowOutOfRoot,
    display: &str,
    opts: &LoadOptions<'_>,
) -> Result<()> {
    if is_top && opts.out_of_root_policy == crate::OutOfRootPolicy::Deny && !allow.is_confined() {
        return Err(Error::Other(format!(
            "{display}: `allow_out_of_root:` lets a config read and write outside the \
             repository; an untrusted load (`--untrusted`, an editor workspace that is not \
             trusted, or a policy test) refuses it"
        )));
    }
    Ok(())
}

/// Refuse an `extends:` entry of a scheme the loader does not read, or a
/// remote one in a load that fetches nothing.
fn refuse_unloadable(
    kind: ExtendsEntryKind<'_>,
    source_display: &str,
    chosen: bool,
    opts: &LoadOptions<'_>,
) -> Result<()> {
    if kind == ExtendsEntryKind::PlainHttp {
        return Err(Error::Other(format!(
            "plain http:// is not allowed in `extends:` (entry {source_display:?}); \
             use https:// with an SRI hash instead"
        )));
    }
    if let ExtendsEntryKind::Unsupported(scheme) = kind {
        let shown_scheme = if chosen {
            alint_core::CONFIGURATION_MASK.to_owned()
        } else {
            scheme.to_ascii_lowercase()
        };
        return Err(Error::Other(format!(
            "unsupported `extends:` scheme `{shown_scheme}` (entry {source_display:?}); \
             use https://, alint://bundled/ or a local path",
        )));
    }
    if kind == ExtendsEntryKind::Https && opts.remote_policy == crate::RemotePolicy::Deny {
        return Err(Error::Other(format!(
            "remote `extends` is disabled for this load (entry {source_display:?}): \
             an untrusted load (`--untrusted`, an editor workspace that is not trusted, \
             or a policy test) fetches nothing"
        )));
    }
    Ok(())
}

/// Drop the top-level settings an extended `parent` declares, which only the
/// user's own config and its drop-ins set, with a warning given once per file
/// however entries spell it (`local_target`, as joined, for a local one).
fn drop_extended_settings(
    parent: &mut RawConfig,
    state: &mut LoadState,
    opts: &LoadOptions<'_>,
    local_target: Option<PathBuf>,
    source_display: &str,
    chosen: bool,
) {
    let declared = parent.drop_top_level_settings();
    if !declared.is_empty() {
        let key = local_target
            .and_then(|target| opts.config_source.canonicalize(&target).ok())
            .map_or_else(
                || source_display.to_owned(),
                |canonical| display_path(&canonical),
            );
        state.warn_ignored_settings(&key, source_display, chosen, declared);
    }
}

/// `from_environment`: the entry read the environment, so it is also removed
/// from errors in the forms the loader prints it in, which need not contain the
/// value as written (a remote URL without its credentials, query and fragment;
/// a local path canonical, or relative to the root). `context` is the
/// local-target context, with the extending config's version and name.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn load_extended_source(
    entry: &alint_core::ExtendsEntry,
    from_environment: bool,
    source_dir: &Path,
    state: &mut LoadState,
    opts: &LoadOptions<'_>,
    confine: Option<&Path>,
    trusted: &TrustedExtends,
    context: SourceContext<'_>,
) -> Result<RawConfig> {
    let consumer_version = context.consumer_version;
    let url = entry.url();
    let kind = ExtendsEntryKind::of(url);
    let context = context.chosen_by_environment(from_environment);
    let chosen = context.environment_chosen;
    let mut source_display = if kind.is_url() {
        remote_source_display(url)
    } else {
        url.to_string()
    };
    if from_environment || (chosen && kind != ExtendsEntryKind::Local) {
        state.environment_values.add(&source_display);
        if let ExtendsEntryKind::Bundled(spec) = kind {
            state
                .environment_values
                .add(&format!("alint://bundled/{spec}"));
            state.environment_values.add(spec);
        }
    }
    if chosen {
        alint_core::CONFIGURATION_MASK.clone_into(&mut source_display);
    }
    let mut local_target = None;
    refuse_unloadable(kind, &source_display, chosen, opts)?;
    let mut parent = if kind == ExtendsEntryKind::Https {
        let mut remote = load_remote(
            url,
            chosen.then_some(alint_core::CONFIGURATION_MASK),
            opts,
            state,
        )?;
        validate_source_compatibility(
            &mut remote,
            &source_display,
            ConfigSourceClass::Remote,
            consumer_version,
            state.running_version,
        )?;
        reject_kinds_newer_than_source(&remote, &source_display, opts.kind_catalog)?;
        if !remote.extends.is_empty() {
            return Err(Error::Other(format!(
                "remote config at {source_display} contains its own `extends:`; \
                 nested remote extends are not supported in this build"
            )));
        }
        crate::reject_env_expansion_in(&remote.rules, &remote.templates, &source_display)?;
        remote
    } else if let ExtendsEntryKind::Bundled(spec) = kind {
        let bundled_display = if chosen {
            alint_core::CONFIGURATION_MASK.to_owned()
        } else {
            format!("alint://bundled/{spec}")
        };
        let mut bundled = state.bundled_source(spec, || load_bundled(spec, &bundled_display))?;
        validate_source_compatibility(
            &mut bundled,
            &bundled_display,
            ConfigSourceClass::Bundled,
            consumer_version,
            state.running_version,
        )?;
        reject_kinds_newer_than_source(&bundled, &bundled_display, opts.kind_catalog)?;
        if !bundled.extends.is_empty() {
            return Err(Error::internal(format!(
                "bundled ruleset '{bundled_display}' declares its own `extends:`"
            )));
        }
        bundled
    } else {
        // A target the environment chose, or one a source it chose extends,
        // is named `<redacted>` wherever it is named: the forms the loader
        // prints its path in are recorded with the environment's values, for
        // an error from below, resolved (once per file, however it is spelled;
        // `load_recursive` records it as joined when it does not resolve).
        // Decided on the text, before any file-system call reaches the path.
        if confine.is_some() && names_a_path_of_its_own(url) {
            return Err(path_of_its_own_refusal(&source_display));
        }
        let target = resolve_relative(source_dir, url);
        if chosen && let Ok(canonical) = opts.config_source.canonicalize(&target) {
            let root = confine.or(state.display_root.as_deref());
            state
                .environment_values
                .add(&local_source_display(&canonical, root));
            state.environment_values.add(&display_path(&canonical));
        }
        confine_extends_target(
            &target,
            &source_display,
            confine,
            opts.config_source.as_ref(),
        )?;
        let loaded = load_recursive(&target, state, opts, confine, false, trusted, context)?;
        local_target = Some(target);
        loaded
    };
    gate_extended_source(state, &parent, &source_display, opts.kind_catalog)?;
    let field_source = state.field_source_names.intern(&source_display);
    mark_extended_provenance(
        &mut parent,
        &source_display,
        &field_source,
        kind == ExtendsEntryKind::Https,
    );
    drop_extended_settings(
        &mut parent,
        state,
        opts,
        local_target,
        &source_display,
        chosen,
    );
    parent.rules = apply_rule_filter(parent.rules, entry, &source_display)?;
    if kind == ExtendsEntryKind::Https {
        let base = url.split('#').next().unwrap_or(url);
        if !trusted.contains(base) && !trusted.contains(url) {
            cap_untrusted_remote(&mut parent, &source_display, opts.kind_catalog)?;
        }
    }
    Ok(parent)
}

/// Record that the `extends:` source `url` (fetched when `remote`) shaped
/// `parent`'s rules / templates, so `finalize` can judge the EFFECTIVE rule
/// after the id-based field-merge has blurred where each field came from (e.g.
/// refuse any contribution to a spawning rule, or a remote-assembled `${` env
/// reference). `field_source` names `url` in the field-source marker.
fn mark_extended_provenance(
    parent: &mut RawConfig,
    url: &str,
    field_source: &ayaml::Value,
    remote: bool,
) {
    let classes: &[crate::SourceClass] = if remote {
        &[crate::SourceClass::Extended, crate::SourceClass::Remote]
    } else {
        &[crate::SourceClass::Extended]
    };
    crate::trust::mark_extended_source_in(&mut parent.rules, url, field_source, classes);
    crate::trust::mark_extended_source_in(&mut parent.templates, url, field_source, classes);
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

/// The remote `entry`, parsed. A body is pinned by its hash, so entries that
/// name one body by other URLs (another query, say) read, hash and parse it
/// once ([`LoadState::remote_source`]).
fn load_remote(
    entry: &str,
    display_override: Option<&str>,
    opts: &LoadOptions<'_>,
    state: &mut LoadState,
) -> Result<RawConfig> {
    let source = display_override.map_or_else(|| remote_source_display(entry), str::to_owned);
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
    let source = display_override.map_or_else(|| remote_source_display(&url), str::to_owned);
    state.remote_source(&sri.encoded(), |state| {
        parse_remote(&url, &sri, &source, opts, &mut state.visiting)
    })
}

fn parse_remote(
    url: &str,
    sri: &extends::Sri,
    source: &str,
    opts: &LoadOptions<'_>,
    visiting: &mut std::collections::HashSet<PathBuf>,
) -> Result<RawConfig> {
    let cache = match opts.cache.clone() {
        Some(c) => c,
        None => extends::Cache::user_default()
            .map_err(|e| Error::Other(format!("could not open cache: {e}")))?,
    };
    let fetcher = opts.fetcher.clone().unwrap_or_default();
    let body = extends::resolve_remote(url, sri, &fetcher, &cache)
        .map_err(|error| sanitized_remote_resolution_error(source, &error))?;

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
    let (expansion, expanded) = yaml_guards.expansion_and_size(body_str);
    if !expansion.within_limit_for_ayaml() {
        return Err(Error::Other(format!(
            "remote config at {source}: YAML alias expansion exceeds the maximum supported size"
        )));
    }
    // A remote source must be version 1 (design section 3 item 6). One that
    // decodes as plainly version 1 is; any other is dispatched, whose refusal
    // of version 2 or a v2-only key comes before a decode error, which would
    // misreport a v2 key.
    let origin = crate::v2::Origin::new(ConfigSourceClass::Remote, source);
    let decoded = deserialize_raw_config(body_str, source);
    if !decoded
        .as_ref()
        .is_ok_and(|config| crate::v2::plainly_v1(config, &origin))
    {
        let document = crate::v2::read_document(body_str, &origin)?;
        crate::v2::dispatch(document.as_ref(), body_str, &origin)?;
    }
    let mut config = decoded.map_err(|error| match error {
        Error::Yaml(error) => Error::Other(format!(
            "remote config at {source}: YAML parse error: {error}"
        )),
        other => other,
    })?;
    crate::reject_ambiguous_yaml_in(&config, source)?;
    config.source_size = source_size(body_str, expanded);
    // Cycle guard token for the URL itself so a self-referencing
    // fetched config can't loop.
    let token = std::path::PathBuf::from(format!("remote://{}", sri.encoded()));
    if !visiting.insert(token.clone()) {
        return Err(Error::Other(format!("cycle on remote extends: {source}")));
    }
    visiting.remove(&token);
    Ok(config)
}

/// Every per-source refusal for one `extends:`'d config: what loading it cost
/// the load's budget ([`LoadState::charge_extends_load`]), then each trust
/// refusal. Runs before the config merges, so `url` names the offending
/// source in each error.
fn gate_extended_source(
    state: &mut LoadState,
    parent: &RawConfig,
    url: &str,
    catalog: Option<alint_core::KindCatalog<'_>>,
) -> Result<()> {
    state.charge_extends_load(parent)?;
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
fn load_bundled(spec: &str, display: &str) -> Result<RawConfig> {
    let body = crate::bundled::resolve(spec).ok_or_else(|| {
        let shipped: Vec<String> = crate::bundled::catalog()
            .map(|(n, r)| format!("alint://bundled/{n}@{r}"))
            .collect();
        Error::Other(format!(
            "unknown bundled ruleset '{display}'; \
             this build ships: [{}]",
            shipped.join(", "),
        ))
    })?;
    decode_bundled(body, spec)
}

/// Decode the body of the bundled ruleset `spec`. Anything wrong with it is an
/// alint bug, an internal error.
fn decode_bundled(body: &str, spec: &str) -> Result<RawConfig> {
    // Bundled rulesets are compiled-in and trusted (byte-identical every build), so
    // these guards are defense-in-depth, not an attack surface -- but keeping the
    // checks uniform with the remote/local paths means no parse in the loader is
    // ever unguarded. A bundled ruleset tripping one is an alint bug.
    let yaml_guards = alint_core::yaml_depth::scan_guards(body);
    let (expansion, expanded) = yaml_guards.expansion_and_size(body);
    if !yaml_guards.flow_within_limit() || !expansion.within_limit_for_ayaml() {
        return Err(Error::internal(format!(
            "built-in ruleset '{spec}' exceeds a maximum supported YAML complexity limit"
        )));
    }
    // A bundled ruleset must be version 1 (design section 3 item 6), as the
    // tests check of each; one that is not, or that uses a v2-only key, is an
    // alint bug, like one that fails to parse. One that decodes as plainly
    // version 1 needs no dispatch.
    let display = format!("alint://bundled/{spec}");
    let origin = crate::v2::Origin::new(ConfigSourceClass::Bundled, &display);
    let decoded = deserialize_raw_config(body, &display);
    if !decoded
        .as_ref()
        .is_ok_and(|config| crate::v2::plainly_v1(config, &origin))
    {
        crate::v2::read_document(body, &origin)
            .and_then(|document| crate::v2::dispatch(document.as_ref(), body, &origin))
            .map_err(|error| Error::internal(error.to_string()))?;
    }
    let config = decoded.map_err(|error| {
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
    crate::reject_ambiguous_yaml_in(&config, &display)
        .map_err(|e| Error::internal(e.to_string()))?;
    Ok(RawConfig {
        source_size: source_size(body, expanded),
        ..config
    })
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

/// Whether the local `extends:` entry `entry` names a path of its own rather
/// than one relative to the config that names it: absolute or rooted (`/x`,
/// `\x`, `C:\x`), drive-relative (`C:x`), or a Windows network or device path
/// (`\\server\share\x`, `//server/share/x`, `\\?\UNC\...`). Read as text,
/// alike on every platform, so it is decided before any file-system call: on
/// Windows, resolving a network path, even only to canonicalize it, connects to
/// that server and offers it the user's credentials.
fn names_a_path_of_its_own(entry: &str) -> bool {
    let path = Path::new(entry);
    let drive = entry
        .as_bytes()
        .get(..2)
        .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':');
    entry.starts_with(['/', '\\'])
        || drive
        || path.has_root()
        || matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(_))
        )
}

/// Why a confined load refuses the local `extends:` entry `display`
/// ([`names_a_path_of_its_own`]).
fn path_of_its_own_refusal(display: &str) -> Error {
    Error::Other(format!(
        "`extends:` target {display:?} is an absolute, rooted or network path; a local \
         `extends:` names a file relative to the config that extends it, within the linted \
         tree. Set `allow_out_of_root: true` on the top-level config to name one elsewhere."
    ))
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
