//! How an `extends:` entry loads, by its scheme.

/// How an `extends:` entry loads, by its scheme. A URL scheme matches in any
/// case (RFC 3986), so `HTTPS://` is a remote entry: SRI-pinned, fetched, and
/// shown without its credentials and query, never read as a local path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind<'a> {
    /// `http://`, which is refused.
    PlainHttp,
    /// `https://`, fetched and checked against its `#sha256-` pin.
    Https,
    /// `alint://bundled/<name>@<rev>`, with the `<name>@<rev>` part.
    Bundled(&'a str),
    /// Anything else, a path relative to the extending config.
    Local,
}

impl<'a> EntryKind<'a> {
    const BUNDLED_PREFIX: &'static str = "alint://bundled/";

    pub(crate) fn of(entry: &'a str) -> Self {
        let has_prefix = |prefix: &str| {
            entry
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        };
        if has_prefix("http://") {
            Self::PlainHttp
        } else if has_prefix("https://") {
            Self::Https
        } else if has_prefix(Self::BUNDLED_PREFIX) {
            Self::Bundled(&entry[Self::BUNDLED_PREFIX.len()..])
        } else {
            Self::Local
        }
    }
}
